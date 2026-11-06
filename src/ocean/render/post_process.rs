//! Display path only: full-scene linear HDR, multi-scale Bloom, then SDR mapping.
use crate::ocean::{
    gpu::context::{Gpu, buffer},
    parameters::post_process::PostProcessConfig,
};
use std::cell::Cell;

pub(super) const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

struct Level {
    down: wgpu::TextureView,
    up: wgpu::TextureView,
    down_bind: wgpu::BindGroup,
    up_bind: wgpu::BindGroup,
}

pub(super) struct PostProcess {
    pub scene: wgpu::TextureView,
    levels: Vec<Level>,
    composite_bind: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    prefilter: wgpu::RenderPipeline,
    downsample: wgpu::RenderPipeline,
    upsample: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    config: Cell<PostProcessConfig>,
    debug: Cell<bool>,
}

fn target(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("HDR / Bloom target"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl PostProcess {
    pub fn new(gpu: &Gpu, format: wgpu::TextureFormat, width: u32, height: u32) -> Self {
        let device = &gpu.device;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HDR / Bloom layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HDR / Bloom pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HDR / Bloom"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(
                    include_str!("shaders/agx.wgsl"),
                    "\n",
                    include_str!("shaders/post_process.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = |entry, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let uniform = buffer(
            device,
            "HDR / Bloom settings",
            &[[0.0_f32; 4]; 2],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Bloom linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let scene = target(device, width, height);
        let composite_bind = Self::bind(device, &layout, &sampler, &uniform, &scene, &scene);
        let mut result = Self {
            scene,
            levels: Vec::new(),
            composite_bind,
            prefilter: pipeline("prefilter", HDR_FORMAT),
            downsample: pipeline("downsample", HDR_FORMAT),
            upsample: pipeline("upsample", HDR_FORMAT),
            composite: pipeline("composite", format),
            layout,
            sampler,
            uniform,
            config: Cell::new(PostProcessConfig::default()),
            debug: Cell::new(false),
        };
        result.resize(device, width, height);
        result
    }

    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        uniform: &wgpu::Buffer,
        source: &wgpu::TextureView,
        low: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HDR / Bloom inputs"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(low),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform.as_entire_binding(),
                },
            ],
        })
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.scene = target(device, width, height);
        // Separate textures avoid sampled/render-attachment subresource aliasing.
        let mut views = Vec::new();
        let (mut w, mut h) = (width.max(1), height.max(1));
        for _ in 0..6 {
            w = w.div_ceil(2);
            h = h.div_ceil(2);
            views.push((target(device, w, h), target(device, w, h)));
            if w == 1 && h == 1 {
                break;
            }
        }
        let bind = |source, low| {
            Self::bind(
                device,
                &self.layout,
                &self.sampler,
                &self.uniform,
                source,
                low,
            )
        };
        self.levels = views
            .iter()
            .enumerate()
            .map(|(i, (down, up))| {
                let source = if i == 0 { &self.scene } else { &views[i - 1].0 };
                let low = views.get(i + 1).map_or(down, |next| &next.1);
                Level {
                    down: down.clone(),
                    up: up.clone(),
                    down_bind: bind(source, source),
                    up_bind: bind(down, low),
                }
            })
            .collect();
        self.composite_bind = bind(&self.scene, &views[0].1);
    }

    pub fn configure(&self, config: PostProcessConfig, debug: bool) {
        self.config.set(config);
        self.debug.set(debug);
    }

    pub fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        camera_debug: bool,
    ) {
        let config = self.config.get();
        let enabled = config.enabled && !self.debug.get() && !camera_debug;
        let strength = if enabled && config.bloom_enabled {
            config.strength
        } else {
            0.0
        };
        let uniform = [
            [
                config.threshold,
                config.soft_knee,
                strength,
                config.exposure.exp2(),
            ],
            [
                config.radius,
                f32::from(enabled),
                f32::from(config.tonemapping_enabled),
                0.0,
            ],
        ];
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&uniform));
        if strength > 0.0 {
            for (i, level) in self.levels.iter().enumerate() {
                Self::draw(
                    encoder,
                    &level.down,
                    if i == 0 {
                        &self.prefilter
                    } else {
                        &self.downsample
                    },
                    &level.down_bind,
                );
            }
            for level in self.levels.iter().rev() {
                Self::draw(encoder, &level.up, &self.upsample, &level.up_bind);
            }
        }
        Self::draw(encoder, target, &self.composite, &self.composite_bind);
    }

    fn draw(
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        pipeline: &wgpu::RenderPipeline,
        bind: &wgpu::BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("HDR / Bloom pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

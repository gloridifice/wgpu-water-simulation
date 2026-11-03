//! Sunlight refracted through FFT/SWE onto planes or a static heightfield.
mod height_pyramid;
mod verification;
use crate::ocean::{
    gpu::context::Gpu, parameters::optics::OpticsConfig, simulation::nearshore::Shore,
};
use std::cell::Cell;
pub(in crate::ocean::render) use verification::verify;

pub(super) struct Caustics {
    pub fields: wgpu::Buffer,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    trace: wgpu::ComputePipeline,
    trace_bind: wgpu::BindGroup,
    draw: wgpu::RenderPipeline,
    draw_bind: wgpu::BindGroup,
    resolution: Cell<u32>,
    enabled: Cell<bool>,
    model: Cell<bool>,
    terrain_key: Cell<Option<(crate::ocean::parameters::model::ModelConfig, f32)>>,
    pyramid: height_pyramid::HeightPyramid,
}

impl Caustics {
    pub fn new(
        gpu: &Gpu,
        camera: &wgpu::Buffer,
        optics: &wgpu::Buffer,
        fields: &wgpu::Buffer,
        shore: &Shore,
    ) -> Self {
        let device = &gpu.device;
        let shared = concat!(
            include_str!("../shaders/scene_types.wgsl"),
            "\n",
            include_str!("../../simulation/nearshore/shaders/shore.wgsl"),
            "\n",
            include_str!("../shaders/water_light.wgsl")
        );
        let module = |label, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let pyramid = height_pyramid::HeightPyramid::new(gpu, shore);
        let shader = module(
            "caustic light tracing",
            format!(
                "{shared}\n{}\n{}\n{}\n{}",
                include_str!("../shaders/surface.wgsl"),
                include_str!("../../simulation/nearshore/shaders/shore_read.wgsl"),
                include_str!("shaders/trace.wgsl"),
                include_str!("shaders/height_trace.wgsl")
            ),
        );
        let trace = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("caustic light tracing"),
            layout: None,
            module: &shader,
            entry_point: Some("trace"),
            compilation_options: Default::default(),
            cache: None,
        });
        let hits = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("refracted light vertices"),
            size: 513 * 513 * 48,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let trace_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("caustic trace inputs"),
            layout: &trace.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: pyramid.ranges.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: fields.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: optics.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: shore.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: shore.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: shore.terrain_bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: hits.as_entire_binding(),
                },
            ],
        });
        let shader = module(
            "caustic flux raster",
            format!("{shared}\n{}", include_str!("shaders/draw.wgsl")),
        );
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("caustic flux raster"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let draw_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("caustic raster inputs"),
            layout: &draw.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: shore.terrain_bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: optics.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: shore.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: hits.as_entire_binding(),
                },
            ],
        });
        let view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("world caustic irradiance"),
                size: wgpu::Extent3d {
                    width: 512,
                    height: 512,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba16Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("caustic linear clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            fields: fields.clone(),
            view,
            sampler,
            trace,
            trace_bind,
            draw,
            draw_bind,
            resolution: Cell::new(256),
            enabled: Cell::new(false),
            model: Cell::new(false),
            terrain_key: Cell::new(None),
            pyramid,
        }
    }

    pub fn configure(&self, config: &OpticsConfig) {
        self.enabled.set(config.caustics);
        self.model.set(config.model.enabled);
        let key = Some((config.model.physics_key(), config.depth));
        if self.terrain_key.replace(key) != key {
            self.pyramid.invalidate();
        }
        self.resolution.set(config.caustics_resolution);
    }

    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timing: Option<super::RenderTiming<'_>>,
    ) {
        if !self.enabled.get() {
            if let Some(t) = timing {
                t.mark(encoder, 2);
                t.mark(encoder, 3);
            }
            return;
        }
        if self.model.get() {
            self.pyramid.encode(encoder);
        }
        if let Some(t) = timing {
            t.mark(encoder, 2);
        }
        let n = self.resolution.get();
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("caustic trace"),
                ..Default::default()
            });
            pass.set_pipeline(&self.trace);
            pass.set_bind_group(0, &self.trace_bind, &[]);
            pass.dispatch_workgroups((n + 1).div_ceil(8), (n + 1).div_ceil(8), 1);
        }
        if let Some(t) = timing {
            t.mark(encoder, 3);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("caustic accumulation"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.draw);
        pass.set_bind_group(0, &self.draw_bind, &[]);
        pass.draw(0..n * n * 6, 0..1);
    }
}

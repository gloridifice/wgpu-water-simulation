//! Scene resources and rendering; simulation ownership is unchanged.
use super::{
    optics::OpticsUniform, seabed::Seabed, skylight::SkyLut, textures::foam::FoamTextures,
};
use crate::ocean::{
    Error,
    camera::CameraUniform,
    gpu::context::{Gpu, buffer},
    parameters::optics::OpticsConfig,
    simulation::{
        nearshore::Shore,
        offshore::{Ocean, foam::Foam},
    },
};

pub struct Renderer {
    pub shore: Shore,
    sky: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    local_water: wgpu::RenderPipeline,
    local_indices: wgpu::Buffer,
    model_active: bool,
    pub(crate) model_assets: Option<std::sync::Arc<crate::ocean::render::model::ModelAssets>>,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
    depth: wgpu::TextureView,
    seabed: Seabed,
    optics: wgpu::Buffer,
    sky_lut: SkyLut,
    layout: wgpu::BindGroupLayout,
    fields: wgpu::Buffer,
    foam: wgpu::Buffer,
    foam_textures: FoamTextures,
    spray: Option<crate::ocean::simulation::spray::SprayParticles>,
    spray_depth_dirty: bool,
    format: wgpu::TextureFormat,
}

fn grid_indices(device: &wgpu::Device, n: u32) -> wgpu::Buffer {
    let mut indices = Vec::with_capacity((n * n * 6) as usize);
    for z in 0..n {
        for x in 0..n {
            let a = z * (n + 1) + x;
            let b = a + 1;
            let c = a + n + 1;
            indices.extend_from_slice(&[a, c, b, b, c, c + 1]);
        }
    }
    buffer(
        device,
        "water grid indices",
        &indices,
        wgpu::BufferUsages::INDEX,
    )
}

impl Renderer {
    pub fn new(
        gpu: &Gpu,
        ocean: &Ocean,
        foam: &Foam,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let device = &gpu.device;
        let uniform = buffer(
            device,
            "camera",
            &[<CameraUniform as bytemuck::Zeroable>::zeroed()],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let optics = buffer(
            device,
            "water optics",
            &[OpticsConfig::default().uniform()],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let sky_lut = SkyLut::new(gpu);
        let foam_textures = FoamTextures::new(gpu);
        let shore = Shore::new(gpu, ocean);
        let seabed = Seabed::new(gpu, &uniform, &optics, &shore, format, width, height);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ocean render layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 13,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 11,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ocean render bindings"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: shore.terrain_bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&foam_textures.view),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::Sampler(&foam_textures.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: shore.lace.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: shore.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: shore.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ocean.fields.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: foam.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: optics.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&seabed.color),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&seabed.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&sky_lut.view),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ocean render pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ocean and sky"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}\n{}\n{}\n{}",
                    include_str!("shaders/optics.wgsl"),
                    include_str!("shaders/sky.wgsl"),
                    include_str!("shaders/skylight.wgsl"),
                    include_str!("shaders/render.wgsl"),
                    include_str!("../simulation/nearshore/shaders/shore.wgsl"),
                    include_str!("../simulation/nearshore/shaders/shore_read.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = |vertex, fragment, write_depth| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(write_depth),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
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
        let sky = pipeline("sky_vertex", "sky_fragment", false);
        let water = pipeline("water_vertex", "water_fragment", true);
        let local_water = pipeline("local_water_vertex", "water_fragment", true);
        let n = ocean.config.mesh_resolution;
        let index_count = n * n * 6;
        let indices = grid_indices(device, n);
        let local_indices = grid_indices(device, shore.resolution);
        Self {
            shore,
            sky,
            water,
            local_water,
            local_indices,
            model_active: false,
            model_assets: None,
            bind,
            uniform,
            indices,
            index_count,
            depth: Self::depth_view(device, width, height),
            seabed,
            optics,
            sky_lut,
            layout,
            fields: ocean.fields.clone(),
            foam: foam.field.clone(),
            foam_textures,
            spray: None,
            spray_depth_dirty: false,
            format,
        }
    }

    /// Rebind every reader to the new nearshore storage; FFT resources stay live.
    pub fn set_shore_resolution(
        &mut self,
        gpu: &Gpu,
        ocean: &Ocean,
        resolution: u32,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) {
        if self.shore.resolution == resolution {
            return;
        }
        let mut model = self.seabed.model.take();
        self.spray = None;
        self.shore = Shore::with_resolution(gpu, ocean, resolution);
        self.local_indices = grid_indices(&gpu.device, resolution);
        self.shore.timer = crate::ocean::gpu::timer::GpuTimer::new(gpu);
        self.seabed = Seabed::new(
            gpu,
            &self.uniform,
            &self.optics,
            &self.shore,
            format,
            size[0],
            size[1],
        );
        if let Some(model) = &mut model {
            model.rebind(gpu, &self.uniform, &self.shore);
        }
        self.seabed.model = model;
        self.resize(&gpu.device, size[0], size[1]);
    }

    /// Fallible lazy loading keeps the original ocean independent of model assets.
    pub fn prepare_scene(
        &mut self,
        gpu: &Gpu,
        optics: &OpticsConfig,
        format: wgpu::TextureFormat,
    ) -> Result<(), Error> {
        optics.model.validate()?;
        if optics.model.enabled && self.model_assets.is_none() {
            self.model_assets = Some(std::sync::Arc::new(
                crate::ocean::render::model::ModelAssets::load()?,
            ));
        }
        if optics.model.enabled && self.seabed.model.is_none() {
            self.seabed.model = Some(crate::ocean::render::model::ModelRenderer::new(
                gpu,
                &self.uniform,
                &self.shore,
                &self
                    .model_assets
                    .as_ref()
                    .ok_or("model assets missing")?
                    .model,
                format,
            )?);
        }
        self.shore
            .prepare_terrain(gpu, optics, self.model_assets.as_ref().map(|a| &a.height))?;
        self.seabed.ground_vertices = if optics.model.enabled {
            6 + 6 * self.shore.resolution.pow(2)
        } else {
            6
        };
        self.model_active = optics.model.enabled && optics.swe_enabled();
        if let Some(model) = &mut self.seabed.model {
            model.update(gpu, optics, self.shore.resolution);
        }
        Ok(())
    }

    fn depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth = Self::depth_view(device, width, height);
        self.spray_depth_dirty = true;
        self.seabed.resize(device, width, height);
        self.bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resized ocean scene bindings"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: self.shore.terrain_bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&self.foam_textures.view),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::Sampler(&self.foam_textures.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: self.shore.lace.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: self.shore.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: self.shore.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.fields.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.foam.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.optics.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.seabed.color),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&self.seabed.depth),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.sky_lut.view),
                },
            ],
        });
    }

    /// Consume GPU-only emission after Shore::encode, including snapshot warmup steps.
    pub fn encode_spray(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        let Some(source) = &self.shore.spray else {
            self.spray = None;
            return;
        };
        if self.shore.spray_error.is_some() {
            return;
        }
        if self.spray.is_none() {
            match crate::ocean::simulation::spray::SprayParticles::new(
                gpu,
                &self.shore,
                source,
                &self.uniform,
                &self.depth,
                self.format,
            ) {
                Ok(particles) => self.spray = Some(particles),
                Err(error) => {
                    self.shore.spray_error = Some(error.to_string());
                    return;
                }
            }
        }
        let particles = self.spray.as_mut().expect("spray created above");
        if self.spray_depth_dirty {
            particles.resize(gpu, source, &self.uniform, &self.depth);
            self.spray_depth_dirty = false;
        }
        particles.encode(encoder, source, self.shore.spray_epoch);
    }

    pub fn update_optics(&self, gpu: &Gpu, optics: &OpticsConfig) {
        self.shore.configure(gpu, optics);
        self.sky_lut.update(gpu, &optics.sky);
        let value: OpticsUniform = optics.uniform();
        gpu.queue
            .write_buffer(&self.optics, 0, bytemuck::bytes_of(&value));
    }

    pub fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        camera: &CameraUniform,
    ) {
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(camera));
        self.sky_lut.encode(encoder);
        self.seabed.encode(encoder);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sky, seabed and integrated water"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.sky);
        pass.draw(0..3, 0..1);
        self.seabed.draw(&mut pass);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&self.water);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        // 3×3 周期块。边界采样取模，因此几何位置和法线都连续。
        pass.draw_indexed(0..self.index_count, 0, 0..9);
        if self.model_active {
            pass.set_pipeline(&self.local_water);
            pass.set_index_buffer(self.local_indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..6 * self.shore.resolution.pow(2), 0, 0..1);
        }
        if let Some(model) = &self.seabed.model {
            model.draw_bounds(&mut pass);
        }
        drop(pass);
        if let (Some(particles), Some(source)) = (&self.spray, &self.shore.spray) {
            particles.render(encoder, target, source);
        }
    }
}

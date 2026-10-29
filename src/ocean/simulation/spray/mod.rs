//! Nearshore spray: GPU source integrals, bounded particle storage and indirect drawing.
use crate::ocean::Error;
use crate::ocean::gpu::context::Gpu;
use crate::ocean::simulation::nearshore::Shore;
use wgpu::util::DeviceExt;

const CAPACITY: u32 = 65_536;
// Shared storage ABI; each vec4 occupies 16 bytes in WGSL.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SpraySignal {
    position_amount: [f32; 4],
    velocity_reserved: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Particle {
    position_age: [f32; 4],
    velocity_life: [f32; 4],
    appearance: [f32; 4],
}
const SIGNAL_BYTES: u64 = std::mem::size_of::<SpraySignal>() as u64;
const PARTICLE_BYTES: u64 = std::mem::size_of::<Particle>() as u64;

mod verification;
pub use verification::verify;

// Resource creation only: error scopes do not map or wait for simulation data.
fn allocate<T>(gpu: &Gpu, build: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    gpu.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = build();
    let validation = pollster::block_on(gpu.device.pop_error_scope());
    let allocation = pollster::block_on(gpu.device.pop_error_scope());
    if let Some(error) = validation.or(allocation) {
        return Err(format!("GPU spray disabled: {error}").into());
    }
    result
}

pub use crate::ocean::parameters::spray::SprayConfig;

impl SprayConfig {
    fn uniform(self, dt: f32, cells: u32, area: f32) -> SprayUniform {
        SprayUniform {
            emission: [self.rate, self.slope, self.rise, self.depth],
            motion: [self.upward, self.forward, self.lifetime, self.size],
            style: [self.drag, self.softness, dt, f32::from(self.debug)],
            counts: [CAPACITY, cells, 0x19ab_3217, 0],
            grid: [area, 0.0, self.size_range[0], self.size_range[1]],
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SprayUniform {
    emission: [f32; 4],
    motion: [f32; 4],
    style: [f32; 4],
    counts: [u32; 4],
    grid: [f32; 4],
}

fn storage(
    device: &wgpu::Device,
    name: &str,
    size: u64,
    extra: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(name),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC
            | extra,
        mapped_at_creation: false,
    })
}
fn pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    entry: &str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: None,
        module: shader,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}
fn bindings(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    buffers: &[(u32, &wgpu::Buffer)],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spray buffers"),
        layout,
        entries: &buffers
            .iter()
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: *binding,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    })
}
fn dispatch(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    bind: &wgpu::BindGroup,
    count: u32,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("spray compute"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind, &[]);
    pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
}

pub struct SpraySource {
    pub field: wgpu::Buffer,
    uniform: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: [wgpu::BindGroup; 2],
    cells: u32,
    pub config: SprayConfig,
    pub dt: f32,
    display_dt: f32,
}
impl SpraySource {
    pub fn new(gpu: &Gpu, shore: &Shore, previous: &wgpu::Buffer) -> Result<Self, Error> {
        allocate(gpu, || Self::create(gpu, shore, previous))
    }
    fn create(gpu: &Gpu, shore: &Shore, previous: &wgpu::Buffer) -> Result<Self, Error> {
        let cells = shore.resolution.pow(2);
        let limits = gpu.device.limits();
        let largest =
            (u64::from(cells) * SIGNAL_BYTES).max(16 + u64::from(CAPACITY) * PARTICLE_BYTES);
        if largest > u64::from(limits.max_storage_buffer_binding_size)
            || largest > limits.max_buffer_size
            || limits.max_storage_buffers_per_shader_stage < 7
        {
            return Err("device storage limits do not support GPU spray".into());
        }
        let device = &gpu.device;
        let field = storage(
            device,
            "spray weighted positions/counts and velocities",
            u64::from(cells) * SIGNAL_BYTES,
            wgpu::BufferUsages::empty(),
        );
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("spray parameters"),
            contents: bytemuck::bytes_of(&SprayConfig::default().uniform(0.0, cells, 1.0)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spray source"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../nearshore/shaders/shore.wgsl"),
                    include_str!("shaders/spray_common.wgsl"),
                    include_str!("shaders/spray_source.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = pipeline(device, &shader, "emit_source");
        let bind = [(&shore.field, previous), (previous, &shore.field)].map(|(current, old)| {
            bindings(
                device,
                &pipeline.get_bind_group_layout(0),
                &[
                    (0, &uniform),
                    (1, &shore.uniform),
                    (2, current),
                    (3, old),
                    (4, &shore.terrain_bed),
                    (5, &field),
                ],
            )
        });
        Ok(Self {
            field,
            uniform,
            pipeline,
            bind,
            cells,
            config: SprayConfig::default(),
            dt: 0.0,
            display_dt: 0.0,
        })
    }
    pub fn configure(&mut self, gpu: &Gpu, config: SprayConfig, dt: f32, area: f32) {
        self.config = config;
        self.dt = dt;
        if dt > 0.0 {
            self.display_dt = dt;
        }
        let mut uniform = config.uniform(dt, self.cells, area);
        uniform.grid[1] = self.display_dt;
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }
    pub fn begin_frame(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.field, 0, None);
    }
    pub fn encode_substep(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        self.encode_substep_direction(encoder, false, timestamp_writes);
    }
    pub(in crate::ocean) fn encode_substep_direction(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        reverse: bool,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("SWE spray source"),
            timestamp_writes,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind[usize::from(reverse)], &[]);
        pass.dispatch_workgroups(self.cells.div_ceil(64), 1, 1);
    }
}

pub struct SprayParticles {
    lists: [wgpu::Buffer; 2],
    remainder: wgpu::Buffer,
    indirect: wgpu::Buffer,
    update: wgpu::ComputePipeline,
    spawn: wgpu::ComputePipeline,
    finish: wgpu::ComputePipeline,
    update_bind: [wgpu::BindGroup; 2],
    spawn_bind: [wgpu::BindGroup; 2],
    finish_bind: [wgpu::BindGroup; 2],
    render_bind: [wgpu::BindGroup; 2],
    render_pipeline: wgpu::RenderPipeline,
    front: usize,
    epoch: u64,
}
impl SprayParticles {
    pub fn new(
        gpu: &Gpu,
        shore: &Shore,
        source: &SpraySource,
        camera: &wgpu::Buffer,
        depth: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        allocate(gpu, || {
            Ok(Self::create(gpu, shore, source, camera, depth, format))
        })
    }
    fn create(
        gpu: &Gpu,
        shore: &Shore,
        source: &SpraySource,
        camera: &wgpu::Buffer,
        depth: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Self {
        let device = &gpu.device;
        let lists = std::array::from_fn(|_| {
            storage(
                device,
                "spray particles",
                16 + u64::from(CAPACITY) * PARTICLE_BYTES,
                wgpu::BufferUsages::empty(),
            )
        });
        let remainder = storage(
            device,
            "spray fractional emission and birth sequence",
            u64::from(source.cells) * 8,
            wgpu::BufferUsages::empty(),
        );
        let indirect = storage(
            device,
            "spray indirect arguments",
            16,
            wgpu::BufferUsages::INDIRECT,
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spray simulation"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../nearshore/shaders/shore.wgsl"),
                    include_str!("shaders/spray_common.wgsl"),
                    include_str!("shaders/spray_compute.wgsl")
                )
                .into(),
            ),
        });
        let update = pipeline(device, &shader, "update_particles");
        let spawn = pipeline(device, &shader, "spawn_particles");
        let finish = pipeline(device, &shader, "finish_particles");
        let update_bind = std::array::from_fn(|i| {
            bindings(
                device,
                &update.get_bind_group_layout(0),
                &[
                    (0, &source.uniform),
                    (1, &shore.uniform),
                    (2, &shore.field),
                    (4, &shore.terrain_bed),
                    (6, &lists[1 - i]),
                    (7, &lists[i]),
                ],
            )
        });
        let spawn_bind = std::array::from_fn(|i| {
            bindings(
                device,
                &spawn.get_bind_group_layout(0),
                &[
                    (0, &source.uniform),
                    (1, &shore.uniform),
                    (2, &shore.field),
                    (4, &shore.terrain_bed),
                    (5, &source.field),
                    (7, &lists[i]),
                    (8, &remainder),
                ],
            )
        });
        let finish_bind = std::array::from_fn(|i| {
            bindings(
                device,
                &finish.get_bind_group_layout(0),
                &[(0, &source.uniform), (7, &lists[i]), (9, &indirect)],
            )
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("textured spray"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("shaders/spray_common.wgsl"),
                    include_str!("shaders/spray_render.wgsl")
                )
                .into(),
            ),
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("soft spray"),
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
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let render_bind =
            Self::render_bindings(gpu, &render_pipeline, &lists, source, camera, depth);
        Self {
            lists,
            remainder,
            indirect,
            update,
            spawn,
            finish,
            update_bind,
            spawn_bind,
            finish_bind,
            render_bind,
            render_pipeline,
            front: 0,
            epoch: shore.spray_epoch,
        }
    }
    fn render_bindings(
        gpu: &Gpu,
        pipeline: &wgpu::RenderPipeline,
        lists: &[wgpu::Buffer; 2],
        source: &SpraySource,
        camera: &wgpu::Buffer,
        depth: &wgpu::TextureView,
    ) -> [wgpu::BindGroup; 2] {
        let device = &gpu.device;
        // Four independently padded alpha layers from Pierre's CC0 Splash atlas.
        // Mip-major/layer-major bytes; provenance and reproducible bake live with the asset.
        let data = include_bytes!("../../../../assets/spray/splash-mips.r8");
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Pierre splash alpha array"),
            size: wgpu::Extent3d {
                width: 256,
                height: 256,
                depth_or_array_layers: 4,
            },
            mip_level_count: 9,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut offset = 0;
        for level in 0..9 {
            let size = 256 >> level;
            let bytes = (size * size * 4) as usize;
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    mip_level: level,
                    ..texture.as_image_copy()
                },
                &data[offset..offset + bytes],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size),
                    rows_per_image: Some(size),
                },
                wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 4,
                },
            );
            offset += bytes;
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("spray draw bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: source.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: camera.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: lists[i].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: source.field.as_entire_binding(),
                    },
                ],
            })
        })
    }
    pub fn resize(
        &mut self,
        gpu: &Gpu,
        source: &SpraySource,
        camera: &wgpu::Buffer,
        depth: &wgpu::TextureView,
    ) {
        self.render_bind = Self::render_bindings(
            gpu,
            &self.render_pipeline,
            &self.lists,
            source,
            camera,
            depth,
        );
    }
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, source: &SpraySource, epoch: u64) {
        if self.epoch != epoch {
            for list in &self.lists {
                encoder.clear_buffer(list, 0, None);
            }
            encoder.clear_buffer(&self.remainder, 0, None);
            encoder.clear_buffer(&self.indirect, 0, None);
            self.epoch = epoch;
        }
        if source.dt <= 0.0 {
            return;
        }
        self.front = 1 - self.front;
        encoder.clear_buffer(&self.lists[self.front], 0, Some(16));
        dispatch(
            encoder,
            &self.update,
            &self.update_bind[self.front],
            CAPACITY,
        );
        dispatch(
            encoder,
            &self.spawn,
            &self.spawn_bind[self.front],
            source.cells,
        );
        dispatch(encoder, &self.finish, &self.finish_bind[self.front], 1);
    }
    pub fn render(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        source: &SpraySource,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("soft spray overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, &self.render_bind[self.front], &[]);
        if source.config.debug {
            pass.draw(0..6, 0..source.cells);
        } else {
            pass.draw_indirect(&self.indirect, 0);
        }
    }
}

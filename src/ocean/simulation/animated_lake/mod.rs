//! Procedural shore waves with optional shared FFT height; independent of SWE.
use crate::ocean::{
    Error,
    gpu::context::{Gpu, buffer},
    parameters::lake::LakeConfig,
    scene::lake::LakeScene,
    simulation::spectral::FftWaves,
};
pub mod verification;
pub mod waves;
use waves::{CAPACITY, WaveRecord, Waves};

#[derive(Default)]
pub struct LakeCommands {
    pub emit: bool,
    pub clear: bool,
    pub reset: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    grid: [f32; 4],
    options: [f32; 4],
    fft: [f32; 4],
    foam: [f32; 4],
    foam_flow: [f32; 4],
}

pub struct AnimatedLake {
    /// Cloned GPU handles, not a second spectrum/FFT allocation.
    pub(crate) spectrum: FftWaves,
    pub config: LakeConfig,
    pub waves: Waves,
    pub rocks: u32,
    pub n: u32,
    pub extent: f32,
    pub bind: wgpu::BindGroup,
    pub uniform: wgpu::Buffer,
    pub terrain: wgpu::TextureView,
    pub field: wgpu::Texture,
    /// Two render-only material displacements (A.xy, B.zw), in metres.
    foam_motion: wgpu::Texture,
    records: wgpu::Buffer,
    height_pipeline: wgpu::ComputePipeline,
    normal_pipeline: wgpu::ComputePipeline,
    foam_flow_pipeline: wgpu::ComputePipeline,
    height_bind: wgpu::BindGroup,
    normal_bind: wgpu::BindGroup,
    foam_flow_bind: wgpu::BindGroup,
}
impl AnimatedLake {
    pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lake published fields"),
            entries: &[
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        })
    }
    pub fn new(
        gpu: &Gpu,
        config: LakeConfig,
        layout: &wgpu::BindGroupLayout,
        spectrum: &FftWaves,
    ) -> Result<Self, Error> {
        let scene = if config.enabled {
            Some(LakeScene::build(&config)?)
        } else {
            None
        };
        let n = if config.enabled {
            config.terrain.resolution
        } else {
            2
        };
        let extent = scene.as_ref().map_or(1.0, |s| s.extent);
        let uniform = buffer(
            &gpu.device,
            "lake uniform",
            &[Uniform {
                grid: [n as f32, extent, 0.0, 0.0],
                options: [0.0; 4],
                foam: [0.0; 4],
                foam_flow: [0.0; 4],
                fft: [
                    spectrum.config.resolution as f32,
                    spectrum.config.length,
                    0.0,
                    0.0,
                ],
            }],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let records = buffer(
            &gpu.device,
            "lake waves",
            &vec![<WaveRecord as bytemuck::Zeroable>::zeroed(); CAPACITY],
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let texture = |label, format| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: n,
                    height: n,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let terrain_texture = texture(
            "lake terrain and propagation",
            wgpu::TextureFormat::Rgba32Float,
        );
        let empty = vec![[1.0, 0.0, 0.0, -1.0]; (n * n) as usize];
        gpu.queue.write_texture(
            terrain_texture.as_image_copy(),
            bytemuck::cast_slice(
                scene
                    .as_ref()
                    .map_or(empty.as_slice(), |s| s.cells.as_slice()),
            ),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n * 16),
                rows_per_image: Some(n),
            },
            terrain_texture.size(),
        );
        let terrain = terrain_texture.create_view(&Default::default());
        let height = texture("lake height scratch", wgpu::TextureFormat::R32Float)
            .create_view(&Default::default());
        let field = texture("lake height and slopes", wgpu::TextureFormat::Rgba32Float);
        let view = field.create_view(&Default::default());
        let foam_motion = texture(
            "lake foam material offsets",
            wgpu::TextureFormat::Rgba32Float,
        );
        let foam_view = foam_motion.create_view(&Default::default());
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("animated lake"),
                source: wgpu::ShaderSource::Wgsl(
                    concat!(
                        include_str!("../spectral/shaders/sample.wgsl"),
                        "\n",
                        include_str!("shaders/waves.wgsl"),
                        "\n",
                        include_str!("shaders/foam_flow.wgsl")
                    )
                    .into(),
                ),
            });
        let pipeline = |entry| {
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        };
        let height_pipeline = pipeline("height_main");
        let normal_pipeline = pipeline("normal_main");
        let foam_flow_pipeline = pipeline("foam_flow_main");
        let make_bind =
            |label, layout: &wgpu::BindGroupLayout, entries: &[wgpu::BindGroupEntry<'_>]| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout,
                    entries,
                })
            };
        let height_bind = make_bind(
            "lake height inputs",
            &height_pipeline.get_bind_group_layout(0),
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&terrain),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: records.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&height),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: spectrum.fields.as_entire_binding(),
                },
            ],
        );
        let normal_bind = make_bind(
            "lake slope inputs",
            &normal_pipeline.get_bind_group_layout(0),
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&height),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        );
        let foam_flow_bind = make_bind(
            "lake foam flow inputs",
            &foam_flow_pipeline.get_bind_group_layout(0),
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&terrain),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&foam_view),
                },
            ],
        );
        let bind = make_bind(
            "lake render fields",
            layout,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&terrain),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&foam_view),
                },
            ],
        );
        Ok(Self {
            spectrum: spectrum.clone(),
            config,
            waves: Waves::default(),
            rocks: scene.map_or(0, |s| s.rocks),
            n,
            extent,
            bind,
            uniform,
            terrain,
            field,
            foam_motion,
            records,
            height_pipeline,
            normal_pipeline,
            foam_flow_pipeline,
            height_bind,
            normal_bind,
            foam_flow_bind,
        })
    }
    pub fn deactivate(&mut self, gpu: &Gpu) {
        if self.config.enabled {
            self.config.enabled = false;
            gpu.queue
                .write_buffer(&self.uniform, 16, bytemuck::bytes_of(&0.0_f32));
        }
    }

    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        dt: f64,
        config: LakeConfig,
        emit: bool,
        clear: bool,
    ) {
        if clear {
            self.waves.clear(&config);
        }
        self.waves.advance(dt, &config);
        // All callers (window, snapshot and MCP) consume the same absolute lake clock.
        // FFT is encoded once, before composing the bounded final height and slopes.
        if config.fft_strength > 0.0 {
            self.spectrum.encode(gpu, encoder, self.waves.time as f32);
        }
        if emit {
            self.waves.emit(&config);
        }
        self.config = config;
        let (time, records) = self.waves.records();
        if !records.is_empty() {
            gpu.queue
                .write_buffer(&self.records, 0, bytemuck::cast_slice(&records));
        }
        let u = Uniform {
            grid: [self.n as f32, self.extent, time, records.len() as f32],
            foam: [
                config.foam_strength,
                config.foam_width,
                config.foam_size,
                0.0,
            ],
            // Absolute f64 clock: wave-record time rebasing must not reset UV flow.
            foam_flow: [
                config.foam_flow_speed,
                config.foam_flow_period,
                (self.waves.time / f64::from(config.foam_flow_period)).rem_euclid(1.0) as f32,
                config.foam_flow_distortion,
            ],
            options: [
                f32::from(config.enabled),
                f32::from(config.mode == crate::ocean::parameters::lake::PropagationMode::Sdf),
                config.view as f32,
                0.0,
            ],
            fft: [
                self.spectrum.config.resolution as f32,
                self.spectrum.config.length,
                config.fft_strength,
                f32::from(config.fft_shore_fade),
            ],
        };
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
        for (pipeline, bind) in [
            (&self.height_pipeline, &self.height_bind),
            (&self.normal_pipeline, &self.normal_bind),
            (&self.foam_flow_pipeline, &self.foam_flow_bind),
        ] {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(self.n.div_ceil(8), self.n.div_ceil(8), 1);
        }
    }
}

use super::{
    foam::Foam,
    optics::{OpticsConfig, OpticsUniform},
    seabed::Seabed,
    shore::Shore,
    skylight::SkyLut,
    spectrum::{OceanConfig, initial_spectrum},
};
use wgpu::util::DeviceExt;

pub type Error = Box<dyn std::error::Error>;

pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub name: String,
}

impl Gpu {
    pub async fn new(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, Error> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
            })
            .await?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("FFT ocean device"),
                ..Default::default()
            })
            .await?;
        Ok(Self {
            adapter,
            device,
            queue,
            name: format!("{} ({:?})", info.name, info.backend),
        })
    }

    pub fn read_buffer(&self, source: &wgpu::Buffer, size: u64) -> Result<Vec<u8>, Error> {
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(source, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        self.map_buffer(&staging)
    }

    pub fn map_buffer(&self, buffer: &wgpu::Buffer) -> Result<Vec<u8>, Error> {
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let result = slice.get_mapped_range().to_vec();
        buffer.unmap();
        Ok(result)
    }
}

fn buffer<T: bytemuck::NoUninit>(
    device: &wgpu::Device,
    label: &str,
    data: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage,
    })
}

fn bind(
    device: &wgpu::Device,
    pipeline: &wgpu::ComputePipeline,
    buffers: &[&wgpu::Buffer],
) -> wgpu::BindGroup {
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("compute bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    })
}

pub struct Ocean {
    pub config: OceanConfig,
    pub height_bound: f32,
    pub fields: wgpu::Buffer,
    evolve: wgpu::ComputePipeline,
    evolve_bind: wgpu::BindGroup,
    time_buffer: wgpu::Buffer,
    fft: wgpu::ComputePipeline,
    stages: Vec<wgpu::BindGroup>,
}

impl Ocean {
    pub fn new(gpu: &Gpu, config: OceanConfig) -> Result<Self, Error> {
        config.validate()?;
        Self::from_spectrum(gpu, config, initial_spectrum(config))
    }

    pub(crate) fn from_spectrum(
        gpu: &Gpu,
        config: OceanConfig,
        modes: Vec<[f32; 4]>,
    ) -> Result<Self, Error> {
        config.validate()?;
        assert_eq!(modes.len(), config.resolution.pow(2) as usize);
        let device = &gpu.device;
        let height_bound = 2.0 * modes.iter().map(|m| m[0].hypot(m[1])).sum::<f32>()
            / config.resolution.pow(2) as f32;
        let initial = buffer(device, "initial h0", &modes, wgpu::BufferUsages::STORAGE);
        let size = 6 * u64::from(config.resolution).pow(2) * 8;
        let make_field = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let fields = make_field("height, displacement X/Z, inlet height, momentum X/Z");
        let scratch = make_field("FFT ping pong");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("spectral evolution"),
            source: wgpu::ShaderSource::Wgsl(include_str!("evolve.wgsl").into()),
        });
        let evolve = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("evolve"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let time_buffer = buffer(
            device,
            "time parameters",
            &[
                config.resolution,
                0,
                config.length.to_bits(),
                12.0_f32.to_bits(),
                12.0_f32.to_bits(),
                1.0_f32.to_bits(),
                0,
                0,
            ],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let evolve_bind = bind(device, &evolve, &[&initial, &fields, &time_buffer]);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("radix 2 inverse FFT"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ifft.wgsl").into()),
        });
        let fft = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("IFFT butterfly"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let bits = config.resolution.ilog2();
        let mut stages = Vec::new();
        for axis in 0..2 {
            for bit in 1..=bits {
                // 每一级有独立且不变的参数，避免提交前反复写同一个 uniform。
                let params = buffer(
                    device,
                    "FFT stage",
                    &[config.resolution, 1 << bit, axis, bits],
                    wgpu::BufferUsages::UNIFORM,
                );
                let (input, output) = if stages.len() % 2 == 0 {
                    (&fields, &scratch)
                } else {
                    (&scratch, &fields)
                };
                stages.push(bind(device, &fft, &[input, output, &params]));
            }
        }
        // 总共 2*log2(N) 级，最终结果总是落回 fields。
        Ok(Self {
            config,
            height_bound,
            fields,
            evolve,
            evolve_bind,
            time_buffer,
            fft,
            stages,
        })
    }

    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, time: f32) {
        self.encode_filtered(gpu, encoder, time, &super::shore::ShoreConfig::default());
    }

    /// Filter only the three SWE input channels; far-ocean rendering is unchanged.
    pub fn encode_filtered(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        time: f32,
        shore: &super::shore::ShoreConfig,
    ) {
        let params = [
            self.config.resolution,
            time.to_bits(),
            self.config.length.to_bits(),
            shore.filter_cutoff.to_bits(),
            shore.filter_transition.to_bits(),
            f32::from(shore.filter_enabled).to_bits(),
            0,
            0,
        ];
        gpu.queue
            .write_buffer(&self.time_buffer, 0, bytemuck::cast_slice(&params));

        let count = self.config.resolution.pow(2);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("evolve spectrum"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.evolve);
            pass.set_bind_group(0, &self.evolve_bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
        }
        for stage in &self.stages {
            // 每个 pass 结束后，wgpu 插入所需的存储读写屏障。
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("IFFT stage"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.fft);
            pass.set_bind_group(0, stage, &[]);
            pass.dispatch_workgroups((6 * count).div_ceil(64), 1, 1);
        }
    }

    pub fn read_fields(&self, gpu: &Gpu) -> Result<Vec<[f32; 2]>, Error> {
        let bytes = gpu.read_buffer(&self.fields, self.fields.size())?;
        // 不依赖 Vec<u8> 的对齐保证。
        Ok(bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| bytemuck::pod_read_unaligned(bytes))
            .collect())
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_projection: [[f32; 4]; 4],
    pub inverse_view_projection: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub sun: [f32; 4],
    /// n, patch length, horizontal choppiness, vertical/horizontal amplitude gain
    pub ocean: [f32; 4],
    pub misc: [f32; 4],
}

pub struct Renderer {
    pub shore: Shore,
    sky: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
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
        let shore = Shore::new(gpu, ocean);
        let seabed = Seabed::new(device, &uniform, &optics, &shore, format, width, height);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ocean render layout"),
            entries: &[
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
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ocean and sky"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}\n{}\n{}\n{}",
                    include_str!("optics.wgsl"),
                    include_str!("sky.wgsl"),
                    include_str!("skylight.wgsl"),
                    include_str!("render.wgsl"),
                    include_str!("shore.wgsl"),
                    include_str!("shore_read.wgsl")
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
                    depth_write_enabled: write_depth,
                    depth_compare: wgpu::CompareFunction::LessEqual,
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
                multiview: None,
                cache: None,
            })
        };
        let sky = pipeline("sky_vertex", "sky_fragment", false);
        let water = pipeline("water_vertex", "water_fragment", true);
        let n = ocean.config.mesh_resolution;
        let mut indices = Vec::with_capacity((n * n * 6) as usize);
        for z in 0..n {
            for x in 0..n {
                let a = z * (n + 1) + x;
                let b = a + 1;
                let c = a + n + 1;
                indices.extend_from_slice(&[a, c, b, b, c, c + 1]);
            }
        }
        let index_count = indices.len() as u32;
        let indices = buffer(
            device,
            "water grid indices",
            &indices,
            wgpu::BufferUsages::INDEX,
        );
        Self {
            shore,
            sky,
            water,
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
        }
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.depth = Self::depth_view(device, width, height);
        self.seabed.resize(device, width, height);
        self.bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resized ocean scene bindings"),
            layout: &self.layout,
            entries: &[
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
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
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
    }
}

//! GPU-owned CFL/retry scheduler. One encode request per queue submission.
use crate::{
    Error,
    interactions::{Event, texture_data},
    parameters::FogSettings,
    terrain::Terrain,
};
use common::gpu::{Gpu, buffer};
use std::{
    num::NonZeroU64,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
#[derive(Clone, Copy, Debug, Default)]
pub struct Diagnostics {
    pub remaining: f32,
    pub dt: f32,
    pub simulated_time: f32,
    pub accepted: u32,
    pub retries: u32,
    pub dropped_time: f32,
    pub fault: u32,
    pub min_height: f32,
    pub max_height: f32,
    pub max_speed: f32,
}
impl Diagnostics {
    pub fn decode(bytes: &[u8]) -> Self {
        let u = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        let f = |i| f32::from_bits(u(i));
        Self {
            remaining: f(0),
            dt: f(1),
            simulated_time: f(2),
            accepted: u(6),
            retries: u(7),
            dropped_time: f(9),
            fault: u(10),
            min_height: f(12),
            max_height: f(13),
            max_speed: f(14),
        }
    }
}
struct Pending {
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    epoch: u64,
}
pub struct Simulation {
    pub resolution: [u32; 2],
    pub domain: [f32; 4],
    pub published: wgpu::Texture,
    /// CPU publication generation, shared with render caches; never a GPU readback.
    pub(crate) published_revision: Arc<AtomicU64>,
    pub bed: wgpu::Buffer,
    pub masks: wgpu::Buffer,
    pub control: wgpu::Buffer,
    states: wgpu::Buffer,
    parameters: wgpu::Buffer,
    events: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    selectors: wgpu::BindGroup,
    readback: wgpu::Buffer,
    pending: Option<Pending>,
    epoch: u64,
}
impl Simulation {
    pub fn new(gpu: &Gpu, terrain: &Terrain) -> Result<Self, Error> {
        terrain.validate()?;
        let [nx, nz] = terrain.metadata.resolution;
        let n = u64::from(nx * nz);
        let device = &gpu.device;
        let make = |name, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(name),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        let states = make("fog four complete state blocks", n * 64, usage);
        let recon = make(
            "fog reconstruction, limiters and accounted corrections",
            n * 80,
            usage,
        );
        let faces = make(
            "fog shared face fluxes",
            u64::from((nx + 1) * nz + nx * (nz + 1)) * 32,
            usage,
        );
        let bed_values: Vec<[f32; 4]> = terrain
            .heights
            .iter()
            .zip(&terrain.valid)
            .map(|(h, v)| [*h, f32::from(*v), 0.0, 0.0])
            .collect();
        let bed = buffer(
            device,
            "fog terrain",
            &bed_values,
            wgpu::BufferUsages::STORAGE,
        );
        let mask_values: Vec<u32> = terrain
            .pass_x
            .iter()
            .chain(&terrain.pass_z)
            .map(|v| u32::from(*v))
            .collect();
        let masks = buffer(
            device,
            "fog shared passable faces",
            &mask_values,
            wgpu::BufferUsages::STORAGE,
        );
        let control = make("fog GPU clock and reductions", 64, usage);
        let parameters = make(
            "fog parameters",
            128,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let events = make(
            "fog interaction batch",
            16 + 64 * 48,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        let published = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fog h, m, ux, uz"),
            size: wgpu::Extent3d {
                width: nx,
                height: nz,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let disturbance = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("signed local XZ disturbance velocities"),
            size: wgpu::Extent3d {
                width: 128,
                height: 128,
                depth_or_array_layers: 2,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gpu.queue.write_texture(
            disturbance.as_image_copy(),
            bytemuck::cast_slice(&texture_data()),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(128),
            },
            wgpu::Extent3d {
                width: 128,
                height: 128,
                depth_or_array_layers: 2,
            },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("disturbance linear clamp; shader rejects outside"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut entries: Vec<_> = (0..8)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: matches!(binding, 2 | 3 | 7),
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        entries.extend([
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::WriteOnly,
                    format: wgpu::TextureFormat::Rgba32Float,
                    view_dimension: wgpu::TextureViewDimension::D2,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 9,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 10,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog seven storage bindings"),
            entries: &entries,
        });
        let mut resources: Vec<_> = [
            &parameters,
            &states,
            &bed,
            &masks,
            &recon,
            &faces,
            &control,
            &events,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
        let view = published.create_view(&Default::default());
        let disturbance_view = disturbance.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        resources.extend([
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&disturbance_view),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ]);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog solver resources"),
            layout: &layout,
            entries: &resources,
        });
        let selector_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog immutable dispatch selector"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: NonZeroU64::new(16),
                },
                count: None,
            }],
        });
        let mut selector_data = vec![0u32; 17 * 4 * 64];
        for mode in 0..17 {
            for stage in 0..2 {
                for axis in 0..2 {
                    let i = (mode * 4 + stage * 2 + axis) * 64;
                    selector_data[i..i + 4].copy_from_slice(&[
                        mode as u32,
                        stage as u32,
                        axis as u32,
                        0,
                    ]);
                }
            }
        }
        let selector_buffer = buffer(
            device,
            "fog dispatch records at 256 byte offsets",
            &selector_data,
            wgpu::BufferUsages::UNIFORM,
        );
        let selectors = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog dispatch selector binding"),
            layout: &selector_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &selector_buffer,
                    offset: 0,
                    size: NonZeroU64::new(16),
                }),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog solver layout"),
            bind_group_layouts: &[Some(&layout), Some(&selector_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog hydrostatic SWE"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/swe.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fog SWE and sources"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let readback = make(
            "fog asynchronous 64 byte diagnostics",
            64,
            wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        );
        Ok(Self {
            resolution: [nx, nz],
            domain: terrain.metadata.domain,
            published,
            published_revision: Arc::new(AtomicU64::new(0)),
            bed,
            masks,
            control,
            states,
            parameters,
            events,
            pipeline,
            bind,
            selectors,
            readback,
            pending: None,
            epoch: 0,
        })
    }
    fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        mode: u32,
        stage: u32,
        axis: u32,
        single: bool,
    ) {
        // Pass boundaries supply device-wide dependencies; workgroup barriers cannot.
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fog numerical stage"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_bind_group(1, &self.selectors, &[(mode * 4 + stage * 2 + axis) * 256]);
        pass.dispatch_workgroups(
            if single {
                1
            } else {
                (self.resolution[0] + 1).div_ceil(8)
            },
            if single {
                1
            } else {
                (self.resolution[1] + 1).div_ceil(8)
            },
            1,
        );
    }
    /// Caller submits before the next encode, since parameter/event uploads are per request.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        settings: &FogSettings,
        dt: f32,
        reset: bool,
        events: &[Event],
    ) -> Result<(), Error> {
        settings.validate()?;
        if !dt.is_finite() || !(0.0..=0.1).contains(&dt) {
            return Err("fog request dt must be 0..0.1 s".into());
        }
        gpu.queue.write_buffer(
            &self.parameters,
            0,
            bytemuck::cast_slice(&settings.gpu(self.domain, dt)),
        );
        let valid: Vec<_> = events
            .iter()
            .copied()
            .filter(Event::valid)
            .take(64)
            .collect();
        let mut batch = vec![0u8; 16 + valid.len() * 48];
        batch[..4].copy_from_slice(&(valid.len() as u32).to_le_bytes());
        batch[16..].copy_from_slice(bytemuck::cast_slice(&valid));
        gpu.queue.write_buffer(&self.events, 0, &batch);
        if reset {
            self.epoch += 1;
            self.dispatch(encoder, 0, 0, 0, false);
            self.dispatch(encoder, 1, 0, 0, true);
        } else {
            self.dispatch(encoder, 2, 0, 0, true);
            self.dispatch(encoder, 16, 0, 0, false);
            for _ in 0..8 {
                self.dispatch(encoder, 3, 0, 0, true);
                self.dispatch(encoder, 4, 0, 0, false);
                self.dispatch(encoder, 5, 0, 0, true);
                self.dispatch(encoder, 6, 0, 0, false);
                for stage in 0..2 {
                    self.dispatch(encoder, 7, stage, 0, false);
                    self.dispatch(encoder, 8, stage, 0, false);
                    self.dispatch(encoder, 8, stage, 1, false);
                    self.dispatch(encoder, 9, stage, 0, false);
                    self.dispatch(encoder, 10, stage, 0, false);
                    self.dispatch(encoder, 11, stage, 0, false);
                }
                self.dispatch(encoder, 14, 0, 0, false);
                self.dispatch(encoder, 12, 0, 0, true);
            }
        }
        self.dispatch(encoder, 15, 0, 0, true);
        self.dispatch(encoder, 13, 0, 0, false);
        self.published_revision.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    pub fn request_diagnostics(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.pending.is_some() {
            return;
        }
        encoder.copy_buffer_to_buffer(&self.control, 0, &self.readback, 0, 64);
        let (tx, rx) = mpsc::channel();
        encoder.map_buffer_on_submit(&self.readback, wgpu::MapMode::Read, 0..64, move |r| {
            let _ = tx.send(r);
        });
        self.pending = Some(Pending {
            receiver: rx,
            epoch: self.epoch,
        });
    }
    pub fn poll(&mut self, gpu: &Gpu) -> Result<Option<Diagnostics>, Error> {
        gpu.device.poll(wgpu::PollType::Poll)?;
        let Some(pending) = &self.pending else {
            return Ok(None);
        };
        match pending.receiver.try_recv() {
            Ok(Ok(())) => {
                let epoch = pending.epoch;
                let bytes = self.readback.slice(..).get_mapped_range();
                let result = Diagnostics::decode(&bytes);
                drop(bytes);
                self.readback.unmap();
                self.pending = None;
                Ok((epoch == self.epoch).then_some(result))
            }
            Ok(Err(e)) => {
                self.pending = None;
                Err(e.into())
            }
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(e) => {
                self.pending = None;
                Err(e.into())
            }
        }
    }
    /// Explicit inspection/export only; never used to select real-time step sizes.
    pub fn snapshot(&self, gpu: &Gpu) -> Result<(Diagnostics, Vec<[f32; 4]>), Error> {
        let bytes = gpu.read_buffer(&self.control, 64)?;
        let diagnostics = Diagnostics::decode(&bytes);
        let role = u32::from_le_bytes(bytes[16..20].try_into()?);
        let bytes = gpu.read_buffer(&self.states, self.states.size())?;
        let n = (self.resolution[0] * self.resolution[1]) as usize;
        let values = bytes[role as usize * n * 16..(role as usize + 1) * n * 16]
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| {
                let q: [f32; 4] = bytemuck::pod_read_unaligned(b);
                [q[0], q[3], q[1] / q[0].max(0.0001), q[2] / q[0].max(0.0001)]
            })
            .collect();
        Ok((diagnostics, values))
    }
}

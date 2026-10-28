//! Isolated FFT-J source-weight experiment. Never reads settings or updates SWE.
mod verify;
mod viewer;

use super::{
    foam::{Foam, FoamConfig, FoamStep},
    gpu::{Error, Gpu, Ocean},
    spectrum::OceanConfig,
};
use wgpu::util::DeviceExt;

pub use verify::run as verify;
pub use viewer::run;

#[derive(Clone, Copy, PartialEq)]
struct Controls {
    grid: u32,
    extent: [f32; 2], // Multiples of the FFT tile, not another wave spectrum.
    chop: f32,
    threshold: f32,
    transition: f32,
    strength: f32,
    gain: f32,
    base_rate: f32,
    j_range: f32,
    weight_range: f32,
    rate_range: f32,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            grid: 256,
            extent: [1.0, 1.0],
            chop: 1.0,
            threshold: 0.01,
            transition: 0.06,
            strength: 1.0,
            gain: 2.0,
            base_rate: 1.0,
            j_range: 0.25,
            weight_range: 2.0,
            rate_range: 2.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    sizes: [u32; 4],
    domain: [f32; 4], // FFT length, output width/depth, uniform demonstration source /s
    weights: [f32; 4], // Compression threshold/transition, modulation strength, mask gain
    display: [f32; 4], // J range about 1, weight max, source max, reserved
}
impl Controls {
    fn params(self, ocean: OceanConfig) -> Params {
        Params {
            sizes: [ocean.resolution, self.grid, 0, 0],
            domain: [
                ocean.length,
                self.extent[0] * ocean.length,
                self.extent[1] * ocean.length,
                self.base_rate,
            ],
            weights: [self.threshold, self.transition, self.strength, self.gain],
            display: [self.j_range, self.weight_range, self.rate_range, 0.0],
        }
    }
}

struct Maps {
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    values: wgpu::Buffer,
    texture: wgpu::Texture,
    grid: u32,
}
impl Maps {
    fn new(gpu: &Gpu, jacobian: &wgpu::Buffer, grid: u32) -> Self {
        let uniform = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("weight lab parameters"),
                contents: bytemuck::bytes_of(&Params::zeroed()),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let values = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("weight lab J/mask/weight/source"),
            size: u64::from(grid) * u64::from(grid) * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("weight lab diagnostic maps"),
            size: wgpu::Extent3d {
                width: grid,
                height: grid,
                depth_or_array_layers: 4,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("FFT J weight experiment"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/weights.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("box-filtered foam source weights"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("reuse production Jacobian"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: jacobian.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: values.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &texture.create_view(&Default::default()),
                    ),
                },
            ],
        });
        Self {
            pipeline,
            bind,
            uniform,
            values,
            texture,
            grid,
        }
    }
    fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, params: &Params) {
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(params));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("J to foam weight maps"),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.dispatch_workgroups(self.grid.div_ceil(8), self.grid.div_ceil(8), 1);
    }
}

struct Experiment {
    ocean: Ocean,
    foam: Foam,
    maps: Maps,
}
impl Experiment {
    fn new(gpu: &Gpu, config: OceanConfig, grid: u32) -> Result<Self, Error> {
        let ocean = Ocean::new(gpu, config)?;
        let foam = Foam::new(gpu, &ocean.fields, config.resolution);
        let maps = Maps::new(gpu, &foam.field, grid);
        Ok(Self { ocean, foam, maps })
    }
    fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, time: f32, controls: Controls) {
        self.ocean.encode(gpu, encoder, time);
        // Production foam kernel always writes current J, even at dt=0. Disable
        // its separate coverage history; the experiment reads ONLY field.y.
        self.foam.encode(
            gpu,
            encoder,
            FoamStep::new(
                self.ocean.config.resolution,
                self.ocean.config.length,
                -controls.chop,
                0.0,
                FoamConfig::default(),
                true,
            ),
        );
        self.maps
            .encode(gpu, encoder, &controls.params(self.ocean.config));
    }
}

// Only diagnostics read back, asynchronously and at most four times a second.
// The shader-produced textures go directly to egui without CPU image uploads.
#[derive(Clone)]
struct Statistics {
    min: [f32; 4],
    max: [f32; 4],
    mean: [f64; 4],
    active: f64,
    folded: f64,
    time: f32,
    params: Params,
}
impl Statistics {
    fn from_bytes(bytes: &[u8], time: f32, params: Params) -> Self {
        let mut result = Self {
            min: [f32::INFINITY; 4],
            max: [f32::NEG_INFINITY; 4],
            mean: [0.0; 4],
            active: 0.0,
            folded: 0.0,
            time,
            params,
        };
        let cells = bytes.as_chunks::<16>().0;
        for bytes in cells {
            let q: [f32; 4] = bytemuck::pod_read_unaligned(bytes);
            for (a, value) in q.into_iter().enumerate() {
                result.min[a] = result.min[a].min(value);
                result.max[a] = result.max[a].max(value);
                result.mean[a] += f64::from(value);
            }
            result.active += f64::from(q[1] > 0.5);
            result.folded += f64::from(q[0] <= 0.0);
        }
        for mean in &mut result.mean {
            *mean /= cells.len() as f64;
        }
        result.active /= cells.len() as f64;
        result.folded /= cells.len() as f64;
        result
    }
}
use bytemuck::Zeroable;

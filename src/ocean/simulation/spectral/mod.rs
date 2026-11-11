//! Shared spectral waves: ocean and bounded procedural water use the same GPU fields.
pub mod spectrum;
use spectrum::initial_spectrum;

/// Optional filtering for the matched height/momentum inlet channels only.
/// The displayed height and displacement channels are never filtered.
#[derive(Clone, Copy)]
pub struct InputFilter {
    pub cutoff: f32,
    pub transition: f32,
    pub enabled: bool,
}
impl Default for InputFilter {
    fn default() -> Self {
        Self {
            cutoff: 12.0,
            transition: 12.0,
            enabled: true,
        }
    }
}
use crate::ocean::{
    Error,
    gpu::context::{Gpu, buffer},
    parameters::ocean::OceanConfig,
};

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

#[derive(Clone)]
pub struct FftWaves {
    pub config: OceanConfig,
    pub height_bound: f32,
    pub fields: wgpu::Buffer,
    evolve: wgpu::ComputePipeline,
    evolve_bind: wgpu::BindGroup,
    time_buffer: wgpu::Buffer,
    fft: wgpu::ComputePipeline,
    stages: Vec<wgpu::BindGroup>,
}

impl FftWaves {
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
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/evolve.wgsl").into()),
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
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ifft.wgsl").into()),
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
        self.encode_filtered(gpu, encoder, time, InputFilter::default());
    }

    /// Filter only the three SWE input channels; far-ocean rendering is unchanged.
    pub fn encode_filtered(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        time: f32,
        filter: impl Into<InputFilter>,
    ) {
        let filter = filter.into();
        let params = [
            self.config.resolution,
            time.to_bits(),
            self.config.length.to_bits(),
            filter.cutoff.to_bits(),
            filter.transition.to_bits(),
            f32::from(filter.enabled).to_bits(),
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

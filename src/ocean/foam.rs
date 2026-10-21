//! Jacobian whitecaps: a local source/decay model, not a fluid or spray solver.
use super::gpu::{Error, Gpu};
use wgpu::util::DeviceExt;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FoamConfig {
    pub threshold: f32,
    pub generation: f32,
    pub decay: f32,
}

impl Default for FoamConfig {
    fn default() -> Self {
        Self {
            threshold: 0.65,
            generation: 4.0,
            decay: 0.6,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FoamStep {
    pub n: u32,
    pub length: f32,
    /// Actual horizontal multiplier, including choppiness sign AND height gain.
    pub displacement_scale: f32,
    pub dt: f32,
    pub threshold: f32,
    pub generation: f32,
    pub decay: f32,
    pub reset: u32,
}

impl FoamStep {
    pub fn new(n: u32, length: f32, scale: f32, dt: f32, config: FoamConfig, reset: bool) -> Self {
        Self {
            n,
            length,
            displacement_scale: scale,
            dt,
            threshold: config.threshold,
            generation: config.generation,
            decay: config.decay,
            reset: u32::from(reset),
        }
    }
}

pub struct Foam {
    /// One vec2 per FFT sample: accumulated foam F, current Jacobian J.
    pub field: wgpu::Buffer,
    params: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
}

impl Foam {
    pub fn new(gpu: &Gpu, fields: &wgpu::Buffer, n: u32) -> Self {
        let field = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("foam coverage and Jacobian"),
                contents: bytemuck::cast_slice(&vec![[0.0_f32, 1.0]; n.pow(2) as usize]),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let params = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("foam parameters"),
            size: std::mem::size_of::<FoamStep>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Jacobian foam"),
                source: wgpu::ShaderSource::Wgsl(include_str!("foam.wgsl").into()),
            });
        let pipeline = gpu
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("foam accumulation"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("foam bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: fields.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        Self {
            field,
            params,
            pipeline,
            bind,
        }
    }

    /// Submit the encoder before writing another step's uniform parameters.
    pub fn encode(&self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder, step: FoamStep) {
        gpu.queue
            .write_buffer(&self.params, 0, bytemuck::bytes_of(&step));
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Jacobian and foam history"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.dispatch_workgroups(step.n.pow(2).div_ceil(64), 1, 1);
    }
}

/// Independent f64 reference for GPU validation, including periodic borders.
pub fn reference(fields: &[[f32; 2]], previous: &[[f32; 2]], step: FoamStep) -> Vec<[f32; 2]> {
    let n = step.n as usize;
    let count = n * n;
    let scale = f64::from(step.displacement_scale) / (2.0 * f64::from(step.length) / n as f64);
    (0..count)
        .map(|i| {
            let x = i % n;
            let z = i / n;
            let left = z * n + (x + n - 1) % n;
            let right = z * n + (x + 1) % n;
            let back = ((z + n - 1) % n) * n + x;
            let front = ((z + 1) % n) * n + x;
            let diff = |channel: usize, a: usize, b: usize| {
                (f64::from(fields[channel * count + a][0])
                    - f64::from(fields[channel * count + b][0]))
                    * scale
            };
            let j = (1.0 + diff(1, right, left)) * (1.0 + diff(2, front, back))
                - diff(1, front, back) * diff(2, right, left);
            let old = if step.reset != 0 {
                0.0
            } else {
                previous[i][0] as f64
            };
            let source =
                f64::from(step.generation) * (f64::from(step.threshold) - j).clamp(0.0, 1.0);
            let rate = source + f64::from(step.decay);
            let f = if step.dt > 0.0 && rate > 0.0 {
                let target = source / rate;
                target + (old - target) * (-rate * f64::from(step.dt)).exp()
            } else {
                old
            };
            [f.clamp(0.0, 1.0) as f32, j as f32]
        })
        .collect()
}

/// Actual compute shader versus the CPU reference on cross-coupled sinusoidal
/// displacements. Includes compression, stretching, seams, pause and reset.
pub fn verify(gpu: &Gpu) -> Result<(), Error> {
    let n = 16u32;
    let count = n.pow(2) as usize;
    let mut fields = vec![[0.0_f32; 2]; count * 3];
    for i in 0..count {
        let x = (i % n as usize) as f32 * std::f32::consts::TAU / n as f32;
        let z = (i / n as usize) as f32 * std::f32::consts::TAU / n as f32;
        fields[count + i][0] = 1.8 * x.sin() + 0.4 * z.sin();
        fields[2 * count + i][0] = 1.3 * z.sin() + 0.7 * x.sin();
    }
    let source = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("foam verification waves"),
            contents: bytemuck::cast_slice(&fields),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let foam = Foam::new(gpu, &source, n);
    let mut previous = vec![[0.0, 1.0]; count];
    for (dt, scale, reset) in [
        (0.1, -1.15, false),
        (0.2, -1.15, false),
        (0.0, -1.15, false),
        (0.1, 0.0, false),
        (0.0, -1.15, true),
    ] {
        let step = FoamStep::new(n, 8.0, scale, dt, FoamConfig::default(), reset);
        let expected = reference(&fields, &previous, step);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        foam.encode(gpu, &mut encoder, step);
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&foam.field, foam.field.size())?;
        let actual: Vec<[f32; 2]> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| bytemuck::pod_read_unaligned(bytes))
            .collect();
        let mut error = 0.0_f32;
        for (a, b) in actual.iter().zip(&expected) {
            if !a.iter().all(|v| v.is_finite()) || !(0.0..=1.0).contains(&a[0]) {
                return Err("invalid foam coverage or Jacobian".into());
            }
            error = error.max((a[0] - b[0]).abs()).max((a[1] - b[1]).abs());
        }
        if error > 2e-5 {
            return Err(format!("GPU foam error {error}").into());
        }
        println!("GPU foam: dt={dt}, scale={scale}, reset={reset}, max error={error:.3e} OK");
        previous = actual;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn step(dt: f32) -> FoamStep {
        FoamStep::new(8, 8.0, -1.15, dt, FoamConfig::default(), false)
    }
    #[test]
    fn flat_water_has_unit_jacobian_and_no_new_foam() {
        let result = reference(&vec![[0.0; 2]; 192], &vec![[0.0, 1.0]; 64], step(1.0));
        assert!(result.iter().all(|v| *v == [0.0, 1.0]));
    }
    #[test]
    fn decay_is_in_seconds_and_pause_preserves_history() {
        let fields = vec![[0.0; 2]; 192];
        let initial = vec![[0.8, 1.0]; 64];
        assert_eq!(initial, reference(&fields, &initial, step(0.0)));
        for frames in [30, 60, 120] {
            let mut history = initial.clone();
            for _ in 0..frames {
                history = reference(&fields, &history, step(1.0 / frames as f32));
            }
            assert!((history[0][0] - 0.8 * (-0.6_f32).exp()).abs() < 2e-6);
        }
        let mut clear = step(0.0);
        clear.reset = 1;
        assert!(
            reference(&fields, &initial, clear)
                .iter()
                .all(|v| v[0] == 0.0)
        );
    }
    #[test]
    fn steady_source_is_independent_of_frame_rate_and_bounded() {
        let fields = vec![[0.0; 2]; 192];
        // Test integration independently of the visual threshold range.
        let mut p = step(1.0);
        p.threshold = 1.5;
        let zero = vec![[0.0, 1.0]; 64];
        let expected = reference(&fields, &zero, p)[0][0];
        for frames in [30, 60, 120] {
            let mut history = zero.clone();
            p.dt = 1.0 / frames as f32;
            for _ in 0..frames {
                history = reference(&fields, &history, p);
            }
            assert!((history[0][0] - expected).abs() < 2e-6);
            assert!(history.iter().all(|v| (0.0..=1.0).contains(&v[0])));
        }
    }

    #[test]
    fn cross_derivatives_are_included_and_scale_zero_disables_sources() {
        let mut fields = vec![[0.0; 2]; 192];
        // At (0,0), Dx_z = 0.4 and Dz_x = 0.5, other derivatives zero.
        fields[64 + 8][0] = 0.4;
        fields[64 + 56][0] = -0.4;
        fields[128 + 1][0] = 0.5;
        fields[128 + 7][0] = -0.5;
        let mut p = step(1.0);
        p.displacement_scale = 2.0;
        let result = reference(&fields, &vec![[0.0, 1.0]; 64], p);
        assert!((result[0][1] - 0.2).abs() < 1e-6);
        assert!(result[0][0] > 0.0);
        p.displacement_scale = 0.0;
        assert!(
            reference(&fields, &vec![[0.0, 1.0]; 64], p)
                .iter()
                .all(|v| *v == [0.0, 1.0])
        );
    }
}

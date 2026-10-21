//! Real GPU LUT / production integrator against independent f64 angular sums
//! with exact line integrals. No CPU copy of the segmented WGSL algorithm.
use super::{
    gpu::{Error, Gpu},
    optics::{self, OpticsConfig},
    skylight::{LUT_SIZE, SkyLut, incident_reference},
};
use wgpu::util::DeviceExt;

pub fn reference(config: &OpticsConfig, distance: f64, origin_y: f64, down: f64, c: usize) -> f64 {
    let sigma = config.sigma_t[c] as f64;
    let sigma_s = sigma * config.sky.albedo[c] as f64;
    if !config.sky.enabled || sigma_s == 0.0 {
        return 0.0;
    }
    let above = if origin_y > 0.0 {
        if down > 0.0 {
            (origin_y / down).min(distance)
        } else {
            distance
        }
    } else {
        0.0
    };
    let z0 = (-origin_y + down * above).max(0.0);
    let mut result = 0.0;
    for i in 0..32 {
        let mu = (i as f64 + 0.5) / 32.0;
        let horizon = [0.64, 0.79, 0.85][c];
        let zenith = [0.09, 0.30, 0.57][c];
        let light = (horizon + (zenith - horizon) * mu.powf(0.45))
            * config.sky.color[c] as f64
            * config.sky.intensity as f64;
        let first = optics::integrate(0.0, sigma, light, above)[2];
        let remaining = optics::integrate(0.0, sigma * (1.0 + down / mu), light, distance - above)
            [2]
            * (-sigma * above - sigma * z0 / mu).exp();
        result += first + remaining;
    }
    result * sigma_s * 0.5 / 32.0
}

pub fn run(gpu: &Gpu) -> Result<(), Error> {
    let device = &gpu.device;
    let lut = SkyLut::new(gpu);
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky check optics"),
        size: std::mem::size_of::<super::optics::OpticsUniform>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut cases = Vec::<[f32; 4]>::new();
    for distance in [0.0, 0.01, 2.0, 40.0, 1000.0] {
        for y in [-2.0, 0.0, 2.0] {
            for down in [0.0, 0.02, 0.6, 1.0] {
                cases.push([distance, y, down, 0.0]);
            }
        }
    }
    let inputs = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sky check rays"),
        contents: bytemuck::cast_slice(&cases),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let outputs = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sky check results"),
        size: cases.len() as u64 * 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    // Reuse the actual render uniform declaration to catch CPU/WGSL layout drift.
    let render = include_str!("render.wgsl");
    let declaration = render
        .split("struct Optics {")
        .nth(1)
        .unwrap()
        .split('}')
        .next()
        .unwrap();
    let source = format!(
        "struct Optics {{{declaration}}}\n{}\n{}\n{}",
        include_str!("optics.wgsl"),
        include_str!("skylight.wgsl"),
        r#"
        @group(0) @binding(0) var<uniform> optics: Optics;
        @group(0) @binding(1) var sky_lut: texture_2d<f32>;
        @group(0) @binding(2) var<storage, read> cases: array<vec4<f32>>;
        @group(0) @binding(3) var<storage, read_write> results: array<vec4<f32>>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            if id.x >= arrayLength(&cases) { return; }
            let c = cases[id.x];
            let origin = vec3(0.0, c.y, 0.0);
            let direction = vec3(sqrt(max(1.0 - c.z*c.z, 0.0)), -c.z, 0.0);
            results[2u * id.x] = vec4(integrate_skylight(origin, direction, c.x), 0.0);
            results[2u * id.x + 1u] = vec4(sky_source_at(origin + direction * (c.x*0.5)), 0.0);
        }
    "#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sky production function check"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("sky check"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky check bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&lut.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: inputs.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: outputs.as_entire_binding(),
            },
        ],
    });
    let default = OpticsConfig::default();
    let mut error_by_quality = [0.0_f64; 3];
    let mut lut_error = 0.0_f64;
    for (quality, steps) in [8, 16, 32].into_iter().enumerate() {
        for variant in 0..7 {
            let mut config = default;
            config.sky.steps = steps;
            match variant {
                1 => config.sky.enabled = false,
                2 => config.sky.albedo = [0.0; 3],
                3 => config.sigma_t = [0.0, 1e-9, 10.0],
                4 => {
                    config.sky.intensity = 2.0;
                    config.sky.color = [0.3, 1.0, 0.7];
                }
                5 => config.sky.intensity = 0.0,
                6 => config.sigma_t = [10.0, 0.00001, 0.02],
                _ => {}
            }
            gpu.queue
                .write_buffer(&uniform, 0, bytemuck::bytes_of(&config.uniform()));
            lut.update(gpu, &config.sky);
            let mut encoder = device.create_command_encoder(&Default::default());
            lut.encode(&mut encoder);
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sky LUT readback"),
                size: u64::from(LUT_SIZE) * 16,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                lut.texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(LUT_SIZE * 16),
                        rows_per_image: Some(1),
                    },
                },
                lut.texture.size(),
            );
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups((cases.len() as u32).div_ceil(64), 1, 1);
            }
            gpu.queue.submit([encoder.finish()]);
            let bytes = gpu.map_buffer(&readback)?;
            let texels: Vec<[f32; 4]> = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            for (i, texel) in texels.iter().enumerate() {
                let tau = 32.0 * (i as f64 / (LUT_SIZE - 1) as f64).powi(2);
                for (c, value) in texel.iter().take(3).enumerate() {
                    let expected = incident_reference(&config.sky, tau, c, 32);
                    let error = (*value as f64 - expected).abs();
                    if !value.is_finite() || error > 2e-6 {
                        return Err(
                            format!("sky LUT mismatch: {i}/{c}, {value} vs {expected}").into()
                        );
                    }
                    lut_error = lut_error.max(error);
                }
            }
            let bytes = gpu.read_buffer(&outputs, outputs.size())?;
            let values: Vec<[f32; 4]> = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            for (i, case) in cases.iter().enumerate() {
                for (c, actual_value) in values[2 * i].iter().take(3).enumerate() {
                    let expected =
                        reference(&config, case[0] as f64, case[1] as f64, case[2] as f64, c);
                    let value = *actual_value as f64;
                    let error = (value - expected).abs();
                    let tolerance =
                        [0.025, 0.008, 0.003][quality] * (1.0 + config.sky.intensity as f64);
                    if !value.is_finite() || error > tolerance || (expected == 0.0 && value != 0.0)
                    {
                        return Err(format!("sky integral mismatch: steps={steps}, variant={variant}, ray={case:?}, channel={c}: {value} vs {expected}, error={error}").into());
                    }
                    error_by_quality[quality] = error_by_quality[quality].max(error);
                    let z = (-(case[1] as f64) + case[2] as f64 * case[0] as f64 * 0.5).max(0.0);
                    let local = if config.sky.enabled {
                        config.sigma_t[c] as f64
                            * config.sky.albedo[c] as f64
                            * incident_reference(&config.sky, config.sigma_t[c] as f64 * z, c, 32)
                    } else {
                        0.0
                    };
                    let actual = values[2 * i + 1][c] as f64;
                    if !actual.is_finite() || (actual - local).abs() > 0.002 * (1.0 + local) {
                        return Err(
                            format!("sky local source mismatch: {actual} vs {local}").into()
                        );
                    }
                }
            }
        }
    }
    if error_by_quality[2] >= error_by_quality[0] {
        return Err("sky integration did not converge".into());
    }
    println!(
        "GPU skylight: LUT max error={lut_error:.3e}; 60 rays x 7 optical cases/RGB, max integration errors (8/16/32)={error_by_quality:?} OK"
    );
    Ok(())
}

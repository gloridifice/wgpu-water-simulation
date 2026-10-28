//! Controlled experiments using the production GPU foam transport and sources.
use super::{Gpu, Ocean, OpticsConfig, RESOLUTION, Shore};

type Error = crate::ocean::Error;

fn read(gpu: &Gpu, shore: &Shore) -> Result<Vec<[f32; 4]>, Error> {
    Ok(gpu
        .read_buffer(&shore.field, shore.field.size())?
        .as_chunks::<16>()
        .0
        .iter()
        .map(|b| bytemuck::pod_read_unaligned(b))
        .collect())
}

// Area factors cancel in the comparisons because the grid is uniform.
fn totals(data: &[[f32; 4]], n: usize) -> [f64; 5] {
    let cells = n * n;
    let mut result = [0.0; 5];
    for i in 0..cells {
        let amount = f64::from(data[cells + i][3]);
        let px = f64::from(data[3 * cells + i][0]);
        let pz = f64::from(data[3 * cells + i][1]);
        result[0] += amount;
        result[1] += px;
        result[2] += pz;
        result[3] += (i % n) as f64 * 0.25 * amount;
        if amount > 0.0 {
            result[4] += 0.5 * (px * px + pz * pz) / amount;
        }
    }
    result
}

pub(super) fn verify(gpu: &Gpu) -> Result<(), Error> {
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    verify_sources(gpu, &ocean)?;
    let n = RESOLUTION as usize;
    let cells = n * n;
    // Two 2 m strips: rear 1 m/s, front 0.2 m/s, separated by 1 m.
    // Water remains stationary. No source, drag, decay, or shoreline deposition.
    // The inlet is over 30 m away; the wave from its relaxation cannot arrive.
    // Repeat MUSCL near vacuum: density scaling must not erase momentum or
    // produce a different velocity because of a reconstruction denominator floor.
    for (order, space_order, amplitude) in [(1, 1, 1.0), (2, 1, 1.0), (2, 2, 1.0), (2, 2, 1e-7)] {
        let mut shore = Shore::new(gpu, &ocean);
        let pipeline = shore.solver_pipeline(gpu, order, space_order).clone();
        let stage_buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("foam collision predictor"),
            size: shore.field.size(),
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let mut seed = vec![[0.0_f32; 4]; cells * 5];
        for z in 0..n {
            for x in 0..n {
                let i = z * n + x;
                let (amount, speed) = if (80..112).contains(&z) {
                    if (40..48).contains(&x) {
                        (0.8, 1.0)
                    } else if (52..60).contains(&x) {
                        (0.2, 0.2)
                    } else {
                        (0.0, 0.0)
                    }
                } else {
                    (0.0, 0.0)
                };
                let amount = amount * amplitude;
                seed[i] = [amount, 1.0, 2.0, 0.0];
                seed[cells + i] = [2.0, 0.0, 0.0, amount];
                seed[2 * cells + i] = [0.0, amount, 2.0, 0.0];
                seed[3 * cells + i] = [amount * speed, 0.0, 0.0, 0.0];
            }
        }
        let initial = totals(&seed, n);
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, 64.0, 64.0];
        p.terrain[1] = 0.0;
        p.options[0] = 0.0;
        p.options[3] = 0.0;
        p.foam_front[0] = 0.0;
        p.foam_inertia[0] = 0.0;
        p.foam_inertia[2] = space_order as f32;
        p.timing[1] = 0.002;
        p.timing[2] = 0.0;
        p.solver[0] = order as f32;
        // Two immutable stage uniforms allow batching steps without queue writes
        // aliasing uniforms used by earlier dispatches.
        let mut stage_bindings = Vec::new();
        for stage in 0..order {
            p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
            use wgpu::util::DeviceExt;
            let uniform = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("foam collision stage parameters"),
                    contents: bytemuck::bytes_of(&p),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
            // Access buffers through the production bindings: their non-uniform
            // entries are identical to Shore::new's main and RK bindings.
            let (input, output) = if order == 1 {
                (&shore.previous, &shore.field)
            } else if stage == 0 {
                (&shore.previous, &stage_buffer)
            } else {
                (&stage_buffer, &shore.field)
            };
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("foam collision stage"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: input.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: output.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: shore.fft_fields.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: shore.fft_previous.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: shore.previous.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: shore.terrain_bed.as_entire_binding(),
                    },
                ],
            });
            stage_bindings.push(bind);
        }
        let steps = 2500;
        for _ in 0..steps / 50 {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            for _ in 0..50 {
                for bind in &stage_bindings {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, bind, &[]);
                    pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                }
                enc.copy_buffer_to_buffer(&shore.field, 0, &shore.previous, 0, shore.field.size());
            }
            gpu.queue.submit([enc.finish()]);
        }
        let data = read(gpu, &shore)?;
        let end = totals(&data, n);
        let peak = data[cells..2 * cells]
            .iter()
            .map(|q| q[3])
            .fold(0.0, f32::max);
        let travel = end[3] / end[0] - initial[3] / initial[0];
        let expected_travel = initial[1] / initial[0] * f64::from(p.timing[1]) * steps as f64;
        let mass_error = (end[0] / initial[0] - 1.0).abs();
        let momentum_error = (end[1] / initial[1] - 1.0).abs();
        // Faster foam catches slower foam: the peak exceeds either initial
        // density and mixing dissipates relative kinetic energy, not momentum.
        if data.iter().flatten().any(|v| !v.is_finite())
            || data[cells..2 * cells].iter().any(|q| q[3] < 0.0)
            || mass_error > 0.0003
            || momentum_error > 0.0003
            || end[2].abs() > 1e-6
            // Spatially varying Rusanov viscosity shifts the first moment by
            // O(dx), while total amount/momentum must remain conservative.
            || (travel - expected_travel).abs() > 0.125
            || peak < 0.9 * amplitude
            || end[4] >= initial[4] * 0.99
        {
            return Err(format!("foam catch-up time {order} space {space_order} scale {amplitude}: mass error {mass_error}, momentum error {momentum_error}, travel {travel}/{expected_travel}, peak {peak}, energy {}/{}, state {end:?}", end[4], initial[4]).into());
        }
        eprintln!(
            "Foam catch-up time {order} space {space_order} scale {amplitude}: 5 s, mean speed {:.6} m/s, travel {travel:.6} m, peak/scale {:.6} (initial 0.8), mass error {mass_error:.3e}, momentum error {momentum_error:.3e}, energy {:.3}->{:.3}",
            end[1] / end[0],
            peak / amplitude,
            initial[4],
            end[4]
        );
    }
    Ok(())
}

fn verify_sources(gpu: &Gpu, ocean: &Ocean) -> Result<(), Error> {
    let shore = Shore::new(gpu, ocean);
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("foam momentum source probe"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../shaders/shore.wgsl"),
                    include_str!("../shaders/shore_compute.wgsl"),
                    r#"
@compute @workgroup_size(1)
fn source_probe() {
    let input=old_shore[0];
    new_shore[0]=vec4(foam_sources(input.xyz,vec2(1.0,-0.5),shore.options.x),0.0);
}"#
                )
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("source_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shore.previous.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
        ],
    });
    for (drag, response, decay, source, birth, amount, steps) in [
        (false, 2.0, 0.0, 0.8, 1.0, 0.2, 1),
        (false, 2.0, 0.7, 0.0, 1.0, 0.2, 100),
        (true, 2.0, 0.3, 0.8, 1.0, 0.2, 1),
        (true, 2.0, 0.3, 0.8, 1.0, 0.2, 100),
        (true, 0.0, 0.3, 0.8, 0.5, 0.2, 1),
        (true, 0.000000001, 0.3, 0.8, 0.5, 0.2, 1),
        (true, 20.0, 0.0, 0.8, 0.0, 0.0, 100),
        (false, 0.0, 0.0, 0.0, 1.0, 0.0, 1),
    ] {
        let seed = [amount as f32, (amount * 0.2) as f32, 0.0, 0.0];
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.foam_motion = [1.0, response as f32, 0.0, 0.0];
        p.foam_inertia = [f32::from(drag), birth as f32, 0.0, 0.0];
        p.timing[1] = 1.0 / steps as f32;
        p.timing[2] = decay as f32;
        p.options[0] = source as f32;
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        for _ in 0..steps {
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            enc.copy_buffer_to_buffer(&shore.field, 0, &shore.previous, 0, 16);
        }
        gpu.queue.submit([enc.finish()]);
        let bytes = gpu.read_buffer(&shore.field, 16)?;
        let actual: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[..16]);
        // Independent double-precision integration (10,000 RK4 steps), rather
        // than repeating the shader's closed-form expression as the reference.
        let mut expected = [amount, amount * 0.2, 0.0];
        let instant = drag && response < 0.000001;
        let rhs = |s: [f64; 3]| {
            let rate = if drag && !instant {
                1.0 / response
            } else {
                0.0
            };
            [
                source - decay * s[0],
                source * birth - decay * s[1] + rate * (s[0] - s[1]),
                -0.5 * source * birth - decay * s[2] + rate * (-0.5 * s[0] - s[2]),
            ]
        };
        let add =
            |a: [f64; 3], b: [f64; 3], factor: f64| std::array::from_fn(|i| a[i] + b[i] * factor);
        for _ in 0..10000 {
            let a = rhs(expected);
            let b = rhs(add(expected, a, 0.00005));
            let c = rhs(add(expected, b, 0.00005));
            let d = rhs(add(expected, c, 0.0001));
            expected = std::array::from_fn(|i| {
                expected[i] + (a[i] + 2.0 * b[i] + 2.0 * c[i] + d[i]) / 60000.0
            });
        }
        if instant {
            expected[1] = expected[0];
            expected[2] = -0.5 * expected[0];
        }
        if actual.iter().any(|v| !v.is_finite())
            || (0..3).any(|i| (f64::from(actual[i]) - expected[i]).abs() > 0.00002)
        {
            return Err(format!("foam momentum sources drag {drag} response {response} steps {steps}: {actual:?} expected {expected:?}").into());
        }
    }
    eprintln!(
        "Foam source momentum: 0.2 at 0.2 m/s + 0.8 at 1 m/s -> 1 at 0.84 m/s; birth, drag, decay, empty cells and timestep consistency vs independent RK4 OK"
    );
    Ok(())
}

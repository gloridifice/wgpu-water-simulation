//! Runtime GPU behavior checks; compiled for --verify as well as tests.
use super::*;
mod collider;
mod foam_dual;
mod foam_height;
mod foam_material;
mod foam_momentum;
mod foam_ridge;
mod terrain;

pub fn verify_terrain(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    terrain::verify(gpu)
}

/// Deterministic FFT field tests reset sampling, the entire patch and gain changes
/// at paused time. The previous FFT is intentionally unrelated to this field.
fn verify_fft_initialization(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            resolution: 32,
            ..Default::default()
        },
    )?;
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("synthetic FFT initialization test"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var<storage,read_write> data:array<vec2<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=6144u {return;}
    var value=0.0;
    if id.x>=3072u && id.x<4096u {value=0.02;}
    if id.x>=4096u && id.x<5120u {value=0.03;}
    if id.x>=5120u {value=-0.01;}
    data[id.x]=vec2(value,0.0);
}"#
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("synthetic FFT fill"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: ocean.fields.as_entire_binding(),
        }],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(96, 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let mut shore = Shore::new(gpu, &ocean);
    let mut optics = OpticsConfig::beach_scene();
    let cells = (RESOLUTION * RESOLUTION) as usize;
    for gain in [0.0, 1.0, 3.0, 0.5] {
        optics.shore.input_gain = gain;
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        shore.encode(gpu, &mut enc, &optics, 2.0, false, 0.5);
        gpu.queue.submit([enc.finish()]);
        let p = shore.last.unwrap();
        if p.options[3] != 1.0 || p.coupling[2] != 0.5 * gain {
            return Err("FFT input gain did not reset/scale".into());
        }
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        let data: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        for y in 0..RESOLUTION as usize {
            for x in 0..RESOLUTION as usize {
                let i = y * RESOLUTION as usize + x;
                let world_x = p.domain[0] + (x as f32 + 0.5) * p.domain[2] / RESOLUTION as f32;
                let bottom = -optics.beach_slope * (world_x - optics.shore_x);
                let h = (0.02 * 0.5 * gain - bottom).max(0.0);
                let mut mx = 0.03 * 0.5 * gain;
                let mut mz = -0.01 * 0.5 * gain;
                let limit = (8.0 * h / mx.hypot(mz).max(1e-6)).min(1.0);
                mx *= limit;
                mz *= limit;
                if h <= 0.003 {
                    mx = 0.0;
                    mz = 0.0;
                }
                let actual = data[cells + i];
                if (actual[0] - h).abs() > 2e-5
                    || (actual[1] - mx).abs() > 2e-5
                    || (actual[2] - mz).abs() > 2e-5
                    || data[i][0] != 0.0
                    || data[2 * cells + i][0] != 0.0
                {
                    return Err(format!("FFT initialization mismatch gain={gain} cell={i}: {actual:?}, expected {h}/{mx}/{mz}").into());
                }
            }
        }
        // Render-only refresh must not multiply the input gain twice.
        shore.configure(gpu, &optics);
        let refreshed = shore.parameters(&optics, 2.0, false, p.solver[2]);
        if refreshed.coupling[2] != p.coupling[2] || refreshed.options[3] != 0.0 {
            return Err("FFT gain changed on refresh".into());
        }
    }
    eprintln!(
        "SWE FFT initialization: whole patch, current-frame sampling, dry momentum, paused gain resets 0/1/3/0.5 OK"
    );
    Ok(())
}

/// Check production inlet sampling against CPU bilinear/time interpolation.
fn verify_inlet(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            resolution: 32,
            ..Default::default()
        },
    )?;
    let shore = Shore::new(gpu, &ocean);
    let mut frames = Vec::new();
    for time in [1.0, 2.0] {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode(gpu, &mut encoder, time);
        gpu.queue.submit([encoder.finish()]);
        frames.push(ocean.read_fields(gpu)?);
    }
    let n = ocean.config.resolution as usize;
    let count = n * n;
    gpu.queue.write_buffer(
        &shore.fft_previous,
        0,
        bytemuck::cast_slice(&frames[0][3 * count..]),
    );
    let mut params = shore.parameters(&OpticsConfig::beach_scene(), 8.0, false, 0.22);
    params.waves[1] = 8.0;
    params.coupling[3] = 0.25;
    gpu.queue
        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&params));
    let source = format!(
        "{}\n{}\n{}",
        include_str!("../shaders/shore.wgsl"),
        include_str!("../shaders/shore_compute.wgsl"),
        r#"
@compute @workgroup_size(8,8)
fn inlet_probe(@builtin(global_invocation_id) id:vec3<u32>) {
    let n=u32(shore.options.z); if id.x>=n || id.y>=n { return; }
    let c=vec2<i32>(id.xy);
    new_shore[id.y*n+id.x]=vec4(incident(world(c),max(-bed(c),0.1)),0.0);
}"#
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("inlet sampling verification"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("inlet probe"),
            layout: None,
            module: &shader,
            entry_point: Some("inlet_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("inlet probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: ocean.fields.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: shore.fft_previous.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: shore.terrain_bed.as_entire_binding(),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.read_buffer(&shore.field, u64::from(RESOLUTION.pow(2)) * 16)?;
    let samples: Vec<[f32; 4]> = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|b| bytemuck::pod_read_unaligned(b))
        .collect();
    let mut max_error = 0.0_f32;
    for z in [0, 17, 128, 255] {
        for x in [220, 240, 255] {
            let p = [
                params.domain[0] + (x as f32 + 0.5) / RESOLUTION as f32 * params.domain[2],
                params.domain[1] + (z as f32 + 0.5) / RESOLUTION as f32 * params.domain[3],
            ];
            let grid = p.map(|v| (v / ocean.config.length + 0.5) * n as f32);
            let ix = grid[0].floor() as i32;
            let iz = grid[1].floor() as i32;
            let fx = grid[0] - grid[0].floor();
            let fz = grid[1] - grid[1].floor();
            let mut expected = [0.0_f32; 3];
            for (channel, value) in expected.iter_mut().enumerate() {
                let at = |dx: i32, dz: i32| {
                    let i = (3 + channel) * count
                        + (iz + dz).rem_euclid(n as i32) as usize * n
                        + (ix + dx).rem_euclid(n as i32) as usize;
                    0.75 * frames[0][i][0] + 0.25 * frames[1][i][0]
                };
                *value = ((1.0 - fz) * ((1.0 - fx) * at(0, 0) + fx * at(1, 0))
                    + fz * ((1.0 - fx) * at(0, 1) + fx * at(1, 1)))
                    * params.coupling[2];
            }
            let depth = (params.terrain[1] * (p[0] - params.terrain[2])).max(0.1);
            let safety = 1.0_f32
                .min(0.5 * depth / expected[0].abs().max(1e-6))
                .min(8.0 * depth / expected[1].hypot(expected[2]).max(1e-6));
            for c in 0..3 {
                max_error = max_error
                    .max((samples[z * RESOLUTION as usize + x][c] - expected[c] * safety).abs());
            }
        }
    }
    if max_error > 0.00001 {
        return Err(format!("FFT inlet coordinate/time/gain mismatch {max_error}").into());
    }
    eprintln!(
        "FFT inlet: world-space wrapping, height gain and substep temporal interpolation error {max_error:.2e} OK"
    );
    Ok(())
}

/// Compare both orders against a linear traveling gravity wave (16 cells/wavelength).
/// Flat bed and an interior measurement region isolate propagation from the sponge.
fn verify_wave_accuracy(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let n = RESOLUTION as usize;
    let cells = n * n;
    let depth = 2.0_f32;
    let amplitude = 0.001_f32;
    let speed = (9.81 * depth).sqrt();
    let k = std::f32::consts::TAU / 4.0;
    let dt = 0.004_f32;
    let steps = 250;
    let mut errors = [0.0_f64; 2];
    for order in [1u32, 2] {
        let shore = Shore::new(gpu, &ocean);
        let mut seed = vec![[0.0_f32; 4]; cells * 5];
        for z in 0..n {
            for x in 0..n {
                let eta = amplitude * (k * (x as f32 + 0.5) * 0.25).sin();
                let i = z * n + x;
                seed[i] = [0.0, 1.0, depth + eta, 0.0];
                seed[cells + i] = [depth + eta, speed * eta, 0.0, 0.0];
                seed[2 * cells + i] = [0.0, 0.0, depth + eta, 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let optics = OpticsConfig::beach_scene();
        let mut p = shore.parameters(&optics, 0.0, false, 0.0);
        p.terrain[1] = 0.0;
        p.terrain[2] = 0.0;
        p.domain = [0.0, 0.0, 64.0, 64.0];
        p.options[0] = 0.0;
        p.foam_front[0] = 0.0;
        p.options[3] = 0.0;
        p.timing[1] = dt;
        p.solver[0] = order as f32;
        for _ in 0..steps {
            for stage in 0..order {
                p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
                gpu.queue
                    .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&shore.pipeline);
                    pass.set_bind_group(
                        0,
                        if order == 1 {
                            &shore.bind
                        } else if stage == 0 {
                            &shore.predictor_bind
                        } else {
                            &shore.corrector_bind
                        },
                        &[],
                    );
                    pass.dispatch_workgroups(RESOLUTION.div_ceil(8), RESOLUTION.div_ceil(8), 1);
                }
                if stage + 1 == order {
                    enc.copy_buffer_to_buffer(
                        &shore.field,
                        0,
                        &shore.previous,
                        0,
                        shore.field.size(),
                    );
                }
                gpu.queue.submit([enc.finish()]);
            }
        }
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        let data: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        if data.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite traveling wave".into());
        }
        let mut error = 0.0;
        let mut norm = 0.0;
        for x in 48..160 {
            let expected =
                amplitude * (k * ((x as f32 + 0.5) * 0.25 - speed * dt * steps as f32)).sin();
            let actual = data[cells + n / 2 * n + x][0] - depth;
            error += (actual - expected).powi(2) as f64;
            norm += expected.powi(2) as f64;
        }
        errors[(order - 1) as usize] = (error / norm).sqrt();
    }
    if errors[1] >= errors[0] * 0.75 || errors[1] > 0.25 {
        return Err(format!("second-order wave accuracy did not improve: {errors:?}").into());
    }
    eprintln!(
        "SWE 4 m wave after 1 s: relative L2 error first={:.3}, second={:.3}",
        errors[0], errors[1]
    );
    Ok(())
}

/// Run the production solver on the GPU, including stationary and moving water.
pub fn verify(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    foam_material::verify(gpu)?;
    collider::verify(gpu)?;
    foam_height::verify(gpu)?;
    foam_momentum::verify(gpu)?;
    foam_ridge::verify(gpu)?;
    foam_dual::verify(gpu)?;
    verify_inlet(gpu)?;
    verify_fft_initialization(gpu)?;
    verify_wave_accuracy(gpu)?;
    verify_surface_foam(gpu)?;
    verify_foam_motion(gpu)?;
    verify_foam_generation_controls(gpu)?;
    verify_front_foam(gpu)?;
    for (order, space_order) in [(1, 1), (2, 1), (2, 2)] {
        verify_order(gpu, order, space_order)?;
    }
    Ok(())
}

fn verify_order(gpu: &Gpu, order: u32, space_order: u32) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            wind_angle: 3.64,
            ..Default::default()
        },
    )?;
    let mut shore = Shore::new(gpu, &ocean);
    let mut optics = OpticsConfig::beach_scene();
    optics.shore.order = order;
    optics.shore.foam_transport_order = space_order;
    let gain = 0.0;
    let advance = |shore: &mut Shore, optics: &OpticsConfig, time, reset, gain| {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode_filtered(gpu, &mut encoder, time, &optics.shore);
        shore.encode(gpu, &mut encoder, optics, time, reset, gain);
        gpu.queue.submit([encoder.finish()]);
    };
    let read = |shore: &Shore| -> Result<Vec<[f32; 4]>, crate::ocean::Error> {
        Ok(gpu
            .read_buffer(&shore.field, shore.field.size())?
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect())
    };
    let cells = (RESOLUTION * RESOLUTION) as usize;
    advance(&mut shore, &optics, 0.0, true, gain);
    let initial = read(&shore)?;
    for i in 1..=60 {
        advance(&mut shore, &optics, i as f32 / 60.0, false, gain);
    }
    let still = read(&shore)?;
    let error = still[cells..2 * cells]
        .iter()
        .zip(&initial[cells..2 * cells])
        .map(|(a, b)| (a[0] - b[0]).abs().max(a[1].abs()).max(a[2].abs()))
        .fold(0.0_f32, f32::max);
    if error > 0.0001 {
        return Err(format!("SWE lake-at-rest drift: {error}").into());
    }
    // Local pulse: conserve water away from the open sponge and propagate in both axes.
    let mut seed = still.clone();
    let n = RESOLUTION as usize;
    let cx = n / 2;
    let cz = n / 2;
    let center = cz * n + cx;
    seed[cells + center][0] += 0.2;
    seed[cells + center][3] = 0.1;
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    advance(&mut shore, &optics, 1.05, false, gain);
    let pulse = read(&shore)?;
    let mass_before: f64 = seed[cells..2 * cells].iter().map(|q| q[0] as f64).sum();
    let mass_after: f64 = pulse[cells..2 * cells].iter().map(|q| q[0] as f64).sum();
    if (mass_before - mass_after).abs() > 0.005
        || pulse[cells + center + 1][0] <= still[cells + center + 1][0]
        || pulse[cells + center + n][0] <= still[cells + center + n][0]
        || pulse[cells + center + n][3] <= 0.0
    {
        return Err(format!(
            "SWE pulse mass/2D transport failed: mass delta {}",
            mass_after - mass_before
        )
        .into());
    }
    // Drying clears foam and momentum but preserves independently drying sand.
    let mut seed = still.clone();
    let dry = n / 8 + n / 2 * n;
    seed[dry] = [0.8, 1.0, 0.01, 0.0];
    seed[cells + dry][3] = 0.8;
    seed[3 * cells + dry][0] = 0.8;
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    advance(&mut shore, &optics, 1.10, false, gain);
    let dried = read(&shore)?;
    if dried[dry][0] != 0.0
        || dried[cells + dry][3] != 0.0
        || dried[dry][1] < 0.9
        || dried[3 * cells + dry][..2] != [0.0, 0.0]
        || dried[2 * cells + dry][..2] != [0.0, 0.0]
    {
        return Err("dry cell retained foam or lost wet sand history".into());
    }
    advance(&mut shore, &optics, 1.15, false, gain);
    let faded = read(&shore)?;
    let expected = dried[dry][1] * (-0.05_f32 / optics.shore.drying_time).exp();
    if (faded[dry][1] - expected).abs() > 0.0001 {
        return Err("wet sand drying is not in seconds".into());
    }
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&still));
    advance(&mut shore, &optics, 1.20, false, gain);
    if read(&shore)?[center][1] < 0.99 {
        return Err("water did not restore sand wetness".into());
    }
    // Mode changes reset water and ground history, even with time paused.
    optics.shore.order = 3 - order;
    advance(&mut shore, &optics, 1.20, false, gain);
    if shore.last.unwrap().options[3] != 1.0 {
        return Err("SWE order switch did not reset".into());
    }
    optics.shore.order = order;
    // Timestamp path is optional; check real asynchronous readback on supporting devices.
    shore.timer = crate::ocean::gpu::timer::GpuTimer::new(gpu);
    advance(&mut shore, &optics, 1.20, true, gain);
    advance(&mut shore, &optics, 1.25, false, gain);
    let _ = read(&shore)?;
    if let Some(timer) = &mut shore.timer {
        timer.poll(gpu);
        let sample = timer.latest.ok_or("SWE timestamp readback missing")?;
        if !sample.ms.is_finite()
            || sample.ms <= 0.0
            || sample.order != order
            || sample.substeps == 0
        {
            return Err("invalid SWE GPU timing".into());
        }
        eprintln!(
            "SWE order {order}, foam space {space_order}: GPU {:.3} ms / {} substeps / {:.1} simulated ms",
            sample.ms,
            sample.substeps,
            sample.simulated_dt * 1000.0
        );
    }
    shore.timer = None;
    let gain = 1.0;
    advance(&mut shore, &optics, 0.0, true, gain);
    for i in 1..=1440 {
        advance(&mut shore, &optics, i as f32 / 60.0, false, gain);
    }
    let active = read(&shore)?;
    if active.iter().flatten().any(|v| !v.is_finite())
        || active[cells..2 * cells]
            .iter()
            .any(|q| q[0] < 0.0 || q[3] < 0.0 || q[3] > 1e6)
        || !active[..cells].iter().any(|q| q[0] > 0.1)
        || active[2 * cells..3 * cells]
            .iter()
            .any(|q| q[..2] != [0.0, 0.0])
    {
        return Err(
            "SWE run-up: invalid state, missing water foam or unexpected ground residue".into(),
        );
    }
    advance(&mut shore, &optics, 24.0, false, gain);
    if read(&shore)? != active {
        return Err("SWE pause changed state".into());
    }
    advance(&mut shore, &optics, 24.0, true, gain);
    let cleared = read(&shore)?;
    if cleared[..cells].iter().any(|q| q[0] != 0.0 || q[1] != 0.0)
        || cleared[2 * cells..3 * cells].iter().any(|q| q[0] != 0.0)
        || cleared[3 * cells..].iter().any(|q| *q != [0.0; 4])
    {
        return Err("SWE reset failed".into());
    }
    optics.shore.enabled = false;
    advance(&mut shore, &optics, 24.1, false, gain);
    if read(&shore)?.iter().any(|q| *q != [0.0; 4]) {
        return Err("SWE disable failed".into());
    }
    optics.shore.enabled = true;
    advance(&mut shore, &optics, 24.1, false, gain);
    let before_normal_edit = read(&shore)?;
    optics.shore.normal_strength = 0.0;
    optics.shore.foam_height_max = 0.3;
    optics.shore.foam_height_threshold = 0.2;
    optics.shore.foam_height_transition = 0.8;
    optics.shore.foam_generation_threshold = 0.05;
    optics.shore.foam_generation_transition = 0.15;
    optics.shore.foam_front_generation = 5.0;
    optics.shore.foam_front_distance = 0.5;
    optics.shore.foam_velocity_threshold = 0.2;
    optics.shore.foam_velocity_transition = 1.0;
    optics.shore.foam_follow = 0.6;
    optics.shore.foam_response_time = 0.7;
    optics.shore.foam_drag_enabled = false;
    optics.shore.foam_birth_speed = 0.5;
    optics.shore.foam_transport_order = 3 - space_order;
    optics.shore.foam_background_multiplier = 0.05;
    optics.shore.water_foam_color = [0.2, 0.7, 0.9];
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 0.0 || read(&shore)? != before_normal_edit {
        return Err("live normal/color/foam-height/generation edit changed SWE history".into());
    }
    optics.shore.normal_strength = 1.0;
    optics.shore.foam_generation_threshold = 0.25;
    optics.shore.foam_generation_transition = 0.4;
    optics.shore.foam_front_generation = 8.0;
    optics.shore.foam_front_distance = 1.0;
    optics.shore.foam_velocity_threshold = 0.1;
    optics.shore.foam_velocity_transition = 0.5;
    optics.shore.foam_follow = 0.8;
    optics.shore.foam_response_time = 0.35;
    optics.shore.foam_drag_enabled = true;
    optics.shore.foam_birth_speed = 1.0;
    optics.shore.foam_transport_order = space_order;
    optics.shore.foam_background_multiplier = 0.15;
    optics.shore.filter_cutoff = 3.0;
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 1.0 {
        return Err("SWE filter edit did not reset history".into());
    }
    optics.shore.filter_enabled = false;
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 1.0 {
        return Err("SWE filter toggle did not reset history".into());
    }
    optics.shore.filter_enabled = true;
    optics.shore.filter_cutoff = 12.0;
    let gain = 2.0;
    for slope in [0.01, 1.0] {
        optics.beach_slope = slope;
        advance(&mut shore, &optics, 0.0, true, gain);
        for i in 1..=180 {
            advance(&mut shore, &optics, i as f32 / 30.0, false, gain);
        }
        let extreme = read(&shore)?;
        if extreme.iter().flatten().any(|v| !v.is_finite())
            || extreme[cells..2 * cells]
                .iter()
                .any(|q| q[0] < 0.0 || q[3] < 0.0 || q[3] > 1e6)
        {
            return Err(format!("SWE invalid state at slope {slope}").into());
        }
    }
    optics.shore_x += 4.0;
    advance(&mut shore, &optics, 6.0, false, gain);
    let moved = read(&shore)?;
    if moved[..cells].iter().any(|q| q[0] != 0.0 || q[1] != 0.0) {
        return Err("SWE moved terrain retained history".into());
    }
    let zero = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let mut quiet = Shore::new(gpu, &zero);
    let mut quiet_optics = OpticsConfig::beach_scene();
    quiet_optics.shore.order = order;
    quiet_optics.shore.foam_transport_order = space_order;
    for i in 0..=120 {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let time = i as f32 / 30.0;
        zero.encode(gpu, &mut encoder, time);
        quiet.encode(gpu, &mut encoder, &quiet_optics, time, i == 0, 1.0);
        gpu.queue.submit([encoder.finish()]);
    }
    let quiet_state = read(&quiet)?;
    if quiet_state[..cells]
        .iter()
        .any(|v| v[0] > 0.00001 || v[3].abs() > 0.0001)
    {
        return Err("zero FFT amplitude generated shore waves/foam".into());
    }
    eprintln!(
        "SWE GPU order {order}, foam space {space_order}: lake at rest ({error:.2e}), pulse mass/2D foam transport, dry foam removal and sand drying/rewetting, run-up, pause/reset/disable, terrain reset and extreme slopes OK"
    );
    Ok(())
}

/// Probe the production generation function, including a zero-width cutoff.
fn verify_foam_generation_controls(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(gpu, crate::ocean::parameters::ocean::OceanConfig::default())?;
    let shore = Shore::new(gpu, &ocean);
    let source = format!(
        "{}\n{}\n{}",
        include_str!("../shaders/shore.wgsl"),
        include_str!("../shaders/shore_compute.wgsl"),
        r#"
@compute @workgroup_size(8)
fn generation_probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let samples=array<f32,8>(0.0,0.05,0.1,0.25,0.45,0.65,1.0,4.0);
    if id.x>=8u { return; }
    new_shore[id.x]=vec4(samples[id.x],foam_breaking_weight(samples[id.x]),foam_velocity_weight(samples[id.x]),0.0);
}"#
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("foam generation control probe"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("foam generation control probe"),
            layout: None,
            module: &shader,
            entry_point: Some("generation_probe"),
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
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
        ],
    });
    for (threshold, transition) in [
        (0.25_f32, 0.4_f32),
        (0.05, 0.2),
        (0.25, 0.0),
        (0.0, 0.0),
        (2.0, 2.0),
    ] {
        let mut optics = OpticsConfig::beach_scene();
        optics.shore.foam_generation_threshold = threshold;
        optics.shore.foam_generation_transition = transition;
        optics.shore.foam_velocity_threshold = threshold;
        optics.shore.foam_velocity_transition = transition;
        optics.validate()?;
        let params = shore.parameters(&optics, 0.0, false, 1.0);
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&params));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&shore.field, 8 * 16)?;
        for b in bytes.as_chunks::<16>().0 {
            let sample: [f32; 4] = bytemuck::pod_read_unaligned(b);
            let expected = if transition == 0.0 {
                f32::from(sample[0] > threshold)
            } else {
                let t = ((sample[0] - threshold) / transition).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            if sample[1..=2]
                .iter()
                .any(|v| !v.is_finite() || (*v - expected).abs() > 1e-6)
            {
                return Err(format!("foam generation threshold={threshold} width={transition}: {sample:?} expected={expected}").into());
            }
        }
    }
    eprintln!(
        "Foam generation controls: rise/velocity thresholds, transition width, hard cutoff and quiet water OK"
    );
    Ok(())
}

/// Probe directional compression and source integration using analytic flows.
fn verify_front_foam(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(gpu, crate::ocean::parameters::ocean::OceanConfig::default())?;
    let shore = Shore::new(gpu, &ocean);
    let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("front foam probe"),
        source: wgpu::ShaderSource::Wgsl(format!("{}\n{}\n{}",
            include_str!("../shaders/shore.wgsl"), include_str!("../shaders/shore_compute.wgsl"),
            "@compute @workgroup_size(1) fn front_probe() { new_shore[0]=vec4(foam_front_weight(vec2<i32>(i32(shore.visual.y),96)),0.0,0.0,0.0); }").into()),
    });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("front foam probe"),
            layout: None,
            module: &shader,
            entry_point: Some("front_probe"),
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
    let n = RESOLUTION as usize;
    let cells = n * n;
    // Analytic linear flows distinguish catch-up, separation, reversal and shear.
    for (case, (flow, gradient, spacing, distance, strength)) in [
        (
            [2.0_f32, 0.0],
            [-0.4_f32, 0.0],
            [0.25_f32, 1.5],
            1.0_f32,
            0.4_f32,
        ),
        ([2.0, 0.0], [0.4, 0.0], [0.25, 1.5], 1.0, 0.0),
        ([2.0, 0.0], [-4.0, 0.0], [0.25, 1.5], 1.0, 4.0),
        ([2.0, 0.0], [0.0, 0.4], [0.25, 1.5], 1.0, 0.0),
        ([-2.0, 0.0], [0.4, 0.0], [0.25, 1.5], 1.0, 0.4),
        ([0.0, 2.0], [0.0, -0.4], [0.25, 1.5], 1.0, 0.4),
        ([1.2, 1.6], [-0.24, -0.32], [0.25, 1.5], 1.0, 0.4),
        ([1.2, 1.6], [-0.24, -0.32], [0.5, 0.75], 1.0, 0.4),
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 0.5, 0.4),
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 2.0, 0.4),
        ([2.0, 0.0], [0.0, 0.0], [0.25, 1.5], 1.0, 0.0),
        ([0.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 1.0, 0.0),
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 1.0, 0.0), // Dry ahead.
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 1.0, 0.0), // Patch edge.
    ]
    .into_iter()
    .enumerate()
    {
        let center_x = if case == 13 { n - 1 } else { 96 };
        let center_flow = glam::Vec2::from_array(flow);
        let direction = center_flow.normalize_or_zero();
        let mut seed = vec![[0.0_f32; 4]; 5 * cells];
        for z in 0..n {
            for x in 0..n {
                let offset = glam::vec2(
                    (x as f32 - center_x as f32) * spacing[0],
                    (z as f32 - 96.0) * spacing[1],
                );
                let along = offset.dot(direction);
                let v = center_flow + glam::Vec2::from_array(gradient) * along;
                let h = if case == 12 && x > center_x { 0.0 } else { 2.0 };
                seed[cells + z * n + x] = [h, h * v.x, h * v.y, 0.0];
            }
        }
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, n as f32 * spacing[0], n as f32 * spacing[1]];
        p.terrain[1] = 0.0;
        p.terrain[2] = 0.0;
        p.foam_front = [8.0, distance, 0.0, 0.0];
        p.visual[1] = center_x as f32;
        p.options[0] = 0.0;
        p.options[3] = 0.0;
        p.timing[1] = 0.0001;
        p.timing[2] = 0.0;
        let mut water_by_order = [[0.0_f32; 3]; 2];
        for enabled in [false, true] {
            p.foam_front[0] = if enabled { 8.0 } else { 0.0 };
            p.foam_velocity = [0.0, 0.1, 0.5, 0.0];
            let strength = if enabled { strength } else { 0.0 };
            let t = ((strength - 0.1) / 0.5).clamp(0.0, 1.0);
            let expected = t * t * (3.0 - 2.0 * t);
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            gpu.queue
                .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            gpu.queue.submit([enc.finish()]);
            let bytes = gpu.read_buffer(&shore.field, 16)?;
            let result: [f32; 4] = bytemuck::pod_read_unaligned(&bytes);
            if !result[0].is_finite() || (result[0] - expected).abs() > 0.0001 {
                return Err(format!(
                    "compression front case {case} enabled {enabled}: {} != {expected}",
                    result[0]
                )
                .into());
            }
            // Dry/boundary/zero-flow guards are sampled above. For wet moving
            // cases verify the real source is added once, for both RK variants.
            if case >= 11 {
                continue;
            }
            for order in [1, 2] {
                gpu.queue
                    .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
                p.solver[0] = order as f32;
                for stage in 0..order {
                    p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
                    gpu.queue
                        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                    let mut enc = gpu.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = enc.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&shore.pipeline);
                        pass.set_bind_group(
                            0,
                            if order == 1 {
                                &shore.bind
                            } else if stage == 0 {
                                &shore.predictor_bind
                            } else {
                                &shore.corrector_bind
                            },
                            &[],
                        );
                        pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                    }
                    gpu.queue.submit([enc.finish()]);
                }
                let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
                let i = cells + 96 * n + center_x;
                let q: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[16 * i..16 * (i + 1)]);
                // RK2 samples the predictor flow. At this small dt its source
                // differs from the analytic initial flow by less than 1e-5.
                if q.iter().any(|v| !v.is_finite())
                    || (q[3] - 8.0 * expected * p.timing[1]).abs() > 0.00001
                {
                    return Err(format!(
                        "compression source case {case} enabled {enabled} order {order}: {q:?}"
                    )
                    .into());
                }
                if !enabled {
                    water_by_order[order - 1].copy_from_slice(&q[..3]);
                } else if q[..3] != water_by_order[order - 1] {
                    return Err(format!(
                        "front foam changed water state in case {case}, order {order}"
                    )
                    .into());
                }
            }
        }
    }
    eprintln!(
        "Front foam: signed catch-up/collision vs separation/shear, distance/grid normalization, wet/dry/edge guards, source integration and unchanged water in both orders OK"
    );
    Ok(())
}

/// Exercise production surface flux with sources disabled: mass conservation,
/// translation, convergence, expansion and decay, for both SWE integration modes.
fn verify_surface_foam(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let n = RESOLUTION as usize;
    let cells = n * n;
    let dt = 0.004_f32;
    for (order, space_order) in [(1, 1), (2, 1), (2, 2)] {
        for mode in 0..7 {
            let shore = Shore::new(gpu, &ocean);
            let mut seed = vec![[0.0_f32; 4]; cells * 5];
            for y in 0..n {
                for x in 0..n {
                    let i = y * n + x;
                    let u = match mode {
                        0 | 4 => 1.0,
                        1 => (24.0 - (x as f32 + 0.5) * 0.25) * 0.1,
                        2 => ((x as f32 + 0.5) * 0.25 - 24.0) * 0.1,
                        _ => 0.0,
                    };
                    let density = if (64..128).contains(&x) && (64..128).contains(&y) {
                        0.5
                    } else {
                        0.0
                    };
                    let depth = if mode == 5 {
                        0.002
                    } else if mode == 6 {
                        0.01
                    } else if mode == 4 {
                        0.2
                    } else {
                        2.0
                    };
                    seed[i] = [density, 1.0, depth, u];
                    seed[cells + i] = [depth, depth * u, 0.0, density];
                    seed[2 * cells + i] = [0.0, density, 2.0, 0.0];
                }
            }
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
            p.terrain[1] = 0.0;
            p.domain = [0.0, 0.0, 64.0, 64.0];
            p.foam_motion = [1.0, 0.0, 0.0, 0.0];
            p.foam_inertia[2] = space_order as f32;
            p.options[0] = 0.0;
            p.foam_front[0] = 0.0;
            p.options[3] = 0.0;
            p.timing[1] = dt;
            p.timing[2] = if mode == 3 { 0.7 } else { 0.0 };
            p.solver[0] = order as f32;
            for stage in 0..order {
                p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
                gpu.queue
                    .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&shore.pipeline);
                    pass.set_bind_group(
                        0,
                        if order == 1 {
                            &shore.bind
                        } else if stage == 0 {
                            &shore.predictor_bind
                        } else {
                            &shore.corrector_bind
                        },
                        &[],
                    );
                    pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                }
                gpu.queue.submit([enc.finish()]);
            }
            let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
            let data: Vec<[f32; 4]> = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            let total: f64 = data[cells..2 * cells].iter().map(|q| q[3] as f64).sum();
            let expected = 2048.0
                * if mode == 3 {
                    (-0.7 * dt).exp() as f64
                } else if mode == 5 {
                    0.0
                } else {
                    1.0
                };
            let center = data[cells + 96 * n + 96][3];
            if data.iter().flatten().any(|v| !v.is_finite())
                || data[cells..2 * cells].iter().any(|q| q[3] < 0.0)
                || (total - expected).abs() > 0.002
                || ((mode == 0 || mode == 4)
                    && (data[cells + 96 * n + 128][3] <= 0.0 || (center - 0.5).abs() > 1e-6))
                || (mode == 5
                    && (data[2 * cells + 96 * n + 96][..2] != [0.0, 0.0]
                        || (data[96 * n + 96][1] - (-dt / p.timing[3]).exp()).abs() > 1e-6))
                || (mode == 6 && (center - 0.5).abs() > 1e-6)
                || (mode == 1 && center <= 0.5)
                || (mode == 2 && center >= 0.5)
                || (mode == 3 && (center - 0.5 * (-0.7 * dt).exp()).abs() > 1e-6)
            {
                return Err(format!("surface foam time {order} space {space_order} mode {mode}: total {total}, expected {expected}, center {center}").into());
            }
        }
    }
    eprintln!(
        "Surface foam: Euler/RK2 and both spatial orders conserve area amount, transport, concentrate/dilute and decay independently of depth; dry-cell removal OK"
    );
    Ok(())
}

/// Prescribed water flow isolates production foam flux/motion from water dynamics.
/// Check material displacement against analytic trajectories, not texture phase.
fn verify_foam_motion(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    let ocean = Ocean::new(gpu, crate::ocean::parameters::ocean::OceanConfig::default())?;
    let shore = Shore::new(gpu, &ocean);
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("foam motion trajectory probe"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../shaders/shore.wgsl"),
                    include_str!("../shaders/shore_compute.wgsl"),
                    r#"
@compute @workgroup_size(8,8)
fn motion_probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let n=u32(shore.options.z); if id.x>=n || id.y>=n { return; }
    let cells=n*n; let index=id.y*n+id.x; let c=vec2<i32>(id.xy);
    if shore.foam_motion.w>0.5 {
        new_shore[index]=sample_initial_texture(vec2(shore.foam_motion.z,96.0)); return;
    }
    let original=state(c); var q=original;
    let foam=foam_sources(transport_foam(c),velocity(q),0.0);
    q.w=foam.x;
    new_shore[index]=vec4(q.w,1.0,q.x,velocity(q).x);
    new_shore[cells+index]=q;
    new_shore[2u*cells+index]=initial_shore[2u*cells+index];
    let motion=advance_foam_motion(c,q,original,foam.yz);
    new_shore[3u*cells+index]=motion.motion;
    new_shore[4u*cells+index]=motion.texture;
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
            entry_point: Some("motion_probe"),
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
    let n = RESOLUTION as usize;
    let cells = n * n;
    // Follow, response, water speed, initial foam speed, stretching, dt, steps, reset period.
    for (case, (follow, response, water, initial, stretch, dt, steps, period)) in [
        (
            0.5_f32, 0.0_f32, 1.0_f32, 0.0_f32, false, 0.02_f32, 120, 2.0_f32,
        ),
        (0.0, 0.0, 1.0, 0.0, false, 0.02, 120, 2.0),
        (0.8, 0.35, 1.0, 0.0, false, 0.02, 120, 2.0),
        (0.8, 0.35, -1.0, 0.8, false, 0.02, 120, 2.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 120, 2.0),
        (0.8, 0.35, 1.0, 0.0, false, 0.01, 240, 2.0),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020, 2.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020, 2.0),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020, 8.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020, 8.0),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020, 0.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut seed = vec![[0.0_f32; 4]; 5 * cells];
        for z in 0..n {
            for x in 0..n {
                let i = z * n + x;
                let u = if stretch {
                    (x as f32 - 96.0) * 0.25 * 0.1
                } else {
                    water
                };
                let density = if (64..128).contains(&x) && (64..128).contains(&z) {
                    0.5
                } else {
                    0.0
                };
                seed[i] = [density, 1.0, 2.0, u];
                seed[cells + i] = [2.0, 2.0 * u, 0.0, density];
                seed[2 * cells + i] = [0.0, 0.0, 2.0, 0.0];
                seed[3 * cells + i] = [initial * density, 0.0, 0.0, 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, 64.0, 64.0];
        p.timing[1] = dt;
        p.foam_motion = [follow, response, 0.0, 0.0];
        p.foam_shading[3] = period;
        p.timing[2] = 0.0;
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
        for step in 0..steps {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
            }
            enc.copy_buffer_to_buffer(&shore.field, 0, &shore.previous, 0, shore.field.size());
            gpu.queue.submit([enc.finish()]);
            if case == 3 && step == 0 {
                let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
                let i = 3 * cells + 96 * n + 96;
                let m: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[16 * i..16 * (i + 1)]);
                if m[0] <= 0.0 || m[2] <= 0.0 {
                    return Err("foam reversal lost initial inertia".into());
                }
            }
        }
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        let data: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        let mass: f64 = data[cells..2 * cells].iter().map(|q| q[3] as f64).sum();
        if data.iter().flatten().any(|v| !v.is_finite()) || (mass - 2048.0).abs() > 0.02 {
            return Err(format!("foam motion case {case} mass/finite failed: {mass}").into());
        }
        let time = dt * steps as f32;
        let mid = 96 * n + 96;
        let motion = data[3 * cells + mid];
        let texture = data[4 * cells + mid];
        let age_a = if period == 0.0 { time } else { time % period };
        let age_b = if period == 0.0 || time < period * 0.5 {
            time
        } else {
            (time + period * 0.5) % period
        };
        let expected_phase = if period == 0.0 { 0.0 } else { age_a / period };
        if (texture[2] - expected_phase).abs() > 0.0001 {
            return Err(format!(
                "foam texture phase does not track simulated time: {} vs {time}",
                texture[2]
            )
            .into());
        }
        if stretch {
            let material_span = 2.0 - (data[3 * cells + mid + 4][2] - data[3 * cells + mid - 4][2]);
            let span_b = 2.0 - (data[4 * cells + mid + 4][0] - data[4 * cells + mid - 4][0]);
            if (material_span / 2.0 - (-0.1 * age_a).exp()).abs() > 0.002
                || (span_b / 2.0 - (-0.1 * age_b).exp()).abs() > 0.002
                || (data[cells + mid][3] / 0.5 - (-0.1 * time).exp()).abs() > 0.002
            {
                return Err(format!(
                    "foam stretch: material span {material_span}, density {}",
                    data[cells + mid][3]
                )
                .into());
            }
        } else {
            let target = follow * water;
            let (expected_velocity, expected_distance) = if response == 0.0 {
                (target, target * time)
            } else {
                let attenuation = (-time / response).exp();
                (
                    target + (initial - target) * attenuation,
                    target * time + (initial - target) * response * (1.0 - attenuation),
                )
            };
            let center: f64 = data[cells..2 * cells]
                .iter()
                .enumerate()
                .map(|(i, q)| (i % n) as f64 * 0.25 * q[3] as f64)
                .sum::<f64>()
                / mass;
            let density_distance = center - 95.5 * 0.25;
            let layer_distance = |age: f32| {
                target * age
                    + if response == 0.0 {
                        0.0
                    } else {
                        (initial - target)
                            * response
                            * ((-(time - age) / response).exp() - (-time / response).exp())
                    }
            };
            if (motion[0] / data[cells + mid][3] - expected_velocity).abs() > 0.001
                || (motion[2] - layer_distance(age_a)).abs() > 0.002
                || (texture[0] - layer_distance(age_b)).abs() > 0.002
                || (density_distance - expected_distance as f64).abs() > 0.002
            {
                return Err(format!("foam motion {case}: velocity {}, texture {}, density {density_distance}; expected {expected_velocity}, {expected_distance}",motion[0],motion[2]).into());
            }
        }
    }
    // Open boundaries export density AND momentum, never import foam. The
    // independent reference is uniform density * outward velocity * edge length
    // * dt (divided by cell area here, since we sum cell densities).
    // Closed-domain tests above cannot detect a sign error on a negative edge.
    gpu.queue.write_buffer(
        &shore.terrain_bed,
        0,
        bytemuck::cast_slice(&vec![[0.0_f32, 1.0]; cells]),
    );
    for space_order in [1, 2] {
        for edge in 0..4 {
            for (open, outward) in [(false, true), (true, true), (true, false)] {
                let axis = edge / 2;
                let mut speed = [0.25_f32; 2];
                speed[axis] = if (edge % 2 == 0) == outward {
                    -1.0
                } else {
                    1.0
                };
                let mut seed = vec![[0.0_f32; 4]; 5 * cells];
                for i in 0..cells {
                    seed[cells + i] = [2.0, 0.0, 0.0, 0.5];
                    seed[3 * cells + i] = [0.5 * speed[0], 0.5 * speed[1], 0.0, 0.0];
                }
                let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
                p.domain = [0.0, 0.0, 64.0, 64.0];
                p.model[0] = 1.0;
                p.edges = [0.0; 4];
                p.edges[edge] = f32::from(open);
                p.foam_inertia = [0.0, 1.0, space_order as f32, 0.0];
                p.timing[1] = 0.02;
                p.timing[2] = 0.0;
                gpu.queue
                    .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
                gpu.queue
                    .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                }
                gpu.queue.submit([enc.finish()]);
                let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
                let data: Vec<[f32; 4]> = bytes
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(|b| bytemuck::pod_read_unaligned(b))
                    .collect();
                let lost = if open && outward {
                    0.5 * n as f64 * f64::from(p.timing[1]) / (64.0 / n as f64)
                } else {
                    0.0
                };
                let expected = 0.5 * cells as f64 - lost;
                let amount: f64 = data[cells..2 * cells].iter().map(|q| f64::from(q[3])).sum();
                let momentum: [f64; 2] = std::array::from_fn(|a| {
                    data[3 * cells..4 * cells]
                        .iter()
                        .map(|m| f64::from(m[a]))
                        .sum()
                });
                if data.iter().flatten().any(|v| !v.is_finite())
                    || data[cells..2 * cells].iter().any(|q| q[3] < 0.0)
                    || (amount - expected).abs() > 0.003
                    || (0..2).any(|a| (momentum[a] - expected * f64::from(speed[a])).abs() > 0.003)
                {
                    return Err(format!("foam boundary space {space_order}, edge {edge}, open {open}, outward {outward}: amount {amount} vs {expected}, momentum {momentum:?}").into());
                }
            }
        }
    }
    eprintln!(
        "Foam boundaries: both spatial orders, four edges, closed conservation, outward amount/momentum flux and no inward injection OK"
    );
    // A wet donor retains its coordinates next to dry/outside samples, rather
    // than interpolating toward cleared zero displacement. All-dry is finite.
    let mut seed = vec![[0.0_f32; 4]; 5 * cells];
    let donor = 96 * n + 64;
    seed[cells + donor][0] = 2.0;
    seed[3 * cells + donor] = [0.0, 0.0, 3.0, -2.0];
    seed[4 * cells + donor] = [5.0, -4.0, 0.0, 0.0];
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    for (x, expected) in [(63.75, [3.0, -2.0, 5.0, -4.0]), (62.25, [0.0; 4])] {
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.foam_motion = [0.8, 0.35, x, 1.0];
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        gpu.queue.submit([enc.finish()]);
        let bytes = gpu.read_buffer(&shore.field, 16)?;
        let actual: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[..16]);
        if actual != expected {
            return Err(
                format!("foam wet/dry texture interpolation: {actual:?} vs {expected:?}").into(),
            );
        }
    }
    eprintln!(
        "Foam motion: follow/stop/reversal, density transport independent of two staggered texture resets, 20 s bounded stretch, wet/dry interpolation and timestep consistency OK"
    );
    Ok(())
}

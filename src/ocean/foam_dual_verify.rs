//! GPU contracts: exact two-population kinetics and real SWE stage/lifecycle wiring.
use super::super::gpu::Error;
use super::{Gpu, Ocean, OpticsConfig, Shore, ShoreUniform};

// Independent f64 RK4 reference of the stated ODE, not a copy of the shader formula.
fn reference(mut nr: [f64; 2], source: f64, time: f64, rates: [f64; 3]) -> [f64; 2] {
    let [a, b, fraction] = rates;
    let rhs = |[n, r]: [f64; 2]| [source - a * n, fraction * a * n - b * r];
    let dt = time / 8192.0;
    for _ in 0..8192 {
        let k1 = rhs(nr);
        let k2 = rhs(std::array::from_fn(|i| nr[i] + dt * k1[i] * 0.5));
        let k3 = rhs(std::array::from_fn(|i| nr[i] + dt * k2[i] * 0.5));
        let k4 = rhs(std::array::from_fn(|i| nr[i] + dt * k3[i]));
        for i in 0..2 {
            nr[i] += dt / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]);
        }
    }
    nr
}

fn verify_kinetics(gpu: &Gpu) -> Result<(), Error> {
    // Fresh/residual half-lives, transfer fraction, source, initial N/R, duration.
    let cases: [[f32; 7]; 9] = [
        [1.4, 9.0, 0.65, 0.0, 1.0, 0.0, 1.4],
        [1.4, 9.0, 0.65, 3.0, 0.0, 0.0, 1.4],
        [2.0, 2.0, 0.65, 2.0, 1.0, 0.7, 5.0],
        [2.0, 2.00001, 1.0, 2.0, 1.0, 0.7, 5.0],
        [9.0, 1.4, 0.65, 0.0, 2.0, 1.0, 9.0],
        [0.1, 120.0, 1.0, 3.0, 5.0, 0.0, 8.0],
        [60.0, 0.1, 0.0, 3.0, 0.0, 2.0, 8.0],
        [1.4, 9.0, 0.65, 3.0, 1.0, 2.0, 0.0],
        [1.4, 9.0, 0.65, 0.0, 0.0, 1.0, 9.0],
    ];
    let fixtures = cases
        .iter()
        .map(|c| {
            format!(
                "Case(vec3({:?},{:?},{:?}),vec2({:?},{:?}),{:?},{:?})",
                std::f32::consts::LN_2 / c[0],
                std::f32::consts::LN_2 / c[1],
                c[2],
                c[4],
                c[5],
                c[3],
                c[6]
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    let shader =
        gpu.device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("two-population analytic kinetics"),
                source: wgpu::ShaderSource::Wgsl(
                    format!(
            "{}\n{}\nstruct Case {{ rates:vec3<f32>, nr:vec2<f32>, source:f32, time:f32 }}
@compute @workgroup_size(1) fn kinetics(@builtin(global_invocation_id) id:vec3<u32>) {{
    let fixtures=array({fixtures}); let c=fixtures[id.x/3u];
    let steps=array(1u,10u,100u)[id.x%3u];
    var nr=c.nr;
    for(var i=0u;i<steps;i++) {{ nr=dual_foam_sources(nr,c.source,c.time/f32(steps),c.rates); }}
    new_shore[id.x]=vec4(nr,0.0,0.0);
}}", include_str!("shaders/shore.wgsl"), include_str!("shaders/shore_compute.wgsl")
        )
                    .into(),
                ),
            });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("dual foam kinetics"),
            layout: None,
            module: &shader,
            entry_point: Some("kinetics"),
            compilation_options: Default::default(),
            cache: None,
        });
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dual kinetics readback"),
        size: cases.len() as u64 * 3 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 2,
            resource: output.as_entire_binding(),
        }],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(cases.len() as u32 * 3, 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.read_buffer(&output, output.size())?;
    let actual: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
    for (index, c) in cases.iter().enumerate() {
        let rates = [
            std::f64::consts::LN_2 / f64::from(c[0]),
            std::f64::consts::LN_2 / f64::from(c[1]),
            f64::from(c[2]),
        ];
        let expected = reference(
            [f64::from(c[4]), f64::from(c[5])],
            f64::from(c[3]),
            f64::from(c[6]),
            rates,
        );
        for subdivision in 0..3 {
            for i in 0..2 {
                let value = f64::from(actual[index * 3 + subdivision][i]);
                if !value.is_finite()
                    || value < 0.0
                    || (value - expected[i]).abs() > 3e-5 * expected[i].max(1.0)
                {
                    return Err(format!(
                        "dual kinetics case {index}, split {subdivision}: {value} vs {expected:?}"
                    )
                    .into());
                }
            }
        }
    }
    println!(
        "Dual foam kinetics: N(1.4s)={:.6}, R={:.6}; f64 ODE, equal rates, 0/100% conversion, continuous birth, timestep splits OK",
        actual[0][0], actual[0][1]
    );
    Ok(())
}

fn dispatch(
    gpu: &Gpu,
    shore: &Shore,
    p: &mut ShoreUniform,
    order: u32,
) -> Result<Vec<[f32; 4]>, Error> {
    p.solver[0] = order as f32;
    for stage in 0..order {
        p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(p));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
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
            pass.dispatch_workgroups(shore.resolution / 8, shore.resolution / 8, 1);
        }
        gpu.queue.submit([encoder.finish()]);
    }
    let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
    Ok(bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|b| bytemuck::pod_read_unaligned(b))
        .collect())
}

pub(super) fn verify(gpu: &Gpu) -> Result<(), Error> {
    verify_kinetics(gpu)?;
    let ocean = Ocean::new(
        gpu,
        super::super::spectrum::OceanConfig {
            resolution: 8,
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let mut shore = Shore::with_resolution(gpu, &ocean, 64);
    let n = 64_usize;
    let cells = n * n;
    let mid = 32 * n + 32;
    let mut optics = OpticsConfig::beach_scene();
    optics.shore.foam_dual_phase = true;
    optics.shore.foam_emission_enabled = false;
    let mut p = shore.parameters(&optics, 0.0, false, 0.0);
    p.domain = [0.0, 0.0, 16.0, 32.0];
    p.terrain[1] = 0.0;
    p.options[3] = 0.0;
    p.timing[1] = 0.005;
    // Deliberately incompatible legacy settings must NOT stop/decay the two species.
    p.foam_motion = [0.0, 20.0, 0.0, 0.0];
    p.foam_inertia[0] = 0.0;
    p.timing[2] = 4.0;
    let rates = p.foam_lifetime[1..].try_into().unwrap();
    let rates = <[f32; 3]>::map(rates, f64::from);
    for order in [1, 2] {
        for slope in [-0.2_f32, 0.0, 0.2] {
            let mut seed = vec![[0.0_f32; 4]; cells * 5];
            for z in 0..n {
                for x in 0..n {
                    let i = z * n + x;
                    let dx = x as f32 - 32.0;
                    let dz = z as f32 - 32.0;
                    let u = 0.7 + slope * dx * 0.25;
                    let v = -0.3 + slope * dz * 0.5;
                    let fresh = 2.0 + 0.02 * dx + 0.01 * dz;
                    seed[i] = [fresh * 1.5, 0.0, 2.0, u];
                    seed[cells + i] = [2.0, 2.0 * u, 2.0 * v, fresh * 1.5];
                    seed[2 * cells + i] = [0.0, 0.0, 2.0, v];
                    seed[4 * cells + i][3] = fresh * 0.5;
                }
            }
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            let result = dispatch(gpu, &shore, &mut p, order)?;
            let dt = f64::from(p.timing[1]);
            // Linear density is exactly bilinear-sampled. Flow is affine; RK2
            // water evolution/drag adds only O(dt²) to this frozen-flow reference.
            let transported = (2.0 - dt * (0.02 * 0.7 / 0.25 - 0.01 * 0.3 / 0.5))
                * (-2.0 * f64::from(slope) * dt).exp();
            let expected = reference([transported, transported * 0.5], 0.0, dt, rates);
            let residual = result[4 * cells + mid][3];
            let actual = [result[mid][0] - residual, residual];
            if result.iter().flatten().any(|v| !v.is_finite())
                || actual
                    .iter()
                    .zip(expected)
                    .any(|(&a, b)| (f64::from(a) - b).abs() > 5e-5)
            {
                return Err(format!(
                    "dual transport order {order}, slope {slope}: {actual:?} vs {expected:?}"
                )
                .into());
            }
            let offset = &result[3 * cells + mid][2..];
            if (offset[0] - 0.7 * p.timing[1]).abs() > 1e-5
                || (offset[1] + 0.3 * p.timing[1]).abs() > 1e-5
            {
                return Err("dual material coordinates did not follow the shared SWE flow".into());
            }
            // Local mode must not feed either density into water dynamics.
            p.foam_lifetime[0] = 0.0;
            let legacy = dispatch(gpu, &shore, &mut p, order)?;
            p.foam_lifetime[0] = 1.0;
            if legacy[cells..2 * cells]
                .iter()
                .zip(&result[cells..2 * cells])
                .any(|(a, b)| a[..3] != b[..3])
            {
                return Err("dual foam changed water dynamics".into());
            }
            p.timing[1] = 0.0;
            let paused = dispatch(gpu, &shore, &mut p, order)?;
            if paused != seed {
                return Err("paused dual foam changed history".into());
            }
            p.timing[1] = 0.005;
        }
        // Saturated affine compression emits 8/s into N, never directly into R.
        let mut seed = vec![[0.0_f32; 4]; cells * 5];
        for z in 0..n {
            for x in 0..n {
                let i = z * n + x;
                let u = 2.0 - 0.8 * (x as f32 - 32.0) * 0.25;
                seed[i] = [0.0, 0.0, 2.0, u];
                seed[cells + i] = [2.0, 2.0 * u, 0.0, 0.0];
                seed[2 * cells + i] = [0.0, 0.0, 2.0, 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        p.foam_front[3] = 0.0;
        p.foam_ridge[3] = 1.0;
        let born = dispatch(gpu, &shore, &mut p, order)?;
        let expected = reference([0.0, 0.0], 8.0, f64::from(p.timing[1]), rates);
        let r = born[4 * cells + mid][3];
        if (f64::from(born[mid][0] - r) - expected[0]).abs() > 2e-6
            || (f64::from(r) - expected[1]).abs() > 2e-7
        {
            return Err(
                format!("dual emission repeated or entered R directly, order {order}").into(),
            );
        }
        p.foam_ridge[3] = 0.0;
        // Dry history is discarded, including R; neither species seeds dry rock.
        for i in 0..cells {
            seed[i] = [3.0, 0.8, 0.0, 0.0];
            seed[cells + i] = [0.0, 0.0, 0.0, 3.0];
            seed[4 * cells + i][3] = 2.0;
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let dry = dispatch(gpu, &shore, &mut p, order)?;
        if dry[mid][0] != 0.0 || dry[4 * cells + mid][3] != 0.0 || dry[mid][1] <= 0.0 {
            return Err("dual drying retained foam or cleared wet-sand history".into());
        }
        p.options[3] = 1.0;
        let reset = dispatch(gpu, &shore, &mut p, order)?;
        p.options[3] = 0.0;
        if reset[..cells].iter().any(|q| q[0] != 0.0)
            || reset[4 * cells..].iter().any(|q| q[3] != 0.0)
        {
            return Err("dual reset retained a species".into());
        }
    }
    // Emergent wall: stale density on a dry solid must not leak to the lee side.
    // Lowering the same wall below water is a positive transport control.
    p.collider_meta[0] = 1.0;
    p.colliders[0].footprint = [8.125, 16.0, 0.12, 32.0];
    p.colliders[0].solid = [2.0, 4.0, 1.0, 0.0];
    for order in [1, 2] {
        for overtopped in [false, true] {
            p.colliders[0].solid[1] = if overtopped { 0.5 } else { 4.0 };
            let mut seed = vec![[0.0_f32; 4]; cells * 5];
            for z in 0..n {
                for x in 0..n {
                    let i = z * n + x;
                    let h = if x == 32 {
                        if overtopped { 1.5 } else { 0.0 }
                    } else {
                        2.0
                    };
                    let amount = if x < 32 || (x == 32 && !overtopped) {
                        3.0
                    } else {
                        0.0
                    };
                    seed[i] = [amount, 0.8, h, 2.0];
                    seed[cells + i] = [h, 2.0 * h, 0.0, amount];
                    seed[4 * cells + i][3] = amount / 3.0;
                }
            }
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            let result = dispatch(gpu, &shore, &mut p, order)?;
            if overtopped {
                if result[mid][0] <= 0.01 || result[4 * cells + mid][3] <= 0.003 {
                    return Err("overtopped collider blocked dual foam transport".into());
                }
            } else if result[mid][0] != 0.0
                || result[mid + 1][0] != 0.0
                || result[4 * cells + mid][3] != 0.0
                || result[4 * cells + mid + 1][3] != 0.0
            {
                return Err("dual foam leaked through dry collider or retained R on rock".into());
            }
        }
    }
    // Live collider edits must clear R even while simulation time is paused.
    let old = p;
    p.colliders[0].solid[1] = 4.0;
    let mut edit = gpu.device.create_command_encoder(&Default::default());
    edit.copy_buffer_to_buffer(&shore.field, 0, &shore.previous, 0, shore.field.size());
    shore.encode_collider_edit(gpu, &mut edit, p, old);
    gpu.queue.submit([edit.finish()]);
    let edited = gpu.read_buffer(&shore.field, shore.field.size())?;
    let edited: &[[f32; 4]] = bytemuck::cast_slice(&edited);
    if edited[mid][0] != 0.0 || edited[4 * cells + mid][3] != 0.0 || edited[mid][1] <= 0.0 {
        return Err("live collider edit retained R or erased wetness".into());
    }
    // Changing mode resets at the real host boundary; lifetime edits preserve history.
    let before = shore.parameters(&optics, 1.0, true, 0.0);
    shore.last = Some(before);
    optics.shore.foam_fresh_half_life = 2.0;
    if shore.parameters(&optics, 1.0, false, 0.0).options[3] != 0.0 {
        return Err("lifetime edit reset history".into());
    }
    optics.shore.foam_dual_phase = false;
    if shore.parameters(&optics, 1.0, false, 0.0).options[3] != 1.0 {
        return Err("dual mode switch did not reset history".into());
    }
    println!(
        "Dual foam SWE: Euler/RK2 shared transport, compression/dilation, N-only birth, no legacy drag/decay, water isolation, dry/overtopped colliders, paused edits, pause/dry/reset/live settings OK"
    );
    Ok(())
}

//! Actual SWE dispatches: a compression ridge is a line source, not an area source.
use super::{Gpu, Ocean, OpticsConfig, Shore, ShoreUniform};
use crate::ocean::Error;

fn step(
    gpu: &Gpu,
    shore: &Shore,
    p: &mut ShoreUniform,
    order: u32,
) -> Result<Vec<[f32; 4]>, Error> {
    p.solver[0] = order as f32;
    p.foam_inertia[2] = order as f32;
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
    let ocean = Ocean::new(
        gpu,
        crate::ocean::parameters::ocean::OceanConfig {
            resolution: 8,
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let shore = Shore::with_resolution(gpu, &ocean, 64);
    let n = shore.resolution as usize;
    let cells = n * n;
    let center = n / 2;
    let index = center * n + center;
    let spacing = glam::vec2(0.5, 0.75);
    let mut optics = OpticsConfig::beach_scene();
    optics.shore.foam_ridge_enabled = true;
    optics.shore.foam_drag_enabled = false;
    optics.shore.decay = 0.0;
    let mut p = shore.parameters(&optics, 0.0, false, 0.0);
    p.domain = [0.0, 0.0, n as f32 * spacing.x, n as f32 * spacing.y];
    p.terrain[1] = 0.0;
    p.options[3] = 0.0;
    p.timing[1] = 0.0001;
    // Independent analytic fields: tanh velocity has one sech² compression
    // maximum; affine compression has NO maximum despite exceeding threshold.
    // Different world-space directions on a rectangular grid expose axis bias.
    for (case, angle) in [
        0.0_f32,
        std::f32::consts::FRAC_PI_2,
        std::f32::consts::FRAC_PI_4,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0,
    ]
    .into_iter()
    .enumerate()
    {
        let direction = glam::vec2(angle.cos(), angle.sin());
        let mut seed = vec![[0.0_f32; 4]; cells * 5];
        for z in 0..n {
            for x in 0..n {
                let along = (glam::vec2(x as f32 - center as f32, z as f32 - center as f32)
                    * spacing)
                    .dot(direction);
                let speed = match case {
                    3 => 2.0 - 0.4 * along,  // Uniform compression, not a ridge.
                    4 => 2.0 + along.tanh(), // Expansion.
                    5 => 2.0,                // Translation.
                    _ => 2.0 - along.tanh(),
                };
                let h = if (case == 6 && x == center + 1) || case == 7 {
                    0.0
                } else {
                    2.0
                };
                let i = z * n + x;
                seed[i] = [0.0, 0.0, h, speed * direction.x];
                seed[cells + i] = [h, h * speed * direction.x, h * speed * direction.y, 0.0];
                seed[2 * cells + i] = [0.0, 0.0, h, speed * direction.y];
            }
        }
        for order in [1, 2] {
            let mut control = Vec::new();
            for enabled in [false, true] {
                p.foam_ridge[3] = f32::from(enabled);
                gpu.queue
                    .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
                let result = step(gpu, &shore, &mut p, order)?;
                let q = result[cells + index];
                let source = result[2 * cells + index][0];
                if result.iter().flatten().any(|v| !v.is_finite())
                    || result[cells..2 * cells]
                        .iter()
                        .any(|q| q[0] < 0.0 || q[3] < 0.0)
                {
                    return Err(format!(
                        "ridge case {case} order {order}: nonfinite/negative state"
                    )
                    .into());
                }
                let expected = if enabled && case < 3 {
                    8.0 * p.timing[1]
                } else {
                    0.0
                };
                if (q[3] - expected).abs() > 1e-6 || (source * p.timing[1] - q[3]).abs() > 1e-6 {
                    return Err(format!("ridge case {case} order {order} enabled {enabled}: amount {}, expected {expected}, source {source}",q[3]).into());
                }
                if !enabled {
                    control = result;
                    continue;
                }
                if result[cells..2 * cells]
                    .iter()
                    .zip(&control[cells..2 * cells])
                    .any(|(a, b)| a[..3] != b[..3])
                {
                    return Err("ridge emission changed SWE water state".into());
                }
                if case < 3 {
                    let tangent = match case {
                        0 => (0, 4),
                        1 => (4, 0),
                        _ => (-3, 2),
                    };
                    let tangent_index = (center as i32 + tangent.1) as usize * n
                        + (center as i32 + tangent.0) as usize;
                    if result[2 * cells + tangent_index][0] < 7.5 {
                        return Err(
                            "ridge source lost continuity along the wavefront tangent".into()
                        );
                    }
                    for sign in [-1.0, 1.0] {
                        let offset = (direction * sign * 4.0 / spacing).round();
                        let far = (center as i32 + offset.y as i32) as usize * n
                            + (center as i32 + offset.x as i32) as usize;
                        if result[2 * cells + far][0] > 0.001 {
                            return Err(
                                "ridge source leaked into the trailing/leading region".into()
                            );
                        }
                    }
                }
            }
        }
        if case == 3 {
            // Positive control: this compressive affine field does activate the
            // old broad source. Zero ridge output is not a dormant fixture.
            p.foam_ridge[0] = 0.0;
            p.options[1] = 7.0;
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            let broad = step(gpu, &shore, &mut p, 1)?;
            if broad[2 * cells + index][0] < 1.0 {
                return Err("affine compression positive control did not emit".into());
            }
            p.foam_ridge[0] = 1.0;
            p.options[1] = 0.0;
        }
    }
    // Seed an existing compact foam patch with >1 density and forward momentum.
    // A stopped source must conserve it (no decay), transport it, and not clamp
    // it. Water is uniform here so a moving source cannot mask a failed stop.
    for order in [1, 2] {
        let mut seed = vec![[0.0_f32; 4]; cells * 5];
        for z in 0..n {
            for x in 0..n {
                let i = z * n + x;
                let amount = if (28..36).contains(&x) && (28..36).contains(&z) {
                    2.0
                } else {
                    0.0
                };
                seed[i] = [amount, 0.0, 2.0, 2.0];
                seed[cells + i] = [2.0, 4.0, 0.0, amount];
                seed[2 * cells + i] = [0.0, 0.0, 2.0, 0.0];
                seed[3 * cells + i] = [amount * 2.0, 0.0, 0.0, 0.0];
            }
        }
        p.foam_ridge[3] = 0.0;
        p.timing[1] = 0.002;
        let moments = |data: &[[f32; 4]]| -> (f64, f64) {
            data[cells..2 * cells]
                .iter()
                .enumerate()
                .fold((0.0, 0.0), |(mass, x), (i, q)| {
                    (
                        mass + f64::from(q[3]),
                        x + f64::from(q[3]) * (i % n) as f64 * f64::from(spacing.x),
                    )
                })
        };
        let before = moments(&seed);
        let mut result = seed;
        for _ in 0..100 {
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&result));
            result = step(gpu, &shore, &mut p, order)?;
        }
        let after = moments(&result);
        if (after.0 / before.0 - 1.0).abs() > 0.0002
            || after.1 / after.0 - before.1 / before.0 < 0.3
            || result[2 * cells..3 * cells].iter().any(|q| q[0] != 0.0)
            || result[cells..2 * cells]
                .iter()
                .map(|q| q[3])
                .fold(0.0_f32, f32::max)
                < 1.5
        {
            return Err(format!(
                "stopped ridge source order {order}: mass/centroid {before:?} -> {after:?}"
            )
            .into());
        }
    }
    eprintln!(
        "Foam ridge: narrow line vs broad compression, rotated rectangular grid, expansion/dry guards, once-per-step source, unchanged water, stopped-source conservation/advection (>1 density), both orders OK"
    );
    Ok(())
}

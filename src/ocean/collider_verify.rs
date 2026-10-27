//! Controlled GPU checks of static obstacles, using the production SWE stages.
use super::{Gpu, Ocean, OpticsConfig, Shore, ShoreUniform};
use crate::ocean::{
    collider::{Collider, ColliderUniform, Shape},
    gpu::Error,
    spectrum::OceanConfig,
};
use wgpu::util::DeviceExt;
const N: usize = 64;
const CELLS: usize = N * N;
type History = Vec<[f32; 4]>;

fn bed(x: f64, z: f64, collider: Option<ColliderUniform>) -> f64 {
    let mut height = -0.02 * (x + 50.0);
    if let Some(c) = collider {
        let dx = x - c.footprint[0] as f64;
        let dz = z - c.footprint[1] as f64;
        let lx = c.solid[2] as f64 * dx + c.solid[3] as f64 * dz;
        let lz = -c.solid[3] as f64 * dx + c.solid[2] as f64 * dz;
        let inside = if c.solid[0] < 1.5 {
            dx.hypot(dz) <= c.footprint[2] as f64
        } else {
            lx.abs() <= c.footprint[2] as f64 && lz.abs() <= c.footprint[3] as f64
        };
        if inside {
            height = height.max(c.solid[1] as f64);
        }
    }
    height
}

fn simulate(
    gpu: &Gpu,
    shore: &Shore,
    mut p: ShoreUniform,
    collider: Option<ColliderUniform>,
    pulse: bool,
    steps: u32,
) -> Result<(History, History), Error> {
    p.domain = [0.0, 0.0, 16.0, 16.0];
    p.options[2] = N as f32;
    p.terrain = [1.0, 0.02, -50.0, 1.0];
    p.timing[1] = 0.002;
    p.timing[2] = 0.0;
    p.options[0] = 0.0;
    p.options[3] = 0.0;
    p.foam_front[0] = 0.0;
    p.foam_inertia[0] = 0.0;
    p.collider_meta[0] = f32::from(collider.is_some());
    if let Some(c) = collider {
        p.colliders[0] = c;
    }
    let mut seed = vec![[0.0; 4]; CELLS * 5];
    for i in 0..CELLS {
        let x = (i % N) as f64 * 0.25 + 0.125;
        let z = (i / N) as f64 * 0.25 + 0.125;
        let bottom = bed(x, z, collider);
        let elevation = if pulse {
            0.12 * (-((x - 4.0) / 0.5).powi(2)).exp()
        } else {
            0.0
        };
        let h = (elevation - bottom).max(0.0) as f32;
        let hu = if h > 0.0 {
            ((9.81 * (-bottom).max(0.0)).sqrt() * elevation) as f32
        } else {
            0.0
        };
        let foam = if pulse && (6.0..7.0).contains(&x) && h > 0.0 {
            0.4
        } else {
            0.0
        };
        seed[i] = [foam, 0.0, h, hu / h.max(0.005)];
        seed[CELLS + i] = [h, hu, 0.0, foam];
        seed[2 * CELLS + i] = [0.0, 0.0, h + bottom as f32, 0.0];
        seed[3 * CELLS + i] = [foam, 0.0, 0.0, 0.0];
    }
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    let stage = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("collider RK stage"),
        size: shore.field.size(),
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let order = p.solver[0] as u32;
    let mut bindings = Vec::new();
    for s in 0..order {
        p.solver[1] = if order == 2 { (s + 1) as f32 } else { 0.0 };
        let uniform = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("collider stage parameters"),
                contents: bytemuck::bytes_of(&p),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let input = if s == 0 { &shore.previous } else { &stage };
        let output = if order == 2 && s == 0 {
            &stage
        } else {
            &shore.field
        };
        bindings.push(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("collider production solver test"),
            layout: &shore.pipeline.get_bind_group_layout(0),
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
        }));
    }
    for start in (0..steps).step_by(50) {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        for _ in start..(start + 50).min(steps) {
            for bind in &bindings {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&shore.pipeline);
                pass.set_bind_group(0, bind, &[]);
                pass.dispatch_workgroups(N as u32 / 8, N as u32 / 8, 1);
            }
            encoder.copy_buffer_to_buffer(&shore.field, 0, &shore.previous, 0, (CELLS * 80) as u64);
        }
        gpu.queue.submit([encoder.finish()]);
    }
    let bytes = gpu.read_buffer(&shore.field, (CELLS * 80) as u64)?;
    let data: Vec<[f32; 4]> = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|v| bytemuck::pod_read_unaligned(v))
        .collect();
    if data.iter().flatten().any(|v| !v.is_finite())
        || data[CELLS..2 * CELLS]
            .iter()
            .any(|q| q[0] < 0.0 || q[3] < 0.0)
    {
        return Err("collider simulation produced nonfinite/negative state".into());
    }
    Ok((seed, data))
}

pub(super) fn verify(gpu: &Gpu) -> Result<(), Error> {
    let ocean = Ocean::new(
        gpu,
        OceanConfig {
            amplitude: 0.0,
            resolution: 8,
            ..Default::default()
        },
    )?;
    let shore = Shore::new(gpu, &ocean);
    let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
    for (order, space_order) in [(1, 1), (2, 1), (2, 2)] {
        p.solver[0] = order as f32;
        p.foam_inertia[2] = space_order as f32;
        for shape in [Shape::Cylinder, Shape::Box] {
            for top in [2.0, -0.5] {
                let mut c = Collider {
                    shape,
                    position: [8.0, 8.0],
                    angle: 35.0,
                    size: [3.0, 6.0],
                    ..Default::default()
                }
                .uniform(0.02, -50.0);
                c.solid[1] = top;
                let (seed, data) = simulate(gpu, &shore, p, Some(c), false, 200)?;
                let error = seed[CELLS..2 * CELLS]
                    .iter()
                    .zip(&data[CELLS..2 * CELLS])
                    .flat_map(|(a, b)| (0..3).map(move |j| (a[j] - b[j]).abs()))
                    .fold(0.0_f32, f32::max);
                if error > 0.0002 {
                    return Err(format!(
                        "collider lake at rest: order {order}, {shape:?}, top {top}, error {error}"
                    )
                    .into());
                }
            }
        }
        let mut wall = Collider {
            shape: Shape::Box,
            position: [8.0, 8.0],
            size: [1.0, 40.0],
            ..Default::default()
        }
        .uniform(0.02, -50.0);
        wall.solid[1] = 2.0;
        let (seed, blocked) = simulate(gpu, &shore, p, Some(wall), true, 900)?;
        let mut reflected = 0.0;
        let mut leak = 0.0_f32;
        for i in 0..CELLS {
            let x = (i % N) as f32 * 0.25 + 0.125;
            if x < 7.5 {
                reflected += blocked[CELLS + i][1] as f64;
            }
            if x > 8.5 {
                leak = leak.max((blocked[CELLS + i][0] - seed[CELLS + i][0]).abs());
            }
            if (7.5..8.5).contains(&x)
                && (blocked[CELLS + i][0] > 1e-6
                    || blocked[CELLS + i][3] > 1e-6
                    || blocked[2 * CELLS + i][0] > 1e-6)
            {
                return Err("water/foam leaked into emergent collider".into());
            }
        }
        let total = |data: &[[f32; 4]], channel: usize| {
            data[CELLS..2 * CELLS]
                .iter()
                .map(|q| q[channel] as f64)
                .sum::<f64>()
        };
        let water_error = (total(&blocked, 0) / total(&seed, 0) - 1.0).abs();
        let foam_error = (total(&blocked, 3) / total(&seed, 3) - 1.0).abs();
        if reflected >= -0.1 || leak > 0.0002 || water_error > 0.0001 || foam_error > 0.0001 {
            return Err(format!("collider wall time {order} space {space_order}: reflected={reflected}, leak={leak}, water={water_error}, foam={foam_error}").into());
        }
        let circle = Collider {
            position: [8.0, 8.0],
            ..Default::default()
        }
        .uniform(0.02, -50.0);
        let (_, around) = simulate(gpu, &shore, p, Some(circle), true, 800)?;
        let positive = around[CELLS..2 * CELLS]
            .iter()
            .map(|q| q[2])
            .fold(0.0_f32, f32::max);
        let negative = around[CELLS..2 * CELLS]
            .iter()
            .map(|q| q[2])
            .fold(0.0_f32, f32::min);
        if positive < 0.005 || negative > -0.005 {
            return Err(format!("no lateral flow around cylinder: {positive}/{negative}").into());
        }
        wall.solid[1] = 0.02;
        let (_, over) = simulate(gpu, &shore, p, Some(wall), true, 600)?;
        let overtopped = over[CELLS..2 * CELLS]
            .iter()
            .enumerate()
            .filter(|(i, _)| (30..34).contains(&(i % N)))
            .map(|(_, q)| q[0])
            .fold(0.0_f32, f32::max);
        if overtopped < 0.005 {
            return Err(format!("wave did not overtop low obstacle: {overtopped}").into());
        }
        println!(
            "Colliders time {order} space {space_order}: rotated box/cylinder static balance, wall reflection {reflected:.3}, no water/foam leak, conserved water {water_error:.2e}/foam {foam_error:.2e}, lateral flow {negative:.3}..{positive:.3}, overtopping depth {overtopped:.3} m OK"
        );
    }
    verify_live_edits(gpu, &ocean)?;
    Ok(())
}

fn edit_bed(p: ShoreUniform, x: f32, z: f32) -> f32 {
    let mut height = -p.terrain[1] * (x - p.terrain[2]);
    for c in &p.colliders[..p.collider_meta[0] as usize] {
        let dx = x - c.footprint[0];
        let dz = z - c.footprint[1];
        let inside = if c.solid[0] < 1.5 {
            dx.hypot(dz) <= c.footprint[2]
        } else {
            (c.solid[2] * dx + c.solid[3] * dz).abs() <= c.footprint[2]
                && (-c.solid[3] * dx + c.solid[2] * dz).abs() <= c.footprint[3]
        };
        if inside {
            height = height.max(c.solid[1]);
        }
    }
    height
}

fn verify_live_edits(gpu: &Gpu, ocean: &Ocean) -> Result<(), Error> {
    let cells = super::RESOLUTION.pow(2) as usize;
    for order in [1, 2] {
        let mut shore = Shore::new(gpu, ocean);
        let mut settings = OpticsConfig::beach_scene();
        settings.shore.order = order;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        shore.encode(gpu, &mut encoder, &settings, 3.0, false, 0.0);
        gpu.queue.submit([encoder.finish()]);
        let mut p = shore.last.unwrap();
        let mut expected = vec![[0.0; 4]; cells * 5];
        for i in 0..cells {
            let x = p.domain[0]
                + (i % super::RESOLUTION as usize) as f32 * p.domain[2] / super::RESOLUTION as f32
                + 0.5 * p.domain[2] / super::RESOLUTION as f32;
            let bottom = -settings.beach_slope * (x - settings.shore_x);
            let h = (0.08 * (x * 0.4).sin() - bottom).max(0.0);
            let foam = if h > p.density[2] { 0.7 } else { 0.0 };
            let u = if h > 0.005 { 0.3 } else { 0.0 };
            expected[i] = [foam, 0.6, h, u];
            expected[cells + i] = [h, h * u, -h * u, foam];
            expected[2 * cells + i] = [0.0, 0.0, h + bottom, -u];
            expected[3 * cells + i] = [foam * 0.4, -foam * 0.2, 0.9, 0.8];
            expected[4 * cells + i] = [0.7, 0.6, 0.3, 0.0];
        }
        for buffer in [&shore.previous, &shore.field] {
            gpu.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(&expected));
        }
        for change in 0..16 {
            match change {
                0 => {
                    settings.shore.colliders[0] = Some(Collider {
                        position: [settings.shore_x + 8.0, 0.0],
                        radius: 4.0,
                        ..Default::default()
                    })
                }
                1 => settings.shore.colliders[0].as_mut().unwrap().position[0] += 2.0,
                2 => settings.shore.colliders[0].as_mut().unwrap().radius = 6.0,
                3 => settings.shore.colliders[0].as_mut().unwrap().shape = Shape::Box,
                4 => settings.shore.colliders[0].as_mut().unwrap().size = [8.0, 10.0],
                5 => settings.shore.colliders[0].as_mut().unwrap().angle = 35.0,
                6 => settings.shore.colliders[0].as_mut().unwrap().height = 0.1,
                7 => settings.shore.colliders[0].as_mut().unwrap().height = 3.0,
                8 => settings.shore.colliders[0].as_mut().unwrap().enabled = false,
                9 => settings.shore.colliders[0].as_mut().unwrap().enabled = true,
                10 => {
                    settings.shore.colliders[1] = Some(Collider {
                        position: [settings.shore_x + 10.0, 0.0],
                        radius: 10.0,
                        height: 5.0,
                        ..Default::default()
                    })
                }
                11 => settings.shore.colliders[0].as_mut().unwrap().height = 2.0,
                12 => settings.shore.colliders[1] = None,
                13 => settings.shore.colliders[0].as_mut().unwrap().position[0] = 1000.0,
                14 => settings.shore.colliders[0] = None,
                15 => {
                    settings.shore.colliders[2] = Some(Collider {
                        position: [settings.shore_x + 16.0, 12.0],
                        radius: 4.0,
                        height: 0.1,
                        ..Default::default()
                    })
                }
                _ => unreachable!(),
            }
            let next = shore.parameters(&settings, 3.0, false, 0.0);
            let before = expected.clone();
            let mut affected = 0;
            let mut changed_bed = vec![false; cells];
            for i in 0..cells {
                let x = next.domain[0]
                    + ((i % super::RESOLUTION as usize) as f32 + 0.5) * next.domain[2]
                        / super::RESOLUTION as f32;
                let z = next.domain[1]
                    + ((i / super::RESOLUTION as usize) as f32 + 0.5) * next.domain[3]
                        / super::RESOLUTION as f32;
                let old_bed = edit_bed(p, x, z);
                let bottom = edit_bed(next, x, z);
                if old_bed == bottom {
                    continue;
                }
                affected += 1;
                changed_bed[i] = true;
                let old = before[cells + i];
                let h = old[0].min((old_bed + old[0] - bottom).max(0.0));
                let h = if h < 0.00001 { 0.0 } else { h };
                let ratio = if old[0] > 0.0 { h / old[0] } else { 0.0 };
                let foam = if h > next.density[2] { old[3] } else { 0.0 };
                let q = [h, old[1] * ratio, old[2] * ratio, foam];
                expected[cells + i] = q;
                expected[i] = [foam, before[i][1], h, q[1] / h.max(0.005)];
                expected[2 * cells + i][2] = h + bottom;
                expected[2 * cells + i][3] = q[2] / h.max(0.005);
                if h <= next.density[2] {
                    expected[3 * cells + i] = [0.0; 4];
                    expected[4 * cells + i][..2].fill(0.0);
                }
            }
            if [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 15].contains(&change) && affected == 0 {
                return Err(format!("collider edit {change} has no changed bed cells").into());
            }
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            shore.encode(gpu, &mut encoder, &settings, 3.0, false, 0.0);
            gpu.queue.submit([encoder.finish()]);
            let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
            let actual: History = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            for (i, (a, b)) in actual.iter().zip(&expected).enumerate() {
                if a.iter()
                    .zip(b)
                    .any(|(a, b)| !a.is_finite() || (a - b).abs() > 0.00002)
                    || (!changed_bed[i % cells] && a != &before[i])
                {
                    return Err(format!("live collider edit reset/changed history: order {order}, edit {change}, block/cell {i}: {a:?}, expected {b:?}").into());
                }
            }
            if shore.start_time != 3.0 || shore.simulated_dt != 0.0 {
                return Err("paused collider edit restarted/advanced the simulation".into());
            }
            expected = actual;
            p = shore.last.unwrap();
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            shore.encode(gpu, &mut encoder, &settings, 3.0, false, 0.0);
            gpu.queue.submit([encoder.finish()]);
            if gpu.read_buffer(&shore.field, shore.field.size())? != bytes {
                return Err("unchanged paused colliders modified history".into());
            }
        }
        // Edit in an advancing frame as well: the update must precede Euler/RK2.
        settings.shore.colliders[2].as_mut().unwrap().position[0] += 1.0;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        shore.encode(gpu, &mut encoder, &settings, 3.01, false, 0.0);
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        if bytes
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| !f32::from_ne_bytes(*b).is_finite())
            || shore.simulated_dt <= 0.0
            || shore.start_time != 3.0
        {
            return Err("SWE did not resume after live collider edits".into());
        }
    }
    println!(
        "Live collider edits: both orders, paused add/move/resize/rotate/height/enable/delete/overlap, local water clipping, unchanged foam/sand/texture history and resume OK"
    );
    Ok(())
}

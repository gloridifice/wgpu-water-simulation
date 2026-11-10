//! Owner-boundary GPU checks: analytic sources, rest and conservation, not a second solver.
//! Snapshot is also the production export path; no test-only injection interfaces.
use crate::{
    Error,
    interactions::{Event, EventQueue, InteractionSettings, PlayerEmitter},
    parameters::FogSettings,
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::Gpu;
fn check(ok: bool, label: &str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(format!("fog verification failed: {label}").into())
    }
}
fn submit(
    gpu: &Gpu,
    sim: &mut Simulation,
    p: &FogSettings,
    dt: f32,
    reset: bool,
    events: &[Event],
) -> Result<(), Error> {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    sim.encode(gpu, &mut encoder, p, dt, reset, events)?;
    gpu.queue.submit([encoder.finish()]);
    Ok(())
}
fn advance(gpu: &Gpu, sim: &mut Simulation, p: &FogSettings, count: u32) -> Result<(), Error> {
    for i in 0..count {
        submit(gpu, sim, p, 0.1, false, &[])?;
        if i % 100 == 99 {
            sim.snapshot(gpu)?;
        }
    }
    Ok(())
}
fn diff(a: &[[f32; 4]], b: &[[f32; 4]]) -> f32 {
    a.iter()
        .flatten()
        .zip(b.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max)
}
fn sum(a: &[[f32; 4]], axis: usize) -> f64 {
    a.iter().map(|a| f64::from(a[axis])).sum()
}
pub async fn run() -> Result<(), Error> {
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    eprintln!("Fog GPU verification: {}", gpu.name);
    let terrain = Terrain::bake(
        &BakeSource::playground(),
        [-8.0, -6.0, 16.0, 12.0],
        [16, 12],
    )?;
    let mut sim = Simulation::new(&gpu, &terrain)?;
    let mut p = FogSettings {
        noise_amplitude: 0.0,
        height_offset: 0.0,
        wind: [0.0; 2],
        decay_enabled: false,
        closed_boundary: true,
        ..Default::default()
    };
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    let (_, initial) = sim.snapshot(&gpu)?;
    check(
        (initial[0][0] - 0.25).abs() < 1e-6,
        "actual initial GPU state",
    )?;
    advance(&gpu, &mut sim, &p, 600)?;
    let (clock, still) = sim.snapshot(&gpu)?;
    check(
        diff(&initial, &still) < 1e-5
            && clock.fault == 0
            && (clock.simulated_time - 60.0).abs() < 0.002,
        "60 s flat lake and GPU clock",
    )?;
    p.height_offset = -0.6;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    let (_, empty) = sim.snapshot(&gpu)?;
    check(
        sum(&empty, 0) == 0.0 && sum(&empty, 1) == 0.0,
        "negative offset clips h and m",
    )?;
    p.height_offset = -0.1;
    p.noise_amplitude = 0.7;
    p.noise_frequency = 0.5;
    p.gravity_enabled = false;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    let (_, noisy) = sim.snapshot(&gpu)?;
    check(
        noisy.iter().any(|v| v[0] == 0.0) && noisy.iter().any(|v| v[0] > 0.0),
        "signed noise holes",
    )?;
    advance(&gpu, &mut sim, &p, 20)?;
    check(
        diff(&noisy, &sim.snapshot(&gpu)?.1) < 1e-6,
        "gravity off is stationary",
    )?;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    check(
        diff(&noisy, &sim.snapshot(&gpu)?.1) == 0.0,
        "same seed replays",
    )?;
    p.gravity_enabled = true;
    advance(&gpu, &mut sim, &p, 100)?;
    let (clock, evolved) = sim.snapshot(&gpu)?;
    check(
        diff(&noisy, &evolved) > 0.001 && clock.fault == 0,
        "gravity on actually evolves",
    )?;
    p.noise_amplitude = 0.15;
    p.base_height = 0.6;
    p.height_offset = 0.0;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    let (_, initial) = sim.snapshot(&gpu)?;
    advance(&gpu, &mut sim, &p, 600)?;
    let (clock, evolved) = sim.snapshot(&gpu)?;
    for axis in 0..2 {
        check(
            (sum(&evolved, axis) - sum(&initial, axis)).abs() / sum(&initial, axis) < 0.001,
            "closed h/m conservation over 60 s",
        )?;
    }
    check(
        clock.fault == 0
            && evolved.iter().flatten().all(|v| v.is_finite())
            && evolved.iter().all(|v| v[0] >= 0.0 && v[1] >= 0.0),
        "positive finite accepted state",
    )?;
    let mut slope_source = BakeSource::playground();
    for tri in &mut slope_source.floor {
        for v in tri {
            v[1] = 0.01 * (v[0] + 32.0) + 0.005 * (v[2] + 32.0);
        }
    }
    let slope = Terrain::bake(&slope_source, [-8.0, -6.0, 16.0, 12.0], [16, 12])?;
    let mut slope_sim = Simulation::new(&gpu, &slope)?;
    p.base_height = 1.0;
    p.noise_amplitude = 0.0;
    p.uniform_top = true;
    submit(&gpu, &mut slope_sim, &p, 0.0, true, &[])?;
    let (_, rest) = slope_sim.snapshot(&gpu)?;
    advance(&gpu, &mut slope_sim, &p, 600)?;
    let (clock, after) = slope_sim.snapshot(&gpu)?;
    check(
        diff(&rest, &after) < 0.001 && clock.fault == 0,
        "non-flat lake-at-rest hydrostatic balance",
    )?;
    p.uniform_top = false;
    p.base_height = 0.6;
    p.gravity_enabled = false;
    p.wind = [0.05, 0.0];
    p.wind_response = 2.0;
    p.decay_enabled = true;
    p.half_life = 1.0;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    advance(&gpu, &mut sim, &p, 10)?;
    let (clock, source) = sim.snapshot(&gpu)?;
    let i = 6 * 16 + 8;
    check(
        (source[i][1] / source[i][0] - 0.5).abs() < 0.001
            && (source[i][2] - 0.05 * (1.0 - (-0.5f32).exp())).abs() < 0.0001
            && clock.fault == 0,
        "analytic wind/half-life",
    )?;
    p.base_height = 0.25;
    p.wind = [0.0; 2];
    p.wind_response = 600.0;
    p.decay_enabled = false;
    let settings = InteractionSettings::default();
    let mut event = Event {
        position_rotation: [0.5, 0.2, 0.5, 0.0],
        footprint: [4.0, 4.0, 0.1, 0.6],
        metadata: [0, 0, 1.0f32.to_bits(), 0],
    };
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    let (_, base) = sim.snapshot(&gpu)?;
    submit(&gpu, &mut sim, &p, 0.0, false, &[event])?;
    let (_, kick) = sim.snapshot(&gpu)?;
    let front = 5 * 16 + 8;
    check(
        (kick[front][3] + 0.3).abs() < 0.005 && kick[front][2].abs() < 0.005,
        "RG16F forward impulse and -Z basis",
    )?;
    check(
        base.iter()
            .zip(&kick)
            .all(|(a, b)| a[0] == b[0] && a[1] == b[1]),
        "direct impulse changes momentum only",
    )?;
    submit(&gpu, &mut sim, &p, 0.0, false, &[])?;
    check(
        diff(&kick, &sim.snapshot(&gpu)?.1) == 0.0,
        "empty batch never replays impulse",
    )?;
    event.position_rotation[3] = std::f32::consts::FRAC_PI_2;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    submit(&gpu, &mut sim, &p, 0.0, false, &[event])?;
    let (_, rotated) = sim.snapshot(&gpu)?;
    check(
        (rotated[6 * 16 + 7][2] + 0.3).abs() < 0.005,
        "positive Y rotation",
    )?;
    event.position_rotation = [0.5, 0.25, 0.5, 0.0];
    event.footprint[3] = 0.25;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    submit(&gpu, &mut sim, &p, 0.0, false, &[event])?;
    let (_, partial) = sim.snapshot(&gpu)?;
    check(
        (partial[front][3] + 0.15).abs() < 0.005,
        "vertical overlap fraction",
    )?;
    event.position_rotation[1] = 3.0;
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    submit(&gpu, &mut sim, &p, 0.0, false, &[event])?;
    check(
        diff(&base, &sim.snapshot(&gpu)?.1) == 0.0,
        "no vertical overlap",
    )?;
    let radial = Event::radial(glam::Vec3::new(0.5, 0.0, 0.5), 0.7, 2.0, &settings);
    submit(&gpu, &mut sim, &p, 0.0, true, &[])?;
    submit(&gpu, &mut sim, &p, 0.0, false, &[radial])?;
    let (_, rad) = sim.snapshot(&gpu)?;
    check(
        (rad[6 * 16 + 9][2] - 0.7).abs() < 0.01,
        "radial jump/landing texture",
    )?;
    eprintln!(
        "Fog GPU checks: PASS (rest, slope, gravity, conservation, wind, decay, noise, impulses, rotation, overlap, batch lifetime)"
    );
    verify_continuous(&gpu, &mut sim)?;
    verify_wake(&gpu)?;
    verify_grid_and_jump(&gpu)?;
    verify_wall(&gpu)?;
    verify_render(&gpu, &mut sim, &terrain)?;
    crate::render::raster_verification::run(&gpu)?;
    eprintln!("Fog legacy marching reference checks (not the interactive raster path):");
    crate::render::verification::run(&gpu)?;
    verify_sampling(&gpu)?;
    crate::render::benchmark::run(&gpu)?;
    Ok(())
}
fn verify_grid_and_jump(gpu: &Gpu) -> Result<(), Error> {
    let source = BakeSource::playground();
    let native = Terrain::decode(include_bytes!("../assets/playground.fogterrain"), &source)?;
    let p = FogSettings {
        noise_amplitude: 0.0,
        height_offset: 0.0,
        wind: [0.0; 2],
        decay_enabled: false,
        closed_boundary: true,
        ..Default::default()
    };
    let settings = InteractionSettings::default();
    for resolution in [[128, 128], [256, 256], [64, 64]] {
        let terrain = native.at_resolution(&source, Some(resolution))?;
        let mut sim = Simulation::new(gpu, &terrain)?;
        submit(gpu, &mut sim, &p, 0.0, true, &[])?;
        let (_, initial) = sim.snapshot(gpu)?;
        let jump = Event::radial(
            glam::Vec3::ZERO,
            settings.jump_strength,
            settings.jump_radius,
            &settings,
        );
        submit(gpu, &mut sim, &p, 0.0, false, &[jump])?;
        let (_, kicked) = sim.snapshot(gpu)?;
        let max_speed =
            |field: &[[f32; 4]]| field.iter().map(|q| q[2].hypot(q[3])).fold(0.0, f32::max);
        let jump_speed = max_speed(&kicked);
        check(
            kicked.iter().zip(&initial).all(|(a, b)| a[..2] == b[..2]),
            "takeoff impulse cannot change h/m",
        )?;
        for _ in 0..3 {
            submit(gpu, &mut sim, &p, 1.0 / 30.0, false, &[])?;
        }
        let (clock, _) = sim.snapshot(gpu)?;
        check(
            clock.fault == 0 && clock.simulated_time > 0.09 && sim.resolution == resolution,
            "resized grid accepts strong takeoff impulse and stable steps",
        )?;
        if resolution == [256, 256] {
            submit(gpu, &mut sim, &p, 0.0, true, &[])?;
            // Even at the maximum existing landing impact multiplier, landing
            // should have a lower local peak than the compact takeoff kick.
            let landing = Event::radial(
                glam::Vec3::ZERO,
                settings.landing_strength * 2.0,
                settings.landing_radius,
                &settings,
            );
            submit(gpu, &mut sim, &p, 0.0, false, &[landing])?;
            check(
                jump_speed > 2.0 * max_speed(&sim.snapshot(gpu)?.1),
                "takeoff peak exceeds the strongest scaled landing",
            )?;
        }
        let renderer = crate::render::Renderer::new(
            gpu,
            &sim,
            &terrain,
            &source,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        )?;
        let session = crate::parameters::Session {
            fog: p.clone(),
            ..Default::default()
        };
        let image = renderer.read_image(gpu, &session, sim.domain, [64, 48])?;
        check(
            image.len() == 64 * 48 * 4
                && image.as_chunks::<4>().0.iter().any(|p| p[..3] != [0, 0, 0]),
            "resized field and terrain bind into the production renderer",
        )?;
    }
    eprintln!(
        "Fog grid/jump checks: PASS (128/256/64 rebuild, production rendering, compact strong takeoff, weaker landing, stable GPU steps)"
    );
    Ok(())
}
fn verify_wake(gpu: &Gpu) -> Result<(), Error> {
    let source = BakeSource {
        floor: vec![
            [[-6.0, 0.0, -6.0], [-6.0, 0.0, 6.0], [6.0, 0.0, 6.0]],
            [[-6.0, 0.0, -6.0], [6.0, 0.0, 6.0], [6.0, 0.0, -6.0]],
        ],
        obstacle: vec![],
    };
    let terrain = Terrain::bake(&source, [-6.0, -6.0, 12.0, 12.0], [48, 48])?;
    let mut sim = Simulation::new(gpu, &terrain)?;
    let mut p = FogSettings {
        base_height: 0.25,
        height_offset: 0.0,
        noise_amplitude: 0.0,
        gravity_enabled: false,
        wind: [0.0; 2],
        wind_response: 600.0,
        decay_enabled: false,
        closed_boundary: true,
        ..Default::default()
    };
    let settings = InteractionSettings {
        wake_radius: 1.5,
        wake_separation: 4.0,
        wake_offset: 0.0,
        ..Default::default()
    };
    let mut queue = EventQueue::default();
    PlayerEmitter::default().movement(
        &settings,
        glam::Vec3::ZERO,
        glam::Vec2::new(0.0, -3.0),
        3.0,
        1.0 / 60.0,
        &mut queue,
    );
    let events = queue.take();
    // A spatially uniform disk source isolates the independent exponential
    // solution, including zero target speed (which must still exert drag).
    let mut body = *events
        .iter()
        .find(|e| e.metadata[0] == 2)
        .ok_or("missing body source")?;
    body.footprint[..2].fill(100_000_000.0);
    let i = 24 * 48 + 24;
    let rate = f64::from(body.footprint[2]) + 1.0 / 600.0;
    let expected = -1.8 * f64::from(body.footprint[2]) / rate * (1.0 - (-rate * 0.1).exp());
    for parts in [1, 3] {
        submit(gpu, &mut sim, &p, 0.0, true, &[])?;
        for _ in 0..parts {
            submit(gpu, &mut sim, &p, 0.1 / parts as f32, false, &[body])?;
        }
        let (clock, field) = sim.snapshot(gpu)?;
        check(
            clock.fault == 0
                && (clock.simulated_time - 0.1).abs() < 1e-5
                && (f64::from(field[i][3]) - expected).abs() < 0.0001,
            "analytic body drag follows exponential response across request partitions",
        )?;
    }
    body.metadata[2] = 0.0f32.to_bits();
    submit(gpu, &mut sim, &p, 0.1, false, &[body])?;
    check(
        (f64::from(sim.snapshot(gpu)?.1[i][3]) - expected * (-rate * 0.1).exp()).abs() < 0.0001,
        "zero target retains spatial drag weight",
    )?;
    let vortices: Vec<_> = events.into_iter().filter(|e| e.metadata[0] >= 3).collect();
    let circulation = |field: &[[f32; 4]], centre_x: f32| -> f64 {
        field
            .iter()
            .enumerate()
            .filter_map(|(i, q)| {
                let x = -6.0 + (i % 48) as f32 * 0.25 + 0.125 - centre_x;
                let z = -6.0 + (i / 48) as f32 * 0.25 + 0.125;
                (x * x + z * z < 2.25).then_some(f64::from(q[0] * (x * q[3] - z * q[2])))
            })
            .sum()
    };
    for order in [1, 2] {
        p.spatial_order = order;
        submit(gpu, &mut sim, &p, 0.0, true, &[])?;
        let (_, initial) = sim.snapshot(gpu)?;
        submit(gpu, &mut sim, &p, 0.0, false, &vortices)?;
        check(
            diff(&initial, &sim.snapshot(gpu)?.1) == 0.0,
            "vortex sources have no zero-dt kick",
        )?;
        for _ in 0..4 {
            submit(gpu, &mut sim, &p, 1.0 / 60.0, false, &vortices)?;
        }
        let (clock, field) = sim.snapshot(gpu)?;
        let left = circulation(&field, -2.0);
        let right = circulation(&field, 2.0);
        check(
            clock.fault == 0 && left < -0.1 && right > 0.1 && (left + right).abs() < 0.001,
            "paired wake sources generate opposite circulation",
        )?;
        for axis in 0..2 {
            check(
                (sum(&field, axis) - sum(&initial, axis)).abs() / sum(&initial, axis) < 0.00001,
                "vortex forcing preserves closed h/m mass",
            )?;
        }
        submit(gpu, &mut sim, &p, 0.1, false, &[])?;
        let (_, released) = sim.snapshot(gpu)?;
        check(
            circulation(&released, -2.0) < 0.6 * left && circulation(&released, 2.0) > 0.6 * right,
            "removing sources retains fluid rotation",
        )?;
        let mut high = vortices.clone();
        for e in &mut high {
            e.position_rotation[1] = 10.0;
        }
        submit(gpu, &mut sim, &p, 0.0, true, &[])?;
        submit(gpu, &mut sim, &p, 0.1, false, &high)?;
        check(
            diff(&initial, &sim.snapshot(gpu)?.1) < 1e-6,
            "vortices above the layer do not stir ground fog",
        )?;
    }
    p.base_height = 0.0;
    submit(gpu, &mut sim, &p, 0.0, true, &[])?;
    submit(gpu, &mut sim, &p, 0.1, false, &vortices)?;
    let (clock, dry) = sim.snapshot(gpu)?;
    check(
        clock.fault == 0 && dry.iter().flatten().all(|v| *v == 0.0),
        "wake sources cannot create fog in dry cells",
    )?;
    eprintln!(
        "Fog wake sources: PASS (analytic drag, zero target, opposite circulation, both orders, h/m conservation, release, overlap, dry cells)"
    );
    Ok(())
}

fn verify_continuous(gpu: &Gpu, sim: &mut Simulation) -> Result<(), Error> {
    let p = FogSettings {
        base_height: 0.25,
        height_offset: 0.0,
        noise_amplitude: 0.0,
        gravity_enabled: false,
        wind: [0.05, 0.0],
        wind_response: 2.0,
        decay_enabled: false,
        closed_boundary: true,
        ..Default::default()
    };
    // Huge footprint makes the center an effectively uniform source, so the
    // independent local exponential solution is not polluted by advection.
    let event = Event {
        position_rotation: [0.0, 0.2, 25_000_000.0, 0.0],
        footprint: [100_000_000.0, 100_000_000.0, 2.0, 0.6],
        metadata: [0, 1, 1.0f32.to_bits(), 0],
    };
    let i = 6 * 16 + 8;
    for parts in [10, 20] {
        submit(gpu, sim, &p, 0.0, true, &[])?;
        let (_, initial) = sim.snapshot(gpu)?;
        submit(gpu, sim, &p, 0.0, false, &[event])?;
        check(
            diff(&initial, &sim.snapshot(gpu)?.1) == 0.0,
            "continuous source has no zero-dt kick",
        )?;
        for _ in 0..parts {
            submit(gpu, sim, &p, 1.0 / parts as f32, false, &[event])?;
        }
        let (clock, values) = sim.snapshot(gpu)?;
        let response = 1.0 - (-2.5f64).exp();
        check(
            (f64::from(values[i][2]) - 0.01 * response).abs() < 0.01
                && (f64::from(values[i][3]) + 2.4 * response).abs() < 0.01
                && clock.fault == 0
                && (clock.simulated_time - 1.0).abs() < 1e-4,
            "wind plus continuous drag follows independent exponential solution",
        )?;
    }
    let mut other = event;
    other.position_rotation = [25_000_000.0, 0.2, 0.0, std::f32::consts::FRAC_PI_2];
    other.footprint[2] = 1.0;
    submit(gpu, sim, &p, 0.0, true, &[])?;
    submit(gpu, sim, &p, 0.1, false, &[event, other])?;
    let (_, first) = sim.snapshot(gpu)?;
    submit(gpu, sim, &p, 0.0, true, &[])?;
    submit(gpu, sim, &p, 0.1, false, &[other, event])?;
    let (_, second) = sim.snapshot(gpu)?;
    check(
        diff(&first, &second) < 1e-5,
        "continuous source ordering does not change the local drag solution",
    )?;
    eprintln!(
        "Fog continuous sources: PASS (wind combination, 10/20 time slices, zero dt, source ordering)"
    );
    Ok(())
}
fn verify_wall(gpu: &Gpu) -> Result<(), Error> {
    let mut source = BakeSource::playground();
    source.obstacle = vec![
        [[0.0, 0.0, -6.0], [0.0, 2.0, -6.0], [0.0, 2.0, 6.0]],
        [[0.0, 0.0, -6.0], [0.0, 2.0, 6.0], [0.0, 0.0, 6.0]],
    ];
    let t = Terrain::bake(&source, [-8.0, -6.0, 16.0, 12.0], [16, 12])?;
    let mut sim = Simulation::new(gpu, &t)?;
    for order in [1, 2] {
        let p = FogSettings {
            base_height: 0.25,
            height_offset: 0.0,
            noise_amplitude: 0.0,
            wind: [0.0; 2],
            decay_enabled: false,
            closed_boundary: true,
            spatial_order: order,
            ..Default::default()
        };
        submit(gpu, &mut sim, &p, 0.0, true, &[])?;
        let (_, initial) = sim.snapshot(gpu)?;
        let kick = Event::radial(
            glam::Vec3::new(-2.5, 0.0, 0.0),
            0.7,
            2.0,
            &InteractionSettings::default(),
        );
        submit(gpu, &mut sim, &p, 0.0, false, &[kick])?;
        advance(gpu, &mut sim, &p, 100)?;
        let (clock, after) = sim.snapshot(gpu)?;
        check(clock.fault == 0, "thin wall run has no GPU fault")?;
        for axis in 0..2 {
            check(
                (sum(&after, axis) - sum(&initial, axis)).abs() / sum(&initial, axis) < 0.001,
                "thin wall closed h/m conservation",
            )?;
        }
        for z in 0..12 {
            for x in 8..16 {
                let i = z * 16 + x;
                check(
                    after[i]
                        .iter()
                        .zip(initial[i])
                        .all(|(a, b)| (*a - b).abs() < 1e-5),
                    "one-shot pulse cannot cross a baked thin wall",
                )?;
            }
        }
    }
    eprintln!(
        "Fog thin-wall transport: PASS (both spatial orders, one-sided pulse, closed h/m conservation)"
    );
    Ok(())
}
fn verify_render(gpu: &Gpu, sim: &mut Simulation, terrain: &Terrain) -> Result<(), Error> {
    use crate::{parameters::Session, render::Renderer};
    let renderer = Renderer::new(
        gpu,
        sim,
        terrain,
        &BakeSource::playground(),
        wgpu::TextureFormat::Rgba8Unorm,
    )?;
    let size = [320u32, 180u32];
    let mut session = Session::default();
    session.fog.noise_amplitude = 0.0;
    session.fog.height_offset = 0.0;
    session.fog.wind = [0.0; 2];
    session.fog.decay_enabled = false;
    for height in [0.01, 0.25, 0.6] {
        for reconstruction in [
            crate::parameters::Reconstruction::Bilinear,
            crate::parameters::Reconstruction::PreSmoothed,
        ] {
            session.reconstruction = reconstruction;
            session.fog.base_height = height;
            submit(gpu, sim, &session.fog, 0.0, true, &[])?;
            session.visible = false;
            let bare = renderer.read_image(gpu, &session, sim.domain, size)?;
            session.visible = true;
            let fog = renderer.read_image(gpu, &session, sim.domain, size)?;
            let actor = ((90 * size[0] + 160) * 4) as usize;
            check(
                fog[actor..actor + 3] == bare[actor..actor + 3],
                "opaque capsule above the layer clips fog behind it",
            )?;
            for (x, y) in [(64u32, 150u32), (250, 150), (64, 170), (250, 170)] {
                // Independent pinhole geometry and full-column Beer-Lambert result.
                let pitch = f64::from(session.camera[1]);
                let eye_y = 0.9 + 5.5 * pitch.sin();
                let tan = (55.0f64.to_radians() * 0.5).tan();
                let sx = (2.0 * (f64::from(x) + 0.5) / f64::from(size[0]) - 1.0)
                    * tan
                    * f64::from(size[0])
                    / f64::from(size[1]);
                let sy = (1.0 - 2.0 * (f64::from(y) + 0.5) / f64::from(size[1])) * tan;
                let ray_y = (-pitch.sin() + sy * pitch.cos()) / (1.0 + sx * sx + sy * sy).sqrt();
                check(
                    eye_y > f64::from(height) && ray_y < 0.0,
                    "analytic ray enters complete thin layer",
                )?;
                let trans =
                    (-f64::from(session.fog.extinction) * f64::from(height) / ray_y.abs()).exp();
                let i = ((y * size[0] + x) * 4) as usize;
                for c in 0..3 {
                    let expected = f64::from(bare[i + c]) / 255.0 * trans
                        + f64::from(session.fog.brightness) * (1.0 - trans);
                    check(
                        (f64::from(fog[i + c]) / 255.0 - expected).abs() < 0.012,
                        "actual renderer matches analytic layer optical depth",
                    )?;
                }
            }
        }
    }
    session.fog.base_height = 0.25;
    submit(gpu, sim, &session.fog, 0.0, true, &[])?;
    session.player.feet[1] = -0.9;
    session.camera = [0.0, 0.05, 1.5];
    session.debug_view = 5;
    let inside = renderer.read_image(gpu, &session, sim.domain, size)?;
    let pitch = 0.05f64;
    let eye_y = 1.5 * pitch.sin();
    let tan = (55.0f64.to_radians() * 0.5).tan();
    let sx = (2.0 * 64.5 / 320.0 - 1.0) * tan * 320.0 / 180.0;
    let sy = (1.0 - 2.0 * 10.5 / 180.0) * tan;
    let dy = (-pitch.sin() + sy * pitch.cos()) / (1.0 + sx * sx + sy * sy).sqrt();
    let z = eye_y / 0.25;
    // Tetrahedral density is linear from 2m/h at the bed to zero at the top.
    let integral = 0.25 * (1.0 - z).powi(2);
    let expected = 1.0 - (-4.0 * integral / dy).exp();
    check(
        (f64::from(inside[((10 * size[0] + 64) * 4) as usize]) / 255.0 - expected).abs() < 0.01,
        "camera inside the fog integrates the partial upward column",
    )?;
    // The upward interval crosses the 8 m view-depth split. Low extinction
    // must retain the far half; high extinction may reject it only after near
    // opacity is saturated. This also exercises near-eye face fallback.
    session.fog.base_height = 4.0;
    session.camera = [0.0, 0.05, 5.5];
    submit(gpu, sim, &session.fog, 0.0, true, &[])?;
    for extinction in [0.05, 4.0] {
        session.fog.extinction = extinction;
        let image = renderer.read_image(gpu, &session, sim.domain, size)?;
        let integral = 4.0 * (1.0 - 5.5 * pitch.sin() / 4.0).powi(2);
        let expected = 1.0 - (-f64::from(extinction) * integral / dy).exp();
        let actual = f64::from(image[((10 * size[0] + 64) * 4) as usize]) / 255.0;
        check(
            (actual - expected).abs() < 0.01,
            &format!(
                "near/far tetrahedron opacity extinction={extinction}: {actual} vs {expected}"
            ),
        )?;
    }
    session.player = crate::scene::Player::default();
    session.camera = [0.0, 0.3, 5.5];
    session.debug_view = 0;
    session.fog = FogSettings {
        wind: [0.0; 2],
        decay_enabled: false,
        ..Default::default()
    };
    submit(gpu, sim, &session.fog, 0.0, true, &[])?;
    let picture = renderer.read_image(gpu, &session, sim.domain, size)?;
    for _ in 0..16 {
        check(
            renderer.read_image(gpu, &session, sim.domain, size)? == picture,
            "paused thin fog has no temporal jitter",
        )?;
    }
    session.camera[0] = 0.7;
    renderer.read_image(gpu, &session, sim.domain, size)?;
    session.camera[0] = 0.0;
    check(
        renderer.read_image(gpu, &session, sim.domain, size)? == picture,
        "orbit return leaves no history trail",
    )?;
    eprintln!(
        "Fog projected tetrahedra: PASS (1 cm analytic optical depth, opaque clipping, inside-layer camera, near/far intervals at low/high extinction, pause/orbit stability)"
    );
    Ok(())
}
fn verify_sampling(gpu: &Gpu) -> Result<(), Error> {
    use crate::{parameters::Session, render::Renderer};
    let source = BakeSource::playground();
    let terrain = Terrain::decode(include_bytes!("../assets/playground.fogterrain"), &source)?;
    let mut sim = Simulation::new(gpu, &terrain)?;
    let renderer = Renderer::new(
        gpu,
        &sim,
        &terrain,
        &source,
        wgpu::TextureFormat::Rgba8Unorm,
    )?;
    let mut s = Session::default();
    s.fog.noise_amplitude = 0.64;
    s.fog.noise_frequency = 1.05;
    s.fog.noise_octaves = 2;
    s.fog.height_offset = -0.23;
    s.fog.gravity_enabled = false;
    s.fog.wind = [0.0; 2];
    s.fog.decay_enabled = false;
    s.camera[0] = 0.45;
    // Inspect actual integrated opacity, not color differences: the ground's
    // constant-pixel-width lines deliberately change world width with resolution.
    s.debug_view = 5;
    submit(gpu, &mut sim, &s.fog, 0.0, true, &[])?;
    let (_, before) = sim.snapshot(gpu)?;
    let mut linear_images = Vec::new();
    for (quality, detail) in [false, true].into_iter().enumerate() {
        s.reconstruction_detail = detail;
        for reconstruction in [
            crate::parameters::Reconstruction::Bilinear,
            crate::parameters::Reconstruction::PreSmoothed,
            crate::parameters::Reconstruction::CubicBSpline,
            crate::parameters::Reconstruction::CatmullRom,
            crate::parameters::Reconstruction::SmoothStep,
        ] {
            if !detail && reconstruction == crate::parameters::Reconstruction::SmoothStep {
                continue;
            }
            s.reconstruction = reconstruction;
            for (view, pitch) in [0.05, 0.3, 0.9].into_iter().enumerate() {
                s.camera[1] = pitch;
                s.visible = true;
                let low = renderer.read_image(gpu, &s, sim.domain, [320, 200])?;
                if reconstruction == crate::parameters::Reconstruction::Bilinear {
                    linear_images.push(low.clone());
                } else {
                    check(
                        low.iter()
                            .zip(&linear_images[quality * 3 + view])
                            .any(|(a, b)| a.abs_diff(*b) > 1),
                        "reconstruction selection reaches the real fragment renderer",
                    )?;
                }
                let high = renderer.read_image(gpu, &s, sim.domain, [960, 600])?;
                if view == 1 {
                    for steps in [8, 32] {
                        s.integration_steps = steps;
                        check(
                            renderer.read_image(gpu, &s, sim.domain, [320, 200])? == low,
                            "legacy integration setting cannot change analytic tetrahedron rendering",
                        )?;
                    }
                    s.integration_steps = 2;
                    check(
                        renderer.read_image(gpu, &s, sim.domain, [320, 200])? == low,
                        "restoring integration quality restores the paused image",
                    )?;
                }
                if reconstruction == crate::parameters::Reconstruction::PreSmoothed {
                    s.reconstruction_smoothing = 0.0;
                    check(
                        renderer.read_image(gpu, &s, sim.domain, [320, 200])?
                            == linear_images[quality * 3 + view],
                        "zero prefilter strength bypasses to original bilinear",
                    )?;
                    s.reconstruction_smoothing = 1.0;
                    check(
                        renderer.read_image(gpu, &s, sim.domain, [320, 200])? != low,
                        "paused strength change invalidates render filter cache",
                    )?;
                    s.reconstruction_smoothing = 0.6;
                    check(
                        renderer.read_image(gpu, &s, sim.domain, [320, 200])? == low,
                        "restoring strength rebuilds the same paused image",
                    )?;
                }
                let mut error = 0.0;
                let mut count = 0;
                for y in 100..200 {
                    for x in 0..320 {
                        if (110..210).contains(&x) {
                            continue;
                        }
                        let i = (y * 320 + x) * 4;
                        for c in 0..3 {
                            let mut reference = 0.0;
                            for yy in 0..3 {
                                for xx in 0..3 {
                                    let j = ((y * 3 + yy) * 960 + x * 3 + xx) * 4 + c;
                                    reference += f64::from(high[j]);
                                }
                            }
                            reference /= 9.0;
                            error += (f64::from(low[i + c]) - reference).abs() / 255.0;
                            count += 1;
                        }
                    }
                }
                let mae = error / f64::from(count);
                eprintln!(
                    "Fog sampling {reconstruction:?} detail={detail} pitch={pitch:.2}: opacity MAE versus 3x supersampling={mae:.6}"
                );
                check(
                    mae < 0.01,
                    "thin fog spatial sampling agrees with supersampled reference",
                )?;
            }
        }
    }
    s.reconstruction = crate::parameters::Reconstruction::Bilinear;
    s.reconstruction_detail = false;
    s.camera[1] = 0.3;
    check(
        renderer.read_image(gpu, &s, sim.domain, [320, 200])? == linear_images[1],
        "restoring the basic topology and kernel restores the paused image",
    )?;
    check(
        diff(&before, &sim.snapshot(gpu)?.1) == 0.0,
        "render reconstruction switches preserve the complete simulation field",
    )?;
    Ok(())
}

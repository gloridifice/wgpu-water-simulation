//! Owner-boundary GPU checks: analytic sources, rest and conservation, not a second solver.
//! Snapshot is also the production export path; no test-only injection interfaces.
use crate::{
    Error,
    interactions::{Event, InteractionSettings},
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
    verify_wall(&gpu)?;
    verify_render(&gpu, &mut sim, &terrain)?;
    verify_sampling(&gpu)?;
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
            let sx =
                (2.0 * (f64::from(x) + 0.5) / f64::from(size[0]) - 1.0) * tan * f64::from(size[0])
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
    let integral = 0.25 * (1.0 - z * z * (3.0 - 2.0 * z));
    let expected = 1.0 - (-4.0 * integral / dy).exp();
    check(
        (f64::from(inside[((10 * size[0] + 64) * 4) as usize]) / 255.0 - expected).abs() < 0.01,
        "camera inside the fog integrates the partial upward column",
    )?;
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
        "Fog render checks: PASS (1 cm analytic optical depth, opaque clipping, inside-layer camera, pause/orbit stability)"
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
    for pitch in [0.05, 0.3, 0.9] {
        s.camera[1] = pitch;
        s.visible = true;
        let low = renderer.read_image(gpu, &s, sim.domain, [320, 200])?;
        let high = renderer.read_image(gpu, &s, sim.domain, [960, 600])?;
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
        eprintln!("Fog sampling pitch={pitch:.2}: opacity MAE versus 3x supersampling={mae:.6}");
        check(
            mae < 0.01,
            "thin fog spatial sampling agrees with supersampled reference",
        )?;
    }
    Ok(())
}

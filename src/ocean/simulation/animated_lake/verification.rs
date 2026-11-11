//! Actual production compute path; reads back only during explicit verification.
use super::AnimatedLake;
use crate::ocean::{
    Error,
    gpu::context::Gpu,
    parameters::lake::{LakeConfig, PropagationMode},
};
fn read(
    gpu: &Gpu,
    lake: &AnimatedLake,
    encoder: wgpu::CommandEncoder,
) -> Result<Vec<[f32; 4]>, Error> {
    let mut encoder = encoder;
    let n = lake.n;
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("lake verification readback"),
        size: u64::from(n * n) * 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        lake.field.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &output,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n * 16),
                rows_per_image: Some(n),
            },
        },
        lake.field.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    Ok(gpu
        .read_buffer(&output, output.size())?
        .as_chunks::<16>()
        .0
        .iter()
        .map(|v| bytemuck::pod_read_unaligned(v))
        .collect())
}
pub fn run(gpu: &Gpu) -> Result<(), Error> {
    let spectrum = super::FftWaves::new(gpu, Default::default())?;
    let layout = AnimatedLake::layout(&gpu.device);
    let mut results = Vec::new();
    for mode in [PropagationMode::Depth, PropagationMode::Sdf] {
        let mut c = LakeConfig {
            enabled: true,
            automatic: false,
            mode,
            jitter_deg: 0.0,
            ..Default::default()
        };
        c.terrain.resolution = 128;
        let scene = crate::ocean::scene::lake::LakeScene::build(&c)?;
        let mut lake = AnimatedLake::new(gpu, c, &layout, &spectrum)?;
        lake.waves.emit(&c);
        let encoder = gpu.device.create_command_encoder(&Default::default());
        let mut encoder = encoder;
        lake.encode(gpu, &mut encoder, 2.0, c, false, false);
        let field = read(gpu, &lake, encoder)?;
        let n = lake.n as usize;
        let step = lake.extent / (lake.n - 1) as f32;
        let mut peak = 0.0_f32;
        for z in 1..n - 1 {
            for x in 1..n - 1 {
                let i = z * n + x;
                let value = field[i];
                if !value.iter().all(|v| v.is_finite()) {
                    return Err("nonfinite lake field".into());
                }
                if scene.cells[i][0] >= 0.0 && value[0].abs() > 1e-6 {
                    return Err("lake waves on dry terrain".into());
                }
                if value[0] < scene.cells[i][0].min(0.0) * 0.7 - 1e-6 {
                    return Err("lake trough below bed".into());
                }
                let dx = (field[i + 1][0] - field[i - 1][0]) / (2.0 * step);
                let dz = (field[i + n][0] - field[i - n][0]) / (2.0 * step);
                if (value[1] - dx).abs() > 1e-5 || (value[2] - dz).abs() > 1e-5 {
                    return Err("lake slopes disagree with published height".into());
                }
                peak = peak.max(value[0].abs());
                // The opposite half-lake is outside the selected angular sector.
                if (x as f32 / (n - 1) as f32 - 0.5) * c.direction_deg.to_radians().cos()
                    + (z as f32 / (n - 1) as f32 - 0.5) * c.direction_deg.to_radians().sin()
                    < -0.01
                    && value[0].abs() > 1e-5
                {
                    return Err("lake wave jumped to opposite sector".into());
                }
            }
        }
        if peak < 0.005 {
            return Err("missing lake wave".into());
        }
        // Compare samples just before and after reflection; birth is not frame-quantized.
        let p = c.propagation();
        let hit = f64::from((p.start - p.reflection) / p.speed);
        let mut handoff = Vec::new();
        for t in [hit - 0.0001, hit + 0.0001] {
            lake.waves.reset();
            lake.waves.emit(&c);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            lake.encode(gpu, &mut encoder, t, c, false, false);
            handoff.push(read(gpu, &lake, encoder)?);
        }
        let jump = handoff[0]
            .iter()
            .zip(&handoff[1])
            .map(|(a, b)| (a[0] - b[0]).abs())
            .fold(0.0_f32, f32::max);
        if jump > 0.001 {
            return Err(format!("lake reflection discontinuity {jump}").into());
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        lake.encode(gpu, &mut encoder, 0.0, c, false, true);
        if read(gpu, &lake, encoder)?.iter().any(|v| v[0] != 0.0) {
            return Err("clear left a lake wave".into());
        }
        println!(
            "GPU lake {mode:?}: wet/dry, trough bound, height/slopes, sector, reflection handoff (max jump {jump:.3e}), clear OK"
        );
        results.push(field);
    }
    if results[0]
        .iter()
        .zip(&results[1])
        .map(|(a, b)| (a[0] - b[0]).abs())
        .fold(0.0_f32, f32::max)
        < 0.001
    {
        return Err("depth and SDF unexpectedly identical".into());
    }
    angular_seam_and_time_origin(gpu)?;
    shared_fft(gpu)?;
    benchmark(gpu)?;
    Ok(())
}

fn angular_seam_and_time_origin(gpu: &Gpu) -> Result<(), Error> {
    let layout = AnimatedLake::layout(&gpu.device);
    let mut c = LakeConfig {
        enabled: true,
        automatic: false,
        jitter_deg: 0.0,
        ..Default::default()
    };
    c.terrain.resolution = 128;
    let spectrum = super::FftWaves::new(gpu, Default::default())?;
    let mut lake = AnimatedLake::new(gpu, c, &layout, &spectrum)?;
    let mut reference: Option<Vec<[f32; 4]>> = None;
    for (angle, origin) in [(-180.0, 0.0), (180.0, 0.0), (-180.0, 512.0)] {
        c.direction_deg = angle;
        lake.waves.reset();
        lake.waves.advance(origin, &c);
        lake.waves.emit(&c);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        lake.encode(gpu, &mut encoder, 2.0, c, false, false);
        let field = read(gpu, &lake, encoder)?;
        if let Some(expected) = &reference {
            let error = expected
                .iter()
                .zip(&field)
                .flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| (a - b).abs()))
                .fold(0.0_f32, f32::max);
            if error > 1e-5 {
                return Err(format!("lake angular seam/time rebase error {error}").into());
            }
        } else {
            reference = Some(field);
        }
    }
    println!("GPU lake angular wrap and 512-second time rebase OK");
    Ok(())
}

/// Independent traveling cosine: catches stale time, wrong metric/periodic mapping,
/// double gain, missing shore mask and slopes differentiated before composition.
fn shared_fft(gpu: &Gpu) -> Result<(), Error> {
    use crate::ocean::{
        parameters::ocean::OceanConfig,
        simulation::spectral::{FftWaves, spectrum::initial_spectrum},
    };
    let config = OceanConfig {
        resolution: 16,
        length: 16.0,
        ..Default::default()
    };
    let n = config.resolution as usize;
    let mut modes = initial_spectrum(config);
    for mode in &mut modes {
        mode[0] = 0.0;
        mode[1] = 0.0;
    }
    modes[1][0] = 0.1 * (n * n) as f32;
    let spectrum = FftWaves::from_spectrum(gpu, config, modes)?;
    let mut c = LakeConfig {
        enabled: true,
        automatic: false,
        fft_strength: 0.3,
        ..Default::default()
    };
    c.terrain.resolution = 128;
    let scene = crate::ocean::scene::lake::LakeScene::build(&c)?;
    let layout = AnimatedLake::layout(&gpu.device);
    let mut lake = AnimatedLake::new(gpu, c, &layout, &spectrum)?;
    let smooth = |v: f64| {
        let t = v.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let mut previous = None;
    for (dt, strength) in [(0.0, 0.3), (0.7, 0.3), (0.0, 0.3), (0.0, 0.6), (0.0, 0.0)] {
        c.fft_strength = strength;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        // Clear only the procedural events; it must not erase or reset FFT.
        lake.encode(gpu, &mut encoder, dt, c, false, true);
        let field = read(gpu, &lake, encoder)?;
        let size = lake.n as usize;
        let spacing = f64::from(lake.extent) / (size - 1) as f64;
        let phase =
            (9.81 * std::f64::consts::TAU / f64::from(config.length)).sqrt() * lake.waves.time;
        let mut maximum = 0.0_f64;
        for z in 0..size {
            for x in 0..size {
                let i = z * size + x;
                let bed = f64::from(scene.cells[i][0]);
                let distance = f64::from(scene.cells[i][3]);
                let world_x = (x as f64 / (size - 1) as f64 - 0.5) * f64::from(lake.extent);
                let grid = (world_x / f64::from(config.length) + 0.5) * n as f64;
                let cell = grid.floor();
                let t = grid - cell;
                let cosine = |j: f64| (std::f64::consts::TAU * j / n as f64 + phase).cos() * 0.2;
                let expected = if bed >= 0.0 {
                    0.0
                } else {
                    ((cosine(cell) * (1.0 - t) + cosine(cell + 1.0) * t)
                        * f64::from(strength)
                        * smooth(-bed / 0.5)
                        * smooth(distance))
                    .max(bed * 0.7)
                };
                maximum = maximum.max((f64::from(field[i][0]) - expected).abs());
                if !field[i].iter().all(|v| v.is_finite()) {
                    return Err("nonfinite shared FFT lake field".into());
                }
                if x > 0 && x < size - 1 && z > 0 && z < size - 1 {
                    let dx = f64::from(field[i + 1][0] - field[i - 1][0]) / (2.0 * spacing);
                    let dz = f64::from(field[i + size][0] - field[i - size][0]) / (2.0 * spacing);
                    if (f64::from(field[i][1]) - dx).abs() > 1e-5
                        || (f64::from(field[i][2]) - dz).abs() > 1e-5
                    {
                        return Err("combined FFT lake normal disagrees with final height".into());
                    }
                }
            }
        }
        if maximum > 2e-5 {
            return Err(format!("shared FFT lake cosine/mask/gain mismatch {maximum}").into());
        }
        if dt == 0.0
            && strength == 0.3
            && lake.waves.time > 0.0
            && previous.as_ref() != Some(&field)
        {
            return Err("paused FFT lake changed".into());
        }
        previous = Some(field);
    }
    println!(
        "GPU shared FFT lake: analytic traveling cosine, periodic metric mapping, shore mask, gain, final slopes, pause/clear/off OK"
    );
    Ok(())
}

fn benchmark(gpu: &Gpu) -> Result<(), Error> {
    let Some(mut timer) = common::gpu::timer::GpuTimer::new(gpu) else {
        println!("GPU lake timing: timestamp unsupported");
        return Ok(());
    };
    let layout = AnimatedLake::layout(&gpu.device);
    let spectrum = super::FftWaves::new(gpu, Default::default())?;
    for full in [false, true] {
        let c = LakeConfig {
            enabled: true,
            automatic: !full,
            reflect: !full,
            ..Default::default()
        };
        let mut lake = AnimatedLake::new(gpu, c, &layout, &spectrum)?;
        if full {
            for _ in 0..super::waves::CAPACITY {
                lake.waves.emit(&c);
            }
        }
        lake.waves.advance(if full { 2.0 } else { 7.0 }, &c);
        let mut samples = Vec::new();
        for i in 0..10 {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            timer.mark(&mut encoder, 0);
            lake.encode(gpu, &mut encoder, 0.0, c, false, false);
            timer.mark(&mut encoder, 1);
            timer.resolve(&mut encoder);
            // Explicit verification only: this readback also waits for timestamp completion.
            let _ = read(gpu, &lake, encoder)?;
            timer.poll(gpu);
            let sample = timer
                .latest
                .take()
                .ok_or("lake timestamp readback did not complete")?;
            if i > 0 {
                samples.push(sample.ms);
            }
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "GPU lake compute: {}x{}, {} active waves, median {:.3} ms, max {:.3} ms (9 samples, warm-up excluded)",
            lake.n,
            lake.n,
            lake.waves.active_count(),
            samples[4],
            samples[8]
        );
    }
    Ok(())
}

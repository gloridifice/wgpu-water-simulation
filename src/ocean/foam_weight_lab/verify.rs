//! GPU contracts for the experiment, separate from production foam dynamics.
use super::*;

pub fn run() -> Result<(), Error> {
    let gpu = pollster::block_on(Gpu::new(&wgpu::Instance::default(), None))?;
    // Uniform analytic inputs test threshold, hard-cutoff and modulation laws.
    for (j, threshold, width, expected_mask) in [
        (1.0, 0.0, 0.0, 0.0),
        (0.875, 0.125, 0.0, 0.0),
        (0.75, 0.125, 0.0, 1.0),
        (0.875, 0.0625, 0.125, 0.5),
        (-0.5, 0.0, 0.25, 1.0),
    ] {
        let input = vec![[123.0_f32, j]; 64]; // Coverage is deliberately unrelated.
        check(
            &gpu,
            &input,
            4,
            [1.0, 1.0],
            threshold,
            width,
            0.75,
            2.0,
            j,
            expected_mask,
        )?;
    }
    // Every target cell covers an even number of checkerboard samples. Correct
    // filtering yields mask=1/2; thresholding mean J instead would yield 1.
    let checker: Vec<_> = (0..64)
        .map(|i| [0.0, if (i % 8 + i / 8) % 2 == 0 { 0.0 } else { 1.0 }])
        .collect();
    for extent in [[1.0, 1.0], [2.0, 3.0]] {
        check(&gpu, &checker, 4, extent, 0.0, 0.25, 1.0, 4.0, 0.5, 0.5)?;
        check(&gpu, &checker, 4, extent, 0.0, 0.25, 0.0, 7.0, 0.5, 0.5)?;
    }
    // Exercise the actual FFT -> production J -> diagnostic maps pipeline,
    // including changing time while paused and the zero-displacement limit.
    let config = OceanConfig {
        resolution: 64,
        ..Default::default()
    };
    let experiment = Experiment::new(&gpu, config, 64)?;
    let mut controls = Controls {
        grid: 64,
        ..Default::default()
    };
    let sample = |controls: Controls, time: f32| -> Result<Vec<[f32; 4]>, Error> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        experiment.encode(&gpu, &mut encoder, time, controls);
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&experiment.maps.values, experiment.maps.values.size())?;
        Ok(bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect())
    };
    let a = sample(controls, 8.0)?;
    let paused = sample(controls, 8.0)?;
    let b = sample(controls, 8.5)?;
    if a != paused
        || a == b
        || a.iter()
            .flatten()
            .chain(b.iter().flatten())
            .any(|v| !v.is_finite())
    {
        return Err("FFT weight time evolution/pause/finite contract failed".into());
    }
    controls.chop = 0.0;
    for q in sample(controls, 8.5)? {
        if (q[0] - 1.0).abs() > 1e-6 || q[1..] != [0.0, 0.0, 0.0] {
            return Err(format!("flat displacement produced weights: {q:?}").into());
        }
    }
    println!(
        "GPU J weight lab: thresholds, zero-strength baseline, gain/source units, nonlinear-before-area filtering, periodic rectangular sampling, real FFT/J, pause and zero-displacement passed on {}",
        gpu.name
    );
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn check(
    gpu: &Gpu,
    input: &[[f32; 2]],
    grid: u32,
    extent: [f32; 2],
    threshold: f32,
    transition: f32,
    strength: f32,
    gain: f32,
    expected_j: f32,
    expected_mask: f32,
) -> Result<(), Error> {
    let buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("analytic J fixture"),
            contents: bytemuck::cast_slice(input),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let maps = Maps::new(gpu, &buffer, grid);
    let controls = Controls {
        grid,
        extent,
        threshold,
        transition,
        strength,
        gain,
        base_rate: 3.0,
        ..Default::default()
    };
    let params = controls.params(OceanConfig {
        resolution: 8,
        length: 8.0,
        ..Default::default()
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    maps.encode(gpu, &mut encoder, &params);
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.read_buffer(&maps.values, maps.values.size())?;
    let expected_weight = 1.0 - strength + strength * gain * expected_mask;
    let expected = [
        expected_j,
        expected_mask,
        expected_weight,
        3.0 * expected_weight,
    ];
    for (i, bytes) in bytes.as_chunks::<16>().0.iter().enumerate() {
        let q: [f32; 4] = bytemuck::pod_read_unaligned(bytes);
        if q.iter()
            .zip(expected)
            .any(|(&v, e)| !v.is_finite() || (v - e).abs() > 1e-5)
        {
            return Err(format!("J weight fixture cell {i}: {q:?} != {expected:?}").into());
        }
    }
    Ok(())
}

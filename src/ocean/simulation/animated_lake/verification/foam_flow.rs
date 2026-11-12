//! Verify actual published coordinates: nonzero displacement alone is not deformation.
use super::{AnimatedLake, Error, Gpu, LakeConfig, read};

pub(super) fn run(gpu: &Gpu) -> Result<(), Error> {
    let mut c = LakeConfig {
        enabled: true,
        automatic: false,
        foam_strength: 1.0,
        ..Default::default()
    };
    c.terrain.rocks = 0;
    c.terrain.irregularity = 0.0;
    let scene = crate::ocean::scene::lake::LakeScene::build(&c)?;
    let layout = AnimatedLake::layout(&gpu.device);
    let spectrum = super::super::FftWaves::new(gpu, Default::default())?;
    let mut lake = AnimatedLake::new(gpu, c, &layout, &spectrum)?;
    let capture = |lake: &mut AnimatedLake, c, dt| -> Result<Vec<[f32; 4]>, Error> {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        lake.encode(gpu, &mut encoder, dt, c, false, false);
        read(gpu, &lake.foam_motion, encoder)
    };
    let n = lake.n as usize;
    let extent = lake.extent;
    let spacing = extent / (lake.n - 1) as f32;
    let index = |x: f32| n / 2 * n + ((x / extent + 0.5) * (n - 1) as f32).round() as usize;
    let inside = index(c.terrain.radius - 2.2);
    let outside = inside + 1;
    let material_stretch =
        |field: &[[f32; 4]]| 1.0 - (field[outside][0] - field[inside][0]) / spacing;
    c.foam_flow_distortion = 0.0;
    let uniform = capture(&mut lake, c, 3.0)?;
    // Independent constant radial-speed reference well away from the boundary/centre.
    let i = index(20.0);
    if (uniform[i][0] + c.foam_flow_speed * 3.0).abs() > 0.01 {
        return Err("lake foam coordinate motion lost its metres, direction or layer age".into());
    }
    let uniform_stretch = material_stretch(&uniform);
    lake.waves.reset();
    c.foam_flow_distortion = 1.0;
    let deformed = capture(&mut lake, c, 3.0)?;
    let deformed_stretch = material_stretch(&deformed);
    // Backward material spacing contracts as the forward foam stretches away from
    // the shore. The former instantaneous velocity*age path (and pure translation)
    // must not pass this contract; no shader formula is used to generate its oracle.
    if !(0.05..uniform_stretch * 0.8).contains(&deformed_stretch) {
        return Err(format!("lake foam lacks nearshore deformation: Jacobian {uniform_stretch:.3} -> {deformed_stretch:.3}").into());
    }
    for (cell, value) in scene.cells.iter().zip(&deformed) {
        if !value.iter().all(|v| v.is_finite())
            || (cell[0] >= 0.0 && *value != [0.0; 4])
            || value[0].hypot(value[1]) > c.foam_flow_speed * 3.0 + 0.001
        {
            return Err("invalid lake foam displacement, dry mask or speed bound".into());
        }
    }
    if capture(&mut lake, c, 0.0)? != deformed {
        return Err("paused lake foam coordinates changed".into());
    }
    lake.waves.reset();
    let _ = capture(&mut lake, c, 1.0)?;
    if capture(&mut lake, c, 2.0)? != deformed {
        return Err("lake foam depends on frame partition".into());
    }
    // Each resetting layer is hidden; the OTHER layer must remain continuous.
    for (time, channel) in [(3.0, 0), (6.0, 2)] {
        lake.waves.reset();
        let before = capture(&mut lake, c, time - 0.001)?;
        let after = capture(&mut lake, c, 0.002)?;
        let jump = before
            .iter()
            .zip(after)
            .map(|(a, b)| (a[channel] - b[channel]).hypot(a[channel + 1] - b[channel + 1]))
            .fold(0.0_f32, f32::max);
        if jump > 0.005 {
            return Err(format!("visible foam coordinates jump at cycle reset: {jump}").into());
        }
    }
    c.foam_flow_speed = 0.0;
    if capture(&mut lake, c, 1.0)?.iter().any(|v| *v != [0.0; 4]) {
        return Err("zero lake flow retained material displacement".into());
    }
    println!(
        "GPU lake foam UV: radial direction/metres, nearshore stretch Jacobian {uniform_stretch:.3} -> {deformed_stretch:.3}, wet/dry, pause, partition, cycle reset, zero speed OK"
    );
    Ok(())
}

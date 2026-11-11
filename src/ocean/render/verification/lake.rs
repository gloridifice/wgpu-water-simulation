//! Production lake raster, optical selection and mode/scene round trips.
use super::optics::raster_image;
use crate::ocean::{
    Error,
    camera::Camera,
    gpu::context::Gpu,
    parameters::{
        lake::PropagationMode,
        ocean::OceanConfig,
        optics::{OpticalMethod, OpticsConfig},
    },
    render::Renderer,
    simulation::{offshore::foam::Foam, spectral::FftWaves as Ocean},
};
pub(in crate::ocean) fn run(gpu: &Gpu) -> Result<(), Error> {
    let size = [129, 97];
    let config = OceanConfig {
        resolution: 8,
        mesh_resolution: 8,
        ..Default::default()
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let format = wgpu::TextureFormat::Rgba32Float;
    let mut renderer = Renderer::new(gpu, &ocean, &foam, format, size[0], size[1]);
    let mut optics = OpticsConfig::lake_scene();
    optics.lake.terrain.resolution = 128;
    optics.lake.automatic = false;
    optics.lake.fft_strength = 0.0;
    let camera =
        Camera::lake(optics.lake.terrain.radius).uniform(config, size[0] as f32 / size[1] as f32);
    let mut modes = Vec::new();
    for mode in [PropagationMode::Depth, PropagationMode::Sdf] {
        optics.lake.mode = mode;
        renderer.prepare_scene(gpu, &optics, format)?;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.encode_lake(
            gpu,
            &mut encoder,
            0.0,
            optics.lake,
            crate::ocean::simulation::animated_lake::LakeCommands {
                emit: true,
                reset: true,
                ..Default::default()
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let still = raster_image(gpu, &renderer, &camera, optics, size)?;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        renderer.encode_lake(gpu, &mut encoder, 2.0, optics.lake, Default::default());
        gpu.queue.submit([encoder.finish()]);
        let moving = raster_image(gpu, &renderer, &camera, optics, size)?;
        let changed = still
            .iter()
            .zip(&moving)
            .filter(|(a, b)| (a[0] - b[0]).abs() > 0.0001)
            .count();
        if changed < 5 || moving.iter().flatten().any(|v| !v.is_finite()) {
            return Err("lake raster missing animated water or nonfinite pixels".into());
        }
        // Unsupported physical paths retain settings but must not alter lake rendering.
        optics.caustics = true;
        optics.refraction = true;
        optics.refraction_method = OpticalMethod::Physical;
        let unsupported = raster_image(gpu, &renderer, &camera, optics, size)?;
        if unsupported != moving {
            return Err("lake silently used unsupported physical optics".into());
        }
        optics.caustics = false;
        optics.refraction_method = OpticalMethod::Fast;
        optics.screen_space_reflections = true;
        let fast = raster_image(gpu, &renderer, &camera, optics, size)?;
        if fast.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid lake fast refraction/SSR".into());
        }
        optics.refraction = false;
        optics.screen_space_reflections = false;
        modes.push(moving);
    }
    if modes[0] == modes[1] {
        return Err("lake propagation selector did not reach raster".into());
    }
    // A numerical field test cannot catch a raster still bound to the old wave surface.
    // No procedural events: the only animation here must come from shared FFT.
    optics.lake.fft_strength = 0.2;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.encode_lake(
        gpu,
        &mut encoder,
        0.0,
        optics.lake,
        crate::ocean::simulation::animated_lake::LakeCommands {
            reset: true,
            ..Default::default()
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let fft_start = raster_image(gpu, &renderer, &camera, optics, size)?;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.encode_lake(gpu, &mut encoder, 1.0, optics.lake, Default::default());
    gpu.queue.submit([encoder.finish()]);
    let fft_moving = raster_image(gpu, &renderer, &camera, optics, size)?;
    if fft_start == fft_moving || fft_moving.iter().flatten().any(|v| !v.is_finite()) {
        return Err("shared FFT did not reach lake water shading".into());
    }
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.encode_lake(
        gpu,
        &mut encoder,
        0.0,
        optics.lake,
        crate::ocean::simulation::animated_lake::LakeCommands {
            clear: true,
            ..Default::default()
        },
    );
    gpu.queue.submit([encoder.finish()]);
    if raster_image(gpu, &renderer, &camera, optics, size)? != fft_moving {
        return Err("paused lake clear changed shared FFT raster".into());
    }
    optics.lake.fft_strength = 0.0;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    renderer.encode_lake(gpu, &mut encoder, 0.0, optics.lake, Default::default());
    gpu.queue.submit([encoder.finish()]);
    if raster_image(gpu, &renderer, &camera, optics, size)? == fft_moving {
        return Err("lake FFT strength zero did not reach raster".into());
    }
    let sea = OpticsConfig::default();
    renderer.prepare_scene(gpu, &sea, format)?;
    let restored = raster_image(gpu, &renderer, &camera, sea, size)?;
    let fresh = Renderer::new(gpu, &ocean, &foam, format, size[0], size[1]);
    let original = raster_image(gpu, &fresh, &camera, sea, size)?;
    if restored != original {
        return Err("switching away from lake changed original ocean raster".into());
    }
    println!(
        "GPU lake raster: depth/SDF, shared FFT animation/pause/clear/off, physical-optics isolation, fast refraction/SSR, ocean restore OK"
    );
    Ok(())
}

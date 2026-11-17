//! Offscreen snapshots using the existing production pipeline.
use crate::ocean::{
    Error,
    gpu::context::Gpu,
    render::Renderer,
    settings::SavedSettings,
    simulation::{
        offshore::foam::{Foam, FoamStep},
        spectral::FftWaves as Ocean,
    },
};

/// Shares the window's compute and render pipelines for headless verification and image export.
pub async fn snapshot(
    initial: &SavedSettings,
    time: f32,
    path: &std::path::Path,
    foam_view: Option<u32>,
    emission_window: Option<[f32; 2]>,
) -> Result<(), Error> {
    let config = initial.ocean;
    let mut optics = initial.optics;
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    eprintln!("GPU: {}", gpu.name);
    let ocean = Ocean::new(&gpu, config)?;
    let (width, height) = (1280, 800);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let foam_field = Foam::new(&gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new_display(&gpu, &ocean, &foam_field, format, width, height);
    renderer.set_shore_resolution(
        &gpu,
        &ocean,
        optics.shore.resolution,
        format,
        [width, height],
    );
    renderer.prepare_scene(&gpu, &optics, format)?;
    renderer.update_optics(&gpu, &optics);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("image readback"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    // Deterministic history warm-up: up to 60 s for wet sand, 8 s for open ocean.
    let mut camera = initial.camera.uniform(config, width as f32 / height as f32);
    if let Some(view) = foam_view {
        camera.misc[2] = view as f32;
    }
    if emission_window.is_some() && !optics.swe_enabled() {
        return Err("--foam-emission-window requires an active SWE scene".into());
    }
    let duration = if optics.lake.enabled {
        0.0
    } else {
        time.min(if optics.swe_enabled() { 60.0 } else { 8.0 })
    };
    let steps = (duration * 60.0).ceil() as u32;
    if steps > 0 {
        let dt = duration / steps as f32;
        for i in 1..=steps {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            // Roundoff must not put the last warm-up step beyond the target:
            // the final render would otherwise look like a time rewind/reset.
            let step_time = (time - duration + i as f32 * dt).min(time);
            optics.shore.foam_emission_enabled = initial.optics.shore.foam_emission_enabled
                && emission_window.is_none_or(|[start, end]| step_time > start && step_time <= end);
            ocean.encode_filtered(&gpu, &mut encoder, step_time, &optics.shore);
            renderer.shore.encode(
                &gpu,
                &mut encoder,
                &optics,
                step_time,
                false,
                camera.ocean[3],
            );
            renderer.encode_spray(&gpu, &mut encoder);
            foam_field.encode(
                &gpu,
                &mut encoder,
                FoamStep::new(
                    config.resolution,
                    config.length,
                    camera.ocean[2] * camera.ocean[3],
                    dt,
                    initial.foam,
                    false,
                ),
            );
            gpu.queue.submit([encoder.finish()]);
        }
    }
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    if optics.lake.enabled {
        renderer.encode_lake(
            &gpu,
            &mut encoder,
            f64::from(time),
            optics.lake,
            Default::default(),
        );
    } else {
        ocean.encode_filtered(&gpu, &mut encoder, time, &optics.shore);
        renderer
            .shore
            .encode(&gpu, &mut encoder, &optics, time, false, camera.ocean[3]);
        renderer.encode_spray(&gpu, &mut encoder);
        if let Some(error) = &renderer.shore.spray_error {
            return Err(error.clone().into());
        }
        foam_field.encode(
            &gpu,
            &mut encoder,
            FoamStep::new(
                config.resolution,
                config.length,
                camera.ocean[2] * camera.ocean[3],
                0.0,
                initial.foam,
                false,
            ),
        );
    }
    renderer.encode(
        &gpu,
        &mut encoder,
        &texture.create_view(&Default::default()),
        &camera,
    );
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    if optics.lake.enabled {
        let (waves, dropped, rocks) = renderer.lake_status();
        eprintln!(
            "Lake {:?}: {waves} active waves, {dropped} dropped, {rocks} rocks",
            optics.lake.mode
        );
    } else {
        let foam_bytes = gpu.read_buffer(&foam_field.field, foam_field.field.size())?;
        let diagnostics: Vec<[f32; 2]> = foam_bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        let min_j = diagnostics
            .iter()
            .map(|v| v[1])
            .fold(f32::INFINITY, f32::min);
        let max_f = diagnostics.iter().map(|v| v[0]).fold(0.0_f32, f32::max);
        let covered = diagnostics.iter().filter(|v| v[0] > 0.02).count();
        eprintln!(
            "Foam: J min={min_j:.3}, F max={max_f:.3}, coverage F>0.02={:.1}%",
            100.0 * covered as f32 / diagnostics.len() as f32
        );
    }
    let mapped = gpu.map_buffer(&staging)?;
    if optics.swe_enabled() {
        let bytes = gpu.read_buffer(
            &renderer.shore.field,
            u64::from(renderer.shore.resolution.pow(2)) * 16,
        )?;
        let samples: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        let foam = samples.iter().map(|v| v[0]).fold(0.0_f32, f32::max);
        let wet = samples
            .iter()
            .filter(|v| v[2] < 0.005 && v[1] > 0.1)
            .count();
        eprintln!("Shore: foam max={foam:.3}, exposed wet sand cells={wet}");
        if optics.shore.foam_ridge_enabled || optics.shore.view >= 7 {
            let bytes = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
            let cells = renderer.shore.resolution.pow(2) as usize;
            let extra: Vec<[f32; 4]> = bytes[2 * cells * 16..3 * cells * 16]
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            let emitting = extra.iter().filter(|q| q[0] > 0.001).count();
            let source_max = extra.iter().map(|q| q[0]).fold(0.0_f32, f32::max);
            let foam_cells = samples.iter().filter(|q| q[0] > 0.02).count();
            eprintln!(
                "Front: emitting cells={emitting}/{cells}, source max={source_max:.4}/s, transported foam cells (>0.02)={foam_cells}"
            );
        }
    }
    let pixels: Vec<u8> = mapped
        .chunks_exact(bytes_per_row as usize)
        .flat_map(|row| row[..(width * 4) as usize].iter().copied())
        .collect();
    let mut png = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create_new(path)?),
        width,
        height,
    );
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(&pixels)?;
    eprintln!("Saved {} at t={time:.2}s", path.display());
    Ok(())
}

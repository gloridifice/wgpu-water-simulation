//! Headless, fixed-time production-path profiling. Never saves personal settings.
use std::{fs, path::PathBuf};
use water_simulation::ocean::{
    Error,
    gpu::context::Gpu,
    render::Renderer,
    settings::load_config,
    simulation::offshore::{
        Ocean,
        foam::{Foam, FoamStep},
    },
};

fn mark(encoder: &mut wgpu::CommandEncoder, queries: &wgpu::QuerySet, index: u32) {
    let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("offline profile boundary"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: queries,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: None,
        }),
    });
}
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}
fn main() -> Result<(), Error> {
    let mut config = None;
    let mut output = None;
    let mut resolution = None;
    let mut no_spray = false;
    let mut frames = 120u32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => config = Some(PathBuf::from(args.next().ok_or("missing config")?)),
            "--output" => output = Some(PathBuf::from(args.next().ok_or("missing output")?)),
            "--resolution" => resolution = Some(args.next().ok_or("missing resolution")?.parse()?),
            "--frames" => frames = args.next().ok_or("missing frames")?.parse()?,
            "--no-spray" => no_spray = true,
            "--help" | "-h" => {
                println!(
                    "ocean-profile --config PATH --output NEW_DIR [--resolution N] [--frames 120] [--no-spray]\nRead-only config; 30 warm-up frames, fixed 1/60 simulated second, 1280x800 offscreen.\nGPU timestamps cover FFT, offshore foam, complete SWE, spray particles, rendering.\nWrites profile.json, effective.json, final-state.bin and final.rgba; no UI/present timing."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    if !(1..=1000).contains(&frames) {
        return Err("frames must be 1..1000".into());
    }
    let mut settings = load_config(&config.ok_or("--config is required")?)?;
    if let Some(n) = resolution {
        settings.optics.shore.resolution = n;
    }
    if no_spray {
        settings.optics.spray.enabled = false;
    }
    settings.validate()?;
    let output = output.ok_or("--output is required")?;
    fs::create_dir(&output)?;
    fs::write(
        output.join("effective.json"),
        serde_json::to_vec_pretty(&settings)?,
    )?;
    pollster::block_on(async {
        let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
        if !gpu
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return Err("GPU timestamp queries are required for profiling".into());
        }
        eprintln!("GPU: {}", gpu.name);
        let ocean = Ocean::new(&gpu, settings.ocean)?;
        let foam = Foam::new(&gpu, &ocean.fields, settings.ocean.resolution);
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let (width, height) = (1280, 800);
        let mut renderer = Renderer::new(&gpu, &ocean, &foam, format, width, height);
        renderer.set_shore_resolution(
            &gpu,
            &ocean,
            settings.optics.shore.resolution,
            format,
            [width, height],
        );
        renderer.shore.timer = None;
        renderer.prepare_scene(&gpu, &settings.optics, format)?;
        renderer.update_optics(&gpu, &settings.optics);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("profile target"),
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
        let view = texture.create_view(&Default::default());
        let camera = settings
            .camera
            .uniform(settings.ocean, width as f32 / height as f32);
        let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("production path profile"),
            ty: wgpu::QueryType::Timestamp,
            count: 6 * frames,
        });
        let timings = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("profile timestamps"),
            size: u64::from(6 * frames) * 8,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut metadata = Vec::new();
        for frame in 0..30 + frames {
            let time = frame as f32 / 60.0;
            let dt = if frame == 0 {
                0.0
            } else {
                time - (frame - 1) as f32 / 60.0
            };
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let boundary = |encoder: &mut wgpu::CommandEncoder, i| {
                if frame >= 30 {
                    mark(encoder, &queries, (frame - 30) * 6 + i);
                }
            };
            boundary(&mut encoder, 0);
            ocean.encode_filtered(&gpu, &mut encoder, time, &settings.optics.shore);
            boundary(&mut encoder, 1);
            foam.encode(
                &gpu,
                &mut encoder,
                FoamStep::new(
                    settings.ocean.resolution,
                    settings.ocean.length,
                    camera.ocean[2] * camera.ocean[3],
                    dt,
                    settings.foam,
                    frame == 0,
                ),
            );
            boundary(&mut encoder, 2);
            renderer.shore.encode(
                &gpu,
                &mut encoder,
                &settings.optics,
                time,
                frame == 0,
                camera.ocean[3],
            );
            boundary(&mut encoder, 3);
            renderer.encode_spray(&gpu, &mut encoder);
            boundary(&mut encoder, 4);
            renderer.encode(&gpu, &mut encoder, &view, &camera);
            boundary(&mut encoder, 5);
            if frame >= 30 {
                metadata.push((time, renderer.shore.substeps, renderer.shore.simulated_dt));
            }
            gpu.queue.submit([encoder.finish()]);
        }
        if let Some(error) = &renderer.shore.spray_error {
            return Err(error.clone().into());
        }
        let pixels = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("profile pixels"),
            size: u64::from(width * height * 4),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.resolve_query_set(&queries, 0..6 * frames, &timings, 0);
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &pixels,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
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
        let bytes = gpu.read_buffer(&timings, timings.size())?;
        let period = f64::from(gpu.queue.get_timestamp_period()) / 1e6;
        let samples: Vec<_> = bytes
            .as_chunks::<48>()
            .0
            .iter()
            .zip(metadata)
            .map(|(row, (time, steps, dt))| {
                let t: Vec<_> = row
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|b| u64::from_le_bytes(*b))
                    .collect();
                let ms: Vec<_> = t
                    .windows(2)
                    .map(|p| p[1].wrapping_sub(p[0]) as f64 * period)
                    .collect();
                serde_json::json!({"time_s":time,"substeps":steps,"simulated_dt":dt,"gpu_ms":ms})
            })
            .collect();
        let medians: Vec<_> = (0..5)
            .map(|i| {
                median(
                    samples
                        .iter()
                        .map(|s| s["gpu_ms"][i].as_f64().unwrap())
                        .collect(),
                )
            })
            .collect();
        let state = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
        let n = renderer.shore.resolution as usize;
        let cells = n * n;
        let depths: Vec<_> = state[cells * 16..cells * 32]
            .as_chunks::<16>()
            .0
            .iter()
            .map(|q| f32::from_le_bytes(q[..4].try_into().unwrap()))
            .collect();
        let mut dry_tiles = 0;
        // Diagnostic only: dry 16x16 tiles with a two-cell halo, not a runtime mask.
        for z in (0..n).step_by(16) {
            for x in (0..n).step_by(16) {
                if (z.saturating_sub(2)..(z + 18).min(n)).all(|j| {
                    (x.saturating_sub(2)..(x + 18).min(n)).all(|i| depths[j * n + i] == 0.0)
                }) {
                    dry_tiles += 1;
                }
            }
        }
        fs::write(output.join("final-state.bin"), state)?;
        fs::write(output.join("final.rgba"), gpu.map_buffer(&pixels)?)?;
        let names = [
            "fft",
            "offshore_foam",
            "swe_including_copies_and_spray_source",
            "spray_particles",
            "render",
        ];
        eprintln!(
            "GPU median ms: {names:?} = {medians:?}; dry tiles {dry_tiles}/{}",
            (n / 16).pow(2)
        );
        fs::write(
            output.join("profile.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "gpu":gpu.name,"frames":frames,"warmup_frames":30,"fixed_dt_s":1.0/60.0,
                "width":width,"height":height,"components":names,"median_gpu_ms":medians,
                "dry_tiles_with_two_cell_halo":dry_tiles,"tiles":(n/16).pow(2),"samples":samples,
                "method":"Production calls, fixed simulation time, no UI or presentation. Timestamp boundary passes add overhead. Readback only after all frames. Dry tile count is a final-state diagnostic, not proof of safe multi-stage skipping."
            }))?,
        )?;
        Ok(())
    })
}

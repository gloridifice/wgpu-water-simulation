//! Explicit paired offscreen benchmark. Only this diagnostic path waits for timestamps.
use super::Acceleration;
use crate::{
    Error,
    parameters::{Reconstruction, Session},
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::Gpu;

pub(crate) fn run(gpu: &Gpu) -> Result<(), Error> {
    if !gpu
        .device
        .features()
        .contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        eprintln!("Fog render benchmark: skipped (GPU timestamps unsupported)");
        return Ok(());
    }
    let source = BakeSource::playground();
    let terrain = Terrain::decode(
        include_bytes!("../../assets/playground.fogterrain"),
        &source,
    )?;
    let mut sim = Simulation::new(gpu, &terrain)?;
    let variants = [
        Acceleration::PerRayBounds,
        Acceleration::CachedBounds,
        Acceleration::Hierarchy,
        Acceleration::Raster,
    ];
    let renderers = variants
        .map(|mode| {
            super::Renderer::new_with_acceleration(
                gpu,
                &sim,
                &terrain,
                &source,
                wgpu::TextureFormat::Rgba8Unorm,
                mode,
            )
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut session = Session {
        render_mode: crate::parameters::RenderMode::Tetrahedra,
        ..Default::default()
    };
    session.fog.wind = [0.0; 2];
    session.fog.decay_enabled = false;
    session.camera[0] = 0.45;
    let size = [1280, 800];
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("fixed render benchmark target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
        label: Some("fog benchmark phase timestamps"),
        ty: wgpu::QueryType::Timestamp,
        count: 4,
    });
    let timestamps = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fog benchmark timestamps"),
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mark = |encoder: &mut wgpu::CommandEncoder, index| {
        let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fog benchmark timing boundary"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &queries,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: None,
            }),
        });
    };
    eprintln!(
        "Fog paired benchmark: seed=7, time=0, grid=256x256, target=1280x800, yaw=0.45, base_segments=2, subdivision=3; 20 warmups + 21 samples, rotating variant order; GPU ms, excludes SWE/UI/present"
    );
    for sparse in [false, true] {
        let scene = if sparse { "sparse" } else { "default" };
        if sparse {
            session.fog.noise_amplitude = 0.5;
            session.fog.noise_frequency = 0.12;
            session.fog.height_offset = -0.38;
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        sim.encode(gpu, &mut encoder, &session.fog, 0.0, true, &[])?;
        gpu.queue.submit([encoder.finish()]);
        let (_, field) = sim.snapshot(gpu)?;
        let occupied = field.iter().filter(|q| q[0] > 0.0001 && q[1] > 0.0).count();
        eprintln!(
            "Fog benchmark scene={scene}: occupied={occupied}/{} raw cells",
            field.len()
        );
        for pitch in [0.05, 0.3] {
            if sparse && pitch != 0.05 {
                continue;
            }
            session.camera[1] = pitch;
            for mode in [
                Reconstruction::Bilinear,
                Reconstruction::SmoothStep,
                Reconstruction::PreSmoothed,
                Reconstruction::CubicBSpline,
            ] {
                if sparse && mode != Reconstruction::Bilinear {
                    continue;
                }
                session.reconstruction = mode;
                // Outside timing: compare actual opacity against the original
                // traversal, allowing only one RGBA8 quantization step.
                session.debug_view = 5;
                let reference = renderers[0].read_image(gpu, &session, sim.domain, [320, 200])?;
                for (renderer, variant) in renderers.iter().zip(variants).skip(1) {
                    let image = renderer.read_image(gpu, &session, sim.domain, [320, 200])?;
                    let max_delta = image
                        .iter()
                        .zip(&reference)
                        .map(|(a, b)| a.abs_diff(*b))
                        .max()
                        .unwrap_or(0);
                    let changed = image
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(reference.as_chunks::<4>().0.iter())
                        .filter(|(a, b)| a != b)
                        .count();
                    eprintln!(
                        "Fog acceleration opacity scene={scene} {mode:?} pitch={pitch:.2} {variant:?}: max_delta={max_delta}/255 changed_pixels={changed}/64000"
                    );
                    if variant != Acceleration::Raster && max_delta > 1 {
                        return Err(
                            "fog acceleration changed opacity by more than one quantization step"
                                .into(),
                        );
                    }
                }
                session.debug_view = 0;
                for refresh in [true, false] {
                    let mut samples: [Vec<[f64; 4]>; 4] = std::array::from_fn(|_| Vec::new());
                    for frame in 0..41 {
                        for order in 0..4 {
                            let index = (frame + order) % 4;
                            let renderer = &renderers[index];
                            let mut encoder =
                                gpu.device.create_command_encoder(&Default::default());
                            if refresh && order == 0 {
                                // Identical values, one shared new publication per
                                // group. All variants see precisely the same field.
                                sim.encode(gpu, &mut encoder, &session.fog, 0.0, false, &[])?;
                            }
                            mark(&mut encoder, 0);
                            let reconstruction =
                                renderer.prepare_field(gpu, &mut encoder, &session);
                            mark(&mut encoder, 1);
                            renderer.prepare_bounds(gpu, &mut encoder, &session, reconstruction);
                            mark(&mut encoder, 2);
                            renderer.draw(
                                gpu,
                                &mut encoder,
                                &view,
                                &session,
                                sim.domain,
                                size,
                                reconstruction,
                            );
                            mark(&mut encoder, 3);
                            encoder.resolve_query_set(&queries, 0..4, &timestamps, 0);
                            gpu.queue.submit([encoder.finish()]);
                            let bytes = gpu.read_buffer(&timestamps, 32)?;
                            let ticks: Vec<_> = bytes
                                .as_chunks::<8>()
                                .0
                                .iter()
                                .map(|b| u64::from_le_bytes(*b))
                                .collect();
                            let ms = |a: usize, b: usize| {
                                ticks[b].wrapping_sub(ticks[a]) as f64
                                    * f64::from(gpu.queue.get_timestamp_period())
                                    / 1e6
                            };
                            if frame >= 20 {
                                samples[index].push([ms(0, 1), ms(1, 2), ms(2, 3), ms(0, 3)]);
                            }
                        }
                    }
                    for (variant, samples) in variants.iter().zip(&samples) {
                        let median: [f64; 4] = std::array::from_fn(|axis| {
                            let mut values: Vec<_> = samples.iter().map(|v| v[axis]).collect();
                            values.sort_by(f64::total_cmp);
                            values[values.len() / 2]
                        });
                        eprintln!(
                            "Fog paired scene={scene} {mode:?} pitch={pitch:.2} refresh={refresh} {variant:?}: filter={:.4} bounds={:.4} draw={:.4} total={:.4} GPU ms (independent medians)",
                            median[0], median[1], median[2], median[3]
                        );
                    }
                }
            }
        }
    }
    super::raster_benchmark::run(gpu)
}

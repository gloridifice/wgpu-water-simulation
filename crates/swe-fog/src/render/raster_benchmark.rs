//! Production-pass timestamps, including dense/inside-volume overdraw stress.
use crate::{
    Error,
    parameters::{Reconstruction, Session},
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::Gpu;
pub(super) fn run(gpu: &Gpu) -> Result<(), Error> {
    let source = BakeSource::playground();
    let terrain = Terrain::decode(
        include_bytes!("../../assets/playground.fogterrain"),
        &source,
    )?;
    let mut sim = Simulation::new(gpu, &terrain)?;
    let renderer = super::Renderer::new(
        gpu,
        &sim,
        &terrain,
        &source,
        wgpu::TextureFormat::Rgba8Unorm,
    )?;
    let raster = renderer.raster.as_ref().unwrap();
    let size = [1280, 800];
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("raster fog benchmark"),
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
        label: Some("fog production pass times"),
        ty: wgpu::QueryType::Timestamp,
        count: 12,
    });
    let results = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("explicit raster timing readback"),
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mark = |encoder: &mut wgpu::CommandEncoder, index| {
        let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("fog filter timestamp"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &queries,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: None,
            }),
        });
    };
    eprintln!(
        "Fog raster phases: 1280x800, 256x256, seed=7, time=0, 20 warmups + 41 samples; GPU ms; full refresh including filter, opaque scene, optical depth and composite; no SWE/UI/present"
    );
    for scene in ["default", "sparse", "dense", "rough_dense"] {
        let mut session = Session {
            render_mode: crate::parameters::RenderMode::Tetrahedra,
            ..Default::default()
        };
        session.camera[0] = 0.45;
        session.fog.wind = [0.0; 2];
        session.fog.decay_enabled = false;
        if scene == "sparse" {
            session.fog.noise_amplitude = 0.5;
            session.fog.noise_frequency = 0.12;
            session.fog.height_offset = -0.38;
        }
        if scene == "dense" || scene == "rough_dense" {
            session.fog.base_height = 4.0;
            session.fog.noise_amplitude = if scene == "dense" { 0.0 } else { 0.25 };
            session.fog.height_offset = 0.0;
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        sim.encode(gpu, &mut encoder, &session.fog, 0.0, true, &[])?;
        gpu.queue.submit([encoder.finish()]);
        for pitch in [0.05, 0.3, 0.9] {
            session.camera[1] = pitch;
            let mut cases = vec![
                (Reconstruction::Bilinear, false),
                (Reconstruction::PreSmoothed, false),
            ];
            if scene == "default" {
                cases.extend([
                    (Reconstruction::CubicBSpline, false),
                    (Reconstruction::CatmullRom, false),
                    (Reconstruction::Bilinear, true),
                    (Reconstruction::SmoothStep, true),
                    (Reconstruction::CubicBSpline, true),
                    (Reconstruction::CatmullRom, true),
                    (Reconstruction::PreSmoothed, true),
                ]);
            }
            for (mode, detail) in cases {
                session.reconstruction = mode;
                session.reconstruction_detail = detail;
                let mut samples = Vec::new();
                for frame in 0..61 {
                    let mut encoder = gpu.device.create_command_encoder(&Default::default());
                    sim.encode(gpu, &mut encoder, &session.fog, 0.0, false, &[])?;
                    mark(&mut encoder, 0);
                    let actual = renderer.prepare_field(gpu, &mut encoder, &session);
                    mark(&mut encoder, 1);
                    raster.encode(
                        gpu,
                        &mut encoder,
                        &view,
                        &session,
                        &renderer.collision_source,
                        sim.domain,
                        size,
                        actual,
                        Some(&queries),
                    );
                    encoder.resolve_query_set(&queries, 0..12, &results, 0);
                    gpu.queue.submit([encoder.finish()]);
                    let bytes = gpu.read_buffer(&results, 96)?;
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
                        samples.push([
                            ms(0, 1),
                            ms(8, 9),
                            ms(10, 11),
                            ms(2, 3),
                            ms(4, 5),
                            ms(6, 7),
                            ms(0, 7),
                        ]);
                    }
                }
                let median: [f64; 7] = std::array::from_fn(|axis| {
                    let mut values: Vec<_> = samples.iter().map(|v| v[axis]).collect();
                    values.sort_by(f64::total_cmp);
                    values[values.len() / 2]
                });
                let mut total: Vec<_> = samples.iter().map(|v| v[6]).collect();
                total.sort_by(f64::total_cmp);
                let p95 = total[(total.len() * 95).div_ceil(100) - 1];
                eprintln!(
                    "Fog raster scene={scene} {mode:?} detail={detail} pitch={pitch:.2}: filter={:.4} prepare={:.4} project={:.4} scene={:.4} optical={:.4} composite={:.4} total={:.4} p95={p95:.4} budget_2ms={} GPU ms",
                    median[0],
                    median[1],
                    median[2],
                    median[3],
                    median[4],
                    median[5],
                    median[6],
                    if p95 <= 2.0 { "PASS" } else { "OVER" }
                );
            }
        }
    }
    Ok(())
}

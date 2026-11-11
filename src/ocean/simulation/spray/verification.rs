//! Real GPU source/consumer boundary checks. Readback is confined to verification.
use super::*;
use crate::ocean::camera::Camera;
use crate::ocean::camera::CameraUniform;
use crate::ocean::parameters::ocean::OceanConfig;
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::render::Renderer;
use crate::ocean::simulation::offshore::foam::Foam;
use crate::ocean::simulation::spectral::FftWaves as Ocean;

fn floats(gpu: &Gpu, buffer: &wgpu::Buffer) -> Result<Vec<f32>, Error> {
    let bytes = gpu.read_buffer(buffer, buffer.size())?;
    Ok(bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}
fn count(gpu: &Gpu, particles: &SprayParticles) -> Result<u32, Error> {
    let bytes = gpu.read_buffer(&particles.indirect, 16)?;
    let v: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    if v[0] != 6 || v[1] > CAPACITY || v[2] != 0 || v[3] != 0 {
        return Err(format!("spray invalid indirect: {v:?}").into());
    }
    Ok(v[1])
}
fn submit(gpu: &Gpu, run: impl FnOnce(&mut wgpu::CommandEncoder)) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    run(&mut encoder);
    gpu.queue.submit([encoder.finish()]);
}

pub fn verify(gpu: &Gpu) -> Result<(), Error> {
    let ocean = Ocean::new(
        gpu,
        OceanConfig {
            resolution: 32,
            ..Default::default()
        },
    )?;
    let shore = Shore::with_resolution(gpu, &ocean, 64);
    let mut optics = OpticsConfig::beach_scene();
    optics.spray.enabled = true;
    let n = shore.resolution as usize;
    let cells = n * n;
    let old = storage(
        &gpu.device,
        "spray verification previous",
        shore.field.size(),
        wgpu::BufferUsages::empty(),
    );
    let mut source = SpraySource::new(gpu, &shore, &old)?;
    let mut p = shore.parameters(&optics, 0.0, false, 1.0);
    // A wholly wet patch with no FFT blend, one meter square cells.
    p.domain = [-32.0, -32.0, 64.0, 64.0];
    p.terrain = [1.0, 0.0, 0.0, 1.0];
    p.options[3] = 0.0;
    p.waves[2] = 5.0;
    let config = SprayConfig {
        enabled: true,
        rate: 10.0,
        slope: 0.0,
        rise: 0.0,
        depth: 0.001,
        ..Default::default()
    };
    let center = 32 * n + 32;
    // Independently prescribed fields isolate source gates and area/time integration.
    for (name, slope, rise, depth, expected) in [
        ("still", 0.0, 0.0, 3.0, false),
        ("dry", 0.4, 1.0, 0.0, false),
        ("flat retreat", 0.0, -1.0, 3.0, false),
        ("rising front", 0.4, 1.0, 3.0, true),
    ] {
        for (pieces, reverse) in [(1, true), (4, true), (1, false), (4, false)] {
            let dt = 0.04 / pieces as f32;
            let mut current = vec![[0.0f32; 4]; cells * 5];
            let mut previous = current.clone();
            for z in 0..n {
                for x in 0..n {
                    let i = z * n + x;
                    let h = if depth == 0.0 {
                        0.0
                    } else {
                        depth + slope * (x as f32 - 32.0).max(-5.0)
                    };
                    current[i] = [100.0, 0.0, h, 1.0]; // Large foam density must not emit by itself.
                    current[cells + i] = [h, h, 0.0, 100.0];
                    current[2 * cells + i] = [0.0, 0.0, h - 1.0, 0.0];
                    previous[cells + i] = [h - rise * dt, h, 0.0, 100.0];
                }
            }
            let (current_buffer, previous_buffer) = if reverse {
                (&old, &shore.field)
            } else {
                (&shore.field, &old)
            };
            gpu.queue
                .write_buffer(current_buffer, 0, bytemuck::cast_slice(&current));
            gpu.queue
                .write_buffer(previous_buffer, 0, bytemuck::cast_slice(&previous));
            p.timing[1] = dt;
            gpu.queue
                .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
            source.configure(gpu, config, 0.04, 1.0);
            submit(gpu, |e| {
                source.begin_frame(e);
                for _ in 0..pieces {
                    source.encode_substep_direction(e, reverse, None);
                }
            });
            let result = floats(gpu, &source.field)?;
            if result.iter().any(|x| !x.is_finite()) {
                return Err("non-finite spray signal".into());
            }
            if expected {
                let target = [0.2, 0.8, 0.2, 0.4, 0.6, 0.8, 0.0, 0.0];
                if result[center * 8..center * 8 + 8]
                    .iter()
                    .zip(target)
                    .any(|(a, b)| (a - b).abs() > 0.00001)
                {
                    return Err("spray weighted world position/velocity ABI mismatch".into());
                }
            }
            let amount = result[center * 8 + 3];
            let target = if expected { 0.4 } else { 0.0 };
            if (amount - target).abs() > 0.00001 {
                return Err(format!(
                    "spray source {name}/{pieces}/reverse={reverse}: {amount} != {target}"
                )
                .into());
            }
        }
    }
    // Vary physical cell area without changing prescribed center slope/rise plateaus.
    p.domain[2] = 128.0;
    p.domain[3] = 128.0;
    gpu.queue
        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
    submit(gpu, |e| {
        source.begin_frame(e);
        source.encode_substep(e, None);
    });
    if (floats(gpu, &source.field)?[center * 8 + 3] - 0.4).abs() > 0.00001 {
        return Err("spray area normalization".into());
    }
    p.domain = [-32.0, -32.0, 64.0, 64.0];
    // Flat water for consumer collision; source is an independent GPU ABI input.
    let mut field = vec![[0.0f32; 4]; cells * 5];
    for i in 0..cells {
        field[i] = [0.0, 0.0, 1.0, 0.0];
        field[cells + i] = [1.0, 0.0, 0.0, 0.0];
    }
    gpu.queue
        .write_buffer(&shore.field, 0, bytemuck::cast_slice(&field));
    gpu.queue
        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
    let camera = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(
                &Camera::beach(ocean.config.length).uniform(ocean.config, 1.0),
            ),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let depth = gpu
        .device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 16,
                height: 16,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let mut particles = SprayParticles::new(
        gpu,
        &shore,
        &source,
        &camera,
        &depth,
        wgpu::TextureFormat::Rgba8Unorm,
    )?;
    let signal = |amount: f32| -> [f32; 8] {
        [
            0.5 * amount,
            0.0,
            0.5 * amount,
            amount,
            0.0,
            8.0 * amount,
            0.0,
            0.0,
        ]
    };
    for steps in [1, 8] {
        source.configure(gpu, config, 0.0, 1.0);
        submit(gpu, |e| {
            particles.encode(e, &source, particles.epoch + 1);
            source.begin_frame(e);
        });
        source.configure(
            gpu,
            SprayConfig {
                lifetime: 5.0,
                ..config
            },
            0.008 / steps as f32,
            1.0,
        );
        gpu.queue.write_buffer(
            &source.field,
            (center * 32) as u64,
            bytemuck::cast_slice(&signal(2.5 / steps as f32)),
        );
        for _ in 0..steps {
            submit(gpu, |e| particles.encode(e, &source, particles.epoch));
        }
        if count(gpu, &particles)? != 2 {
            return Err(format!("spray fractional emission / {steps}").into());
        }
        let fractions = floats(gpu, &particles.remainder)?;
        if (fractions[center * 2] - 0.5).abs() > 0.00001 {
            return Err("spray fraction lost".into());
        }
    }
    // Verify configured size bounds on particles actually born through the GPU consumer.
    for range in [[0.5, 0.5], [0.25, 1.75]] {
        source.configure(gpu, config, 0.0, 1.0);
        submit(gpu, |e| {
            particles.encode(e, &source, particles.epoch + 1);
            source.begin_frame(e);
        });
        source.configure(
            gpu,
            SprayConfig {
                size: 2.0,
                size_range: range,
                ..config
            },
            0.001,
            1.0,
        );
        gpu.queue.write_buffer(
            &source.field,
            (center * 32) as u64,
            bytemuck::cast_slice(&signal(64.0)),
        );
        submit(gpu, |e| particles.encode(e, &source, particles.epoch));
        if count(gpu, &particles)? != 64 {
            return Err("spray size check missing births".into());
        }
        let bytes = gpu.read_buffer(&particles.lists[particles.front], 16 + 64 * 48)?;
        let sizes: Vec<f32> = bytes[16..]
            .as_chunks::<48>()
            .0
            .iter()
            .map(|p| f32::from_le_bytes(p[32..36].try_into().unwrap()))
            .collect();
        if sizes
            .iter()
            .any(|s| !s.is_finite() || *s < 2.0 * range[0] - 1e-6 || *s > 2.0 * range[1] + 1e-6)
        {
            return Err(format!("GPU spray size outside configured range {range:?}").into());
        }
        let spread = sizes.iter().copied().fold(f32::NEG_INFINITY, f32::max)
            - sizes.iter().copied().fold(f32::INFINITY, f32::min);
        if range[0] < range[1] && spread < 0.1 {
            return Err("GPU spray random size collapsed to a constant".into());
        }
    }
    // Pause must neither respawn nor move: compare actual particle buffer, not code paths.
    let before = gpu.read_buffer(
        &particles.lists[particles.front],
        particles.lists[particles.front].size(),
    )?;
    source.configure(gpu, config, 0.0, 1.0);
    submit(gpu, |e| particles.encode(e, &source, particles.epoch));
    let after = gpu.read_buffer(
        &particles.lists[particles.front],
        particles.lists[particles.front].size(),
    )?;
    if before != after {
        return Err("paused spray changed".into());
    }
    // Saturate, keep surviving occupants, then expire and ensure no backlog is emitted.
    source.configure(gpu, config, 0.0, 1.0);
    submit(gpu, |e| {
        particles.encode(e, &source, particles.epoch + 1);
        source.begin_frame(e);
    });
    source.configure(
        gpu,
        SprayConfig {
            lifetime: 0.05,
            ..config
        },
        0.001,
        1.0,
    );
    gpu.queue.write_buffer(
        &source.field,
        (center * 32) as u64,
        bytemuck::cast_slice(&signal(CAPACITY as f32 * 2.0)),
    );
    submit(gpu, |e| particles.encode(e, &source, particles.epoch));
    if count(gpu, &particles)? != CAPACITY {
        return Err("spray pool failed to saturate".into());
    }
    submit(gpu, |e| particles.encode(e, &source, particles.epoch));
    if count(gpu, &particles)? != CAPACITY {
        return Err("spray full pool corrupted survivors".into());
    }
    let survivors = gpu.read_buffer(
        &particles.lists[particles.front],
        particles.lists[particles.front].size(),
    )?;
    if survivors[16..]
        .as_chunks::<48>()
        .0
        .iter()
        .any(|p| f32::from_le_bytes(p[12..16].try_into().unwrap()) < 0.0009)
    {
        return Err("full spray pool replaced a survivor with a newborn".into());
    }
    source.configure(gpu, config, 0.1, 1.0);
    submit(gpu, |e| {
        source.begin_frame(e);
        particles.encode(e, &source, particles.epoch);
    });
    if count(gpu, &particles)? != 0 {
        return Err("spray expired pool retained particles/backlog".into());
    }
    gpu.queue.write_buffer(
        &source.field,
        (center * 32) as u64,
        bytemuck::cast_slice(&signal(1.0)),
    );
    submit(gpu, |e| particles.encode(e, &source, particles.epoch));
    if count(gpu, &particles)? != 1 {
        return Err("spray capacity not reusable".into());
    }
    verify_raster(gpu, &mut particles, &source)?;
    // Force a downward particle through the water in one frame, exercising swept substeps.
    let mut falling = [0.0f32; 12];
    falling[0] = 0.5;
    falling[1] = 0.1;
    falling[2] = 0.5;
    falling[5] = -20.0;
    falling[7] = 1.0;
    gpu.queue.write_buffer(
        &particles.lists[particles.front],
        16,
        bytemuck::cast_slice(&falling),
    );
    submit(gpu, |e| {
        source.begin_frame(e);
        particles.encode(e, &source, particles.epoch);
    });
    if count(gpu, &particles)? != 0 {
        return Err("spray tunneled through water".into());
    }
    verify_integration(gpu, &ocean)?;
    println!(
        "GPU spray: source gates/area/time, fractional emission, pool saturation/reuse, pause/reset and water collision OK"
    );
    Ok(())
}

// Check real Shore scheduling against the source contract using the observable
// pre/post states of a single completed step, not a second GPU source dispatch.
fn verify_completed_step(
    gpu: &Gpu,
    renderer: &mut Renderer,
    optics: &OpticsConfig,
) -> Result<(), Error> {
    let before = floats(gpu, &renderer.shore.field)?;
    let time = 4.0 / 60.0 + 0.0001;
    let params = renderer.shore.parameters(optics, time, false, 1.0);
    submit(gpu, |e| {
        renderer.shore.encode(gpu, e, optics, time, false, 1.0);
        renderer.encode_spray(gpu, e);
    });
    if renderer.shore.substeps != 1 {
        return Err("spray stage check needs one completed substep".into());
    }
    let dt = f64::from(renderer.shore.simulated_dt);
    let after = floats(gpu, &renderer.shore.field)?;
    let source = renderer
        .shore
        .spray
        .as_ref()
        .ok_or("missing spray source")?;
    let signals = floats(gpu, &source.field)?;
    let n = renderer.shore.resolution as usize;
    let cells = n * n;
    let dx = f64::from(params.domain[2]) / n as f64;
    let dz = f64::from(params.domain[3]) / n as f64;
    let smooth = |v: f64, a: f64, w: f64| {
        let t = ((v - a) / w).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let c = optics.spray;
    let mut total = 0.0;
    for z in 1..n - 1 {
        for x in 1..n - 1 {
            let world_x = f64::from(params.domain[0]) + (x as f64 + 0.5) * dx;
            // Only pure-SWE beach cells, so the domain gate is exactly one.
            if f64::from(optics.beach_slope) * (world_x - f64::from(optics.shore_x))
                > f64::from(optics.shore.breaking_depth)
            {
                continue;
            }
            let i = z * n + x;
            let h = f64::from(after[4 * (cells + i)]);
            if h <= f64::from(c.depth) {
                continue;
            }
            let eta = |j: usize| f64::from(after[4 * (2 * cells + j) + 2]);
            let slope = ((eta(i + 1) - eta(i - 1)) / (2.0 * dx))
                .hypot((eta(i + n) - eta(i - n)) / (2.0 * dz));
            let rise = ((h - f64::from(before[4 * (cells + i)])) / dt).max(0.0);
            let expected = f64::from(c.rate)
                * dx
                * dz
                * dt
                * smooth(slope, f64::from(c.slope), 0.1)
                * smooth(rise, f64::from(c.rise), 0.2)
                * smooth(h, f64::from(c.depth), f64::from(c.depth));
            let actual = f64::from(signals[i * 8 + 3]);
            if (actual - expected).abs() > 1e-6 + expected * 0.001 {
                return Err(format!(
                    "spray order {} stage integration {actual} != {expected}",
                    optics.shore.order
                )
                .into());
            }
            total += expected;
        }
    }
    if total <= 0.00001 {
        return Err("spray stage check had no positive source".into());
    }
    Ok(())
}

// Render a known droplet against planes at independently projected depths. This
// owns soft-depth/blending/resize coverage; simulation tests above own birth counts.
fn verify_raster(
    gpu: &Gpu,
    particles: &mut SprayParticles,
    source: &SpraySource,
) -> Result<(), Error> {
    use glam::{Mat4, Vec3, Vec4};
    let center = Vec3::new(0.5, 0.075, 0.5);
    let eye = center + Vec3::new(0.0, 0.0, 3.0);
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    let projection = Mat4::perspective_rh(0.8, 1.0, 0.1, 10.0) * view;
    let camera = CameraUniform {
        view_projection: projection.to_cols_array_2d(),
        inverse_view_projection: projection.inverse().to_cols_array_2d(),
        eye: eye.extend(1.0).to_array(),
        sun: [0.0, 1.0, 0.0, 0.0],
        ocean: [0.0; 4],
        misc: [0.0; 4],
    };
    let uniform = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&camera),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let mut particle = [
        0.5f32, 0.075, 0.5, 0.25, 0.0, 0.0, 0.0, 1.0, 1.0, 0.7, 0.0, 0.0,
    ];
    let mut rendered_variants = std::collections::BTreeSet::new();
    for (size, layer) in [32, 64]
        .into_iter()
        .flat_map(|size| (0..4).map(move |layer| (size, layer)))
    {
        particle[10] = layer as f32;
        gpu.queue.write_buffer(
            &particles.lists[particles.front],
            16,
            bytemuck::cast_slice(&particle),
        );
        let extent = wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        };
        let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let target_view = target.create_view(&Default::default());
        let depth = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        particles.resize(gpu, source, &uniform, &depth_view);
        let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(size) * 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut energy = Vec::new();
        for distance in [2.9f32, 3.03, 4.0] {
            let clip = projection * Vec4::new(center.x, center.y, eye.z - distance, 1.0);
            submit(gpu, |encoder| {
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: None,
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &target_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(clip.z / clip.w),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        }),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                }
                particles.render(encoder, &target_view, source);
                encoder.copy_texture_to_buffer(
                    target.as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &output,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(256),
                            rows_per_image: Some(size),
                        },
                    },
                    extent,
                );
            });
            let bytes = gpu.read_buffer(&output, output.size())?;
            let sum: u64 = bytes
                .chunks(256)
                .map(|row| {
                    row[..size as usize * 4]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|px| u64::from(px[0]) + u64::from(px[1]) + u64::from(px[2]))
                        .sum::<u64>()
                })
                .sum();
            if distance == 4.0 && !rendered_variants.insert(bytes) {
                return Err(format!(
                    "spray texture layers rendered identically at size {size}, layer {layer}"
                )
                .into());
            }
            energy.push(sum);
        }
        if energy[0] != 0 || energy[1] == 0 || energy[2] < energy[1] * 3 {
            return Err(
                format!("spray soft-depth raster {size}, layer {layer}: {energy:?}").into(),
            );
        }
    }
    Ok(())
}

fn verify_integration(gpu: &Gpu, ocean: &Ocean) -> Result<(), Error> {
    let foam = Foam::new(gpu, &ocean.fields, ocean.config.resolution);
    for order in [1, 2] {
        let mut optics = OpticsConfig::beach_scene();
        optics.shore.order = order;
        optics.shore.resolution = 64;
        optics.spray.enabled = true;
        let mut renderer =
            Renderer::new(gpu, ocean, &foam, wgpu::TextureFormat::Rgba8Unorm, 32, 32);
        renderer.set_shore_resolution(gpu, ocean, 64, wgpu::TextureFormat::Rgba8Unorm, [32, 32]);
        let mut plain = Shore::with_resolution(gpu, ocean, 64);
        let mut without = optics;
        without.spray.enabled = false;
        for frame in 0..5 {
            let t = frame as f32 / 60.0;
            submit(gpu, |e| {
                ocean.encode_filtered(gpu, e, t, &optics.shore);
                renderer.shore.encode(gpu, e, &optics, t, frame == 0, 1.0);
                renderer.encode_spray(gpu, e);
                plain.encode(gpu, e, &without, t, frame == 0, 1.0);
            });
        }
        if let Some(error) = &renderer.shore.spray_error {
            return Err(error.clone().into());
        }
        let a = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
        let b = gpu.read_buffer(&plain.field, plain.field.size())?;
        if a != b {
            return Err(format!("spray changed SWE order {order}").into());
        }
        verify_completed_step(gpu, &mut renderer, &optics)?;
        // Paused reset must clear even if no simulation dt is advanced.
        submit(gpu, |e| {
            renderer
                .shore
                .encode(gpu, e, &optics, 4.0 / 60.0 + 0.0001, true, 1.0);
            renderer.encode_spray(gpu, e);
        });
        // No sources survive a reset, including RK predictor initialization.
        let s = renderer
            .shore
            .spray
            .as_ref()
            .ok_or("spray integration failed to allocate")?;
        if floats(gpu, &s.field)?.iter().any(|x| *x != 0.0) {
            return Err("spray reset leaked source".into());
        }
        optics.spray.enabled = false;
        submit(gpu, |e| {
            renderer
                .shore
                .encode(gpu, e, &optics, 4.0 / 60.0 + 0.0001, false, 1.0);
            renderer.encode_spray(gpu, e);
        });
        if renderer.shore.spray.is_some() {
            return Err("disabled spray source retained".into());
        }
        optics.spray.enabled = true;
        submit(gpu, |e| {
            renderer
                .shore
                .encode(gpu, e, &optics, 4.0 / 60.0 + 0.0001, false, 1.0);
            renderer.encode_spray(gpu, e);
        });
        renderer.resize(&gpu.device, 48, 24);
        let target = gpu
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 48,
                    height: 24,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let camera: CameraUniform = Camera::beach(ocean.config.length).uniform(ocean.config, 2.0);
        submit(gpu, |e| {
            renderer.encode_spray(gpu, e);
            renderer.encode(gpu, e, &target, &camera);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocean::settings::SavedSettings;
    #[test]
    fn legacy_sessions_keep_optics_and_spray_round_trips_with_validation() {
        let mut saved: SavedSettings =
            serde_json::from_str(r#"{"optics":{"depth":9.0,"shore":{"generation":2.5}}}"#).unwrap();
        assert_eq!(saved.optics.depth, 9.0);
        assert_eq!(saved.optics.shore.generation, 2.5);
        assert!(!saved.optics.spray.enabled);
        let legacy: SprayConfig = serde_json::from_str(r#"{"size":0.8}"#).unwrap();
        assert_eq!(legacy.size, 0.8);
        assert_eq!(legacy.size_range, [0.65, 1.35]);
        saved.optics.spray = SprayConfig {
            enabled: true,
            rate: 321.0,
            size: 2.0,
            size_range: [0.25, 1.75],
            debug: true,
            ..Default::default()
        };
        saved.validate().unwrap();
        let restored: SavedSettings =
            serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
        assert_eq!(restored, saved);
        for bad in [f32::NAN, f32::INFINITY, -1.0, 2001.0] {
            saved.optics.spray.rate = bad;
            assert!(saved.validate().is_err());
        }
        saved.optics.spray = SprayConfig::default();
        saved.optics.spray.lifetime = 0.0;
        assert!(saved.validate().is_err());
        saved.optics.spray = SprayConfig::default();
        saved.optics.spray.depth = 0.0;
        assert!(saved.validate().is_err());
        for range in [
            [1.0, 0.5],
            [-0.1, 1.0],
            [0.5, 2.01],
            [f32::NAN, 1.0],
            [1.0, f32::INFINITY],
        ] {
            let config = SprayConfig {
                size_range: range,
                ..Default::default()
            };
            assert!(config.validate().is_err());
        }
        assert!(
            SprayConfig {
                size: 2.01,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        for range in [[0.0, 0.0], [1.0, 1.0], [0.0, 2.0]] {
            SprayConfig {
                size_range: range,
                ..Default::default()
            }
            .validate()
            .unwrap();
        }
    }
}

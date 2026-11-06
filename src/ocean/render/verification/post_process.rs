//! Actual GPU proof: HDR survives until mapping, Bloom spreads only highlights,
//! disabled/debug paths preserve color, and resize handles odd/tiny rectangles.
//! Color charts cover neutral grays, colored highlights and final sRGB encoding.
use crate::ocean::{
    Error,
    gpu::context::{Gpu, buffer},
    parameters::post_process::PostProcessConfig,
    render::post_process::{HDR_FORMAT, PostProcess},
};

fn read_image(
    gpu: &Gpu,
    mut encoder: wgpu::CommandEncoder,
    output: &wgpu::Texture,
) -> Result<Vec<[f32; 4]>, Error> {
    let float = output.format() == wgpu::TextureFormat::Rgba32Float;
    let bytes_per_pixel = if float { 16 } else { 4 };
    let row_bytes = output.width() * bytes_per_pixel;
    let pitch = row_bytes.div_ceil(256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("post-process verification readback"),
        size: u64::from(pitch) * u64::from(output.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(output.height()),
            },
        },
        output.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.map_buffer(&readback)?;
    Ok(bytes
        .chunks_exact(pitch as usize)
        .flat_map(|row| {
            row[..row_bytes as usize]
                .chunks_exact(bytes_per_pixel as usize)
                .map(|pixel| {
                    if float {
                        bytemuck::pod_read_unaligned(pixel)
                    } else {
                        [pixel[0], pixel[1], pixel[2], pixel[3]].map(|v| f32::from(v) / 255.0)
                    }
                })
        })
        .collect())
}

fn verify_color_mapping(gpu: &Gpu) -> Result<(), Error> {
    // Reference patches evaluated independently in f64 from Godot 4.6's
    // AgX/allenwp transform at its default contrast 1.25 and white 16.29.
    // Do not derive expected values from the production WGSL or its constants.
    let references: [([f32; 3], [f32; 3]); 9] = [
        ([0.18; 3], [0.18; 3]),
        ([1.0; 3], [0.596111; 3]),
        ([0.02, 0.10, 0.20], [0.013887, 0.103689, 0.200414]),
        ([0.40, 0.20, 0.10], [0.364056, 0.197541, 0.101646]),
        ([1.0, 0.20, 0.01], [0.671881, 0.228038, 0.037283]),
        ([4.0, 0.0, 0.0], [1.0, 0.351346, 0.226766]),
        ([0.0, 4.0, 0.0], [0.597722, 0.873649, 0.404093]),
        ([0.0, 0.0, 4.0], [0.195407, 0.377459, 1.0]),
        ([8.0, 4.0, 2.0], [0.956435, 0.867307, 0.788778]),
    ];
    let mut patches: Vec<[f32; 4]> = references
        .iter()
        .map(|(c, _)| [c[0], c[1], c[2], 1.0])
        .collect();
    let gray_start = patches.len();
    patches.push([0.0, 0.0, 0.0, 1.0]);
    for ev in -14..=14 {
        let v = 2.0_f32.powi(ev);
        patches.push([v, v, v, 1.0]);
    }
    let gray_end = patches.len();
    // All six primary/secondary directions must tend toward neutral white at
    // extreme exposure, not remain neon or clip to a different saturated hue.
    for color in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 0.0],
        [1.0, 0.0, 1.0],
        [0.0, 1.0, 1.0],
    ] {
        patches.push([
            60000.0 * color[0],
            60000.0 * color[1],
            60000.0 * color[2],
            1.0,
        ]);
    }
    let input = buffer(
        &gpu.device,
        "HDR color chart",
        &patches,
        wgpu::BufferUsages::STORAGE,
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HDR color chart"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/post_process_fixture.wgsl").into(),
            ),
        });
    let fixture = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("HDR color chart"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("color_chart"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("HDR color chart"),
        layout: &fixture.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: input.as_entire_binding(),
        }],
    });
    let width = patches.len() as u32;
    let mut linear_images = Vec::new();
    for format in [
        wgpu::TextureFormat::Rgba32Float,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    ] {
        let post = PostProcess::new(gpu, format, width, 1);
        let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tone mapping output"),
            size: wgpu::Extent3d {
                width,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        for (step, exposure) in [-8.0, 0.0, 8.0].into_iter().enumerate() {
            post.configure(
                PostProcessConfig {
                    enabled: true,
                    strength: 0.0,
                    exposure,
                    ..Default::default()
                },
                false,
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("HDR color chart"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &post.scene,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&fixture);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
            post.encode(
                gpu,
                &mut encoder,
                &output.create_view(&Default::default()),
                false,
            );
            let pixels = read_image(gpu, encoder, &output)?;
            if pixels
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(format!("AgX invalid output at exposure {exposure}").into());
            }
            if format == wgpu::TextureFormat::Rgba32Float {
                if exposure == 0.0 {
                    for (i, (_, expected)) in references.iter().enumerate() {
                        for (value, reference) in pixels[i][..3].iter().zip(expected) {
                            if (value - reference).abs() > 0.001 {
                                return Err(format!(
                                    "AgX color/gray regression patch {i}: {:?} != {expected:?}",
                                    pixels[i]
                                )
                                .into());
                            }
                        }
                    }
                }
                let grays = &pixels[gray_start..gray_end];
                if grays[0] != [0.0, 0.0, 0.0, 1.0]
                    || grays
                        .iter()
                        .any(|p| (p[0] - p[1]).abs() > 2e-6 || (p[1] - p[2]).abs() > 2e-6)
                    || grays.windows(2).any(|p| p[1][0] < p[0][0] - 1e-6)
                    || pixels[gray_end..]
                        .iter()
                        .any(|p| p[..3].iter().any(|v| *v < 0.999))
                {
                    return Err(format!(
                        "AgX neutral axis or highlight desaturation failed at {exposure}"
                    )
                    .into());
                }
                linear_images.push(pixels);
            } else {
                for (pixel, linear) in pixels.iter().zip(&linear_images[step]) {
                    for (value, linear) in pixel[..3].iter().zip(&linear[..3]) {
                        let encoded = if *linear <= 0.0031308 {
                            12.92 * linear
                        } else {
                            1.055 * linear.powf(1.0 / 2.4) - 0.055
                        };
                        if (value - encoded).abs() > 1.5 / 255.0 {
                            return Err("AgX output has missing or duplicate sRGB encoding".into());
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "AgX GPU: reference colors, neutral/monotone gray ramp, bright RGB/CMY desaturation, exposure -8/0/+8 and single sRGB encoding passed"
    );
    Ok(())
}

pub fn run(gpu: &Gpu) -> Result<(), Error> {
    verify_color_mapping(gpu)?;
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HDR highlight fixture"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/post_process_fixture.wgsl").into(),
            ),
        });
    let fixture = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("HDR highlight fixture"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
    let mut post = PostProcess::new(gpu, wgpu::TextureFormat::Rgba32Float, 65, 49);
    for [width, height] in [[65, 49], [9, 7], [1, 13], [13, 1], [1, 1]] {
        post.resize(&gpu.device, width, height);
        let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Bloom verification output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let image = |config, debug, camera_debug, highlight| -> Result<Vec<[f32; 4]>, Error> {
            post.configure(config, debug);
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Bloom verification fixture"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &post.scene,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.125,
                            g: 0.125,
                            b: 0.125,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if highlight {
                let (w, h) = (width.min(3), height.min(3));
                pass.set_scissor_rect((width - w) / 2, (height - h) / 2, w, h);
                pass.set_pipeline(&fixture);
                pass.draw(0..3, 0..1);
            }
            drop(pass);
            post.encode(
                gpu,
                &mut encoder,
                &output.create_view(&Default::default()),
                camera_debug,
            );
            let pixels = read_image(gpu, encoder, &output)?;
            if pixels.iter().flatten().any(|v| !v.is_finite()) {
                return Err(format!("Bloom nonfinite output at {width}x{height}").into());
            }
            Ok(pixels)
        };
        let off = PostProcessConfig::default();
        let on = PostProcessConfig {
            enabled: true,
            strength: 1.0,
            soft_knee: 0.0,
            ..off
        };
        let raw = image(off, false, false, true)?;
        let center = (height / 2 * width + width / 2) as usize;
        if raw[center] != [8.0, 4.0, 2.0, 1.0] {
            return Err("HDR highlight clipped before post-processing".into());
        }
        for (debug, camera_debug) in [(true, false), (false, true)] {
            if image(
                PostProcessConfig {
                    exposure: 2.0,
                    ..on
                },
                debug,
                camera_debug,
                true,
            )? != raw
            {
                return Err("post-processing changed a diagnostic view".into());
            }
        }
        let mapped = image(
            PostProcessConfig {
                strength: 0.0,
                ..on
            },
            false,
            false,
            true,
        )?;
        let bloom = image(on, false, false, true)?;
        if bloom.iter().flatten().any(|v| !(0.0..=1.0).contains(v)) {
            return Err("tone mapping escaped SDR range".into());
        }
        // A bright source must illuminate at least one previously dark neighbor.
        if width * height > 9
            && !raw
                .iter()
                .zip(&mapped)
                .zip(&bloom)
                .any(|((r, m), b)| r[0] < 1.0 && b[0] > m[0] + 0.001)
        {
            return Err(format!("Bloom did not spread HDR highlights at {width}x{height}").into());
        }
        let high_threshold = image(
            PostProcessConfig {
                threshold: 20.0,
                ..on
            },
            false,
            false,
            true,
        )?;
        if high_threshold != mapped {
            return Err("Bloom leaked below a hard threshold".into());
        }
        // Turning Bloom off after a populated pyramid must not retain any glow,
        // and turning mapping off must not silently disable Bloom or exposure.
        if image(
            PostProcessConfig {
                bloom_enabled: false,
                ..on
            },
            false,
            false,
            true,
        )? != mapped
        {
            return Err("Bloom switch retained stale glow or disabled tone mapping".into());
        }
        let linear = PostProcessConfig {
            tonemapping_enabled: false,
            ..on
        };
        if image(
            PostProcessConfig {
                bloom_enabled: false,
                ..linear
            },
            false,
            false,
            true,
        )? != raw
        {
            return Err("independent mapping/Bloom bypass changed HDR color".into());
        }
        let bloom_only = image(linear, false, false, true)?;
        if bloom_only[center][0] < 8.0
            || (width * height > 9
                && !raw
                    .iter()
                    .zip(&bloom_only)
                    .any(|(r, b)| r[0] < 1.0 && b[0] > r[0] + 0.001))
        {
            return Err("disabling mapping also disabled or clipped Bloom".into());
        }
        // Check Bloom/exposure energy without depending on any display curve.
        let dark = image(linear, false, false, false)?;
        if dark.iter().any(|p| *p != [0.125, 0.125, 0.125, 1.0]) {
            return Err("dark field gained Bloom or an extra gamma transform".into());
        }
        let soft = image(
            PostProcessConfig {
                threshold: 0.25,
                soft_knee: 1.0,
                ..linear
            },
            false,
            false,
            false,
        )?;
        if soft[0][0] <= dark[0][0] + 0.005 {
            return Err("soft knee failed to admit near-threshold highlights".into());
        }
        let uniform_bloom = image(
            PostProcessConfig {
                threshold: 0.0,
                ..linear
            },
            false,
            false,
            false,
        )?;
        // Uniform 0.125 + Bloom 0.125 stays 0.25 at every pyramid depth.
        if uniform_bloom.iter().any(|p| (p[0] - 0.25).abs() > 0.0005) {
            return Err("Bloom energy depends on resolution/pyramid depth".into());
        }
        let exposed = image(
            PostProcessConfig {
                exposure: 2.0,
                strength: 0.0,
                ..linear
            },
            false,
            false,
            false,
        )?;
        if exposed.iter().any(|p| (p[0] - 0.5).abs() > 0.0005) {
            return Err("exposure compensation was not applied exactly once".into());
        }
        // Both zero threshold/knee and extreme allowed exposure must stay finite.
        image(
            PostProcessConfig {
                threshold: 0.0,
                soft_knee: 0.0,
                exposure: 8.0,
                strength: 2.0,
                ..on
            },
            false,
            false,
            true,
        )?;
        // Toggle back after populated pyramid: no stale glow may survive.
        if image(off, false, false, true)? != raw {
            return Err("disabled Bloom retained stale history".into());
        }
    }
    eprintln!(
        "HDR/Bloom GPU: independent mapping/Bloom switches, highlight spread, threshold, exposure, debug bypass and resize passed"
    );
    Ok(())
}

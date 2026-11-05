//! Actual GPU proof: HDR survives until mapping, Bloom spreads only highlights,
//! disabled/debug paths preserve color, and resize handles odd/tiny rectangles.
use crate::ocean::{
    Error,
    gpu::context::Gpu,
    parameters::post_process::PostProcessConfig,
    render::post_process::{HDR_FORMAT, PostProcess},
};

pub fn run(gpu: &Gpu) -> Result<(), Error> {
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
        let pitch = (width * 16).div_ceil(256) * 256;
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Bloom verification readback"),
            size: u64::from(pitch * height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
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
            encoder.copy_texture_to_buffer(
                output.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(pitch),
                        rows_per_image: Some(height),
                    },
                },
                output.size(),
            );
            gpu.queue.submit([encoder.finish()]);
            let bytes = gpu.map_buffer(&readback)?;
            let pixels: Vec<[f32; 4]> = bytes
                .chunks_exact(pitch as usize)
                .flat_map(|row| {
                    row[..width as usize * 16]
                        .as_chunks::<16>()
                        .0
                        .iter()
                        .map(|b| bytemuck::pod_read_unaligned(b))
                })
                .collect();
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
        let dark = image(on, false, false, false)?;
        // ACES approximation maps linear 0.125 to about 0.171, without gamma here.
        if dark
            .iter()
            .any(|p| (p[0] - 0.1707).abs() > 0.001 || p[0] != p[1] || p[1] != p[2])
        {
            return Err("dark field gained Bloom or an extra gamma transform".into());
        }
        let soft = image(
            PostProcessConfig {
                threshold: 0.25,
                soft_knee: 1.0,
                ..on
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
                ..on
            },
            false,
            false,
            false,
        )?;
        // Uniform 0.125 + Bloom 0.125 maps to about 0.3741 at every pyramid depth.
        if uniform_bloom.iter().any(|p| (p[0] - 0.3741).abs() > 0.001) {
            return Err("Bloom energy depends on resolution/pyramid depth".into());
        }
        let exposed = image(
            PostProcessConfig {
                exposure: 2.0,
                strength: 0.0,
                ..on
            },
            false,
            false,
            false,
        )?;
        if exposed[0][0] <= dark[0][0] {
            return Err("exposure compensation did not increase luminance".into());
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
        "HDR/Bloom GPU: highlight spread, threshold, exposure, debug bypass and resize passed"
    );
    Ok(())
}

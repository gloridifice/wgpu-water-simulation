//! Bundled sand maps; color mips use the GPU, data maps retain CPU filtering.
use crate::ocean::gpu::context::Gpu;

#[derive(Clone, Copy)]
enum Map {
    Color,
    Normal,
    Scalar,
}

pub(in crate::ocean::render) fn textures(gpu: &Gpu) -> [wgpu::TextureView; 4] {
    [
        (
            include_bytes!("../../../../assets/sand/Ground061/Ground061_2K-JPG_Color.jpg")
                .as_slice(),
            Map::Color,
        ),
        (
            include_bytes!("../../../../assets/sand/Ground061/Ground061_2K-JPG_NormalGL.jpg")
                .as_slice(),
            Map::Normal,
        ),
        (
            include_bytes!("../../../../assets/sand/Ground061/Ground061_2K-JPG_Roughness.jpg")
                .as_slice(),
            Map::Scalar,
        ),
        (
            include_bytes!(
                "../../../../assets/sand/Ground061/Ground061_2K-JPG_AmbientOcclusion.jpg"
            )
            .as_slice(),
            Map::Scalar,
        ),
    ]
    .map(|(bytes, map)| upload(gpu, bytes, map).expect("bundled Ground061 texture"))
}

pub(in crate::ocean::render) fn color_texture(
    gpu: &Gpu,
    bytes: &[u8],
) -> Result<wgpu::TextureView, crate::ocean::Error> {
    upload(gpu, bytes, Map::Color)
}

// Hardware sRGB decode/encode keeps each box filter in linear light. Only mip 0
// crosses the CPU/GPU boundary; submission is ordered before subsequent draws.
fn color_mips(gpu: &Gpu, image: &image::RgbaImage) -> wgpu::Texture {
    let (width, height) = image.dimensions();
    let levels = width.max(height).ilog2() + 1;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("color GPU mip chain"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        image.as_raw(),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    if levels == 1 {
        return texture;
    }
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("linear-light color downsample"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/color_mips.wgsl").into()),
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("color mip generation"),
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
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
    let layout = pipeline.get_bind_group_layout(0);
    let views: Vec<_> = (0..levels)
        .map(|level| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: level,
                mip_level_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    for pair in views.windows(2) {
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("previous color mip"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&pair[0]),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("color mip downsample"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &pair[1],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
    }
    gpu.queue.submit([encoder.finish()]);
    texture
}

/// Read back production mip chains: preserve mip 0 and average in linear light,
/// including rectangular chains whose short axis reaches one first.
pub(in crate::ocean) fn verify(gpu: &Gpu) -> Result<(), crate::ocean::Error> {
    for (width, height) in [(8u32, 2u32), (2, 8), (8, 1), (1, 8), (1, 1)] {
        let image = image::RgbaImage::from_fn(width, height, |x, y| {
            let bright = if width >= height {
                x >= width / 2
            } else {
                y >= height / 2
            };
            let value = if bright { 255 } else { 0 };
            image::Rgba([value, value, value, 255])
        });
        let texture = color_mips(gpu, &image);
        for mip in 0..texture.mip_level_count() {
            let w = (width >> mip).max(1);
            let h = (height >> mip).max(1);
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("color mip verification"),
                size: u64::from(h) * 256,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: mip,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(h),
                    },
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
            gpu.queue.submit([encoder.finish()]);
            let bytes = gpu.map_buffer(&buffer)?;
            for y in 0..h {
                for x in 0..w {
                    let expected = if w == 1 && h == 1 && width.max(height) > 1 {
                        188 // sRGB encoding of 50% linear intensity, NOT 128.
                    } else if if width >= height {
                        x >= w / 2
                    } else {
                        y >= h / 2
                    } {
                        255
                    } else {
                        0
                    };
                    let offset = (y * 256 + x * 4) as usize;
                    if bytes[offset..offset + 3]
                        .iter()
                        .any(|&v| (i32::from(v) - expected).abs() > 1)
                        || bytes[offset + 3] != 255
                    {
                        return Err(format!(
                            "color mip mismatch: {width}x{height}, mip {mip}, ({x},{y})"
                        )
                        .into());
                    }
                }
            }
        }
    }
    println!("GPU color mips: base level, linear-light average and rectangular chains OK");
    Ok(())
}

fn upload(gpu: &Gpu, bytes: &[u8], map: Map) -> Result<wgpu::TextureView, crate::ocean::Error> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)?.to_rgb8();
    let (mut width, mut height) = image.dimensions();
    if !width.is_power_of_two()
        || !height.is_power_of_two()
        || width.max(height) > gpu.device.limits().max_texture_dimension_2d
    {
        return Err("texture must be power-of-two and fit device limits".into());
    }
    if matches!(map, Map::Color) {
        let rgba = image::DynamicImage::ImageRgb8(image).into_rgba8();
        return Ok(color_mips(gpu, &rgba).create_view(&Default::default()));
    }
    let scalar = matches!(map, Map::Scalar);
    let format = match map {
        Map::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
        Map::Normal => wgpu::TextureFormat::Rgba8Unorm,
        Map::Scalar => wgpu::TextureFormat::R8Unorm,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Ground061 sand map"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: width.max(height).ilog2() + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // Keep unquantized linear values between mip levels. Normal vectors are
    // averaged before normalization so small-scale directions cancel correctly.
    let mut level: Vec<[f32; 3]> = image
        .pixels()
        .map(|p| {
            p.0.map(|v| {
                let v = v as f32 / 255.0;
                match map {
                    Map::Color => unreachable!("color mips use GPU"),
                    Map::Normal => v * 2.0 - 1.0,
                    Map::Scalar => v,
                }
            })
        })
        .collect();
    let mut mip = 0;
    loop {
        let mut pixels = Vec::with_capacity(level.len() * if scalar { 1 } else { 4 });
        for p in &level {
            let encoded = match map {
                Map::Color => unreachable!("color mips use GPU"),
                Map::Normal => {
                    let n = glam::Vec3::from_array(*p)
                        .try_normalize()
                        .unwrap_or(glam::Vec3::Z);
                    (n * 0.5 + glam::Vec3::splat(0.5)).to_array()
                }
                Map::Scalar => *p,
            };
            let rgb = encoded.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            if scalar {
                pixels.push(rgb[0]);
            } else {
                pixels.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: mip,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * if scalar { 1 } else { 4 }),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        if width == 1 && height == 1 {
            break;
        }
        let nw = (width / 2).max(1);
        let nh = (height / 2).max(1);
        let mut next = vec![[0.0; 3]; (nw * nh) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let p = level[((2 * y + dy).min(height - 1) * width
                            + (2 * x + dx).min(width - 1))
                            as usize];
                        for (value, sample) in next[(y * nw + x) as usize].iter_mut().zip(p) {
                            *value += sample * 0.25;
                        }
                    }
                }
            }
        }
        level = next;
        width = nw;
        height = nh;
        mip += 1;
    }
    Ok(texture.create_view(&Default::default()))
}

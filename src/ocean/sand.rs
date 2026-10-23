//! Bundled sand maps and linear-space mip generation (initialization only).
use super::gpu::Gpu;

#[derive(Clone, Copy)]
enum Map {
    Color,
    Normal,
    Scalar,
}

pub(super) fn textures(gpu: &Gpu) -> [wgpu::TextureView; 4] {
    [
        (
            include_bytes!("../../assets/sand/Ground061/Ground061_2K-JPG_Color.jpg").as_slice(),
            Map::Color,
        ),
        (
            include_bytes!("../../assets/sand/Ground061/Ground061_2K-JPG_NormalGL.jpg").as_slice(),
            Map::Normal,
        ),
        (
            include_bytes!("../../assets/sand/Ground061/Ground061_2K-JPG_Roughness.jpg").as_slice(),
            Map::Scalar,
        ),
        (
            include_bytes!("../../assets/sand/Ground061/Ground061_2K-JPG_AmbientOcclusion.jpg")
                .as_slice(),
            Map::Scalar,
        ),
    ]
    .map(|(bytes, map)| upload(gpu, bytes, map))
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn upload(gpu: &Gpu, bytes: &[u8], map: Map) -> wgpu::TextureView {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .expect("bundled Ground061 JPEG")
        .to_rgb8();
    let (mut width, mut height) = image.dimensions();
    assert!(width.is_power_of_two() && height.is_power_of_two());
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
                    Map::Color => srgb_to_linear(v),
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
                Map::Color => p.map(linear_to_srgb),
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
    texture.create_view(&Default::default())
}

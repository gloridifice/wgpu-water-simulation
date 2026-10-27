//! Linear NSH texture array: tangent normal XY, soft coverage, relative height.
//!
//! Only encoding normalizes mip normals. Keeping their unnormalized averages
//! between levels preserves the contribution of the original normal samples.
use super::gpu::Gpu;

pub(super) struct FoamTextures {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
}

#[derive(Clone, Copy)]
struct Texel {
    normal: glam::Vec3,
    soft: f32,
    height: f32,
}

impl Texel {
    fn decode(rgba: &[u8; 4]) -> Self {
        let xy = glam::Vec2::new(rgba[0] as f32, rgba[1] as f32) / 127.5 - glam::Vec2::ONE;
        let z = (1.0 - xy.length_squared()).max(0.0).sqrt();
        Self {
            normal: xy.extend(z).try_normalize().unwrap_or(glam::Vec3::Z),
            soft: rgba[2] as f32 / 255.0,
            height: rgba[3] as f32 / 255.0,
        }
    }

    fn encode(self) -> [u8; 4] {
        let n = self.normal.try_normalize().unwrap_or(glam::Vec3::Z);
        [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, self.soft, self.height]
            .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

fn decode_png(bytes: &[u8]) -> (u32, u32, Vec<Texel>) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("bundled NSH PNG header");
    let mut pixels = vec![0; reader.output_buffer_size().expect("NSH PNG size")];
    let info = reader
        .next_frame(&mut pixels)
        .expect("bundled NSH PNG pixels");
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    assert_eq!(info.color_type, png::ColorType::Rgba);
    assert!(info.width.is_power_of_two() && info.height.is_power_of_two());
    let texels = pixels[..info.buffer_size()]
        .as_chunks::<4>()
        .0
        .iter()
        .map(Texel::decode)
        .collect();
    (info.width, info.height, texels)
}

fn downsample(texels: &[Texel], width: u32, height: u32) -> (u32, u32, Vec<Texel>) {
    let next_width = (width / 2).max(1);
    let next_height = (height / 2).max(1);
    let mut next = Vec::with_capacity((next_width * next_height) as usize);
    for y in 0..next_height {
        for x in 0..next_width {
            let mut average = Texel {
                normal: glam::Vec3::ZERO,
                soft: 0.0,
                height: 0.0,
            };
            for dy in 0..2 {
                for dx in 0..2 {
                    let sample = texels[((2 * y + dy).min(height - 1) * width
                        + (2 * x + dx).min(width - 1))
                        as usize];
                    average.normal += sample.normal * 0.25;
                    average.soft += sample.soft * 0.25;
                    average.height += sample.height * 0.25;
                }
            }
            next.push(average);
        }
    }
    (next_width, next_height, next)
}

impl FoamTextures {
    pub fn new(gpu: &Gpu) -> Self {
        let sources = [
            include_bytes!("../../assets/foam/baked/shorewaves_nsh.png").as_slice(),
            include_bytes!("../../assets/foam/baked/crest_foam_nsh.png").as_slice(),
            include_bytes!("../../assets/foam/baked/crest_foam2_nsh.png").as_slice(),
        ];
        let mut images = sources.into_iter().map(decode_png);
        let first = images.next().expect("NSH layer zero");
        let (width, height) = (first.0, first.1);
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("foam NSH: Shorewaves, Crest foam, Crest Foam2"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 3,
            },
            mip_level_count: width.max(height).ilog2() + 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (layer, (mut w, mut h, mut texels)) in std::iter::once(first).chain(images).enumerate()
        {
            assert_eq!((w, h), (width, height), "NSH layers must share dimensions");
            let mut mip = 0;
            loop {
                let pixels: Vec<u8> = texels.iter().flat_map(|p| p.encode()).collect();
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: mip,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                if w == 1 && h == 1 {
                    break;
                }
                (w, h, texels) = downsample(&texels, w, h);
                mip += 1;
            }
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("linear foam NSH array"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("foam repeat trilinear"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        Self { view, sampler }
    }
}

#[cfg(test)]
mod tests {
    use super::{Texel, downsample};

    #[test]
    fn opposing_slopes_cancel_without_mixing_soft_and_height() {
        let texels = [
            Texel {
                normal: glam::Vec3::new(0.6, 0.0, 0.8),
                soft: 0.0,
                height: 0.2,
            },
            Texel {
                normal: glam::Vec3::new(-0.6, 0.0, 0.8),
                soft: 0.2,
                height: 0.4,
            },
            Texel {
                normal: glam::Vec3::new(0.0, 0.6, 0.8),
                soft: 0.4,
                height: 0.6,
            },
            Texel {
                normal: glam::Vec3::new(0.0, -0.6, 0.8),
                soft: 0.6,
                height: 0.8,
            },
        ];
        let (w, h, mip) = downsample(&texels, 2, 2);
        assert_eq!((w, h), (1, 1));
        assert!((mip[0].normal - glam::Vec3::new(0.0, 0.0, 0.8)).length() < 1e-6);
        assert!((mip[0].soft - 0.3).abs() < 1e-6);
        assert!((mip[0].height - 0.5).abs() < 1e-6);
        assert_eq!(mip[0].encode(), [128, 128, 77, 128]);
    }

    #[test]
    fn mip_chain_preserves_global_scalar_means() {
        let (mut width, mut height) = (8, 4);
        let mut texels: Vec<_> = (0..width * height)
            .map(|i| Texel {
                normal: glam::Vec3::Z,
                soft: i as f32 / 31.0,
                height: if i % 4 == 0 { 0.8 } else { 0.0 },
            })
            .collect();
        while width > 1 || height > 1 {
            (width, height, texels) = downsample(&texels, width, height);
        }
        assert!((texels[0].soft - 0.5).abs() < 1e-6);
        assert!((texels[0].height - 0.2).abs() < 1e-6);
        assert_eq!(texels[0].encode(), [128, 128, 128, 51]);
    }

    #[test]
    fn decoding_keeps_height_alpha_and_normal_finite() {
        for rgba in [[255, 255, 32, 47], [0, 0, 254, 0], [128, 128, 0, 255]] {
            let texel = Texel::decode(&rgba);
            assert!(texel.normal.is_finite());
            assert!((texel.normal.length() - 1.0).abs() < 1e-6);
            assert!(texel.normal.z >= 0.0);
            assert_eq!(&texel.encode()[2..], &rgba[2..]);
        }
    }
}

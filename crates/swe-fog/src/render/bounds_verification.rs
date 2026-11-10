//! Independent impulse-envelope oracle for odd rectangular hierarchy extents.
use crate::{Error, parameters::Reconstruction};
use common::gpu::{Gpu, buffer};

pub(super) fn run(gpu: &Gpu) -> Result<(), Error> {
    let dims = [13u32, 7u32];
    let field = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("odd support envelope fixture"),
        size: wgpu::Extent3d {
            width: dims[0],
            height: dims[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let bed = buffer(
        &gpu.device,
        "odd bounds bed",
        &vec![[-0.25f32, 1.0, 0.0, 0.0]; 91],
        wgpu::BufferUsages::STORAGE,
    );
    let masks = buffer(
        &gpu.device,
        "odd bounds faces",
        &vec![1u32; 14 * 7 + 13 * 8],
        wgpu::BufferUsages::STORAGE,
    );
    let hierarchy =
        super::bounds::LayerHierarchy::new(gpu, &field, &field, &bed, &masks, dims, true);
    // Same revision, different support; then new publication clearing all fog.
    // The far corner must survive every reduction, without padding creating fog.
    for (revision, mode, amplitude, radius) in [
        (1, Reconstruction::Bilinear, 0.75, 1),
        (1, Reconstruction::CubicBSpline, 0.75, 2),
        (2, Reconstruction::CubicBSpline, 0.0, 2),
    ] {
        let mut values = vec![[0.0f32; 4]; 91];
        values[90] = [amplitude, amplitude * 2.0, 0.0, 0.0];
        gpu.queue.write_texture(
            field.as_image_copy(),
            bytemuck::cast_slice(&values),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(13 * 16),
                rows_per_image: Some(7),
            },
            field.size(),
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        hierarchy.encode(gpu, &mut encoder, revision, mode, 0.6);
        gpu.queue.submit([encoder.finish()]);
        for level in 0..hierarchy.upper.mip_level_count() {
            let width = (hierarchy.upper.width() >> level).max(1);
            let height = (hierarchy.upper.height() >> level).max(1);
            let stride = (width * 16).div_ceil(256) * 256;
            let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("explicit bounds mip readback"),
                size: u64::from(stride * height),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &hierarchy.upper,
                    mip_level: level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride),
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
            let bytes = gpu.map_buffer(&staging)?;
            for z in 0..height {
                for x in 0..width {
                    let offset = (z * stride + x * 16) as usize;
                    let upper: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[offset..offset + 16]);
                    let low = [x << level, z << level];
                    let high = [(x + 1) << level, (z + 1) << level];
                    let valid = low[0] < dims[0] && low[1] < dims[1];
                    // A corner impulse can influence only the last radius+1
                    // leaf columns/rows. Parent support is their geometric union.
                    let touches =
                        valid && high[0] > dims[0] - 1 - radius && high[1] > dims[1] - 1 - radius;
                    let expected = if touches { amplitude } else { 0.0 };
                    let ok = if valid {
                        upper[0] == expected
                            && upper[1] == 2.0 * expected
                            && upper[3] == -0.25 + expected
                    } else {
                        upper[0] <= 0.0 && upper[1] <= 0.0
                    };
                    if !ok {
                        return Err(format!("odd bounds {mode:?} revision={revision} mip={level} ({x},{z}): {upper:?}, expected height {expected}").into());
                    }
                }
            }
        }
    }
    Ok(())
}

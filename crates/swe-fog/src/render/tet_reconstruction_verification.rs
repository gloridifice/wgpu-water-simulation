//! Kernel contracts independent of tetrahedron topology: quadratic moments,
//! bounded step response, and shared vertices on either side of a solid wall.
use crate::{Error, parameters::Reconstruction};
use common::gpu::{Gpu, buffer};
pub(super) fn run(gpu: &Gpu) -> Result<(), Error> {
    let device = &gpu.device;
    let shader=device.create_shader_module(wgpu::ShaderModuleDescriptor {label:Some("vertex kernel contract probes"),source:wgpu::ShaderSource::Wgsl(format!("{}\n{}\n{}\n{}",include_str!("shaders/raster_types.wgsl"),include_str!("shaders/connectivity.wgsl"),include_str!("shaders/raster_prepare.wgsl"),r#"
@group(0) @binding(8) var<storage,read_write> result:array<vec4f>;
@compute @workgroup_size(8)
fn probe(@builtin(global_invocation_id) id:vec3u){
    if id.x>=7u{return;}
    let p=array<vec2f,7>(vec2f(4.25),vec2f(4),vec2f(4),vec2f(4),vec2f(4),vec2f(5.25,4.25),vec2f(2.75,4.25));
    let anchor=array<vec2i,7>(vec2i(4),vec2i(3),vec2i(4),vec2i(3,4),vec2i(4,3),vec2i(5,4),vec2i(2,4));
    result[id.x]=vec4f(vertex_value(p[id.x],anchor[id.x],vec2i(8)),0);
}"#).into())});
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("vertex kernel probes"),
        layout: None,
        module: &shader,
        entry_point: Some("probe"),
        compilation_options: Default::default(),
        cache: None,
    });
    let uniform = buffer(
        device,
        "probe camera ABI",
        &[0f32; 52],
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let bed = buffer(
        device,
        "flat valid bed",
        &[[0f32, 1.0, 0.0, 0.0]; 64],
        wgpu::BufferUsages::STORAGE,
    );
    let masks = buffer(
        device,
        "probe open faces",
        &[1u32; 144],
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    );
    let output = buffer(
        device,
        "vertex probe result",
        &[[0f32; 4]; 7],
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("kernel polynomial and step inputs"),
        size: wgpu::Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vertex probe resources"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: bed.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: masks.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: output.as_entire_binding(),
            },
        ],
    });
    for fixture in 0..3 {
        let pixels: Vec<[f32; 4]> = (0..64)
            .map(|i| {
                let x = i % 8;
                let h = if fixture == 0 {
                    (x * x) as f32
                } else {
                    f32::from(x >= 4)
                };
                [
                    h,
                    if fixture == 0 { 2.0 * h + 3.0 } else { 1.0 - h },
                    0.0,
                    0.0,
                ]
            })
            .collect();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            bytemuck::cast_slice(&pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(128),
                rows_per_image: Some(8),
            },
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        let mut faces = [1u32; 144];
        if fixture == 2 {
            for z in 0..8 {
                faces[z * 9 + 4] = 0;
            }
        }
        gpu.queue
            .write_buffer(&masks, 0, bytemuck::cast_slice(&faces));
        for mode in [
            Reconstruction::Bilinear,
            Reconstruction::SmoothStep,
            Reconstruction::CubicBSpline,
            Reconstruction::CatmullRom,
        ] {
            // RasterCamera's documented 208-byte ABI: player.w selects the kernel.
            let mut camera = [0f32; 52];
            camera[43] = mode as u32 as f32;
            gpu.queue
                .write_buffer(&uniform, 0, bytemuck::cast_slice(&camera));
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            gpu.queue.submit([encoder.finish()]);
            let bytes = gpu.read_buffer(&output, 112)?;
            let values: Vec<f32> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_le_bytes(*b))
                .collect();
            let close = |actual: f32, expected: f32| -> Result<(), Error> {
                if !actual.is_finite() || (actual - expected).abs() > 2e-5 {
                    Err(format!(
                        "vertex kernel {mode:?}, fixture={fixture}: {actual} vs {expected}"
                    )
                    .into())
                } else {
                    Ok(())
                }
            };
            if fixture == 0 {
                // At index coordinate 3.75: Catmull reproduces x², B-spline
                // adds its 1/3 variance; linear/Hermite weights have closed values.
                let expected = match mode {
                    Reconstruction::Bilinear => 14.25,
                    Reconstruction::SmoothStep => 14.90625,
                    Reconstruction::CubicBSpline => 14.0625 + 1.0 / 3.0,
                    Reconstruction::CatmullRom => 14.0625,
                    _ => unreachable!(),
                };
                close(values[0], expected)?;
                close(values[1], 2.0 * expected + 3.0)?;
                for p in 2..5 {
                    close(values[p * 4], values[4])?;
                    close(values[p * 4 + 1], values[5])?;
                }
            } else {
                for p in 0..7 {
                    for axis in 0..2 {
                        let v = values[p * 4 + axis];
                        if !v.is_finite() || !(-1e-6..=1.000001).contains(&v) {
                            return Err(format!("unbounded vertex kernel {mode:?}: {v}").into());
                        }
                    }
                }
                if fixture == 2 {
                    for (p, h) in [1f32, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0].into_iter().enumerate() {
                        close(values[p * 4], h)?;
                        close(values[p * 4 + 1], 1.0 - h)?;
                    }
                }
            }
        }
    }
    eprintln!(
        "Fog vertex kernels: PASS (independent quadratic moments, bounded step response, shared vertices and wall isolation)"
    );
    Ok(())
}

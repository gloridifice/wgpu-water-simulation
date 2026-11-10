//! Production WGSL reconstruction and ray integration checked against independent f64 oracles.
use super::Camera;
use crate::{Error, parameters::Reconstruction};
use common::gpu::{Gpu, buffer};

fn ensure(ok: bool, label: &str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(format!("fog reconstruction verification failed: {label}").into())
    }
}

pub(crate) fn run(gpu: &Gpu) -> Result<(), Error> {
    let device = &gpu.device;
    let dims = [8u32, 6u32];
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("reconstruction fixture"),
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
    let mut terrain = vec![[0.0, 1.0, 0.0, 0.0]; 48];
    for (i, cell) in terrain.iter_mut().enumerate() {
        cell[0] = 0.01 * (i / 8) as f32 + 0.03 * (i % 8) as f32;
    }
    let mut masks = vec![1u32; 9 * 6 + 8 * 7];
    let bed = buffer(
        device,
        "reconstruction fixture bed",
        &terrain,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    );
    let faces = buffer(
        device,
        "reconstruction fixture faces",
        &masks,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    );
    let camera = Camera {
        inverse: [[0.0; 4]; 4],
        eye: [0.0; 4],
        domain: [-8.0, -3.0, 16.0, 6.0],
        player: [0.0; 4],
        params: [0.0; 4],
        viewport: [0.0; 4],
    };
    let uniform = buffer(
        device,
        "reconstruction fixture parameters",
        &[camera],
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let mut points = Vec::new();
    for z in [0.01f32, 0.5, 1.83, 4.27, 5.99] {
        for x in [0.01f32, 0.5, 1.37, 3.9, 4.1, 6.51, 7.99] {
            points.push([-8.0 + 2.0 * x, -3.0 + z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        }
    }
    for [x, z] in [
        [-1.0, -0.5],
        [1.0, -0.5],
        [-1.0, 0.5],
        [1.0, 0.5],
        [3.0, 1.5],
    ] {
        points.push([x, z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    }
    let probes = buffer(
        device,
        "reconstruction fixture probes",
        &points,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
    );
    let source = format!(
        "{}\n{}\n{}\n{}\n{}",
        include_str!("shaders/connectivity.wgsl"),
        include_str!("shaders/layer_bounds.wgsl"),
        include_str!("shaders/fog.wgsl"),
        include_str!("shaders/raymarch.wgsl"),
        r#"
struct Probe { point:vec4f, result:vec4f }
@group(0) @binding(6) var<storage,read_write> probes:array<Probe>;
@compute @workgroup_size(1)
fn reconstruction_probe(@builtin(global_invocation_id) id:vec3u) {
    let pos=probes[id.x].point.xy;let dims=vec2i(textureDimensions(field));
    let anchor=clamp(vec2i(floor((pos-camera.domain.xy)/(camera.domain.zw/vec2f(dims)))),vec2i(0),dims-1);
    probes[id.x].result=vec4f(sample_layer(pos,anchor,dims),0);
}
@compute @workgroup_size(1)
fn integration_probe(@builtin(global_invocation_id) id:vec3u) {
    let p=probes[id.x].point;let ray=camera.eye.xyz;
    probes[id.x].result=vec4f(integrate_ray(p.xyz,ray,0.0,p.w,vec2i(textureDimensions(field))),0,0,0);
}
"#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production reconstruction probe"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("reconstruction probe layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 7,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 9,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("production reconstruction probe"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("reconstruction_probe"),
        compilation_options: Default::default(),
        cache: None,
    });
    let integration_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("production ray integration probe"),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("integration_probe"),
        compilation_options: Default::default(),
        cache: None,
    });
    let view = texture.create_view(&Default::default());
    let smoothing = super::presmooth::PreSmooth::new(gpu, &texture, &bed, &faces, dims);
    let smooth_view = smoothing.texture.create_view(&Default::default());
    let hierarchy = super::bounds::LayerHierarchy::new(
        gpu,
        &texture,
        &smoothing.texture,
        &bed,
        &faces,
        dims,
        true,
    );
    let lower_view = hierarchy.lower.create_view(&Default::default());
    let upper_view = hierarchy.upper.create_view(&Default::default());
    let revision = std::cell::Cell::new(0u64);
    let strength = std::cell::Cell::new(0.6f32);
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: bed.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: faces.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: probes.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&smooth_view),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&lower_view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::TextureView(&upper_view),
            },
        ],
    });
    let dispatch = |field: &[[f32; 4]],
                    terrain: &[[f32; 4]],
                    masks: &[u32],
                    c: Camera,
                    pipeline: &wgpu::ComputePipeline,
                    count: u32|
     -> Result<Vec<[f32; 4]>, Error> {
        gpu.queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&c));
        gpu.queue
            .write_buffer(&bed, 0, bytemuck::cast_slice(terrain));
        gpu.queue
            .write_buffer(&faces, 0, bytemuck::cast_slice(masks));
        gpu.queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(field),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(8 * 16),
                rows_per_image: Some(6),
            },
            wgpu::Extent3d {
                width: 8,
                height: 6,
                depth_or_array_layers: 1,
            },
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        revision.set(revision.get() + 1);
        let reconstruction = match c.player[3] as u32 {
            1 => Reconstruction::CubicBSpline,
            2 => Reconstruction::PreSmoothed,
            3 => Reconstruction::SmoothStep,
            _ => Reconstruction::Bilinear,
        };
        if reconstruction == Reconstruction::PreSmoothed {
            smoothing.encode(gpu, &mut encoder, revision.get(), strength.get());
        }
        hierarchy.encode(
            gpu,
            &mut encoder,
            revision.get(),
            reconstruction,
            strength.get(),
        );
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(count, 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        Ok(gpu
            .read_buffer(&probes, probes.size())?
            .as_chunks::<32>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(&b[16..32]))
            .collect())
    };
    let sample = |field: &[[f32; 4]], terrain: &[[f32; 4]], masks: &[u32], mode: Reconstruction| {
        let mut c = camera;
        c.player[3] = mode as u32 as f32;
        dispatch(field, terrain, masks, c, &pipeline, points.len() as u32)
    };
    let field: Vec<_> = (0..48)
        .map(|i| {
            let x = (i % 8) as f32;
            let z = (i / 8) as f32;
            [
                0.03 + 0.01 * x * x + 0.02 * z,
                0.02 + 0.07 * ((i * 13) % 11) as f32,
                0.0,
                0.0,
            ]
        })
        .collect();
    let cubic = sample(&field, &terrain, &masks, Reconstruction::CubicBSpline)?;
    let linear = sample(&field, &terrain, &masks, Reconstruction::Bilinear)?;
    let smooth = sample(&field, &terrain, &masks, Reconstruction::SmoothStep)?;
    // Tensor-product cubic Hermite interpolation with zero endpoint tangents.
    // This independent f64 reference also exercises centres and clamped borders.
    let hermite = |a: f64, b: f64, t: f64| {
        (2.0 * t.powi(3) - 3.0 * t.powi(2) + 1.0) * a + (-2.0 * t.powi(3) + 3.0 * t.powi(2)) * b
    };
    for (point, result) in points.iter().zip(&smooth) {
        let x = (f64::from(point[0]) + 8.0) / 2.0 - 0.5;
        let z = f64::from(point[1]) + 3.0 - 0.5;
        let at = |dx: i32, dz: i32, axis: usize| {
            let ix = (x.floor() as i32 + dx).clamp(0, 7);
            let iz = (z.floor() as i32 + dz).clamp(0, 5);
            let index = (iz * 8 + ix) as usize;
            f64::from([field[index][0], field[index][1], terrain[index][0]][axis])
        };
        for (axis, value) in result.iter().take(3).enumerate() {
            let lower = hermite(at(0, 0, axis), at(1, 0, axis), x - x.floor());
            let upper = hermite(at(0, 1, axis), at(1, 1, axis), x - x.floor());
            let expected = hermite(lower, upper, z - z.floor());
            ensure(
                (f64::from(*value) - expected).abs() < 0.000002,
                "SmoothStep matches zero-tangent Hermite h/m/bed at centres, interiors and edges",
            )?;
        }
    }
    // Independent centred cardinal B-spline definition, evaluated in f64.
    let kernel = |r: f64| {
        let r = r.abs();
        if r < 1.0 {
            (4.0 - 6.0 * r * r + 3.0 * r * r * r) / 6.0
        } else if r < 2.0 {
            (2.0 - r).powi(3) / 6.0
        } else {
            0.0
        }
    };
    for (point, result) in points.iter().zip(&cubic) {
        let x = (f64::from(point[0]) + 8.0) / 2.0 - 0.5;
        let z = f64::from(point[1]) + 3.0 - 0.5;
        let mut expected = [0.0; 3];
        for iz in z.floor() as i32 - 1..=z.floor() as i32 + 2 {
            for ix in x.floor() as i32 - 1..=x.floor() as i32 + 2 {
                let w = kernel(x - f64::from(ix)) * kernel(z - f64::from(iz));
                let index = (iz.clamp(0, 5) * 8 + ix.clamp(0, 7)) as usize;
                let q = [field[index][0], field[index][1], terrain[index][0]];
                for axis in 0..3 {
                    expected[axis] += w * f64::from(q[axis]);
                }
            }
        }
        ensure(
            (0..3).all(|axis| (f64::from(result[axis]) - expected[axis]).abs() < 0.000002),
            "rectangular grid cubic agrees with independent f64 kernel at interior and edges",
        )?;
    }
    ensure(
        cubic
            .iter()
            .zip(&linear)
            .any(|(a, b)| (a[0] - b[0]).abs() > 0.001),
        "selector changes the actual reconstruction",
    )?;
    for constant in [0.0, 0.25] {
        for mode in [
            Reconstruction::CubicBSpline,
            Reconstruction::PreSmoothed,
            Reconstruction::SmoothStep,
        ] {
            let result = sample(
                &vec![[constant, constant, 0.0, 0.0]; 48],
                &terrain,
                &masks,
                mode,
            )?;
            ensure(
                result
                    .iter()
                    .all(|q| (q[0] - constant).abs() < 1e-6 && (q[1] - constant).abs() < 1e-6),
                "zero and constant fields are preserved including clamped edges",
            )?;
        }
    }
    let mut impulse = vec![[0.0f32; 4]; 48];
    impulse[2 * 8 + 3] = [1.0, 2.0, 0.0, 0.0];
    for amount in [0.0f32, 0.6, 1.0] {
        strength.set(amount);
        let filtered = sample(&impulse, &terrain, &masks, Reconstruction::PreSmoothed)?;
        for (q, (weight, original)) in filtered[filtered.len() - 5..].iter().zip([
            (0.25, 1.0),
            (0.125, 0.0),
            (0.125, 0.0),
            (0.0625, 0.0),
            (0.0, 0.0),
        ]) {
            let expected = original * (1.0 - amount) + weight * amount;
            ensure(
                (q[0] - expected).abs() < 1e-6 && (q[1] - 2.0 * expected).abs() < 1e-6,
                "render prefilter impulse response and strength match analytic 3x3 binomial kernel",
            )?;
        }
    }
    strength.set(0.6);
    // Distinct constants across a wall make any illegal second-neighbour tap visible.
    for horizontal in [false, true] {
        let low_side = |i: usize| if horizontal { i / 8 < 3 } else { i % 8 < 4 };
        let mut field: Vec<_> = (0..48)
            .map(|i| {
                if low_side(i) {
                    [0.1, 0.2, 0.0, 0.0]
                } else {
                    [3.0, 4.0, 0.0, 0.0]
                }
            })
            .collect();
        field[0] = [100.0, 200.0, 0.0, 0.0];
        masks.fill(1);
        if horizontal {
            for x in 0..8 {
                masks[9 * 6 + 3 * 8 + x] = 0;
            }
        } else {
            for z in 0..6 {
                masks[z * 9 + 4] = 0;
            }
        }
        terrain[0][1] = 0.0;
        masks[0] = 0;
        masks[1] = 0;
        masks[9 * 6] = 0;
        masks[9 * 6 + 8] = 0;
        for mode in [
            Reconstruction::CubicBSpline,
            Reconstruction::PreSmoothed,
            Reconstruction::SmoothStep,
        ] {
            let result = sample(&field, &terrain, &masks, mode)?;
            for (point, q) in points.iter().zip(&result) {
                let anchor_x = ((point[0] + 8.0) / 2.0).floor() as i32;
                let anchor_z = (point[1] + 3.0).floor() as i32;
                if anchor_x == 0 && anchor_z == 0 {
                    continue;
                }
                let expected = if low_side((anchor_z * 8 + anchor_x) as usize) {
                    [0.1, 0.2]
                } else {
                    [3.0, 4.0]
                };
                ensure(
                    (q[0] - expected[0]).abs() < 1e-6 && (q[1] - expected[1]).abs() < 1e-6,
                    "thin wall and invalid cells cannot leak into reconstruction",
                )?;
            }
        }
    }
    eprintln!(
        "Fog reconstruction: PASS (production WGSL, f64 B-spline/Hermite oracles, rectangular grid, borders, zero/constant, thin walls, voids, selector)"
    );

    // A 1 cm layer on a slope is crossed BETWEEN both base segment midpoints.
    // The old frozen-midpoint rule returns zero even though the integral is positive.
    // All rays stay inside one rectangular DDA cell; screen/geometry clipping is
    // covered separately by verify_render's production fragment tests.
    masks.fill(1);
    for (i, cell) in terrain.iter_mut().enumerate() {
        *cell = [0.025 * (-7.0 + 2.0 * (i % 8) as f32), 1.0, 0.0, 0.0];
    }
    let thin = vec![[0.01f32, 0.02, 0.0, 0.0]; 48];
    let march = |field: &[[f32; 4]],
                 terrain: &[[f32; 4]],
                 mode: Reconstruction,
                 origin: [f32; 3],
                 ray: [f32; 3],
                 length: f32|
     -> Result<f64, Error> {
        let point = [origin[0], origin[1], origin[2], length, 0.0, 0.0, 0.0, 0.0];
        gpu.queue
            .write_buffer(&probes, 0, bytemuck::cast_slice(&point));
        let mut c = camera;
        c.eye = [ray[0], ray[1], ray[2], 2.0];
        c.player[3] = mode as u32 as f32;
        c.params[0] = 1.0;
        Ok(f64::from(
            dispatch(field, terrain, &masks, c, &integration_pipeline, 1)?[0][0],
        ))
    };
    // Dense midpoint quadrature of the pointwise physical density, not the
    // shader's frozen-layer antiderivative or subdivision/culling decisions.
    let reference = |field: &[[f32; 4]],
                     terrain: &[[f32; 4]],
                     mode: Reconstruction,
                     origin: [f32; 3],
                     ray: [f32; 3],
                     length: f32| {
        let dt = f64::from(length) / 65536.0;
        let mut optical = 0.0;
        for i in 0..65536 {
            let t = (f64::from(i) + 0.5) * dt;
            let pos: [f64; 3] =
                std::array::from_fn(|axis| f64::from(origin[axis]) + f64::from(ray[axis]) * t);
            let x = (pos[0] + 8.0) / 2.0 - 0.5;
            let z = pos[2] + 3.0 - 0.5;
            let at = |ix: i32, iz: i32, axis: usize| {
                let index = (iz.clamp(0, 5) * 8 + ix.clamp(0, 7)) as usize;
                f64::from([field[index][0], field[index][1], terrain[index][0]][axis])
            };
            let layer: [f64; 3] = std::array::from_fn(|axis| {
                if mode == Reconstruction::CubicBSpline {
                    let mut value = 0.0;
                    for iz in z.floor() as i32 - 1..=z.floor() as i32 + 2 {
                        for ix in x.floor() as i32 - 1..=x.floor() as i32 + 2 {
                            value += kernel(x - f64::from(ix))
                                * kernel(z - f64::from(iz))
                                * at(ix, iz, axis);
                        }
                    }
                    value
                } else {
                    // Pre-smoothed cases pass constant h/m or independently
                    // filtered coefficients; b is always unfiltered.
                    let mix = |a: f64, b: f64, t: f64| {
                        if mode == Reconstruction::SmoothStep {
                            hermite(a, b, t)
                        } else {
                            a + (b - a) * t
                        }
                    };
                    let ix = x.floor() as i32;
                    let iz = z.floor() as i32;
                    mix(
                        mix(at(ix, iz, axis), at(ix + 1, iz, axis), x - x.floor()),
                        mix(
                            at(ix, iz + 1, axis),
                            at(ix + 1, iz + 1, axis),
                            x - x.floor(),
                        ),
                        z - z.floor(),
                    )
                }
            });
            if layer[0] > 0.0001 {
                let s = (pos[1] - layer[2]) / layer[0];
                if s > 0.0 && s < 1.0 {
                    optical += layer[1] / layer[0] * 6.0 * s * (1.0 - s) * dt;
                }
            }
        }
        optical
    };
    for mode in [
        Reconstruction::Bilinear,
        Reconstruction::SmoothStep,
        Reconstruction::PreSmoothed,
        Reconstruction::CubicBSpline,
    ] {
        for (label, origin, ray, length) in [
            (
                "sloped thin layer",
                [-2.0, -0.023125, -0.5],
                [1.0, 0.0, 0.0],
                2.0,
            ),
            ("reverse ray", [0.0, -0.023125, -0.5], [-1.0, 0.0, 0.0], 2.0),
            ("above", [-2.0, 0.3, -0.5], [1.0, 0.0, 0.0], 2.0),
            ("below", [-2.0, -0.3, -0.5], [1.0, 0.0, 0.0], 2.0),
            ("vertical", [-1.125, 0.02, -0.5], [0.0, -1.0, 0.0], 0.1),
            ("zero span", [-1.125, -0.023125, -0.5], [1.0, 0.0, 0.0], 0.0),
        ] {
            let expected = reference(&thin, &terrain, mode, origin, ray, length);
            let actual = march(&thin, &terrain, mode, origin, ray, length)?;
            eprintln!(
                "Fog adaptive {mode:?} {label}: optical={actual:.6}, f64 reference={expected:.6}"
            );
            ensure(
                (actual - expected).abs() <= 0.05 * expected + 0.00001,
                "adaptive thin-layer coverage matches dense f64 integration (5% budget)",
            )?;
        }
    }
    // A local top peak is invisible at both base midpoints. Bounding the support
    // rather than just sampled endpoints must keep it eligible for refinement.
    terrain.fill([0.0, 1.0, 0.0, 0.0]);
    let mut peak = vec![[0.0f32; 4]; 48];
    peak[2 * 8 + 3] = [0.16, 0.16, 0.0, 0.0];
    let ray = [2.0 / 5.0f32.sqrt(), 0.0, 1.0 / 5.0f32.sqrt()];
    for (mode, height) in [
        (Reconstruction::Bilinear, 0.12),
        (Reconstruction::SmoothStep, 0.14),
        (Reconstruction::CubicBSpline, 0.066),
    ] {
        let origin = [-2.0, height, -1.0];
        let expected = reference(&peak, &terrain, mode, origin, ray, 5.0f32.sqrt());
        let actual = march(&peak, &terrain, mode, origin, ray, 5.0f32.sqrt())?;
        eprintln!(
            "Fog adaptive {mode:?} interior peak: optical={actual:.6}, f64 reference={expected:.6}"
        );
        ensure(
            expected > 0.01 && (actual - expected).abs() < 0.05 * expected,
            "interior top peak is not culled when base samples miss it",
        )?;
    }
    // The raw peak lies outside this cell's linear support, but its filtered
    // tail reaches the ray. Culling against raw rather than smoothed h loses it.
    peak.fill([0.0; 4]);
    peak[2 * 8 + 4] = [0.16, 0.16, 0.0, 0.0];
    let filtered: Vec<[f32; 4]> = (0..48)
        .map(|i| {
            let mut hm = [0.0; 4];
            for dz in -1i32..=1 {
                for dx in -1i32..=1 {
                    let j = ((i / 8 + dz).clamp(0, 5) * 8 + (i % 8 + dx).clamp(0, 7)) as usize;
                    let weight = (2 - dx.abs()) * (2 - dz.abs());
                    for axis in 0..2 {
                        hm[axis] += peak[j][axis] * weight as f32 / 16.0;
                    }
                }
            }
            for axis in 0..2 {
                hm[axis] = 0.4 * peak[i as usize][axis] + 0.6 * hm[axis];
            }
            hm
        })
        .collect();
    let origin = [-4.0, 0.003, -0.5];
    let ray = [1.0, 0.0, 0.0];
    let expected = reference(
        &filtered,
        &terrain,
        Reconstruction::Bilinear,
        origin,
        ray,
        2.0,
    );
    let actual = march(
        &peak,
        &terrain,
        Reconstruction::PreSmoothed,
        origin,
        ray,
        2.0,
    )?;
    ensure(
        expected > 0.01 && (actual - expected).abs() < 0.05 * expected,
        "pre-smoothed candidate bounds include filtered fog outside the raw support",
    )?;
    for empty in [vec![[0.0; 4]; 48], vec![[0.2, 0.0, 0.0, 0.0]; 48]] {
        ensure(
            march(
                &empty,
                &terrain,
                Reconstruction::Bilinear,
                [-2.0, 0.1, -0.5],
                [1.0, 0.0, 0.0],
                2.0,
            )? == 0.0,
            "zero thickness or zero mass contributes no optical depth",
        )?;
    }
    eprintln!(
        "Fog adaptive integration: PASS (sloped 1 cm layer, interior peak, filtered support, reverse/vertical rays, empty intervals, independent f64 density quadrature)"
    );
    // A distant column after an empty 4x4 node exercises actual hierarchical
    // jumps. The oracle integrates density, not another DDA/tree traversal.
    let distant: Vec<_> = (0..48)
        .map(|i| {
            if i % 8 == 6 {
                [0.16, 0.16, 0.0, 0.0]
            } else {
                [0.0; 4]
            }
        })
        .collect();
    let diagonal = 292.0f32.sqrt();
    for mode in [
        Reconstruction::Bilinear,
        Reconstruction::SmoothStep,
        Reconstruction::CubicBSpline,
    ] {
        for (label, origin, ray, length) in [
            ("forward skip", [-8.0, 0.05, -0.5], [1.0, 0.0, 0.0], 16.0),
            ("reverse skip", [8.0, 0.05, -0.5], [-1.0, 0.0, 0.0], 16.0),
            (
                "parallel node face",
                [-8.0, 0.05, 0.0],
                [1.0, 0.0, 0.0],
                16.0,
            ),
            (
                "corner ties",
                [-8.0, 0.05, -3.0],
                [16.0 / diagonal, 0.0, 6.0 / diagonal],
                diagonal,
            ),
            (
                "reverse corner ties",
                [8.0, 0.05, 3.0],
                [-16.0 / diagonal, 0.0, -6.0 / diagonal],
                diagonal,
            ),
            (
                "clipped before fog",
                [-8.0, 0.05, -0.5],
                [1.0, 0.0, 0.0],
                8.0,
            ),
            (
                "nearly parallel",
                [-8.0, 0.05, -0.5],
                [1.0, 0.0001, 0.0],
                16.0,
            ),
        ] {
            let expected = reference(&distant, &terrain, mode, origin, ray, length);
            let actual = march(&distant, &terrain, mode, origin, ray, length)?;
            ensure(
                (actual - expected).abs() < 0.03 * expected + 0.00001,
                &format!("hierarchy {mode:?} {label}: {actual} versus {expected}"),
            )?;
        }
    }
    super::bounds_verification::run(gpu)?;
    eprintln!(
        "Fog hierarchy: PASS (independent f64 long-ray density, empty-node jumps, reverse/corner/parallel/clipped rays, odd rectangular padding and all mips)"
    );
    Ok(())
}

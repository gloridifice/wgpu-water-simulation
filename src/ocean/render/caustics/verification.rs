//! GPU height-ray checks against densely sampled independent CPU terrain.
use super::height_pyramid::HeightPyramid;
use crate::ocean::{
    Error,
    gpu::context::{Gpu, buffer},
    parameters::{ocean::OceanConfig, optics::OpticsConfig},
    simulation::{nearshore::Shore, spectral::FftWaves as Ocean},
};
use glam::Vec3;

pub(in crate::ocean::render) fn verify(gpu: &Gpu) -> Result<(), Error> {
    for n in [64, 1024] {
        verify_resolution(gpu, n)?;
    }
    Ok(())
}
fn verify_resolution(gpu: &Gpu, n: u32) -> Result<(), Error> {
    let ocean = Ocean::new(
        gpu,
        OceanConfig {
            resolution: 32,
            mesh_resolution: 32,
            ..Default::default()
        },
    )?;
    let shore = Shore::with_resolution(gpu, &ocean, n);
    let mut config = OpticsConfig::default();
    config.model.enabled = true;
    let mut params = shore.parameters(&config, 0.0, false, 1.0);
    params.domain = [-16.0, -8.0, 32.0, 16.0];
    gpu.queue
        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&params));
    let pyramid = HeightPyramid::new(gpu, &shore);
    let mut rays = vec![
        (Vec3::new(0.0, 8.0, 0.0), -Vec3::Y),
        (
            Vec3::new(-15.5, 1.0, 0.0),
            Vec3::new(1.0, -0.05, 0.0).normalize(),
        ),
        (
            Vec3::new(15.5, 1.0, 0.0),
            Vec3::new(-1.0, -0.05, 0.0).normalize(),
        ),
        (
            Vec3::new(-30.0, 10.0, 0.0),
            Vec3::new(1.0, -0.2, 0.0).normalize(),
        ),
        (Vec3::new(-15.0, 20.0, 0.0), Vec3::X),
        (Vec3::new(17.0, 5.0, 0.0), -Vec3::Y),
    ];
    for i in 0..128 {
        let x = -15.0 + 30.0 * (i % 17) as f32 / 16.0;
        let z = -7.0 + 14.0 * (i % 13) as f32 / 12.0;
        let direction = Vec3::new(
            (i as f32 * 1.7).sin() * 0.8,
            -0.3 - (i % 7) as f32 * 0.1,
            (i as f32 * 2.3).cos() * 0.6,
        )
        .normalize();
        rays.push((Vec3::new(x, 6.0, z), direction));
    }
    let cases: Vec<[[f32; 4]; 2]> = rays
        .iter()
        .map(|(o, d)| [o.extend(0.0).to_array(), d.extend(60.0).to_array()])
        .collect();
    let input = buffer(
        &gpu.device,
        "height query cases",
        &cases,
        wgpu::BufferUsages::STORAGE,
    );
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("height results"),
        size: rays.len() as u64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("height query verification"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../../simulation/nearshore/shaders/shore.wgsl"),
                    include_str!("shaders/height_trace.wgsl"),
                    r#"
@group(0) @binding(0) var<uniform> shore: ShoreParams;
@group(0) @binding(1) var<storage,read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(2) var<storage,read> height_ranges: array<vec2<f32>>;
struct Ray { origin:vec4<f32>, direction:vec4<f32> }
@group(0) @binding(3) var<storage,read> rays: array<Ray>;
@group(0) @binding(4) var<storage,read_write> results: array<vec4<f32>>;
@compute @workgroup_size(64) fn query(@builtin(global_invocation_id) id:vec3<u32>) {
 if id.x>=arrayLength(&rays) { return; }
 let r=rays[id.x]; let hit=trace_heightfield(r.origin.xyz,r.direction.xyz,r.direction.w);
 results[id.x]=vec4(hit.distance,f32(hit.visits),f32(hit.status),0.0);
}
"#
                )
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("height checks"),
            layout: None,
            module: &shader,
            entry_point: Some("query"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("height checks"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            &shore.uniform,
            &shore.terrain_bed,
            &pyramid.ranges,
            &input,
            &output,
        ]
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect::<Vec<_>>(),
    });
    let mut max_visits = 0.0f32;
    for shape in 0..3 {
        let bed: Vec<[f32; 2]> = (0..n * n)
            .map(|i| {
                let x = -16.0 + ((i % n) as f32 + 0.5) * 32.0 / n as f32;
                let z = -8.0 + ((i / n) as f32 + 0.5) * 16.0 / n as f32;
                let h = match shape {
                    0 => -3.0 + 0.08 * x - 0.12 * z,
                    1 => -4.0 + 7.0 * (-(x - 0.25).powi(2) * 8.0).exp(),
                    _ => -3.0 + 2.0 * (x * 0.9).sin() * (z * 1.1).cos(),
                };
                [h, 1.0]
            })
            .collect();
        gpu.queue
            .write_buffer(&shore.terrain_bed, 0, bytemuck::cast_slice(&bed));
        pyramid.invalidate();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        pyramid.encode(&mut encoder);
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups((rays.len() as u32).div_ceil(64), 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&output, output.size())?;
        let results: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
        let height = |p: Vec3| {
            let gx = ((p.x + 16.0) * n as f32 / 32.0 - 0.5).clamp(0.0, (n - 1) as f32);
            let gz = ((p.z + 8.0) * n as f32 / 16.0 - 0.5).clamp(0.0, (n - 1) as f32);
            let x = gx.floor() as usize;
            let z = gz.floor() as usize;
            let sample = |xx: usize, zz: usize| {
                bed[zz.min(n as usize - 1) * n as usize + xx.min(n as usize - 1)][0]
            };
            let a = sample(x, z) * (1.0 - gx.fract()) + sample(x + 1, z) * gx.fract();
            let b = sample(x, z + 1) * (1.0 - gx.fract()) + sample(x + 1, z + 1) * gx.fract();
            a * (1.0 - gz.fract()) + b * gz.fract()
        };
        for (i, ((origin, ray), actual)) in rays.iter().zip(results).enumerate() {
            // Independent dense first-crossing reference, including both sides
            // of the thin ridge. The reference step is 1 mm in world space.
            let mut expected = None;
            let mut previous = None;
            for step in 0..=60_000 {
                let t = step as f32 * 0.001;
                let p = *origin + *ray * t;
                if p.x < -16.0 || p.x > 16.0 || p.z < -8.0 || p.z > 8.0 {
                    previous = None;
                    continue;
                }
                let gap = p.y - height(p);
                if gap.abs() < 1e-6 || previous.is_some_and(|g: f32| g * gap < 0.0) {
                    expected = Some(t);
                    break;
                }
                previous = Some(gap);
            }
            let valid = actual[2] == 1.0;
            if actual[2] == 2.0
                || valid != expected.is_some()
                || expected.is_some_and(|t| (actual[0] - t).abs() > 0.003)
            {
                return Err(format!(
                    "height trace n={n} shape={shape} ray={i}: GPU={actual:?}, reference={expected:?}"
                )
                .into());
            }
            max_visits = max_visits.max(actual[1]);
        }
        if results[4][1] > 2.0 {
            return Err("height hierarchy failed to skip empty space".into());
        }
    }
    eprintln!(
        "GPU height hierarchy {n}: planes, thin ridge nearest hit, rough bed, vertical/reverse/outside rays, rebuild, rectangular domain; max {max_visits} visits, empty-space ray <=2"
    );
    Ok(())
}

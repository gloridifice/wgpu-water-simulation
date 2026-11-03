//! Production small-wave normals: cross-cell smoothness, periodic seams and
//! unchanged nodal slopes. Analytic Fourier modes keep the reference independent
//! of the shader's interpolation, without requiring a particular cubic kernel.
use crate::ocean::{
    Error,
    camera::CameraUniform,
    gpu::context::{Gpu, buffer},
    simulation::nearshore::ShoreUniform,
};
use glam::{Vec2, Vec3};

const N: usize = 32;
const MODES: [(f32, f32, f32, f32); 2] = [(0.15, 3.0, 2.0, 0.4), (0.09, -1.0, 5.0, -0.2)];

pub(super) fn run(gpu: &Gpu) -> Result<(), Error> {
    let device = &gpu.device;
    let mut camera: CameraUniform = bytemuck::Zeroable::zeroed();
    camera.ocean = [N as f32, 64.0, 0.0, 1.7];
    let mut shore: ShoreUniform = bytemuck::Zeroable::zeroed();
    shore.terrain = [1.0, 0.1, -10.0, 6.0];
    shore.waves = [0.0, 0.0, 2.0, 1.0];
    shore.visual[0] = 1.0;
    let base = shore;
    let camera_buffer = buffer(
        device,
        "normal probe camera",
        &[camera],
        wgpu::BufferUsages::UNIFORM,
    );
    let shore_buffer = buffer(
        device,
        "normal probe shore",
        &[shore],
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let mut heights = vec![[0.0_f32; 2]; N * N];
    for z in 0..N {
        for x in 0..N {
            heights[z * N + x][0] = MODES
                .iter()
                .map(|&(a, kx, kz, phase)| {
                    a * (std::f32::consts::TAU / N as f32 * (kx * x as f32 + kz * z as f32) + phase)
                        .sin()
                })
                .sum();
        }
    }
    let fields = buffer(
        device,
        "analytic wave heights",
        &heights,
        wgpu::BufferUsages::STORAGE,
    );
    let terrain = buffer(
        device,
        "unused flat terrain",
        &[[0.0_f32; 2]],
        wgpu::BufferUsages::STORAGE,
    );
    let epsilon = 1.0 / 512.0;
    let mut cases = Vec::new();
    // Probe both grid axes, including negative coordinates and periodic edges.
    // World position stays inside fully wet SWE so blend masks cannot hide kinks.
    for axis in 0..2 {
        for edge in [-1.0, 0.0, 5.0, 31.0, 32.0] {
            for delta in [-epsilon, 0.0, epsilon] {
                let mut p = [2.375, 3.625, 1.0, 0.1];
                p[axis] = edge + delta;
                cases.push(p);
            }
        }
    }
    let smooth_count = cases.len();
    cases.extend([[2.25, -3.375, 1.0, 0.1], [34.25, 28.625, 1.0, 0.1]]);
    let node_start = cases.len();
    cases.extend([
        [0.0, 0.0, 1.0, 0.1],
        [1.0, 2.0, 1.0, 0.1],
        [7.0, 4.0, 1.0, 0.1],
    ]);
    let fade_start = cases.len();
    cases.extend([
        [1.0, 2.0, 0.135, 0.1],
        [1.0, 2.0, 1.0, 1.25],
        [1.0, 2.0, 0.02, 0.1],
        [1.0, 2.0, 1.0, 2.0],
    ]);
    let inputs = buffer(
        device,
        "normal probe positions",
        &cases,
        wgpu::BufferUsages::STORAGE,
    );
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("normal probe results"),
        size: cases.len() as u64 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let source = format!(
        "{}\n{}\n{}\n{}\n{}",
        include_str!("../shaders/scene_types.wgsl"),
        include_str!("../../simulation/nearshore/shaders/shore.wgsl"),
        include_str!("../../simulation/nearshore/shaders/shore_read.wgsl"),
        include_str!("../shaders/surface.wgsl"),
        r#"
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> fields: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> shore: ShoreParams;
@group(0) @binding(3) var<storage, read> terrain_bed: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> shore_field: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> shore_lace: array<f32>;
@group(0) @binding(6) var<storage, read> cases: array<vec4<f32>>;
@group(0) @binding(7) var<storage, read_write> results: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&cases) { return; }
    let c = cases[id.x];
    let normal = shore_detail_normal(normalize(vec3(0.2, 1.0, -0.1)), c.xy, vec3(0.0), c.z, c.w);
    results[id.x] = vec4(normal, 0.0);
}
"#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production small-wave normal probe"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("small-wave normal probe"),
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let entries: Vec<_> = [
        (0, &camera_buffer),
        (1, &fields),
        (2, &shore_buffer),
        (3, &terrain),
        (6, &inputs),
        (7, &output),
    ]
    .into_iter()
    .map(|(binding, resource)| wgpu::BindGroupEntry {
        binding,
        resource: resource.as_entire_binding(),
    })
    .collect();
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("small-wave normal probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let sample = || -> Result<Vec<Vec2>, Error> {
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups((cases.len() as u32).div_ceil(64), 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&output, output.size())?;
        bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|bytes| {
                let n: [f32; 4] = bytemuck::pod_read_unaligned(bytes);
                let normal = Vec3::new(n[0], n[1], n[2]);
                if !normal.is_finite() || (normal.length() - 1.0).abs() > 2e-6 || normal.y <= 0.0 {
                    return Err(format!("invalid small-wave normal: {normal:?}").into());
                }
                Ok(Vec2::new(
                    0.2 - normal.x / normal.y,
                    -0.1 - normal.z / normal.y,
                ))
            })
            .collect()
    };
    let slopes = sample()?;
    let jump = slopes[..smooth_count]
        .as_chunks::<3>()
        .0
        .iter()
        .map(|s| ((s[2] - s[1]) / epsilon - (s[1] - s[0]) / epsilon).length())
        .fold(0.0_f32, f32::max);
    if jump > 0.002 {
        return Err(format!(
            "small-wave normal grid kink: derivative jump={jump:.6} (limit 0.002)"
        )
        .into());
    }
    if (slopes[smooth_count] - slopes[smooth_count + 1]).length() > 2e-6 {
        return Err("small-wave normal periodic seam".into());
    }
    // For a sinusoid, a centered difference has the exact response sin(k*d)/d.
    // Check existing integer-node detail and gain/metric scaling, not the cubic formula.
    let expected = |p: [f32; 4]| -> Vec2 {
        let mut result = Vec2::ZERO;
        for (amplitude, kx, kz, phase) in MODES {
            let k = Vec2::new(kx, kz) * (std::f32::consts::TAU / N as f32);
            let phase = k.dot(Vec2::new(p[0], p[1])) + phase;
            let response = Vec2::new(
                k.x.sin() - (4.0 * k.x).sin() / 4.0,
                k.y.sin() - (4.0 * k.y).sin() / 4.0,
            );
            result += amplitude * phase.cos() * response;
        }
        result * camera.ocean[3] * camera.ocean[0] / camera.ocean[1]
    };
    for i in node_start..fade_start {
        if (slopes[i] - expected(cases[i])).length() > 2e-6 {
            return Err(format!(
                "small-wave nodal slope changed at {:?}: {:?}",
                cases[i], slopes[i]
            )
            .into());
        }
    }
    for (i, weight) in [0.5, 0.5, 0.0, 0.0].into_iter().enumerate() {
        if (slopes[fade_start + i] - expected(cases[fade_start + i]) * weight).length() > 2e-6 {
            return Err("small-wave depth/footprint fade changed".into());
        }
    }
    for guard in 0..3 {
        shore = base;
        match guard {
            0 => shore.visual[0] = 0.0,
            1 => shore.waves[3] = 0.0,
            _ => shore.terrain[2] = -100.0, // Fully offshore: no SWE detail.
        }
        gpu.queue
            .write_buffer(&shore_buffer, 0, bytemuck::bytes_of(&shore));
        if sample()?.iter().any(|s| s.length() > 2e-6) {
            return Err(
                format!("small-wave disabled/offshore guard {guard} changed base normal").into(),
            );
        }
    }
    println!(
        "GPU small-wave normals: grid C1/periodic seams, nodal amplitude, fades and disabled/offshore guards OK (derivative jump={jump:.6})"
    );
    Ok(())
}

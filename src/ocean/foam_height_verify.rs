//! Probe the actual vertex routine: lifted clip position, unchanged water data,
//! density mapping, dry/display guards and a separate foam shading normal.
use super::{Shore, ShoreConfig};
use crate::ocean::{
    gpu::{CameraUniform, Error, Gpu, Ocean},
    optics::OpticsConfig,
    spectrum::OceanConfig,
};
use wgpu::util::DeviceExt;

pub(super) fn verify(gpu: &Gpu) -> Result<(), Error> {
    let ocean = Ocean::new(
        gpu,
        OceanConfig {
            resolution: 8,
            mesh_resolution: 8,
            length: 8.0,
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let mut shore = Shore::new(gpu, &ocean);
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("foam vertex lift probe"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}\n{}\n{}\n{}\n{}",
                    include_str!("shaders/optics.wgsl"),
                    include_str!("shaders/sky.wgsl"),
                    include_str!("shaders/skylight.wgsl"),
                    include_str!("shaders/render.wgsl"),
                    include_str!("shaders/shore.wgsl"),
                    include_str!("shaders/shore_read.wgsl"),
                    r#"
@group(0) @binding(12) var<storage, read_write> result: array<vec4<f32>>;
@compute @workgroup_size(1) fn height_probe() {
    let vertex = water_vertex_data(40u, 4u);
    result[0] = vertex.position;
    result[1] = vec4(vertex.world, 1.0);
    result[2] = vec4(vertex.normal, 0.0);
    result[3] = vec4(vertex.foam_normal, 0.0);
}"#
                )
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("foam vertex lift probe"),
            layout: None,
            module: &shader,
            entry_point: Some("height_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let mut camera: CameraUniform = bytemuck::Zeroable::zeroed();
    camera.view_projection = glam::Mat4::IDENTITY.to_cols_array_2d();
    camera.ocean = [8.0, 8.0, 0.0, 1.0];
    camera.misc = [1.0, 8.0, 0.0, 0.0];
    let camera_buffer = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("foam height camera"),
            contents: bytemuck::bytes_of(&camera),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("foam height results"),
        size: 64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("foam vertex lift probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: ocean.fields.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: shore.field.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 12,
                resource: output.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 13,
                resource: shore.terrain_bed.as_entire_binding(),
            },
        ],
    });
    let optics = OpticsConfig {
        beach: true,
        shore_x: -1.0,
        ..Default::default()
    };
    let base = shore.parameters(&optics, 0.0, false, 1.0);
    let cells = (super::RESOLUTION * super::RESOLUTION) as usize;
    let mut field = vec![[0.0_f32; 4]; cells * 5];
    // Analytic smoothstep landmarks, saturation, zero maximum, dry and thin
    // water, disabled visibility/SWE, independent color reference and gain.
    for case in 0..17 {
        let mut params = base;
        params.domain = [-4.5, -4.5, 256.0, 256.0];
        params.visual = [1.0, 0.1, 0.2, 0.8];
        camera.misc[0] = 1.0;
        camera.ocean[3] = 1.0;
        let mut depth = 1.0;
        let (density, expected) = match case {
            0 => (0.0, 0.0),
            1 => (0.2, 0.0),
            2 => (0.4, 0.015625),
            3 => (0.6, 0.05),
            4 => (0.8, 0.084375),
            5 => (1.0, 0.1),
            6 => (50.0, 0.1),
            7 => {
                params.visual[1] = 0.0;
                (1.0, 0.0)
            }
            8 => {
                depth = 0.0;
                (1.0, 0.0)
            }
            9 => {
                depth = 0.008;
                (1.0, 0.0)
            }
            10 => {
                depth = 0.014;
                (1.0, 0.05)
            }
            11 => {
                camera.misc[0] = 0.0;
                (1.0, 0.0)
            }
            12 => {
                params.waves[3] = 0.0;
                (1.0, 0.0)
            }
            13 => {
                params.density[1] = 0.01;
                camera.ocean[3] = 3.0;
                (0.6, 0.05)
            }
            14 => {
                params.visual = [1.0, 1.0, 0.0, 0.001];
                (0.0005, 0.5)
            }
            15 => {
                params.domain[0] = 100.0;
                (1.0, 0.0)
            }
            _ => (0.6, 0.05), // Linear density gradient raises only the foam normal.
        };
        for i in 0..cells {
            let gradient = if case == 16 {
                (i % 256) as f32 * 0.1 - 0.4
            } else {
                0.0
            };
            field[i] = [(density + gradient).max(0.0), 0.0, depth, 0.0];
            field[2 * cells + i] = [10.0, 0.0, 0.25, 0.0]; // Ground foam must not lift water.
        }
        gpu.queue
            .write_buffer(&shore.field, 0, bytemuck::cast_slice(&field));
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&params));
        gpu.queue
            .write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&camera));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&output, 64)?;
        let values: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
        let physical_y = if case == 12 || case == 15 { 0.0 } else { 0.25 };
        if values.iter().flatten().any(|v| !v.is_finite())
            || (values[0][1] - physical_y - expected).abs() > 2e-6
            || values[1] != [0.0, physical_y, 0.0, 1.0]
            || values[2] != [0.0, 1.0, 0.0, 0.0]
            || (case != 16 && values[2] != values[3])
            || (case == 16 && values[3][0] >= -0.01)
        {
            return Err(
                format!("foam height case {case}: {values:?}, expected lift {expected}").into(),
            );
        }
    }
    // Advance the actual solver twice from identical initial state. Render-only
    // controls must not change any water, foam, momentum or sand history.
    for order in [1, 2] {
        let a = OpticsConfig {
            shore: ShoreConfig {
                order,
                foam_height_max: 0.0,
                ..Default::default()
            },
            ..optics
        };
        let b = OpticsConfig {
            shore: ShoreConfig {
                foam_height_max: 1.0,
                foam_height_threshold: 2.0,
                foam_height_transition: 0.01,
                foam_texture: 3,
                foam_texture_size: 3.5,
                foam_micro_normal: 2.0,
                foam_opacity_scale: 8.0,
                foam_roughness: 0.3,
                foam_material_view: 4,
                ..a.shore
            },
            ..a
        };
        let mut baseline = Vec::new();
        for (index, config) in [a, b].iter().enumerate() {
            for (time, reset) in [(0.0, true), (0.02, false)] {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                shore.encode(gpu, &mut encoder, config, time, reset, 1.0);
                gpu.queue.submit([encoder.finish()]);
                if reset {
                    let bytes = gpu.read_buffer(&shore.field, (field.len() * 16) as u64)?;
                    let mut seeded: Vec<[f32; 4]> = bytemuck::cast_slice(&bytes).to_vec();
                    for i in 0..cells {
                        if seeded[cells + i][0] > 0.02 {
                            seeded[i][0] = 0.4;
                            seeded[cells + i][3] = 0.4;
                            seeded[3 * cells + i][0] = 0.08;
                            seeded[3 * cells + i][1] = -0.04;
                        }
                    }
                    gpu.queue
                        .write_buffer(&shore.field, 0, bytemuck::cast_slice(&seeded));
                }
            }
            let bytes = gpu.read_buffer(&shore.field, (field.len() * 16) as u64)?;
            if index == 0 {
                baseline = bytes;
            } else if baseline != bytes {
                return Err(
                    format!("foam vertex height changed solver state, order {order}").into(),
                );
            }
        }
    }
    println!(
        "Foam vertex height: smoothstep landmarks, limits/dry/visibility guards, independent water data/foam normal, and unchanged SWE state in both orders OK"
    );
    Ok(())
}

//! Inspection of the actual static collider and raw solver cells, not render-filtered terrain.
use crate::{
    parameters::{GeometryView, Inspection},
    terrain::{BakeSource, Terrain, Triangle},
};
use common::gpu::{Gpu, buffer};
use std::cell::RefCell;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 4],
    color: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    matrix: [[f32; 4]; 4],
    params: [f32; 4],
}
struct Mesh {
    buffer: wgpu::Buffer,
    count: u32,
}
struct Resources {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    solid: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    xray: wgpu::RenderPipeline,
    mesh: Mesh,
    collision: Mesh,
    heightfield: Mesh,
    support: Mesh,
    targets: Option<([u32; 2], wgpu::TextureView, wgpu::TextureView)>,
}
pub(super) struct Inspector {
    source: BakeSource,
    terrain: Terrain,
    resources: RefCell<Option<Resources>>,
    format: wgpu::TextureFormat,
}
impl Inspector {
    pub fn new(source: &BakeSource, terrain: &Terrain, format: wgpu::TextureFormat) -> Self {
        Self {
            source: source.clone(),
            terrain: terrain.clone(),
            resources: RefCell::new(None),
            format,
        }
    }
    pub fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        settings: &Inspection,
        size: [u32; 2],
    ) {
        let mut resources = self.resources.borrow_mut();
        let r = resources
            .get_or_insert_with(|| Resources::new(gpu, &self.source, &self.terrain, self.format));
        let orbit = common::camera::Orbit {
            yaw: settings.orbit[0],
            pitch: settings.orbit[1],
            distance: settings.orbit[2],
            target: glam::Vec3::from_array(settings.target),
        };
        let (matrix, _) = orbit.matrices(
            55.0f32.to_radians(),
            size[0] as f32 / size[1].max(1) as f32,
            0.05,
            500.0,
        );
        let overlay = matches!(
            settings.view,
            GeometryView::CollisionOverlay
                | GeometryView::HeightfieldOverlay
                | GeometryView::SupportOverlay
        );
        gpu.queue.write_buffer(
            &r.uniform,
            0,
            bytemuck::bytes_of(&Camera {
                matrix: matrix.to_cols_array_2d(),
                params: [f32::from(overlay), 0.0, 0.0, 0.0],
            }),
        );
        if r.targets.as_ref().is_none_or(|t| t.0 != size) {
            let texture = |format, label| {
                gpu.device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 4,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            r.targets = Some((
                size,
                texture(self.format, "inspection MSAA"),
                texture(wgpu::TextureFormat::Depth32Float, "inspection depth"),
            ));
        }
        let (_, color, depth) = r.targets.as_ref().unwrap();
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("static geometry inspection"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: Some(target),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.025,
                        g: 0.035,
                        b: 0.055,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &r.bind, &[]);
        pass.set_pipeline(&r.solid);
        let draw = |pass: &mut wgpu::RenderPass<'_>, mesh: &Mesh| {
            pass.set_vertex_buffer(0, mesh.buffer.slice(..));
            pass.draw(0..mesh.count, 0..1);
        };
        if overlay || settings.view == GeometryView::Mesh {
            draw(&mut pass, &r.mesh);
        }
        if overlay {
            pass.set_pipeline(if settings.xray { &r.xray } else { &r.overlay });
        }
        match settings.view {
            GeometryView::Collision | GeometryView::CollisionOverlay => {
                draw(&mut pass, &r.collision)
            }
            GeometryView::Heightfield | GeometryView::HeightfieldOverlay => {
                draw(&mut pass, &r.heightfield)
            }
            GeometryView::SupportOverlay => draw(&mut pass, &r.support),
            _ => {}
        }
    }
}
impl Resources {
    fn new(gpu: &Gpu, source: &BakeSource, terrain: &Terrain, format: wgpu::TextureFormat) -> Self {
        let device = &gpu.device;
        let mesh = |label, vertices: Vec<Vertex>| Mesh {
            count: vertices.len() as u32,
            buffer: buffer(
                device,
                label,
                if vertices.is_empty() {
                    &[Vertex {
                        position: [0.0; 4],
                        color: [0.0; 4],
                    }]
                } else {
                    &vertices
                },
                wgpu::BufferUsages::VERTEX,
            ),
        };
        let triangle_vertices =
            |triangles: &[Triangle], color: [f32; 3], kind: f32| -> Vec<Vertex> {
                triangles
                    .iter()
                    .flatten()
                    .map(|p| Vertex {
                        position: [p[0], p[1], p[2], kind],
                        color: [color[0], color[1], color[2], 1.0],
                    })
                    .collect()
            };
        let mut vertices = triangle_vertices(&source.floor, [0.35, 0.42, 0.18], 0.0);
        vertices.extend(triangle_vertices(&source.obstacle, [0.24, 0.27, 0.22], 0.0));
        for (v, c) in vertices.iter_mut().zip(&source.colors) {
            v.color = [c[0], c[1], c[2], 1.0];
        }
        let visual = mesh("inspection original mesh", vertices);
        let collision = if source.collision.is_empty() {
            source
                .floor
                .iter()
                .chain(&source.obstacle)
                .copied()
                .collect()
        } else {
            source.collision.clone()
        };
        let collision = mesh(
            "inspection real collider",
            triangle_vertices(&collision, [0.02, 0.7, 0.9], 1.0),
        );
        let support = mesh(
            "inspection original support",
            triangle_vertices(
                if source.support.is_empty() {
                    &source.floor
                } else {
                    &source.support
                },
                [0.6, 0.1, 1.0],
                3.0,
            ),
        );
        let mut cells = Vec::new();
        let [nx, nz] = terrain.metadata.resolution;
        let [ox, oz, _, _] = terrain.metadata.domain;
        let step = terrain.spacing();
        for z in 0..nz {
            for x in 0..nx {
                let i = (z * nx + x) as usize;
                if terrain.valid[i] == 0 {
                    continue;
                }
                let h = terrain.heights[i];
                let a = glam::Vec3::new(ox + x as f32 * step.x, h, oz + z as f32 * step.y);
                let b = a + glam::Vec3::X * step.x;
                let c = b + glam::Vec3::Z * step.y;
                let d = a + glam::Vec3::Z * step.y;
                cells.extend(triangle_vertices(
                    &[
                        [a.to_array(), d.to_array(), b.to_array()],
                        [b.to_array(), d.to_array(), c.to_array()],
                    ],
                    [1.0, 0.55, 0.025],
                    2.0,
                ));
                // Red edges show actual blocked solver/player faces, not the render-only mask.
                for (blocked, p, q) in [
                    (terrain.pass_x[(z * (nx + 1) + x) as usize] == 0, a, d),
                    (terrain.pass_x[(z * (nx + 1) + x + 1) as usize] == 0, b, c),
                    (terrain.pass_z[(z * nx + x) as usize] == 0, a, b),
                    (terrain.pass_z[((z + 1) * nx + x) as usize] == 0, d, c),
                ] {
                    if blocked {
                        let up = glam::Vec3::Y * 0.12;
                        cells.extend(triangle_vertices(
                            &[
                                [p.to_array(), q.to_array(), (p + up).to_array()],
                                [(p + up).to_array(), q.to_array(), (q + up).to_array()],
                            ],
                            [1.0, 0.025, 0.02],
                            4.0,
                        ));
                    }
                }
            }
        }
        let heightfield = mesh("inspection raw cell heights and barriers", cells);
        let uniform = buffer(
            device,
            "inspection camera",
            &[Camera {
                matrix: [[0.0; 4]; 4],
                params: [0.0; 4],
            }],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("geometry inspection"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/geometry.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |overlay: bool, xray: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("geometry inspection"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4],
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!overlay),
                    depth_compare: Some(if xray {
                        wgpu::CompareFunction::Always
                    } else {
                        wgpu::CompareFunction::LessEqual
                    }),
                    stencil: Default::default(),
                    bias: wgpu::DepthBiasState {
                        constant: if overlay { -2 } else { 0 },
                        slope_scale: if overlay { -1.0 } else { 0.0 },
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: 4,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            solid: pipeline(false, false),
            overlay: pipeline(true, false),
            xray: pipeline(true, true),
            uniform,
            bind,
            mesh: visual,
            collision,
            heightfield,
            support,
            targets: None,
        }
    }
}

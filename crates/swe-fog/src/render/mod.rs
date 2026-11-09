//! Analytic thin-layer integration; no screen-space history or fog readback.
use crate::{
    Error,
    parameters::Session,
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::{Gpu, buffer};
use glam::Vec3;
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    inverse: [[f32; 4]; 4],
    eye: [f32; 4],
    domain: [f32; 4],
    player: [f32; 4],
    params: [f32; 4],
    viewport: [f32; 4],
}
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    bed_bounds: [f32; 2],
    format: wgpu::TextureFormat,
    collision_source: BakeSource,
}
impl Renderer {
    pub fn new(
        gpu: &Gpu,
        sim: &Simulation,
        terrain: &Terrain,
        source: &BakeSource,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        source.validate()?;
        if source.floor.len() + source.obstacle.len() > 4096 {
            return Err("fog playground renderer supports at most 4096 static triangles".into());
        }
        let mut geometry = vec![[
            f32::from_bits((source.floor.len() + source.obstacle.len()) as u32),
            0.0,
            0.0,
            0.0,
        ]];
        for (kind, triangles) in [&source.floor, &source.obstacle].into_iter().enumerate() {
            for tri in triangles {
                for p in tri {
                    geometry.push([p[0], p[1], p[2], kind as f32]);
                }
            }
        }
        let scene_buffer = buffer(
            &gpu.device,
            "fog static world triangles",
            &geometry,
            wgpu::BufferUsages::STORAGE,
        );
        let heights: Vec<_> = terrain
            .heights
            .iter()
            .zip(&terrain.valid)
            .filter(|(_, v)| **v == 1)
            .map(|(h, _)| *h)
            .collect();
        let bed_bounds = [
            heights.iter().copied().fold(f32::INFINITY, f32::min),
            heights.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ];
        let device = &gpu.device;
        let uniform = buffer(
            device,
            "fog camera",
            &[Camera {
                inverse: [[0.0; 4]; 4],
                eye: [0.0; 4],
                domain: [0.0; 4],
                player: [0.0; 4],
                params: [0.0; 4],
                viewport: [0.0; 4],
            }],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog rendered field"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let view = sim.published.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog visual state"),
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
                    resource: sim.bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: scene_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: sim.control.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: sim.masks.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog ray integration"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/fog.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog display"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fog depth-clipped display"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            pipeline,
            bind,
            uniform,
            bed_bounds,
            format,
            collision_source: source.clone(),
        })
    }
    pub fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        session: &Session,
        domain: [f32; 4],
        size: [u32; 2],
    ) {
        let orbit = crate::scene::camera_collision::orbit(
            &session.player,
            session.camera,
            &self.collision_source,
        );
        let (_, inverse) = orbit.matrices(
            55.0f32.to_radians(),
            size[0].max(1) as f32 / size[1].max(1) as f32,
            0.1,
            200.0,
        );
        let data = Camera {
            inverse: inverse.to_cols_array_2d(),
            eye: orbit.eye().extend(1.0).to_array(),
            domain,
            player: Vec3::from_array(session.player.feet).extend(0.0).to_array(),
            params: [
                session.fog.extinction,
                session.fog.brightness,
                u32::from(session.visible) as f32,
                session.debug_view as f32,
            ],
            viewport: [
                size[0] as f32,
                size[1] as f32,
                self.bed_bounds[0],
                self.bed_bounds[1],
            ],
        };
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fog scene and ray integration"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
    /// Explicit offscreen capture; never part of the interactive render loop.
    pub fn read_image(
        &self,
        gpu: &Gpu,
        session: &Session,
        domain: [f32; 4],
        size: [u32; 2],
    ) -> Result<Vec<u8>, Error> {
        if !matches!(
            self.format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb
        ) {
            return Err("capture renderer must use RGBA8".into());
        }
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fog snapshot"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = (size[0] * 4).div_ceil(256) * 256;
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fog explicit image readback"),
            size: u64::from(stride * size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.encode(
            gpu,
            &mut encoder,
            &texture.create_view(&Default::default()),
            session,
            domain,
            size,
        );
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size[1]),
                },
            },
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.map_buffer(&staging)?;
        Ok(bytes
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..size[0] as usize * 4].iter().copied())
            .collect())
    }
}
pub fn save_png(path: &std::path::Path, pixels: &[u8], size: [u32; 2]) -> Result<(), Error> {
    let mut png = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create_new(path)?),
        size[0],
        size[1],
    );
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(pixels)?;
    Ok(())
}

//! Screen-space height fog with switchable projected tetrahedra and diagnostic marching.
pub(crate) mod benchmark;
mod bounds;
mod bounds_verification;
mod presmooth;
mod raster;
mod raster_benchmark;
pub(crate) mod raster_verification;
mod tet_reconstruction_verification;
pub(crate) mod verification;
use crate::{
    Error,
    parameters::{Reconstruction, RenderMode, Session},
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::{Gpu, buffer};
use glam::Vec3;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    inverse: [[f32; 4]; 4],
    /// xyz: camera position; w: base integration segments per XZ cell before refinement.
    eye: [f32; 4],
    domain: [f32; 4],
    /// xyz: capsule feet; w: reconstruction (0 linear, 1 cubic, 2 pre-smoothed, 3 SmoothStep).
    player: [f32; 4],
    params: [f32; 4],
    viewport: [f32; 4],
}
// Diagnostic construction modes let the benchmark compare identical physics
// in one process. Interactive rendering uses the switchable raster renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum Acceleration {
    PerRayBounds = 0,
    CachedBounds = 1,
    Hierarchy = 2,
    Raster = 3,
}
pub struct Renderer {
    raster: Option<raster::Raster>,
    acceleration: Acceleration,
    pipeline: wgpu::RenderPipeline,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    bed_bounds: [f32; 2],
    format: wgpu::TextureFormat,
    collision_source: crate::scene::camera_collision::Collider,
    smoothing: presmooth::PreSmooth,
    bounds: bounds::LayerHierarchy,
    published_revision: Arc<AtomicU64>,
}
impl Renderer {
    pub fn new(
        gpu: &Gpu,
        sim: &Simulation,
        terrain: &Terrain,
        source: &BakeSource,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        Self::new_with_acceleration(gpu, sim, terrain, source, format, Acceleration::Raster)
    }
    fn new_with_acceleration(
        gpu: &Gpu,
        sim: &Simulation,
        terrain: &Terrain,
        source: &BakeSource,
        format: wgpu::TextureFormat,
        acceleration: Acceleration,
    ) -> Result<Self, Error> {
        source.validate()?;
        if acceleration != Acceleration::Raster && source.floor.len() + source.obstacle.len() > 4096
        {
            return Err("fog playground renderer supports at most 4096 static triangles".into());
        }
        let mut geometry = vec![[
            f32::from_bits((source.floor.len() + source.obstacle.len()) as u32),
            f32::from_bits(u32::from(!source.colors.is_empty())),
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
        let smoothing =
            presmooth::PreSmooth::new(gpu, &sim.published, &sim.bed, &sim.masks, sim.resolution);
        let bounds = bounds::LayerHierarchy::new(
            gpu,
            &sim.published,
            &smoothing.texture,
            &sim.bed,
            &sim.masks,
            sim.resolution,
            acceleration == Acceleration::Hierarchy,
        );
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
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let view = sim.published.create_view(&Default::default());
        let smooth_view = smoothing.texture.create_view(&Default::default());
        let lower_view = bounds.lower.create_view(&Default::default());
        let upper_view = bounds.upper.create_view(&Default::default());
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog ray integration"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}\n{}",
                    include_str!("shaders/connectivity.wgsl"),
                    include_str!("shaders/layer_bounds.wgsl"),
                    include_str!("shaders/fog.wgsl"),
                    include_str!("shaders/raymarch.wgsl")
                )
                .into(),
            ),
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
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("ACCELERATION", acceleration as u32 as f64)],
                    ..Default::default()
                },
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
        let raster = (acceleration == Acceleration::Raster)
            .then(|| {
                raster::Raster::new(
                    gpu,
                    sim,
                    &smoothing.texture,
                    &scene_buffer,
                    &source.colors,
                    ((source.floor.len() + source.obstacle.len()) * 3) as u32,
                    format,
                )
            })
            .transpose()?;
        Ok(Self {
            raster,
            acceleration,
            pipeline,
            bind,
            uniform,
            bed_bounds,
            format,
            collision_source: crate::scene::camera_collision::Collider::new(source),
            smoothing,
            bounds,
            published_revision: sim.published_revision.clone(),
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
        let reconstruction = self.prepare_field(gpu, encoder, session);
        self.prepare_bounds(gpu, encoder, session, reconstruction);
        self.draw(gpu, encoder, target, session, domain, size, reconstruction);
    }
    fn prepare_field(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        session: &Session,
    ) -> Reconstruction {
        let pre_smoothed = (self.acceleration != Acceleration::Raster
            || session.render_mode == RenderMode::Tetrahedra)
            && session.reconstruction == Reconstruction::PreSmoothed
            && session.reconstruction_smoothing > 0.0
            && session.visible
            && (session.debug_view == 0 || session.debug_view == 5);
        if pre_smoothed {
            self.smoothing.encode(
                gpu,
                encoder,
                self.published_revision.load(Ordering::Relaxed),
                session.reconstruction_smoothing,
            );
        }
        if session.reconstruction == Reconstruction::PreSmoothed && !pre_smoothed {
            Reconstruction::Bilinear
        } else {
            session.reconstruction
        }
    }
    fn prepare_bounds(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        session: &Session,
        reconstruction: Reconstruction,
    ) {
        if matches!(
            self.acceleration,
            Acceleration::CachedBounds | Acceleration::Hierarchy
        ) && session.visible
            && (session.debug_view == 0 || session.debug_view == 5)
        {
            self.bounds.encode(
                gpu,
                encoder,
                self.published_revision.load(Ordering::Relaxed),
                reconstruction,
                session.reconstruction_smoothing,
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        session: &Session,
        domain: [f32; 4],
        size: [u32; 2],
        reconstruction: Reconstruction,
    ) {
        if let Some(raster) = &self.raster {
            raster.encode(
                gpu,
                encoder,
                target,
                session,
                &self.collision_source,
                domain,
                size,
                reconstruction,
                None,
            );
            return;
        }
        let orbit = self.collision_source.orbit(&session.player, session.camera);
        let (_, inverse) = orbit.matrices(
            55.0f32.to_radians(),
            size[0].max(1) as f32 / size[1].max(1) as f32,
            0.1,
            200.0,
        );
        let data = Camera {
            inverse: inverse.to_cols_array_2d(),
            eye: orbit
                .eye()
                .extend(session.integration_steps.clamp(2, 32) as f32)
                .to_array(),
            domain,
            player: Vec3::from_array(session.player.feet)
                .extend(reconstruction as u32 as f32)
                .to_array(),
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

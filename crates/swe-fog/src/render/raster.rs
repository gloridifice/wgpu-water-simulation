//! Shared 4x MSAA scene with height-fog composition or projected-tetrahedron intervals.
use crate::{
    parameters::{Reconstruction, RenderMode, Session},
    scene::camera_collision::Collider,
    simulation::Simulation,
};
use common::gpu::{Gpu, buffer};
use std::{
    cell::{Cell, RefCell},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Camera {
    view_projection: [[f32; 4]; 4],
    inverse: [[f32; 4]; 4],
    eye: [f32; 4],
    domain: [f32; 4],
    player: [f32; 4],
    params: [f32; 4],
    viewport: [f32; 4],
}
struct Targets {
    size: [u32; 2],
    color: wgpu::TextureView,
    distance: wgpu::TextureView,
    depth: wgpu::TextureView,
    optical: wgpu::TextureView,
    occlusion: wgpu::TextureView,
    volume_bind: wgpu::BindGroup,
    composite_bind: wgpu::BindGroup,
    height_bind: wgpu::BindGroup,
}
pub(super) struct Raster {
    uniform: wgpu::Buffer,
    prepare: wgpu::ComputePipeline,
    prepare_bind: wgpu::BindGroup,
    project: wgpu::ComputePipeline,
    project_bind: wgpu::BindGroup,
    draws: wgpu::Buffer,
    fallback: wgpu::RenderPipeline,
    far_volume: wgpu::RenderPipeline,
    far_fallback: wgpu::RenderPipeline,
    opacity_mask: wgpu::RenderPipeline,
    revision: Arc<AtomicU64>,
    cached: Cell<Option<(u64, u32, u32, bool)>>,
    resolution: [u32; 2],
    bind: wgpu::BindGroup,
    indices: wgpu::Buffer,
    sky: wgpu::RenderPipeline,
    scene: wgpu::RenderPipeline,
    capsule: wgpu::RenderPipeline,
    volume: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    height_fog: wgpu::RenderPipeline,
    distance_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    height_layout: wgpu::BindGroupLayout,
    targets: RefCell<Option<Targets>>,
    vertex_count: u32,
    cell_count: u32,
}
impl Raster {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        gpu: &Gpu,
        sim: &Simulation,
        terrain: &super::terrain_filter::RenderTerrain,
        smoothed: &wgpu::Texture,
        geometry: &wgpu::Buffer,
        colors: &[[f32; 3]],
        vertex_count: u32,
        format: wgpu::TextureFormat,
    ) -> Result<Self, crate::Error> {
        let device = &gpu.device;
        let count = u64::from(sim.resolution[0]) * u64::from(sim.resolution[1]) * 12;
        if count * 112 > device.limits().max_storage_buffer_binding_size {
            return Err("投影四面体原型超出设备单缓冲限制；请使用 256×256 或更小渲染网格".into());
        }
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raster fog camera"),
            size: std::mem::size_of::<Camera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let columns = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cached world tetrahedra"),
            size: count * 64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let proxies = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("projected tetrahedron intervals"),
            size: count * 112,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let prepare_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("continuous tetrahedron field"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("shaders/raster_types.wgsl"),
                    include_str!("shaders/connectivity.wgsl"),
                    include_str!("shaders/raster_prepare.wgsl")
                )
                .into(),
            ),
        });
        let prepare = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("prepare world tetrahedra"),
            layout: None,
            module: &prepare_shader,
            entry_point: Some("prepare_tets"),
            compilation_options: Default::default(),
            cache: None,
        });
        let prepare_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tetrahedron preparation resources"),
            layout: &prepare.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &sim.published.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: terrain.bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        &smoothed.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: columns.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: terrain.masks.as_entire_binding(),
                },
            ],
        });
        let project_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("project tetrahedron intervals"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("shaders/raster_types.wgsl"),
                    include_str!("shaders/tet_project.wgsl")
                )
                .into(),
            ),
        });
        let project = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("project tetrahedra"),
            layout: None,
            module: &project_shader,
            entry_point: Some("project_tets"),
            compilation_options: Default::default(),
            cache: None,
        });
        let draws = buffer(
            device,
            "tetrahedron conditional indirect draws",
            &[
                12u32, 0, 0, 0, 0, 12, 0, 0, 0, 0, 12, 0, 0, 0, 0, 12, 0, 0, 0, 0,
            ],
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::INDIRECT
                | wgpu::BufferUsages::COPY_DST,
        );
        let project_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tetrahedron projection resources"),
            layout: &project.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: columns.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: proxies.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: draws.as_entire_binding(),
                },
            ],
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let storage_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("raster fog resources"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage_entry(1),
                texture_entry(2),
                storage_entry(3),
                texture_entry(4),
                storage_entry(5),
                storage_entry(6),
                storage_entry(7),
                storage_entry(8),
            ],
        });
        let colors: Vec<_> = if colors.is_empty() {
            vec![[0.0; 4]]
        } else {
            colors.iter().map(|c| [c[0], c[1], c[2], 1.0]).collect()
        };
        let color_buffer = buffer(
            device,
            "baked model corner colors",
            &colors,
            wgpu::BufferUsages::STORAGE,
        );
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raster fog resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: color_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: columns.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: proxies.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: terrain.masks.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: geometry.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(
                        &sim.published.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: terrain.bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(
                        &smoothed.create_view(&Default::default()),
                    ),
                },
            ],
        });
        let ms_texture = |binding| {
            let mut entry = texture_entry(binding);
            if let wgpu::BindingType::Texture {
                ref mut multisampled,
                ..
            } = entry.ty
            {
                *multisampled = true;
            }
            entry
        };
        let distance_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog opaque distance"),
            entries: &[ms_texture(0)],
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog composition"),
            entries: &[ms_texture(0), ms_texture(1), ms_texture(2)],
        });
        let height_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("height fog scene inputs"),
            entries: &[ms_texture(0), ms_texture(1)],
        });
        let height_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("height fog accumulation"),
                bind_group_layouts: &[Some(&layout), Some(&height_layout)],
                immediate_size: 0,
            });
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog opaque pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let volume_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog volume pipeline"),
            bind_group_layouts: &[Some(&layout), Some(&distance_layout)],
            immediate_size: 0,
        });
        let resolve_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog resolve pipeline"),
            bind_group_layouts: &[Some(&layout), Some(&composite_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("projected tetrahedron fog"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}\n{}",
                    include_str!("shaders/raster_types.wgsl"),
                    include_str!("shaders/raster.wgsl"),
                    include_str!("shaders/connectivity.wgsl"),
                    include_str!("shaders/height_fog.wgsl")
                )
                .into(),
            ),
        });
        let color = |format, blend| {
            Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })
        };
        let scene_targets = [
            color(wgpu::TextureFormat::Rgba16Float, None),
            color(wgpu::TextureFormat::R32Float, None),
        ];
        let depth = |write| wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(write),
            depth_compare: Some(if write {
                wgpu::CompareFunction::LessEqual
            } else {
                wgpu::CompareFunction::Always
            }),
            stencil: Default::default(),
            bias: Default::default(),
        };
        let pipeline = |label: &str,
                        vertex,
                        fragment,
                        layout: &wgpu::PipelineLayout,
                        targets: &[Option<wgpu::ColorTargetState>],
                        depth_stencil,
                        cull_mode| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("FAR_PASS", f64::from(label.starts_with("far ")))],
                        ..Default::default()
                    },
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("FAR_PASS", f64::from(label.starts_with("far ")))],
                        ..Default::default()
                    },
                    targets,
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode,
                    ..Default::default()
                },
                depth_stencil,
                multisample: wgpu::MultisampleState {
                    count: if fragment == "composite" { 1 } else { 4 },
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let sky = pipeline(
            "fog sky",
            "fullscreen",
            "sky",
            &scene_layout,
            &scene_targets,
            Some(depth(false)),
            None,
        );
        let scene = pipeline(
            "fog opaque scene",
            "scene_vertex",
            "scene_fragment",
            &scene_layout,
            &scene_targets,
            Some(depth(true)),
            None,
        );
        let capsule = pipeline(
            "fog opaque capsule",
            "fullscreen",
            "capsule",
            &scene_layout,
            &scene_targets,
            Some(depth(true)),
            None,
        );
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let volume = pipeline(
            "fog optical depth",
            "interval_vertex",
            "interval_fragment",
            &volume_layout,
            &[color(
                wgpu::TextureFormat::R16Float,
                Some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
            )],
            None,
            None,
        );
        let fallback = pipeline(
            "near-eye tetrahedron intervals",
            "fallback_vertex",
            "fallback_fragment",
            &volume_layout,
            &[color(
                wgpu::TextureFormat::R16Float,
                Some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
            )],
            None,
            Some(wgpu::Face::Front),
        );
        let mut far_depth = depth(false);
        far_depth.depth_compare = Some(wgpu::CompareFunction::Less);
        let far_targets = [color(
            wgpu::TextureFormat::R16Float,
            Some(wgpu::BlendState {
                color: add,
                alpha: add,
            }),
        )];
        let far_volume = pipeline(
            "far projected tetrahedra",
            "interval_vertex",
            "interval_fragment",
            &volume_layout,
            &far_targets,
            Some(far_depth.clone()),
            None,
        );
        let far_fallback = pipeline(
            "far near-eye tetrahedra",
            "fallback_vertex",
            "fallback_fragment",
            &volume_layout,
            &far_targets,
            Some(far_depth),
            Some(wgpu::Face::Front),
        );
        let opacity_mask = pipeline(
            "opaque fog sample mask",
            "fullscreen",
            "opacity_mask",
            &resolve_layout,
            &[],
            Some(depth(true)),
            None,
        );
        let composite = pipeline(
            "fog optical composite",
            "fullscreen",
            "composite",
            &resolve_layout,
            &[color(format, None)],
            None,
            None,
        );
        let height_fog = pipeline(
            "screen-space height fog",
            "fullscreen",
            "height_accumulate",
            &height_pipeline_layout,
            &[color(
                wgpu::TextureFormat::R16Float,
                Some(wgpu::BlendState {
                    color: add,
                    alpha: add,
                }),
            )],
            None,
            None,
        );
        let indices = buffer(
            device,
            "projected tetrahedron fan",
            &[0u16, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4],
            wgpu::BufferUsages::INDEX,
        );
        Ok(Self {
            project,
            project_bind,
            draws,
            fallback,
            far_volume,
            far_fallback,
            opacity_mask,
            prepare,
            prepare_bind,
            revision: sim.published_revision.clone(),
            cached: Cell::new(None),
            resolution: sim.resolution,
            uniform,
            bind,
            indices,
            sky,
            scene,
            capsule,
            volume,
            composite,
            height_fog,
            distance_layout,
            composite_layout,
            height_layout,
            targets: RefCell::new(None),
            vertex_count,
            cell_count: count as u32,
        })
    }
    fn targets(&self, device: &wgpu::Device, size: [u32; 2]) -> Targets {
        let texture = |label, format| {
            device
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
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = texture("fog opaque color", wgpu::TextureFormat::Rgba16Float);
        let distance = texture("fog opaque distance", wgpu::TextureFormat::R32Float);
        let depth = texture("fog opaque depth", wgpu::TextureFormat::Depth32Float);
        let optical = texture(
            "fog accumulated optical depth",
            wgpu::TextureFormat::R16Float,
        );
        let occlusion = texture("fog opacity depth mask", wgpu::TextureFormat::Depth32Float);
        let entry = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let volume_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog distance input"),
            layout: &self.distance_layout,
            entries: &[entry(0, &distance)],
        });
        let composite_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog composite inputs"),
            layout: &self.composite_layout,
            entries: &[entry(0, &distance), entry(1, &color), entry(2, &optical)],
        });
        let height_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("height fog inputs"),
            layout: &self.height_layout,
            entries: &[entry(0, &distance), entry(1, &color)],
        });
        Targets {
            height_bind,
            size,
            color,
            distance,
            depth,
            optical,
            occlusion,
            volume_bind,
            composite_bind,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        session: &Session,
        source: &Collider,
        domain: [f32; 4],
        size: [u32; 2],
        reconstruction: Reconstruction,
        queries: Option<&wgpu::QuerySet>,
    ) {
        let mut targets = self.targets.borrow_mut();
        if targets.as_ref().is_none_or(|t| t.size != size) {
            *targets = Some(self.targets(&gpu.device, size));
        }
        self.encode_layer(
            gpu,
            encoder,
            target,
            session,
            source,
            domain,
            size,
            reconstruction,
            queries,
            targets.as_ref().unwrap(),
            true,
            true,
        );
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn encode_fields(
        fields: &[(&Self, [f32; 4], Reconstruction)],
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        session: &Session,
        source: &Collider,
        size: [u32; 2],
    ) {
        let first = fields[0].0;
        let mut targets = first.targets.borrow_mut();
        if targets.as_ref().is_none_or(|t| t.size != size) {
            *targets = Some(first.targets(&gpu.device, size));
        }
        for (i, (field, domain, reconstruction)) in fields.iter().enumerate() {
            field.encode_layer(
                gpu,
                encoder,
                target,
                session,
                source,
                *domain,
                size,
                *reconstruction,
                None,
                targets.as_ref().unwrap(),
                i == 0,
                i + 1 == fields.len(),
            );
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn encode_layer(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        session: &Session,
        source: &Collider,
        domain: [f32; 4],
        size: [u32; 2],
        reconstruction: Reconstruction,
        queries: Option<&wgpu::QuerySet>,
        t: &Targets,
        first: bool,
        last: bool,
    ) {
        let orbit = source.orbit(&session.player, session.camera);
        let (view_projection, inverse) = orbit.matrices(
            55.0f32.to_radians(),
            size[0].max(1) as f32 / size[1].max(1) as f32,
            0.1,
            200.0,
        );
        let detail = session.reconstruction_detail || reconstruction == Reconstruction::SmoothStep;
        let active_count = self.cell_count / if detail { 1 } else { 2 };
        let data = Camera {
            view_projection: view_projection.to_cols_array_2d(),
            inverse: inverse.to_cols_array_2d(),
            eye: orbit.eye().extend(session.height_fog_noise).to_array(),
            domain,
            player: glam::Vec3::from_array(session.player.feet)
                .extend(reconstruction as u32 as f32)
                .to_array(),
            params: [
                session.fog.extinction,
                session.fog.brightness,
                f32::from(session.visible),
                session.debug_view as f32,
            ],
            viewport: [
                size[0] as f32,
                size[1] as f32,
                active_count as f32,
                f32::from(detail),
            ],
        };
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mode = reconstruction as u32;
        let key = (
            self.revision.load(Ordering::Relaxed),
            mode,
            if mode == 2 {
                session.reconstruction_smoothing.to_bits()
            } else {
                0
            },
            detail,
        );
        let height_mode = session.render_mode == RenderMode::HeightFog;
        let rebuild = !height_mode
            && self.cached.get() != Some(key)
            && session.visible
            && (session.debug_view == 0 || session.debug_view == 5);
        if rebuild || queries.is_some() {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fog world tetrahedron preparation"),
                timestamp_writes: queries.map(|query_set| wgpu::ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(8),
                    end_of_pass_write_index: Some(9),
                }),
            });
            if rebuild {
                pass.set_pipeline(&self.prepare);
                pass.set_bind_group(0, &self.prepare_bind, &[]);
                pass.dispatch_workgroups(
                    self.resolution[0].div_ceil(8),
                    self.resolution[1].div_ceil(8),
                    1,
                );
                self.cached.set(Some(key));
            }
        }
        if !height_mode && session.visible && (session.debug_view == 0 || session.debug_view == 5) {
            for i in 0..4 {
                encoder.clear_buffer(&self.draws, i * 20 + 4, Some(4));
            }
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("projected tetrahedron setup"),
                timestamp_writes: queries.map(|query_set| wgpu::ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(10),
                    end_of_pass_write_index: Some(11),
                }),
            });
            pass.set_pipeline(&self.project);
            pass.set_bind_group(0, &self.project_bind, &[]);
            pass.dispatch_workgroups(active_count.div_ceil(64), 1, 1);
        }
        let timestamps = |index| {
            queries.map(|query_set| wgpu::RenderPassTimestampWrites {
                query_set,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: Some(index + 1),
            })
        };
        let attachment = |view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        };
        if first {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fog raster opaque scene"),
                timestamp_writes: timestamps(2),
                color_attachments: &[attachment(&t.color), attachment(&t.distance)],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_pipeline(&self.sky);
            pass.draw(0..3, 0..1);
            pass.set_pipeline(&self.scene);
            pass.draw(0..self.vertex_count, 0..1);
            pass.set_pipeline(&self.capsule);
            pass.draw(0..3, 0..1);
        }
        let mut optical_attachment = attachment(&t.optical).unwrap();
        if !first {
            optical_attachment.ops.load = wgpu::LoadOp::Load;
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fog additive optical depth"),
                timestamp_writes: queries.map(|query_set| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: Some(4),
                    end_of_pass_write_index: None,
                }),
                color_attachments: &[Some(optical_attachment)],
                ..Default::default()
            });
            if height_mode {
                pass.set_pipeline(&self.height_fog);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_bind_group(1, &t.height_bind, &[]);
                pass.draw(0..3, 0..1);
            } else if session.visible && (session.debug_view == 0 || session.debug_view == 5) {
                pass.set_pipeline(&self.volume);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_bind_group(1, &t.volume_bind, &[]);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed_indirect(&self.draws, 0);
                pass.set_pipeline(&self.fallback);
                pass.draw_indirect(&self.draws, 20);
            }
        }
        if !height_mode {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fog opacity occlusion mask"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.occlusion,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.opacity_mask);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_bind_group(1, &t.composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        if !height_mode {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("far fog with early opacity rejection"),
                timestamp_writes: queries.map(|query_set| wgpu::RenderPassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: Some(5),
                }),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.optical,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.occlusion,
                    depth_ops: None,
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            if session.visible && (session.debug_view == 0 || session.debug_view == 5) {
                pass.set_bind_group(0, &self.bind, &[]);
                pass.set_bind_group(1, &t.volume_bind, &[]);
                pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
                pass.set_pipeline(&self.far_volume);
                pass.draw_indexed_indirect(&self.draws, 40);
                pass.set_pipeline(&self.far_fallback);
                pass.draw_indirect(&self.draws, 60);
            }
        }
        if last {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("fog optical composition"),
                timestamp_writes: timestamps(6),
                color_attachments: &[attachment(target)],
                ..Default::default()
            });
            pass.set_pipeline(&self.composite);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_bind_group(1, &t.composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

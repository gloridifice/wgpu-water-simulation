//! Opaque seabed prepass: linear scene color + sampleable scene depth.
pub struct Seabed {
    pub color: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    pub model: Option<crate::ocean::render::model::ModelRenderer>,
    pub ground_vertices: u32,
    bind: wgpu::BindGroup,
    prepass: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    collider_prepass: wgpu::RenderPipeline,
    collider_composite: wgpu::RenderPipeline,
}

impl Seabed {
    pub(super) fn new(
        gpu: &crate::ocean::gpu::context::Gpu,
        camera: &wgpu::Buffer,
        optics: &wgpu::Buffer,
        shore: &crate::ocean::simulation::nearshore::Shore,
        caustics: &super::caustics::Caustics,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> Self {
        let [width, height] = size;
        let device = &gpu.device;
        let maps = crate::ocean::render::textures::sand::textures(gpu);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sand repeat trilinear"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let mut entries: Vec<_> = (0..5)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 2 || binding == 4 {
                        wgpu::BufferBindingType::Storage { read_only: true }
                    } else {
                        wgpu::BufferBindingType::Uniform
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        for binding in 5..9 {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 9,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 10,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 11,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 12,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 13,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("seabed uniforms"),
            entries: &entries,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("seabed bindings"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: caustics.fields.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::TextureView(&caustics.view),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: wgpu::BindingResource::Sampler(&caustics.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: shore.terrain_bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&maps[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&maps[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&maps[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&maps[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: shore.lace.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: shore.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: shore.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: optics.as_entire_binding(),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("seabed layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("seabed"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}\n{}",
                    concat!(
                        include_str!("shaders/scene_types.wgsl"),
                        "\n",
                        include_str!("shaders/water_light.wgsl"),
                        "\n",
                        include_str!("shaders/caustic_sample.wgsl"),
                        "\n",
                        include_str!("shaders/surface.wgsl"),
                        "\n",
                        include_str!("shaders/seabed.wgsl")
                    ),
                    include_str!("../simulation/nearshore/shaders/shore.wgsl"),
                    include_str!("../simulation/nearshore/shaders/shore_read.wgsl"),
                    include_str!("shaders/collider_render.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = |format: wgpu::TextureFormat, vertex, fragment| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("seabed pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let color = Self::target(device, width, height, wgpu::TextureFormat::Rgba16Float);
        let depth = Self::target(device, width, height, wgpu::TextureFormat::Depth32Float);
        Self {
            model: None,
            ground_vertices: 6,
            color,
            depth,
            bind,
            prepass: pipeline(
                wgpu::TextureFormat::Rgba16Float,
                "ground_vertex",
                "ground_fragment",
            ),
            composite: pipeline(format, "ground_vertex", "ground_fragment"),
            collider_prepass: pipeline(
                wgpu::TextureFormat::Rgba16Float,
                "collider_vertex",
                "collider_fragment",
            ),
            collider_composite: pipeline(format, "collider_vertex", "collider_fragment"),
        }
    }

    fn target(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("seabed prepass target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.color = Self::target(device, width, height, wgpu::TextureFormat::Rgba16Float);
        self.depth = Self::target(device, width, height, wgpu::TextureFormat::Depth32Float);
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("opaque seabed color and depth"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.prepass);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..self.ground_vertices, 0..1);
        pass.set_pipeline(&self.collider_prepass);
        pass.draw(
            0..576,
            0..crate::ocean::scene::collider::MAX_COLLIDERS as u32,
        );
        if let Some(model) = &self.model {
            model.draw(&mut pass, true);
        }
    }

    /// Redraw ground and colliders into the final depth buffer. This handles
    /// occlusion without sampling a texture that is also a writable attachment.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..self.ground_vertices, 0..1);
        pass.set_pipeline(&self.collider_composite);
        pass.draw(
            0..576,
            0..crate::ocean::scene::collider::MAX_COLLIDERS as u32,
        );
        if let Some(model) = &self.model {
            model.draw(pass, false);
        }
    }
}

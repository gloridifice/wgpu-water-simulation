//! Opaque seabed prepass: linear scene color + sampleable scene depth.
pub struct Seabed {
    pub color: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    bind: wgpu::BindGroup,
    prepass: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
}

impl Seabed {
    pub fn new(
        device: &wgpu::Device,
        camera: &wgpu::Buffer,
        optics: &wgpu::Buffer,
        shore: &super::shore::Shore,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let entries: Vec<_> = (0..5)
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("seabed uniforms"),
            entries: &entries,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("seabed bindings"),
            layout: &layout,
            entries: &[
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
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("seabed"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("seabed.wgsl"),
                    include_str!("shore.wgsl"),
                    include_str!("shore_read.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = |format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("seabed pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("ground_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("ground_fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview: None,
                cache: None,
            })
        };
        let color = Self::target(device, width, height, wgpu::TextureFormat::Rgba16Float);
        let depth = Self::target(device, width, height, wgpu::TextureFormat::Depth32Float);
        Self {
            color,
            depth,
            bind,
            prepass: pipeline(wgpu::TextureFormat::Rgba16Float),
            composite: pipeline(format),
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
        pass.draw(0..6, 0..1);
    }

    /// Redraw just two triangles into the final depth buffer. This handles
    /// occlusion without sampling a texture that is also a writable attachment.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..6, 0..1);
    }
}

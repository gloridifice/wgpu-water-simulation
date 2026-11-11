//! Lake opaque prepass/composite. Uses the exact height field used for water masking.
use crate::ocean::{
    Error,
    gpu::context::Gpu,
    parameters::lake::LakeConfig,
    simulation::{animated_lake::AnimatedLake, spectral::FftWaves},
};
pub(super) struct LakeRenderer {
    pub simulation: AnimatedLake,
    pub indices: wgpu::Buffer,
    camera: wgpu::BindGroup,
    prepass: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
}
impl LakeRenderer {
    pub fn new(
        gpu: &Gpu,
        config: LakeConfig,
        layout: &wgpu::BindGroupLayout,
        camera: &wgpu::Buffer,
        optics: &wgpu::Buffer,
        format: wgpu::TextureFormat,
        spectrum: &FftWaves,
    ) -> Result<Self, Error> {
        let simulation = AnimatedLake::new(gpu, config, layout, spectrum)?;
        let camera_layout = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("lake ground uniforms"),
                entries: &[0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }),
            });
        let camera = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lake ground camera and optics"),
            layout: &camera_layout,
            entries: &[
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
        let pipeline_layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("lake ground"),
                bind_group_layouts: &[Some(&camera_layout), Some(layout)],
                immediate_size: 0,
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("lake terrain"),
                source: wgpu::ShaderSource::Wgsl(
                    concat!(
                        include_str!("shaders/scene_types.wgsl"),
                        "\n",
                        include_str!("shaders/lake_sample.wgsl"),
                        "\n",
                        include_str!("shaders/lake_ground.wgsl")
                    )
                    .into(),
                ),
            });
        let make = |format: wgpu::TextureFormat| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("lake ground"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("ground_vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: None,
                        ..Default::default()
                    },
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
                        entry_point: Some("ground_fragment"),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
        };
        let indices = super::mesh::square_indices(&gpu.device, simulation.n - 1);
        Ok(Self {
            simulation,
            indices,
            camera,
            prepass: make(wgpu::TextureFormat::Rgba16Float),
            composite: make(format),
        })
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, prepass: bool) {
        pass.set_pipeline(if prepass {
            &self.prepass
        } else {
            &self.composite
        });
        pass.set_bind_group(0, &self.camera, &[]);
        pass.set_bind_group(1, &self.simulation.bind, &[]);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..6 * (self.simulation.n - 1).pow(2), 0, 0..1);
    }
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("lake opaque scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        self.draw(&mut pass, true);
    }
}

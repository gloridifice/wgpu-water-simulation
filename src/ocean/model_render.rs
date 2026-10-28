//! Opaque unlit model in both scene-depth and final passes, plus the SWE box.
use super::{
    gpu::{Error, Gpu},
    model::{self, Model, Vertex},
    optics::OpticsConfig,
    shore::Shore,
    terrain::HeightField,
};
use wgpu::util::DeviceExt;

pub struct ModelAssets {
    pub model: Model,
    pub height: HeightField,
}
impl ModelAssets {
    pub fn load() -> Result<Self, Error> {
        let mut model = Model::load(&model::asset_path(model::MODEL_PATH))?;
        let path = model::asset_path(model::HEIGHT_PATH);
        let height = HeightField::load(&path, &model.source_hash).map_err(|e| {
            format!(
                "{}: {e}; run cargo run --release --bin terrain-bake",
                path.display()
            )
        })?;
        if height.metadata.cap_epsilon > 0.0 {
            model.remove_scan_caps(height.metadata.cap_epsilon);
        }
        Ok(Self { model, height })
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    transform: [[f32; 4]; 4],
    domain: [f32; 4],
    low: [f32; 4],
    high: [f32; 4],
    options: [f32; 4],
}
pub struct ModelRenderer {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    texture: wgpu::TextureView,
    sampler: wgpu::Sampler,
    prepass: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    pub enabled: bool,
    show_bounds: bool,
}
impl ModelRenderer {
    pub fn new(
        gpu: &Gpu,
        camera: &wgpu::Buffer,
        shore: &Shore,
        model: &Model,
        format: wgpu::TextureFormat,
    ) -> Result<Self, Error> {
        let device = &gpu.device;
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("coast scan vertices"),
            contents: bytemuck::cast_slice(&model.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("coast scan indices without artificial caps"),
            contents: bytemuck::cast_slice(&model.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("model transform and domain box"),
            contents: bytemuck::bytes_of(&Uniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let texture = super::sand::color_texture(gpu, &model.image)?;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("model trilinear"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        });
        let entries = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: match binding {
                    2 => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    3 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    _ => wgpu::BindingType::Buffer {
                        ty: if binding < 2 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage { read_only: true }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                },
                count: None,
            })
            .collect::<Vec<_>>();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model render layout"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("model pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("unlit coast scan"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/model_render.wgsl").into()),
        });
        let attributes = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x2];
        let buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &attributes,
        }];
        let pipeline = |target: wgpu::TextureFormat, lines: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("coast mesh/box"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if lines { "box_vertex" } else { "model_vertex" }),
                    compilation_options: Default::default(),
                    buffers: if lines { &[] } else { &buffers },
                },
                primitive: wgpu::PrimitiveState {
                    topology: if lines {
                        wgpu::PrimitiveTopology::LineList
                    } else {
                        wgpu::PrimitiveTopology::TriangleList
                    },
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: !lines,
                    depth_compare: if lines {
                        wgpu::CompareFunction::Always
                    } else {
                        wgpu::CompareFunction::LessEqual
                    },
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if lines {
                        "box_fragment"
                    } else {
                        "model_fragment"
                    }),
                    compilation_options: Default::default(),
                    targets: &[Some(target.into())],
                }),
                multiview: None,
                cache: None,
            })
        };
        let bind = Self::bind(gpu, &layout, camera, &uniform, &texture, &sampler, shore);
        Ok(Self {
            vertices,
            indices,
            count: model.indices.len() as u32,
            uniform,
            bind,
            layout,
            texture,
            sampler,
            prepass: pipeline(wgpu::TextureFormat::Rgba16Float, false),
            composite: pipeline(format, false),
            lines: pipeline(format, true),
            enabled: false,
            show_bounds: false,
        })
    }
    fn bind(
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        camera: &wgpu::Buffer,
        uniform: &wgpu::Buffer,
        texture: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        shore: &Shore,
    ) -> wgpu::BindGroup {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model bindings"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(texture),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: shore.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: shore.terrain_bed.as_entire_binding(),
                },
            ],
        })
    }
    pub fn rebind(&mut self, gpu: &Gpu, camera: &wgpu::Buffer, shore: &Shore) {
        self.bind = Self::bind(
            gpu,
            &self.layout,
            camera,
            &self.uniform,
            &self.texture,
            &self.sampler,
            shore,
        );
    }
    pub fn update(&mut self, gpu: &Gpu, optics: &OpticsConfig, n: u32) {
        let c = optics.model;
        self.enabled = c.enabled;
        self.show_bounds = c.show_bounds;
        let low = std::array::from_fn(|a| {
            if a < 3 {
                c.center[a] - c.size[a] * 0.5 - if a == 1 { c.sea_level } else { 0.0 }
            } else {
                0.0
            }
        });
        let high = std::array::from_fn(|a| {
            if a < 3 {
                c.center[a] + c.size[a] * 0.5 - if a == 1 { c.sea_level } else { 0.0 }
            } else {
                0.0
            }
        });
        let value = Uniform {
            transform: c.transform().to_cols_array_2d(),
            domain: c.domain(),
            low,
            high,
            options: [
                n as f32,
                f32::from(optics.swe_enabled()),
                0.0,
                optics.shore.view as f32,
            ],
        };
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&value));
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, prepass: bool) {
        if !self.enabled {
            return;
        }
        pass.set_pipeline(if prepass {
            &self.prepass
        } else {
            &self.composite
        });
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
    }
    pub fn draw_bounds(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.enabled && self.show_bounds {
            pass.set_pipeline(&self.lines);
            pass.set_bind_group(0, &self.bind, &[]);
            pass.draw(0..24, 0..1);
        }
    }
}
use bytemuck::Zeroable;

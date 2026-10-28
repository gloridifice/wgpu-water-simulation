//! 独立 demo 的表面预览渲染器；输入完全来自实际提取的 SurfaceMesh。
//! 水材质只用于观察几何，未实现透明、屏幕折射或体散射。

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

use super::gpu_timer::SceneGpuTimer;
use super::gui::{Gui, GuiAction, PanelData, RenderTiming};
use crate::surface::SurfaceMesh;

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// 绕 y 轴旋转的弧度。
    pub yaw: f32,
    /// 仰角，弧度。
    pub pitch: f32,
    /// 相机到目标的距离，米。
    pub distance: f32,
    pub target: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Water,
    Mesh,
    Normals,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
    camera: [f32; 4],
    extent: [f32; 4],
    control: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    barycentric: [f32; 3],
}

struct Pipelines {
    ground: wgpu::RenderPipeline,
    water: wgpu::RenderPipeline,
    frame: wgpu::RenderPipeline,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    minimized: bool,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipelines: Pipelines,
    snapshot_pipelines: Pipelines,
    vertices: wgpu::Buffer,
    vertex_count: u32,
    frame: wgpu::Buffer,
    frame_count: u32,
    extent: [f32; 3],
    depth: wgpu::Texture,
    gui: Gui,
    timer: SceneGpuTimer,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::default();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("无法创建窗口渲染适配器：{e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("tall-cell standalone preview"),
                required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("窗口 surface 没有可用配置")?;
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(format) = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
        {
            config.format = format;
        }
        let maximum = device.limits().max_texture_dimension_2d;
        config.width = config.width.min(maximum);
        config.height = config.height.min(maximum);
        config.present_mode = wgpu::PresentMode::Fifo;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&device, &config);

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("preview camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("preview camera"),
            contents: bytemuck::bytes_of(&Uniforms::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("preview camera bind"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("preview pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tall cell surface preview"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/render.wgsl").into()),
        });
        let pipelines = make_pipelines(&device, &pipeline_layout, &shader, config.format);
        let snapshot_pipelines = make_pipelines(
            &device,
            &pipeline_layout,
            &shader,
            wgpu::TextureFormat::Rgba8UnormSrgb,
        );
        if let Some(error) = device.pop_error_scope().await {
            return Err(format!("预览着色器初始化失败：{error}"));
        }
        let vertices = vertex_buffer(&device, "empty preview surface", &[]);
        let frame = vertex_buffer(&device, "empty tank frame", &[]);
        let depth = depth_texture(&device, config.width, config.height);
        let gui = Gui::new(&window, &device, config.format);
        let timer = SceneGpuTimer::new(&device, &queue);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            minimized: size.width == 0 || size.height == 0,
            uniform,
            bind,
            pipelines,
            snapshot_pipelines,
            vertices,
            vertex_count: 0,
            frame,
            frame_count: 0,
            extent: [1.0; 3],
            depth,
            gui,
            timer,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.minimized = width == 0 || height == 0;
        if self.minimized {
            return;
        }
        let maximum = self.device.limits().max_texture_dimension_2d;
        self.config.width = width.min(maximum);
        self.config.height = height.min(maximum);
        self.surface.configure(&self.device, &self.config);
    }

    pub fn handle_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        self.gui.handle_event(window, event)
    }

    /// 只展开真实索引，为网格预览增加 barycentric 属性；不重建或合成水面。
    pub fn upload(&mut self, mesh: &SurfaceMesh, extent: [f32; 3]) -> Result<(), String> {
        if extent.iter().any(|x| !x.is_normal() || *x <= 0.0)
            || !mesh.indices.len().is_multiple_of(3)
            || mesh.indices.len() > u32::MAX as usize
            || mesh.indices.len() as u64 * size_of::<Vertex>() as u64
                > self.device.limits().max_buffer_size
        {
            return Err("预览尺寸、三角形索引数量或缓冲区大小无效".into());
        }
        let mut vertices = Vec::with_capacity(mesh.indices.len());
        for (n, &index) in mesh.indices.iter().enumerate() {
            let source = mesh
                .vertices
                .get(index as usize)
                .ok_or("预览三角形索引越界")?;
            if source
                .position
                .iter()
                .chain(&source.normal)
                .any(|v| !v.is_finite())
            {
                return Err("预览表面含非有限位置或法线".into());
            }
            let mut barycentric = [0.0; 3];
            barycentric[n % 3] = 1.0;
            vertices.push(Vertex {
                position: source.position,
                normal: source.normal,
                barycentric,
            });
        }
        let frame = tank_frame(extent);
        let vertex_buffer = vertex_buffer(
            &self.device,
            "simulation surface",
            bytemuck::cast_slice(&vertices),
        );
        let frame_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("simulation domain frame"),
                contents: bytemuck::cast_slice(&frame),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.vertices = vertex_buffer;
        self.vertex_count = vertices.len() as u32;
        self.frame = frame_buffer;
        self.frame_count = frame.len() as u32;
        self.extent = extent;
        Ok(())
    }

    pub fn draw(
        &mut self,
        window: &Window,
        camera: Camera,
        mode: ViewMode,
        data: &PanelData,
    ) -> Result<Vec<GuiAction>, String> {
        if self.minimized {
            return Ok(Vec::new());
        }
        // 在 egui 构建前读取最近已完成的样本；绝不把 CPU 绘制提交时间标成 GPU 耗时。
        self.timer.poll(&self.device);
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(Vec::new());
            }
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => return Ok(Vec::new()),
            Err(error) => return Err(format!("预览 surface 失败：{error}")),
        };
        let timing = RenderTiming {
            supported: self.timer.supported,
            failed: self.timer.failed,
            latest_ms: self.timer.latest_ms,
        };
        let gui_frame = self.gui.prepare(window, &self.device, data, timing);
        let [width, height] = self.gui.viewport_size;
        if width == 0 || height == 0 {
            return Err("GUI 场景纹理尺寸不能为零".into());
        }
        if self.depth.width() != width || self.depth.height() != height {
            self.depth = depth_texture(&self.device, width, height);
        }
        self.update_uniform(camera, mode, width, height)?;
        let view = frame.texture.create_view(&Default::default());
        let depth = self.depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let timed = self.timer.ready();
        self.render_pass(
            &mut encoder,
            &self.gui.viewport_view,
            &depth,
            &self.pipelines,
            self.timer.timestamp_writes(),
        );
        if timed {
            self.timer.resolve(&mut encoder);
        }
        // egui pass 位于计时结束之后，只把离屏场景合成到窗口。
        let additional = self.gui.encode(
            window,
            &self.device,
            &self.queue,
            &mut encoder,
            &view,
            gui_frame.output,
        );
        self.queue.submit(
            additional
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        frame.present();
        Ok(gui_frame.actions)
    }

    /// 按当前预览尺寸离屏渲染同一网格和相机，PNG 路径必须不存在。
    pub fn snapshot(&mut self, camera: Camera, mode: ViewMode, path: &Path) -> Result<(), String> {
        let width = self.config.width;
        let height = self.config.height;
        let row_bytes = width as u64 * 4;
        let padded_row_bytes = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as u64;
        let byte_count = padded_row_bytes
            .checked_mul(height as u64)
            .ok_or("截图大小溢出")?;
        if byte_count > self.device.limits().max_buffer_size {
            return Err("截图超过 GPU 回读缓冲区上限".into());
        }
        self.update_uniform(camera, mode, width, height)?;
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("preview offscreen snapshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = depth_texture(&self.device, width, height);
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("preview snapshot readback"),
            size: byte_count,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.render_pass(
            &mut encoder,
            &target.create_view(&Default::default()),
            &depth.create_view(&Default::default()),
            &self.snapshot_pipelines,
            None,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row_bytes as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mapped = readback.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((row_bytes * height as u64) as usize);
        for row in mapped.chunks_exact(padded_row_bytes as usize) {
            pixels.extend_from_slice(&row[..row_bytes as usize]);
        }
        drop(mapped);
        readback.unmap();
        let file = std::fs::File::create_new(path)
            .map_err(|e| format!("无法创建截图 {}：{e}", path.display()))?;
        let mut output = std::io::BufWriter::new(file);
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer
            .write_image_data(&pixels)
            .map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())
    }

    fn update_uniform(
        &self,
        camera: Camera,
        mode: ViewMode,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if !camera.yaw.is_finite()
            || !camera.pitch.is_finite()
            || !camera.distance.is_finite()
            || camera.distance <= 0.0
            || camera.target.iter().any(|v| !v.is_finite())
        {
            return Err("预览相机参数不是有限有效值".into());
        }
        let pitch = camera.pitch.clamp(-1.5, 1.5);
        let target = Vec3::from_array(camera.target);
        let eye = target
            + camera.distance
                * Vec3::new(
                    pitch.cos() * camera.yaw.sin(),
                    pitch.sin(),
                    pitch.cos() * camera.yaw.cos(),
                );
        let scale = self.extent.into_iter().fold(0.0f32, f32::max);
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let projection = Mat4::perspective_rh(
            45.0f32.to_radians(),
            width as f32 / height as f32,
            (scale * 0.002).max(1e-5),
            (scale * 100.0 + camera.distance * 4.0).max(10.0),
        );
        let view_projection = projection * view;
        let inverse = view_projection.inverse();
        if !view_projection.is_finite() || !inverse.is_finite() {
            return Err("预览相机矩阵不可表示".into());
        }
        let uniform = Uniforms {
            view_projection: view_projection.to_cols_array_2d(),
            inverse_view_projection: inverse.to_cols_array_2d(),
            camera: [eye.x, eye.y, eye.z, 0.0],
            extent: [self.extent[0], self.extent[1], self.extent[2], scale],
            control: [
                match mode {
                    ViewMode::Water => 0,
                    ViewMode::Mesh => 1,
                    ViewMode::Normals => 2,
                },
                0,
                0,
                0,
            ],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        Ok(())
    }

    fn render_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        pipelines: &Pipelines,
        timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("standalone fluid preview"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
            timestamp_writes,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_pipeline(&pipelines.ground);
        pass.draw(0..3, 0..1);
        if self.vertex_count > 0 {
            pass.set_pipeline(&pipelines.water);
            pass.set_vertex_buffer(0, self.vertices.slice(..));
            pass.draw(0..self.vertex_count, 0..1);
        }
        if self.frame_count > 0 {
            pass.set_pipeline(&pipelines.frame);
            pass.set_vertex_buffer(0, self.frame.slice(..));
            pass.draw(0..self.frame_count, 0..1);
        }
    }
}

fn vertex_buffer(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: if bytes.is_empty() { &[0; 4] } else { bytes },
        usage: wgpu::BufferUsages::VERTEX,
    })
}

fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("preview depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

fn tank_frame(extent: [f32; 3]) -> Vec<[f32; 3]> {
    let corners = std::array::from_fn::<_, 8, _>(|i| {
        [
            (i & 1) as f32 * extent[0],
            ((i >> 1) & 1) as f32 * extent[1],
            ((i >> 2) & 1) as f32 * extent[2],
        ]
    });
    let mut vertices = Vec::with_capacity(24);
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                vertices.extend([corners[i], corners[i | bit]]);
            }
        }
    }
    vertices
}

fn make_pipelines(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> Pipelines {
    const SURFACE_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
    const FRAME_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];
    let surface_layout = wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &SURFACE_ATTRIBUTES,
    };
    let frame_layout = wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &FRAME_ATTRIBUTES,
    };
    let pipeline =
        |vertex, fragment, buffers: &[wgpu::VertexBufferLayout<'_>], topology, depth_write| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fragment),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(vertex),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: depth_write,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };
    Pipelines {
        ground: pipeline(
            "ground_vertex",
            "ground_fragment",
            &[],
            wgpu::PrimitiveTopology::TriangleList,
            true,
        ),
        water: pipeline(
            "water_vertex",
            "water_fragment",
            &[surface_layout],
            wgpu::PrimitiveTopology::TriangleList,
            true,
        ),
        frame: pipeline(
            "frame_vertex",
            "frame_fragment",
            &[frame_layout],
            wgpu::PrimitiveTopology::LineList,
            false,
        ),
    }
}

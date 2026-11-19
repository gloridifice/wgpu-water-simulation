//! egui platform/GPU integration and application-independent viewport targets.
pub mod layout;
use crate::gpu::Gpu;
use winit::window::Window;
pub struct Egui {
    pub context: egui::Context,
    pub input: egui_winit::State,
    pub renderer: egui_wgpu::Renderer,
}
impl Egui {
    pub fn new(window: &Window, gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let context = egui::Context::default();
        yummy_egui::Theme::default().apply(&context);
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let renderer = egui_wgpu::Renderer::new(&gpu.device, format, Default::default());
        Self {
            context,
            input,
            renderer,
        }
    }
    pub fn encode(
        &mut self,
        window: &Window,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        output: egui::FullOutput,
    ) -> Vec<wgpu::CommandBuffer> {
        self.input
            .handle_platform_output(window, output.platform_output);
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        let jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let size = window.inner_size();
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: output.pixels_per_point,
        };
        let commands =
            self.renderer
                .update_buffers(&gpu.device, &gpu.queue, encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui overlay"),
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
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        commands
    }
}
pub struct Viewport {
    pub view: wgpu::TextureView,
    pub id: egui::TextureId,
    texture: wgpu::Texture,
}
impl Viewport {
    fn create_texture(gpu: &Gpu, format: wgpu::TextureFormat, size: [u32; 2]) -> wgpu::Texture {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("docked scene view"),
            size: wgpu::Extent3d {
                width: size[0].max(1),
                height: size[1].max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[format.remove_srgb_suffix()],
        })
    }
    pub fn new(backend: &mut Egui, gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let texture = Self::create_texture(gpu, format, [1, 1]);
        let view = texture.create_view(&Default::default());
        // egui expects the encoded display color, not a second sRGB decode.
        let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        let id = backend.renderer.register_native_texture(
            &gpu.device,
            &sampled,
            wgpu::FilterMode::Linear,
        );
        Self { texture, view, id }
    }
    pub fn texture(&self) -> &wgpu::Texture {
        &self.texture
    }
    pub fn resize(
        &mut self,
        backend: &mut Egui,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) {
        self.texture = Self::create_texture(gpu, format, size);
        self.view = self.texture.create_view(&Default::default());
        let sampled = self.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        backend.renderer.update_egui_texture_from_wgpu_texture(
            &gpu.device,
            &sampled,
            wgpu::FilterMode::Linear,
            self.id,
        );
    }
}

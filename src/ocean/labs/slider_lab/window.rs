use super::Demo;
use crate::ocean::{Error, gpu::context::Gpu, ui::fonts};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

struct WindowState {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    context: egui::Context,
    input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    demo: Demo,
    frames: u32,
}

impl WindowState {
    async fn new(window: Arc<Window>) -> Result<Self, Error> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let gpu = Gpu::new(&instance, Some(&surface)).await?;
        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("no window surface configuration")?;
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &surface_config);
        let context = egui::Context::default();
        charming_egui::Theme::default().apply(&context);
        fonts::install(&context);
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let renderer =
            egui_wgpu::Renderer::new(&gpu.device, surface_config.format, Default::default());
        eprintln!("Slider Lab | {} | no simulation or settings", gpu.name);
        Ok(Self {
            window,
            surface,
            surface_config,
            gpu,
            context,
            input,
            renderer,
            demo: Demo::default(),
            frames: 0,
        })
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.surface_config.width = size.width;
            self.surface_config.height = size.height;
            self.surface
                .configure(&self.gpu.device, &self.surface_config);
        }
    }

    fn draw(&mut self) -> Result<(), Error> {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.resize(self.window.inner_size());
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let input = self.input.take_egui_input(&self.window);
        let output = self.context.run(input, |context| self.demo.ui(context));
        self.input
            .handle_platform_output(&self.window, output.platform_output);
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }
        let jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let commands = self.renderer.update_buffers(
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            &jobs,
            &screen,
        );
        {
            let view = frame.texture.create_view(&Default::default());
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("slider lab UI"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
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
        self.gpu
            .queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        self.window.pre_present_notify();
        frame.present();
        self.frames += 1;
        Ok(())
    }
}

#[derive(Default)]
struct App {
    state: Option<WindowState>,
    error: Option<String>,
    frame_limit: Option<u32>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let result = (|| -> Result<WindowState, Error> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("Slider Lab | 矩形滑块外观实验")
                        .with_inner_size(winit::dpi::LogicalSize::new(1040.0, 720.0))
                        .with_min_inner_size(winit::dpi::LogicalSize::new(820.0, 560.0)),
                )?,
            );
            pollster::block_on(WindowState::new(window))
        })();
        match result {
            Ok(state) => self.state = Some(state),
            Err(error) => {
                self.error = Some(error.to_string());
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else { return };
        if state.window.id() != id {
            return;
        }
        let response = state.input.on_window_event(&state.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::KeyboardInput { event, .. }
                if !response.consumed
                    && event.state == ElementState::Pressed
                    && event.physical_key == PhysicalKey::Code(KeyCode::Escape) =>
            {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested
                if state.window.inner_size().width > 0 && state.window.inner_size().height > 0 =>
            {
                if let Err(error) = state.draw() {
                    self.error = Some(error.to_string());
                    event_loop.exit();
                }
                if self.frame_limit.is_some_and(|limit| state.frames >= limit) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

pub fn run(frames: Option<u32>) -> Result<(), Error> {
    if frames == Some(0) {
        return Err("--frames must be positive".into());
    }
    let mut app = App {
        frame_limit: frames,
        ..Default::default()
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.error.map_or(Ok(()), |error| Err(error.into()))
}

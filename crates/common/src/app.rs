//! A single-window event loop. Applications own input policy and simulation time.
use crate::{Error, gpu::Gpu};
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

pub struct Desktop {
    pub window: Arc<Window>,
    pub surface: wgpu::Surface<'static>,
    pub instance: wgpu::Instance,
    pub gpu: Gpu,
    pub config: wgpu::SurfaceConfiguration,
}
impl Desktop {
    pub async fn new(window: Arc<Window>) -> Result<Self, Error> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let gpu = Gpu::new(&instance, Some(&surface)).await?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("adapter cannot present to this window")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        Ok(Self {
            window,
            surface,
            instance,
            gpu,
            config,
        })
    }
}
/// None means that presentation was unavailable this frame, not a simulation fault.
pub fn acquire(
    window: &Arc<Window>,
    instance: &wgpu::Instance,
    surface: &mut wgpu::Surface<'static>,
    gpu: &Gpu,
    config: &wgpu::SurfaceConfiguration,
) -> Result<Option<wgpu::SurfaceTexture>, Error> {
    match surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(Some(frame)),
        wgpu::CurrentSurfaceTexture::Outdated => {
            surface.configure(&gpu.device, config);
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Lost => {
            *surface = instance.create_surface(window.clone())?;
            surface.configure(&gpu.device, config);
            Ok(None)
        }
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Ok(None),
        wgpu::CurrentSurfaceTexture::Validation => Err("surface texture validation failed".into()),
    }
}

pub trait WindowScene {
    fn window(&self) -> &Window;
    /// Return true to request exit. Route egui events before scene controls here.
    fn event(&mut self, event: WindowEvent) -> bool;
    /// Return true only for a successfully presented frame.
    fn draw(&mut self) -> Result<bool, Error>;
    /// Service external scene commands even when the window cannot present a frame.
    fn poll_commands(&mut self) -> Result<(), Error> {
        Ok(())
    }
    /// Optional raw input for captured-pointer applications; regular UI apps ignore it.
    fn device_event(&mut self, _event: DeviceEvent) {}
    fn exiting(&mut self) {}
}

pub fn run<S: WindowScene, F: FnOnce(Arc<Window>) -> Result<S, Error>>(
    attributes: WindowAttributes,
    frames: Option<u32>,
    factory: F,
) -> Result<(), Error> {
    struct App<S, F> {
        scene: Option<S>,
        factory: Option<F>,
        attributes: WindowAttributes,
        limit: Option<u32>,
        frames: u32,
        error: Option<String>,
    }
    impl<S: WindowScene, F: FnOnce(Arc<Window>) -> Result<S, Error>> ApplicationHandler for App<S, F> {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.scene.is_some() {
                return;
            }
            let result = event_loop
                .create_window(self.attributes.clone())
                .map_err(|e| -> Error { e.into() })
                .and_then(|w| self.factory.take().expect("single window factory")(Arc::new(w)));
            match result {
                Ok(scene) => self.scene = Some(scene),
                Err(e) => {
                    self.error = Some(e.to_string());
                    event_loop.exit();
                }
            }
        }
        fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
            let Some(scene) = &mut self.scene else {
                return;
            };
            if scene.window().id() != id {
                return;
            }
            match event {
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::RedrawRequested => {
                    let size = scene.window().inner_size();
                    if size.width == 0 || size.height == 0 {
                        return;
                    }
                    match scene.draw() {
                        Ok(presented) => {
                            self.frames += u32::from(presented);
                            if self.limit.is_some_and(|n| self.frames >= n) {
                                event_loop.exit();
                            }
                        }
                        Err(e) => {
                            self.error = Some(e.to_string());
                            event_loop.exit();
                        }
                    }
                }
                _ => {
                    if scene.event(event) {
                        event_loop.exit();
                    }
                }
            }
        }
        fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
            if let Some(scene) = &mut self.scene {
                scene.device_event(event);
            }
        }
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(scene) = &mut self.scene {
                if let Err(e) = scene.poll_commands() {
                    self.error = Some(e.to_string());
                    event_loop.exit();
                } else {
                    scene.window().request_redraw();
                }
            }
        }
        fn exiting(&mut self, _: &ActiveEventLoop) {
            if let Some(scene) = &mut self.scene {
                scene.exiting();
            }
        }
    }
    let mut app = App {
        scene: None,
        factory: Some(factory),
        attributes,
        limit: frames,
        frames: 0,
        error: None,
    };
    EventLoop::new()?.run_app(&mut app)?;
    match app.error {
        Some(e) => Err(e.into()),
        None => Ok(()),
    }
}

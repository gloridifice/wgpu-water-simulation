//! A bare scene window: no egui, dock, viewport texture or layout persistence.
use super::headless::Scene;
use crate::ocean::{
    Error,
    settings::{SavedSettings, SettingsStore},
};
use common::{
    app::{Desktop, WindowScene},
    control::Request,
};
use std::{
    sync::{
        Arc,
        mpsc::{Receiver, sync_channel},
    },
    time::Instant,
};
use winit::{
    event::WindowEvent,
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

struct State {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scene: Scene,
    commands: Receiver<Request>,
    last: Instant,
}
impl WindowScene for State {
    fn window(&self) -> &Window {
        &self.window
    }
    fn event(&mut self, event: WindowEvent) -> bool {
        match event {
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                self.config.width = size.width;
                self.config.height = size.height;
                self.surface
                    .configure(&self.scene.gpu().device, &self.config);
                self.scene.resize([size.width, size.height]);
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state.is_pressed()
                    && event.physical_key == PhysicalKey::Code(KeyCode::KeyQ) =>
            {
                return true;
            }
            _ => {}
        }
        false
    }
    fn poll_commands(&mut self) -> Result<(), Error> {
        // Scene state and GPU resources are owned by this thread, even while minimized.
        while let Ok(request) = self.commands.try_recv() {
            if !request.reply.is_closed() {
                let _ = request.reply.send(self.scene.handle(request.command));
            }
        }
        Ok(())
    }
    fn draw(&mut self) -> Result<bool, Error> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let Some(frame) = common::app::acquire(
            &self.window,
            &self.instance,
            &mut self.surface,
            self.scene.gpu(),
            &self.config,
        )?
        else {
            return Ok(false);
        };
        let dt = if self.scene.settings().paused {
            0.0
        } else {
            elapsed * self.scene.settings().speed
        };
        self.scene
            .step_with_dt(1, dt)
            .map_err(|error| -> Error { error.into() })?;
        self.scene
            .draw_into(&frame.texture.create_view(&Default::default()));
        frame.present();
        Ok(true)
    }
}

pub fn run(
    initial: SavedSettings,
    store: Option<SettingsStore>,
    frames: Option<u32>,
) -> Result<(), Error> {
    initial.validate()?;
    let (tx, rx) = sync_channel::<Request>(32);
    // The winit/Surface owner must run on the main thread; MCP never touches GPU state.
    std::thread::spawn(move || {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Ocean MCP runtime: {error}");
                return;
            }
        };
        if let Err(error) = runtime.block_on(common::control::serve_stdio(tx)) {
            eprintln!("Ocean MCP stdio: {error}");
        }
    });
    common::app::run(
        Window::default_attributes()
            .with_title("Ocean scene (no UI)")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0)),
        frames,
        move |window| {
            let Desktop {
                window,
                surface,
                instance,
                gpu,
                config,
            } = pollster::block_on(Desktop::new(window))?;
            eprintln!("Ocean scene GPU: {}", gpu.name);
            let mut scene = Scene::with_gpu(
                gpu,
                config.format,
                [config.width, config.height],
                initial,
                store,
            )?;
            scene
                .step_with_dt(0, 0.0)
                .map_err(|error| -> Error { error.into() })?;
            Ok(State {
                window,
                surface,
                instance,
                config,
                scene,
                commands: rx,
                last: Instant::now(),
            })
        },
    )
}

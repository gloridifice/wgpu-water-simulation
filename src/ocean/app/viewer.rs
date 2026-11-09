//! Window events, live state and frame scheduling.
use crate::ocean::{
    Error,
    camera::Camera,
    gpu::context::Gpu,
    parameters::ocean::OceanConfig,
    render::Renderer,
    settings::{ConfigurationLibrary, SavedSettings, SettingsStore},
    simulation::offshore::{
        Ocean,
        foam::{Foam, FoamStep},
    },
    ui::{
        gui::Gui,
        panels::configuration::ConfigurationPanel,
        performance::{FrameGpuStatus, FrameTimings, SweStatus},
    },
};
use std::{sync::Arc, time::Instant};
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{KeyCode, PhysicalKey},
    window::Window,
};

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    instance: wgpu::Instance,
    gpu: Gpu,
    ocean: Ocean,
    foam_field: Foam,
    foam_time: f32,
    renderer: Renderer,
    config: wgpu::SurfaceConfiguration,
    camera: Camera,
    gui: Gui,
    time: f32,
    paused: bool,
    render_size: [u32; 2],
    last: Instant,
    settings: Option<SettingsStore>,
    applied_model: crate::ocean::parameters::model::ModelConfig,
    frame_timer: Option<crate::ocean::gpu::timer::GpuTimer>,
}

impl State {
    async fn new(window: Arc<Window>, ocean_config: OceanConfig) -> Result<Self, Error> {
        let common::app::Desktop {
            window,
            surface,
            instance,
            gpu,
            config,
        } = common::app::Desktop::new(window).await?;
        eprintln!("GPU: {}", gpu.name);
        let ocean = Ocean::new(&gpu, ocean_config)?;
        let foam_field = Foam::new(&gpu, &ocean.fields, ocean_config.resolution);
        let mut renderer = Renderer::new_display(
            &gpu,
            &ocean,
            &foam_field,
            config.format,
            config.width,
            config.height,
        );
        renderer.shore.timer = crate::ocean::gpu::timer::GpuTimer::new(&gpu);
        let gui = Gui::new(&window, &gpu, config.format, ocean_config);
        let frame_timer = crate::ocean::gpu::timer::GpuTimer::new(&gpu);
        Ok(Self {
            frame_timer,
            gui,
            window,
            surface,
            instance,
            gpu,
            ocean,
            foam_field,
            foam_time: 0.0,
            renderer,
            config,
            camera: Camera::new(ocean_config.length),
            time: 0.0,
            paused: false,
            render_size: [0, 0],
            last: Instant::now(),
            settings: None,
            applied_model: crate::ocean::parameters::model::ModelConfig::default(),
        })
    }

    fn saved_settings(&self) -> SavedSettings {
        SavedSettings {
            ocean: self.ocean.config,
            draft: self.gui.draft,
            foam: self.gui.foam_settings,
            optics: self.gui.optics,
            camera: self.camera,
            speed: self.gui.speed,
            paused: self.paused,
            ..Default::default()
        }
    }

    fn persist_layout(&mut self, force: bool) {
        if let Some(store) = &mut self.gui.workspace_store {
            store.update(&self.gui.workspaces, Instant::now(), force);
        }
    }

    fn update_settings(&mut self, save_requested: bool) {
        let current = self.saved_settings();
        if let Some(store) = &mut self.settings {
            store.set_configurations(&self.gui.configuration_panel.library);
            store.update(current, save_requested);
            self.gui.save_status = store.status.clone();
            self.gui.save_failed = store.failed;
        }
    }

    fn rebuild_ocean(&mut self, config: OceanConfig) -> Result<(), Error> {
        config.validate()?;
        let spectrum = OceanConfig {
            mesh_resolution: self.ocean.config.mesh_resolution,
            triangular_mesh: self.ocean.config.triangular_mesh,
            ..config
        };
        if spectrum == self.ocean.config {
            self.renderer.set_ocean_mesh(&self.gpu.device, config);
            self.ocean.config = config;
            return Ok(());
        }
        let ocean = Ocean::new(&self.gpu, config)?;
        let foam_field = Foam::new(&self.gpu, &ocean.fields, config.resolution);
        let mut renderer = Renderer::new_display(
            &self.gpu,
            &ocean,
            &foam_field,
            self.config.format,
            self.gui.viewport_size[0],
            self.gui.viewport_size[1],
        );
        renderer.shore.timer = crate::ocean::gpu::timer::GpuTimer::new(&self.gpu);
        renderer.model_assets = self.renderer.model_assets.clone();
        self.ocean = ocean;
        self.foam_field = foam_field;
        self.renderer = renderer;
        self.foam_time = self.time;
        Ok(())
    }

    fn load_configuration(&mut self, settings: SavedSettings) -> Result<(), Error> {
        settings.validate()?;
        // Build GPU resources before committing any of the restored UI values.
        if settings.ocean != self.ocean.config {
            self.rebuild_ocean(settings.ocean)?;
        }
        self.gui.draft = settings.draft;
        self.gui.foam_settings = settings.foam;
        self.gui.optics = settings.optics;
        self.gui.speed = settings.speed;
        self.camera = settings.camera;
        self.paused = settings.paused;
        self.time = 0.0;
        self.gui.clear_foam = true;
        self.gui.error = None;
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.gpu.device, &self.config);
    }

    fn draw(&mut self) -> Result<bool, Error> {
        let now = Instant::now();
        let interval = now.duration_since(self.last).as_secs_f64() * 1000.0;
        let elapsed = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        if !self.paused {
            self.time += elapsed * self.gui.speed;
        }
        let acquire_start = Instant::now();
        let Some(frame) = common::app::acquire(
            &self.window,
            &self.instance,
            &mut self.surface,
            &self.gpu,
            &self.config,
        )?
        else {
            return Ok(false);
        };
        let acquire = acquire_start.elapsed().as_secs_f64() * 1000.0;
        let view = frame.texture.create_view(&Default::default());
        let (status, sample) = if let Some(timer) = &mut self.renderer.shore.timer {
            timer.poll(&self.gpu);
            let sample = timer.latest.take();
            let status = if timer.failed {
                SweStatus::Failed
            } else if self.paused || self.gui.speed == 0.0 || !self.gui.optics.swe_enabled() {
                SweStatus::Idle
            } else {
                SweStatus::Active(self.gui.optics.shore.order)
            };
            (status, sample)
        } else {
            (SweStatus::Unsupported, None)
        };
        self.gui.performance.record_gpu(status, sample);
        let (status, sample) = if let Some(timer) = &mut self.frame_timer {
            timer.poll(&self.gpu);
            (
                if timer.failed {
                    FrameGpuStatus::Failed
                } else {
                    FrameGpuStatus::Sampling
                },
                timer.latest.take(),
            )
        } else {
            (FrameGpuStatus::Unsupported, None)
        };
        self.gui.performance.record_frame_gpu(status, sample);
        self.gui.swe_domain = self
            .renderer
            .shore
            .parameters(&self.gui.optics, self.time, false, self.camera.gain)
            .domain;
        let gui_start = Instant::now();
        let (gui_output, apply) = self.gui.frame(
            &self.window,
            self.ocean.config,
            &mut self.camera,
            &mut self.paused,
            &mut self.time,
        );
        let gui = gui_start.elapsed().as_secs_f64() * 1000.0;
        if let Some(config) = apply {
            let distance_scale = config.length / self.ocean.config.length;
            match self.rebuild_ocean(config) {
                Ok(()) => self.camera.distance *= distance_scale,
                Err(error) => self.gui.error = Some(error.to_string()),
            }
        }
        if let Some(settings) = self.gui.load_configuration.take() {
            let result = self
                .load_configuration(settings)
                .map_err(|error| format!("载入失败：{error}"));
            self.gui.configuration_panel.finish_load(result);
        }
        if self.render_size != self.gui.viewport_size {
            self.render_size = self.gui.viewport_size;
            self.gui.resize_view(&self.gpu, self.config.format);
            self.renderer
                .resize(&self.gpu.device, self.render_size[0], self.render_size[1]);
        }
        self.renderer.set_shore_resolution(
            &self.gpu,
            &self.ocean,
            self.gui.optics.shore.resolution,
            self.config.format,
            self.render_size,
        );
        if let Err(error) =
            self.renderer
                .prepare_scene(&self.gpu, &self.gui.optics, self.config.format)
        {
            self.gui.model_panel.error = Some(error.to_string());
            self.gui.optics.model = self.applied_model;
            // Re-establish the previous valid scene after a failed asset load.
            if let Err(error) =
                self.renderer
                    .prepare_scene(&self.gpu, &self.gui.optics, self.config.format)
            {
                self.gui.error = Some(error.to_string());
                return Ok(false);
            }
        }
        self.applied_model = self.gui.optics.model;
        self.gui.model_panel.source_bounds = self
            .renderer
            .model_assets
            .as_ref()
            .map(|a| a.height.metadata.bounds);
        self.gui.model_panel.bed_range = self.renderer.shore.terrain_height_range();
        self.persist_layout(false);
        let save_requested = std::mem::take(&mut self.gui.save_now);
        self.update_settings(save_requested);
        self.renderer.update_optics(&self.gpu, &self.gui.optics);
        let simulation_start = Instant::now();
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let timed_frame = self.frame_timer.as_ref().is_some_and(|t| t.ready());
        if timed_frame {
            self.frame_timer.as_ref().unwrap().mark(&mut encoder, 0);
        }
        self.ocean
            .encode_filtered(&self.gpu, &mut encoder, self.time, &self.gui.optics.shore);
        let camera = self.camera.uniform(
            self.ocean.config,
            self.render_size[0] as f32 / self.render_size[1] as f32,
        );
        let reset = std::mem::take(&mut self.gui.clear_foam) || self.time < self.foam_time;
        let dt = if reset {
            0.0
        } else {
            (self.time - self.foam_time).max(0.0)
        };
        self.foam_field.encode(
            &self.gpu,
            &mut encoder,
            FoamStep::new(
                self.ocean.config.resolution,
                self.ocean.config.length,
                camera.ocean[2] * camera.ocean[3],
                dt,
                self.gui.foam_settings,
                reset,
            ),
        );
        self.foam_time = self.time;
        self.renderer.shore.encode(
            &self.gpu,
            &mut encoder,
            &self.gui.optics,
            self.time,
            reset,
            camera.ocean[3],
        );
        self.renderer.encode_spray(&self.gpu, &mut encoder);
        self.gui.spray_error = self.renderer.shore.spray_error.clone();
        let simulation = simulation_start.elapsed().as_secs_f64() * 1000.0;
        let render_start = Instant::now();
        self.renderer
            .encode(&self.gpu, &mut encoder, &self.gui.viewport.view, &camera);
        let extra = self
            .gui
            .encode(&self.window, &self.gpu, &mut encoder, &view, gui_output);
        if timed_frame {
            let timer = self.frame_timer.as_mut().unwrap();
            timer.mark(&mut encoder, 1);
            timer.resolve(&mut encoder, 0, 1, dt);
        }
        let render = render_start.elapsed().as_secs_f64() * 1000.0;
        let submit_start = Instant::now();
        self.gpu
            .queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        frame.present();
        let finished = Instant::now();
        self.gui.performance.record_frame(
            finished,
            FrameTimings {
                interval,
                cpu: finished.duration_since(now).as_secs_f64() * 1000.0,
                acquire,
                gui,
                simulation,
                render,
                submit_present: finished.duration_since(submit_start).as_secs_f64() * 1000.0,
            },
        );
        Ok(true)
    }
}

impl common::app::WindowScene for State {
    fn window(&self) -> &Window {
        &self.window
    }
    fn draw(&mut self) -> Result<bool, Error> {
        State::draw(self)
    }
    fn exiting(&mut self) {
        self.persist_layout(true);
    }
    fn event(&mut self, event: WindowEvent) -> bool {
        let response = self.gui.input.on_window_event(&self.window, &event);
        if response.consumed
            && matches!(
                &event,
                WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::CursorMoved { .. }
                    | WindowEvent::KeyboardInput { .. }
                    | WindowEvent::Ime(_)
            )
        {
            return false;
        }
        match event {
            WindowEvent::Resized(size) => self.resize(size.width, size.height),
            WindowEvent::Focused(false) => self.persist_layout(true),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyQ) => return true,
                    PhysicalKey::Code(KeyCode::Space) if !event.repeat => {
                        self.paused = !self.paused
                    }
                    PhysicalKey::Code(KeyCode::KeyC) if !event.repeat => {
                        self.camera.choppy = !self.camera.choppy
                    }
                    PhysicalKey::Code(KeyCode::KeyF) if !event.repeat => {
                        self.camera.foam = !self.camera.foam
                    }
                    PhysicalKey::Code(KeyCode::ArrowUp) => {
                        self.camera.gain = (self.camera.gain + 0.1).min(2.0)
                    }
                    PhysicalKey::Code(KeyCode::ArrowDown) => {
                        self.camera.gain = (self.camera.gain - 0.1).max(0.1)
                    }
                    PhysicalKey::Code(KeyCode::KeyR) => {
                        self.camera = if self.gui.optics.beach {
                            Camera::beach(self.ocean.config.length)
                        } else {
                            Camera::new(self.ocean.config.length)
                        };
                        self.time = 0.0;
                        self.gui.clear_foam = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }
}
pub fn run(
    initial: SavedSettings,
    frame_limit: Option<u32>,
    settings: Option<SettingsStore>,
) -> Result<(), Error> {
    initial.validate()?;
    let attributes = Window::default_attributes()
        .with_title("Wgpu Water Simulation")
        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0));
    common::app::run(attributes, frame_limit, move |window| {
        let mut state = pollster::block_on(State::new(window, initial.ocean))?;
        state.gui.optics = initial.optics;
        state.gui.draft = initial.draft;
        state.gui.foam_settings = initial.foam;
        state.gui.speed = initial.speed;
        state.paused = initial.paused;
        state.camera = initial.camera;
        state.renderer.set_shore_resolution(
            &state.gpu,
            &state.ocean,
            state.gui.optics.shore.resolution,
            state.config.format,
            [state.config.width, state.config.height],
        );
        state
            .renderer
            .prepare_scene(&state.gpu, &state.gui.optics, state.config.format)?;
        state.applied_model = state.gui.optics.model;
        state.gui.configuration_panel = ConfigurationPanel::new(ConfigurationLibrary::new(initial));
        state.settings = settings;
        if let Some(store) = &state.settings {
            state.gui.configuration_panel = ConfigurationPanel::new(store.configurations().clone());
            state.gui.settings_path = Some(store.path.clone());
            state.gui.workspace_store =
                Some(crate::ocean::ui::workspace::store::WorkspaceStore::open(
                    &store.path,
                    &mut state.gui.workspaces,
                ));
            state.gui.save_status = store.status.clone();
            state.gui.save_failed = store.failed;
            eprintln!("Local settings: {}", store.path.display());
            if store.failed {
                eprintln!("{}", store.status);
            }
        }
        Ok(state)
    })
}

use super::{
    configuration_panel::ConfigurationPanel,
    foam::{Foam, FoamStep},
    gpu::{CameraUniform, Error, Gpu, Ocean, Renderer},
    gui::Gui,
    performance::{FrameGpuStatus, FrameTimings, SweStatus},
    settings::{ConfigurationLibrary, SavedSettings, SettingsStore},
    spectrum::OceanConfig,
};
use glam::{Mat4, Vec2, Vec3};
use std::{sync::Arc, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 2],
    pub gain: f32,
    pub choppy: bool,
    pub chop_amount: f32,
    pub foam_debug: u32,
    pub foam: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new(OceanConfig::default().length)
    }
}

impl Camera {
    pub fn new(length: f32) -> Self {
        Self {
            yaw: 0.55,
            pitch: 0.30,
            distance: length * 0.58,
            target: [0.0; 2],
            gain: 1.0,
            choppy: true,
            chop_amount: 1.15,
            foam_debug: 0,
            foam: true,
        }
    }

    pub fn beach(length: f32) -> Self {
        Self {
            yaw: -0.72,
            pitch: 0.70,
            distance: length * 0.60,
            gain: 0.22,
            chop_amount: 0.65,
            foam: true,
            ..Self::new(length)
        }
    }

    pub fn model(length: f32) -> Self {
        Self {
            yaw: 0.65,
            pitch: 0.65,
            distance: 95.0,
            ..Self::beach(length)
        }
    }

    pub fn pan(&mut self, direction: Vec2, elapsed: f32) {
        let direction = direction.normalize_or_zero();
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(-self.yaw.sin(), -self.yaw.cos());
        let offset = (right * direction.x + forward * direction.y) * self.distance * 0.5 * elapsed;
        self.target[0] += offset.x;
        self.target[1] += offset.y;
    }

    pub fn uniform(&self, config: OceanConfig, aspect: f32) -> CameraUniform {
        let target = Vec3::new(self.target[0], 0.0, self.target[1]);
        let eye = target
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance;
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let projection =
            Mat4::perspective_rh(55.0_f32.to_radians(), aspect, 0.1, config.length * 8.0);
        let matrix = projection * view;
        let sun = Vec3::new(-0.35, 0.40, -0.65).normalize();
        CameraUniform {
            view_projection: matrix.to_cols_array_2d(),
            inverse_view_projection: matrix.inverse().to_cols_array_2d(),
            eye: eye.extend(1.0).to_array(),
            sun: sun.extend(0.0).to_array(),
            ocean: [
                config.resolution as f32,
                config.length,
                if self.choppy { -self.chop_amount } else { 0.0 },
                self.gain,
            ],
            misc: [
                if self.foam { 1.0 } else { 0.0 },
                config.mesh_resolution as f32,
                self.foam_debug as f32,
                0.0,
            ],
        }
    }
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
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
    frames: u32,
    settings: Option<SettingsStore>,
    applied_model: super::terrain::ModelConfig,
    frame_timer: Option<super::gpu_timer::GpuTimer>,
}

impl State {
    async fn new(window: Arc<Window>, ocean_config: OceanConfig) -> Result<Self, Error> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let gpu = Gpu::new(&instance, Some(&surface)).await?;
        eprintln!("GPU: {}", gpu.name);
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("adapter cannot present to this window")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        let ocean = Ocean::new(&gpu, ocean_config)?;
        let foam_field = Foam::new(&gpu, &ocean.fields, ocean_config.resolution);
        let mut renderer = Renderer::new(
            &gpu,
            &ocean,
            &foam_field,
            config.format,
            config.width,
            config.height,
        );
        renderer.shore.timer = super::gpu_timer::GpuTimer::new(&gpu);
        let gui = Gui::new(&window, &gpu, config.format, ocean_config);
        let frame_timer = super::gpu_timer::GpuTimer::new(&gpu);
        Ok(Self {
            frame_timer,
            gui,
            window,
            surface,
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
            frames: 0,
            settings: None,
            applied_model: super::terrain::ModelConfig::default(),
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

    fn persist(&mut self, force: bool) {
        if let Some(store) = &mut self.gui.workspace_store {
            store.update(&self.gui.workspaces, Instant::now(), force);
        }
        let current = self.saved_settings();
        if let Some(store) = &mut self.settings {
            let now = Instant::now();
            store.set_configurations(&self.gui.configuration_panel.library, now);
            store.update(current, now, force);
            self.gui.save_status = store.status.clone();
            self.gui.save_failed = store.failed;
        }
    }

    fn rebuild_ocean(&mut self, config: OceanConfig) -> Result<(), Error> {
        let ocean = Ocean::new(&self.gpu, config)?;
        let foam_field = Foam::new(&self.gpu, &ocean.fields, config.resolution);
        let mut renderer = Renderer::new(
            &self.gpu,
            &ocean,
            &foam_field,
            self.config.format,
            self.gui.viewport_size[0],
            self.gui.viewport_size[1],
        );
        renderer.shore.timer = super::gpu_timer::GpuTimer::new(&self.gpu);
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
        self.gui.save_now = true;
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

    fn draw(&mut self) -> Result<bool, wgpu::SurfaceError> {
        let now = Instant::now();
        let interval = now.duration_since(self.last).as_secs_f64() * 1000.0;
        let elapsed = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        if !self.paused {
            self.time += elapsed * self.gui.speed;
        }
        let acquire_start = Instant::now();
        let frame = self.surface.get_current_texture()?;
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
                return Err(wgpu::SurfaceError::Other);
            }
        }
        self.applied_model = self.gui.optics.model;
        self.gui.model_panel.source_bounds = self
            .renderer
            .model_assets
            .as_ref()
            .map(|a| a.height.metadata.bounds);
        self.gui.model_panel.bed_range = self.renderer.shore.terrain_height_range();
        let force_save = std::mem::take(&mut self.gui.save_now);
        self.persist(force_save);
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
            .encode(&self.gpu, &mut encoder, &self.gui.viewport_view, &camera);
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
        self.frames += 1;
        Ok(true)
    }
}

struct App {
    initial: SavedSettings,
    settings: Option<SettingsStore>,
    state: Option<State>,
    error: Option<String>,
    frame_limit: Option<u32>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let result = (|| -> Result<State, Error> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("Wgpu Water Simulation")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 800.0)),
                )?,
            );
            pollster::block_on(State::new(window, self.initial.ocean))
        })();
        match result {
            Ok(mut state) => {
                state.gui.optics = self.initial.optics;
                state.gui.draft = self.initial.draft;
                state.gui.foam_settings = self.initial.foam;
                state.gui.speed = self.initial.speed;
                state.paused = self.initial.paused;
                state.camera = self.initial.camera;
                state.renderer.set_shore_resolution(
                    &state.gpu,
                    &state.ocean,
                    state.gui.optics.shore.resolution,
                    state.config.format,
                    [state.config.width, state.config.height],
                );
                if let Err(error) =
                    state
                        .renderer
                        .prepare_scene(&state.gpu, &state.gui.optics, state.config.format)
                {
                    self.error = Some(error.to_string());
                    event_loop.exit();
                    return;
                }
                state.applied_model = state.gui.optics.model;
                state.gui.configuration_panel =
                    ConfigurationPanel::new(ConfigurationLibrary::new(self.initial.clone()));
                state.settings = self.settings.take();
                if let Some(store) = &state.settings {
                    state.gui.configuration_panel =
                        ConfigurationPanel::new(store.configurations().clone());
                    state.gui.settings_path = Some(store.path.clone());
                    state.gui.workspace_store =
                        Some(super::workspace_settings::WorkspaceStore::open(
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
                self.state = Some(state);
            }
            Err(error) => {
                self.error = Some(error.to_string());
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != window_id {
            return;
        }
        let response = state.gui.input.on_window_event(&state.window, &event);
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
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::Focused(false) => {
                state.persist(true);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::KeyQ) => event_loop.exit(),
                    PhysicalKey::Code(KeyCode::Space) if !event.repeat => {
                        state.paused = !state.paused
                    }
                    PhysicalKey::Code(KeyCode::KeyC) if !event.repeat => {
                        state.camera.choppy = !state.camera.choppy
                    }
                    PhysicalKey::Code(KeyCode::KeyF) if !event.repeat => {
                        state.camera.foam = !state.camera.foam
                    }
                    PhysicalKey::Code(KeyCode::ArrowUp) => {
                        state.camera.gain = (state.camera.gain + 0.1).min(2.0)
                    }
                    PhysicalKey::Code(KeyCode::ArrowDown) => {
                        state.camera.gain = (state.camera.gain - 0.1).max(0.1)
                    }
                    PhysicalKey::Code(KeyCode::KeyR) => {
                        state.camera = if state.gui.optics.beach {
                            Camera::beach(state.ocean.config.length)
                        } else {
                            Camera::new(state.ocean.config.length)
                        };
                        state.time = 0.0;
                        state.gui.clear_foam = true;
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                if state.window.inner_size().width == 0 || state.window.inner_size().height == 0 {
                    return;
                }
                match state.draw() {
                    Ok(_) if self.frame_limit.is_some_and(|limit| state.frames >= limit) => {
                        event_loop.exit()
                    }
                    Ok(_) => {}
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        state.resize(state.config.width, state.config.height)
                    }
                    Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => {}
                    Err(error) => {
                        self.error = Some(error.to_string());
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some(state) = &mut self.state {
            state.persist(true);
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

pub fn run(
    initial: SavedSettings,
    frame_limit: Option<u32>,
    settings: Option<SettingsStore>,
) -> Result<(), Error> {
    initial.validate()?;
    let mut app = App {
        initial,
        settings,
        state: None,
        error: None,
        frame_limit,
    };
    EventLoop::new()?.run_app(&mut app)?;
    if let Some(error) = app.error {
        return Err(error.into());
    }
    Ok(())
}

/// 和窗口共用相同的计算与渲染管线，方便无窗口验证和保存结果。
pub async fn snapshot(
    initial: &SavedSettings,
    time: f32,
    path: &std::path::Path,
    foam_view: Option<u32>,
    emission_window: Option<[f32; 2]>,
) -> Result<(), Error> {
    let config = initial.ocean;
    let mut optics = initial.optics;
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    eprintln!("GPU: {}", gpu.name);
    let ocean = Ocean::new(&gpu, config)?;
    let (width, height) = (1280, 800);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let foam_field = Foam::new(&gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new(&gpu, &ocean, &foam_field, format, width, height);
    renderer.set_shore_resolution(
        &gpu,
        &ocean,
        optics.shore.resolution,
        format,
        [width, height],
    );
    renderer.prepare_scene(&gpu, &optics, format)?;
    renderer.update_optics(&gpu, &optics);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("snapshot"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let bytes_per_row = (width * 4).div_ceil(256) * 256;
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("image readback"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    // Deterministic history warm-up: up to 60 s for wet sand, 8 s for open ocean.
    let mut camera = initial.camera.uniform(config, width as f32 / height as f32);
    if let Some(view) = foam_view {
        camera.misc[2] = view as f32;
    }
    if emission_window.is_some() && !optics.swe_enabled() {
        return Err("--foam-emission-window requires an active SWE scene".into());
    }
    let duration = time.min(if optics.swe_enabled() { 60.0 } else { 8.0 });
    let steps = (duration * 60.0).ceil() as u32;
    if steps > 0 {
        let dt = duration / steps as f32;
        for i in 1..=steps {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            // Roundoff must not put the last warm-up step beyond the target:
            // the final render would otherwise look like a time rewind/reset.
            let step_time = (time - duration + i as f32 * dt).min(time);
            optics.shore.foam_emission_enabled = initial.optics.shore.foam_emission_enabled
                && emission_window.is_none_or(|[start, end]| step_time > start && step_time <= end);
            ocean.encode_filtered(&gpu, &mut encoder, step_time, &optics.shore);
            renderer.shore.encode(
                &gpu,
                &mut encoder,
                &optics,
                step_time,
                false,
                camera.ocean[3],
            );
            renderer.encode_spray(&gpu, &mut encoder);
            foam_field.encode(
                &gpu,
                &mut encoder,
                FoamStep::new(
                    config.resolution,
                    config.length,
                    camera.ocean[2] * camera.ocean[3],
                    dt,
                    initial.foam,
                    false,
                ),
            );
            gpu.queue.submit([encoder.finish()]);
        }
    }
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    ocean.encode_filtered(&gpu, &mut encoder, time, &optics.shore);
    renderer
        .shore
        .encode(&gpu, &mut encoder, &optics, time, false, camera.ocean[3]);
    renderer.encode_spray(&gpu, &mut encoder);
    if let Some(error) = &renderer.shore.spray_error {
        return Err(error.clone().into());
    }
    foam_field.encode(
        &gpu,
        &mut encoder,
        FoamStep::new(
            config.resolution,
            config.length,
            camera.ocean[2] * camera.ocean[3],
            0.0,
            initial.foam,
            false,
        ),
    );
    renderer.encode(
        &gpu,
        &mut encoder,
        &texture.create_view(&Default::default()),
        &camera,
    );
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let foam_bytes = gpu.read_buffer(&foam_field.field, foam_field.field.size())?;
    let diagnostics: Vec<[f32; 2]> = foam_bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| bytemuck::pod_read_unaligned(b))
        .collect();
    let min_j = diagnostics
        .iter()
        .map(|v| v[1])
        .fold(f32::INFINITY, f32::min);
    let max_f = diagnostics.iter().map(|v| v[0]).fold(0.0_f32, f32::max);
    let covered = diagnostics.iter().filter(|v| v[0] > 0.02).count();
    eprintln!(
        "Foam: J min={min_j:.3}, F max={max_f:.3}, coverage F>0.02={:.1}%",
        100.0 * covered as f32 / diagnostics.len() as f32
    );
    let mapped = gpu.map_buffer(&staging)?;
    if optics.swe_enabled() {
        let bytes = gpu.read_buffer(
            &renderer.shore.field,
            u64::from(renderer.shore.resolution.pow(2)) * 16,
        )?;
        let samples: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        let foam = samples.iter().map(|v| v[0]).fold(0.0_f32, f32::max);
        let wet = samples
            .iter()
            .filter(|v| v[2] < 0.005 && v[1] > 0.1)
            .count();
        eprintln!("Shore: foam max={foam:.3}, exposed wet sand cells={wet}");
        if optics.shore.foam_ridge_enabled || optics.shore.view >= 7 {
            let bytes = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
            let cells = renderer.shore.resolution.pow(2) as usize;
            let extra: Vec<[f32; 4]> = bytes[2 * cells * 16..3 * cells * 16]
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            let emitting = extra.iter().filter(|q| q[0] > 0.001).count();
            let source_max = extra.iter().map(|q| q[0]).fold(0.0_f32, f32::max);
            let foam_cells = samples.iter().filter(|q| q[0] > 0.02).count();
            eprintln!(
                "Front: emitting cells={emitting}/{cells}, source max={source_max:.4}/s, transported foam cells (>0.02)={foam_cells}"
            );
        }
    }
    let pixels: Vec<u8> = mapped
        .chunks_exact(bytes_per_row as usize)
        .flat_map(|row| row[..(width * 4) as usize].iter().copied())
        .collect();
    let mut png = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create_new(path)?),
        width,
        height,
    );
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.write_header()?.write_image_data(&pixels)?;
    eprintln!("Saved {} at t={time:.2}s", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_pan_preserves_height_and_heading_at_any_pitch() {
        let config = OceanConfig::default();
        for pitch in [0.07, 0.7, 1.35] {
            for (yaw, direction, expected) in [
                (0.0, Vec2::Y, Vec3::NEG_Z),
                (0.0, Vec2::NEG_Y, Vec3::Z),
                (0.0, Vec2::X, Vec3::X),
                (0.0, Vec2::NEG_X, Vec3::NEG_X),
                (std::f32::consts::FRAC_PI_2, Vec2::Y, Vec3::NEG_X),
                (0.0, Vec2::ONE, Vec3::new(1.0, 0.0, -1.0).normalize()),
            ] {
                let mut camera = Camera {
                    yaw,
                    pitch,
                    distance: 20.0,
                    ..Camera::default()
                };
                let before = camera.uniform(config, 1.0);
                camera.pan(direction, 0.1);
                let after = camera.uniform(config, 1.0);
                let displacement = Vec3::from_slice(&after.eye) - Vec3::from_slice(&before.eye);
                assert!((displacement - expected).length() < 1e-5);
                let before_view = Mat4::from_cols_array_2d(&before.view_projection);
                let after_view = Mat4::from_cols_array_2d(&after.view_projection);
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    assert!(
                        (before_view.transform_vector3(axis) - after_view.transform_vector3(axis))
                            .length()
                            < 1e-5
                    );
                }
            }
        }
    }
}

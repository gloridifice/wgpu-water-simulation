//! Fog application lifecycle. common owns the actual winit loop and GPU/egui plumbing.
mod ui;
use crate::{
    Error,
    interactions::{Event, EventQueue, PlayerEmitter},
    parameters::Session,
    render::Renderer,
    simulation::{Diagnostics, Simulation},
    terrain::{BakeSource, Terrain},
};
use common::{
    app::{Desktop, WindowScene},
    gpu::timer::GpuTimer,
    settings::{Settings, SettingsStore},
};
use glam::{Vec2, Vec3};
use std::{sync::Arc, time::Instant};
use winit::{
    event::{DeviceEvent, ElementState, WindowEvent},
    keyboard::{KeyCode, ModifiersState, PhysicalKey},
    window::{CursorGrabMode, Window},
};
fn gpu_label(timer: &Option<GpuTimer>, sample: Option<f64>, idle: bool) -> String {
    match timer {
        None => "不支持".into(),
        Some(t) if t.failed => "读取失败".into(),
        Some(_) if idle => "空闲".into(),
        Some(_) => sample.map_or_else(|| "等待采样".into(), |v| format!("{v:.2} ms")),
    }
}
struct State {
    desktop: Desktop,
    session: Session,
    terrain: Terrain,
    native_terrain: Terrain,
    source: BakeSource,
    simulation: Simulation,
    renderer: Renderer,
    ui: ui::Interface,
    store: Option<SettingsStore<Session>>,
    queue: EventQueue,
    emitter: PlayerEmitter,
    keys: [bool; 4],
    jump: bool,
    captured: bool,
    focused: bool,
    modifiers: ModifiersState,
    last: Instant,
    physics_accumulator: f32,
    fog_accumulator: f32,
    diagnostic_clock: f32,
    diagnostics: Diagnostics,
    reset: bool,
    timer: Option<GpuTimer>,
    gpu_ms: Option<f64>,
    render_timer: Option<GpuTimer>,
    render_gpu_ms: Option<f64>,
    previous_feet: [f32; 3],
    error: Option<String>,
    fps_clock: f32,
    fps_frames: u32,
    fps: f32,
    view_size: [u32; 2],
}
impl State {
    async fn new(
        window: Arc<Window>,
        mut session: Session,
        terrain: Terrain,
        source: BakeSource,
        store: Option<SettingsStore<Session>>,
    ) -> Result<Self, Error> {
        session.validate()?;
        let native_terrain = terrain;
        let terrain =
            native_terrain.at_region(&source, session.grid_resolution, session.domain_scale)?;
        session.player.place(&terrain)?;
        let desktop = Desktop::new(window).await?;
        eprintln!(
            "SWE Fog | {} | 128-bit state, GPU CFL, analytic thin-layer rendering",
            desktop.gpu.name
        );
        let simulation = Simulation::new(&desktop.gpu, &terrain)?;
        let renderer = Renderer::new(
            &desktop.gpu,
            &simulation,
            &terrain,
            &source,
            desktop.config.format,
        )?;
        let ui = ui::Interface::new(
            &desktop.window,
            &desktop.gpu,
            desktop.config.format,
            &session,
            store.as_ref(),
        );
        let timer = GpuTimer::new(&desktop.gpu);
        let render_timer = GpuTimer::new(&desktop.gpu);
        let previous_feet = session.player.feet;
        Ok(Self {
            desktop,
            session,
            terrain,
            native_terrain,
            source,
            simulation,
            renderer,
            ui,
            store,
            queue: EventQueue::default(),
            emitter: PlayerEmitter::default(),
            keys: [false; 4],
            jump: false,
            captured: false,
            focused: true,
            modifiers: ModifiersState::empty(),
            last: Instant::now(),
            physics_accumulator: 0.0,
            fog_accumulator: 0.0,
            diagnostic_clock: 0.5,
            diagnostics: Diagnostics::default(),
            reset: true,
            timer,
            gpu_ms: None,
            render_timer,
            render_gpu_ms: None,
            previous_feet,
            error: None,
            fps_clock: 0.0,
            fps_frames: 0,
            fps: 0.0,
            view_size: [1, 1],
        })
    }
    fn clear_events(&mut self) {
        self.queue.clear();
        self.emitter.clear();
    }
    fn rebuild_grid(&mut self) -> Result<(), Error> {
        let terrain = self.native_terrain.at_region(
            &self.source,
            self.session.grid_resolution,
            self.session.domain_scale,
        )?;
        let mut player = self.session.player.clone();
        if terrain.metadata.domain != self.terrain.metadata.domain {
            player.place(&terrain)?;
        }
        let simulation = Simulation::new(&self.desktop.gpu, &terrain)?;
        let renderer = Renderer::new(
            &self.desktop.gpu,
            &simulation,
            &terrain,
            &self.source,
            self.desktop.config.format,
        )?;
        self.session.player = player;
        self.physics_accumulator = 0.0;
        self.terrain = terrain;
        self.simulation = simulation;
        self.renderer = renderer;
        self.previous_feet = self.session.player.feet;
        self.gpu_ms = None;
        self.render_gpu_ms = None;
        self.reset = true;
        Ok(())
    }
    fn switch_scene(&mut self) -> Result<(), Error> {
        let (source, native_terrain) = self.session.scene.load()?;
        let terrain = native_terrain.at_region(
            &source,
            self.session.grid_resolution,
            self.session.domain_scale,
        )?;
        let mut player = self.session.player.clone();
        player.feet = [0.0; 3];
        player.place(&terrain)?;
        let simulation = Simulation::new(&self.desktop.gpu, &terrain)?;
        let renderer = Renderer::new(
            &self.desktop.gpu,
            &simulation,
            &terrain,
            &source,
            self.desktop.config.format,
        )?;
        // Commit only after every resource is ready; failed switches retain the old scene.
        self.source = source;
        self.native_terrain = native_terrain;
        self.terrain = terrain;
        self.simulation = simulation;
        self.renderer = renderer;
        self.session.player = player;
        self.previous_feet = self.session.player.feet;
        self.physics_accumulator = 0.0;
        self.fog_accumulator = 0.0;
        self.diagnostics = Diagnostics::default();
        self.diagnostic_clock = 0.5;
        self.gpu_ms = None;
        self.render_gpu_ms = None;
        self.clear_events();
        self.capture(false);
        self.reset = true;
        Ok(())
    }
    fn capture(&mut self, value: bool) {
        self.captured = value && self.focused;
        if self.captured {
            if self
                .desktop
                .window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| {
                    self.desktop
                        .window
                        .set_cursor_grab(CursorGrabMode::Confined)
                })
                .is_err()
            {
                self.captured = false;
            }
            if let Some(focus) = self.ui.backend.context.memory(|m| m.focused()) {
                self.ui
                    .backend
                    .context
                    .memory_mut(|m| m.surrender_focus(focus));
            }
        } else {
            let _ = self.desktop.window.set_cursor_grab(CursorGrabMode::None);
            self.keys = [false; 4];
            self.jump = false;
        }
        self.desktop.window.set_cursor_visible(!self.captured);
    }
    fn physics(&mut self, elapsed: f32) {
        self.physics_accumulator = (self.physics_accumulator + elapsed).min(0.1);
        while self.physics_accumulator + 1e-8 >= 1.0 / 60.0 {
            let active = self.captured && self.focused;
            let axis = if active {
                Vec2::new(
                    i32::from(self.keys[1]) as f32 - i32::from(self.keys[0]) as f32,
                    i32::from(self.keys[2]) as f32 - i32::from(self.keys[3]) as f32,
                )
            } else {
                Vec2::ZERO
            };
            let jump = std::mem::take(&mut self.jump) && active;
            self.previous_feet = self.session.player.feet;
            let motion = self.session.player.step(
                axis,
                self.session.camera[0],
                jump,
                1.0 / 60.0,
                &self.terrain,
            );
            if !self.session.paused
                && self.session.interactions_enabled
                && self.diagnostics.fault == 0
            {
                let p = &self.session.interaction;
                self.emitter.movement(
                    p,
                    Vec3::from_array(self.session.player.feet),
                    motion.horizontal,
                    self.session.player.walk_speed,
                    1.0 / 60.0,
                    &mut self.queue,
                );
                if p.jump_enabled
                    && let Some(feet) = motion.jumped
                {
                    self.queue
                        .push(Event::radial(feet, p.jump_strength, p.jump_radius, p));
                }
                if p.landing_enabled
                    && let Some((feet, impact)) = motion.landed
                {
                    self.queue.push(Event::radial(
                        feet,
                        p.landing_strength
                            * (impact / self.session.player.jump_speed.max(0.001)).clamp(0.5, 2.0),
                        p.landing_radius,
                        p,
                    ));
                }
            } else {
                self.clear_events();
            }
            self.physics_accumulator = (self.physics_accumulator - 1.0 / 60.0).max(0.0);
        }
    }
}
impl WindowScene for State {
    fn window(&self) -> &Window {
        &self.desktop.window
    }
    fn device_event(&mut self, event: DeviceEvent) {
        if self.captured
            && self.focused
            && let DeviceEvent::MouseMotion { delta } = event
        {
            let inspecting = self.session.inspection.view != crate::parameters::GeometryView::Off;
            let camera = if inspecting {
                &mut self.session.inspection.orbit
            } else {
                &mut self.session.camera
            };
            camera[0] = (camera[0] - delta.0 as f32 * 0.004).rem_euclid(std::f32::consts::TAU);
            camera[1] = (camera[1] + delta.1 as f32 * 0.004).clamp(
                if inspecting { -1.5 } else { 0.05 },
                if inspecting { 1.56 } else { 1.35 },
            );
        }
    }
    fn event(&mut self, event: WindowEvent) -> bool {
        let response = self
            .ui
            .backend
            .input
            .on_window_event(&self.desktop.window, &event);
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
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    self.desktop.config.width = size.width;
                    self.desktop.config.height = size.height;
                    self.desktop
                        .surface
                        .configure(&self.desktop.gpu.device, &self.desktop.config);
                }
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                if !focused {
                    self.capture(false);
                    self.clear_events();
                    self.ui.persist(true);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) if pressed => self.capture(false),
                    PhysicalKey::Code(KeyCode::KeyQ) if pressed && self.captured => return true,
                    PhysicalKey::Code(KeyCode::Space)
                        if pressed && !event.repeat && self.captured =>
                    {
                        self.jump = true
                    }
                    PhysicalKey::Code(key) => {
                        let index = match key {
                            KeyCode::KeyA => Some(0),
                            KeyCode::KeyD => Some(1),
                            KeyCode::KeyW => Some(2),
                            KeyCode::KeyS => Some(3),
                            _ => None,
                        };
                        if let Some(i) = index
                            && (!pressed || !self.modifiers.control_key())
                        {
                            self.keys[i] = pressed;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }
    fn draw(&mut self) -> Result<bool, Error> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f32().min(0.1);
        self.last = now;
        let Some(frame) = common::app::acquire(
            &self.desktop.window,
            &self.desktop.instance,
            &mut self.desktop.surface,
            &self.desktop.gpu,
            &self.desktop.config,
        )?
        else {
            return Ok(false);
        };
        if let Some(d) = self.simulation.poll(&self.desktop.gpu)? {
            self.diagnostics = d;
            if d.fault != 0 {
                self.error = Some(
                    "GPU 稳定步重试耗尽；雾已停止推进，玩家仍可操作。修改参数或重置以恢复。".into(),
                );
            }
        }
        if let Some(timer) = &mut self.timer {
            timer.poll(&self.desktop.gpu);
            if let Some(sample) = timer.latest.take() {
                self.gpu_ms = Some(sample.ms);
            }
        }
        if let Some(timer) = &mut self.render_timer {
            timer.poll(&self.desktop.gpu);
            if let Some(sample) = timer.latest.take() {
                self.render_gpu_ms = Some(sample.ms);
            }
        }
        let gpu_status = format!(
            "模拟 {} / 场景雾 {}",
            gpu_label(&self.timer, self.gpu_ms, self.session.paused),
            gpu_label(&self.render_timer, self.render_gpu_ms, false)
        );
        let spacing = self.terrain.spacing().max_element();
        let nominal = self.session.fog.noise_frequency
            * 2.0f32.powi(self.session.fog.noise_octaves as i32 - 1);
        let status = ui::Status {
            resolution: self.simulation.resolution,
            domain: self.simulation.domain,
            diagnostics: self.diagnostics,
            fps: self.fps,
            gpu: gpu_status,
            save_failed: self.store.as_ref().is_some_and(|s| s.failed),
            save_path: self.store.as_ref().map(|s| s.path.display().to_string()),
            save: self
                .store
                .as_ref()
                .map(|s| s.status.clone())
                .unwrap_or_else(|| "保存已禁用".into()),
            error: self.error.clone(),
            warning: (nominal * spacing > 0.5)
                .then(|| "初始化噪波的细层超出网格可分辨尺度；可降低频率或层数。".into()),
        };
        let previous_session = self.session.clone();
        let was_paused = self.session.paused;
        let interactions_enabled = self.session.interactions_enabled;
        let movement_mode = self.session.interaction.movement_mode;
        let move_enabled = self.session.interaction.move_enabled;
        let grid_resolution = self.session.grid_resolution;
        let domain_scale = self.session.domain_scale;
        let (output, actions) = self
            .ui
            .frame(&self.desktop.window, &mut self.session, &status);
        if let Some((index, mut session)) = actions.load {
            match session.validate().and_then(|_| {
                if session.scene == previous_session.scene {
                    session.player.place(&self.terrain)
                } else {
                    Ok(())
                }
            }) {
                Ok(()) => {
                    self.session = session;
                    self.previous_feet = self.session.player.feet;
                    self.physics_accumulator = 0.0;
                    self.ui.library.select(index)?;
                    self.reset = true;
                    self.error = None;
                    self.capture(false);
                }
                Err(e) => self.error = Some(e),
            }
        }
        self.reset |= actions.reset;
        if self.session.scene != previous_session.scene {
            if let Err(e) = self.switch_scene() {
                self.session = previous_session;
                self.error = Some(format!("场景切换失败：{e}"));
            } else {
                self.error = None;
            }
        }
        let requested_resolution = self
            .session
            .grid_resolution
            .unwrap_or(self.native_terrain.metadata.resolution);
        if (requested_resolution != self.simulation.resolution
            || domain_scale != self.session.domain_scale)
            && let Err(e) = self.rebuild_grid()
        {
            self.session.grid_resolution = grid_resolution;
            self.session.domain_scale = domain_scale;
            self.reset = false;
            self.error = Some(format!("网格重建失败：{e}"));
        }
        if self.reset
            || was_paused != self.session.paused
            || interactions_enabled != self.session.interactions_enabled
        {
            self.clear_events();
            self.fog_accumulator = 0.0;
        }
        if movement_mode != self.session.interaction.movement_mode
            || move_enabled != self.session.interaction.move_enabled
        {
            self.clear_events();
        }
        if actions.capture {
            self.capture(true);
        }
        self.physics(elapsed);
        if let Some(store) = &mut self.store {
            store.set_configurations(&self.ui.library);
            store.update(self.session.clone(), actions.save);
        }
        self.ui.persist(false);
        let gpu = &self.desktop.gpu;
        if self.view_size != self.ui.size {
            self.view_size = self.ui.size;
            self.ui.viewport.resize(
                &mut self.ui.backend,
                gpu,
                self.desktop.config.format,
                self.view_size,
            );
        }
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        self.diagnostic_clock += elapsed;
        if !self.session.paused {
            self.fog_accumulator = (self.fog_accumulator + elapsed).min(0.1);
        }
        let ticks = ((self.fog_accumulator + 1e-8) * 30.0).floor().min(3.0) as u32;
        let advance = ticks > 0 && !self.session.paused && self.diagnostics.fault == 0;
        let timed = (self.reset || advance) && self.timer.as_ref().is_some_and(|t| t.ready());
        if timed {
            self.timer.as_ref().unwrap().mark(&mut encoder, 0);
        }
        if self.reset {
            self.simulation
                .encode(gpu, &mut encoder, &self.session.fog, 0.0, true, &[])?;
            self.reset = false;
            self.diagnostics = Diagnostics::default();
            self.error = None;
            self.fog_accumulator = 0.0;
            self.diagnostic_clock = 0.5;
        } else if advance {
            let dt = ticks as f32 / 30.0;
            let events = self.queue.take();
            self.simulation
                .encode(gpu, &mut encoder, &self.session.fog, dt, false, &events)?;
            self.fog_accumulator = (self.fog_accumulator - dt).max(0.0);
        }
        if timed {
            let timer = self.timer.as_mut().unwrap();
            timer.mark(&mut encoder, 1);
            timer.resolve(&mut encoder);
        }
        if self.diagnostic_clock >= 0.5 {
            self.simulation.request_diagnostics(&mut encoder);
            self.diagnostic_clock = 0.0;
        }
        let render_timed = self.render_timer.as_ref().is_some_and(|t| t.ready());
        if render_timed {
            self.render_timer.as_ref().unwrap().mark(&mut encoder, 0);
        }
        let mut visual = self.session.clone();
        visual.player.feet = Vec3::from_array(self.previous_feet)
            .lerp(
                Vec3::from_array(self.session.player.feet),
                (self.physics_accumulator * 60.0).clamp(0.0, 1.0),
            )
            .to_array();
        self.renderer.encode(
            gpu,
            &mut encoder,
            &self.ui.viewport.view,
            &visual,
            self.simulation.domain,
            self.view_size,
        );
        if render_timed {
            let timer = self.render_timer.as_mut().unwrap();
            timer.mark(&mut encoder, 1);
            timer.resolve(&mut encoder);
        }
        let view = frame.texture.create_view(&Default::default());
        let commands =
            self.ui
                .backend
                .encode(&self.desktop.window, gpu, &mut encoder, &view, output);
        gpu.queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        frame.present();
        self.fps_frames += 1;
        self.fps_clock += elapsed;
        if self.fps_clock >= 1.0 {
            self.fps = self.fps_frames as f32 / self.fps_clock;
            self.fps_frames = 0;
            self.fps_clock = 0.0;
        }
        Ok(true)
    }
    fn exiting(&mut self) {
        self.ui.persist(true);
        self.capture(false);
    }
}
pub fn run(
    session: Session,
    terrain: Terrain,
    source: BakeSource,
    store: Option<SettingsStore<Session>>,
    frames: Option<u32>,
) -> Result<(), Error> {
    session.validate()?;
    common::app::run(
        Window::default_attributes()
            .with_title("SWE Fog — Rust / wgpu")
            .with_inner_size(winit::dpi::LogicalSize::new(1360.0, 860.0)),
        frames,
        move |window| pollster::block_on(State::new(window, session, terrain, source, store)),
    )
}

async fn warm_offscreen(
    session: &Session,
    terrain: &Terrain,
    source: &BakeSource,
    time: f32,
) -> Result<(common::gpu::Gpu, Simulation), Error> {
    session.validate()?;
    let terrain = terrain.at_region(source, session.grid_resolution, session.domain_scale)?;
    if !time.is_finite() || !(0.0..=120.0).contains(&time) {
        return Err("offscreen time must be 0..120 s".into());
    }
    let gpu = common::gpu::Gpu::new(&wgpu::Instance::default(), None).await?;
    let mut sim = Simulation::new(&gpu, &terrain)?;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    sim.encode(&gpu, &mut encoder, &session.fog, 0.0, true, &[])?;
    gpu.queue.submit([encoder.finish()]);
    let steps = (time * 30.0).ceil() as u32;
    for _ in 0..steps {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        sim.encode(
            &gpu,
            &mut encoder,
            &session.fog,
            time / steps.max(1) as f32,
            false,
            &[],
        )?;
        gpu.queue.submit([encoder.finish()]);
    }
    Ok((gpu, sim))
}
pub async fn snapshot(
    session: &Session,
    terrain: &Terrain,
    source: &BakeSource,
    time: f32,
    path: &std::path::Path,
) -> Result<(), Error> {
    let (gpu, sim) = warm_offscreen(session, terrain, source, time).await?;
    let terrain = terrain.at_region(source, session.grid_resolution, session.domain_scale)?;
    let (clock, _) = sim.snapshot(&gpu)?;
    if clock.fault != 0 {
        return Err("fog snapshot simulation fault".into());
    }
    let renderer = Renderer::new(
        &gpu,
        &sim,
        &terrain,
        source,
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )?;
    let size = [1280, 800];
    let mut placed = session.clone();
    placed.player.place(&terrain)?;
    let pixels = renderer.read_image(&gpu, &placed, sim.domain, size)?;
    crate::render::save_png(path, &pixels, size)?;
    eprintln!(
        "Fog snapshot {} at actual {:.3} simulated seconds",
        path.display(),
        clock.simulated_time
    );
    Ok(())
}
pub async fn dump(
    session: &Session,
    terrain: &Terrain,
    source: &BakeSource,
    time: f32,
    path: &std::path::Path,
) -> Result<(), Error> {
    let (gpu, sim) = warm_offscreen(session, terrain, source, time).await?;
    let (clock, field) = sim.snapshot(&gpu)?;
    let value = serde_json::json!({"version":1,"domain":sim.domain,"resolution":sim.resolution,"simulated_time":clock.simulated_time,"fault":clock.fault,"dropped_time":clock.dropped_time,"layout":["h","m","ux","uz"],"field":field});
    let mut file = std::fs::File::create_new(path)?;
    std::io::Write::write_all(&mut file, &serde_json::to_vec(&value)?)?;
    Ok(())
}

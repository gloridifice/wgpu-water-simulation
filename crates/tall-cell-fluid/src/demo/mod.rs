//! 独立可视化示例。模拟在线程中运行，窗口显示真实 GPU 提取的表面快照。
//! 通过 `cargo run --features demo --bin fluid-demo` 启动，不引用 Ocean。

mod gpu_timer;
mod gui;
mod render;
mod verify;
mod worker;

use gui::{GuiAction, PanelData};
use render::{Camera, Renderer, ViewMode};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};
use worker::{Command, DemoConfig, Frame, Scene, Worker, WorkerEvent};

const HELP: &str = "独立三维液体 GPU demo（不使用 Ocean）

cargo run --release --features demo --bin fluid-demo -- [选项]
  --scene drop|dam   液滴落水 / 水柱溃坝，默认 drop
  --resolution N     2m 立方域每轴单元数，8..40，默认 16
  --paused           初始暂停
  --frames N         显示 N 帧后退出，用于有限时长冒烟验证
  --steps N          达到 N 个模拟子步后退出（配合 snapshot 可保存指定时刻）
  --snapshot PATH    保存 PNG 后退出；不覆盖已有文件，未给 steps 时截初始状态
  --view water|mesh|normals  水面材质 / 三角网格 / 法线，默认 water
  --verify           无窗口验证两个场景、真实表面、暂停/单步/重置；steps 默认 6，至少 2
  -h, --help         显示帮助

空格：播放/暂停 | N：暂停时单步 | R：重置 | 1/2：液滴/溃坝
V：材质/三角网格/法线 | 鼠标左键拖动：环绕 | 滚轮：缩放 | Home：恢复相机
Esc：退出。egui 面板显示模拟、Mesh 和着色耗时，也可通过面板控制场景与显示方式。
模拟 / Mesh 为包含回读等待的墙钟时间；着色为真实 GPU 时间，三者采样频率不同。

求解在后台按 CFL 推进，窗口独立刷新；模拟时间不保证与现实时间同步。
";

struct Options {
    config: DemoConfig,
    frames: Option<u64>,
    steps: Option<usize>,
    snapshot: Option<PathBuf>,
    view: ViewMode,
    verify: bool,
}

fn value<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<T, String> {
    args.next()
        .ok_or_else(|| format!("{flag} 缺少值"))?
        .parse()
        .map_err(|_| format!("{flag} 的值不合法"))
}

fn options() -> Result<Option<Options>, String> {
    let mut result = Options {
        config: DemoConfig {
            resolution: 16,
            scene: Scene::Drop,
            paused: false,
        },
        frames: None,
        steps: None,
        snapshot: None,
        view: ViewMode::Water,
        verify: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            "--scene" => {
                result.config.scene = match value::<String>(&mut args, &arg)?.as_str() {
                    "drop" => Scene::Drop,
                    "dam" => Scene::Dam,
                    _ => return Err("scene 必须为 drop 或 dam".into()),
                }
            }
            "--resolution" => result.config.resolution = value(&mut args, &arg)?,
            "--paused" => result.config.paused = true,
            "--frames" => result.frames = Some(value(&mut args, &arg)?),
            "--steps" => result.steps = Some(value(&mut args, &arg)?),
            "--snapshot" => result.snapshot = Some(value(&mut args, &arg)?),
            "--verify" => result.verify = true,
            "--view" => {
                result.view = match value::<String>(&mut args, &arg)?.as_str() {
                    "water" => ViewMode::Water,
                    "mesh" => ViewMode::Mesh,
                    "normals" => ViewMode::Normals,
                    _ => return Err("view 必须为 water、mesh 或 normals".into()),
                }
            }
            _ => return Err(format!("未知选项 {arg}；使用 --help 查看帮助")),
        }
    }
    if !(8..=40).contains(&result.config.resolution) {
        return Err("resolution 必须在 8..40 之间".into());
    }
    if result.frames == Some(0) {
        return Err("frames 必须为正数".into());
    }
    if result.verify && (result.snapshot.is_some() || result.frames.is_some()) {
        return Err("verify 为无窗口验证，不能同时指定 snapshot 或 frames".into());
    }
    if result.verify && result.steps.unwrap_or(6) < 2 {
        return Err("verify 至少需要 2 个子步".into());
    }
    if result.snapshot.as_ref().is_some_and(|path| path.exists()) {
        return Err("snapshot 文件已存在；请选择新路径".into());
    }
    if result.snapshot.is_some() && result.steps.is_none() {
        result.steps = Some(0);
    }
    if result.config.paused && result.steps.is_some_and(|n| n > 0) && !result.verify {
        return Err("paused 与正数 steps 会阻止自动完成；请去掉其中一个".into());
    }
    Ok(Some(result))
}

/// CLI 入口，独立创建事件循环和工作线程；不加载/保存任何个人设置。
pub fn run() -> Result<(), String> {
    let Some(options) = options()? else {
        return Ok(());
    };
    if options.verify {
        return verify::verify(options.config, options.steps.unwrap_or(6));
    }
    println!("{HELP}");
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let paused = options.config.paused || options.steps == Some(0);
    let scene = options.config.scene;
    // 有限子步运行逐帧请求单步，避免最新帧 mailbox 越过指定截图时刻。
    let worker = Worker::spawn(DemoConfig {
        paused: paused || options.steps.is_some(),
        ..options.config
    })?;
    let mut app = App {
        worker: Some(worker),
        renderer: None,
        window: None,
        frame: None,
        camera: default_camera(),
        paused,
        scene,
        expected_generation: 0,
        options,
        presented: 0,
        error: None,
        worker_error: None,
        next_redraw: Instant::now(),
        snapshot_done: false,
    };
    let result = event_loop.run_app(&mut app).map_err(|e| e.to_string());
    if let Some(worker) = app.worker.take() {
        worker.finish()?;
    }
    result?;
    if let Some(error) = app.error {
        return Err(error);
    }
    Ok(())
}

fn default_camera() -> Camera {
    Camera {
        yaw: 0.78,
        pitch: 0.40,
        distance: 4.8,
        target: [1.0, 0.8, 1.0],
    }
}

struct App {
    worker: Option<Worker>,
    renderer: Option<Renderer>,
    window: Option<Arc<Window>>,
    frame: Option<Frame>,
    camera: Camera,
    paused: bool,
    scene: Scene,
    expected_generation: u64,
    options: Options,
    presented: u64,
    error: Option<String>,
    worker_error: Option<String>,
    next_redraw: Instant,
    snapshot_done: bool,
}

impl App {
    fn send(&mut self, command: Command) {
        if let Some(worker) = &self.worker {
            worker.send(command);
        }
    }

    fn reset(&mut self, scene: Scene) {
        self.scene = scene;
        self.expected_generation += 1;
        self.worker_error = None;
        self.send(Command::Reset(scene));
        self.title();
    }

    fn title(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let scene = match self.scene {
            Scene::Drop => "液滴落水",
            Scene::Dam => "水柱溃坝",
        };
        let view = match self.options.view {
            ViewMode::Water => "水面",
            ViewMode::Mesh => "三角网格",
            ViewMode::Normals => "法线",
        };
        if let Some(error) = &self.worker_error {
            window.set_title(&format!(
                "Tall Cell Fluid | 计算失败：{error} | R 重置 / Esc 退出"
            ));
        } else if let Some(frame) = &self.frame {
            let state = if frame.generation < self.expected_generation {
                "重置中"
            } else if self.paused && !frame.paused {
                "等待暂停"
            } else if self.paused {
                "已暂停"
            } else {
                "运行中"
            };
            window.set_title(&format!("Tall Cell Fluid | {scene} · {view} | {state} | t={:.3}s · step={} · {} triangles | Space/N/R · 1/2 · V",
                frame.time,frame.steps,frame.mesh.indices.len()/3));
        } else {
            window
                .set_title("Tall Cell Fluid | 正在初始化 GPU 与初始水面… | 鼠标拖动/滚轮调整相机");
        }
    }

    fn update(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(event) = self.worker.as_ref().and_then(Worker::take_latest) {
            match event {
                WorkerEvent::Frame(frame) => {
                    if frame.generation >= self.expected_generation {
                        if let Some(renderer) = &mut self.renderer
                            && let Err(error) = renderer.upload(&frame.mesh, frame.extent)
                        {
                            self.fail(event_loop, error);
                            return;
                        }
                        if self.frame.is_none() {
                            println!("Simulation adapter: {}", frame.adapter);
                        }
                        self.frame = Some(*frame);
                        self.worker_error = None;
                        if !self.paused
                            && self
                                .options
                                .steps
                                .is_some_and(|n| self.frame.as_ref().is_some_and(|f| f.steps < n))
                        {
                            self.send(Command::Step);
                        }
                    }
                }
                WorkerEvent::Error(error) => {
                    eprintln!("GPU simulation: {error}");
                    self.worker_error = Some(error.clone());
                    self.paused = true;
                    if self.options.frames.is_some()
                        || self.options.steps.is_some()
                        || self.options.snapshot.is_some()
                    {
                        self.fail(event_loop, error);
                        return;
                    }
                }
            }
            self.title();
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: String) {
        self.error = Some(error);
        self.send(Command::Stop);
        event_loop.exit();
    }

    fn draw(&mut self, event_loop: &ActiveEventLoop) {
        self.update(event_loop);
        if self.error.is_some() {
            return;
        }
        let (Some(renderer), Some(window)) = (&mut self.renderer, &self.window) else {
            return;
        };
        let data = PanelData {
            frame: self.frame.as_ref(),
            paused: self.paused,
            scene: self.scene,
            view: self.options.view,
            resetting: self
                .frame
                .as_ref()
                .is_some_and(|f| f.generation < self.expected_generation),
            error: self.worker_error.as_deref(),
            finite_steps: self.options.steps.is_some(),
            resolution: self.options.config.resolution,
        };
        let actions = match renderer.draw(window, self.camera, self.options.view, &data) {
            Ok(actions) => actions,
            Err(error) => {
                self.fail(event_loop, error);
                return;
            }
        };
        // 冒烟帧数从实际水面就绪后开始，避免初始化画面耗尽整个验证窗口。
        if self.frame.is_some() {
            self.presented += 1;
        }
        let reached = self.options.steps.is_some_and(|n| {
            self.frame
                .as_ref()
                .is_some_and(|f| f.steps >= n && f.generation == self.expected_generation)
        });
        if reached {
            if let Some(path) = &self.options.snapshot
                && !self.snapshot_done
            {
                if let Err(error) = renderer.snapshot(self.camera, self.options.view, path) {
                    self.fail(event_loop, error);
                    return;
                }
                self.snapshot_done = true;
                println!(
                    "Saved snapshot: {} (step {})",
                    path.display(),
                    self.frame.as_ref().unwrap().steps
                );
            }
            self.send(Command::Stop);
            event_loop.exit();
        } else if self.options.frames.is_some_and(|n| self.presented >= n) {
            if self.options.snapshot.is_some() {
                self.fail(event_loop, "达到 frames 上限，尚未到达截图目标子步".into());
            } else {
                self.send(Command::Stop);
                event_loop.exit();
            }
        }
        for action in actions {
            match action {
                GuiAction::SetPaused(paused) => {
                    self.paused = paused;
                    self.send(Command::SetPaused(paused));
                }
                GuiAction::Step => self.send(Command::Step),
                GuiAction::Reset(scene) => self.reset(scene),
                GuiAction::View(view) => self.options.view = view,
                GuiAction::ResetCamera => self.camera = default_camera(),
                GuiAction::Orbit(delta) => {
                    self.camera.yaw -= delta.x * 0.006;
                    self.camera.pitch = (self.camera.pitch + delta.y * 0.006).clamp(0.08, 1.45);
                }
                GuiAction::Zoom(scroll) => {
                    self.camera.distance =
                        (self.camera.distance * (-scroll * 0.1).exp()).clamp(2.2, 12.0);
                }
            }
            self.title();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Tall Cell Fluid")
                .with_inner_size(winit::dpi::LogicalSize::new(1152, 800)),
        ) {
            Ok(window) => Arc::new(window),
            Err(e) => {
                self.fail(event_loop, e.to_string());
                return;
            }
        };
        match pollster::block_on(Renderer::new(window.clone())) {
            Ok(renderer) => {
                self.renderer = Some(renderer);
                self.window = Some(window);
                self.title();
            }
            Err(error) => self.fail(event_loop, error),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self.window.as_ref().is_none_or(|w| w.id() != window_id) {
            return;
        }
        let consumed = match (&mut self.renderer, &self.window) {
            (Some(renderer), Some(window)) => renderer.handle_event(window, &event),
            _ => false,
        };
        match event {
            WindowEvent::CloseRequested => {
                self.send(Command::Stop);
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.draw(event_loop),
            WindowEvent::KeyboardInput { event, .. }
                if !consumed && event.state == ElementState::Pressed && !event.repeat =>
            {
                if let PhysicalKey::Code(key) = event.physical_key {
                    match key {
                        KeyCode::Escape => {
                            self.send(Command::Stop);
                            event_loop.exit();
                        }
                        KeyCode::Space if self.options.steps.is_none() => {
                            self.paused = !self.paused;
                            self.send(Command::SetPaused(self.paused));
                        }
                        KeyCode::KeyN if self.paused => self.send(Command::Step),
                        KeyCode::KeyR => self.reset(self.scene),
                        KeyCode::Digit1 => self.reset(Scene::Drop),
                        KeyCode::Digit2 => self.reset(Scene::Dam),
                        KeyCode::KeyV => {
                            self.options.view = match self.options.view {
                                ViewMode::Water => ViewMode::Mesh,
                                ViewMode::Mesh => ViewMode::Normals,
                                ViewMode::Normals => ViewMode::Water,
                            }
                        }
                        KeyCode::Home => self.camera = default_camera(),
                        _ => {}
                    }
                    self.title();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.update(event_loop);
        let now = Instant::now();
        if now >= self.next_redraw {
            if let Some(window) = &self.window
                && !window.is_minimized().unwrap_or(false)
            {
                window.request_redraw();
            }
            self.next_redraw = now + Duration::from_millis(16);
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_redraw));
    }
}

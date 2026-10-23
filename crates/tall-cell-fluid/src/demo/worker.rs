//! 后台 GPU 模拟和表面提取；窗口线程只消费最新完整快照。
//!
//! `simulation_ms`（ms/step）与 `mesh_ms`（ms/mesh update）分别记录模拟子步和
//! 表面生成的 CPU 墙钟耗时，均包含 GPU 工作、同步和回读等待；并非 GPU timestamp。
//! `elapsed_ms` 是两者之和，不计设备/场景初始化，也不用于推算物理时间。

use crate::gpu::{GpuContext, GpuSimulation, default_config, extract_surface};
use crate::surface::{SurfaceLimits, SurfaceMesh};
use crate::{Domain, Grid, StepReport};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Drop,
    Dam,
}

#[derive(Clone, Copy, Debug)]
pub struct DemoConfig {
    /// 每轴单元数；演示域固定为 2 米立方体。
    pub resolution: usize,
    pub scene: Scene,
    pub paused: bool,
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            resolution: 16,
            scene: Scene::Drop,
            paused: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Frame {
    /// 每次 Reset 尝试递增，包括重置失败的情况。
    pub generation: u64,
    pub mesh: SurfaceMesh,
    pub extent: [f32; 3],
    pub time: f64,
    pub steps: usize,
    pub report: Option<StepReport>,
    /// 本步推进前计算的 CFL 步长上限；初始帧与重置帧为 None。
    pub stable_dt: Option<f64>,
    /// 最后一个模拟子步的 CPU 墙钟耗时（ms/step），包含 CFL 计算、GPU 工作与回读等待。
    /// 初始帧和重置帧为 None；初始化耗时不充当模拟子步样本。
    pub simulation_ms: Option<f64>,
    /// 最后一次表面更新的 CPU 墙钟耗时（ms/mesh update）：halo、GPU 提取与回读整理。
    pub mesh_ms: f64,
    /// simulation_ms.unwrap_or(0) + mesh_ms；暂停确认沿用最近样本，不新增零耗时样本。
    pub elapsed_ms: f64,
    pub adapter: String,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    SetPaused(bool),
    /// 仅暂停时推进一个实际模拟子步。
    Step,
    Reset(Scene),
    Stop,
}

#[derive(Clone, Debug)]
pub enum WorkerEvent {
    Frame(Box<Frame>),
    Error(String),
}

type Latest = Arc<Mutex<Option<WorkerEvent>>>;

pub struct Worker {
    sender: mpsc::Sender<Command>,
    latest: Latest,
    handle: Option<JoinHandle<Result<(), String>>>,
}

impl Worker {
    /// GPU 初始化也在后台执行；设备错误以 WorkerEvent::Error 发布。
    pub fn spawn(config: DemoConfig) -> Result<Self, String> {
        if !(8..=40).contains(&config.resolution) {
            return Err("演示分辨率必须在 8～40 之间（每轴单元数）".into());
        }
        let (sender, receiver) = mpsc::channel();
        let latest = Arc::new(Mutex::new(None));
        let output = Arc::clone(&latest);
        let handle = thread::Builder::new()
            .name("tall-cell-fluid-demo".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run(config, receiver, &output)
                }));
                match result {
                    Ok(result) => {
                        if let Err(error) = &result {
                            publish(&output, WorkerEvent::Error(error.clone()));
                        }
                        result
                    }
                    Err(payload) => {
                        let detail = payload
                            .downcast_ref::<String>()
                            .map(String::as_str)
                            .or_else(|| payload.downcast_ref::<&str>().copied())
                            .unwrap_or("未知 panic");
                        let error = format!("模拟后台线程异常：{detail}");
                        publish(&output, WorkerEvent::Error(error.clone()));
                        Err(error)
                    }
                }
            })
            .map_err(|e| format!("无法创建模拟后台线程：{e}"))?;
        Ok(Self {
            sender,
            latest,
            handle: Some(handle),
        })
    }

    pub fn send(&self, command: Command) {
        if self.sender.send(command).is_err() {
            publish(
                &self.latest,
                WorkerEvent::Error("模拟后台线程已退出".into()),
            );
        }
    }

    /// 取走最新事件。较旧快照被新快照替换，不累计大型三角形数组。
    pub fn take_latest(&self) -> Option<WorkerEvent> {
        self.latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    /// 请求停止并等待当前 GPU 阶段结束，只回收本 Worker 创建的线程。
    pub fn finish(mut self) -> Result<(), String> {
        let _ = self.sender.send(Command::Stop);
        match self.handle.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| "模拟后台线程 join 失败".to_owned())?,
            None => Ok(()),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // UI 意外提前退出时也请求停止；析构不阻塞窗口线程。
        let _ = self.sender.send(Command::Stop);
    }
}

fn publish(latest: &Latest, event: WorkerEvent) {
    *latest
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(event);
}

fn run(
    mut config: DemoConfig,
    commands: mpsc::Receiver<Command>,
    latest: &Latest,
) -> Result<(), String> {
    let mut generation = 0u64;
    // 初始化失败仍保持命令通道存活，用户可 Reset 重试设备初始化。
    let context = loop {
        match pollster::block_on(GpuContext::new()) {
            Ok(context) => break context,
            Err(error) => publish(latest, WorkerEvent::Error(error)),
        }
        loop {
            match commands.recv() {
                Ok(Command::Reset(scene)) => {
                    generation = generation.checked_add(1).ok_or("重置代数溢出")?;
                    config.scene = scene;
                    break;
                }
                Ok(Command::SetPaused(value)) => config.paused = value,
                Ok(Command::Step) => {}
                Ok(Command::Stop) | Err(_) => return Ok(()),
            }
        }
    };
    let mut simulation = make_simulation(&context, config.resolution, config.scene)?;
    let mut paused = config.paused;
    let mut cached = snapshot(&simulation, generation, paused, None, None, None)?;
    publish(latest, WorkerEvent::Frame(Box::new(cached.clone())));

    loop {
        let command = if paused {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => return Ok(()),
            }
        } else {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        };
        match command {
            Some(Command::Stop) => return Ok(()),
            Some(Command::SetPaused(value)) => {
                paused = value;
                cached.paused = paused;
                publish(latest, WorkerEvent::Frame(Box::new(cached.clone())));
                continue;
            }
            Some(Command::Reset(scene)) => {
                generation = generation.checked_add(1).ok_or("重置代数溢出")?;
                match make_simulation(&context, config.resolution, scene).and_then(|new| {
                    let frame = snapshot(&new, generation, paused, None, None, None)?;
                    Ok((new, frame))
                }) {
                    Ok((new, frame)) => {
                        simulation = new;
                        config.scene = scene;
                        cached = frame;
                        publish(latest, WorkerEvent::Frame(Box::new(cached.clone())));
                    }
                    Err(error) => {
                        paused = true;
                        cached.paused = true;
                        publish(latest, WorkerEvent::Error(format!("重置失败：{error}")));
                    }
                }
                continue;
            }
            Some(Command::Step) if !paused => continue,
            Some(Command::Step) | None => {}
        }

        let started = Instant::now();
        let stable_dt = simulation.stable_time_step();
        match simulation.step(stable_dt).and_then(|report| {
            let simulation_ms = started.elapsed().as_secs_f64() * 1000.0;
            snapshot(
                &simulation,
                generation,
                paused,
                Some(report),
                Some(stable_dt),
                Some(simulation_ms),
            )
        }) {
            Ok(frame) => {
                cached = frame;
                publish(latest, WorkerEvent::Frame(Box::new(cached.clone())));
            }
            Err(error) => {
                // 数值失败会暂停，保留最近成功快照，并允许用户重置场景。
                paused = true;
                cached.paused = true;
                publish(latest, WorkerEvent::Error(format!("模拟已暂停：{error}")));
            }
        }
    }
}

fn make_simulation(
    context: &GpuContext,
    resolution: usize,
    scene: Scene,
) -> Result<GpuSimulation<'_>, String> {
    let mut config = default_config();
    config.max_time_step = 1.0 / 30.0;
    GpuSimulation::from_grid(context, scene_grid(resolution, scene)?, config)
}

fn scene_grid(resolution: usize, scene: Scene) -> Result<Grid, String> {
    let mut grid = Grid::new(Domain::new([resolution; 3], 2.0 / resolution as f64)?);
    match scene {
        Scene::Drop => grid.set_liquid_sdf(|p| {
            let sphere =
                ((p[0] - 1.0).powi(2) + (p[1] - 1.35).powi(2) + (p[2] - 1.0).powi(2)).sqrt() - 0.28;
            (p[1] - 0.5).min(sphere)
        }),
        Scene::Dam => grid.set_liquid_sdf(|p| {
            let q = [
                (p[0] - 0.375).abs() - 0.375,
                (p[1] - 0.7).abs() - 0.7,
                (p[2] - 1.0).abs() - 1.0,
            ];
            let outside = q.iter().map(|v| v.max(0.0).powi(2)).sum::<f64>().sqrt();
            let inside = q.into_iter().fold(f64::NEG_INFINITY, f64::max).min(0.0);
            (p[1] - 0.12).min(outside + inside)
        }),
    }
    Ok(grid)
}

fn snapshot(
    simulation: &GpuSimulation<'_>,
    generation: u64,
    paused: bool,
    report: Option<StepReport>,
    stable_dt: Option<f64>,
    simulation_ms: Option<f64>,
) -> Result<Frame, String> {
    let mesh_started = Instant::now();
    // 仅渲染采样添加一层空气，封闭接触槽壁的水体侧面；不改变模拟 phi 或压力域。
    let surface_grid = surface_halo(&simulation.grid)?;
    let mesh = extract_surface(simulation.context, &surface_grid, SurfaceLimits::default())?;
    let mesh_ms = mesh_started.elapsed().as_secs_f64() * 1000.0;
    Ok(Frame {
        generation,
        mesh,
        extent: simulation.grid.domain.extent().map(|v| v as f32),
        time: simulation.time,
        steps: simulation.steps,
        report,
        stable_dt,
        simulation_ms,
        mesh_ms,
        elapsed_ms: simulation_ms.unwrap_or(0.0) + mesh_ms,
        adapter: simulation.context.adapter_name.clone(),
        paused,
    })
}

fn surface_halo(grid: &Grid) -> Result<Grid, String> {
    let mut domain = Domain::new(grid.domain.size.map(|n| n + 2), grid.domain.cell_size)?;
    domain.origin = grid.domain.origin.map(|v| v - grid.domain.cell_size);
    domain.validate()?;
    let mut output = Grid::new(domain);
    for cell in 0..domain.cell_count() {
        let p = domain.coord(cell);
        let source =
            std::array::from_fn(|axis| p[axis].saturating_sub(1).min(grid.domain.size[axis] - 1));
        let value = grid.phi[grid.domain.index(source)];
        // 只对渲染隐式表面与物理域求交。内侧边界距为负半格，外侧为正半格，
        // 保证四面体斜边的零点也不会越过槽壁；单纯镜像 phi 会在斜边上外伸。
        let position = domain.center(p);
        let extent = grid.domain.extent();
        let boundary_distance = (0..3)
            .map(|axis| {
                (grid.domain.origin[axis] - position[axis])
                    .max(position[axis] - grid.domain.origin[axis] - extent[axis])
            })
            .fold(f64::NEG_INFINITY, f64::max);
        output.phi[cell] = value.max(boundary_distance);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_halo_preserves_physical_state_and_offsets_samples() {
        let source = scene_grid(16, Scene::Drop).unwrap();
        let original = source.phi.clone();
        let halo = surface_halo(&source).unwrap();
        assert_eq!(halo.domain.size, [18; 3]);
        assert_eq!(halo.domain.origin, [-0.125; 3]);
        assert_eq!(source.phi, original);
        for cell in 0..source.domain.cell_count() {
            let p = source.domain.coord(cell);
            let position = source.domain.center(p);
            let boundary = position
                .into_iter()
                .map(|x| (-x).max(x - 2.0))
                .fold(f64::NEG_INFINITY, f64::max);
            assert_eq!(
                source.phi[cell].max(boundary),
                halo.phi[halo.domain.index(p.map(|v| v + 1))]
            );
        }
        for cell in 0..halo.domain.cell_count() {
            let p = halo.domain.coord(cell);
            if p.into_iter().any(|v| v == 0 || v == 17) {
                assert!(halo.phi[cell] > 0.0);
            }
        }
    }

    #[test]
    #[ignore = "真实 GPU 的演示场景长时间稳定性验证"]
    fn demo_scenes_run_half_second_on_gpu() {
        let context = pollster::block_on(GpuContext::new()).unwrap();
        for scene in [Scene::Drop, Scene::Dam] {
            let mut sim = make_simulation(&context, 16, scene).unwrap();
            let original_phi = sim.grid.phi.clone();
            let initial_volume = sim.grid.liquid_volume();
            let clock = Instant::now();
            let mut fallback_count = 0;
            while sim.time < 0.5 {
                let dt = sim.stable_time_step().min(0.5 - sim.time);
                let report = sim
                    .step(dt)
                    .unwrap_or_else(|e| panic!("{scene:?}, t={}: {e}", sim.time));
                assert!(report.projection.converged);
                if report.projection.fallback_used {
                    fallback_count += 1;
                }
            }
            let surface = snapshot(&sim, 0, true, None, None, None).unwrap();
            assert!(!surface.mesh.vertices.is_empty());
            assert!(
                sim.grid
                    .phi
                    .iter()
                    .zip(&original_phi)
                    .any(|(a, b)| (a - b).abs() > 0.05)
            );
            assert!((sim.grid.liquid_volume() / initial_volume - 1.0).abs() < 0.01);
            eprintln!(
                "{scene:?}: t={:.3}s, {} steps, {:.1}ms wall, {} GPU fine fallbacks, {} triangles",
                sim.time,
                sim.steps,
                clock.elapsed().as_secs_f64() * 1000.0,
                fallback_count,
                surface.mesh.indices.len() / 3
            );
        }
    }
}

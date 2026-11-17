//! Background GPU simulation and surface extraction; the window thread consumes only the latest complete snapshot.
//!
//! `simulation_ms` (ms/step) and `mesh_ms` (ms/mesh update) record CPU wall-clock time for simulation substeps and
//! surface generation respectively, including GPU work, synchronization, and readback waits; these are not GPU timestamps.
//! `elapsed_ms` is their sum, excluding device/scene initialization, and is not used to infer physical time.

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
    /// Cells per axis; the demo domain is fixed at a 2-meter cube.
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
    /// Incremented for every Reset attempt, including failed resets.
    pub generation: u64,
    pub mesh: SurfaceMesh,
    pub extent: [f32; 3],
    pub time: f64,
    pub steps: usize,
    pub report: Option<StepReport>,
    /// CFL time step limit computed before this step; None for initial and reset frames.
    pub stable_dt: Option<f64>,
    /// CPU wall-clock time of the last simulation substep (ms/step), including CFL computation, GPU work, and readback waits.
    /// None for initial and reset frames; initialization time is not used as a substep sample.
    pub simulation_ms: Option<f64>,
    /// CPU wall-clock time of the last surface update (ms/mesh update): halo, GPU extraction, and readback processing.
    pub mesh_ms: f64,
    /// simulation_ms.unwrap_or(0) + mesh_ms; pause confirmation retains the latest sample without adding a zero-duration sample.
    pub elapsed_ms: f64,
    pub adapter: String,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Command {
    SetPaused(bool),
    /// Advance one actual simulation substep only while paused.
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
    /// GPU initialization also runs in the background; device errors are published as WorkerEvent::Error.
    pub fn spawn(config: DemoConfig) -> Result<Self, String> {
        if !(8..=40).contains(&config.resolution) {
            return Err("Demo resolution must be between 8 and 40 (cells per axis)".into());
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
                            .unwrap_or("unknown panic");
                        let error = format!("Simulation worker thread panicked: {detail}");
                        publish(&output, WorkerEvent::Error(error.clone()));
                        Err(error)
                    }
                }
            })
            .map_err(|e| format!("Cannot create simulation worker thread: {e}"))?;
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
                WorkerEvent::Error("Simulation worker thread has exited".into()),
            );
        }
    }

    /// Take the latest event. New snapshots replace older ones, avoiding accumulation of large triangle arrays.
    pub fn take_latest(&self) -> Option<WorkerEvent> {
        self.latest
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }

    /// Request a stop and wait for the current GPU stage, joining only the thread created by this Worker.
    pub fn finish(mut self) -> Result<(), String> {
        let _ = self.sender.send(Command::Stop);
        match self.handle.take() {
            Some(handle) => handle
                .join()
                .map_err(|_| "Failed to join simulation worker thread".to_owned())?,
            None => Ok(()),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Request a stop even if the UI exits unexpectedly; destruction does not block the window thread.
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
    // Keep the command channel alive after initialization failure so the user can Reset to retry device initialization.
    let context = loop {
        match pollster::block_on(GpuContext::new()) {
            Ok(context) => break context,
            Err(error) => publish(latest, WorkerEvent::Error(error)),
        }
        loop {
            match commands.recv() {
                Ok(Command::Reset(scene)) => {
                    generation = generation
                        .checked_add(1)
                        .ok_or("Reset generation overflow")?;
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
                generation = generation
                    .checked_add(1)
                    .ok_or("Reset generation overflow")?;
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
                        publish(latest, WorkerEvent::Error(format!("Reset failed: {error}")));
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
                // A numerical failure pauses, retains the latest successful snapshot, and allows the user to reset the scene.
                paused = true;
                cached.paused = true;
                publish(
                    latest,
                    WorkerEvent::Error(format!("Simulation paused: {error}")),
                );
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
    // Add an air layer only for rendering samples, closing water sides against tank walls without changing simulation phi or the pressure domain.
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
        // Intersect only the rendered implicit surface with the physical domain. Boundary distances are negative half a cell inside and positive outside,
        // keeping zero crossings on tetrahedron diagonals within the tank walls; simply mirroring phi would extend beyond them.
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
    #[ignore = "Long-duration stability verification of demo scenes on a real GPU"]
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

//! Headless demo verification: real GPU advancement, surface changes, and the worker pause/single-step/reset protocol.

use std::thread;
use std::time::{Duration, Instant};

use super::worker::{Command, DemoConfig, Frame, Scene, Worker, WorkerEvent};
use crate::surface::SurfaceMesh;

const FRAME_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Perform `steps` real GPU single steps per scene, then verify playback, pause confirmation, and reset.
///
/// At least two steps: from rest, the first mainly establishes velocity, and the interface may not move measurably until the second.
pub fn verify(mut config: DemoConfig, steps: usize) -> Result<(), String> {
    if steps < 2 {
        return Err("Demo verification requires --steps of at least 2 to check surface motion after velocity develops".into());
    }
    config.paused = true;
    let first_scene = config.scene;
    let worker = Worker::spawn(config)?;
    let result = verify_worker(&worker, first_scene, steps);
    worker.send(Command::Stop);
    let shutdown = worker.finish();
    match (result, shutdown) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(shutdown)) => {
            Err(format!("{error}; worker thread cleanup failed: {shutdown}"))
        }
    }
}

fn verify_worker(worker: &Worker, first_scene: Scene, steps: usize) -> Result<(), String> {
    let mut frame = wait_frame(worker, "initialization", |frame| {
        if frame.generation != 0 || frame.steps != 0 || frame.time != 0.0 || !frame.paused {
            return Err("Paused startup did not produce a zero-time snapshot".into());
        }
        Ok(true)
    })?;
    check_reset_frame(&frame)?;
    check_surface_bounds(&frame)?;
    println!("Demo verification adapter: {}", frame.adapter);
    println!(
        "PASS demo initial timing: simulation=not run, mesh={:.3}ms, total={:.3}ms",
        frame.mesh_ms, frame.elapsed_ms
    );
    let second_scene = match first_scene {
        Scene::Drop => Scene::Dam,
        Scene::Dam => Scene::Drop,
    };
    for (scene_index, scene) in [first_scene, second_scene].into_iter().enumerate() {
        let start_positions = sorted_positions(&frame.mesh);
        let mut initial_volume = None;
        for step in 0..steps {
            frame = single_step(worker, &frame)?;
            check_surface_bounds(&frame)?;
            let report = frame
                .report
                .as_ref()
                .ok_or("Single step completed without a solver report")?;
            let volume = *initial_volume.get_or_insert(report.volume_before);
            // This is a mass drift regression bound for demo scenes, not a claim that level-set correction is strictly conservative.
            if (report.volume_after - volume).abs() > 0.15 * volume {
                return Err(format!(
                    "{scene:?} scene volume estimate drifted over 15%: {volume} -> {} m³",
                    report.volume_after
                ));
            }
            println!(
                "PASS demo {scene:?} step {}/{}: t={:.6}s, dt={:.6}s, triangles={}, divergence={:.3e}/s, simulation={:.3}ms, mesh={:.3}ms",
                step + 1,
                steps,
                frame.time,
                report.dt,
                frame.mesh.indices.len() / 3,
                report.projection.divergence_after,
                frame
                    .simulation_ms
                    .ok_or("Single step completed without simulation timing")?,
                frame.mesh_ms,
            );
        }
        if !surface_moved(&start_positions, &frame.mesh, frame.extent) {
            return Err(format!(
                "{scene:?} advanced {steps} steps, but the extracted surface has no measurable position change"
            ));
        }
        frame = confirm_paused(worker, &frame)?;
        ensure_quiet(worker, &frame)?;
        println!("PASS demo {scene:?}: surface moved; paused snapshot stayed fixed");

        if scene_index == 0 {
            let generation = frame
                .generation
                .checked_add(1)
                .ok_or("Generation overflow")?;
            let old_positions = sorted_positions(&frame.mesh);
            worker.send(Command::Reset(second_scene));
            frame = wait_frame(worker, "scene reset", |next| {
                if next.generation > generation {
                    return Err("A single reset unexpectedly skipped multiple generations".into());
                }
                Ok(next.generation == generation)
            })?;
            check_reset_frame(&frame)?;
            check_surface_bounds(&frame)?;
            if !surface_moved(&old_positions, &frame.mesh, frame.extent) {
                return Err("Surface remains identical to the previous scene after resetting to another scene".into());
            }
            println!(
                "PASS demo reset: generation={generation}, time=0, steps=0, paused=true, simulation=not run, mesh={:.3}ms",
                frame.mesh_ms
            );
        }
    }

    // Playback confirmation and computation results may arrive separately; then allow the in-flight substep to finish before awaiting pause confirmation.
    let generation = frame.generation;
    let previous_steps = frame.steps;
    worker.send(Command::SetPaused(false));
    frame = wait_frame(worker, "resume playback", |next| {
        if next.generation != generation {
            return Err("Resuming playback unexpectedly changed the scene generation".into());
        }
        Ok(!next.paused && next.steps > previous_steps)
    })?;
    check_step_report(&frame)?;
    check_surface_bounds(&frame)?;
    let playing_steps = frame.steps;
    worker.send(Command::SetPaused(true));
    frame = wait_frame(worker, "pause after playback", |next| {
        if next.generation != generation || next.steps < playing_steps {
            return Err("Pause confirmation returned to an earlier scene or time step".into());
        }
        Ok(next.paused)
    })?;
    ensure_quiet(worker, &frame)?;
    frame = single_step(worker, &frame)?;
    check_surface_bounds(&frame)?;
    ensure_quiet(worker, &frame)?;
    println!(
        "PASS demo playback/pause/single-step: generation={}, steps={}, t={:.6}s",
        frame.generation, frame.steps, frame.time
    );
    println!(
        "PASS: demo GPU motion, surface, CFL, stage timings and worker protocol checks completed"
    );
    Ok(())
}

fn wait_frame(
    worker: &Worker,
    stage: &str,
    accept: impl Fn(&Frame) -> Result<bool, String>,
) -> Result<Frame, String> {
    let started = Instant::now();
    let mut last = String::from("No snapshot received yet");
    while started.elapsed() < FRAME_TIMEOUT {
        if let Some(event) = worker.take_latest() {
            match event {
                WorkerEvent::Frame(frame) => {
                    check_frame(&frame)?;
                    last = format!(
                        "generation={}, steps={}, time={}, paused={}",
                        frame.generation, frame.steps, frame.time, frame.paused
                    );
                    if accept(&frame)? {
                        return Ok(*frame);
                    }
                }
                WorkerEvent::Error(error) => return Err(format!("{stage}: {error}")),
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err(format!("Waiting for {stage} exceeded 60 seconds ({last})"))
}

fn single_step(worker: &Worker, previous: &Frame) -> Result<Frame, String> {
    if !previous.paused {
        return Err(
            "Verification requires issuing a single-step command after pause confirmation".into(),
        );
    }
    let expected = previous.steps.checked_add(1).ok_or("Step count overflow")?;
    worker.send(Command::Step);
    let next = wait_frame(worker, "paused single step", |next| {
        if next.generation != previous.generation {
            return Err("Single step unexpectedly changed the scene generation".into());
        }
        if next.steps > expected || next.steps < previous.steps {
            return Err(format!(
                "Single step expected steps={expected}, actual steps={}",
                next.steps
            ));
        }
        Ok(next.steps == expected)
    })?;
    if !next.paused || next.time <= previous.time {
        return Err("Single step did not remain paused or simulation time did not increase".into());
    }
    check_step_report(&next)?;
    let dt = next.report.as_ref().ok_or("Missing single-step report")?.dt;
    if ((next.time - previous.time) - dt).abs() > 1e-9 * (1.0 + next.time.abs()) {
        return Err("Snapshot time increment does not match solver dt".into());
    }
    Ok(next)
}

fn confirm_paused(worker: &Worker, previous: &Frame) -> Result<Frame, String> {
    worker.send(Command::SetPaused(true));
    wait_frame(worker, "pause confirmation", |next| {
        if next.generation != previous.generation
            || next.steps != previous.steps
            || next.time != previous.time
        {
            return Err("Already paused simulation advanced during pause confirmation".into());
        }
        check_retained_timings(previous, next)?;
        Ok(next.paused)
    })
}

fn ensure_quiet(worker: &Worker, previous: &Frame) -> Result<(), String> {
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(200) {
        if let Some(event) = worker.take_latest() {
            match event {
                WorkerEvent::Frame(next) => {
                    if next.generation != previous.generation
                        || next.steps != previous.steps
                        || next.time != previous.time
                        || !next.paused
                    {
                        return Err("State advanced after pause confirmation without a single-step/reset command".into());
                    }
                    check_retained_timings(previous, &next)?;
                }
                WorkerEvent::Error(error) => return Err(error),
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

fn check_reset_frame(frame: &Frame) -> Result<(), String> {
    if frame.time != 0.0
        || frame.steps != 0
        || !frame.paused
        || frame.report.is_some()
        || frame.stable_dt.is_some()
        || frame.simulation_ms.is_some()
    {
        return Err("Initialization/reset did not clear simulation time, steps, simulation timing, or the previous solver report".into());
    }
    Ok(())
}

fn check_step_report(frame: &Frame) -> Result<(), String> {
    if frame.simulation_ms.is_none() {
        return Err("Actual simulation substep lacks separate simulation timing".into());
    }
    let report = frame
        .report
        .as_ref()
        .ok_or("GPU advancement snapshot lacks a solver report")?;
    let limit = frame
        .stable_dt
        .ok_or("GPU advancement snapshot lacks the pre-step CFL time step limit")?;
    if !limit.is_finite()
        || limit <= 0.0
        || !report.dt.is_finite()
        || report.dt <= 0.0
        || report.dt > limit * (1.0 + 1e-6)
    {
        return Err(format!(
            "Time step violates CFL limit: dt={}, limit={limit}",
            report.dt
        ));
    }
    if !report.time.is_finite()
        || (report.time - frame.time).abs() > 1e-9 * (1.0 + frame.time.abs())
        || !report.volume_before.is_finite()
        || !report.volume_after.is_finite()
        || report.volume_before <= 0.0
        || report.volume_after <= 0.0
        || !report.volume_correction.is_finite()
        || !report.projection.divergence_after.is_finite()
        || !report.projection.converged
    {
        return Err(
            "GPU solver report contains invalid state or pressure projection did not converge"
                .into(),
        );
    }
    Ok(())
}

fn check_frame(frame: &Frame) -> Result<(), String> {
    check_timings(frame)?;
    if !frame.time.is_finite()
        || frame.time < 0.0
        || !frame.elapsed_ms.is_finite()
        || frame.elapsed_ms < 0.0
        || frame.adapter.is_empty()
        || frame.extent.iter().any(|v| !v.is_finite() || *v <= 0.0)
    {
        return Err("Demo snapshot time, extent, or adapter information is invalid".into());
    }
    let mesh = &frame.mesh;
    if mesh.vertices.is_empty() || mesh.indices.is_empty() || !mesh.indices.len().is_multiple_of(3)
    {
        return Err("Demo water has no valid triangle surface".into());
    }
    for vertex in &mesh.vertices {
        if vertex
            .position
            .iter()
            .chain(&vertex.normal)
            .any(|v| !v.is_finite())
        {
            return Err("Water surface vertices or normals contain non-finite values".into());
        }
        let normal_squared: f32 = vertex.normal.iter().map(|v| v * v).sum();
        if !(0.9..=1.1).contains(&normal_squared) {
            return Err("Water surface normals are not validly normalized".into());
        }
    }
    if mesh
        .indices
        .iter()
        .any(|&index| index as usize >= mesh.vertices.len())
    {
        return Err("Water surface triangle index out of bounds".into());
    }
    Ok(())
}

fn check_timings(frame: &Frame) -> Result<(), String> {
    if !frame.mesh_ms.is_finite()
        || frame.mesh_ms < 0.0
        || frame
            .simulation_ms
            .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(
            "Simulation/mesh generation stage timings are not finite nonnegative numbers".into(),
        );
    }
    let sum = frame.simulation_ms.unwrap_or(0.0) + frame.mesh_ms;
    let tolerance = 8.0 * f64::EPSILON * sum.max(1.0);
    if !sum.is_finite()
        || !frame.elapsed_ms.is_finite()
        || (frame.elapsed_ms - sum).abs() > tolerance
    {
        return Err(format!(
            "Total worker time should equal simulation plus mesh generation (excluding initialization/waiting): total={}, simulation={:?}, mesh={} ms",
            frame.elapsed_ms, frame.simulation_ms, frame.mesh_ms
        ));
    }
    Ok(())
}

fn check_retained_timings(previous: &Frame, next: &Frame) -> Result<(), String> {
    if next.simulation_ms != previous.simulation_ms
        || next.mesh_ms != previous.mesh_ms
        || next.elapsed_ms != previous.elapsed_ms
    {
        return Err(
            "Pause confirmation overwrote the timing of the most recently completed work".into(),
        );
    }
    Ok(())
}

fn check_surface_bounds(frame: &Frame) -> Result<(), String> {
    for vertex in &frame.mesh.vertices {
        for (axis, &position) in vertex.position.iter().enumerate() {
            // Before demo extraction, clip using the physical domain's box distance field; diagonals at wall corners should also remain inside.
            let tolerance = frame.extent[axis] * 1e-4;
            if position < -tolerance || position > frame.extent[axis] + tolerance {
                return Err("Water surface vertex lies outside the closed demo domain".into());
            }
        }
    }
    Ok(())
}

fn sorted_positions(mesh: &SurfaceMesh) -> Vec<[f32; 3]> {
    let mut positions: Vec<_> = mesh.vertices.iter().map(|v| v.position).collect();
    positions.sort_unstable_by(|a, b| {
        a[0].total_cmp(&b[0])
            .then(a[1].total_cmp(&b[1]))
            .then(a[2].total_cmp(&b[2]))
    });
    positions
}

fn surface_moved(before: &[[f32; 3]], mesh: &SurfaceMesh, extent: [f32; 3]) -> bool {
    // GPU atomic allocation can change triangle output order, so raw vertex arrays cannot be compared directly.
    let after = sorted_positions(mesh);
    if before.len() != after.len() {
        return true;
    }
    let tolerance = extent.into_iter().fold(0.0_f32, f32::max) * 1e-6;
    before
        .iter()
        .zip(after)
        .any(|(a, b)| a.iter().zip(b).any(|(a, b)| (*a - b).abs() > tolerance))
}

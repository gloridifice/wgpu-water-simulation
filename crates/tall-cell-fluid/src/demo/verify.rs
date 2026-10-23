//! 无窗口演示验证：真实 GPU 推进、表面变化以及后台线程暂停/单步/重置协议。

use std::thread;
use std::time::{Duration, Instant};

use super::worker::{Command, DemoConfig, Frame, Scene, Worker, WorkerEvent};
use crate::surface::SurfaceMesh;

const FRAME_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// 每个场景执行 `steps` 次真实 GPU 单步，再验证播放、暂停确认及重置。
///
/// 至少两步：静止初态第一步主要建立速度，界面可能到第二步才产生可测位移。
pub fn verify(mut config: DemoConfig, steps: usize) -> Result<(), String> {
    if steps < 2 {
        return Err("演示验证的 --steps 至少为 2，才能检查速度建立后的水面运动".into());
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
        (Err(error), Err(shutdown)) => Err(format!("{error}；后台线程清理失败：{shutdown}")),
    }
}

fn verify_worker(worker: &Worker, first_scene: Scene, steps: usize) -> Result<(), String> {
    let mut frame = wait_frame(worker, "初始化", |frame| {
        if frame.generation != 0 || frame.steps != 0 || frame.time != 0.0 || !frame.paused {
            return Err("启动暂停状态未得到零时刻快照".into());
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
            let report = frame.report.as_ref().ok_or("单步完成但缺少求解报告")?;
            let volume = *initial_volume.get_or_insert(report.volume_before);
            // 这是演示场景的质量漂移回归界限，不将 level-set 修正称为严格守恒。
            if (report.volume_after - volume).abs() > 0.15 * volume {
                return Err(format!(
                    "{scene:?} 场景体积估计漂移超过 15%：{volume} -> {} m³",
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
                frame.simulation_ms.ok_or("单步完成但缺少模拟耗时")?,
                frame.mesh_ms,
            );
        }
        if !surface_moved(&start_positions, &frame.mesh, frame.extent) {
            return Err(format!(
                "{scene:?} 已推进 {steps} 步，但提取的表面没有可测位置变化"
            ));
        }
        frame = confirm_paused(worker, &frame)?;
        ensure_quiet(worker, &frame)?;
        println!("PASS demo {scene:?}: surface moved; paused snapshot stayed fixed");

        if scene_index == 0 {
            let generation = frame.generation.checked_add(1).ok_or("代次溢出")?;
            let old_positions = sorted_positions(&frame.mesh);
            worker.send(Command::Reset(second_scene));
            frame = wait_frame(worker, "场景重置", |next| {
                if next.generation > generation {
                    return Err("一次重置意外跳过多个代次".into());
                }
                Ok(next.generation == generation)
            })?;
            check_reset_frame(&frame)?;
            check_surface_bounds(&frame)?;
            if !surface_moved(&old_positions, &frame.mesh, frame.extent) {
                return Err("重置到另一场景后表面仍与旧场景相同".into());
            }
            println!(
                "PASS demo reset: generation={generation}, time=0, steps=0, paused=true, simulation=not run, mesh={:.3}ms",
                frame.mesh_ms
            );
        }
    }

    // 播放确认与计算结果可以分别到达；随后允许当前在途子步完成，再等待暂停确认。
    let generation = frame.generation;
    let previous_steps = frame.steps;
    worker.send(Command::SetPaused(false));
    frame = wait_frame(worker, "恢复播放", |next| {
        if next.generation != generation {
            return Err("恢复播放意外切换了场景代次".into());
        }
        Ok(!next.paused && next.steps > previous_steps)
    })?;
    check_step_report(&frame)?;
    check_surface_bounds(&frame)?;
    let playing_steps = frame.steps;
    worker.send(Command::SetPaused(true));
    frame = wait_frame(worker, "播放后暂停", |next| {
        if next.generation != generation || next.steps < playing_steps {
            return Err("暂停确认退回旧场景或旧时间步".into());
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
    let mut last = String::from("尚未收到快照");
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
                WorkerEvent::Error(error) => return Err(format!("{stage}：{error}")),
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
    Err(format!("等待{stage}超过 60 秒（{last}）"))
}

fn single_step(worker: &Worker, previous: &Frame) -> Result<Frame, String> {
    if !previous.paused {
        return Err("验证要求在暂停确认后发出单步命令".into());
    }
    let expected = previous.steps.checked_add(1).ok_or("步数溢出")?;
    worker.send(Command::Step);
    let next = wait_frame(worker, "暂停单步", |next| {
        if next.generation != previous.generation {
            return Err("单步意外改变场景代次".into());
        }
        if next.steps > expected || next.steps < previous.steps {
            return Err(format!(
                "单步期望 steps={expected}，实际 steps={}",
                next.steps
            ));
        }
        Ok(next.steps == expected)
    })?;
    if !next.paused || next.time <= previous.time {
        return Err("单步完成后未保持暂停，或模拟时间没有增加".into());
    }
    check_step_report(&next)?;
    let dt = next.report.as_ref().ok_or("缺少单步报告")?.dt;
    if ((next.time - previous.time) - dt).abs() > 1e-9 * (1.0 + next.time.abs()) {
        return Err("快照时间增量与求解器 dt 不一致".into());
    }
    Ok(next)
}

fn confirm_paused(worker: &Worker, previous: &Frame) -> Result<Frame, String> {
    worker.send(Command::SetPaused(true));
    wait_frame(worker, "暂停确认", |next| {
        if next.generation != previous.generation
            || next.steps != previous.steps
            || next.time != previous.time
        {
            return Err("已经暂停的模拟在确认暂停时继续前进".into());
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
                        return Err("暂停确认后出现未经单步/重置命令的状态推进".into());
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
        return Err("初始化/重置没有清除模拟时间、步数、模拟耗时或旧求解报告".into());
    }
    Ok(())
}

fn check_step_report(frame: &Frame) -> Result<(), String> {
    if frame.simulation_ms.is_none() {
        return Err("实际模拟子步缺少独立模拟耗时".into());
    }
    let report = frame.report.as_ref().ok_or("GPU 推进快照缺少求解报告")?;
    let limit = frame
        .stable_dt
        .ok_or("GPU 推进快照缺少推进前的 CFL 时间步上限")?;
    if !limit.is_finite()
        || limit <= 0.0
        || !report.dt.is_finite()
        || report.dt <= 0.0
        || report.dt > limit * (1.0 + 1e-6)
    {
        return Err(format!(
            "时间步违反 CFL 上限：dt={}, limit={limit}",
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
        return Err("GPU 求解报告包含非法状态或压力投影未收敛".into());
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
        return Err("演示快照时间、范围或适配器信息非法".into());
    }
    let mesh = &frame.mesh;
    if mesh.vertices.is_empty() || mesh.indices.is_empty() || !mesh.indices.len().is_multiple_of(3)
    {
        return Err("演示水体没有有效三角形表面".into());
    }
    for vertex in &mesh.vertices {
        if vertex
            .position
            .iter()
            .chain(&vertex.normal)
            .any(|v| !v.is_finite())
        {
            return Err("水面顶点或法线出现非有限数".into());
        }
        let normal_squared: f32 = vertex.normal.iter().map(|v| v * v).sum();
        if !(0.9..=1.1).contains(&normal_squared) {
            return Err("水面法线没有有效归一化".into());
        }
    }
    if mesh
        .indices
        .iter()
        .any(|&index| index as usize >= mesh.vertices.len())
    {
        return Err("水面三角形索引越界".into());
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
        return Err("模拟/网格生成分段耗时不是有限非负数".into());
    }
    let sum = frame.simulation_ms.unwrap_or(0.0) + frame.mesh_ms;
    let tolerance = 8.0 * f64::EPSILON * sum.max(1.0);
    if !sum.is_finite()
        || !frame.elapsed_ms.is_finite()
        || (frame.elapsed_ms - sum).abs() > tolerance
    {
        return Err(format!(
            "后台总耗时应等于模拟与网格生成之和（初始化/等待不计入）：total={}, simulation={:?}, mesh={} ms",
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
        return Err("暂停确认重写了最近一次完成工作的耗时".into());
    }
    Ok(())
}

fn check_surface_bounds(frame: &Frame) -> Result<(), String> {
    for vertex in &frame.mesh.vertices {
        for (axis, &position) in vertex.position.iter().enumerate() {
            // 演示提取前以物理计算域的盒子距离场裁切，包含墙角的斜边也应留在域内。
            let tolerance = frame.extent[axis] * 1e-4;
            if position < -tolerance || position > frame.extent[axis] + tolerance {
                return Err("水面顶点超出封闭演示计算域".into());
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
    // GPU 原子分配可改变三角形输出顺序，不能直接比较原始顶点数组。
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

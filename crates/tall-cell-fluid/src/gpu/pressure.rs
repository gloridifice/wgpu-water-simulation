//! GPU 压力投影。CPU 只组装拓扑/CSR 和读取收敛标量，不计算 RHS 或迭代向量。

use super::GpuContext;
use crate::adaptivity::AdaptiveGrid;
use crate::grid::Grid;
use crate::pressure::{PressureConfig, ProjectionReport, gpu_structure};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    grid: [u32; 4],
    counts: [u32; 4],
    meta0: [u32; 4],
    meta1: [u32; 4],
    values0: [u32; 4],
    values1: [u32; 4],
    scales: [f32; 4],
}

struct ResultData {
    pressure: Vec<f32>,
    velocity: Vec<f32>,
    iterations: usize,
    residual: f64,
    algebraic_converged: bool,
    before: f64,
    after: f64,
}

/// 实际执行 WGSL RHS、CSR 乘法、Jacobi-PCG、点积、压力重建及 MAC 速度修正。
///
/// 回退仍使用 GPU 完整细基求解，绝不调用 CPU `pressure::project`。
pub fn project(
    context: &GpuContext,
    grid: &mut Grid,
    adaptive: &AdaptiveGrid,
    dt: f64,
    density: f64,
    config: &PressureConfig,
) -> Result<ProjectionReport, String> {
    config.validate()?;
    context.validate_grid(grid)?;
    if !dt.is_finite()
        || dt <= 0.0
        || !density.is_finite()
        || density <= 0.0
        || !((dt / density) as f32).is_normal()
        || !((density / dt) as f32).is_normal()
    {
        return Err("GPU 投影时间步和密度必须为有限正数，且比例可由 f32 表示".into());
    }
    let mut output = solve(context, grid, adaptive, dt, density, config)?;
    let mut report = ProjectionReport {
        converged: output.algebraic_converged && output.after <= config.divergence_tolerance,
        iterations: output.iterations,
        adaptive_iterations: output.iterations,
        pressure_dofs: adaptive.dof_count,
        adaptive_pressure_dofs: adaptive.dof_count,
        fallback_used: false,
        divergence_before: output.before,
        divergence_after: output.after,
        adaptive_divergence: output.after,
        residual_norm: output.residual,
        adaptive_residual_norm: output.residual,
    };
    if !report.converged && config.fine_fallback {
        // 尚未提交第一次求解的速度，因而回退天然从同一个临时状态重新开始。
        let uniform = AdaptiveGrid::uniform(grid);
        output = solve(context, grid, &uniform, dt, density, config)?;
        report.iterations += output.iterations;
        report.pressure_dofs = uniform.dof_count;
        report.fallback_used = true;
        report.residual_norm = output.residual;
        report.divergence_after = output.after;
        report.converged =
            output.algebraic_converged && output.after <= config.divergence_tolerance;
    }
    let mut offset = 0;
    for axis in 0..3 {
        for value in &mut grid.velocity.faces[axis] {
            *value = f64::from(output.velocity[offset]);
            offset += 1;
        }
    }
    for (p, value) in grid.pressure.iter_mut().zip(output.pressure) {
        *p = f64::from(value);
    }
    grid.validate()?;
    Ok(report)
}

fn append<T: Copy>(destination: &mut Vec<T>, values: &[T]) -> Result<u32, String> {
    let offset = u32::try_from(destination.len()).map_err(|_| "GPU 压力缓冲区下标溢出")?;
    destination.extend_from_slice(values);
    Ok(offset)
}

fn solve(
    context: &GpuContext,
    grid: &Grid,
    adaptive: &AdaptiveGrid,
    dt: f64,
    density: f64,
    config: &PressureConfig,
) -> Result<ResultData, String> {
    let topology = gpu_structure(grid, adaptive, config.minimum_interface_fraction)?;
    let count = grid.domain.cell_count();
    let dofs = adaptive.dof_count;
    let mut metadata = Vec::new();
    let mut values = Vec::new();
    let flags: Vec<u32> = (0..count)
        .map(|cell| {
            if grid.solid_phi[cell] < 0.0 {
                2
            } else if grid.phi[cell] < 0.0 {
                1
            } else {
                0
            }
        })
        .collect();
    let fluid_count = flags.iter().filter(|&&flag| flag == 1).count();
    let basis_indices: Vec<u32> = adaptive
        .basis
        .iter()
        .flat_map(|basis| basis.indices)
        .map(|i| u32::try_from(i).map_err(|_| "GPU 压力基函数下标溢出".to_owned()))
        .collect::<Result<_, _>>()?;
    let basis_weights: Vec<f32> = adaptive
        .basis
        .iter()
        .flat_map(|basis| basis.weights)
        .map(|w| w as f32)
        .collect();
    let mut transpose = vec![Vec::new(); dofs];
    for (cell, basis) in adaptive.basis.iter().enumerate() {
        for (&index, &weight) in basis.indices.iter().zip(&basis.weights) {
            if weight != 0.0 {
                transpose[index].push((cell as u32, weight as f32));
            }
        }
    }
    let mut transpose_rows = vec![0u32];
    let mut transpose_cells = Vec::new();
    let mut transpose_weights = Vec::new();
    for entries in transpose {
        for (cell, weight) in entries {
            transpose_cells.push(cell);
            transpose_weights.push(weight);
        }
        transpose_rows.push(transpose_cells.len() as u32);
    }
    let velocity_data: Vec<f32> = grid
        .velocity
        .faces
        .iter()
        .flatten()
        .map(|&v| v as f32)
        .collect();
    let mut params = Params {
        grid: [
            grid.domain.size[0] as u32,
            grid.domain.size[1] as u32,
            grid.domain.size[2] as u32,
            count as u32,
        ],
        counts: [
            dofs as u32,
            velocity_data.len() as u32,
            fluid_count as u32,
            0,
        ],
        meta0: [
            append(&mut metadata, &flags)?,
            append(&mut metadata, &basis_indices)?,
            append(&mut metadata, &transpose_rows)?,
            append(&mut metadata, &transpose_cells)?,
        ],
        meta1: [
            append(&mut metadata, &topology.rows)?,
            append(&mut metadata, &topology.columns)?,
            append(&mut metadata, &topology.pinned)?,
            0,
        ],
        values0: [
            append(&mut values, &topology.face_inverse_distance)?,
            append(&mut values, &basis_weights)?,
            append(&mut values, &transpose_weights)?,
            append(&mut values, &topology.values)?,
        ],
        values1: [
            append(&mut values, &topology.diagonal)?,
            grid.velocity.faces[0].len() as u32,
            grid.velocity.faces[1].len() as u32,
            0,
        ],
        scales: [
            grid.domain.cell_size as f32,
            (dt / density) as f32,
            (density / dt) as f32,
            0.0,
        ],
    };
    let work_size = (6 * dofs).max(1);
    for length in [
        metadata.len(),
        values.len(),
        work_size,
        velocity_data.len(),
        count,
    ] {
        if length as u64 * 4 > context.device.limits().max_storage_buffer_binding_size
            || length > u32::MAX as usize
        {
            return Err("GPU 压力工作缓冲区超过设备上限".into());
        }
    }
    let metadata_buffer = context.buffer("pressure topology", &metadata);
    let coefficients = context.buffer("pressure coefficients", &values);
    let velocity = context.buffer("pressure MAC velocity", &velocity_data);
    let pressure = context.buffer("pressure fine reconstruction", &vec![0.0f32; count]);
    let vectors = context.buffer("pressure PCG vectors", &vec![0.0f32; work_size]);
    let stats = context.buffer("pressure GPU reductions", &[0.0f32; 8]);
    let uniform = context.uniform("pressure parameters", &params);
    let pipeline = context.pipeline(
        "GPU pressure projection",
        include_str!("shaders/pressure.wgsl"),
        "pressure_main",
    )?;
    let bindings = context.bind(
        &pipeline,
        &[
            &uniform,
            &metadata_buffer,
            &coefficients,
            &velocity,
            &pressure,
            &vectors,
            &stats,
        ],
    );
    let dispatch = |params: &mut Params, mode: u32, length: usize| {
        params.counts[3] = mode;
        context
            .queue
            .write_buffer(&uniform, 0, bytemuck::bytes_of(params));
        context.dispatch(&pipeline, &bindings, length.max(1).div_ceil(256) as u32);
    };
    dispatch(&mut params, 0, velocity_data.len());
    dispatch(&mut params, 10, 256);
    let before = f64::from(context.read::<f32>(&stats, 8)?[5]);
    dispatch(&mut params, 1, dofs);
    dispatch(&mut params, 4, 256);
    let mut statistics = context.read::<f32>(&stats, 8)?;
    let rhs_norm = f64::from(statistics[3]).sqrt();
    // 固定的 Neumann 参考行可累积其余行残差，使用保守的绝对散度约束。
    let tolerance = config
        .absolute_tolerance
        .max(config.relative_tolerance * rhs_norm)
        .min(config.divergence_tolerance * density / dt * 0.25);
    let mut iterations = 0;
    while f64::from(statistics[0]).sqrt() > tolerance && iterations < config.max_iterations {
        dispatch(&mut params, 2, dofs);
        dispatch(&mut params, 4, 256);
        statistics = context.read::<f32>(&stats, 8)?;
        if !statistics[2].is_finite() || statistics[2] <= 0.0 || !statistics[1].is_finite() {
            break;
        }
        dispatch(&mut params, 3, dofs);
        dispatch(&mut params, 4, 256);
        statistics = context.read::<f32>(&stats, 8)?;
        iterations += 1;
        if iterations.is_multiple_of(50) || f64::from(statistics[0]).sqrt() <= tolerance {
            dispatch(&mut params, 6, dofs);
            dispatch(&mut params, 7, dofs);
            dispatch(&mut params, 4, 256);
            statistics = context.read::<f32>(&stats, 8)?;
        } else {
            if statistics[4] == 0.0 || !statistics[1].is_finite() {
                break;
            }
            dispatch(&mut params, 5, dofs);
        }
    }
    // 一律使用重新计算的真实残差，不能仅相信 PCG 递推残差。
    dispatch(&mut params, 6, dofs);
    dispatch(&mut params, 7, dofs);
    dispatch(&mut params, 4, 256);
    statistics = context.read::<f32>(&stats, 8)?;
    let residual = f64::from(statistics[0]).sqrt();
    dispatch(&mut params, 8, count);
    dispatch(&mut params, 9, velocity_data.len());
    dispatch(&mut params, 10, 256);
    let after = f64::from(context.read::<f32>(&stats, 8)?[5]);
    let pressure = context.read::<f32>(&pressure, count)?;
    let velocity = context.read::<f32>(&velocity, velocity_data.len())?;
    if !before.is_finite()
        || !after.is_finite()
        || !residual.is_finite()
        || pressure.iter().chain(&velocity).any(|v| !v.is_finite())
    {
        return Err("GPU 压力求解产生非有限数".into());
    }
    Ok(ResultData {
        pressure,
        velocity,
        iterations,
        residual,
        algebraic_converged: residual <= tolerance,
        before,
        after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Domain;
    use crate::adaptivity::PressureBasis;

    #[test]
    #[ignore = "需要实际 GPU；通过 --ignored 显式执行"]
    fn gpu_pressure_closed_free_surface_reduced_and_fallback() {
        let context = pollster::block_on(GpuContext::new()).expect("GPU adapter");
        let config = PressureConfig {
            relative_tolerance: 2e-5,
            absolute_tolerance: 1e-5,
            divergence_tolerance: 5e-4,
            ..PressureConfig::default()
        };
        for obstacle in [false, true] {
            let mut grid = Grid::new(Domain::new([7, 5, 4], 0.2).unwrap());
            grid.phi.fill(-1.0);
            if obstacle {
                grid.set_solid_sdf(|p| (p[0] - 0.7).abs() - 0.09);
            }
            for axis in 0..3 {
                for (i, value) in grid.velocity.faces[axis].iter_mut().enumerate() {
                    *value = ((i * 17 + axis * 13) as f64 * 0.23).sin();
                }
            }
            let mut reduced = AdaptiveGrid::uniform(&grid);
            reduced.dof_count = 1;
            for basis in &mut reduced.basis {
                if basis.weights.iter().sum::<f64>() > 0.0 {
                    *basis = PressureBasis {
                        indices: [0, 0],
                        weights: [1.0, 0.0],
                    };
                }
            }
            let report = project(&context, &mut grid, &reduced, 0.02, 1000.0, &config).unwrap();
            assert!(report.converged, "{report:?}");
            assert!(report.fallback_used, "{report:?}");
            assert!(grid.divergence_rms() < config.divergence_tolerance);
        }
        let mut lake = Grid::new(Domain::new([4, 7, 3], 0.2).unwrap());
        lake.set_liquid_sdf(|p| p[1] - 0.93);
        lake.velocity.faces[1].fill(-9.81 * 0.015);
        let uniform = AdaptiveGrid::uniform(&lake);
        let report = project(&context, &mut lake, &uniform, 0.015, 1000.0, &config).unwrap();
        assert!(report.converged, "{report:?}");
        assert!(lake.divergence_rms() < config.divergence_tolerance);
        let mut tall_lake = Grid::new(Domain::new([4, 7, 3], 0.2).unwrap());
        tall_lake.set_liquid_sdf(|p| p[1] - 0.93);
        tall_lake.velocity.faces[1].fill(-9.81 * 0.015);
        let mut tall = AdaptiveGrid::uniform(&tall_lake);
        tall.dof_count = 24;
        for (cell, basis) in tall.basis.iter_mut().enumerate() {
            let p = tall_lake.domain.coord(cell);
            if tall_lake.is_fluid(p) {
                let column = p[0] + 4 * p[2];
                let t = p[1] as f64 / 4.0;
                *basis = PressureBasis {
                    indices: [2 * column, 2 * column + 1],
                    weights: [1.0 - t, t],
                };
            }
        }
        let report = project(&context, &mut tall_lake, &tall, 0.015, 1000.0, &config).unwrap();
        assert!(report.converged && !report.fallback_used, "{report:?}");
        assert_eq!(report.pressure_dofs, 24);
        assert!(tall_lake.divergence_rms() < config.divergence_tolerance);
        let mut empty = Grid::new(Domain::new([3; 3], 0.2).unwrap());
        let uniform = AdaptiveGrid::uniform(&empty);
        let report = project(&context, &mut empty, &uniform, 0.01, 1000.0, &config).unwrap();
        assert!(report.converged && report.pressure_dofs == 0);
    }
}

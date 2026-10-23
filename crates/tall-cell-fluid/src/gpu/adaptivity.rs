//! GPU 上执行速度评价、沿柱积分、光学层 L1 膨胀和高度平滑。
//! 阶段间允许同步回读以便离线验证；四叉树和稀疏压力基底由 CPU 整理。

use bytemuck::{Pod, Zeroable};

use super::GpuContext;
use crate::adaptivity::{AdaptiveGrid, AdaptivityConfig};
use crate::grid::Grid;

const SHADER: &str = include_str!("adaptivity.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    dimensions: [u32; 4],
    offsets: [u32; 4],
    controls: [u32; 4],
    coefficients: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Metric {
    local_error: f32,
    run_error: f32,
    first: u32,
    limit: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Column {
    integrated_error: f32,
    first: u32,
    limit: u32,
    single_grounded: u32,
}

/// 构建自适应压力网格；数值评价和光学层均使用实际 WGSL 计算结果。
///
/// GPU 使用 f32，CPU 参考使用 f64；精细化阈值恰好贴近舍入误差时，拓扑可能不同。
pub fn build_adaptive(
    context: &GpuContext,
    grid: &Grid,
    config: &AdaptivityConfig,
) -> Result<AdaptiveGrid, String> {
    context.validate_grid(grid)?;
    config.validate()?;
    let optical_scale = config.optical_scale as f32;
    if !optical_scale.is_finite() {
        return Err("光学层比例无法由 GPU f32 表示".into());
    }
    let [nx, ny, nz] = grid.domain.size;
    let count = grid.domain.cell_count();
    let column_count = nx * nz;
    let airborne_layers = config.airborne_layers.min(nx + ny + nz);
    let params = Params {
        dimensions: [nx as u32, ny as u32, nz as u32, count as u32],
        offsets: [
            0,
            grid.velocity.faces[0].len() as u32,
            (grid.velocity.faces[0].len() + grid.velocity.faces[1].len()) as u32,
            column_count as u32,
        ],
        controls: [airborne_layers as u32, 0, 0, 0],
        coefficients: [optical_scale, 0.0, 0.0, 0.0],
    };
    let params_buffer = context.uniform("optical layer parameters", &params);
    let interface: Vec<[f32; 2]> = grid
        .phi
        .iter()
        .zip(&grid.solid_phi)
        .map(|(&phi, &solid)| [phi as f32, solid as f32])
        .collect();
    let interface_buffer = context.buffer("optical layer liquid/solid", &interface);
    let velocity: Vec<f32> = grid
        .velocity
        .faces
        .iter()
        .flatten()
        .map(|v| *v as f32)
        .collect();
    let velocity_buffer = context.buffer("optical layer MAC velocities", &velocity);
    let metric_buffer = context.buffer("optical layer metrics", &vec![Metric::zeroed(); count]);
    let column_buffer = context.buffer(
        "optical layer columns",
        &vec![Column::zeroed(); column_count],
    );
    let evaluate = context.pipeline("optical layer evaluation", SHADER, "evaluate_metrics")?;
    let evaluate_bindings = context.bind_at(
        &evaluate,
        &[
            (0, &params_buffer),
            (1, &interface_buffer),
            (2, &velocity_buffer),
            (3, &metric_buffer),
            (4, &column_buffer),
        ],
    );
    let column_groups = (column_count as u32).div_ceil(64);
    let cell_groups = (count as u32).div_ceil(64);
    context.dispatch(&evaluate, &evaluate_bindings, column_groups);

    // 回读只用于四叉树决策；没有 CPU 重算 q 或 I 的备用路径。
    let metrics = context.read::<Metric>(&metric_buffer, count)?;
    let columns = context.read::<Column>(&column_buffer, column_count)?;
    if metrics
        .iter()
        .any(|m| !m.local_error.is_finite() || !m.run_error.is_finite())
        || columns.iter().any(|c| !c.integrated_error.is_finite())
    {
        return Err("GPU 水柱评价出现非有限数".into());
    }
    let local_cost: Vec<f64> = metrics.iter().map(|m| f64::from(m.local_error)).collect();
    let integrated_cost: Vec<f64> = columns
        .iter()
        .map(|c| f64::from(c.integrated_error))
        .collect();
    let leaves = AdaptiveGrid::leaves_from_cost(grid, config, &local_cost);
    let mut widths = vec![1u32; column_count];
    for leaf in &leaves {
        for z in leaf.min[1]..leaf.max[1] {
            for x in leaf.min[0]..leaf.max[0] {
                widths[x + nx * z] = leaf.width() as u32;
            }
        }
    }
    let maximum_width = widths.iter().copied().max().unwrap_or(1) as usize;
    let maximum_radius = (ny / 8)
        .max((2 * maximum_width).max(4).min(ny))
        .max(airborne_layers)
        .min(nx + ny + nz - 3);
    let width_buffer = context.buffer("optical layer leaf widths", &widths);
    let mut mask_a = context.buffer("optical layer radius A", &vec![-1i32; count]);
    let mut mask_b = context.buffer("optical layer radius B", &vec![-1i32; count]);
    let seed = context.pipeline("optical layer seeds", SHADER, "seed_optical_layer")?;
    let seed_bindings = context.bind_at(
        &seed,
        &[
            (0, &params_buffer),
            (1, &interface_buffer),
            (3, &metric_buffer),
            (5, &width_buffer),
            (7, &mask_a),
        ],
    );
    context.dispatch(&seed, &seed_bindings, cell_groups);

    let dilate = context.pipeline("optical layer L1 dilation", SHADER, "dilate_optical_layer")?;
    for _ in 0..maximum_radius {
        let bindings = context.bind_at(&dilate, &[(0, &params_buffer), (6, &mask_a), (7, &mask_b)]);
        context.dispatch(&dilate, &bindings, cell_groups);
        std::mem::swap(&mut mask_a, &mut mask_b);
    }

    let mut height_a = context.buffer("optical layer heights A", &vec![-1i32; column_count]);
    let mut height_b = context.buffer("optical layer heights B", &vec![-1i32; column_count]);
    let initialize =
        context.pipeline("optical layer height field", SHADER, "initialize_heights")?;
    let initialize_bindings = context.bind_at(
        &initialize,
        &[
            (0, &params_buffer),
            (4, &column_buffer),
            (6, &mask_a),
            (7, &height_a),
        ],
    );
    context.dispatch(&initialize, &initialize_bindings, column_groups);

    let smooth = context.pipeline("optical layer height smoothing", SHADER, "smooth_heights")?;
    for _ in 0..config.smoothing_passes {
        let bindings = context.bind_at(
            &smooth,
            &[
                (0, &params_buffer),
                (4, &column_buffer),
                (6, &height_a),
                (7, &height_b),
            ],
        );
        context.dispatch(&smooth, &bindings, column_groups);
        std::mem::swap(&mut height_a, &mut height_b);
    }

    let finalize =
        context.pipeline("optical layer final mask", SHADER, "finalize_optical_layer")?;
    let finalize_bindings = context.bind_at(
        &finalize,
        &[
            (0, &params_buffer),
            (4, &column_buffer),
            (6, &height_a),
            (7, &mask_a),
        ],
    );
    context.dispatch(&finalize, &finalize_bindings, cell_groups);
    let optical_mask = context
        .read::<i32>(&mask_a, count)?
        .into_iter()
        .map(|value| value >= 0)
        .collect();
    Ok(AdaptiveGrid::from_metrics(
        grid,
        local_cost,
        integrated_cost,
        optical_mask,
        leaves,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Domain;

    #[test]
    #[ignore = "需要真实 wgpu 计算适配器；使用 --ignored 显式运行"]
    fn gpu_column_error_matches_hand_calculation_and_empty_domain_is_valid() {
        let context = pollster::block_on(GpuContext::new()).unwrap();
        let mut grid = Grid::new(Domain::new([2, 8, 2], 1.0).unwrap());
        grid.set_liquid_sdf(|p| p[1] - 3.0);
        let dims = grid.velocity.dimensions(0);
        for z in 0..dims[2] {
            for x in 0..dims[0] {
                grid.velocity.set(0, [x, 1, z], 3.0);
            }
        }
        let config = AdaptivityConfig::default();
        let result = build_adaptive(&context, &grid, &config).unwrap();
        // [0,3,0] 线性最小二乘重建为 [1,1,1]；左右面各贡献一次。
        for (y, expected) in [2.0, 4.0, 2.0].into_iter().enumerate() {
            assert!((result.local_cost[grid.domain.index([0, y, 0])] - expected).abs() < 1e-6);
        }
        assert!((result.integrated_cost[0] - 1.0).abs() < 1e-6);
        let cpu = AdaptiveGrid::build(&grid, &config);
        assert_eq!(result.optical_mask, cpu.optical_mask);

        let too_large = AdaptivityConfig {
            optical_scale: f64::MAX,
            ..config.clone()
        };
        assert!(build_adaptive(&context, &grid, &too_large).is_err());
        let saturated = AdaptivityConfig {
            optical_scale: f64::from(f32::MAX),
            ..config.clone()
        };
        let saturated_result = build_adaptive(&context, &grid, &saturated).unwrap();
        assert_eq!(saturated_result.optical_mask, cpu.optical_mask);

        grid.set_liquid_sdf(|_| 1.0);
        let empty = build_adaptive(&context, &grid, &config).unwrap();
        assert_eq!(empty.dof_count, 0);
        assert!(empty.optical_mask.iter().all(|value| !value));
        assert!(empty.local_cost.iter().all(|value| *value == 0.0));
    }
}

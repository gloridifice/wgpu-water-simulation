//! 在 MAC 背景网格上构造一致的压力投影，再用线性基函数压缩压力自由度。
//!
//! `B` 将自适应压力插值到细单元，真正求解 `Bᵀ L B p = Bᵀ b`。
//! 空气界面使用 ghost-fluid 零压条件，静态固体采用体素无穿透边界。
//! 缩减系统收敛只约束投影后的散度；细网格散度仍需独立检查。

use crate::adaptivity::{AdaptiveGrid, PressureBasis};
use crate::grid::Grid;

#[derive(Clone, Debug)]
pub struct PressureConfig {
    pub max_iterations: usize,
    /// 代数残差相对初始右端项的容限。
    pub relative_tolerance: f64,
    /// 代数残差二范数的绝对容限。
    pub absolute_tolerance: f64,
    /// 全细网格液体单元散度的 RMS 容限，单位 1/s。
    pub divergence_tolerance: f64,
    /// 缩减基函数无法解析当前流动时，使用完整细网格重做压力投影。
    pub fine_fallback: bool,
    /// 水面到液体中心的距离下限，以格距比例计。
    pub minimum_interface_fraction: f64,
}

impl Default for PressureConfig {
    fn default() -> Self {
        Self {
            max_iterations: 2000,
            relative_tolerance: 1e-9,
            absolute_tolerance: 1e-10,
            divergence_tolerance: 1e-5,
            fine_fallback: true,
            minimum_interface_fraction: 0.01,
        }
    }
}

impl PressureConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_iterations == 0 {
            return Err("压力迭代次数必须为正".into());
        }
        for (name, value) in [
            ("相对残差容限", self.relative_tolerance),
            ("绝对残差容限", self.absolute_tolerance),
            ("散度容限", self.divergence_tolerance),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name}必须为有限正数"));
            }
        }
        if !self.minimum_interface_fraction.is_finite()
            || self.minimum_interface_fraction <= 0.0
            || self.minimum_interface_fraction > 1.0
        {
            return Err("水面距离比例下限必须在 (0, 1] 内".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProjectionReport {
    /// 代数求解收敛且最终细网格散度满足要求。
    pub converged: bool,
    /// 包括缩减求解和可能的完整细网格重试。
    pub iterations: usize,
    pub adaptive_iterations: usize,
    /// 最终采用的压力自由度数；启用回退后可能等于液体细单元数。
    pub pressure_dofs: usize,
    pub adaptive_pressure_dofs: usize,
    pub fallback_used: bool,
    /// 各散度均为细网格液体单元的 RMS，单位 1/s。
    pub divergence_before: f64,
    pub divergence_after: f64,
    pub adaptive_divergence: f64,
    /// 最终系统的真实代数残差二范数，而非散度。
    pub residual_norm: f64,
    pub adaptive_residual_norm: f64,
}

/// 更新 `grid.pressure` 和面速度，不输送液体界面。
///
/// 完整细网格回退从同一个临时速度重新开始，因此报告中的最终压力与最终速度一致。
pub fn project(
    grid: &mut Grid,
    adaptive: &AdaptiveGrid,
    dt: f64,
    density: f64,
    config: &PressureConfig,
) -> Result<ProjectionReport, String> {
    config.validate()?;
    grid.validate()?;
    if !dt.is_finite() || dt <= 0.0 || !density.is_finite() || density <= 0.0 {
        return Err("压力投影的时间步和密度必须为有限正数".into());
    }
    if !(density / dt).is_finite() || !(dt / density).is_finite() {
        return Err("压力投影的密度与时间步比例溢出".into());
    }
    validate_basis(grid, adaptive)?;
    grid.enforce_boundaries();
    let divergence_before = grid.divergence_rms();
    let edges = fluid_edges(grid, config.minimum_interface_fraction);
    let saved_velocity = config.fine_fallback.then(|| grid.velocity.clone());
    let first = solve_and_correct(grid, adaptive, &edges, dt, density, config);
    let adaptive_divergence = grid.divergence_rms();
    let mut report = ProjectionReport {
        converged: first.converged && adaptive_divergence <= config.divergence_tolerance,
        iterations: first.iterations,
        adaptive_iterations: first.iterations,
        pressure_dofs: adaptive.dof_count,
        adaptive_pressure_dofs: adaptive.dof_count,
        fallback_used: false,
        divergence_before,
        divergence_after: adaptive_divergence,
        adaptive_divergence,
        residual_norm: first.residual,
        adaptive_residual_norm: first.residual,
    };
    if !report.converged
        && let Some(velocity) = saved_velocity
    {
        grid.velocity = velocity;
        let fine = AdaptiveGrid::uniform(grid);
        let second = solve_and_correct(grid, &fine, &edges, dt, density, config);
        report.fallback_used = true;
        report.iterations += second.iterations;
        report.pressure_dofs = fine.dof_count;
        report.divergence_after = grid.divergence_rms();
        report.residual_norm = second.residual;
        report.converged =
            second.converged && report.divergence_after <= config.divergence_tolerance;
    }
    grid.validate()?;
    Ok(report)
}

fn validate_basis(grid: &Grid, adaptive: &AdaptiveGrid) -> Result<(), String> {
    if adaptive.basis.len() != grid.domain.cell_count()
        || adaptive.dof_count > 2 * grid.domain.cell_count()
    {
        return Err("自适应压力基函数尺寸不匹配".into());
    }
    for (cell, basis) in adaptive.basis.iter().enumerate() {
        let fluid = grid.is_fluid(grid.domain.coord(cell));
        let sum: f64 = basis.weights.iter().sum();
        if basis.weights.iter().any(|w| !w.is_finite() || *w < 0.0)
            || (sum - if fluid { 1.0 } else { 0.0 }).abs() > 1e-10
        {
            return Err("压力基函数必须有限非负，且在液体单元处保持常量".into());
        }
        for (&index, &weight) in basis.indices.iter().zip(&basis.weights) {
            if weight != 0.0 && index >= adaptive.dof_count {
                return Err("压力基函数引用了越界自由度".into());
            }
        }
    }
    Ok(())
}

/// 一个开放 MAC 面；空气侧的压力为零。没有固体面或计算域外侧面。
struct FluidEdge {
    axis: usize,
    face: [usize; 3],
    left: Option<usize>,
    right: Option<usize>,
    inverse_distance: f64,
    coefficient: f64,
}

fn fluid_edges(grid: &Grid, minimum_fraction: f64) -> Vec<FluidEdge> {
    let mut edges = Vec::new();
    let dx = grid.domain.cell_size;
    for axis in 0..3 {
        for n in 0..grid.domain.cell_count() {
            let right_coord = grid.domain.coord(n);
            if right_coord[axis] == 0 {
                continue;
            }
            let mut left_coord = right_coord;
            left_coord[axis] -= 1;
            let left_index = grid.domain.index(left_coord);
            if grid.solid_phi[left_index] < 0.0 || grid.solid_phi[n] < 0.0 {
                continue;
            }
            let left = grid.is_fluid(left_coord).then_some(left_index);
            let right = grid.is_fluid(right_coord).then_some(n);
            let theta = match (left, right) {
                (None, None) => continue,
                (Some(_), Some(_)) => 1.0,
                (Some(liquid), None) => (grid.phi[liquid] / (grid.phi[liquid] - grid.phi[n]))
                    .clamp(minimum_fraction, 1.0),
                (None, Some(liquid)) => (grid.phi[liquid]
                    / (grid.phi[liquid] - grid.phi[left_index]))
                    .clamp(minimum_fraction, 1.0),
            };
            let inverse_distance = 1.0 / (theta * dx);
            edges.push(FluidEdge {
                axis,
                face: right_coord,
                left,
                right,
                inverse_distance,
                coefficient: inverse_distance / dx,
            });
        }
    }
    edges
}

/// 每条细面贡献一个正半定秩一项，最多关联四个缩减压力样本。
struct RankOne {
    entries: Vec<(usize, f64)>,
    coefficient: f64,
}

struct ReducedSystem {
    terms: Vec<RankOne>,
    diagonal: Vec<f64>,
    rhs: Vec<f64>,
    pinned: Vec<bool>,
}

impl ReducedSystem {
    fn build(
        grid: &Grid,
        adaptive: &AdaptiveGrid,
        edges: &[FluidEdge],
        scale: Option<f64>,
    ) -> Self {
        let n = adaptive.dof_count;
        let mut diagonal = vec![0.0; n];
        let mut rhs = vec![0.0; n];
        let mut terms = Vec::with_capacity(edges.len());
        let mut components = Components::new(n);
        let mut touches_air = vec![false; n];
        if let Some(scale) = scale {
            for (cell, basis) in adaptive.basis.iter().enumerate() {
                if !grid.is_fluid(grid.domain.coord(cell)) {
                    continue;
                }
                let source = -scale * grid.divergence(grid.domain.coord(cell));
                for (&index, &weight) in basis.indices.iter().zip(&basis.weights) {
                    if weight != 0.0 {
                        rhs[index] += weight * source;
                    }
                }
            }
        }
        for edge in edges {
            let mut entries = Vec::<(usize, f64)>::with_capacity(4);
            for (cell, sign) in [(edge.left, 1.0), (edge.right, -1.0)] {
                if let Some(cell) = cell {
                    let basis = &adaptive.basis[cell];
                    for (&index, &weight) in basis.indices.iter().zip(&basis.weights) {
                        if weight == 0.0 {
                            continue;
                        }
                        if let Some(entry) = entries.iter_mut().find(|v| v.0 == index) {
                            entry.1 += sign * weight;
                        } else {
                            entries.push((index, sign * weight));
                        }
                    }
                }
            }
            entries.retain(|(_, value)| *value != 0.0);
            let dirichlet = edge.left.is_none() || edge.right.is_none();
            for &(index, value) in &entries {
                diagonal[index] += edge.coefficient * value * value;
                touches_air[index] |= dirichlet;
                components.union(entries[0].0, index);
            }
            terms.push(RankOne {
                entries,
                coefficient: edge.coefficient,
            });
        }
        let mut anchored = vec![false; n];
        for (index, &air) in touches_air.iter().enumerate() {
            let root = components.root(index);
            anchored[root] |= air;
        }
        let mut pinned = vec![false; n];
        let mut has_gauge = vec![false; n];
        for index in 0..n {
            let root = components.root(index);
            // 每个纯 Neumann 连通分量固定一个参考压力；无能量自由度也固定为零。
            if diagonal[index] == 0.0 || (!anchored[root] && !has_gauge[root]) {
                pinned[index] = true;
                has_gauge[root] = true;
                diagonal[index] = 1.0;
                rhs[index] = 0.0;
            }
        }
        Self {
            terms,
            diagonal,
            rhs,
            pinned,
        }
    }

    fn apply(&self, x: &[f64], output: &mut [f64]) {
        output.fill(0.0);
        for term in &self.terms {
            let difference: f64 = term
                .entries
                .iter()
                .filter(|(index, _)| !self.pinned[*index])
                .map(|(index, weight)| weight * x[*index])
                .sum();
            for &(index, weight) in &term.entries {
                if !self.pinned[index] {
                    output[index] += term.coefficient * weight * difference;
                }
            }
        }
        for (index, &pin) in self.pinned.iter().enumerate() {
            if pin {
                output[index] = x[index];
            }
        }
    }
}

struct SolveResult {
    pressure: Vec<f64>,
    iterations: usize,
    residual: f64,
    converged: bool,
}

fn solve_and_correct(
    grid: &mut Grid,
    adaptive: &AdaptiveGrid,
    edges: &[FluidEdge],
    dt: f64,
    density: f64,
    config: &PressureConfig,
) -> SolveResult {
    let system = ReducedSystem::build(grid, adaptive, edges, Some(density / dt));
    // 对完整细基，残差与散度成比例。固定的 Neumann 行误差可等于其余行误差之和，
    // 因此不再按 sqrt(单元数) 放宽二范数容限；预留余量供参考行及舍入误差。
    let divergence_residual = config.divergence_tolerance * density / dt * 0.1;
    let result = pcg(&system, config, divergence_residual);
    for (cell, basis) in adaptive.basis.iter().enumerate() {
        grid.pressure[cell] = interpolate(basis, &result.pressure);
    }
    for edge in edges {
        let left = edge.left.map_or(0.0, |cell| grid.pressure[cell]);
        let right = edge.right.map_or(0.0, |cell| grid.pressure[cell]);
        let velocity = grid.velocity.get(edge.axis, edge.face)
            - dt / density * (right - left) * edge.inverse_distance;
        grid.velocity.set(edge.axis, edge.face, velocity);
    }
    grid.enforce_boundaries();
    result
}

fn interpolate(basis: &PressureBasis, values: &[f64]) -> f64 {
    basis
        .indices
        .iter()
        .zip(&basis.weights)
        .filter(|(_, weight)| **weight != 0.0)
        .map(|(index, weight)| weight * values[*index])
        .sum()
}

fn pcg(system: &ReducedSystem, config: &PressureConfig, divergence_residual: f64) -> SolveResult {
    let n = system.rhs.len();
    let mut pressure = vec![0.0; n];
    let mut residual = system.rhs.clone();
    let rhs_norm = dot(&residual, &residual).sqrt();
    let tolerance = config
        .absolute_tolerance
        .max(config.relative_tolerance * rhs_norm)
        .min(divergence_residual);
    let mut z: Vec<f64> = residual
        .iter()
        .zip(&system.diagonal)
        .map(|(r, d)| r / d)
        .collect();
    let mut direction = z.clone();
    let mut rz = dot(&residual, &z);
    let mut product = vec![0.0; n];
    let mut iterations = 0;
    while dot(&residual, &residual).sqrt() > tolerance && iterations < config.max_iterations {
        system.apply(&direction, &mut product);
        let curvature = dot(&direction, &product);
        if !curvature.is_finite() || curvature <= 0.0 || !rz.is_finite() {
            break;
        }
        let alpha = rz / curvature;
        for index in 0..n {
            pressure[index] += alpha * direction[index];
            residual[index] -= alpha * product[index];
        }
        iterations += 1;
        // 递推残差可能漂移；定期及候选收敛时重新计算真实残差并重启方向。
        let refresh = iterations % 50 == 0 || dot(&residual, &residual).sqrt() <= tolerance;
        if refresh {
            system.apply(&pressure, &mut product);
            for index in 0..n {
                residual[index] = system.rhs[index] - product[index];
            }
        }
        for index in 0..n {
            z[index] = residual[index] / system.diagonal[index];
        }
        let next_rz = dot(&residual, &z);
        let beta = if refresh || rz == 0.0 {
            0.0
        } else {
            next_rz / rz
        };
        for index in 0..n {
            direction[index] = z[index] + beta * direction[index];
        }
        rz = next_rz;
    }
    system.apply(&pressure, &mut product);
    let norm = system
        .rhs
        .iter()
        .zip(product)
        .map(|(b, ap)| (b - ap).powi(2))
        .sum::<f64>()
        .sqrt();
    SolveResult {
        pressure,
        iterations,
        residual: norm,
        converged: norm.is_finite() && norm <= tolerance,
    }
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

struct Components {
    parent: Vec<usize>,
}

/// GPU 只复用 CPU 拓扑组装；右端项、迭代、归约、速度修正均由 GPU 后端计算。
#[cfg(feature = "gpu")]
pub(crate) struct GpuPressureStructure {
    pub rows: Vec<u32>,
    pub columns: Vec<u32>,
    pub values: Vec<f32>,
    pub diagonal: Vec<f32>,
    pub pinned: Vec<u32>,
    pub face_inverse_distance: Vec<f32>,
}

#[cfg(feature = "gpu")]
pub(crate) fn gpu_structure(
    grid: &Grid,
    adaptive: &AdaptiveGrid,
    minimum_fraction: f64,
) -> Result<GpuPressureStructure, String> {
    use std::collections::BTreeMap;
    validate_basis(grid, adaptive)?;
    let edges = fluid_edges(grid, minimum_fraction);
    // None 明确跳过 CPU 散度和 RHS 装配。
    let system = ReducedSystem::build(grid, adaptive, &edges, None);
    let mut sparse = vec![BTreeMap::<usize, f64>::new(); adaptive.dof_count];
    for term in &system.terms {
        for &(row, a) in &term.entries {
            if system.pinned[row] {
                continue;
            }
            for &(column, b) in &term.entries {
                if !system.pinned[column] {
                    *sparse[row].entry(column).or_default() += term.coefficient * a * b;
                }
            }
        }
    }
    let mut rows = vec![0];
    let mut columns = Vec::new();
    let mut values = Vec::new();
    for (row, entries) in sparse.iter_mut().enumerate() {
        if system.pinned[row] {
            entries.insert(row, 1.0);
        }
        for (&column, &value) in entries.iter() {
            if value != 0.0 {
                columns.push(u32::try_from(column).map_err(|_| "GPU 压力列下标溢出")?);
                values.push(value as f32);
            }
        }
        rows.push(u32::try_from(columns.len()).map_err(|_| "GPU 压力非零元数量溢出")?);
    }
    let nx = grid.velocity.faces[0].len();
    let ny = grid.velocity.faces[1].len();
    let offsets = [0, nx, nx + ny];
    let mut face_inverse_distance = vec![0.0; grid.velocity.faces.iter().map(Vec::len).sum()];
    for edge in edges {
        face_inverse_distance[offsets[edge.axis] + grid.velocity.index(edge.axis, edge.face)] =
            edge.inverse_distance as f32;
    }
    let diagonal: Vec<_> = system.diagonal.iter().map(|&v| v as f32).collect();
    if values
        .iter()
        .chain(&diagonal)
        .chain(&face_inverse_distance)
        .any(|v| !v.is_finite())
        || diagonal.iter().any(|&v| v <= 0.0)
    {
        return Err("GPU 压力矩阵不能由有限 f32 表示".into());
    }
    Ok(GpuPressureStructure {
        rows,
        columns,
        values,
        diagonal,
        pinned: system.pinned.iter().map(|&v| u32::from(v)).collect(),
        face_inverse_distance,
    })
}

impl Components {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }
    fn root(&mut self, mut n: usize) -> usize {
        while self.parent[n] != n {
            self.parent[n] = self.parent[self.parent[n]];
            n = self.parent[n];
        }
        n
    }
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.root(a);
        let rb = self.root(b);
        self.parent[rb] = ra;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Domain;

    fn full_water(size: [usize; 3]) -> Grid {
        let mut grid = Grid::new(Domain::new(size, 0.2).unwrap());
        grid.phi.fill(-1.0);
        grid
    }

    fn disturb(grid: &mut Grid) {
        for axis in 0..3 {
            for (n, velocity) in grid.velocity.faces[axis].iter_mut().enumerate() {
                *velocity = ((n * 17 + axis * 13) as f64 * 0.23).sin();
            }
        }
    }

    #[test]
    fn closed_three_dimensional_projection_reduces_divergence() {
        let mut grid = full_water([5, 4, 3]);
        disturb(&mut grid);
        let uniform = AdaptiveGrid::uniform(&grid);
        let result = project(
            &mut grid,
            &uniform,
            0.02,
            1000.0,
            &PressureConfig::default(),
        )
        .unwrap();
        assert!(result.divergence_before > 1.0);
        assert!(result.converged, "{result:?}");
        assert!(result.divergence_after < 1e-6, "{result:?}");
        assert!(!result.fallback_used);
        assert!(grid.pressure.iter().all(|x| x.is_finite()));
        assert_eq!(grid.pressure[0], 0.0); // 一个闭合连通分量的参考压力。
    }

    #[test]
    fn gravity_balances_hydrostatic_lake_with_subcell_surface() {
        let mut grid = full_water([4, 7, 3]);
        grid.set_liquid_sdf(|p| p[1] - 0.93);
        let dt = 0.015;
        grid.velocity.faces[1].fill(-9.81 * dt);
        let uniform = AdaptiveGrid::uniform(&grid);
        let result = project(&mut grid, &uniform, dt, 1000.0, &PressureConfig::default()).unwrap();
        assert!(result.converged, "{result:?}");
        for n in 0..grid.domain.cell_count() {
            let p = grid.domain.coord(n);
            if grid.is_fluid(p) {
                let expected = 1000.0 * 9.81 * (0.93 - grid.domain.center(p)[1]);
                assert!((grid.pressure[n] - expected).abs() < 1e-3);
                assert!(
                    grid.sample_velocity(grid.domain.center(p))
                        .iter()
                        .all(|v| v.abs() < 1e-7)
                );
            }
        }
    }

    #[test]
    fn solid_wall_separates_neumann_components_without_leaks() {
        let mut grid = full_water([7, 4, 3]);
        for n in 0..grid.domain.cell_count() {
            if grid.domain.coord(n)[0] == 3 {
                grid.solid_phi[n] = -1.0;
            }
        }
        disturb(&mut grid);
        let uniform = AdaptiveGrid::uniform(&grid);
        let result = project(
            &mut grid,
            &uniform,
            0.01,
            1000.0,
            &PressureConfig::default(),
        )
        .unwrap();
        assert!(result.converged, "{result:?}");
        for k in 0..3 {
            for j in 0..4 {
                assert_eq!(grid.velocity.get(0, [3, j, k]), 0.0);
                assert_eq!(grid.velocity.get(0, [4, j, k]), 0.0);
            }
        }
    }

    #[test]
    fn column_reduction_is_used_when_it_can_represent_the_pressure() {
        let mut grid = full_water([5, 4, 3]);
        for k in 0..3 {
            for j in 0..4 {
                for i in 1..5 {
                    grid.velocity.set(0, [i, j, k], (i as f64).sin());
                }
            }
        }
        let mut reduced = AdaptiveGrid::uniform(&grid);
        reduced.dof_count = 5 * 3;
        for (n, basis) in reduced.basis.iter_mut().enumerate() {
            let p = grid.domain.coord(n);
            *basis = PressureBasis {
                indices: [p[0] + 5 * p[2], 0],
                weights: [1.0, 0.0],
            };
        }
        let result = project(
            &mut grid,
            &reduced,
            0.02,
            1000.0,
            &PressureConfig::default(),
        )
        .unwrap();
        assert!(result.converged, "{result:?}");
        assert_eq!(result.pressure_dofs, 15);
        assert!(!result.fallback_used);
        assert!(result.divergence_after < 1e-6);
    }

    #[test]
    fn two_endpoint_tall_basis_preserves_hydrostatic_pressure() {
        let mut grid = full_water([4, 7, 3]);
        grid.set_liquid_sdf(|p| p[1] - 0.93);
        let dt = 0.015;
        grid.velocity.faces[1].fill(-9.81 * dt);
        let mut reduced = AdaptiveGrid::uniform(&grid);
        reduced.dof_count = 2 * 4 * 3;
        for (cell, basis) in reduced.basis.iter_mut().enumerate() {
            let p = grid.domain.coord(cell);
            if grid.is_fluid(p) {
                let column = p[0] + 4 * p[2];
                let t = p[1] as f64 / 4.0;
                *basis = PressureBasis {
                    indices: [column * 2, column * 2 + 1],
                    weights: [1.0 - t, t],
                };
            }
        }
        let report = project(&mut grid, &reduced, dt, 1000.0, &PressureConfig::default()).unwrap();
        assert!(report.converged, "{report:?}");
        assert!(!report.fallback_used, "{report:?}");
        assert_eq!(report.pressure_dofs, 24);
        for n in 0..grid.domain.cell_count() {
            let p = grid.domain.coord(n);
            if grid.is_fluid(p) {
                let expected = 1000.0 * 9.81 * (0.93 - grid.domain.center(p)[1]);
                assert!((grid.pressure[n] - expected).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn reduced_residual_does_not_hide_fine_divergence_and_fallback_repairs_it() {
        let mut grid = full_water([4, 4, 3]);
        disturb(&mut grid);
        let original = grid.clone();
        let mut reduced = AdaptiveGrid::uniform(&grid);
        reduced.dof_count = 1;
        reduced.basis.fill(PressureBasis {
            indices: [0, 0],
            weights: [1.0, 0.0],
        });
        let no_fallback = PressureConfig {
            fine_fallback: false,
            ..PressureConfig::default()
        };
        let report = project(&mut grid, &reduced, 0.02, 1000.0, &no_fallback).unwrap();
        assert!(!report.converged);
        assert_eq!(report.residual_norm, 0.0);
        assert!(report.divergence_after > 1.0);
        grid = original;
        let report = project(
            &mut grid,
            &reduced,
            0.02,
            1000.0,
            &PressureConfig::default(),
        )
        .unwrap();
        assert!(report.converged, "{report:?}");
        assert!(report.fallback_used);
        assert_eq!(report.adaptive_pressure_dofs, 1);
        assert_eq!(report.pressure_dofs, 48);
        assert!(report.adaptive_divergence > 1.0);
    }

    #[test]
    fn empty_liquid_and_invalid_input_are_handled() {
        let mut grid = Grid::new(Domain::new([3; 3], 1.0).unwrap());
        let uniform = AdaptiveGrid::uniform(&grid);
        let result = project(
            &mut grid,
            &uniform,
            0.01,
            1000.0,
            &PressureConfig::default(),
        )
        .unwrap();
        assert!(result.converged);
        assert_eq!(result.pressure_dofs, 0);
        assert_eq!(result.iterations, 0);
        assert!(project(&mut grid, &uniform, 0.0, 1000.0, &PressureConfig::default()).is_err());
        let invalid = PressureConfig {
            minimum_interface_fraction: f64::NAN,
            ..PressureConfig::default()
        };
        assert!(invalid.validate().is_err());
    }
}

//! 水平四叉树、速度重建误差与自适应光学层的 CPU 参考实现。
//!
//! 局部评价在六个 MAC 面上累加速度重建的绝对误差；水平速度沿水柱线性
//! 最小二乘拟合，竖直速度取平均。压力使用细网格上的分片常量/竖直线性基底，
//! 供 Galerkin 投影使用，并非论文专用虚拟面和变分单元结构的逐项复现。

use std::collections::BinaryHeap;

use crate::grid::{Domain, Grid};

#[derive(Clone, Debug)]
pub struct AdaptivityConfig {
    /// 水平叶子最大宽度，以最细网格单元数计；不要求为二的幂。
    pub max_leaf_width: usize,
    /// 同一水平叶子内允许的自由表面高度差，单位为最细网格格数。
    pub surface_height_tolerance: f64,
    /// 六面速度重建绝对误差之和超过此值时细分，单位 m/s。
    pub velocity_residual_tolerance: f64,
    /// 将水柱累积评价转换为扩张距离的系数，单位 格/(m/s)。
    pub optical_scale: f64,
    /// 不与底部/静态固体相接的液体使用的固定扩张距离，单位格。
    pub airborne_layers: usize,
    /// 限制相邻光学层下边界高度跳变的扫描次数；只扩大精细区域。
    pub smoothing_passes: usize,
}

impl Default for AdaptivityConfig {
    fn default() -> Self {
        Self {
            max_leaf_width: 4,
            surface_height_tolerance: 1.0,
            velocity_residual_tolerance: 0.1,
            optical_scale: 64.0,
            airborne_layers: 4,
            smoothing_passes: 2,
        }
    }
}

impl AdaptivityConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_leaf_width == 0 || self.airborne_layers == 0 {
            return Err("水平叶子宽度和独立液体扩张距离必须至少为一格".into());
        }
        if [
            self.surface_height_tolerance,
            self.velocity_residual_tolerance,
            self.optical_scale,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err("自适应阈值和光学层比例必须为有限非负数".into());
        }
        Ok(())
    }
}

/// 水平矩形叶子，坐标顺序为 x、z；max 是排他上界。
/// 计算域边缘会裁剪正方形，因此非二次幂尺寸也能完整覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuadLeaf {
    pub min: [usize; 2],
    pub max: [usize; 2],
}

impl QuadLeaf {
    pub fn width(&self) -> usize {
        (self.max[0] - self.min[0]).max(self.max[1] - self.min[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellKind {
    /// 水平叶子内的一层单元，用一个压力自由度表示。
    Regular,
    /// 连续深水柱段，用上下两个压力自由度线性插值。
    Tall,
}

#[derive(Clone, Debug)]
pub struct AdaptiveCell {
    /// 最细网格坐标下的包围盒，max 为排他上界。
    pub min: [usize; 3],
    pub max: [usize; 3],
    pub kind: CellKind,
    pub pressure_indices: [usize; 2],
    pub pressure_count: usize,
}

/// 每个细网格中心至压缩压力向量的 prolongation 行。
/// 两个权重非负，液体格权重和为一；非液体格权重均为零。
#[derive(Clone, Copy, Debug, Default)]
pub struct PressureBasis {
    pub indices: [usize; 2],
    pub weights: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct AdaptiveGrid {
    /// 六面重建误差之和，长度为最细网格单元数，单位 m/s。
    pub local_cost: Vec<f64>,
    /// 沿水柱求和后除以 Ny；下标为 x + Nx*z，单位 m/s。
    pub integrated_cost: Vec<f64>,
    /// L1 膨胀和高度平滑后的精细区域，包括被膨胀覆盖的空气格。
    pub optical_mask: Vec<bool>,
    pub leaves: Vec<QuadLeaf>,
    pub cells: Vec<AdaptiveCell>,
    pub basis: Vec<PressureBasis>,
    pub dof_count: usize,
}

#[derive(Clone, Debug)]
struct FluidRun {
    start: usize,
    end: usize,
    grounded: bool,
    cost: f64,
}

impl AdaptiveGrid {
    /// 仅根据已求得的局部评价构建 CPU 水平拓扑；GPU 路径不会在此重算评价。
    #[cfg(feature = "gpu")]
    pub(crate) fn leaves_from_cost(
        grid: &Grid,
        config: &AdaptivityConfig,
        local_cost: &[f64],
    ) -> Vec<QuadLeaf> {
        let columns = topology_columns(grid);
        let mut leaves = Vec::new();
        subdivide(
            grid,
            config,
            &columns,
            local_cost,
            [0, 0],
            grid.domain.size[0]
                .max(grid.domain.size[2])
                .next_power_of_two(),
            &mut leaves,
        );
        leaves
    }

    /// GPU 回读后的拓扑整理。只构造单元和压力基底，不进行任何评价/光学层计算。
    #[cfg(feature = "gpu")]
    pub(crate) fn from_metrics(
        grid: &Grid,
        local_cost: Vec<f64>,
        integrated_cost: Vec<f64>,
        optical_mask: Vec<bool>,
        leaves: Vec<QuadLeaf>,
    ) -> Self {
        let mut result = Self {
            local_cost,
            integrated_cost,
            optical_mask,
            leaves,
            cells: Vec::new(),
            basis: vec![PressureBasis::default(); grid.domain.cell_count()],
            dof_count: 0,
        };
        result.build_basis(grid.domain, &topology_columns(grid));
        result
    }

    /// 调用方应先通过 Grid::validate 和 AdaptivityConfig::validate 校验输入。
    pub fn build(grid: &Grid, config: &AdaptivityConfig) -> Self {
        let domain = grid.domain;
        let [nx, ny, nz] = domain.size;
        let mut result = Self {
            local_cost: vec![0.0; domain.cell_count()],
            integrated_cost: vec![0.0; nx * nz],
            optical_mask: vec![false; domain.cell_count()],
            leaves: Vec::new(),
            cells: Vec::new(),
            basis: vec![PressureBasis::default(); domain.cell_count()],
            dof_count: 0,
        };
        let mut columns = vec![Vec::new(); nx * nz];
        for z in 0..nz {
            for x in 0..nx {
                let column = x + nx * z;
                let mut y = 0;
                while y < ny {
                    if !grid.is_fluid([x, y, z]) {
                        y += 1;
                        continue;
                    }
                    let start = y;
                    while y < ny && grid.is_fluid([x, y, z]) {
                        y += 1;
                    }
                    let grounded =
                        start == 0 || grid.solid_phi[domain.index([x, start - 1, z])] < 0.0;
                    let mut run = FluidRun {
                        start,
                        end: y,
                        grounded,
                        cost: 0.0,
                    };
                    evaluate_column(grid, x, z, &run, &mut result.local_cost);
                    run.cost = (start..y)
                        .map(|j| result.local_cost[domain.index([x, j, z])])
                        .sum::<f64>()
                        / ny as f64;
                    result.integrated_cost[column] += run.cost;
                    columns[column].push(run);
                }
            }
        }

        let width = nx.max(nz).next_power_of_two();
        subdivide(
            grid,
            config,
            &columns,
            &result.local_cost,
            [0, 0],
            width,
            &mut result.leaves,
        );
        let mut leaf_width = vec![1; nx * nz];
        for leaf in &result.leaves {
            for z in leaf.min[1]..leaf.max[1] {
                for x in leaf.min[0]..leaf.max[0] {
                    leaf_width[x + nx * z] = leaf.width();
                }
            }
        }
        let mut seeds = Vec::new();
        for z in 0..nz {
            for x in 0..nx {
                let column = x + nx * z;
                for run in &columns[column] {
                    let radius = if run.grounded {
                        // 论文的上下限在小测试域内可能颠倒。此处提升上限以保留
                        // 最低精细层，并将最低距离限制在域高以内，避免非法 clamp。
                        let lower = leaf_width[column].saturating_mul(2).max(4).min(ny);
                        let upper = (ny / 8).max(lower);
                        (config.optical_scale * run.cost)
                            .clamp(lower as f64, upper as f64)
                            .ceil() as usize
                    } else {
                        config
                            .airborne_layers
                            .min(nx.saturating_add(ny).saturating_add(nz))
                    };
                    for y in run.start..run.end {
                        let p = [x, y, z];
                        if touches_air(grid, p) {
                            seeds.push((domain.index(p), radius));
                        }
                    }
                }
            }
        }
        result.optical_mask = dilate(domain, &seeds);
        smooth_heights(
            domain,
            &columns,
            &mut result.optical_mask,
            config.smoothing_passes,
        );
        result.build_basis(domain, &columns);
        result
    }

    fn build_basis(&mut self, domain: Domain, columns: &[Vec<FluidRun>]) {
        // 光学层从细网格映射到水平叶子时采用保守规则：只要叶子内任一格
        // 需要精细表示，该高度整层都保留，不采用论文的过半数判据。
        // 这样不会因粗化丢掉薄的精细区域，代价是压力未知量可能更多。
        for leaf_index in 0..self.leaves.len() {
            let leaf = self.leaves[leaf_index];
            let runs = &columns[leaf.min[0] + domain.size[0] * leaf.min[1]];
            for run in runs {
                let allow_tall = run.grounded && runs.len() == 1;
                let mut y = run.start;
                while y < run.end {
                    let tall_start = y;
                    if allow_tall && !layer_is_optical(domain, leaf, y, &self.optical_mask) {
                        while y < run.end && !layer_is_optical(domain, leaf, y, &self.optical_mask)
                        {
                            y += 1;
                        }
                    }
                    if y - tall_start >= 3 {
                        self.add_cell(domain, leaf, tall_start, y, CellKind::Tall);
                    } else {
                        let end = if y == tall_start { y + 1 } else { y };
                        for j in tall_start..end {
                            self.add_cell(domain, leaf, j, j + 1, CellKind::Regular);
                        }
                        y = end;
                    }
                }
            }
        }
    }

    /// 完全细分的压力基底，用于参考验证及压缩投影误差过大时的回退。
    pub fn uniform(grid: &Grid) -> Self {
        let domain = grid.domain;
        let [nx, ny, nz] = domain.size;
        let mut result = Self {
            local_cost: vec![0.0; domain.cell_count()],
            integrated_cost: vec![0.0; nx * nz],
            optical_mask: vec![false; domain.cell_count()],
            leaves: Vec::with_capacity(nx * nz),
            cells: Vec::new(),
            basis: vec![PressureBasis::default(); domain.cell_count()],
            dof_count: 0,
        };
        for z in 0..nz {
            for x in 0..nx {
                let leaf = QuadLeaf {
                    min: [x, z],
                    max: [x + 1, z + 1],
                };
                result.leaves.push(leaf);
                for y in 0..ny {
                    if grid.is_fluid([x, y, z]) {
                        result.optical_mask[domain.index([x, y, z])] = true;
                        result.add_cell(domain, leaf, y, y + 1, CellKind::Regular);
                    }
                }
            }
        }
        result
    }

    fn add_cell(
        &mut self,
        domain: Domain,
        leaf: QuadLeaf,
        start: usize,
        end: usize,
        kind: CellKind,
    ) {
        let pressure_count = if kind == CellKind::Tall { 2 } else { 1 };
        let first = self.dof_count;
        let indices = [first, first + pressure_count - 1];
        self.dof_count += pressure_count;
        for z in leaf.min[1]..leaf.max[1] {
            for x in leaf.min[0]..leaf.max[0] {
                for y in start..end {
                    // 等距网格下等价于在两端单元中心的物理高度之间插值。
                    let t = if pressure_count == 2 {
                        (y - start) as f64 / (end - start - 1) as f64
                    } else {
                        0.0
                    };
                    self.basis[domain.index([x, y, z])] = PressureBasis {
                        indices,
                        weights: [1.0 - t, t],
                    };
                }
            }
        }
        self.cells.push(AdaptiveCell {
            min: [leaf.min[0], start, leaf.min[1]],
            max: [leaf.max[0], end, leaf.max[1]],
            kind,
            pressure_indices: indices,
            pressure_count,
        });
    }
}

#[cfg(feature = "gpu")]
fn topology_columns(grid: &Grid) -> Vec<Vec<FluidRun>> {
    let [nx, ny, nz] = grid.domain.size;
    let mut columns = vec![Vec::new(); nx * nz];
    for z in 0..nz {
        for x in 0..nx {
            let mut y = 0;
            while y < ny {
                if !grid.is_fluid([x, y, z]) {
                    y += 1;
                    continue;
                }
                let start = y;
                while y < ny && grid.is_fluid([x, y, z]) {
                    y += 1;
                }
                columns[x + nx * z].push(FluidRun {
                    start,
                    end: y,
                    grounded: start == 0
                        || grid.solid_phi[grid.domain.index([x, start - 1, z])] < 0.0,
                    cost: 0.0,
                });
            }
        }
    }
    columns
}

/// 水柱两侧的四条水平速度采样列分别拟合，避免把横向速度差混入竖直拟合。
fn evaluate_column(grid: &Grid, x: usize, z: usize, run: &FluidRun, cost: &mut [f64]) {
    let center = (run.start + run.end - 1) as f64 * 0.5;
    let count = (run.end - run.start) as f64;
    for axis in [0, 2] {
        for side in 0..2 {
            let sample = |y| {
                let mut p = [x, y, z];
                p[axis] += side;
                grid.velocity.get(axis, p)
            };
            let mean = (run.start..run.end).map(sample).sum::<f64>() / count;
            let variance = (run.start..run.end)
                .map(|y| (y as f64 - center).powi(2))
                .sum::<f64>();
            let slope = if variance > 0.0 {
                (run.start..run.end)
                    .map(|y| (y as f64 - center) * (sample(y) - mean))
                    .sum::<f64>()
                    / variance
            } else {
                0.0
            };
            for y in run.start..run.end {
                let reconstructed = mean + slope * (y as f64 - center);
                cost[grid.domain.index([x, y, z])] += (reconstructed - sample(y)).abs();
            }
        }
    }
    let vertical_mean = (run.start..=run.end)
        .map(|y| grid.velocity.get(1, [x, y, z]))
        .sum::<f64>()
        / (count + 1.0);
    for y in run.start..run.end {
        cost[grid.domain.index([x, y, z])] += (grid.velocity.get(1, [x, y, z]) - vertical_mean)
            .abs()
            + (grid.velocity.get(1, [x, y + 1, z]) - vertical_mean).abs();
    }
}

fn subdivide(
    grid: &Grid,
    config: &AdaptivityConfig,
    columns: &[Vec<FluidRun>],
    cost: &[f64],
    min: [usize; 2],
    width: usize,
    leaves: &mut Vec<QuadLeaf>,
) {
    let [nx, ny, nz] = grid.domain.size;
    if min[0] >= nx || min[1] >= nz {
        return;
    }
    let leaf = QuadLeaf {
        min,
        max: [(min[0] + width).min(nx), (min[1] + width).min(nz)],
    };
    let reference = &columns[min[0] + nx * min[1]];
    let mut split = leaf.width() > config.max_leaf_width;
    let mut min_height = f64::INFINITY;
    let mut max_height = f64::NEG_INFINITY;
    for z in leaf.min[1]..leaf.max[1] {
        for x in leaf.min[0]..leaf.max[0] {
            let runs = &columns[x + nx * z];
            // 复杂拓扑保持水平最细，垂直方向也不会跨不连续液柱合并。
            split |= runs.len() > 1
                || runs.iter().any(|r| !r.grounded)
                || runs.len() != reference.len()
                || runs
                    .iter()
                    .zip(reference)
                    .any(|(a, b)| a.start != b.start || a.end != b.end || a.grounded != b.grounded);
            // 相同液柱区间也可能一侧顶着固体、另一侧接空气；保留这种边界区别。
            for y in 0..ny {
                let a = grid.domain.index([x, y, z]);
                let b = grid.domain.index([min[0], y, min[1]]);
                split |= (grid.solid_phi[a] < 0.0) != (grid.solid_phi[b] < 0.0);
            }
            for run in runs {
                let mut height = run.end as f64 - 0.5;
                if run.end < ny {
                    let below = grid.domain.index([x, run.end - 1, z]);
                    let above = grid.domain.index([x, run.end, z]);
                    if grid.solid_phi[above] >= 0.0 && grid.phi[above] >= 0.0 {
                        let fraction = -grid.phi[below] / (grid.phi[above] - grid.phi[below]);
                        height += fraction;
                    }
                }
                min_height = min_height.min(height);
                max_height = max_height.max(height);
                for y in run.start..run.end {
                    split |=
                        cost[grid.domain.index([x, y, z])] > config.velocity_residual_tolerance;
                }
            }
        }
    }
    split |= max_height - min_height > config.surface_height_tolerance;
    if split && width > 1 {
        let half = width / 2;
        for z in 0..2 {
            for x in 0..2 {
                subdivide(
                    grid,
                    config,
                    columns,
                    cost,
                    [min[0] + x * half, min[1] + z * half],
                    half,
                    leaves,
                );
            }
        }
    } else {
        leaves.push(leaf);
    }
}

fn neighbors(domain: Domain, p: [usize; 3]) -> impl Iterator<Item = [usize; 3]> {
    (0..3).flat_map(move |axis| {
        let mut minus = p;
        let mut plus = p;
        minus[axis] = minus[axis].saturating_sub(1);
        plus[axis] += 1;
        [
            (p[axis] > 0).then_some(minus),
            (plus[axis] < domain.size[axis]).then_some(plus),
        ]
        .into_iter()
        .flatten()
    })
}

fn touches_air(grid: &Grid, p: [usize; 3]) -> bool {
    neighbors(grid.domain, p).any(|q| {
        let index = grid.domain.index(q);
        grid.phi[index] >= 0.0 && grid.solid_phi[index] >= 0.0
    })
}

/// 最大剩余半径传播，等价于变半径种子的 L1 球并集，避免逐种子遍历立方体。
fn dilate(domain: Domain, seeds: &[(usize, usize)]) -> Vec<bool> {
    let mut distance: Vec<Option<usize>> = vec![None; domain.cell_count()];
    let mut queue = BinaryHeap::new();
    for &(index, radius) in seeds {
        if distance[index].is_none_or(|old| old < radius) {
            distance[index] = Some(radius);
            queue.push((radius, index));
        }
    }
    while let Some((remaining, index)) = queue.pop() {
        if distance[index] != Some(remaining) || remaining == 0 {
            continue;
        }
        for p in neighbors(domain, domain.coord(index)) {
            let next = domain.index(p);
            if distance[next].is_none_or(|old| old < remaining - 1) {
                distance[next] = Some(remaining - 1);
                queue.push((remaining - 1, next));
            }
        }
    }
    distance.into_iter().map(|d| d.is_some()).collect()
}

fn smooth_heights(domain: Domain, columns: &[Vec<FluidRun>], mask: &mut [bool], passes: usize) {
    let [nx, _, nz] = domain.size;
    let mut heights = vec![None; nx * nz];
    for z in 0..nz {
        for x in 0..nx {
            let c = x + nx * z;
            if columns[c].len() == 1 && columns[c][0].grounded {
                let run = &columns[c][0];
                heights[c] = Some(
                    (run.start..run.end)
                        .find(|&y| mask[domain.index([x, y, z])])
                        .unwrap_or(run.end),
                );
            }
        }
    }
    for _ in 0..passes {
        let mut next = heights.clone();
        for z in 0..nz {
            for x in 0..nx {
                let c = x + nx * z;
                let Some(mut height) = heights[c] else {
                    continue;
                };
                let run = &columns[c][0];
                for p in neighbors(domain, [x, 0, z]).filter(|p| p[1] == 0) {
                    let neighbor = p[0] + nx * p[2];
                    if let Some(neighbor_height) = heights[neighbor] {
                        let other = &columns[neighbor][0];
                        if run.start < other.end && other.start < run.end {
                            height = height.min((neighbor_height + 1).max(run.start));
                        }
                    }
                }
                next[c] = Some(height);
            }
        }
        heights = next;
    }
    for z in 0..nz {
        for x in 0..nx {
            let c = x + nx * z;
            if let Some(height) = heights[c] {
                for y in height..columns[c][0].end {
                    mask[domain.index([x, y, z])] = true;
                }
            }
        }
    }
}

fn layer_is_optical(domain: Domain, leaf: QuadLeaf, y: usize, mask: &[bool]) -> bool {
    (leaf.min[1]..leaf.max[1])
        .any(|z| (leaf.min[0]..leaf.max[0]).any(|x| mask[domain.index([x, y, z])]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tank(size: [usize; 3], depth: f64) -> Grid {
        let mut grid = Grid::new(Domain::new(size, 1.0).unwrap());
        grid.set_liquid_sdf(|p| p[1] - depth);
        grid
    }

    fn set_velocity(grid: &mut Grid, f: impl Fn(usize, [f64; 3]) -> f64) {
        for axis in 0..3 {
            let dims = grid.velocity.dimensions(axis);
            for z in 0..dims[2] {
                for y in 0..dims[1] {
                    for x in 0..dims[0] {
                        let p = [x, y, z];
                        let value = f(axis, grid.velocity.position(axis, p));
                        grid.velocity.set(axis, p, value);
                    }
                }
            }
        }
    }

    #[test]
    fn linear_horizontal_velocity_and_constant_vertical_have_zero_cost() {
        let mut grid = tank([5, 32, 7], 24.0);
        // 不调用封闭墙面条件；测试的是可精确重建的解析速度场。
        set_velocity(&mut grid, |axis, p| match axis {
            0 => 0.2 * p[1] + 3.0,
            1 => 1.5,
            _ => -0.7 * p[1] + 2.0,
        });
        let adaptive = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        assert!(adaptive.local_cost.iter().all(|q| q.abs() < 1e-12));
        assert!(adaptive.integrated_cost.iter().all(|q| q.abs() < 1e-12));
    }

    #[test]
    fn nonlinear_velocity_increases_error_and_optical_thickness() {
        let mut grid = tank([4, 128, 4], 96.0);
        let config = AdaptivityConfig {
            max_leaf_width: 1,
            smoothing_passes: 0,
            ..AdaptivityConfig::default()
        };
        let calm = AdaptiveGrid::build(&grid, &config);
        set_velocity(&mut grid, |axis, p| {
            if axis == 0 {
                0.01 * (p[1] - 48.0).powi(2)
            } else {
                0.0
            }
        });
        let disturbed = AdaptiveGrid::build(&grid, &config);
        assert!(disturbed.integrated_cost.iter().all(|q| *q > 1.0));
        assert!(
            disturbed.optical_mask.iter().filter(|v| **v).count()
                > calm.optical_mask.iter().filter(|v| **v).count()
        );
        assert!(disturbed.dof_count > calm.dof_count);
    }

    #[test]
    fn clipped_quadtree_covers_non_power_of_two_domain_once() {
        let grid = tank([7, 64, 11], 48.0);
        let adaptive = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        let mut coverage = vec![0; 7 * 11];
        for leaf in &adaptive.leaves {
            for z in leaf.min[1]..leaf.max[1] {
                for x in leaf.min[0]..leaf.max[0] {
                    coverage[x + 7 * z] += 1;
                }
            }
        }
        assert!(coverage.iter().all(|&n| n == 1));
        assert!(adaptive.leaves.iter().any(|l| l.width() > 1));
        let fine = AdaptiveGrid::uniform(&grid);
        assert!(adaptive.dof_count < fine.dof_count / 2);
        for (n, basis) in adaptive.basis.iter().enumerate() {
            let expected = if grid.is_fluid(grid.domain.coord(n)) {
                1.0
            } else {
                0.0
            };
            assert!((basis.weights.iter().sum::<f64>() - expected).abs() < 1e-12);
            if expected > 0.0 {
                assert!(basis.indices.iter().all(|&i| i < adaptive.dof_count));
            }
        }
    }

    #[test]
    fn tall_basis_reproduces_linear_pressure_at_physical_centers() {
        let grid = tank([4, 64, 4], 48.0);
        let adaptive = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        let mut pressure = vec![0.0; adaptive.dof_count];
        for cell in &adaptive.cells {
            pressure[cell.pressure_indices[0]] = 7.0 - 3.0 * grid.domain.center(cell.min)[1];
            if cell.kind == CellKind::Tall {
                let mut top = cell.min;
                top[1] = cell.max[1] - 1;
                pressure[cell.pressure_indices[1]] = 7.0 - 3.0 * grid.domain.center(top)[1];
            }
        }
        assert!(adaptive.cells.iter().any(|c| c.kind == CellKind::Tall));
        for n in 0..grid.domain.cell_count() {
            let p = grid.domain.coord(n);
            if grid.is_fluid(p) {
                let b = adaptive.basis[n];
                let value =
                    b.weights[0] * pressure[b.indices[0]] + b.weights[1] * pressure[b.indices[1]];
                assert!((value - (7.0 - 3.0 * grid.domain.center(p)[1])).abs() < 1e-10);
            }
        }
    }

    #[test]
    fn solid_shelves_and_air_gaps_are_never_merged() {
        let mut grid = tank([4, 64, 4], 48.0);
        // 一层固体平台，上下均有水；另一列中间留空气洞。
        for n in 0..grid.domain.cell_count() {
            let p = grid.domain.coord(n);
            if p[1] == 20 {
                grid.solid_phi[n] = -1.0;
            }
            if p[0] == 1 && (10..14).contains(&p[1]) {
                grid.phi[n] = 1.0;
            }
        }
        let adaptive = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        for cell in &adaptive.cells {
            for z in cell.min[2]..cell.max[2] {
                for y in cell.min[1]..cell.max[1] {
                    for x in cell.min[0]..cell.max[0] {
                        assert!(grid.is_fluid([x, y, z]));
                    }
                }
            }
        }
        for (n, b) in adaptive.basis.iter().enumerate() {
            assert_eq!(
                b.weights.iter().sum::<f64>() > 0.0,
                grid.is_fluid(grid.domain.coord(n))
            );
        }
    }

    #[test]
    fn detached_drop_keeps_full_vertical_resolution_and_small_domain_is_valid() {
        let mut grid = tank([4, 16, 4], -1.0);
        grid.set_liquid_sdf(|p| {
            ((p[0] - 2.0).powi(2) + (p[1] - 8.0).powi(2) + (p[2] - 2.0).powi(2)).sqrt() - 1.5
        });
        let adaptive = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        assert!(adaptive.cells.iter().all(|c| c.kind == CellKind::Regular));
        assert_eq!(adaptive.dof_count, AdaptiveGrid::uniform(&grid).dof_count);
        let small = tank([2, 2, 2], 1.0);
        let small_adaptive = AdaptiveGrid::build(&small, &AdaptivityConfig::default());
        assert_eq!(small_adaptive.dof_count, 1);
        assert!(
            small_adaptive
                .cells
                .iter()
                .all(|c| c.kind == CellKind::Regular)
        );
    }

    #[test]
    fn face_residual_sum_and_domain_height_normalization_match_hand_calculation() {
        let mut grid = tank([2, 8, 2], 3.0);
        set_velocity(&mut grid, |axis, p| {
            if axis == 0 && (1.0..2.0).contains(&p[1]) {
                3.0
            } else {
                0.0
            }
        });
        let adaptive = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        // 每条 x 面列 [0,3,0] 最小二乘重建为 [1,1,1]，左右两面各贡献一次。
        for (y, expected) in [2.0, 4.0, 2.0].into_iter().enumerate() {
            assert!((adaptive.local_cost[grid.domain.index([0, y, 0])] - expected).abs() < 1e-12);
        }
        // 除以整个域的8层，不是液柱的3层。
        assert!((adaptive.integrated_cost[0] - 1.0).abs() < 1e-12);
    }

    #[test]
    fn dilation_is_manhattan_and_smoothing_never_removes_optical_cells() {
        let domain = Domain::new([9; 3], 1.0).unwrap();
        let mask = dilate(domain, &[(domain.index([4; 3]), 2)]);
        assert!(mask[domain.index([6, 4, 4])]);
        assert!(mask[domain.index([5, 5, 4])]);
        assert!(!mask[domain.index([5, 5, 5])]);
        let mut grid = tank([4, 64, 4], 48.0);
        set_velocity(&mut grid, |axis, p| {
            if axis == 0 && p[0] < 1.0 {
                (p[1] * 0.5).sin()
            } else {
                0.0
            }
        });
        let unsmoothed = AdaptiveGrid::build(
            &grid,
            &AdaptivityConfig {
                smoothing_passes: 0,
                ..AdaptivityConfig::default()
            },
        );
        let smoothed = AdaptiveGrid::build(&grid, &AdaptivityConfig::default());
        assert!(
            unsmoothed
                .optical_mask
                .iter()
                .zip(&smoothed.optical_mask)
                .all(|(old, new)| !old || *new)
        );
    }

    #[test]
    fn invalid_config_is_rejected() {
        assert!(
            AdaptivityConfig {
                optical_scale: f64::NAN,
                ..AdaptivityConfig::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            AdaptivityConfig {
                max_leaf_width: 0,
                ..AdaptivityConfig::default()
            }
            .validate()
            .is_err()
        );
    }
}

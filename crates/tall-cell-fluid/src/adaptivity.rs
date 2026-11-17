//! CPU reference implementation of horizontal quadtrees, velocity reconstruction error, and adaptive optical layers.
//!
//! The local metric sums absolute velocity reconstruction errors over six MAC faces;
//! horizontal velocities are fitted linearly by least squares along the water column,
//! and vertical velocity is averaged. Pressure uses piecewise-constant/vertically linear
//! bases on the fine grid for Galerkin projection, not a feature-by-feature reproduction
//! of the paper's specialized virtual faces and variational cell structure.

use std::collections::BinaryHeap;

use crate::grid::{Domain, Grid};

#[derive(Clone, Debug)]
pub struct AdaptivityConfig {
    /// Maximum horizontal leaf width in finest-grid cells; need not be a power of two.
    pub max_leaf_width: usize,
    /// Allowed free-surface height difference within a horizontal leaf, in finest-grid cells.
    pub surface_height_tolerance: f64,
    /// Subdivide when the sum of absolute velocity reconstruction errors on six faces exceeds this value, in m/s.
    pub velocity_residual_tolerance: f64,
    /// Factor converting the integrated water-column metric to a dilation distance, in cells/(m/s).
    pub optical_scale: f64,
    /// Fixed dilation distance for liquid not touching the bottom or static solids, in cells.
    pub airborne_layers: usize,
    /// Number of passes limiting jumps in neighboring optical-layer lower boundary heights; only expands the fine region.
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
            return Err("Horizontal leaf width and detached liquid dilation distance must be at least one cell".into());
        }
        if [
            self.surface_height_tolerance,
            self.velocity_residual_tolerance,
            self.optical_scale,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(
                "Adaptive thresholds and optical layer scale must be finite and nonnegative".into(),
            );
        }
        Ok(())
    }
}

/// Horizontal rectangular leaf, with coordinates in x, z order; max is exclusive.
/// Squares are clipped at domain edges, so non-power-of-two dimensions are fully covered.
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
    /// A layer of cells within a horizontal leaf, represented by one pressure degree of freedom.
    Regular,
    /// A continuous deep water-column segment, linearly interpolated from upper and lower pressure degrees of freedom.
    Tall,
}

#[derive(Clone, Debug)]
pub struct AdaptiveCell {
    /// Bounding box in finest-grid coordinates; max is exclusive.
    pub min: [usize; 3],
    pub max: [usize; 3],
    pub kind: CellKind,
    pub pressure_indices: [usize; 2],
    pub pressure_count: usize,
}

/// Prolongation row from each fine-grid center to the compressed pressure vector.
/// Both weights are nonnegative and sum to one in liquid cells; both are zero elsewhere.
#[derive(Clone, Copy, Debug, Default)]
pub struct PressureBasis {
    pub indices: [usize; 2],
    pub weights: [f64; 2],
}

#[derive(Clone, Debug)]
pub struct AdaptiveGrid {
    /// Sum of six-face reconstruction errors, one per finest-grid cell, in m/s.
    pub local_cost: Vec<f64>,
    /// Summed along the water column and divided by Ny; indexed by x + Nx*z, in m/s.
    pub integrated_cost: Vec<f64>,
    /// Fine region after L1 dilation and height smoothing, including air cells covered by dilation.
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
    /// Build CPU horizontal topology using only previously computed local metrics; the GPU path does not recompute them here.
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

    /// Arrange topology after GPU readback. Only builds cells and pressure bases, without metric or optical-layer calculations.
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

    /// Callers should first validate inputs with Grid::validate and AdaptivityConfig::validate.
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
                        // The paper's bounds may be reversed in small test domains. Raise the
                        // upper bound to retain the minimum fine layer and cap the minimum
                        // distance at the domain height to avoid an invalid clamp.
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
        // Map the optical layer from the fine grid to horizontal leaves conservatively:
        // retain the entire layer at a height if any cell in the leaf needs fine representation,
        // rather than using the paper's majority criterion. This avoids losing thin fine
        // regions to coarsening, at the cost of potentially more pressure unknowns.
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

    /// Fully refined pressure basis for reference validation and fallback when compressed projection error is too large.
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
                    // On a uniform grid, equivalent to interpolating between the physical heights of the endpoint cell centers.
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

/// Fit the four horizontal velocity sample columns on the water-column sides separately, avoiding lateral velocity differences in the vertical fit.
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
            // Keep complex topology at the finest horizontal resolution and never merge across discontinuous liquid columns vertically.
            split |= runs.len() > 1
                || runs.iter().any(|r| !r.grounded)
                || runs.len() != reference.len()
                || runs
                    .iter()
                    .zip(reference)
                    .any(|(a, b)| a.start != b.start || a.end != b.end || a.grounded != b.grounded);
            // Even identical liquid-column intervals may touch solid on one side and air on the other; preserve this boundary distinction.
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

/// Propagate maximum remaining radius, equivalent to the union of variable-radius seed L1 balls, without traversing a cube per seed.
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
        // Do not apply closed-wall conditions; this tests an analytically reconstructible velocity field.
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
        // A solid shelf with water above and below; another column has an air gap in the middle.
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
        // Each x-face column [0,3,0] reconstructs to [1,1,1] by least squares; each side contributes once.
        for (y, expected) in [2.0, 4.0, 2.0].into_iter().enumerate() {
            assert!((adaptive.local_cost[grid.domain.index([0, y, 0])] - expected).abs() < 1e-12);
        }
        // Divide by the entire domain's 8 layers, not the liquid column's 3 layers.
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

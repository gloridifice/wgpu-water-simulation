//! Render-only terrain eligibility and connectivity. Never writes solver/collision terrain.
use crate::terrain::Terrain;
use common::gpu::{Gpu, buffer};

/// A jump larger than half a metre belongs to another surface level.
const MAX_STEP: f32 = 0.5;
/// Rise/run = 1 corresponds to 45 degrees; see height_fog.wgsl surface gate.
const MAX_SLOPE: f32 = 1.0;

pub(super) struct RenderTerrain {
    pub bed: wgpu::Buffer,
    pub masks: wgpu::Buffer,
}
impl RenderTerrain {
    pub fn new(gpu: &Gpu, terrain: &Terrain) -> Self {
        let filtered = filter(terrain);
        let bed: Vec<_> = filtered
            .heights
            .iter()
            .zip(&filtered.valid)
            .map(|(&height, &valid)| [height, f32::from(valid), 0.0, 0.0])
            .collect();
        let masks: Vec<_> = filtered
            .pass_x
            .iter()
            .chain(&filtered.pass_z)
            .map(|&v| u32::from(v))
            .collect();
        Self {
            bed: buffer(
                &gpu.device,
                "fog render-only terrain eligibility",
                &bed,
                wgpu::BufferUsages::STORAGE,
            ),
            masks: buffer(
                &gpu.device,
                "fog render-only level connections",
                &masks,
                wgpu::BufferUsages::STORAGE,
            ),
        }
    }
}
fn filter(terrain: &Terrain) -> Terrain {
    let mut result = terrain.clone();
    let [nx, nz] = terrain.metadata.resolution.map(|v| v as usize);
    let spacing = terrain.spacing();
    let connected = |a: usize, b: usize| {
        terrain.valid[a] == 1
            && terrain.valid[b] == 1
            && (terrain.heights[a] - terrain.heights[b]).abs() <= MAX_STEP
    };
    // Cut level jumps before differentiating: a flat ledge next to a cliff is
    // still flat, and must not be averaged into the floor at the cliff's foot.
    for z in 0..nz {
        for x in 1..nx {
            let i = z * nx + x;
            if !connected(i - 1, i) {
                result.pass_x[z * (nx + 1) + x] = 0;
            }
        }
    }
    for z in 1..nz {
        for x in 0..nx {
            let i = z * nx + x;
            if !connected(i - nx, i) {
                result.pass_z[z * nx + x] = 0;
            }
        }
    }
    for z in 0..nz {
        for x in 0..nx {
            let i = z * nx + x;
            // Retain the smaller opposing raw derivative as a coarse-grid
            // fallback. A cliff edge has a flat neighbour (zero), whereas a
            // steep ramp can have *every* edge cut by MAX_STEP and must not
            // masquerade as isolated flat cells. At the domain edge use the
            // available one-sided derivative; void cells contribute nothing.
            let raw_slope = |neighbors: [Option<usize>; 2], step: f32| {
                neighbors
                    .into_iter()
                    .flatten()
                    .filter(|&j| terrain.valid[j] == 1)
                    .map(|j| (terrain.heights[i] - terrain.heights[j]).abs() / step)
                    .reduce(f32::min)
                    .unwrap_or(0.0)
            };
            let mut dx = raw_slope(
                [
                    x.checked_sub(1).map(|_| i - 1),
                    (x + 1 < nx).then_some(i + 1),
                ],
                spacing.x,
            );
            let mut dz = raw_slope(
                [
                    z.checked_sub(1).map(|_| i - nx),
                    (z + 1 < nz).then_some(i + nx),
                ],
                spacing.y,
            );
            if x > 0 && result.pass_x[z * (nx + 1) + x] != 0 {
                dx = dx.max((terrain.heights[i] - terrain.heights[i - 1]).abs() / spacing.x);
            }
            if x + 1 < nx && result.pass_x[z * (nx + 1) + x + 1] != 0 {
                dx = dx.max((terrain.heights[i + 1] - terrain.heights[i]).abs() / spacing.x);
            }
            if z > 0 && result.pass_z[z * nx + x] != 0 {
                dz = dz.max((terrain.heights[i] - terrain.heights[i - nx]).abs() / spacing.y);
            }
            if z + 1 < nz && result.pass_z[(z + 1) * nx + x] != 0 {
                dz = dz.max((terrain.heights[i + nx] - terrain.heights[i]).abs() / spacing.y);
            }
            // One-sided maxima avoid a central-difference cancellation at ridges.
            if dx.hypot(dz) > MAX_SLOPE {
                result.valid[i] = 0;
            }
        }
    }
    // Close all four faces of excluded cells only after the slope pass, so the
    // result does not depend on traversal order. All reconstruction kernels and
    // the prefilter then obey the same disconnected support.
    for z in 0..nz {
        for x in 0..nx {
            if result.valid[z * nx + x] == 0 {
                result.pass_x[z * (nx + 1) + x] = 0;
                result.pass_x[z * (nx + 1) + x + 1] = 0;
                result.pass_z[z * nx + x] = 0;
                result.pass_z[(z + 1) * nx + x] = 0;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{BakeSource, Terrain};
    #[test]
    fn separates_levels_without_eroding_flat_ledges_and_rejects_steep_ramps() {
        let mut terrain =
            Terrain::bake(&BakeSource::playground(), [-1.0, -2.0, 2.0, 4.0], [8, 8]).unwrap();
        for z in 0..8 {
            for x in 0..8 {
                terrain.heights[z * 8 + x] = if x < 4 { 0.0 } else { 2.0 };
            }
        }
        let filtered = filter(&terrain);
        assert!(
            filtered.valid.iter().all(|&v| v == 1),
            "flat ledges must survive a disconnected cliff"
        );
        for z in 0..8 {
            assert_eq!(filtered.pass_x[z * 9 + 4], 0);
        }
        assert!(
            terrain.pass_x.iter().all(|&v| v == 1),
            "render filtering must not change simulation/collision connectivity"
        );
        // Non-square 0.25 x 0.5 m cells: 0.3 m per X cell is steep; the same
        // rise per Z cell is gentle. A diagonal 0.8/0.8 gradient is also steep.
        for (width, sx, sz, visible) in [
            (2.0, 1.2, 0.0, false),
            (2.0, 0.0, 0.6, true),
            (2.0, 0.8, 0.8, false),
            (2.0, 1.0, 0.0, true),
            (16.0, 1.2, 0.0, false),
        ] {
            terrain.metadata.domain[2] = width;
            let spacing = terrain.spacing();
            for z in 0..8 {
                for x in 0..8 {
                    terrain.heights[z * 8 + x] =
                        x as f32 * spacing.x * sx + z as f32 * spacing.y * sz;
                }
            }
            let filtered = filter(&terrain);
            assert!(
                filtered.valid.iter().all(|&v| (v == 1) == visible),
                "slope {sx}/{sz}"
            );
            if !visible {
                assert!(
                    filtered
                        .pass_x
                        .iter()
                        .chain(&filtered.pass_z)
                        .all(|&v| v == 0)
                );
            }
        }
    }
}

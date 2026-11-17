use super::{CellType, Grid, Particle};

/// Return the four bilinear neighbors and weights on the given staggered grid.
fn stencil(
    x: f64,
    y: f64,
    h: f64,
    width: usize,
    height: usize,
    ox: f64,
    oy: f64,
) -> [(usize, f64); 4] {
    let gx = (x / h - ox).clamp(0.0, (width - 1) as f64 - 1e-8);
    let gy = (y / h - oy).clamp(0.0, (height - 1) as f64 - 1e-8);
    let i = gx.floor() as usize;
    let j = gy.floor() as usize;
    let tx = gx - i as f64;
    let ty = gy - j as f64;
    [
        (j * width + i, (1.0 - tx) * (1.0 - ty)),
        (j * width + i + 1, tx * (1.0 - ty)),
        ((j + 1) * width + i, (1.0 - tx) * ty),
        ((j + 1) * width + i + 1, tx * ty),
    ]
}

pub(crate) fn particles_to_grid(particles: &[Particle], grid: &mut Grid) {
    grid.u.fill(0.0);
    grid.v.fill(0.0);
    grid.u_weight.fill(0.0);
    grid.v_weight.fill(0.0);
    for p in particles {
        for (k, weight) in stencil(p.x, p.y, grid.h, grid.nx + 1, grid.ny, 0.0, 0.5) {
            grid.u[k] += weight * p.u;
            grid.u_weight[k] += weight;
        }
        for (k, weight) in stencil(p.x, p.y, grid.h, grid.nx, grid.ny + 1, 0.5, 0.0) {
            grid.v[k] += weight * p.v;
            grid.v_weight[k] += weight;
        }
    }
    for (value, weight) in grid.u.iter_mut().zip(&grid.u_weight) {
        if *weight > 0.0 {
            *value /= weight;
        }
    }
    for (value, weight) in grid.v.iter_mut().zip(&grid.v_weight) {
        if *weight > 0.0 {
            *value /= weight;
        }
    }
}

pub(crate) fn extrapolate(grid: &mut Grid, layers: usize) {
    for j in 0..grid.ny {
        for i in 0..=grid.nx {
            let k = grid.ui(i, j);
            grid.u_valid[k] = grid.at(i as isize - 1, j as isize) != CellType::Air
                || grid.at(i as isize, j as isize) != CellType::Air;
        }
    }
    for j in 0..=grid.ny {
        for i in 0..grid.nx {
            let k = grid.vi(i, j);
            grid.v_valid[k] = grid.at(i as isize, j as isize - 1) != CellType::Air
                || grid.at(i as isize, j as isize) != CellType::Air;
        }
    }
    extend(&mut grid.u, &mut grid.u_valid, grid.nx + 1, grid.ny, layers);
    extend(&mut grid.v, &mut grid.v_valid, grid.nx, grid.ny + 1, layers);
}

fn extend(values: &mut [f64], valid: &mut [bool], width: usize, height: usize, layers: usize) {
    for _ in 0..layers {
        let old = valid.to_vec();
        for j in 1..height - 1 {
            for i in 1..width - 1 {
                let k = j * width + i;
                if old[k] {
                    continue;
                }
                let mut sum = 0.0;
                let mut count = 0;
                for n in [k - 1, k + 1, k - width, k + width] {
                    if old[n] {
                        sum += values[n];
                        count += 1;
                    }
                }
                if count > 0 {
                    values[k] = sum / count as f64;
                    valid[k] = true;
                }
            }
        }
    }
}

fn sample(values: &[f64], valid: &[bool], nodes: [(usize, f64); 4]) -> f64 {
    let mut sum = 0.0;
    let mut weights = 0.0;
    for (k, weight) in nodes {
        if valid[k] {
            sum += weight * values[k];
            weights += weight;
        }
    }
    if weights > 1e-12 { sum / weights } else { 0.0 }
}

fn velocity(grid: &Grid, x: f64, y: f64) -> (f64, f64) {
    (
        sample(
            &grid.u,
            &grid.u_valid,
            stencil(x, y, grid.h, grid.nx + 1, grid.ny, 0.0, 0.5),
        ),
        sample(
            &grid.v,
            &grid.v_valid,
            stencil(x, y, grid.h, grid.nx, grid.ny + 1, 0.5, 0.0),
        ),
    )
}

/// Pure PIC: interpolate the new grid velocity directly. A future FLIP scheme could interpolate velocity changes here.
pub(crate) fn grid_to_particles(
    particles: &mut [Particle],
    grid: &Grid,
    dt: f64,
    paddle_x: f64,
    paddle_velocity: f64,
) {
    let radius = 0.2 * grid.h;
    for p in particles {
        let (u0, v0) = velocity(grid, p.x, p.y);
        let (u, v) = velocity(grid, p.x + 0.5 * dt * u0, p.y + 0.5 * dt * v0);
        let old_x = p.x;
        let old_y = p.y;
        p.u = u;
        p.v = v;
        p.x += dt * u;
        p.y += dt * v;
        if p.x < paddle_x + radius {
            p.x = paddle_x + radius;
            p.u = p.u.max(paddle_velocity);
        }
        let right = (grid.nx - 1) as f64 * grid.h - radius;
        if p.x > right {
            p.x = right;
            p.u = p.u.min(0.0);
        }
        let roof = (grid.ny - 1) as f64 * grid.h - radius;
        if p.y > roof {
            p.y = roof;
            p.v = p.v.min(0.0);
        }
        let bed = grid.bed_height(p.x) + radius;
        if p.y < bed {
            if old_y >= bed || grid.bed_height(old_x) == grid.bed_height(p.x) {
                p.y = bed;
                p.v = p.v.max(0.0);
            } else {
                // Revert on collision with a vertical stair face rather than lifting the particle onto the step.
                p.x = old_x;
                p.u = 0.0;
                p.y = p.y.max(grid.bed_height(p.x) + radius);
                p.v = p.v.max(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pic::Config;

    #[test]
    fn uniform_velocity_survives_a_pic_round_trip() {
        let config = Config {
            nx: 32,
            ny: 20,
            ..Config::default()
        };
        let mut grid = Grid::new(config);
        let mut particles = Vec::new();
        for j in 3..10 {
            for i in 3..15 {
                particles.push(Particle {
                    x: (i as f64 + 0.3) * grid.h,
                    y: (j as f64 + 0.4) * grid.h,
                    u: 1.25,
                    v: -0.75,
                });
            }
        }
        grid.mark_solids(2.0 * grid.h);
        grid.mark_fluid(&particles);
        particles_to_grid(&particles, &mut grid);
        // Test transfer alone; stationary solid walls are incompatible with uniform flow, so omit boundary extrapolation.
        for (valid, weight) in grid.u_valid.iter_mut().zip(&grid.u_weight) {
            *valid = *weight > 0.0;
        }
        for (valid, weight) in grid.v_valid.iter_mut().zip(&grid.v_weight) {
            *valid = *weight > 0.0;
        }
        for p in &particles {
            let (u, v) = velocity(&grid, p.x, p.y);
            assert!((u - p.u).abs() < 1e-12 && (v - p.v).abs() < 1e-12);
        }
    }
}

use super::{CellType, Grid};

/// 解 A p = -h/dt * (uR-uL+vT-vB)，再做 u -= dt * grad(p)。
/// 密度约去为 1。空气 p=0；固体面已给定法向速度，不进入梯度修正。
/// Jacobi 预条件共轭梯度，矩阵只通过四邻接作用计算，不存稠密矩阵。
pub(crate) fn project(grid: &mut Grid, dt: f64, limit: usize) -> usize {
    let size = grid.kind.len();
    let mut diagonal = vec![0.0; size];
    let mut residual = vec![0.0; size];
    grid.pressure.fill(0.0);
    for j in 1..grid.ny - 1 {
        for i in 1..grid.nx - 1 {
            let k = grid.cell(i, j);
            if grid.kind[k] != CellType::Fluid {
                continue;
            }
            diagonal[k] = neighbors(grid, i, j)
                .iter()
                .filter(|&&n| grid.kind[n] != CellType::Solid)
                .count() as f64;
            residual[k] = -grid.h / dt
                * (grid.u[grid.ui(i + 1, j)] - grid.u[grid.ui(i, j)] + grid.v[grid.vi(i, j + 1)]
                    - grid.v[grid.vi(i, j)]);
        }
    }
    let tolerance = (dot(&residual, &residual) * 1e-10).max(1e-20);
    let mut direction: Vec<_> = residual
        .iter()
        .zip(&diagonal)
        .map(|(r, d)| if *d > 0.0 { r / d } else { 0.0 })
        .collect();
    let mut rho = dot(&residual, &direction);
    let mut product = vec![0.0; size];
    let mut used = 0;
    for iteration in 0..limit {
        if dot(&residual, &residual) <= tolerance {
            break;
        }
        apply(grid, &diagonal, &direction, &mut product);
        let denominator = dot(&direction, &product);
        if denominator <= 1e-30 || rho <= 1e-30 {
            break;
        }
        let alpha = rho / denominator;
        for k in 0..size {
            grid.pressure[k] += alpha * direction[k];
            residual[k] -= alpha * product[k];
        }
        let new_rho: f64 = residual
            .iter()
            .zip(&diagonal)
            .map(|(r, d)| if *d > 0.0 { r * r / d } else { 0.0 })
            .sum();
        let beta = new_rho / rho;
        for k in 0..size {
            let z = if diagonal[k] > 0.0 {
                residual[k] / diagonal[k]
            } else {
                0.0
            };
            direction[k] = z + beta * direction[k];
        }
        rho = new_rho;
        used = iteration + 1;
    }
    for j in 0..grid.ny {
        for i in 1..grid.nx {
            let l = grid.cell(i - 1, j);
            let r = grid.cell(i, j);
            if grid.kind[l] != CellType::Solid && grid.kind[r] != CellType::Solid {
                let k = grid.ui(i, j);
                grid.u[k] -= dt / grid.h * (grid.pressure[r] - grid.pressure[l]);
            }
        }
    }
    for j in 1..grid.ny {
        for i in 0..grid.nx {
            let b = grid.cell(i, j - 1);
            let t = grid.cell(i, j);
            if grid.kind[b] != CellType::Solid && grid.kind[t] != CellType::Solid {
                let k = grid.vi(i, j);
                grid.v[k] -= dt / grid.h * (grid.pressure[t] - grid.pressure[b]);
            }
        }
    }
    used
}

fn neighbors(grid: &Grid, i: usize, j: usize) -> [usize; 4] {
    [
        grid.cell(i - 1, j),
        grid.cell(i + 1, j),
        grid.cell(i, j - 1),
        grid.cell(i, j + 1),
    ]
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

fn apply(grid: &Grid, diagonal: &[f64], input: &[f64], output: &mut [f64]) {
    output.fill(0.0);
    for j in 1..grid.ny - 1 {
        for i in 1..grid.nx - 1 {
            let k = grid.cell(i, j);
            if grid.kind[k] == CellType::Fluid {
                output[k] = diagonal[k] * input[k];
                for n in neighbors(grid, i, j) {
                    if grid.kind[n] == CellType::Fluid {
                        output[k] -= input[n];
                    }
                }
            }
        }
    }
}

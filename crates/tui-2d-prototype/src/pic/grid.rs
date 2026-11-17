use super::{Config, Particle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellType {
    Air,
    Fluid,
    Solid,
}

/// MAC grid: u on vertical faces, v on horizontal faces, pressure and cell type at cell centers.
pub struct Grid {
    pub nx: usize,
    pub ny: usize,
    pub h: f64,
    pub u: Vec<f64>,
    pub v: Vec<f64>,
    pub pressure: Vec<f64>,
    pub kind: Vec<CellType>,
    pub(crate) u_weight: Vec<f64>,
    pub(crate) v_weight: Vec<f64>,
    pub(crate) u_valid: Vec<bool>,
    pub(crate) v_valid: Vec<bool>,
    pub(crate) bed_cells: Vec<usize>,
    pub(crate) left_column: usize,
}

impl Grid {
    pub(crate) fn new(config: Config) -> Self {
        let bed_cells = (0..config.nx)
            .map(|i| {
                let fraction = (i as f64 + 0.5) / config.nx as f64;
                let rise = if config.beach {
                    ((fraction - 0.48) / 0.47).clamp(0.0, 1.0) * 0.56 * config.ny as f64
                } else {
                    0.0
                };
                1 + rise.floor() as usize
            })
            .collect();
        let u_size = (config.nx + 1) * config.ny;
        let v_size = config.nx * (config.ny + 1);
        Self {
            nx: config.nx,
            ny: config.ny,
            h: config.cell_size,
            u: vec![0.0; u_size],
            v: vec![0.0; v_size],
            pressure: vec![0.0; config.nx * config.ny],
            kind: vec![CellType::Air; config.nx * config.ny],
            u_weight: vec![0.0; u_size],
            v_weight: vec![0.0; v_size],
            u_valid: vec![false; u_size],
            v_valid: vec![false; v_size],
            bed_cells,
            left_column: 2,
        }
    }
    pub fn cell(&self, i: usize, j: usize) -> usize {
        j * self.nx + i
    }
    pub fn ui(&self, i: usize, j: usize) -> usize {
        j * (self.nx + 1) + i
    }
    pub fn vi(&self, i: usize, j: usize) -> usize {
        j * self.nx + i
    }
    pub fn bed_height(&self, x: f64) -> f64 {
        self.bed_cells[((x / self.h).floor() as usize).min(self.nx - 1)] as f64 * self.h
    }
    pub fn at(&self, i: isize, j: isize) -> CellType {
        if i < 0 || j < 0 || i >= self.nx as isize || j >= self.ny as isize {
            CellType::Solid
        } else {
            self.kind[self.cell(i as usize, j as usize)]
        }
    }
    pub(crate) fn mark_solids(&mut self, paddle_x: f64) {
        self.left_column = ((paddle_x / self.h).floor() as usize).min(self.nx - 3);
        for j in 0..self.ny {
            for i in 0..self.nx {
                let k = self.cell(i, j);
                self.kind[k] = if i < self.left_column
                    || i == self.nx - 1
                    || j == self.ny - 1
                    || j < self.bed_cells[i]
                {
                    CellType::Solid
                } else {
                    CellType::Air
                };
            }
        }
    }
    pub(crate) fn mark_fluid(&mut self, particles: &[Particle]) {
        for p in particles {
            let i = (p.x / self.h).floor() as usize;
            let j = (p.y / self.h).floor() as usize;
            if i < self.nx && j < self.ny {
                let k = self.cell(i, j);
                if self.kind[k] != CellType::Solid {
                    self.kind[k] = CellType::Fluid;
                }
            }
        }
    }
    pub(crate) fn add_gravity(&mut self, gravity: f64, dt: f64) {
        for j in 1..self.ny {
            for i in 0..self.nx {
                if self.at(i as isize, j as isize - 1) == CellType::Fluid
                    || self.at(i as isize, j as isize) == CellType::Fluid
                {
                    let k = self.vi(i, j);
                    self.v[k] -= gravity * dt;
                }
            }
        }
    }
    pub(crate) fn enforce_boundaries(&mut self, paddle_velocity: f64) {
        for j in 0..self.ny {
            for i in 0..=self.nx {
                let l = self.at(i as isize - 1, j as isize);
                let r = self.at(i as isize, j as isize);
                if l == CellType::Solid || r == CellType::Solid {
                    let k = self.ui(i, j);
                    self.u[k] = if i == self.left_column && r != CellType::Solid {
                        paddle_velocity
                    } else {
                        0.0
                    };
                }
            }
        }
        for j in 0..=self.ny {
            for i in 0..self.nx {
                if self.at(i as isize, j as isize - 1) == CellType::Solid
                    || self.at(i as isize, j as isize) == CellType::Solid
                {
                    let k = self.vi(i, j);
                    self.v[k] = 0.0;
                }
            }
        }
    }
    pub fn divergence_rms(&self) -> f64 {
        let mut sum = 0.0;
        let mut count = 0;
        for j in 1..self.ny - 1 {
            for i in 1..self.nx - 1 {
                if self.kind[self.cell(i, j)] == CellType::Fluid {
                    let divergence = (self.u[self.ui(i + 1, j)] - self.u[self.ui(i, j)]
                        + self.v[self.vi(i, j + 1)]
                        - self.v[self.vi(i, j)])
                        / self.h;
                    sum += divergence * divergence;
                    count += 1;
                }
            }
        }
        (sum / (count.max(1) as f64)).sqrt()
    }
}

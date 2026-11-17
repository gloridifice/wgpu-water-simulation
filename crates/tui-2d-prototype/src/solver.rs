//! One-dimensional, flat-bottomed, fully wet tank: finite volumes, Rusanov flux and explicit Euler integration.
//! A `Cell` stores cell-averaged conserved quantities, not a particle moving with the water.

/// Q = (h, hu). Density is constant; hu is momentum per unit density and width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub h: f64,
    pub hu: f64,
}

impl Cell {
    /// Only valid for fully wet states with h > 0.
    pub fn velocity(self) -> f64 {
        self.hu / self.h
    }

    /// The characteristic speeds are u ± sqrt(g h); the maximum absolute speed is |u| + sqrt(g h).
    pub fn wave_speed(self, gravity: f64) -> f64 {
        self.velocity().abs() + (gravity * self.h).sqrt()
    }

    /// F(Q) = (hu, hu² / h + g h² / 2). Here hu² means (hu)².
    pub fn physical_flux(self, gravity: f64) -> Flux {
        Flux {
            mass: self.hu,
            momentum: self.hu * self.velocity() + 0.5 * gravity * self.h * self.h,
        }
    }

    /// Ghost state outside the wall: unchanged depth, reversed normal momentum.
    pub fn reflected(self) -> Self {
        Self {
            h: self.h,
            hu: -self.hu,
        }
    }
}

/// Positive flux crosses the interface in the positive x direction; it is not a new cell state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flux {
    pub mass: f64,
    pub momentum: f64,
}

/// F_hat = (F_L + F_R)/2 - alpha (Q_R - Q_L)/2。
/// Requires h > 0 on both sides, finite states and gravity > 0; Simulation checks these conditions.
pub fn rusanov_flux(left: Cell, right: Cell, gravity: f64) -> Flux {
    let fl = left.physical_flux(gravity);
    let fr = right.physical_flux(gravity);
    let alpha = left.wave_speed(gravity).max(right.wave_speed(gravity));
    Flux {
        mass: 0.5 * (fl.mass + fr.mass) - 0.5 * alpha * (right.h - left.h),
        momentum: 0.5 * (fl.momentum + fr.momentum) - 0.5 * alpha * (right.hu - left.hu),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub cells: usize,
    pub length: f64,
    pub gravity: f64,
    pub cfl: f64,
    pub rest_depth: f64,
    pub bump_height: f64,
    /// Standard deviation of the Gaussian bump, in meters.
    pub bump_width: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            cells: 201,
            length: 20.0,
            gravity: 9.81,
            cfl: 0.45,
            rest_depth: 1.0,
            bump_height: 0.2,
            bump_width: 0.8,
        }
    }
}

impl Config {
    fn validate(self) -> Result<(), String> {
        if self.cells < 3 {
            return Err("cells must be at least 3".into());
        }
        for (name, value) in [
            ("length", self.length),
            ("gravity", self.gravity),
            ("rest_depth", self.rest_depth),
            ("bump_width", self.bump_width),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and positive"));
            }
        }
        if !self.cfl.is_finite() || self.cfl <= 0.0 || self.cfl > 0.5 {
            return Err("cfl must be in (0, 0.5] for this solver".into());
        }
        if !self.bump_height.is_finite() || self.bump_height < 0.0 {
            return Err("bump_height must be finite and nonnegative".into());
        }
        let dx = self.length / self.cells as f64;
        if !dx.is_finite() || dx <= 0.0 {
            return Err("cell width must be finite and positive".into());
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Simulation {
    config: Config,
    dx: f64,
    current: Vec<Cell>,
    next: Vec<Cell>,
    /// N cells have N+1 interfaces; adjacent cells share the same flux.
    interfaces: Vec<Flux>,
    time: f64,
    steps: usize,
    last_dt: f64,
}

impl Simulation {
    pub fn new(config: Config) -> Result<Self, String> {
        config.validate()?;
        let dx = config.length / config.cells as f64;
        let middle = (config.cells - 1) as f64 * 0.5;
        let cells = (0..config.cells)
            .map(|i| {
                // Approximate initial cell averages by center samples, preserving mirror symmetry.
                let distance = (i as f64 - middle) * dx / config.bump_width;
                Cell {
                    h: config.rest_depth + config.bump_height * (-0.5 * distance * distance).exp(),
                    hu: 0.0,
                }
            })
            .collect();
        Self::from_cells(config, cells)
    }

    /// Custom fully wet initial state for checking still water, symmetry and steeper profiles.
    pub fn from_cells(config: Config, cells: Vec<Cell>) -> Result<Self, String> {
        config.validate()?;
        if cells.len() != config.cells {
            return Err("initial state length must equal config.cells".into());
        }
        for (i, cell) in cells.iter().enumerate() {
            validate_cell(*cell, config.gravity, i)?;
        }
        Ok(Self {
            dx: config.length / config.cells as f64,
            next: cells.clone(),
            current: cells,
            interfaces: vec![Flux::default(); config.cells + 1],
            config,
            time: 0.0,
            steps: 0,
            last_dt: 0.0,
        })
    }

    pub fn cells(&self) -> &[Cell] {
        &self.current
    }
    pub fn config(&self) -> Config {
        self.config
    }
    pub fn dx(&self) -> f64 {
        self.dx
    }
    pub fn time(&self) -> f64 {
        self.time
    }
    pub fn steps(&self) -> usize {
        self.steps
    }
    pub fn last_dt(&self) -> f64 {
        self.last_dt
    }

    /// Water volume per unit width (also mass at density 1), excluding ghost cells.
    pub fn mass(&self) -> f64 {
        self.current.iter().map(|q| q.h).sum::<f64>() * self.dx
    }

    pub fn max_wave_speed(&self) -> f64 {
        self.current
            .iter()
            .map(|q| q.wave_speed(self.config.gravity))
            .fold(0.0, f64::max)
    }

    /// Re-estimate dt <= CFL * dx / max(|u| + sqrt(g h)) on every step.
    pub fn stable_dt(&self) -> f64 {
        self.config.cfl * self.dx / self.max_wave_speed()
    }

    /// Redistribute water near x=center to create a wave packet traveling in both directions.
    /// The discrete sums of the narrow and wide Gaussians cancel, adding no water; hu and time are unchanged.
    /// Returns the actual amplitude. In shallow water the disturbance is scaled to retain at least 75% of the original depth.
    pub fn add_disturbance(
        &mut self,
        center: f64,
        amplitude: f64,
        width: f64,
    ) -> Result<f64, String> {
        if !center.is_finite()
            || !(0.0..=self.config.length).contains(&center)
            || !amplitude.is_finite()
            || amplitude <= 0.0
            || !width.is_finite()
            || width <= 0.0
        {
            return Err(
                "disturbance needs x in the tank and finite positive amplitude/width".into(),
            );
        }
        let weights: Vec<_> = (0..self.current.len())
            .map(|i| {
                let distance = ((i as f64 + 0.5) * self.dx - center) / width;
                (
                    (-0.5 * distance * distance).exp(),
                    (-0.125 * distance * distance).exp(),
                )
            })
            .collect();
        let narrow_sum: f64 = weights.iter().map(|(narrow, _)| narrow).sum();
        let broad_sum: f64 = weights.iter().map(|(_, broad)| broad).sum();
        if broad_sum == 0.0 {
            return Err("disturbance width is too small for this grid".into());
        }
        // Normalize using discrete sums on the actual grid, conserving mass even when the packet is truncated by a wall.
        let ratio = narrow_sum / broad_sum;
        let shape: Vec<_> = weights
            .iter()
            .map(|(narrow, broad)| narrow - ratio * broad)
            .collect();
        let peak = shape.iter().copied().fold(0.0, f64::max);
        if peak <= f64::EPSILON {
            return Err("disturbance width cannot resolve a local ripple on this grid".into());
        }
        let mut applied = amplitude;
        for (q, weight) in self.current.iter().zip(&shape) {
            if *weight < 0.0 {
                applied = applied.min(0.25 * q.h * peak / -weight);
            }
        }
        for (i, (q, weight)) in self.current.iter().zip(&shape).enumerate() {
            self.next[i] = Cell {
                h: q.h + applied * (weight / peak),
                hu: q.hu,
            };
            validate_cell(self.next[i], self.config.gravity, i)?;
        }
        std::mem::swap(&mut self.current, &mut self.next);
        Ok(applied)
    }

    /// Advance one step; max_dt is usually the time remaining until the next output.
    /// Invalid results return an error without changing current or time; depth is not clipped to hide errors.
    pub fn step(&mut self, max_dt: f64) -> Result<f64, String> {
        if !max_dt.is_finite() || max_dt <= 0.0 {
            return Err("max_dt must be finite and positive".into());
        }
        let dt = self.stable_dt().min(max_dt);
        let new_time = self.time + dt;
        if !dt.is_finite() || dt <= 0.0 || !new_time.is_finite() || new_time <= self.time {
            return Err("time step cannot advance time; check the state and scale".into());
        }

        let n = self.current.len();
        let g = self.config.gravity;
        // The left boundary order is (outside, inside); the right is (inside, outside).
        self.interfaces[0] = rusanov_flux(self.current[0].reflected(), self.current[0], g);
        for i in 1..n {
            self.interfaces[i] = rusanov_flux(self.current[i - 1], self.current[i], g);
        }
        self.interfaces[n] = rusanov_flux(self.current[n - 1], self.current[n - 1].reflected(), g);

        let dt_over_dx = dt / self.dx;
        for i in 0..n {
            let q = self.current[i];
            let left = self.interfaces[i];
            let right = self.interfaces[i + 1];
            // Q_i^{n+1} = Q_i^n - dt/dx * (F_{i+1/2} - F_{i-1/2})。
            self.next[i] = Cell {
                h: q.h - dt_over_dx * (right.mass - left.mass),
                hu: q.hu - dt_over_dx * (right.momentum - left.momentum),
            };
            validate_cell(self.next[i], g, i)?;
        }
        // Read only current and write only next throughout; swap the Vecs afterward without copying the state.
        std::mem::swap(&mut self.current, &mut self.next);
        self.time = new_time;
        self.steps += 1;
        self.last_dt = dt;
        Ok(dt)
    }

    /// Output intervals are independent of physical dt; the last substep aligns exactly with the target time.
    pub fn advance_to(&mut self, target: f64) -> Result<(), String> {
        if !target.is_finite() || target < self.time {
            return Err("target time must be finite and not earlier than current time".into());
        }
        while self.time < target {
            let remaining = target - self.time;
            let dt = self.step(remaining)?;
            if dt == remaining {
                self.time = target;
            }
        }
        Ok(())
    }
}

fn validate_cell(cell: Cell, gravity: f64, index: usize) -> Result<(), String> {
    let flux = cell.physical_flux(gravity);
    if cell.h <= 0.0
        || !cell.h.is_finite()
        || !cell.hu.is_finite()
        || !cell.wave_speed(gravity).is_finite()
        || !flux.momentum.is_finite()
    {
        return Err(format!(
            "cell {index}: expected finite wet state (h > 0), got {cell:?}"
        ));
    }
    Ok(())
}

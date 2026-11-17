//! Two-dimensional side-view, single-phase incompressible PIC: particles carry motion and a MAC grid solves pressure.
//! Air has zero pressure and no modeled inertia; dimensions use meters and seconds.
mod grid;
mod pressure;
mod transfer;

pub use grid::{CellType, Grid};

#[derive(Clone, Copy, Debug)]
pub struct Particle {
    pub x: f64,
    pub y: f64,
    pub u: f64,
    pub v: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub nx: usize,
    pub ny: usize,
    pub cell_size: f64,
    pub gravity: f64,
    pub cfl: f64,
    pub pressure_iterations: usize,
    /// Full paddle cycle: forward motion takes 1/4 and return takes 3/4.
    pub wave_period: f64,
    pub wave_stroke: f64,
    pub beach: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            nx: 112,
            ny: 64,
            cell_size: 0.1,
            gravity: 9.81,
            cfl: 0.4,
            pressure_iterations: 100,
            wave_period: 3.0,
            wave_stroke: 1.2,
            beach: true,
        }
    }
}

impl Config {
    pub fn width(self) -> f64 {
        self.nx as f64 * self.cell_size
    }
    pub fn height(self) -> f64 {
        self.ny as f64 * self.cell_size
    }
    pub fn water_level(self) -> f64 {
        (self.ny as f64 * 0.4).floor() * self.cell_size
    }
    pub fn validate(self) -> Result<(), String> {
        if !(24..=320).contains(&self.nx) || !(16..=192).contains(&self.ny) {
            return Err("PIC grid needs nx in 24..=320 and ny in 16..=192".into());
        }
        for (name, value) in [
            ("cell_size", self.cell_size),
            ("gravity", self.gravity),
            ("wave_period", self.wave_period),
            ("wave_stroke", self.wave_stroke),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and positive"));
            }
        }
        if !self.cfl.is_finite() || self.cfl <= 0.0 || self.cfl > 0.5 {
            return Err("PIC cfl must be in (0, 0.5]".into());
        }
        if !(10..=1000).contains(&self.pressure_iterations)
            || self.wave_stroke > 0.25 * self.width()
        {
            return Err(
                "invalid pressure iterations or paddle stroke (must be <= tank width / 4)".into(),
            );
        }
        if !self.width().is_finite()
            || !self.height().is_finite()
            || !(self.gravity * self.cell_size).is_finite()
        {
            return Err("PIC physical scale is too large".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Diagnostics {
    pub divergence_before: f64,
    pub divergence_after: f64,
    pub pressure_iterations: usize,
    pub max_speed: f64,
    /// Number of columns with water below an air gap below another water cell; an overturning/splash indicator, not a wave quality score.
    pub overhang_columns: usize,
}

pub struct Simulation {
    config: Config,
    particles: Vec<Particle>,
    grid: Grid,
    time: f64,
    steps: usize,
    last_dt: f64,
    wavemaker: bool,
    paddle_x: f64,
    paddle_velocity: f64,
    wave_clock: f64,
    diagnostics: Diagnostics,
    particle_volume: f64,
}

impl Simulation {
    pub fn new(config: Config) -> Result<Self, String> {
        config.validate()?;
        let h = config.cell_size;
        let paddle_x = 2.0 * h;
        let mut grid = Grid::new(config);
        grid.mark_solids(paddle_x);
        let mut particles = Vec::new();
        for j in 1..config.ny - 1 {
            for i in 2..config.nx - 1 {
                if grid.kind[grid.cell(i, j)] == CellType::Solid {
                    continue;
                }
                for oy in [0.25, 0.75] {
                    for ox in [0.25, 0.75] {
                        let x = (i as f64 + ox) * h;
                        let y = (j as f64 + oy) * h;
                        if y < config.water_level() {
                            particles.push(Particle {
                                x,
                                y,
                                u: 0.0,
                                v: 0.0,
                            });
                        }
                    }
                }
            }
        }
        if particles.is_empty() {
            return Err("PIC initial water region is empty".into());
        }
        grid.mark_fluid(&particles);
        Ok(Self {
            config,
            particles,
            grid,
            time: 0.0,
            steps: 0,
            last_dt: 0.0,
            wavemaker: true,
            paddle_x,
            paddle_velocity: 0.0,
            wave_clock: 0.0,
            diagnostics: Diagnostics::default(),
            particle_volume: 0.25 * h * h,
        })
    }

    pub fn config(&self) -> Config {
        self.config
    }
    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }
    pub fn grid(&self) -> &Grid {
        &self.grid
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
    pub fn wavemaker(&self) -> bool {
        self.wavemaker
    }
    pub fn set_wavemaker(&mut self, enabled: bool) {
        self.wavemaker = enabled;
    }
    pub fn paddle_x(&self) -> f64 {
        self.paddle_x
    }
    pub fn diagnostics(&self) -> Diagnostics {
        self.diagnostics
    }
    /// Fixed area represented by particles; no particles are created or removed per step. This does not directly measure reconstructed surface volume error.
    pub fn volume(&self) -> f64 {
        self.particle_volume * self.particles.len() as f64
    }

    pub fn stable_dt(&self) -> f64 {
        let speed = self
            .particles
            .iter()
            .map(|p| p.u.hypot(p.v))
            .fold(0.0, f64::max);
        let paddle_max = if self.wavemaker {
            2.0 * std::f64::consts::PI * self.config.wave_stroke / self.config.wave_period
        } else {
            0.0
        };
        (self.config.cfl * self.config.cell_size
            / (speed.max(paddle_max) + (self.config.gravity * self.config.cell_size).sqrt()))
        .min(1.0 / 90.0)
    }

    pub fn step(&mut self, max_dt: f64) -> Result<f64, String> {
        if !max_dt.is_finite() || max_dt <= 0.0 {
            return Err("PIC max_dt must be finite and positive".into());
        }
        let dt = self.stable_dt().min(max_dt);
        if !dt.is_finite() || self.time + dt <= self.time || !(self.time + dt).is_finite() {
            return Err("PIC time step cannot advance time".into());
        }
        if self.wavemaker {
            self.wave_clock += dt;
            (self.paddle_x, self.paddle_velocity) = self.paddle_motion(self.wave_clock);
        } else {
            self.paddle_velocity = 0.0;
        }
        self.grid.mark_solids(self.paddle_x);
        // The moving paddle first pushes swept particles out; the grid boundary uses the same paddle velocity.
        for p in &mut self.particles {
            if p.x < self.paddle_x + 0.2 * self.config.cell_size {
                p.x = self.paddle_x + 0.2 * self.config.cell_size;
                p.u = p.u.max(self.paddle_velocity);
            }
        }
        self.grid.mark_fluid(&self.particles);
        transfer::particles_to_grid(&self.particles, &mut self.grid);
        self.grid.add_gravity(self.config.gravity, dt);
        self.grid.enforce_boundaries(self.paddle_velocity);
        self.diagnostics.divergence_before = self.grid.divergence_rms();
        self.diagnostics.pressure_iterations =
            pressure::project(&mut self.grid, dt, self.config.pressure_iterations);
        self.diagnostics.divergence_after = self.grid.divergence_rms();
        transfer::extrapolate(&mut self.grid, 3);
        transfer::grid_to_particles(
            &mut self.particles,
            &self.grid,
            dt,
            self.paddle_x,
            self.paddle_velocity,
        );
        if self
            .particles
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.u.is_finite() || !p.v.is_finite())
        {
            return Err("PIC produced a non-finite particle".into());
        }
        self.time += dt;
        self.steps += 1;
        self.last_dt = dt;
        self.diagnostics.max_speed = self
            .particles
            .iter()
            .map(|p| p.u.hypot(p.v))
            .fold(0.0, f64::max);
        self.diagnostics.overhang_columns = self.count_overhangs();
        Ok(dt)
    }

    pub fn advance_to(&mut self, target: f64) -> Result<(), String> {
        if !target.is_finite() || target < self.time {
            return Err("invalid PIC target time".into());
        }
        while self.time < target {
            let remaining = target - self.time;
            if self.step(remaining)? == remaining {
                self.time = target;
            }
        }
        Ok(())
    }

    /// Apply a local upward/forward velocity impulse with the mouse; changes momentum without adding particles.
    pub fn disturb(&mut self, x: f64, y: f64) -> Result<usize, String> {
        if !x.is_finite()
            || !y.is_finite()
            || !(0.0..=self.config.width()).contains(&x)
            || !(0.0..=self.config.height()).contains(&y)
        {
            return Err("PIC impulse position is outside the tank".into());
        }
        let radius = 5.0 * self.config.cell_size;
        let mut affected = 0;
        for p in &mut self.particles {
            let dx = p.x - x;
            let dy = p.y - y;
            let r2 = (dx * dx + dy * dy) / (radius * radius);
            if r2 < 1.0 {
                let weight = (1.0 - r2).powi(2);
                p.u += 1.5 * weight;
                p.v += 2.5 * weight;
                affected += 1;
            }
        }
        self.diagnostics.max_speed = self
            .particles
            .iter()
            .map(|p| p.u.hypot(p.v))
            .fold(0.0, f64::max);
        Ok(affected)
    }

    /// Apply a repeatable breaking-wave impulse: a local clockwise velocity field at the slope entrance.
    /// This external momentum input does not move particles; subsequent motion is driven entirely by PIC and gravity.
    pub fn wave_pulse(&mut self) {
        let radius = 0.12 * self.config.width();
        let center_x = 0.48 * self.config.width();
        let center_y = self.config.water_level() - 0.6 * radius;
        for p in &mut self.particles {
            let dx = p.x - center_x;
            let dy = p.y - center_y;
            let r2 = (dx * dx + dy * dy) / (radius * radius);
            if r2 < 4.0 {
                let spin = 24.0 * (-2.0 * r2).exp();
                p.u += spin * dy;
                p.v -= spin * dx;
            }
        }
        self.diagnostics.max_speed = self
            .particles
            .iter()
            .map(|p| p.u.hypot(p.v))
            .fold(0.0, f64::max);
    }

    fn paddle_motion(&self, time: f64) -> (f64, f64) {
        let phase = (time / self.config.wave_period).fract();
        let (position, derivative) = if phase < 0.25 {
            let angle = std::f64::consts::PI * phase / 0.25;
            (
                0.5 * (1.0 - angle.cos()),
                0.5 * angle.sin() * std::f64::consts::PI / 0.25,
            )
        } else {
            let angle = std::f64::consts::PI * (phase - 0.25) / 0.75;
            (
                0.5 * (1.0 + angle.cos()),
                -0.5 * angle.sin() * std::f64::consts::PI / 0.75,
            )
        };
        (
            2.0 * self.config.cell_size + self.config.wave_stroke * position,
            self.config.wave_stroke / self.config.wave_period * derivative,
        )
    }

    fn count_overhangs(&self) -> usize {
        let mut counts = vec![0_u16; self.config.nx * self.config.ny];
        for p in &self.particles {
            let i = (p.x / self.config.cell_size) as usize;
            let j = (p.y / self.config.cell_size) as usize;
            if i < self.config.nx && j < self.config.ny {
                counts[self.grid.cell(i, j)] += 1;
            }
        }
        (1..self.config.nx - 1)
            .filter(|&i| {
                let mut wet_below = false;
                let mut air_gap = 0;
                for j in self.grid.bed_cells[i]..self.config.ny - 1 {
                    let count = counts[self.grid.cell(i, j)];
                    if count > 0 {
                        if wet_below && air_gap >= 2 && count >= 2 {
                            return true;
                        }
                        wet_below = true;
                        air_gap = 0;
                    } else if wet_below {
                        air_gap += 1;
                    }
                }
                false
            })
            .count()
    }
}

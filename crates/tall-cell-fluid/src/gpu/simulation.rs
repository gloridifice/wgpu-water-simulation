use super::{GpuContext, build_adaptive, project, transport};
use crate::adaptivity::AdaptiveGrid;
use crate::{Grid, SimulationConfig, StepReport};

/// Tolerances for f32 solving. The CPU f64 default relative residual of 1e-9 is unsuitable for GPU f32.
pub fn default_config() -> SimulationConfig {
    let mut config = SimulationConfig::default();
    config.pressure.relative_tolerance = 2e-5;
    config.pressure.absolute_tolerance = 1e-5;
    config.pressure.divergence_tolerance = 5e-4;
    config
}

/// Headless 3D GPU liquid. `grid` is an inspectable stage-readback snapshot, not a CPU solver.
pub struct GpuSimulation<'a> {
    pub context: &'a GpuContext,
    pub grid: Grid,
    pub config: SimulationConfig,
    pub adaptive: AdaptiveGrid,
    pub time: f64,
    pub steps: usize,
}

impl<'a> GpuSimulation<'a> {
    pub fn from_grid(
        context: &'a GpuContext,
        grid: Grid,
        config: SimulationConfig,
    ) -> Result<Self, String> {
        context.validate_grid(&grid)?;
        validate_config(&config)?;
        let adaptive = build_adaptive(context, &grid, &config.adaptivity)?;
        Ok(Self {
            context,
            grid,
            config,
            adaptive,
            time: 0.0,
            steps: 0,
        })
    }

    pub fn stable_time_step(&self) -> f64 {
        let acceleration = self
            .config
            .gravity
            .iter()
            .map(|x| x * x)
            .sum::<f64>()
            .sqrt();
        let speed = self.grid.velocity.max_speed() + acceleration * self.config.max_time_step;
        self.config
            .max_time_step
            .min(self.config.cfl * self.grid.domain.cell_size / speed.max(1e-12))
    }

    /// On failure, do not commit grid/time/steps; there is no CPU numerical solver fallback.
    pub fn step(&mut self, dt: f64) -> Result<StepReport, String> {
        validate_config(&self.config)?;
        self.context.validate_grid(&self.grid)?;
        let next_step = self
            .steps
            .checked_add(1)
            .ok_or("Simulation step count overflow")?;
        if !dt.is_finite()
            || dt <= 0.0
            || !(dt as f32).is_normal()
            || dt > self.stable_time_step() * (1.0 + 1e-12)
        {
            return Err("GPU time step must be positive, representable as f32, and no greater than stable_time_step()".into());
        }
        if !(self.time + dt).is_finite() || self.time + dt == self.time {
            return Err("Accumulated time is not representable".into());
        }
        let volume_before = self.grid.liquid_volume();
        let (mut next, volume_correction) =
            transport::prepare(self.context, &self.grid, &self.config, dt, next_step)?;
        let adaptive = build_adaptive(self.context, &next, &self.config.adaptivity)?;
        let projection = project(
            self.context,
            &mut next,
            &adaptive,
            dt,
            self.config.density,
            &self.config.pressure,
        )?;
        if !projection.converged {
            return Err(format!(
                "GPU pressure projection did not converge: residual={}, divergence={} /s",
                projection.residual_norm, projection.divergence_after
            ));
        }
        let next = transport::finish(self.context, &next, &self.config)?;
        next.validate()?;
        let report = StepReport {
            time: self.time + dt,
            dt,
            volume_before,
            volume_after: next.liquid_volume(),
            volume_correction,
            optical_cells: adaptive.optical_mask.iter().filter(|&&x| x).count(),
            adaptive_cells: adaptive.cells.len(),
            projection,
        };
        self.grid = next;
        self.adaptive = adaptive;
        self.time += dt;
        self.steps = next_step;
        Ok(report)
    }

    /// Automatically split into CFL substeps; retain previously successful substeps if a later one fails.
    pub fn advance(&mut self, seconds: f64) -> Result<Vec<StepReport>, String> {
        validate_config(&self.config)?;
        self.context.validate_grid(&self.grid)?;
        let target = self.time + seconds;
        if !seconds.is_finite()
            || seconds < 0.0
            || !target.is_finite()
            || (seconds > 0.0 && target == self.time)
        {
            return Err("Advance duration is not representable".into());
        }
        let mut reports = Vec::new();
        while self.time < target {
            if reports.len() >= self.config.max_substeps {
                return Err("GPU substep limit reached; successful substeps retained".into());
            }
            reports.push(self.step(self.stable_time_step().min(target - self.time))?);
        }
        Ok(reports)
    }
}

fn validate_config(config: &SimulationConfig) -> Result<(), String> {
    config.validate()?;
    if !(config.density as f32).is_normal()
        || !(config.max_time_step as f32).is_normal()
        || config
            .gravity
            .iter()
            .any(|&v| !(v as f32).is_finite() || (v != 0.0 && (v as f32).abs() < f32::MIN_POSITIVE))
    {
        return Err("GPU parameters exceed f32 range".into());
    }
    Ok(())
}

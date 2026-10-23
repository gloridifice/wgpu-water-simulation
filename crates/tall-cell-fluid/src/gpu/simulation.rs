use super::{GpuContext, build_adaptive, project, transport};
use crate::adaptivity::AdaptiveGrid;
use crate::{Grid, SimulationConfig, StepReport};

/// f32 求解所用容限。CPU f64 默认的 1e-9 相对残差不适合 GPU f32。
pub fn default_config() -> SimulationConfig {
    let mut config = SimulationConfig::default();
    config.pressure.relative_tolerance = 2e-5;
    config.pressure.absolute_tolerance = 1e-5;
    config.pressure.divergence_tolerance = 5e-4;
    config
}

/// 无窗口三维 GPU 液体。`grid` 是阶段回读的可检查快照，不是 CPU 求解器。
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

    /// 失败不提交 grid/time/steps；没有 CPU 数值求解回退。
    pub fn step(&mut self, dt: f64) -> Result<StepReport, String> {
        validate_config(&self.config)?;
        self.context.validate_grid(&self.grid)?;
        let next_step = self.steps.checked_add(1).ok_or("模拟步数溢出")?;
        if !dt.is_finite()
            || dt <= 0.0
            || !(dt as f32).is_normal()
            || dt > self.stable_time_step() * (1.0 + 1e-12)
        {
            return Err("GPU 时间步须为 f32 可表示的正数且不超过 stable_time_step()".into());
        }
        if !(self.time + dt).is_finite() || self.time + dt == self.time {
            return Err("累计时间不可表示".into());
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
                "GPU 压力投影未收敛：residual={}, divergence={} /s",
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

    /// 自动按 CFL 分子步；后续失败时保留此前已成功子步。
    pub fn advance(&mut self, seconds: f64) -> Result<Vec<StepReport>, String> {
        validate_config(&self.config)?;
        self.context.validate_grid(&self.grid)?;
        let target = self.time + seconds;
        if !seconds.is_finite()
            || seconds < 0.0
            || !target.is_finite()
            || (seconds > 0.0 && target == self.time)
        {
            return Err("推进时长不可表示".into());
        }
        let mut reports = Vec::new();
        while self.time < target {
            if reports.len() >= self.config.max_substeps {
                return Err("达到 GPU 子步上限，已成功子步保留".into());
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
        return Err("GPU 参数超出 f32 范围".into());
    }
    Ok(())
}

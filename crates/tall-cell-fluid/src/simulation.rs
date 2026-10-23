//! 时间推进：RK2 半拉格朗日输送、静态边界、重力、自适应投影和 level-set 维护。

use crate::adaptivity::{AdaptiveGrid, AdaptivityConfig};
use crate::grid::{Domain, Grid, Vec3};
use crate::pressure::{self, PressureConfig, ProjectionReport};

#[derive(Clone, Debug)]
pub struct SimulationConfig {
    /// kg/m³。
    pub density: f64,
    /// m/s²。
    pub gravity: Vec3,
    /// 一次输送最多跨越的背景网格数。
    pub cfl: f64,
    /// 秒；advance 会按此上限分子步。
    pub max_time_step: f64,
    pub max_substeps: usize,
    /// 向空气外推速度的层数，用于表面附近的回溯采样。
    pub extrapolation_layers: usize,
    /// 每多少步重建一次近似符号距离；0 禁用。
    pub reinitialize_every: usize,
    /// 全局平移 level set，以保持平滑 Heaviside 估计体积；不是局部保守输送。
    pub conserve_volume: bool,
    pub adaptivity: AdaptivityConfig,
    pub pressure: PressureConfig,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            density: 1000.0,
            gravity: [0.0, -9.81, 0.0],
            cfl: 0.9,
            max_time_step: 1.0 / 60.0,
            max_substeps: 1024,
            extrapolation_layers: 4,
            reinitialize_every: 4,
            conserve_volume: true,
            adaptivity: AdaptivityConfig::default(),
            pressure: PressureConfig::default(),
        }
    }
}

impl SimulationConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.density.is_finite()
            || self.density <= 0.0
            || self.gravity.iter().any(|x| !x.is_finite())
            || !self.cfl.is_finite()
            || self.cfl <= 0.0
            || self.cfl > 1.0
            || !self.max_time_step.is_finite()
            || self.max_time_step <= 0.0
            || self.max_substeps == 0
            || self.extrapolation_layers > 128
        {
            return Err(
                "密度/步长须为有限正数；CFL 在 (0,1]，重力有限，子步上限非零，外推不超过128层"
                    .into(),
            );
        }
        self.adaptivity.validate()?;
        self.pressure.validate()
    }
}

#[derive(Clone, Debug)]
pub struct StepReport {
    pub time: f64,
    pub dt: f64,
    pub volume_before: f64,
    pub volume_after: f64,
    /// 对 phi 施加的统一偏移量，米；负值扩大液体。
    pub volume_correction: f64,
    pub optical_cells: usize,
    pub adaptive_cells: usize,
    pub projection: ProjectionReport,
}

pub struct Simulation {
    pub grid: Grid,
    pub config: SimulationConfig,
    pub adaptive: AdaptiveGrid,
    pub time: f64,
    pub steps: usize,
}

impl Simulation {
    pub fn new(domain: Domain, config: SimulationConfig) -> Result<Self, String> {
        domain.validate()?;
        config.validate()?;
        Self::from_grid(Grid::new(domain), config)
    }

    pub fn from_grid(mut grid: Grid, config: SimulationConfig) -> Result<Self, String> {
        grid.validate()?;
        config.validate()?;
        grid.enforce_boundaries();
        let adaptive = AdaptiveGrid::build(&grid, &config.adaptivity);
        Ok(Self {
            grid,
            config,
            adaptive,
            time: 0.0,
            steps: 0,
        })
    }

    /// 在当前边界条件和重力下采用保守的速度上界，返回可用步长（秒）。
    pub fn stable_time_step(&self) -> f64 {
        let acceleration = norm(self.config.gravity);
        let speed = self.grid.velocity.max_speed() + acceleration * self.config.max_time_step;
        self.config
            .max_time_step
            .min(self.config.cfl * self.grid.domain.cell_size / speed.max(1e-12))
    }

    /// 推进一步。拒绝超过 CFL/配置上限的 dt；失败不提交新的物理状态。
    pub fn step(&mut self, dt: f64) -> Result<StepReport, String> {
        self.config.validate()?;
        self.grid.validate()?;
        let next_step = self.steps.checked_add(1).ok_or("模拟步数溢出")?;
        if !dt.is_finite() || dt <= 0.0 || dt > self.stable_time_step() * (1.0 + 1e-12) {
            return Err(
                "时间步必须为有限正数且不超过 stable_time_step()；长时间推进请用 advance".into(),
            );
        }
        if !(self.time + dt).is_finite() || self.time + dt == self.time {
            return Err("累计时间溢出或时间步小于浮点分辨率".into());
        }
        let mut old = self.grid.clone();
        old.enforce_boundaries();
        extrapolate_velocity(&mut old, self.config.extrapolation_layers);
        let volume_before = old.liquid_volume();
        let mut next = advect(&old, dt);
        if self.config.reinitialize_every > 0
            && next_step.is_multiple_of(self.config.reinitialize_every)
        {
            reinitialize_level_set(&mut next);
        }
        let volume_correction = if self.config.conserve_volume {
            correct_volume(&mut next, volume_before)
        } else {
            0.0
        };
        add_gravity(&mut next, self.config.gravity, dt);
        next.enforce_boundaries();
        // 背景输送不依赖四叉树；在最新界面上构造自适应压力空间。
        let adaptive = AdaptiveGrid::build(&next, &self.config.adaptivity);
        let projection = pressure::project(
            &mut next,
            &adaptive,
            dt,
            self.config.density,
            &self.config.pressure,
        )?;
        if !projection.converged {
            return Err(format!(
                "压力投影未收敛：residual={}, divergence={} /s",
                projection.residual_norm, projection.divergence_after
            ));
        }
        extrapolate_velocity(&mut next, self.config.extrapolation_layers);
        next.enforce_boundaries();
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

    /// 自动分子步推进指定秒数。若后续子步失败，已成功的子步保留；time 是进度依据。
    pub fn advance(&mut self, seconds: f64) -> Result<Vec<StepReport>, String> {
        self.config.validate()?;
        self.grid.validate()?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("推进时长须为有限非负数".into());
        }
        let target = self.time + seconds;
        if !target.is_finite() || (seconds > 0.0 && target == self.time) {
            return Err("目标时间不可表示".into());
        }
        let mut reports = Vec::new();
        while self.time < target {
            if reports.len() >= self.config.max_substeps {
                return Err("达到子步上限，已成功的子步保留".into());
            }
            let dt = self.stable_time_step().min(target - self.time);
            reports.push(self.step(dt)?);
        }
        Ok(reports)
    }
}

fn norm(v: Vec3) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// 中点 RK2 回溯；碰到静态固体时在路径首次采样的阻挡段二分截断。
fn backtrace(grid: &Grid, p: Vec3, dt: f64) -> Vec3 {
    let u = grid.sample_velocity(p);
    let midpoint = grid
        .domain
        .clamp_point(std::array::from_fn(|a| p[a] - 0.5 * dt * u[a]));
    let mid_u = grid.sample_velocity(midpoint);
    let end = grid
        .domain
        .clamp_point(std::array::from_fn(|a| p[a] - dt * mid_u[a]));
    if grid.sample_solid_phi(p) < 0.0 {
        return p;
    }
    let mut previous = p;
    for segment in 1..=4 {
        let t = segment as f64 / 4.0;
        let point = std::array::from_fn(|a| p[a] + t * (end[a] - p[a]));
        if grid.sample_solid_phi(point) < 0.0 {
            let mut safe = previous;
            let mut blocked = point;
            for _ in 0..20 {
                let mid = std::array::from_fn(|a| 0.5 * (safe[a] + blocked[a]));
                if grid.sample_solid_phi(mid) < 0.0 {
                    blocked = mid;
                } else {
                    safe = mid;
                }
            }
            return safe;
        }
        previous = point;
    }
    end
}

fn advect(old: &Grid, dt: f64) -> Grid {
    let mut new = old.clone();
    new.pressure.fill(0.0);
    for n in 0..old.domain.cell_count() {
        let p = old.domain.center(old.domain.coord(n));
        new.phi[n] = old.sample_phi(backtrace(old, p, dt));
    }
    for axis in 0..3 {
        let dims = old.velocity.dimensions(axis);
        for k in 0..dims[2] {
            for j in 0..dims[1] {
                for i in 0..dims[0] {
                    let c = [i, j, k];
                    let p = old.velocity.position(axis, c);
                    let value = old.sample_velocity(backtrace(old, p, dt))[axis];
                    new.velocity.set(axis, c, value);
                }
            }
        }
    }
    new.enforce_boundaries();
    new
}

fn face_cells(
    domain: Domain,
    axis: usize,
    p: [usize; 3],
) -> (Option<[usize; 3]>, Option<[usize; 3]>) {
    let low = if p[axis] > 0 {
        let mut q = p;
        q[axis] -= 1;
        Some(q)
    } else {
        None
    };
    let high = if p[axis] < domain.size[axis] {
        Some(p)
    } else {
        None
    };
    (low, high)
}

fn face_is_open(grid: &Grid, axis: usize, p: [usize; 3]) -> bool {
    let (low, high) = face_cells(grid.domain, axis, p);
    match (low, high) {
        (Some(a), Some(b)) => {
            grid.solid_phi[grid.domain.index(a)] >= 0.0
                && grid.solid_phi[grid.domain.index(b)] >= 0.0
        }
        _ => false,
    }
}

fn add_gravity(grid: &mut Grid, gravity: Vec3, dt: f64) {
    for (axis, acceleration) in gravity.into_iter().enumerate() {
        let dims = grid.velocity.dimensions(axis);
        for k in 0..dims[2] {
            for j in 0..dims[1] {
                for i in 0..dims[0] {
                    let p = [i, j, k];
                    let (low, high) = face_cells(grid.domain, axis, p);
                    if face_is_open(grid, axis, p)
                        && (low.is_some_and(|c| grid.is_fluid(c))
                            || high.is_some_and(|c| grid.is_fluid(c)))
                    {
                        grid.velocity
                            .set(axis, p, grid.velocity.get(axis, p) + dt * acceleration);
                    }
                }
            }
        }
    }
}

/// 只向空气面扩展，绝不覆盖投影后的液体相邻面。
fn extrapolate_velocity(grid: &mut Grid, layers: usize) {
    for axis in 0..3 {
        let dims = grid.velocity.dimensions(axis);
        let count = grid.velocity.faces[axis].len();
        let mut valid = vec![false; count];
        let mut open = vec![false; count];
        for k in 0..dims[2] {
            for j in 0..dims[1] {
                for i in 0..dims[0] {
                    let p = [i, j, k];
                    let n = grid.velocity.index(axis, p);
                    open[n] = face_is_open(grid, axis, p);
                    let (low, high) = face_cells(grid.domain, axis, p);
                    valid[n] = open[n]
                        && (low.is_some_and(|c| grid.is_fluid(c))
                            || high.is_some_and(|c| grid.is_fluid(c)));
                    if !valid[n] {
                        grid.velocity.faces[axis][n] = 0.0;
                    }
                }
            }
        }
        for _ in 0..layers {
            let previous = grid.velocity.faces[axis].clone();
            let previous_valid = valid.clone();
            let mut changed = false;
            for k in 0..dims[2] {
                for j in 0..dims[1] {
                    for i in 0..dims[0] {
                        let p = [i, j, k];
                        let n = grid.velocity.index(axis, p);
                        if valid[n] || !open[n] {
                            continue;
                        }
                        let mut sum = 0.0;
                        let mut neighbors = 0;
                        for a in 0..3 {
                            for direction in [-1_isize, 1] {
                                let c = p[a] as isize + direction;
                                if c < 0 || c >= dims[a] as isize {
                                    continue;
                                }
                                let mut q = p;
                                q[a] = c as usize;
                                let other = grid.velocity.index(axis, q);
                                if previous_valid[other] {
                                    sum += previous[other];
                                    neighbors += 1;
                                }
                            }
                        }
                        if neighbors > 0 {
                            grid.velocity.faces[axis][n] = sum / neighbors as f64;
                            valid[n] = true;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
    grid.enforce_boundaries();
}

/// 用格间符号变化初始化距离，再做八方向 fast sweeping。
pub fn reinitialize_level_set(grid: &mut Grid) {
    let domain = grid.domain;
    let h = domain.cell_size;
    let original = grid.phi.clone();
    let far = norm(domain.extent());
    let mut distance = vec![far; original.len()];
    let mut fixed = vec![false; original.len()];
    for n in 0..original.len() {
        if original[n] == 0.0 {
            distance[n] = 0.0;
            fixed[n] = true;
            continue;
        }
        let p = domain.coord(n);
        for axis in 0..3 {
            for direction in [-1_isize, 1] {
                let c = p[axis] as isize + direction;
                if c < 0 || c >= domain.size[axis] as isize {
                    continue;
                }
                let mut q = p;
                q[axis] = c as usize;
                let phi = original[domain.index(q)];
                if (phi < 0.0) != (original[n] < 0.0) || phi == 0.0 {
                    let d = h * original[n].abs()
                        / (original[n].abs() + phi.abs()).max(f64::MIN_POSITIVE);
                    distance[n] = distance[n].min(d);
                    fixed[n] = true;
                }
            }
        }
        if fixed[n] {
            // 过零边距是沿坐标轴的距离；斜面上不能把它当作法向距离。
            // 局部线性化 |phi|/|grad(phi)| 对任意方向的平面给出正确种子。
            let gradient: Vec3 = std::array::from_fn(|axis| {
                let mut lo = p;
                let mut hi = p;
                lo[axis] = lo[axis].saturating_sub(1);
                hi[axis] = (hi[axis] + 1).min(domain.size[axis] - 1);
                (original[domain.index(hi)] - original[domain.index(lo)])
                    / ((hi[axis] - lo[axis]) as f64 * h)
            });
            let magnitude = norm(gradient);
            if magnitude.is_finite() && magnitude > 1e-12 {
                distance[n] = original[n].abs() / magnitude;
            }
        }
    }
    if !fixed.iter().any(|x| *x) {
        return;
    }
    for _ in 0..3 {
        for mask in 0..8 {
            for z in 0..domain.size[2] {
                for y in 0..domain.size[1] {
                    for x in 0..domain.size[0] {
                        let iteration = [x, y, z];
                        let p = std::array::from_fn(|a| {
                            if mask & (1 << a) == 0 {
                                iteration[a]
                            } else {
                                domain.size[a] - 1 - iteration[a]
                            }
                        });
                        let n = domain.index(p);
                        if fixed[n] {
                            continue;
                        }
                        let mut adjacent = [far; 3];
                        for axis in 0..3 {
                            for direction in [-1_isize, 1] {
                                let c = p[axis] as isize + direction;
                                if c < 0 || c >= domain.size[axis] as isize {
                                    continue;
                                }
                                let mut q = p;
                                q[axis] = c as usize;
                                adjacent[axis] = adjacent[axis].min(distance[domain.index(q)]);
                            }
                        }
                        adjacent.sort_by(f64::total_cmp);
                        let mut candidate = adjacent[0] + h;
                        for used in 2..=3 {
                            if candidate <= adjacent[used - 1] {
                                break;
                            }
                            let sum: f64 = adjacent[..used].iter().sum();
                            let squares: f64 = adjacent[..used].iter().map(|x| x * x).sum();
                            candidate = (sum
                                + (sum * sum - used as f64 * (squares - h * h))
                                    .max(0.0)
                                    .sqrt())
                                / used as f64;
                        }
                        distance[n] = distance[n].min(candidate);
                    }
                }
            }
        }
    }
    for n in 0..original.len() {
        grid.phi[n] = distance[n].copysign(original[n]);
    }
}

fn correct_volume(grid: &mut Grid, target: f64) -> f64 {
    let h = grid.domain.cell_size;
    let volume_at = |shift: f64| {
        grid.phi
            .iter()
            .zip(&grid.solid_phi)
            .filter(|(_, s)| **s >= 0.0)
            .map(|(p, _)| (0.5 - (p + shift) / h).clamp(0.0, 1.0))
            .sum::<f64>()
            * h.powi(3)
    };
    if (volume_at(0.0) - target).abs() <= h.powi(3) * 1e-12 {
        return 0.0;
    }
    // 限制最大校正，避免界面丢失时通过全局平移凭空制造大块液体。
    let mut lo = -h;
    let mut hi = h;
    for _ in 0..48 {
        let mid = 0.5 * (lo + hi);
        if volume_at(mid) > target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let shift = 0.5 * (lo + hi);
    for p in &mut grid.phi {
        *p += shift;
    }
    shift
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advection_translates_plane_and_preserves_linear_velocity() {
        let domain = Domain::new([12, 8, 8], 0.1).unwrap();
        let mut grid = Grid::new(domain);
        grid.set_liquid_sdf(|p| p[0] - 0.6);
        grid.velocity.faces[0].fill(0.3);
        let new = advect(&grid, 0.1);
        for z in 1..7 {
            for y in 1..7 {
                for x in 2..10 {
                    let p = domain.center([x, y, z]);
                    assert!((new.phi[domain.index([x, y, z])] - (p[0] - 0.63)).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn reinitialization_recovers_plane_distance_without_moving_interface() {
        let mut grid = Grid::new(Domain::new([8; 3], 0.1).unwrap());
        grid.set_liquid_sdf(|p| 3.0 * (p[1] - 0.4));
        reinitialize_level_set(&mut grid);
        for n in 0..grid.phi.len() {
            let p = grid.domain.center(grid.domain.coord(n));
            assert!((grid.phi[n] - (p[1] - 0.4)).abs() < 1e-10);
        }
    }

    #[test]
    fn reinitialization_uses_normal_distance_for_oblique_plane() {
        let mut grid = Grid::new(Domain::new([8; 3], 0.1).unwrap());
        grid.set_liquid_sdf(|p| 3.0 * (p[0] + p[1] - 0.8));
        reinitialize_level_set(&mut grid);
        for n in 0..grid.phi.len() {
            let p = grid.domain.center(grid.domain.coord(n));
            let expected = (p[0] + p[1] - 0.8) / 2.0_f64.sqrt();
            assert!(
                (grid.phi[n] - expected).abs() < 1e-10,
                "{p:?}: {} vs {expected}",
                grid.phi[n]
            );
        }
    }

    #[test]
    fn volume_correction_reports_actual_volume() {
        let mut grid = Grid::new(Domain::new([8; 3], 0.1).unwrap());
        grid.set_liquid_sdf(|p| p[1] - 0.4);
        let target = grid.liquid_volume();
        for p in &mut grid.phi {
            *p += 0.02;
        }
        let shift = correct_volume(&mut grid, target);
        assert!((shift + 0.02).abs() < 1e-12);
        assert!((grid.liquid_volume() - target).abs() < 1e-12);
    }

    #[test]
    fn invalid_step_leaves_state_unchanged() {
        let mut sim = Simulation::new(
            Domain::new([8; 3], 0.1).unwrap(),
            SimulationConfig::default(),
        )
        .unwrap();
        let before = sim.grid.phi.clone();
        assert!(sim.step(f64::NAN).is_err());
        assert!(sim.step(100.0).is_err());
        assert_eq!(sim.time, 0.0);
        assert_eq!(sim.grid.phi, before);
        sim.steps = usize::MAX;
        assert!(sim.step(0.001).is_err());
        let invalid = Domain {
            size: [usize::MAX; 3],
            cell_size: 0.1,
            origin: [0.0; 3],
        };
        assert!(Simulation::new(invalid, SimulationConfig::default()).is_err());
    }
}

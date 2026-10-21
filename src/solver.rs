//! 一维、平底、全湿水槽：有限体积法 + Rusanov 通量 + 显式 Euler 时间推进。
//! `Cell` 保存一个网格内的平均守恒量，而不是一个随水移动的粒子。

/// Q = (h, hu)。密度取常数，hu 是单位密度、单位宽度的动量。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub h: f64,
    pub hu: f64,
}

impl Cell {
    /// 仅适用于 h > 0 的全湿状态。
    pub fn velocity(self) -> f64 {
        self.hu / self.h
    }

    /// 两个特征速度是 u ± sqrt(g h)，最大绝对值为 |u| + sqrt(g h)。
    pub fn wave_speed(self, gravity: f64) -> f64 {
        self.velocity().abs() + (gravity * self.h).sqrt()
    }

    /// F(Q) = (hu, hu² / h + g h² / 2)。这里 hu² 指 (hu)²。
    pub fn physical_flux(self, gravity: f64) -> Flux {
        Flux {
            mass: self.hu,
            momentum: self.hu * self.velocity() + 0.5 * gravity * self.h * self.h,
        }
    }

    /// 墙外的虚拟状态：水深不变，法向动量反向。
    pub fn reflected(self) -> Self {
        Self {
            h: self.h,
            hu: -self.hu,
        }
    }
}

/// 正通量表示沿 x 正方向通过界面；它不是新的网格状态。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Flux {
    pub mass: f64,
    pub momentum: f64,
}

/// F_hat = (F_L + F_R)/2 - alpha (Q_R - Q_L)/2。
/// 前提：两侧 h > 0，状态有限，gravity > 0；Simulation 会检查这些条件。
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
    /// 高斯凸起的标准差，单位：米。
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
    /// N 个网格对应 N+1 个界面；相邻网格共享同一个通量。
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
                // 用网格中心采样近似初始单元平均值；此写法保持左右镜像。
                let distance = (i as f64 - middle) * dx / config.bump_width;
                Cell {
                    h: config.rest_depth + config.bump_height * (-0.5 * distance * distance).exp(),
                    hu: 0.0,
                }
            })
            .collect();
        Self::from_cells(config, cells)
    }

    /// 自定义全湿初态，便于检查静水、对称性以及较陡的水位变化。
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

    /// 单位宽度水体体积（密度为 1 时也是质量），不包含虚拟网格。
    pub fn mass(&self) -> f64 {
        self.current.iter().map(|q| q.h).sum::<f64>() * self.dx
    }

    pub fn max_wave_speed(&self) -> f64 {
        self.current
            .iter()
            .map(|q| q.wave_speed(self.config.gravity))
            .fold(0.0, f64::max)
    }

    /// 每一步重新估计 dt <= CFL * dx / max(|u| + sqrt(g h))。
    pub fn stable_dt(&self) -> f64 {
        self.config.cfl * self.dx / self.max_wave_speed()
    }

    /// 在 x=center 附近重分布水位，产生向两侧传播的局部波包。
    /// 窄/宽高斯的离散和相抵消，因此不添加水量；不直接改变 hu 或时间。
    /// 返回实际使用的振幅。水浅时整体缩小扰动，保留至少 75% 原水深。
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
        // 用实际网格上的离散和归一化，靠墙截断的波包也保持质量。
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

    /// 前进一步，max_dt 通常是到下一次输出时刻的剩余时间。
    /// 无效结果会报错，current 和时间保持不变；不会裁剪水深来掩盖错误。
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
        // 左边界顺序是 (墙外, 墙内)，右边界顺序是 (墙内, 墙外)。
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
        // 整轮只读 current、只写 next；完成后交换 Vec，不复制整份状态。
        std::mem::swap(&mut self.current, &mut self.next);
        self.time = new_time;
        self.steps += 1;
        self.last_dt = dt;
        Ok(dt)
    }

    /// 输出间隔与物理 dt 独立；最后一个小步精确对齐目标时间。
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

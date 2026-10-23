//! 规则背景网格：米、秒、米/秒、帕斯卡；y 为竖直方向。

pub type Vec3 = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Domain {
    pub size: [usize; 3],
    pub cell_size: f64,
    pub origin: Vec3,
}

impl Domain {
    pub fn new(size: [usize; 3], cell_size: f64) -> Result<Self, String> {
        let result = Self {
            size,
            cell_size,
            origin: [0.0; 3],
        };
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.size.iter().any(|&n| n < 2)
            || !self.cell_size.is_finite()
            || self.cell_size <= 0.0
            || self.origin.iter().any(|v| !v.is_finite())
        {
            return Err("每轴至少两个单元；网格间距必须为有限正数，原点必须有限".into());
        }
        let count = self.size.iter().try_fold(1usize, |a, &b| a.checked_mul(b));
        if count.is_none_or(|n| n > isize::MAX as usize / (8 * 32)) {
            return Err("网格尺寸溢出或超过可寻址范围".into());
        }
        for axis in 0..3 {
            let mut dims = self.size;
            dims[axis] = dims[axis].checked_add(1).ok_or("面网格尺寸溢出")?;
            if dims
                .iter()
                .try_fold(1usize, |a, &b| a.checked_mul(b))
                .is_none()
            {
                return Err("面网格尺寸溢出".into());
            }
            let upper = self.origin[axis] + self.size[axis] as f64 * self.cell_size;
            if !upper.is_finite() {
                return Err("计算域物理尺寸溢出".into());
            }
        }
        Ok(())
    }

    pub fn cell_count(&self) -> usize {
        self.size.iter().product()
    }

    pub fn index(&self, p: [usize; 3]) -> usize {
        p[0] + self.size[0] * (p[1] + self.size[1] * p[2])
    }

    pub fn coord(&self, index: usize) -> [usize; 3] {
        [
            index % self.size[0],
            index / self.size[0] % self.size[1],
            index / (self.size[0] * self.size[1]),
        ]
    }

    pub fn center(&self, p: [usize; 3]) -> Vec3 {
        std::array::from_fn(|a| self.origin[a] + (p[a] as f64 + 0.5) * self.cell_size)
    }

    pub fn extent(&self) -> Vec3 {
        self.size.map(|n| n as f64 * self.cell_size)
    }

    pub fn clamp_point(&self, p: Vec3) -> Vec3 {
        std::array::from_fn(|a| p[a].clamp(self.origin[a], self.origin[a] + self.extent()[a]))
    }
}

#[derive(Clone, Debug)]
pub struct MacVelocity {
    pub domain: Domain,
    /// x/y/z 面法向速度，尺寸分别为 (Nx+1,Ny,Nz)、(Nx,Ny+1,Nz)、(Nx,Ny,Nz+1)。
    pub faces: [Vec<f64>; 3],
}

impl MacVelocity {
    pub fn new(domain: Domain) -> Self {
        Self {
            domain,
            faces: std::array::from_fn(|axis| {
                let mut size = domain.size;
                size[axis] += 1;
                vec![0.0; size.iter().product()]
            }),
        }
    }

    pub fn dimensions(&self, axis: usize) -> [usize; 3] {
        let mut size = self.domain.size;
        size[axis] += 1;
        size
    }

    pub fn index(&self, axis: usize, p: [usize; 3]) -> usize {
        let dims = self.dimensions(axis);
        p[0] + dims[0] * (p[1] + dims[1] * p[2])
    }

    pub fn get(&self, axis: usize, p: [usize; 3]) -> f64 {
        self.faces[axis][self.index(axis, p)]
    }

    pub fn set(&mut self, axis: usize, p: [usize; 3], value: f64) {
        let index = self.index(axis, p);
        self.faces[axis][index] = value;
    }

    pub fn position(&self, axis: usize, p: [usize; 3]) -> Vec3 {
        std::array::from_fn(|a| {
            self.domain.origin[a]
                + (p[a] as f64 + if axis == a { 0.0 } else { 0.5 }) * self.domain.cell_size
        })
    }

    pub fn sample(&self, p: Vec3) -> Vec3 {
        std::array::from_fn(|axis| {
            let coordinates = std::array::from_fn(|a| {
                (p[a] - self.domain.origin[a]) / self.domain.cell_size
                    - if axis == a { 0.0 } else { 0.5 }
            });
            sample_array(&self.faces[axis], self.dimensions(axis), coordinates)
        })
    }

    pub fn max_speed(&self) -> f64 {
        // 各分量最大值的范数，是插值速度的保守上界。
        self.faces
            .iter()
            .map(|v| v.iter().fold(0.0_f64, |m, x| m.max(x.abs())).powi(2))
            .sum::<f64>()
            .sqrt()
    }
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub domain: Domain,
    /// 液体为负、空气为正，单位米；存储在单元中心。
    pub phi: Vec<f64>,
    /// 静态固体为负、可流动区域为正，单位米；此参考实现使用体素边界。
    pub solid_phi: Vec<f64>,
    pub pressure: Vec<f64>,
    pub velocity: MacVelocity,
}

impl Grid {
    pub fn new(domain: Domain) -> Self {
        let count = domain.cell_count();
        let outside = domain.extent().iter().copied().fold(0.0_f64, f64::max);
        Self {
            domain,
            phi: vec![outside; count],
            solid_phi: vec![outside; count],
            pressure: vec![0.0; count],
            velocity: MacVelocity::new(domain),
        }
    }

    pub fn set_liquid_sdf(&mut self, sdf: impl Fn(Vec3) -> f64) {
        for n in 0..self.phi.len() {
            self.phi[n] = sdf(self.domain.center(self.domain.coord(n)));
        }
    }

    pub fn set_solid_sdf(&mut self, sdf: impl Fn(Vec3) -> f64) {
        for n in 0..self.solid_phi.len() {
            self.solid_phi[n] = sdf(self.domain.center(self.domain.coord(n)));
        }
    }

    pub fn is_fluid(&self, p: [usize; 3]) -> bool {
        let n = self.domain.index(p);
        self.phi[n] < 0.0 && self.solid_phi[n] >= 0.0
    }

    pub fn sample_phi(&self, p: Vec3) -> f64 {
        self.sample_centered(&self.phi, p)
    }
    pub fn sample_solid_phi(&self, p: Vec3) -> f64 {
        self.sample_centered(&self.solid_phi, p)
    }
    pub fn sample_velocity(&self, p: Vec3) -> Vec3 {
        self.velocity.sample(p)
    }

    pub fn sample_centered(&self, data: &[f64], p: Vec3) -> f64 {
        let coords =
            std::array::from_fn(|a| (p[a] - self.domain.origin[a]) / self.domain.cell_size - 0.5);
        sample_array(data, self.domain.size, coords)
    }

    /// 闭合计算域；固体相邻面零法向速度。没有移动固体或部分面开口近似。
    pub fn enforce_boundaries(&mut self) {
        for axis in 0..3 {
            let dims = self.velocity.dimensions(axis);
            for k in 0..dims[2] {
                for j in 0..dims[1] {
                    for i in 0..dims[0] {
                        let p = [i, j, k];
                        let blocked = if p[axis] == 0 || p[axis] == self.domain.size[axis] {
                            true
                        } else {
                            let mut before = p;
                            before[axis] -= 1;
                            self.solid_phi[self.domain.index(before)] < 0.0
                                || self.solid_phi[self.domain.index(p)] < 0.0
                        };
                        if blocked {
                            self.velocity.set(axis, p, 0.0);
                        }
                    }
                }
            }
        }
    }

    pub fn divergence(&self, p: [usize; 3]) -> f64 {
        (0..3)
            .map(|a| {
                let mut upper = p;
                upper[a] += 1;
                self.velocity.get(a, upper) - self.velocity.get(a, p)
            })
            .sum::<f64>()
            / self.domain.cell_size
    }

    pub fn divergence_rms(&self) -> f64 {
        let mut squared = 0.0;
        let mut count = 0;
        for n in 0..self.domain.cell_count() {
            let p = self.domain.coord(n);
            if self.is_fluid(p) {
                squared += self.divergence(p).powi(2);
                count += 1;
            }
        }
        (squared / count.max(1) as f64).sqrt()
    }

    /// 平滑 Heaviside 的体积估计，非严格几何体积。
    pub fn liquid_volume(&self) -> f64 {
        let h = self.domain.cell_size;
        self.phi
            .iter()
            .zip(&self.solid_phi)
            .filter(|(_, s)| **s >= 0.0)
            .map(|(p, _)| (0.5 - p / h).clamp(0.0, 1.0))
            .sum::<f64>()
            * h.powi(3)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.domain.validate()?;
        if self.domain != self.velocity.domain {
            return Err("速度网格与单元网格不一致".into());
        }
        for (label, data) in [
            ("phi", &self.phi),
            ("solid_phi", &self.solid_phi),
            ("pressure", &self.pressure),
        ] {
            if data.len() != self.domain.cell_count() || data.iter().any(|x| !x.is_finite()) {
                return Err(format!("{label} 长度不匹配或包含非有限数"));
            }
        }
        for axis in 0..3 {
            let data = &self.velocity.faces[axis];
            if data.len() != self.velocity.dimensions(axis).iter().product::<usize>()
                || data.iter().any(|x| !x.is_finite())
            {
                return Err("速度数组长度不匹配或包含非有限数".into());
            }
        }
        Ok(())
    }
}

fn sample_array(data: &[f64], dims: [usize; 3], coordinate: Vec3) -> f64 {
    let q: Vec3 = std::array::from_fn(|a| coordinate[a].clamp(0.0, (dims[a] - 1) as f64));
    let lo = q.map(|v| v.floor() as usize);
    let hi: [usize; 3] = std::array::from_fn(|a| (lo[a] + 1).min(dims[a] - 1));
    let t: Vec3 = std::array::from_fn(|a| q[a] - lo[a] as f64);
    let mut result = 0.0;
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let bits = [x, y, z];
                let p: [usize; 3] =
                    std::array::from_fn(|a| if bits[a] == 0 { lo[a] } else { hi[a] });
                let weight: f64 = (0..3)
                    .map(|a| if bits[a] == 0 { 1.0 - t[a] } else { t[a] })
                    .product();
                result += weight * data[p[0] + dims[0] * (p[1] + dims[1] * p[2])];
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staggered_linear_sampling_and_indexing() {
        let domain = Domain::new([5, 6, 7], 0.25).unwrap();
        let mut grid = Grid::new(domain);
        grid.set_liquid_sdf(|p| p[0] + 2.0 * p[1] - p[2]);
        for axis in 0..3 {
            let d = grid.velocity.dimensions(axis);
            for k in 0..d[2] {
                for j in 0..d[1] {
                    for i in 0..d[0] {
                        let p = grid.velocity.position(axis, [i, j, k]);
                        grid.velocity
                            .set(axis, [i, j, k], p[0] + 2.0 * p[1] + 3.0 * p[2]);
                    }
                }
            }
        }
        let p = [0.43, 0.68, 0.79];
        assert!((grid.sample_phi(p) - (p[0] + 2.0 * p[1] - p[2])).abs() < 1e-12);
        for v in grid.sample_velocity(p) {
            assert!((v - (p[0] + 2.0 * p[1] + 3.0 * p[2])).abs() < 1e-12);
        }
        for i in 0..domain.cell_count() {
            assert_eq!(domain.index(domain.coord(i)), i);
        }
    }

    #[test]
    fn invalid_domains_and_state_are_rejected() {
        assert!(Domain::new([0, 4, 4], 1.0).is_err());
        assert!(Domain::new([4, 4, 4], f64::NAN).is_err());
        assert!(Domain::new([usize::MAX, 4, 4], 1.0).is_err());
        let mut grid = Grid::new(Domain::new([4; 3], 1.0).unwrap());
        grid.phi[0] = f64::NAN;
        assert!(grid.validate().is_err());
    }
}

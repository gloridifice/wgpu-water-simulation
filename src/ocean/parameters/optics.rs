use crate::ocean::parameters::sky::SkyConfig;
pub const VIEW_NAMES: [&str; 12] = [
    "正常着色",
    "水中光程（米）",
    "透射率",
    "衰减后的海底（吸收）",
    "入射散射",
    "消光损失",
    "原始海底",
    "介质合成",
    "光线中点天空散射源",
    "天空散射积分",
    "手动散射源积分",
    "光线中点总散射源",
];

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OpticsConfig {
    pub model: crate::ocean::parameters::model::ModelConfig,
    pub shore: crate::ocean::parameters::shore::ShoreConfig,
    pub spray: crate::ocean::parameters::spray::SprayConfig,
    pub depth: f32,
    pub beach: bool,
    pub beach_slope: f32,
    /// World-space length of one sand tile along the slope, in meters.
    pub sand_tile_size: f32,
    /// Rotation in the slope/shore plane, in degrees.
    pub sand_rotation_deg: f32,
    pub shore_x: f32,
    pub sigma_t: [f32; 3],
    pub source: [f32; 3],
    pub view: u32,
    pub path_range: f32,
    pub reflections: bool,
    pub color_bands: bool,
    pub checker_size: f32,
    #[serde(default = "SkyConfig::legacy")]
    pub sky: SkyConfig,
}

impl Default for OpticsConfig {
    fn default() -> Self {
        Self {
            model: crate::ocean::parameters::model::ModelConfig::default(),
            shore: crate::ocean::parameters::shore::ShoreConfig::default(),
            spray: crate::ocean::parameters::spray::SprayConfig::default(),
            depth: 6.0,
            beach: false,
            beach_slope: 0.18,
            sand_tile_size: 4.0,
            sand_rotation_deg: 0.0,
            shore_x: -20.0,
            sigma_t: [0.18, 0.06, 0.025],
            source: [0.0025, 0.009, 0.006],
            view: 0,
            path_range: 50.0,
            reflections: true,
            color_bands: false,
            checker_size: 4.0,
            sky: SkyConfig::default(),
        }
    }
}

impl OpticsConfig {
    pub fn beach_scene() -> Self {
        Self {
            beach: true,
            source: [0.0; 3],
            sigma_t: [0.22, 0.075, 0.035],
            path_range: 35.0,
            sky: SkyConfig {
                albedo: [0.25, 0.55, 0.75],
                steps: 32,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn model_scene() -> Self {
        let mut optics = Self::beach_scene();
        optics.model.enabled = true;
        optics
    }

    pub fn swe_enabled(&self) -> bool {
        (self.beach || self.model.enabled) && self.shore.enabled
    }

    pub fn validate(&self) -> Result<(), String> {
        self.model.validate()?;
        self.shore.validate()?;
        self.spray.validate()?;
        self.sky.validate()?;
        if !self.beach_slope.is_finite()
            || !(0.01..=1.0).contains(&self.beach_slope)
            || !self.shore_x.is_finite()
            || self.shore_x.abs() > 1000.0
        {
            return Err("beach slope must be 0.01..1; shoreline must be within +/-1000 m".into());
        }
        if !self.sand_tile_size.is_finite() || !(0.25..=128.0).contains(&self.sand_tile_size) {
            return Err("sand tile size must be finite and in 0.25..=128 m".into());
        }
        if !self.sand_rotation_deg.is_finite()
            || !(-180.0..=180.0).contains(&self.sand_rotation_deg)
        {
            return Err("sand rotation must be finite and in -180..=180 degrees".into());
        }
        for (name, value) in [
            ("seabed depth", self.depth),
            ("path view range", self.path_range),
            ("checker size", self.checker_size),
        ] {
            if !value.is_finite() || value <= 0.0 || value > 1000.0 {
                return Err(format!("{name} must be finite and in (0, 1000]"));
            }
        }
        for (name, rgb) in [("sigma_t", self.sigma_t), ("source", self.source)] {
            if rgb.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > 10.0) {
                return Err(format!("{name} RGB must be finite and in 0..=10"));
            }
        }
        if self.view >= VIEW_NAMES.len() as u32 {
            return Err("water view must be in 0..=11".into());
        }
        Ok(())
    }
}

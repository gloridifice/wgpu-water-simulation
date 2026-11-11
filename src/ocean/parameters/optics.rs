use crate::ocean::parameters::sky::SkyConfig;
pub const VIEW_NAMES: [&str; 16] = [
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
    "焦散入射光（线性 RGB）",
    "折射方向",
    "折射命中（绿／红）",
    "仅 SSR（未命中黑色）",
];

/// Kept separate from the legacy enable flags so old enabled configs stay physical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpticalMethod {
    Fast,
    #[default]
    Physical,
}
impl OpticalMethod {
    pub fn code(self, enabled: bool) -> f32 {
        if !enabled {
            0.0
        } else if self == Self::Physical {
            1.0
        } else {
            2.0
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OpticsConfig {
    pub lake: crate::ocean::parameters::lake::LakeConfig,
    pub post_process: crate::ocean::parameters::post_process::PostProcessConfig,
    pub model: crate::ocean::parameters::model::ModelConfig,
    pub shore: crate::ocean::parameters::shore::ShoreConfig,
    pub spray: crate::ocean::parameters::spray::SprayConfig,
    /// Opt-in so legacy sessions retain their appearance.
    pub caustics: bool,
    pub caustics_strength: f32,
    /// World-space receiver square width in meters.
    pub caustics_extent: f32,
    /// Filter radius in caustic-map texels.
    pub caustics_softness: f32,
    pub caustics_resolution: u32,
    pub refraction: bool,
    pub refraction_method: OpticalMethod,
    pub fast_refraction_strength: f32,
    /// Water thickness for full distortion, in meters.
    pub fast_refraction_depth: f32,
    /// Maximum screen displacement as a fraction of viewport height.
    pub fast_refraction_limit: f32,
    pub water_ior: f32,
    /// Maximum refracted water path in meters.
    pub refraction_distance: f32,
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
    pub screen_space_reflections: bool,
    /// Maximum reflected ray length in meters.
    pub reflection_distance: f32,
    /// Blend weight of a valid SSR hit against the sky reflection.
    pub reflection_strength: f32,
    /// Maximum hierarchical depth-block visits per reflected ray.
    pub reflection_steps: u32,
    /// Multiplier of the pixel-footprint-based intersection tolerance.
    pub reflection_tolerance: f32,
    /// Screen-edge fade width in render-target pixels; zero disables fading.
    pub reflection_edge_fade: f32,
    pub color_bands: bool,
    pub checker_size: f32,
    #[serde(default = "SkyConfig::legacy")]
    pub sky: SkyConfig,
}

impl Default for OpticsConfig {
    fn default() -> Self {
        Self {
            lake: crate::ocean::parameters::lake::LakeConfig::default(),
            post_process: crate::ocean::parameters::post_process::PostProcessConfig::default(),
            model: crate::ocean::parameters::model::ModelConfig::default(),
            shore: crate::ocean::parameters::shore::ShoreConfig::default(),
            spray: crate::ocean::parameters::spray::SprayConfig::default(),
            caustics: false,
            caustics_strength: 1.0,
            caustics_extent: 128.0,
            caustics_softness: 1.0,
            caustics_resolution: 256,
            refraction: false,
            refraction_method: OpticalMethod::Physical,
            fast_refraction_strength: 1.0,
            fast_refraction_depth: 2.0,
            fast_refraction_limit: 0.04,
            water_ior: 1.333,
            refraction_distance: 100.0,
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
            screen_space_reflections: false,
            reflection_distance: 100.0,
            reflection_strength: 1.0,
            reflection_steps: 128,
            reflection_tolerance: 1.0,
            reflection_edge_fade: 4.0,
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

    pub fn lake_scene() -> Self {
        let mut config = Self::default();
        config.lake.enabled = true;
        config.lake.fft_strength = 0.2;
        config
    }

    pub fn swe_enabled(&self) -> bool {
        !self.lake.enabled && (self.beach || self.model.enabled) && self.shore.enabled
    }

    pub fn validate(&self) -> Result<(), String> {
        self.lake.validate()?;
        for (name, value, low, high) in [
            ("SSR strength", self.reflection_strength, 0.0, 1.0),
            (
                "SSR tolerance multiplier",
                self.reflection_tolerance,
                0.25,
                4.0,
            ),
            (
                "SSR edge fade (pixels)",
                self.reflection_edge_fade,
                0.0,
                128.0,
            ),
            (
                "fast refraction strength",
                self.fast_refraction_strength,
                0.0,
                2.0,
            ),
            (
                "fast refraction depth (m)",
                self.fast_refraction_depth,
                0.01,
                20.0,
            ),
            (
                "fast refraction limit",
                self.fast_refraction_limit,
                0.0,
                0.2,
            ),
            ("caustics strength", self.caustics_strength, 0.0, 4.0),
            ("caustics extent (m)", self.caustics_extent, 8.0, 512.0),
            (
                "caustics softness (texels)",
                self.caustics_softness,
                0.0,
                4.0,
            ),
            ("water refractive index", self.water_ior, 1.0, 1.6),
            (
                "reflection distance (m)",
                self.reflection_distance,
                1.0,
                500.0,
            ),
            (
                "refraction distance (m)",
                self.refraction_distance,
                1.0,
                500.0,
            ),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..={high}"));
            }
        }
        if !(16..=256).contains(&self.reflection_steps) {
            return Err("SSR steps must be in 16..=256".into());
        }
        if ![128, 256, 512].contains(&self.caustics_resolution) {
            return Err("caustics resolution must be 128, 256 or 512".into());
        }
        self.post_process.validate()?;
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
            return Err("water view must be in 0..=15".into());
        }
        Ok(())
    }
}

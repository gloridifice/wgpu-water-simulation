use crate::{interactions::InteractionSettings, scene::Player};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum Reconstruction {
    #[default]
    Bilinear = 0,
    CubicBSpline = 1,
    PreSmoothed = 2,
    SmoothStep = 3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogSettings {
    pub base_height: f32,
    pub noise_amplitude: f32,
    pub noise_frequency: f32,
    pub noise_octaves: u32,
    pub noise_seed: u32,
    pub height_offset: f32,
    pub noise_offset: [f32; 2],
    pub uniform_top: bool,
    pub gravity_enabled: bool,
    pub reduced_gravity: f32,
    pub wind: [f32; 2],
    pub wind_response: f32,
    pub decay_enabled: bool,
    pub half_life: f32,
    pub diffusion: f32,
    pub extinction: f32,
    pub brightness: f32,
    pub spatial_order: u32,
    pub closed_boundary: bool,
}
impl Default for FogSettings {
    fn default() -> Self {
        Self {
            base_height: 0.25,
            noise_amplitude: 0.25,
            noise_frequency: 0.32,
            noise_octaves: 3,
            noise_seed: 7,
            height_offset: -0.2,
            noise_offset: [0.0; 2],
            uniform_top: false,
            gravity_enabled: true,
            reduced_gravity: 0.02,
            wind: [0.05, 0.0],
            wind_response: 20.0,
            decay_enabled: true,
            half_life: 300.0,
            diffusion: 0.0,
            extinction: 4.0,
            brightness: 0.8,
            spatial_order: 2,
            closed_boundary: false,
        }
    }
}
pub fn range(name: &str, v: f32, lo: f32, hi: f32) -> Result<(), String> {
    if !v.is_finite() || !(lo..=hi).contains(&v) {
        Err(format!("{name} 必须在 {lo}..{hi} 内且为有限数"))
    } else {
        Ok(())
    }
}
impl FogSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (name, v, lo, hi) in [
            ("基础高度", self.base_height, 0.0, 4.0),
            ("振幅", self.noise_amplitude, 0.0, 2.0),
            ("频率", self.noise_frequency, 0.001, 4.0),
            ("高度偏移", self.height_offset, -2.0, 2.0),
            ("重力", self.reduced_gravity, 0.0, 1.0),
            ("风响应", self.wind_response, 0.1, 600.0),
            ("半衰期", self.half_life, 1.0, 3600.0),
            ("扩散", self.diffusion, 0.0, 1.0),
            ("消光", self.extinction, 0.0, 40.0),
            ("亮度", self.brightness, 0.0, 4.0),
        ] {
            range(name, v, lo, hi)?;
        }
        for v in self.noise_offset {
            range("噪波偏移", v, -1000.0, 1000.0)?;
        }
        if !self.wind.iter().all(|v| v.is_finite())
            || self.wind[0].hypot(self.wind[1]) > 2.0
            || !(1..=6).contains(&self.noise_octaves)
            || self.noise_seed > 65535
            || !(1..=2).contains(&self.spatial_order)
        {
            return Err("风、噪波层数/种子或空间阶数无效".into());
        }
        Ok(())
    }
    pub fn gpu(&self, domain: [f32; 4], dt: f32) -> [[f32; 4]; 8] {
        [
            domain,
            [
                self.reduced_gravity,
                self.wind_response,
                self.half_life,
                self.diffusion,
            ],
            [self.wind[0], self.wind[1], self.base_height, 0.0],
            [
                self.base_height,
                self.noise_amplitude,
                self.noise_frequency,
                self.height_offset,
            ],
            [self.noise_seed as f32, self.noise_octaves as f32, 2.0, 0.5],
            [
                self.noise_offset[0],
                self.noise_offset[1],
                u32::from(self.uniform_top) as f32,
                self.spatial_order as f32,
            ],
            [
                self.extinction,
                self.brightness,
                u32::from(self.gravity_enabled) as f32,
                u32::from(self.decay_enabled) as f32,
            ],
            [dt, u32::from(self.closed_boundary) as f32, 0.0, 0.0],
        ]
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    /// Explicit simulation grid override; None keeps the validated terrain grid.
    pub grid_resolution: Option<[u32; 2]>,
    /// Render-only reconstruction; switching preserves the complete simulation.
    pub reconstruction: Reconstruction,
    /// Dimensionless blend between raw h/m and a barrier-aware 3x3 render filter.
    pub reconstruction_smoothing: f32,
    /// Render-only base segments per crossed XZ cell before adaptive refinement, 2..=32.
    pub integration_steps: u32,
    pub fog: FogSettings,
    pub interaction: InteractionSettings,
    pub player: Player,
    /// yaw, downward pitch, follow distance (radians, radians, metres).
    pub camera: [f32; 3],
    pub paused: bool,
    pub visible: bool,
    pub interactions_enabled: bool,
    pub debug_view: u32,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            version: 1,
            grid_resolution: None,
            reconstruction: Reconstruction::default(),
            reconstruction_smoothing: 0.6,
            integration_steps: 2,
            fog: FogSettings::default(),
            interaction: InteractionSettings::default(),
            player: Player::default(),
            camera: [0.0, 0.30, 5.5],
            paused: false,
            visible: true,
            interactions_enabled: true,
            debug_view: 0,
        }
    }
}
impl common::settings::Settings for Session {
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("不支持的雾配置版本".into());
        }
        if self
            .grid_resolution
            .is_some_and(|r| r.iter().any(|n| !(2..=1024).contains(n)))
        {
            return Err("网格分辨率每轴必须为 2–1024".into());
        }
        self.fog.validate()?;
        range("渲染平滑强度", self.reconstruction_smoothing, 0.0, 1.0)?;
        if !(2..=32).contains(&self.integration_steps) {
            return Err("积分精度必须为每格 2–32 段".into());
        }
        self.interaction.validate()?;
        self.player.validate()?;
        range("相机 yaw", self.camera[0], -10000.0, 10000.0)?;
        range("相机 pitch", self.camera[1], 0.05, 1.35)?;
        range("相机距离", self.camera[2], 1.5, 20.0)?;
        if self.debug_view > 5 {
            return Err("无效诊断视图".into());
        }
        Ok(())
    }
}

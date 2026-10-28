#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SkyConfig {
    pub enabled: bool,
    pub albedo: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub steps: u32,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            albedo: [0.3, 0.5, 0.65],
            color: [1.0; 3],
            intensity: 1.0,
            steps: 16,
        }
    }
}

impl SkyConfig {
    /// Old JSON without a sky section keeps its previous rendered appearance.
    pub fn legacy() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .albedo
            .iter()
            .chain(&self.color)
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
        {
            return Err("sky color and scattering albedo RGB must be in 0..=1".into());
        }
        if !self.intensity.is_finite() || !(0.0..=20.0).contains(&self.intensity) {
            return Err("sky intensity must be finite and in 0..=20".into());
        }
        if ![8, 16, 32].contains(&self.steps) {
            return Err("sky integration steps must be 8, 16 or 32".into());
        }
        Ok(())
    }

    pub fn tint(&self) -> [f32; 4] {
        [self.color[0], self.color[1], self.color[2], self.intensity]
    }
}

/// A periodic ocean tile in meters and seconds. Frequency arrays use unshifted FFT ordering.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OceanConfig {
    pub resolution: u32,
    pub length: f32,
    pub phillips_length: f32,
    pub amplitude: f32,
    pub mesh_resolution: u32,
    /// Render-only staggered near-equilateral mesh; missing legacy fields stay square.
    pub triangular_mesh: bool,
    pub wind_angle: f32,
    pub seed: u64,
}

impl Default for OceanConfig {
    fn default() -> Self {
        Self {
            resolution: 128,
            length: 128.0,
            phillips_length: 14.0 * 14.0 / 9.81,
            amplitude: 0.002,
            mesh_resolution: 128,
            triangular_mesh: false,
            wind_angle: 0.5,
            seed: 42,
        }
    }
}

impl OceanConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.resolution.is_power_of_two() || !(8..=512).contains(&self.resolution) {
            return Err("resolution must be a power of two in 8..=512".into());
        }
        for (name, value) in [
            ("length", self.length),
            ("Phillips L", self.phillips_length),
        ] {
            if !value.is_finite() || value <= 0.0 {
                return Err(format!("{name} must be finite and positive"));
            }
        }
        if !self.amplitude.is_finite() || !(0.0..=1.0).contains(&self.amplitude) {
            return Err("Phillips A must be finite and in 0..=1".into());
        }
        if !(8..=512).contains(&self.mesh_resolution) {
            return Err("mesh resolution must be in 8..=512".into());
        }
        if !self.wind_angle.is_finite() {
            return Err("wind angle must be finite".into());
        }
        Ok(())
    }
}

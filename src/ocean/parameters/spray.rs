#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SprayConfig {
    pub enabled: bool,
    pub debug: bool,
    /// Particles / m² / simulated second at full source weight.
    pub rate: f32,
    pub slope: f32,
    pub rise: f32,
    pub depth: f32,
    pub upward: f32,
    pub forward: f32,
    pub lifetime: f32,
    pub size: f32,
    /// Inclusive bounds of the random multiplier applied to the base diameter.
    pub size_range: [f32; 2],
    pub drag: f32,
    pub softness: f32,
}
impl Default for SprayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            debug: false,
            rate: 150.0,
            slope: 0.03,
            rise: 0.02,
            depth: 0.02,
            upward: 2.0,
            forward: 0.5,
            lifetime: 1.2,
            size: 0.2,
            size_range: [0.65, 1.35],
            drag: 0.5,
            softness: 0.15,
        }
    }
}
impl SprayConfig {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, min, max) in [
            ("rate", self.rate, 0.0, 2000.0),
            ("slope", self.slope, 0.0, 2.0),
            ("rise", self.rise, 0.0, 5.0),
            ("depth", self.depth, 0.001, 1.0),
            ("upward", self.upward, 0.0, 10.0),
            ("forward", self.forward, 0.0, 10.0),
            ("lifetime", self.lifetime, 0.05, 5.0),
            ("size", self.size, 0.01, 2.0),
            ("size multiplier minimum", self.size_range[0], 0.0, 2.0),
            ("size multiplier maximum", self.size_range[1], 0.0, 2.0),
            ("drag", self.drag, 0.0, 10.0),
            ("softness", self.softness, 0.001, 1.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("spray {name} must be finite in {min}..={max}"));
            }
        }
        if self.size_range[0] > self.size_range[1] {
            return Err("spray size multiplier minimum must not exceed maximum".into());
        }
        Ok(())
    }
}

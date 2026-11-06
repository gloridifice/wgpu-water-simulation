//! Display-only HDR/Bloom controls; legacy disabled sessions remain bypassed.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PostProcessConfig {
    pub enabled: bool,
    /// AgX display mapping, independent of Bloom; gated by the master switch.
    pub tonemapping_enabled: bool,
    /// Bloom can be used with or without tone mapping; strength zero also skips it.
    pub bloom_enabled: bool,
    /// Exposure compensation in stops, applied before the bright-pass threshold.
    pub exposure: f32,
    pub strength: f32,
    /// Exposed linear RGB peak, not an sRGB slider value.
    pub threshold: f32,
    /// Soft transition width as a fraction of the threshold.
    pub soft_knee: f32,
    /// Upsampling filter radius in pyramid texels.
    pub radius: f32,
}

impl Default for PostProcessConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tonemapping_enabled: true,
            bloom_enabled: true,
            exposure: 0.0,
            strength: 0.1,
            threshold: 1.0,
            soft_knee: 0.5,
            radius: 1.0,
        }
    }
}

impl PostProcessConfig {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value, low, high) in [
            ("HDR exposure (stops)", self.exposure, -8.0, 8.0),
            ("Bloom strength", self.strength, 0.0, 2.0),
            ("Bloom threshold", self.threshold, 0.0, 20.0),
            ("Bloom soft knee", self.soft_knee, 0.0, 1.0),
            ("Bloom radius (texels)", self.radius, 0.5, 2.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..={high}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::ocean::parameters::optics::OpticsConfig;

    #[test]
    fn legacy_and_partial_configs_preserve_optics_and_validate_bloom() {
        let old: OpticsConfig = serde_json::from_str(r#"{"depth":17.0}"#).unwrap();
        assert_eq!(old.depth, 17.0);
        assert!(!old.post_process.enabled);
        let partial: OpticsConfig = serde_json::from_str(
            r#"{"depth":17.0,"post_process":{"enabled":true,"strength":0.7}}"#,
        )
        .unwrap();
        partial.validate().unwrap();
        assert_eq!(partial.depth, 17.0);
        assert!(partial.post_process.enabled);
        assert_eq!(partial.post_process.strength, 0.7);
        assert!(partial.post_process.tonemapping_enabled);
        assert!(partial.post_process.bloom_enabled);
        for (mapping, bloom) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut config = partial;
            config.post_process.tonemapping_enabled = mapping;
            config.post_process.bloom_enabled = bloom;
            let saved = serde_json::to_string(&config).unwrap();
            assert_eq!(
                serde_json::from_str::<OpticsConfig>(&saved).unwrap(),
                config
            );
        }
        for bad in [f32::NAN, f32::INFINITY, -0.1, 20.1] {
            let mut config = partial;
            config.post_process.threshold = bad;
            assert!(config.validate().is_err(), "threshold={bad}");
        }
        for bad in [f32::NEG_INFINITY, -8.1, 8.1] {
            let mut config = partial;
            config.post_process.exposure = bad;
            assert!(config.validate().is_err(), "exposure={bad}");
        }
    }
}

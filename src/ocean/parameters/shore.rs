#[cfg(test)]
use crate::ocean::parameters::optics::OpticsConfig;
pub const RESOLUTION: u32 = 256;
pub const RESOLUTIONS: [u32; 5] = [64, 128, 256, 512, 1024];
pub const FOAM_TEXTURE_NAMES: [&str; 4] = [
    "Original Grayscale",
    "Shorewaves NSH",
    "Crest Foam NSH",
    "Crest Foam2 NSH",
];
pub const FOAM_MATERIAL_VIEWS: [&str; 6] = [
    "Normal Shading",
    "Soft Pattern",
    "Height Pattern",
    "Foam Normals",
    "Opacity (Black: Clear / White: Opaque)",
    "Transport Coordinate Grid",
];
pub const VIEWS: [(u32, &str); 10] = [
    (0, "Normal Shading"),
    (1, "Water Depth"),
    (2, "Flow Direction"),
    (3, "Nearshore Foam"),
    (4, "Sand Wetness"),
    (6, "Foam Density (Low: Green / High: Red)"),
    (7, "Actual Front Source Rate (Last Substep)"),
    (8, "Compression Ridge Emission Weight"),
    (9, "Fresh Foam N Density"),
    (10, "Surface Residual R Density"),
];

fn deserialize_view<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let view = <u32 as serde::Deserialize>::deserialize(deserializer)?;
    // Removed sand-foam view falls back without discarding the saved session.
    Ok(if view == 5 { 0 } else { view })
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShoreConfig {
    pub colliders: [Option<crate::ocean::scene::collider::Collider>;
        crate::ocean::scene::collider::MAX_COLLIDERS],
    pub enabled: bool,
    pub resolution: u32,
    pub order: u32,
    pub input_gain: f32,
    pub filter_enabled: bool,
    pub filter_cutoff: f32,
    pub filter_transition: f32,
    pub normal_strength: f32,
    pub water_foam_color: [f32; 3],
    pub foam_texture: u32,
    pub foam_texture_size: f32,
    pub foam_micro_normal: f32,
    pub foam_opacity_scale: f32,
    pub foam_roughness: f32,
    pub foam_material_view: u32,
    pub foam_prototype: bool,
    pub foam_texture_period: f32,
    pub foam_texture_distortion: bool,
    pub foam_dual_phase: bool,
    // Render only: 0 both, 1 fresh N, 2 residual R.
    pub foam_species_view: u32,
    // Render only: 0 constant threshold, 1 two-octave noise (legacy default), 2 NSH.
    pub foam_fresh_threshold_mode: u32,
    pub foam_fresh_threshold: f32,
    pub foam_fresh_noise_frequency: f32,
    pub foam_fresh_noise_detail_frequency: f32,
    pub foam_fresh_noise_detail_weight: f32,
    // Independent N-only NSH appearance; never changes the stored concentrations.
    pub foam_fresh_texture: u32,
    pub foam_fresh_texture_size: f32,
    pub foam_fresh_micro_normal: f32,
    pub foam_fresh_roughness: f32,
    pub foam_fresh_coverage_reference: f32,
    pub foam_fresh_density_opacity: bool,
    pub foam_fresh_opacity_range: f32,
    pub foam_fresh_opacity_power: f32,
    pub foam_residual_density_opacity: bool,
    pub foam_residual_opacity_range: f32,
    pub foam_residual_opacity_power: f32,
    pub foam_fresh_half_life: f32,
    pub foam_residual_half_life: f32,
    pub foam_conversion: f32,
    pub breaking_depth: f32,
    pub generation: f32,
    pub foam_generation_threshold: f32,
    pub foam_generation_transition: f32,
    pub foam_front_generation: f32,
    pub foam_front_distance: f32,
    pub foam_ridge_enabled: bool,
    pub foam_ridge_width: f32,
    pub foam_ridge_prominence: f32,
    pub foam_emission_enabled: bool,
    // Velocity differences divided by their world-space sampling distance (/s).
    pub foam_velocity_threshold: f32,
    pub foam_velocity_transition: f32,
    pub foam_background_multiplier: f32,
    pub foam_follow: f32,
    pub foam_response_time: f32,
    pub foam_drag_enabled: bool,
    pub foam_birth_speed: f32,
    pub foam_transport_order: u32,
    pub decay: f32,
    pub foam_compression_threshold: f32,
    pub foam_density_reference: f32,
    // Render-only mapping from unbounded surface amount/area to vertex lift.
    pub foam_height_max: f32,
    pub foam_height_threshold: f32,
    pub foam_height_transition: f32,
    pub drying_time: f32,
    #[serde(deserialize_with = "deserialize_view")]
    pub view: u32,
}
impl Default for ShoreConfig {
    fn default() -> Self {
        Self {
            colliders: [None; crate::ocean::scene::collider::MAX_COLLIDERS],
            enabled: true,
            resolution: RESOLUTION,
            order: 1,
            input_gain: 1.0,
            filter_enabled: true,
            filter_cutoff: 12.0,
            filter_transition: 12.0,
            normal_strength: 1.0,
            water_foam_color: [0.87, 0.92, 0.90],
            foam_texture: 1,
            foam_texture_size: 12.0,
            foam_micro_normal: 0.65,
            foam_opacity_scale: 2.0,
            foam_roughness: 0.85,
            foam_material_view: 0,
            foam_prototype: false,
            foam_texture_period: 2.0,
            foam_texture_distortion: true,
            foam_dual_phase: false,
            foam_species_view: 0,
            foam_fresh_threshold_mode: 1,
            foam_fresh_threshold: 0.5,
            foam_fresh_noise_frequency: 0.8,
            foam_fresh_noise_detail_frequency: 2.5,
            foam_fresh_noise_detail_weight: 0.3,
            foam_fresh_texture: 1,
            foam_fresh_texture_size: 12.0,
            foam_fresh_micro_normal: 0.65,
            foam_fresh_roughness: 0.85,
            foam_fresh_coverage_reference: 2.0,
            foam_fresh_density_opacity: false,
            foam_fresh_opacity_range: 1.0,
            foam_fresh_opacity_power: 1.0,
            foam_residual_density_opacity: false,
            foam_residual_opacity_range: 1.0,
            foam_residual_opacity_power: 1.0,
            foam_fresh_half_life: 1.4,
            foam_residual_half_life: 9.0,
            foam_conversion: 0.65,
            breaking_depth: 2.0,
            generation: 4.0,
            foam_generation_threshold: 0.25,
            foam_generation_transition: 0.4,
            foam_front_generation: 8.0,
            foam_front_distance: 1.0,
            foam_ridge_enabled: false,
            foam_ridge_width: 1.0,
            foam_ridge_prominence: 0.02,
            foam_emission_enabled: true,
            foam_velocity_threshold: 0.1,
            foam_velocity_transition: 0.5,
            foam_background_multiplier: 0.15,
            foam_follow: 0.8,
            foam_response_time: 0.35,
            foam_drag_enabled: true,
            foam_birth_speed: 1.0,
            foam_transport_order: 1,
            decay: 0.45,
            foam_compression_threshold: 0.1,
            foam_density_reference: 2.0,
            foam_height_max: 0.05,
            foam_height_threshold: 0.1,
            foam_height_transition: 1.9,
            drying_time: 18.0,
            view: 0,
        }
    }
}
impl ShoreConfig {
    /// Physical-model starting point, not a calibrated seawater material preset.
    pub fn apply_foam_reference(&mut self) {
        self.foam_drag_enabled = true;
        self.foam_follow = 1.0;
        self.foam_response_time = 2.0;
        self.foam_birth_speed = 1.0;
        self.decay = 0.3;
    }

    pub fn validate(self) -> Result<(), String> {
        if !RESOLUTIONS.contains(&self.resolution) {
            return Err("SWE resolution must be 64, 128, 256, 512 or 1024".into());
        }
        for collider in self.colliders.iter().flatten() {
            collider.validate()?;
        }
        for (name, value, low, high) in [
            ("SWE input gain", self.input_gain, 0.0, 3.0),
            ("SWE filter cutoff", self.filter_cutoff, 0.0, 512.0),
            ("SWE filter transition", self.filter_transition, 0.0, 512.0),
            ("shore normal strength", self.normal_strength, 0.0, 3.0),
            ("foam texture size (m)", self.foam_texture_size, 0.25, 64.0),
            (
                "foam texture period (s; 0 disables resets)",
                self.foam_texture_period,
                0.0,
                60.0,
            ),
            ("foam micro normal", self.foam_micro_normal, 0.0, 3.0),
            (
                "foam opacity density scale",
                self.foam_opacity_scale,
                0.01,
                20.0,
            ),
            ("foam roughness", self.foam_roughness, 0.15, 1.0),
            (
                "fresh foam constant threshold",
                self.foam_fresh_threshold,
                0.0,
                1.0,
            ),
            (
                "fresh foam noise frequency (/m)",
                self.foam_fresh_noise_frequency,
                0.05,
                20.0,
            ),
            (
                "fresh foam detail frequency (/m)",
                self.foam_fresh_noise_detail_frequency,
                0.05,
                60.0,
            ),
            (
                "fresh foam detail weight",
                self.foam_fresh_noise_detail_weight,
                0.0,
                1.0,
            ),
            (
                "fresh foam texture size (m)",
                self.foam_fresh_texture_size,
                0.25,
                64.0,
            ),
            (
                "fresh foam micro normal",
                self.foam_fresh_micro_normal,
                0.0,
                3.0,
            ),
            ("fresh foam roughness", self.foam_fresh_roughness, 0.15, 1.0),
            (
                "fresh foam coverage reference",
                self.foam_fresh_coverage_reference,
                0.01,
                20.0,
            ),
            (
                "fresh opacity concentration range",
                self.foam_fresh_opacity_range,
                0.0,
                10.0,
            ),
            (
                "fresh opacity power",
                self.foam_fresh_opacity_power,
                0.1,
                5.0,
            ),
            (
                "residual opacity concentration range",
                self.foam_residual_opacity_range,
                0.0,
                10.0,
            ),
            (
                "residual opacity power",
                self.foam_residual_opacity_power,
                0.1,
                5.0,
            ),
            ("breaking depth", self.breaking_depth, 0.5, 5.0),
            ("shore foam generation", self.generation, 0.0, 15.0),
            (
                "foam generation threshold",
                self.foam_generation_threshold,
                0.0,
                2.0,
            ),
            (
                "foam generation transition",
                self.foam_generation_transition,
                0.0,
                2.0,
            ),
            (
                "ridge emission width (m)",
                self.foam_ridge_width,
                0.05,
                10.0,
            ),
            (
                "ridge relative prominence",
                self.foam_ridge_prominence,
                0.0,
                1.0,
            ),
            (
                "fresh foam half-life (s)",
                self.foam_fresh_half_life,
                0.1,
                60.0,
            ),
            (
                "residual foam half-life (s)",
                self.foam_residual_half_life,
                0.1,
                120.0,
            ),
            ("fresh to residual fraction", self.foam_conversion, 0.0, 1.0),
            ("shore foam decay", self.decay, 0.0, 4.0),
            ("foam follow multiplier", self.foam_follow, 0.0, 1.0),
            ("foam response time", self.foam_response_time, 0.0, 20.0),
            ("foam birth speed", self.foam_birth_speed, 0.0, 1.0),
            ("foam height maximum (m)", self.foam_height_max, 0.0, 1.0),
            (
                "foam height threshold",
                self.foam_height_threshold,
                0.0,
                20.0,
            ),
            (
                "foam height transition",
                self.foam_height_transition,
                0.001,
                20.0,
            ),
            (
                "front foam generation",
                self.foam_front_generation,
                0.0,
                30.0,
            ),
            (
                "front sampling distance",
                self.foam_front_distance,
                0.05,
                10.0,
            ),
            (
                "foam velocity threshold",
                self.foam_velocity_threshold,
                0.0,
                20.0,
            ),
            (
                "foam velocity transition",
                self.foam_velocity_transition,
                0.0,
                20.0,
            ),
            (
                "background foam multiplier",
                self.foam_background_multiplier,
                0.0,
                1.0,
            ),
            (
                "foam compression threshold",
                self.foam_compression_threshold,
                0.0,
                4.0,
            ),
            (
                "foam density reference",
                self.foam_density_reference,
                0.01,
                20.0,
            ),
            ("sand drying time", self.drying_time, 1.0, 120.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..={high}"));
            }
        }
        if self
            .water_foam_color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("foam color must be finite linear RGB in 0..=1".into());
        }
        if !(1..=2).contains(&self.order) {
            return Err("SWE order must be 1 or 2".into());
        }
        if !(1..=2).contains(&self.foam_transport_order) {
            return Err("foam transport order must be 1 or 2".into());
        }
        if self.foam_texture_period > 0.0 && self.foam_texture_period < 0.25 {
            return Err("foam texture period must be 0 or 0.25..=60 seconds".into());
        }
        if self.foam_texture >= FOAM_TEXTURE_NAMES.len() as u32
            || self.foam_material_view >= FOAM_MATERIAL_VIEWS.len() as u32
            || self.foam_species_view > 2
            || self.foam_fresh_threshold_mode > 2
            || !(1..FOAM_TEXTURE_NAMES.len() as u32).contains(&self.foam_fresh_texture)
        {
            return Err("invalid foam material selection".into());
        }
        if !VIEWS.iter().any(|&(id, _)| id == self.view) {
            return Err("invalid shore view".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn removed_flow_source_fields_do_not_discard_saved_foam_settings() {
        let saved: super::ShoreConfig = serde_json::from_str(
            r#"{"generation":2.5,"foam_front_generation":7.0,"foam_source_mode":1,"foam_flow_slope_gain":12.0,"foam_flow_speed_gain":0.7,"foam_flow_compression_gain":1.5,"foam_flow_bias":-0.3,"foam_flow_depth_mm":125.0}"#,
        )
        .unwrap();
        saved.validate().unwrap();
        assert_eq!(saved.generation, 2.5);
        assert_eq!(saved.foam_front_generation, 7.0);
    }

    #[test]
    fn foam_material_legacy_defaults_and_invalid_inputs() {
        let old: super::ShoreConfig = serde_json::from_str(
            r#"{"generation":2.5,"foam_soft_opacity":0.4,"foam_thick_start":0.6,"foam_material_view":4}"#,
        ).unwrap();
        old.validate().unwrap();
        assert_eq!(old.generation, 2.5);
        assert_eq!(old.foam_opacity_scale, 2.0);
        assert_eq!(old.foam_material_view, 4);
        assert_eq!(old.foam_texture, 1);
        assert_eq!(old.foam_texture_size, 12.0);
        assert!(!old.foam_prototype);
        assert_eq!(old.foam_texture_period, 2.0);
        assert!(old.foam_texture_distortion);
        let fixed = super::ShoreConfig {
            foam_texture_distortion: false,
            ..old
        };
        let restored: super::ShoreConfig =
            serde_json::from_str(&serde_json::to_string(&fixed).unwrap()).unwrap();
        assert_eq!(fixed, restored);
        assert_eq!(old.foam_species_view, 0);
        for view in 0..=2 {
            let config = super::ShoreConfig {
                foam_species_view: view,
                ..old
            };
            config.validate().unwrap();
            let restored: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(config, restored);
        }
        assert!(
            super::ShoreConfig {
                foam_species_view: 3,
                ..old
            }
            .validate()
            .is_err()
        );
        for period in [0.0, 0.25, 8.0, 60.0] {
            let config = super::ShoreConfig {
                foam_prototype: true,
                foam_texture_period: period,
                foam_material_view: 5,
                ..old
            };
            config.validate().unwrap();
            let restored: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(config, restored);
        }
        for period in [-1.0, 0.01, 60.1, f32::NAN, f32::INFINITY] {
            assert!(
                super::ShoreConfig {
                    foam_texture_period: period,
                    ..old
                }
                .validate()
                .is_err()
            );
        }
        for source in 0..super::FOAM_TEXTURE_NAMES.len() as u32 {
            let config = super::ShoreConfig {
                foam_texture: source,
                foam_material_view: 4,
                ..old
            };
            config.validate().unwrap();
            let loaded: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(config, loaded);
        }
        for config in [
            super::ShoreConfig {
                foam_texture: 4,
                ..old
            },
            super::ShoreConfig {
                foam_material_view: 6,
                ..old
            },
            super::ShoreConfig {
                foam_texture_size: 0.0,
                ..old
            },
            super::ShoreConfig {
                foam_micro_normal: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: 0.0,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: 20.01,
                ..old
            },
            super::ShoreConfig {
                foam_roughness: 0.0,
                ..old
            },
        ] {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn fresh_appearance_settings_preserve_old_configs_and_validate_all_modes() {
        let old: super::ShoreConfig = serde_json::from_str(r#"{"generation":2.5}"#).unwrap();
        assert_eq!(old.generation, 2.5);
        assert_eq!(old.foam_fresh_threshold_mode, 1);
        assert_eq!(old.foam_fresh_threshold, 0.5);
        assert_eq!(old.foam_fresh_noise_frequency, 0.8);
        assert_eq!(old.foam_fresh_noise_detail_frequency, 2.5);
        assert_eq!(old.foam_fresh_noise_detail_weight, 0.3);
        assert_eq!(old.foam_fresh_texture, 1);
        assert_eq!(old.foam_fresh_texture_size, 12.0);
        assert_eq!(old.foam_fresh_micro_normal, 0.65);
        assert_eq!(old.foam_fresh_roughness, 0.85);
        assert_eq!(old.foam_fresh_coverage_reference, 2.0);
        assert!(!old.foam_fresh_density_opacity);
        assert!(!old.foam_residual_density_opacity);
        assert_eq!(old.foam_fresh_opacity_range, 1.0);
        assert_eq!(old.foam_residual_opacity_range, 1.0);
        assert_eq!(old.foam_fresh_opacity_power, 1.0);
        assert_eq!(old.foam_residual_opacity_power, 1.0);
        for mode in 0..=2 {
            for (source, size, normal, roughness, reference) in [
                (1, 0.25, 0.0, 0.15, 0.01),
                (2, 12.0, 0.65, 0.85, 2.0),
                (3, 64.0, 3.0, 1.0, 20.0),
            ] {
                let config = super::ShoreConfig {
                    foam_fresh_threshold_mode: mode,
                    foam_fresh_threshold: 1.0,
                    foam_fresh_noise_frequency: 20.0,
                    foam_fresh_noise_detail_frequency: 60.0,
                    foam_fresh_noise_detail_weight: 1.0,
                    foam_fresh_texture: source,
                    foam_fresh_texture_size: size,
                    foam_fresh_micro_normal: normal,
                    foam_fresh_roughness: roughness,
                    foam_fresh_coverage_reference: reference,
                    foam_fresh_density_opacity: source == 2,
                    foam_fresh_opacity_range: (source - 1) as f32 * 5.0,
                    foam_fresh_opacity_power: if source == 1 { 0.1 } else { 5.0 },
                    foam_residual_density_opacity: source != 2,
                    foam_residual_opacity_range: (3 - source) as f32 * 5.0,
                    foam_residual_opacity_power: if source == 3 { 0.1 } else { 5.0 },
                    ..old
                };
                config.validate().unwrap();
                let restored: super::ShoreConfig =
                    serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
                assert_eq!(
                    config, restored,
                    "mode {mode} must retain every mode's parameters"
                );
            }
        }
        for patch in [
            r#"{"foam_fresh_threshold_mode":3}"#,
            r#"{"foam_fresh_texture":0}"#,
            r#"{"foam_fresh_texture":4}"#,
            r#"{"foam_fresh_texture_size":0}"#,
            r#"{"foam_fresh_texture_size":64.01}"#,
            r#"{"foam_fresh_micro_normal":-0.01}"#,
            r#"{"foam_fresh_micro_normal":3.01}"#,
            r#"{"foam_fresh_roughness":0.14}"#,
            r#"{"foam_fresh_roughness":1.01}"#,
            r#"{"foam_fresh_coverage_reference":0}"#,
            r#"{"foam_fresh_coverage_reference":20.01}"#,
            r#"{"foam_fresh_opacity_range":-0.01}"#,
            r#"{"foam_fresh_opacity_range":10.01}"#,
            r#"{"foam_fresh_opacity_power":0}"#,
            r#"{"foam_fresh_opacity_power":0.09}"#,
            r#"{"foam_fresh_opacity_power":5.01}"#,
            r#"{"foam_residual_opacity_range":-0.01}"#,
            r#"{"foam_residual_opacity_range":10.01}"#,
            r#"{"foam_residual_opacity_power":0}"#,
            r#"{"foam_residual_opacity_power":0.09}"#,
            r#"{"foam_residual_opacity_power":5.01}"#,
            r#"{"foam_fresh_threshold":-0.01}"#,
            r#"{"foam_fresh_threshold":1.01}"#,
            r#"{"foam_fresh_noise_frequency":0}"#,
            r#"{"foam_fresh_noise_frequency":20.01}"#,
            r#"{"foam_fresh_noise_detail_frequency":0}"#,
            r#"{"foam_fresh_noise_detail_frequency":60.01}"#,
            r#"{"foam_fresh_noise_detail_weight":-0.01}"#,
            r#"{"foam_fresh_noise_detail_weight":1.01}"#,
        ] {
            let invalid: super::ShoreConfig = serde_json::from_str(patch).unwrap();
            assert!(
                invalid.validate().is_err(),
                "accepted invalid settings: {patch}"
            );
        }
        for invalid in [
            super::ShoreConfig {
                foam_fresh_threshold: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_noise_frequency: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_noise_detail_frequency: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_noise_detail_weight: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_texture_size: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_micro_normal: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_roughness: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_coverage_reference: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_opacity_range: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_fresh_opacity_power: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_residual_opacity_range: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_residual_opacity_power: f32::NAN,
                ..old
            },
        ] {
            assert!(
                invalid.validate().is_err(),
                "accepted nonfinite appearance settings"
            );
        }
    }

    #[test]
    fn resolution_config_compatibility_and_validation() {
        let old: super::ShoreConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(old.resolution, 256);
        for resolution in super::RESOLUTIONS {
            let config = super::ShoreConfig { resolution, ..old };
            config.validate().unwrap();
            let restored: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(restored, config);
        }
        for resolution in [0, 63, 192, 2048, u32::MAX] {
            assert!(super::ShoreConfig { resolution, ..old }.validate().is_err());
        }
    }
    use super::*;

    #[test]
    fn foam_height_settings_keep_legacy_configs_and_validate_boundaries() {
        let legacy: ShoreConfig =
            serde_json::from_str(r#"{"foam_density_reference":3.0}"#).unwrap();
        assert_eq!(legacy.foam_height_max, 0.05);
        assert_eq!(legacy.foam_height_threshold, 0.1);
        assert_eq!(legacy.foam_height_transition, 1.9);
        for (maximum, threshold, transition) in [(0.0, 0.0, 0.001), (1.0, 20.0, 20.0)] {
            let config = ShoreConfig {
                foam_height_max: maximum,
                foam_height_threshold: threshold,
                foam_height_transition: transition,
                ..legacy
            };
            config.validate().unwrap();
            let json = serde_json::to_string(&config).unwrap();
            assert_eq!(serde_json::from_str::<ShoreConfig>(&json).unwrap(), config);
        }
        for (maximum, threshold, transition) in [
            (-0.01, 0.1, 1.9),
            (1.01, 0.1, 1.9),
            (f32::NAN, 0.1, 1.9),
            (0.05, -0.1, 1.9),
            (0.05, 20.1, 1.9),
            (0.05, f32::INFINITY, 1.9),
            (0.05, 0.1, 0.0),
            (0.05, 0.1, 20.1),
            (0.05, 0.1, f32::NAN),
        ] {
            assert!(
                ShoreConfig {
                    foam_height_max: maximum,
                    foam_height_threshold: threshold,
                    foam_height_transition: transition,
                    ..legacy
                }
                .validate()
                .is_err()
            );
        }
    }
    #[test]
    fn foam_momentum_controls_are_compatible_and_persist() {
        let mut config: ShoreConfig = serde_json::from_str(r#"{"foam_follow":0.6}"#).unwrap();
        assert!(config.foam_drag_enabled);
        assert_eq!(config.foam_birth_speed, 1.0);
        assert_eq!(config.foam_follow, 0.6);
        assert_eq!(config.foam_transport_order, 1);
        config.foam_transport_order = 2;
        config.apply_foam_reference();
        assert_eq!(config.foam_transport_order, 2);
        config.validate().unwrap();
        config.foam_drag_enabled = false;
        config.foam_response_time = 20.0;
        config.foam_birth_speed = 0.4;
        config.validate().unwrap();
        let saved = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<ShoreConfig>(&saved).unwrap(), config);
        for order in [0, 3, u32::MAX] {
            assert!(
                ShoreConfig {
                    foam_transport_order: order,
                    ..config
                }
                .validate()
                .is_err()
            );
        }
        for value in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            config.foam_birth_speed = value;
            assert!(config.validate().is_err());
        }
    }
    #[test]
    fn removed_front_modes_preserve_saved_compression_controls() {
        for mode in 0..3 {
            let config: ShoreConfig = serde_json::from_value(serde_json::json!({
                "foam_front_mode": mode,
                "foam_front_depth_drop": 0.02,
                "foam_front_generation": 6.0,
                "foam_front_distance": 0.7,
                "foam_velocity_threshold": 0.35,
                "foam_velocity_transition": 1.2
            }))
            .unwrap();
            config.validate().unwrap();
            assert_eq!(config.foam_front_generation, 6.0);
            assert_eq!(config.foam_front_distance, 0.7);
            assert_eq!(config.foam_velocity_threshold, 0.35);
            assert_eq!(config.foam_velocity_transition, 1.2);
            let saved = serde_json::to_value(config).unwrap();
            assert!(saved.get("foam_front_mode").is_none());
            assert!(saved.get("foam_front_depth_drop").is_none());
            assert_eq!(
                serde_json::from_value::<ShoreConfig>(saved).unwrap(),
                config
            );
        }
    }
    #[test]
    fn removed_sand_foam_settings_preserve_wetness_and_density_view() {
        for (view, expected) in [(5, 0), (6, 6)] {
            let config: ShoreConfig = serde_json::from_value(serde_json::json!({
                "view": view, "drying_time": 27.0,
                "stranded_foam_color": [0.6, 0.4, 0.2], "foam_deposit_depth_mm": 50.0
            }))
            .unwrap();
            config.validate().unwrap();
            assert_eq!(config.view, expected);
            assert_eq!(config.drying_time, 27.0);
            let saved = serde_json::to_value(config).unwrap();
            assert!(saved.get("stranded_foam_color").is_none());
            assert!(saved.get("foam_deposit_depth_mm").is_none());
            assert_eq!(
                serde_json::from_value::<ShoreConfig>(saved).unwrap(),
                config
            );
        }
    }
    #[test]
    fn removed_gain_settings_preserve_other_controls() {
        let config: ShoreConfig = serde_json::from_str(
            r#"{"foam_front_gain_points":[0,1.2,1.62,2.5],"foam_front_power":2,"foam_front_generation":3.5,"foam_background_multiplier":0.05}"#,
        ).unwrap();
        config.validate().unwrap();
        assert_eq!(config.foam_front_generation, 3.5);
        assert_eq!(config.foam_background_multiplier, 0.05);
        let saved = serde_json::to_string(&config).unwrap();
        assert!(!saved.contains("foam_front_gain_points"));
        assert!(!saved.contains("foam_front_power"));
        assert_eq!(serde_json::from_str::<ShoreConfig>(&saved).unwrap(), config);
    }
    #[test]
    fn legacy_settings_get_valid_shore_defaults() {
        let optics: OpticsConfig = serde_json::from_str(
            r#"{"beach":true,"beach_slope":0.18,"shore":{"amplitude":1.3,"period":8.0}}"#,
        )
        .unwrap();
        assert_eq!(optics.shore, ShoreConfig::default());
        optics.validate().unwrap();
    }
    #[test]
    fn rejects_singular_and_nonfinite_controls() {
        for c in [
            ShoreConfig {
                foam_velocity_threshold: -0.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_velocity_threshold: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_velocity_transition: f32::INFINITY,
                ..Default::default()
            },
            ShoreConfig {
                foam_velocity_transition: 20.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_follow: 1.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_response_time: -0.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_response_time: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                input_gain: 3.1,
                ..Default::default()
            },
            ShoreConfig {
                input_gain: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                order: 3,
                ..Default::default()
            },
            ShoreConfig {
                filter_cutoff: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                filter_transition: -1.0,
                ..Default::default()
            },
            ShoreConfig {
                breaking_depth: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                drying_time: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                foam_generation_threshold: -0.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_generation_transition: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_front_generation: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_front_distance: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                foam_ridge_width: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_ridge_prominence: f32::INFINITY,
                ..Default::default()
            },
            ShoreConfig {
                foam_background_multiplier: 1.01,
                ..Default::default()
            },
            ShoreConfig {
                generation: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_density_reference: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                foam_compression_threshold: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_fresh_half_life: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_residual_half_life: f32::INFINITY,
                ..Default::default()
            },
            ShoreConfig {
                foam_conversion: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                view: 11,
                ..Default::default()
            },
        ] {
            assert!(c.validate().is_err());
        }
    }
}

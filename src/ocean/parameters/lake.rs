//! Parameters for render-only lake waves. Propagation coordinates are dimensionless.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropagationMode {
    #[default]
    Depth,
    Sdf,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LakeTerrain {
    pub seed: u32,
    pub resolution: u32,
    pub radius: f32,
    pub depth: f32,
    pub irregularity: f32,
    pub rocks: u32,
    pub rock_size: f32,
}
impl Default for LakeTerrain {
    fn default() -> Self {
        Self {
            seed: 73,
            resolution: 256,
            radius: 32.0,
            depth: 4.0,
            irregularity: 0.22,
            rocks: 8,
            rock_size: 3.0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LakePropagation {
    pub start: f32,
    pub speed: f32,
    pub width: f32,
    pub reflection: f32,
}
impl Default for LakePropagation {
    fn default() -> Self {
        Self {
            start: 0.72,
            speed: 0.10,
            width: 0.035,
            reflection: 0.06,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LakeConfig {
    pub enabled: bool,
    pub terrain: LakeTerrain,
    pub mode: PropagationMode,
    pub depth_wave: LakePropagation,
    pub sdf_wave: LakePropagation,
    pub wave_seed: u32,
    pub automatic: bool,
    pub interval: f32,
    pub direction_deg: f32,
    pub spread_deg: f32,
    pub jitter_deg: f32,
    /// Relative random ranges for angular span and propagation-coordinate thickness.
    pub spread_jitter: f32,
    pub width_jitter: f32,
    /// Random offset of the nearshore launch coordinate (dimensionless).
    pub position_jitter: f32,
    pub amplitude: f32,
    /// Shared FFT height multiplier; zero preserves legacy procedural-only water.
    pub fft_strength: f32,
    /// Fade FFT height by physical water depth and distance to shore.
    pub fft_shore_fade: bool,
    /// Render-only foam opacity; zero preserves legacy lake appearance.
    pub foam_strength: f32,
    /// Shore band width and cellular pattern size, in world metres.
    pub foam_width: f32,
    pub foam_size: f32,
    /// Inward render-only flow, metres per simulation second; zero freezes the UVs.
    pub foam_flow_speed: f32,
    /// Duration of each UV advection/reset cycle, simulation seconds.
    pub foam_flow_period: f32,
    /// Nearshore speed-gradient strength, not a density or wave-height multiplier.
    pub foam_flow_distortion: f32,
    pub tip_ratio: f32,
    pub reflect: bool,
    pub reflectivity: f32,
    pub reflected_half_life: f32,
    pub transition: f32,
    /// 0 shaded, 1 depth, 2 signed distance, 3 propagation contours, 4 wave height.
    pub view: u32,
}
impl Default for LakeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            terrain: LakeTerrain::default(),
            mode: PropagationMode::Depth,
            depth_wave: LakePropagation::default(),
            sdf_wave: LakePropagation {
                start: 0.65,
                ..LakePropagation::default()
            },
            wave_seed: 42,
            automatic: true,
            interval: 1.8,
            direction_deg: 15.0,
            spread_deg: 55.0,
            jitter_deg: 18.0,
            spread_jitter: 0.35,
            width_jitter: 0.35,
            position_jitter: 0.04,
            amplitude: 0.09,
            fft_strength: 0.0,
            fft_shore_fade: true,
            foam_strength: 0.0,
            foam_width: 5.0,
            foam_size: 2.0,
            foam_flow_speed: 0.35,
            foam_flow_period: 6.0,
            foam_flow_distortion: 1.0,
            tip_ratio: 0.2,
            reflect: true,
            reflectivity: 0.4,
            reflected_half_life: 2.5,
            transition: 0.35,
            view: 0,
        }
    }
}
impl LakeConfig {
    pub fn propagation(&self) -> LakePropagation {
        match self.mode {
            PropagationMode::Depth => self.depth_wave,
            PropagationMode::Sdf => self.sdf_wave,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let t = self.terrain;
        if !matches!(t.resolution, 128 | 256 | 512) || t.rocks > 32 || self.view > 4 {
            return Err("invalid lake resolution, rock count or view".into());
        }
        for (name, v, low, high) in [
            ("radius", t.radius, 8.0, 100.0),
            ("depth", t.depth, 0.5, 12.0),
            ("irregularity", t.irregularity, 0.0, 0.35),
            ("rock size", t.rock_size, 0.5, 8.0),
            ("interval", self.interval, 0.1, 30.0),
            ("direction", self.direction_deg, -180.0, 180.0),
            ("angular half width", self.spread_deg, 5.0, 150.0),
            ("jitter", self.jitter_deg, 0.0, 180.0),
            ("angular span jitter", self.spread_jitter, 0.0, 1.0),
            ("thickness jitter", self.width_jitter, 0.0, 1.0),
            ("launch position jitter", self.position_jitter, 0.0, 0.3),
            ("amplitude", self.amplitude, 0.0, 0.5),
            ("FFT strength", self.fft_strength, 0.0, 1.0),
            ("foam strength", self.foam_strength, 0.0, 1.0),
            ("foam width", self.foam_width, 0.1, 10.0),
            ("foam size", self.foam_size, 0.1, 5.0),
            ("foam flow speed", self.foam_flow_speed, 0.0, 2.0),
            ("foam flow period", self.foam_flow_period, 1.0, 20.0),
            ("foam flow distortion", self.foam_flow_distortion, 0.0, 1.0),
            ("tip width ratio", self.tip_ratio, 0.05, 1.0),
            ("reflectivity", self.reflectivity, 0.0, 0.95),
            ("reflection half life", self.reflected_half_life, 0.1, 30.0),
            ("transition", self.transition, 0.05, 2.0),
        ] {
            if !v.is_finite() || !(low..=high).contains(&v) {
                return Err(format!("invalid lake {name}"));
            }
        }
        for p in [self.depth_wave, self.sdf_wave] {
            if ![p.start, p.speed, p.width, p.reflection]
                .iter()
                .all(|v| v.is_finite())
                || !(0.1..=0.95).contains(&p.start)
                || !(0.005..=0.5).contains(&p.speed)
                || !(0.005..=0.15).contains(&p.width)
                || !(0.005..=0.3).contains(&p.reflection)
                || p.reflection >= p.start
            {
                return Err(
                    "invalid lake propagation coordinates (reflection must be below start)".into(),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_and_lake_settings_remain_loadable() {
        let legacy: crate::ocean::settings::SavedSettings =
            serde_json::from_str(r#"{"optics":{"depth":7.0}}"#).unwrap();
        assert!(!legacy.optics.lake.enabled);
        assert_eq!(legacy.optics.depth, 7.0);
        let old_lake: crate::ocean::settings::SavedSettings =
            serde_json::from_str(r#"{"optics":{"lake":{"enabled":true,"mode":"sdf"}}}"#).unwrap();
        assert!(old_lake.optics.lake.enabled);
        assert_eq!(old_lake.optics.lake.fft_strength, 0.0);
        assert!(old_lake.optics.lake.fft_shore_fade);
        assert_eq!(old_lake.optics.lake.spread_jitter, 0.35);
        assert_eq!(old_lake.optics.lake.width_jitter, 0.35);
        assert_eq!(old_lake.optics.lake.position_jitter, 0.04);
        assert_eq!(old_lake.optics.lake.foam_flow_distortion, 1.0);
        assert_eq!(old_lake.optics.lake.mode, PropagationMode::Sdf);
        let mut settings = legacy;
        settings.optics.lake.enabled = true;
        settings.optics.lake.mode = PropagationMode::Sdf;
        settings.optics.lake.fft_strength = 0.37;
        settings.optics.lake.fft_shore_fade = false;
        settings.optics.lake.spread_jitter = 0.7;
        settings.optics.lake.width_jitter = 0.6;
        settings.optics.lake.position_jitter = 0.12;
        settings.optics.lake.foam_flow_speed = 0.72;
        settings.optics.lake.foam_flow_period = 9.0;
        settings.optics.lake.foam_flow_distortion = 0.6;
        let restored: crate::ocean::settings::SavedSettings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        restored.validate().unwrap();
        for invalid in [f32::NAN, f32::INFINITY, -1.0, 1.0] {
            let mut lake = LakeConfig::default();
            lake.sdf_wave.speed = invalid;
            assert!(lake.validate().is_err());
        }
        for strength in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            let lake = LakeConfig {
                fft_strength: strength,
                ..Default::default()
            };
            assert!(lake.validate().is_err());
        }
        for (speed, period) in [(0.0, 1.0), (2.0, 20.0)] {
            let lake = LakeConfig {
                foam_flow_speed: speed,
                foam_flow_period: period,
                ..Default::default()
            };
            lake.validate().unwrap();
        }
        for (speed, period) in [
            (f32::NAN, 6.0),
            (f32::INFINITY, 6.0),
            (-0.01, 6.0),
            (2.01, 6.0),
            (0.35, 0.0),
            (0.35, 0.99),
            (0.35, 20.01),
            (0.35, f32::NAN),
            (0.35, f32::INFINITY),
        ] {
            let lake = LakeConfig {
                foam_flow_speed: speed,
                foam_flow_period: period,
                ..Default::default()
            };
            assert!(lake.validate().is_err());
        }
        for distortion in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            let lake = LakeConfig {
                foam_flow_distortion: distortion,
                ..Default::default()
            };
            assert!(lake.validate().is_err());
        }
        for invalid in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            for lake in [
                LakeConfig {
                    spread_jitter: invalid,
                    ..Default::default()
                },
                LakeConfig {
                    width_jitter: invalid,
                    ..Default::default()
                },
                LakeConfig {
                    position_jitter: invalid,
                    ..Default::default()
                },
            ] {
                assert!(lake.validate().is_err());
            }
        }
        assert!(
            LakeConfig {
                position_jitter: 0.31,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        LakeConfig {
            spread_jitter: 1.0,
            width_jitter: 1.0,
            position_jitter: 0.3,
            ..Default::default()
        }
        .validate()
        .unwrap();
        let mut lake = LakeConfig::default();
        lake.depth_wave.reflection = lake.depth_wave.start;
        assert!(lake.validate().is_err());
    }
}

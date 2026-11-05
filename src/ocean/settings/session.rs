use crate::ocean::camera::Camera;
use crate::ocean::parameters::foam::FoamConfig;
use crate::ocean::parameters::ocean::OceanConfig;
use crate::ocean::parameters::optics::OpticsConfig;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SavedSettings {
    pub version: u32,
    pub ocean: OceanConfig,
    pub draft: OceanConfig,
    pub foam: FoamConfig,
    pub optics: OpticsConfig,
    pub camera: Camera,
    pub speed: f32,
    pub paused: bool,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            version: 1,
            ocean: OceanConfig::default(),
            draft: OceanConfig::default(),
            foam: FoamConfig::default(),
            optics: OpticsConfig::default(),
            camera: Camera::default(),
            speed: 1.0,
            paused: false,
        }
    }
}

impl SavedSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("unsupported settings version {}", self.version));
        }
        self.ocean.validate()?;
        self.draft.validate()?;
        self.optics.validate()?;
        for (name, value, min, max) in [
            ("foam threshold", self.foam.threshold, -0.5, 1.0),
            ("foam generation", self.foam.generation, 0.0, 20.0),
            ("foam decay", self.foam.decay, 0.0, 5.0),
            ("time speed", self.speed, 0.0, 3.0),
            ("camera pitch", self.camera.pitch, 0.07, 1.35),
            ("height gain", self.camera.gain, 0.0, 3.0),
            ("chop strength", self.camera.chop_amount, 0.0, 3.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("invalid {name}"));
            }
        }
        if !self.camera.yaw.is_finite()
            || !self.camera.target.iter().all(|value| value.is_finite())
            || !self.camera.distance.is_finite()
            || self.camera.distance <= 0.0
            || self.camera.foam_debug > 2
        {
            return Err("invalid camera settings".into());
        }
        Ok(())
    }

    /// Explicit CLI values override persisted values; all other settings survive.
    pub fn apply_cli(&mut self, ocean: OceanConfig, optics: OpticsConfig, flags: &[String]) {
        let old_length = self.ocean.length;
        if flags.iter().any(|f| f == "--beach" || f == "--model") {
            self.optics = optics;
            self.camera = if optics.model.enabled {
                Camera::model(old_length)
            } else {
                Camera::beach(old_length)
            };
        }
        for flag in flags {
            match flag.as_str() {
                "--resolution" => {
                    self.ocean.resolution = ocean.resolution;
                    self.draft.resolution = ocean.resolution;
                }
                "--mesh-resolution" => {
                    self.ocean.mesh_resolution = ocean.mesh_resolution;
                    self.draft.mesh_resolution = ocean.mesh_resolution;
                }
                "--triangular-mesh" | "--square-mesh" => {
                    self.ocean.triangular_mesh = ocean.triangular_mesh;
                    self.draft.triangular_mesh = ocean.triangular_mesh;
                }
                "--length" => {
                    self.ocean.length = ocean.length;
                    self.draft.length = ocean.length;
                }
                "--wind" | "--phillips-length" => {
                    self.ocean.phillips_length = ocean.phillips_length;
                    self.draft.phillips_length = ocean.phillips_length;
                }
                "--amplitude" => {
                    self.ocean.amplitude = ocean.amplitude;
                    self.draft.amplitude = ocean.amplitude;
                }
                "--wind-angle" => {
                    self.ocean.wind_angle = ocean.wind_angle;
                    self.draft.wind_angle = ocean.wind_angle;
                }
                "--seed" => {
                    self.ocean.seed = ocean.seed;
                    self.draft.seed = ocean.seed;
                }
                "--model-position" => self.optics.model.position = optics.model.position,
                "--model-scale" => self.optics.model.scale = optics.model.scale,
                "--model-yaw" => self.optics.model.yaw_deg = optics.model.yaw_deg,
                "--sea-level" => self.optics.model.sea_level = optics.model.sea_level,
                "--swe-center" => self.optics.model.center = optics.model.center,
                "--swe-size" => self.optics.model.size = optics.model.size,
                "--swe-resolution" => self.optics.shore.resolution = optics.shore.resolution,
                "--swe-sponge" => self.optics.model.sponge_width = optics.model.sponge_width,
                "--swe-open-edges" => self.optics.model.open_edges = optics.model.open_edges,
                "--hide-swe-box" => self.optics.model.show_bounds = false,
                "--seabed-depth" => self.optics.depth = optics.depth,
                "--sigma-t" => self.optics.sigma_t = optics.sigma_t,
                "--source" => self.optics.source = optics.source,
                "--sky-color" => self.optics.sky.color = optics.sky.color,
                "--sky-intensity" => self.optics.sky.intensity = optics.sky.intensity,
                "--scattering-albedo" => self.optics.sky.albedo = optics.sky.albedo,
                "--sky-steps" => self.optics.sky.steps = optics.sky.steps,
                "--no-skylight" | "--skylight" => self.optics.sky.enabled = optics.sky.enabled,
                "--caustics-mode" => self.optics.caustics = optics.caustics,
                "--refraction-mode" => {
                    self.optics.refraction = optics.refraction;
                    if optics.refraction {
                        self.optics.refraction_method = optics.refraction_method;
                    }
                }
                "--caustics" | "--no-caustics" => self.optics.caustics = optics.caustics,
                "--caustics-strength" => self.optics.caustics_strength = optics.caustics_strength,
                "--caustics-extent" => self.optics.caustics_extent = optics.caustics_extent,
                "--caustics-softness" => self.optics.caustics_softness = optics.caustics_softness,
                "--caustics-resolution" => {
                    self.optics.caustics_resolution = optics.caustics_resolution
                }
                "--refraction" | "--no-refraction" => self.optics.refraction = optics.refraction,
                "--water-ior" => self.optics.water_ior = optics.water_ior,
                "--refraction-distance" => {
                    self.optics.refraction_distance = optics.refraction_distance
                }
                "--water-view" => self.optics.view = optics.view,
                "--shore-view" => self.optics.shore.view = optics.shore.view,
                "--spray" => self.optics.spray.enabled = optics.spray.enabled,
                "--spray-debug" => {
                    self.optics.spray.enabled = optics.spray.enabled;
                    self.optics.spray.debug = optics.spray.debug;
                }
                "--foam-texture" => self.optics.shore.foam_texture = optics.shore.foam_texture,
                "--foam-dual-phase" | "--no-foam-dual-phase" => {
                    self.optics.shore.foam_dual_phase = optics.shore.foam_dual_phase
                }
                "--foam-fresh-half-life" => {
                    self.optics.shore.foam_fresh_half_life = optics.shore.foam_fresh_half_life
                }
                "--foam-residual-half-life" => {
                    self.optics.shore.foam_residual_half_life = optics.shore.foam_residual_half_life
                }
                "--foam-conversion" => {
                    self.optics.shore.foam_conversion = optics.shore.foam_conversion
                }
                "--foam-prototype" | "--no-foam-prototype" => {
                    self.optics.shore.foam_prototype = optics.shore.foam_prototype
                }
                "--foam-texture-period" => {
                    self.optics.shore.foam_texture_period = optics.shore.foam_texture_period
                }
                "--foam-ridges" | "--no-foam-ridges" => {
                    self.optics.shore.foam_ridge_enabled = optics.shore.foam_ridge_enabled
                }
                "--no-foam-emission" => self.optics.shore.foam_emission_enabled = false,
                "--foam-ridge-width" => {
                    self.optics.shore.foam_ridge_width = optics.shore.foam_ridge_width
                }
                "--foam-ridge-prominence" => {
                    self.optics.shore.foam_ridge_prominence = optics.shore.foam_ridge_prominence
                }
                "--foam-transport-order" => {
                    self.optics.shore.foam_transport_order = optics.shore.foam_transport_order
                }
                "--foam-opacity-scale" => {
                    self.optics.shore.foam_opacity_scale = optics.shore.foam_opacity_scale
                }
                "--foam-material-view" => {
                    self.optics.shore.foam_material_view = optics.shore.foam_material_view
                }
                "--swe-filter-cutoff" => {
                    self.optics.shore.filter_cutoff = optics.shore.filter_cutoff
                }
                "--swe-filter-transition" => {
                    self.optics.shore.filter_transition = optics.shore.filter_transition
                }
                "--water-foam-color" => {
                    self.optics.shore.water_foam_color = optics.shore.water_foam_color
                }
                "--swe-input-gain" => self.optics.shore.input_gain = optics.shore.input_gain,
                "--swe-order" => self.optics.shore.order = optics.shore.order,
                "--shore-normal-strength" => {
                    self.optics.shore.normal_strength = optics.shore.normal_strength
                }
                "--no-swe-filter" => self.optics.shore.filter_enabled = false,
                "--no-shore" => self.optics.shore.enabled = false,
                _ => {}
            }
        }
        self.camera.distance *= self.ocean.length / old_length;
    }
}

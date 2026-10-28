#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FoamConfig {
    pub threshold: f32,
    pub generation: f32,
    pub decay: f32,
}

impl Default for FoamConfig {
    fn default() -> Self {
        Self {
            threshold: 0.65,
            generation: 4.0,
            decay: 0.6,
        }
    }
}

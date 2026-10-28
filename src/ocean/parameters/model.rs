use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub enabled: bool,
    pub position: [f32; 3],
    pub scale: f32,
    pub yaw_deg: f32,
    /// Heights in the asset datum; rendering/simulation use sea-relative Y.
    pub sea_level: f32,
    pub center: [f32; 3],
    pub size: [f32; 3],
    /// -X, +X, -Z, +Z: true = FFT open, false = reflective wall.
    pub open_edges: [bool; 4],
    pub sponge_width: f32,
    pub extension_width: f32,
    pub show_bounds: bool,
}
impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            position: [0.0, 13.0, -8.7735],
            scale: 1.0,
            yaw_deg: 0.0,
            sea_level: 0.5,
            center: [0.0, 4.0, 0.0],
            size: [128.0, 64.0, 128.0],
            open_edges: [true; 4],
            sponge_width: 20.0,
            extension_width: 12.0,
            show_bounds: true,
        }
    }
}
impl ModelConfig {
    pub fn validate(self) -> Result<(), String> {
        for (value, low, high) in self
            .position
            .into_iter()
            .map(|v| (v, -2000.0, 2000.0))
            .chain(self.center.into_iter().map(|v| (v, -2000.0, 2000.0)))
            .chain(self.size.into_iter().map(|v| (v, 4.0, 2048.0)))
            .chain([
                (self.scale, 0.01, 100.0),
                (self.yaw_deg, -180.0, 180.0),
                (self.sea_level, -2000.0, 2000.0),
                (self.sponge_width, 0.5, 512.0),
                (self.extension_width, 0.5, 256.0),
            ])
        {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err("invalid model/SWE domain parameter".into());
            }
        }
        if self.sponge_width * 2.0 >= self.size[0].min(self.size[2]) {
            return Err("SWE domain must be wider than two sponge bands".into());
        }
        Ok(())
    }
    pub fn domain(self) -> [f32; 4] {
        [
            self.center[0] - self.size[0] * 0.5,
            self.center[2] - self.size[2] * 0.5,
            self.size[0],
            self.size[2],
        ]
    }
    /// Bounds available for obstacles, excluding open-edge sponge bands.
    pub fn interior(self, resolution: u32) -> [f32; 4] {
        let domain = self.domain();
        std::array::from_fn(|i| {
            let axis = i % 2;
            let high = i >= 2;
            let edge = axis * 2 + usize::from(high);
            let pad = if self.open_edges[edge] {
                self.sponge_width
            } else {
                domain[axis + 2] / resolution as f32
            };
            if high {
                domain[axis] + domain[axis + 2] - pad
            } else {
                domain[axis] + pad
            }
        })
    }
    pub fn transform(self) -> Mat4 {
        Mat4::from_translation(Vec3::from_array(self.position) - Vec3::Y * self.sea_level)
            * Mat4::from_rotation_y(self.yaw_deg.to_radians())
            * Mat4::from_scale(Vec3::splat(self.scale))
    }
    /// Display-only box Y and visibility must never reset water history.
    pub fn physics_key(mut self) -> Self {
        self.center[1] = 0.0;
        self.size[1] = 0.0;
        self.show_bounds = false;
        self
    }
    pub fn fit(&mut self, bounds: [[f32; 3]; 2]) {
        let transform = self.transform();
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for x in 0..2 {
            for y in 0..2 {
                for z in 0..2 {
                    let p = transform.transform_point3(Vec3::new(
                        bounds[x][0],
                        bounds[y][1],
                        bounds[z][2],
                    ));
                    low = low.min(p);
                    high = high.max(p);
                }
            }
        }
        let mut center = (low + high) * 0.5;
        center.y += self.sea_level;
        self.center = center.to_array();
        let horizontal =
            ((high - low).x.max((high - low).z) + 2.0 * (self.sponge_width + 12.0)).ceil();
        self.size = [horizontal, ((high - low).y + 24.0).max(32.0), horizontal];
    }
}

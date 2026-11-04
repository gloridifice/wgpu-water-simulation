//! Shared camera state and GPU layout, independent of the window.
use crate::ocean::parameters::ocean::OceanConfig;
use glam::{Mat4, Vec2, Vec3};

const VERTICAL_FOV: f32 = 55.0_f32.to_radians();

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: [f32; 2],
    pub gain: f32,
    pub choppy: bool,
    pub chop_amount: f32,
    pub foam_debug: u32,
    pub foam: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self::new(OceanConfig::default().length)
    }
}

impl Camera {
    pub fn new(length: f32) -> Self {
        Self {
            yaw: 0.55,
            pitch: 0.30,
            distance: length * 0.58,
            target: [0.0; 2],
            gain: 1.0,
            choppy: true,
            chop_amount: 1.15,
            foam_debug: 0,
            foam: true,
        }
    }

    pub fn beach(length: f32) -> Self {
        Self {
            yaw: -0.72,
            pitch: 0.70,
            distance: length * 0.60,
            gain: 0.22,
            chop_amount: 0.65,
            foam: true,
            ..Self::new(length)
        }
    }

    pub fn model(length: f32) -> Self {
        Self {
            yaw: 0.65,
            pitch: 0.65,
            distance: 95.0,
            ..Self::beach(length)
        }
    }

    pub fn pan(&mut self, direction: Vec2, elapsed: f32) {
        let direction = direction.normalize_or_zero();
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(-self.yaw.sin(), -self.yaw.cos());
        let offset = (right * direction.x + forward * direction.y) * self.distance * 0.5 * elapsed;
        self.target[0] += offset.x;
        self.target[1] += offset.y;
    }

    /// Drag in logical points (right/down), with viewport height in the same units.
    pub fn pan_drag(&mut self, delta: Vec2, viewport_height: f32) {
        let scale = 2.0 * self.distance * (VERTICAL_FOV * 0.5).tan() / viewport_height.max(1.0);
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(-self.yaw.sin(), -self.yaw.cos());
        // Keep height fixed; compensate for the horizontal plane's screen foreshortening.
        let offset = (-right * delta.x + forward * (delta.y / self.pitch.sin())) * scale;
        self.target[0] += offset.x;
        self.target[1] += offset.y;
    }

    pub fn uniform(&self, config: OceanConfig, aspect: f32) -> CameraUniform {
        let target = Vec3::new(self.target[0], 0.0, self.target[1]);
        let eye = target
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance;
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let projection = Mat4::perspective_rh(VERTICAL_FOV, aspect, 0.1, config.length * 8.0);
        let matrix = projection * view;
        let sun = Vec3::new(-0.35, 0.40, -0.65).normalize();
        CameraUniform {
            view_projection: matrix.to_cols_array_2d(),
            inverse_view_projection: matrix.inverse().to_cols_array_2d(),
            eye: eye.extend(1.0).to_array(),
            sun: sun.extend(0.0).to_array(),
            ocean: [
                config.resolution as f32,
                config.length,
                if self.choppy { -self.chop_amount } else { 0.0 },
                self.gain,
            ],
            misc: [
                if self.foam { 1.0 } else { 0.0 },
                config.mesh_resolution as f32,
                self.foam_debug as f32,
                0.0,
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_projection: [[f32; 4]; 4],
    pub inverse_view_projection: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub sun: [f32; 4],
    /// n, patch length, horizontal choppiness, vertical/horizontal amplitude gain
    pub ocean: [f32; 4],
    pub misc: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_pan_preserves_height_and_heading_at_any_pitch() {
        let config = OceanConfig::default();
        for pitch in [0.07, 0.7, 1.35] {
            for (yaw, direction, expected) in [
                (0.0, Vec2::Y, Vec3::NEG_Z),
                (0.0, Vec2::NEG_Y, Vec3::Z),
                (0.0, Vec2::X, Vec3::X),
                (0.0, Vec2::NEG_X, Vec3::NEG_X),
                (std::f32::consts::FRAC_PI_2, Vec2::Y, Vec3::NEG_X),
                (0.0, Vec2::ONE, Vec3::new(1.0, 0.0, -1.0).normalize()),
            ] {
                let mut camera = Camera {
                    yaw,
                    pitch,
                    distance: 20.0,
                    ..Camera::default()
                };
                let before = camera.uniform(config, 1.0);
                camera.pan(direction, 0.1);
                let after = camera.uniform(config, 1.0);
                let displacement = Vec3::from_slice(&after.eye) - Vec3::from_slice(&before.eye);
                assert!((displacement - expected).length() < 1e-5);
                let before_view = Mat4::from_cols_array_2d(&before.view_projection);
                let after_view = Mat4::from_cols_array_2d(&after.view_projection);
                for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                    assert!(
                        (before_view.transform_vector3(axis) - after_view.transform_vector3(axis))
                            .length()
                            < 1e-5
                    );
                }
            }
        }
    }
}

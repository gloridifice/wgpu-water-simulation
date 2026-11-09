//! Perspective orbit geometry, independent of simulation settings and window APIs.
use glam::{Mat4, Vec2, Vec3};
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}
impl Orbit {
    pub fn eye(self) -> Vec3 {
        self.target
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance
    }
    pub fn matrices(self, fov: f32, aspect: f32, near: f32, far: f32) -> (Mat4, Mat4) {
        let view = Mat4::look_at_rh(self.eye(), self.target, Vec3::Y);
        let projection = Mat4::perspective_rh(fov, aspect, near, far);
        let vp = projection * view;
        (vp, vp.inverse())
    }
    pub fn pan(&mut self, direction: Vec2, elapsed: f32) {
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(-self.yaw.sin(), -self.yaw.cos());
        let offset = (right * direction.normalize_or_zero().x
            + forward * direction.normalize_or_zero().y)
            * self.distance
            * 0.5
            * elapsed;
        self.target.x += offset.x;
        self.target.z += offset.y;
    }
    pub fn pan_drag(&mut self, delta: Vec2, height: f32, fov: f32) {
        let scale = 2.0 * self.distance * (fov * 0.5).tan() / height.max(1.0);
        let right = Vec2::new(self.yaw.cos(), -self.yaw.sin());
        let forward = Vec2::new(-self.yaw.sin(), -self.yaw.cos());
        let offset = (-right * delta.x + forward * (delta.y / self.pitch.sin())) * scale;
        self.target.x += offset.x;
        self.target.z += offset.y;
    }
}

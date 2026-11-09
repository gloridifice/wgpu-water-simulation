//! Kinematic capsule for the static heightfield playground; normal gravity is independent of fog.
use crate::{parameters::range, terrain::Terrain};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
pub mod camera_collision;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Player {
    pub feet: [f32; 3],
    pub walk_speed: f32,
    pub jump_speed: f32,
    #[serde(skip)]
    velocity_y: f32,
    #[serde(skip)]
    contacted: bool,
}
impl PartialEq for Player {
    fn eq(&self, other: &Self) -> bool {
        self.feet == other.feet
            && self.walk_speed == other.walk_speed
            && self.jump_speed == other.jump_speed
    }
}
impl Default for Player {
    fn default() -> Self {
        Self {
            feet: [0.0; 3],
            walk_speed: 3.0,
            jump_speed: 4.5,
            velocity_y: 0.0,
            contacted: false,
        }
    }
}
#[derive(Default)]
pub struct Motion {
    pub horizontal: Vec2,
    pub jumped: Option<Vec3>,
    pub landed: Option<(Vec3, f32)>,
}
impl Player {
    pub fn validate(&self) -> Result<(), String> {
        range("步行速度", self.walk_speed, 0.0, 10.0)?;
        range("起跳速度", self.jump_speed, 1.0, 10.0)?;
        for v in self.feet {
            range("角色位置", v, -10000.0, 10000.0)?;
        }
        Ok(())
    }
    pub fn place(&mut self, terrain: &Terrain) -> Result<(), String> {
        let d = terrain.metadata.domain;
        if d[2] <= 0.7 || d[3] <= 0.7 {
            return Err("实验场范围不足以容纳角色".into());
        }
        let mut p = Vec2::new(self.feet[0], self.feet[2]).clamp(
            Vec2::new(d[0] + 0.35, d[1] + 0.35),
            Vec2::new(d[0] + d[2] - 0.35, d[1] + d[3] - 0.35),
        );
        if terrain.height_at(p).is_none() {
            p = terrain
                .valid
                .iter()
                .enumerate()
                .filter(|(_, v)| **v == 1)
                .map(|(i, _)| {
                    Vec2::new(d[0], d[1])
                        + (Vec2::new(
                            (i % terrain.metadata.resolution[0] as usize) as f32,
                            (i / terrain.metadata.resolution[0] as usize) as f32,
                        ) + Vec2::splat(0.5))
                            * terrain.spacing()
                })
                .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
                .ok_or("没有可用地面")?;
        }
        self.feet = [p.x, self.feet[1].max(terrain.height_at(p).unwrap()), p.y];
        self.velocity_y = 0.0;
        self.contacted = false;
        Ok(())
    }
    pub fn step(&mut self, axis: Vec2, yaw: f32, jump: bool, dt: f32, terrain: &Terrain) -> Motion {
        if !dt.is_finite() || dt <= 0.0 {
            return Motion::default();
        }
        let old = Vec3::from_array(self.feet);
        let mut p = old;
        let direction = axis.normalize_or_zero();
        let right = Vec2::new(yaw.cos(), -yaw.sin());
        let forward = Vec2::new(-yaw.sin(), -yaw.cos());
        let move_xz = (right * direction.x + forward * direction.y) * self.walk_speed * dt;
        let domain = terrain.metadata.domain;
        let radius = 0.35;
        let mut proposed = Vec2::new(p.x + move_xz.x, p.z + move_xz.y).clamp(
            Vec2::new(domain[0] + radius, domain[1] + radius),
            Vec2::new(
                domain[0] + domain[2] - radius,
                domain[1] + domain[3] - radius,
            ),
        );
        // Invalid floor cells and shared blocked faces cannot be walked through.
        let before = (Vec2::new(p.x, p.z) - Vec2::new(domain[0], domain[1])) / terrain.spacing();
        let after = (proposed - Vec2::new(domain[0], domain[1])) / terrain.spacing();
        let nx = terrain.metadata.resolution[0] as usize;
        let a = before.floor().as_ivec2();
        let b = after.floor().as_ivec2();
        for face in (a.x.min(b.x) + 1)..=a.x.max(b.x) {
            if terrain.pass_x[a.y as usize * (nx + 1) + face as usize] == 0 {
                proposed.x = p.x;
                break;
            }
        }
        let resolved_x = if proposed.x == p.x { a.x } else { b.x };
        for face in (a.y.min(b.y) + 1)..=a.y.max(b.y) {
            if terrain.pass_z[face as usize * nx + resolved_x as usize] == 0 {
                proposed.y = p.z;
                break;
            }
        }
        if let Some(h) = terrain.height_at(proposed)
            && h <= p.y + 0.3
        {
            p.x = proposed.x;
            p.z = proposed.y;
        }
        let floor = terrain.height_at(Vec2::new(p.x, p.z)).unwrap_or(old.y);
        let grounded = p.y <= floor + 0.0001 && self.velocity_y <= 0.0;
        let mut motion = Motion::default();
        if grounded {
            p.y = floor;
            self.velocity_y = 0.0;
            if jump {
                self.velocity_y = self.jump_speed;
                motion.jumped = Some(p);
            }
        } else {
            self.velocity_y -= 9.81 * dt;
        }
        let impact = (-self.velocity_y).max(0.0);
        p.y += self.velocity_y * dt;
        if p.y <= floor {
            p.y = floor;
            self.velocity_y = 0.0;
            if !grounded && self.contacted && impact > 0.5 {
                motion.landed = Some((p, impact));
            }
            self.contacted = true;
        }
        motion.horizontal = Vec2::new(p.x - old.x, p.z - old.z) / dt.max(1e-6);
        self.feet = p.to_array();
        motion
    }
    pub fn orbit(&self, camera: [f32; 3]) -> common::camera::Orbit {
        let target = Vec3::from_array(self.feet) + Vec3::Y * 0.9;
        common::camera::Orbit {
            yaw: camera[0],
            pitch: camera[1],
            distance: camera[2],
            target,
        }
    }
}

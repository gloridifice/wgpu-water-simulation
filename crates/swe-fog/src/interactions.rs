//! Bounded event transport and player action emitter; no GPU-to-CPU feedback.
use crate::parameters::range;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractionSettings {
    pub move_enabled: bool,
    pub jump_enabled: bool,
    pub landing_enabled: bool,
    pub move_impulse_speed: f32,
    pub move_impulse_strength: f32,
    pub move_impulse_interval: f32,
    pub move_width: f32,
    pub move_reach: f32,
    pub move_forward_offset: f32,
    pub jump_strength: f32,
    pub landing_strength: f32,
    pub jump_radius: f32,
    pub landing_radius: f32,
    pub center_height: f32,
    pub vertical_extent: f32,
}
impl Default for InteractionSettings {
    fn default() -> Self {
        Self {
            move_enabled: true,
            jump_enabled: true,
            landing_enabled: true,
            move_impulse_speed: 3.0,
            move_impulse_strength: 0.1,
            move_impulse_interval: 0.12,
            move_width: 2.4,
            move_reach: 1.8,
            move_forward_offset: 0.15,
            jump_strength: 0.7,
            landing_strength: 1.2,
            jump_radius: 1.8,
            landing_radius: 1.8,
            center_height: 0.2,
            vertical_extent: 0.6,
        }
    }
}
impl InteractionSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (name, v, lo, hi) in [
            ("移动纹理速度", self.move_impulse_speed, 0.0, 6.0),
            ("移动冲量强度", self.move_impulse_strength, 0.0, 1.0),
            ("移动间隔", self.move_impulse_interval, 0.02, 2.0),
            ("移动宽度", self.move_width, 0.25, 8.0),
            ("移动前伸", self.move_reach, 0.1, 6.0),
            ("移动前移", self.move_forward_offset, 0.0, 2.0),
            ("起跳强度", self.jump_strength, 0.0, 3.0),
            ("落地强度", self.landing_strength, 0.0, 4.0),
            ("起跳半径", self.jump_radius, 0.25, 6.0),
            ("落地半径", self.landing_radius, 0.25, 6.0),
            ("竖直中心", self.center_height, -2.0, 2.0),
            ("竖直范围", self.vertical_extent, 0.05, 4.0),
        ] {
            range(name, v, lo, hi)?;
        }
        Ok(())
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Event {
    pub position_rotation: [f32; 4],
    pub footprint: [f32; 4],
    pub metadata: [u32; 4],
}
impl Event {
    pub fn valid(&self) -> bool {
        self.position_rotation
            .iter()
            .chain(&self.footprint)
            .all(|v| v.is_finite())
            && self.footprint[0] > 0.0
            && self.footprint[1] > 0.0
            && self.footprint[2] >= 0.0
            && self.footprint[3] > 0.0
            && self.metadata[0] < 2
            && self.metadata[1] < 2
            && f32::from_bits(self.metadata[2]).is_finite()
            && f32::from_bits(self.metadata[2]) >= 0.0
    }
    pub fn radial(feet: Vec3, strength: f32, radius: f32, settings: &InteractionSettings) -> Self {
        Self {
            position_rotation: [feet.x, feet.y + settings.center_height, feet.z, 0.0],
            footprint: [
                radius * 2.0,
                radius * 2.0,
                strength,
                settings.vertical_extent,
            ],
            metadata: [1, 0, 1.0f32.to_bits(), 0],
        }
    }
}
#[derive(Default)]
pub struct EventQueue {
    impulses: Vec<Event>,
    continuous: BTreeMap<u64, Event>,
}
impl EventQueue {
    pub fn push(&mut self, event: Event) -> bool {
        if !event.valid() || event.metadata[1] != 0 || self.impulses.len() >= 32 {
            return false;
        }
        self.impulses.push(event);
        true
    }
    pub fn set_continuous(&mut self, id: u64, event: Option<Event>) {
        if let Some(e) = event.filter(|e| e.valid() && e.metadata[1] == 1) {
            if self.continuous.contains_key(&id) || self.continuous.len() < 32 {
                self.continuous.insert(id, e);
            }
        } else {
            self.continuous.remove(&id);
        }
    }
    pub fn clear(&mut self) {
        self.impulses.clear();
        self.continuous.clear();
    }
    pub fn take(&mut self) -> Vec<Event> {
        let mut v = std::mem::take(&mut self.impulses);
        v.extend(self.continuous.values().copied());
        v
    }
}
#[derive(Default)]
pub struct PlayerEmitter {
    elapsed: f32,
    moving: bool,
}
impl PlayerEmitter {
    pub fn clear(&mut self) {
        self.elapsed = 0.0;
        self.moving = false;
    }
    pub fn movement(
        &mut self,
        settings: &InteractionSettings,
        feet: Vec3,
        horizontal: Vec2,
        walk_speed: f32,
        dt: f32,
        queue: &mut EventQueue,
    ) {
        if !settings.move_enabled || horizontal.length() < 0.05 {
            self.clear();
            return;
        }
        let mut count = 0;
        if !self.moving {
            self.moving = true;
            self.elapsed = 0.0;
            count = 1;
        } else {
            self.elapsed = (self.elapsed + dt).min(settings.move_impulse_interval * 4.0);
            while self.elapsed + 1e-8 >= settings.move_impulse_interval {
                self.elapsed = (self.elapsed - settings.move_impulse_interval).max(0.0);
                count += 1;
            }
        }
        let d = horizontal.normalize();
        for _ in 0..count {
            queue.push(Event {
                position_rotation: [
                    feet.x + d.x * settings.move_forward_offset,
                    feet.y + settings.center_height,
                    feet.z + d.y * settings.move_forward_offset,
                    (-d.x).atan2(-d.y),
                ],
                footprint: [
                    settings.move_width,
                    settings.move_reach * 2.0,
                    settings.move_impulse_strength
                        * (horizontal.length() / walk_speed.max(0.001)).clamp(0.0, 1.0),
                    settings.vertical_extent,
                ],
                metadata: [0, 0, (settings.move_impulse_speed / 3.0).to_bits(), 0],
            });
        }
    }
}
/// Two signed RG16F velocity layers. No sRGB conversion or asset dependency.
pub fn texture_data() -> Vec<u16> {
    let mut result = Vec::with_capacity(128 * 128 * 4);
    for layer in 0..2 {
        for z in 0..128 {
            for x in 0..128 {
                let local =
                    (Vec2::new(x as f32, z as f32) + Vec2::splat(0.5)) / 128.0 * 2.0 - Vec2::ONE;
                let mut v = Vec2::ZERO;
                if layer == 0 {
                    let forward = -local.y;
                    if forward > 0.0 && forward < 1.0 {
                        let width = 0.2 + 0.75 * forward;
                        let lateral = (1.0 - (local.x / width).powi(2)).max(0.0);
                        let envelope =
                            (std::f32::consts::PI * forward).sin().powi(2) * lateral * lateral;
                        v = Vec2::new(local.x * 0.8, -1.0).normalize() * envelope * 3.0;
                    }
                } else {
                    let r = local.length();
                    if r > 0.0 && r < 1.0 {
                        v = local / r * (std::f32::consts::PI * r).sin().powi(2);
                    }
                }
                result.push(half::f16::from_f32(v.x).to_bits());
                result.push(half::f16::from_f32(v.y).to_bits());
            }
        }
    }
    result
}

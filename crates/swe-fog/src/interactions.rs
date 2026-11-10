//! Bounded event transport and player action emitter; no GPU-to-CPU feedback.
use crate::parameters::range;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementMode {
    Impulse,
    #[default]
    Wake,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InteractionSettings {
    pub move_enabled: bool,
    pub jump_enabled: bool,
    pub landing_enabled: bool,
    pub movement_mode: MovementMode,
    /// Radius in metres, response time in seconds, and dimensionless speed ratio.
    pub drag_radius: f32,
    pub drag_response: f32,
    pub drag_velocity_ratio: f32,
    pub wake_radius: f32,
    /// Angular speed in radians per second at full walking speed.
    pub wake_angular_speed: f32,
    pub wake_response: f32,
    pub wake_offset: f32,
    /// Full distance between the two vortex centres, in metres.
    pub wake_separation: f32,
    pub wake_spacing: f32,
    /// Source lifetime in active physics seconds; removing it retains fluid inertia.
    pub wake_lifetime: f32,
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
            movement_mode: MovementMode::Wake,
            drag_radius: 1.5,
            drag_response: 0.3,
            drag_velocity_ratio: 0.6,
            wake_radius: 2.2,
            wake_angular_speed: 1.2,
            wake_response: 0.3,
            wake_offset: 1.0,
            wake_separation: 2.8,
            wake_spacing: 1.0,
            wake_lifetime: 0.8,
            move_impulse_speed: 3.0,
            move_impulse_strength: 0.1,
            move_impulse_interval: 0.12,
            move_width: 2.4,
            move_reach: 1.8,
            move_forward_offset: 0.15,
            jump_strength: 2.4,
            landing_strength: 0.45,
            jump_radius: 0.9,
            landing_radius: 1.8,
            center_height: 0.2,
            vertical_extent: 0.6,
        }
    }
}
impl InteractionSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (name, v, lo, hi) in [
            ("拖带半径", self.drag_radius, 0.5, 4.0),
            ("拖带响应时间", self.drag_response, 0.05, 3.0),
            ("拖带速度比例", self.drag_velocity_ratio, 0.0, 1.0),
            ("尾涡半径", self.wake_radius, 0.5, 4.0),
            ("尾涡角速度", self.wake_angular_speed, 0.0, 3.0),
            ("尾涡响应时间", self.wake_response, 0.05, 3.0),
            ("尾涡后移", self.wake_offset, 0.0, 4.0),
            ("尾涡左右间距", self.wake_separation, 0.5, 6.0),
            ("尾涡发射间距", self.wake_spacing, 0.25, 4.0),
            ("尾涡施力时间", self.wake_lifetime, 0.05, 3.0),
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
            && self.metadata[0] < 5
            && self.metadata[1] < 2
            && (self.metadata[0] < 2 || self.metadata[1] == 1)
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
    // Shapes 2/3/4 are analytic disk drag / opposite vortices. The 48-byte ABI
    // and the two legacy texture layers stay unchanged. Scale means m/s for
    // disk drag and rad/s for vortices; strength is the response rate in 1/s.
    fn source(
        feet: Vec3,
        radius: f32,
        rate: f32,
        shape: u32,
        scale: f32,
        p: &InteractionSettings,
    ) -> Self {
        Self {
            position_rotation: [feet.x, feet.y + p.center_height, feet.z, 0.0],
            footprint: [2.0 * radius, 2.0 * radius, rate, p.vertical_extent],
            metadata: [shape, 1, scale.to_bits(), 0],
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
    wake: WakeEmitter,
}
impl PlayerEmitter {
    pub fn clear(&mut self) {
        self.elapsed = 0.0;
        self.moving = false;
        self.wake = WakeEmitter::default();
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
        if !settings.move_enabled || settings.movement_mode != MovementMode::Wake {
            self.wake.remove_sources(queue);
            self.wake = WakeEmitter::default();
        } else {
            self.elapsed = 0.0;
            self.moving = false;
            self.wake
                .step(settings, feet, horizontal, walk_speed, dt, queue);
            return;
        }
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

// Reserve the last 31 source IDs for this emitter: one body + 15 vortex pairs.
// Replacement is bounded even with fast movement and a long source lifetime.
const BODY_ID: u64 = u64::MAX;
const MAX_PAIRS: usize = 15;
struct WakePair {
    events: [Event; 2],
    age: f32,
    lifetime: f32,
}
#[derive(Default)]
struct WakeEmitter {
    pairs: [Option<WakePair>; MAX_PAIRS],
    next: usize,
    distance: f32,
    moving: bool,
}
impl WakeEmitter {
    fn pair_id(slot: usize, side: usize) -> u64 {
        BODY_ID - 1 - (slot * 2 + side) as u64
    }
    fn remove_sources(&self, queue: &mut EventQueue) {
        for offset in 0..=MAX_PAIRS * 2 {
            queue.set_continuous(BODY_ID - offset as u64, None);
        }
    }
    fn emit(&mut self, p: &InteractionSettings, feet: Vec3, d: Vec2, speed_ratio: f32) {
        if p.wake_angular_speed == 0.0 {
            return;
        }
        let right = Vec2::new(-d.y, d.x);
        let events = std::array::from_fn(|side| {
            let sign = if side == 0 { -1.0 } else { 1.0 };
            let xz = -d * p.wake_offset + right * (sign * 0.5 * p.wake_separation);
            Event::source(
                feet + Vec3::new(xz.x, 0.0, xz.y),
                p.wake_radius,
                1.0 / p.wake_response,
                if side == 0 { 4 } else { 3 },
                p.wake_angular_speed * speed_ratio,
                p,
            )
        });
        self.pairs[self.next] = Some(WakePair {
            events,
            age: 0.0,
            lifetime: p.wake_lifetime,
        });
        self.next = (self.next + 1) % MAX_PAIRS;
    }
    fn step(
        &mut self,
        p: &InteractionSettings,
        feet: Vec3,
        horizontal: Vec2,
        walk_speed: f32,
        dt: f32,
        queue: &mut EventQueue,
    ) {
        // Age in the same fixed active physics clock as player emission. Pause,
        // reset, focus loss and the interaction switch clear the whole emitter.
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        for pair in self.pairs.iter_mut().flatten() {
            pair.age += dt;
        }
        let speed = horizontal.length();
        if speed >= 0.05 && dt > 0.0 {
            let d = horizontal / speed;
            let ratio = (speed / walk_speed.max(0.001)).clamp(0.0, 1.0);
            let mut body = Event::source(
                feet,
                p.drag_radius,
                1.0 / p.drag_response,
                2,
                speed * p.drag_velocity_ratio,
                p,
            );
            body.position_rotation[3] = (-d.x).atan2(-d.y);
            queue.set_continuous(BODY_ID, Some(body));
            if !self.moving {
                self.distance = 0.0;
                self.emit(p, feet, d, ratio);
            } else {
                self.distance = (self.distance + speed * dt).min(p.wake_spacing * 4.0);
                while self.distance >= p.wake_spacing {
                    self.distance -= p.wake_spacing;
                    let behind = d * self.distance;
                    self.emit(p, feet - Vec3::new(behind.x, 0.0, behind.y), d, ratio);
                }
            }
            self.moving = true;
        } else {
            queue.set_continuous(BODY_ID, None);
            self.moving = false;
            self.distance = 0.0;
        }
        for (slot, pair) in self.pairs.iter_mut().enumerate() {
            if pair.as_ref().is_some_and(|p| p.age >= p.lifetime) {
                *pair = None;
            }
            for side in 0..2 {
                let event = pair.as_ref().map(|p| {
                    let mut event = p.events[side];
                    // Fade the response, retaining the original target speed.
                    event.footprint[2] *= (1.0 - p.age / p.lifetime).powi(2);
                    event
                });
                queue.set_continuous(Self::pair_id(slot, side), event);
            }
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

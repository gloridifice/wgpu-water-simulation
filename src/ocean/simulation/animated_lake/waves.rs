//! Bounded deterministic event scheduler. No GPU readback is involved.
use crate::ocean::{parameters::lake::LakeConfig, scene::lake::random};
pub const CAPACITY: usize = 128;
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WaveRecord {
    pub direction_birth_life: [f32; 4],
    pub propagation: [f32; 4],
    pub envelope: [f32; 4],
    pub appearance: [f32; 4],
    pub limits: [f32; 4],
}
#[derive(Clone)]
struct Wave {
    record: WaveRecord,
    birth: f64,
    end: f64,
    reflection: Option<(f64, WaveRecord)>,
}
#[derive(Default)]
pub struct Waves {
    pub time: f64,
    pub dropped: u64,
    live: Vec<Wave>,
    next: f64,
    cadence_origin: f64,
    cadence_tick: u64,
    sequence: u32,
    cadence: Option<(bool, f32)>,
}
impl Waves {
    pub fn clear(&mut self, config: &LakeConfig) {
        self.live.clear();
        self.restart_cadence(config);
    }
    fn restart_cadence(&mut self, config: &LakeConfig) {
        self.cadence_origin = self.time;
        self.cadence_tick = 1;
        self.next = self.time + f64::from(config.interval);
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn active_count(&self) -> usize {
        self.live.len()
    }
    fn occupied(&self) -> usize {
        self.live.len() + self.live.iter().filter(|w| w.reflection.is_some()).count()
    }
    pub fn emit(&mut self, config: &LakeConfig) {
        let serial = self.sequence;
        self.sequence = self.sequence.wrapping_add(1);
        if self.occupied() + 1 + usize::from(config.reflect) > CAPACITY {
            self.dropped += 1;
            return;
        }
        let p = config.propagation();
        let angle = (config.direction_deg
            + (random(config.wave_seed, serial) * 2.0 - 1.0) * config.jitter_deg)
            .to_radians();
        let travel = (p.start - p.reflection) / p.speed;
        let record = WaveRecord {
            direction_birth_life: [angle.cos(), angle.sin(), 0.0, travel + config.transition],
            propagation: [p.start, -p.speed, p.width, config.amplitude],
            envelope: [0.0, 0.0, config.spread_deg.to_radians(), config.tip_ratio],
            appearance: [config.transition, config.transition, 0.0, 0.0],
            limits: [p.reflection, 0.98, config.transition, 0.0],
        };
        let reflection = config.reflect.then(|| {
            let mut r = record;
            r.direction_birth_life[3] = ((0.98 - p.reflection) / p.speed)
                .min(config.reflected_half_life * 8.0)
                .max(config.transition * 2.0);
            r.propagation[0] = p.reflection;
            r.propagation[1] = p.speed;
            r.propagation[3] *= config.reflectivity;
            r.appearance[2] = config.reflected_half_life;
            (self.time + f64::from(travel), r)
        });
        self.live.push(Wave {
            record,
            birth: self.time,
            end: self.time + f64::from(record.direction_birth_life[3]),
            reflection,
        });
    }
    pub fn advance(&mut self, dt: f64, config: &LakeConfig) {
        let cadence = (config.automatic, config.interval);
        if self.cadence.is_some_and(|c| c != cadence) {
            self.restart_cadence(config);
        }
        self.cadence = Some(cadence);
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        let target = self.time + dt;
        loop {
            let reflection_time = self
                .live
                .iter()
                .filter_map(|w| w.reflection.map(|r| r.0))
                .fold(f64::INFINITY, f64::min);
            let auto_time = if config.automatic {
                self.next
            } else {
                f64::INFINITY
            };
            let event = reflection_time.min(auto_time);
            if event > target {
                break;
            }
            self.time = event;
            self.live.retain(|w| w.end > event);
            // Reservation guarantees room. Stable parent order resolves simultaneous events.
            let mut children = Vec::new();
            for w in &mut self.live {
                if w.reflection.is_some_and(|r| r.0 <= event) {
                    let (birth, record) = w.reflection.take().unwrap();
                    children.push(Wave {
                        record,
                        birth,
                        end: birth + f64::from(record.direction_birth_life[3]),
                        reflection: None,
                    });
                }
            }
            self.live.extend(children);
            if auto_time <= event {
                self.emit(config);
                self.cadence_tick += 1;
                self.next =
                    self.cadence_origin + self.cadence_tick as f64 * f64::from(config.interval);
            }
        }
        self.time = target;
        self.live.retain(|w| w.end > target);
    }
    pub fn records(&self) -> (f32, Vec<WaveRecord>) {
        let origin = (self.time / 256.0).floor() * 256.0;
        (
            (self.time - origin) as f32,
            self.live
                .iter()
                .map(|w| {
                    let mut r = w.record;
                    r.direction_birth_life[2] = (w.birth - origin) as f32;
                    r
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reflection_events_survive_time_partitioning_and_keep_the_same_sector() {
        let mut c = LakeConfig {
            jitter_deg: 0.0,
            ..Default::default()
        };
        let mut a = Waves::default();
        let mut b = Waves::default();
        a.advance(9.0, &c);
        for _ in 0..900 {
            b.advance(0.01, &c);
        }
        let (ta, ra) = a.records();
        let (tb, rb) = b.records();
        assert!((ta - tb).abs() < 1e-4);
        assert_eq!(ra, rb);
        let reflected: Vec<_> = ra.iter().filter(|r| r.propagation[1] > 0.0).collect();
        assert!(!reflected.is_empty());
        for r in reflected {
            assert!((r.direction_birth_life[0] - c.direction_deg.to_radians().cos()).abs() < 1e-6);
            assert_eq!(r.propagation[3], c.amplitude * c.reflectivity);
        }
        c.automatic = false;
        a.clear(&c);
        a.emit(&c);
        let before = a.records();
        a.advance(0.0, &c);
        assert_eq!(before, a.records());
        a.advance(60.0, &c);
        assert_eq!(a.active_count(), 0);
    }
    #[test]
    fn reserved_reflections_are_not_lost_when_full_and_clear_cancels_them() {
        let c = LakeConfig {
            automatic: false,
            ..Default::default()
        };
        let mut a = Waves::default();
        for _ in 0..128 {
            a.emit(&c);
        }
        assert_eq!(a.active_count(), 64);
        assert_eq!(a.dropped, 64);
        let p = c.propagation();
        a.advance(f64::from((p.start - p.reflection) / p.speed) + 0.01, &c);
        assert_eq!(a.active_count(), 128);
        assert_eq!(
            a.records()
                .1
                .iter()
                .filter(|r| r.propagation[1] > 0.0)
                .count(),
            64
        );
        a.clear(&c);
        a.advance(10.0, &c);
        assert_eq!(a.active_count(), 0);
    }
}

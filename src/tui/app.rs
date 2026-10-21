use std::time::Duration;
use water_simulation::pic::{Config, Simulation};

pub struct App {
    pub sim: Simulation,
    pub paused: bool,
    pub speed: f64,
    pub selected: usize,
    pub show_grid: bool,
    pub splashes: usize,
    pub pulses: usize,
    pub view_height: f64,
    startup_pulse: bool,
    accumulated_time: f64,
}

impl App {
    pub fn new(config: Config, paused: bool, waves: bool, pulse: bool) -> Result<Self, String> {
        let mut sim = Simulation::new(config)?;
        // 从静水位置和一个已知的外部动量脉冲开始，随后不修改粒子形状。
        if pulse {
            sim.wave_pulse();
        }
        sim.set_wavemaker(waves);
        Ok(Self {
            selected: sim.particles().len() / 2,
            sim,
            paused,
            speed: 0.25,
            show_grid: false,
            splashes: 0,
            pulses: usize::from(pulse),
            startup_pulse: pulse,
            accumulated_time: 0.0,
            view_height: (config.water_level() * 1.6).min(config.height()),
        })
    }
    pub fn tick(&mut self, elapsed: Duration) -> Result<(), String> {
        if !self.paused {
            self.accumulated_time =
                (self.accumulated_time + elapsed.as_secs_f64().min(0.1) * self.speed).min(0.25);
            // 限制一次 UI 迭代的物理工作量；高负载时降低实际播放速度而不堵塞按键。
            for _ in 0..8 {
                let dt = self.sim.stable_dt();
                if self.accumulated_time + 1e-12 < dt {
                    break;
                }
                self.sim.step(dt)?;
                self.expand_view();
                self.accumulated_time = (self.accumulated_time - dt).max(0.0);
            }
        }
        Ok(())
    }
    pub fn single_step(&mut self) -> Result<(), String> {
        self.paused = true;
        self.accumulated_time = 0.0;
        self.sim.step(self.sim.stable_dt())?;
        self.expand_view();
        Ok(())
    }
    fn expand_view(&mut self) {
        let top = self.sim.particles().iter().map(|p| p.y).fold(0.0, f64::max);
        if top > self.view_height - 2.0 * self.sim.config().cell_size {
            self.view_height =
                (top + 3.0 * self.sim.config().cell_size).min(self.sim.config().height());
        }
    }
    pub fn reset(&mut self) -> Result<(), String> {
        let (speed, show_grid) = (self.speed, self.show_grid);
        *self = Self::new(
            self.sim.config(),
            true,
            self.sim.wavemaker(),
            self.startup_pulse,
        )?;
        self.speed = speed;
        self.show_grid = show_grid;
        Ok(())
    }
    pub fn wave_pulse(&mut self) {
        self.sim.wave_pulse();
        self.pulses += 1;
    }
    pub fn splash(&mut self, x: f64, y: f64) -> Result<(), String> {
        if self.sim.disturb(x, y)? > 0 {
            self.splashes += 1;
        }
        self.selected = self
            .sim
            .particles()
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                ((a.x - x).powi(2) + (a.y - y).powi(2))
                    .total_cmp(&((b.x - x).powi(2) + (b.y - y).powi(2)))
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        Ok(())
    }
    pub fn change_speed(&mut self, faster: bool) {
        self.speed = (self.speed * if faster { 2.0 } else { 0.5 }).clamp(0.0625, 4.0);
    }
    pub fn move_selection(&mut self, right: bool) {
        self.selected = if right {
            (self.selected + 1).min(self.sim.particles().len() - 1)
        } else {
            self.selected.saturating_sub(1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn render_frame_rate_does_not_change_pic_transfer_frequency() {
        let config = Config {
            nx: 48,
            ny: 32,
            cell_size: 0.2,
            ..Config::default()
        };
        let mut a = App::new(config, false, true, true).unwrap();
        let mut b = App::new(config, false, true, true).unwrap();
        for _ in 0..20 {
            a.tick(Duration::from_millis(10)).unwrap();
        }
        for _ in 0..10 {
            b.tick(Duration::from_millis(20)).unwrap();
        }
        assert_eq!(a.sim.steps(), b.sim.steps());
        for (p, q) in a.sim.particles().iter().zip(b.sim.particles()) {
            assert!((p.x - q.x).abs() < 1e-12 && (p.y - q.y).abs() < 1e-12);
        }
    }
    #[test]
    fn pause_step_impulse_and_reset_work_without_changing_particle_count() {
        let config = Config {
            nx: 48,
            ny: 32,
            cell_size: 0.2,
            ..Config::default()
        };
        let mut app = App::new(config, true, true, true).unwrap();
        let count = app.sim.particles().len();
        app.tick(Duration::from_millis(30)).unwrap();
        assert_eq!(app.sim.steps(), 0);
        app.single_step().unwrap();
        assert_eq!(app.sim.steps(), 1);
        let time = app.sim.time();
        let p = app.sim.particles()[app.selected];
        app.splash(p.x, p.y).unwrap();
        assert_eq!(app.splashes, 1);
        assert_eq!(app.sim.time(), time);
        app.wave_pulse();
        assert_eq!(app.pulses, 2);
        assert_eq!(app.sim.particles().len(), count);
        app.change_speed(true);
        app.reset().unwrap();
        assert!(app.paused);
        assert_eq!(app.sim.time(), 0.0);
        assert_eq!(app.splashes, 0);
        assert_eq!(app.pulses, 1);
        assert_eq!(app.speed, 0.5);
    }
}

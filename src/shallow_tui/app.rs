use std::time::Duration;
use water_simulation::solver::{Config, Simulation};

/// 交互状态只调用求解器的公开接口，不参与通量和 CFL 计算。
pub struct App {
    pub sim: Simulation,
    pub initial_mass: f64,
    pub initial_surface: Vec<(f64, f64)>,
    pub paused: bool,
    pub selected: usize,
    pub speed: f64,
    pub disturbances: usize,
    /// 保持纵轴不缩小；扰动超出范围时才向外扩展。
    pub surface_bounds: [f64; 2],
}

impl App {
    pub fn new(config: Config, paused: bool) -> Result<Self, String> {
        let sim = Simulation::new(config)?;
        let initial_surface = sim
            .cells()
            .iter()
            .enumerate()
            .map(|(i, q)| ((i as f64 + 0.5) * sim.dx(), q.h))
            .collect();
        Ok(Self {
            initial_mass: sim.mass(),
            initial_surface,
            sim,
            paused,
            selected: config.cells / 2,
            speed: 1.0,
            disturbances: 0,
            surface_bounds: [
                config.rest_depth - 0.25 * config.bump_height.max(0.05),
                config.rest_depth + 1.25 * config.bump_height.max(0.05),
            ],
        })
    }

    pub fn tick(&mut self, elapsed: Duration) -> Result<(), String> {
        if !self.paused {
            // 终端拖动/挂起后不追赶积累的几秒时间，避免一次长更新卡住输入。
            let advance = elapsed.as_secs_f64().min(0.1) * self.speed;
            if advance > 0.0 {
                self.sim.advance_to(self.sim.time() + advance)?;
                self.expand_surface_bounds();
            }
        }
        Ok(())
    }

    pub fn single_step(&mut self) -> Result<(), String> {
        self.paused = true;
        self.sim.step(self.sim.stable_dt())?;
        self.expand_surface_bounds();
        Ok(())
    }

    pub fn disturb_at(&mut self, x: f64) -> Result<(), String> {
        // 鼠标只传入物理坐标；实际状态修改仍由独立求解模块负责。
        let width = (0.02 * self.sim.config().length).max(2.0 * self.sim.dx());
        self.sim
            .add_disturbance(x, 0.08 * self.sim.config().rest_depth, width)?;
        self.selected = ((x / self.sim.dx()).floor() as usize).min(self.sim.cells().len() - 1);
        self.disturbances += 1;
        self.expand_surface_bounds();
        Ok(())
    }

    fn expand_surface_bounds(&mut self) {
        let padding = 0.02 * self.sim.config().rest_depth;
        for q in self.sim.cells() {
            if q.h < self.surface_bounds[0] {
                self.surface_bounds[0] = q.h - padding;
            }
            if q.h > self.surface_bounds[1] {
                self.surface_bounds[1] = q.h + padding;
            }
        }
    }

    pub fn reset(&mut self) -> Result<(), String> {
        let speed = self.speed;
        let selected = self.selected;
        *self = Self::new(self.sim.config(), true)?;
        self.speed = speed;
        self.selected = selected;
        Ok(())
    }

    pub fn toggle_grid(&mut self) -> Result<(), String> {
        let mut config = self.sim.config();
        config.cells = if config.cells == 41 { 201 } else { 41 };
        let speed = self.speed;
        *self = Self::new(config, true)?;
        self.speed = speed;
        Ok(())
    }

    pub fn change_speed(&mut self, faster: bool) {
        self.speed = (self.speed * if faster { 2.0 } else { 0.5 }).clamp(0.125, 8.0);
    }

    pub fn move_selection(&mut self, right: bool) {
        self.selected = if right {
            (self.selected + 1).min(self.sim.cells().len() - 1)
        } else {
            self.selected.saturating_sub(1)
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicking_a_paused_tank_changes_depth_without_advancing_or_adding_mass() {
        let mut app = App::new(Config::default(), true).unwrap();
        let mass = app.sim.mass();
        let before = app.sim.cells()[60].h;
        let x = 60.5 * app.sim.dx();
        app.disturb_at(x).unwrap();
        assert_eq!(app.selected, 60);
        assert_eq!(app.disturbances, 1);
        assert!(app.sim.cells()[60].h > before + 0.07);
        assert_eq!(app.sim.time(), 0.0);
        assert!(app.paused);
        assert!((app.sim.mass() - mass).abs() / mass < 1e-12);
        for _ in 0..12 {
            app.disturb_at(x).unwrap();
        }
        assert!(app.surface_bounds[1] > 1.25);
        app.reset().unwrap();
        assert_eq!(app.disturbances, 0);
        assert_eq!(app.surface_bounds, [0.95, 1.25]);
        app.paused = false;
        app.disturb_at(x).unwrap();
        assert!(!app.paused);
        app.toggle_grid().unwrap();
        assert_eq!(app.disturbances, 0);
    }

    #[test]
    fn pause_step_reset_and_grid_switch_preserve_expected_state() {
        let mut app = App::new(Config::default(), true).unwrap();
        app.tick(Duration::from_millis(50)).unwrap();
        assert_eq!(app.sim.time(), 0.0);
        let dt = app.sim.stable_dt();
        app.single_step().unwrap();
        assert_eq!(app.sim.steps(), 1);
        assert_eq!(app.sim.time(), dt);
        assert!(app.paused);
        app.paused = false;
        app.tick(Duration::from_millis(50)).unwrap();
        assert!(app.sim.time() > dt);
        app.change_speed(true);
        app.reset().unwrap();
        assert!(app.paused);
        assert_eq!(app.sim.time(), 0.0);
        assert_eq!(app.sim.steps(), 0);
        assert_eq!(app.speed, 2.0);
        app.toggle_grid().unwrap();
        assert_eq!(app.sim.cells().len(), 41);
        assert_eq!(app.selected, 20);
        assert_eq!(app.sim.mass(), app.initial_mass);
        app.toggle_grid().unwrap();
        assert_eq!(app.sim.cells().len(), 201);
        for _ in 0..300 {
            app.move_selection(true);
        }
        assert_eq!(app.selected, 200);
        for _ in 0..300 {
            app.move_selection(false);
        }
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn playback_speed_changes_simulated_time_and_is_bounded() {
        let mut slow = App::new(Config::default(), false).unwrap();
        let mut fast = App::new(Config::default(), false).unwrap();
        fast.change_speed(true);
        slow.tick(Duration::from_millis(40)).unwrap();
        fast.tick(Duration::from_millis(40)).unwrap();
        assert!((fast.sim.time() - 2.0 * slow.sim.time()).abs() < 1e-14);
        for _ in 0..20 {
            fast.change_speed(true);
        }
        assert_eq!(fast.speed, 8.0);
        for _ in 0..20 {
            fast.change_speed(false);
        }
        assert_eq!(fast.speed, 0.125);
        let mut delayed = App::new(Config::default(), false).unwrap();
        delayed.tick(Duration::from_secs(10)).unwrap();
        assert!((delayed.sim.time() - 0.1).abs() < 1e-14);
    }
}

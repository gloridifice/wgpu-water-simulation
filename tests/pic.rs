use water_simulation::pic::{CellType, Config, Simulation};

fn small_config() -> Config {
    Config {
        nx: 48,
        ny: 32,
        cell_size: 0.2,
        pressure_iterations: 100,
        ..Config::default()
    }
}

#[test]
fn pressure_projection_reduces_divergence_and_respects_cfl() {
    let mut sim = Simulation::new(small_config()).unwrap();
    sim.wave_pulse();
    for _ in 0..25 {
        let speed = sim.diagnostics().max_speed;
        let dt = sim.step(0.01).unwrap();
        assert!(dt * speed / sim.config().cell_size <= sim.config().cfl + 1e-12);
        let d = sim.diagnostics();
        assert!(
            d.divergence_after <= (d.divergence_before * 0.05).max(1e-5),
            "{} -> {}",
            d.divergence_before,
            d.divergence_after
        );
    }
}

#[test]
fn flat_lake_stays_near_rest_with_fixed_walls() {
    let mut sim = Simulation::new(Config {
        beach: false,
        ..small_config()
    })
    .unwrap();
    sim.set_wavemaker(false);
    let initial = sim.particles().to_vec();
    sim.advance_to(0.2).unwrap();
    assert!(sim.diagnostics().max_speed < 0.001);
    for (before, after) in initial.iter().zip(sim.particles()) {
        assert!((before.x - after.x).abs() < 0.001 && (before.y - after.y).abs() < 0.001);
    }
}

#[test]
fn particles_are_not_lost_and_stay_inside_walls_and_beach() {
    let mut sim = Simulation::new(small_config()).unwrap();
    sim.wave_pulse();
    let (count, volume) = (sim.particles().len(), sim.volume());
    for frame in 1..=20 {
        sim.advance_to(frame as f64 * 0.1).unwrap();
        assert_eq!(sim.particles().len(), count);
        assert_eq!(sim.volume(), volume);
        for p in sim.particles() {
            assert!(p.x.is_finite() && p.y.is_finite() && p.u.is_finite() && p.v.is_finite());
            assert!(p.x >= sim.paddle_x() - 1e-10 && p.x < sim.config().width());
            assert!(p.y >= sim.grid().bed_height(p.x) - 1e-10 && p.y < sim.config().height());
        }
    }
}

#[test]
fn breaking_pulse_produces_an_overhanging_free_surface() {
    let mut sim = Simulation::new(Config::default()).unwrap();
    sim.wave_pulse();
    let mut peak_overhangs = 0;
    while sim.time() < 0.4 {
        sim.step(0.4 - sim.time()).unwrap();
        peak_overhangs = peak_overhangs.max(sim.diagnostics().overhang_columns);
    }
    assert!(
        peak_overhangs >= 1,
        "expected air below an overturned water sheet"
    );
}

#[test]
fn mouse_impulse_changes_only_velocity_and_invalid_input_is_rejected() {
    let mut sim = Simulation::new(small_config()).unwrap();
    let original = sim.particles().to_vec();
    let n = sim.disturb(2.0, 1.5).unwrap();
    assert!(n > 0);
    assert_eq!(sim.time(), 0.0);
    assert!(sim.diagnostics().max_speed > 0.0);
    for (a, b) in sim.particles().iter().zip(&original) {
        assert_eq!((a.x, a.y), (b.x, b.y));
    }
    assert!(sim.disturb(f64::NAN, 1.0).is_err());
    assert!(sim.disturb(-1.0, 1.0).is_err());
    for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(sim.step(dt).is_err());
    }
    assert!(sim.advance_to(-1.0).is_err());
    assert!(
        Simulation::new(Config {
            nx: 1,
            ..Config::default()
        })
        .is_err()
    );
    assert!(
        Simulation::new(Config {
            gravity: f64::NAN,
            ..Config::default()
        })
        .is_err()
    );
    assert_eq!(sim.grid().u.len(), (sim.config().nx + 1) * sim.config().ny);
    assert_eq!(sim.grid().v.len(), sim.config().nx * (sim.config().ny + 1));
    assert!(sim.grid().kind.contains(&CellType::Fluid) && sim.grid().kind.contains(&CellType::Air));
}

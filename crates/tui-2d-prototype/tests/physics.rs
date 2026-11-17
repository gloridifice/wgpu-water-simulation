use tui_2d_prototype::{Cell, Config, Simulation, rusanov_flux};

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "actual={actual:.17e}, expected={expected:.17e}, tolerance={tolerance:.1e}"
    );
}

#[test]
fn equal_states_give_the_physical_flux() {
    let q = Cell { h: 2.0, hu: 3.0 };
    let flux = rusanov_flux(q, q, 9.81);
    close(flux.mass, 3.0, 0.0);
    close(flux.momentum, 24.12, 1e-14);
}

#[test]
fn a_depth_jump_has_the_expected_diffusive_flux() {
    let flux = rusanov_flux(Cell { h: 1.0, hu: 0.0 }, Cell { h: 2.0, hu: 0.0 }, 9.81);
    // With both sides at rest, numerical mass flux should point left from high to low water.
    close(flux.mass, -0.5 * 19.62_f64.sqrt(), 1e-14);
    close(flux.momentum, 12.2625, 1e-14);
}

#[test]
fn reflecting_walls_have_zero_mass_flux_but_exert_force() {
    for hu in [-2.0, 0.0, 2.0] {
        let q = Cell { h: 1.3, hu };
        let left = rusanov_flux(q.reflected(), q, 9.81);
        let right = rusanov_flux(q, q.reflected(), 9.81);
        close(left.mass, 0.0, 0.0);
        close(right.mass, 0.0, 0.0);
        close(
            left.momentum,
            q.physical_flux(9.81).momentum - q.wave_speed(9.81) * hu,
            1e-14,
        );
        close(
            right.momentum,
            q.physical_flux(9.81).momentum + q.wave_speed(9.81) * hu,
            1e-14,
        );
    }
}

#[test]
fn a_lake_at_rest_stays_at_rest() {
    let config = Config {
        bump_height: 0.0,
        ..Config::default()
    };
    let mut sim = Simulation::new(config).unwrap();
    sim.advance_to(10.0).unwrap();
    for q in sim.cells() {
        assert_eq!(*q, Cell { h: 1.0, hu: 0.0 });
    }
}

#[test]
fn mass_is_conserved_through_wall_reflections() {
    for cells in [41, 201] {
        let mut sim = Simulation::new(Config {
            cells,
            ..Config::default()
        })
        .unwrap();
        let initial = sim.mass();
        while sim.time() < 12.0 {
            sim.step(12.0 - sim.time()).unwrap();
            assert!((sim.mass() - initial).abs() / initial < 1e-12);
        }
    }
}

#[test]
fn depth_is_symmetric_and_momentum_is_antisymmetric() {
    for cells in [41, 201] {
        let mut sim = Simulation::new(Config {
            cells,
            ..Config::default()
        })
        .unwrap();
        while sim.time() < 12.0 {
            sim.step(12.0 - sim.time()).unwrap();
            for (left, right) in sim.cells().iter().zip(sim.cells().iter().rev()) {
                close(left.h, right.h, 1e-12);
                close(left.hu, -right.hu, 1e-12);
            }
            close(sim.cells()[cells / 2].hu, 0.0, 1e-12);
        }
    }
}

#[test]
fn every_step_respects_the_current_cfl_limit() {
    let mut sim = Simulation::new(Config::default()).unwrap();
    let initial_dt = sim.stable_dt();
    let mut changed = false;
    for _ in 0..300 {
        let speed = sim.max_wave_speed();
        let dt = sim.step(1.0).unwrap();
        close(dt * speed / sim.dx(), sim.config().cfl, 1e-14);
        changed |= (dt - initial_dt).abs() > 1e-6;
    }
    assert!(changed, "dt must respond to the evolving water state");
    let cap = sim.stable_dt() * 0.1;
    close(sim.step(cap).unwrap(), cap, 0.0);
}

fn energy(sim: &Simulation) -> f64 {
    sim.cells()
        .iter()
        .map(|q| 0.5 * q.hu * q.hu / q.h + 0.5 * sim.config().gravity * q.h * q.h)
        .sum::<f64>()
        * sim.dx()
}

#[test]
fn wet_steep_mound_stays_finite_positive_and_bounded() {
    // Steeper than the default Gaussian bump; a long run spans multiple reflections without dry cells.
    for count in [41, 201] {
        let config = Config {
            cells: count,
            cfl: 0.5,
            ..Config::default()
        };
        let cells = (0..count)
            .map(|i| Cell {
                h: if i.abs_diff(count / 2) <= count / 10 {
                    2.0
                } else {
                    1.0
                },
                hu: 0.0,
            })
            .collect();
        let mut sim = Simulation::from_cells(config, cells).unwrap();
        let initial_energy = energy(&sim);
        while sim.time() < 30.0 {
            sim.step(30.0 - sim.time()).unwrap();
            for q in sim.cells() {
                assert!(q.h.is_finite() && q.hu.is_finite());
                assert!(q.h > 0.0 && q.h < 3.0, "unbounded depth: {}", q.h);
                assert!(q.velocity().abs() < 5.0);
            }
            assert!(energy(&sim) <= initial_energy * (1.0 + 1e-10));
        }
    }
}

#[test]
fn small_waves_propagate_at_the_expected_speed() {
    let config = Config {
        cells: 401,
        bump_height: 0.001,
        bump_width: 0.4,
        ..Config::default()
    };
    let mut sim = Simulation::new(config).unwrap();
    sim.advance_to(0.8).unwrap();
    let (peak_index, _) = sim
        .cells()
        .iter()
        .enumerate()
        .skip(config.cells / 2)
        .max_by(|(_, a), (_, b)| a.h.total_cmp(&b.h))
        .unwrap();
    let peak_x = (peak_index as f64 + 0.5) * sim.dx();
    // In the small-amplitude limit, the right-moving crest is near L/2 + sqrt(g h0) * t.
    let expected = 0.5 * config.length + (config.gravity * config.rest_depth).sqrt() * sim.time();
    close(peak_x, expected, 0.15);
}

#[test]
fn outgoing_wave_returns_after_hitting_the_wall() {
    let mut sim = Simulation::new(Config::default()).unwrap();
    sim.advance_to(2.0).unwrap();
    let right_momentum = |sim: &Simulation| {
        sim.cells()[sim.cells().len() / 2 + 1..]
            .iter()
            .map(|q| q.hu)
            .sum::<f64>()
    };
    assert!(right_momentum(&sim) > 0.5);
    sim.advance_to(4.0).unwrap();
    assert!(right_momentum(&sim) < -0.5);
}

#[test]
fn invalid_inputs_fail_without_changing_the_current_state() {
    for cfl in [0.0, -0.1, 0.51, f64::NAN, f64::INFINITY] {
        assert!(
            Simulation::new(Config {
                cfl,
                ..Config::default()
            })
            .is_err()
        );
    }
    assert!(
        Simulation::new(Config {
            cells: 2,
            ..Config::default()
        })
        .is_err()
    );
    for h in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let config = Config {
            cells: 3,
            ..Config::default()
        };
        assert!(Simulation::from_cells(config, vec![Cell { h, hu: 0.0 }; 3]).is_err());
    }
    let mut sim = Simulation::new(Config::default()).unwrap();
    let before = sim.cells().to_vec();
    for dt in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(sim.step(dt).is_err());
        assert_eq!(sim.cells(), before);
        assert_eq!(sim.time(), 0.0);
        assert_eq!(sim.steps(), 0);
    }
    sim.advance_to(0.12345).unwrap();
    assert_eq!(sim.time(), 0.12345);
    let steps = sim.steps();
    sim.advance_to(sim.time()).unwrap();
    assert_eq!(sim.steps(), steps);
    assert!(sim.advance_to(0.0).is_err());
    assert!(sim.advance_to(f64::NAN).is_err());
}

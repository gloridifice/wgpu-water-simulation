use water_simulation::solver::{Config, Simulation};

#[test]
fn disturbances_conserve_mass_at_the_center_and_walls_and_then_propagate() {
    for cells in [41, 201] {
        for x in [0.0, 5.0, 10.0, 20.0] {
            let mut sim = Simulation::new(Config {
                cells,
                ..Config::default()
            })
            .unwrap();
            sim.advance_to(0.2).unwrap();
            let before = sim.cells().to_vec();
            let mass = sim.mass();
            let time = sim.time();
            let steps = sim.steps();
            let width = 0.4_f64.max(2.0 * sim.dx());
            let applied = sim.add_disturbance(x, 0.08, width).unwrap();
            assert!((applied - 0.08).abs() < 1e-14);
            assert_eq!(sim.time(), time);
            assert_eq!(sim.steps(), steps);
            assert!((sim.mass() - mass).abs() / mass < 1e-12);
            let mut rises = 0;
            let mut falls = 0;
            for (q, original) in sim.cells().iter().zip(&before) {
                assert_eq!(q.hu, original.hu);
                rises += usize::from(q.h > original.h);
                falls += usize::from(q.h < original.h);
            }
            assert!(
                rises > 0 && falls > 0,
                "ripple should redistribute existing water"
            );
            let disturbed = sim.cells().to_vec();
            sim.advance_to(6.0).unwrap();
            assert_ne!(sim.cells(), disturbed);
            assert!((sim.mass() - mass).abs() / mass < 1e-12);
            assert!(
                sim.cells()
                    .iter()
                    .all(|q| q.h > 0.0 && q.h.is_finite() && q.hu.is_finite())
            );
        }
    }
}

#[test]
fn repeated_large_disturbances_stay_wet_and_symmetric() {
    let mut sim = Simulation::new(Config::default()).unwrap();
    let mass = sim.mass();
    let mut limited = false;
    for _ in 0..40 {
        let before = sim.cells().to_vec();
        limited |= sim.add_disturbance(10.0, 10.0, 0.4).unwrap() < 10.0;
        for (q, original) in sim.cells().iter().zip(before) {
            assert!(q.h >= 0.74 * original.h);
        }
    }
    assert!(limited);
    assert!((sim.mass() - mass).abs() / mass < 1e-12);
    for (left, right) in sim.cells().iter().zip(sim.cells().iter().rev()) {
        assert!((left.h - right.h).abs() < 1e-12);
    }
}

#[test]
fn invalid_disturbances_leave_state_untouched() {
    let mut sim = Simulation::new(Config::default()).unwrap();
    let before = sim.cells().to_vec();
    for (x, amplitude, width) in [
        (-1.0, 0.08, 0.4),
        (21.0, 0.08, 0.4),
        (f64::NAN, 0.08, 0.4),
        (10.0, 0.0, 0.4),
        (10.0, f64::INFINITY, 0.4),
        (10.0, 0.08, 0.0),
        (10.0, 0.08, f64::NAN),
        (0.0, 0.08, f64::MIN_POSITIVE),
    ] {
        assert!(sim.add_disturbance(x, amplitude, width).is_err());
        assert_eq!(sim.cells(), before);
        assert_eq!(sim.time(), 0.0);
    }
}

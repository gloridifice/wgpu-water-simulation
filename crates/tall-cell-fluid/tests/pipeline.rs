use tall_cell_fluid::surface::extract_surface;
use tall_cell_fluid::{Domain, Grid, Simulation, SimulationConfig};

#[test]
fn hydrostatic_pool_survives_multiple_steps_with_reduced_pressure() {
    let mut grid = Grid::new(Domain::new([8, 32, 8], 0.1).unwrap());
    grid.set_liquid_sdf(|p| p[1] - 2.0);
    let mut sim = Simulation::from_grid(grid, SimulationConfig::default()).unwrap();
    let volume = sim.grid.liquid_volume();
    let reports = sim.advance(0.08).unwrap();
    assert!(reports.len() > 1);
    assert!(reports.iter().all(|r| r.projection.converged));
    assert!(
        reports
            .iter()
            .any(|r| r.projection.adaptive_pressure_dofs < 8 * 20 * 8)
    );
    assert!(reports.iter().any(|r| !r.projection.fallback_used));
    assert!(
        sim.grid.velocity.max_speed() < 1e-5,
        "{}",
        sim.grid.velocity.max_speed()
    );
    assert!((sim.grid.liquid_volume() - volume).abs() < 1e-9);
    let mesh = extract_surface(&sim.grid).unwrap();
    assert!(!mesh.indices.is_empty());
    assert!(
        mesh.vertices
            .iter()
            .all(|v| (v.position[1] - 2.0).abs() < 1e-4)
    );
}

#[test]
fn moving_liquid_and_obstacle_produce_finite_surface_and_bounded_volume() {
    let mut grid = Grid::new(Domain::new([14; 3], 0.1).unwrap());
    grid.set_liquid_sdf(|p| {
        let pool = p[1] - 0.4;
        let drop =
            ((p[0] - 0.7).powi(2) + (p[1] - 0.9).powi(2) + (p[2] - 0.7).powi(2)).sqrt() - 0.22;
        pool.min(drop)
    });
    grid.set_solid_sdf(|p| ((p[0] - 0.4).powi(2) + (p[2] - 0.4).powi(2)).sqrt() - 0.12);
    let mut sim = Simulation::from_grid(grid, SimulationConfig::default()).unwrap();
    let initial = sim.grid.phi.clone();
    let volume = sim.grid.liquid_volume();
    let reports = sim.advance(0.06).unwrap();
    assert!(reports.iter().all(|r| r.projection.converged));
    assert!(
        sim.grid
            .phi
            .iter()
            .zip(initial)
            .any(|(a, b)| (a - b).abs() > 1e-5)
    );
    assert!((sim.grid.liquid_volume() - volume).abs() / volume < 1e-6);
    assert!(sim.grid.divergence_rms() < 1e-4);
    sim.grid.validate().unwrap();
    let mesh = extract_surface(&sim.grid).unwrap();
    assert!(!mesh.vertices.is_empty());
    assert!(
        mesh.indices
            .iter()
            .all(|i| (*i as usize) < mesh.vertices.len())
    );
}

#[test]
fn empty_domain_and_invalid_inputs_do_not_create_water() {
    let mut sim = Simulation::new(
        Domain::new([6; 3], 0.2).unwrap(),
        SimulationConfig::default(),
    )
    .unwrap();
    sim.advance(0.05).unwrap();
    assert_eq!(sim.grid.liquid_volume(), 0.0);
    assert!(extract_surface(&sim.grid).unwrap().vertices.is_empty());
    assert!(sim.advance(-1.0).is_err());
    assert!(sim.advance(f64::INFINITY).is_err());
    let time = sim.time;
    assert!(sim.advance(0.0).unwrap().is_empty());
    assert_eq!(sim.time, time);
}

#[test]
fn cfl_limit_accounts_for_velocity_and_gravity() {
    let mut sim = Simulation::new(
        Domain::new([6; 3], 0.1).unwrap(),
        SimulationConfig::default(),
    )
    .unwrap();
    let calm = sim.stable_time_step();
    sim.grid.velocity.faces[0].fill(100.0);
    let fast = sim.stable_time_step();
    assert!(fast < calm);
    assert!(fast * 100.0 <= sim.config.cfl * sim.grid.domain.cell_size);
    assert!(sim.step(calm).is_err());
}

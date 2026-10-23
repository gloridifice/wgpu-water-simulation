//! 无窗口、无需资源下载的实际 GPU 验证。失败以非零退出码结束，不静默跳过。
use tall_cell_fluid::adaptivity::{AdaptiveGrid, AdaptivityConfig, PressureBasis};
use tall_cell_fluid::gpu::{self, GpuContext, GpuSimulation};
use tall_cell_fluid::surface::SurfaceLimits;
use tall_cell_fluid::{Domain, Grid};

fn main() -> Result<(), String> {
    let context = pollster::block_on(GpuContext::new())?;
    println!("Adapter: {}", context.adapter_name);
    verify_adaptivity(&context)?;
    verify_pressure(&context)?;
    verify_surface(&context)?;
    verify_pipeline(&context)?;
    println!("PASS: all offline GPU checks completed");
    Ok(())
}

fn verify_adaptivity(context: &GpuContext) -> Result<(), String> {
    for size in [[7, 33, 5], [4, 128, 4], [2, 2, 2]] {
        let mut grid = Grid::new(Domain::new(size, 0.1)?);
        let top = size[1] as f64 * 0.07;
        grid.set_liquid_sdf(|p| p[1] - top);
        if size[1] == 33 {
            // 不规则静态台阶以及空中液体，验证连续水柱分段。
            grid.set_solid_sdf(|p| if p[0] < 0.25 { p[1] - 0.3 } else { 1.0 });
            for n in 0..grid.phi.len() {
                let c = grid.domain.coord(n);
                if c[0] == 5 && c[1] > 10 && c[1] < 14 {
                    grid.phi[n] = 0.1;
                }
            }
        }
        for axis in 0..3 {
            let d = grid.velocity.dimensions(axis);
            for n in 0..grid.velocity.faces[axis].len() {
                let c = [n % d[0], (n / d[0]) % d[1], n / (d[0] * d[1])];
                let p = grid.velocity.position(axis, c);
                grid.velocity.faces[axis][n] =
                    ((p[1] * 2.3).sin() + 0.03 * p[0] * p[2]) * (axis + 1) as f64 * 0.2;
            }
        }
        grid.enforce_boundaries();
        let config = AdaptivityConfig::default();
        let cpu = AdaptiveGrid::build(&grid, &config);
        let gpu = gpu::build_adaptive(context, &grid, &config)?;
        let error = cpu
            .local_cost
            .iter()
            .zip(&gpu.local_cost)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        let integral_error = cpu
            .integrated_cost
            .iter()
            .zip(&gpu.integrated_cost)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        assert!(error < 2e-4, "q error {error}");
        assert!(integral_error < 2e-4, "I error {integral_error}");
        assert_eq!(
            cpu.optical_mask, gpu.optical_mask,
            "GPU optical mask differs"
        );
        assert_eq!(cpu.dof_count, gpu.dof_count);
        println!(
            "PASS adaptivity {size:?}: q max error={error:.3e}, I max error={integral_error:.3e}, DOFs={}",
            gpu.dof_count
        );
    }
    Ok(())
}

fn verify_pressure(context: &GpuContext) -> Result<(), String> {
    let mut grid = Grid::new(Domain::new([8, 10, 6], 0.1)?);
    grid.set_liquid_sdf(|_| -1.0);
    grid.set_solid_sdf(|p| (p[0] - 0.4).abs() - 0.045);
    for axis in 0..3 {
        for (i, v) in grid.velocity.faces[axis].iter_mut().enumerate() {
            *v = ((i * 17 + axis * 3) as f64).sin() * 0.2;
        }
    }
    grid.enforce_boundaries();
    let mut cpu = grid.clone();
    let fine = AdaptiveGrid::uniform(&grid);
    let config = gpu::default_config().pressure;
    let report = gpu::project(context, &mut grid, &fine, 0.01, 1000.0, &config)?;
    assert!(report.converged, "{report:?}");
    let cpu_config = tall_cell_fluid::pressure::PressureConfig::default();
    tall_cell_fluid::pressure::project(&mut cpu, &fine, 0.01, 1000.0, &cpu_config)?;
    let difference = grid
        .velocity
        .faces
        .iter()
        .flatten()
        .zip(cpu.velocity.faces.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    assert!(difference < 5e-4, "CPU/GPU velocity error {difference}");
    println!(
        "PASS closed pressure: divergence {:.3e} -> {:.3e} /s, velocity CPU error {difference:.3e}",
        report.divergence_before, report.divergence_after
    );

    let mut coarse_grid = Grid::new(Domain::new([5, 8, 5], 0.1)?);
    coarse_grid.set_liquid_sdf(|p| p[1] - 0.55);
    for (i, v) in coarse_grid.velocity.faces[0].iter_mut().enumerate() {
        *v = (i as f64).sin() * 0.1;
    }
    coarse_grid.enforce_boundaries();
    let mut collapsed = AdaptiveGrid::uniform(&coarse_grid);
    collapsed.dof_count = 1;
    for n in 0..collapsed.basis.len() {
        if coarse_grid.is_fluid(coarse_grid.domain.coord(n)) {
            collapsed.basis[n] = PressureBasis {
                indices: [0, 0],
                weights: [1.0, 0.0],
            };
        }
    }
    let fallback = gpu::project(context, &mut coarse_grid, &collapsed, 0.01, 1000.0, &config)?;
    assert!(fallback.converged && fallback.fallback_used, "{fallback:?}");
    println!(
        "PASS GPU fine fallback: adaptive divergence {:.3e} -> {:.3e} /s",
        fallback.adaptive_divergence, fallback.divergence_after
    );
    Ok(())
}

fn verify_surface(context: &GpuContext) -> Result<(), String> {
    let mut grid = Grid::new(Domain::new([12; 3], 0.1)?);
    for value in [-1.0, 0.0, 1.0] {
        grid.phi.fill(value);
        assert!(
            gpu::extract_surface(context, &grid, SurfaceLimits::default())?
                .indices
                .is_empty()
        );
    }
    grid.set_liquid_sdf(|p| p[1] - 0.43);
    let plane = gpu::extract_surface(context, &grid, SurfaceLimits::default())?;
    assert!(!plane.vertices.is_empty());
    assert!(
        plane
            .vertices
            .iter()
            .all(|v| (v.position[1] - 0.43).abs() < 1e-6 && v.normal[1] > 0.999)
    );
    grid.set_liquid_sdf(|p| p.iter().map(|v| (v - 0.6).powi(2)).sum::<f64>().sqrt() - 0.35);
    let sphere = gpu::extract_surface(context, &grid, SurfaceLimits::default())?;
    assert!(!sphere.vertices.is_empty());
    for v in &sphere.vertices {
        let p = v.position.map(|x| f64::from(x) - 0.6);
        let r = p.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((r - 0.35).abs() < 0.025);
        assert!(
            p.iter()
                .zip(v.normal)
                .map(|(a, b)| a * f64::from(b) / r)
                .sum::<f64>()
                > 0.96
        );
    }
    assert!(
        gpu::extract_surface(
            context,
            &grid,
            SurfaceLimits {
                max_vertices: 2,
                max_triangles: 1
            }
        )
        .is_err()
    );
    println!(
        "PASS GPU surface: plane {} triangles, sphere {} triangles, capacity error",
        plane.indices.len() / 3,
        sphere.indices.len() / 3
    );
    Ok(())
}

fn verify_pipeline(context: &GpuContext) -> Result<(), String> {
    let mut pool = Grid::new(Domain::new([8, 32, 8], 0.1)?);
    pool.set_liquid_sdf(|p| p[1] - 2.0);
    let initial_volume = pool.liquid_volume();
    let mut config = gpu::default_config();
    config.reinitialize_every = 2; // 本次验证明确覆盖距离重建。
    let mut sim = GpuSimulation::from_grid(context, pool, config.clone())?;
    let reports = sim.advance(0.04)?;
    assert!(reports.iter().all(|r| r.projection.converged));
    assert!(
        reports
            .iter()
            .any(|r| !r.projection.fallback_used && r.projection.adaptive_pressure_dofs < 1280)
    );
    assert!(
        sim.grid.velocity.max_speed() < 5e-4,
        "hydrostatic speed {}",
        sim.grid.velocity.max_speed()
    );
    assert!((sim.grid.liquid_volume() - initial_volume).abs() / initial_volume < 5e-5);
    let mesh = gpu::extract_surface(context, &sim.grid, SurfaceLimits::default())?;
    assert!(
        mesh.vertices
            .iter()
            .all(|v| (v.position[1] - 2.0).abs() < 2e-4)
    );
    println!(
        "PASS hydrostatic GPU pipeline: steps={}, speed={:.3e} m/s, DOFs={}",
        sim.steps,
        sim.grid.velocity.max_speed(),
        reports.last().unwrap().projection.pressure_dofs
    );

    let mut moving = Grid::new(Domain::new([12; 3], 0.1)?);
    moving.set_liquid_sdf(|p| {
        (p[1] - 0.35)
            .min(((p[0] - 0.6).powi(2) + (p[1] - 0.85).powi(2) + (p[2] - 0.6).powi(2)).sqrt() - 0.2)
    });
    moving.set_solid_sdf(|p| ((p[0] - 0.3).powi(2) + (p[2] - 0.3).powi(2)).sqrt() - 0.09);
    let initial = moving.phi.clone();
    let volume = moving.liquid_volume();
    let mut sim = GpuSimulation::from_grid(context, moving, config.clone())?;
    let moving_reports = sim.advance(0.04)?;
    assert!(
        sim.grid
            .phi
            .iter()
            .zip(initial)
            .any(|(a, b)| (a - b).abs() > 1e-5)
    );
    assert!(
        (sim.grid.liquid_volume() - volume).abs() / volume < 1e-4,
        "volume before={volume}, after={}, reports={moving_reports:?}",
        sim.grid.liquid_volume()
    );
    assert!(sim.grid.divergence_rms() < config.pressure.divergence_tolerance);
    assert!(
        !gpu::extract_surface(context, &sim.grid, SurfaceLimits::default())?
            .indices
            .is_empty()
    );
    let saved = sim.grid.phi.clone();
    let time = sim.time;
    assert!(sim.step(1.0).is_err());
    assert_eq!(sim.grid.phi, saved);
    assert_eq!(sim.time, time);
    println!(
        "PASS moving liquid + static solid + reinitialization: divergence={:.3e} /s",
        sim.grid.divergence_rms()
    );

    let mut empty =
        GpuSimulation::from_grid(context, Grid::new(Domain::new([4; 3], 0.1)?), config)?;
    empty.advance(0.02)?;
    assert_eq!(empty.grid.liquid_volume(), 0.0);
    assert_eq!(empty.grid.velocity.max_speed(), 0.0);
    println!("PASS empty domain and transactional CFL rejection");
    Ok(())
}

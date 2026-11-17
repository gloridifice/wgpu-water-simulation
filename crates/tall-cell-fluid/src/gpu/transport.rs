use super::GpuContext;
use crate::{Grid, SimulationConfig};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    dims: [u32; 4],
    offsets: [u32; 4],
    metric: [f32; 4],
    gravity: [f32; 4],
    control: [u32; 4],
}

struct Transport<'a> {
    context: &'a GpuContext,
    pipeline: wgpu::ComputePipeline,
    volume_pipeline: wgpu::ComputePipeline,
    params: Params,
    uniform: wgpu::Buffer,
    states: [wgpu::Buffer; 2],
    masks: [wgpu::Buffer; 2],
    original: wgpu::Buffer,
    statistics: wgpu::Buffer,
    current: usize,
}

impl<'a> Transport<'a> {
    fn new(
        context: &'a GpuContext,
        grid: &Grid,
        config: &SimulationConfig,
        dt: f64,
    ) -> Result<Self, String> {
        context.validate_grid(grid)?;
        let n = grid.domain.cell_count();
        let mut packed: Vec<f32> = grid
            .phi
            .iter()
            .chain(&grid.solid_phi)
            .map(|v| *v as f32)
            .collect();
        let mut offsets = [0u32; 4];
        for (axis, faces) in grid.velocity.faces.iter().enumerate() {
            offsets[axis] = packed.len() as u32;
            packed.extend(faces.iter().map(|v| *v as f32));
        }
        offsets[3] = packed.len() as u32;
        let dims = grid.domain.size.map(|n| n as u32);
        let h = grid.domain.cell_size;
        let params = Params {
            dims: [dims[0], dims[1], dims[2], n as u32],
            offsets,
            metric: [
                h as f32,
                dt as f32,
                config.density as f32,
                (grid.liquid_volume() / h.powi(3)) as f32,
            ],
            gravity: [
                config.gravity[0] as f32,
                config.gravity[1] as f32,
                config.gravity[2] as f32,
                0.0,
            ],
            control: [0; 4],
        };
        let mut statistics = vec![0.0f32; 4 + n.div_ceil(64)];
        statistics[1] = -(h as f32);
        statistics[2] = h as f32;
        Ok(Self {
            context,
            pipeline: context.pipeline(
                "transport and level set",
                include_str!("shaders/transport.wgsl"),
                "main",
            )?,
            volume_pipeline: context.pipeline(
                "parallel volume correction",
                include_str!("shaders/volume.wgsl"),
                "main",
            )?,
            uniform: context.uniform("transport params", &params),
            states: [
                context.buffer("transport A", &packed),
                context.buffer("transport B", &packed),
            ],
            masks: [
                context.buffer("valid A", &vec![0u32; packed.len()]),
                context.buffer("valid B", &vec![0u32; packed.len()]),
            ],
            original: context.buffer("original phi", &packed[..n]),
            statistics: context.buffer("volume shift and partial sums", &statistics),
            params,
            current: 0,
        })
    }

    fn run(&mut self, mode: u32) {
        self.params.control[0] = mode;
        self.context
            .queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&self.params));
        if mode == 7 || mode == 9 {
            let bindings = self.context.bind(
                &self.volume_pipeline,
                &[&self.uniform, &self.states[self.current], &self.statistics],
            );
            self.context.dispatch(
                &self.volume_pipeline,
                &bindings,
                if mode == 7 {
                    self.params.dims[3].div_ceil(64)
                } else {
                    1
                },
            );
            return;
        }
        let bindings = self.context.bind(
            &self.pipeline,
            &[
                &self.uniform,
                &self.states[self.current],
                &self.states[1 - self.current],
                &self.masks[self.current],
                &self.masks[1 - self.current],
                &self.original,
                &self.statistics,
            ],
        );
        self.context.dispatch(
            &self.pipeline,
            &bindings,
            self.params.offsets[3].div_ceil(64),
        );
        self.current = 1 - self.current;
    }

    fn extrapolate(&mut self, layers: usize) {
        self.run(0);
        for _ in 0..layers {
            self.run(1);
        }
    }

    fn reinitialize(&mut self) {
        let mut encoder = self
            .context
            .device
            .create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(
            &self.states[self.current],
            0,
            &self.original,
            0,
            u64::from(self.params.dims[3]) * 4,
        );
        self.context.queue.submit([encoder.finish()]);
        self.run(4);
        // The Jacobi wavefront spans at most the L1 diameter of the domain. The CPU reference backend uses fast sweeping.
        for _ in 0..self.params.dims[..3].iter().sum::<u32>() {
            self.run(5);
        }
        self.run(6);
    }

    fn download(&self, grid: &Grid) -> Result<Grid, String> {
        let packed: Vec<f32> = self
            .context
            .read(&self.states[self.current], self.params.offsets[3] as usize)?;
        if packed.iter().any(|v| !v.is_finite()) {
            return Err("GPU transport produced non-finite values".into());
        }
        let mut output = grid.clone();
        output.phi = packed[..grid.phi.len()]
            .iter()
            .map(|&v| f64::from(v))
            .collect();
        output.pressure.fill(0.0);
        for axis in 0..3 {
            output.velocity.faces[axis] = packed
                [self.params.offsets[axis] as usize..self.params.offsets[axis + 1] as usize]
                .iter()
                .map(|&v| f64::from(v))
                .collect();
        }
        Ok(output)
    }
}

pub(super) fn prepare(
    context: &GpuContext,
    grid: &Grid,
    config: &SimulationConfig,
    dt: f64,
    step: usize,
) -> Result<(Grid, f64), String> {
    let mut work = Transport::new(context, grid, config, dt)?;
    work.extrapolate(config.extrapolation_layers);
    work.run(2);
    if config.reinitialize_every > 0 && step.is_multiple_of(config.reinitialize_every) {
        work.reinitialize();
    }
    let shift = if config.conserve_volume {
        for _ in 0..28 {
            work.run(7);
            work.run(9);
        }
        work.run(8);
        f64::from(context.read::<f32>(&work.statistics, 1)?[0])
    } else {
        0.0
    };
    work.run(3);
    Ok((work.download(grid)?, shift))
}

pub(super) fn finish(
    context: &GpuContext,
    grid: &Grid,
    config: &SimulationConfig,
) -> Result<Grid, String> {
    let mut work = Transport::new(context, grid, config, 0.0)?;
    work.extrapolate(config.extrapolation_layers);
    let mut result = work.download(grid)?;
    result.pressure.clone_from(&grid.pressure);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Domain;

    #[test]
    #[ignore = "Requires a real GPU; run explicitly with --ignored"]
    fn gpu_advects_plane_reinitializes_distance_and_enforces_solids() {
        let context = pollster::block_on(GpuContext::new()).unwrap();
        let mut grid = Grid::new(Domain::new([12, 8, 8], 0.1).unwrap());
        grid.set_liquid_sdf(|p| p[0] - 0.6);
        grid.velocity.faces[0].fill(0.3);
        grid.enforce_boundaries();
        let config = SimulationConfig {
            gravity: [0.0; 3],
            reinitialize_every: 0,
            conserve_volume: false,
            ..SimulationConfig::default()
        };
        let mut volume_work = Transport::new(&context, &grid, &config, 0.0).unwrap();
        volume_work.run(7);
        let partials: Vec<f32> = context
            .read(&volume_work.statistics, 4 + grid.phi.len().div_ceil(64))
            .unwrap();
        let gpu_sum: f64 = partials[4..].iter().map(|&v| f64::from(v)).sum();
        assert!(
            (gpu_sum - grid.liquid_volume() / grid.domain.cell_size.powi(3)).abs() < 1e-3,
            "GPU partial volume={gpu_sum}, CPU={}, partials={partials:?}",
            grid.liquid_volume() / grid.domain.cell_size.powi(3)
        );
        volume_work.run(9);
        let stats: Vec<f32> = context.read(&volume_work.statistics, 4).unwrap();
        assert_eq!(stats[3], 1.0, "volume reduction {stats:?}");
        assert_eq!(stats[0], 0.0);
        let mut droplet = Grid::new(Domain::new([12; 3], 0.1).unwrap());
        droplet.set_liquid_sdf(|p| {
            (p[1] - 0.35).min(
                ((p[0] - 0.6).powi(2) + (p[1] - 0.85).powi(2) + (p[2] - 0.6).powi(2)).sqrt() - 0.2,
            )
        });
        droplet.set_solid_sdf(|p| ((p[0] - 0.3).powi(2) + (p[2] - 0.3).powi(2)).sqrt() - 0.09);
        let mut volume_work = Transport::new(&context, &droplet, &config, 0.0).unwrap();
        volume_work.run(7);
        let partials: Vec<f32> = context
            .read(&volume_work.statistics, 4 + droplet.phi.len().div_ceil(64))
            .unwrap();
        let sum: f64 = partials[4..].iter().map(|&v| f64::from(v)).sum();
        let cpu_partials: Vec<f64> = (0..droplet.phi.len())
            .collect::<Vec<_>>()
            .chunks(64)
            .map(|part| {
                part.iter()
                    .filter(|&&n| droplet.solid_phi[n] >= 0.0)
                    .map(|&n| (0.5 - droplet.phi[n] / 0.1).clamp(0.0, 1.0))
                    .sum()
            })
            .collect();
        assert!(
            (sum - droplet.liquid_volume() / 0.001).abs() < 1e-3,
            "droplet sum {sum} vs {}, GPU {:?}, CPU {:?}",
            droplet.liquid_volume() / 0.001,
            &partials[4..],
            cpu_partials
        );
        volume_work.run(9);
        let stats: Vec<f32> = context.read(&volume_work.statistics, 4).unwrap();
        assert_eq!(stats[3], 1.0, "droplet reduction {stats:?}");
        let mut cfg = config.clone();
        cfg.conserve_volume = true;
        let (corrected, shift) = prepare(&context, &droplet, &cfg, 0.01, 1).unwrap();
        assert!(
            (corrected.liquid_volume() - droplet.liquid_volume()).abs() < 1e-5,
            "stationary droplet volume before {},after {},shift {shift}",
            droplet.liquid_volume(),
            corrected.liquid_volume()
        );
        let target = droplet.liquid_volume();
        for value in &mut droplet.phi {
            *value += 0.02;
        }
        let mut shifted = Transport::new(&context, &droplet, &config, 0.0).unwrap();
        shifted.params.metric[3] = (target / 0.001) as f32;
        for _ in 0..28 {
            shifted.run(7);
            shifted.run(9);
        }
        shifted.run(8);
        let restored = shifted.download(&droplet).unwrap();
        let correction = context.read::<f32>(&shifted.statistics, 1).unwrap()[0];
        assert!(
            (correction + 0.02).abs() < 2e-6,
            "volume correction {correction}"
        );
        assert!((restored.liquid_volume() - target).abs() < 1e-6);
        let (advected, _) = prepare(&context, &grid, &config, 0.05, 1).unwrap();
        for z in 2..6 {
            for y in 2..6 {
                for x in 4..7 {
                    let c = [x, y, z];
                    let expected = grid.domain.center(c)[0] - 0.6 - 0.3 * 0.05;
                    assert!((advected.phi[grid.domain.index(c)] - expected).abs() < 2e-7);
                }
            }
        }
        for normal in [[0.0, 1.0, 0.0], [0.6, 0.8, 0.0]] {
            grid.set_liquid_sdf(|p| 2.0 * (normal[0] * p[0] + normal[1] * p[1] - 0.43));
            let mut cpu = grid.clone();
            crate::simulation::reinitialize_level_set(&mut cpu);
            let mut work = Transport::new(&context, &grid, &config, 0.0).unwrap();
            work.reinitialize();
            let rebuilt = work.download(&grid).unwrap();
            for n in 0..grid.phi.len() {
                assert!(
                    (rebuilt.phi[n] - cpu.phi[n]).abs() < 2e-5,
                    "GPU/CPU distance n={n}: {} vs {}",
                    rebuilt.phi[n],
                    cpu.phi[n]
                );
                let c = grid.domain.coord(n);
                // Require exact plane-normal distance only at seeds with a zero-crossing neighbor;
                // projections outside the domain and first-order far-field Eikonal discretization do not satisfy this analytic constraint.
                let seed = (0..3).any(|axis| {
                    [-1_isize, 1].into_iter().any(|direction| {
                        let j = c[axis] as isize + direction;
                        if j < 0 || j >= grid.domain.size[axis] as isize {
                            return false;
                        }
                        let mut q = c;
                        q[axis] = j as usize;
                        (grid.phi[grid.domain.index(q)] < 0.0) != (grid.phi[n] < 0.0)
                    })
                });
                if seed {
                    assert!(
                        (rebuilt.phi[n] - 0.5 * grid.phi[n]).abs() < 3e-6,
                        "distance n={n}, {} vs {}",
                        rebuilt.phi[n],
                        grid.phi[n] * 0.5
                    );
                }
            }
        }
        grid.set_liquid_sdf(|_| -1.0);
        grid.set_solid_sdf(|p| (p[0] - 0.6).abs() - 0.15);
        for values in &mut grid.velocity.faces {
            values.fill(1.0);
        }
        let finished = finish(&context, &grid, &config).unwrap();
        let mut enforced = finished.clone();
        enforced.enforce_boundaries();
        assert_eq!(finished.velocity.faces, enforced.velocity.faces);
        println!(
            "PASS GPU transport: analytic advection, oblique distance, solid walls on {}",
            context.adapter_name
        );
    }
}

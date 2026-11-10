//! Independent affine-density oracle on a sloped bed. A constant-height layer
//! over b=0.05*x has rho=2*decay*(1-(y-b)/h), which is exactly affine inside
//! every tetrahedron. No projected fan, tetrahedron split or GPU clipping code
//! is reproduced in this oracle; compare actual MSAA-resolved opacity.
use crate::{
    Error,
    parameters::Session,
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::Gpu;
pub(crate) fn run(gpu: &Gpu) -> Result<(), Error> {
    super::tet_reconstruction_verification::run(gpu)?;
    let floor = vec![
        [[-8.0, -0.4, -6.0], [8.0, 0.4, 6.0], [8.0, 0.4, -6.0]],
        [[-8.0, -0.4, -6.0], [-8.0, -0.4, 6.0], [8.0, 0.4, 6.0]],
    ];
    let wall = vec![
        [[-8.0, -0.4, 0.0], [8.0, 0.4, 0.0], [8.0, 4.0, 0.0]],
        [[-8.0, -0.4, 0.0], [8.0, 4.0, 0.0], [-8.0, 4.0, 0.0]],
    ];
    let mut visible_cases = 0;
    let mut wall_cases = 0;
    for opaque in [false, true] {
        let source = BakeSource {
            floor: floor.clone(),
            obstacle: if opaque { wall.clone() } else { vec![] },
        };
        let terrain = Terrain::bake(&source, [-8.0, -6.0, 16.0, 12.0], [13, 7])?;
        let mut sim = Simulation::new(gpu, &terrain)?;
        let mut s = Session {
            debug_view: 5,
            ..Default::default()
        };
        s.fog.base_height = 0.6;
        s.fog.height_offset = 0.0;
        s.fog.noise_amplitude = 0.0;
        s.fog.gravity_enabled = false;
        s.fog.wind = [0.0; 2];
        s.fog.half_life = 1.0;
        s.fog.decay_enabled = true;
        s.fog.extinction = 0.5;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        sim.encode(gpu, &mut encoder, &s.fog, 0.0, true, &[])?;
        gpu.queue.submit([encoder.finish()]);
        for _ in 0..5 {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            sim.encode(gpu, &mut encoder, &s.fog, 0.1, false, &[])?;
            gpu.queue.submit([encoder.finish()]);
        }
        let (clock, _) = sim.snapshot(gpu)?;
        let decay = (-std::f64::consts::LN_2 * f64::from(clock.simulated_time)).exp();
        let renderer = super::Renderer::new(
            gpu,
            &sim,
            &terrain,
            &source,
            wgpu::TextureFormat::Rgba8Unorm,
        )?;
        let size = [320, 180];
        for detail in [false, true] {
            s.reconstruction_detail = detail;
            for mode in [
                crate::parameters::Reconstruction::Bilinear,
                crate::parameters::Reconstruction::CubicBSpline,
                crate::parameters::Reconstruction::CatmullRom,
                crate::parameters::Reconstruction::PreSmoothed,
                crate::parameters::Reconstruction::SmoothStep,
            ] {
                if !detail && mode == crate::parameters::Reconstruction::SmoothStep {
                    continue;
                }
                s.reconstruction = mode;
                for yaw in [0.0, std::f32::consts::PI] {
                    s.camera = [yaw, 0.3, 5.5];
                    let orbit = crate::scene::camera_collision::orbit(&s.player, s.camera, &source);
                    let (_, inverse) =
                        orbit.matrices(55.0f32.to_radians(), 320.0 / 180.0, 0.1, 200.0);
                    let origin = orbit.eye().as_dvec3();
                    let image = renderer.read_image(gpu, &s, sim.domain, size)?;
                    for (x, y) in [
                        (32, 100),
                        (64, 110),
                        (64, 150),
                        (64, 170),
                        (250, 100),
                        (250, 110),
                        (250, 150),
                        (280, 170),
                    ] {
                        let far = inverse
                            * glam::Vec4::new(
                                (x as f32 + 0.5) / 320.0 * 2.0 - 1.0,
                                1.0 - (y as f32 + 0.5) / 180.0 * 2.0,
                                1.0,
                                1.0,
                            );
                        let ray = (far.truncate() / far.w - orbit.eye())
                            .normalize()
                            .as_dvec3();
                        let mut end = 64.0;
                        let slope = ray.y - 0.05 * ray.x;
                        if slope < 0.0 {
                            let t = -(origin.y - 0.05 * origin.x) / slope;
                            let p = origin + ray * t;
                            if p.x.abs() <= 8.0 && p.z.abs() <= 6.0 {
                                end = t.min(end);
                            }
                        }
                        if opaque && ray.z.abs() > 1e-10 {
                            let t = -origin.z / ray.z;
                            let p = origin + ray * t;
                            if t > 0.0
                                && p.x.abs() <= 8.0
                                && p.y >= 0.05 * p.x
                                && p.y <= 4.0
                                && t < end
                            {
                                end = t;
                                wall_cases += 1;
                            }
                        }
                        let dt = end / 131072.0;
                        let mut optical = 0.0;
                        for i in 0..131072 {
                            let p = origin + ray * ((i as f64 + 0.5) * dt);
                            let relative = (p.y - 0.05 * p.x) / 0.6;
                            if (0.0..1.0).contains(&relative) && p.x.abs() < 8.0 && p.z.abs() < 6.0
                            {
                                // Keep the oracle away from the intentionally clamped
                                // outer vertex ring, whose field is not the infinite plane.
                                if p.x.abs() > 7.3 || p.z.abs() > 5.1 {
                                    return Err(
                                        "tet oracle ray touched the outer reconstruction ring"
                                            .into(),
                                    );
                                }
                                optical += f64::from(s.fog.extinction)
                                    * 2.0
                                    * decay
                                    * (1.0 - relative)
                                    * dt;
                            }
                        }
                        let expected = 1.0 - (-optical).exp();
                        let actual = f64::from(image[((y * 320 + x) * 4) as usize]) / 255.0;
                        if expected > 0.01 && expected < 0.95 {
                            visible_cases += 1;
                        }
                        if (actual - expected).abs() > 0.006 {
                            return Err(format!("tetrahedron affine oracle {mode:?} detail={detail} wall={opaque} yaw={yaw} pixel=({x},{y}): opacity {actual}, f64 {expected}").into());
                        }
                    }
                }
            }
        }
    }
    if visible_cases < 4 || wall_cases < 4 {
        return Err("tet oracle did not exercise visible fog and wall clipping".into());
    }
    eprintln!(
        "Fog tetrahedron density: PASS (all vertex kernels and both topologies, 13x7 sloped affine field, independent f64 quadrature and m/h decay, reverse views, opaque wall; {visible_cases} visible and {wall_cases} wall-clipped rays)"
    );
    Ok(())
}

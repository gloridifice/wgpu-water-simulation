//! Raster coverage across periodic tile seams and reversible topology switches.
use super::optics::raster_image;
use crate::ocean::{
    Error,
    camera::Camera,
    gpu::context::Gpu,
    parameters::{ocean::OceanConfig, optics::OpticsConfig},
    render::Renderer,
    simulation::offshore::{Ocean, foam::Foam},
};
use glam::{Mat4, Vec4};

pub(super) fn run(gpu: &Gpu) -> Result<(), Error> {
    let size = [97, 81];
    for amplitude in [0.0, 0.02] {
        let config = OceanConfig {
            resolution: 32,
            length: 32.0,
            mesh_resolution: 47,
            amplitude,
            ..Default::default()
        };
        let ocean = Ocean::new(gpu, config)?;
        let foam = Foam::new(gpu, &ocean.fields, config.resolution);
        let mut renderer = Renderer::new(
            gpu,
            &ocean,
            &foam,
            wgpu::TextureFormat::Rgba32Float,
            size[0],
            size[1],
        );
        let mut camera = Camera::new(config.length);
        camera.pitch = 1.25;
        camera.distance = 28.0;
        camera.target = [16.0, 16.0]; // Four tiles meet in the middle of the view.
        camera.foam = false;
        let settings = OpticsConfig {
            depth: 4.0,
            view: 1,
            ..Default::default()
        };
        let uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
        let inverse = Mat4::from_cols_array_2d(&uniform.inverse_view_projection);
        let eye = Vec4::from_array(uniform.eye).truncate();
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode(gpu, &mut encoder, 2.75);
        gpu.queue.submit([encoder.finish()]);
        let fields_before = ocean.read_fields(gpu)?;
        let square = raster_image(gpu, &renderer, &uniform, settings, size)?;
        renderer.set_ocean_mesh(
            &gpu.device,
            OceanConfig {
                triangular_mesh: true,
                ..config
            },
        );
        let triangular = raster_image(gpu, &renderer, &uniform, settings, size)?;
        renderer.set_ocean_mesh(&gpu.device, config);
        let restored = raster_image(gpu, &renderer, &uniform, settings, size)?;
        if square != restored || ocean.read_fields(gpu)? != fields_before {
            return Err(
                "mesh switch changed paused FFT state or failed to restore square raster".into(),
            );
        }
        if amplitude == 0.0 {
            // Every pixel sees the flat ocean. Analytic y=0 / y=-depth ray
            // separation catches missing coverage, bad UVs and broken tile seams.
            for (index, (a, b)) in square.iter().zip(&triangular).enumerate() {
                let x = index as u32 % size[0];
                let y = index as u32 / size[0];
                let far = inverse
                    * Vec4::new(
                        (x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0,
                        1.0 - (y as f32 + 0.5) / size[1] as f32 * 2.0,
                        1.0,
                        1.0,
                    );
                let ray = (far.truncate() / far.w - eye).normalize();
                let expected = settings.depth / -ray.y / settings.path_range;
                for value in a[..3].iter().chain(&b[..3]) {
                    if !value.is_finite() || (value - expected).abs() > 0.0005 {
                        return Err(format!(
                            "mesh tile coverage at ({x},{y}): {value} != {expected}"
                        )
                        .into());
                    }
                }
            }
        } else {
            let difference = square
                .iter()
                .zip(&triangular)
                .map(|(a, b)| (a[0] - b[0]).abs())
                .fold(0.0_f32, f32::max);
            if difference < 1e-6 || triangular.iter().flatten().any(|v| !v.is_finite()) {
                return Err(
                    "triangular mesh was not drawn, or produced invalid wave pixels".into(),
                );
            }
        }
    }
    println!(
        "GPU ocean mesh: square/staggered seam coverage, live switch/restore and unchanged FFT state OK"
    );
    Ok(())
}

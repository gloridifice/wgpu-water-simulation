//! End-to-end raster check: actual seabed depth, world-position reconstruction,
//! water integration, debug outputs and resized targets, without egui.
use super::{
    foam::Foam,
    gpu::{Error, Gpu, Ocean, Renderer},
    optics::{self, OpticsConfig},
    spectrum::OceanConfig,
    viewer::Camera,
};
use glam::{Mat4, Vec4};

fn image(
    gpu: &Gpu,
    renderer: &Renderer,
    ocean: &Ocean,
    camera: &super::gpu::CameraUniform,
    optics: OpticsConfig,
    size: [u32; 2],
) -> Result<Vec<[f32; 4]>, Error> {
    renderer.update_optics(gpu, &optics);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("optics raster check"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let pitch = (size[0] * 16).div_ceil(256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("optics raster readback"),
        size: u64::from(pitch * size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    ocean.encode(gpu, &mut encoder, 0.0);
    renderer.encode(
        gpu,
        &mut encoder,
        &texture.create_view(&Default::default()),
        camera,
    );
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pitch),
                rows_per_image: Some(size[1]),
            },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.map_buffer(&readback)?;
    Ok(bytes
        .chunks_exact(pitch as usize)
        .flat_map(|row| {
            row[..size[0] as usize * 16]
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
        })
        .collect())
}

pub fn run(gpu: &Gpu) -> Result<(), Error> {
    let config = OceanConfig {
        length: 64.0,
        resolution: 8,
        mesh_resolution: 8,
        amplitude: 0.0,
        ..Default::default()
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new(gpu, &ocean, &foam, wgpu::TextureFormat::Rgba32Float, 65, 49);
    let mut camera = Camera::new(config.length);
    camera.distance = 20.0;
    camera.pitch = 0.7;
    camera.yaw = 0.0;
    camera.foam = false;
    let mut max_error = 0.0_f64;
    for size in [[65, 49], [31, 33]] {
        renderer.resize(&gpu.device, size[0], size[1]);
        let uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
        let inverse = Mat4::from_cols_array_2d(&uniform.inverse_view_projection);
        let eye = Vec4::from_array(uniform.eye).truncate();
        for depth in [2.0, 8.0, 30.0] {
            let settings = OpticsConfig {
                depth,
                view: 6,
                sky: super::skylight::SkyConfig::legacy(),
                ..Default::default()
            };
            let raw = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
            for mode in 1..=7 {
                let pixels = image(
                    gpu,
                    &renderer,
                    &ocean,
                    &uniform,
                    OpticsConfig {
                        view: mode,
                        ..settings
                    },
                    size,
                )?;
                for [x, y] in [
                    [size[0] / 2, size[1] / 2],
                    [size[0] / 3, size[1] / 2],
                    [size[0] * 2 / 3, size[1] * 2 / 3],
                ] {
                    let pixel = (y * size[0] + x) as usize;
                    let uv = [
                        (x as f32 + 0.5) / size[0] as f32,
                        (y as f32 + 0.5) / size[1] as f32,
                    ];
                    let far = inverse * Vec4::new(uv[0] * 2.0 - 1.0, 1.0 - uv[1] * 2.0, 1.0, 1.0);
                    let direction = (far.truncate() / far.w - eye).normalize();
                    // Independent analytic intersections with y=0 and y=-depth.
                    let d = f64::from(depth / -direction.y);
                    for channel in 0..3 {
                        let bg = raw[pixel][channel] as f64;
                        if !bg.is_finite() || bg <= 0.0 {
                            return Err("missing seabed raster hit".into());
                        }
                        let parts = optics::integrate(
                            bg,
                            settings.sigma_t[channel] as f64,
                            settings.source[channel] as f64,
                            d,
                        );
                        let expected = match mode {
                            1 => (d / settings.path_range as f64).min(1.0),
                            2 => parts[0],
                            3 => parts[1],
                            4 => parts[2],
                            5 => bg * (1.0 - parts[0]),
                            6 => bg,
                            _ => parts[1] + parts[2],
                        };
                        let value = pixels[pixel][channel] as f64;
                        let error = (value - expected).abs();
                        if !value.is_finite() || error > 0.0005 {
                            return Err(format!("optics raster check failed: size={size:?}, depth={depth}, view={mode}, pixel=({x},{y}), RGB={channel}, actual={value}, expected={expected}").into());
                        }
                        max_error = max_error.max(error);
                    }
                }
            }
        }
    }
    // Real raster coverage for new debug modes, LUT invalidation and resize.
    for size in [[65, 49], [31, 33]] {
        renderer.resize(&gpu.device, size[0], size[1]);
        let uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
        let inverse = Mat4::from_cols_array_2d(&uniform.inverse_view_projection);
        let eye = Vec4::from_array(uniform.eye).truncate();
        for intensity in [0.0, 2.0, 1.0] {
            let settings = OpticsConfig {
                depth: 8.0,
                view: 6,
                sky: super::skylight::SkyConfig {
                    intensity,
                    color: [0.6, 1.0, 0.8],
                    steps: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let raw = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
            for mode in [4, 7, 8, 9, 10, 11] {
                let pixels = image(
                    gpu,
                    &renderer,
                    &ocean,
                    &uniform,
                    OpticsConfig {
                        view: mode,
                        ..settings
                    },
                    size,
                )?;
                for [x, y] in [
                    [size[0] / 2, size[1] / 2],
                    [size[0] / 3, size[1] / 2],
                    [size[0] * 2 / 3, size[1] * 2 / 3],
                ] {
                    let pixel = (y * size[0] + x) as usize;
                    let far = inverse
                        * Vec4::new(
                            2.0 * (x as f32 + 0.5) / size[0] as f32 - 1.0,
                            1.0 - 2.0 * (y as f32 + 0.5) / size[1] as f32,
                            1.0,
                            1.0,
                        );
                    let direction = (far.truncate() / far.w - eye).normalize();
                    let down = -direction.y as f64;
                    let d = settings.depth as f64 / down;
                    for c in 0..3 {
                        let manual = optics::integrate(
                            raw[pixel][c] as f64,
                            settings.sigma_t[c] as f64,
                            settings.source[c] as f64,
                            d,
                        );
                        let sky = super::skylight_verify::reference(&settings, d, 0.0, down, c);
                        let local = settings.sigma_t[c] as f64
                            * settings.sky.albedo[c] as f64
                            * super::skylight::incident_reference(
                                &settings.sky,
                                settings.sigma_t[c] as f64 * settings.depth as f64 * 0.5,
                                c,
                                32,
                            );
                        let expected = match mode {
                            4 => manual[2] + sky,
                            7 => manual[1] + manual[2] + sky,
                            8 => local,
                            9 => sky,
                            10 => manual[2],
                            _ => settings.source[c] as f64 + local,
                        };
                        let value = pixels[pixel][c] as f64;
                        if !value.is_finite() || (value - expected).abs() > 0.001 {
                            return Err(format!("sky raster mismatch: size={size:?}, intensity={intensity}, mode={mode}, RGB={c}: {value} vs {expected}").into());
                        }
                    }
                }
            }
        }
    }
    // Check tilted geometry against independent ray/plane intersections.
    let size = [65, 49];
    renderer.resize(&gpu.device, size[0], size[1]);
    let uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
    let inverse = Mat4::from_cols_array_2d(&uniform.inverse_view_projection);
    let eye = Vec4::from_array(uniform.eye).truncate();
    for slope in [0.10, 0.35] {
        let settings = OpticsConfig {
            beach_slope: slope,
            // This check has an analytic flat-water ray/plane reference.
            shore: super::shore::ShoreConfig {
                enabled: false,
                ..Default::default()
            },
            view: 6,
            ..OpticsConfig::beach_scene()
        };
        let raw = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        for mode in [1, 2, 3] {
            let pixels = image(
                gpu,
                &renderer,
                &ocean,
                &uniform,
                OpticsConfig {
                    view: mode,
                    ..settings
                },
                size,
            )?;
            for x in [size[0] / 3, size[0] / 2, size[0] * 2 / 3] {
                let y = size[1] / 2;
                let index = (y * size[0] + x) as usize;
                let far = inverse
                    * Vec4::new(
                        2.0 * (x as f32 + 0.5) / size[0] as f32 - 1.0,
                        1.0 - 2.0 * (y as f32 + 0.5) / size[1] as f32,
                        1.0,
                        1.0,
                    );
                let direction = (far.truncate() / far.w - eye).normalize();
                let surface = eye + direction * (-eye.y / direction.y);
                let distance = (-slope * (surface.x - settings.shore_x)
                    / (direction.y + slope * direction.x)) as f64;
                if distance <= 0.0 {
                    return Err("beach validation ray missed submerged slope".into());
                }
                for c in 0..3 {
                    let t = (-settings.sigma_t[c] as f64 * distance).exp();
                    let expected = match mode {
                        1 => (distance / settings.path_range as f64).min(1.0),
                        2 => t,
                        _ => raw[index][c] as f64 * t,
                    };
                    if (pixels[index][c] as f64 - expected).abs() > 0.001 {
                        return Err(format!(
                            "beach depth mismatch: slope={slope}, mode={mode}, x={x}, channel={c}"
                        )
                        .into());
                    }
                }
            }
        }
    }
    println!(
        "GPU beach: 2 slopes, 3 rays; path/transmittance/attenuated sand agree with ray-plane intersections OK"
    );
    println!(
        "GPU sky raster: 2 sizes, LUT tint/intensity updates, 6 combined/local/integrated debug views OK"
    );
    println!(
        "GPU optics raster: 2 sizes, 3 depths, 7 debug views, 3 pixels/RGB; max error={max_error:.3e} OK"
    );
    // Real render bindings must consume accumulated shore history after resize.
    let config = OceanConfig {
        mesh_resolution: 128,
        resolution: 128,
        amplitude: 0.002,
        wind_angle: 3.64,
        ..config
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new(gpu, &ocean, &foam, wgpu::TextureFormat::Rgba32Float, 97, 73);
    let settings = OpticsConfig {
        shore_x: 0.0,
        shore: super::shore::ShoreConfig {
            view: 3,
            ..Default::default()
        },
        ..OpticsConfig::beach_scene()
    };
    renderer.update_optics(gpu, &settings);
    for i in 0..=1440 {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode(gpu, &mut encoder, i as f32 / 60.0);
        renderer
            .shore
            .encode(gpu, &mut encoder, &settings, i as f32 / 60.0, i == 0, 0.22);
        gpu.queue.submit([encoder.finish()]);
    }
    for size in [[97, 73], [49, 37]] {
        renderer.resize(&gpu.device, size[0], size[1]);
        let camera = Camera::beach(config.length).uniform(config, size[0] as f32 / size[1] as f32);
        let pixels = image(gpu, &renderer, &ocean, &camera, settings, size)?;
        let foam_pixels = pixels
            .iter()
            .filter(|p| p[0] > 0.1 && (p[0] - p[1]).abs() < 1e-6 && (p[0] - p[2]).abs() < 1e-6)
            .count();
        if foam_pixels < 8 {
            return Err(format!(
                "shore render lost foam history at {size:?}: {foam_pixels} pixels"
            )
            .into());
        }
    }
    println!("GPU shore raster: world-space history reaches water shading at both target sizes OK");
    Ok(())
}

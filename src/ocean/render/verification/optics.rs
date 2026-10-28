//! End-to-end raster check: actual seabed depth, world-position reconstruction,
//! water integration, debug outputs and resized targets, without egui.
use crate::ocean::Error;
use crate::ocean::camera::Camera;
use crate::ocean::gpu::context::Gpu;
use crate::ocean::parameters::ocean::OceanConfig;
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::render::Renderer;
use crate::ocean::render::optics;
use crate::ocean::simulation::offshore::Ocean;
use crate::ocean::simulation::offshore::foam::Foam;
use glam::{Mat4, Vec4};

fn image(
    gpu: &Gpu,
    renderer: &Renderer,
    ocean: &Ocean,
    camera: &crate::ocean::camera::CameraUniform,
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
    ocean.encode_filtered(gpu, &mut encoder, 0.0, &optics.shore);
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
    swe_resolution(gpu)?;
    collider_raster(gpu)?;
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
                sky: crate::ocean::parameters::sky::SkyConfig::legacy(),
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
                sky: crate::ocean::parameters::sky::SkyConfig {
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
                        let sky = crate::ocean::render::verification::skylight::reference(
                            &settings, d, 0.0, down, c,
                        );
                        let local = settings.sigma_t[c] as f64
                            * settings.sky.albedo[c] as f64
                            * crate::ocean::render::skylight::incident_reference(
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
            shore: crate::ocean::parameters::shore::ShoreConfig {
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
        shore: crate::ocean::parameters::shore::ShoreConfig {
            view: 3,
            ..Default::default()
        },
        ..OpticsConfig::beach_scene()
    };
    renderer.update_optics(gpu, &settings);
    for i in 0..=1440 {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode_filtered(gpu, &mut encoder, i as f32 / 60.0, &settings.shore);
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
    let size = [385, 289];
    renderer.resize(&gpu.device, size[0], size[1]);
    let camera = Camera::beach(config.length).uniform(config, size[0] as f32 / size[1] as f32);
    // Exercise the actual vertex/fragment interface, with identical history,
    // camera and time: only the render-only foam height changes.
    let mut height_settings = settings;
    height_settings.shore.view = 6;
    height_settings.shore.foam_height_threshold = 0.0;
    height_settings.shore.foam_height_transition = 0.2;
    height_settings.shore.foam_height_max = 0.0;
    let flat = image(gpu, &renderer, &ocean, &camera, height_settings, size)?;
    height_settings.shore.foam_height_max = 0.3;
    let raised = image(gpu, &renderer, &ocean, &camera, height_settings, size)?;
    if raised.iter().flatten().any(|v| !v.is_finite()) {
        return Err("foam height raster produced nonfinite pixels".into());
    }
    let changed = flat
        .iter()
        .zip(&raised)
        .filter(|(a, b)| (0..3).any(|c| (a[c] - b[c]).abs() > 0.005))
        .count();
    if changed < 4 {
        return Err(format!("foam height did not change raster geometry: {changed} pixels").into());
    }
    println!(
        "GPU foam height raster: {changed} pixels change with fixed coastal history/camera/time"
    );
    // Exercise CPU configuration -> uniform -> real fragment wiring on the
    // same history/camera/time. Wider density response must reduce NSH opacity,
    // while neither the legacy material nor the raw density view may change.
    let mut opacity_settings = settings;
    opacity_settings.shore.view = 0;
    opacity_settings.shore.foam_material_view = 4;
    for (source, view) in [(0, 0), (1, 0), (2, 0), (3, 0), (1, 6)] {
        opacity_settings.shore.foam_texture = source;
        opacity_settings.shore.view = view;
        opacity_settings.shore.foam_opacity_scale = 2.0;
        let dense = image(gpu, &renderer, &ocean, &camera, opacity_settings, size)?;
        opacity_settings.shore.foam_opacity_scale = 8.0;
        let transparent = image(gpu, &renderer, &ocean, &camera, opacity_settings, size)?;
        if transparent.iter().flatten().any(|v| !v.is_finite()) {
            return Err("NSH opacity raster produced nonfinite pixels".into());
        }
        if source == 0 || view == 6 {
            if dense != transparent {
                return Err("NSH opacity scale changed legacy shading or density debug".into());
            }
            continue;
        }
        let mut decreased = 0;
        for (a, b) in dense.iter().zip(&transparent) {
            if (0..3).any(|c| b[c] > a[c] + 1e-6) {
                return Err("larger NSH density scale increased raster opacity".into());
            }
            if a[0] - b[0] > 0.005 {
                if (b[0] - b[1]).abs() > 1e-6 || (b[0] - b[2]).abs() > 1e-6 {
                    return Err("NSH opacity debug is not grayscale".into());
                }
                decreased += 1;
            }
        }
        if decreased < 8 {
            return Err(format!(
                "NSH source {source} opacity parameter did not reach raster: {decreased} pixels"
            )
            .into());
        }
    }
    println!(
        "GPU NSH opacity raster: all 3 textures respond continuously to density scale; legacy material and raw density view unchanged OK"
    );
    println!("GPU shore raster: world-space history reaches water shading at both target sizes OK");
    Ok(())
}

fn collider_raster(gpu: &Gpu) -> Result<(), Error> {
    use crate::ocean::scene::collider::{Collider, Shape};
    let size = [385, 289];
    let config = OceanConfig {
        length: 64.0,
        resolution: 8,
        mesh_resolution: 128,
        amplitude: 0.0,
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
    let camera = Camera::beach(config.length).uniform(config, size[0] as f32 / size[1] as f32);
    let mut settings = OpticsConfig {
        shore_x: 0.0,
        view: 6,
        ..OpticsConfig::beach_scene()
    };
    settings.shore.colliders[1] = Some(Collider {
        shape: Shape::Box,
        position: [12.0, 8.0],
        angle: -20.0,
        ..Default::default()
    });
    for shape in [Shape::Cylinder, Shape::Box] {
        for top in [2.92, -0.5] {
            settings.shore.colliders[0] = Some(Collider {
                shape,
                position: [6.0, 0.0],
                height: top + 1.08,
                angle: 35.0,
                ..Default::default()
            });
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            ocean.encode_filtered(gpu, &mut encoder, 0.0, &settings.shore);
            renderer
                .shore
                .encode(gpu, &mut encoder, &settings, 0.0, true, 0.0);
            gpu.queue.submit([encoder.finish()]);
            let pixels = image(gpu, &renderer, &ocean, &camera, settings, size)?;
            let clip =
                Mat4::from_cols_array_2d(&camera.view_projection) * Vec4::new(6.0, top, 0.0, 1.0);
            let x = ((clip.x / clip.w * 0.5 + 0.5) * size[0] as f32) as usize;
            let y = ((0.5 - clip.y / clip.w * 0.5) * size[1] as f32) as usize;
            let actual = pixels[y * size[0] as usize + x];
            let wet = if top < 0.0 { 0.65 } else { 1.0 };
            for (channel, albedo) in [0.38, 0.41, 0.44].into_iter().enumerate() {
                let expected = albedo * (0.3 + 0.7 * camera.sun[1]) * wet;
                if !actual[channel].is_finite() || (actual[channel] - expected).abs() > 0.003 {
                    return Err(format!("collider raster {shape:?} top {top}: {actual:?}, expected channel {channel}={expected}").into());
                }
            }
            // Optional review artifact, inside the caller's chosen output path.
            if shape == Shape::Cylinder
                && top > 0.0
                && let Some(path) = std::env::var_os("OCEAN_COLLIDER_PREVIEW")
            {
                let shaded = image(
                    gpu,
                    &renderer,
                    &ocean,
                    &camera,
                    OpticsConfig {
                        view: 0,
                        ..settings
                    },
                    size,
                )?;
                let pixels: Vec<u8> = shaded
                    .iter()
                    .flat_map(|p| {
                        p[..3].iter().map(|v| {
                            let x = v.clamp(0.0, 1.0);
                            let srgb = if x <= 0.0031308 {
                                12.92 * x
                            } else {
                                1.055 * x.powf(1.0 / 2.4) - 0.055
                            };
                            (srgb * 255.0).round() as u8
                        })
                    })
                    .collect();
                let mut encoder = png::Encoder::new(
                    std::io::BufWriter::new(std::fs::File::create(path)?),
                    size[0],
                    size[1],
                );
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(&pixels)?;
            }
        }
    }
    println!(
        "GPU collider raster: cylinder/rotated box above and below water, opaque top/depth and underwater scene-color sampling OK"
    );
    Ok(())
}

/// Exercise production allocation, dispatch, CFL stepping and all raster readers
/// while switching in both directions, including reselecting an unchanged size.
fn swe_resolution(gpu: &Gpu) -> Result<(), Error> {
    let config = OceanConfig {
        resolution: 8,
        mesh_resolution: 8,
        amplitude: 0.0,
        ..Default::default()
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let size = [33, 25];
    let format = wgpu::TextureFormat::Rgba32Float;
    let mut renderer = Renderer::new(gpu, &ocean, &foam, format, size[0], size[1]);
    let mut optics = OpticsConfig::beach_scene();
    let camera = Camera::beach(config.length).uniform(config, size[0] as f32 / size[1] as f32);
    for n in [64, 128, 256, 512, 1024, 128, 128] {
        optics.shore.resolution = n;
        renderer.set_shore_resolution(gpu, &ocean, n, format, size);
        if renderer.shore.field.size() != u64::from(n * n) * 80 {
            return Err("SWE resized storage has incorrect size".into());
        }
        for order in [1, 2] {
            optics.shore.order = order;
            for (time, reset) in [(0.0, true), (0.001, false)] {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                ocean.encode_filtered(gpu, &mut encoder, time, &optics.shore);
                renderer
                    .shore
                    .encode(gpu, &mut encoder, &optics, time, reset, 1.0);
                gpu.queue.submit([encoder.finish()]);
            }
            let p = renderer.shore.parameters(&optics, 0.001, false, 1.0);
            let bytes = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
            let fields: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
            if fields.iter().flatten().any(|v| !v.is_finite()) {
                return Err(format!("SWE {n} order {order}: invalid state").into());
            }
            for (i, cell) in fields[..(n * n) as usize].iter().enumerate() {
                let x = p.domain[0] + ((i % n as usize) as f32 + 0.5) * p.domain[2] / n as f32;
                let expected = (optics.beach_slope * (x - optics.shore_x)).max(0.0);
                if cell[2] < 0.0 || (cell[2] - expected).abs() > 2e-4 {
                    return Err(
                        format!("SWE {n} order {order}: lake-at-rest mismatch at {i}").into(),
                    );
                }
            }
            if image(gpu, &renderer, &ocean, &camera, optics, size)?
                .iter()
                .flatten()
                .any(|v| !v.is_finite())
            {
                return Err(format!("SWE {n}: invalid resized raster").into());
            }
        }
    }
    eprintln!(
        "GPU SWE resolution: 64/128/256/512/1024, resize back, both orders, lake at rest and raster bindings OK"
    );
    Ok(())
}

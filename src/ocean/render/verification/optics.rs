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
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    ocean.encode_filtered(gpu, &mut encoder, 0.0, &optics.shore);
    gpu.queue.submit([encoder.finish()]);
    raster_image(gpu, renderer, camera, optics, size)
}

pub(super) fn raster_image(
    gpu: &Gpu,
    renderer: &Renderer,
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
    // Pure-species limits must preserve normal shading for the visible species
    // and exactly match bare water for the hidden one, through real uniforms.
    let mut species_settings = settings;
    species_settings.shore.view = 0;
    species_settings.shore.foam_dual_phase = true;
    species_settings.shore.foam_material_view = 0;
    species_settings.shore.foam_height_max = 0.0;
    species_settings.shore.foam_opacity_scale = 1.0;
    let cells = (renderer.shore.resolution as usize).pow(2);
    let history = vec![[4.0_f32, 0.0, 1.0, 0.0]; cells];
    gpu.queue
        .write_buffer(&renderer.shore.field, 0, bytemuck::cast_slice(&history));
    let mut bare_camera = camera;
    bare_camera.misc[0] = 0.0;
    let bare = image(gpu, &renderer, &ocean, &bare_camera, species_settings, size)?;
    for mode in 0..=2 {
        species_settings.shore.foam_fresh_threshold_mode = mode;
        for (visible, hidden, residual) in [(1, 2, 0.0_f32), (2, 1, 4.0)] {
            let texture = vec![[0.0_f32, 0.0, 0.0, residual]; cells];
            gpu.queue.write_buffer(
                &renderer.shore.field,
                (4 * cells * 16) as u64,
                bytemuck::cast_slice(&texture),
            );
            species_settings.shore.foam_species_view = 0;
            let both = image(gpu, &renderer, &ocean, &camera, species_settings, size)?;
            species_settings.shore.foam_species_view = visible;
            let selected = image(gpu, &renderer, &ocean, &camera, species_settings, size)?;
            species_settings.shore.foam_species_view = hidden;
            let excluded = image(gpu, &renderer, &ocean, &camera, species_settings, size)?;
            if both != selected || excluded != bare {
                return Err(format!(
                    "foam species {visible} isolation changed visible shading or leaked hidden foam"
                )
                .into());
            }
            let changed = selected
                .iter()
                .zip(&bare)
                .filter(|(a, b)| (0..3).any(|c| (a[c] - b[c]).abs() > 0.005))
                .count();
            if changed < 8 {
                return Err(format!(
                    "foam species {visible} fixture produced no visible coverage: {changed}"
                )
                .into());
            }
        }
    }
    println!(
        "GPU foam species: all 3 N modes preserve selected shading and hide the other species OK"
    );
    // Fixed-coordinate rendering must ignore stored motion, while the enabled
    // path must respond to it. Exercise the real render bindings and shaders.
    species_settings.shore.foam_species_view = 0;
    species_settings.shore.foam_material_view = 5;
    let mut fixed = Vec::new();
    let mut moving = Vec::new();
    for offset in [0.0_f32, 0.13] {
        let texture = vec![[offset, offset, 0.0, 4.0]; cells];
        gpu.queue.write_buffer(
            &renderer.shore.field,
            (4 * cells * 16) as u64,
            bytemuck::cast_slice(&texture),
        );
        species_settings.shore.foam_texture_distortion = false;
        fixed.push(image(
            gpu,
            &renderer,
            &ocean,
            &camera,
            species_settings,
            size,
        )?);
        species_settings.shore.foam_texture_distortion = true;
        moving.push(image(
            gpu,
            &renderer,
            &ocean,
            &camera,
            species_settings,
            size,
        )?);
    }
    if fixed[0] != fixed[1] || moving[0] == moving[1] || fixed[0] != moving[0] {
        return Err("foam texture distortion toggle did not isolate stored motion".into());
    }
    println!(
        "GPU foam texture distortion: disabled ignores motion, enabled responds, zero offset agrees OK"
    );
    fresh_material_raster(gpu, &renderer, &ocean, &camera, species_settings, size)?;
    Ok(())
}

// Real configuration -> uniform -> raster checks. The legacy NSH path is the
// compatibility oracle for pure N; mixed N/R checks protect species isolation.
fn fresh_material_raster(
    gpu: &Gpu,
    renderer: &Renderer,
    ocean: &Ocean,
    camera: &crate::ocean::camera::CameraUniform,
    mut settings: OpticsConfig,
    size: [u32; 2],
) -> Result<(), Error> {
    settings.shore.foam_material_view = 0;
    settings.shore.foam_species_view = 1;
    settings.shore.foam_fresh_threshold_mode = 2;
    settings.shore.foam_texture = 3;
    settings.shore.foam_texture_size = 37.0;
    settings.shore.foam_micro_normal = 0.1;
    settings.shore.foam_roughness = 0.25;
    settings.shore.foam_density_reference = 7.0;
    let cells = (renderer.shore.resolution as usize).pow(2);
    let bytes = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
    let mut seed: Vec<[f32; 4]> = bytemuck::cast_slice(&bytes).to_vec();
    for i in 0..cells {
        seed[i][0] = 1.2;
        seed[3 * cells + i][2..].fill(0.0);
        seed[4 * cells + i] = [0.0; 4];
    }
    gpu.queue
        .write_buffer(&renderer.shore.field, 0, bytemuck::cast_slice(&seed));
    let differences = |a: &[[f32; 4]], b: &[[f32; 4]]| {
        a.iter()
            .zip(b)
            .filter(|(a, b)| (0..3).any(|c| (a[c] - b[c]).abs() > 2e-6))
            .count()
    };
    let mut previous = None;
    for source in 1..=3 {
        settings.shore.foam_fresh_texture = source;
        let textured = image(gpu, renderer, ocean, camera, settings, size)?;
        let mut legacy = settings;
        legacy.shore.foam_dual_phase = false;
        legacy.shore.foam_prototype = false;
        legacy.shore.foam_texture = source;
        legacy.shore.foam_texture_size = settings.shore.foam_fresh_texture_size;
        legacy.shore.foam_micro_normal = settings.shore.foam_fresh_micro_normal;
        legacy.shore.foam_roughness = settings.shore.foam_fresh_roughness;
        legacy.shore.foam_density_reference = settings.shore.foam_fresh_coverage_reference;
        let expected = image(gpu, renderer, ocean, camera, legacy, size)?;
        if textured.iter().flatten().any(|v| !v.is_finite())
            || differences(&textured, &expected) != 0
        {
            return Err(
                format!("N texture {source} did not preserve legacy NSH appearance").into(),
            );
        }
        if previous
            .as_ref()
            .is_some_and(|before: &Vec<[f32; 4]>| differences(before, &textured) < 8)
        {
            return Err("N texture selection did not reach the raster".into());
        }
        previous = Some(textured);
    }
    settings.shore.foam_fresh_texture = 1;
    let baseline = image(gpu, renderer, ocean, camera, settings, size)?;
    for parameter in 0..4 {
        let mut changed = settings;
        match parameter {
            0 => changed.shore.foam_fresh_texture_size = 3.5,
            1 => changed.shore.foam_fresh_micro_normal = 3.0,
            2 => changed.shore.foam_fresh_roughness = 0.15,
            _ => changed.shore.foam_fresh_coverage_reference = 5.0,
        }
        let actual = image(gpu, renderer, ocean, camera, changed, size)?;
        if actual.iter().flatten().any(|v| !v.is_finite()) || differences(&baseline, &actual) < 8 {
            return Err(format!("N NSH parameter {parameter} did not reach the raster").into());
        }
    }
    // Nonzero N and R: N material controls cannot affect the isolated residual,
    // including its own lighting/normal and opacity diagnostics.
    for i in 0..cells {
        seed[i][0] = 2.4;
        seed[4 * cells + i][3] = 1.2;
    }
    gpu.queue
        .write_buffer(&renderer.shore.field, 0, bytemuck::cast_slice(&seed));
    for debug in [0, 3, 4] {
        settings.shore.foam_species_view = 2;
        settings.shore.foam_material_view = debug;
        settings.shore.foam_fresh_threshold_mode = 1;
        let residual = image(gpu, renderer, ocean, camera, settings, size)?;
        for mode in 0..=2 {
            let mut changed = settings;
            changed.shore.foam_fresh_threshold_mode = mode;
            changed.shore.foam_fresh_texture = 3;
            changed.shore.foam_fresh_texture_size = 3.5;
            changed.shore.foam_fresh_micro_normal = 3.0;
            changed.shore.foam_fresh_roughness = 0.15;
            changed.shore.foam_fresh_coverage_reference = 5.0;
            changed.shore.foam_fresh_density_opacity = true;
            changed.shore.foam_fresh_opacity_range = 4.0;
            changed.shore.foam_fresh_opacity_power = 2.0;
            if image(gpu, renderer, ocean, camera, changed, size)? != residual {
                return Err(format!("N mode {mode} changed isolated R, debug {debug}").into());
            }
        }
    }
    settings.shore.foam_material_view = 0;
    settings.shore.foam_species_view = 0;
    for view in [6, 9, 10] {
        settings.shore.view = view;
        settings.shore.foam_fresh_threshold_mode = 0;
        settings.shore.foam_fresh_coverage_reference = 2.0;
        settings.shore.foam_fresh_density_opacity = false;
        settings.shore.foam_residual_density_opacity = false;
        let density = image(gpu, renderer, ocean, camera, settings, size)?;
        settings.shore.foam_fresh_threshold_mode = 2;
        settings.shore.foam_fresh_coverage_reference = 5.0;
        settings.shore.foam_fresh_density_opacity = true;
        settings.shore.foam_fresh_opacity_range = 4.0;
        settings.shore.foam_fresh_opacity_power = 2.0;
        settings.shore.foam_residual_density_opacity = true;
        settings.shore.foam_residual_opacity_range = 4.0;
        settings.shore.foam_residual_opacity_power = 2.0;
        if image(gpu, renderer, ocean, camera, settings, size)? != density {
            return Err(format!("N NSH appearance changed density view {view}").into());
        }
    }
    concentration_opacity_raster(gpu, renderer, ocean, camera, settings, size)?;
    println!(
        "GPU N NSH: 3 textures match legacy, independent size/normal/roughness/coverage controls, mixed-species R lighting/normal/opacity and raw N/R/total density isolation OK"
    );
    Ok(())
}

fn concentration_opacity_raster(
    gpu: &Gpu,
    renderer: &Renderer,
    ocean: &Ocean,
    camera: &crate::ocean::camera::CameraUniform,
    mut settings: OpticsConfig,
    size: [u32; 2],
) -> Result<(), Error> {
    // The caller seeded N=R=1.2. Restrict the analytic comparison to the
    // uniform interior identified by raw R density, not patch-edge interpolation.
    settings.shore.view = 10;
    let density = image(gpu, renderer, ocean, camera, settings, size)?;
    let level = 1.2 / settings.shore.foam_density_reference;
    let density_color = [level, 1.0 - level, 0.0];
    settings.shore.view = 0;
    settings.shore.foam_fresh_coverage_reference = 2.0;
    settings.shore.foam_fresh_opacity_range = 4.0;
    settings.shore.foam_fresh_opacity_power = 2.0;
    settings.shore.foam_residual_opacity_range = 4.0;
    settings.shore.foam_residual_opacity_power = 2.0;
    let mut bare_camera = *camera;
    bare_camera.misc[0] = 0.0;
    let bare = image(gpu, renderer, ocean, &bare_camera, settings, size)?;
    for mode in 0..=2 {
        settings.shore.foam_fresh_threshold_mode = mode;
        for species in [1, 2] {
            settings.shore.foam_species_view = species;
            settings.shore.foam_fresh_density_opacity = false;
            settings.shore.foam_residual_density_opacity = false;
            let baseline = image(gpu, renderer, ocean, camera, settings, size)?;
            settings.shore.foam_fresh_density_opacity = species == 1;
            settings.shore.foam_residual_density_opacity = species == 2;
            let applied = image(gpu, renderer, ocean, camera, settings, size)?;
            if applied.iter().flatten().any(|v| !v.is_finite()) {
                return Err("concentration opacity produced nonfinite raster".into());
            }
            let mut changed = 0;
            for (i, pixel) in applied.iter().enumerate() {
                if !(0..3).all(|c| (density[i][c] - density_color[c]).abs() < 1e-6) {
                    continue;
                }
                for c in 0..3 {
                    // pow(1.2 / 4, 2) = 0.09. Scaling color AND alpha moves
                    // the result toward bare water; RGB-only darkening fails.
                    let expected = bare[i][c] + 0.09 * (baseline[i][c] - bare[i][c]);
                    if (pixel[c] - expected).abs() > 2e-5 {
                        return Err(format!("concentration opacity did not scale premultiplied color/alpha: mode {mode}, species {species}").into());
                    }
                }
                if (0..3).any(|c| (pixel[c] - baseline[i][c]).abs() > 0.001) {
                    changed += 1;
                }
            }
            if changed < 8 {
                return Err(format!("concentration opacity fixture lacked visible foam: mode {mode}, species {species}").into());
            }
        }
    }
    println!(
        "GPU concentration opacity raster: N/R independent factors, all 3 N modes, real CPU-to-GPU range/power wiring, premultiplied color and alpha blend toward bare water OK"
    );
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

/// Analytic flat/tilted receiver irradiance and Snell path, then actual animated FFT.
pub fn water_lighting(gpu: &Gpu) -> Result<(), Error> {
    super::surface::run(gpu)?;
    super::mesh::run(gpu)?;
    crate::ocean::render::caustics::verify(gpu)?;
    model_water_lighting(gpu)?;
    fast_refraction(gpu)?;
    use glam::Vec3;
    let config = OceanConfig {
        length: 64.0,
        resolution: 32,
        mesh_resolution: 64,
        amplitude: 0.0,
        ..Default::default()
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new(gpu, &ocean, &foam, wgpu::TextureFormat::Rgba32Float, 65, 49);
    let mut camera = Camera::new(config.length);
    camera.distance = 12.0;
    camera.pitch = 0.9;
    camera.foam = false;
    let mut settings = OpticsConfig {
        caustics: true,
        caustics_extent: 64.0,
        refraction: true,
        sigma_t: [0.0; 3],
        source: [0.0; 3],
        reflections: false,
        ..Default::default()
    };
    settings.shore.enabled = false;
    settings.sky.enabled = false;
    for size in [[65, 49], [47, 35]] {
        renderer.resize(&gpu.device, size[0], size[1]);
        for beach in [false, true] {
            settings.beach = beach;
            let slope = if beach { settings.beach_slope } else { 0.0 };
            let bed = if beach {
                slope * settings.shore_x
            } else {
                -settings.depth
            };
            let bed_normal = Vec3::new(slope, 1.0, 0.0).normalize();
            for sun in [Vec3::Y, Vec3::new(-0.35, 0.4, -0.65).normalize()] {
                let mut uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
                uniform.sun = sun.extend(0.0).to_array();
                let eta = 1.0 / settings.water_ior;
                let light = Vec3::new(
                    -sun.x * eta,
                    -(1.0 - eta * eta * (1.0 - sun.y * sun.y)).sqrt(),
                    -sun.z * eta,
                );
                let f0 = ((settings.water_ior - 1.0) / (settings.water_ior + 1.0)).powi(2);
                let expected = sun.y / (-light.y)
                    * (-light.dot(bed_normal))
                    * (1.0 - f0 - (1.0 - f0) * (1.0 - sun.y).powi(5));
                for resolution in [128, 512] {
                    settings.caustics_resolution = resolution;
                    settings.view = 12;
                    let pixels = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
                    for [x, y] in [[size[0] / 2, size[1] / 2], [size[0] / 3, size[1] / 2]] {
                        let value = pixels[(y * size[0] + x) as usize][0];
                        if !value.is_finite() || (value - expected).abs() > 0.015 {
                            return Err(format!("caustic planar flux: beach={beach}, grid={resolution}, value={value}, expected={expected}").into());
                        }
                    }
                }
                settings.view = 14;
                let hit = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
                settings.view = 1;
                let paths = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
                let inverse = Mat4::from_cols_array_2d(&uniform.inverse_view_projection);
                let eye = Vec4::from_array(uniform.eye).truncate();
                let mut checked = 0;
                for [x, y] in [
                    [size[0] / 2, size[1] / 2],
                    [size[0] / 3, size[1] / 2],
                    [size[0] * 2 / 3, size[1] / 2],
                ] {
                    let pixel = (y * size[0] + x) as usize;
                    if hit[pixel][1] < 0.7 {
                        continue;
                    }
                    let far = inverse
                        * Vec4::new(
                            (x as f32 + 0.5) / size[0] as f32 * 2.0 - 1.0,
                            1.0 - (y as f32 + 0.5) / size[1] as f32 * 2.0,
                            1.0,
                            1.0,
                        );
                    let incoming = (far.truncate() / far.w - eye).normalize();
                    let surface = eye + incoming * (-eye.y / incoming.y);
                    let ray = Vec3::new(
                        incoming.x * eta,
                        -(1.0 - eta * eta * (1.0 - incoming.y * incoming.y)).sqrt(),
                        incoming.z * eta,
                    );
                    let expected = (bed - slope * surface.x) / (ray.y + slope * ray.x);
                    let actual = paths[pixel][0] * settings.path_range;
                    if (actual - expected).abs() > 0.015 {
                        return Err(format!(
                            "Snell plane path: {actual} != {expected}, beach={beach}"
                        )
                        .into());
                    }
                    checked += 1;
                }
                if checked < 2 {
                    return Err(format!("refraction rejected visible planar hits: beach={beach}, size={size:?}, checked={checked}").into());
                }
                settings.refraction_distance = 1.0;
                settings.view = 14;
                let fallback = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
                if fallback[(size[1] / 2 * size[0] + size[0] / 2) as usize][0] < 0.7 {
                    return Err("refraction maximum path did not fall back".into());
                }
                settings.refraction_distance = 100.0;
            }
        }
    }
    settings.beach = false;
    settings.view = 1;
    settings.water_ior = 1.0;
    let uniform = camera.uniform(config, 65.0 / 49.0);
    renderer.resize(&gpu.device, 65, 49);
    let straight = image(
        gpu,
        &renderer,
        &ocean,
        &uniform,
        OpticsConfig {
            refraction: false,
            ..settings
        },
        [65, 49],
    )?;
    let index_one = image(gpu, &renderer, &ocean, &uniform, settings, [65, 49])?;
    for pixel in [24 * 65 + 32, 24 * 65 + 22] {
        if (straight[pixel][0] - index_one[pixel][0]).abs() > 0.0001 {
            return Err("index-one refraction changed the straight water path".into());
        }
    }
    settings.water_ior = 1.333;
    settings.view = 12;
    let mut night = uniform;
    night.sun = [0.0, -1.0, 0.0, 0.0];
    let dark = image(gpu, &renderer, &ocean, &night, settings, [65, 49])?;
    if dark[24 * 65 + 32][0] > 0.0001 {
        return Err("sun below horizon still casts caustics".into());
    }
    // Production FFT animation must move the focal pattern; repeated paused state
    // must remain bit-identical. No independent caustic animation clock is allowed.
    let config = OceanConfig {
        amplitude: 0.002,
        resolution: 64,
        mesh_resolution: 64,
        ..config
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new(gpu, &ocean, &foam, wgpu::TextureFormat::Rgba32Float, 65, 49);
    let uniform = camera.uniform(config, 65.0 / 49.0);
    settings.beach = false;
    settings.view = 12;
    settings.caustics_resolution = 256;
    settings.refraction = false;
    let mut images = Vec::new();
    for time in [0.0, 1.0, 1.0] {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode_filtered(gpu, &mut encoder, time, &settings.shore);
        gpu.queue.submit([encoder.finish()]);
        images.push(raster_image(gpu, &renderer, &uniform, settings, [65, 49])?);
    }
    if images.iter().flatten().flatten().any(|v| !v.is_finite()) {
        return Err("nonfinite caustic raster".into());
    }
    let change: f32 = images[0]
        .iter()
        .zip(&images[1])
        .map(|(a, b)| (a[0] - b[0]).abs())
        .sum::<f32>()
        / images[0].len() as f32;
    if change < 0.002 || images[1] != images[2] {
        return Err(format!("caustic animation/pause failed: mean change={change}").into());
    }
    settings.beach = true;
    settings.shore.enabled = true;
    settings.shore.normal_strength = 1.0;
    camera.target = [settings.shore_x + 3.0, 0.0];
    camera.distance = 24.0;
    camera.pitch = 1.15;
    let uniform = camera.uniform(config, 65.0 / 49.0);
    for (resolution, order) in [(64, 1), (128, 2)] {
        settings.shore.resolution = resolution;
        settings.shore.order = order;
        renderer.set_shore_resolution(
            gpu,
            &ocean,
            resolution,
            wgpu::TextureFormat::Rgba32Float,
            [65, 49],
        );
        renderer.prepare_scene(gpu, &settings, wgpu::TextureFormat::Rgba32Float)?;
        for step in 0..12 {
            let time = step as f32 / 60.0;
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            ocean.encode_filtered(gpu, &mut encoder, time, &settings.shore);
            renderer
                .shore
                .encode(gpu, &mut encoder, &settings, time, false, uniform.ocean[3]);
            gpu.queue.submit([encoder.finish()]);
        }
        let before = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
        let pixels = raster_image(gpu, &renderer, &uniform, settings, [65, 49])?;
        let after = gpu.read_buffer(&renderer.shore.field, renderer.shore.field.size())?;
        if before != after || pixels.iter().flatten().any(|v| !v.is_finite()) {
            return Err("caustics mutated SWE or produced nonfinite raster after rebind".into());
        }
        if !pixels.iter().any(|p| p[0] > 0.1 && p[0] < 5.0) {
            return Err("SWE caustic receiver is empty".into());
        }
        let inverse = Mat4::from_cols_array_2d(&uniform.inverse_view_projection);
        let eye = Vec4::from_array(uniform.eye).truncate();
        let mut dry_count = 0;
        for y in 5..44 {
            for x in 5..60 {
                let far = inverse
                    * Vec4::new(
                        (x as f32 + 0.5) / 65.0 * 2.0 - 1.0,
                        1.0 - (y as f32 + 0.5) / 49.0 * 2.0,
                        1.0,
                        1.0,
                    );
                let ray = (far.truncate() / far.w - eye).normalize();
                let t = (settings.beach_slope * (settings.shore_x - eye.x) - eye.y)
                    / (ray.y + settings.beach_slope * ray.x);
                let hit = eye + ray * t;
                if t > 0.0 && hit.x < settings.shore_x - 1.5 {
                    dry_count += 1;
                    if pixels[y * 65 + x][0] > 0.0001 {
                        return Err("caustics illuminated dry beach".into());
                    }
                }
            }
        }
        if dry_count < 10 {
            return Err("SWE caustic check did not cover dry beach".into());
        }
    }
    println!(
        "GPU water lighting: flat/slope flux at 128/512, two sun directions, Snell paths, fallback, resize, FFT animation/pause, SWE 64/128 both orders and dry beach OK (change={change:.4})"
    );
    Ok(())
}

/// Actual GLB depth/height receivers, including resize and static terrain edits.
fn model_water_lighting(gpu: &Gpu) -> Result<(), Error> {
    let config = OceanConfig {
        length: 128.0,
        resolution: 64,
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
        129,
        97,
    );
    let mut settings = OpticsConfig {
        caustics: true,
        caustics_extent: 128.0,
        refraction: true,
        sigma_t: [0.0; 3],
        source: [0.0; 3],
        reflections: false,
        ..Default::default()
    };
    settings.model.enabled = true;
    settings.model.scale = 2.0;
    settings.model.show_bounds = false;
    settings.shore.enabled = false;
    settings.sky.enabled = false;
    let mut camera = Camera::model(config.length);
    camera.yaw = 1.402;
    camera.pitch = 0.578;
    camera.distance = 75.244;
    camera.foam = false;
    // Screen-space tracing must cover a continuous visible planar receiver:
    // isolated exact pixel-depth roots leave alternating hit/fallback stripes.
    let size = [257, 193];
    renderer.resize(&gpu.device, size[0], size[1]);
    let model_position = settings.model.position;
    settings.model.position = [1000.0, 1000.0, 1000.0];
    renderer.prepare_scene(gpu, &settings, wgpu::TextureFormat::Rgba32Float)?;
    settings.caustics = false;
    settings.view = 14;
    let uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
    let plane = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
    let mut covered = 0;
    let mut samples = 0;
    for y in size[1] / 2..size[1] * 4 / 5 {
        for x in size[0] / 4..size[0] * 3 / 4 {
            let p = plane[(y * size[0] + x) as usize];
            samples += 1;
            covered += usize::from(p[1] > 0.7 && p[0] < 0.03 && p[2] < 0.11);
        }
    }
    if covered * 100 < samples * 99 {
        return Err(format!("screen planar coverage has holes: {covered}/{samples}").into());
    }
    eprintln!("GPU screen planar coverage: {covered}/{samples}");
    settings.refraction = false;
    settings.reflections = true;
    settings.view = 0;
    let sky_only = raster_image(gpu, &renderer, &uniform, settings, size)?;
    settings.screen_space_reflections = true;
    let open_water = raster_image(gpu, &renderer, &uniform, settings, size)?;
    if open_water != sky_only {
        return Err("screen reflections changed sky fallback over an empty surface".into());
    }
    settings.model.position = model_position;
    renderer.prepare_scene(gpu, &settings, wgpu::TextureFormat::Rgba32Float)?;
    let reflected = raster_image(gpu, &renderer, &uniform, settings, size)?;
    settings.screen_space_reflections = false;
    let sky_only = raster_image(gpu, &renderer, &uniform, settings, size)?;
    let reflected_pixels = reflected
        .iter()
        .zip(&sky_only)
        .filter(|(a, b)| {
            a[..3]
                .iter()
                .zip(&b[..3])
                .any(|(x, y)| (x - y).abs() > 0.005)
        })
        .count();
    if reflected_pixels < 10 {
        return Err(format!(
            "screen reflections missed the exposed model: {reflected_pixels} pixels"
        )
        .into());
    }
    settings.screen_space_reflections = true;
    settings.view = 15;
    let diagnostic = raster_image(gpu, &renderer, &uniform, settings, size)?;
    settings.screen_space_reflections = false;
    let disabled = raster_image(gpu, &renderer, &uniform, settings, size)?;
    for ((full, sky), (shown, off)) in reflected
        .iter()
        .zip(&sky_only)
        .zip(diagnostic.iter().zip(&disabled))
    {
        if full[..3]
            .iter()
            .zip(&sky[..3])
            .any(|(a, b)| (a - b).abs() > 0.005)
            && (shown[..3].iter().any(|v| !v.is_finite())
                || shown[..3].iter().all(|v| *v <= 0.0)
                || off[..3].iter().any(|v| v.abs() > 0.00001))
        {
            return Err("SSR-only view lost reflected radiance or black fallback".into());
        }
    }
    settings.view = 0;
    settings.screen_space_reflections = true;
    for strength in [0.0, 0.5] {
        settings.reflection_strength = strength;
        let actual = raster_image(gpu, &renderer, &uniform, settings, size)?;
        for ((pixel, sky), full) in actual.iter().zip(&sky_only).zip(&reflected) {
            for channel in 0..3 {
                let expected = sky[channel] + strength * (full[channel] - sky[channel]);
                if !pixel[channel].is_finite() || (pixel[channel] - expected).abs() > 0.0001 {
                    return Err("SSR strength did not blend against the sky fallback".into());
                }
            }
        }
    }
    settings.reflection_strength = 1.0;
    settings.reflection_steps = 16;
    let limited = raster_image(gpu, &renderer, &uniform, settings, size)?;
    let limited_pixels = limited
        .iter()
        .zip(&sky_only)
        .filter(|(a, b)| {
            a[..3]
                .iter()
                .zip(&b[..3])
                .any(|(x, y)| (x - y).abs() > 0.005)
        })
        .count();
    if limited_pixels >= reflected_pixels {
        return Err("SSR traversal budget did not limit model reflections".into());
    }
    settings.reflection_steps = 128;
    settings.screen_space_reflections = false;
    eprintln!(
        "GPU screen reflections: {reflected_pixels} model reflection pixels, empty sky fallback, strength and traversal budget OK"
    );
    settings.refraction = true;
    settings.reflections = false;
    settings.caustics = true;
    use crate::ocean::parameters::optics::OpticalMethod;
    for (size, level, method) in [
        ([129, 97], 0.5, OpticalMethod::Physical),
        ([93, 67], 1.5, OpticalMethod::Physical),
        ([129, 97], 0.5, OpticalMethod::Fast),
    ] {
        settings.refraction_method = method;
        settings.model.sea_level = level;
        renderer.prepare_scene(gpu, &settings, wgpu::TextureFormat::Rgba32Float)?;
        renderer.resize(&gpu.device, size[0], size[1]);
        let uniform = camera.uniform(config, size[0] as f32 / size[1] as f32);
        settings.view = 14;
        settings.water_ior = 1.333;
        let hits = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        let count = hits
            .iter()
            .filter(|p| p[1] > 0.7 && p[0] < 0.03 && p[2] < 0.11)
            .count();
        if count < hits.len() / 8 {
            return Err(format!(
                "model screen refraction too few hits: {count}/{}",
                hits.len()
            )
            .into());
        }
        settings.refraction_distance = 1.0;
        let short = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        let short_count = short
            .iter()
            .filter(|p| p[1] > 0.7 && p[0] < 0.03 && p[2] < 0.11)
            .count();
        if short_count >= count / 2 {
            return Err("model refraction ignored maximum path".into());
        }
        settings.refraction_distance = 100.0;
        // IOR=1 must recover the same visible scene and Euclidean water path.
        settings.water_ior = 1.0;
        settings.view = 14;
        let direct_hits = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        settings.view = 1;
        let refracted = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        settings.refraction = false;
        let straight = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        let mut direct_count = 0;
        for ((a, b), hit) in refracted.iter().zip(&straight).zip(&direct_hits) {
            if hit[1] > 0.7 && hit[0] < 0.03 && hit[2] < 0.11 {
                direct_count += 1;
                if (a[0] - b[0]).abs() * settings.path_range > 0.08 {
                    return Err(format!(
                        "model index-one path differs: traced={} straight={} range={} hit={hit:?}",
                        a[0], b[0], settings.path_range
                    )
                    .into());
                }
            }
        }
        if direct_count < count / 2 {
            return Err("model screen trace failed straight projected rays".into());
        }
        settings.view = 12;
        settings.water_ior = 1.333;
        let light = image(gpu, &renderer, &ocean, &uniform, settings, size)?;
        if light.iter().flatten().any(|v| !v.is_finite())
            || light.iter().filter(|p| p[0] > 0.15).count() < light.len() / 8
        {
            return Err("model caustic receiver empty or nonfinite".into());
        }
        let paused = raster_image(gpu, &renderer, &uniform, settings, size)?;
        if light != paused {
            return Err("model static hierarchy changed while paused".into());
        }
        settings.refraction = true;
        eprintln!(
            "GPU model optics {method:?}: size={size:?}, sea level={level}, refracted hits={count}, index-one checked={direct_count}, short-path hits={short_count}"
        );
    }
    Ok(())
}

/// Fast art follows simulation time, pauses exactly, and never changes fluid state.
fn fast_refraction(gpu: &Gpu) -> Result<(), Error> {
    use crate::ocean::parameters::optics::OpticalMethod;
    let config = OceanConfig {
        resolution: 32,
        mesh_resolution: 64,
        length: 64.0,
        amplitude: 0.0,
        ..Default::default()
    };
    let ocean = Ocean::new(gpu, config)?;
    let foam = Foam::new(gpu, &ocean.fields, config.resolution);
    let mut renderer = Renderer::new(gpu, &ocean, &foam, wgpu::TextureFormat::Rgba32Float, 65, 49);
    let mut camera = Camera::new(config.length);
    camera.distance = 12.0;
    camera.pitch = 0.9;
    camera.foam = false;
    let uniform = camera.uniform(config, 65.0 / 49.0);
    let mut settings = OpticsConfig {
        refraction: true,
        refraction_method: OpticalMethod::Fast,
        sigma_t: [0.0; 3],
        source: [0.0; 3],
        reflections: false,
        view: 6,
        ..Default::default()
    };
    settings.shore.enabled = false;
    settings.sky.enabled = false;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    ocean.encode_filtered(gpu, &mut encoder, 0.0, &settings.shore);
    renderer
        .shore
        .encode(gpu, &mut encoder, &settings, 0.0, false, uniform.ocean[3]);
    gpu.queue.submit([encoder.finish()]);
    settings.view = 6;
    settings.refraction = false;
    let straight = raster_image(gpu, &renderer, &uniform, settings, [65, 49])?;
    settings.refraction = true;
    settings.fast_refraction_strength = 0.0;
    let zero = raster_image(gpu, &renderer, &uniform, settings, [65, 49])?;
    for i in [24 * 65 + 32, 24 * 65 + 22] {
        if (straight[i][0] - zero[i][0]).abs() > 0.002 {
            return Err("zero-strength fast refraction changed background".into());
        }
    }
    // Each selected path remains valid after repeatedly changing modes.
    settings.fast_refraction_strength = 1.0;
    for method in [
        OpticalMethod::Physical,
        OpticalMethod::Fast,
        OpticalMethod::Physical,
    ] {
        settings.refraction_method = method;
        settings.view = 14;
        let hit = raster_image(gpu, &renderer, &uniform, settings, [65, 49])?;
        if hit[24 * 65 + 32][1] < 0.7 {
            return Err("water effect method switch lost a visible receiver".into());
        }
    }
    eprintln!("GPU fast refraction: zero offset and mode switches OK");
    Ok(())
}

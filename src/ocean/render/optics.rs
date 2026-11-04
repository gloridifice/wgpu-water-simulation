//! Homogeneous water: manual constant source plus depth-dependent sky scattering.
use crate::ocean::{Error, gpu::context::Gpu};
use wgpu::util::DeviceExt;

pub use crate::ocean::parameters::optics::OpticsConfig;
pub use crate::ocean::parameters::optics::VIEW_NAMES;

impl OpticsConfig {
    pub fn uniform(&self) -> OpticsUniform {
        let (sin, cos) = self.sand_rotation_deg.to_radians().sin_cos();
        OpticsUniform {
            reflection: [
                f32::from(
                    self.reflections
                        && self.screen_space_reflections
                        && self.reflection_strength > 0.0,
                ),
                self.reflection_distance,
                self.reflection_strength,
                self.reflection_edge_fade,
            ],
            reflection_trace: [
                self.reflection_steps as f32,
                self.reflection_tolerance,
                0.0,
                0.0,
            ],
            caustics: [
                f32::from(self.caustics),
                self.caustics_strength,
                self.caustics_extent,
                self.caustics_softness,
            ],
            refraction: [
                self.refraction_method.code(self.refraction),
                self.water_ior,
                self.refraction_distance,
                self.caustics_resolution as f32,
            ],
            fast_refraction: [
                self.fast_refraction_strength,
                self.fast_refraction_depth,
                self.fast_refraction_limit,
                0.0,
            ],
            sand_mapping: [cos, sin, 0.0, 0.0],
            sigma_depth: [
                self.sigma_t[0],
                self.sigma_t[1],
                self.sigma_t[2],
                self.depth,
            ],
            source_view: [
                self.source[0],
                self.source[1],
                self.source[2],
                self.view as f32,
            ],
            beach: [
                f32::from(self.beach),
                self.beach_slope,
                self.shore_x,
                self.sand_tile_size,
            ],
            sky_color: self.sky.tint(),
            sky_albedo_enabled: [
                self.sky.albedo[0],
                self.sky.albedo[1],
                self.sky.albedo[2],
                f32::from(self.sky.enabled),
            ],
            sky_quality: [self.sky.steps as f32, 0.0, 0.0, 0.0],
            options: [
                self.path_range,
                f32::from(self.reflections),
                f32::from(self.color_bands),
                self.checker_size,
            ],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct OpticsUniform {
    pub sigma_depth: [f32; 4],
    pub source_view: [f32; 4],
    pub options: [f32; 4],
    pub sky_color: [f32; 4],
    pub sky_albedo_enabled: [f32; 4],
    pub sky_quality: [f32; 4],
    pub beach: [f32; 4],
    pub sand_mapping: [f32; 4],
    pub caustics: [f32; 4],
    pub refraction: [f32; 4],
    pub fast_refraction: [f32; 4],
    pub reflection: [f32; 4],
    pub reflection_trace: [f32; 4],
}

/// f64 reference with exp_m1 for accuracy, independent of the WGSL series.
pub fn integrate(bg: f64, sigma: f64, source: f64, d: f64) -> [f64; 3] {
    let t = (-sigma * d).exp();
    let weight = if sigma == 0.0 {
        d
    } else {
        -(-sigma * d).exp_m1() / sigma
    };
    [t, bg * t, source * weight]
}

/// Run the exact production shader function on zero, tiny and large optical
/// thicknesses. Compare every RGB component against an independent f64 result.
pub fn verify(gpu: &Gpu) -> Result<(), Error> {
    let mut cases: Vec<[[f32; 4]; 3]> = Vec::new();
    for d in [0.0, 0.00001, 1.0, 20.0, 1000.0] {
        for sigma in [0.0, 1e-9, 0.00005, 0.000999, 0.001001, 0.02, 0.5, 8.0] {
            cases.push([
                [0.2, 0.6, 0.9, d],
                [sigma, sigma * 0.5, sigma * 2.0, 0.0],
                [0.003, 0.008, 0.005, 0.0],
            ]);
        }
    }
    let source = format!(
        "{}\n{}",
        include_str!("shaders/optics.wgsl"),
        r#"
        struct Case { bg_d: vec4<f32>, sigma: vec4<f32>, source: vec4<f32> }
        @group(0) @binding(0) var<storage, read> cases: array<Case>;
        @group(0) @binding(1) var<storage, read_write> results: array<vec4<f32>>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) id: vec3<u32>) {
            if id.x >= arrayLength(&cases) { return; }
            let c = cases[id.x];
            let result = integrate_medium(c.bg_d.xyz, c.sigma.xyz, c.source.xyz, c.bg_d.w);
            results[id.x * 3u] = vec4(result.transmittance, 0.0);
            results[id.x * 3u + 1u] = vec4(result.background, 0.0);
            results[id.x * 3u + 2u] = vec4(result.scattering, 0.0);
        }
    "#
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("optical integration verification"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("optics check"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let input = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("optics cases"),
            contents: bytemuck::cast_slice(&cases),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let output = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("optics results"),
        size: cases.len() as u64 * 3 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("optics check bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: input.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((cases.len() as u32).div_ceil(64), 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.read_buffer(&output, output.size())?;
    let actual: Vec<[f32; 4]> = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|b| bytemuck::pod_read_unaligned(b))
        .collect();
    let mut max_error = 0.0_f64;
    for (i, case) in cases.iter().enumerate() {
        for channel in 0..3 {
            let expected = integrate(
                case[0][channel] as f64,
                case[1][channel] as f64,
                case[2][channel] as f64,
                case[0][3] as f64,
            );
            for component in 0..3 {
                let value = actual[i * 3 + component][channel] as f64;
                let error = (value - expected[component]).abs();
                if !value.is_finite() || error > 3e-5 * (1.0 + expected[component].abs()) {
                    return Err(format!("optics check failed: case {i}, channel {channel}, actual {value}, expected {}", expected[component]).into());
                }
                max_error = max_error.max(error);
            }
        }
    }
    println!(
        "GPU optics: {} cases, all RGB T/background/scattering, max error={max_error:.3e} OK",
        cases.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_distance_zero_extinction_and_pure_absorption_limits() {
        assert_eq!(integrate(0.7, 0.2, 0.3, 0.0), [1.0, 0.7, 0.0]);
        assert_eq!(integrate(0.7, 0.0, 0.3, 2.0), [1.0, 0.7, 0.6]);
        let value = integrate(0.7, 0.2, 0.0, 5.0);
        assert!((value[0] - (-1.0_f64).exp()).abs() < 1e-15);
        assert_eq!(value[2], 0.0);
    }
    #[test]
    fn extinction_and_source_are_independent_and_rgb_can_differ() {
        let a = integrate(1.0, 0.3, 0.02, 4.0);
        let b = integrate(1.0, 0.3, 0.04, 4.0);
        assert_eq!(a[..2], b[..2]);
        assert_eq!(a[2] * 2.0, b[2]);
        assert!(integrate(1.0, 0.3, 0.0, 4.0)[0] < integrate(1.0, 0.03, 0.0, 4.0)[0]);
    }
    #[test]
    fn analytic_integral_agrees_with_numerical_quadrature() {
        for sigma in [0.0, 1e-9, 0.1, 2.0] {
            let steps = 10000;
            let d = 7.0;
            let dt = d / steps as f64;
            let sum: f64 = (0..steps)
                .map(|i| 0.03 * (-sigma * (i as f64 + 0.5) * dt).exp() * dt)
                .sum();
            assert!((sum - integrate(0.6, sigma, 0.03, d)[2]).abs() < 1e-8);
        }
    }
    #[test]
    fn sand_mapping_preserves_legacy_config_and_validates_saved_values() {
        let legacy: OpticsConfig =
            serde_json::from_str(r#"{"beach":true,"beach_slope":0.25}"#).unwrap();
        assert_eq!(legacy.sand_tile_size, 4.0);
        assert_eq!(legacy.sand_rotation_deg, 0.0);
        for angle in [-180.0, -45.0, 90.0, 180.0] {
            let config = OpticsConfig {
                sand_rotation_deg: angle,
                ..legacy
            };
            assert!(config.validate().is_ok());
            let loaded: OpticsConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(loaded.sand_rotation_deg, angle);
        }
        for angle in [f32::NAN, f32::INFINITY, -181.0, 181.0] {
            assert!(
                OpticsConfig {
                    sand_rotation_deg: angle,
                    ..legacy
                }
                .validate()
                .is_err()
            );
        }
        assert_eq!(legacy.beach_slope, 0.25);
        for size in [0.25, 16.0, 128.0] {
            let config = OpticsConfig {
                sand_tile_size: size,
                ..legacy
            };
            assert!(config.validate().is_ok());
            let loaded: OpticsConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(loaded.sand_tile_size, size);
        }
        for size in [f32::NAN, f32::INFINITY, -1.0, 0.0, 0.24, 129.0] {
            assert!(
                OpticsConfig {
                    sand_tile_size: size,
                    ..legacy
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn water_lighting_settings_legacy_roundtrip_and_finite_bounds() {
        let legacy: OpticsConfig = serde_json::from_str(r#"{"depth":9.0}"#).unwrap();
        assert!(!legacy.caustics && !legacy.refraction);
        assert_eq!(legacy.depth, 9.0);
        assert_eq!(legacy.reflection_strength, 1.0);
        assert_eq!(legacy.reflection_steps, 128);
        assert_eq!(legacy.reflection_tolerance, 1.0);
        assert_eq!(legacy.reflection_edge_fade, 4.0);
        use crate::ocean::parameters::optics::OpticalMethod;
        let old_enabled: OpticsConfig =
            serde_json::from_str(r#"{"caustics":true,"caustics_method":"fast","fast_caustics_size":8.0,"refraction":true}"#).unwrap();
        assert!(old_enabled.caustics);
        assert_eq!(old_enabled.refraction_method, OpticalMethod::Physical);

        let configured = OpticsConfig {
            caustics: true,
            refraction: true,
            refraction_method: OpticalMethod::Fast,
            fast_refraction_strength: 0.6,
            fast_refraction_depth: 3.0,
            fast_refraction_limit: 0.02,
            caustics_strength: 2.5,
            caustics_extent: 32.0,
            caustics_softness: 2.0,
            caustics_resolution: 512,
            water_ior: 1.4,
            refraction_distance: 75.0,
            screen_space_reflections: true,
            reflection_distance: 60.0,
            reflection_strength: 0.5,
            reflection_steps: 64,
            reflection_tolerance: 0.75,
            reflection_edge_fade: 24.0,
            view: 15,
            ..legacy
        };
        let restored: OpticsConfig =
            serde_json::from_str(&serde_json::to_string(&configured).unwrap()).unwrap();
        assert_eq!(restored, configured);
        assert!(restored.validate().is_ok());
        for bad in [f32::NAN, f32::INFINITY, -1.0] {
            for field in 0..13 {
                let mut config = configured;
                match field {
                    0 => config.caustics_strength = bad,
                    1 => config.caustics_extent = bad,
                    2 => config.caustics_softness = bad,
                    3 => config.water_ior = bad,
                    4 => config.refraction_distance = bad,
                    5 => config.fast_refraction_strength = bad,
                    6 => config.fast_refraction_depth = bad,
                    7 => config.fast_refraction_limit = bad,
                    8 => config.reflection_strength = bad,
                    9 => config.reflection_tolerance = bad,
                    10 => config.reflection_edge_fade = bad,
                    11 => config.reflection_distance = bad,
                    _ => config.caustics_resolution = 129,
                }
                assert!(config.validate().is_err());
            }
        }
        for steps in [0, 15, 257, u32::MAX] {
            assert!(
                OpticsConfig {
                    reflection_steps: steps,
                    ..configured
                }
                .validate()
                .is_err()
            );
        }
        for (strength, tolerance, fade, steps) in [(0.0, 0.25, 0.0, 16), (1.0, 4.0, 128.0, 256)] {
            assert!(
                OpticsConfig {
                    reflection_strength: strength,
                    reflection_tolerance: tolerance,
                    reflection_edge_fade: fade,
                    reflection_steps: steps,
                    ..configured
                }
                .validate()
                .is_ok()
            );
        }
    }

    #[test]
    fn invalid_optical_parameters_are_rejected() {
        for invalid in [
            OpticsConfig {
                depth: -1.0,
                ..Default::default()
            },
            OpticsConfig {
                sigma_t: [f32::NAN, 0.0, 0.0],
                ..Default::default()
            },
            OpticsConfig {
                source: [-0.1, 0.0, 0.0],
                ..Default::default()
            },
            OpticsConfig {
                view: 16,
                ..Default::default()
            },
        ] {
            assert!(invalid.validate().is_err());
        }
        assert!(
            OpticsConfig {
                sigma_t: [0.0; 3],
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
    }
}

//! Homogeneous water: manual constant source plus depth-dependent sky scattering.
use super::gpu::{Error, Gpu};
use super::skylight::SkyConfig;
use wgpu::util::DeviceExt;

pub const VIEW_NAMES: [&str; 12] = [
    "Shaded water",
    "Water path (m)",
    "Transmittance T",
    "Attenuated seabed (absorption)",
    "In-scattering",
    "Extinction loss",
    "Raw seabed",
    "Medium sum",
    "Sky source at ray midpoint",
    "Integrated sky scattering",
    "Integrated manual source",
    "Total source at ray midpoint",
];

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct OpticsConfig {
    pub shore: super::shore::ShoreConfig,
    pub depth: f32,
    pub beach: bool,
    pub beach_slope: f32,
    pub shore_x: f32,
    pub sigma_t: [f32; 3],
    pub source: [f32; 3],
    pub view: u32,
    pub path_range: f32,
    pub reflections: bool,
    pub color_bands: bool,
    pub checker_size: f32,
    #[serde(default = "SkyConfig::legacy")]
    pub sky: SkyConfig,
}

impl Default for OpticsConfig {
    fn default() -> Self {
        Self {
            shore: super::shore::ShoreConfig::default(),
            depth: 6.0,
            beach: false,
            beach_slope: 0.18,
            shore_x: -20.0,
            sigma_t: [0.18, 0.06, 0.025],
            source: [0.0025, 0.009, 0.006],
            view: 0,
            path_range: 50.0,
            reflections: true,
            color_bands: false,
            checker_size: 4.0,
            sky: SkyConfig::default(),
        }
    }
}

impl OpticsConfig {
    pub fn beach_scene() -> Self {
        Self {
            beach: true,
            source: [0.0; 3],
            sigma_t: [0.22, 0.075, 0.035],
            path_range: 35.0,
            sky: SkyConfig {
                albedo: [0.25, 0.55, 0.75],
                steps: 32,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.shore.validate()?;
        self.sky.validate()?;
        if !self.beach_slope.is_finite()
            || !(0.01..=1.0).contains(&self.beach_slope)
            || !self.shore_x.is_finite()
            || self.shore_x.abs() > 1000.0
        {
            return Err("beach slope must be 0.01..1; shoreline must be within +/-1000 m".into());
        }
        for (name, value) in [
            ("seabed depth", self.depth),
            ("path view range", self.path_range),
            ("checker size", self.checker_size),
        ] {
            if !value.is_finite() || value <= 0.0 || value > 1000.0 {
                return Err(format!("{name} must be finite and in (0, 1000]"));
            }
        }
        for (name, rgb) in [("sigma_t", self.sigma_t), ("source", self.source)] {
            if rgb.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > 10.0) {
                return Err(format!("{name} RGB must be finite and in 0..=10"));
            }
        }
        if self.view >= VIEW_NAMES.len() as u32 {
            return Err("water view must be in 0..=11".into());
        }
        Ok(())
    }

    pub fn uniform(&self) -> OpticsUniform {
        OpticsUniform {
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
            beach: [f32::from(self.beach), self.beach_slope, self.shore_x, 0.0],
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
        include_str!("optics.wgsl"),
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
                view: 12,
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

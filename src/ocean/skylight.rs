//! Isotropic single-scattered sky light, tabulated against optical depth.
//! The LUT contains angular-integrated incident radiance, before sigma_s.
use super::gpu::Gpu;
use std::cell::Cell;
use wgpu::util::DeviceExt;

pub const LUT_SIZE: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct SkyConfig {
    pub enabled: bool,
    pub albedo: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub steps: u32,
}

impl Default for SkyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            albedo: [0.3, 0.5, 0.65],
            color: [1.0; 3],
            intensity: 1.0,
            steps: 16,
        }
    }
}

impl SkyConfig {
    /// Old JSON without a sky section keeps its previous rendered appearance.
    pub fn legacy() -> Self {
        Self {
            enabled: false,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .albedo
            .iter()
            .chain(&self.color)
            .any(|x| !x.is_finite() || !(0.0..=1.0).contains(x))
        {
            return Err("sky color and scattering albedo RGB must be in 0..=1".into());
        }
        if !self.intensity.is_finite() || !(0.0..=20.0).contains(&self.intensity) {
            return Err("sky intensity must be finite and in 0..=20".into());
        }
        if ![8, 16, 32].contains(&self.steps) {
            return Err("sky integration steps must be 8, 16 or 32".into());
        }
        Ok(())
    }

    pub fn tint(&self) -> [f32; 4] {
        [self.color[0], self.color[1], self.color[2], self.intensity]
    }
}

pub struct SkyLut {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    uniform: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    tint: Cell<Option<[f32; 4]>>,
    dirty: Cell<bool>,
}

impl SkyLut {
    pub fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sky optical depth LUT"),
            size: wgpu::Extent3d {
                width: LUT_SIZE,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("sky tint and strength"),
            contents: bytemuck::cast_slice(&SkyConfig::default().tint()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky depth precomputation"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("sky.wgsl"),
                    include_str!("skylight_lut.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("sky depth LUT"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky LUT bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        Self {
            texture,
            view,
            uniform,
            pipeline,
            bind,
            tint: Cell::new(None),
            dirty: Cell::new(true),
        }
    }

    pub fn update(&self, gpu: &Gpu, config: &SkyConfig) {
        let tint = config.tint();
        if self.tint.get() != Some(tint) {
            gpu.queue
                .write_buffer(&self.uniform, 0, bytemuck::cast_slice(&tint));
            self.tint.set(Some(tint));
            self.dirty.set(true);
        }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.dirty.replace(false) {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("update sky depth LUT"),
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.dispatch_workgroups(LUT_SIZE.div_ceil(64), 1, 1);
    }
}

/// Independent f64 angular quadrature used by validation and the GUI depth graph.
/// tau is extinction times vertical depth; no surface cosine or sun disc.
pub fn incident_reference(config: &SkyConfig, tau: f64, channel: usize, directions: u32) -> f64 {
    let horizon = [0.64, 0.79, 0.85];
    let zenith = [0.09, 0.30, 0.57];
    (0..directions)
        .map(|i| {
            let mu = (i as f64 + 0.5) / directions as f64;
            let sky = horizon[channel] + (zenith[channel] - horizon[channel]) * mu.powf(0.45);
            sky * config.color[channel] as f64 * config.intensity as f64 * (-tau / mu).exp()
        })
        .sum::<f64>()
        * 0.5
        / directions as f64
}

pub fn depth_graph(ui: &mut egui::Ui, optics: &super::optics::OpticsConfig) {
    let depth = optics.depth;
    let colors = [
        egui::Color32::LIGHT_RED,
        egui::Color32::LIGHT_GREEN,
        egui::Color32::LIGHT_BLUE,
    ];
    let source = |z: f32, c: usize| {
        if !optics.sky.enabled {
            return 0.0;
        }
        optics.sigma_t[c]
            * optics.sky.albedo[c]
            * incident_reference(&optics.sky, f64::from(z * optics.sigma_t[c]), c, 32) as f32
    };
    let peak = (0..3).map(|c| source(0.0, c)).fold(0.0_f32, f32::max);
    ui.small(format!("Depth: 0 .. {depth:.2} m; source: 0 .. {peak:.5}"));
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, egui::Color32::from_gray(18));
    for (c, color) in colors.into_iter().enumerate() {
        let points = (0..65)
            .map(|i| {
                let u = i as f32 / 64.0;
                egui::pos2(
                    rect.left() + u * rect.width(),
                    rect.bottom() - source(depth * u, c) / peak.max(1e-20) * rect.height(),
                )
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
    }
    ui.small("Local sky source (radiance/m), before integration to the eye.");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_decreases_with_depth_and_scales_with_sky() {
        let sky = SkyConfig::default();
        for c in 0..3 {
            let mut previous = f64::INFINITY;
            for tau in [0.0, 0.001, 0.1, 1.0, 10.0, 32.0] {
                let value = incident_reference(&sky, tau, c, 4096);
                assert!(value >= 0.0 && value < previous);
                previous = value;
                let brighter = SkyConfig {
                    intensity: 2.0,
                    ..sky
                };
                assert_eq!(incident_reference(&brighter, tau, c, 4096), 2.0 * value);
            }
        }
    }
    #[test]
    fn sky_validation_and_old_config_compatibility() {
        for sky in [
            SkyConfig {
                albedo: [1.1, 0.0, 0.0],
                ..Default::default()
            },
            SkyConfig {
                intensity: f32::NAN,
                ..Default::default()
            },
            SkyConfig {
                steps: 0,
                ..Default::default()
            },
        ] {
            assert!(sky.validate().is_err());
        }
        let old: super::super::optics::OpticsConfig =
            serde_json::from_str(r#"{"source":[0.1,0.2,0.3]}"#).unwrap();
        assert!(!old.sky.enabled);
        assert_eq!(old.source, [0.1, 0.2, 0.3]);
    }
}

//! Isotropic single-scattered sky light, tabulated against optical depth.
//! The LUT contains angular-integrated incident radiance, before sigma_s.
use crate::ocean::gpu::context::Gpu;
use std::cell::Cell;
use wgpu::util::DeviceExt;

pub const LUT_SIZE: u32 = 256;

pub use crate::ocean::parameters::sky::SkyConfig;

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
                    include_str!("shaders/sky.wgsl"),
                    include_str!("shaders/skylight_lut.wgsl")
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
        let old: crate::ocean::parameters::optics::OpticsConfig =
            serde_json::from_str(r#"{"source":[0.1,0.2,0.3]}"#).unwrap();
        assert!(!old.sky.enabled);
        assert_eq!(old.source, [0.1, 0.2, 0.3]);
    }
}

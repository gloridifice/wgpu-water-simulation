//! Local hydrostatic-reconstruction SWE, foam transport and ground memory.
use super::{
    gpu::{Gpu, Ocean},
    optics::OpticsConfig,
};
use wgpu::util::DeviceExt;

#[path = "terrain_verify.rs"]
mod terrain_verify;
pub fn verify_terrain(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    terrain_verify::verify(gpu)
}

#[path = "collider_verify.rs"]
mod collider_verify;
#[path = "foam_dual_verify.rs"]
mod foam_dual_verify;
#[path = "foam_height_verify.rs"]
mod foam_height_verify;
#[path = "foam_material_verify.rs"]
mod foam_material_verify;
#[path = "foam_momentum_verify.rs"]
mod foam_momentum_verify;
#[path = "foam_ridge_verify.rs"]
mod foam_ridge_verify;

pub const RESOLUTION: u32 = 256;
pub const RESOLUTIONS: [u32; 5] = [64, 128, 256, 512, 1024];
pub const FOAM_TEXTURE_NAMES: [&str; 4] = [
    "原灰度效果",
    "Shorewaves NSH",
    "Crest Foam NSH",
    "Crest Foam2 NSH",
];
pub const FOAM_MATERIAL_VIEWS: [&str; 6] = [
    "正常着色",
    "Soft 图案",
    "Height 图案",
    "泡沫法线",
    "不透明度（黑透／白实）",
    "输运坐标网格",
];
pub const VIEWS: [(u32, &str); 10] = [
    (0, "正常着色"),
    (1, "水深"),
    (2, "水流方向"),
    (3, "近岸泡沫"),
    (4, "沙地湿度"),
    (6, "泡沫密度（低绿／高红）"),
    (7, "前沿实际生成率（最近子步）"),
    (8, "压缩脊线发射权重"),
    (9, "新鲜白沫 N 密度"),
    (10, "水面残余 R 密度"),
];

fn deserialize_view<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let view = <u32 as serde::Deserialize>::deserialize(deserializer)?;
    // Removed sand-foam view falls back without discarding the saved session.
    Ok(if view == 5 { 0 } else { view })
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShoreConfig {
    pub colliders: [Option<super::collider::Collider>; super::collider::MAX_COLLIDERS],
    pub enabled: bool,
    pub resolution: u32,
    pub order: u32,
    pub input_gain: f32,
    pub filter_enabled: bool,
    pub filter_cutoff: f32,
    pub filter_transition: f32,
    pub normal_strength: f32,
    pub water_foam_color: [f32; 3],
    pub foam_texture: u32,
    pub foam_texture_size: f32,
    pub foam_micro_normal: f32,
    pub foam_opacity_scale: f32,
    pub foam_roughness: f32,
    pub foam_material_view: u32,
    pub foam_prototype: bool,
    pub foam_texture_period: f32,
    pub foam_dual_phase: bool,
    pub foam_fresh_half_life: f32,
    pub foam_residual_half_life: f32,
    pub foam_conversion: f32,
    pub breaking_depth: f32,
    pub generation: f32,
    pub foam_generation_threshold: f32,
    pub foam_generation_transition: f32,
    pub foam_front_generation: f32,
    pub foam_front_distance: f32,
    pub foam_ridge_enabled: bool,
    pub foam_ridge_width: f32,
    pub foam_ridge_prominence: f32,
    pub foam_emission_enabled: bool,
    // Velocity differences divided by their world-space sampling distance (/s).
    pub foam_velocity_threshold: f32,
    pub foam_velocity_transition: f32,
    pub foam_background_multiplier: f32,
    pub foam_follow: f32,
    pub foam_response_time: f32,
    pub foam_drag_enabled: bool,
    pub foam_birth_speed: f32,
    pub foam_transport_order: u32,
    pub decay: f32,
    pub foam_compression_threshold: f32,
    pub foam_density_reference: f32,
    // Render-only mapping from unbounded surface amount/area to vertex lift.
    pub foam_height_max: f32,
    pub foam_height_threshold: f32,
    pub foam_height_transition: f32,
    pub drying_time: f32,
    #[serde(deserialize_with = "deserialize_view")]
    pub view: u32,
}
impl Default for ShoreConfig {
    fn default() -> Self {
        Self {
            colliders: [None; super::collider::MAX_COLLIDERS],
            enabled: true,
            resolution: RESOLUTION,
            order: 1,
            input_gain: 1.0,
            filter_enabled: true,
            filter_cutoff: 12.0,
            filter_transition: 12.0,
            normal_strength: 1.0,
            water_foam_color: [0.87, 0.92, 0.90],
            foam_texture: 1,
            foam_texture_size: 12.0,
            foam_micro_normal: 0.65,
            foam_opacity_scale: 2.0,
            foam_roughness: 0.85,
            foam_material_view: 0,
            foam_prototype: false,
            foam_texture_period: 2.0,
            foam_dual_phase: false,
            foam_fresh_half_life: 1.4,
            foam_residual_half_life: 9.0,
            foam_conversion: 0.65,
            breaking_depth: 2.0,
            generation: 4.0,
            foam_generation_threshold: 0.25,
            foam_generation_transition: 0.4,
            foam_front_generation: 8.0,
            foam_front_distance: 1.0,
            foam_ridge_enabled: false,
            foam_ridge_width: 1.0,
            foam_ridge_prominence: 0.02,
            foam_emission_enabled: true,
            foam_velocity_threshold: 0.1,
            foam_velocity_transition: 0.5,
            foam_background_multiplier: 0.15,
            foam_follow: 0.8,
            foam_response_time: 0.35,
            foam_drag_enabled: true,
            foam_birth_speed: 1.0,
            foam_transport_order: 1,
            decay: 0.45,
            foam_compression_threshold: 0.1,
            foam_density_reference: 2.0,
            foam_height_max: 0.05,
            foam_height_threshold: 0.1,
            foam_height_transition: 1.9,
            drying_time: 18.0,
            view: 0,
        }
    }
}
impl ShoreConfig {
    /// Physical-model starting point, not a calibrated seawater material preset.
    pub fn apply_foam_reference(&mut self) {
        self.foam_drag_enabled = true;
        self.foam_follow = 1.0;
        self.foam_response_time = 2.0;
        self.foam_birth_speed = 1.0;
        self.decay = 0.3;
    }

    pub fn validate(self) -> Result<(), String> {
        if !RESOLUTIONS.contains(&self.resolution) {
            return Err("SWE resolution must be 64, 128, 256, 512 or 1024".into());
        }
        for collider in self.colliders.iter().flatten() {
            collider.validate()?;
        }
        for (name, value, low, high) in [
            ("SWE input gain", self.input_gain, 0.0, 3.0),
            ("SWE filter cutoff", self.filter_cutoff, 0.0, 512.0),
            ("SWE filter transition", self.filter_transition, 0.0, 512.0),
            ("shore normal strength", self.normal_strength, 0.0, 3.0),
            ("foam texture size (m)", self.foam_texture_size, 0.25, 64.0),
            (
                "foam texture period (s; 0 disables resets)",
                self.foam_texture_period,
                0.0,
                60.0,
            ),
            ("foam micro normal", self.foam_micro_normal, 0.0, 3.0),
            (
                "foam opacity density scale",
                self.foam_opacity_scale,
                0.01,
                20.0,
            ),
            ("foam roughness", self.foam_roughness, 0.15, 1.0),
            ("breaking depth", self.breaking_depth, 0.5, 5.0),
            ("shore foam generation", self.generation, 0.0, 15.0),
            (
                "foam generation threshold",
                self.foam_generation_threshold,
                0.0,
                2.0,
            ),
            (
                "foam generation transition",
                self.foam_generation_transition,
                0.0,
                2.0,
            ),
            (
                "ridge emission width (m)",
                self.foam_ridge_width,
                0.05,
                10.0,
            ),
            (
                "ridge relative prominence",
                self.foam_ridge_prominence,
                0.0,
                1.0,
            ),
            (
                "fresh foam half-life (s)",
                self.foam_fresh_half_life,
                0.1,
                60.0,
            ),
            (
                "residual foam half-life (s)",
                self.foam_residual_half_life,
                0.1,
                120.0,
            ),
            ("fresh to residual fraction", self.foam_conversion, 0.0, 1.0),
            ("shore foam decay", self.decay, 0.0, 4.0),
            ("foam follow multiplier", self.foam_follow, 0.0, 1.0),
            ("foam response time", self.foam_response_time, 0.0, 20.0),
            ("foam birth speed", self.foam_birth_speed, 0.0, 1.0),
            ("foam height maximum (m)", self.foam_height_max, 0.0, 1.0),
            (
                "foam height threshold",
                self.foam_height_threshold,
                0.0,
                20.0,
            ),
            (
                "foam height transition",
                self.foam_height_transition,
                0.001,
                20.0,
            ),
            (
                "front foam generation",
                self.foam_front_generation,
                0.0,
                30.0,
            ),
            (
                "front sampling distance",
                self.foam_front_distance,
                0.05,
                10.0,
            ),
            (
                "foam velocity threshold",
                self.foam_velocity_threshold,
                0.0,
                20.0,
            ),
            (
                "foam velocity transition",
                self.foam_velocity_transition,
                0.0,
                20.0,
            ),
            (
                "background foam multiplier",
                self.foam_background_multiplier,
                0.0,
                1.0,
            ),
            (
                "foam compression threshold",
                self.foam_compression_threshold,
                0.0,
                4.0,
            ),
            (
                "foam density reference",
                self.foam_density_reference,
                0.01,
                20.0,
            ),
            ("sand drying time", self.drying_time, 1.0, 120.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..={high}"));
            }
        }
        if self
            .water_foam_color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("foam color must be finite linear RGB in 0..=1".into());
        }
        if !(1..=2).contains(&self.order) {
            return Err("SWE order must be 1 or 2".into());
        }
        if !(1..=2).contains(&self.foam_transport_order) {
            return Err("foam transport order must be 1 or 2".into());
        }
        if self.foam_texture_period > 0.0 && self.foam_texture_period < 0.25 {
            return Err("foam texture period must be 0 or 0.25..=60 seconds".into());
        }
        if self.foam_texture >= FOAM_TEXTURE_NAMES.len() as u32
            || self.foam_material_view >= FOAM_MATERIAL_VIEWS.len() as u32
        {
            return Err("invalid foam material selection".into());
        }
        if !VIEWS.iter().any(|&(id, _)| id == self.view) {
            return Err("invalid shore view".into());
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ShoreUniform {
    pub terrain: [f32; 4],
    pub waves: [f32; 4],
    pub domain: [f32; 4],
    pub timing: [f32; 4],
    pub coupling: [f32; 4],
    pub filter: [f32; 4],
    pub solver: [f32; 4],
    pub visual: [f32; 4],
    pub density: [f32; 4],
    pub foam_generation: [f32; 4],
    pub foam_front: [f32; 4],
    pub foam_ridge: [f32; 4],
    pub foam_velocity: [f32; 4],
    pub foam_motion: [f32; 4],
    pub foam_inertia: [f32; 4],
    pub water_foam_color: [f32; 4],
    pub foam_material: [f32; 4],
    pub foam_shading: [f32; 4],
    pub foam_lifetime: [f32; 4], // dual mode, fresh/residual exit rates /s, conversion fraction
    pub options: [f32; 4],
    pub collider_meta: [f32; 4],
    pub model: [f32; 4], // enabled, minimum bed, fallback bed, sponge width
    pub edges: [f32; 4], // open -X,+X,-Z,+Z
    pub colliders: [super::collider::ColliderUniform; super::collider::MAX_COLLIDERS],
}
pub struct Shore {
    pub resolution: u32,
    pub uniform: wgpu::Buffer,
    pub field: wgpu::Buffer,
    pub lace: wgpu::Buffer,
    pub terrain_bed: wgpu::Buffer,
    terrain_grid: Option<super::terrain::Grid>,
    terrain_key: Option<(super::terrain::ModelConfig, f32)>,
    previous: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    shader: wgpu::ShaderModule,
    solver_pipelines: [Option<wgpu::ComputePipeline>; 3],
    collider_edit_pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    predictor_bind: wgpu::BindGroup,
    corrector_bind: wgpu::BindGroup,
    pub substeps: u32,
    pub simulated_dt: f32,
    pub spray: Option<super::spray::SpraySource>,
    pub spray_epoch: u64,
    pub spray_error: Option<String>,
    pub timer: Option<super::gpu_timer::GpuTimer>,
    length: f32,
    fft_resolution: u32,
    height_bound: f32,
    fft_fields: wgpu::Buffer,
    fft_previous: wgpu::Buffer,
    start_time: f32,
    last: Option<ShoreUniform>,
}
impl Shore {
    pub fn new(gpu: &Gpu, ocean: &Ocean) -> Self {
        Self::with_resolution(gpu, ocean, RESOLUTION)
    }

    pub fn with_resolution(gpu: &Gpu, ocean: &Ocean, resolution: u32) -> Self {
        assert!(RESOLUTIONS.contains(&resolution));
        let length = ocean.config.length;
        let device = &gpu.device;
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shore parameters"),
            size: std::mem::size_of::<ShoreUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let make_field = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: u64::from(resolution * resolution) * 80,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let terrain_bed = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("static SWE bed height and scan coverage"),
            size: u64::from(resolution * resolution) * 8,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let field = make_field("SWE visible, conservative, ground, foam motion and texture blocks");
        let previous = make_field("previous shore history");
        let stage = make_field("SWE RK predictor");
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("local shallow water compute"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    include_str!("shore.wgsl"),
                    include_str!("shore_compute.wgsl")
                )
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("hydrostatic SWE and ground memory"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let collider_edit_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("local collider bed update"),
                layout: None,
                module: &shader,
                entry_point: Some("edit_colliders"),
                compilation_options: Default::default(),
                cache: None,
            });
        let fft_previous = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("previous FFT boundary elevation and momentum"),
            size: u64::from(ocean.config.resolution.pow(2)) * 3 * 8,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let make_bind = |input: &wgpu::Buffer, output: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SWE stage bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: terrain_bed.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: input.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: output.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: ocean.fields.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: fft_previous.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: previous.as_entire_binding(),
                    },
                ],
            })
        };
        let bind = make_bind(&previous, &field);
        let predictor_bind = make_bind(&previous, &stage);
        let corrector_bind = make_bind(&stage, &field);
        let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
            "../../assets/shorewaves/voronoi_lace.png"
        )))
        .read_info()
        .expect("bundled foam PNG");
        let mut pixels = vec![0; reader.output_buffer_size().expect("foam size")];
        let info = reader.next_frame(&mut pixels).expect("foam pixels");
        let channels = info.color_type.samples();
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        assert_eq!(info.width, info.height);
        assert!(info.width.is_power_of_two());
        let mut data = vec![info.width as f32, info.height as f32, 0.0, 0.0];
        let mut level: Vec<f32> = pixels[..info.buffer_size()]
            .chunks_exact(channels)
            .map(|p| p[0] as f32 / 255.0)
            .collect();
        let mut width = info.width as usize;
        loop {
            data.extend_from_slice(&level);
            if width == 1 {
                break;
            }
            let next_width = width / 2;
            let mut next = vec![0.0; next_width * next_width];
            for y in 0..next_width {
                for x in 0..next_width {
                    let i = 2 * y * width + 2 * x;
                    next[y * next_width + x] =
                        0.25 * (level[i] + level[i + 1] + level[i + width] + level[i + width + 1]);
                }
            }
            level = next;
            width = next_width;
        }
        let lace = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shorewaves Voronoi lace"),
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        Self {
            resolution,
            terrain_bed,
            terrain_grid: None,
            terrain_key: None,
            lace,
            uniform,
            field,
            previous,
            pipeline,
            shader,
            solver_pipelines: [None, None, None],
            collider_edit_pipeline,
            bind,
            predictor_bind,
            corrector_bind,
            substeps: 0,
            simulated_dt: 0.0,
            spray: None,
            spray_epoch: 0,
            spray_error: None,
            timer: None,
            length,
            fft_resolution: ocean.config.resolution,
            height_bound: ocean.height_bound,
            fft_fields: ocean.fields.clone(),
            fft_previous,
            start_time: 0.0,
            last: None,
        }
    }

    /// The production and fixed-input benchmark share these lazy variants.
    /// All variants use the same binding layout; changing one does not reset state.
    pub fn solver_pipeline(
        &mut self,
        gpu: &Gpu,
        order: u32,
        foam_order: u32,
    ) -> &wgpu::ComputePipeline {
        assert!((1..=2).contains(&order) && (1..=2).contains(&foam_order));
        let index = if order == 1 { 0 } else { foam_order as usize };
        self.solver_pipelines[index].get_or_insert_with(|| {
            let layout = gpu
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("shared SWE solver layout"),
                    bind_group_layouts: &[&self.pipeline.get_bind_group_layout(0)],
                    push_constant_ranges: &[],
                });
            gpu.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("specialized SWE/foam solver"),
                    layout: Some(&layout),
                    module: &self.shader,
                    entry_point: Some("main"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("SWE_ORDER", f64::from(order)),
                            (
                                "FOAM_ORDER",
                                f64::from(if order == 1 { 1 } else { foam_order }),
                            ),
                        ],
                        ..Default::default()
                    },
                    cache: None,
                })
        })
    }

    /// Regrid only on Apply/scene load/resolution changes, never per time step.
    pub fn prepare_terrain(
        &mut self,
        gpu: &Gpu,
        optics: &OpticsConfig,
        source: Option<&super::terrain::HeightField>,
    ) -> Result<(), super::gpu::Error> {
        if !optics.model.enabled {
            if self.terrain_key.take().is_some() {
                self.terrain_grid = None;
                self.last = None;
            }
            return Ok(());
        }
        let key = (optics.model.physics_key(), optics.depth);
        if self.terrain_key == Some(key) {
            return Ok(());
        }
        let source = source.ok_or("model heightfield is not loaded")?;
        let start = std::time::Instant::now();
        let grid = source.regrid(optics.model, self.resolution, optics.depth)?;
        gpu.queue
            .write_buffer(&self.terrain_bed, 0, bytemuck::cast_slice(&grid.samples));
        eprintln!(
            "SWE terrain rebuilt: {}², {:.2} ms CPU, bed {:.3}..{:.3} m relative to sea",
            self.resolution,
            start.elapsed().as_secs_f64() * 1000.0,
            grid.min_height,
            grid.max_height
        );
        self.terrain_grid = Some(grid);
        self.terrain_key = Some(key);
        self.last = None;
        Ok(())
    }

    pub fn terrain_height_range(&self) -> Option<[f32; 2]> {
        self.terrain_grid
            .as_ref()
            .map(|g| [g.min_height, g.max_height])
    }

    pub fn parameters(
        &self,
        optics: &OpticsConfig,
        time: f32,
        reset: bool,
        gain: f32,
    ) -> ShoreUniform {
        let c = optics.shore;
        let effective_gain = gain * c.input_gain;
        // Cover the whole beach along Z, with a fine cross-shore strip.
        // Widen for gentle slopes so the wet/dry front cannot leave the history.
        let inland =
            (self.height_bound * effective_gain.abs() * 2.0 / optics.beach_slope).clamp(12.0, 64.0);
        let offshore = (c.breaking_depth * 2.5 / optics.beach_slope).max(52.0);
        let domain = if optics.model.enabled {
            optics.model.domain()
        } else {
            [
                optics.shore_x - inland,
                -self.length * 1.5,
                inland + offshore,
                self.length * 3.0,
            ]
        };
        let (mut colliders, mut collider_count) = super::collider::packed(
            &c.colliders,
            domain,
            optics.beach_slope,
            optics.shore_x,
            self.resolution,
        );
        if optics.model.enabled {
            colliders = bytemuck::Zeroable::zeroed();
            collider_count = 0;
            for collider in c.colliders.iter().flatten().filter(|c| c.enabled) {
                let extent = collider.extent();
                let interior = optics.model.interior(self.resolution);
                let inside = (0..2).all(|a| {
                    collider.position[a] - extent[a] >= interior[a]
                        && collider.position[a] + extent[a] <= interior[a + 2]
                });
                if !inside {
                    continue;
                }
                let coord = [0, 1].map(|a| {
                    (((collider.position[a] - domain[a]) / domain[a + 2] * self.resolution as f32)
                        as u32)
                        .min(self.resolution - 1)
                });
                let bed = self.terrain_grid.as_ref().map_or(-optics.depth, |g| {
                    g.samples[(coord[1] * self.resolution + coord[0]) as usize][0]
                });
                let mut packed = collider.uniform(0.0, 0.0);
                packed.solid[1] = bed + collider.height;
                colliders[collider_count as usize] = packed;
                collider_count += 1;
            }
        }
        let mut s = ShoreUniform {
            terrain: [
                f32::from(optics.beach),
                optics.beach_slope,
                optics.shore_x,
                optics.depth,
            ],
            waves: [
                self.height_bound * effective_gain.abs(),
                (time - self.start_time).max(0.0),
                c.breaking_depth,
                f32::from(c.enabled),
            ],
            domain,
            timing: [time, 0.0, c.decay, c.drying_time],
            coupling: [self.fft_resolution as f32, self.length, effective_gain, 1.0],
            filter: [
                f32::from(c.filter_enabled),
                c.filter_cutoff,
                c.filter_transition,
                0.0,
            ],
            solver: [c.order as f32, 0.0, gain, c.input_gain],
            visual: [
                c.normal_strength,
                c.foam_height_max,
                c.foam_height_threshold,
                c.foam_height_transition,
            ],
            density: [
                c.foam_compression_threshold,
                c.foam_density_reference,
                0.003, // Fixed minimum water depth for foam transport/retention.
                0.0,
            ],
            foam_generation: [
                c.foam_generation_threshold,
                c.foam_generation_transition,
                0.0,
                0.0,
            ],
            foam_front: [
                c.foam_front_generation,
                c.foam_front_distance,
                0.0,
                c.foam_background_multiplier,
            ],
            foam_ridge: [
                f32::from(c.foam_ridge_enabled),
                c.foam_ridge_width,
                c.foam_ridge_prominence,
                f32::from(c.foam_emission_enabled),
            ],
            foam_velocity: [
                0.0,
                c.foam_velocity_threshold,
                c.foam_velocity_transition,
                0.0,
            ],
            foam_motion: [c.foam_follow, c.foam_response_time, 0.0, 0.0],
            foam_inertia: [
                f32::from(c.foam_drag_enabled),
                c.foam_birth_speed,
                if c.order == 2 {
                    c.foam_transport_order as f32
                } else {
                    1.0
                },
                0.0,
            ],
            water_foam_color: [
                c.water_foam_color[0],
                c.water_foam_color[1],
                c.water_foam_color[2],
                1.0,
            ],
            foam_material: [
                c.foam_texture as f32,
                c.foam_texture_size,
                c.foam_micro_normal,
                c.foam_opacity_scale,
            ],
            foam_shading: [
                c.foam_prototype as u32 as f32,
                c.foam_roughness,
                c.foam_material_view as f32,
                c.foam_texture_period,
            ],
            foam_lifetime: [
                f32::from(c.foam_dual_phase),
                std::f32::consts::LN_2 / c.foam_fresh_half_life,
                std::f32::consts::LN_2 / c.foam_residual_half_life,
                c.foam_conversion,
            ],
            options: [c.generation, c.view as f32, self.resolution as f32, 0.0],
            collider_meta: [collider_count as f32, 0.0, 0.0, 0.0],
            model: [
                f32::from(optics.model.enabled),
                self.terrain_grid
                    .as_ref()
                    .map_or(-optics.depth, |g| g.min_height),
                -optics.depth,
                optics.model.sponge_width,
            ],
            edges: optics.model.open_edges.map(f32::from),
            colliders,
        };
        let changed = self.last.is_none_or(|old| {
            old.solver[0] != s.solver[0]
                || old.foam_lifetime[0] != s.foam_lifetime[0]
                || old.solver[2..] != s.solver[2..]
                || old.filter != s.filter
                || old.terrain != s.terrain
                || old.waves[2..] != s.waves[2..]
                || old.domain != s.domain
                || old.model != s.model
                || old.edges != s.edges
                || time < old.timing[0]
        });
        let reset = reset || changed;
        s.options[3] = f32::from(reset);
        if !reset {
            s.timing[1] = self.last.map_or(0.0, |old| (time - old.timing[0]).max(0.0));
        }
        s
    }

    /// Refresh shared terrain/wave settings for render-only validation callers.
    /// During simulation call this before encode, never after it in a submission.
    pub fn configure(&self, gpu: &Gpu, optics: &OpticsConfig) {
        let time = self.last.map_or(0.0, |s| s.timing[0]);
        let step = self.parameters(optics, time, false, self.last.map_or(0.22, |s| s.solver[2]));
        gpu.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&step));
    }

    // Separate from time stepping so paused edits and both RK stages see the
    // same corrected state. Each upload is immutable for this command buffer.
    fn encode_collider_edit(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        step: ShoreUniform,
        old: ShoreUniform,
    ) {
        let parameters = |value: &ShoreUniform| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("collider bed edit parameters"),
                    contents: bytemuck::bytes_of(value),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        };
        let current = parameters(&step);
        let previous = parameters(&old);
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("collider bed edit bindings"),
            layout: &self.collider_edit_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: current.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.previous.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: previous.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: self.terrain_bed.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("apply local collider bed changes"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.collider_edit_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(self.resolution.div_ceil(8), self.resolution.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(&self.field, 0, &self.previous, 0, self.field.size());
    }

    /// Submit before calling again: each encode writes the same parameter buffer.
    pub fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        optics: &OpticsConfig,
        time: f32,
        reset: bool,
        height_gain: f32,
    ) {
        self.substeps = 0;
        self.simulated_dt = 0.0;
        let mut step = self.parameters(optics, time, reset, height_gain);
        if !optics.spray.enabled || !optics.swe_enabled() {
            self.spray = None;
            self.spray_error = None;
        } else if self.spray_error.is_some() {
            self.spray = None;
        } else if self.spray.is_none() {
            match super::spray::SpraySource::new(gpu, self, &self.previous) {
                Ok(source) => self.spray = Some(source),
                Err(error) => self.spray_error = Some(error.to_string()),
            }
        }
        if let Some(source) = &mut self.spray {
            // A paused frame retains the last debug signal but never consumes it (dt=0).
            if step.timing[1] > 0.0 || step.options[3] > 0.5 {
                source.begin_frame(encoder);
            }
            source.configure(
                gpu,
                optics.spray,
                0.0,
                step.domain[2] * step.domain[3] / self.resolution.pow(2) as f32,
            );
        }
        if step.options[3] > 0.5 {
            self.spray_epoch = self.spray_epoch.wrapping_add(1);
            self.start_time = time;
        }
        step.waves[1] = (time - self.start_time).max(0.0);
        if step.options[3] == 0.0
            && let Some(old) = self.last
            && (old.colliders != step.colliders || old.collider_meta[0] != step.collider_meta[0])
        {
            self.encode_collider_edit(gpu, encoder, step, old);
            self.spray_epoch = self.spray_epoch.wrapping_add(1);
            if let Some(source) = &self.spray {
                source.begin_frame(encoder);
            }
        }
        if step.options[3] == 0.0 && (step.timing[1] == 0.0 || !optics.swe_enabled()) {
            gpu.queue
                .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&step));
            self.last = Some(step);
            return;
        }
        // A conservative global CFL bound includes the enforced velocity cap.
        // Long UI stalls advance at most 0.1 s rather than risking an unstable step.
        let still_depth = if optics.model.enabled {
            -step.model[1]
        } else {
            step.domain[2] * step.terrain[1]
        };
        let depth_bound = (still_depth + 4.0 * step.waves[0]).max(1.0);
        let speed = 12.0 + (9.81 * depth_bound).sqrt();
        let cell_x = step.domain[2] / self.resolution as f32;
        let cell_z = step.domain[3] / self.resolution as f32;
        let stable_dt = 0.2 / (speed / cell_x + speed / cell_z);
        let elapsed = step.timing[1].min(0.1).min(stable_dt * 128.0);
        let count = (elapsed / stable_dt).ceil().max(1.0) as u32;
        let dt = elapsed / count as f32;
        self.substeps = count;
        self.simulated_dt = elapsed;
        if let Some(source) = &mut self.spray {
            source.configure(
                gpu,
                optics.spray,
                if step.options[3] > 0.5 { 0.0 } else { elapsed },
                step.domain[2] * step.domain[3] / self.resolution.pow(2) as f32,
            );
        }
        let stages = if optics.shore.order == 2 && step.options[3] == 0.0 {
            2
        } else {
            1
        };
        let mut uniforms = Vec::new();
        for i in 0..count {
            for stage in 0..stages {
                let mut sub = step;
                // SSP-RK2 evaluates its RHS at t_n and t_n+dt.
                let fraction = if stages == 2 { stage } else { 1 };
                sub.timing[0] = time - elapsed + (i + fraction) as f32 * dt;
                sub.timing[1] = dt;
                sub.waves[1] = (sub.timing[0] - self.start_time).max(0.0);
                sub.coupling[3] = ((sub.timing[0] - (time - step.timing[1]))
                    / step.timing[1].max(0.000001))
                .clamp(0.0, 1.0);
                if step.options[3] > 0.5 {
                    sub.coupling[3] = 1.0;
                }
                sub.solver[1] = if stages == 2 { (stage + 1) as f32 } else { 0.0 };
                uniforms.push(sub);
            }
        }
        uniforms.push(step);
        let upload = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SWE substep parameters"),
                contents: bytemuck::cast_slice(&uniforms),
                usage: wgpu::BufferUsages::COPY_SRC,
            });
        let stride = std::mem::size_of::<ShoreUniform>() as u64;
        let timed = self.timer.as_ref().is_some_and(|t| t.ready())
            && elapsed > 0.0
            && step.options[3] == 0.0;
        let pipeline = self
            .solver_pipeline(gpu, optics.shore.order, step.foam_inertia[2] as u32)
            .clone();
        for i in 0..count * stages {
            encoder.copy_buffer_to_buffer(&upload, i as u64 * stride, &self.uniform, 0, stride);
            {
                let timestamps = if timed {
                    Some(wgpu::ComputePassTimestampWrites {
                        query_set: &self.timer.as_ref().unwrap().queries,
                        beginning_of_pass_write_index: if i == 0 { Some(0) } else { None },
                        end_of_pass_write_index: if i + 1 == count * stages && self.spray.is_none()
                        {
                            Some(1)
                        } else {
                            None
                        },
                    })
                } else {
                    None
                };
                // Middle passes have neither timestamp; omit descriptor entirely.
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("local SWE and water foam"),
                    timestamp_writes: timestamps.filter(|t| {
                        t.beginning_of_pass_write_index.is_some()
                            || t.end_of_pass_write_index.is_some()
                    }),
                });
                pass.set_pipeline(&pipeline);
                let bind = if stages == 1 {
                    &self.bind
                } else if i % 2 == 0 {
                    &self.predictor_bind
                } else {
                    &self.corrector_bind
                };
                pass.set_bind_group(0, bind, &[]);
                pass.dispatch_workgroups(
                    self.resolution.div_ceil(8),
                    self.resolution.div_ceil(8),
                    1,
                );
            }
            if (i + 1) % stages == 0 {
                if step.options[3] == 0.0
                    && let Some(source) = &self.spray
                {
                    source.encode_substep(
                        encoder,
                        if timed && i + 1 == count * stages {
                            Some(wgpu::ComputePassTimestampWrites {
                                query_set: &self.timer.as_ref().unwrap().queries,
                                beginning_of_pass_write_index: None,
                                end_of_pass_write_index: Some(1),
                            })
                        } else {
                            None
                        },
                    );
                }
                encoder.copy_buffer_to_buffer(&self.field, 0, &self.previous, 0, self.field.size());
            }
        }
        if timed {
            self.timer
                .as_mut()
                .unwrap()
                .resolve(encoder, optics.shore.order, count, elapsed);
        }
        encoder.copy_buffer_to_buffer(
            &upload,
            (count * stages) as u64 * stride,
            &self.uniform,
            0,
            stride,
        );
        encoder.copy_buffer_to_buffer(
            &self.fft_fields,
            self.fft_previous.size(),
            &self.fft_previous,
            0,
            self.fft_previous.size(),
        );
        self.last = Some(step);
    }
}

/// Deterministic FFT field tests reset sampling, the entire patch and gain changes
/// at paused time. The previous FFT is intentionally unrelated to this field.
fn verify_fft_initialization(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            resolution: 32,
            ..Default::default()
        },
    )?;
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("synthetic FFT initialization test"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var<storage,read_write> data:array<vec2<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=6144u {return;}
    var value=0.0;
    if id.x>=3072u && id.x<4096u {value=0.02;}
    if id.x>=4096u && id.x<5120u {value=0.03;}
    if id.x>=5120u {value=-0.01;}
    data[id.x]=vec2(value,0.0);
}"#
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("synthetic FFT fill"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: ocean.fields.as_entire_binding(),
        }],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(96, 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let mut shore = Shore::new(gpu, &ocean);
    let mut optics = OpticsConfig::beach_scene();
    let cells = (RESOLUTION * RESOLUTION) as usize;
    for gain in [0.0, 1.0, 3.0, 0.5] {
        optics.shore.input_gain = gain;
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        shore.encode(gpu, &mut enc, &optics, 2.0, false, 0.5);
        gpu.queue.submit([enc.finish()]);
        let p = shore.last.unwrap();
        if p.options[3] != 1.0 || p.coupling[2] != 0.5 * gain {
            return Err("FFT input gain did not reset/scale".into());
        }
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        let data: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        for y in 0..RESOLUTION as usize {
            for x in 0..RESOLUTION as usize {
                let i = y * RESOLUTION as usize + x;
                let world_x = p.domain[0] + (x as f32 + 0.5) * p.domain[2] / RESOLUTION as f32;
                let bottom = -optics.beach_slope * (world_x - optics.shore_x);
                let h = (0.02 * 0.5 * gain - bottom).max(0.0);
                let mut mx = 0.03 * 0.5 * gain;
                let mut mz = -0.01 * 0.5 * gain;
                let limit = (8.0 * h / mx.hypot(mz).max(1e-6)).min(1.0);
                mx *= limit;
                mz *= limit;
                if h <= 0.003 {
                    mx = 0.0;
                    mz = 0.0;
                }
                let actual = data[cells + i];
                if (actual[0] - h).abs() > 2e-5
                    || (actual[1] - mx).abs() > 2e-5
                    || (actual[2] - mz).abs() > 2e-5
                    || data[i][0] != 0.0
                    || data[2 * cells + i][0] != 0.0
                {
                    return Err(format!("FFT initialization mismatch gain={gain} cell={i}: {actual:?}, expected {h}/{mx}/{mz}").into());
                }
            }
        }
        // Render-only refresh must not multiply the input gain twice.
        shore.configure(gpu, &optics);
        let refreshed = shore.parameters(&optics, 2.0, false, p.solver[2]);
        if refreshed.coupling[2] != p.coupling[2] || refreshed.options[3] != 0.0 {
            return Err("FFT gain changed on refresh".into());
        }
    }
    eprintln!(
        "SWE FFT initialization: whole patch, current-frame sampling, dry momentum, paused gain resets 0/1/3/0.5 OK"
    );
    Ok(())
}

/// Check production inlet sampling against CPU bilinear/time interpolation.
fn verify_inlet(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            resolution: 32,
            ..Default::default()
        },
    )?;
    let shore = Shore::new(gpu, &ocean);
    let mut frames = Vec::new();
    for time in [1.0, 2.0] {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode(gpu, &mut encoder, time);
        gpu.queue.submit([encoder.finish()]);
        frames.push(ocean.read_fields(gpu)?);
    }
    let n = ocean.config.resolution as usize;
    let count = n * n;
    gpu.queue.write_buffer(
        &shore.fft_previous,
        0,
        bytemuck::cast_slice(&frames[0][3 * count..]),
    );
    let mut params = shore.parameters(&OpticsConfig::beach_scene(), 8.0, false, 0.22);
    params.waves[1] = 8.0;
    params.coupling[3] = 0.25;
    gpu.queue
        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&params));
    let source = format!(
        "{}\n{}\n{}",
        include_str!("shore.wgsl"),
        include_str!("shore_compute.wgsl"),
        r#"
@compute @workgroup_size(8,8)
fn inlet_probe(@builtin(global_invocation_id) id:vec3<u32>) {
    let n=u32(shore.options.z); if id.x>=n || id.y>=n { return; }
    let c=vec2<i32>(id.xy);
    new_shore[id.y*n+id.x]=vec4(incident(world(c),max(-bed(c),0.1)),0.0);
}"#
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("inlet sampling verification"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("inlet probe"),
            layout: None,
            module: &shader,
            entry_point: Some("inlet_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("inlet probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: ocean.fields.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: shore.fft_previous.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: shore.terrain_bed.as_entire_binding(),
            },
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    let bytes = gpu.read_buffer(&shore.field, u64::from(RESOLUTION.pow(2)) * 16)?;
    let samples: Vec<[f32; 4]> = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|b| bytemuck::pod_read_unaligned(b))
        .collect();
    let mut max_error = 0.0_f32;
    for z in [0, 17, 128, 255] {
        for x in [220, 240, 255] {
            let p = [
                params.domain[0] + (x as f32 + 0.5) / RESOLUTION as f32 * params.domain[2],
                params.domain[1] + (z as f32 + 0.5) / RESOLUTION as f32 * params.domain[3],
            ];
            let grid = p.map(|v| (v / ocean.config.length + 0.5) * n as f32);
            let ix = grid[0].floor() as i32;
            let iz = grid[1].floor() as i32;
            let fx = grid[0] - grid[0].floor();
            let fz = grid[1] - grid[1].floor();
            let mut expected = [0.0_f32; 3];
            for (channel, value) in expected.iter_mut().enumerate() {
                let at = |dx: i32, dz: i32| {
                    let i = (3 + channel) * count
                        + (iz + dz).rem_euclid(n as i32) as usize * n
                        + (ix + dx).rem_euclid(n as i32) as usize;
                    0.75 * frames[0][i][0] + 0.25 * frames[1][i][0]
                };
                *value = ((1.0 - fz) * ((1.0 - fx) * at(0, 0) + fx * at(1, 0))
                    + fz * ((1.0 - fx) * at(0, 1) + fx * at(1, 1)))
                    * params.coupling[2];
            }
            let depth = (params.terrain[1] * (p[0] - params.terrain[2])).max(0.1);
            let safety = 1.0_f32
                .min(0.5 * depth / expected[0].abs().max(1e-6))
                .min(8.0 * depth / expected[1].hypot(expected[2]).max(1e-6));
            for c in 0..3 {
                max_error = max_error
                    .max((samples[z * RESOLUTION as usize + x][c] - expected[c] * safety).abs());
            }
        }
    }
    if max_error > 0.00001 {
        return Err(format!("FFT inlet coordinate/time/gain mismatch {max_error}").into());
    }
    eprintln!(
        "FFT inlet: world-space wrapping, height gain and substep temporal interpolation error {max_error:.2e} OK"
    );
    Ok(())
}

/// Compare both orders against a linear traveling gravity wave (16 cells/wavelength).
/// Flat bed and an interior measurement region isolate propagation from the sponge.
fn verify_wave_accuracy(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let n = RESOLUTION as usize;
    let cells = n * n;
    let depth = 2.0_f32;
    let amplitude = 0.001_f32;
    let speed = (9.81 * depth).sqrt();
    let k = std::f32::consts::TAU / 4.0;
    let dt = 0.004_f32;
    let steps = 250;
    let mut errors = [0.0_f64; 2];
    for order in [1u32, 2] {
        let shore = Shore::new(gpu, &ocean);
        let mut seed = vec![[0.0_f32; 4]; cells * 5];
        for z in 0..n {
            for x in 0..n {
                let eta = amplitude * (k * (x as f32 + 0.5) * 0.25).sin();
                let i = z * n + x;
                seed[i] = [0.0, 1.0, depth + eta, 0.0];
                seed[cells + i] = [depth + eta, speed * eta, 0.0, 0.0];
                seed[2 * cells + i] = [0.0, 0.0, depth + eta, 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let optics = OpticsConfig::beach_scene();
        let mut p = shore.parameters(&optics, 0.0, false, 0.0);
        p.terrain[1] = 0.0;
        p.terrain[2] = 0.0;
        p.domain = [0.0, 0.0, 64.0, 64.0];
        p.options[0] = 0.0;
        p.foam_front[0] = 0.0;
        p.options[3] = 0.0;
        p.timing[1] = dt;
        p.solver[0] = order as f32;
        for _ in 0..steps {
            for stage in 0..order {
                p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
                gpu.queue
                    .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&shore.pipeline);
                    pass.set_bind_group(
                        0,
                        if order == 1 {
                            &shore.bind
                        } else if stage == 0 {
                            &shore.predictor_bind
                        } else {
                            &shore.corrector_bind
                        },
                        &[],
                    );
                    pass.dispatch_workgroups(RESOLUTION.div_ceil(8), RESOLUTION.div_ceil(8), 1);
                }
                if stage + 1 == order {
                    enc.copy_buffer_to_buffer(
                        &shore.field,
                        0,
                        &shore.previous,
                        0,
                        shore.field.size(),
                    );
                }
                gpu.queue.submit([enc.finish()]);
            }
        }
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        let data: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        if data.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite traveling wave".into());
        }
        let mut error = 0.0;
        let mut norm = 0.0;
        for x in 48..160 {
            let expected =
                amplitude * (k * ((x as f32 + 0.5) * 0.25 - speed * dt * steps as f32)).sin();
            let actual = data[cells + n / 2 * n + x][0] - depth;
            error += (actual - expected).powi(2) as f64;
            norm += expected.powi(2) as f64;
        }
        errors[(order - 1) as usize] = (error / norm).sqrt();
    }
    if errors[1] >= errors[0] * 0.75 || errors[1] > 0.25 {
        return Err(format!("second-order wave accuracy did not improve: {errors:?}").into());
    }
    eprintln!(
        "SWE 4 m wave after 1 s: relative L2 error first={:.3}, second={:.3}",
        errors[0], errors[1]
    );
    Ok(())
}

/// Run the production solver on the GPU, including stationary and moving water.
pub fn verify(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    foam_material_verify::verify(gpu)?;
    collider_verify::verify(gpu)?;
    foam_height_verify::verify(gpu)?;
    foam_momentum_verify::verify(gpu)?;
    foam_ridge_verify::verify(gpu)?;
    foam_dual_verify::verify(gpu)?;
    verify_inlet(gpu)?;
    verify_fft_initialization(gpu)?;
    verify_wave_accuracy(gpu)?;
    verify_surface_foam(gpu)?;
    verify_foam_motion(gpu)?;
    verify_foam_generation_controls(gpu)?;
    verify_front_foam(gpu)?;
    for (order, space_order) in [(1, 1), (2, 1), (2, 2)] {
        verify_order(gpu, order, space_order)?;
    }
    Ok(())
}

fn verify_order(gpu: &Gpu, order: u32, space_order: u32) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            wind_angle: 3.64,
            ..Default::default()
        },
    )?;
    let mut shore = Shore::new(gpu, &ocean);
    let mut optics = OpticsConfig::beach_scene();
    optics.shore.order = order;
    optics.shore.foam_transport_order = space_order;
    let gain = 0.0;
    let advance = |shore: &mut Shore, optics: &OpticsConfig, time, reset, gain| {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode_filtered(gpu, &mut encoder, time, &optics.shore);
        shore.encode(gpu, &mut encoder, optics, time, reset, gain);
        gpu.queue.submit([encoder.finish()]);
    };
    let read = |shore: &Shore| -> Result<Vec<[f32; 4]>, super::gpu::Error> {
        Ok(gpu
            .read_buffer(&shore.field, shore.field.size())?
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect())
    };
    let cells = (RESOLUTION * RESOLUTION) as usize;
    advance(&mut shore, &optics, 0.0, true, gain);
    let initial = read(&shore)?;
    for i in 1..=60 {
        advance(&mut shore, &optics, i as f32 / 60.0, false, gain);
    }
    let still = read(&shore)?;
    let error = still[cells..2 * cells]
        .iter()
        .zip(&initial[cells..2 * cells])
        .map(|(a, b)| (a[0] - b[0]).abs().max(a[1].abs()).max(a[2].abs()))
        .fold(0.0_f32, f32::max);
    if error > 0.0001 {
        return Err(format!("SWE lake-at-rest drift: {error}").into());
    }
    // Local pulse: conserve water away from the open sponge and propagate in both axes.
    let mut seed = still.clone();
    let n = RESOLUTION as usize;
    let cx = n / 2;
    let cz = n / 2;
    let center = cz * n + cx;
    seed[cells + center][0] += 0.2;
    seed[cells + center][3] = 0.1;
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    advance(&mut shore, &optics, 1.05, false, gain);
    let pulse = read(&shore)?;
    let mass_before: f64 = seed[cells..2 * cells].iter().map(|q| q[0] as f64).sum();
    let mass_after: f64 = pulse[cells..2 * cells].iter().map(|q| q[0] as f64).sum();
    if (mass_before - mass_after).abs() > 0.005
        || pulse[cells + center + 1][0] <= still[cells + center + 1][0]
        || pulse[cells + center + n][0] <= still[cells + center + n][0]
        || pulse[cells + center + n][3] <= 0.0
    {
        return Err(format!(
            "SWE pulse mass/2D transport failed: mass delta {}",
            mass_after - mass_before
        )
        .into());
    }
    // Drying clears foam and momentum but preserves independently drying sand.
    let mut seed = still.clone();
    let dry = n / 8 + n / 2 * n;
    seed[dry] = [0.8, 1.0, 0.01, 0.0];
    seed[cells + dry][3] = 0.8;
    seed[3 * cells + dry][0] = 0.8;
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    advance(&mut shore, &optics, 1.10, false, gain);
    let dried = read(&shore)?;
    if dried[dry][0] != 0.0
        || dried[cells + dry][3] != 0.0
        || dried[dry][1] < 0.9
        || dried[3 * cells + dry][..2] != [0.0, 0.0]
        || dried[2 * cells + dry][..2] != [0.0, 0.0]
    {
        return Err("dry cell retained foam or lost wet sand history".into());
    }
    advance(&mut shore, &optics, 1.15, false, gain);
    let faded = read(&shore)?;
    let expected = dried[dry][1] * (-0.05_f32 / optics.shore.drying_time).exp();
    if (faded[dry][1] - expected).abs() > 0.0001 {
        return Err("wet sand drying is not in seconds".into());
    }
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&still));
    advance(&mut shore, &optics, 1.20, false, gain);
    if read(&shore)?[center][1] < 0.99 {
        return Err("water did not restore sand wetness".into());
    }
    // Mode changes reset water and ground history, even with time paused.
    optics.shore.order = 3 - order;
    advance(&mut shore, &optics, 1.20, false, gain);
    if shore.last.unwrap().options[3] != 1.0 {
        return Err("SWE order switch did not reset".into());
    }
    optics.shore.order = order;
    // Timestamp path is optional; check real asynchronous readback on supporting devices.
    shore.timer = super::gpu_timer::GpuTimer::new(gpu);
    advance(&mut shore, &optics, 1.20, true, gain);
    advance(&mut shore, &optics, 1.25, false, gain);
    let _ = read(&shore)?;
    if let Some(timer) = &mut shore.timer {
        timer.poll(gpu);
        let sample = timer.latest.ok_or("SWE timestamp readback missing")?;
        if !sample.ms.is_finite()
            || sample.ms <= 0.0
            || sample.order != order
            || sample.substeps == 0
        {
            return Err("invalid SWE GPU timing".into());
        }
        eprintln!(
            "SWE order {order}, foam space {space_order}: GPU {:.3} ms / {} substeps / {:.1} simulated ms",
            sample.ms,
            sample.substeps,
            sample.simulated_dt * 1000.0
        );
    }
    shore.timer = None;
    let gain = 1.0;
    advance(&mut shore, &optics, 0.0, true, gain);
    for i in 1..=1440 {
        advance(&mut shore, &optics, i as f32 / 60.0, false, gain);
    }
    let active = read(&shore)?;
    if active.iter().flatten().any(|v| !v.is_finite())
        || active[cells..2 * cells]
            .iter()
            .any(|q| q[0] < 0.0 || q[3] < 0.0 || q[3] > 1e6)
        || !active[..cells].iter().any(|q| q[0] > 0.1)
        || active[2 * cells..3 * cells]
            .iter()
            .any(|q| q[..2] != [0.0, 0.0])
    {
        return Err(
            "SWE run-up: invalid state, missing water foam or unexpected ground residue".into(),
        );
    }
    advance(&mut shore, &optics, 24.0, false, gain);
    if read(&shore)? != active {
        return Err("SWE pause changed state".into());
    }
    advance(&mut shore, &optics, 24.0, true, gain);
    let cleared = read(&shore)?;
    if cleared[..cells].iter().any(|q| q[0] != 0.0 || q[1] != 0.0)
        || cleared[2 * cells..3 * cells].iter().any(|q| q[0] != 0.0)
        || cleared[3 * cells..].iter().any(|q| *q != [0.0; 4])
    {
        return Err("SWE reset failed".into());
    }
    optics.shore.enabled = false;
    advance(&mut shore, &optics, 24.1, false, gain);
    if read(&shore)?.iter().any(|q| *q != [0.0; 4]) {
        return Err("SWE disable failed".into());
    }
    optics.shore.enabled = true;
    advance(&mut shore, &optics, 24.1, false, gain);
    let before_normal_edit = read(&shore)?;
    optics.shore.normal_strength = 0.0;
    optics.shore.foam_height_max = 0.3;
    optics.shore.foam_height_threshold = 0.2;
    optics.shore.foam_height_transition = 0.8;
    optics.shore.foam_generation_threshold = 0.05;
    optics.shore.foam_generation_transition = 0.15;
    optics.shore.foam_front_generation = 5.0;
    optics.shore.foam_front_distance = 0.5;
    optics.shore.foam_velocity_threshold = 0.2;
    optics.shore.foam_velocity_transition = 1.0;
    optics.shore.foam_follow = 0.6;
    optics.shore.foam_response_time = 0.7;
    optics.shore.foam_drag_enabled = false;
    optics.shore.foam_birth_speed = 0.5;
    optics.shore.foam_transport_order = 3 - space_order;
    optics.shore.foam_background_multiplier = 0.05;
    optics.shore.water_foam_color = [0.2, 0.7, 0.9];
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 0.0 || read(&shore)? != before_normal_edit {
        return Err("live normal/color/foam-height/generation edit changed SWE history".into());
    }
    optics.shore.normal_strength = 1.0;
    optics.shore.foam_generation_threshold = 0.25;
    optics.shore.foam_generation_transition = 0.4;
    optics.shore.foam_front_generation = 8.0;
    optics.shore.foam_front_distance = 1.0;
    optics.shore.foam_velocity_threshold = 0.1;
    optics.shore.foam_velocity_transition = 0.5;
    optics.shore.foam_follow = 0.8;
    optics.shore.foam_response_time = 0.35;
    optics.shore.foam_drag_enabled = true;
    optics.shore.foam_birth_speed = 1.0;
    optics.shore.foam_transport_order = space_order;
    optics.shore.foam_background_multiplier = 0.15;
    optics.shore.filter_cutoff = 3.0;
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 1.0 {
        return Err("SWE filter edit did not reset history".into());
    }
    optics.shore.filter_enabled = false;
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 1.0 {
        return Err("SWE filter toggle did not reset history".into());
    }
    optics.shore.filter_enabled = true;
    optics.shore.filter_cutoff = 12.0;
    let gain = 2.0;
    for slope in [0.01, 1.0] {
        optics.beach_slope = slope;
        advance(&mut shore, &optics, 0.0, true, gain);
        for i in 1..=180 {
            advance(&mut shore, &optics, i as f32 / 30.0, false, gain);
        }
        let extreme = read(&shore)?;
        if extreme.iter().flatten().any(|v| !v.is_finite())
            || extreme[cells..2 * cells]
                .iter()
                .any(|q| q[0] < 0.0 || q[3] < 0.0 || q[3] > 1e6)
        {
            return Err(format!("SWE invalid state at slope {slope}").into());
        }
    }
    optics.shore_x += 4.0;
    advance(&mut shore, &optics, 6.0, false, gain);
    let moved = read(&shore)?;
    if moved[..cells].iter().any(|q| q[0] != 0.0 || q[1] != 0.0) {
        return Err("SWE moved terrain retained history".into());
    }
    let zero = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let mut quiet = Shore::new(gpu, &zero);
    let mut quiet_optics = OpticsConfig::beach_scene();
    quiet_optics.shore.order = order;
    quiet_optics.shore.foam_transport_order = space_order;
    for i in 0..=120 {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let time = i as f32 / 30.0;
        zero.encode(gpu, &mut encoder, time);
        quiet.encode(gpu, &mut encoder, &quiet_optics, time, i == 0, 1.0);
        gpu.queue.submit([encoder.finish()]);
    }
    let quiet_state = read(&quiet)?;
    if quiet_state[..cells]
        .iter()
        .any(|v| v[0] > 0.00001 || v[3].abs() > 0.0001)
    {
        return Err("zero FFT amplitude generated shore waves/foam".into());
    }
    eprintln!(
        "SWE GPU order {order}, foam space {space_order}: lake at rest ({error:.2e}), pulse mass/2D foam transport, dry foam removal and sand drying/rewetting, run-up, pause/reset/disable, terrain reset and extreme slopes OK"
    );
    Ok(())
}

/// Probe the production generation function, including a zero-width cutoff.
fn verify_foam_generation_controls(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(gpu, super::spectrum::OceanConfig::default())?;
    let shore = Shore::new(gpu, &ocean);
    let source = format!(
        "{}\n{}\n{}",
        include_str!("shore.wgsl"),
        include_str!("shore_compute.wgsl"),
        r#"
@compute @workgroup_size(8)
fn generation_probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let samples=array<f32,8>(0.0,0.05,0.1,0.25,0.45,0.65,1.0,4.0);
    if id.x>=8u { return; }
    new_shore[id.x]=vec4(samples[id.x],foam_breaking_weight(samples[id.x]),foam_velocity_weight(samples[id.x]),0.0);
}"#
    );
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("foam generation control probe"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("foam generation control probe"),
            layout: None,
            module: &shader,
            entry_point: Some("generation_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
        ],
    });
    for (threshold, transition) in [
        (0.25_f32, 0.4_f32),
        (0.05, 0.2),
        (0.25, 0.0),
        (0.0, 0.0),
        (2.0, 2.0),
    ] {
        let mut optics = OpticsConfig::beach_scene();
        optics.shore.foam_generation_threshold = threshold;
        optics.shore.foam_generation_transition = transition;
        optics.shore.foam_velocity_threshold = threshold;
        optics.shore.foam_velocity_transition = transition;
        optics.validate()?;
        let params = shore.parameters(&optics, 0.0, false, 1.0);
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&params));
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
        let bytes = gpu.read_buffer(&shore.field, 8 * 16)?;
        for b in bytes.as_chunks::<16>().0 {
            let sample: [f32; 4] = bytemuck::pod_read_unaligned(b);
            let expected = if transition == 0.0 {
                f32::from(sample[0] > threshold)
            } else {
                let t = ((sample[0] - threshold) / transition).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            if sample[1..=2]
                .iter()
                .any(|v| !v.is_finite() || (*v - expected).abs() > 1e-6)
            {
                return Err(format!("foam generation threshold={threshold} width={transition}: {sample:?} expected={expected}").into());
            }
        }
    }
    eprintln!(
        "Foam generation controls: rise/velocity thresholds, transition width, hard cutoff and quiet water OK"
    );
    Ok(())
}

/// Probe directional compression and source integration using analytic flows.
fn verify_front_foam(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(gpu, super::spectrum::OceanConfig::default())?;
    let shore = Shore::new(gpu, &ocean);
    let shader = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("front foam probe"),
        source: wgpu::ShaderSource::Wgsl(format!("{}\n{}\n{}",
            include_str!("shore.wgsl"), include_str!("shore_compute.wgsl"),
            "@compute @workgroup_size(1) fn front_probe() { new_shore[0]=vec4(foam_front_weight(vec2<i32>(i32(shore.visual.y),96)),0.0,0.0,0.0); }").into()),
    });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("front foam probe"),
            layout: None,
            module: &shader,
            entry_point: Some("front_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shore.previous.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
        ],
    });
    let n = RESOLUTION as usize;
    let cells = n * n;
    // Analytic linear flows distinguish catch-up, separation, reversal and shear.
    for (case, (flow, gradient, spacing, distance, strength)) in [
        (
            [2.0_f32, 0.0],
            [-0.4_f32, 0.0],
            [0.25_f32, 1.5],
            1.0_f32,
            0.4_f32,
        ),
        ([2.0, 0.0], [0.4, 0.0], [0.25, 1.5], 1.0, 0.0),
        ([2.0, 0.0], [-4.0, 0.0], [0.25, 1.5], 1.0, 4.0),
        ([2.0, 0.0], [0.0, 0.4], [0.25, 1.5], 1.0, 0.0),
        ([-2.0, 0.0], [0.4, 0.0], [0.25, 1.5], 1.0, 0.4),
        ([0.0, 2.0], [0.0, -0.4], [0.25, 1.5], 1.0, 0.4),
        ([1.2, 1.6], [-0.24, -0.32], [0.25, 1.5], 1.0, 0.4),
        ([1.2, 1.6], [-0.24, -0.32], [0.5, 0.75], 1.0, 0.4),
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 0.5, 0.4),
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 2.0, 0.4),
        ([2.0, 0.0], [0.0, 0.0], [0.25, 1.5], 1.0, 0.0),
        ([0.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 1.0, 0.0),
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 1.0, 0.0), // Dry ahead.
        ([2.0, 0.0], [-0.4, 0.0], [0.25, 1.5], 1.0, 0.0), // Patch edge.
    ]
    .into_iter()
    .enumerate()
    {
        let center_x = if case == 13 { n - 1 } else { 96 };
        let center_flow = glam::Vec2::from_array(flow);
        let direction = center_flow.normalize_or_zero();
        let mut seed = vec![[0.0_f32; 4]; 5 * cells];
        for z in 0..n {
            for x in 0..n {
                let offset = glam::vec2(
                    (x as f32 - center_x as f32) * spacing[0],
                    (z as f32 - 96.0) * spacing[1],
                );
                let along = offset.dot(direction);
                let v = center_flow + glam::Vec2::from_array(gradient) * along;
                let h = if case == 12 && x > center_x { 0.0 } else { 2.0 };
                seed[cells + z * n + x] = [h, h * v.x, h * v.y, 0.0];
            }
        }
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, n as f32 * spacing[0], n as f32 * spacing[1]];
        p.terrain[1] = 0.0;
        p.terrain[2] = 0.0;
        p.foam_front = [8.0, distance, 0.0, 0.0];
        p.visual[1] = center_x as f32;
        p.options[0] = 0.0;
        p.options[3] = 0.0;
        p.timing[1] = 0.0001;
        p.timing[2] = 0.0;
        let mut water_by_order = [[0.0_f32; 3]; 2];
        for enabled in [false, true] {
            p.foam_front[0] = if enabled { 8.0 } else { 0.0 };
            p.foam_velocity = [0.0, 0.1, 0.5, 0.0];
            let strength = if enabled { strength } else { 0.0 };
            let t = ((strength - 0.1) / 0.5).clamp(0.0, 1.0);
            let expected = t * t * (3.0 - 2.0 * t);
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            gpu.queue
                .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            gpu.queue.submit([enc.finish()]);
            let bytes = gpu.read_buffer(&shore.field, 16)?;
            let result: [f32; 4] = bytemuck::pod_read_unaligned(&bytes);
            if !result[0].is_finite() || (result[0] - expected).abs() > 0.0001 {
                return Err(format!(
                    "compression front case {case} enabled {enabled}: {} != {expected}",
                    result[0]
                )
                .into());
            }
            // Dry/boundary/zero-flow guards are sampled above. For wet moving
            // cases verify the real source is added once, for both RK variants.
            if case >= 11 {
                continue;
            }
            for order in [1, 2] {
                gpu.queue
                    .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
                p.solver[0] = order as f32;
                for stage in 0..order {
                    p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
                    gpu.queue
                        .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                    let mut enc = gpu.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = enc.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&shore.pipeline);
                        pass.set_bind_group(
                            0,
                            if order == 1 {
                                &shore.bind
                            } else if stage == 0 {
                                &shore.predictor_bind
                            } else {
                                &shore.corrector_bind
                            },
                            &[],
                        );
                        pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                    }
                    gpu.queue.submit([enc.finish()]);
                }
                let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
                let i = cells + 96 * n + center_x;
                let q: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[16 * i..16 * (i + 1)]);
                // RK2 samples the predictor flow. At this small dt its source
                // differs from the analytic initial flow by less than 1e-5.
                if q.iter().any(|v| !v.is_finite())
                    || (q[3] - 8.0 * expected * p.timing[1]).abs() > 0.00001
                {
                    return Err(format!(
                        "compression source case {case} enabled {enabled} order {order}: {q:?}"
                    )
                    .into());
                }
                if !enabled {
                    water_by_order[order - 1].copy_from_slice(&q[..3]);
                } else if q[..3] != water_by_order[order - 1] {
                    return Err(format!(
                        "front foam changed water state in case {case}, order {order}"
                    )
                    .into());
                }
            }
        }
    }
    eprintln!(
        "Front foam: signed catch-up/collision vs separation/shear, distance/grid normalization, wet/dry/edge guards, source integration and unchanged water in both orders OK"
    );
    Ok(())
}

/// Exercise production surface flux with sources disabled: mass conservation,
/// translation, convergence, expansion and decay, for both SWE integration modes.
fn verify_surface_foam(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let n = RESOLUTION as usize;
    let cells = n * n;
    let dt = 0.004_f32;
    for (order, space_order) in [(1, 1), (2, 1), (2, 2)] {
        for mode in 0..7 {
            let shore = Shore::new(gpu, &ocean);
            let mut seed = vec![[0.0_f32; 4]; cells * 5];
            for y in 0..n {
                for x in 0..n {
                    let i = y * n + x;
                    let u = match mode {
                        0 | 4 => 1.0,
                        1 => (24.0 - (x as f32 + 0.5) * 0.25) * 0.1,
                        2 => ((x as f32 + 0.5) * 0.25 - 24.0) * 0.1,
                        _ => 0.0,
                    };
                    let density = if (64..128).contains(&x) && (64..128).contains(&y) {
                        0.5
                    } else {
                        0.0
                    };
                    let depth = if mode == 5 {
                        0.002
                    } else if mode == 6 {
                        0.01
                    } else if mode == 4 {
                        0.2
                    } else {
                        2.0
                    };
                    seed[i] = [density, 1.0, depth, u];
                    seed[cells + i] = [depth, depth * u, 0.0, density];
                    seed[2 * cells + i] = [0.0, density, 2.0, 0.0];
                }
            }
            gpu.queue
                .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
            let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
            p.terrain[1] = 0.0;
            p.domain = [0.0, 0.0, 64.0, 64.0];
            p.foam_motion = [1.0, 0.0, 0.0, 0.0];
            p.foam_inertia[2] = space_order as f32;
            p.options[0] = 0.0;
            p.foam_front[0] = 0.0;
            p.options[3] = 0.0;
            p.timing[1] = dt;
            p.timing[2] = if mode == 3 { 0.7 } else { 0.0 };
            p.solver[0] = order as f32;
            for stage in 0..order {
                p.solver[1] = if order == 2 { (stage + 1) as f32 } else { 0.0 };
                gpu.queue
                    .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&shore.pipeline);
                    pass.set_bind_group(
                        0,
                        if order == 1 {
                            &shore.bind
                        } else if stage == 0 {
                            &shore.predictor_bind
                        } else {
                            &shore.corrector_bind
                        },
                        &[],
                    );
                    pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                }
                gpu.queue.submit([enc.finish()]);
            }
            let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
            let data: Vec<[f32; 4]> = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|b| bytemuck::pod_read_unaligned(b))
                .collect();
            let total: f64 = data[cells..2 * cells].iter().map(|q| q[3] as f64).sum();
            let expected = 2048.0
                * if mode == 3 {
                    (-0.7 * dt).exp() as f64
                } else if mode == 5 {
                    0.0
                } else {
                    1.0
                };
            let center = data[cells + 96 * n + 96][3];
            if data.iter().flatten().any(|v| !v.is_finite())
                || data[cells..2 * cells].iter().any(|q| q[3] < 0.0)
                || (total - expected).abs() > 0.002
                || ((mode == 0 || mode == 4)
                    && (data[cells + 96 * n + 128][3] <= 0.0 || (center - 0.5).abs() > 1e-6))
                || (mode == 5
                    && (data[2 * cells + 96 * n + 96][..2] != [0.0, 0.0]
                        || (data[96 * n + 96][1] - (-dt / p.timing[3]).exp()).abs() > 1e-6))
                || (mode == 6 && (center - 0.5).abs() > 1e-6)
                || (mode == 1 && center <= 0.5)
                || (mode == 2 && center >= 0.5)
                || (mode == 3 && (center - 0.5 * (-0.7 * dt).exp()).abs() > 1e-6)
            {
                return Err(format!("surface foam time {order} space {space_order} mode {mode}: total {total}, expected {expected}, center {center}").into());
            }
        }
    }
    eprintln!(
        "Surface foam: Euler/RK2 and both spatial orders conserve area amount, transport, concentrate/dilute and decay independently of depth; dry-cell removal OK"
    );
    Ok(())
}

/// Prescribed water flow isolates production foam flux/motion from water dynamics.
/// Check material displacement against analytic trajectories, not texture phase.
fn verify_foam_motion(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    let ocean = Ocean::new(gpu, super::spectrum::OceanConfig::default())?;
    let shore = Shore::new(gpu, &ocean);
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("foam motion trajectory probe"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("shore.wgsl"),
                    include_str!("shore_compute.wgsl"),
                    r#"
@compute @workgroup_size(8,8)
fn motion_probe(@builtin(global_invocation_id) id: vec3<u32>) {
    let n=u32(shore.options.z); if id.x>=n || id.y>=n { return; }
    let cells=n*n; let index=id.y*n+id.x; let c=vec2<i32>(id.xy);
    if shore.foam_motion.w>0.5 {
        new_shore[index]=sample_initial_texture(vec2(shore.foam_motion.z,96.0)); return;
    }
    let original=state(c); var q=original;
    let foam=foam_sources(transport_foam(c),velocity(q),0.0);
    q.w=foam.x;
    new_shore[index]=vec4(q.w,1.0,q.x,velocity(q).x);
    new_shore[cells+index]=q;
    new_shore[2u*cells+index]=initial_shore[2u*cells+index];
    let motion=advance_foam_motion(c,q,original,foam.yz);
    new_shore[3u*cells+index]=motion.motion;
    new_shore[4u*cells+index]=motion.texture;
}"#
                )
                .into(),
            ),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &shader,
            entry_point: Some("motion_probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: shore.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: shore.previous.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: shore.field.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: shore.previous.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: shore.terrain_bed.as_entire_binding(),
            },
        ],
    });
    let n = RESOLUTION as usize;
    let cells = n * n;
    // Follow, response, water speed, initial foam speed, stretching, dt, steps, reset period.
    for (case, (follow, response, water, initial, stretch, dt, steps, period)) in [
        (
            0.5_f32, 0.0_f32, 1.0_f32, 0.0_f32, false, 0.02_f32, 120, 2.0_f32,
        ),
        (0.0, 0.0, 1.0, 0.0, false, 0.02, 120, 2.0),
        (0.8, 0.35, 1.0, 0.0, false, 0.02, 120, 2.0),
        (0.8, 0.35, -1.0, 0.8, false, 0.02, 120, 2.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 120, 2.0),
        (0.8, 0.35, 1.0, 0.0, false, 0.01, 240, 2.0),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020, 2.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020, 2.0),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020, 8.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020, 8.0),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020, 0.0),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020, 0.0),
    ]
    .into_iter()
    .enumerate()
    {
        let mut seed = vec![[0.0_f32; 4]; 5 * cells];
        for z in 0..n {
            for x in 0..n {
                let i = z * n + x;
                let u = if stretch {
                    (x as f32 - 96.0) * 0.25 * 0.1
                } else {
                    water
                };
                let density = if (64..128).contains(&x) && (64..128).contains(&z) {
                    0.5
                } else {
                    0.0
                };
                seed[i] = [density, 1.0, 2.0, u];
                seed[cells + i] = [2.0, 2.0 * u, 0.0, density];
                seed[2 * cells + i] = [0.0, 0.0, 2.0, 0.0];
                seed[3 * cells + i] = [initial * density, 0.0, 0.0, 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, 64.0, 64.0];
        p.timing[1] = dt;
        p.foam_motion = [follow, response, 0.0, 0.0];
        p.foam_shading[3] = period;
        p.timing[2] = 0.0;
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
        for step in 0..steps {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
            }
            enc.copy_buffer_to_buffer(&shore.field, 0, &shore.previous, 0, shore.field.size());
            gpu.queue.submit([enc.finish()]);
            if case == 3 && step == 0 {
                let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
                let i = 3 * cells + 96 * n + 96;
                let m: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[16 * i..16 * (i + 1)]);
                if m[0] <= 0.0 || m[2] <= 0.0 {
                    return Err("foam reversal lost initial inertia".into());
                }
            }
        }
        let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
        let data: Vec<[f32; 4]> = bytes
            .as_chunks::<16>()
            .0
            .iter()
            .map(|b| bytemuck::pod_read_unaligned(b))
            .collect();
        let mass: f64 = data[cells..2 * cells].iter().map(|q| q[3] as f64).sum();
        if data.iter().flatten().any(|v| !v.is_finite()) || (mass - 2048.0).abs() > 0.02 {
            return Err(format!("foam motion case {case} mass/finite failed: {mass}").into());
        }
        let time = dt * steps as f32;
        let mid = 96 * n + 96;
        let motion = data[3 * cells + mid];
        let texture = data[4 * cells + mid];
        let age_a = if period == 0.0 { time } else { time % period };
        let age_b = if period == 0.0 || time < period * 0.5 {
            time
        } else {
            (time + period * 0.5) % period
        };
        let expected_phase = if period == 0.0 { 0.0 } else { age_a / period };
        if (texture[2] - expected_phase).abs() > 0.0001 {
            return Err(format!(
                "foam texture phase does not track simulated time: {} vs {time}",
                texture[2]
            )
            .into());
        }
        if stretch {
            let material_span = 2.0 - (data[3 * cells + mid + 4][2] - data[3 * cells + mid - 4][2]);
            let span_b = 2.0 - (data[4 * cells + mid + 4][0] - data[4 * cells + mid - 4][0]);
            if (material_span / 2.0 - (-0.1 * age_a).exp()).abs() > 0.002
                || (span_b / 2.0 - (-0.1 * age_b).exp()).abs() > 0.002
                || (data[cells + mid][3] / 0.5 - (-0.1 * time).exp()).abs() > 0.002
            {
                return Err(format!(
                    "foam stretch: material span {material_span}, density {}",
                    data[cells + mid][3]
                )
                .into());
            }
        } else {
            let target = follow * water;
            let (expected_velocity, expected_distance) = if response == 0.0 {
                (target, target * time)
            } else {
                let attenuation = (-time / response).exp();
                (
                    target + (initial - target) * attenuation,
                    target * time + (initial - target) * response * (1.0 - attenuation),
                )
            };
            let center: f64 = data[cells..2 * cells]
                .iter()
                .enumerate()
                .map(|(i, q)| (i % n) as f64 * 0.25 * q[3] as f64)
                .sum::<f64>()
                / mass;
            let density_distance = center - 95.5 * 0.25;
            let layer_distance = |age: f32| {
                target * age
                    + if response == 0.0 {
                        0.0
                    } else {
                        (initial - target)
                            * response
                            * ((-(time - age) / response).exp() - (-time / response).exp())
                    }
            };
            if (motion[0] / data[cells + mid][3] - expected_velocity).abs() > 0.001
                || (motion[2] - layer_distance(age_a)).abs() > 0.002
                || (texture[0] - layer_distance(age_b)).abs() > 0.002
                || (density_distance - expected_distance as f64).abs() > 0.002
            {
                return Err(format!("foam motion {case}: velocity {}, texture {}, density {density_distance}; expected {expected_velocity}, {expected_distance}",motion[0],motion[2]).into());
            }
        }
    }
    // Open boundaries export density AND momentum, never import foam. The
    // independent reference is uniform density * outward velocity * edge length
    // * dt (divided by cell area here, since we sum cell densities).
    // Closed-domain tests above cannot detect a sign error on a negative edge.
    gpu.queue.write_buffer(
        &shore.terrain_bed,
        0,
        bytemuck::cast_slice(&vec![[0.0_f32, 1.0]; cells]),
    );
    for space_order in [1, 2] {
        for edge in 0..4 {
            for (open, outward) in [(false, true), (true, true), (true, false)] {
                let axis = edge / 2;
                let mut speed = [0.25_f32; 2];
                speed[axis] = if (edge % 2 == 0) == outward {
                    -1.0
                } else {
                    1.0
                };
                let mut seed = vec![[0.0_f32; 4]; 5 * cells];
                for i in 0..cells {
                    seed[cells + i] = [2.0, 0.0, 0.0, 0.5];
                    seed[3 * cells + i] = [0.5 * speed[0], 0.5 * speed[1], 0.0, 0.0];
                }
                let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
                p.domain = [0.0, 0.0, 64.0, 64.0];
                p.model[0] = 1.0;
                p.edges = [0.0; 4];
                p.edges[edge] = f32::from(open);
                p.foam_inertia = [0.0, 1.0, space_order as f32, 0.0];
                p.timing[1] = 0.02;
                p.timing[2] = 0.0;
                gpu.queue
                    .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
                gpu.queue
                    .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
                let mut enc = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&pipeline);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(RESOLUTION / 8, RESOLUTION / 8, 1);
                }
                gpu.queue.submit([enc.finish()]);
                let bytes = gpu.read_buffer(&shore.field, shore.field.size())?;
                let data: Vec<[f32; 4]> = bytes
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(|b| bytemuck::pod_read_unaligned(b))
                    .collect();
                let lost = if open && outward {
                    0.5 * n as f64 * f64::from(p.timing[1]) / (64.0 / n as f64)
                } else {
                    0.0
                };
                let expected = 0.5 * cells as f64 - lost;
                let amount: f64 = data[cells..2 * cells].iter().map(|q| f64::from(q[3])).sum();
                let momentum: [f64; 2] = std::array::from_fn(|a| {
                    data[3 * cells..4 * cells]
                        .iter()
                        .map(|m| f64::from(m[a]))
                        .sum()
                });
                if data.iter().flatten().any(|v| !v.is_finite())
                    || data[cells..2 * cells].iter().any(|q| q[3] < 0.0)
                    || (amount - expected).abs() > 0.003
                    || (0..2).any(|a| (momentum[a] - expected * f64::from(speed[a])).abs() > 0.003)
                {
                    return Err(format!("foam boundary space {space_order}, edge {edge}, open {open}, outward {outward}: amount {amount} vs {expected}, momentum {momentum:?}").into());
                }
            }
        }
    }
    eprintln!(
        "Foam boundaries: both spatial orders, four edges, closed conservation, outward amount/momentum flux and no inward injection OK"
    );
    // A wet donor retains its coordinates next to dry/outside samples, rather
    // than interpolating toward cleared zero displacement. All-dry is finite.
    let mut seed = vec![[0.0_f32; 4]; 5 * cells];
    let donor = 96 * n + 64;
    seed[cells + donor][0] = 2.0;
    seed[3 * cells + donor] = [0.0, 0.0, 3.0, -2.0];
    seed[4 * cells + donor] = [5.0, -4.0, 0.0, 0.0];
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    for (x, expected) in [(63.75, [3.0, -2.0, 5.0, -4.0]), (62.25, [0.0; 4])] {
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.foam_motion = [0.8, 0.35, x, 1.0];
        gpu.queue
            .write_buffer(&shore.uniform, 0, bytemuck::bytes_of(&p));
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        gpu.queue.submit([enc.finish()]);
        let bytes = gpu.read_buffer(&shore.field, 16)?;
        let actual: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[..16]);
        if actual != expected {
            return Err(
                format!("foam wet/dry texture interpolation: {actual:?} vs {expected:?}").into(),
            );
        }
    }
    eprintln!(
        "Foam motion: follow/stop/reversal, density transport independent of two staggered texture resets, 20 s bounded stretch, wet/dry interpolation and timestep consistency OK"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn removed_flow_source_fields_do_not_discard_saved_foam_settings() {
        let saved: super::ShoreConfig = serde_json::from_str(
            r#"{"generation":2.5,"foam_front_generation":7.0,"foam_source_mode":1,"foam_flow_slope_gain":12.0,"foam_flow_speed_gain":0.7,"foam_flow_compression_gain":1.5,"foam_flow_bias":-0.3,"foam_flow_depth_mm":125.0}"#,
        )
        .unwrap();
        saved.validate().unwrap();
        assert_eq!(saved.generation, 2.5);
        assert_eq!(saved.foam_front_generation, 7.0);
    }

    #[test]
    fn foam_material_legacy_defaults_and_invalid_inputs() {
        let old: super::ShoreConfig = serde_json::from_str(
            r#"{"generation":2.5,"foam_soft_opacity":0.4,"foam_thick_start":0.6,"foam_material_view":4}"#,
        ).unwrap();
        old.validate().unwrap();
        assert_eq!(old.generation, 2.5);
        assert_eq!(old.foam_opacity_scale, 2.0);
        assert_eq!(old.foam_material_view, 4);
        assert_eq!(old.foam_texture, 1);
        assert_eq!(old.foam_texture_size, 12.0);
        assert!(!old.foam_prototype);
        assert_eq!(old.foam_texture_period, 2.0);
        for period in [0.0, 0.25, 8.0, 60.0] {
            let config = super::ShoreConfig {
                foam_prototype: true,
                foam_texture_period: period,
                foam_material_view: 5,
                ..old
            };
            config.validate().unwrap();
            let restored: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(config, restored);
        }
        for period in [-1.0, 0.01, 60.1, f32::NAN, f32::INFINITY] {
            assert!(
                super::ShoreConfig {
                    foam_texture_period: period,
                    ..old
                }
                .validate()
                .is_err()
            );
        }
        for source in 0..super::FOAM_TEXTURE_NAMES.len() as u32 {
            let config = super::ShoreConfig {
                foam_texture: source,
                foam_material_view: 4,
                ..old
            };
            config.validate().unwrap();
            let loaded: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(config, loaded);
        }
        for config in [
            super::ShoreConfig {
                foam_texture: 4,
                ..old
            },
            super::ShoreConfig {
                foam_material_view: 6,
                ..old
            },
            super::ShoreConfig {
                foam_texture_size: 0.0,
                ..old
            },
            super::ShoreConfig {
                foam_micro_normal: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: f32::INFINITY,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: 0.0,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: f32::NAN,
                ..old
            },
            super::ShoreConfig {
                foam_opacity_scale: 20.01,
                ..old
            },
            super::ShoreConfig {
                foam_roughness: 0.0,
                ..old
            },
        ] {
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn resolution_config_compatibility_and_validation() {
        let old: super::ShoreConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(old.resolution, 256);
        for resolution in super::RESOLUTIONS {
            let config = super::ShoreConfig { resolution, ..old };
            config.validate().unwrap();
            let restored: super::ShoreConfig =
                serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
            assert_eq!(restored, config);
        }
        for resolution in [0, 63, 192, 2048, u32::MAX] {
            assert!(super::ShoreConfig { resolution, ..old }.validate().is_err());
        }
    }
    use super::*;

    #[test]
    fn foam_height_settings_keep_legacy_configs_and_validate_boundaries() {
        let legacy: ShoreConfig =
            serde_json::from_str(r#"{"foam_density_reference":3.0}"#).unwrap();
        assert_eq!(legacy.foam_height_max, 0.05);
        assert_eq!(legacy.foam_height_threshold, 0.1);
        assert_eq!(legacy.foam_height_transition, 1.9);
        for (maximum, threshold, transition) in [(0.0, 0.0, 0.001), (1.0, 20.0, 20.0)] {
            let config = ShoreConfig {
                foam_height_max: maximum,
                foam_height_threshold: threshold,
                foam_height_transition: transition,
                ..legacy
            };
            config.validate().unwrap();
            let json = serde_json::to_string(&config).unwrap();
            assert_eq!(serde_json::from_str::<ShoreConfig>(&json).unwrap(), config);
        }
        for (maximum, threshold, transition) in [
            (-0.01, 0.1, 1.9),
            (1.01, 0.1, 1.9),
            (f32::NAN, 0.1, 1.9),
            (0.05, -0.1, 1.9),
            (0.05, 20.1, 1.9),
            (0.05, f32::INFINITY, 1.9),
            (0.05, 0.1, 0.0),
            (0.05, 0.1, 20.1),
            (0.05, 0.1, f32::NAN),
        ] {
            assert!(
                ShoreConfig {
                    foam_height_max: maximum,
                    foam_height_threshold: threshold,
                    foam_height_transition: transition,
                    ..legacy
                }
                .validate()
                .is_err()
            );
        }
    }
    #[test]
    fn foam_momentum_controls_are_compatible_and_persist() {
        let mut config: ShoreConfig = serde_json::from_str(r#"{"foam_follow":0.6}"#).unwrap();
        assert!(config.foam_drag_enabled);
        assert_eq!(config.foam_birth_speed, 1.0);
        assert_eq!(config.foam_follow, 0.6);
        assert_eq!(config.foam_transport_order, 1);
        config.foam_transport_order = 2;
        config.apply_foam_reference();
        assert_eq!(config.foam_transport_order, 2);
        config.validate().unwrap();
        config.foam_drag_enabled = false;
        config.foam_response_time = 20.0;
        config.foam_birth_speed = 0.4;
        config.validate().unwrap();
        let saved = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<ShoreConfig>(&saved).unwrap(), config);
        for order in [0, 3, u32::MAX] {
            assert!(
                ShoreConfig {
                    foam_transport_order: order,
                    ..config
                }
                .validate()
                .is_err()
            );
        }
        for value in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            config.foam_birth_speed = value;
            assert!(config.validate().is_err());
        }
    }
    #[test]
    fn removed_front_modes_preserve_saved_compression_controls() {
        for mode in 0..3 {
            let config: ShoreConfig = serde_json::from_value(serde_json::json!({
                "foam_front_mode": mode,
                "foam_front_depth_drop": 0.02,
                "foam_front_generation": 6.0,
                "foam_front_distance": 0.7,
                "foam_velocity_threshold": 0.35,
                "foam_velocity_transition": 1.2
            }))
            .unwrap();
            config.validate().unwrap();
            assert_eq!(config.foam_front_generation, 6.0);
            assert_eq!(config.foam_front_distance, 0.7);
            assert_eq!(config.foam_velocity_threshold, 0.35);
            assert_eq!(config.foam_velocity_transition, 1.2);
            let saved = serde_json::to_value(config).unwrap();
            assert!(saved.get("foam_front_mode").is_none());
            assert!(saved.get("foam_front_depth_drop").is_none());
            assert_eq!(
                serde_json::from_value::<ShoreConfig>(saved).unwrap(),
                config
            );
        }
    }
    #[test]
    fn removed_sand_foam_settings_preserve_wetness_and_density_view() {
        for (view, expected) in [(5, 0), (6, 6)] {
            let config: ShoreConfig = serde_json::from_value(serde_json::json!({
                "view": view, "drying_time": 27.0,
                "stranded_foam_color": [0.6, 0.4, 0.2], "foam_deposit_depth_mm": 50.0
            }))
            .unwrap();
            config.validate().unwrap();
            assert_eq!(config.view, expected);
            assert_eq!(config.drying_time, 27.0);
            let saved = serde_json::to_value(config).unwrap();
            assert!(saved.get("stranded_foam_color").is_none());
            assert!(saved.get("foam_deposit_depth_mm").is_none());
            assert_eq!(
                serde_json::from_value::<ShoreConfig>(saved).unwrap(),
                config
            );
        }
    }
    #[test]
    fn removed_gain_settings_preserve_other_controls() {
        let config: ShoreConfig = serde_json::from_str(
            r#"{"foam_front_gain_points":[0,1.2,1.62,2.5],"foam_front_power":2,"foam_front_generation":3.5,"foam_background_multiplier":0.05}"#,
        ).unwrap();
        config.validate().unwrap();
        assert_eq!(config.foam_front_generation, 3.5);
        assert_eq!(config.foam_background_multiplier, 0.05);
        let saved = serde_json::to_string(&config).unwrap();
        assert!(!saved.contains("foam_front_gain_points"));
        assert!(!saved.contains("foam_front_power"));
        assert_eq!(serde_json::from_str::<ShoreConfig>(&saved).unwrap(), config);
    }
    #[test]
    fn legacy_settings_get_valid_shore_defaults() {
        let optics: OpticsConfig = serde_json::from_str(
            r#"{"beach":true,"beach_slope":0.18,"shore":{"amplitude":1.3,"period":8.0}}"#,
        )
        .unwrap();
        assert_eq!(optics.shore, ShoreConfig::default());
        optics.validate().unwrap();
    }
    #[test]
    fn rejects_singular_and_nonfinite_controls() {
        for c in [
            ShoreConfig {
                foam_velocity_threshold: -0.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_velocity_threshold: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_velocity_transition: f32::INFINITY,
                ..Default::default()
            },
            ShoreConfig {
                foam_velocity_transition: 20.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_follow: 1.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_response_time: -0.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_response_time: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                input_gain: 3.1,
                ..Default::default()
            },
            ShoreConfig {
                input_gain: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                order: 3,
                ..Default::default()
            },
            ShoreConfig {
                filter_cutoff: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                filter_transition: -1.0,
                ..Default::default()
            },
            ShoreConfig {
                breaking_depth: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                drying_time: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                foam_generation_threshold: -0.01,
                ..Default::default()
            },
            ShoreConfig {
                foam_generation_transition: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_front_generation: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_front_distance: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                foam_ridge_width: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_ridge_prominence: f32::INFINITY,
                ..Default::default()
            },
            ShoreConfig {
                foam_background_multiplier: 1.01,
                ..Default::default()
            },
            ShoreConfig {
                generation: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_density_reference: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                foam_compression_threshold: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_fresh_half_life: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                foam_residual_half_life: f32::INFINITY,
                ..Default::default()
            },
            ShoreConfig {
                foam_conversion: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                view: 11,
                ..Default::default()
            },
        ] {
            assert!(c.validate().is_err());
        }
    }
}

//! Local hydrostatic-reconstruction SWE, foam transport and ground memory.
use crate::ocean::gpu::context::Gpu;
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::simulation::offshore::Ocean;
use wgpu::util::DeviceExt;

pub use crate::ocean::parameters::shore::*;
mod uniform;
pub use uniform::ShoreUniform;
mod verification;
pub use verification::{verify, verify_terrain};

pub struct Shore {
    pub resolution: u32,
    pub uniform: wgpu::Buffer,
    pub field: wgpu::Buffer,
    pub lace: wgpu::Buffer,
    pub terrain_bed: wgpu::Buffer,
    terrain_grid: Option<crate::ocean::scene::terrain::Grid>,
    terrain_key: Option<(crate::ocean::parameters::model::ModelConfig, f32)>,
    previous: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    shader: wgpu::ShaderModule,
    solver_pipelines: [Option<wgpu::ComputePipeline>; 3],
    collider_edit_pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    predictor_bind: wgpu::BindGroup,
    corrector_bind: wgpu::BindGroup,
    reverse_bind: wgpu::BindGroup,
    reverse_predictor_bind: wgpu::BindGroup,
    reverse_corrector_bind: wgpu::BindGroup,
    pub substeps: u32,
    pub simulated_dt: f32,
    pub spray: Option<crate::ocean::simulation::spray::SpraySource>,
    pub spray_epoch: u64,
    pub spray_error: Option<String>,
    pub timer: Option<crate::ocean::gpu::timer::GpuTimer>,
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
                    include_str!("shaders/shore.wgsl"),
                    include_str!("shaders/shore_compute.wgsl")
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
        let make_bind = |input: &wgpu::Buffer, output: &wgpu::Buffer, initial: &wgpu::Buffer| {
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
                        resource: initial.as_entire_binding(),
                    },
                ],
            })
        };
        let bind = make_bind(&previous, &field, &previous);
        let predictor_bind = make_bind(&previous, &stage, &previous);
        let corrector_bind = make_bind(&stage, &field, &previous);
        let reverse_bind = make_bind(&field, &previous, &field);
        let reverse_predictor_bind = make_bind(&field, &stage, &field);
        let reverse_corrector_bind = make_bind(&stage, &previous, &field);
        let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
            "../../../../assets/shorewaves/voronoi_lace.png"
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
            reverse_bind,
            reverse_predictor_bind,
            reverse_corrector_bind,
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
        source: Option<&crate::ocean::scene::terrain::HeightField>,
    ) -> Result<(), crate::ocean::Error> {
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
        let (mut colliders, mut collider_count) = crate::ocean::scene::collider::packed(
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
            match crate::ocean::simulation::spray::SpraySource::new(gpu, self, &self.previous) {
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
                let reverse = (i / stages) % 2 == 1;
                let bind = match (stages, i % stages, reverse) {
                    (1, _, false) => &self.bind,
                    (1, _, true) => &self.reverse_bind,
                    (_, 0, false) => &self.predictor_bind,
                    (_, 0, true) => &self.reverse_predictor_bind,
                    (_, _, false) => &self.corrector_bind,
                    (_, _, true) => &self.reverse_corrector_bind,
                };
                pass.set_bind_group(0, bind, &[]);
                pass.dispatch_workgroups(
                    self.resolution.div_ceil(8),
                    self.resolution.div_ceil(8),
                    1,
                );
            }
            if (i + 1) % stages == 0
                && step.options[3] == 0.0
                && let Some(source) = &self.spray
            {
                source.encode_substep_direction(
                    encoder,
                    (i / stages) % 2 == 1,
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
        }
        // Keep both public/current and initial history synchronized at frame boundaries.
        // Within a frame the full-step outputs alternate; the RK predictor stays separate.
        let (from, to) = if count % 2 == 1 {
            (&self.field, &self.previous)
        } else {
            (&self.previous, &self.field)
        };
        encoder.copy_buffer_to_buffer(from, 0, to, 0, self.field.size());
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

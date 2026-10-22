//! Local hydrostatic-reconstruction SWE, foam transport and ground memory.
use super::{
    gpu::{Gpu, Ocean},
    optics::OpticsConfig,
};
use wgpu::util::DeviceExt;

pub const RESOLUTION: u32 = 256;
pub const VIEW_NAMES: [&str; 7] = [
    "正常着色",
    "水深",
    "水流方向",
    "近岸泡沫",
    "沙地湿度",
    "沙地残留泡沫",
    "泡沫密度（低绿／高红）",
];

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShoreConfig {
    pub enabled: bool,
    pub order: u32,
    pub input_gain: f32,
    pub filter_enabled: bool,
    pub filter_cutoff: f32,
    pub filter_transition: f32,
    pub normal_strength: f32,
    pub water_foam_color: [f32; 3],
    pub stranded_foam_color: [f32; 3],
    pub breaking_depth: f32,
    pub generation: f32,
    pub foam_generation_threshold: f32,
    pub foam_generation_transition: f32,
    pub foam_front_generation: f32,
    pub foam_front_distance: f32,
    // Legacy serialized name retained; now measures free-surface elevation drop.
    pub foam_front_depth_drop: f32,
    pub foam_background_multiplier: f32,
    pub foam_follow: f32,
    pub foam_response_time: f32,
    pub decay: f32,
    pub foam_compression_threshold: f32,
    pub foam_density_reference: f32,
    pub foam_deposit_depth_mm: f32,
    pub drying_time: f32,
    pub view: u32,
}
impl Default for ShoreConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            order: 1,
            input_gain: 1.0,
            filter_enabled: true,
            filter_cutoff: 12.0,
            filter_transition: 12.0,
            normal_strength: 1.0,
            water_foam_color: [0.87, 0.92, 0.90],
            stranded_foam_color: [0.36, 0.50, 0.58],
            breaking_depth: 2.0,
            generation: 4.0,
            foam_generation_threshold: 0.25,
            foam_generation_transition: 0.4,
            foam_front_generation: 8.0,
            foam_front_distance: 1.0,
            foam_front_depth_drop: 0.03,
            foam_background_multiplier: 0.15,
            foam_follow: 0.8,
            foam_response_time: 0.35,
            decay: 0.45,
            foam_compression_threshold: 0.1,
            foam_density_reference: 2.0,
            foam_deposit_depth_mm: 3.0,
            drying_time: 18.0,
            view: 0,
        }
    }
}
impl ShoreConfig {
    pub fn validate(self) -> Result<(), String> {
        for (name, value, low, high) in [
            ("SWE input gain", self.input_gain, 0.0, 3.0),
            ("SWE filter cutoff", self.filter_cutoff, 0.0, 512.0),
            ("SWE filter transition", self.filter_transition, 0.0, 512.0),
            ("shore normal strength", self.normal_strength, 0.0, 3.0),
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
            ("shore foam decay", self.decay, 0.0, 4.0),
            ("foam follow multiplier", self.foam_follow, 0.0, 1.0),
            ("foam response time", self.foam_response_time, 0.0, 5.0),
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
            ("front surface drop", self.foam_front_depth_drop, 0.001, 1.0),
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
            (
                "foam deposit depth (mm)",
                self.foam_deposit_depth_mm,
                0.0,
                100.0,
            ),
            ("sand drying time", self.drying_time, 1.0, 120.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..={high}"));
            }
        }
        for color in [self.water_foam_color, self.stranded_foam_color] {
            if color
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("foam colors must be finite linear RGB in 0..=1".into());
            }
        }
        if !(1..=2).contains(&self.order) {
            return Err("SWE order must be 1 or 2".into());
        }
        if self.view >= VIEW_NAMES.len() as u32 {
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
    pub foam_motion: [f32; 4],
    pub water_foam_color: [f32; 4],
    pub stranded_foam_color: [f32; 4],
    pub options: [f32; 4],
}
pub struct Shore {
    pub uniform: wgpu::Buffer,
    pub field: wgpu::Buffer,
    pub lace: wgpu::Buffer,
    previous: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    predictor_bind: wgpu::BindGroup,
    corrector_bind: wgpu::BindGroup,
    pub substeps: u32,
    pub simulated_dt: f32,
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
                size: u64::from(RESOLUTION * RESOLUTION) * 80,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
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
            lace,
            uniform,
            field,
            previous,
            pipeline,
            bind,
            predictor_bind,
            corrector_bind,
            substeps: 0,
            simulated_dt: 0.0,
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
            domain: [
                optics.shore_x - inland,
                -self.length * 1.5,
                inland + offshore,
                self.length * 3.0,
            ],
            timing: [time, 0.0, c.decay, c.drying_time],
            coupling: [self.fft_resolution as f32, self.length, effective_gain, 1.0],
            filter: [
                f32::from(c.filter_enabled),
                c.filter_cutoff,
                c.filter_transition,
                0.0,
            ],
            solver: [c.order as f32, 0.0, gain, c.input_gain],
            visual: [c.normal_strength, 0.0, 0.0, 0.0],
            density: [
                c.foam_compression_threshold,
                c.foam_density_reference,
                c.foam_deposit_depth_mm * 0.001,
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
                c.foam_front_depth_drop,
                c.foam_background_multiplier,
            ],
            foam_motion: [c.foam_follow, c.foam_response_time, 0.0, 0.0],
            water_foam_color: [
                c.water_foam_color[0],
                c.water_foam_color[1],
                c.water_foam_color[2],
                1.0,
            ],
            stranded_foam_color: [
                c.stranded_foam_color[0],
                c.stranded_foam_color[1],
                c.stranded_foam_color[2],
                1.0,
            ],
            options: [c.generation, c.view as f32, RESOLUTION as f32, 0.0],
        };
        let changed = self.last.is_none_or(|old| {
            old.solver[0] != s.solver[0]
                || old.solver[2..] != s.solver[2..]
                || old.filter != s.filter
                || old.terrain != s.terrain
                || old.waves[2..] != s.waves[2..]
                || old.domain != s.domain
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
        if step.options[3] > 0.5 {
            self.start_time = time;
        }
        step.waves[1] = (time - self.start_time).max(0.0);
        if step.options[3] == 0.0
            && (step.timing[1] == 0.0 || !optics.beach || !optics.shore.enabled)
        {
            gpu.queue
                .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&step));
            self.last = Some(step);
            return;
        }
        // A conservative global CFL bound includes the enforced velocity cap.
        // Long UI stalls advance at most 0.1 s rather than risking an unstable step.
        let depth_bound = (step.domain[2] * step.terrain[1] + 4.0 * step.waves[0]).max(1.0);
        let speed = 12.0 + (9.81 * depth_bound).sqrt();
        let cell_x = step.domain[2] / RESOLUTION as f32;
        let cell_z = step.domain[3] / RESOLUTION as f32;
        let stable_dt = 0.2 / (speed / cell_x + speed / cell_z);
        let elapsed = step.timing[1].min(0.1).min(stable_dt * 128.0);
        let count = (elapsed / stable_dt).ceil().max(1.0) as u32;
        let dt = elapsed / count as f32;
        self.substeps = count;
        self.simulated_dt = elapsed;
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
        for i in 0..count * stages {
            encoder.copy_buffer_to_buffer(&upload, i as u64 * stride, &self.uniform, 0, stride);
            {
                let timestamps = if timed {
                    Some(wgpu::ComputePassTimestampWrites {
                        query_set: &self.timer.as_ref().unwrap().queries,
                        beginning_of_pass_write_index: if i == 0 { Some(0) } else { None },
                        end_of_pass_write_index: if i + 1 == count * stages {
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
                    label: Some("local SWE and stranded foam"),
                    timestamp_writes: timestamps.filter(|t| {
                        t.beginning_of_pass_write_index.is_some()
                            || t.end_of_pass_write_index.is_some()
                    }),
                });
                pass.set_pipeline(&self.pipeline);
                let bind = if stages == 1 {
                    &self.bind
                } else if i % 2 == 0 {
                    &self.predictor_bind
                } else {
                    &self.corrector_bind
                };
                pass.set_bind_group(0, bind, &[]);
                pass.dispatch_workgroups(RESOLUTION.div_ceil(8), RESOLUTION.div_ceil(8), 1);
            }
            if (i + 1) % stages == 0 {
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
    verify_inlet(gpu)?;
    verify_fft_initialization(gpu)?;
    verify_wave_accuracy(gpu)?;
    verify_surface_foam(gpu)?;
    verify_foam_motion(gpu)?;
    verify_foam_generation_controls(gpu)?;
    verify_front_foam(gpu)?;
    for order in [1, 2] {
        verify_order(gpu, order)?;
    }
    Ok(())
}

fn verify_order(gpu: &Gpu, order: u32) -> Result<(), super::gpu::Error> {
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
    // Ground memory: a dry cell deposits its remaining surface foam amount.
    let mut seed = still.clone();
    let dry = n / 8 + n / 2 * n;
    seed[dry] = [0.8, 1.0, 0.01, 0.0];
    seed[2 * cells + dry][1] = 0.8;
    seed[cells + dry][3] = 0.8;
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
    advance(&mut shore, &optics, 1.10, false, gain);
    let deposited = read(&shore)?;
    if deposited[2 * cells + dry][0] < 0.7 || deposited[dry][1] < 0.9 {
        return Err("SWE stranded foam deposition failed".into());
    }
    advance(&mut shore, &optics, 1.15, false, gain);
    let faded = read(&shore)?;
    if faded[2 * cells + dry][0] >= deposited[2 * cells + dry][0] {
        return Err("stranded foam did not fade".into());
    }
    let expected = deposited[2 * cells + dry][0] * (-0.05_f32 / 10.0).exp();
    if (faded[2 * cells + dry][0] - expected).abs() > 0.0001 {
        return Err("stranded foam decay is not in seconds".into());
    }
    let mut rewet = still.clone();
    rewet[2 * cells + center][0] = 0.8;
    gpu.queue
        .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&rewet));
    advance(&mut shore, &optics, 1.20, false, gain);
    let washed = read(&shore)?;
    if washed[2 * cells + center][0] >= 0.6 || washed[center][1] < 0.99 {
        return Err("rewetting did not wash away ground foam".into());
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
            "SWE order {order}: GPU {:.3} ms / {} substeps / {:.1} simulated ms",
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
        || !active[2 * cells..3 * cells].iter().any(|q| q[0] > 0.01)
    {
        return Err("SWE run-up: invalid state or missing water/stranded foam".into());
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
    optics.shore.foam_generation_threshold = 0.05;
    optics.shore.foam_generation_transition = 0.15;
    optics.shore.foam_front_generation = 5.0;
    optics.shore.foam_front_distance = 0.5;
    optics.shore.foam_front_depth_drop = 0.01;
    optics.shore.foam_follow = 0.6;
    optics.shore.foam_response_time = 0.7;
    optics.shore.foam_background_multiplier = 0.05;
    optics.shore.water_foam_color = [0.2, 0.7, 0.9];
    optics.shore.stranded_foam_color = [0.8, 0.3, 0.2];
    advance(&mut shore, &optics, 24.1, false, gain);
    if shore.last.unwrap().options[3] != 0.0 || read(&shore)? != before_normal_edit {
        return Err("live normal/color/foam-generation edit changed SWE history".into());
    }
    optics.shore.normal_strength = 1.0;
    optics.shore.foam_generation_threshold = 0.25;
    optics.shore.foam_generation_transition = 0.4;
    optics.shore.foam_front_generation = 8.0;
    optics.shore.foam_front_distance = 1.0;
    optics.shore.foam_front_depth_drop = 0.03;
    optics.shore.foam_follow = 0.8;
    optics.shore.foam_response_time = 0.35;
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
        "SWE GPU order {order}: lake at rest ({error:.2e}), pulse mass/2D foam transport, stranded deposition/decay/rewet, run-up, pause/reset/disable, terrain reset and extreme slopes OK"
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
    new_shore[id.x]=vec4(samples[id.x],foam_breaking_weight(samples[id.x]),0.0,0.0);
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
            if !sample[1].is_finite() || (sample[1] - expected).abs() > 1e-6 {
                return Err(format!("foam generation threshold={threshold} width={transition}: {sample:?} expected={expected}").into());
            }
        }
    }
    eprintln!(
        "Foam generation controls: default/lowered thresholds, transition width, hard cutoff and quiet water OK"
    );
    Ok(())
}

/// Probe directional surface sampling and actual source integration on wet ramps.
/// The expected ramp drop is analytic in world units, including oblique flow.
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
    let full = 20.0_f32 / 27.0; // smoothstep(0, .03, .02)
    let half = 7.0_f32 / 27.0; // smoothstep(0, .03, .01)
    for (case, (flow, slope, distance, drop, rate, x, expected)) in [
        ([1.0_f32, 0.0], [-0.02_f32, 0.0], 1.0, 0.03, 8.0, 96, full),
        ([-1.0, 0.0], [0.02, 0.0], 1.0, 0.03, 8.0, 96, full),
        ([0.0, 1.0], [0.0, -0.02], 1.0, 0.03, 8.0, 96, full),
        ([0.6, 0.8], [-0.012, -0.016], 1.0, 0.03, 8.0, 96, full),
        ([1.0, 0.0], [0.02, 0.0], 1.0, 0.03, 8.0, 96, 0.0),
        ([1.0, 0.0], [0.0, 0.0], 1.0, 0.03, 8.0, 96, 0.0),
        ([0.0, 0.0], [-0.02, 0.0], 1.0, 0.03, 8.0, 96, 0.0),
        ([1.0, 0.0], [-0.02, 0.0], 1.0, 0.03, 8.0, 255, 0.0),
        ([1.0, 0.0], [-0.02, 0.0], 1.0, 0.03, 0.0, 96, 0.0),
        ([1.0, 0.0], [-0.02, 0.0], 0.5, 0.03, 8.0, 96, half),
        ([1.0, 0.0], [-0.02, 0.0], 1.0, 0.06, 8.0, 96, half),
        ([-1.0, 0.0], [0.0, 0.0], 1.0, 0.03, 8.0, 96, 0.0),
        ([1.0, 0.0], [-0.02, 0.0], 1.0, 0.03, 8.0, 96, full),
        ([-1.0, 0.0], [0.02, 0.0], 1.0, 0.03, 8.0, 96, full),
        ([1.0, 0.0], [0.0, 0.0], 1.0, 0.03, 8.0, 96, 0.0),
        ([1.0, 0.0], [-0.03, 0.0], 1.0, 0.03, 8.0, 96, 1.0),
        ([1.0, 0.0], [-0.06, 0.0], 1.0, 0.03, 8.0, 96, 1.0),
    ]
    .into_iter()
    .enumerate()
    {
        let bed_slope = if (11..14).contains(&case) { 0.18 } else { 0.0 };
        let mut seed = vec![[0.0_f32; 4]; 5 * cells];
        for z in 0..n {
            for x in 0..n {
                let mut h = (2.0
                    + slope[0] * (x as f32 - 96.0) * 0.25
                    + slope[1] * (z as f32 - 96.0) * 1.5
                    + bed_slope * (x as f32 + 0.5) * 0.25)
                    .max(0.0);
                if case == 14 && x > 96 {
                    h = 0.0;
                }
                seed[cells + z * n + x] = [h, h * flow[0], h * flow[1], 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, 64.0, 384.0];
        p.terrain[1] = bed_slope;
        p.terrain[2] = 0.0;
        p.foam_front = [rate, distance, drop, 0.0];
        p.visual[1] = x as f32;
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
            return Err(format!("front probe {case}: {} != {expected}", result[0]).into());
        }
        if x == 255 || case == 14 {
            continue;
        } // Boundary damping is tested separately.
        p.options[0] = 0.0;
        p.options[3] = 0.0;
        p.timing[1] = 0.004;
        p.timing[2] = 0.0;
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
            let i = cells + 96 * n + x;
            let result: [f32; 4] = bytemuck::pod_read_unaligned(&bytes[16 * i..16 * (i + 1)]);
            if !result[3].is_finite() || (result[3] - rate * expected * 0.004).abs() > 0.0001 {
                return Err(format!("front source case {case}, order {order}: {result:?}").into());
            }
        }
    }
    eprintln!(
        "Front foam: falling/rising/flat surfaces, stationary/disabled/edge/dry samples, sloping-bed cancellation, anisotropic oblique sampling, full-strength saturation without gain suppression and both solver source updates OK"
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
    for order in [1, 2] {
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
                    let depth = if mode >= 5 {
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
            p.options[0] = 0.0;
            p.foam_front[0] = 0.0;
            p.options[3] = 0.0;
            p.timing[1] = dt;
            p.timing[2] = if mode == 3 { 0.7 } else { 0.0 };
            if mode == 5 {
                p.density[2] = 0.02;
            }
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
                    && ((data[2 * cells + 96 * n + 96][0] - 0.5).abs() > 1e-6
                        || data[96 * n + 96][1] != 1.0))
                || (mode == 6 && (center - 0.5).abs() > 1e-6)
                || (mode == 1 && center <= 0.5)
                || (mode == 2 && center >= 0.5)
                || (mode == 3 && (center - 0.5 * (-0.7 * dt).exp()).abs() > 1e-6)
            {
                return Err(format!("surface foam order {order} mode {mode}: total {total}, expected {expected}, center {center}").into());
            }
        }
    }
    eprintln!(
        "Surface foam: both orders conserve area amount, transport, concentrate/dilute and decay independently of depth; configurable deposition threshold OK"
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
    let spacing=shore.domain.zw/f32(n);
    q.w=max(0.0,q.w-shore.timing.y*((foam_flux(c,vec2(1,0),0u)+foam_flux(c,vec2(-1,0),0u))/spacing.x
        +(foam_flux(c,vec2(0,1),1u)+foam_flux(c,vec2(0,-1),1u))/spacing.y));
    new_shore[index]=vec4(q.w,1.0,q.x,velocity(q).x);
    new_shore[cells+index]=q;
    new_shore[2u*cells+index]=initial_shore[2u*cells+index];
    let motion=advance_foam_motion(c,q,original);
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
        ],
    });
    let n = RESOLUTION as usize;
    let cells = n * n;
    // Follow, response, water speed, initial foam speed, stretching, dt, steps.
    for (case, (follow, response, water, initial, stretch, dt, steps)) in [
        (0.5_f32, 0.0_f32, 1.0_f32, 0.0_f32, false, 0.02_f32, 120),
        (0.0, 0.0, 1.0, 0.0, false, 0.02, 120),
        (0.8, 0.35, 1.0, 0.0, false, 0.02, 120),
        (0.8, 0.35, -1.0, 0.8, false, 0.02, 120),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 120),
        (0.8, 0.35, 1.0, 0.0, false, 0.01, 240),
        (0.5, 0.0, 1.0, 0.0, false, 0.02, 1020),
        (1.0, 0.0, 0.0, 0.0, true, 0.02, 1020),
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
                seed[3 * cells + i] = [initial, 0.0, 0.0, 0.0];
            }
        }
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        let mut p = shore.parameters(&OpticsConfig::beach_scene(), 0.0, false, 0.0);
        p.domain = [0.0, 0.0, 64.0, 64.0];
        p.timing[1] = dt;
        p.foam_motion = [follow, response, 0.0, 0.0];
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
        let age_a = time % 2.0;
        let age_b = if time < 1.0 { time } else { (time + 1.0) % 2.0 };
        if (texture[2] - age_a / 2.0).abs() > 0.0001 {
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
            if (motion[0] - expected_velocity).abs() > 0.001
                || (motion[2] - layer_distance(age_a)).abs() > 0.002
                || (texture[0] - layer_distance(age_b)).abs() > 0.002
                || (density_distance - expected_distance as f64).abs() > 0.002
            {
                return Err(format!("foam motion {case}: velocity {}, texture {}, density {density_distance}; expected {expected_velocity}, {expected_distance}",motion[0],motion[2]).into());
            }
        }
    }
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
    use super::*;
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
                foam_front_depth_drop: 0.0,
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
                view: 7,
                ..Default::default()
            },
        ] {
            assert!(c.validate().is_err());
        }
    }
}

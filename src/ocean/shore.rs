//! Local hydrostatic-reconstruction SWE, foam transport and ground memory.
use super::{
    gpu::{Gpu, Ocean},
    optics::OpticsConfig,
};
use wgpu::util::DeviceExt;

pub const RESOLUTION: u32 = 256;
pub const VIEW_NAMES: [&str; 6] = [
    "Shaded",
    "Water depth",
    "Flow direction",
    "Shore foam",
    "Sand wetness",
    "Stranded foam",
];

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ShoreConfig {
    pub enabled: bool,
    pub breaking_depth: f32,
    pub generation: f32,
    pub decay: f32,
    pub drying_time: f32,
    pub view: u32,
}
impl Default for ShoreConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            breaking_depth: 2.0,
            generation: 4.0,
            decay: 0.45,
            drying_time: 18.0,
            view: 0,
        }
    }
}
impl ShoreConfig {
    pub fn validate(self) -> Result<(), String> {
        for (name, value, low, high) in [
            ("breaking depth", self.breaking_depth, 0.5, 5.0),
            ("shore foam generation", self.generation, 0.0, 15.0),
            ("shore foam decay", self.decay, 0.0, 4.0),
            ("sand drying time", self.drying_time, 1.0, 120.0),
        ] {
            if !value.is_finite() || !(low..=high).contains(&value) {
                return Err(format!("{name} must be finite and in {low}..={high}"));
            }
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
    pub options: [f32; 4],
}
pub struct Shore {
    pub uniform: wgpu::Buffer,
    pub field: wgpu::Buffer,
    pub lace: wgpu::Buffer,
    previous: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
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
                size: u64::from(RESOLUTION * RESOLUTION) * 48,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let field = make_field("SWE visible, conservative and ground blocks");
        let previous = make_field("previous shore history");
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
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shore compute bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: ocean.fields.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: fft_previous.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: previous.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: field.as_entire_binding(),
                },
            ],
        });
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
        // Cover the whole beach along Z, with a fine cross-shore strip.
        // Widen for gentle slopes so the wet/dry front cannot leave the history.
        let inland = (self.height_bound * gain.abs() * 2.0 / optics.beach_slope).clamp(12.0, 64.0);
        let offshore = (c.breaking_depth * 2.5 / optics.beach_slope).max(52.0);
        let mut s = ShoreUniform {
            terrain: [
                f32::from(optics.beach),
                optics.beach_slope,
                optics.shore_x,
                optics.depth,
            ],
            waves: [
                self.height_bound * gain.abs(),
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
            coupling: [self.fft_resolution as f32, self.length, gain, 1.0],
            options: [c.generation, c.view as f32, RESOLUTION as f32, 0.0],
        };
        let changed = self.last.is_none_or(|old| {
            old.terrain != s.terrain
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
        let step = self.parameters(
            optics,
            time,
            false,
            self.last.map_or(0.22, |s| s.coupling[2]),
        );
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
        let gain_changed = self.last.is_some_and(|s| s.coupling[2] != height_gain);
        let mut step = self.parameters(optics, time, reset || gain_changed, height_gain);
        step.coupling[2] = height_gain;
        step.waves[0] = self.height_bound * height_gain.abs();
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
        let mut uniforms = Vec::new();
        for i in 0..count {
            let mut sub = step;
            sub.timing[0] = time - elapsed + (i + 1) as f32 * dt;
            sub.timing[1] = dt;
            sub.waves[1] = (sub.timing[0] - self.start_time).max(0.0);
            sub.coupling[3] = ((sub.timing[0] - (time - step.timing[1]))
                / step.timing[1].max(0.000001))
            .clamp(0.0, 1.0);
            uniforms.push(sub);
        }
        // Copies ordered in the command buffer avoid queue-write aliasing across substeps.
        uniforms.push(step);
        let upload = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SWE substep parameters"),
                contents: bytemuck::cast_slice(&uniforms),
                usage: wgpu::BufferUsages::COPY_SRC,
            });
        let stride = std::mem::size_of::<ShoreUniform>() as u64;
        for i in 0..count {
            encoder.copy_buffer_to_buffer(&upload, i as u64 * stride, &self.uniform, 0, stride);
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("local SWE and stranded foam"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind, &[]);
                pass.dispatch_workgroups(RESOLUTION.div_ceil(8), RESOLUTION.div_ceil(8), 1);
            }
            encoder.copy_buffer_to_buffer(&self.field, 0, &self.previous, 0, self.field.size());
        }
        encoder.copy_buffer_to_buffer(&upload, count as u64 * stride, &self.uniform, 0, stride);
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

/// Run the production solver on the GPU, including stationary and moving water.
pub fn verify(gpu: &Gpu) -> Result<(), super::gpu::Error> {
    verify_inlet(gpu)?;
    let ocean = Ocean::new(
        gpu,
        super::spectrum::OceanConfig {
            wind_angle: 3.64,
            ..Default::default()
        },
    )?;
    let mut shore = Shore::new(gpu, &ocean);
    let mut optics = OpticsConfig::beach_scene();
    let gain = 0.0;
    let advance = |shore: &mut Shore, optics: &OpticsConfig, time, reset, gain| {
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        ocean.encode(gpu, &mut encoder, time);
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
    // Ground memory: a wet-to-dry transition deposits the last concentration.
    let mut seed = still.clone();
    let dry = n / 8 + n / 2 * n;
    seed[dry] = [0.8, 1.0, 0.01, 0.0];
    seed[2 * cells + dry][1] = 0.8;
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
    let gain = 1.0;
    advance(&mut shore, &optics, 0.0, true, gain);
    for i in 1..=1440 {
        advance(&mut shore, &optics, i as f32 / 60.0, false, gain);
    }
    let active = read(&shore)?;
    if active.iter().flatten().any(|v| !v.is_finite())
        || active[cells..2 * cells]
            .iter()
            .any(|q| q[0] < 0.0 || q[3] < 0.0 || q[3] > q[0] + 1e-5)
        || !active[..cells].iter().any(|q| q[0] > 0.1)
        || !active[2 * cells..].iter().any(|q| q[0] > 0.01)
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
        || cleared[2 * cells..].iter().any(|q| q[0] != 0.0)
    {
        return Err("SWE reset failed".into());
    }
    optics.shore.enabled = false;
    advance(&mut shore, &optics, 24.1, false, gain);
    if read(&shore)?.iter().any(|q| *q != [0.0; 4]) {
        return Err("SWE disable failed".into());
    }
    optics.shore.enabled = true;
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
                .any(|q| q[0] < 0.0 || q[3] < 0.0 || q[3] > q[0] + 1e-5)
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
    let quiet_optics = OpticsConfig::beach_scene();
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
        "SWE GPU: lake at rest ({error:.2e}), pulse mass/2D foam transport, stranded deposition/decay/rewet, run-up, pause/reset/disable, terrain reset and extreme slopes OK"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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
                breaking_depth: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                drying_time: 0.0,
                ..Default::default()
            },
            ShoreConfig {
                generation: f32::NAN,
                ..Default::default()
            },
            ShoreConfig {
                view: 6,
                ..Default::default()
            },
        ] {
            assert!(c.validate().is_err());
        }
    }
}

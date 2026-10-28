//! Fixed-input timings of the full production SWE entry, not frame-time feedback.
use super::*;
use water_simulation::ocean::{
    collider::Collider, gpu::Ocean, optics::OpticsConfig, shore::Shore, spectrum::OceanConfig,
};

const ROUNDS: u32 = 7;
const STEPS: u32 = 32;

struct Variant {
    order: u32,
    space: u32,
    output: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bindings: Vec<wgpu::BindGroup>,
    timer: Option<(wgpu::QuerySet, wgpu::Buffer)>,
}

fn encode(gpu: &Gpu, v: &Variant, n: u32, steps: u32, round: Option<u32>) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    for step in 0..steps {
        for (stage, bind) in v.bindings.iter().enumerate() {
            let timestamp_writes = v
                .timer
                .as_ref()
                .and_then(|(queries, _)| {
                    round.map(|r| wgpu::ComputePassTimestampWrites {
                        query_set: queries,
                        beginning_of_pass_write_index: (step == 0 && stage == 0).then_some(2 * r),
                        end_of_pass_write_index: (step + 1 == steps
                            && stage + 1 == v.bindings.len())
                        .then_some(2 * r + 1),
                    })
                })
                .filter(|t| {
                    t.beginning_of_pass_write_index.is_some() || t.end_of_pass_write_index.is_some()
                });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                timestamp_writes,
                ..Default::default()
            });
            pass.set_pipeline(&v.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(n.div_ceil(8), n.div_ceil(8), 1);
        }
    }
    // No output->input copy: every iteration repeats exactly the same substep.
    // RK2's predictor is recomputed each time, using the immutable original state.
    gpu.queue.submit([encoder.finish()]);
}

fn fingerprint(bytes: &[u8]) -> String {
    format!(
        "{:016x}",
        bytes
            .iter()
            .fold(0xcbf29ce484222325u64, |h, &b| (h ^ u64::from(b))
                .wrapping_mul(0x100000001b3))
    )
}

pub(super) fn run(gpu: &Gpu, output: &std::path::Path, resolutions: &[usize]) -> Result<(), Error> {
    let ocean = Ocean::new(
        gpu,
        OceanConfig {
            resolution: 8,
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    let mut results = Vec::new();
    for &n in resolutions {
        let cells = n * n;
        let mut shore = Shore::with_resolution(gpu, &ocean, n as u32);
        let make_buffer = |label| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (80 * cells) as u64,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let input = make_buffer("fixed benchmark input");
        let stage = make_buffer("fixed benchmark predictor");
        let fft_before = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("zero previous FFT"),
            size: 8 * 8 * 3 * 8,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        for fixture in [
            "beach-sparse",
            "beach-dense",
            "model-dense",
            "obstacles-dense",
        ] {
            let mut p = shore.parameters(&OpticsConfig::beach_scene(), 8.0, false, 0.0);
            p.domain = [-32.0, -32.0, 64.0, 64.0];
            p.terrain = [1.0, 0.02, -150.0, 2.0];
            p.options[0] = 0.0;
            p.options[3] = 0.0;
            p.foam_front[0] = 0.0;
            p.timing[1] = 0.2 / ((12.0 + (9.81_f32 * 5.0).sqrt()) * 2.0 * n as f32 / 64.0);
            p.timing[2] = 0.3;
            p.foam_motion = [0.8, 0.35, 0.0, 0.0];
            p.foam_inertia = [1.0, 1.0, 1.0, 0.0];
            p.model = [f32::from(fixture == "model-dense"), -5.0, -2.0, 2.0];
            p.edges = [0.0; 4];
            if fixture == "obstacles-dense" {
                p.collider_meta[0] = 8.0;
                for (i, c) in p.colliders.iter_mut().take(8).enumerate() {
                    *c = Collider {
                        position: [-21.0 + (i % 4) as f32 * 14.0, -12.0 + (i / 4) as f32 * 24.0],
                        size: [3.0, 6.0],
                        angle: 23.0,
                        ..Default::default()
                    }
                    .uniform(0.02, -150.0);
                    c.solid[1] = 1.0;
                }
            }
            let mut seed = vec![[0.0_f32; 4]; 5 * cells];
            let mut terrain = vec![[0.0_f32; 2]; cells];
            for i in 0..cells {
                let x = -32.0 + (i % n) as f32 * 64.0 / n as f32 + 32.0 / n as f32;
                let z = -32.0 + (i / n) as f32 * 64.0 / n as f32 + 32.0 / n as f32;
                let mut bottom = -0.02 * (x + 150.0);
                if fixture == "model-dense" {
                    bottom = -2.0
                        + 0.6 * (x * 0.15).sin() * (z * 0.12).cos()
                        + 2.5 * (-((x - 5.0).powi(2) + (z + 3.0).powi(2)) / 25.0).exp();
                }
                terrain[i] = [bottom, 1.0];
                for c in p.colliders.iter().take(p.collider_meta[0] as usize) {
                    let dx = x - c.footprint[0];
                    let dz = z - c.footprint[1];
                    let inside = if c.solid[0] < 1.5 {
                        dx * dx + dz * dz <= c.footprint[2].powi(2)
                    } else {
                        (c.solid[2] * dx + c.solid[3] * dz).abs() <= c.footprint[2]
                            && (-c.solid[3] * dx + c.solid[2] * dz).abs() <= c.footprint[3]
                    };
                    if inside {
                        bottom = bottom.max(c.solid[1]);
                    }
                }
                let h = (-bottom).max(0.0);
                let amount =
                    if h <= p.density[2] || (fixture == "beach-sparse" && x * x + z * z > 16.0) {
                        0.0
                    } else {
                        0.6 + 0.3 * (x * 1.1).sin() * (z * 0.8).cos()
                    };
                let water = [0.4 * (z * 0.1).cos(), 0.25 * (x * 0.1).sin()];
                let foam = [0.7 + 0.2 * (z * 0.2).sin(), -0.3 + 0.15 * (x * 0.2).cos()];
                seed[i] = [amount, 1.0, h, water[0]];
                seed[cells + i] = [h, h * water[0], h * water[1], amount];
                seed[2 * cells + i] = [0.0, 0.0, h + bottom, water[1]];
                seed[3 * cells + i] = [amount * foam[0], amount * foam[1], 0.1, 0.2];
                seed[4 * cells + i] = [0.3, 0.4, 0.37, 0.0];
            }
            gpu.queue
                .write_buffer(&input, 0, bytemuck::cast_slice(&seed));
            gpu.queue
                .write_buffer(&shore.terrain_bed, 0, bytemuck::cast_slice(&terrain));
            let mut variants = Vec::new();
            for (order, space) in [(1u32, 1u32), (2, 1), (2, 2)] {
                let pipeline = shore.solver_pipeline(gpu, order, space).clone();
                let result = make_buffer("fixed benchmark output");
                let mut bindings = Vec::new();
                p.solver[0] = order as f32;
                p.foam_inertia[2] = space as f32;
                for s in 0..order {
                    p.solver[1] = if order == 2 { (s + 1) as f32 } else { 0.0 };
                    let uniform =
                        gpu.device
                            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                                label: Some("immutable benchmark parameters"),
                                contents: bytemuck::bytes_of(&p),
                                usage: wgpu::BufferUsages::UNIFORM,
                            });
                    let from = if s == 0 { &input } else { &stage };
                    let to = if order == 2 && s == 0 {
                        &stage
                    } else {
                        &result
                    };
                    let entries: Vec<_> = [
                        (0, &uniform),
                        (1, from),
                        (2, to),
                        (3, &ocean.fields),
                        (4, &fft_before),
                        (5, &input),
                        (7, &shore.terrain_bed),
                    ]
                    .into_iter()
                    .map(|(binding, buffer)| wgpu::BindGroupEntry {
                        binding,
                        resource: buffer.as_entire_binding(),
                    })
                    .collect();
                    bindings.push(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("fixed production SWE benchmark"),
                        layout: &pipeline.get_bind_group_layout(0),
                        entries: &entries,
                    }));
                }
                let timer = gpu
                    .device
                    .features()
                    .contains(wgpu::Features::TIMESTAMP_QUERY)
                    .then(|| {
                        (
                            gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                                label: Some("benchmark rounds"),
                                ty: wgpu::QueryType::Timestamp,
                                count: 2 * ROUNDS,
                            }),
                            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                                label: Some("benchmark timestamps"),
                                size: u64::from(ROUNDS) * 16,
                                usage: wgpu::BufferUsages::QUERY_RESOLVE
                                    | wgpu::BufferUsages::COPY_SRC,
                                mapped_at_creation: false,
                            }),
                        )
                    });
                variants.push(Variant {
                    order,
                    space,
                    output: result,
                    pipeline,
                    bindings,
                    timer,
                });
            }
            for v in &variants {
                encode(gpu, v, n as u32, 8, None);
            }
            // Alternate order to reduce systematic clock/thermal bias.
            for round in 0..ROUNDS {
                for index in 0..variants.len() {
                    let index = if round % 2 == 0 {
                        index
                    } else {
                        variants.len() - 1 - index
                    };
                    encode(gpu, &variants[index], n as u32, STEPS, Some(round));
                }
            }
            for v in variants {
                let samples = if let Some((queries, buffer)) = &v.timer {
                    let mut enc = gpu.device.create_command_encoder(&Default::default());
                    enc.resolve_query_set(queries, 0..2 * ROUNDS, buffer, 0);
                    gpu.queue.submit([enc.finish()]);
                    let bytes = gpu.read_buffer(buffer, buffer.size())?;
                    Some(
                        bytes
                            .as_chunks::<16>()
                            .0
                            .iter()
                            .map(|pair| {
                                let start = u64::from_le_bytes(pair[..8].try_into().unwrap());
                                let end = u64::from_le_bytes(pair[8..].try_into().unwrap());
                                end.wrapping_sub(start) as f64
                                    * f64::from(gpu.queue.get_timestamp_period())
                                    / 1e6
                                    / f64::from(STEPS)
                            })
                            .collect::<Vec<_>>(),
                    )
                } else {
                    None
                };
                let median = samples.as_ref().map(|s| {
                    let mut s = s.clone();
                    s.sort_by(f64::total_cmp);
                    s[s.len() / 2]
                });
                let bytes = gpu.read_buffer(&v.output, v.output.size())?;
                let values: Vec<[f32; 4]> = bytes
                    .as_chunks::<16>()
                    .0
                    .iter()
                    .map(|b| bytemuck::pod_read_unaligned(b))
                    .collect();
                if values.iter().flatten().any(|v| !v.is_finite())
                    || values[cells..2 * cells]
                        .iter()
                        .any(|q| q[0] < 0.0 || q[3] < 0.0)
                {
                    return Err(format!("invalid benchmark state: {fixture}").into());
                }
                let name = format!("{fixture}-n{n}-o{}-s{}", v.order, v.space);
                fs::write(output.join(format!("{name}.bin")), &bytes)?;
                println!("{name}: GPU ms/substep {median:?}");
                results.push(serde_json::json!({"fixture":fixture,"resolution":n,"time_order":v.order,"space_order":v.space,"dt_s":p.timing[1],"gpu_ms_per_substep":median,"gpu_ms_per_simulated_second":median.map(|ms| ms / f64::from(p.timing[1])),"gpu_ms_samples":samples,"input_fnv1a":fingerprint(bytemuck::cast_slice(&seed)),"terrain_fnv1a":fingerprint(bytemuck::cast_slice(&terrain)),"output_fnv1a":fingerprint(&bytes),"state_file":format!("{name}.bin")}));
            }
        }
    }
    fs::write(
        output.join("benchmark.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "gpu":gpu.name,"fixture_version":1,"rounds":ROUNDS,"iterations_per_round":STEPS,"warmup_iterations":8,
            "method":"Full production SWE main entry and specialized pipelines. Immutable initial input; RK2 recomputes predictor each iteration. Fixed dt; alternating variant order. GPU timestamps exclude compilation, uploads, normal-frame state copies, FFT, spray, rendering, readback and inter-round idle. Per-simulated-second cost is extrapolated from repeated fixed inputs, not continuous evolution. No personal settings.",
            "state_format":"Little-endian f32; production five vec4 blocks per cell; output after ONE fixed substep, not a time evolution.",
            "shader_fnv1a":fingerprint(format!("{}\n{}",include_str!("../../ocean/shaders/shore.wgsl"),include_str!("../../ocean/shaders/shore_compute.wgsl")).as_bytes()),
            "results":results
        }))?,
    )?;
    Ok(())
}

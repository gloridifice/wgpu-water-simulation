//! Reproducible, headless foam-shape experiments using production GPU transport.
//! Prescribed divergence-free flows have independent exact characteristic maps.
//! Snapshots are fixed-scale density plots, not the normal ocean material.
use std::{fmt::Write as _, fs, io::Write, path::PathBuf};
use water_simulation::ocean::{
    gpu::{Error, Gpu},
    shore::ShoreUniform,
};
use wgpu::util::DeviceExt;

#[path = "foam_lab/benchmark.rs"]
mod benchmark;

const SIDE: f64 = 32.0;
const SAMPLES: usize = 8;
const IMAGE_SIZE: usize = 512;
// <= 1/4 per face preserves nonnegative MUSCL face-state contributions.
// Both schemes use this same bound; old 0.4-CFL runs are not the A/B baseline.
const CFL: f64 = 0.2;
const HELP: &str = "GPU foam shape laboratory (does not load or save personal settings)
Usage: cargo run --release --bin foam-lab -- [options]
  --output PATH       New output directory (default: target/foam-lab)
  --benchmark         Fixed-input full SWE GPU timings (sparse/dense/model/obstacles);
                      uses --resolutions and --output, writes JSON and raw state binaries
  --scenario NAME     all, translation, shear, vortex (default: all)
  --resolutions LIST  Comma-separated: 64,128,256,512,1024 (default: 128,256,512)
  --orders LIST       Time integration: 1=Euler, 2=RK2 (default: 2)
  --space-orders LIST Spatial transport: 1=baseline, 2=MUSCL (default: 1,2)
  --times LIST        Increasing simulation seconds, starting at 0, <=8 (default: 0,2,4,8)
  -h, --help          Show help
Fixed: 32x32 m domain, 2 m water, density 0..1 initially, no sources/decay,
instantaneous flow following, production foam flux; common CFL=0.2.
Outputs: index.html, manifest.json, metrics.csv, density/reference/error PNGs.
No window or wall-clock capture delay; every snapshot follows the requested simulation time.
";

#[derive(Clone, Copy)]
enum Scene {
    Translation,
    Shear,
    Vortex,
}
impl Scene {
    const ALL: [Self; 3] = [Self::Translation, Self::Shear, Self::Vortex];
    fn name(self) -> &'static str {
        match self {
            Self::Translation => "translation",
            Self::Shear => "shear",
            Self::Vortex => "vortex",
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Translation => {
                "平移：中心 (−5,0) 米，0.5 米宽、8 米长的条带；速度 (0.75, 0.25) 米/秒。"
            }
            Self::Shear => "剪切：外半径 2.5 米、内半径 1.5 米的圆环；X 速度 = 0.5/秒 × Z。",
            Self::Vortex => {
                "差速旋流：中心 (3,0) 米、外半径 1.75 米、内半径 1 米的偏心圆环；角速度 = 0.8 × exp(−半径²/25) 弧度/秒。"
            }
        }
    }
    fn flow(self, x: f64, z: f64) -> [f64; 2] {
        match self {
            Self::Translation => [0.75, 0.25],
            Self::Shear => [0.5 * z, 0.0],
            Self::Vortex => {
                let omega = 0.8 * (-(x * x + z * z) / 25.0).exp();
                [-omega * z, omega * x]
            }
        }
    }
    // Independent analytic characteristics, not a CPU copy of the grid flux.
    fn density(self, mut x: f64, mut z: f64, time: f64) -> f64 {
        match self {
            Self::Translation => {
                x -= 0.75 * time;
                z -= 0.25 * time;
            }
            Self::Shear => x -= 0.5 * z * time,
            Self::Vortex => {
                let angle = -0.8 * (-(x * x + z * z) / 25.0).exp() * time;
                let (s, c) = angle.sin_cos();
                (x, z) = (c * x - s * z, s * x + c * z);
            }
        }
        f64::from(match self {
            Self::Translation => (x + 5.0).abs() < 0.25 && z.abs() < 4.0,
            Self::Shear => (1.5..2.5).contains(&x.hypot(z)),
            Self::Vortex => (1.0..1.75).contains(&(x - 3.0).hypot(z)),
        })
    }
    fn reference(self, n: usize, time: f64) -> Vec<f32> {
        let dx = SIDE / n as f64;
        (0..n * n)
            .map(|i| {
                let mut sum = 0.0;
                for sy in 0..SAMPLES {
                    for sx in 0..SAMPLES {
                        let x = -SIDE / 2.0
                            + (i % n) as f64 * dx
                            + (sx as f64 + 0.5) * dx / SAMPLES as f64;
                        let z = -SIDE / 2.0
                            + (i / n) as f64 * dx
                            + (sy as f64 + 0.5) * dx / SAMPLES as f64;
                        sum += self.density(x, z, time);
                    }
                }
                (sum / (SAMPLES * SAMPLES) as f64) as f32
            })
            .collect()
    }
}

struct Config {
    output: PathBuf,
    scenes: Vec<Scene>,
    resolutions: Vec<usize>,
    orders: Vec<u32>,
    space_orders: Vec<u32>,
    times: Vec<f64>,
    benchmark: bool,
}
fn list<T: std::str::FromStr>(text: &str) -> Result<Vec<T>, Error> {
    text.split(',')
        .map(|v| {
            v.parse()
                .map_err(|_| format!("invalid list: {text}").into())
        })
        .collect()
}
fn config() -> Result<Option<Config>, Error> {
    let mut c = Config {
        output: "target/foam-lab".into(),
        scenes: Scene::ALL.to_vec(),
        resolutions: vec![128, 256, 512],
        orders: vec![2],
        space_orders: vec![1, 2],
        times: vec![0.0, 2.0, 4.0, 8.0],
        benchmark: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            print!("{HELP}");
            return Ok(None);
        }
        if arg == "--benchmark" {
            c.benchmark = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {arg}"))?;
        match arg.as_str() {
            "--output" => c.output = value.into(),
            "--scenario" => {
                c.scenes = if value == "all" {
                    Scene::ALL.to_vec()
                } else {
                    vec![
                        Scene::ALL
                            .into_iter()
                            .find(|s| s.name() == value)
                            .ok_or("unknown scenario")?,
                    ]
                }
            }
            "--resolutions" => c.resolutions = list(&value)?,
            "--orders" => c.orders = list(&value)?,
            "--space-orders" => c.space_orders = list(&value)?,
            "--times" => c.times = list(&value)?,
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    if c.resolutions
        .iter()
        .any(|n| ![64, 128, 256, 512, 1024].contains(n))
        || c.orders.iter().any(|n| ![1, 2].contains(n))
        || c.space_orders.iter().any(|n| ![1, 2].contains(n))
        || c.space_orders.windows(2).any(|w| w[0] >= w[1])
        || c.resolutions.windows(2).any(|w| w[0] >= w[1])
        || c.orders.windows(2).any(|w| w[0] >= w[1])
        || c.times.len() < 2
        || c.times[0] != 0.0
        || c.times
            .iter()
            .any(|t| !t.is_finite() || !(0.0..=8.0).contains(t))
        || c.times.windows(2).any(|w| w[0] >= w[1])
    {
        return Err("invalid configuration; see --help (lists must be increasing)".into());
    }
    Ok(Some(c))
}

struct Experiment {
    states: [wgpu::Buffer; 2],
    stage: wgpu::Buffer,
    terrain: wgpu::Buffer,
    n: usize,
    order: u32,
    space_order: u32,
    current: usize,
    maximum_speed_sum: f64,
    initial_mass: f64,
}
impl Experiment {
    fn new(gpu: &Gpu, scene: Scene, n: usize, order: u32, space_order: u32) -> Self {
        let cells = n * n;
        let density = scene.reference(n, 0.0);
        let mut seed = vec![[0.0_f32; 4]; 5 * cells];
        let mut maximum_speed_sum = 0.0_f64;
        for i in 0..cells {
            let x = -SIDE / 2.0 + (i % n) as f64 * SIDE / n as f64 + SIDE / n as f64 / 2.0;
            let z = -SIDE / 2.0 + (i / n) as f64 * SIDE / n as f64 + SIDE / n as f64 / 2.0;
            let v = scene.flow(x, z);
            maximum_speed_sum = maximum_speed_sum.max(v[0].abs() + v[1].abs());
            let [u, v] = v.map(|v| v as f32);
            seed[i] = [density[i], 1.0, 2.0, u];
            seed[cells + i] = [2.0, 2.0 * u, 2.0 * v, density[i]];
            seed[2 * cells + i] = [0.0, 0.0, 0.0, v];
            seed[3 * cells + i] = [density[i] * u, density[i] * v, 0.0, 0.0];
        }
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
        let states = std::array::from_fn(|_| {
            gpu.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("foam lab state"),
                    contents: bytemuck::cast_slice(&seed),
                    usage,
                })
        });
        let stage = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("foam lab RK predictor"),
            size: states[0].size(),
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let terrain = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("foam lab unused flat terrain"),
            size: (cells * 8) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        Self {
            states,
            stage,
            terrain,
            n,
            order,
            space_order,
            current: 0,
            maximum_speed_sum,
            initial_mass: density.iter().map(|v| f64::from(*v)).sum::<f64>()
                * (SIDE / n as f64).powi(2),
        }
    }
    fn advance(
        &mut self,
        gpu: &Gpu,
        pipeline: &wgpu::ComputePipeline,
        duration: f64,
    ) -> Result<(usize, f64, Option<f64>), Error> {
        let max_dt = CFL * SIDE / self.n as f64 / self.maximum_speed_sum;
        let steps = (duration / max_dt).ceil() as usize;
        let dt = (duration / steps as f64) as f32;
        let mut params: ShoreUniform = bytemuck::Zeroable::zeroed();
        params.terrain = [1.0, 0.0, 0.0, 2.0];
        params.waves[3] = 1.0;
        params.domain = [-16.0, -16.0, 32.0, 32.0];
        params.options[2] = self.n as f32;
        params.density[2] = 0.003;
        params.foam_motion = [1.0, 0.0, 0.0, 0.0];
        params.foam_inertia = [1.0, 1.0, self.space_order as f32, 0.0];
        params.timing[1] = dt;
        params.solver[0] = self.order as f32;
        let mut bindings = Vec::new();
        for ping in 0..2 {
            for stage in 0..self.order {
                params.solver[1] = if self.order == 2 {
                    (stage + 1) as f32
                } else {
                    0.0
                };
                let uniform = gpu
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("foam lab immutable stage"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                let input = if stage == 1 {
                    &self.stage
                } else {
                    &self.states[ping]
                };
                let output = if self.order == 2 && stage == 0 {
                    &self.stage
                } else {
                    &self.states[1 - ping]
                };
                let entries: Vec<_> = [
                    (0, &uniform),
                    (1, input),
                    (2, output),
                    (5, &self.states[ping]),
                    (7, &self.terrain),
                ]
                .into_iter()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
                bindings.push(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("foam lab transport"),
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                }));
            }
        }
        // Timestamp each bounded batch, excluding CPU/queue gaps between batches.
        // Offline readback at snapshot times only; no timer dependency on devices
        // without TIMESTAMP_QUERY. Includes all lab kernel work, not just fluxes.
        let query_count = 2 * steps.div_ceil(64) as u32;
        let timer = gpu
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| {
                let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("foam lab batch timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: query_count,
                });
                let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("foam lab timestamp resolve"),
                    size: u64::from(query_count) * 8,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                (queries, buffer)
            });
        for (batch, start) in (0..steps).step_by(64).enumerate() {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            for step in start..(start + 64).min(steps) {
                for stage in 0..self.order as usize {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        timestamp_writes: timer
                            .as_ref()
                            .map(|(queries, _)| wgpu::ComputePassTimestampWrites {
                                query_set: queries,
                                beginning_of_pass_write_index: (step == start && stage == 0)
                                    .then_some(2 * batch as u32),
                                end_of_pass_write_index: (step + 1 == (start + 64).min(steps)
                                    && stage + 1 == self.order as usize)
                                    .then_some(2 * batch as u32 + 1),
                            })
                            .filter(|t| {
                                t.beginning_of_pass_write_index.is_some()
                                    || t.end_of_pass_write_index.is_some()
                            }),
                        ..Default::default()
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(
                        0,
                        &bindings[self.current * self.order as usize + stage],
                        &[],
                    );
                    pass.dispatch_workgroups(
                        (self.n as u32).div_ceil(8),
                        (self.n as u32).div_ceil(8),
                        1,
                    );
                }
                self.current = 1 - self.current;
            }
            gpu.queue.submit([encoder.finish()]);
        }
        let gpu_ms = if let Some((queries, buffer)) = timer {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encoder.resolve_query_set(&queries, 0..query_count, &buffer, 0);
            gpu.queue.submit([encoder.finish()]);
            let bytes = gpu.read_buffer(&buffer, buffer.size())?;
            let ticks: u64 = bytes
                .as_chunks::<16>()
                .0
                .iter()
                .map(|pair| {
                    let start = u64::from_le_bytes(pair[..8].try_into().unwrap());
                    let end = u64::from_le_bytes(pair[8..].try_into().unwrap());
                    end.wrapping_sub(start)
                })
                .sum();
            Some(ticks as f64 * f64::from(gpu.queue.get_timestamp_period()) / 1e6)
        } else {
            None
        };
        Ok((steps, f64::from(dt), gpu_ms))
    }
    fn read(&self, gpu: &Gpu) -> Result<Vec<f32>, Error> {
        let bytes =
            gpu.read_buffer(&self.states[self.current], self.states[self.current].size())?;
        let values: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
        if values.iter().flatten().any(|v| !v.is_finite()) {
            return Err("nonfinite GPU lab state".into());
        }
        let cells = self.n * self.n;
        let density: Vec<_> = values[cells..2 * cells].iter().map(|v| v[3]).collect();
        if density.iter().any(|v| *v < 0.0) {
            return Err("negative foam density".into());
        }
        Ok(density)
    }
}

struct Metrics {
    mass: f64,
    relative_mass_error: f64,
    relative_l1: f64,
    peak: f64,
    area_above_half: f64,
    centroid_error: f64,
    x_sigma: f64,
}
fn metrics(actual: &[f32], reference: &[f32], n: usize, initial_mass: f64) -> Metrics {
    let dx = SIDE / n as f64;
    let moments = |field: &[f32]| {
        let mut m = [0.0; 4];
        for (i, &v) in field.iter().enumerate() {
            let x = -SIDE / 2.0 + (i % n) as f64 * dx + dx / 2.0;
            let z = -SIDE / 2.0 + (i / n) as f64 * dx + dx / 2.0;
            let amount = f64::from(v) * dx * dx;
            m[0] += amount;
            m[1] += amount * x;
            m[2] += amount * z;
            m[3] += amount * x * x;
        }
        m
    };
    let a = moments(actual);
    let r = moments(reference);
    Metrics {
        mass: a[0],
        relative_mass_error: a[0] / initial_mass - 1.0,
        relative_l1: actual
            .iter()
            .zip(reference)
            .map(|(&a, &r)| f64::from((a - r).abs()) * dx * dx)
            .sum::<f64>()
            / r[0],
        peak: actual.iter().copied().fold(0.0_f32, f32::max).into(),
        area_above_half: actual.iter().filter(|v| **v >= 0.5).count() as f64 * dx * dx,
        centroid_error: (a[1] / a[0] - r[1] / r[0]).hypot(a[2] / a[0] - r[2] / r[0]),
        x_sigma: (a[3] / a[0] - (a[1] / a[0]).powi(2)).max(0.0).sqrt(),
    }
}

fn png(path: PathBuf, values: &[f32], n: usize, error: bool) -> Result<(), Error> {
    let mut pixels = Vec::with_capacity(IMAGE_SIZE * IMAGE_SIZE * 3);
    // One fixed orthographic extent and linear 0..1 color scale for every run.
    // Nearest cells when enlarging, area means when reducing (e.g. 1024).
    // No interpolated detail or automatic contrast enhancement.
    let block = (n / IMAGE_SIZE).max(1);
    for y in 0..IMAGE_SIZE {
        for x in 0..IMAGE_SIZE {
            let mut sum = 0.0;
            for by in 0..block {
                for bx in 0..block {
                    sum += values[(n - 1 - y * n / IMAGE_SIZE - by) * n + x * n / IMAGE_SIZE + bx];
                }
            }
            let byte = ((sum / (block * block) as f32).clamp(0.0, 1.0) * 255.0).round() as u8;
            pixels.extend(if error {
                [byte, 0, 0]
            } else {
                [byte, byte, byte]
            });
        }
    }
    let mut encoder = png::Encoder::new(
        fs::File::create(path)?,
        IMAGE_SIZE as u32,
        IMAGE_SIZE as u32,
    );
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(())
}
fn gallery_row(html: &mut String, label: &str, names: &[String]) {
    write!(html, "<tr><th>{label}</th>").unwrap();
    for name in names {
        write!(
            html,
            "<td><a href='{name}.png'><img src='{name}.png' loading='lazy'></a></td>"
        )
        .unwrap();
    }
    html.push_str("</tr>\n");
}

fn transport_pipeline(gpu: &Gpu) -> wgpu::ComputePipeline {
    let shader = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("production foam transport laboratory"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("../ocean/shore.wgsl"),
                    include_str!("../ocean/shore_compute.wgsl"),
                    include_str!("foam_lab/transport.wgsl")
                )
                .into(),
            ),
        });
    gpu.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("foam lab"),
            layout: None,
            module: &shader,
            entry_point: Some("lab_step"),
            compilation_options: Default::default(),
            cache: None,
        })
}

async fn run(c: Config) -> Result<(), Error> {
    // Refuse to clobber any existing run, even a partly completed one.
    if let Some(parent) = c.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(&c.output)?;
    let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
    println!("GPU: {}", gpu.name);
    if c.benchmark {
        return benchmark::run(&gpu, &c.output, &c.resolutions);
    }
    let pipeline = transport_pipeline(&gpu);
    fs::write(
        c.output.join("manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "gpu":gpu.name,"scenarios":c.scenes.iter().map(|s|s.name()).collect::<Vec<_>>(),
            "scene_descriptions":c.scenes.iter().map(|s|s.description()).collect::<Vec<_>>(),
            "resolutions":c.resolutions,"time_orders":c.orders,"space_orders":c.space_orders,"snapshot_seconds":c.times,
            "gpu_timing":"Sum of per-batch GPU timestamps; all lab kernels, excluding inter-batch queue gaps, image export and readback. Null/blank if unsupported.",
            "domain_m":[-16,-16,32,32],"water_depth_m":2,"cfl":CFL,"source_per_second":0,"decay_per_second":0,
            "flow_follow":1,"drag_response_seconds":0,"density_peak_initial":1,
            "reference_subsamples_per_cell_axis":SAMPLES,"image_extent_m":[-16,-16,32,32],
            "image_linear_density_range":[0,1],"image_size":IMAGE_SIZE,
            "production_shaders_fnv1a":format!("{:016x}",include_bytes!("../ocean/shore.wgsl").iter().chain(include_bytes!("../ocean/shore_compute.wgsl")).fold(0xcbf29ce484222325u64,|h,&b|(h^u64::from(b)).wrapping_mul(0x100000001b3))),
            "limitations":"Prescribed analytic flow, no SWE evolution or foam birth. Time and spatial orders are selected independently; common CFL=0.2. PNGs are linear density plots, not NSH or window screenshots. Finite-volume references use 8x8 quadrature."
        }))?,
    )?;
    let mut csv = fs::File::create(c.output.join("metrics.csv"))?;
    writeln!(
        csv,
        "scenario,resolution,time_order,space_order,requested_time_s,actual_time_s,steps,dt_s,cell_m,mass,relative_mass_error,relative_l1,peak,area_density_ge_half_m2,centroid_error_m,x_sigma_m,gpu_ms,gpu_ms_per_step"
    )?;
    let mut html = String::from(
        "<!doctype html><meta charset='utf-8'><title>泡沫输送对照</title><style>body{background:#17212b;color:#eef;font:16px sans-serif;padding:20px}table{border-collapse:collapse}td,th{border:1px solid #567;padding:8px}img{width:240px;image-rendering:pixelated}a{color:#9cf}th{max-width:180px}p{max-width:1000px;line-height:1.6}</style><h1>泡沫输送形态对照</h1><p>固定 32×32 米俯视范围，+X 向右、+Z 向上。黑色密度 0、白色密度 1，所有图统一线性色标，不自动拉伸对比度。这里只隔离生产泡沫输送，不运行 SWE／FFT，不使用 NSH。解析参考为精确流线回溯后的单元面积近似平均。默认固定 RK2 时间推进，对比一阶空间通量与受限二阶 MUSCL；共同 CFL=0.2。生成、衰减关闭，泡沫立即随规定流场。<a href='metrics.csv'>原始指标 CSV</a> · <a href='manifest.json'>运行参数</a></p>",
    );
    for scene in &c.scenes {
        write!(
            html,
            "<h2>{}</h2><p>{}</p><table><tr><th>场景／时间</th>",
            scene.name(),
            scene.description()
        )?;
        for time in &c.times {
            write!(html, "<th>{time} 模拟秒</th>")?;
        }
        html.push_str("</tr>");
        let mut reference_names = Vec::new();
        for (index, &time) in c.times.iter().enumerate() {
            let name = format!("{}-reference-t{index}", scene.name());
            png(
                c.output.join(format!("{name}.png")),
                &scene.reference(IMAGE_SIZE, time),
                IMAGE_SIZE,
                false,
            )?;
            reference_names.push(name);
        }
        gallery_row(&mut html, "解析参考", &reference_names);
        for &n in &c.resolutions {
            for (&order, &space_order) in c
                .orders
                .iter()
                .flat_map(|order| c.space_orders.iter().map(move |space| (order, space)))
            {
                let mut experiment = Experiment::new(&gpu, *scene, n, order, space_order);
                let mut elapsed = 0.0;
                let mut previous_time = 0.0;
                let mut names = Vec::new();
                let mut errors = Vec::new();
                for (index, &time) in c.times.iter().enumerate() {
                    let (steps, dt, gpu_ms) = if index == 0 {
                        (0, 0.0, None)
                    } else {
                        experiment.advance(&gpu, &pipeline, time - previous_time)?
                    };
                    elapsed += steps as f64 * dt;
                    let actual = experiment.read(&gpu)?;
                    let reference = scene.reference(n, elapsed);
                    let m = metrics(&actual, &reference, n, experiment.initial_mass);
                    if m.relative_mass_error.abs() > 0.001 || (elapsed - time).abs() > 1e-5 {
                        return Err(format!(
                            "{} {n} time order {order} space order {space_order}: invalid mass/time: {} / {}",
                            scene.name(),
                            m.relative_mass_error,
                            elapsed - time
                        )
                        .into());
                    }
                    let name = format!("{}-n{n}-o{order}-s{space_order}-t{index}", scene.name());
                    png(c.output.join(format!("{name}.png")), &actual, n, false)?;
                    let error: Vec<_> = actual
                        .iter()
                        .zip(&reference)
                        .map(|(a, b)| (a - b).abs())
                        .collect();
                    png(c.output.join(format!("{name}-error.png")), &error, n, true)?;
                    writeln!(
                        csv,
                        "{},{n},{order},{space_order},{time},{elapsed:.9},{steps},{dt:.9},{:.6},{:.9},{:.9},{:.6},{:.6},{:.6},{:.6},{:.6},{},{}",
                        scene.name(),
                        SIDE / n as f64,
                        m.mass,
                        m.relative_mass_error,
                        m.relative_l1,
                        m.peak,
                        m.area_above_half,
                        m.centroid_error,
                        m.x_sigma,
                        gpu_ms.map_or_else(String::new, |ms| format!("{ms:.6}")),
                        gpu_ms.map_or_else(String::new, |ms| format!("{:.6}", ms / steps as f64)),
                    )?;
                    println!(
                        "{} n={n} time_order={order} space_order={space_order} t={time:.1}: mass error={:.2e}, L1={:.3}, peak={:.3}, area>=0.5={:.2} m2",
                        scene.name(),
                        m.relative_mass_error,
                        m.relative_l1,
                        m.peak,
                        m.area_above_half
                    );
                    errors.push(format!("{name}-error"));
                    names.push(name);
                    previous_time = time;
                }
                gallery_row(
                    &mut html,
                    &format!(
                        "{n}² / {} / 空间 {space_order} 阶<br>{:.4} 米/格",
                        if order == 1 { "Euler" } else { "RK2" },
                        SIDE / n as f64
                    ),
                    &names,
                );
                gallery_row(&mut html, "绝对密度误差<br>红色 0～1", &errors);
                csv.flush()?;
            }
        }
        html.push_str("</table>");
    }
    html.push_str("<p>相对 L1 = 总绝对密度误差 / 解析参考总量；不是丢失质量的比例。峰值与 ≥0.5 面积衡量可见浓密结构的保留。质心误差单位米；X 标准差在平移场景中反映条带横向扩散，在剪切／旋流中也包含真实形变。边界关闭，初始结构与解析轨迹远离边界；没有人为泡沫扩散项。</p>");
    fs::write(c.output.join("index.html"), html)?;
    println!("Saved {}", c.output.join("index.html").display());
    Ok(())
}
fn main() -> Result<(), Error> {
    if let Some(c) = config()? {
        pollster::block_on(run(c))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shape retention is not covered by the full-ocean conservation checks:
    // accidentally selecting first-order flux still passes those invariants.
    // This uses independent analytic trajectories, not stored output images.
    #[test]
    #[ignore = "requires GPU; cargo test --release --features ocean --bin foam-lab -- --ignored"]
    fn muscl_reduces_shape_error_without_losing_mass_or_overshooting() {
        pollster::block_on(async {
            let gpu = Gpu::new(&wgpu::Instance::default(), None).await.unwrap();
            let pipeline = transport_pipeline(&gpu);
            for scene in Scene::ALL {
                let mut errors = [0.0; 2];
                for space in [1, 2] {
                    let mut run = Experiment::new(&gpu, scene, 256, 2, space);
                    let (steps, dt, _) = run.advance(&gpu, &pipeline, 8.0).unwrap();
                    let actual = run.read(&gpu).unwrap();
                    let reference = scene.reference(256, steps as f64 * dt);
                    let m = metrics(&actual, &reference, 256, run.initial_mass);
                    assert!(
                        m.relative_mass_error.abs() < 1e-4,
                        "{} space {space}: mass {}",
                        scene.name(),
                        m.relative_mass_error
                    );
                    // These prescribed divergence-free flows have exact density <=1.
                    // This bound must NOT be applied to compressible ocean foam.
                    assert!(
                        m.peak <= 1.001,
                        "{} space {space}: peak {}",
                        scene.name(),
                        m.peak
                    );
                    errors[space as usize - 1] = m.relative_l1;
                }
                assert!(
                    errors[1] < 0.8 * errors[0],
                    "{}: MUSCL did not reduce shape error sufficiently: {errors:?}",
                    scene.name()
                );
                eprintln!(
                    "{} RK2 8s relative L1: first={:.6}, MUSCL={:.6}",
                    scene.name(),
                    errors[0],
                    errors[1]
                );
            }
        });
    }
}

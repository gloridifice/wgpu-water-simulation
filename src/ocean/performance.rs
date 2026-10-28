use super::{
    gpu_timer::Sample,
    gui::{parameter, parameter_table},
};
use std::time::{Duration, Instant};

pub(super) const SUMMARY_HELP: &str = "最近 1 秒统计。FPS 包含垂直同步与事件等待；CPU 为绘制入口到 submit/present 返回的帧处理时间，含交换链等待。GPU 是主命令缓冲的真实时间戳，含模拟、场景和 GUI，不含 CPU 编码、呈现等待或额外回调缓冲。SWE 是其中的 GPU 子区间（含泡沫及浪花源，不含粒子更新/绘制），不应与总 GPU 相加；关闭 SWE 时隐藏，暂停无子步时显示空闲。";

const FRAME_GPU_HELP: &str = "真实 GPU 时间戳异步采样，最近 1 秒样本平均。覆盖本帧主命令缓冲中的 FFT、泡沫、SWE、浪花、场景和 GUI；不含 CPU 编码/提交等待、呈现等待或额外 GUI 回调缓冲。SWE 已包含在该区间内。设备不支持时不以 CPU 耗时替代。";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SweStatus {
    Unsupported,
    Failed,
    Idle,
    Active(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum FrameGpuStatus {
    #[default]
    Unsupported,
    Failed,
    Sampling,
}

#[derive(Default)]
pub(super) struct FrameTimings {
    pub interval: f64,
    pub cpu: f64,
    pub acquire: f64,
    pub gui: f64,
    pub simulation: f64,
    pub render: f64,
    pub submit_present: f64,
}

impl FrameTimings {
    fn values(&self) -> [f64; 7] {
        [
            self.interval,
            self.cpu,
            self.acquire,
            self.gui,
            self.simulation,
            self.render,
            self.submit_present,
        ]
    }
}

#[derive(Clone, Copy, Default)]
struct GpuTotals {
    count: u32,
    ms: f64,
    substeps: u32,
    simulated_dt: f64,
}

struct Report {
    frame: [f64; 7],
    gpu: GpuTotals,
    status: SweStatus,
    frame_gpu: GpuTotals,
    frame_gpu_status: FrameGpuStatus,
}

pub(super) struct Performance {
    started: Instant,
    frames: u32,
    sums: [f64; 7],
    gpu: GpuTotals,
    status: SweStatus,
    frame_gpu: GpuTotals,
    frame_gpu_status: FrameGpuStatus,
    report: Option<Report>,
}

impl Default for Performance {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl Performance {
    fn new(now: Instant) -> Self {
        Self {
            started: now,
            frames: 0,
            sums: [0.0; 7],
            gpu: GpuTotals::default(),
            status: SweStatus::Unsupported,
            frame_gpu: GpuTotals::default(),
            frame_gpu_status: FrameGpuStatus::Unsupported,
            report: None,
        }
    }

    pub fn record_gpu(&mut self, status: SweStatus, sample: Option<Sample>) {
        if self.status != status {
            self.gpu = GpuTotals::default();
            self.status = status;
        }
        if let Some(sample) = sample.filter(|s| status == SweStatus::Active(s.order)) {
            self.gpu.count += 1;
            self.gpu.ms += sample.ms;
            self.gpu.substeps += sample.substeps;
            self.gpu.simulated_dt += sample.simulated_dt as f64;
        }
    }

    pub fn record_frame_gpu(&mut self, status: FrameGpuStatus, sample: Option<Sample>) {
        if status != self.frame_gpu_status {
            self.frame_gpu = GpuTotals::default();
            self.frame_gpu_status = status;
        }
        if status == FrameGpuStatus::Sampling
            && let Some(sample) = sample
        {
            self.frame_gpu.count += 1;
            self.frame_gpu.ms += sample.ms;
        }
    }

    fn frame_gpu_text(&self) -> String {
        match self.frame_gpu_status {
            FrameGpuStatus::Unsupported => "不支持".into(),
            FrameGpuStatus::Failed => "读取失败".into(),
            FrameGpuStatus::Sampling => self
                .report
                .as_ref()
                .filter(|r| r.frame_gpu_status == self.frame_gpu_status && r.frame_gpu.count > 0)
                .map_or_else(
                    || "等待采样".into(),
                    |r| format!("{:.2} ms", r.frame_gpu.ms / r.frame_gpu.count as f64),
                ),
        }
    }

    fn swe_text(&self) -> String {
        match self.status {
            SweStatus::Unsupported => "不支持".into(),
            SweStatus::Failed => "读取失败".into(),
            SweStatus::Idle => "空闲".into(),
            SweStatus::Active(_) => self
                .report
                .as_ref()
                .filter(|r| r.status == self.status && r.gpu.count > 0)
                .map_or_else(
                    || "等待采样".into(),
                    |r| format!("{:.2} ms", r.gpu.ms / r.gpu.count as f64),
                ),
        }
    }

    pub fn summary_text(&self, swe_enabled: bool) -> String {
        let frame = self.report.as_ref().map(|r| r.frame);
        let fps = frame.map_or_else(
            || "等待采样".into(),
            |f| format!("{:.1}", 1000.0 / f[0].max(f64::EPSILON)),
        );
        let cpu = frame.map_or_else(|| "等待采样".into(), |f| format!("{:.2} ms", f[1]));
        let mut text = format!("FPS {fps}   CPU {cpu}   GPU {}", self.frame_gpu_text());
        if swe_enabled {
            text.push_str(&format!("   SWE {}", self.swe_text()));
        }
        text
    }

    pub fn record_frame(&mut self, now: Instant, timings: FrameTimings) {
        self.frames += 1;
        for (sum, value) in self.sums.iter_mut().zip(timings.values()) {
            *sum += value;
        }
        if now.duration_since(self.started) < Duration::from_secs(1) {
            return;
        }
        self.report = Some(Report {
            frame: self.sums.map(|sum| sum / self.frames as f64),
            gpu: self.gpu,
            status: self.status,
            frame_gpu: self.frame_gpu,
            frame_gpu_status: self.frame_gpu_status,
        });
        self.started = now;
        self.frames = 0;
        self.sums = [0.0; 7];
        self.gpu = GpuTotals::default();
        self.frame_gpu = GpuTotals::default();
    }

    pub fn ui(&self, ui: &mut egui::Ui) {
        let frame = self.report.as_ref().map(|report| report.frame);
        let ms = |value: Option<f64>| {
            value.map_or_else(|| "等待采样".into(), |v| format!("{v:.3} 毫秒"))
        };
        parameter_table(ui, "performance table", |ui| {
            parameter(
                ui,
                "指标",
                "每 1 秒刷新一次，CPU 数据取该窗口内每帧平均值；GPU 数据只统计新读回的采样。",
                |ui| ui.strong("数值"),
            );
            parameter(
                ui,
                "帧率",
                "由未截断的平均帧间隔计算，单位帧／秒，包含垂直同步与事件循环等待。",
                |ui| {
                    ui.label(frame.map_or_else(
                        || "等待采样".into(),
                        |f| format!("{:.1} FPS", 1000.0 / f[0].max(f64::EPSILON)),
                    ))
                },
            );
            for (index, name, help) in [
                (
                    0,
                    "端到端帧间隔",
                    "相邻两次绘制开始之间的现实时间，包含 CPU、GPU 背压、垂直同步与事件循环等待。不是输入到屏幕显示的延迟；不受模拟时间的 0.1 秒截断影响。",
                ),
                (
                    1,
                    "CPU 帧处理",
                    "从绘制入口到提交并调用 present 返回的现实耗时，包含界面、配置处理、指令编码与阻塞等待；不表示 GPU 执行完成。",
                ),
                (
                    2,
                    "获取画面",
                    "CPU 获取交换链图像的等待耗时，可能含 GPU 背压与垂直同步等待。",
                ),
                (
                    3,
                    "界面 CPU",
                    "egui 参数面板布局与交互处理耗时，不包含界面绘制指令编码。",
                ),
                (
                    4,
                    "模拟 CPU",
                    "FFT、远海泡沫、近岸 SWE 与浪花源/粒子的 CPU 指令编码及参数上传耗时，不是 GPU 执行耗时。",
                ),
                (
                    5,
                    "渲染编码 CPU",
                    "场景与 egui 绘制指令编码耗时，不是 GPU 渲染耗时。",
                ),
                (
                    6,
                    "提交与呈现 CPU",
                    "命令缓冲完成、队列提交和 present 调用返回的现实耗时，不等待 GPU 执行完成或屏幕扫描输出。",
                ),
            ] {
                parameter(ui, name, help, |ui| ui.label(ms(frame.map(|f| f[index]))));
            }
            parameter(ui, "GPU 主帧", FRAME_GPU_HELP, |ui| {
                ui.label(self.frame_gpu_text())
            });
            let report = self.report.as_ref();
            let gpu = report
                .filter(|r| matches!(r.status, SweStatus::Active(_)) && r.gpu.count > 0)
                .map(|r| r.gpu);
            let status = match report.map(|r| r.status) {
                None => "等待采样",
                Some(SweStatus::Unsupported) => "设备不支持时间戳",
                Some(SweStatus::Failed) => "时间戳读取失败",
                Some(SweStatus::Idle) => "空闲（无模拟子步）",
                Some(SweStatus::Active(_)) => "等待当前求解器采样",
            };
            parameter(
                ui,
                "SWE GPU",
                "真实 GPU 时间戳的采样平均耗时；包含 SWE、近岸泡沫和中间子步复制，开启浪花时包含全部发射信号 pass。不包含 FFT、远海泡沫、粒子更新/绘制及首尾复制。",
                |ui| {
                    ui.label(gpu.map_or_else(|| status.into(), |g| ms(Some(g.ms / g.count as f64))))
                },
            );
            parameter(
                ui,
                "SWE GPU／子步",
                "GPU 采样总耗时除以采样总子步数。一阶为欧拉子步，二阶为完整 RK2 子步，单位毫秒／子步。",
                |ui| {
                    ui.label(gpu.map_or_else(
                        || status.into(),
                        |g| ms(Some(g.ms / g.substeps.max(1) as f64)),
                    ))
                },
            );
            parameter(
                ui,
                "SWE 模拟推进",
                "GPU 采样中每帧实际推进的模拟时长，单位毫秒；受时间倍率、CFL 与子步上限影响，不是计算耗时。",
                |ui| {
                    ui.label(gpu.map_or_else(
                        || status.into(),
                        |g| ms(Some(g.simulated_dt * 1000.0 / g.count as f64)),
                    ))
                },
            );
            parameter(
                ui,
                "SWE 子步／帧",
                "GPU 采样内每帧平均子步数。",
                |ui| {
                    ui.label(gpu.map_or_else(
                        || status.into(),
                        |g| format!("{:.1}", g.substeps as f64 / g.count as f64),
                    ))
                },
            );
            parameter(
                ui,
                "SWE 求解阶数",
                "当前统计窗口对应的求解器阶数；切换阶数时丢弃尚未发布的旧阶数 GPU 采样。",
                |ui| {
                    ui.label(match report.map(|r| r.status) {
                        Some(SweStatus::Active(order)) => format!("{order} 阶"),
                        _ => "--".into(),
                    })
                },
            );
            parameter(
                ui,
                "SWE GPU 采样数",
                "最近一秒内成功读回且匹配当前求解器的新 GPU 样本数；异步读取，可能少于绘制帧数。",
                |ui| ui.label(report.map_or_else(|| "--".into(), |r| r.gpu.count.to_string())),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_window_average_once_per_second() {
        let start = Instant::now();
        let mut performance = Performance::new(start);
        let frame = |interval, cpu| FrameTimings {
            interval,
            cpu,
            ..Default::default()
        };
        performance.record_frame(start + Duration::from_millis(400), frame(400.0, 2.0));
        performance.record_frame(start + Duration::from_millis(999), frame(599.0, 4.0));
        assert!(performance.report.is_none());
        performance.record_frame(start + Duration::from_secs(1), frame(1.0, 6.0));
        let report = performance.report.as_ref().unwrap();
        assert!((report.frame[0] - 1000.0 / 3.0).abs() < 1e-9);
        assert_eq!(report.frame[1], 4.0);
        performance.record_frame(start + Duration::from_millis(1999), frame(999.0, 8.0));
        assert_eq!(performance.report.as_ref().unwrap().frame[1], 4.0);
        performance.record_frame(start + Duration::from_secs(2), frame(1.0, 12.0));
        assert_eq!(performance.report.as_ref().unwrap().frame[1], 10.0);
    }

    #[test]
    fn gpu_samples_exclude_old_solver_and_idle_history() {
        let start = Instant::now();
        let mut performance = Performance::new(start);
        let sample = |order, ms| Sample {
            order,
            ms,
            substeps: 2,
            simulated_dt: 0.01,
        };
        performance.record_frame_gpu(FrameGpuStatus::Sampling, Some(sample(0, 2.0)));
        performance.record_frame_gpu(FrameGpuStatus::Sampling, None);
        performance.record_frame_gpu(FrameGpuStatus::Sampling, Some(sample(0, 6.0)));
        performance.record_gpu(SweStatus::Active(1), Some(sample(1, 20.0)));
        performance.record_gpu(SweStatus::Active(2), Some(sample(1, 30.0)));
        performance.record_gpu(SweStatus::Active(2), Some(sample(2, 2.0)));
        performance.record_gpu(SweStatus::Active(2), None);
        performance.record_gpu(SweStatus::Active(2), Some(sample(2, 4.0)));
        performance.record_frame(start + Duration::from_secs(1), FrameTimings::default());
        let report = performance.report.as_ref().unwrap();
        assert_eq!(report.frame_gpu.count, 2);
        assert_eq!(report.frame_gpu.ms / report.frame_gpu.count as f64, 4.0);
        assert_eq!(report.gpu.count, 2);
        assert_eq!(report.gpu.ms / report.gpu.count as f64, 3.0);
        assert_eq!(report.gpu.ms / report.gpu.substeps as f64, 1.5);
        performance.record_gpu(SweStatus::Active(2), Some(sample(2, 10.0)));
        performance.record_gpu(SweStatus::Idle, None);
        performance.record_frame_gpu(FrameGpuStatus::Failed, None);
        assert_eq!(performance.frame_gpu_text(), "读取失败");
        performance.record_frame(start + Duration::from_secs(2), FrameTimings::default());
        let report = performance.report.as_ref().unwrap();
        assert_eq!(report.status, SweStatus::Idle);
        assert_eq!(report.gpu.count, 0);
        assert_eq!(report.frame_gpu.count, 0);
    }
}

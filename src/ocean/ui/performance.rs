use crate::ocean::gpu::timer::Sample;
use std::time::{Duration, Instant};
use yummy_egui::{parameter, parameter_table};

pub(super) const SUMMARY_HELP: &str = "Statistics for the last second. FPS includes vertical sync and event waits; CPU is frame processing from draw entry through submit/present return, including swapchain waits. GPU uses actual timestamps for the main command buffer, including simulation, scene and GUI but excluding CPU encoding, presentation waits and extra callback buffers. SWE is a GPU subinterval (including foam and spray sources, but not particle updates/drawing) and must not be added to total GPU time. Hidden when SWE is off; idle when paused without substeps.";

const FRAME_GPU_HELP: &str = "Asynchronously sampled GPU timestamps, averaged over the last second. Covers FFT, foam, SWE, spray, scene and GUI in the main frame command buffer; excludes CPU encoding/submission waits, presentation waits and extra GUI callback buffers. SWE is included. CPU time is not substituted on unsupported devices.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ocean) enum SweStatus {
    Unsupported,
    Failed,
    Idle,
    Active(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ocean) enum FrameGpuStatus {
    #[default]
    Unsupported,
    Failed,
    Sampling,
}

#[derive(Default)]
pub(in crate::ocean) struct FrameTimings {
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

pub(in crate::ocean) struct Performance {
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
            FrameGpuStatus::Unsupported => "Unsupported".into(),
            FrameGpuStatus::Failed => "Read failed".into(),
            FrameGpuStatus::Sampling => self
                .report
                .as_ref()
                .filter(|r| r.frame_gpu_status == self.frame_gpu_status && r.frame_gpu.count > 0)
                .map_or_else(
                    || "Awaiting sample".into(),
                    |r| format!("{:.2} ms", r.frame_gpu.ms / r.frame_gpu.count as f64),
                ),
        }
    }

    fn swe_text(&self) -> String {
        match self.status {
            SweStatus::Unsupported => "Unsupported".into(),
            SweStatus::Failed => "Read failed".into(),
            SweStatus::Idle => "Idle".into(),
            SweStatus::Active(_) => self
                .report
                .as_ref()
                .filter(|r| r.status == self.status && r.gpu.count > 0)
                .map_or_else(
                    || "Awaiting sample".into(),
                    |r| format!("{:.2} ms", r.gpu.ms / r.gpu.count as f64),
                ),
        }
    }

    pub fn summary_text(&self, swe_enabled: bool) -> String {
        let frame = self.report.as_ref().map(|r| r.frame);
        let fps = frame.map_or_else(
            || "Awaiting sample".into(),
            |f| format!("{:.1}", 1000.0 / f[0].max(f64::EPSILON)),
        );
        let cpu = frame.map_or_else(|| "Awaiting sample".into(), |f| format!("{:.2} ms", f[1]));
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
            value.map_or_else(|| "Awaiting sample".into(), |v| format!("{v:.3} ms"))
        };
        parameter_table(ui, "performance table", |ui| {
            parameter(
                ui,
                "Metric",
                "Updated every second. CPU values are per-frame averages in the window; GPU values include only newly read samples.",
                |ui| yummy_egui::h3(ui, "Value"),
            );
            parameter(
                ui,
                "Frame rate",
                "Calculated from the unclamped average frame interval, in frames per second, including vertical sync and event loop waits.",
                |ui| {
                    ui.label(frame.map_or_else(
                        || "Awaiting sample".into(),
                        |f| format!("{:.1} FPS", 1000.0 / f[0].max(f64::EPSILON)),
                    ))
                },
            );
            for (index, name, help) in [
                (
                    0,
                    "End-to-end frame interval",
                    "Wall time between successive draw starts, including CPU, GPU backpressure, vertical sync and event loop waits. Not input-to-display latency; unaffected by the 0.1 s simulation time clamp.",
                ),
                (
                    1,
                    "CPU frame processing",
                    "Wall time from draw entry through submit and present return, including UI, settings, command encoding and blocking waits; does not imply GPU completion.",
                ),
                (
                    2,
                    "Acquire image",
                    "CPU wait for the swapchain image, potentially including GPU backpressure and vertical sync waits.",
                ),
                (
                    3,
                    "UI CPU",
                    "Time spent on egui panel layout and interaction, excluding UI draw command encoding.",
                ),
                (
                    4,
                    "Simulation CPU",
                    "CPU command encoding and parameter uploads for FFT, offshore foam, nearshore SWE and spray sources/particles; not GPU execution time.",
                ),
                (
                    5,
                    "Render encoding CPU",
                    "Time encoding scene and egui draw commands, not GPU rendering time.",
                ),
                (
                    6,
                    "Submit and present CPU",
                    "Wall time to finish command buffers, submit the queue and return from present; does not wait for GPU completion or display scanout.",
                ),
            ] {
                parameter(ui, name, help, |ui| ui.label(ms(frame.map(|f| f[index]))));
            }
            parameter(ui, "Main frame GPU", FRAME_GPU_HELP, |ui| {
                ui.label(self.frame_gpu_text())
            });
            let report = self.report.as_ref();
            let gpu = report
                .filter(|r| matches!(r.status, SweStatus::Active(_)) && r.gpu.count > 0)
                .map(|r| r.gpu);
            let status = match report.map(|r| r.status) {
                None => "Awaiting sample",
                Some(SweStatus::Unsupported) => "Device does not support timestamps",
                Some(SweStatus::Failed) => "Timestamp read failed",
                Some(SweStatus::Idle) => "Idle (no simulation substeps)",
                Some(SweStatus::Active(_)) => "Awaiting current solver sample",
            };
            parameter(
                ui,
                "SWE GPU",
                "Average GPU timestamp duration; includes SWE, nearshore foam and intermediate substep copies, plus all emission signal passes when spray is enabled. Excludes FFT, offshore foam, particle updates/drawing and initial/final copies.",
                |ui| {
                    ui.label(gpu.map_or_else(|| status.into(), |g| ms(Some(g.ms / g.count as f64))))
                },
            );
            parameter(
                ui,
                "SWE GPU / substep",
                "Total sampled GPU time divided by sampled substeps. First order uses Euler substeps; second order uses complete RK2 substeps, in ms/substep.",
                |ui| {
                    ui.label(gpu.map_or_else(
                        || status.into(),
                        |g| ms(Some(g.ms / g.substeps.max(1) as f64)),
                    ))
                },
            );
            parameter(
                ui,
                "SWE simulation advance",
                "Simulation time advanced per frame in GPU samples, in milliseconds. Affected by time scale, CFL and the substep limit; not computation time.",
                |ui| {
                    ui.label(gpu.map_or_else(
                        || status.into(),
                        |g| ms(Some(g.simulated_dt * 1000.0 / g.count as f64)),
                    ))
                },
            );
            parameter(
                ui,
                "SWE substeps / frame",
                "Average substeps per frame in GPU samples.",
                |ui| {
                    ui.label(gpu.map_or_else(
                        || status.into(),
                        |g| format!("{:.1}", g.substeps as f64 / g.count as f64),
                    ))
                },
            );
            parameter(
                ui,
                "SWE solver order",
                "Solver order for the current statistics window; unpublished GPU samples from the old order are discarded on switching.",
                |ui| {
                    ui.label(match report.map(|r| r.status) {
                        Some(SweStatus::Active(order)) => format!("Order {order}"),
                        _ => "--".into(),
                    })
                },
            );
            parameter(
                ui,
                "SWE GPU samples",
                "New GPU samples successfully read in the last second and matching the current solver; asynchronous reads may yield fewer samples than rendered frames.",
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
        assert_eq!(performance.frame_gpu_text(), "Read failed");
        performance.record_frame(start + Duration::from_secs(2), FrameTimings::default());
        let report = performance.report.as_ref().unwrap();
        assert_eq!(report.status, SweStatus::Idle);
        assert_eq!(report.gpu.count, 0);
        assert_eq!(report.frame_gpu.count, 0);
    }
}

//! Ocean metadata adapter over the shared non-blocking GPU timestamp sampler.
use super::context::Gpu;
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub ms: f64,
    pub order: u32,
    pub substeps: u32,
    pub simulated_dt: f32,
}
pub struct GpuTimer {
    inner: common::gpu::timer::GpuTimer,
    metadata: (u32, u32, f32),
    pub latest: Option<Sample>,
}
impl std::ops::Deref for GpuTimer {
    type Target = common::gpu::timer::GpuTimer;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl GpuTimer {
    pub fn new(gpu: &Gpu) -> Option<Self> {
        Some(Self {
            inner: common::gpu::timer::GpuTimer::new(gpu)?,
            metadata: (1, 0, 0.0),
            latest: None,
        })
    }
    pub fn resolve(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        order: u32,
        substeps: u32,
        dt: f32,
    ) {
        self.metadata = (order, substeps, dt);
        self.inner.resolve(encoder);
    }
    pub fn poll(&mut self, gpu: &Gpu) {
        self.inner.poll(gpu);
        if let Some(sample) = self.inner.latest.take() {
            self.latest = Some(Sample {
                ms: sample.ms,
                order: self.metadata.0,
                substeps: self.metadata.1,
                simulated_dt: self.metadata.2,
            });
        }
    }
}

//! Non-blocking, sampled GPU span timing. One outstanding readback; no frame stalls.
use super::Gpu;
use std::sync::mpsc::{self, Receiver};

#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub ms: f64,
}
pub struct GpuTimer {
    pub queries: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback: wgpu::Buffer,
    pending: Option<Receiver<Result<(), wgpu::BufferAsyncError>>>,
    pub latest: Option<Sample>,
    pub failed: bool,
}
impl GpuTimer {
    pub fn new(gpu: &Gpu) -> Option<Self> {
        if !gpu
            .device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            return None;
        }
        let queries = gpu.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("GPU span timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let buffer = |usage| {
            gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("GPU span readback"),
                size: 256,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            queries,
            resolve_buffer: buffer(
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            readback: buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ),
            pending: None,
            latest: None,
            failed: false,
        })
    }
    /// Portable frame boundary without requiring TIMESTAMP_QUERY_INSIDE_ENCODERS.
    pub fn mark(&self, encoder: &mut wgpu::CommandEncoder, index: u32) {
        let _pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("GPU frame timing boundary"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: &self.queries,
                beginning_of_pass_write_index: Some(index),
                end_of_pass_write_index: None,
            }),
        });
    }
    pub fn ready(&self) -> bool {
        self.pending.is_none() && !self.failed
    }
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.queries, 0..2, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(&self.resolve_buffer, 0, &self.readback, 0, 16);
        let (tx, rx) = mpsc::channel();
        encoder.map_buffer_on_submit(&self.readback, wgpu::MapMode::Read, 0..16, move |result| {
            let _ = tx.send(result);
        });
        self.pending = Some(rx);
    }
    pub fn poll(&mut self, gpu: &Gpu) {
        let _ = gpu.device.poll(wgpu::PollType::Poll);
        let Some(rx) = &self.pending else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                let bytes = self.readback.slice(0..16).get_mapped_range();
                let start = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
                let end = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                let ms =
                    end.wrapping_sub(start) as f64 * gpu.queue.get_timestamp_period() as f64 / 1e6;
                drop(bytes);
                self.readback.unmap();
                self.pending = None;
                self.latest = Some(Sample { ms });
            }
            Err(mpsc::TryRecvError::Empty) => {}
            _ => {
                self.pending = None;
                self.failed = true;
                self.latest = None;
            }
        }
    }
}

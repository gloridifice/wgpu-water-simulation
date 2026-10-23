//! 单个场景 render pass 的真实 GPU timestamp 计时。
//!
//! 最多保留一个异步回读；尚未返回时跳过下一次采样，绘制不等待 GPU。
//! 查询仅包住场景 pass，上传、egui、查询回读和呈现均不计入数值。

use std::sync::mpsc::{self, Receiver, TryRecvError};

pub(super) struct SceneGpuTimer {
    pub supported: bool,
    pub failed: bool,
    pub latest_ms: Option<f64>,
    timestamp_period_ns: f64,
    resources: Option<Resources>,
}

struct Resources {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    pending: Option<Receiver<Result<(), wgpu::BufferAsyncError>>>,
}

impl SceneGpuTimer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let supported = device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let timestamp_period_ns = f64::from(queue.get_timestamp_period());
        let failed = supported && (!timestamp_period_ns.is_finite() || timestamp_period_ns <= 0.0);
        let resources = if supported && !failed {
            Some(Resources {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("demo scene GPU timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("demo timestamp resolve"),
                    size: wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                readback: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("demo timestamp async readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
                pending: None,
            })
        } else {
            None
        };
        Self {
            supported,
            failed,
            latest_ms: None,
            timestamp_period_ns,
            resources,
        }
    }

    pub fn ready(&self) -> bool {
        !self.failed
            && self
                .resources
                .as_ref()
                .is_some_and(|resources| resources.pending.is_none())
    }

    pub fn timestamp_writes(&self) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        if !self.ready() {
            return None;
        }
        let resources = self.resources.as_ref()?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &resources.queries,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        })
    }

    /// 必须在对应 scene pass 结束后调用，并提交同一个 encoder。
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if !self.ready() {
            return;
        }
        let Some(resources) = &mut self.resources else {
            return;
        };
        encoder.resolve_query_set(&resources.queries, 0..2, &resources.resolve, 0);
        encoder.copy_buffer_to_buffer(&resources.resolve, 0, &resources.readback, 0, 16);
        let (sender, receiver) = mpsc::channel();
        encoder.map_buffer_on_submit(
            &resources.readback,
            wgpu::MapMode::Read,
            0..16,
            move |result| {
                let _ = sender.send(result);
            },
        );
        resources.pending = Some(receiver);
    }

    /// 只轮询已完成的 map，不调用 wait，也不等待通道收到消息。
    pub fn poll(&mut self, device: &wgpu::Device) {
        if self.failed {
            return;
        }
        let Some(resources) = &mut self.resources else {
            return;
        };
        if device.poll(wgpu::PollType::Poll).is_err() {
            resources.readback.unmap();
            resources.pending = None;
            self.failed = true;
            self.latest_ms = None;
            return;
        }
        let Some(receiver) = &resources.pending else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(())) => {
                let mapped = resources.readback.slice(0..16).get_mapped_range();
                let start = u64::from_le_bytes(mapped[..8].try_into().expect("8 timestamp bytes"));
                let end = u64::from_le_bytes(mapped[8..16].try_into().expect("8 timestamp bytes"));
                drop(mapped);
                resources.readback.unmap();
                resources.pending = None;
                let milliseconds =
                    end.wrapping_sub(start) as f64 * self.timestamp_period_ns / 1_000_000.0;
                if milliseconds.is_finite() {
                    self.latest_ms = Some(milliseconds);
                } else {
                    self.failed = true;
                    self.latest_ms = None;
                }
            }
            Err(TryRecvError::Empty) => {}
            Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                resources.readback.unmap();
                resources.pending = None;
                self.failed = true;
                self.latest_ms = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "需要实际 GPU；验证真实 timestamp 与异步 map"]
    fn asynchronous_scene_timestamp_reports_sample_and_unsupported_state() {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .unwrap();
            let features = adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor {
                    required_features: features,
                    ..Default::default()
                })
                .await
                .unwrap();
            let mut timer = SceneGpuTimer::new(&device, &queue);
            assert_eq!(
                timer.supported,
                features.contains(wgpu::Features::TIMESTAMP_QUERY)
            );
            assert!(!timer.failed);
            assert!(timer.latest_ms.is_none());
            if timer.supported {
                let target = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("timestamp verification scene"),
                    size: wgpu::Extent3d {
                        width: 32,
                        height: 32,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                });
                let view = target.create_view(&Default::default());
                let mut encoder = device.create_command_encoder(&Default::default());
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("timed verification pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: timer.timestamp_writes(),
                        occlusion_query_set: None,
                    });
                }
                timer.resolve(&mut encoder);
                assert!(!timer.ready());
                queue.submit([encoder.finish()]);
                let deadline = Instant::now() + Duration::from_secs(5);
                while timer.latest_ms.is_none() && !timer.failed && Instant::now() < deadline {
                    timer.poll(&device);
                    std::thread::sleep(Duration::from_millis(1));
                }
                assert!(!timer.failed);
                assert!(
                    timer
                        .latest_ms
                        .is_some_and(|ms| ms.is_finite() && ms >= 0.0)
                );
                assert!(timer.ready());
                println!(
                    "GPU scene timestamp: {:.6} ms on {}",
                    timer.latest_ms.unwrap(),
                    adapter.get_info().name
                );
            } else {
                assert!(!timer.ready());
                assert!(timer.timestamp_writes().is_none());
                println!(
                    "GPU adapter does not support timestamp queries; unsupported state verified"
                );
            }
            // 同一适配器不请求可选 feature 时，应用仍能正常使用显式 unsupported 状态。
            let (plain_device, plain_queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .unwrap();
            let plain = SceneGpuTimer::new(&plain_device, &plain_queue);
            assert!(
                !plain.supported && !plain.failed && plain.latest_ms.is_none() && !plain.ready()
            );
        });
    }
}

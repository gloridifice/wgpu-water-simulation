//! 独立 wgpu 计算后端。数值更新由 WGSL 执行，CPU 整理自适应拓扑和调度。
//!
//! 当前阶段之间同步回读，适合离线验证；不承诺实时性能或完全 GPU 驻留。

mod adaptivity;
mod pressure;
mod simulation;
mod surface;
mod transport;

pub use adaptivity::build_adaptive;
pub use pressure::project;
pub use simulation::{GpuSimulation, default_config};
pub use surface::extract_surface;

use bytemuck::Pod;
use std::collections::HashMap;
use std::sync::Mutex;
use wgpu::util::DeviceExt;

/// 无窗口的计算设备；没有适配器时显式报错，绝不静默退回 CPU 求解。
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_name: String,
    pipelines: Mutex<HashMap<String, wgpu::ComputePipeline>>,
}

impl GpuContext {
    pub async fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("找不到计算适配器：{e}"))?;
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("tall-cell-fluid offline compute"),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("创建设备失败：{e}"))?;
        Ok(Self {
            device,
            queue,
            adapter_name: format!("{} ({:?}, {:?})", info.name, info.backend, info.device_type),
            pipelines: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn buffer<T: Pod>(&self, label: &str, data: &[T]) -> wgpu::Buffer {
        let bytes = bytemuck::cast_slice(data);
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if bytes.is_empty() { &[0; 4] } else { bytes },
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
    }

    pub(crate) fn uniform<T: Pod>(&self, label: &str, data: &T) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
    }

    pub(crate) fn pipeline(
        &self,
        label: &str,
        source: &str,
        entry: &str,
    ) -> Result<wgpu::ComputePipeline, String> {
        let key = format!("{entry}\n{source}");
        let mut cache = self
            .pipelines
            .lock()
            .map_err(|_| "GPU pipeline cache poisoned")?;
        if let Some(pipeline) = cache.get(&key) {
            return Ok(pipeline.clone());
        }
        self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
        if let Some(error) = pollster::block_on(self.device.pop_error_scope()) {
            return Err(format!("{label}: {error}"));
        }
        cache.insert(key, pipeline.clone());
        Ok(pipeline)
    }

    pub(crate) fn bind(
        &self,
        pipeline: &wgpu::ComputePipeline,
        buffers: &[&wgpu::Buffer],
    ) -> wgpu::BindGroup {
        let numbered: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| (i as u32, *b))
            .collect();
        self.bind_at(pipeline, &numbered)
    }

    pub(crate) fn bind_at(
        &self,
        pipeline: &wgpu::ComputePipeline,
        buffers: &[(u32, &wgpu::Buffer)],
    ) -> wgpu::BindGroup {
        let entries: Vec<_> = buffers
            .iter()
            .map(|&(binding, buffer)| wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tall cell compute bindings"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        })
    }

    pub(crate) fn dispatch(
        &self,
        pipeline: &wgpu::ComputePipeline,
        bindings: &wgpu::BindGroup,
        groups: u32,
    ) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("tall cell compute"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bindings, &[]);
            pass.dispatch_workgroups(groups, 1, 1);
        }
        self.queue.submit([encoder.finish()]);
    }

    pub(crate) fn read<T: Pod>(
        &self,
        buffer: &wgpu::Buffer,
        count: usize,
    ) -> Result<Vec<T>, String> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let size = count
            .checked_mul(std::mem::size_of::<T>())
            .ok_or("回读大小溢出")? as u64;
        if size > buffer.size() || !size.is_multiple_of(4) {
            return Err("回读范围或对齐不合法".into());
        }
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offline GPU readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let mapped = staging.slice(..).get_mapped_range();
        // 不依赖 Vec<u8> 的分配对齐。
        let values = mapped
            .chunks_exact(std::mem::size_of::<T>())
            .map(bytemuck::pod_read_unaligned)
            .collect();
        drop(mapped);
        staging.unmap();
        Ok(values)
    }

    pub(crate) fn validate_grid(&self, grid: &crate::Grid) -> Result<(), String> {
        grid.validate()?;
        let count =
            2 * grid.domain.cell_count() + grid.velocity.faces.iter().map(Vec::len).sum::<usize>();
        let limits = self.device.limits();
        if count > u32::MAX as usize / 64
            || (count as u64 * 4) > u64::from(limits.max_storage_buffer_binding_size)
            || count.div_ceil(64) > limits.max_compute_workgroups_per_dimension as usize
        {
            return Err("网格超过当前 GPU 单缓冲区/dispatch 上限".into());
        }
        let h = grid.domain.cell_size as f32;
        if !h.is_normal()
            || !(h * h).is_normal()
            || !(h * h).recip().is_finite()
            || grid.domain.origin.iter().enumerate().any(|(axis, &v)| {
                let base = v as f32;
                let end = (v + grid.domain.extent()[axis]) as f32;
                !end.is_finite() || base + h == base || end + h == end
            })
            || grid
                .phi
                .iter()
                .chain(&grid.solid_phi)
                .chain(&grid.pressure)
                .chain(grid.velocity.faces.iter().flatten())
                .any(|v| {
                    !(*v as f32).is_finite() || (*v != 0.0 && (*v as f32).abs() < f32::MIN_POSITIVE)
                })
        {
            return Err("网格数值无法由 GPU f32 有效表示".into());
        }
        Ok(())
    }
}

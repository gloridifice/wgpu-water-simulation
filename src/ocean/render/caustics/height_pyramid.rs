//! Conservative min/max quadtree over the uploaded, sea-relative model bed.
use crate::ocean::{
    gpu::context::{Gpu, buffer},
    simulation::nearshore::Shore,
};
use std::cell::Cell;

pub(super) struct HeightPyramid {
    pub ranges: wgpu::Buffer,
    first: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    levels: Vec<(wgpu::BindGroup, u32)>,
    dirty: Cell<bool>,
}

impl HeightPyramid {
    pub fn new(gpu: &Gpu, shore: &Shore) -> Self {
        let device = &gpu.device;
        let n = shore.resolution;
        let count = (0..=n.ilog2())
            .map(|level| (n >> level).pow(2))
            .sum::<u32>();
        let ranges = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("model height min/max pyramid"),
            size: u64::from(count) * 8,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("height min/max reduction"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/height_build.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("height reduction bindings"),
            entries: &[0, 1, 2].map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: match binding {
                        0 => wgpu::BufferBindingType::Uniform,
                        1 => wgpu::BufferBindingType::Storage { read_only: true },
                        _ => wgpu::BufferBindingType::Storage { read_only: false },
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("height pyramid pipeline"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let levels = (0..=n.ilog2())
            .map(|level| {
                let params = buffer(
                    device,
                    "height level",
                    &[[n, level, 0u32, 0u32]],
                    wgpu::BufferUsages::UNIFORM,
                );
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("height pyramid level"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: shore.terrain_bed.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: ranges.as_entire_binding(),
                        },
                    ],
                });
                (bind, n >> level)
            })
            .collect();
        Self {
            ranges,
            first: pipeline("base"),
            reduce: pipeline("reduce"),
            levels,
            dirty: Cell::new(true),
        }
    }

    pub fn invalidate(&self) {
        self.dirty.set(true);
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        if !self.dirty.replace(false) {
            return;
        }
        for (level, (bind, n)) in self.levels.iter().enumerate() {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("static height pyramid"),
                ..Default::default()
            });
            pass.set_pipeline(if level == 0 {
                &self.first
            } else {
                &self.reduce
            });
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(n.div_ceil(8), n.div_ceil(8), 1);
        }
    }
}

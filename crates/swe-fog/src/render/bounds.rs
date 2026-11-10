//! GPU-only render support envelopes. Cached by publication and reconstruction,
//! independent of the camera; never writes the solver's published field.
use crate::parameters::Reconstruction;
use common::gpu::{Gpu, buffer};
use std::cell::Cell;

pub(super) struct LayerHierarchy {
    pub lower: wgpu::Texture,
    pub upper: wgpu::Texture,
    leaf_pipeline: wgpu::ComputePipeline,
    leaf_bind: wgpu::BindGroup,
    reduce_pipeline: wgpu::ComputePipeline,
    reduce_binds: Vec<wgpu::BindGroup>,
    parameters: wgpu::Buffer,
    padded: [u32; 2],
    cached: Cell<Option<(u64, u32, u32)>>,
}
impl LayerHierarchy {
    pub fn new(
        gpu: &Gpu,
        field: &wgpu::Texture,
        smoothed: &wgpu::Texture,
        bed: &wgpu::Buffer,
        masks: &wgpu::Buffer,
        resolution: [u32; 2],
        tree: bool,
    ) -> Self {
        let device = &gpu.device;
        let padded = resolution.map(u32::next_power_of_two);
        let levels = if tree {
            padded[0].max(padded[1]).ilog2() + 1
        } else {
            1
        };
        let make_texture = |label| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: padded[0],
                    height: padded[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: levels,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        };
        let lower = make_texture("fog lower support bounds");
        let upper = make_texture("fog upper support bounds");
        let parameters = buffer(
            device,
            "fog bound reconstruction",
            &[[0.0f32; 4]],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog support bounds"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}\n{}",
                    include_str!("shaders/connectivity.wgsl"),
                    include_str!("shaders/layer_bounds.wgsl"),
                    include_str!("shaders/bounds.wgsl")
                )
                .into(),
            ),
        });
        let leaf_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fog support bounds"),
            layout: None,
            module: &shader,
            entry_point: Some("build_leaves"),
            compilation_options: Default::default(),
            cache: None,
        });
        let field_view = field.create_view(&Default::default());
        let smoothed_view = smoothed.create_view(&Default::default());
        let lower_view = lower.create_view(&wgpu::TextureViewDescriptor {
            mip_level_count: Some(1),
            ..Default::default()
        });
        let upper_view = upper.create_view(&wgpu::TextureViewDescriptor {
            mip_level_count: Some(1),
            ..Default::default()
        });
        let leaf_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog support bound resources"),
            layout: &leaf_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: parameters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&field_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: bed.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: masks.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::TextureView(&smoothed_view),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&lower_view),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::TextureView(&upper_view),
                },
            ],
        });
        let reduce_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog bound pyramid reduction"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bounds_reduce.wgsl").into()),
        });
        let reduce_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("fog bound pyramid reduction"),
            layout: None,
            module: &reduce_shader,
            entry_point: Some("reduce_bounds"),
            compilation_options: Default::default(),
            cache: None,
        });
        let reduce_binds = (1..levels)
            .map(|level| {
                let view = |texture: &wgpu::Texture, mip| {
                    texture.create_view(&wgpu::TextureViewDescriptor {
                        base_mip_level: mip,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
                };
                let lower_in = view(&lower, level - 1);
                let upper_in = view(&upper, level - 1);
                let lower_out = view(&lower, level);
                let upper_out = view(&upper, level);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("fog bound mip resources"),
                    layout: &reduce_pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&lower_in),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&upper_in),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&lower_out),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&upper_out),
                        },
                    ],
                })
            })
            .collect();
        Self {
            lower,
            upper,
            leaf_pipeline,
            leaf_bind,
            reduce_pipeline,
            reduce_binds,
            parameters,
            padded,
            cached: Cell::new(None),
        }
    }
    pub fn encode(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        revision: u64,
        reconstruction: Reconstruction,
        strength: f32,
    ) {
        // SmoothStep changes weights, not their support. Non-filtered modes
        // must not rebuild just because a hidden smoothing slider changed.
        let mode = if reconstruction == Reconstruction::SmoothStep {
            0
        } else {
            reconstruction as u32
        };
        let key = (
            revision,
            mode,
            if mode == 2 { strength.to_bits() } else { 0 },
        );
        if self.cached.get() == Some(key) {
            return;
        }
        gpu.queue.write_buffer(
            &self.parameters,
            0,
            bytemuck::cast_slice(&[mode as f32, 0.0, 0.0, 0.0]),
        );
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fog leaf support bounds"),
                ..Default::default()
            });
            pass.set_pipeline(&self.leaf_pipeline);
            pass.set_bind_group(0, &self.leaf_bind, &[]);
            pass.dispatch_workgroups(self.padded[0].div_ceil(8), self.padded[1].div_ceil(8), 1);
        }
        for (index, bind) in self.reduce_binds.iter().enumerate() {
            let level = index + 1;
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("fog support min/max mip"),
                ..Default::default()
            });
            pass.set_pipeline(&self.reduce_pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(
                (self.padded[0] >> level).max(1).div_ceil(8),
                (self.padded[1] >> level).max(1).div_ceil(8),
                1,
            );
        }
        self.cached.set(Some(key));
    }
}

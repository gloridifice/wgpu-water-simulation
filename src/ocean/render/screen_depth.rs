//! Per-frame screen-depth bounds shared by SSR and model refraction.
use crate::ocean::gpu::context::Gpu;

pub(super) struct ScreenDepth {
    pub view: wgpu::TextureView,
    first: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    levels: Vec<(wgpu::BindGroup, [u32; 2])>,
}
impl ScreenDepth {
    pub fn new(gpu: &Gpu, depth: &wgpu::TextureView, width: u32, height: u32) -> Self {
        Self::from_device(&gpu.device, depth, width, height)
    }
    pub fn from_device(
        device: &wgpu::Device,
        depth: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Self {
        let width = width.max(1).next_power_of_two();
        let height = height.max(1).next_power_of_two();
        let count = width.max(height).ilog2() + 1;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("screen min/max depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("screen depth pyramid"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/depth_reduce.wgsl").into()),
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let first = pipeline("base");
        let reduce = pipeline("reduce");
        let views: Vec<_> = (0..count)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let levels = (0..count)
            .map(|level| {
                let (binding, source, layout) = if level == 0 {
                    (0, depth, first.get_bind_group_layout(0))
                } else {
                    (
                        1,
                        &views[level as usize - 1],
                        reduce.get_bind_group_layout(0),
                    )
                };
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("screen depth level"),
                    layout: &layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding,
                            resource: wgpu::BindingResource::TextureView(source),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&views[level as usize]),
                        },
                    ],
                });
                (bind, [(width >> level).max(1), (height >> level).max(1)])
            })
            .collect();
        Self {
            view: texture.create_view(&Default::default()),
            first,
            reduce,
            levels,
        }
    }
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        for (level, (bind, size)) in self.levels.iter().enumerate() {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("screen depth reduction"),
                ..Default::default()
            });
            pass.set_pipeline(if level == 0 {
                &self.first
            } else {
                &self.reduce
            });
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        }
    }
}

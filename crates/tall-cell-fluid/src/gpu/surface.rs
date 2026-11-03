//! GPU 零等值面提取。CPU 只做参数校验、上传与结果读取。
//!
//! 每个采样立方体由一个计算线程处理，以六四面体提取并用原子计数器
//! 分配三角形。输出为 triangle soup，未做共享顶点去重；索引为连续序号。
//! 采样点仍是单元中心，不对计算域边界自动封口。

use bytemuck::{Pod, Zeroable};

use super::GpuContext;
use crate::grid::Grid;
use crate::surface::{SurfaceLimits, SurfaceMesh, SurfaceVertex};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Parameters {
    dimensions: [u32; 4],
    origin_spacing: [f32; 4],
    capacity: [u32; 4],
}

/// Storage 中 vec4 对齐的中间格式，读回后转换为 24 字节顶点格式。
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OutputVertex {
    position: [f32; 4],
    normal: [f32; 4],
}

/// 直接在 GPU 上从 `phi = 0` 提取表面，读取位置、外法线和连续索引。
///
/// GPU 使用单精度。phi 上传前统一缩放以避免大距离值溢出；过大的动态范围
/// 会返回错误。容量不足时仍统计实际三角形数并返回错误，不交付截断网格。
/// `max_vertices` 按 triangle soup 的独立顶点计，通常比 CPU 去重结果更多。
pub fn extract_surface(
    context: &GpuContext,
    grid: &Grid,
    limits: SurfaceLimits,
) -> Result<SurfaceMesh, String> {
    grid.domain.validate()?;
    let size = grid.domain.size;
    if grid.phi.len() != grid.domain.cell_count() {
        return Err("GPU 表面输入 phi 数量与网格尺寸不一致".into());
    }
    for (axis, &count) in size.iter().enumerate() {
        if count > (1 << 23) {
            return Err("GPU 表面采样下标超出单精度半格坐标范围".into());
        }
        let first = grid.domain.origin[axis] + 0.5 * grid.domain.cell_size;
        let last = grid.domain.origin[axis] + (count as f64 - 0.5) * grid.domain.cell_size;
        if !(first as f32).is_finite() || !(last as f32).is_finite() || first as f32 >= last as f32
        {
            return Err("GPU 表面坐标超出单精度范围或发生尺度坍缩".into());
        }
    }
    let spacing = grid.domain.cell_size as f32;
    if !spacing.is_normal() || spacing <= 0.0 {
        return Err("GPU 表面网格间距必须可用单精度正规正数表示".into());
    }
    let mut phi_scale = 0.0_f64;
    for &value in &grid.phi {
        if !value.is_finite() {
            return Err("GPU 表面 phi 必须全部为有限数".into());
        }
        phi_scale = phi_scale.max(value.abs());
    }
    if phi_scale == 0.0 {
        phi_scale = 1.0;
    }
    let mut phi = Vec::with_capacity(grid.phi.len());
    for &value in &grid.phi {
        let scaled = (value / phi_scale) as f32;
        if value != 0.0 && !scaled.is_normal() {
            return Err("GPU 表面 phi 动态范围超出单精度正规数范围".into());
        }
        phi.push(scaled);
    }
    let cube_count = size.iter().map(|n| (n - 1) as u64).product::<u64>();
    let device_limits = context.device.limits();
    let groups = cube_count.div_ceil(64);
    if groups > u64::from(device_limits.max_compute_workgroups_per_dimension) {
        return Err("GPU 表面网格超过单次线性 dispatch 的设备限制".into());
    }
    if cube_count > u64::from(u32::MAX / 12) {
        return Err("GPU 表面三角形计数可能溢出".into());
    }
    let max_storage_bytes = device_limits
        .max_storage_buffer_binding_size
        .min(device_limits.max_buffer_size);
    if phi.len() as u64 * 4 > max_storage_bytes {
        return Err("GPU 表面 phi 缓冲区超过设备单绑定上限".into());
    }
    let max_device_triangles = max_storage_bytes / (3 * size_of::<OutputVertex>() as u64);
    let triangle_capacity = (limits.max_triangles as u64)
        .min((limits.max_vertices / 3) as u64)
        .min(cube_count * 12)
        .min(max_device_triangles)
        .min(u64::from(u32::MAX / 3)) as u32;
    let parameters = Parameters {
        dimensions: [
            size[0] as u32,
            size[1] as u32,
            size[2] as u32,
            cube_count as u32,
        ],
        origin_spacing: [
            grid.domain.origin[0] as f32,
            grid.domain.origin[1] as f32,
            grid.domain.origin[2] as f32,
            spacing,
        ],
        capacity: [triangle_capacity, 0, 0, 0],
    };
    let uniforms = context.uniform("surface parameters", &parameters);
    let phi_buffer = context.buffer("surface phi", &phi);
    // 绑定不能为零字节；容量为零时仍允许 GPU 检查是否确实存在三角形。
    let output_count = (triangle_capacity as usize * 3).max(1);
    let output = context.buffer(
        "surface vertices",
        &vec![OutputVertex::zeroed(); output_count],
    );
    let counter = context.buffer("surface triangle counter", &[0_u32; 4]);
    let pipeline = context.pipeline(
        "marching tetrahedra",
        include_str!("shaders/surface.wgsl"),
        "main",
    )?;
    let bindings = context.bind(&pipeline, &[&uniforms, &phi_buffer, &output, &counter]);
    context.dispatch(&pipeline, &bindings, groups as u32);
    let triangle_count = context.read::<u32>(&counter, 4)?[0];
    if triangle_count > triangle_capacity {
        return Err(format!(
            "GPU 表面需要 {triangle_count} 个三角形（{} 个独立顶点），容量只有 {triangle_capacity} 个三角形；受输出限制或设备绑定大小限制",
            u64::from(triangle_count) * 3
        ));
    }
    let vertex_count = triangle_count as usize * 3;
    if vertex_count == 0 {
        return Ok(SurfaceMesh::default());
    }
    let vertices = context
        .read::<OutputVertex>(&output, vertex_count)?
        .into_iter()
        .map(|vertex| SurfaceVertex {
            position: [vertex.position[0], vertex.position[1], vertex.position[2]],
            normal: [vertex.normal[0], vertex.normal[1], vertex.normal[2]],
        })
        .collect::<Vec<_>>();
    if vertices
        .iter()
        .any(|v| v.position.iter().chain(&v.normal).any(|x| !x.is_finite()))
    {
        return Err("GPU 表面返回非有限顶点或法线".into());
    }
    Ok(SurfaceMesh {
        vertices,
        indices: (0..vertex_count as u32).collect(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::Domain;

    /// 显式执行：cargo test --features gpu gpu::surface::tests -- --ignored --nocapture
    #[test]
    #[ignore = "需要真实 wgpu 适配器；默认单元测试不申请设备"]
    fn gpu_extracts_empty_plane_sphere_and_reports_capacity_overflow() {
        let context = pollster::block_on(GpuContext::new()).expect("需要真实计算适配器");
        let mut grid = Grid::new(Domain::new([12; 3], 0.1).unwrap());
        for value in [-1.0, 0.0, 1.0] {
            grid.set_liquid_sdf(|_| value);
            assert!(
                extract_surface(&context, &grid, SurfaceLimits::default())
                    .unwrap()
                    .vertices
                    .is_empty()
            );
        }
        for height in [0.43, 0.55] {
            grid.set_liquid_sdf(|p| p[1] - height);
            let mesh = extract_surface(&context, &grid, SurfaceLimits::default()).unwrap();
            assert!(!mesh.vertices.is_empty());
            assert_eq!(mesh.vertices.len(), mesh.indices.len());
            for vertex in &mesh.vertices {
                assert!((f64::from(vertex.position[1]) - height).abs() < 1e-6);
                assert!(vertex.normal[1] > 0.999);
            }
            for triangle in mesh.vertices.as_chunks::<3>().0 {
                let a = triangle[0].position;
                let b = triangle[1].position;
                let c = triangle[2].position;
                let winding_y = (b[2] - a[2]) * (c[0] - a[0]) - (b[0] - a[0]) * (c[2] - a[2]);
                assert!(winding_y > 0.0);
            }
        }
        grid.set_liquid_sdf(|p| {
            p.iter()
                .map(|coordinate| (coordinate - 0.6).powi(2))
                .sum::<f64>()
                .sqrt()
                - 0.35
        });
        let mesh = extract_surface(&context, &grid, SurfaceLimits::default()).unwrap();
        assert!(!mesh.vertices.is_empty());
        for vertex in &mesh.vertices {
            let radial = vertex
                .position
                .map(|coordinate| f64::from(coordinate) - 0.6);
            let length = radial.iter().map(|x| x * x).sum::<f64>().sqrt();
            assert!((length - 0.35).abs() < 0.025);
            let alignment = radial
                .iter()
                .zip(vertex.normal)
                .map(|(a, b)| a * f64::from(b) / length)
                .sum::<f64>();
            assert!(alignment > 0.96);
        }
        // 只在验证中按坐标焊接 triangle soup，检查每条边恰好由两个反向面共享。
        let mut welded = HashMap::new();
        let mut edges = HashMap::<(usize, usize), (usize, i32)>::new();
        for triangle in mesh.vertices.as_chunks::<3>().0 {
            let ids = triangle.map(|vertex| {
                let next = welded.len();
                *welded
                    .entry(vertex.position.map(f32::to_bits))
                    .or_insert(next)
            });
            for j in 0..3 {
                let a = ids[j];
                let b = ids[(j + 1) % 3];
                let edge = edges.entry((a.min(b), a.max(b))).or_default();
                edge.0 += 1;
                edge.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(
            edges
                .values()
                .all(|&(count, orientation)| count == 2 && orientation == 0)
        );
        let tiny = SurfaceLimits {
            max_vertices: 2,
            max_triangles: 1,
        };
        let error = extract_surface(&context, &grid, tiny).unwrap_err();
        assert!(error.contains("容量"));
        println!(
            "GPU surface verified on {}: sphere {} triangles",
            context.adapter_name,
            mesh.indices.len() / 3
        );
    }
}

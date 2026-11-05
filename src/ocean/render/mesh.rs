//! Offshore draw topology only; FFT samples and the local SWE mesh stay square.
use crate::ocean::gpu::context::buffer;
use crate::ocean::parameters::ocean::OceanConfig;

pub(super) struct OceanMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
}

impl OceanMesh {
    pub fn new(device: &wgpu::Device, config: OceanConfig) -> Self {
        let mesh = MeshData::new(config.mesh_resolution, config.triangular_mesh);
        Self {
            vertices: buffer(
                device,
                "offshore mesh UV",
                &mesh.vertices,
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                device,
                "offshore mesh indices",
                &mesh.indices,
                wgpu::BufferUsages::INDEX,
            ),
            index_count: mesh.indices.len() as u32,
        }
    }
}

fn dimensions(budget: u32, triangular: bool) -> [u32; 2] {
    if !triangular {
        return [budget; 2];
    }
    // Equilateral rows are sqrt(3)/2 as far apart as horizontal neighbors.
    // An even row count repeats the stagger across the periodic Z boundary.
    let row_ratio = (3.0_f64.sqrt() / 2.0).sqrt();
    let rows = 2 * (f64::from(budget) / row_ratio / 2.0).round() as u32;
    let columns = (f64::from(budget).powi(2) / f64::from(rows)).round() as u32;
    [columns, rows]
}

struct MeshData {
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl MeshData {
    fn new(budget: u32, triangular: bool) -> Self {
        let [columns, rows] = dimensions(budget, triangular);
        let mut vertices = Vec::with_capacity(((columns + 1) * (rows + 1)) as usize);
        for z in 0..=rows {
            let shift = if triangular && z % 2 == 1 { 0.5 } else { 0.0 };
            for x in 0..=columns {
                // Keep the boundary stagger, rather than squeezing boundary cells.
                // Left/right polylines differ by exactly one tile; neighboring
                // instances share them without gaps, overlaps or thin triangles.
                vertices.push([(x as f32 + shift) / columns as f32, z as f32 / rows as f32]);
            }
        }
        Self {
            vertices,
            indices: indices(columns, rows, triangular),
        }
    }
}

fn indices(columns: u32, rows: u32, triangular: bool) -> Vec<u32> {
    let mut indices = Vec::with_capacity((columns * rows * 6) as usize);
    for z in 0..rows {
        for x in 0..columns {
            let a = z * (columns + 1) + x;
            let b = a + 1;
            let c = a + columns + 1;
            let d = c + 1;
            if triangular && z % 2 == 1 {
                indices.extend_from_slice(&[a, c, d, a, d, b]);
            } else {
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
    }
    indices
}

pub(super) fn square_indices(device: &wgpu::Device, n: u32) -> wgpu::Buffer {
    buffer(
        device,
        "local water grid indices",
        &indices(n, n, false),
        wgpu::BufferUsages::INDEX,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn triangle_budget_stays_close_across_supported_densities() {
        for n in 8..=512 {
            let [columns, rows] = dimensions(n, true);
            let relative_count = f64::from(columns * rows) / f64::from(n * n);
            assert!(
                (relative_count - 1.0).abs() < 0.025,
                "budget {n}: {relative_count}"
            );
        }
    }

    #[test]
    fn both_meshes_cover_one_period_with_consistent_winding_and_watertight_edges() {
        for n in [8, 9, 47, 128, 512] {
            for triangular in [false, true] {
                let mesh = MeshData::new(n, triangular);
                let [columns, rows] = dimensions(n, triangular);
                let stride = columns + 1;
                let mut area = 0.0_f64;
                let mut edges = HashMap::<(u32, u32), (u32, i32)>::new();
                // Identify shared vertices on a torus; every edge must have two
                // oppositely directed uses, including both periodic tile seams.
                let periodic = |i: u32| (i / stride % rows) * columns + i % stride % columns;
                for &triangle in mesh.indices.as_chunks::<3>().0 {
                    let [a, b, c] = triangle.map(|i| mesh.vertices[i as usize]);
                    if triangular {
                        let lengths = [(a, b), (b, c), (c, a)]
                            .map(|(p, q)| (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2));
                        let shortest = lengths.into_iter().fold(f32::INFINITY, f32::min);
                        let longest = lengths.into_iter().fold(0.0_f32, f32::max);
                        assert!(longest / shortest < 1.31, "non-equilateral triangle at {n}");
                    }
                    let determinant = (f64::from(b[0]) - f64::from(a[0]))
                        * (f64::from(c[1]) - f64::from(a[1]))
                        - (f64::from(b[1]) - f64::from(a[1])) * (f64::from(c[0]) - f64::from(a[0]));
                    assert!(determinant < 0.0, "degenerate/reversed triangle at {n}");
                    area -= determinant * 0.5;
                    for (a, b) in [
                        (triangle[0], triangle[1]),
                        (triangle[1], triangle[2]),
                        (triangle[2], triangle[0]),
                    ] {
                        let (a, b) = (periodic(a), periodic(b));
                        let edge = edges.entry((a.min(b), a.max(b))).or_default();
                        edge.0 += 1;
                        edge.1 += if a < b { 1 } else { -1 };
                    }
                }
                assert!((area - 1.0).abs() < 1e-6, "tile area {area}");
                assert!(edges.values().all(|&edge| edge == (2, 0)));
                for z in 0..=rows {
                    let left = mesh.vertices[(z * stride) as usize];
                    let right = mesh.vertices[(z * stride + columns) as usize];
                    assert!((right[0] - left[0] - 1.0).abs() < 1e-7);
                    assert_eq!(left[1], right[1]);
                }
                for x in 0..=columns {
                    let bottom = mesh.vertices[x as usize];
                    let top = mesh.vertices[(rows * stride + x) as usize];
                    assert_eq!(bottom[0], top[0]);
                    assert_eq!(top[1] - bottom[1], 1.0);
                }
            }
        }
    }
}

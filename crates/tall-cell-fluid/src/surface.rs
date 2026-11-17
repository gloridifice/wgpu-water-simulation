//! Extract a triangular surface from the cell-centered liquid distance field.
//!
//! Treat eight neighboring cell centers as a sampling cube and extract `phi = 0`
//! using a consistent six-tetrahedron decomposition. This is not the pressure
//! grid's outer shell and does not depend on tall-cell merging.
//! Adjacent sampling cubes use the same shared-face diagonal and deduplicate
//! intersections on shared edges.
//!
//! Extraction spans the first through last layers of cell centers; liquid touching
//! the domain boundary is not automatically capped. This function only reads liquid
//! `phi`; it does not separately clip solids or generate solid surfaces.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::grid::{Grid, Vec3};

/// A 24-byte layout suitable for direct copying to a vertex buffer: three `f32`s
/// each for position and outward normal.
///
/// Positions are in meters; this is not the aligned WGSL uniform/storage `vec3` layout.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
}

/// Each three indices form a triangle, wound counterclockwise when viewed from outside the water.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SurfaceMesh {
    pub vertices: Vec<SurfaceVertex>,
    pub indices: Vec<u32>,
}

/// Surface output limits. Exceeding a limit returns an error rather than a truncated or broken surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceLimits {
    pub max_vertices: usize,
    pub max_triangles: usize,
}

impl Default for SurfaceLimits {
    fn default() -> Self {
        Self {
            max_vertices: 2_000_000,
            max_triangles: 4_000_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceError {
    InvalidGrid(&'static str),
    NonFinitePhi { index: usize },
    VertexLimit { max: usize },
    TriangleLimit { max: usize },
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidGrid(reason) => write!(f, "Invalid surface sampling grid: {reason}"),
            Self::NonFinitePhi { index } => write!(f, "phi[{index}] is not finite"),
            Self::VertexLimit { max } => write!(f, "Surface vertices exceed limit {max}"),
            Self::TriangleLimit { max } => write!(f, "Surface triangles exceed limit {max}"),
        }
    }
}

impl Error for SurfaceError {}

/// Extract the complete zero isosurface using default resource limits.
///
/// ```
/// use tall_cell_fluid::{Domain, Grid};
/// use tall_cell_fluid::surface::extract_surface;
///
/// let mut grid = Grid::new(Domain::new([12; 3], 0.1)?);
/// grid.set_liquid_sdf(|p| {
///     ((p[0] - 0.6).powi(2) + (p[1] - 0.6).powi(2) + (p[2] - 0.6).powi(2))
///         .sqrt() - 0.35
/// });
/// let mesh = extract_surface(&grid)?;
/// assert!(!mesh.vertices.is_empty());
/// assert_eq!(mesh.indices.len() % 3, 0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn extract_surface(grid: &Grid) -> Result<SurfaceMesh, SurfaceError> {
    extract_surface_with_limits(grid, SurfaceLimits::default())
}

/// Extract a surface from the distance field, limiting vertices, triangles, and associated buffers.
///
/// `phi < 0` is water; samples exactly at zero are outside. Isosurface points are
/// deduplicated by sample index without perturbing distance values to change geometry;
/// duplicate or zero-area triangles are discarded. Normals are interpolated from
/// cell-centered finite-difference gradients, falling back to adjacent outward face normals.
pub fn extract_surface_with_limits(
    grid: &Grid,
    limits: SurfaceLimits,
) -> Result<SurfaceMesh, SurfaceError> {
    let size = grid.domain.size;
    let count = size
        .iter()
        .try_fold(1usize, |product, &n| product.checked_mul(n))
        .ok_or(SurfaceError::InvalidGrid("cell count overflow"))?;
    if size.contains(&0)
        || !grid.domain.cell_size.is_finite()
        || grid.domain.cell_size <= 0.0
        || grid.phi.len() != count
    {
        return Err(SurfaceError::InvalidGrid(
            "invalid dimensions, spacing, or phi length",
        ));
    }
    for (axis, &axis_size) in size.iter().enumerate() {
        let first = grid.domain.origin[axis] + 0.5 * grid.domain.cell_size;
        let last = grid.domain.origin[axis] + (axis_size as f64 - 0.5) * grid.domain.cell_size;
        if !first.is_finite() || !last.is_finite() || first.abs().max(last.abs()) > f32::MAX as f64
        {
            return Err(SurfaceError::InvalidGrid(
                "position outside finite f32 vertex range",
            ));
        }
        if axis_size > 1 && first as f32 >= last as f32 {
            return Err(SurfaceError::InvalidGrid(
                "sampling range collapses at f32 vertex precision",
            ));
        }
    }
    let mut phi_scale = 0.0_f64;
    for (index, &phi) in grid.phi.iter().enumerate() {
        if !phi.is_finite() {
            return Err(SurfaceError::NonFinitePhi { index });
        }
        phi_scale = phi_scale.max(phi.abs());
    }
    if size.iter().any(|&n| n < 2) || phi_scale == 0.0 {
        return Ok(SurfaceMesh::default());
    }

    let mut builder = MeshBuilder {
        grid,
        limits,
        phi_scale,
        mesh: SurfaceMesh::default(),
        vertices_by_edge: HashMap::new(),
        triangles: HashSet::new(),
        face_normals: Vec::new(),
    };
    // Decompose the cube around the 0 -> 7 body diagonal by permutations of the three axes.
    // These connections are geometric and need no third-party Marching Cubes lookup table.
    const TETRAHEDRA: [[usize; 4]; 6] = [
        [0, 1, 3, 7],
        [0, 3, 2, 7],
        [0, 2, 6, 7],
        [0, 6, 4, 7],
        [0, 4, 5, 7],
        [0, 5, 1, 7],
    ];
    for k in 0..size[2] - 1 {
        for j in 0..size[1] - 1 {
            for i in 0..size[0] - 1 {
                let corners = std::array::from_fn::<_, 8, _>(|corner| {
                    [i + (corner & 1), j + ((corner >> 1) & 1), k + (corner >> 2)]
                });
                let inside = corners.map(|p| grid.phi[grid.domain.index(p)] < 0.0);
                if inside.iter().all(|&value| value) || inside.iter().all(|&value| !value) {
                    continue;
                }
                for tetrahedron in TETRAHEDRA {
                    builder.tetrahedron(tetrahedron.map(|corner| corners[corner]))?;
                }
            }
        }
    }
    Ok(builder.finish())
}

struct MeshBuilder<'a> {
    grid: &'a Grid,
    limits: SurfaceLimits,
    phi_scale: f64,
    mesh: SurfaceMesh,
    vertices_by_edge: HashMap<(usize, usize), u32>,
    triangles: HashSet<[u32; 3]>,
    face_normals: Vec<Vec3>,
}

impl MeshBuilder<'_> {
    fn tetrahedron(&mut self, nodes: [[usize; 3]; 4]) -> Result<(), SurfaceError> {
        let mut inside_nodes = [[0; 3]; 4];
        let mut outside_nodes = [[0; 3]; 4];
        let mut inside_count = 0;
        let mut outside_count = 0;
        for node in nodes {
            if self.grid.phi[self.grid.domain.index(node)] < 0.0 {
                inside_nodes[inside_count] = node;
                inside_count += 1;
            } else {
                outside_nodes[outside_count] = node;
                outside_count += 1;
            }
        }
        let inside = &inside_nodes[..inside_count];
        let outside = &outside_nodes[..outside_count];
        if inside.is_empty() || outside.is_empty() {
            return Ok(());
        }
        let average = |nodes: &[[usize; 3]]| {
            let mut result = [0.0; 3];
            for node in nodes {
                let position = self.grid.domain.center(*node);
                for axis in 0..3 {
                    result[axis] += position[axis] / nodes.len() as f64;
                }
            }
            result
        };
        let outward = sub(average(outside), average(inside));
        if inside.len() == 1 || outside.len() == 1 {
            let (tip, others) = if inside.len() == 1 {
                (inside[0], outside)
            } else {
                (outside[0], inside)
            };
            let a = self.intersection(tip, others[0])?;
            let b = self.intersection(tip, others[1])?;
            let c = self.intersection(tip, others[2])?;
            self.triangle([a, b, c], outward)?;
        } else {
            // With two inside and two outside, the four intersections follow the quadrilateral boundary.
            let a = self.intersection(inside[0], outside[0])?;
            let b = self.intersection(inside[0], outside[1])?;
            let c = self.intersection(inside[1], outside[1])?;
            let d = self.intersection(inside[1], outside[0])?;
            self.triangle([a, b, c], outward)?;
            self.triangle([a, c, d], outward)?;
        }
        Ok(())
    }

    fn intersection(&mut self, mut a: [usize; 3], mut b: [usize; 3]) -> Result<u32, SurfaceError> {
        let mut ia = self.grid.domain.index(a);
        let mut ib = self.grid.domain.index(b);
        if ia > ib {
            std::mem::swap(&mut ia, &mut ib);
            std::mem::swap(&mut a, &mut b);
        }
        let pa = self.grid.phi[ia];
        let pb = self.grid.phi[ib];
        let key = if pa == 0.0 {
            (ia, ia)
        } else if pb == 0.0 {
            (ib, ib)
        } else {
            (ia, ib)
        };
        if let Some(&index) = self.vertices_by_edge.get(&key) {
            return Ok(index);
        }
        let limit = self.limits.max_vertices.min(u32::MAX as usize);
        if self.mesh.vertices.len() >= limit {
            return Err(SurfaceError::VertexLimit { max: limit });
        }
        // Scale before adding to avoid overflow when subtracting huge finite distances of opposite signs.
        let scale = pa.abs().max(pb.abs());
        let t = (pa.abs() / scale) / (pa.abs() / scale + pb.abs() / scale);
        let position = lerp(self.grid.domain.center(a), self.grid.domain.center(b), t);
        let normal = normalize(lerp(self.gradient(a), self.gradient(b), t));
        let index = self.mesh.vertices.len() as u32;
        self.mesh.vertices.push(SurfaceVertex {
            position: position.map(|x| x as f32),
            normal: normal.map(|x| x as f32),
        });
        self.face_normals.push([0.0; 3]);
        self.vertices_by_edge.insert(key, index);
        Ok(index)
    }

    fn gradient(&self, p: [usize; 3]) -> Vec3 {
        std::array::from_fn(|axis| {
            let mut low = p;
            let mut high = p;
            low[axis] = p[axis].saturating_sub(1);
            high[axis] = (p[axis] + 1).min(self.grid.domain.size[axis] - 1);
            let a = self.grid.phi[self.grid.domain.index(low)] / self.phi_scale;
            let b = self.grid.phi[self.grid.domain.index(high)] / self.phi_scale;
            // Uniform grid spacing and global phi scaling cancel during normal normalization.
            (b - a) / (high[axis] - low[axis]) as f64
        })
    }

    fn triangle(&mut self, mut indices: [u32; 3], outward: Vec3) -> Result<(), SurfaceError> {
        if indices[0] == indices[1] || indices[1] == indices[2] || indices[2] == indices[0] {
            return Ok(());
        }
        // Check degeneracy using the output f32 positions to avoid zero-area triangles after quantization.
        let positions = indices.map(|i| self.mesh.vertices[i as usize].position.map(f64::from));
        let mut normal = cross(
            sub(positions[1], positions[0]),
            sub(positions[2], positions[0]),
        );
        if normal == [0.0; 3] {
            return Ok(());
        }
        if dot(normal, outward) < 0.0 {
            indices.swap(1, 2);
            normal = normal.map(|x| -x);
        }
        let mut key = indices;
        key.sort_unstable();
        if self.triangles.contains(&key) {
            return Ok(());
        }
        if self.mesh.indices.len() / 3 >= self.limits.max_triangles {
            return Err(SurfaceError::TriangleLimit {
                max: self.limits.max_triangles,
            });
        }
        self.triangles.insert(key);
        self.mesh.indices.extend(indices);
        let normal = normalize(normal);
        for index in indices {
            for (sum, value) in self.face_normals[index as usize].iter_mut().zip(normal) {
                *sum += value;
            }
        }
        Ok(())
    }

    fn finish(mut self) -> SurfaceMesh {
        let mut remap = vec![u32::MAX; self.mesh.vertices.len()];
        for &index in &self.mesh.indices {
            remap[index as usize] = 0;
        }
        let mut used = 0;
        for (index, new_index) in remap.iter_mut().enumerate() {
            if *new_index != u32::MAX {
                let mut vertex = self.mesh.vertices[index];
                if vertex.normal == [0.0; 3] {
                    vertex.normal = normalize(self.face_normals[index]).map(|x| x as f32);
                }
                self.mesh.vertices[used] = vertex;
                *new_index = used as u32;
                used += 1;
            }
        }
        self.mesh.vertices.truncate(used);
        for index in &mut self.mesh.indices {
            *index = remap[*index as usize];
        }
        self.mesh
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    std::array::from_fn(|axis| a[axis] - b[axis])
}

fn lerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    std::array::from_fn(|axis| a[axis] + t * (b[axis] - a[axis]))
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: Vec3) -> Vec3 {
    let scale = v.iter().fold(0.0_f64, |max, x| max.max(x.abs()));
    if scale == 0.0 {
        return [0.0; 3];
    }
    let scaled = v.map(|x| x / scale);
    let length = dot(scaled, scaled).sqrt();
    scaled.map(|x| x / length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Domain;

    fn sample_grid(size: [usize; 3], cell_size: f64, sdf: impl Fn(Vec3) -> f64) -> Grid {
        let mut grid = Grid::new(Domain::new(size, cell_size).unwrap());
        grid.set_liquid_sdf(sdf);
        grid
    }

    #[test]
    fn plane_has_expected_positions_normals_and_outward_winding() {
        let height = 0.83;
        let grid = sample_grid([9, 8, 7], 0.2, |p| p[1] - height);
        let mesh = extract_surface(&grid).unwrap();
        assert!(!mesh.indices.is_empty());
        for v in &mesh.vertices {
            assert!((f64::from(v.position[1]) - height).abs() < 1e-6);
            assert!(v.normal[1] > 0.9999);
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let p: Vec<Vec3> = triangle
                .iter()
                .map(|&i| mesh.vertices[i as usize].position.map(f64::from))
                .collect();
            assert!(cross(sub(p[1], p[0]), sub(p[2], p[0]))[1] > 0.0);
        }
    }

    #[test]
    fn exact_zero_plane_has_no_duplicate_vertices_or_degenerate_triangles() {
        let grid = sample_grid([7, 7, 7], 0.25, |p| p[1] - 0.875);
        let mesh = extract_surface(&grid).unwrap();
        assert_eq!(mesh.vertices.len(), 49);
        assert_eq!(mesh.indices.len() / 3, 72);
        let unique: HashSet<_> = mesh
            .vertices
            .iter()
            .map(|v| v.position.map(f32::to_bits))
            .collect();
        assert_eq!(unique.len(), mesh.vertices.len());
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let p = std::array::from_fn::<_, 3, _>(|j| {
                mesh.vertices[triangle[j] as usize].position.map(f64::from)
            });
            assert!(cross(sub(p[1], p[0]), sub(p[2], p[0]))[1] > 0.0);
        }
    }

    #[test]
    fn sphere_is_closed_oriented_and_approximates_the_requested_radius() {
        let center = [1.0; 3];
        let radius = 0.61;
        let grid = sample_grid([20; 3], 0.1, |p| {
            dot(sub(p, center), sub(p, center)).sqrt() - radius
        });
        let mesh = extract_surface(&grid).unwrap();
        assert!(!mesh.indices.is_empty());
        let mut edges: HashMap<(u32, u32), (usize, i32)> = HashMap::new();
        let mut volume = 0.0;
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let p = std::array::from_fn::<_, 3, _>(|j| {
                mesh.vertices[triangle[j] as usize].position.map(f64::from)
            });
            volume += dot(p[0], cross(p[1], p[2])) / 6.0;
            for j in 0..3 {
                let a = triangle[j];
                let b = triangle[(j + 1) % 3];
                let value = edges.entry((a.min(b), a.max(b))).or_default();
                value.0 += 1;
                value.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(
            edges
                .values()
                .all(|&(count, direction)| count == 2 && direction == 0)
        );
        for vertex in mesh.vertices {
            let radial = sub(vertex.position.map(f64::from), center);
            assert!((dot(radial, radial).sqrt() - radius).abs() < 0.015);
            assert!(dot(normalize(radial), vertex.normal.map(f64::from)) > 0.98);
        }
        let expected_volume = 4.0 * std::f64::consts::PI * radius.powi(3) / 3.0;
        assert!((volume / expected_volume - 1.0).abs() < 0.03);
    }

    #[test]
    fn homogeneous_domains_do_not_generate_artificial_boundary_caps() {
        for phi in [-1.0, 0.0, 1.0] {
            let grid = sample_grid([4; 3], 0.5, |_| phi);
            assert_eq!(extract_surface(&grid).unwrap(), SurfaceMesh::default());
        }
    }

    #[test]
    fn limits_and_nonfinite_fields_fail_without_partial_meshes() {
        let mut grid = sample_grid([4; 3], 0.5, |p| p[1] - 0.9);
        let error = extract_surface_with_limits(
            &grid,
            SurfaceLimits {
                max_vertices: 2,
                max_triangles: 100,
            },
        )
        .unwrap_err();
        assert_eq!(error, SurfaceError::VertexLimit { max: 2 });
        let error = extract_surface_with_limits(
            &grid,
            SurfaceLimits {
                max_vertices: 1000,
                max_triangles: 0,
            },
        )
        .unwrap_err();
        assert_eq!(error, SurfaceError::TriangleLimit { max: 0 });
        grid.phi[5] = f64::NAN;
        assert_eq!(
            extract_surface(&grid),
            Err(SurfaceError::NonFinitePhi { index: 5 })
        );
    }

    #[test]
    fn extremely_large_finite_values_do_not_overflow_intersection_arithmetic() {
        let grid = sample_grid([3; 3], 1.0, |p| if p[1] < 1.0 { -1e308 } else { 1e308 });
        let mesh = extract_surface(&grid).unwrap();
        assert!(!mesh.vertices.is_empty());
        assert!(
            mesh.vertices
                .iter()
                .all(|v| { v.position.iter().chain(&v.normal).all(|x| x.is_finite()) })
        );
    }

    #[test]
    fn malformed_arrays_and_unrepresentable_positions_are_rejected() {
        let mut grid = sample_grid([3; 3], 1.0, |p| p[1] - 1.0);
        grid.phi.pop();
        assert!(matches!(
            extract_surface(&grid),
            Err(SurfaceError::InvalidGrid(_))
        ));
        grid.phi.push(1.0);
        for origin in [f64::NAN, f64::INFINITY, f64::MAX, 1e30] {
            grid.domain.origin = [origin; 3];
            assert!(matches!(
                extract_surface(&grid),
                Err(SurfaceError::InvalidGrid(_))
            ));
        }
        grid.domain.origin = [0.0; 3];
        grid.domain.cell_size = 1e-100;
        assert!(matches!(
            extract_surface(&grid),
            Err(SurfaceError::InvalidGrid(_))
        ));
    }

    #[test]
    fn sphere_through_exact_sample_points_remains_closed() {
        let center = [1.125; 3];
        let grid = sample_grid([9; 3], 0.25, |p| {
            dot(sub(p, center), sub(p, center)).sqrt() - 0.5
        });
        assert!(grid.phi.contains(&0.0));
        let mesh = extract_surface(&grid).unwrap();
        let mut edges = HashMap::<_, usize>::new();
        for triangle in mesh.indices.as_chunks::<3>().0 {
            for j in 0..3 {
                let a = triangle[j];
                let b = triangle[(j + 1) % 3];
                *edges.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        assert!(!edges.is_empty());
        assert!(edges.values().all(|&count| count == 2));
    }
}

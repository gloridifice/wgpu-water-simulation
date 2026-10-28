//! Static glTF scene geometry. Runtime rendering and offline baking share the same
//! node transforms and cap removal; no triangle queries occur during SWE steps.
use crate::ocean::Error;
use glam::{Mat4, Vec3};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const MODEL_PATH: &str = "assets/models/challaborough_pillbox.glb";
pub const HEIGHT_PATH: &str = "assets/models/challaborough_pillbox.sweh";

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

pub struct Model {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub image: Vec<u8>,
    pub bounds: [[f32; 3]; 2],
    pub source_hash: String,
    pub removed_triangles: usize,
}

/// Relative assets work in a packaged assets directory or in a source checkout.
pub fn asset_path(relative: &str) -> PathBuf {
    let path = PathBuf::from(relative);
    if path.is_file() {
        return path;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        let beside_exe = parent.join(relative);
        if beside_exe.is_file() {
            return beside_exe;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

impl Model {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let bytes = std::fs::read(path)?;
        let source_hash = format!("{:x}", Sha256::digest(&bytes));
        let gltf = gltf::Gltf::from_slice(&bytes)?;
        let blob = gltf
            .blob
            .as_deref()
            .ok_or("model must be a self-contained GLB")?;
        if gltf
            .buffers()
            .any(|b| !matches!(b.source(), gltf::buffer::Source::Bin))
        {
            return Err("external glTF buffers are not supported".into());
        }
        let scene = gltf
            .default_scene()
            .or_else(|| gltf.scenes().next())
            .ok_or("GLB has no scene")?;
        let mut model = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            image: Vec::new(),
            bounds: [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]],
            source_hash,
            removed_triangles: 0,
        };
        for node in scene.nodes() {
            model.node(node, Mat4::IDENTITY, blob)?;
        }
        if model.indices.is_empty() {
            return Err("GLB contains no triangles".into());
        }
        let image = gltf.images().next().ok_or("GLB has no base color image")?;
        let gltf::image::Source::View { view, mime_type } = image.source() else {
            return Err("base color image must be embedded in GLB".into());
        };
        if mime_type != "image/jpeg" {
            return Err("this model loader expects an embedded JPEG".into());
        }
        model.image = blob
            .get(view.offset()..view.offset() + view.length())
            .ok_or("invalid image view")?
            .to_vec();
        Ok(model)
    }

    fn node(&mut self, node: gltf::Node<'_>, parent: Mat4, blob: &[u8]) -> Result<(), Error> {
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let normals = transform.inverse().transpose();
        if !transform.is_finite() || !normals.is_finite() {
            return Err("invalid model node transform".into());
        }
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    return Err("only triangle primitives are supported".into());
                }
                let material = primitive.material();
                if !material.unlit()
                    || material
                        .pbr_metallic_roughness()
                        .base_color_texture()
                        .is_none_or(|t| t.texture().source().index() != 0 || t.tex_coord() != 0)
                {
                    return Err(
                        "model requires one unlit base-color texture using TEXCOORD_0".into(),
                    );
                }
                let reader = primitive.reader(|_| Some(blob));
                let positions: Vec<_> =
                    reader.read_positions().ok_or("missing POSITION")?.collect();
                let normal: Vec<_> = reader.read_normals().ok_or("missing NORMAL")?.collect();
                let uv: Vec<_> = reader
                    .read_tex_coords(0)
                    .ok_or("missing TEXCOORD_0")?
                    .into_f32()
                    .collect();
                if normal.len() != positions.len() || uv.len() != positions.len() {
                    return Err("mismatched vertex attributes".into());
                }
                let start = self.vertices.len() as u32;
                for (i, p) in positions.iter().enumerate() {
                    let p = transform.transform_point3(Vec3::from_array(*p));
                    let n = normals
                        .transform_vector3(Vec3::from_array(normal[i]))
                        .normalize_or_zero();
                    if !p.is_finite() || !n.is_finite() || !uv[i].iter().all(|v| v.is_finite()) {
                        return Err("non-finite model vertex".into());
                    }
                    for axis in 0..3 {
                        self.bounds[0][axis] = self.bounds[0][axis].min(p[axis]);
                        self.bounds[1][axis] = self.bounds[1][axis].max(p[axis]);
                    }
                    self.vertices.push(Vertex {
                        position: p.to_array(),
                        normal: n.to_array(),
                        uv: uv[i],
                    });
                }
                let indices: Vec<_> = reader
                    .read_indices()
                    .map(|v| v.into_u32().collect())
                    .unwrap_or_else(|| (0..positions.len() as u32).collect());
                if !indices.len().is_multiple_of(3)
                    || indices.iter().any(|i| *i as usize >= positions.len())
                {
                    return Err("invalid triangle indices".into());
                }
                self.indices.extend(indices.into_iter().map(|i| i + start));
            }
        }
        for child in node.children() {
            self.node(child, transform, blob)?;
        }
        Ok(())
    }

    /// Opt-in scan cleanup: remove only triangles lying entirely on the bottom
    /// or four extremal vertical cutting planes. Do not remove the upper cap.
    pub fn remove_scan_caps(&mut self, epsilon: f32) {
        let before = self.indices.len();
        let vertices = &self.vertices;
        let bounds = self.bounds;
        self.indices = self
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .filter(|t| {
                ![(0, 0), (0, 1), (1, 0), (2, 0), (2, 1)]
                    .iter()
                    .any(|&(axis, side)| {
                        t.iter().all(|&i| {
                            (vertices[i as usize].position[axis] - bounds[side][axis]).abs()
                                <= epsilon
                        })
                    })
            })
            .flatten()
            .copied()
            .collect();
        self.removed_triangles = (before - self.indices.len()) / 3;
    }
}

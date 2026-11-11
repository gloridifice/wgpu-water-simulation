//! Offline GLB -> world-space collision triangles and texture-baked corner colors.
//! The packed scene is also the rendering source: no transform can drift from collision.
use crate::{Error, terrain::BakeSource};
use glam::{Mat4, Vec2, Vec3};
use std::{io::Write, path::Path};

const MAGIC: &[u8; 8] = b"FOGSCN03";
const LEGACY_MAGIC: &[u8; 8] = b"FOGSCN02";
const MAX_TRIANGLES: usize = 2_000_000;

pub fn load(path: &Path) -> Result<BakeSource, Error> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(MAGIC) || bytes.starts_with(LEGACY_MAGIC) {
        decode(&bytes)
    } else {
        let source: BakeSource = serde_json::from_slice(&bytes)?;
        source.validate()?;
        Ok(source)
    }
}

pub fn decode(bytes: &[u8]) -> Result<BakeSource, Error> {
    let header = if bytes.starts_with(MAGIC) {
        28
    } else if bytes.starts_with(LEGACY_MAGIC) {
        24
    } else {
        return Err("invalid packed fog scene header".into());
    };
    if bytes.len() < header {
        return Err("truncated packed fog scene header".into());
    }
    let u32_at = |i| u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    let floor = u32_at(8);
    let obstacle = u32_at(12);
    let colored = u32_at(16);
    let collision_count = u32_at(20);
    let support_count = if header == 28 { u32_at(24) } else { 0 };
    let count = floor.checked_add(obstacle).ok_or("scene count overflow")?;
    if count > MAX_TRIANGLES
        || collision_count > MAX_TRIANGLES
        || support_count > MAX_TRIANGLES
        || colored > 1
        || bytes.len()
            != header + count * (36 + colored * 36) + (collision_count + support_count) * 36
    {
        return Err("invalid packed fog scene counts/length".into());
    }
    let mut values = bytes[header..]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| f32::from_le_bytes(*v));
    let mut triangle = || std::array::from_fn(|_| std::array::from_fn(|_| values.next().unwrap()));
    let floors = (0..floor).map(|_| triangle()).collect();
    let obstacles = (0..obstacle).map(|_| triangle()).collect();
    let colors = if colored == 1 {
        (0..count * 3)
            .map(|_| std::array::from_fn(|_| values.next().unwrap()))
            .collect()
    } else {
        Vec::new()
    };
    let collision = (0..collision_count)
        .map(|_| std::array::from_fn(|_| std::array::from_fn(|_| values.next().unwrap())))
        .collect();
    let support = (0..support_count)
        .map(|_| std::array::from_fn(|_| std::array::from_fn(|_| values.next().unwrap())))
        .collect();
    let source = BakeSource {
        floor: floors,
        obstacle: obstacles,
        colors,
        collision,
        support,
    };
    source.validate()?;
    Ok(source)
}

pub fn save(source: &BakeSource, path: &Path) -> Result<(), Error> {
    source.validate()?;
    if source.floor.len() + source.obstacle.len() > MAX_TRIANGLES
        || source.collision.len() > MAX_TRIANGLES
        || source.support.len() > MAX_TRIANGLES
    {
        return Err("too many scene triangles".into());
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create_new(path)?);
    out.write_all(MAGIC)?;
    for n in [
        source.floor.len(),
        source.obstacle.len(),
        usize::from(!source.colors.is_empty()),
        source.collision.len(),
        source.support.len(),
    ] {
        out.write_all(&(n as u32).to_le_bytes())?;
    }
    for v in source
        .floor
        .iter()
        .chain(&source.obstacle)
        .flatten()
        .flatten()
        .chain(source.colors.iter().flatten())
        .chain(source.collision.iter().flatten().flatten())
        .chain(source.support.iter().flatten().flatten())
    {
        out.write_all(&v.to_le_bytes())?;
    }
    out.flush()?;
    Ok(())
}

/// Explicit asset-specific support selection, never guessed from material opacity.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BakeProfile {
    pub source_sha256: String,
    pub support_meshes: Vec<String>,
    pub collision_exclude_meshes: Vec<String>,
    pub minimum_support_up: f32,
    pub collision_error_m: f32,
    pub lock_collision_borders: bool,
}

/// Normalize the longest horizontal extent to `span` metres, centre XZ, put min Y at zero.
/// Input must be a self-contained GLB with triangle primitives and TEXCOORD_0 textures.
pub fn import_glb(path: &Path, span: f32) -> Result<BakeSource, Error> {
    import_glb_with_profile(path, span, None)
}
pub fn import_glb_with_profile(
    path: &Path,
    span: f32,
    profile: Option<&BakeProfile>,
) -> Result<BakeSource, Error> {
    if !span.is_finite() || !(1.0..=1000.0).contains(&span) {
        return Err("model span must be 1..1000 metres".into());
    }
    let gltf = gltf::Gltf::open(path)?;
    if let Some(profile) = profile {
        use sha2::{Digest, Sha256};
        if format!("{:x}", Sha256::digest(std::fs::read(path)?)) != profile.source_sha256 {
            return Err("bake profile GLB SHA256 mismatch".into());
        }
        crate::parameters::range("support up normal", profile.minimum_support_up, 0.01, 1.0)?;
        crate::parameters::range(
            "collision error metres",
            profile.collision_error_m,
            0.001,
            0.1,
        )?;
        if profile.support_meshes.is_empty() {
            return Err("bake profile requires explicit support meshes".into());
        }
        for name in profile
            .support_meshes
            .iter()
            .chain(&profile.collision_exclude_meshes)
        {
            if !gltf.meshes().any(|mesh| mesh.name() == Some(name.as_str())) {
                return Err(format!("bake profile mesh not found: {name}").into());
            }
        }
    }
    let blob = gltf
        .blob
        .as_deref()
        .ok_or("expected an embedded GLB buffer")?;
    if gltf
        .buffers()
        .any(|b| !matches!(b.source(), gltf::buffer::Source::Bin))
    {
        return Err("external GLB buffers are not supported".into());
    }
    let mut images = Vec::new();
    for image in gltf.images() {
        let gltf::image::Source::View { view, .. } = image.source() else {
            return Err("external GLB images are not supported".into());
        };
        let data = blob
            .get(view.offset()..view.offset() + view.length())
            .ok_or("image outside GLB buffer")?;
        images.push(image::load_from_memory(data)?.to_rgb8());
    }
    let mut source = BakeSource {
        floor: Vec::new(),
        obstacle: Vec::new(),
        colors: Vec::new(),
        collision: Vec::new(),
        support: Vec::new(),
    };
    let scene = gltf
        .default_scene()
        .or_else(|| gltf.scenes().next())
        .ok_or("GLB has no scene")?;
    for node in scene.nodes() {
        visit(node, Mat4::IDENTITY, blob, &images, &mut source, profile)?;
    }
    source.validate()?;
    if profile.is_some() && source.support.is_empty() {
        return Err("bake profile selected no upward support triangles".into());
    }
    if profile.is_some() && source.collision.is_empty() {
        return Err("bake profile excluded every collision triangle".into());
    }
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for p in source.floor.iter().flatten() {
        low = low.min(Vec3::from_array(*p));
        high = high.max(Vec3::from_array(*p));
    }
    let extent = high - low;
    let horizontal = extent.x.max(extent.z);
    if horizontal <= 1e-6 {
        return Err("GLB has no horizontal extent".into());
    }
    let scale = span / horizontal;
    let center = Vec3::new((low.x + high.x) * 0.5, low.y, (low.z + high.z) * 0.5);
    for p in source.floor.iter_mut().flatten() {
        *p = ((Vec3::from_array(*p) - center) * scale).to_array();
    }
    for p in source
        .collision
        .iter_mut()
        .chain(&mut source.support)
        .flatten()
    {
        *p = ((Vec3::from_array(*p) - center) * scale).to_array();
    }
    simplify_collision(&mut source, profile)?;
    eprintln!(
        "Explicit heightfield support: {} triangles",
        source.support.len()
    );
    eprintln!(
        "GLB bake: {} triangles; original bounds {low:?}..{high:?}; subtract {center:?}, scale {scale}; world extent {:?}",
        source.floor.len(),
        extent * scale
    );
    source.validate()?;
    Ok(source)
}

fn visit(
    node: gltf::Node<'_>,
    parent: Mat4,
    blob: &[u8],
    images: &[image::RgbImage],
    source: &mut BakeSource,
    profile: Option<&BakeProfile>,
) -> Result<(), Error> {
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        let support_mesh = profile.is_some_and(|p| {
            p.support_meshes
                .iter()
                .any(|name| Some(name.as_str()) == mesh.name())
        });
        let collision_mesh = !profile.is_some_and(|p| {
            p.collision_exclude_meshes
                .iter()
                .any(|name| Some(name.as_str()) == mesh.name())
        });
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                return Err("only triangle GLB primitives are supported".into());
            }
            let reader = primitive.reader(|_| Some(blob));
            let positions: Vec<_> = reader
                .read_positions()
                .ok_or("GLB primitive has no positions")?
                .collect();
            let uv: Vec<_> = reader
                .read_tex_coords(0)
                .map(|v| v.into_f32().collect())
                .unwrap_or_default();
            let indices: Vec<_> = reader
                .read_indices()
                .map(|v| v.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            if !indices.len().is_multiple_of(3) {
                return Err("incomplete GLB triangle".into());
            }
            if source.floor.len() + indices.len() / 3 > MAX_TRIANGLES {
                return Err("too many GLB triangles".into());
            }
            let solid = primitive.material().alpha_mode() == gltf::material::AlphaMode::Opaque;
            // The opaque playground renderer excludes alpha cards/transmission
            // from both visual depth and the single-layer heightfield.
            if !solid {
                continue;
            }
            let material = primitive.material().pbr_metallic_roughness();
            let factor = material.base_color_factor();
            let texture = material.base_color_texture();
            if texture.as_ref().is_some_and(|t| t.tex_coord() != 0) {
                return Err("only TEXCOORD_0 is supported".into());
            }
            for tri in indices.as_chunks::<3>().0 {
                let mut world = [[0.0; 3]; 3];
                let mut colors = [[0.0; 3]; 3];
                for (corner, &index) in tri.iter().enumerate() {
                    let p = positions
                        .get(index as usize)
                        .ok_or("GLB index outside positions")?;
                    world[corner] = transform.transform_point3(Vec3::from_array(*p)).to_array();
                    let mut color = Vec3::new(factor[0], factor[1], factor[2]);
                    if let Some(info) = &texture {
                        let coord = *uv
                            .get(index as usize)
                            .ok_or("textured primitive has no UV")?;
                        let tex = info.texture();
                        color *= sample(
                            &images[tex.source().index()],
                            Vec2::from_array(coord),
                            tex.sampler(),
                        );
                    }
                    colors[corner] = color.clamp(Vec3::ZERO, Vec3::ONE).to_array();
                }
                source.floor.push(world);
                // A visual mesh can collide without becoming heightfield support.
                if collision_mesh {
                    source.collision.push(world);
                }
                if support_mesh {
                    let [a, b, c] = world.map(Vec3::from_array);
                    let up = (b - a).cross(c - a).normalize_or_zero().y;
                    if up >= profile.unwrap().minimum_support_up {
                        source.support.push(world);
                    }
                }
                source.colors.extend(colors);
            }
        }
    }
    for child in node.children() {
        visit(child, transform, blob, images, source, profile)?;
    }
    Ok(())
}
fn simplify_collision(source: &mut BakeSource, profile: Option<&BakeProfile>) -> Result<(), Error> {
    let before = source.collision.len();
    let mut vertices = Vec::<[f32; 3]>::new();
    let mut map = std::collections::HashMap::new();
    let mut indices = Vec::with_capacity(before * 3);
    for p in source.collision.iter().flatten() {
        // Weld UV/material seams in collision only (10 micrometre quantization).
        let key = p.map(|v| (v * 100_000.0).round() as i64);
        let index = *map.entry(key).or_insert_with(|| {
            let index = vertices.len() as u32;
            vertices.push(*p);
            index
        });
        indices.push(index);
    }
    let adapter = meshopt::VertexDataAdapter::new(bytemuck::cast_slice(&vertices), 12, 0)?;
    let mut error = 0.0;
    let budget = profile.map_or(0.10, |p| p.collision_error_m);
    let mut options = meshopt::SimplifyOptions::ErrorAbsolute;
    if profile.is_some_and(|p| p.lock_collision_borders) {
        options |= meshopt::SimplifyOptions::LockBorder;
    }
    let reduced = meshopt::simplify(
        &indices,
        &adapter,
        (8_000 * 3).min(indices.len()),
        budget,
        options,
        Some(&mut error),
    );
    source.collision = reduced
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            [
                vertices[t[0] as usize],
                vertices[t[1] as usize],
                vertices[t[2] as usize],
            ]
        })
        .collect();
    eprintln!(
        "Collision: {before} -> {} triangles, meshopt reported error {error:.5} m (budget {budget} m, target 8000; topology/error may stop early)",
        source.collision.len()
    );
    Ok(())
}

fn sample(image: &image::RgbImage, uv: Vec2, sampler: gltf::texture::Sampler<'_>) -> Vec3 {
    let p = uv * Vec2::new(image.width() as f32, image.height() as f32) - Vec2::splat(0.5);
    let base = p.floor().as_ivec2();
    let f = p - p.floor();
    let wrap = |i: i32, n: u32, mode| {
        use gltf::texture::WrappingMode::*;
        let n = n as i32;
        (match mode {
            ClampToEdge => i.clamp(0, n - 1),
            Repeat => i.rem_euclid(n),
            MirroredRepeat => {
                let j = i.rem_euclid(2 * n);
                if j < n { j } else { 2 * n - 1 - j }
            }
        }) as u32
    };
    let texel = |x, y| {
        let rgb = image.get_pixel(
            wrap(x, image.width(), sampler.wrap_s()),
            wrap(y, image.height(), sampler.wrap_t()),
        );
        Vec3::from_array(rgb.0.map(|c| {
            let s = f32::from(c) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }))
    };
    texel(base.x, base.y)
        .lerp(texel(base.x + 1, base.y), f.x)
        .lerp(
            texel(base.x, base.y + 1).lerp(texel(base.x + 1, base.y + 1), f.x),
            f.y,
        )
}

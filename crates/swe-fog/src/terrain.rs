//! Explicit offline world-space triangle bake. Missing coverage is never a zero floor.
use crate::Error;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::Path};
pub type Triangle = [[f32; 3]; 3];
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BakeSource {
    pub floor: Vec<Triangle>,
    #[serde(default)]
    pub obstacle: Vec<Triangle>,
}
impl BakeSource {
    pub fn playground() -> Self {
        serde_json::from_str(include_str!("../assets/playground.scene.json"))
            .expect("bundled scene geometry")
    }
    pub fn validate(&self) -> Result<(), Error> {
        if self.floor.is_empty()
            || self
                .floor
                .iter()
                .chain(&self.obstacle)
                .flatten()
                .flatten()
                .any(|v| !v.is_finite())
        {
            return Err("bake requires finite static floor triangles".into());
        }
        Ok(())
    }
    pub fn digest(&self) -> Result<String, Error> {
        self.validate()?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub version: u32,
    pub domain: [f32; 4],
    pub resolution: [u32; 2],
    pub source_sha256: String,
    pub overlaps: u32,
}
#[derive(Clone, Debug)]
pub struct Terrain {
    pub metadata: Metadata,
    pub heights: Vec<f32>,
    pub valid: Vec<u8>,
    pub pass_x: Vec<u8>,
    pub pass_z: Vec<u8>,
}
impl Terrain {
    /// Explicit grid selection rebuilds from the validated static triangles in
    /// memory. It never interpolates away thin walls or rewrites baked assets.
    pub fn at_resolution(
        &self,
        source: &BakeSource,
        resolution: Option<[u32; 2]>,
    ) -> Result<Self, Error> {
        self.at_region(source, resolution, 1.0)
    }
    /// Re-bake a centred subregion in world units; never scale source geometry.
    pub fn at_region(
        &self,
        source: &BakeSource,
        resolution: Option<[u32; 2]>,
        scale: f32,
    ) -> Result<Self, Error> {
        self.validate()?;
        if !scale.is_finite() || !(0.0625..=1.0).contains(&scale) {
            return Err("区域范围缩放必须为 0.0625–1 的有限数".into());
        }
        if source.digest()? != self.metadata.source_sha256 {
            return Err("fog terrain source mismatch; cannot change grid region".into());
        }
        let resolution = resolution.unwrap_or(self.metadata.resolution);
        if scale == 1.0 && resolution == self.metadata.resolution {
            return Ok(self.clone());
        }
        let [x, z, w, d] = self.metadata.domain;
        let domain = [
            x + w * (1.0 - scale) * 0.5,
            z + d * (1.0 - scale) * 0.5,
            w * scale,
            d * scale,
        ];
        Self::bake(source, domain, resolution)
    }
    pub fn spacing(&self) -> Vec2 {
        Vec2::new(
            self.metadata.domain[2] / self.metadata.resolution[0] as f32,
            self.metadata.domain[3] / self.metadata.resolution[1] as f32,
        )
    }
    pub fn validate(&self) -> Result<(), Error> {
        let m = &self.metadata;
        let [nx, nz] = m.resolution;
        let n = nx as usize * nz as usize;
        if m.version != 1
            || [nx, nz].iter().any(|n| !(2..=1024).contains(n))
            || !m.domain.iter().all(|v| v.is_finite())
            || m.domain[2] <= 0.0
            || m.domain[3] <= 0.0
            || self.heights.len() != n
            || self.valid.len() != n
            || self.pass_x.len() != ((nx + 1) * nz) as usize
            || self.pass_z.len() != (nx * (nz + 1)) as usize
            || self.heights.iter().any(|v| !v.is_finite())
            || self
                .valid
                .iter()
                .chain(&self.pass_x)
                .chain(&self.pass_z)
                .any(|v| *v > 1)
            || !self.valid.contains(&1)
            || m.source_sha256.len() != 64
            || !m.source_sha256.bytes().all(|v| v.is_ascii_hexdigit())
        {
            return Err("invalid fog terrain dimensions, samples or metadata".into());
        }
        for z in 0..nz {
            for x in 0..nx {
                if self.valid[(z * nx + x) as usize] == 0
                    && (self.pass_x[(z * (nx + 1) + x) as usize] != 0
                        || self.pass_x[(z * (nx + 1) + x + 1) as usize] != 0
                        || self.pass_z[(z * nx + x) as usize] != 0
                        || self.pass_z[((z + 1) * nx + x) as usize] != 0)
                {
                    return Err("void terrain cells must have four blocked faces".into());
                }
            }
        }
        Ok(())
    }
    pub fn bake(
        source: &BakeSource,
        domain: [f32; 4],
        resolution: [u32; 2],
    ) -> Result<Self, Error> {
        source.validate()?;
        let [nx, nz] = resolution;
        if [nx, nz].iter().any(|n| !(2..=1024).contains(n))
            || !domain.iter().all(|v| v.is_finite())
            || domain[2] <= 0.0
            || domain[3] <= 0.0
        {
            return Err("invalid bake bounds/resolution".into());
        }
        let n = (nx * nz) as usize;
        let mut t = Self {
            metadata: Metadata {
                version: 1,
                domain,
                resolution,
                source_sha256: source.digest()?,
                overlaps: 0,
            },
            heights: vec![0.0; n],
            valid: vec![0; n],
            pass_x: vec![1; ((nx + 1) * nz) as usize],
            pass_z: vec![1; (nx * (nz + 1)) as usize],
        };
        let origin = Vec2::new(domain[0], domain[1]);
        let step = t.spacing();
        for tri in &source.floor {
            let p = tri.map(Vec3::from_array);
            let v = p.map(|v| Vec2::new(v.x, v.z));
            let area = (v[1] - v[0]).perp_dot(v[2] - v[0]);
            if area.abs() < 1e-10 {
                continue;
            }
            let low = ((v[0].min(v[1]).min(v[2]) - origin) / step - Vec2::splat(0.5))
                .ceil()
                .as_ivec2()
                .max(glam::IVec2::ZERO);
            let high = ((v[0].max(v[1]).max(v[2]) - origin) / step - Vec2::splat(0.5))
                .floor()
                .as_ivec2()
                .min(glam::IVec2::new(nx as i32 - 1, nz as i32 - 1));
            for z in low.y..=high.y {
                for x in low.x..=high.x {
                    let point = origin + (Vec2::new(x as f32, z as f32) + Vec2::splat(0.5)) * step;
                    let u = (point - v[0]).perp_dot(v[2] - v[0]) / area;
                    let w = (v[1] - v[0]).perp_dot(point - v[0]) / area;
                    if u < -1e-6 || w < -1e-6 || u + w > 1.000001 {
                        continue;
                    }
                    let h = p[0].y + u * (p[1].y - p[0].y) + w * (p[2].y - p[0].y);
                    let i = (z as u32 * nx + x as u32) as usize;
                    if t.valid[i] == 1 {
                        if (t.heights[i] - h).abs() > 0.001 {
                            t.metadata.overlaps += 1;
                        }
                        t.heights[i] = t.heights[i].max(h);
                    } else {
                        t.heights[i] = h;
                    }
                    t.valid[i] = 1;
                }
            }
        }
        for tri in &source.obstacle {
            let tri = tri.map(|v| Vec2::new(v[0], v[2]));
            let low = ((tri[0].min(tri[1]).min(tri[2]) - origin) / step)
                .floor()
                .as_ivec2()
                .max(glam::IVec2::ZERO);
            let high = ((tri[0].max(tri[1]).max(tri[2]) - origin) / step)
                .ceil()
                .as_ivec2()
                .min(glam::IVec2::new(nx as i32, nz as i32));
            for z in low.y..=(high.y.min(nz as i32 - 1)) {
                for x in low.x..=high.x {
                    let a = origin + Vec2::new(x as f32, z as f32) * step;
                    // Block transport between cell centres, not intersection
                    // with the perpendicular shared face: a thin wall may lie
                    // anywhere inside the cell after a domain/grid change.
                    if touches(
                        a + Vec2::new(-0.5 * step.x, 0.5 * step.y),
                        a + step * 0.5,
                        tri,
                    ) {
                        t.pass_x[z as usize * (nx + 1) as usize + x as usize] = 0;
                    }
                }
            }
            for z in low.y..=high.y {
                for x in low.x..=(high.x.min(nx as i32 - 1)) {
                    let a = origin + Vec2::new(x as f32, z as f32) * step;
                    if touches(
                        a + Vec2::new(0.5 * step.x, -0.5 * step.y),
                        a + step * 0.5,
                        tri,
                    ) {
                        t.pass_z[z as usize * nx as usize + x as usize] = 0;
                    }
                    if z < nz as i32 && inside(a + step * 0.5, tri) {
                        t.valid[z as usize * nx as usize + x as usize] = 0;
                    }
                }
            }
        }
        for z in 0..nz {
            for x in 0..=nx {
                if (x > 0 && t.valid[(z * nx + x - 1) as usize] == 0)
                    || (x < nx && t.valid[(z * nx + x) as usize] == 0)
                {
                    t.pass_x[(z * (nx + 1) + x) as usize] = 0;
                }
            }
        }
        for z in 0..=nz {
            for x in 0..nx {
                if (z > 0 && t.valid[((z - 1) * nx + x) as usize] == 0)
                    || (z < nz && t.valid[(z * nx + x) as usize] == 0)
                {
                    t.pass_z[(z * nx + x) as usize] = 0;
                }
            }
        }
        t.validate()?;
        Ok(t)
    }
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        self.validate()?;
        let json = serde_json::to_vec(&self.metadata)?;
        let mut file = std::fs::File::create_new(path)?;
        file.write_all(b"SWEFOG01")?;
        file.write_all(&(json.len() as u32).to_le_bytes())?;
        file.write_all(&json)?;
        for (h, v) in self.heights.iter().zip(&self.valid) {
            file.write_all(&h.to_le_bytes())?;
            file.write_all(&[*v])?;
        }
        file.write_all(&self.pass_x)?;
        file.write_all(&self.pass_z)?;
        file.sync_all()?;
        Ok(())
    }
    pub fn decode(bytes: &[u8], source: &BakeSource) -> Result<Self, Error> {
        if bytes.len() < 12 || &bytes[..8] != b"SWEFOG01" {
            return Err("invalid fog terrain header".into());
        }
        let len = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        if len > 65536 || bytes.len() < 12 + len {
            return Err("invalid fog metadata size".into());
        }
        let metadata: Metadata = serde_json::from_slice(&bytes[12..12 + len])?;
        let [nx, nz] = metadata.resolution;
        if [nx, nz].iter().any(|n| !(2..=1024).contains(n)) {
            return Err("invalid fog resolution".into());
        }
        let n = (nx * nz) as usize;
        let fx = ((nx + 1) * nz) as usize;
        let fz = (nx * (nz + 1)) as usize;
        if bytes.len() != 12 + len + n * 5 + fx + fz {
            return Err("fog terrain payload mismatch".into());
        }
        if metadata.source_sha256 != source.digest()? {
            return Err("fog terrain is stale; explicitly rebake static geometry".into());
        }
        let payload = &bytes[12 + len..];
        let mut heights = Vec::with_capacity(n);
        let mut valid = Vec::with_capacity(n);
        for cell in payload[..n * 5].as_chunks::<5>().0 {
            heights.push(f32::from_le_bytes(cell[..4].try_into()?));
            valid.push(cell[4]);
        }
        let t = Self {
            metadata,
            heights,
            valid,
            pass_x: payload[n * 5..n * 5 + fx].to_vec(),
            pass_z: payload[n * 5 + fx..].to_vec(),
        };
        t.validate()?;
        Ok(t)
    }
    pub fn load(path: &Path, source: &BakeSource) -> Result<Self, Error> {
        if std::fs::metadata(path)?.len() > 8 * 1024 * 1024 {
            return Err("fog terrain too large".into());
        }
        Self::decode(&std::fs::read(path)?, source)
    }
    pub fn height_at(&self, world: Vec2) -> Option<f32> {
        let d = self.metadata.domain;
        let p = (world - Vec2::new(d[0], d[1])) / self.spacing();
        let x = p.x.floor() as i32;
        let z = p.y.floor() as i32;
        let [nx, nz] = self.metadata.resolution;
        if x < 0 || z < 0 || x >= nx as i32 || z >= nz as i32 {
            return None;
        }
        let i = (z as u32 * nx + x as u32) as usize;
        (self.valid[i] != 0).then_some(self.heights[i])
    }
}
fn inside(p: Vec2, t: [Vec2; 3]) -> bool {
    let a = (t[1] - t[0]).perp_dot(t[2] - t[0]);
    if a.abs() < 1e-10 {
        return false;
    }
    let u = (p - t[0]).perp_dot(t[2] - t[0]) / a;
    let v = (t[1] - t[0]).perp_dot(p - t[0]) / a;
    u >= -1e-6 && v >= -1e-6 && u + v <= 1.000001
}
fn distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let delta = b - a;
    let t = ((p - a).dot(delta) / delta.length_squared().max(1e-20)).clamp(0.0, 1.0);
    p.distance(a + t * delta)
}
fn touches(a: Vec2, b: Vec2, t: [Vec2; 3]) -> bool {
    let edges = [t[1] - t[0], t[2] - t[1], t[0] - t[2]];
    let direction = edges
        .into_iter()
        .max_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
        .unwrap();
    // Crossing a zero-width projected wall is forbidden, travelling along it
    // is not. This matters when a wall lies exactly on a row of cell centres.
    if edges[0].perp_dot(t[2] - t[0]).abs() <= 1e-12
        && direction.length_squared() > 1e-20
        && (b - a).perp_dot(direction).abs() <= 1e-12
    {
        return false;
    }
    if inside(a, t) || inside(b, t) {
        return true;
    }
    for i in 0..3 {
        let c = t[i];
        let d = t[(i + 1) % 3];
        let den = (b - a).perp_dot(d - c);
        if den.abs() > 1e-12 {
            let u = (c - a).perp_dot(d - c) / den;
            let v = (c - a).perp_dot(b - a) / den;
            if (0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v) {
                return true;
            }
        }
        if distance(c, a, b) < 1e-6 || distance(a, c, d) < 1e-6 || distance(b, c, d) < 1e-6 {
            return true;
        }
    }
    false
}

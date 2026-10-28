//! Versioned on-disk model heightfield and inexpensive O(N²) SWE regridding.
//! The source map is independent of the simulation domain and resolution.
use crate::ocean::Error;
use crate::ocean::scene::model::Model;
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::{io::Write, path::Path};

const MAGIC: &[u8; 8] = b"SWEHMAP1";
const MAX_SIDE: u32 = 4096;

pub use crate::ocean::parameters::model::ModelConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub version: u32,
    pub source_sha256: String,
    pub bounds: [[f32; 3]; 2],
    pub resolution: [u32; 2],
    pub cap_epsilon: f32,
}
pub struct HeightField {
    pub metadata: Metadata,
    /// Cell-centered height / coverage (0 or 1), little-endian f32 on disk.
    pub samples: Vec<[f32; 2]>,
}
pub struct Grid {
    pub samples: Vec<[f32; 2]>,
    pub min_height: f32,
    pub max_height: f32,
}
impl HeightField {
    pub fn bake(model: &Model, spacing: f32, cap_epsilon: f32) -> Result<Self, Error> {
        if !spacing.is_finite() || spacing <= 0.0 {
            return Err("bake spacing must be positive and finite".into());
        }
        let bounds = model.bounds;
        let resolution = [0, 2].map(|a| ((bounds[1][a] - bounds[0][a]) / spacing).ceil() as u32);
        if resolution.iter().any(|&n| !(2..=MAX_SIDE).contains(&n)) {
            return Err("heightfield dimensions must be 2..4096; change --cell-size".into());
        }
        let mut map = Self {
            metadata: Metadata {
                version: 1,
                source_sha256: model.source_hash.clone(),
                bounds,
                resolution,
                cap_epsilon,
            },
            samples: vec![[0.0; 2]; (resolution[0] * resolution[1]) as usize],
        };
        let step = [0, 2].map(|a| (bounds[1][a] - bounds[0][a]) / resolution[a / 2] as f32);
        for triangle in model.indices.as_chunks::<3>().0 {
            let p = triangle.map(|i| model.vertices[i as usize].position);
            let area = (p[1][0] - p[0][0]) * (p[2][2] - p[0][2])
                - (p[1][2] - p[0][2]) * (p[2][0] - p[0][0]);
            if area.abs() < 1e-10 {
                continue;
            }
            let low = [0, 2].map(|a| {
                (((p.iter().map(|p| p[a]).fold(f32::INFINITY, f32::min) - bounds[0][a])
                    / step[a / 2]
                    - 0.5)
                    .ceil() as i32)
                    .clamp(0, resolution[a / 2] as i32 - 1)
            });
            let high = [0, 2].map(|a| {
                (((p.iter().map(|p| p[a]).fold(f32::NEG_INFINITY, f32::max) - bounds[0][a])
                    / step[a / 2]
                    - 0.5)
                    .floor() as i32)
                    .clamp(0, resolution[a / 2] as i32 - 1)
            });
            for z in low[1]..=high[1] {
                for x in low[0]..=high[0] {
                    let dx = bounds[0][0] + (x as f32 + 0.5) * step[0] - p[0][0];
                    let dz = bounds[0][2] + (z as f32 + 0.5) * step[1] - p[0][2];
                    let u = (dx * (p[2][2] - p[0][2]) - dz * (p[2][0] - p[0][0])) / area;
                    let v = ((p[1][0] - p[0][0]) * dz - (p[1][2] - p[0][2]) * dx) / area;
                    if u < -1e-5 || v < -1e-5 || u + v > 1.00001 {
                        continue;
                    }
                    let height = p[0][1] + u * (p[1][1] - p[0][1]) + v * (p[2][1] - p[0][1]);
                    let cell = &mut map.samples[(z as u32 * resolution[0] + x as u32) as usize];
                    if cell[1] == 0.0 || height > cell[0] {
                        *cell = [height, 1.0];
                    }
                }
            }
        }
        map.validate()?;
        Ok(map)
    }
    fn validate(&self) -> Result<(), Error> {
        let m = &self.metadata;
        if m.version != 1
            || m.resolution.iter().any(|&n| !(2..=MAX_SIDE).contains(&n))
            || self.samples.len() != m.resolution[0] as usize * m.resolution[1] as usize
            || !m.cap_epsilon.is_finite()
            || !(0.0..=1.0).contains(&m.cap_epsilon)
            || m.source_sha256.len() != 64
            || !m.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || (0..3).any(|a| {
                !m.bounds[0][a].is_finite()
                    || !m.bounds[1][a].is_finite()
                    || m.bounds[0][a] > m.bounds[1][a]
            })
            || [0, 2].iter().any(|&a| m.bounds[0][a] == m.bounds[1][a])
            || self
                .samples
                .iter()
                .any(|v| !v[0].is_finite() || (v[1] != 0.0 && v[1] != 1.0))
            || !self.samples.iter().any(|v| v[1] == 1.0)
        {
            return Err("invalid or unsupported baked heightfield".into());
        }
        Ok(())
    }
    /// Create-new prevents an interrupted bake from overwriting a useful cache.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        self.validate()?;
        let json = serde_json::to_vec(&self.metadata)?;
        let mut bytes = Vec::with_capacity(12 + json.len() + self.samples.len() * 8);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&json);
        for sample in &self.samples {
            for v in sample {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut file = std::fs::File::create_new(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    }
    pub fn load(path: &Path, source_hash: &str) -> Result<Self, Error> {
        if std::fs::metadata(path)?.len() > 12 + 65536 + u64::from(MAX_SIDE).pow(2) * 8 {
            return Err("heightfield file is too large".into());
        }
        let bytes = std::fs::read(path)?;
        Self::decode(&bytes, source_hash)
    }
    fn decode(bytes: &[u8], source_hash: &str) -> Result<Self, Error> {
        if bytes.len() < 12 || &bytes[..8] != MAGIC {
            return Err("invalid heightfield header".into());
        }
        let len = u32::from_le_bytes(bytes[8..12].try_into()?) as usize;
        if len > 65536 || 12 + len > bytes.len() {
            return Err("invalid heightfield metadata size".into());
        }
        let metadata: Metadata = serde_json::from_slice(&bytes[12..12 + len])?;
        if metadata.source_sha256 != source_hash {
            return Err("heightfield does not match GLB; run terrain-bake again".into());
        }
        if metadata
            .resolution
            .iter()
            .any(|&n| !(2..=MAX_SIDE).contains(&n))
            || bytes.len() - 12 - len
                != metadata.resolution[0] as usize * metadata.resolution[1] as usize * 8
        {
            return Err("heightfield dimensions/payload mismatch".into());
        }
        let samples = bytes[12 + len..]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|v| {
                [
                    f32::from_le_bytes(v[..4].try_into().unwrap()),
                    f32::from_le_bytes(v[4..].try_into().unwrap()),
                ]
            })
            .collect();
        let result = Self { metadata, samples };
        result.validate()?;
        Ok(result)
    }
    pub fn sample(&self, p: [f32; 2]) -> Option<f32> {
        let m = &self.metadata;
        if [0, 2]
            .iter()
            .enumerate()
            .any(|(i, &a)| p[i] < m.bounds[0][a] || p[i] > m.bounds[1][a])
        {
            return None;
        }
        let grid = [0, 2].map(|a| {
            (p[a / 2] - m.bounds[0][a]) / (m.bounds[1][a] - m.bounds[0][a])
                * m.resolution[a / 2] as f32
                - 0.5
        });
        let base = grid.map(|v| v.floor() as i32);
        let f = grid.map(|v| v - v.floor());
        let mut height = 0.0;
        let mut weight = 0.0;
        for z in 0..2 {
            for x in 0..2 {
                let c = [
                    (base[0] + x).clamp(0, m.resolution[0] as i32 - 1),
                    (base[1] + z).clamp(0, m.resolution[1] as i32 - 1),
                ];
                let v = self.samples[(c[1] as u32 * m.resolution[0] + c[0] as u32) as usize];
                let w = (if x == 0 { 1.0 - f[0] } else { f[0] })
                    * (if z == 0 { 1.0 - f[1] } else { f[1] })
                    * v[1];
                height += v[0] * w;
                weight += w;
            }
        }
        (weight > 0.5).then(|| height / weight)
    }
    /// Source samples are retained in RAM. Regridding never reloads GLB or casts
    /// rays. No temporal state is interpolated across changed domains.
    pub fn regrid(&self, config: ModelConfig, n: u32, depth: f32) -> Result<Grid, Error> {
        config.validate()?;
        if !(8..=1024).contains(&n) || !depth.is_finite() || depth <= 0.0 {
            return Err("invalid SWE regrid size/depth".into());
        }
        let transform = config.transform();
        let inverse = transform.inverse();
        let domain = config.domain();
        let m = &self.metadata;
        let mut samples = Vec::with_capacity((n * n) as usize);
        let mut min_height = f32::INFINITY;
        let mut max_height = f32::NEG_INFINITY;
        for z in 0..n {
            for x in 0..n {
                let world = Vec3::new(
                    domain[0] + (x as f32 + 0.5) * domain[2] / n as f32,
                    0.0,
                    domain[1] + (z as f32 + 0.5) * domain[3] / n as f32,
                );
                let local = inverse.transform_point3(world);
                let p = [local.x, local.z];
                let (height, coverage) = if let Some(y) = self.sample(p) {
                    (transform.transform_point3(Vec3::new(p[0], y, p[1])).y, 1.0)
                } else {
                    let edge = [
                        p[0].clamp(m.bounds[0][0], m.bounds[1][0]),
                        p[1].clamp(m.bounds[0][2], m.bounds[1][2]),
                    ];
                    let distance = (p[0] - edge[0]).hypot(p[1] - edge[1]) * config.scale;
                    // Missing interior scan data is explicit fallback, not zero elevation.
                    let height = self
                        .sample(edge)
                        .filter(|_| distance > 0.0)
                        .map(|y| {
                            let height =
                                transform.transform_point3(Vec3::new(edge[0], y, edge[1])).y;
                            let t = (distance / config.extension_width).clamp(0.0, 1.0);
                            let blend = t * t * (3.0 - 2.0 * t);
                            height * (1.0 - blend) - depth * blend
                        })
                        .unwrap_or(-depth);
                    (height, 0.0)
                };
                min_height = min_height.min(height);
                max_height = max_height.max(height);
                samples.push([height, coverage]);
            }
        }
        Ok(Grid {
            samples,
            min_height,
            max_height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocean::scene::model::Vertex;
    fn plane() -> Model {
        // Analytic bed y=x+2z, not generated by the rasterizer under test.
        Model {
            vertices: [[0., 0., 0.], [4., 4., 0.], [0., 8., 4.], [4., 12., 4.]]
                .map(|position| Vertex {
                    position,
                    normal: [0., 1., 0.],
                    uv: [0.; 2],
                })
                .to_vec(),
            indices: vec![0, 1, 2, 1, 3, 2],
            image: vec![],
            bounds: [[0., 0., 0.], [4., 12., 4.]],
            source_hash: "a".repeat(64),
            removed_triangles: 0,
        }
    }
    #[test]
    fn disk_asset_regrids_transformed_analytic_bed_and_rejects_corruption() {
        let map = HeightField::bake(&plane(), 0.25, 0.0).unwrap();
        let path = std::env::temp_dir().join(format!(
            "swe-height-{}-{}.sweh",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        map.save(&path).unwrap();
        let loaded = HeightField::load(&path, &"a".repeat(64)).unwrap();
        assert!(map.save(&path).is_err());
        assert!(HeightField::load(&path, &"b".repeat(64)).is_err());
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(HeightField::decode(&bytes[..bytes.len() - 1], &"a".repeat(64)).is_err());
        let mut corrupt = bytes.clone();
        let last_height = corrupt.len() - 8;
        corrupt[last_height..last_height + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(HeightField::decode(&corrupt, &"a".repeat(64)).is_err());
        let c = ModelConfig {
            enabled: true,
            scale: 2.0,
            yaw_deg: 90.0,
            position: [10., 3., 20.],
            sea_level: 1.0,
            center: [14., 0., 16.],
            size: [8., 32., 8.],
            sponge_width: 0.5,
            ..Default::default()
        };
        let grid = loaded.regrid(c, 8, 6.0).unwrap();
        for z in 0..8 {
            for x in 0..8 {
                let wx = 10.0 + x as f32 + 0.5;
                let wz = 12.0 + z as f32 + 0.5;
                let expected = 2.0 + (20.0 - wz) + 2.0 * (wx - 10.0);
                assert!((grid.samples[z * 8 + x][0] - expected).abs() < 1e-4);
                assert_eq!(grid.samples[z * 8 + x][1], 1.0);
            }
        }
    }
    #[test]
    fn missing_coverage_extension_and_legacy_config_have_explicit_defaults() {
        let mut map = HeightField::bake(&plane(), 0.25, 0.0).unwrap();
        for z in 4..12 {
            for x in 4..12 {
                map.samples[z * 16 + x] = [0.0, 0.0];
            }
        }
        assert_eq!(map.sample([2., 2.]), None);
        assert_eq!(map.sample([-1., 2.]), None);
        let extended = map
            .regrid(
                ModelConfig {
                    position: [0.; 3],
                    sea_level: 0.0,
                    center: [4., 0., 2.],
                    size: [8., 32., 8.],
                    sponge_width: 0.5,
                    extension_width: 2.0,
                    ..Default::default()
                },
                8,
                6.0,
            )
            .unwrap();
        // At (4.5,1.5), clamped last-center bed = 6.875; smoothstep(.25)=.15625.
        assert!((f64::from(extended.samples[3 * 8 + 4][0]) - 4.86328125).abs() < 1e-5);
        assert_eq!(extended.samples[3 * 8 + 7], [-6.0, 0.0]);
        let c = ModelConfig::default();
        for invalid in [
            ModelConfig {
                scale: f32::NAN,
                ..c
            },
            ModelConfig {
                size: [10., 64., 10.],
                ..c
            },
        ] {
            assert!(invalid.validate().is_err());
        }
        let legacy: ModelConfig = serde_json::from_str("{}").unwrap();
        assert!(!legacy.enabled);
    }
}

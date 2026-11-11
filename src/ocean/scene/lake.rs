//! Deterministic single-layer lake terrain and Euclidean distance to its wet/dry boundary.
use crate::ocean::{
    Error,
    parameters::lake::{LakeConfig, LakeTerrain},
};

pub struct LakeScene {
    /// bed height (m), normalized depth, normalized positive SDF, signed SDF (m).
    pub cells: Vec<[f32; 4]>,
    pub extent: f32,
    pub rocks: u32,
}

pub(crate) fn random(seed: u32, index: u32) -> f32 {
    let mut x = seed.wrapping_add(index.wrapping_mul(0x9e3779b9));
    x = (x ^ (x >> 16)).wrapping_mul(0x85ebca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2ae35);
    ((x ^ (x >> 16)) >> 8) as f32 / 16777216.0
}
fn radius(t: LakeTerrain, angle: f32) -> f32 {
    let mut noise = 0.0;
    for (i, (frequency, weight)) in [(3.0, 0.5), (5.0, 0.3), (9.0, 0.2)].into_iter().enumerate() {
        noise +=
            weight * (angle * frequency + random(t.seed, i as u32) * std::f32::consts::TAU).sin();
    }
    t.radius * (1.0 + t.irregularity * noise)
}
impl LakeScene {
    pub fn build(config: &LakeConfig) -> Result<Self, Error> {
        config.validate()?;
        let t = config.terrain;
        let n = t.resolution as usize;
        let extent = t.radius * 3.0;
        // A height-field mound is both the visible rock and the obstacle; no hidden proxy.
        let mut rocks = Vec::new();
        for i in 0..t.rocks {
            let angle = random(t.seed ^ 701, i * 5) * std::f32::consts::TAU;
            let r = radius(t, angle) * (0.35 + 0.5 * random(t.seed, i * 5 + 10));
            let size = t.rock_size * (0.65 + 0.7 * random(t.seed, i * 5 + 11));
            let p = [r * angle.cos(), r * angle.sin()];
            if rocks
                .iter()
                .any(|&(q, s, _, _): &([f32; 2], f32, f32, f32)| {
                    (p[0] - q[0]).hypot(p[1] - q[1]) < (size + s) * 1.2
                })
            {
                continue;
            }
            // Every third rock remains below the surface.
            let top = if i % 3 == 0 { -0.3 } else { size * 0.65 };
            rocks.push((p, size, top, angle));
        }
        let mut cells = Vec::with_capacity(n * n);
        for z in 0..n {
            for x in 0..n {
                let p = [
                    (x as f32 / (n - 1) as f32 - 0.5) * extent,
                    (z as f32 / (n - 1) as f32 - 0.5) * extent,
                ];
                let r = p[0].hypot(p[1]);
                let u = r / radius(t, p[1].atan2(p[0]));
                let base = t.depth * (u * u - 1.0);
                let noise = (p[0] * 0.24 + 0.4).sin()
                    * (p[1] * 0.19).sin()
                    * 0.035
                    * t.depth
                    * (1.0 - u).max(0.0);
                let mut bed = base + noise;
                for &(q, size, top, angle) in &rocks {
                    let dx = p[0] - q[0];
                    let dz = p[1] - q[1];
                    let a = (dx * angle.cos() + dz * angle.sin()) / size;
                    let b = (-dx * angle.sin() + dz * angle.cos()) / (size * 0.72);
                    let d2 = a * a + b * b;
                    if d2 < 1.0 {
                        let crest = (1.0 - d2).sqrt();
                        let rough = 1.0 + 0.07 * (a * 12.0 + b * 7.0).sin() * (b * 9.0).cos();
                        let rock_bed = base + (top - base) * crest * rough;
                        bed = bed.max(if top < 0.0 {
                            rock_bed.min(top)
                        } else {
                            rock_bed
                        });
                    }
                }
                cells.push([bed, (-bed).max(0.0), 0.0, 0.0]);
            }
        }
        let wet: Vec<_> = cells.iter().map(|c| c[0] < 0.0).collect();
        if !wet.iter().any(|v| *v) {
            return Err("lake terrain contains no water at this resolution".into());
        }
        let sdf = signed_distance(&wet, n, extent / (n - 1) as f32);
        let max_depth = cells.iter().map(|c| c[1]).fold(0.001_f32, f32::max);
        let max_distance = sdf.iter().copied().fold(0.001_f32, f32::max);
        for (c, d) in cells.iter_mut().zip(sdf) {
            c[1] /= max_depth;
            c[2] = d.max(0.0) / max_distance;
            c[3] = d;
        }
        Ok(Self {
            cells,
            extent,
            rocks: rocks.len() as u32,
        })
    }
}

/// Squared Euclidean distance transform: separable lower envelopes of parabolas.
fn transform_line(f: &[f32]) -> Vec<f32> {
    let n = f.len();
    let mut sites = vec![0_usize; n];
    let mut cuts = vec![0.0_f32; n + 1];
    let mut k = 0;
    cuts[0] = f32::NEG_INFINITY;
    cuts[1] = f32::INFINITY;
    for q in 1..n {
        let mut s;
        loop {
            let v = sites[k];
            s = ((f[q] + (q * q) as f32) - (f[v] + (v * v) as f32)) / (2 * (q - v)) as f32;
            if s > cuts[k] || k == 0 {
                break;
            }
            k -= 1;
        }
        k += 1;
        sites[k] = q;
        cuts[k] = s;
        cuts[k + 1] = f32::INFINITY;
    }
    k = 0;
    (0..n)
        .map(|q| {
            while cuts[k + 1] < q as f32 {
                k += 1;
            }
            let d = q as f32 - sites[k] as f32;
            d * d + f[sites[k]]
        })
        .collect()
}
fn distance_to(mask: &[bool], n: usize, target: bool) -> Vec<f32> {
    let mut result = vec![0.0; n * n];
    for y in 0..n {
        let f: Vec<_> = mask[y * n..(y + 1) * n]
            .iter()
            .map(|v| if *v == target { 0.0 } else { 1e8 })
            .collect();
        result[y * n..(y + 1) * n].copy_from_slice(&transform_line(&f));
    }
    for x in 0..n {
        let f: Vec<_> = (0..n).map(|y| result[y * n + x]).collect();
        for (y, d) in transform_line(&f).into_iter().enumerate() {
            result[y * n + x] = d;
        }
    }
    result
}
fn signed_distance(wet: &[bool], n: usize, spacing: f32) -> Vec<f32> {
    let dry_distance = distance_to(wet, n, false);
    let wet_distance = distance_to(wet, n, true);
    wet.iter()
        .enumerate()
        .map(|(i, wet)| {
            if *wet {
                (dry_distance[i].sqrt() - 0.5) * spacing
            } else {
                -(wet_distance[i].sqrt() - 0.5) * spacing
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distance_field_resolves_a_lake_with_an_island() {
        let n = 65;
        let wet: Vec<_> = (0..n * n)
            .map(|i| {
                let r = ((i % n) as f32 - 32.0).hypot((i / n) as f32 - 32.0);
                r < 28.0 && r > 6.0
            })
            .collect();
        let sdf = signed_distance(&wet, n, 1.0);
        for (i, d) in sdf.iter().enumerate() {
            let r = ((i % n) as f32 - 32.0).hypot((i / n) as f32 - 32.0);
            let reference = (28.0 - r).min(r - 6.0);
            assert!(
                (d - reference).abs() < 1.5,
                "r={r}, distance={d}, reference={reference}"
            );
            assert_eq!(*d > 0.0, wet[i]);
        }
    }
    #[test]
    fn terrain_is_repeatable_and_submerged_rocks_change_depth_not_wet_mask() {
        let mut config = LakeConfig::default();
        config.terrain.resolution = 128;
        config.terrain.rocks = 1;
        let a = LakeScene::build(&config).unwrap();
        assert_eq!(a.cells, LakeScene::build(&config).unwrap().cells);
        config.terrain.rocks = 0;
        let b = LakeScene::build(&config).unwrap();
        assert!(a.cells.iter().zip(&b.cells).any(|(a, b)| a[0] > b[0] + 0.1));
        for (a, b) in a.cells.iter().zip(&b.cells) {
            assert_eq!(a[0] < 0.0, b[0] < 0.0);
        }
    }
}

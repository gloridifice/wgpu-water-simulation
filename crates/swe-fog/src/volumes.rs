//! Independent, vertically clipped fog bake regions. Scene/collider geometry is never edited.
use crate::{
    Error,
    terrain::{BakeSource, Terrain, Triangle},
};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FogVolume {
    pub name: String,
    /// Bottom centre in world metres.
    pub bottom: [f32; 3],
    /// Full local X/Y/Z extents in metres.
    pub size: [f32; 3],
    pub yaw_degrees: f32,
}
impl Default for FogVolume {
    fn default() -> Self {
        Self {
            name: "局部雾场".into(),
            bottom: [0.0; 3],
            size: [16.0, 8.0, 16.0],
            yaw_degrees: 0.0,
        }
    }
}
impl FogVolume {
    pub fn validate(&self) -> Result<(), Error> {
        if self
            .bottom
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1000.0)
            || self
                .size
                .iter()
                .any(|v| !v.is_finite() || !(0.1..=256.0).contains(v))
            || !self.yaw_degrees.is_finite()
            || self.yaw_degrees.abs() > 360.0
        {
            return Err(
                "雾场盒体位置须为 ±1000 m，尺寸 0.1–256 m，Y 旋转 ±360°，且均为有限数".into(),
            );
        }
        Ok(())
    }
    fn local(&self, p: Vec3) -> Vec3 {
        glam::Quat::from_rotation_y(-self.yaw_degrees.to_radians())
            * (p - Vec3::from_array(self.bottom))
    }
    fn world(&self, p: Vec3) -> Vec3 {
        glam::Quat::from_rotation_y(self.yaw_degrees.to_radians()) * p
            + Vec3::from_array(self.bottom)
    }
    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|i| {
            self.world(Vec3::new(
                if i & 1 == 0 {
                    -self.size[0] * 0.5
                } else {
                    self.size[0] * 0.5
                },
                if i & 2 == 0 { 0.0 } else { self.size[1] },
                if i & 4 == 0 {
                    -self.size[2] * 0.5
                } else {
                    self.size[2] * 0.5
                },
            ))
        })
    }
    fn clip(&self, triangles: &[Triangle]) -> Vec<Triangle> {
        let mut result = Vec::new();
        for tri in triangles {
            let mut polygon: Vec<_> = tri
                .iter()
                .map(|p| self.local(Vec3::from_array(*p)))
                .collect();
            for (axis, sign, bound) in [
                (0, -1.0, self.size[0] * 0.5),
                (0, 1.0, self.size[0] * 0.5),
                (1, -1.0, 0.0),
                (1, 1.0, self.size[1]),
                (2, -1.0, self.size[2] * 0.5),
                (2, 1.0, self.size[2] * 0.5),
            ] {
                let mut clipped = Vec::new();
                if let Some(&last) = polygon.last() {
                    let mut a = last;
                    for &b in &polygon {
                        let da = sign * a[axis] - bound;
                        let db = sign * b[axis] - bound;
                        if (da <= 0.0) != (db <= 0.0) {
                            clipped.push(a.lerp(b, da / (da - db)));
                        }
                        if db <= 0.0 {
                            clipped.push(b);
                        }
                        a = b;
                    }
                }
                polygon = clipped;
            }
            for i in 1..polygon.len().saturating_sub(1) {
                result.push(
                    [polygon[0], polygon[i], polygon[i + 1]].map(|p| self.world(p).to_array()),
                );
            }
        }
        result
    }
    pub fn bake(&self, source: &BakeSource, resolution: [u32; 2]) -> Result<Terrain, Error> {
        self.validate()?;
        source.validate()?;
        // Boxes replace the legacy global support selection: any upward visual
        // surface inside the box may carry fog, including upper storeys/roofs.
        let support: Vec<_> = source
            .floor
            .iter()
            .filter(|tri| {
                let p = tri.map(Vec3::from_array);
                (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero().y > 0.5
            })
            .copied()
            .collect();
        let floor = self.clip(&support);
        if floor.is_empty() {
            return Err(format!("雾场“{}”盒内没有承载面", self.name).into());
        }
        let clipped = BakeSource {
            floor,
            obstacle: self.clip(&source.obstacle),
            collision: self.clip(&source.collision),
            support: Vec::new(),
            colors: Vec::new(),
        };
        let corners = self.corners();
        let min = corners
            .iter()
            .map(|p| Vec2::new(p.x, p.z))
            .fold(Vec2::splat(f32::INFINITY), Vec2::min);
        let max = corners
            .iter()
            .map(|p| Vec2::new(p.x, p.z))
            .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
        // A world-aligned grid enclosing the rotated footprint; clipped coverage
        // leaves the exterior invalid rather than introducing an AABB-shaped field.
        let mut terrain = Terrain::bake(
            &clipped,
            [min.x, min.y, max.x - min.x, max.y - min.y],
            resolution,
        )?;
        terrain.metadata.source_sha256 = source.digest()?;
        Ok(terrain)
    }
}

pub fn bake_fields(
    native: &Terrain,
    source: &BakeSource,
    session: &crate::parameters::Session,
) -> Result<Vec<Terrain>, Error> {
    if session.volumes.is_empty() {
        return Ok(vec![native.at_region(
            source,
            session.grid_resolution,
            session.domain_scale,
        )?]);
    }
    native.validate()?;
    if native.metadata.source_sha256 != source.digest()? {
        return Err("fog terrain source mismatch".into());
    }
    session
        .volumes
        .iter()
        .map(|v| {
            v.bake(
                source,
                session
                    .grid_resolution
                    .unwrap_or(native.metadata.resolution),
            )
        })
        .collect()
}

//! Conservative sphere-arm clipping against the same static world triangles used for baking/rendering.
use crate::{scene::Player, terrain::BakeSource};
use glam::Vec3;
pub fn orbit(player: &Player, camera: [f32; 3], source: &BakeSource) -> common::camera::Orbit {
    let mut result = player.orbit(camera);
    let direction = (result.eye() - result.target).normalize();
    let mut distance = result.distance;
    let radius = 0.15;
    for tri in source.floor.iter().chain(&source.obstacle) {
        let [a, b, c] = tri.map(Vec3::from_array);
        let cross = (b - a).cross(c - a);
        if cross.length_squared() > 1e-12 {
            let mut normal = cross.normalize();
            if (result.target - a).dot(normal) < 0.0 {
                normal = -normal;
            }
            let approach = direction.dot(normal);
            if approach < -1e-6 {
                let t = (radius - (result.target - a).dot(normal)) / approach;
                if t >= 0.0 && t < distance {
                    let contact = result.target + direction * t - normal * radius;
                    let ab = b - a;
                    let ac = c - a;
                    let q = contact - a;
                    let det = ab.dot(ab) * ac.dot(ac) - ab.dot(ac).powi(2);
                    let u = (q.dot(ab) * ac.dot(ac) - q.dot(ac) * ab.dot(ac)) / det;
                    let v = (q.dot(ac) * ab.dot(ab) - q.dot(ab) * ab.dot(ac)) / det;
                    if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
                        distance = t;
                    }
                }
            }
        }
        for (a, b) in [(a, b), (b, c), (c, a)] {
            if let Some(t) = capsule(result.target, direction, a, b, radius) {
                distance = distance.min(t);
            }
        }
    }
    result.distance = if distance < result.distance {
        (distance - 0.03).max(0.25)
    } else {
        distance
    };
    result
}
fn sphere(o: Vec3, d: Vec3, c: Vec3, r: f32) -> Option<f32> {
    let q = o - c;
    let b = q.dot(d);
    let h = b * b - q.length_squared() + r * r;
    if h < 0.0 {
        return None;
    }
    let t = -b - h.sqrt();
    (t >= 0.0).then_some(t)
}
fn capsule(o: Vec3, d: Vec3, a: Vec3, b: Vec3, r: f32) -> Option<f32> {
    let ba = b - a;
    let oa = o - a;
    let baba = ba.length_squared();
    if baba < 1e-12 {
        return sphere(o, d, a, r);
    }
    let bard = ba.dot(d);
    let baoa = ba.dot(oa);
    let aa = baba - bard * bard;
    let bb = baba * oa.dot(d) - baoa * bard;
    let cc = baba * oa.length_squared() - baoa * baoa - r * r * baba;
    let h = bb * bb - aa * cc;
    let mut result = sphere(o, d, a, r)
        .into_iter()
        .chain(sphere(o, d, b, r))
        .min_by(f32::total_cmp);
    if h >= 0.0 && aa.abs() > 1e-8 {
        let t = (-bb - h.sqrt()) / aa;
        let y = baoa + t * bard;
        if t >= 0.0 && y > 0.0 && y < baba {
            result = Some(result.map_or(t, |old| old.min(t)));
        }
    }
    result
}

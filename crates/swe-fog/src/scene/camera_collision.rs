//! Conservative sphere-arm clipping against the same static world triangles used for baking/rendering.
use crate::{
    scene::Player,
    terrain::{BakeSource, Triangle},
};
use glam::Vec3;
/// Static BVH built once from the offline simplified collider, never from visual detail each frame.
pub struct Collider {
    triangles: Vec<Triangle>,
    nodes: Vec<Node>,
}
struct Node {
    low: Vec3,
    high: Vec3,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}
impl Collider {
    pub fn new(source: &BakeSource) -> Self {
        let triangles = if source.collision.is_empty() {
            source
                .floor
                .iter()
                .chain(&source.obstacle)
                .copied()
                .collect()
        } else {
            source.collision.clone()
        };
        let mut result = Self {
            triangles,
            nodes: Vec::new(),
        };
        if !result.triangles.is_empty() {
            result.build(0, result.triangles.len());
        }
        result
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for p in self.triangles[start..end].iter().flatten() {
            low = low.min(Vec3::from_array(*p));
            high = high.max(Vec3::from_array(*p));
        }
        let index = self.nodes.len();
        self.nodes.push(Node {
            low,
            high,
            start,
            end,
            children: None,
        });
        if end - start > 8 {
            let extent = high - low;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            self.triangles[start..end].sort_unstable_by(|a, b| {
                let center = |t: &Triangle| t[0][axis] + t[1][axis] + t[2][axis];
                center(a).total_cmp(&center(b))
            });
            let mid = (start + end) / 2;
            let left = self.build(start, mid);
            let right = self.build(mid, end);
            self.nodes[index].children = Some((left, right));
        }
        index
    }
    pub fn orbit(&self, player: &Player, camera: [f32; 3]) -> common::camera::Orbit {
        let orbit = player.orbit(camera);
        let low = orbit.eye().min(orbit.target) - Vec3::splat(0.15);
        let high = orbit.eye().max(orbit.target) + Vec3::splat(0.15);
        let mut candidates = Vec::new();
        if !self.nodes.is_empty() {
            self.query(0, low, high, &mut candidates);
        }
        clip(player, camera, candidates.into_iter())
    }
    fn query<'a>(&'a self, index: usize, low: Vec3, high: Vec3, out: &mut Vec<&'a Triangle>) {
        let node = &self.nodes[index];
        if node.low.cmpgt(high).any() || node.high.cmplt(low).any() {
            return;
        }
        if let Some((left, right)) = node.children {
            self.query(left, low, high, out);
            self.query(right, low, high, out);
        } else {
            out.extend(&self.triangles[node.start..node.end]);
        }
    }
}
pub fn orbit(player: &Player, camera: [f32; 3], source: &BakeSource) -> common::camera::Orbit {
    Collider::new(source).orbit(player, camera)
}
fn clip<'a>(
    player: &Player,
    camera: [f32; 3],
    triangles: impl Iterator<Item = &'a Triangle>,
) -> common::camera::Orbit {
    let mut result = player.orbit(camera);
    let direction = (result.eye() - result.target).normalize();
    let mut distance = result.distance;
    let radius = 0.15;
    for tri in triangles {
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

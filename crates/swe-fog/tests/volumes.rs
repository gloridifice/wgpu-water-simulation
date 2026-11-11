//! Volume bake contracts: stacked floors, partial triangles and rotated void boundaries.
use common::settings::Settings;
use glam::Vec2;
use swe_fog::{
    parameters::Session,
    terrain::{BakeSource, Triangle},
    volumes::FogVolume,
};
fn plane(height: impl Fn(f32) -> f32) -> Vec<Triangle> {
    let p = |x, z| [x, height(x), z];
    vec![
        [p(-8.0, -8.0), p(-8.0, 8.0), p(8.0, 8.0)],
        [p(-8.0, -8.0), p(8.0, 8.0), p(8.0, -8.0)],
    ]
}
fn source(floor: Vec<Triangle>) -> BakeSource {
    BakeSource {
        floor,
        obstacle: vec![],
        colors: vec![],
        collision: vec![],
        support: vec![],
    }
}
#[test]
fn boxes_separate_stacked_surfaces_and_clip_crossing_slopes() {
    let mut scene = source(plane(|_| 0.0));
    scene.floor.extend(plane(|_| 4.0));
    // Old global support excludes the upper floor. Boxes must still find it.
    scene.support = plane(|_| 0.0);
    for (bottom, expected) in [(-1.0, 0.0), (3.0, 4.0)] {
        let b = FogVolume {
            bottom: [0.0, bottom, 0.0],
            size: [8.0, 2.0, 8.0],
            ..Default::default()
        };
        let t = b.bake(&scene, [16, 16]).unwrap();
        assert!(t.valid.iter().all(|v| *v == 1));
        assert!(t.heights.iter().all(|h| (*h - expected).abs() < 1e-5));
    }
    let mut slope = source(plane(|_| 0.0));
    slope.floor.extend(plane(|x| x + 2.0));
    let b = FogVolume {
        bottom: [0.0, -1.0, 0.0],
        size: [8.0, 4.0, 8.0],
        ..Default::default()
    };
    let t = b.bake(&slope, [16, 16]).unwrap();
    // Entire slope triangles cross the ceiling: do not discard the whole
    // triangle and do not clamp its excluded upper part into a false plateau.
    assert!((t.height_at(Vec2::new(0.25, 0.25)).unwrap() - 2.25).abs() < 1e-5);
    assert_eq!(t.height_at(Vec2::new(2.25, 0.25)), Some(0.0));
}
#[test]
fn rotated_footprint_has_no_aabb_fog_and_blocks_void_faces() {
    let b = FogVolume {
        bottom: [2.0, -1.0, -1.0],
        size: [6.0, 2.0, 2.0],
        yaw_degrees: 45.0,
        ..Default::default()
    };
    let t = b.bake(&source(plane(|_| 0.0)), [40, 40]).unwrap();
    let [x, z, w, d] = t.metadata.domain;
    let mut inside = 0;
    for iz in 0..40 {
        for ix in 0..40 {
            let px = x + (ix as f32 + 0.5) * w / 40.0 - 2.0;
            let pz = z + (iz as f32 + 0.5) * d / 40.0 + 1.0;
            // Independent analytic inverse of a +45 degree Y rotation.
            let local_x = (px - pz) * std::f32::consts::FRAC_1_SQRT_2;
            let local_z = (px + pz) * std::f32::consts::FRAC_1_SQRT_2;
            if (local_x.abs() - 3.0).abs() < 1e-4 || (local_z.abs() - 1.0).abs() < 1e-4 {
                continue;
            }
            let valid = local_x.abs() < 3.0 && local_z.abs() < 1.0;
            assert_eq!(t.valid[iz * 40 + ix], u8::from(valid));
            if valid {
                inside += 1;
            } else {
                assert_eq!(
                    [
                        t.pass_x[iz * 41 + ix],
                        t.pass_x[iz * 41 + ix + 1],
                        t.pass_z[iz * 40 + ix],
                        t.pass_z[(iz + 1) * 40 + ix]
                    ],
                    [0; 4]
                );
            }
        }
    }
    assert!(inside > 100 && inside < 1200);
}
#[test]
fn volume_configuration_is_backward_compatible_and_rejects_invalid_boxes() {
    let old: Session = serde_json::from_str("{}").unwrap();
    assert!(old.volumes.is_empty());
    let mut s = Session {
        volumes: vec![FogVolume::default()],
        ..old
    };
    s.validate().unwrap();
    let restored: Session = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
    assert_eq!(restored.volumes, s.volumes);
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        s.volumes[0].size[1] = size;
        assert!(s.validate().is_err());
    }
    s.volumes[0] = FogVolume {
        bottom: [0.0, 50.0, 0.0],
        ..Default::default()
    };
    assert!(
        s.volumes[0]
            .bake(&source(plane(|_| 0.0)), [16, 16])
            .is_err()
    );
}

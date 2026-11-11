//! Asset delivery and world-transform contracts; independent of rendering implementation.
use swe_fog::{model, scenes::ScenePreset, terrain::Terrain};

#[test]
fn bundled_scene_has_explicit_support_matching_heightfield_and_reduced_collider() {
    let (source, terrain) = ScenePreset::Grandma.load().unwrap();
    assert!(
        !source.support.is_empty(),
        "roof/prop exclusion must survive packed asset delivery"
    );
    assert!(!source.collision.is_empty());
    // Small-feature coverage takes priority over the previous 25% reduction.
    assert!(
        source.collision.len() * 100 < source.floor.len() * 85,
        "retain at least 15% triangle reduction without losing thin physical features"
    );
    assert_eq!(terrain.metadata.resolution, [256, 256]);
    assert!(terrain.valid.iter().filter(|v| **v == 1).count() > 26_000);
    // Measured visible surface landmarks in normalized world metres. These
    // protect the actual delivered asset, not a second copy of its bake rules.
    for (name, xz, height) in [
        ("stone staircase", [18.125, -23.625], 25.37864),
        ("upper stone tread", [14.125, -24.625], 29.44157),
        ("gazebo floor (not roof)", [9.125, 6.125], 15.9315),
        ("bridge deck (not railing)", [5.375, 23.125], 33.52908),
    ] {
        let actual = terrain.height_at(glam::Vec2::from_array(xz));
        assert!(
            actual.is_some_and(|h| (h - height).abs() < 0.03),
            "{name}: {actual:?}, expected {height} m"
        );
    }
    for tip in [
        [-23.14349, 60.41555, -24.31067],
        [-22.65673, 51.02592, -20.54791],
    ] {
        let p = glam::Vec3::from_array(tip);
        let nearest = source
            .collision
            .iter()
            .map(|tri| {
                let [a, b, c] = tri.map(glam::Vec3::from_array);
                let n = (b - a).cross(c - a).normalize_or_zero();
                let projected = p - n * (p - a).dot(n);
                let inside = [(a, b), (b, c), (c, a)]
                    .into_iter()
                    .all(|(a, b)| (b - a).cross(projected - a).dot(n) >= -1e-8);
                if inside && n.length_squared() > 0.5 {
                    (p - a).dot(n).abs()
                } else {
                    [(a, b), (b, c), (c, a)]
                        .into_iter()
                        .map(|(a, b)| {
                            let e = b - a;
                            p.distance(
                                a + e
                                    * ((p - a).dot(e) / e.length_squared().max(1e-20))
                                        .clamp(0.0, 1.0),
                            )
                        })
                        .fold(f32::INFINITY, f32::min)
                }
            })
            .fold(f32::INFINITY, f32::min);
        assert!(
            nearest < 0.03,
            "thin branch coverage at {tip:?} lost by {nearest} m"
        );
    }
    let mut player = swe_fog::scene::Player::default();
    player.place(&terrain).unwrap();
    let p = glam::Vec2::new(player.feet[0], player.feet[2]);
    assert!(player.feet[1] >= terrain.height_at(p).unwrap());
    let collider = swe_fog::scene::camera_collision::Collider::new(&source);
    for yaw in [0.0, 1.5, 3.0, 4.5] {
        let orbit = collider.orbit(&player, [yaw, 0.3, 5.5]);
        assert!(orbit.eye().is_finite());
        assert!((0.25..=5.5).contains(&orbit.distance));
    }
}

#[test]
fn explicit_ground_survives_packing_without_baking_an_overhead_roof_into_the_floor() {
    use sha2::{Digest, Sha256};
    let dir = std::env::temp_dir().join(format!(
        "fog-model-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let document = serde_json::json!({
        "asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],
        "nodes":[{"translation":[10,5,-7],"children":[1,2]},{"scale":[2,1,1],"mesh":0},{"scale":[2,1,1],"translation":[0,3,0],"mesh":1}],
        "meshes":[{"name":"ground","primitives":[{"attributes":{"POSITION":0},"material":0}]},{"name":"roof","primitives":[{"attributes":{"POSITION":0},"material":0}]}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.25,0.5,0.75,1]}}],
        "buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[2,1,2]}]
    });
    let mut json = serde_json::to_vec(&document).unwrap();
    while !json.len().is_multiple_of(4) {
        json.push(b' ');
    }
    let mut bytes = Vec::new();
    for v in [
        0x46546c67u32,
        2,
        (12 + 8 + json.len() + 8 + 36) as u32,
        json.len() as u32,
        0x4e4f534a,
    ] {
        bytes.extend(v.to_le_bytes());
    }
    bytes.extend(json);
    bytes.extend(36u32.to_le_bytes());
    bytes.extend(0x004e4942u32.to_le_bytes());
    for v in [0.0f32, 0.0, 0.0, 0.0, 0.0, 2.0, 2.0, 1.0, 0.0] {
        bytes.extend(v.to_le_bytes());
    }
    let glb = dir.join("overhead.glb");
    std::fs::write(&glb, &bytes).unwrap();
    let generic = model::import_glb(&glb, 16.0).unwrap();
    let old_terrain = Terrain::bake(&generic, [-8.0, -4.0, 16.0, 8.0], [4, 2]).unwrap();
    let point = glam::Vec2::new(-6.0, -2.0);
    assert_eq!(
        old_terrain.height_at(point),
        Some(12.5),
        "all-opaque baking reproduces the elevated roof bug"
    );
    let mut profile = model::BakeProfile {
        source_sha256: format!("{:x}", Sha256::digest(&bytes)),
        support_meshes: vec!["ground".into(), "roof".into()],
        support_height_ranges: [("roof".into(), [0.0, 8.0])].into(),
        collision_exclude_meshes: vec![],
        minimum_support_up: 0.5,
        collision_error_m: 0.025,
        lock_collision_borders: true,
        lock_collision_extrema: true,
    };
    let source = model::import_glb_with_profile(&glb, 16.0, Some(&profile)).unwrap();
    assert_eq!(
        source.floor, generic.floor,
        "support selection must not remove visible roofs"
    );
    assert_eq!(
        source.support,
        vec![[[-8.0, 0.0, -4.0], [-8.0, 0.0, 4.0], [8.0, 4.0, -4.0]]]
    );
    assert_eq!(source.colors, vec![[0.25, 0.5, 0.75]; 6]);
    assert!(
        source.collision.iter().flatten().any(|p| p[1] >= 12.0),
        "roof remains a physical collider"
    );
    let packed = dir.join("scene.fogscene");
    model::save(&source, &packed).unwrap();
    let loaded = model::load(&packed).unwrap();
    let terrain = Terrain::bake(&loaded, [-8.0, -4.0, 16.0, 8.0], [4, 2]).unwrap();
    assert_eq!(terrain.height_at(point), Some(0.5));
    let binary = std::fs::read(&packed).unwrap();
    assert!(model::save(&source, &packed).is_err());
    assert_eq!(std::fs::read(&packed).unwrap(), binary);
    assert!(model::decode(&binary[..binary.len() - 1]).is_err());
    let mut invalid = binary.clone();
    invalid[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(model::decode(&invalid).is_err());
    let mut legacy = b"FOGSCN02".to_vec();
    legacy.extend(&binary[8..24]);
    legacy.extend(&binary[28..binary.len() - 36]);
    let legacy = model::decode(&legacy).unwrap();
    assert!(legacy.support.is_empty());
    assert_eq!(legacy.floor, source.floor);
    profile.support_height_ranges.clear();
    profile.support_meshes = vec!["missing-ground".into()];
    assert!(
        model::import_glb_with_profile(&glb, 16.0, Some(&profile)).is_err(),
        "stale mesh names must not fall back to all geometry"
    );
    profile.support_meshes = vec!["ground".into()];
    profile.source_sha256 = "bad".into();
    assert!(
        model::import_glb_with_profile(&glb, 16.0, Some(&profile)).is_err(),
        "a profile belongs to one source revision"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

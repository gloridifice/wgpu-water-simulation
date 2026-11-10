//! Asset delivery and world-transform contracts; independent of rendering implementation.
use swe_fog::{model, scenes::ScenePreset, terrain::Terrain};

#[test]
fn bundled_scenes_have_matching_heightfields_and_reduced_static_colliders() {
    for preset in [ScenePreset::Datun, ScenePreset::Grandma] {
        let (source, terrain) = preset.load().unwrap(); // validates the shipped source digest
        assert!(!source.collision.is_empty());
        assert!(
            source.collision.len() < source.floor.len() / 2,
            "collision must not retain visual detail"
        );
        assert_eq!(terrain.metadata.resolution, [256, 256]);
        assert!(terrain.valid.iter().filter(|v| **v == 1).count() > 26_000);
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
}

#[test]
fn glb_node_transform_normalization_and_packed_collision_survive_offline_delivery() {
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
        "asset":{"version":"2.0"}, "scene":0, "scenes":[{"nodes":[0]}],
        "nodes":[{"translation":[10,5,-7],"children":[1]},{"scale":[2,1,1],"mesh":0}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.25,0.5,0.75,1]}}],
        "buffers":[{"byteLength":36}], "bufferViews":[{"buffer":0,"byteLength":36}],
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
    for v in [0.0f32, 0.0, 0.0, 2.0, 1.0, 0.0, 0.0, 0.0, 2.0] {
        bytes.extend(v.to_le_bytes());
    }
    let glb = dir.join("transformed.glb");
    std::fs::write(&glb, bytes).unwrap();
    let source = model::import_glb(&glb, 16.0).unwrap();
    assert_eq!(
        source.floor,
        vec![[[-8.0, 0.0, -4.0], [8.0, 4.0, -4.0], [-8.0, 0.0, 4.0]]]
    );
    assert_eq!(source.colors, vec![[0.25, 0.5, 0.75]; 3]);
    let packed = dir.join("scene.fogscene");
    model::save(&source, &packed).unwrap();
    let loaded = model::load(&packed).unwrap();
    assert_eq!(loaded.collision, source.floor);
    let terrain = Terrain::bake(&loaded, [-8.0, -4.0, 16.0, 8.0], [4, 2]).unwrap();
    assert_eq!(terrain.height_at(glam::Vec2::new(-6.0, -2.0)), Some(0.5));
    let binary = std::fs::read(&packed).unwrap();
    assert!(model::save(&source, &packed).is_err());
    assert_eq!(std::fs::read(&packed).unwrap(), binary);
    assert!(model::decode(&binary[..binary.len() - 1]).is_err());
    let mut invalid = binary.clone();
    invalid[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(model::decode(&invalid).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

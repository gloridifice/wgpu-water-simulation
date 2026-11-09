//! Independent file/geometry/physics/event contracts, not inventories of implementation.
use common::settings::Settings;
use glam::{Vec2, Vec3};
use swe_fog::{
    interactions::{Event, EventQueue, InteractionSettings, PlayerEmitter},
    parameters::Session,
    scene::Player,
    terrain::{BakeSource, Terrain},
};
#[test]
fn static_bake_preserves_rectangular_coordinates_voids_thin_walls_and_source_identity() {
    let mut source = BakeSource {
        floor: vec![
            [[-2.0, 0.0, -1.0], [-2.0, 0.0, 1.0], [0.0, 0.4, 1.0]],
            [[-2.0, 0.0, -1.0], [0.0, 0.4, 1.0], [0.0, 0.4, -1.0]],
        ],
        obstacle: vec![[[-1.0, 0.0, -1.0], [-1.0, 2.0, -1.0], [-1.0, 2.0, 1.0]]],
    };
    let terrain = Terrain::bake(&source, [-2.0, -1.0, 4.0, 2.0], [8, 4]).unwrap();
    assert_eq!(terrain.spacing(), Vec2::splat(0.5));
    assert!((terrain.height_at(Vec2::new(-1.75, -0.75)).unwrap() - 0.05).abs() < 1e-6);
    assert!(
        terrain.height_at(Vec2::new(1.0, 0.0)).is_none(),
        "a missing floor is not zero elevation"
    );
    for z in 0..4 {
        assert_eq!(
            terrain.pass_x[z * 9 + 2],
            0,
            "zero-area projected wall must block its shared face"
        );
    }
    let dir = std::env::temp_dir().join(format!(
        "swe-fog-bake-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("map.fogterrain");
    terrain.save(&path).unwrap();
    assert!(
        terrain.save(&path).is_err(),
        "explicit bake must not overwrite a useful asset"
    );
    let bytes = std::fs::read(&path).unwrap();
    let restored = Terrain::decode(&bytes, &source).unwrap();
    assert_eq!(restored.valid, terrain.valid);
    assert_eq!(restored.heights, terrain.heights);
    assert_eq!(restored.pass_x, terrain.pass_x);
    let mut inconsistent = restored.clone();
    inconsistent.pass_x[6] = 1;
    assert!(
        inconsistent.validate().is_err(),
        "malformed void masks must not silently lose mass"
    );
    assert!(Terrain::decode(&bytes[..bytes.len() - 1], &source).is_err());
    source.floor[0][0][1] += 0.1;
    assert!(
        Terrain::decode(&bytes, &source).is_err(),
        "changed static geometry needs rebaking"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn fog_session_roundtrip_keeps_independent_density_actions_and_legacy_defaults() {
    let mut s: Session = serde_json::from_str(
        r#"{"version":1,"fog":{"base_height":0.35},"interaction":{"move_impulse_interval":0.2}}"#,
    )
    .unwrap();
    s.validate().unwrap();
    assert_eq!(s.fog.base_height, 0.35);
    assert_eq!(s.interaction.move_impulse_interval, 0.2);
    assert_eq!(s.fog.reduced_gravity, 0.02);
    assert_eq!(s.fog.half_life, 300.0);
    s.fog.extinction = 7.0;
    s.fog.brightness = 0.6;
    s.interaction.landing_radius = 2.5;
    s.player.feet = [2.0, 0.0, -3.0];
    s.camera = [0.5, 0.7, 4.0];
    s.paused = true;
    s.interactions_enabled = false;
    let restored: Session = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(restored, s);
    s.fog.gravity_enabled = false;
    s.fog.reduced_gravity = f32::NAN;
    assert!(
        s.validate().is_err(),
        "hidden parameters are still validated"
    );
}
#[test]
fn kinematic_walk_jump_and_normal_gravity_emit_one_landing_and_respect_boundaries() {
    let t = Terrain::bake(
        &BakeSource::playground(),
        [-8.0, -6.0, 16.0, 12.0],
        [32, 24],
    )
    .unwrap();
    let mut p = Player::default();
    p.place(&t).unwrap();
    let mut diagonal = Player::default();
    diagonal.place(&t).unwrap();
    for _ in 0..60 {
        p.step(Vec2::Y, 0.0, false, 1.0 / 60.0, &t);
        diagonal.step(Vec2::ONE, 0.0, false, 1.0 / 60.0, &t);
    }
    assert!((Vec3::from_array(p.feet).length() - 3.0).abs() < 1e-4);
    assert!((Vec3::from_array(diagonal.feet).length() - 3.0).abs() < 1e-4);
    let motion = p.step(Vec2::ZERO, 0.0, true, 1.0 / 60.0, &t);
    assert!(motion.jumped.is_some());
    assert!(p.feet[1] > 0.0);
    assert!(
        p.step(Vec2::ZERO, 0.0, true, 1.0 / 60.0, &t)
            .jumped
            .is_none(),
        "no airborne second jump"
    );
    let mut landings = 0;
    for _ in 0..90 {
        if p.step(Vec2::ZERO, 0.0, false, 1.0 / 60.0, &t)
            .landed
            .is_some()
        {
            landings += 1;
        }
    }
    assert_eq!(landings, 1);
    assert_eq!(p.feet[1], 0.0);
    for _ in 0..300 {
        p.step(Vec2::Y, 0.0, false, 1.0 / 60.0, &t);
    }
    assert!(p.feet[2] >= -5.65 - 1e-5, "capsule stays within terrain");
}
#[test]
fn static_camera_sphere_arm_clips_a_wall_without_moving_the_follow_target() {
    let mut source = BakeSource::playground();
    source.obstacle = vec![
        [[-3.0, 0.0, 2.0], [3.0, 0.0, 2.0], [3.0, 4.0, 2.0]],
        [[-3.0, 0.0, 2.0], [3.0, 4.0, 2.0], [-3.0, 4.0, 2.0]],
    ];
    let player = Player::default();
    let desired = player.orbit([0.0, 0.3, 5.5]);
    let clipped = swe_fog::scene::camera_collision::orbit(&player, [0.0, 0.3, 5.5], &source);
    assert_eq!(clipped.target, desired.target);
    assert!(clipped.distance < desired.distance);
    assert!(
        clipped.eye().z < 1.85,
        "camera sphere must stop before the wall"
    );
    let free =
        swe_fog::scene::camera_collision::orbit(&player, [std::f32::consts::PI, 0.3, 5.5], &source);
    assert_eq!(free.distance, 5.5);
}
#[test]
fn action_emitter_uses_fractional_physics_cadence_and_never_banks_paused_actions() {
    let settings = InteractionSettings::default();
    let mut emitter = PlayerEmitter::default();
    let mut q = EventQueue::default();
    emitter.movement(
        &settings,
        Vec3::ZERO,
        Vec2::new(0.0, -3.0),
        3.0,
        1.0 / 60.0,
        &mut q,
    );
    assert_eq!(q.take().len(), 1, "immediate start tap");
    for _ in 0..6 {
        emitter.movement(
            &settings,
            Vec3::ZERO,
            Vec2::new(0.0, -3.0),
            3.0,
            1.0 / 60.0,
            &mut q,
        );
    }
    assert!(
        q.take().is_empty(),
        "no continuously applied drag between taps"
    );
    emitter.movement(
        &settings,
        Vec3::ZERO,
        Vec2::new(0.0, -3.0),
        3.0,
        0.02,
        &mut q,
    );
    assert_eq!(q.take().len(), 1);
    emitter.clear();
    q.clear();
    emitter.movement(&settings, Vec3::ZERO, Vec2::ZERO, 3.0, 10.0, &mut q);
    assert!(q.take().is_empty());
    emitter.movement(
        &settings,
        Vec3::ZERO,
        Vec2::new(0.0, -3.0),
        3.0,
        10.0,
        &mut q,
    );
    assert_eq!(q.take().len(), 1, "resume starts now, not a backlog");
    let event = Event::radial(Vec3::ZERO, 1.0, 2.0, &settings);
    for _ in 0..32 {
        assert!(q.push(event));
    }
    assert!(!q.push(event));
    let mut continuous = event;
    continuous.metadata[1] = 1;
    for _ in 0..100 {
        q.set_continuous(1, Some(continuous));
    }
    assert_eq!(
        q.take().len(),
        33,
        "same continuous owner replaces rather than appends"
    );
    assert_eq!(q.take().len(), 1, "impulses are consumed once");
    q.clear();
    assert!(q.take().is_empty());
}

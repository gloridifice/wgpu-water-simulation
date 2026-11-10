//! Independent file/geometry/physics/event contracts, not inventories of implementation.
use common::settings::Settings;
use glam::{Vec2, Vec3};
use swe_fog::{
    interactions::{Event, EventQueue, InteractionSettings, MovementMode, PlayerEmitter},
    parameters::{Reconstruction, Session},
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
    for resolution in [[16, 8], [4, 2]] {
        let regridded = restored.at_resolution(&source, Some(resolution)).unwrap();
        assert_eq!(regridded.metadata.domain, [-2.0, -1.0, 4.0, 2.0]);
        assert_eq!(regridded.metadata.resolution, resolution);
        assert_eq!(
            regridded.metadata.source_sha256,
            restored.metadata.source_sha256
        );
        assert!(
            regridded.height_at(Vec2::new(1.0, 0.0)).is_none(),
            "changing resolution must preserve void floor coverage"
        );
        let wall_face = resolution[0] as usize / 4;
        for z in 0..resolution[1] as usize {
            assert_eq!(
                regridded.pass_x[z * (resolution[0] as usize + 1) + wall_face],
                0,
                "rebuilding must retain a zero-width wall"
            );
        }
    }
    let centred_wall = restored.at_resolution(&source, Some([6, 4])).unwrap();
    for z in 1..4 {
        assert_eq!(
            centred_wall.pass_z[z * 6 + 1],
            1,
            "a thin wall through cell centres must not block tangential transport"
        );
    }
    for z in 0..4 {
        for face in [1, 2] {
            assert_eq!(
                centred_wall.pass_x[z * 7 + face],
                0,
                "normal connections to the thin wall remain blocked"
            );
        }
    }
    let cropped = restored.at_region(&source, None, 0.75).unwrap();
    assert_eq!(cropped.metadata.domain, [-1.5, -0.75, 3.0, 1.5]);
    assert_eq!(cropped.metadata.resolution, [8, 4]);
    assert_eq!(cropped.spacing(), Vec2::splat(0.375));
    assert!(
        (cropped.height_at(Vec2::new(-1.3125, -0.5625)).unwrap() - 0.1375).abs() < 1e-6,
        "cropping must query the original world slope, not scale or resample the old heights"
    );
    assert!(cropped.height_at(Vec2::new(0.5, 0.0)).is_none());
    for z in 0..4 {
        assert_eq!(
            cropped.pass_x[z * 9 + 1],
            0,
            "rebaking the smaller domain retains the world-space thin wall"
        );
    }
    assert_eq!(
        restored.at_region(&source, None, 1.0).unwrap().heights,
        terrain.heights
    );
    assert!(restored.at_region(&source, None, f32::NAN).is_err());
    let mut inconsistent = restored.clone();
    inconsistent.pass_x[6] = 1;
    assert!(
        inconsistent.validate().is_err(),
        "malformed void masks must not silently lose mass"
    );
    assert!(Terrain::decode(&bytes[..bytes.len() - 1], &source).is_err());
    source.floor[0][0][1] += 0.1;
    assert!(
        restored.at_resolution(&source, Some([16, 8])).is_err(),
        "resolution changes must not conceal stale geometry"
    );
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
    assert_eq!(s.interaction.movement_mode, MovementMode::Wake);
    assert_eq!(s.grid_resolution, None);
    assert_eq!(s.domain_scale, 1.0);
    assert_eq!(s.reconstruction, Reconstruction::Bilinear);
    assert_eq!(s.reconstruction_smoothing, 0.6);
    assert_eq!(s.integration_steps, 2);
    assert_eq!(s.fog.reduced_gravity, 0.02);
    assert_eq!(s.fog.half_life, 300.0);
    s.fog.extinction = 7.0;
    s.fog.brightness = 0.6;
    s.interaction.landing_radius = 2.5;
    s.interaction.wake_radius = 3.0;
    s.interaction.drag_response = 0.4;
    s.interaction.movement_mode = MovementMode::Impulse;
    s.player.feet = [2.0, 0.0, -3.0];
    s.camera = [0.5, 0.7, 4.0];
    s.paused = true;
    s.grid_resolution = Some([512, 256]);
    s.domain_scale = 0.25;
    s.reconstruction = Reconstruction::PreSmoothed;
    s.reconstruction_smoothing = 0.8;
    s.integration_steps = 16;
    s.interactions_enabled = false;
    for mode in [Reconstruction::PreSmoothed, Reconstruction::SmoothStep] {
        s.reconstruction = mode;
        let restored: Session = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        assert_eq!(restored, s);
    }
    for steps in [0, 1, 33, u32::MAX] {
        s.integration_steps = steps;
        assert!(
            s.validate().is_err(),
            "integration work must remain bounded"
        );
    }
    for steps in [2, 8, 32] {
        s.integration_steps = steps;
        s.validate().unwrap();
    }
    for scale in [0.0, 0.0624, 1.01, f32::NAN, f32::INFINITY] {
        s.domain_scale = scale;
        assert!(
            s.validate().is_err(),
            "invalid region scale must not reach baking or GPU allocation"
        );
    }
    s.domain_scale = 0.25;
    s.reconstruction_smoothing = f32::NAN;
    assert!(
        s.validate().is_err(),
        "render smoothing must be finite even outside its mode"
    );
    s.reconstruction_smoothing = 0.8;
    s.grid_resolution = Some([0, 256]);
    assert!(
        s.validate().is_err(),
        "invalid dimensions cannot reach GPU resource creation"
    );
    s.grid_resolution = None;
    s.interaction.wake_response = f32::NAN;
    assert!(
        s.validate().is_err(),
        "inactive wake settings must remain finite"
    );
    s.interaction.wake_response = 0.3;
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
    let settings = InteractionSettings {
        movement_mode: MovementMode::Impulse,
        ..Default::default()
    };
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

#[test]
fn wake_sources_remain_on_the_path_expire_and_release_their_bounded_slots() {
    let mut p = InteractionSettings::default();
    let mut emitter = PlayerEmitter::default();
    let mut q = EventQueue::default();
    let velocity = Vec2::new(0.0, -3.0);
    emitter.movement(&p, Vec3::ZERO, velocity, 3.0, 1.0 / 60.0, &mut q);
    let first = q.take();
    assert_eq!(
        first.len(),
        3,
        "body and two vortices, without a legacy tap"
    );
    assert!(first.iter().all(|e| e.valid() && e.metadata[1] == 1));
    let centres: Vec<_> = first
        .iter()
        .filter(|e| e.metadata[0] >= 3)
        .map(|e| e.position_rotation)
        .collect();
    assert!(
        centres.iter().all(|c| c[2] > 0.0),
        "vortices start behind a player walking toward -Z"
    );
    for tick in 1..=10 {
        emitter.movement(
            &p,
            Vec3::new(0.0, 0.0, -0.05 * tick as f32),
            velocity,
            3.0,
            1.0 / 60.0,
            &mut q,
        );
    }
    let moved = q.take();
    assert_eq!(
        moved.len(),
        3,
        "sub-spacing motion must not emit by frame count"
    );
    for centre in centres {
        assert!(
            moved
                .iter()
                .any(|e| e.metadata[0] >= 3 && e.position_rotation == centre),
            "old centres must not follow the player"
        );
    }
    emitter.movement(
        &p,
        Vec3::new(0.0, 0.0, -0.5),
        Vec2::ZERO,
        3.0,
        1.0 / 60.0,
        &mut q,
    );
    let stopped = q.take();
    assert_eq!(
        stopped.len(),
        2,
        "stopping removes body drag but retains fading tail sources"
    );
    for _ in 0..60 {
        emitter.movement(&p, Vec3::ZERO, Vec2::ZERO, 3.0, 1.0 / 60.0, &mut q);
    }
    assert!(
        q.take().is_empty(),
        "expired sources cannot brake existing fluid motion"
    );
    p.wake_lifetime = 3.0;
    p.wake_spacing = 0.25;
    for tick in 0..60 {
        emitter.movement(
            &p,
            Vec3::new(tick as f32, 0.0, 0.0),
            Vec2::new(10.0, 0.0),
            10.0,
            0.1,
            &mut q,
        );
        assert!(
            q.take().len() <= 31,
            "fast movement is bounded by the source budget"
        );
    }
    let mut external = Event::radial(Vec3::ZERO, 1.0, 2.0, &p);
    external.metadata[1] = 1;
    q.set_continuous(7, Some(external));
    p.move_enabled = false;
    emitter.movement(&p, Vec3::ZERO, velocity, 3.0, 0.1, &mut q);
    let remaining = q.take();
    assert_eq!(
        remaining.len(),
        1,
        "disabling movement removes only player-owned sources"
    );
    assert_eq!(remaining[0].metadata, external.metadata);
    q.clear();
    emitter.clear();
    p.move_enabled = true;
    emitter.movement(&p, Vec3::ZERO, velocity, 3.0, 0.1, &mut q);
    assert_eq!(
        q.take().len(),
        3,
        "resume starts a fresh tail without a backlog"
    );
}

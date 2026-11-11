//! Actual GPU regression for fog on steep heightfields and vertical receivers.
use crate::{
    Error,
    parameters::{RenderMode, Session},
    simulation::Simulation,
    terrain::{BakeSource, Terrain},
};
use common::gpu::Gpu;
use glam::Vec3;
const SIZE: [u32; 2] = [320, 180];
fn image(gpu: &Gpu, source: &BakeSource, session: &Session) -> Result<Vec<u8>, Error> {
    let terrain = Terrain::bake(source, [-4.0, -4.0, 8.0, 8.0], [64, 64])?;
    let mut sim = Simulation::new(gpu, &terrain)?;
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    sim.encode(gpu, &mut encoder, &session.fog, 0.0, true, &[])?;
    gpu.queue.submit([encoder.finish()]);
    let renderer =
        super::Renderer::new(gpu, &sim, &terrain, source, wgpu::TextureFormat::Rgba8Unorm)?;
    renderer.read_image(gpu, session, sim.domain, SIZE)
}
fn plane(slope: f32) -> BakeSource {
    BakeSource {
        floor: vec![
            [
                [-4.0, -4.0 * slope, -4.0],
                [-4.0, -4.0 * slope, 4.0],
                [4.0, 4.0 * slope, 4.0],
            ],
            [
                [-4.0, -4.0 * slope, -4.0],
                [4.0, 4.0 * slope, 4.0],
                [4.0, 4.0 * slope, -4.0],
            ],
        ],
        obstacle: vec![],
        colors: vec![],
        collision: vec![],
        support: vec![],
    }
}
fn opacity_at(p: Vec3, pixels: &[u8], session: &Session, source: &BakeSource) -> u8 {
    let orbit = crate::scene::camera_collision::orbit(&session.player, session.camera, source);
    let (projection, _) = orbit.matrices(
        55.0f32.to_radians(),
        SIZE[0] as f32 / SIZE[1] as f32,
        0.1,
        200.0,
    );
    let clip = projection * p.extend(1.0);
    let ndc = clip.truncate() / clip.w;
    let x = ((ndc.x * 0.5 + 0.5) * SIZE[0] as f32).floor() as u32;
    let y = ((0.5 - ndc.y * 0.5) * SIZE[1] as f32).floor() as u32;
    assert!(
        x < SIZE[0] && y < SIZE[1],
        "verification receiver must be in view"
    );
    pixels[((y * SIZE[0] + x) * 4) as usize]
}
pub(crate) fn run(gpu: &Gpu) -> Result<(), Error> {
    verify_multiple_fields(gpu)?;
    let mut session = Session {
        height_fog_noise: 0.0,
        debug_view: 5,
        ..Default::default()
    };
    session.fog.noise_amplitude = 0.0;
    session.fog.height_offset = 0.0;
    session.fog.base_height = 1.0;
    session.fog.decay_enabled = false;
    session.player.feet = [0.0, 0.0, 3.0];
    session.camera = [0.0, 0.3, 5.5];
    let mut wall = plane(0.0);
    // Keep the wall a visual receiver on a flat heightfield. This isolates
    // the normal gate from the independent steep-cell exclusion below.
    wall.obstacle = vec![
        [[-2.0, 0.0, 0.0], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0]],
        [[-2.0, 0.0, 0.0], [2.0, 2.0, 0.0], [-2.0, 2.0, 0.0]],
    ];
    let pixels = image(gpu, &wall, &session)?;
    let wall_opacity = opacity_at(Vec3::new(1.2, 0.4, 0.0), &pixels, &session, &wall);
    let floor_opacity = opacity_at(Vec3::new(1.2, 0.0, 2.0), &pixels, &session, &wall);
    if wall_opacity > 1 || floor_opacity < 32 {
        return Err(format!(
            "vertical receiver fog regression: wall={wall_opacity}, floor={floor_opacity}"
        )
        .into());
    }
    session.player.feet = [0.0, 8.0, 0.0];
    session.camera = [0.0, 1.0, 10.0];
    for mode in [RenderMode::HeightFog, RenderMode::Tetrahedra] {
        session.render_mode = mode;
        let gentle = image(gpu, &plane(0.25), &session)?;
        let boundary = image(gpu, &plane(1.0), &session)?;
        let steep = image(gpu, &plane(2.0), &session)?;
        let max_opacity = |pixels: &[u8]| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[0])
                .max()
                .unwrap()
        };
        if max_opacity(&gentle) < 32 || max_opacity(&boundary) < 32 || max_opacity(&steep) > 1 {
            return Err(format!(
                "steep terrain fog regression {mode:?}: gentle={}, boundary={}, steep={}",
                max_opacity(&gentle),
                max_opacity(&boundary),
                max_opacity(&steep)
            )
            .into());
        }
    }
    eprintln!(
        "Fog terrain eligibility: PASS (vertical receiver rejected with visible floor control; gentle/steep heightfields in both rendering modes)"
    );
    Ok(())
}

fn verify_multiple_fields(gpu: &Gpu) -> Result<(), Error> {
    let source = plane(0.0);
    let volume = crate::volumes::FogVolume {
        bottom: [0.0, -1.0, 0.0],
        size: [8.0, 2.0, 8.0],
        ..Default::default()
    };
    let terrain = volume.bake(&source, [32, 32])?;
    let mut session = Session {
        debug_view: 5,
        height_fog_noise: 0.0,
        camera: [0.0, 1.0, 10.0],
        ..Default::default()
    };
    session.player.feet = [0.0, 5.0, 0.0];
    session.fog.noise_amplitude = 0.0;
    session.fog.height_offset = 0.0;
    session.fog.base_height = 0.25;
    session.fog.extinction = 0.7;
    let mut renderers = Vec::new();
    for _ in 0..2 {
        let mut sim = Simulation::new(gpu, &terrain)?;
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        sim.encode(gpu, &mut encoder, &session.fog, 0.0, true, &[])?;
        gpu.queue.submit([encoder.finish()]);
        renderers.push(super::Renderer::new(
            gpu,
            &sim,
            &terrain,
            &source,
            wgpu::TextureFormat::Rgba8Unorm,
        )?);
    }
    for mode in [RenderMode::HeightFog, RenderMode::Tetrahedra] {
        session.render_mode = mode;
        let domain = terrain.metadata.domain;
        let single = renderers[0].read_image(gpu, &session, domain, SIZE)?;
        let both = super::Renderer::read_fields(
            &[(&renderers[0], domain), (&renderers[1], domain)],
            gpu,
            &session,
            SIZE,
        )?;
        let reversed = super::Renderer::read_fields(
            &[(&renderers[1], domain), (&renderers[0], domain)],
            gpu,
            &session,
            SIZE,
        )?;
        // Two identical media equal one medium with twice the extinction,
        // including MSAA silhouettes where squaring pixel-average T is invalid.
        let mut doubled = session.clone();
        doubled.fog.extinction *= 2.0;
        let reference = renderers[0].read_image(gpu, &doubled, domain, SIZE)?;
        let mut samples = 0;
        for (((a, b), c), expected) in single
            .as_chunks::<4>()
            .0
            .iter()
            .zip(both.as_chunks::<4>().0)
            .zip(reversed.as_chunks::<4>().0)
            .zip(reference.as_chunks::<4>().0)
        {
            if b[0].abs_diff(c[0]) > 1 {
                return Err("fog volume ordering changed optical composition".into());
            }
            if b[0].abs_diff(expected[0]) > 2 {
                return Err(format!(
                    "multi-field optical sum {mode:?}: both={}, expected={}",
                    b[0], expected[0]
                )
                .into());
            }
            if (20..100).contains(&a[0]) && b[0] > a[0] + 5 {
                samples += 1;
            }
        }
        if samples < 100 {
            return Err("multi-field verification has insufficient fog coverage".into());
        }
    }
    eprintln!(
        "Fog multiple fields: PASS (independent resources, shared depth, per-sample optical sum, both render modes)"
    );
    Ok(())
}

//! Actual GPU bed upload, dry shoreline, conservation and paused regrid lifecycle.
use super::{Gpu, Ocean, OpticsConfig, Shore};
use crate::ocean::Error;
use crate::ocean::parameters::model::ModelConfig;
use crate::ocean::parameters::ocean::OceanConfig;
use crate::ocean::scene::model::Model;
use crate::ocean::scene::model::Vertex;
use crate::ocean::scene::terrain::HeightField;

fn read(gpu: &Gpu, shore: &Shore) -> Result<Vec<[f32; 4]>, Error> {
    Ok(gpu
        .read_buffer(&shore.field, shore.field.size())?
        .as_chunks::<16>()
        .0
        .iter()
        .map(|v| bytemuck::pod_read_unaligned(v))
        .collect())
}
fn advance(gpu: &Gpu, ocean: &Ocean, shore: &mut Shore, optics: &OpticsConfig, time: f32) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    ocean.encode_filtered(gpu, &mut encoder, time, &optics.shore);
    shore.encode(gpu, &mut encoder, optics, time, false, 0.22);
    gpu.queue.submit([encoder.finish()]);
}
fn check_lake(data: &[[f32; 4]], config: ModelConfig, n: usize) -> Result<(), Error> {
    let domain = config.domain();
    let cells = n * n;
    for z in 0..n {
        for x in 0..n {
            let i = z * n + x;
            let q = data[cells + i];
            let world_x = domain[0] + (x as f32 + 0.5) * domain[2] / n as f32;
            let bed = 0.05 * world_x - 0.5;
            if !q.iter().all(|v| v.is_finite())
                || (q[0] - (-bed).max(0.0)).abs() > 0.00005
                || q[1].abs() > 0.00005
                || q[2].abs() > 0.00005
                || q[3].abs() > 0.0001
            {
                return Err(
                    format!("model lake-at-rest mismatch at {x},{z}: {q:?}, bed={bed}").into(),
                );
            }
        }
    }
    Ok(())
}
pub(super) fn verify(gpu: &Gpu) -> Result<(), Error> {
    let mesh = Model {
        vertices: [
            [-24., -1.7, -24.],
            [24., 0.7, -24.],
            [-24., -1.7, 24.],
            [24., 0.7, 24.],
        ]
        .map(|position| Vertex {
            position,
            normal: [0., 1., 0.],
            uv: [0.; 2],
        })
        .to_vec(),
        indices: vec![0, 1, 2, 1, 3, 2],
        image: vec![],
        bounds: [[-24., -1.7, -24.], [24., 0.7, 24.]],
        source_hash: "a".repeat(64),
        removed_triangles: 0,
    };
    let source = HeightField::bake(&mesh, 0.125, 0.0)?;
    let ocean = Ocean::new(
        gpu,
        OceanConfig {
            resolution: 8,
            amplitude: 0.0,
            ..Default::default()
        },
    )?;
    for order in [1, 2] {
        let mut shore = Shore::with_resolution(gpu, &ocean, 64);
        let mut optics = OpticsConfig {
            model: ModelConfig {
                enabled: true,
                position: [0.; 3],
                sea_level: 0.,
                center: [0.; 3],
                size: [32.; 3],
                sponge_width: 2.,
                open_edges: [false; 4],
                ..Default::default()
            },
            ..Default::default()
        };
        optics.shore.order = order;
        optics.shore.resolution = 64;
        shore.prepare_terrain(gpu, &optics, Some(&source))?;
        for step in 0..=40 {
            advance(gpu, &ocean, &mut shore, &optics, step as f32 * 0.01);
        }
        let initial = read(gpu, &shore)?;
        check_lake(&initial, optics.model, 64)?;
        // Seed a water pulse through the production state buffers, then exercise
        // closed-domain mass conservation on the sloping wet/dry bed.
        let cells = 64 * 64;
        let mut seed = initial;
        for z in 28..36 {
            for x in 12..20 {
                let i = z * 64 + x;
                seed[cells + i][0] += 0.05;
                seed[i][2] += 0.05;
                seed[2 * cells + i][2] += 0.05;
            }
        }
        let initial_mass: f64 = seed[cells..2 * cells].iter().map(|q| f64::from(q[0])).sum();
        gpu.queue
            .write_buffer(&shore.previous, 0, bytemuck::cast_slice(&seed));
        gpu.queue
            .write_buffer(&shore.field, 0, bytemuck::cast_slice(&seed));
        for step in 41..=100 {
            advance(gpu, &ocean, &mut shore, &optics, step as f32 * 0.01);
        }
        let moving = read(gpu, &shore)?;
        let mass: f64 = moving[cells..2 * cells]
            .iter()
            .map(|q| f64::from(q[0]))
            .sum();
        let error = ((mass - initial_mass) / initial_mass).abs();
        if error > 0.00005
            || moving.iter().flatten().any(|v| !v.is_finite())
            || moving[cells..2 * cells].iter().any(|q| q[0] < 0.0)
            || !moving[cells..2 * cells].iter().any(|q| q[1].abs() > 0.0001)
        {
            return Err(format!(
                "model pulse invariant failed: order={order}, relative mass error={error}"
            )
            .into());
        }
        optics.model.center[1] = 8.;
        optics.model.size[1] = 48.;
        optics.model.show_bounds = false;
        shore.prepare_terrain(gpu, &optics, Some(&source))?;
        advance(gpu, &ocean, &mut shore, &optics, 1.0);
        if read(gpu, &shore)? != moving {
            return Err("display-only box edit reset model water".into());
        }
        optics.model.center[0] = 2.;
        optics.model.size[0] = 40.;
        optics.model.size[2] = 24.;
        shore.prepare_terrain(gpu, &optics, Some(&source))?;
        advance(gpu, &ocean, &mut shore, &optics, 1.0);
        check_lake(&read(gpu, &shore)?, optics.model, 64)?;
        // Open each edge in isolation; a flat free surface must remain still.
        for edge in 0..4 {
            optics.model.open_edges = [false; 4];
            optics.model.open_edges[edge] = true;
            shore.prepare_terrain(gpu, &optics, Some(&source))?;
            for step in 0..=15 {
                advance(
                    gpu,
                    &ocean,
                    &mut shore,
                    &optics,
                    2.0 + edge as f32 + step as f32 * 0.01,
                );
            }
            check_lake(&read(gpu, &shore)?, optics.model, 64)?;
        }
        eprintln!(
            "Model SWE order {order}: uploaded analytic bed, wet/dry lake, moving pulse mass error {error:.2e}, display-only pause, paused domain regrid, four open edges OK"
        );
    }
    Ok(())
}

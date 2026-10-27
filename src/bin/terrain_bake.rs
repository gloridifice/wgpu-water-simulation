//! Offline, create-new GLB -> versioned float heightfield conversion.
use std::{path::PathBuf, process::ExitCode};
use water_simulation::ocean::{
    gpu::Error,
    model::{self, Model},
    terrain::HeightField,
};
fn run() -> Result<(), Error> {
    let mut source = model::asset_path(model::MODEL_PATH);
    let mut output = PathBuf::from(model::HEIGHT_PATH);
    let mut spacing = 0.05_f32;
    let mut cap_epsilon = 0.002_f32;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        if flag == "--help" || flag == "-h" {
            println!(
                "terrain-bake [--input model.glb] [--output map.sweh] [--cell-size 0.05] [--cap-epsilon 0.002]\nOutputs cell-centered f32 heights + coverage in source scene coordinates.\nRemoves triangles on bottom/side cutting planes within cap-epsilon (model units); 0 disables cleanup.\nOutput must not exist; move the old cache aside before rebaking. No personal settings are read or written."
            );
            return Ok(());
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag.as_str() {
            "--input" => source = value.into(),
            "--output" => output = value.into(),
            "--cell-size" => spacing = value.parse()?,
            "--cap-epsilon" => cap_epsilon = value.parse()?,
            _ => return Err(format!("unknown option {flag}").into()),
        }
    }
    if output.exists() {
        return Err("output already exists; move it aside before baking".into());
    }
    if !cap_epsilon.is_finite() || !(0.0..=1.0).contains(&cap_epsilon) {
        return Err("cap epsilon must be in 0..1 model units".into());
    }
    let mut model = Model::load(&source)?;
    if cap_epsilon > 0.0 {
        model.remove_scan_caps(cap_epsilon);
    }
    eprintln!(
        "{}: {} vertices, {} triangles, {} cap triangles removed",
        source.display(),
        model.vertices.len(),
        model.indices.len() / 3,
        model.removed_triangles
    );
    let map = HeightField::bake(&model, spacing, cap_epsilon)?;
    map.save(&output)?;
    let valid = map.samples.iter().filter(|p| p[1] > 0.0).count();
    eprintln!(
        "{}: {:?} cells, {:.2}% scan coverage, bounds {:?}, source SHA256 {}",
        output.display(),
        map.metadata.resolution,
        100.0 * valid as f64 / map.samples.len() as f64,
        map.metadata.bounds,
        map.metadata.source_sha256
    );
    Ok(())
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("terrain-bake: {e}");
            ExitCode::FAILURE
        }
    }
}

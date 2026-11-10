use common::settings::{Settings, SettingsStore};
use std::{path::PathBuf, process::ExitCode};
use swe_fog::{Error, parameters::Session, terrain::Terrain};
const HELP: &str = "SWE Fog — independent Rust / wgpu playground

cargo run --release -p swe-fog -- [options]
  --verify               Run real GPU solver and thin-layer rendering checks
  --bake PATH            Explicit offline bake; output must not exist
  --scene NAME|PATH      datun (default), grandma, playground, or JSON/.fogscene
  --terrain PATH         Baked fog terrain; default is the selected bundled asset
  --import-glb PATH      Offline model conversion (opaque primitives only)
  --bake-scene PATH      Packed visual mesh + simplified collider; must not exist
  --model-span M         Normalize longest XZ extent to M metres (default 64)
  --domain X,Z,W,D       Bake bounds in metres (default -32,-32,64,64)
  --resolution N or NxM  Grid resolution, each 2..1024 (bake default 256)
  --config PATH          Strict read-only session; never saves parameters/layout
  --settings PATH        Writable session; only explicit Save / Ctrl+S writes
  --no-settings          Do not load or save personal session/layout
  --frames N             Exit after N presented frames; use for smoke checks
  --snapshot PATH        Offscreen PNG at --time; never writes personal settings
  --dump PATH            Export actual GPU clock and h,m,ux,uz to JSON
  --time S               Offscreen simulated time, 0..120 seconds (default 3)
  -h, --help             Show help

Click viewport: capture mouse | Esc: release | WASD: walk | Space: jump
Captured mouse: orbit | Q: quit | Shift: fine number drag | Double click: type
Numeric edits reset fog, except render smoothing/noise. Scene changes reset fog/player.
";
fn next(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, Error> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}").into())
}
fn run() -> Result<(), Error> {
    let mut args = std::env::args().skip(1);
    let mut verify = false;
    let mut bake = None;
    let mut import_glb = None;
    let mut bake_scene = None;
    let mut model_span = 64.0;
    let mut scene = None;
    let mut terrain_path = None;
    let mut config = None;
    let mut settings = None;
    let mut no_settings = false;
    let mut frames = None;
    let mut snapshot = None;
    let mut dump = None;
    let mut time = 3.0f32;
    let mut domain = [-32.0, -32.0, 64.0, 64.0];
    let mut resolution = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--help" | "-h" => {
                print!("{HELP}");
                return Ok(());
            }
            "--verify" => verify = true,
            "--bake" => bake = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--import-glb" => import_glb = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--bake-scene" => bake_scene = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--model-span" => model_span = next(&mut args, &flag)?.parse::<f32>()?,
            "--scene" => scene = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--terrain" => terrain_path = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--config" => config = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--settings" => settings = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--no-settings" => no_settings = true,
            "--frames" => frames = Some(next(&mut args, &flag)?.parse::<u32>()?),
            "--snapshot" => snapshot = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--dump" => dump = Some(PathBuf::from(next(&mut args, &flag)?)),
            "--time" => time = next(&mut args, &flag)?.parse()?,
            "--domain" => {
                let values = next(&mut args, &flag)?
                    .split(',')
                    .map(str::parse::<f32>)
                    .collect::<Result<Vec<_>, _>>()?;
                domain = values.try_into().map_err(|_| "--domain requires X,Z,W,D")?;
            }
            "--resolution" => {
                let text = next(&mut args, &flag)?;
                let values = text
                    .split('x')
                    .map(str::parse::<u32>)
                    .collect::<Result<Vec<_>, _>>()?;
                resolution = Some(match values.as_slice() {
                    [n] => [*n, *n],
                    [x, z] => [*x, *z],
                    _ => return Err("resolution must be N or NxM".into()),
                });
            }
            _ => return Err(format!("unknown option {flag}; see --help").into()),
        }
    }
    if verify {
        return pollster::block_on(swe_fog::verify::run());
    }
    if !time.is_finite() || !(0.0..=120.0).contains(&time) || frames == Some(0) {
        return Err("time must be 0..120 s; frames must be positive".into());
    }
    if config.is_some() && settings.is_some() {
        return Err("--config and --settings are mutually exclusive".into());
    }
    if bake.as_ref().is_some_and(|path| path.exists()) {
        return Err("bake output must not exist".into());
    }
    if let Some(input) = import_glb {
        if scene.is_some() {
            return Err("--import-glb and --scene are mutually exclusive".into());
        }
        let output = bake_scene.ok_or("--import-glb requires --bake-scene PATH")?;
        if output.exists() || bake.as_ref().is_some_and(|p| p.exists()) {
            return Err("bake outputs must not exist".into());
        }
        let source = swe_fog::model::import_glb(&input, model_span)?;
        swe_fog::model::save(&source, &output)?;
        if let Some(path) = bake {
            let terrain = Terrain::bake(&source, domain, resolution.unwrap_or([256, 256]))?;
            terrain.save(&path)?;
            eprintln!(
                "Heightfield: {} / {} valid cells, {} overlaps; {:?}",
                terrain.valid.iter().filter(|v| **v == 1).count(),
                terrain.valid.len(),
                terrain.metadata.overlaps,
                terrain.metadata.domain
            );
        }
        return Ok(());
    }
    if bake_scene.is_some() {
        return Err("--bake-scene requires --import-glb".into());
    }
    let offscreen = snapshot.is_some() || dump.is_some();
    if settings.is_some() && (offscreen || no_settings || bake.is_some()) {
        return Err(
            "--settings is for writable windows; use --config for read-only exports".into(),
        );
    }
    let (mut session, store) = if let Some(path) = config {
        (common::settings::load_config::<Session>(&path)?, None)
    } else if offscreen || no_settings || bake.is_some() {
        (Session::default(), None)
    } else {
        let (session, store) = SettingsStore::<Session>::open(settings.unwrap_or_else(|| {
            common::settings::default_path("water-simulation", "fog-settings.json")
        }));
        (session, Some(store))
    };
    use swe_fog::scenes::ScenePreset;
    let mut external = None;
    if let Some(path) = scene {
        if let Some(preset) = path.to_str().and_then(ScenePreset::parse_name) {
            session.scene = preset;
        } else {
            session.scene = ScenePreset::Custom;
            external = Some(swe_fog::model::load(&path)?);
        }
    }
    let (source, bundled_terrain) = if let Some(source) = external {
        (source, None)
    } else {
        let (source, terrain) = session.scene.load()?;
        (source, Some(terrain))
    };
    source.validate()?;
    if let Some(path) = bake {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let t = Terrain::bake(&source, domain, resolution.unwrap_or([256, 256]))?;
        t.save(&path)?;
        eprintln!(
            "Baked {} ({:?}, {} overlaps, SHA256 {})",
            path.display(),
            t.metadata.resolution,
            t.metadata.overlaps,
            t.metadata.source_sha256
        );
        return Ok(());
    }
    let terrain = if let Some(path) = terrain_path {
        Terrain::load(&path, &source)?
    } else {
        bundled_terrain.ok_or("external scenes require --terrain PATH (bake explicitly first)")?
    };
    if let Some(resolution) = resolution {
        session.grid_resolution = Some(resolution);
    }
    session.validate()?;
    if let Some(path) = snapshot {
        pollster::block_on(swe_fog::app::snapshot(
            &session, &terrain, &source, time, &path,
        ))?;
    }
    if let Some(path) = dump {
        pollster::block_on(swe_fog::app::dump(&session, &terrain, &source, time, &path))?;
    }
    if offscreen {
        return Ok(());
    }
    swe_fog::app::run(session, terrain, source, store, frames)
}
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

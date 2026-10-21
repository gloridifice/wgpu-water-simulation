use std::{env, path::PathBuf, process::ExitCode};
use water_simulation::ocean::{
    gpu::Error,
    optics::OpticsConfig,
    settings::{self, SavedSettings, SettingsStore},
    spectrum::OceanConfig,
    verify, viewer,
};

const HELP: &str = "Minimal 3D GPU FFT ocean (wgpu)

Usage: cargo run --release --features ocean --bin ocean -- [options]
  --resolution N   Power of two, 8..512 (default: 128)
  --length M       Periodic patch size in meters (default: 128)
  --wind M/S       Wind speed (default: 14)
  --amplitude A    Phillips power multiplier (default: 0.002)
  --phillips-length M  Phillips L, independent of patch size
  --wind-angle DEG Wind direction in degrees
  --mesh-resolution N  Mesh cells per axis, 8..512 (default: 128)
  --seed N         Deterministic ocean seed (default: 42)
  --verify         Headless GPU IFFT vs independent direct DFT; then exit
  --snapshot PATH  Render an offscreen PNG; file must not exist
  --beach         Sloped sand beach preset, shallow-to-deep water and overview camera
  --shore-view N  0=shaded, 1=water depth, 2=flow, 3=shore foam, 4=sand wetness, 5=stranded foam
  --no-shore      Disable shore waves and wet sand history
  --seabed-depth M Seabed depth below mean water level (default: 6)
  --sigma-t R,G,B  Linear RGB extinction per meter (default: 0.18,0.06,0.025)
  --source R,G,B   Linear RGB in-scattering source per meter
  --sky-color R,G,B  Linear RGB sky tint, each 0..1
  --sky-intensity X  Sky radiance strength, 0..20
  --scattering-albedo R,G,B  sigma_s / sigma_t, each 0..1
  --sky-steps N    Piecewise analytic sky integration: 8, 16, 32
  --no-skylight    Disable the sky source (keep background/reflection)
  --skylight       Enable the sky source
  --water-view N   0=shaded, 1=path, 2=T, 3=attenuated bottom, 4=in-scattering,
                   5=extinction loss, 6=raw bottom, 7=medium sum,
                   8=local sky source, 9=integrated sky, 10=manual, 11=total local source
  --foam-view N    Snapshot: 0=water, 1=Jacobian, 2=foam coverage
  --time S         Snapshot time, >=0 (default: 3)
  --settings PATH  Local GUI settings file (default: LOCALAPPDATA/water-simulation/ocean-settings.json)
  --no-settings    Disable loading/saving local GUI settings
  --frames N       Close window after N presented frames (smoke test)
  -h, --help       Show help

Drag left mouse: orbit | Wheel: zoom | Space: pause | R: reset
C: horizontal choppiness | F: foam | Up/Down: amplitude | Esc/Q: quit

The FFT and spectral evolution run on the GPU. This is a periodic ocean
surface. The beach preset adds local 2D shallow-water waves, transported foam and
wet sand and stranded foam. FFT elevation and momentum drive the inlet; waves cannot overturn.
";

fn value<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<T, Error> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))?
        .parse()
        .map_err(|_| format!("invalid value for {flag}").into())
}

fn rgb(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<[f32; 3], Error> {
    let text: String = value(args, flag)?;
    let values: Vec<f32> = text.split(',').map(str::parse).collect::<Result<_, _>>()?;
    values
        .try_into()
        .map_err(|_| format!("{flag} requires R,G,B").into())
}

fn run() -> Result<(), Error> {
    let mut config = OceanConfig::default();
    let mut optics = OpticsConfig::default();
    let mut verify = false;
    let mut path: Option<PathBuf> = None;
    let mut time = 3.0_f32;
    let mut foam_view = 0u32;
    let mut frames: Option<u32> = None;
    let mut settings_path: Option<PathBuf> = None;
    let mut no_settings = false;
    let mut explicit = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        explicit.push(arg.clone());
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "--resolution" => config.resolution = value(&mut args, &arg)?,
            "--length" => config.length = value(&mut args, &arg)?,
            "--wind" => {
                let speed: f32 = value(&mut args, &arg)?;
                if !speed.is_finite() || speed <= 0.0 {
                    return Err("wind speed must be positive".into());
                }
                config.phillips_length = speed * speed / 9.81;
            }
            "--amplitude" => config.amplitude = value(&mut args, &arg)?,
            "--phillips-length" => config.phillips_length = value(&mut args, &arg)?,
            "--wind-angle" => config.wind_angle = value::<f32>(&mut args, &arg)?.to_radians(),
            "--mesh-resolution" => config.mesh_resolution = value(&mut args, &arg)?,
            "--seed" => config.seed = value(&mut args, &arg)?,
            "--beach" => optics = OpticsConfig::beach_scene(),
            "--shore-view" => optics.shore.view = value(&mut args, &arg)?,
            "--shore-amplitude" | "--shore-period" => return Err("Independent shore waves were removed; use FFT --amplitude / --wind and Height gain".into()),
            "--no-shore" => optics.shore.enabled = false,
            "--seabed-depth" => optics.depth = value(&mut args, &arg)?,
            "--sigma-t" => optics.sigma_t = rgb(&mut args, &arg)?,
            "--source" => optics.source = rgb(&mut args, &arg)?,
            "--sky-color" => optics.sky.color = rgb(&mut args, &arg)?,
            "--sky-intensity" => optics.sky.intensity = value(&mut args, &arg)?,
            "--scattering-albedo" => optics.sky.albedo = rgb(&mut args, &arg)?,
            "--sky-steps" => optics.sky.steps = value(&mut args, &arg)?,
            "--no-skylight" => optics.sky.enabled = false,
            "--skylight" => optics.sky.enabled = true,
            "--water-view" => optics.view = value(&mut args, &arg)?,
            "--verify" => verify = true,
            "--snapshot" => path = Some(value(&mut args, &arg)?),
            "--foam-view" => foam_view = value(&mut args, &arg)?,
            "--time" => time = value(&mut args, &arg)?,
            "--frames" => frames = Some(value(&mut args, &arg)?),
            "--settings" => settings_path = Some(value(&mut args, &arg)?),
            "--no-settings" => no_settings = true,
            _ => return Err(format!("unknown option: {arg}; use --help").into()),
        }
    }
    config.validate()?;
    optics.validate()?;
    if foam_view > 2 || (foam_view != 0 && path.is_none()) {
        return Err("--foam-view requires --snapshot and a value in 0..=2".into());
    }
    if !time.is_finite() || time < 0.0 {
        return Err("--time must be finite and nonnegative".into());
    }
    if frames == Some(0) {
        return Err("--frames must be positive".into());
    }
    if (verify && path.is_some()) || (frames.is_some() && (verify || path.is_some())) {
        return Err("--verify, --snapshot and --frames select different run modes".into());
    }
    if verify {
        pollster::block_on(verify::run())
    } else if let Some(path) = path {
        pollster::block_on(viewer::snapshot(config, time, &path, foam_view, optics))
    } else {
        let (mut initial, store) = if no_settings {
            (SavedSettings::default(), None)
        } else {
            let (initial, store) = SettingsStore::open(
                settings_path.unwrap_or_else(settings::default_path),
                std::time::Instant::now(),
            );
            (initial, Some(store))
        };
        initial.apply_cli(config, optics, &explicit);
        viewer::run(initial, frames, store)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ocean: {error}");
            ExitCode::FAILURE
        }
    }
}

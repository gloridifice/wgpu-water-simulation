mod batch;
mod pic_batch;
mod shallow_tui;
mod tui;

use std::{
    env,
    io::{self, IsTerminal},
    process::ExitCode,
};
use tui_2d_prototype::pic;
use tui_2d_prototype::solver::Config;

const HELP: &str = "Two-dimensional CPU PIC water simulator (side view)

Usage: cargo run -p tui-2d-prototype -- [options]
  --tui           Force interactive TUI (requires a terminal)
  --paused        Start TUI paused, ready for single stepping
  --headless      ASCII / CSV batch output for the selected solver
  --pic           2D PIC mode (default)
  --shallow       Original 1D shallow-water mode
  --nx N          PIC horizontal resolution (default: 112)
  --ny N          PIC vertical resolution (default: 64)
  --cell-size H   PIC cell size in meters (default: 0.1)
  --pressure-iterations N   PIC pressure solve limit (default: 100)
  --no-waves      Start with the moving paddle disabled
  --no-pulse      Omit the initial breaking-wave momentum pulse
  --cells N       Select 1D shallow mode; cell count (default: 201; try 41)
  --cfl C         CFL in (0, 0.5] (PIC default: 0.4; shallow default: 0.45)
  --time S        Batch duration in seconds (default: 6)
  --frames N      Batch output intervals (default: 12; also emits t=0)
  --csv PATH      Batch snapshots t,cell,x,h,hu,u; file must not exist
  --animate       Batch ANSI animation with 50 ms display delay
  --quiet         Batch final statistics only (CSV still includes all frames)
  -h, --help      Show this help

Default: TUI in a terminal; batch when input/output is redirected.
Batch options also select batch mode. --tui/--paused cannot use batch options.

TUI keys:
  Space: pause/resume   N: one CFL step   R: reset and pause
  +/-: playback speed  G: grid overlay   Left/Right: select particle
  W: paddle on/off     B: breaking wave pulse
  Home/End: first/last  C: center        Q/Esc/Ctrl+C: quit
  Mouse: left click in the tank to push water upward/forward
  Shallow mode: G switches 41/201 cells; mouse adds a surface ripple

Examples:
  cargo run -p tui-2d-prototype
  cargo run --release -p tui-2d-prototype -- --paused
  cargo run -p tui-2d-prototype -- --shallow --paused --cells 41
  cargo run -p tui-2d-prototype -- --headless --quiet --csv snapshots.csv
";

struct Options {
    config: Config,
    duration: f64,
    frames: usize,
    csv: Option<String>,
    animate: bool,
    quiet: bool,
    interactive: bool,
    paused: bool,
    shallow: bool,
    pic_config: pic::Config,
    waves: bool,
    pulse: bool,
}

impl Options {
    fn parse() -> Result<Option<Self>, String> {
        let mut options = Self {
            config: Config::default(),
            duration: 6.0,
            frames: 12,
            csv: None,
            animate: false,
            quiet: false,
            interactive: false,
            paused: false,
            shallow: false,
            pic_config: pic::Config::default(),
            waves: true,
            pulse: true,
        };
        let mut request_tui = false;
        let mut request_batch = false;
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => return Ok(None),
                "--tui" => request_tui = true,
                "--paused" => {
                    options.paused = true;
                    request_tui = true;
                }
                "--headless" => request_batch = true,
                "--pic" => options.shallow = false,
                "--shallow" => options.shallow = true,
                "--no-waves" => options.waves = false,
                "--no-pulse" => options.pulse = false,
                "--nx" => options.pic_config.nx = value(&mut args, &arg)?,
                "--ny" => options.pic_config.ny = value(&mut args, &arg)?,
                "--cell-size" => options.pic_config.cell_size = value(&mut args, &arg)?,
                "--pressure-iterations" => {
                    options.pic_config.pressure_iterations = value(&mut args, &arg)?
                }
                "--cells" => {
                    options.config.cells = value(&mut args, &arg)?;
                    options.shallow = true;
                }
                "--cfl" => {
                    let cfl = value(&mut args, &arg)?;
                    options.config.cfl = cfl;
                    options.pic_config.cfl = cfl;
                }
                "--time" => {
                    options.duration = value(&mut args, &arg)?;
                    request_batch = true;
                }
                "--frames" => {
                    options.frames = value(&mut args, &arg)?;
                    request_batch = true;
                }
                "--csv" => {
                    options.csv = Some(value(&mut args, &arg)?);
                    request_batch = true;
                }
                "--animate" => {
                    options.animate = true;
                    request_batch = true;
                }
                "--quiet" => {
                    options.quiet = true;
                    request_batch = true;
                }
                _ => return Err(format!("unknown option: {arg}; use --help")),
            }
        }
        if request_tui && request_batch {
            return Err("--tui/--paused cannot be combined with batch options".into());
        }
        if !options.duration.is_finite() || options.duration <= 0.0 {
            return Err("--time must be finite and positive".into());
        }
        if options.frames == 0 {
            return Err("--frames must be positive".into());
        }
        if options.quiet && options.animate {
            return Err("--quiet and --animate cannot be used together".into());
        }
        let has_terminal = io::stdin().is_terminal() && io::stdout().is_terminal();
        if request_tui && !has_terminal {
            return Err("TUI requires a terminal; use --headless for redirected output".into());
        }
        options.interactive = request_tui || (has_terminal && !request_batch);
        Ok(Some(options))
    }
}

fn value<T: std::str::FromStr>(
    args: &mut impl Iterator<Item = String>,
    flag: &str,
) -> Result<T, String> {
    args.next()
        .ok_or_else(|| format!("missing value for {flag}"))?
        .parse()
        .map_err(|_| format!("invalid value for {flag}"))
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Some(options) = Options::parse()? else {
        print!("{HELP}");
        return Ok(());
    };
    if options.interactive {
        if options.shallow {
            shallow_tui::run(options.config, options.paused)
        } else {
            tui::run(
                options.pic_config,
                options.paused,
                options.waves,
                options.pulse,
            )
        }
    } else if options.shallow {
        batch::run(options)
    } else {
        pic_batch::run(options)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

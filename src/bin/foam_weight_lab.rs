//! Independent interactive diagnostic. No personal settings are read or saved.
use water_simulation::ocean::{foam_weight_lab, gpu::Error};
fn run() -> Result<(), Error> {
    let mut frames = None;
    let mut time = 0.0;
    let mut paused = false;
    let mut verify = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "foam-weight-lab: isolated FFT Jacobian / foam source-weight viewer\n  --paused       Start paused\n  --time SECONDS Initial simulation time, 0..3600\n  --frames N     Exit after N rendered frames\n  --verify       Run headless GPU verification instead of opening a window\nNo SWE evolution or personal settings. Space: pause; Esc: exit."
                );
                return Ok(());
            }
            "--paused" => paused = true,
            "--verify" => verify = true,
            "--frames" => frames = Some(args.next().ok_or("--frames needs an integer")?.parse()?),
            "--time" => time = args.next().ok_or("--time needs seconds")?.parse()?,
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    if verify {
        foam_weight_lab::verify()
    } else {
        foam_weight_lab::run(frames, time, paused)
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("foam-weight-lab: {error}");
        std::process::exit(1);
    }
}

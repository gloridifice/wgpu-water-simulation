//! Independent UI prototype. Never loads or saves ocean settings.
use water_simulation::ocean::{Error, labs::slider_lab};

fn run() -> Result<(), Error> {
    let mut frames = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "slider-lab: interactive rectangular slider appearance demo\n  --frames N  Exit after N rendered frames (positive integer)\nNo ocean simulation or settings. Esc: exit."
                );
                return Ok(());
            }
            "--frames" => frames = Some(args.next().ok_or("--frames needs an integer")?.parse()?),
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    slider_lab::run(frames)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("slider-lab: {error}");
        std::process::exit(1);
    }
}

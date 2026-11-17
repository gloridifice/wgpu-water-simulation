//! Terminal-independent PIC example: cargo run --release -p tui-2d-prototype --example pic_probe -- 8 [frames.csv]
use std::{
    fs::File,
    io::{BufWriter, Write},
};
use tui_2d_prototype::pic::{Config, Simulation};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let duration = args
        .first()
        .map(|s| s.parse::<f64>())
        .transpose()?
        .unwrap_or(8.0);
    let mut csv = args
        .get(1)
        .map(|p| File::create_new(p).map(BufWriter::new))
        .transpose()?;
    if let Some(file) = &mut csv {
        writeln!(file, "t,id,x,y,u,v")?;
    }
    let mut sim = Simulation::new(Config::default())?;
    if args.get(2).is_some_and(|arg| arg == "pulse") {
        sim.wave_pulse();
    }
    let start = std::time::Instant::now();
    for frame in 0..=40 {
        sim.advance_to(duration * frame as f64 / 40.0)?;
        let d = sim.diagnostics();
        println!(
            "t={:.2} steps={} particles={} speed={:.3} div={:.3e}->{:.3e} cg={} overhangs={} wall={:.2} elapsed={:.2}",
            sim.time(),
            sim.steps(),
            sim.particles().len(),
            d.max_speed,
            d.divergence_before,
            d.divergence_after,
            d.pressure_iterations,
            d.overhang_columns,
            sim.paddle_x(),
            start.elapsed().as_secs_f64()
        );
        if let Some(file) = &mut csv {
            for (i, p) in sim.particles().iter().enumerate() {
                writeln!(file, "{},{},{},{},{},{}", sim.time(), i, p.x, p.y, p.u, p.v)?;
            }
        }
    }
    Ok(())
}

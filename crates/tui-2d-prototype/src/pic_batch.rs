use crate::Options;
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    thread,
    time::Duration,
};
use tui_2d_prototype::pic::Simulation;

pub fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let mut sim = Simulation::new(options.pic_config)?;
    sim.set_wavemaker(options.waves);
    if options.pulse {
        sim.wave_pulse();
    }
    let mut csv = options
        .csv
        .as_ref()
        .map(|path| File::create_new(path).map(BufWriter::new))
        .transpose()?;
    if let Some(file) = &mut csv {
        writeln!(file, "t,id,x,y,u,v")?;
    }
    let mut out = BufWriter::new(io::stdout().lock());
    for frame in 0..=options.frames {
        sim.advance_to(options.duration * (frame as f64 / options.frames as f64))?;
        if let Some(file) = &mut csv {
            for (i, p) in sim.particles().iter().enumerate() {
                writeln!(
                    file,
                    "{:.12},{},{:.12},{:.12},{:.12},{:.12}",
                    sim.time(),
                    i,
                    p.x,
                    p.y,
                    p.u,
                    p.v
                )?;
            }
        }
        if !options.quiet || frame == options.frames {
            if options.animate {
                write!(out, "\x1b[2J\x1b[H")?;
            }
            let d = sim.diagnostics();
            writeln!(
                out,
                "PIC t={:.3}s steps={} particles={} volume={:.6} max_speed={:.3} divergence={:.3e}->{:.3e} overhangs={}",
                sim.time(),
                sim.steps(),
                sim.particles().len(),
                sim.volume(),
                d.max_speed,
                d.divergence_before,
                d.divergence_after,
                d.overhang_columns
            )?;
            if !options.quiet {
                draw(&mut out, &sim)?;
            }
            out.flush()?;
        }
        if options.animate && frame < options.frames {
            thread::sleep(Duration::from_millis(50));
        }
    }
    if let Some(file) = &mut csv {
        file.flush()?;
    }
    Ok(())
}

fn draw(out: &mut impl Write, sim: &Simulation) -> io::Result<()> {
    let (width, height) = (80, 24);
    let config = sim.config();
    let mut pixels = vec![b' '; width * height];
    for row in 0..height {
        for col in 0..width {
            let x = col as f64 / (width - 1) as f64 * config.width();
            let y = (1.0 - row as f64 / (height - 1) as f64) * config.height();
            if y < sim.grid().bed_height(x) {
                pixels[row * width + col] = b'#';
            }
        }
    }
    for p in sim.particles() {
        let col = ((p.x / config.width() * (width - 1) as f64).round() as usize).min(width - 1);
        let row = (((1.0 - p.y / config.height()) * (height - 1) as f64).round() as usize)
            .min(height - 1);
        pixels[row * width + col] = b'*';
    }
    for row in pixels.chunks(width) {
        writeln!(out, "|{}|", String::from_utf8_lossy(row))?;
    }
    writeln!(out, "{}", "-".repeat(width + 2))?;
    Ok(())
}

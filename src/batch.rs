use crate::Options;
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    thread,
    time::Duration,
};
use water_simulation::solver::Simulation;

fn statistics(sim: &Simulation, initial_mass: f64) -> String {
    let min_h = sim
        .cells()
        .iter()
        .map(|q| q.h)
        .fold(f64::INFINITY, f64::min);
    let max_h = sim
        .cells()
        .iter()
        .map(|q| q.h)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_u = sim
        .cells()
        .iter()
        .map(|q| q.velocity().abs())
        .fold(0.0, f64::max);
    format!(
        "t={:.3}s  steps={}  dt={:.6}s\nh=[{:.6}, {:.6}]m  max|u|={:.6}m/s  mass={:.12}  relative_mass_error={:+.3e}",
        sim.time(),
        sim.steps(),
        sim.last_dt(),
        min_h,
        max_h,
        max_u,
        sim.mass(),
        (sim.mass() - initial_mass) / initial_mass,
    )
}

fn draw(out: &mut impl Write, sim: &Simulation) -> io::Result<()> {
    let width = sim.cells().len().min(81);
    let height = 16;
    let config = sim.config();
    // 固定纵轴，避免逐帧自动缩放掩盖数值耗散；纵向做了夸张显示。
    let amplitude = config.bump_height.max(0.05);
    let low = config.rest_depth - 0.25 * amplitude;
    let high = config.rest_depth + 1.25 * amplitude;
    let row_for = |h: f64| -> usize {
        (((high - h) / (high - low) * (height - 1) as f64).round() as isize)
            .clamp(0, height as isize - 1) as usize
    };
    let mut surface = Vec::with_capacity(width);
    for column in 0..width {
        // 仅对显示做线性采样，不改变参与求解的网格。
        let position = column as f64 * (sim.cells().len() - 1) as f64 / (width - 1) as f64;
        let left = position.floor() as usize;
        let right = (left + 1).min(sim.cells().len() - 1);
        let fraction = position - left as f64;
        let h = (1.0 - fraction) * sim.cells()[left].h + fraction * sim.cells()[right].h;
        surface.push(row_for(h));
    }
    for row in 0..height {
        let h = high - row as f64 / (height - 1) as f64 * (high - low);
        write!(out, "{h:5.2} |")?;
        for &surface_row in &surface {
            let ch = if row == surface_row {
                '*'
            } else if row == row_for(config.rest_depth) {
                '.'
            } else {
                ' '
            };
            write!(out, "{ch}")?;
        }
        writeln!(out, "|")?;
    }
    writeln!(out, "      +{}+", "-".repeat(width))?;
    writeln!(
        out,
        "       x=0 ... {:.1}m; *=surface, .=initial resting level\n",
        config.length
    )
}

pub fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    let mut sim = Simulation::new(options.config)?;
    let initial_mass = sim.mass();
    let mut csv = options
        .csv
        .as_ref()
        .map(|path| File::create_new(path).map(BufWriter::new))
        .transpose()?;
    if let Some(file) = &mut csv {
        writeln!(file, "t,cell,x,h,hu,u")?;
    }
    let mut out = BufWriter::new(io::stdout().lock());
    for frame in 0..=options.frames {
        let target = options.duration * (frame as f64 / options.frames as f64);
        sim.advance_to(target)?;
        if let Some(file) = &mut csv {
            for (i, q) in sim.cells().iter().enumerate() {
                writeln!(
                    file,
                    "{:.17},{},{:.17},{:.17},{:.17},{:.17}",
                    sim.time(),
                    i,
                    (i as f64 + 0.5) * sim.dx(),
                    q.h,
                    q.hu,
                    q.velocity()
                )?;
            }
        }
        if !options.quiet {
            if options.animate {
                write!(out, "\x1b[2J\x1b[H")?;
            }
            writeln!(out, "{}", statistics(&sim, initial_mass))?;
            draw(&mut out, &sim)?;
            out.flush()?;
            if options.animate && frame < options.frames {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
    if options.quiet {
        writeln!(out, "{}", statistics(&sim, initial_mass))?;
    }
    out.flush()?;
    if let Some(file) = &mut csv {
        file.flush()?;
    }
    Ok(())
}

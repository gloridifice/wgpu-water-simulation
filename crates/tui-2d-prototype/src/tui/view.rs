use super::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        Block, BorderType, Paragraph,
        canvas::{Canvas, Line as CanvasLine, Points},
    },
};

const CYAN: Color = Color::Rgb(65, 207, 232);
const MUTED: Color = Color::Rgb(126, 145, 168);
const BACKGROUND: Color = Color::Rgb(12, 17, 25);

#[derive(Clone, Copy)]
pub struct TankPlot {
    area: Rect,
    width: f64,
    height: f64,
}
impl TankPlot {
    pub fn position(self, column: u16, row: u16) -> Option<(f64, f64)> {
        if self.area.width < 2
            || self.area.height < 2
            || column < self.area.x
            || column >= self.area.right()
            || row < self.area.y
            || row >= self.area.bottom()
        {
            return None;
        }
        Some((
            f64::from(column - self.area.x) / f64::from(self.area.width - 1) * self.width,
            (1.0 - f64::from(row - self.area.y) / f64::from(self.area.height - 1)) * self.height,
        ))
    }
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(Color::Rgb(52, 70, 91)))
}

pub fn draw(frame: &mut Frame, app: &App) -> Option<TankPlot> {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::default().bg(BACKGROUND).fg(Color::White)),
        area,
    );
    if area.width < 70 || area.height < 22 {
        frame.render_widget(
            Paragraph::new(
                "Terminal too small: use at least 70x22.\n120x40 recommended. Q / Esc quits.",
            )
            .style(CYAN),
            area,
        );
        return None;
    }
    let [heading, stats, tank, controls] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(6),
        Constraint::Min(10),
        Constraint::Length(4),
    ])
    .areas(area);
    frame.render_widget(
        Line::from(vec![
            Span::styled(" PIC WATER ", Style::default().fg(CYAN).bold()),
            Span::styled(
                "2D side view / CPU / particles + pressure",
                Style::default().fg(MUTED),
            ),
        ]),
        heading,
    );
    let d = app.sim.diagnostics();
    let state = if app.paused { "PAUSED" } else { "RUNNING" };
    let waves = if app.sim.wavemaker() { "ON" } else { "OFF" };
    let p = app.sim.particles()[app.selected];
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(format!(
                "{state} t={:.3}s  {:.3}x  waves={waves}  pulses={}  splashes={}",
                app.sim.time(),
                app.speed,
                app.pulses,
                app.splashes
            )),
            Line::from(format!(
                "particles={}  steps={}  dt={:.5}s  speed<={:.2}m/s",
                app.sim.particles().len(),
                app.sim.steps(),
                app.sim.last_dt(),
                d.max_speed
            )),
            Line::from(format!(
                "div {:.2e} -> {:.2e}  CG={}  overhangs={}",
                d.divergence_before, d.divergence_after, d.pressure_iterations, d.overhang_columns
            )),
            Line::styled(
                format!(
                    "particle={}  x={:.3} y={:.3}  u={:+.3} v={:+.3}",
                    app.selected, p.x, p.y, p.u, p.v
                ),
                Style::default().fg(Color::Yellow),
            ),
        ])
        .block(panel(" Simulation / selected particle ")),
        stats,
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Space pause  N step  R reset  W paddle  B breaking pulse  +/- speed"),
            Line::from("Left click: splash  Left/Right: particle  G grid  Q/Esc quit"),
        ])
        .style(MUTED)
        .block(panel(" Controls / B triggers a curl pulse ")),
        controls,
    );
    let bounds = panel(" Water / x horizontal, y vertical / click to push water ");
    let inner = bounds.inner(tank);
    frame.render_widget(bounds, tank);
    let plot = TankPlot {
        area: Rect::new(
            inner.x + 5,
            inner.y,
            inner.width.saturating_sub(5),
            inner.height.saturating_sub(1),
        ),
        width: app.sim.config().width(),
        height: app.view_height,
    };
    draw_water(frame, plot, app);
    frame.render_widget(
        Paragraph::new(format!("{:.1}", plot.height)).style(MUTED),
        Rect::new(inner.x, inner.y, 4, 1),
    );
    frame.render_widget(
        Paragraph::new("0 m").style(MUTED),
        Rect::new(inner.x, plot.area.bottom() - 1, 4, 1),
    );
    let labels = format!(
        "0 m{:>width$}",
        format!("{:.1} m", plot.width),
        width = plot.area.width.saturating_sub(3) as usize
    );
    frame.render_widget(
        Paragraph::new(labels).style(MUTED),
        Rect::new(plot.area.x, plot.area.bottom(), plot.area.width, 1),
    );
    Some(plot)
}

fn draw_water(frame: &mut Frame, plot: TankPlot, app: &App) {
    let mut water = Vec::with_capacity(app.sim.particles().len());
    let mut fast = Vec::new();
    for p in app.sim.particles() {
        if p.u.hypot(p.v) > 3.5 {
            fast.push((p.x, p.y));
        } else {
            water.push((p.x, p.y));
        }
    }
    let p = app.sim.particles()[app.selected];
    let selected = [(p.x, p.y)];
    let grid = app.sim.grid();
    frame.render_widget(
        Canvas::default()
            .background_color(BACKGROUND)
            .marker(Marker::Braille)
            .x_bounds([0.0, plot.width])
            .y_bounds([0.0, plot.height])
            .paint(|ctx| {
                if app.show_grid {
                    for i in 0..=grid.nx {
                        let x = i as f64 * grid.h;
                        ctx.draw(&CanvasLine {
                            x1: x,
                            y1: 0.0,
                            x2: x,
                            y2: plot.height,
                            color: Color::Rgb(23, 38, 52),
                        });
                    }
                    for j in 0..=grid.ny {
                        let y = j as f64 * grid.h;
                        ctx.draw(&CanvasLine {
                            x1: 0.0,
                            y1: y,
                            x2: plot.width,
                            y2: y,
                            color: Color::Rgb(23, 38, 52),
                        });
                    }
                }
                for i in 0..grid.nx * 2 {
                    let x = (i as f64 + 0.5) * grid.h * 0.5;
                    ctx.draw(&CanvasLine {
                        x1: x,
                        y1: 0.0,
                        x2: x,
                        y2: grid.bed_height(x),
                        color: Color::Rgb(115, 99, 73),
                    });
                }
                ctx.draw(&CanvasLine {
                    x1: app.sim.paddle_x(),
                    y1: 0.0,
                    x2: app.sim.paddle_x(),
                    y2: app.sim.config().water_level() + grid.h,
                    color: Color::Rgb(241, 161, 78),
                });
                ctx.draw(&Points {
                    coords: &water,
                    color: CYAN,
                });
                ctx.draw(&Points {
                    coords: &fast,
                    color: Color::Rgb(212, 243, 250),
                });
                ctx.layer();
                ctx.draw(&Points {
                    coords: &selected,
                    color: Color::Yellow,
                });
            }),
        plot.area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use tui_2d_prototype::pic::Config;
    #[test]
    fn particle_view_and_mouse_coordinates_follow_window_size() {
        let app = App::new(Config::default(), true, true, true).unwrap();
        for (width, height) in [(120, 40), (80, 24), (70, 22), (40, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut plot = None;
            terminal
                .draw(|frame| {
                    plot = draw(frame, &app);
                })
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            if width < 70 {
                assert!(plot.is_none());
                assert!(text.contains("Terminal too small"));
            } else {
                for label in ["PIC WATER", "PAUSED", "particles=", "Controls", "Water /"] {
                    assert!(text.contains(label));
                }
                let p = plot.unwrap();
                assert_eq!(p.position(p.area.x, p.area.y), Some((0.0, p.height)));
                assert_eq!(
                    p.position(p.area.right() - 1, p.area.bottom() - 1),
                    Some((p.width, 0.0))
                );
                assert!(p.position(p.area.x - 1, p.area.y).is_none());
                assert!(p.position(p.area.x, p.area.bottom()).is_none());
            }
        }
    }
}

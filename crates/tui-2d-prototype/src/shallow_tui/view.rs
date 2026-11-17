use super::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{Axis, Block, BorderType, Chart, Dataset, GraphType, Paragraph},
};

const CYAN: Color = Color::Rgb(74, 211, 233);
const MUTED: Color = Color::Rgb(117, 133, 155);

#[derive(Clone, Copy, Debug)]
pub struct SurfacePlot {
    area: Rect,
    length: f64,
}

impl SurfacePlot {
    /// Accept clicks only inside the water plot; axes, borders and other panels do not create disturbances.
    pub fn x_at(self, column: u16, row: u16) -> Option<f64> {
        if self.area.width < 2
            || column < self.area.x
            || column >= self.area.right()
            || row < self.area.y
            || row >= self.area.bottom()
        {
            return None;
        }
        Some(f64::from(column - self.area.x) / f64::from(self.area.width - 1) * self.length)
    }
}

pub fn draw(frame: &mut Frame, app: &App) -> Option<SurfacePlot> {
    let area = frame.area();
    frame.render_widget(
        Block::new().style(Style::default().bg(Color::Rgb(12, 17, 25)).fg(Color::White)),
        area,
    );
    if area.width < 70 || area.height < 22 {
        frame.render_widget(
            Paragraph::new(
                "Terminal too small: use at least 70x22.\nQ / Esc to quit; resize to continue.",
            )
            .style(CYAN),
            area,
        );
        return None;
    }
    let [heading, stats, charts, controls] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Min(12),
        Constraint::Length(4),
    ])
    .areas(area);
    frame.render_widget(
        Line::from(vec![
            Span::styled(" SHALLOW WATER ", Style::default().fg(CYAN).bold()),
            Span::styled(
                "1D CPU  /  reflective tank  /  Rusanov",
                Style::default().fg(MUTED),
            ),
        ]),
        heading,
    );
    draw_stats(frame, stats, app);
    let chart_constraints = if charts.height < 20 {
        [Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)]
    } else {
        [Constraint::Ratio(3, 5), Constraint::Ratio(2, 5)]
    };
    let [surface_area, velocity_area] = Layout::vertical(chart_constraints).areas(charts);
    let surface_plot = draw_charts(frame, surface_area, velocity_area, app);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Space pause  N step  R reset  +/- speed  G grid  Left click: ripple"),
            Line::from("Left/Right select cell   Home/End first/last   C center   Q/Esc quit"),
        ])
        .block(panel(" Controls "))
        .style(MUTED),
        controls,
    );
    Some(surface_plot)
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(52, 70, 91)))
        .title(title)
}

fn draw_stats(frame: &mut Frame, area: Rect, app: &App) {
    let sim = &app.sim;
    let q = sim.cells()[app.selected];
    let state = if app.paused { "PAUSED" } else { "RUNNING" };
    let color = if app.paused {
        Color::Yellow
    } else {
        Color::Green
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(format!("{state:<7}"), Style::default().fg(color).bold()),
            Span::raw(format!(
                "  t={:.3}s  speed={:.3}x  N={}  steps={}  ripples={}",
                sim.time(),
                app.speed,
                sim.cells().len(),
                sim.steps(),
                app.disturbances
            )),
        ]),
        Line::from(format!(
            "mass={:.9}  rel.err={:+.2e}  dt={:.6}s  CFL={:.2}",
            sim.mass(),
            (sim.mass() - app.initial_mass) / app.initial_mass,
            sim.last_dt(),
            sim.config().cfl
        )),
        Line::styled(
            format!(
                "cell={}  x={:.3}m  h={:.6}  hu={:+.6}  u={:+.6}",
                app.selected,
                (app.selected as f64 + 0.5) * sim.dx(),
                q.h,
                q.hu,
                q.velocity()
            ),
            Style::default().fg(Color::Yellow),
        ),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(panel(" Simulation / selected cell ")),
        area,
    );
}

fn draw_charts(
    frame: &mut Frame,
    surface_area: Rect,
    velocity_area: Rect,
    app: &App,
) -> SurfacePlot {
    let sim = &app.sim;
    let config = sim.config();
    let mut surface = Vec::with_capacity(config.cells);
    let mut velocity = Vec::with_capacity(config.cells);
    for (i, q) in sim.cells().iter().enumerate() {
        let x = (i as f64 + 0.5) * sim.dx();
        surface.push((x, q.h));
        velocity.push((x, q.velocity()));
    }
    let rest = [(0.0, config.rest_depth), (config.length, config.rest_depth)];
    let selected_h = [surface[app.selected]];
    let [low, high] = app.surface_bounds;
    let h_min = sim
        .cells()
        .iter()
        .map(|q| q.h)
        .fold(f64::INFINITY, f64::min);
    let h_max = sim
        .cells()
        .iter()
        .map(|q| q.h)
        .fold(f64::NEG_INFINITY, f64::max);
    let surface_chart = Chart::new(vec![
        curve("rest", &rest, Color::DarkGray),
        curve("initial", &app.initial_surface, Color::Rgb(74, 86, 122)),
        curve("h", &surface, CYAN),
        Dataset::default()
            .marker(Marker::Dot)
            .style(Style::default().fg(Color::Yellow))
            .data(&selected_h),
    ])
    .block(panel(format!(
        " Water surface h [m] / left click: ripple / [{h_min:.4}, {h_max:.4}] "
    )))
    .x_axis(x_axis(config.length))
    .y_axis(y_axis(low, high, "h", surface_area.height));
    frame.render_widget(surface_chart, surface_area);

    let max_u = velocity.iter().map(|(_, u)| u.abs()).fold(0.0, f64::max);
    let velocity_bound = max_u.max(0.05) * 1.2;
    let zero = [(0.0, 0.0), (config.length, 0.0)];
    let selected_u = [velocity[app.selected]];
    let velocity_chart = Chart::new(vec![
        curve("", &zero, Color::DarkGray),
        curve("", &velocity, Color::Green),
        Dataset::default()
            .marker(Marker::Dot)
            .style(Style::default().fg(Color::Yellow))
            .data(&selected_u),
    ])
    .block(panel(format!(
        " Velocity u [m/s] / auto scale / max|u|={max_u:.4} "
    )))
    .x_axis(x_axis(config.length))
    .y_axis(y_axis(
        -velocity_bound,
        velocity_bound,
        "u",
        velocity_area.height,
    ));
    frame.render_widget(velocity_chart, velocity_area);

    // Ratatui 0.30 Chart: reserve the y-label width and one y-axis column inside the border, plus two bottom rows for the x-axis and labels.
    // Share the label source above; tests compare rendered axis positions to prevent hit-area drift.
    let inner = panel("").inner(surface_area);
    let labels = y_labels(low, high, surface_area.height);
    let label_width =
        (labels.iter().map(String::len).max().unwrap_or(0) as u16).min(inner.width / 3);
    SurfacePlot {
        area: Rect::new(
            inner.x + label_width + 1,
            inner.y,
            inner.width.saturating_sub(label_width + 1),
            inner.height.saturating_sub(2),
        ),
        length: config.length,
    }
}

fn curve<'a>(name: &'a str, data: &'a [(f64, f64)], color: Color) -> Dataset<'a> {
    Dataset::default()
        .name(name)
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::default().fg(color))
        .data(data)
}

fn x_axis(length: f64) -> Axis<'static> {
    Axis::default()
        .bounds([0.0, length])
        .style(Style::default().fg(MUTED))
        .labels([
            "0".to_owned(),
            format!("{:.1}", length * 0.5),
            format!("{length:.1} m"),
        ])
}

fn y_axis(low: f64, high: f64, name: &'static str, height: u16) -> Axis<'static> {
    // Small plots show only upper and lower bounds, avoiding three labels in two rows.
    let labels = y_labels(low, high, height);
    let axis = Axis::default()
        .bounds([low, high])
        .style(Style::default().fg(MUTED))
        .labels(labels);
    if height < 7 { axis } else { axis.title(name) }
}

fn y_labels(low: f64, high: f64, height: u16) -> Vec<String> {
    if height < 8 {
        vec![format!("{low:.2}"), format!("{high:.2}")]
    } else {
        vec![
            format!("{low:.2}"),
            format!("{:.2}", (low + high) * 0.5),
            format!("{high:.2}"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    use tui_2d_prototype::solver::Config;

    #[test]
    fn charts_and_selected_state_render_at_normal_and_small_sizes() {
        let app = App::new(Config::default(), true).unwrap();
        for (width, height) in [(120, 36), (80, 24), (70, 22), (40, 12)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut plot = None;
            terminal
                .draw(|frame| {
                    plot = draw(frame, &app);
                })
                .unwrap();
            if let Some(plot) = plot {
                let a = plot.area;
                let buffer = terminal.backend().buffer();
                assert_eq!(buffer[(a.x - 1, a.y)].symbol(), "│");
                assert_eq!(buffer[(a.x, a.bottom())].symbol(), "─");
                assert_eq!(plot.x_at(a.x, a.y), Some(0.0));
                assert_eq!(plot.x_at(a.right() - 1, a.bottom() - 1), Some(20.0));
                assert_eq!(plot.x_at(a.x - 1, a.y), None);
                assert_eq!(plot.x_at(a.x, a.bottom()), None);
                assert_eq!(plot.x_at(a.x, height - 1), None);
            } else {
                assert!(width < 70);
            }
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            if width < 70 {
                assert!(text.contains("Terminal too small"));
            } else {
                for label in [
                    "SHALLOW WATER",
                    "PAUSED",
                    "N=201",
                    "Water surface",
                    "Velocity",
                    "cell=100",
                    "hu=",
                    "Controls",
                ] {
                    assert!(text.contains(label), "{width}x{height}: missing {label}");
                }
            }
        }
    }
}

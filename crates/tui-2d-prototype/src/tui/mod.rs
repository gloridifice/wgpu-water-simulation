mod app;
mod view;

use app::App;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
};
use ratatui::DefaultTerminal;
use std::{
    io,
    time::{Duration, Instant},
};
use tui_2d_prototype::pic::Config;

pub fn run(
    config: Config,
    paused: bool,
    waves: bool,
    pulse: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new(config, paused, waves, pulse)?;
    ratatui::run(|terminal| {
        let _capture = MouseCapture;
        execute!(io::stdout(), EnableMouseCapture)?;
        event_loop(terminal, &mut app)
    })
}

struct MouseCapture;
impl Drop for MouseCapture {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture);
    }
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut previous = Instant::now();
    loop {
        let now = Instant::now();
        app.tick(now.duration_since(previous))?;
        previous = now;
        let mut plot = None;
        terminal.draw(|frame| {
            plot = view::draw(frame, app);
        })?;
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(app, key)? {
                        return Ok(());
                    }
                    previous = Instant::now();
                }
                Event::Mouse(mouse) => handle_mouse(app, mouse, plot)?,
                Event::Resize(_, _) => terminal.clear()?,
                _ => {}
            }
        }
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent, plot: Option<view::TankPlot>) -> io::Result<()> {
    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some((x, y)) = plot.and_then(|p| p.position(mouse.column, mouse.row))
    {
        app.splash(x, y).map_err(io::Error::other)?;
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> io::Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }
    match key.code {
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char(' ') => app.paused = !app.paused,
        KeyCode::Char('n' | 'N') => app.single_step().map_err(io::Error::other)?,
        KeyCode::Char('r' | 'R') => app.reset().map_err(io::Error::other)?,
        KeyCode::Char('w' | 'W') => app.sim.set_wavemaker(!app.sim.wavemaker()),
        KeyCode::Char('b' | 'B') => app.wave_pulse(),
        KeyCode::Char('g' | 'G') => app.show_grid = !app.show_grid,
        KeyCode::Char('+' | '=') => app.change_speed(true),
        KeyCode::Char('-' | '_') => app.change_speed(false),
        KeyCode::Left => app.move_selection(false),
        KeyCode::Right => app.move_selection(true),
        KeyCode::Home => app.selected = 0,
        KeyCode::End => app.selected = app.sim.particles().len() - 1,
        KeyCode::Char('c' | 'C') => app.selected = app.sim.particles().len() / 2,
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn mouse_and_keyboard_control_pic() {
        let mut app = App::new(Config::default(), true, true, true).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let mut plot = None;
        terminal
            .draw(|frame| {
                plot = view::draw(frame, &app);
            })
            .unwrap();
        let click = |column, row, kind| MouseEvent {
            column,
            row,
            kind,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(
            &mut app,
            click(2, 2, MouseEventKind::Down(MouseButton::Left)),
            plot,
        )
        .unwrap();
        assert_eq!(app.splashes, 0);
        handle_mouse(
            &mut app,
            click(30, 29, MouseEventKind::Down(MouseButton::Left)),
            plot,
        )
        .unwrap();
        assert_eq!(app.splashes, 1);
        assert_eq!(app.sim.steps(), 0);
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        handle_key(&mut app, press(KeyCode::Char('n'))).unwrap();
        assert_eq!(app.sim.steps(), 1);
        handle_key(&mut app, press(KeyCode::Char('w'))).unwrap();
        assert!(!app.sim.wavemaker());
        handle_key(&mut app, press(KeyCode::Char('b'))).unwrap();
        assert_eq!(app.pulses, 2);
        handle_key(&mut app, press(KeyCode::Char('g'))).unwrap();
        assert!(app.show_grid);
        assert!(handle_key(&mut app, press(KeyCode::Esc)).unwrap());
    }
}

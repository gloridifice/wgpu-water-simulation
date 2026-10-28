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
use tui_2d_prototype::solver::Config;

pub fn run(config: Config, paused: bool) -> Result<(), Box<dyn std::error::Error>> {
    // 先验证求解参数，再切换终端模式；run 在正常退出/错误/恐慌时恢复终端。
    let mut app = App::new(config, paused)?;
    ratatui::run(|terminal| {
        let _capture = MouseCapture;
        execute!(io::stdout(), EnableMouseCapture)?;
        event_loop(terminal, &mut app)
    })
}

struct MouseCapture;

impl Drop for MouseCapture {
    fn drop(&mut self) {
        // 正常返回、错误与 panic 展开均关闭鼠标捕获，恢复终端的鼠标行为。
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
        let mut surface_plot = None;
        terminal.draw(|frame| {
            surface_plot = view::draw(frame, app);
        })?;
        if event::poll(Duration::from_millis(33))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if handle_key(app, key)? {
                        return Ok(());
                    }
                    // 按键处理（尤其暂停/恢复）前消耗的时间不计入下一个 tick。
                    previous = Instant::now();
                }
                Event::Resize(_, _) => {
                    terminal.clear()?;
                }
                Event::Mouse(mouse) => handle_mouse(app, mouse, surface_plot)?,
                _ => {}
            }
        }
    }
}

fn handle_mouse(
    app: &mut App,
    mouse: MouseEvent,
    plot: Option<view::SurfacePlot>,
) -> io::Result<()> {
    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
        && let Some(x) = plot.and_then(|plot| plot.x_at(mouse.column, mouse.row))
    {
        app.disturb_at(x).map_err(io::Error::other)?;
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool, io::Error> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }
    match key.code {
        KeyCode::Char('q' | 'Q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char(' ') => app.paused = !app.paused,
        KeyCode::Char('n' | 'N') => app.single_step().map_err(io::Error::other)?,
        KeyCode::Char('r' | 'R') => app.reset().map_err(io::Error::other)?,
        KeyCode::Char('g' | 'G') => app.toggle_grid().map_err(io::Error::other)?,
        KeyCode::Char('+' | '=') => app.change_speed(true),
        KeyCode::Char('-' | '_') => app.change_speed(false),
        KeyCode::Left => app.move_selection(false),
        KeyCode::Right => app.move_selection(true),
        KeyCode::Home => app.selected = 0,
        KeyCode::End => app.selected = app.sim.cells().len() - 1,
        KeyCode::Char('c' | 'C') => app.selected = app.sim.cells().len() / 2,
        _ => {}
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn only_left_button_presses_in_the_surface_plot_add_ripples() {
        for (width, height) in [(120, 36), (80, 24), (40, 12)] {
            let mut app = App::new(Config::default(), true).unwrap();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut plot = None;
            terminal
                .draw(|frame| {
                    plot = view::draw(frame, &app);
                })
                .unwrap();
            let event = |kind, column, row| MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            };
            // 水面图外的点击和拖动/放开均不改变水面。
            handle_mouse(
                &mut app,
                event(MouseEventKind::Down(MouseButton::Left), 2, 2),
                plot,
            )
            .unwrap();
            handle_mouse(
                &mut app,
                event(MouseEventKind::Down(MouseButton::Right), width / 2, 9),
                plot,
            )
            .unwrap();
            handle_mouse(
                &mut app,
                event(MouseEventKind::Up(MouseButton::Left), width / 2, 9),
                plot,
            )
            .unwrap();
            handle_mouse(
                &mut app,
                event(MouseEventKind::Drag(MouseButton::Left), width / 2, 9),
                plot,
            )
            .unwrap();
            assert_eq!(app.disturbances, 0);
            handle_mouse(
                &mut app,
                event(MouseEventKind::Down(MouseButton::Left), width / 2, 9),
                plot,
            )
            .unwrap();
            assert_eq!(app.disturbances, usize::from(width >= 70));
            assert!(app.paused);
            assert_eq!(app.sim.steps(), 0);
        }
    }

    #[test]
    fn keyboard_actions_reach_the_simulator() {
        let mut app = App::new(Config::default(), true).unwrap();
        let press = |code| KeyEvent::new(code, KeyModifiers::NONE);
        handle_key(&mut app, press(KeyCode::Char('n'))).unwrap();
        assert_eq!(app.sim.steps(), 1);
        handle_key(&mut app, press(KeyCode::Char(' '))).unwrap();
        assert!(!app.paused);
        handle_key(&mut app, press(KeyCode::Char('g'))).unwrap();
        assert_eq!(app.sim.cells().len(), 41);
        assert!(app.paused);
        handle_key(&mut app, press(KeyCode::Right)).unwrap();
        assert_eq!(app.selected, 21);
        handle_key(&mut app, press(KeyCode::Char('+'))).unwrap();
        assert_eq!(app.speed, 2.0);
        handle_key(&mut app, press(KeyCode::Char('r'))).unwrap();
        assert_eq!(app.sim.time(), 0.0);
        assert!(handle_key(&mut app, press(KeyCode::Char('q'))).unwrap());
        assert!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
            )
            .unwrap()
        );
    }
}

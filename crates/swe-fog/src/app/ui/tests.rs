//! Actual egui input at the fog panel boundary. GPU reset behavior has its own
//! numerical owner; these cases protect numeric binding and reset-intent delivery.
use super::*;
struct Harness {
    ctx: egui::Context,
    session: Session,
    time: f64,
}
impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        yummy_egui::Theme::default().apply(&ctx);
        Self {
            ctx,
            session: Session::default(),
            time: 0.0,
        }
    }
    fn frame(
        &mut self,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> (egui::FullOutput, Actions) {
        self.time += 0.05;
        let mut actions = Actions::default();
        let output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 800.0),
                )),
                time: Some(self.time),
                focused: true,
                events,
                modifiers,
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show_inside(root, |ui| {
                    initialization(ui, &mut self.session, &mut actions)
                });
            },
        );
        (output, actions)
    }
}
fn text_point(output: &egui::FullOutput, find: impl Fn(&str) -> bool) -> egui::Pos2 {
    fn collect(shape: &egui::Shape, texts: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(t) => {
                texts.push((t.galley.text().into(), t.pos + t.galley.size() * 0.5))
            }
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    collect(s, texts);
                }
            }
            _ => {}
        }
    }
    let mut texts = Vec::new();
    for s in &output.shapes {
        collect(&s.shape, &mut texts);
    }
    texts
        .iter()
        .find(|(text, _)| find(text))
        .map(|(_, p)| *p)
        .unwrap_or_else(|| panic!("control text not found: {texts:?}"))
}
fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers,
    }
}
fn offset_point(output: &egui::FullOutput) -> egui::Pos2 {
    text_point(output, |text| {
        text.replace('−', "-")
            .parse::<f32>()
            .is_ok_and(|v| (v + 0.2).abs() < 1e-5)
    })
}
#[test]
fn held_drag_and_precise_input_deliver_reset_before_release_without_moving_player_or_unpausing() {
    let mut h = Harness::new();
    h.session.paused = true;
    h.session.player.feet = [1.0, 0.0, 2.0];
    let camera = h.session.camera;
    let (output, actions) = h.frame(vec![], egui::Modifiers::NONE);
    assert!(!actions.reset);
    let p = offset_point(&output);
    h.frame(
        vec![egui::Event::PointerMoved(p), button(p, true)],
        egui::Modifiers::NONE,
    );
    let (_, actions) = h.frame(
        vec![egui::Event::PointerMoved(p - egui::vec2(100.0, 0.0))],
        egui::Modifiers::NONE,
    );
    assert!(
        actions.reset && h.session.fog.height_offset < -0.6,
        "numeric change must deliver reset while button is still held"
    );
    let (_, actions) = h.frame(vec![egui::Event::PointerMoved(p)], egui::Modifiers::NONE);
    assert!(actions.reset);
    assert!((h.session.fog.height_offset + 0.2).abs() < 0.015);
    assert!(h.session.paused);
    assert_eq!(h.session.player.feet, [1.0, 0.0, 2.0]);
    assert_eq!(h.session.camera, camera);
    h.frame(vec![button(p, false)], egui::Modifiers::NONE);
    // Two clicks, then the same edit events a real native input backend emits.
    for _ in 0..2 {
        h.frame(vec![button(p, true)], egui::Modifiers::NONE);
        h.frame(vec![button(p, false)], egui::Modifiers::NONE);
    }
    let (_, actions) = h.frame(
        vec![
            key(egui::Key::A, egui::Modifiers::COMMAND),
            egui::Event::Text("-0.30".into()),
            key(egui::Key::Enter, egui::Modifiers::NONE),
        ],
        egui::Modifiers::NONE,
    );
    assert!(
        (h.session.fog.height_offset + 0.30).abs() < 1e-6 && actions.reset,
        "precise input must reach configuration and reset intent"
    );
}
#[test]
fn shift_drag_is_finer_and_initialization_switch_does_not_request_a_reset() {
    let drag = |fine| {
        let mut h = Harness::new();
        let (output, _) = h.frame(vec![], egui::Modifiers::NONE);
        let p = offset_point(&output);
        h.frame(
            vec![egui::Event::PointerMoved(p), button(p, true)],
            egui::Modifiers::NONE,
        );
        h.frame(
            vec![egui::Event::PointerMoved(p + egui::vec2(10.0, 0.0))],
            egui::Modifiers::NONE,
        );
        let before = h.session.fog.height_offset;
        let (_, a) = h.frame(
            vec![egui::Event::PointerMoved(p + egui::vec2(30.0, 0.0))],
            if fine {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            },
        );
        assert!(a.reset);
        h.session.fog.height_offset - before
    };
    let normal = drag(false);
    let fine = drag(true);
    assert!(normal > 0.1 && fine > 0.0 && fine < normal / 3.0);
    let mut h = Harness::new();
    let (output, _) = h.frame(vec![], egui::Modifiers::NONE);
    let p = text_point(&output, |t| t.contains("水平雾顶初态"));
    h.frame(
        vec![egui::Event::PointerMoved(p), button(p, true)],
        egui::Modifiers::NONE,
    );
    let (_, actions) = h.frame(vec![button(p, false)], egui::Modifiers::NONE);
    assert!(
        h.session.fog.uniform_top && !actions.reset,
        "switches must not reset numeric history"
    );
}

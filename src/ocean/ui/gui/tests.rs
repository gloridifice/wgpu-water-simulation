use super::*;
#[test]
fn default_workspace_keeps_view_right_of_controls_and_resizes() {
    let ctx = egui::Context::default();
    let theme = charming_egui::Theme::default();
    theme.apply(&ctx);
    crate::ocean::ui::fonts::install(&ctx);
    let dock_style = theme.dock_style().clone();
    let mut dock = Workspaces::default().items.into_iter().next().unwrap().dock;
    let mut previous_width = 0.0;
    for width in [1200.0, 1600.0] {
        let mut rects = std::collections::HashMap::new();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui_dock::DockArea::new(&mut dock)
                        .style(dock_style.clone())
                        .show_inside(
                            ui,
                            &mut PanelViewer(|tab, ui: &mut egui::Ui| {
                                rects.insert(tab, ui.available_rect_before_wrap());
                            }),
                        );
                });
            },
        );
        let view = rects[&Panel::View];
        assert!(rects[&Panel::Scene].right() <= view.left());
        assert!(view.width() > rects[&Panel::Scene].width());
        assert!(view.width() > previous_width);
        previous_width = view.width();
    }
}

#[test]
fn wasd_moves_only_in_focused_view_without_parameter_editing() {
    let ctx = egui::Context::default();
    let mut camera = Camera::default();
    let mut text = String::new();
    let mut time = 0.0;
    let mut frame = |events, focused, editing| {
        time += 0.05;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 600.0),
                )),
                time: Some(time),
                focused,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::left("controls")
                    .exact_width(200.0)
                    .show(ctx, |ui| {
                        let response = ui.text_edit_singleline(&mut text);
                        if editing {
                            response.request_focus();
                        } else {
                            response.surrender_focus();
                        }
                    });
                egui::CentralPanel::default().show(ctx, |ui| {
                    ocean_view(ui, egui::TextureId::User(0), &mut camera, 128.0);
                });
            },
        );
        camera.target
    };
    let key = |pressed| egui::Event::Key {
        key: egui::Key::W,
        physical_key: Some(egui::Key::W),
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let view = egui::Event::PointerMoved(egui::pos2(400.0, 150.0));
    frame(vec![view.clone()], true, false);
    let first = frame(vec![key(true)], true, false);
    assert_ne!(first, [0.0; 2]);
    let held = frame(vec![], true, false);
    assert_ne!(held, first);
    assert_eq!(frame(vec![key(false)], true, false), held);
    assert_eq!(frame(vec![key(true)], true, true), held);
    assert_eq!(frame(vec![], false, false), held);
    assert_eq!(
        frame(
            vec![egui::Event::PointerMoved(egui::pos2(50.0, 150.0))],
            true,
            false
        ),
        held
    );
    assert_ne!(frame(vec![view, key(true)], true, false), held);
}

#[test]
fn orbit_is_owned_by_view_drag_not_sidebar_drag() {
    let ctx = egui::Context::default();
    let mut camera = Camera::new(128.0);
    let initial = camera.yaw;
    let mut frame = |events| {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 600.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::SidePanel::left("controls")
                    .exact_width(200.0)
                    .show(ctx, |ui| {
                        ui.label("Controls");
                    });
                egui::CentralPanel::default().show(ctx, |ui| {
                    let size = ocean_view(ui, egui::TextureId::User(0), &mut camera, 128.0);
                    assert!(size[0] > 700 && size[0] < 800);
                    assert!(size[1] > 500 && size[1] <= 600);
                });
            },
        );
        camera.yaw
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(vec![]);
    frame(vec![
        egui::Event::PointerMoved(egui::pos2(50.0, 150.0)),
        button(egui::pos2(50.0, 150.0), true),
    ]);
    assert_eq!(
        frame(vec![egui::Event::PointerMoved(egui::pos2(100.0, 180.0))]),
        initial
    );
    frame(vec![button(egui::pos2(100.0, 180.0), false)]);
    frame(vec![
        egui::Event::PointerMoved(egui::pos2(400.0, 150.0)),
        button(egui::pos2(400.0, 150.0), true),
    ]);
    assert_ne!(
        frame(vec![egui::Event::PointerMoved(egui::pos2(460.0, 180.0))]),
        initial
    );
}

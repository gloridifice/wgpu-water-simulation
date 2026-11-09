use super::*;
#[test]
fn default_workspace_keeps_view_right_of_controls_and_resizes() {
    let ctx = egui::Context::default();
    let theme = yummy_egui::Theme::default();
    theme.apply(&ctx);
    crate::ocean::ui::fonts::install(&ctx);
    let dock_style = theme.dock_style().clone();
    let mut dock = Workspaces::default()
        .items
        .into_iter()
        .next()
        .unwrap()
        .dock
        .clone();
    let mut previous_width = 0.0;
    for width in [1200.0, 1600.0] {
        let mut rects = std::collections::HashMap::new();
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 800.0),
                )),
                ..Default::default()
            },
            |root_ui| {
                egui::CentralPanel::default().show_inside(root_ui, |ui| {
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
        let _ = ctx.run_ui(
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
            |root_ui| {
                egui::Panel::left("controls")
                    .exact_size(200.0)
                    .show_inside(root_ui, |ui| {
                        let response = ui.text_edit_singleline(&mut text);
                        if editing {
                            response.request_focus();
                        } else {
                            response.surrender_focus();
                        }
                    });
                egui::CentralPanel::default().show_inside(root_ui, |ui| {
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
fn camera_drags_are_owned_by_view_and_keep_orbit_separate_from_pan() {
    for pointer_button in [egui::PointerButton::Primary, egui::PointerButton::Middle] {
        let ctx = egui::Context::default();
        let mut camera = Camera::new(128.0);
        let initial = camera;
        let mut frame = |events| {
            let _ = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |root_ui| {
                    egui::Panel::left("controls")
                        .exact_size(200.0)
                        .show_inside(root_ui, |ui| {
                            ui.label("Controls");
                        });
                    egui::CentralPanel::default().show_inside(root_ui, |ui| {
                        let size = ocean_view(ui, egui::TextureId::User(0), &mut camera, 128.0);
                        assert!(size[0] > 700 && size[0] < 800);
                        assert!(size[1] > 500 && size[1] <= 600);
                    });
                },
            );
            camera
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: pointer_button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(egui::pos2(50.0, 150.0)),
            button(egui::pos2(50.0, 150.0), true),
        ]);
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(egui::pos2(400.0, 150.0))]),
            initial,
            "a sidebar drag entering the view must not move the camera"
        );
        frame(vec![button(egui::pos2(400.0, 150.0), false)]);
        assert_eq!(frame(vec![button(egui::pos2(400.0, 150.0), true)]), initial);
        let dragged = frame(vec![egui::Event::PointerMoved(egui::pos2(460.0, 180.0))]);
        assert_eq!(dragged.distance, initial.distance);
        if pointer_button == egui::PointerButton::Primary {
            assert_ne!(dragged.yaw, initial.yaw);
            assert_ne!(dragged.pitch, initial.pitch);
            assert_eq!(dragged.target, initial.target);
        } else {
            assert_ne!(dragged.target, initial.target, "middle drag must pan");
            assert_eq!(dragged.yaw, initial.yaw);
            assert_eq!(dragged.pitch, initial.pitch);
            let uniform = dragged.uniform(OceanConfig::default(), 1.0);
            let matrix = glam::Mat4::from_cols_array_2d(&uniform.view_projection);
            let scene = matrix.project_point3(glam::Vec3::ZERO);
            assert!(scene.x > 0.0 && scene.y < 0.0, "scene must follow the drag");
        }
        let outside = frame(vec![egui::Event::PointerMoved(egui::pos2(100.0, 180.0))]);
        assert_ne!(
            outside, dragged,
            "view drag stays captured outside the view"
        );
        frame(vec![button(egui::pos2(100.0, 180.0), false)]);
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(egui::pos2(400.0, 150.0))]),
            outside,
            "releasing the button must stop camera movement"
        );
    }
}

use super::controls::{number, toggle};
use crate::ocean::ui::{gui::Gui, help::help};
use yummy_egui::{group, parameter, segmented_control};

impl Gui {
    pub(in crate::ocean::ui) fn reflection_panel(&mut self, ui: &mut egui::Ui) {
        group(ui, "屏幕空间反射（SSR）", |ui| {
            toggle(ui, "天空反射与高光", &mut self.optics.reflections);
            if !self.optics.reflections {
                ui.disable();
            }
            toggle(
                ui,
                "屏幕空间反射（SSR）",
                &mut self.optics.screen_space_reflections,
            );
            if self.optics.screen_space_reflections {
                number(
                    ui,
                    "SSR 强度",
                    &mut self.optics.reflection_strength,
                    0.0..=1.0,
                    false,
                );
                number(
                    ui,
                    "反射距离（米）",
                    &mut self.optics.reflection_distance,
                    1.0..=500.0,
                    true,
                );
                parameter(ui, "SSR 追踪步数", help("SSR 追踪步数"), |ui| {
                    ui.add(yummy_egui::ParameterSlider::new(
                        &mut self.optics.reflection_steps,
                        16..=256,
                    ))
                });
                number(
                    ui,
                    "SSR 命中容差",
                    &mut self.optics.reflection_tolerance,
                    0.25..=4.0,
                    false,
                );
                number(
                    ui,
                    "SSR 边缘淡出（像素）",
                    &mut self.optics.reflection_edge_fade,
                    0.0..=128.0,
                    false,
                );
            }
            parameter(
                ui,
                "SSR 调试视图",
                "显示水面上有效 SSR 的反射颜色，保留强度与边缘淡出；未命中为黑色。需要启用 SSR。实时生效。",
                |ui| {
                    segmented_control(ui, &mut self.optics.view, &[(0, "正常"), (15, "仅 SSR")]);
                },
            );
        });
    }
}

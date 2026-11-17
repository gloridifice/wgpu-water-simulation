use super::controls::{number, toggle};
use crate::ocean::ui::{gui::Gui, help::help};
use yummy_egui::{group, parameter, segmented_control};

impl Gui {
    pub(in crate::ocean::ui) fn reflection_panel(&mut self, ui: &mut egui::Ui) {
        group(ui, "Screen-Space Reflection (SSR)", |ui| {
            toggle(
                ui,
                "Sky Reflection and Highlights",
                &mut self.optics.reflections,
            );
            if !self.optics.reflections {
                ui.disable();
            }
            toggle(
                ui,
                "Screen-Space Reflection (SSR)",
                &mut self.optics.screen_space_reflections,
            );
            if self.optics.screen_space_reflections {
                number(
                    ui,
                    "SSR Strength",
                    &mut self.optics.reflection_strength,
                    0.0..=1.0,
                    false,
                );
                number(
                    ui,
                    "Reflection Distance (m)",
                    &mut self.optics.reflection_distance,
                    1.0..=500.0,
                    true,
                );
                parameter(ui, "SSR Trace Steps", help("SSR Trace Steps"), |ui| {
                    ui.add(yummy_egui::ParameterSlider::new(
                        &mut self.optics.reflection_steps,
                        16..=256,
                    ))
                });
                number(
                    ui,
                    "SSR Hit Tolerance",
                    &mut self.optics.reflection_tolerance,
                    0.25..=4.0,
                    false,
                );
                number(
                    ui,
                    "SSR Edge Fade (pixels)",
                    &mut self.optics.reflection_edge_fade,
                    0.0..=128.0,
                    false,
                );
            }
            parameter(
                ui,
                "SSR Debug View",
                "Show valid SSR reflection colors on the water surface, retaining strength and edge fade; misses are black. Requires SSR. Applies immediately.",
                |ui| {
                    segmented_control(
                        ui,
                        &mut self.optics.view,
                        &[(0, "Normal"), (15, "SSR Only")],
                    );
                },
            );
        });
    }
}

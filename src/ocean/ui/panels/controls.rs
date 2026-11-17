use crate::ocean::ui::help::help;
use std::ops::RangeInclusive;
use yummy_egui::{ParameterSlider, parameter};
pub(super) fn number(
    ui: &mut egui::Ui,
    name: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    log: bool,
) {
    parameter(ui, name, help(name), |ui| {
        ui.add(
            ParameterSlider::new(value, range)
                .logarithmic(log)
                .smallest_positive(0.000001)
                .clamping(egui::SliderClamping::Edits)
                .max_decimals(3),
        );
    });
}
pub(super) fn toggle(ui: &mut egui::Ui, name: &str, value: &mut bool) {
    parameter(ui, name, help(name), |ui| ui.checkbox(value, ""));
}
pub(super) fn list(ui: &mut egui::Ui, name: &str, value: &mut u32, options: &[(u32, &str)]) {
    parameter(ui, name, help(name), |ui| {
        egui::ComboBox::from_id_salt(name)
            .width(ui.available_width())
            .selected_text(
                options
                    .iter()
                    .find(|(v, _)| v == value)
                    .map_or("Unknown", |(_, label)| *label),
            )
            .show_ui(ui, |ui| {
                for (v, label) in options {
                    ui.selectable_value(value, *v, *label);
                }
            });
    });
}

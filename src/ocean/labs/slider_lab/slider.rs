//! Demo adapter; the interactive widget is the same one used by ocean panels.
use std::ops::RangeInclusive;
use yummy_egui::ParameterSlider;
pub(super) use yummy_egui::SliderAppearance as Appearance;

pub(super) fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    appearance: &Appearance,
    height: f32,
) -> egui::Response {
    ui.add(
        ParameterSlider::new(value, range)
            .max_decimals(3)
            .appearance(appearance)
            .height(height),
    )
}

pub(super) fn swatch(ui: &mut egui::Ui, appearance: &Appearance, height: f32, hovered: bool) {
    ui.add_space(appearance.outset + 1.0);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    appearance.paint(ui, rect, 0.5, f32::from(hovered));
    ui.add_space(appearance.outset + 1.0);
}

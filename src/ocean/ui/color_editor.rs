//! A native egui linear-RGB picker plus an independent radiometric strength.
use yummy_egui::{ParameterSlider, parameter, parameter_table};
#[derive(Clone, Debug)]
pub struct CoefficientEditor {
    last_rgb: [f32; 3],
    tint: [f32; 3],
    strength: f32,
    zero_strength: f32,
}

impl CoefficientEditor {
    pub fn new(rgb: [f32; 3], zero_strength: f32) -> Self {
        let strength = rgb.into_iter().fold(0.0_f32, f32::max);
        let tint = if strength > 0.0 {
            rgb.map(|v| v / strength)
        } else {
            [0.0; 3]
        };
        Self {
            last_rgb: rgb,
            tint,
            strength: if strength > 0.0 {
                strength
            } else {
                zero_strength
            },
            zero_strength,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, label: &str, rgb: &mut [f32; 3]) {
        // Defaults, file restore and the numerical editor can update RGB externally.
        if *rgb != self.last_rgb {
            *self = Self::new(*rgb, self.zero_strength);
        }
        ui.push_id(label, |ui| {
            yummy_egui::h3(ui, label).on_hover_text(
                "The linear RGB coefficient is color multiplied by intensity; when extinction is zero, the analytic limit is the scattering source multiplied by distance.",
            );
            let mut changed = false;
            parameter_table(ui, "coefficient table", |ui| {
                changed |= parameter(
                    ui,
                    "Color",
                    "Linear RGB color; the actual coefficient is color multiplied by intensity.",
                    |ui| ui.color_edit_button_rgb(&mut self.tint),
                )
                .changed();
                changed |= parameter(
                    ui,
                    "Intensity",
                    "Adjust coefficient magnitude independently, 0–10, using a logarithmic slider or precise input on the right. Applies immediately without resetting the simulation.",
                    |ui| {
                        ui.add(
                            ParameterSlider::new(&mut self.strength, 0.0..=10.0)
                                .logarithmic(true)
                                .smallest_positive(0.000001)
                                .clamping(egui::SliderClamping::Edits)
                                .max_decimals(6),
                        )
                    },
                )
                .changed();
            });
            if changed {
                *rgb = self.tint.map(|v| v * self.strength);
                self.last_rgb = *rgb;
            }
            ui.collapsing("Exact RGB values", |ui| {
                parameter_table(ui, "RGB table", |ui| {
                    for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
                        parameter(ui, name, "Linear RGB coefficient, 0–10; logarithmic slider with precise input on the right. Applies immediately without resetting the simulation.", |ui| {
                            ui.add(
                                ParameterSlider::new(&mut rgb[channel], 0.0..=10.0)
                                    .logarithmic(true)
                                    .smallest_positive(0.0000001)
                                    .clamping(egui::SliderClamping::Edits)
                                    .max_decimals(7),
                            );
                        });
                    }
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coefficient_decomposition_handles_hdr_tiny_and_zero_values() {
        for rgb in [[2.5, 0.125, 0.0], [0.000001, 0.000004, 0.000002], [0.0; 3]] {
            let editor = CoefficientEditor::new(rgb, 0.01);
            let restored = editor.tint.map(|v| v * editor.strength);
            for i in 0..3 {
                assert!((restored[i] - rgb[i]).abs() <= rgb[i].abs() * 1e-6);
            }
            assert!(editor.tint.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }
}

//! A native egui linear-RGB picker plus an independent radiometric strength.
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
            ui.label(egui::RichText::new(label).strong()).on_hover_text("Linear RGB coefficient = tint multiplied by strength. Zero extinction uses the analytic source * distance limit.");
            let mut changed = false;
            super::gui::parameter_table(ui, "coefficient table", |ui| {
            changed |= super::gui::parameter(ui, "Color", "Linear RGB tint; actual RGB = color × strength.", |ui| ui.color_edit_button_rgb(&mut self.tint)).changed();
            changed |= super::gui::parameter(ui, "Strength", "Independent coefficient magnitude; supports HDR values.", |ui|
                ui.add(egui::DragValue::new(&mut self.strength).range(0.0..=10.0).speed(self.zero_strength*0.02).max_decimals(6))).changed();
            });
            if changed {
                *rgb = self.tint.map(|v| v * self.strength);
                self.last_rgb = *rgb;
            }
            ui.collapsing("Precise RGB values", |ui| {
                super::gui::parameter_table(ui, "RGB table", |ui| {
                for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
                    super::gui::parameter(ui, name, "Exact linear RGB coefficient value.", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut rgb[channel])
                                .range(0.0..=10.0)
                                .speed(self.zero_strength * 0.02)
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

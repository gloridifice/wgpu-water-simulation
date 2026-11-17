use super::controls::{number, toggle};
use crate::ocean::ui::gui::Gui;
use yummy_egui::group;

impl Gui {
    pub(in crate::ocean::ui) fn surface_panel(&mut self, ui: &mut egui::Ui) {
        if self.optics.swe_enabled() {
            group(ui, "Ripple Normals", |ui| {
                number(
                    ui,
                    "Normal Detail Strength",
                    &mut self.optics.shore.normal_strength,
                    0.0..=3.0,
                    false,
                );
            });
        }
        if self.optics.beach {
            group(ui, "Sand Material", |ui| {
                number(
                    ui,
                    "Sand Texture Size (m)",
                    &mut self.optics.sand_tile_size,
                    0.25..=128.0,
                    true,
                );
                number(
                    ui,
                    "Sand Texture Rotation (degrees)",
                    &mut self.optics.sand_rotation_deg,
                    -180.0..=180.0,
                    false,
                );
            });
        } else {
            group(ui, "Seabed Calibration Material", |ui| {
                toggle(ui, "Seabed RGB Test Bands", &mut self.optics.color_bands);
                number(
                    ui,
                    "Checker Size (m)",
                    &mut self.optics.checker_size,
                    0.5..=20.0,
                    false,
                );
            });
        }
    }
}

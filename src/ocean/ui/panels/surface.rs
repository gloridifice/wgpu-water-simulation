use super::controls::{number, toggle};
use crate::ocean::ui::controls::group;
use crate::ocean::ui::gui::Gui;

impl Gui {
    pub(in crate::ocean::ui) fn surface_panel(&mut self, ui: &mut egui::Ui) {
        if self.optics.swe_enabled() {
            group(ui, "小波法线", |ui| {
                number(
                    ui,
                    "法线细节强度",
                    &mut self.optics.shore.normal_strength,
                    0.0..=3.0,
                    false,
                );
            });
        }
        if self.optics.beach {
            group(ui, "沙地材质", |ui| {
                number(
                    ui,
                    "沙地纹理尺寸（米）",
                    &mut self.optics.sand_tile_size,
                    0.25..=128.0,
                    true,
                );
                number(
                    ui,
                    "沙地纹理旋转（度）",
                    &mut self.optics.sand_rotation_deg,
                    -180.0..=180.0,
                    false,
                );
            });
        } else {
            group(ui, "海底标定材质", |ui| {
                toggle(ui, "海底 RGB 测试色带", &mut self.optics.color_bands);
                number(
                    ui,
                    "棋盘格尺寸（米）",
                    &mut self.optics.checker_size,
                    0.5..=20.0,
                    false,
                );
            });
        }
    }
}

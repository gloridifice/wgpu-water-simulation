use super::controls::{number, toggle};
use crate::ocean::camera::Camera;
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::ui::controls::group;
use crate::ocean::ui::controls::parameter;
use crate::ocean::ui::gui::Gui;

impl Gui {
    pub(in crate::ocean::ui) fn scene_panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
        length: f32,
    ) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("沙滩场景")
                .on_hover_text("载入完整沙滩预设并重置近岸历史，不只是切换地形。")
                .clicked()
            {
                self.optics = OpticsConfig::beach_scene();
                *camera = Camera::beach(length);
            }
            if ui
                .button("岩岸模型场景")
                .on_hover_text("载入完整岩岸预设并重置近岸历史。")
                .clicked()
            {
                self.optics = OpticsConfig::model_scene();
                *camera = Camera::model(length);
            }
        });
        group(ui, "地形与水底", |ui| {
            if !self.optics.model.enabled {
                toggle(ui, "倾斜沙滩", &mut self.optics.beach);
            }
            if self.optics.model.enabled {
                parameter(
                    ui,
                    "当前地形",
                    "模型优先于沙坡。模型开关及变换在模型与区域面板显式应用。",
                    |ui| ui.label("模型与补充海床"),
                );
            }
            if self.optics.beach && !self.optics.model.enabled {
                number(
                    ui,
                    "沙滩坡度（高／宽）",
                    &mut self.optics.beach_slope,
                    0.01..=0.5,
                    false,
                );
                number(
                    ui,
                    "岸线 X（米）",
                    &mut self.optics.shore_x,
                    -length..=length,
                    false,
                );
            }
            if !self.optics.beach || self.optics.model.enabled {
                number(
                    ui,
                    "海底深度（米）",
                    &mut self.optics.depth,
                    0.25..=100.0,
                    true,
                );
            }
        });
        group(ui, "局部模拟", |ui| {
            toggle(ui, "启用浅水模拟", &mut self.optics.shore.enabled);
            parameter(
                ui,
                "SWE 状态",
                "仅沙滩或模型场景且启用浅水模拟时运行。",
                |ui| {
                    ui.label(if self.optics.swe_enabled() {
                        "已启用"
                    } else {
                        "需要海岸场景与 SWE"
                    })
                },
            );
        });
    }
}

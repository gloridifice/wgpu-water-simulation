use super::controls::{number, toggle};
use crate::ocean::camera::Camera;
use crate::ocean::parameters::ocean::OceanConfig;
use crate::ocean::ui::controls::choices;
use crate::ocean::ui::controls::group;
use crate::ocean::ui::controls::parameter;
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;

impl Gui {
    pub(in crate::ocean::ui) fn ocean_panel(
        &mut self,
        ui: &mut egui::Ui,
        current: OceanConfig,
        camera: &mut Camera,
    ) -> Option<OceanConfig> {
        group(ui, "Phillips 波谱", |ui| {
            number(ui, "波谱强度 A", &mut self.draft.amplitude, 0.0..=0.1, true);
            number(
                ui,
                "波谱尺度 L（米）",
                &mut self.draft.phillips_length,
                0.1..=500.0,
                true,
            );
            let mut angle = self.draft.wind_angle.to_degrees();
            if parameter(ui, "风向（度）", help("风向（度）"), |ui| {
                ui.add(egui::Slider::new(&mut angle, -180.0..=180.0))
            })
            .changed()
            {
                self.draft.wind_angle = angle.to_radians();
            }
            parameter(ui, "随机种子", help("随机种子"), |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.draft.seed));
                    if ui.button("更换").clicked() {
                        self.draft.seed = self.draft.seed.wrapping_add(1);
                    }
                });
            });
        });
        group(ui, "范围与采样", |ui| {
            number(
                ui,
                "区块边长（米）",
                &mut self.draft.length,
                8.0..=1024.0,
                true,
            );
            parameter(ui, "FFT 分辨率", help("FFT 分辨率"), |ui| {
                egui::ComboBox::from_id_salt("fft resolution")
                    .width(ui.available_width())
                    .selected_text(format!(
                        "{} × {}",
                        self.draft.resolution, self.draft.resolution
                    ))
                    .show_ui(ui, |ui| {
                        for n in [8, 16, 32, 64, 128, 256, 512] {
                            ui.selectable_value(
                                &mut self.draft.resolution,
                                n,
                                format!("{n} × {n}"),
                            );
                        }
                    });
            });
            parameter(ui, "每轴网格数", help("每轴网格数"), |ui| {
                ui.add(egui::Slider::new(&mut self.draft.mesh_resolution, 8..=512))
            });
        });
        let mut apply = None;
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.draft != current, egui::Button::new("应用更改"))
                .clicked()
            {
                match self.draft.validate() {
                    Ok(()) => {
                        apply = Some(self.draft);
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            if ui
                .button("恢复默认")
                .on_hover_text("只恢复待应用的波谱参数，点击应用后生效。")
                .clicked()
            {
                self.draft = OceanConfig::default();
            }
        });
        ui.label(if self.draft == current {
            "参数已应用"
        } else {
            "有待应用修改"
        });
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        group(ui, "波形", |ui| {
            number(ui, "波高增益", &mut camera.gain, 0.0..=3.0, false);
            toggle(ui, "水平挤浪", &mut camera.choppy);
            if camera.choppy {
                number(ui, "挤浪强度", &mut camera.chop_amount, 0.0..=3.0, false);
            }
        });
        apply
    }

    pub(in crate::ocean::ui) fn offshore_foam_panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
    ) {
        group(ui, "远海泡沫生成与衰减", |ui| {
            number(
                ui,
                "雅可比阈值",
                &mut self.foam_settings.threshold,
                -0.5..=1.0,
                false,
            );
            number(
                ui,
                "生成速率（每秒）",
                &mut self.foam_settings.generation,
                0.0..=20.0,
                false,
            );
            number(
                ui,
                "衰减系数（每秒）",
                &mut self.foam_settings.decay,
                0.0..=5.0,
                false,
            );
        });
        group(ui, "远海诊断", |ui| {
            parameter(ui, "泡沫视图", help("泡沫视图"), |ui| {
                choices(
                    ui,
                    &mut camera.foam_debug,
                    &[(0, "正常"), (1, "雅可比"), (2, "覆盖率")],
                )
            });
        });
        if ui
            .button("清空泡沫历史")
            .on_hover_text("沿用全局历史重置，同时重置浅水、泡沫和沙地历史。")
            .clicked()
        {
            self.clear_foam = true;
        }
    }
}

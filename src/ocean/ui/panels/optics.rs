use super::controls::{list, number, toggle};
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::parameters::optics::VIEW_NAMES;
use crate::ocean::render::skylight::incident_reference;
use crate::ocean::ui::controls::parameter;
use crate::ocean::ui::controls::parameter_table;
use crate::ocean::ui::controls::{PanelButton, choices, group};
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;

impl Gui {
    pub(in crate::ocean::ui) fn optics_panel(&mut self, ui: &mut egui::Ui) {
        self.sigma_editor
            .ui(ui, "消光系数（每米）", &mut self.optics.sigma_t);
        self.source_editor
            .ui(ui, "手动散射源（辐亮度／米）", &mut self.optics.source);
        group(ui, "天空照明与反射", |ui| {
            parameter(
                ui,
                "天空颜色（线性 RGB）",
                help("天空颜色（线性 RGB）"),
                |ui| ui.color_edit_button_rgb(&mut self.optics.sky.color),
            );
            number(
                ui,
                "天空强度",
                &mut self.optics.sky.intensity,
                0.0..=20.0,
                true,
            );
            toggle(ui, "天空反射与高光", &mut self.optics.reflections);
        });
        group(ui, "天空散射", |ui| {
            toggle(ui, "启用天空散射", &mut self.optics.sky.enabled);
            if self.optics.sky.enabled {
                parameter(
                    ui,
                    "散射反照率 RGB",
                    help("散射反照率 RGB"),
                    |ui| ui.color_edit_button_rgb(&mut self.optics.sky.albedo),
                );
                parameter(ui, "天空积分步数", help("天空积分步数"), |ui| {
                    choices(
                        ui,
                        &mut self.optics.sky.steps,
                        &[(8, "8"), (16, "16"), (32, "32")],
                    )
                });
            }
        });
        ui.collapsing("天空 RGB 精确值", |ui| {
            parameter_table(ui, "sky exact", |ui| {
                for (c, name) in ["R", "G", "B"].iter().enumerate() {
                    parameter(
                        ui,
                        &format!("颜色 {name}"),
                        "天空颜色的线性 RGB 分量，0～1，实时生效。",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.optics.sky.color[c])
                                    .range(0.0..=1.0)
                                    .speed(0.001),
                            )
                        },
                    );
                    if self.optics.sky.enabled {
                        parameter(
                            ui,
                            &format!("反照率 {name}"),
                            "散射反照率，0～1，实时生效。",
                            |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut self.optics.sky.albedo[c])
                                        .range(0.0..=1.0)
                                        .speed(0.001),
                                )
                            },
                        );
                    }
                }
            });
        });
        if self.optics.sky.enabled {
            ui.collapsing("天空散射源随深度变化", |ui| {
                depth_graph(ui, &self.optics)
            });
        }
        group(ui, "光学诊断", |ui| {
            let names: Vec<_> = VIEW_NAMES
                .iter()
                .enumerate()
                .map(|(i, n)| (i as u32, *n))
                .collect();
            list(ui, "水体调试视图", &mut self.optics.view, &names);
            if self.optics.view == 1 {
                number(
                    ui,
                    "光程白色参考值（米）",
                    &mut self.optics.path_range,
                    1.0..=300.0,
                    true,
                );
            }
        });
        if ui
            .add(PanelButton::new("重置光学参数"))
            .on_hover_text(
                "沿用原操作：恢复整个光学配置（含场景、SWE、材质、浪花），会重置相应模拟历史。",
            )
            .clicked()
        {
            self.optics = OpticsConfig::default();
        }
    }
}

pub fn depth_graph(ui: &mut egui::Ui, optics: &crate::ocean::parameters::optics::OpticsConfig) {
    let depth = optics.depth;
    let colors = [
        egui::Color32::LIGHT_RED,
        egui::Color32::LIGHT_GREEN,
        egui::Color32::LIGHT_BLUE,
    ];
    let source = |z: f32, c: usize| {
        if !optics.sky.enabled {
            return 0.0;
        }
        optics.sigma_t[c]
            * optics.sky.albedo[c]
            * incident_reference(&optics.sky, f64::from(z * optics.sigma_t[c]), c, 32) as f32
    };
    let peak = (0..3).map(|c| source(0.0, c)).fold(0.0_f32, f32::max);
    parameter_table(ui, "graph axes", |ui| {
        parameter(ui, "深度范围", "图表横轴，单位为米。", |ui| {
            ui.label(format!("0 .. {depth:.2} m"))
        });
        parameter(
            ui,
            "散射源范围",
            "局部天空散射源，单位为每米辐亮度，尚未向观察点积分。",
            |ui| ui.label(format!("0 .. {peak:.5}")),
        );
    });
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, egui::Color32::from_gray(18));
    for (c, color) in colors.into_iter().enumerate() {
        let points = (0..65)
            .map(|i| {
                let u = i as f32 / 64.0;
                egui::pos2(
                    rect.left() + u * rect.width(),
                    rect.bottom() - source(depth * u, c) / peak.max(1e-20) * rect.height(),
                )
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
    }
}

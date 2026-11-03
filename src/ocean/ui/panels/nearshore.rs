use super::controls::{list, number, toggle};
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;
use yummy_egui::{PanelButton, action_row, choices, group, parameter};

impl Gui {
    pub(in crate::ocean::ui) fn shore_panel(&mut self, ui: &mut egui::Ui) {
        let available = self.optics.swe_enabled();
        group(ui, "浅水模拟", |ui| {
            toggle(ui, "启用浅水模拟", &mut self.optics.shore.enabled);
            if !available {
                parameter(
                    ui,
                    "运行条件",
                    "在场景工作区选择沙滩或启用模型后，开启浅水模拟。",
                    |ui| ui.label("需要海岸场景与 SWE"),
                );
            }
        });
        if !self.optics.swe_enabled() {
            return;
        }
        let shore = &mut self.optics.shore;
        group(ui, "求解与采样", |ui| {
            parameter(
                ui,
                "每轴网格数",
                "SWE 分辨率 64～1024，切换即时重建并重置近岸历史，范围不变。",
                |ui| {
                    egui::ComboBox::from_id_salt("swe resolution")
                        .width(ui.available_width())
                        .selected_text(format!("{} × {}", shore.resolution, shore.resolution))
                        .show_ui(ui, |ui| {
                            for n in crate::ocean::simulation::nearshore::RESOLUTIONS {
                                ui.selectable_value(&mut shore.resolution, n, format!("{n} × {n}"));
                            }
                        });
                },
            );
            parameter(
                ui,
                "网格间距 X/Z（米）",
                help("网格间距 X/Z（米）"),
                |ui| {
                    ui.label(format!(
                        "{:.3} / {:.3}",
                        self.swe_domain[2] / shore.resolution as f32,
                        self.swe_domain[3] / shore.resolution as f32
                    ))
                },
            );
            parameter(ui, "SWE 求解器", help("SWE 求解器"), |ui| {
                choices(ui, &mut shore.order, &[(1, "一阶"), (2, "二阶")])
            });
        });
        group(ui, "FFT → SWE 输入与混合", |ui| {
            number(
                ui,
                "FFT → SWE 增益",
                &mut shore.input_gain,
                0.0..=3.0,
                false,
            );
            number(
                ui,
                "FFT 混合水深（米）",
                &mut shore.breaking_depth,
                0.5..=5.0,
                false,
            );
            toggle(ui, "过滤短波", &mut shore.filter_enabled);
            if shore.filter_enabled {
                number(
                    ui,
                    "截止波长（米）",
                    &mut shore.filter_cutoff,
                    0.0..=128.0,
                    false,
                );
                number(
                    ui,
                    "过滤过渡宽度（米）",
                    &mut shore.filter_transition,
                    0.0..=128.0,
                    false,
                );
            }
        });
        let [default_filter, short_filter] = action_row(
            ui,
            [
                PanelButton::new("默认 12～24 米"),
                PanelButton::new("3～6 米"),
            ],
        );
        if default_filter.clicked() {
            shore.filter_enabled = true;
            shore.filter_cutoff = 12.0;
            shore.filter_transition = 12.0;
        }
        if short_filter.clicked() {
            shore.filter_enabled = true;
            shore.filter_cutoff = 3.0;
            shore.filter_transition = 3.0;
        }
        group(ui, "沙地历史与诊断", |ui| {
            if self.optics.beach {
                number(
                    ui,
                    "沙地干燥时间（秒）",
                    &mut shore.drying_time,
                    1.0..=120.0,
                    false,
                );
            }
            list(
                ui,
                "浅水调试视图",
                &mut shore.view,
                &crate::ocean::simulation::nearshore::VIEWS,
            );
        });
        if ui.add(PanelButton::new("重置浅水、泡沫和沙地")).clicked() {
            self.clear_foam = true;
        }
    }

    pub(in crate::ocean::ui) fn shore_foam_panel(&mut self, ui: &mut egui::Ui) {
        let shore = &mut self.optics.shore;
        group(ui, "泡沫生成", |ui| {
            toggle(ui, "启用泡沫发射", &mut shore.foam_emission_enabled);
        });
        if shore.foam_emission_enabled {
            group(ui, "前沿生成", |ui| {
                toggle(ui, "压缩脊线实验", &mut shore.foam_ridge_enabled);
                number(
                    ui,
                    "前沿生成速率（每秒）",
                    &mut shore.foam_front_generation,
                    0.0..=30.0,
                    false,
                );
                if shore.foam_ridge_enabled {
                    number(
                        ui,
                        "窄带宽度（米）",
                        &mut shore.foam_ridge_width,
                        0.05..=10.0,
                        true,
                    );
                    number(
                        ui,
                        "峰值相对显著度",
                        &mut shore.foam_ridge_prominence,
                        0.0..=1.0,
                        false,
                    );
                } else {
                    number(
                        ui,
                        "前沿采样距离（米）",
                        &mut shore.foam_front_distance,
                        0.05..=10.0,
                        false,
                    );
                }
                number(
                    ui,
                    "速度变化起始阈值（每秒）",
                    &mut shore.foam_velocity_threshold,
                    0.0..=20.0,
                    false,
                );
                number(
                    ui,
                    "速度变化过渡范围（每秒）",
                    &mut shore.foam_velocity_transition,
                    0.0..=20.0,
                    false,
                );
            });
            if !shore.foam_ridge_enabled {
                group(ui, "背景生成", |ui| {
                    number(
                        ui,
                        "背景生成速率（每秒）",
                        &mut shore.generation,
                        0.0..=15.0,
                        false,
                    );
                    number(
                        ui,
                        "背景生成倍率",
                        &mut shore.foam_background_multiplier,
                        0.0..=1.0,
                        false,
                    );
                    number(
                        ui,
                        "背景生成起始阈值",
                        &mut shore.foam_generation_threshold,
                        0.0..=2.0,
                        false,
                    );
                    number(
                        ui,
                        "背景生成过渡范围",
                        &mut shore.foam_generation_transition,
                        0.0..=2.0,
                        false,
                    );
                    number(
                        ui,
                        "汇聚增强阈值（每秒）",
                        &mut shore.foam_compression_threshold,
                        0.0..=4.0,
                        false,
                    );
                });
            }
        }
        group(ui, "浓度模式与寿命", |ui| {
            toggle(ui, "双浓度实验", &mut shore.foam_dual_phase);
            if shore.foam_dual_phase {
                number(
                    ui,
                    "新鲜半衰期（秒）",
                    &mut shore.foam_fresh_half_life,
                    0.1..=60.0,
                    true,
                );
                number(
                    ui,
                    "残余半衰期（秒）",
                    &mut shore.foam_residual_half_life,
                    0.1..=120.0,
                    true,
                );
                number(
                    ui,
                    "退出转残余比例",
                    &mut shore.foam_conversion,
                    0.0..=1.0,
                    false,
                );
            } else {
                number(
                    ui,
                    "水上泡沫衰减（每秒）",
                    &mut shore.decay,
                    0.0..=4.0,
                    false,
                );
            }
        });
        if !shore.foam_dual_phase {
            group(ui, "泡沫输送与水流拖曳", |ui| {
                if shore.order == 2 {
                    parameter(ui, "泡沫空间输送", help("泡沫空间输送"), |ui| {
                        choices(
                            ui,
                            &mut shore.foam_transport_order,
                            &[(1, "一阶"), (2, "二阶")],
                        )
                    });
                }
                toggle(ui, "启用水流拖曳", &mut shore.foam_drag_enabled);
                if shore.foam_drag_enabled {
                    number(ui, "水流跟随倍率", &mut shore.foam_follow, 0.0..=1.0, false);
                    number(
                        ui,
                        "速度响应时间（秒）",
                        &mut shore.foam_response_time,
                        0.0..=20.0,
                        false,
                    );
                }
                number(
                    ui,
                    "新生泡沫速度倍率",
                    &mut shore.foam_birth_speed,
                    0.0..=1.0,
                    false,
                );
                parameter(ui, "物理参考起点", help("物理参考起点"), |ui| {
                    if ui.add(PanelButton::new("应用推荐值")).clicked() {
                        shore.apply_foam_reference();
                    }
                });
            });
        }
        group(ui, "泡沫密度诊断", |ui| {
            number(
                ui,
                "密度颜色参考值",
                &mut shore.foam_density_reference,
                0.01..=20.0,
                true,
            );
            list(
                ui,
                "浅水调试视图",
                &mut shore.view,
                &crate::ocean::simulation::nearshore::VIEWS,
            );
        });
        let [debug, reset] = action_row(
            ui,
            [
                PanelButton::new("密度调试：低绿／高红"),
                PanelButton::new("重置浅水历史"),
            ],
        );
        if debug.clicked() {
            shore.view = 6;
        }
        if reset.clicked() {
            self.clear_foam = true;
        }
    }
}

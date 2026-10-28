//! Parameter ownership follows the five workspaces; all values remain shared.
use super::{
    gui::{Gui, choices, group, parameter, parameter_table},
    optics::{OpticsConfig, VIEW_NAMES},
    panel_help::help,
    spectrum::OceanConfig,
    viewer::Camera,
};
use std::ops::RangeInclusive;

fn number(ui: &mut egui::Ui, name: &str, value: &mut f32, range: RangeInclusive<f32>, log: bool) {
    parameter(ui, name, help(name), |ui| {
        ui.add(
            egui::Slider::new(value, range)
                .logarithmic(log)
                .smallest_positive(0.000001)
                .clamping(egui::SliderClamping::Edits)
                .max_decimals(3),
        );
    });
}
fn toggle(ui: &mut egui::Ui, name: &str, value: &mut bool) {
    parameter(ui, name, help(name), |ui| ui.checkbox(value, ""));
}
fn list(ui: &mut egui::Ui, name: &str, value: &mut u32, options: &[(u32, &str)]) {
    parameter(ui, name, help(name), |ui| {
        egui::ComboBox::from_id_salt(name)
            .width(ui.available_width())
            .selected_text(
                options
                    .iter()
                    .find(|(v, _)| v == value)
                    .map_or("未知", |(_, label)| *label),
            )
            .show_ui(ui, |ui| {
                for (v, label) in options {
                    ui.selectable_value(value, *v, *label);
                }
            });
    });
}

impl Gui {
    pub(super) fn scene_panel(&mut self, ui: &mut egui::Ui, camera: &mut Camera, length: f32) {
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

    pub(super) fn ocean_panel(
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

    pub(super) fn offshore_foam_panel(&mut self, ui: &mut egui::Ui, camera: &mut Camera) {
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

    pub(super) fn shore_panel(&mut self, ui: &mut egui::Ui) {
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
                            for n in super::shore::RESOLUTIONS {
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
        ui.horizontal_wrapped(|ui| {
            if ui.button("默认 12～24 米").clicked() {
                shore.filter_enabled = true;
                shore.filter_cutoff = 12.0;
                shore.filter_transition = 12.0;
            }
            if ui.button("3～6 米").clicked() {
                shore.filter_enabled = true;
                shore.filter_cutoff = 3.0;
                shore.filter_transition = 3.0;
            }
        });
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
            list(ui, "浅水调试视图", &mut shore.view, &super::shore::VIEWS);
        });
        if ui.button("重置浅水、泡沫和沙地").clicked() {
            self.clear_foam = true;
        }
    }

    pub(super) fn shore_foam_panel(&mut self, ui: &mut egui::Ui) {
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
                    if ui.button("应用推荐值").clicked() {
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
            list(ui, "浅水调试视图", &mut shore.view, &super::shore::VIEWS);
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("密度调试：低绿／高红").clicked() {
                shore.view = 6;
            }
            if ui.button("重置浅水历史").clicked() {
                self.clear_foam = true;
            }
        });
    }

    pub(super) fn foam_material_panel(&mut self, ui: &mut egui::Ui, camera: &mut Camera) {
        group(ui, "泡沫显示", |ui| {
            toggle(ui, "显示泡沫", &mut camera.foam);
        });
        if !camera.foam {
            return;
        }
        let swe = self.optics.swe_enabled();
        let shore = &mut self.optics.shore;
        group(ui, "共用泡沫颜色", |ui| {
            parameter(ui, "水上泡沫", help("水上泡沫"), |ui| {
                ui.color_edit_button_rgb(&mut shore.water_foam_color)
            });
        });
        if ui.button("恢复默认泡沫颜色").clicked() {
            shore.water_foam_color = [0.87, 0.92, 0.90];
        }
        if !swe {
            return;
        }
        group(ui, "近岸泡沫材质", |ui| {
            if !shore.foam_dual_phase {
                toggle(ui, "原型孔洞外观", &mut shore.foam_prototype);
            }
            if !shore.foam_dual_phase && !shore.foam_prototype {
                let names: Vec<_> = super::shore::FOAM_TEXTURE_NAMES
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i as u32, *n))
                    .collect();
                list(ui, "纹理来源", &mut shore.foam_texture, &names);
            }
            parameter(
                ui,
                "坐标重置周期（秒）",
                help("坐标重置周期（秒）"),
                |ui| {
                    ui.add(
                        egui::Slider::new(&mut shore.foam_texture_period, 0.0..=60.0).step_by(0.25),
                    )
                },
            );
            if shore.foam_texture > 0 && !shore.foam_prototype && !shore.foam_dual_phase {
                number(
                    ui,
                    "纹理尺寸（米）",
                    &mut shore.foam_texture_size,
                    0.25..=64.0,
                    true,
                );
                number(
                    ui,
                    "泡沫微法线强度",
                    &mut shore.foam_micro_normal,
                    0.0..=3.0,
                    false,
                );
                number(
                    ui,
                    "泡沫粗糙度",
                    &mut shore.foam_roughness,
                    0.15..=1.0,
                    false,
                );
            }
            if shore.foam_texture > 0 || shore.foam_prototype || shore.foam_dual_phase {
                number(
                    ui,
                    "不透明度密度尺度",
                    &mut shore.foam_opacity_scale,
                    0.01..=20.0,
                    true,
                );
                let names: Vec<_> = super::shore::FOAM_MATERIAL_VIEWS
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i as u32, *n))
                    .collect();
                list(ui, "材质调试", &mut shore.foam_material_view, &names);
            }
            number(
                ui,
                "密度颜色参考值",
                &mut shore.foam_density_reference,
                0.01..=20.0,
                true,
            );
        });
        group(ui, "泡沫高度（仅外观）", |ui| {
            number(
                ui,
                "最大高度（米）",
                &mut shore.foam_height_max,
                0.0..=1.0,
                false,
            );
            if shore.foam_height_max > 0.0 {
                number(
                    ui,
                    "起始密度",
                    &mut shore.foam_height_threshold,
                    0.0..=20.0,
                    false,
                );
                number(
                    ui,
                    "密度过渡范围",
                    &mut shore.foam_height_transition,
                    0.001..=20.0,
                    true,
                );
            }
        });
    }

    pub(super) fn surface_panel(&mut self, ui: &mut egui::Ui) {
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

    pub(super) fn optics_panel(&mut self, ui: &mut egui::Ui) {
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
                super::skylight::depth_graph(ui, &self.optics)
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
            .button("重置光学参数")
            .on_hover_text(
                "沿用原操作：恢复整个光学配置（含场景、SWE、材质、浪花），会重置相应模拟历史。",
            )
            .clicked()
        {
            self.optics = OpticsConfig::default();
        }
    }
}

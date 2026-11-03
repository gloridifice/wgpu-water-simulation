//! Apply-based model/domain editing: changing a draft never resets live water.
use crate::ocean::camera::Camera;
use crate::ocean::parameters::model::ModelConfig;
use crate::ocean::parameters::optics::OpticsConfig;
use yummy_egui::{PanelButton, action_row, parameter, parameter_table};
#[derive(Default)]
pub struct ModelPanel {
    draft: ModelConfig,
    applied: Option<ModelConfig>,
    pub source_bounds: Option<[[f32; 3]; 2]>,
    pub bed_range: Option<[f32; 2]>,
    pub error: Option<String>,
}
impl ModelPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        optics: &mut OpticsConfig,
        camera: &mut Camera,
        length: f32,
    ) {
        if self.applied != Some(optics.model) {
            self.draft = optics.model;
            self.applied = Some(optics.model);
        }
        if ui.add(PanelButton::new("载入岩岸模型场景")).clicked() {
            *optics = OpticsConfig::model_scene();
            *camera = Camera::model(length);
            self.draft = optics.model;
            self.applied = Some(optics.model);
            self.error = None;
        }
        yummy_egui::h3(ui, "Challaborough Pillbox");
        parameter_table(ui, "model asset status", |ui| {
            parameter(
                ui,
                "高度场状态",
                "从 assets/models/challaborough_pillbox.sweh 加载预烘焙高度和覆盖标记。缺失时先运行 terrain-bake；不会在帧循环中遍历三角形。",
                |ui| {
                    ui.label(if self.source_bounds.is_some() {
                        "已加载磁盘资产"
                    } else {
                        "尚未加载"
                    })
                },
            );
            parameter(
                ui,
                "启用模型",
                "应用后切换模型场景并重置近岸历史；关闭后恢复原沙坡／平面。",
                |ui| ui.checkbox(&mut self.draft.enabled, ""),
            );
        });
        if self.draft.enabled {
            yummy_egui::h3(ui, "模型变换与水位");
            parameter_table(ui, "model transform", |ui| {
                for (a, name) in ["位置 X（米）", "位置 Y（米）", "位置 Z（米）"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "模型平移，-2000～2000 米。Y 与海平面使用同一高程基准；应用后重采样床面并重置近岸历史。",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.draft.position[a])
                                    .range(-2000.0..=2000.0)
                                    .speed(0.1),
                            )
                        },
                    );
                }
                parameter(
                    ui,
                    "统一比例",
                    "米／模型单位，0.01～100；扫描比例未经实测校准。应用后重采样，不重新烘焙原资产。",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.draft.scale)
                                .range(0.01..=100.0)
                                .speed(0.01),
                        )
                    },
                );
                parameter(
                    ui,
                    "旋转 Y（度）",
                    "绕竖直轴旋转，-180～180 度。应用后重采样并重置；俯仰／翻滚需要重新烘焙，不在此提供。",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.draft.yaw_deg)
                                .range(-180.0..=180.0)
                                .speed(0.5),
                        )
                    },
                );
                parameter(
                    ui,
                    "海平面高程（米）",
                    "-2000～2000 米。渲染和求解以此水位为 Y=0；提高此值相当于涨潮，应用后重置，不是动态潮汐输入。",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.draft.sea_level)
                                .range(-2000.0..=2000.0)
                                .speed(0.05),
                        )
                    },
                );
            });
            yummy_egui::h3(ui, "SWE 包围盒");
            parameter_table(ui, "model domain", |ui| {
                for (a, name) in ["中心 X（米）", "中心 Y（米）", "中心 Z（米）"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "-2000～2000 米。XZ 决定计算域，应用后重建；Y 仅显示和范围检查，不移动水面、不裁切地形。",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.draft.center[a])
                                    .range(-2000.0..=2000.0)
                                    .speed(0.5),
                            )
                        },
                    );
                }
                for (a, name) in ["尺寸 X（米）", "尺寸 Y（米）", "尺寸 Z（米）"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "4～2048 米。XZ 决定计算域，Y 仅可视化，顶部不是封闭顶盖。应用后 XZ 修改会清空近岸历史；Y 修改不会。",
                        |ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.draft.size[a])
                                    .range(4.0..=2048.0)
                                    .speed(0.5),
                            )
                        },
                    );
                }
                parameter(
                    ui,
                    "显示包围盒",
                    "应用后显示橙色三维线框。仅外观，不重置模拟。",
                    |ui| ui.checkbox(&mut self.draft.show_bounds, ""),
                );
                parameter(
                    ui,
                    "待应用格距 X/Z",
                    "单位米；每轴网格数在 SWE 面板选择。重要通道建议至少跨越 3～5 格，源高度场太粗时提高此分辨率不会恢复细节。",
                    |ui| {
                        ui.label(format!(
                            "{:.3} / {:.3}",
                            self.draft.size[0] / optics.shore.resolution as f32,
                            self.draft.size[2] / optics.shore.resolution as f32
                        ))
                    },
                );
            });
            if ui
                .add_enabled(
                    self.source_bounds.is_some(),
                    PanelButton::new("按模型范围自动覆盖"),
                )
                .clicked()
            {
                self.draft.fit(self.source_bounds.unwrap());
            }
            yummy_egui::h3(ui, "边界与补充海床");
            parameter_table(ui, "model boundaries", |ui| {
                for (a, name) in ["−X 开放", "+X 开放", "−Z 开放", "+Z 开放"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "开启：FFT 来浪与出流边界，配合松弛带；关闭：反射墙。应用后重置近岸历史。FFT 是唯一外部波浪源。",
                        |ui| ui.checkbox(&mut self.draft.open_edges[a], ""),
                    );
                }
                parameter(
                    ui,
                    "松弛带宽（米）",
                    "0.5～512 米，必须小于 X/Z 最小尺寸的一半。需要根据实际波长检查反射；应用后重置。",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.draft.sponge_width)
                                .range(0.5..=512.0)
                                .speed(0.5),
                        )
                    },
                );
                parameter(
                    ui,
                    "海床衔接宽度（米）",
                    "0.5～256 米。扫描边缘在此距离内平滑过渡到场景与地形面板的海底深度；属于人为补充地形，不是实测海底。应用后重建并重置。应留在计算域和松弛带以内。",
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.draft.extension_width)
                                .range(0.5..=256.0)
                                .speed(0.5),
                        )
                    },
                );
            });
        }
        let [apply, cancel] = action_row(
            ui,
            [
                PanelButton::new("应用模型与区域"),
                PanelButton::new("撤销待应用修改"),
            ],
        );
        if apply.clicked() {
            match self.draft.validate() {
                Ok(()) => {
                    optics.model = self.draft;
                    self.applied = Some(self.draft);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        if cancel.clicked() {
            self.draft = optics.model;
            self.error = None;
        }
        if self.draft != optics.model {
            ui.label("存在待应用修改");
        }
        if let Some(range) = self.bed_range {
            let low = optics.model.center[1] - optics.model.size[1] * 0.5 - optics.model.sea_level;
            let high = optics.model.center[1] + optics.model.size[1] * 0.5 - optics.model.sea_level;
            if range[0] < low || range[1] > high {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "床面超出包围盒 Y 范围：仅提示，未裁切。",
                );
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
}

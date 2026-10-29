//! Isolated, non-persistent slider appearance experiment.
mod slider;
mod window;

pub use window::run;

use charming_egui::{group, parameter, parameter_table};
use slider::Appearance;

struct Demo {
    appearance: Appearance,
    height: f32,
    values: [f32; 5],
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            appearance: Appearance {
                background_radius: Some(2.0),
                marker_radius: Some(2.0),
                ..Default::default()
            },
            height: 24.0,
            values: [0.55, 1.2, 15.0, 0.0, 1.0],
        }
    }
}

impl Demo {
    fn ui(&mut self, context: &egui::Context) {
        egui::TopBottomPanel::top("title").show(context, |ui| {
            ui.horizontal_wrapped(|ui| {
                charming_egui::h3(ui, "矩形滑块 · 外观实验");
                ui.separator();
                ui.label("独立窗口 · 不读写配置 · 不运行海洋模拟");
            });
        });
        egui::SidePanel::left("appearance")
            .default_width(340.0)
            .width_range(300.0..=420.0)
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        egui::CentralPanel::default().show(context, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.preview(ui));
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        charming_egui::h3(ui, "外观参数（实时生效）");
        let style = &mut self.appearance;
        group(ui, "背景与数值框", |ui| {
            parameter(
                ui,
                "控件高度",
                "20～40 pt（逻辑像素）。矩形底色匹配右侧数值框的实际高度；字体最小高度仍受 egui 约束。实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut self.height, 20.0..=40.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "数值框宽度",
                "48～100 pt。只调整右侧输入框宽度，轨道使用控制列剩余宽度。实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.value_width, 48.0..=100.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "两端留白",
                "1～24 pt。最小／最大值位置处，悬浮竖条最外缘到背景左右边缘的间距；常态间距略大。完整数值范围不变，实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.edge_padding, 1.0..=24.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "背景圆角",
                "0～12 pt。矩形背景的圆角半径，0 为直角；只改变外观，实时生效。",
                |ui| {
                    ui.add(
                        egui::Slider::new(style.background_radius.get_or_insert(2.0), 0.0..=12.0)
                            .suffix(" pt"),
                    );
                },
            );
            parameter(
                ui,
                "矩形底色",
                "整条轨道的统一不透明颜色，不显示进度填充。实时生效。",
                |ui| color(ui, &mut style.background),
            );
        });
        group(ui, "常态竖条", |ui| {
            parameter(
                ui,
                "宽度",
                "1～6 pt，细实心竖条。悬浮宽度始终至少比常态宽 1 pt。实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.width, 1.0..=6.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "竖条圆角",
                "0～6 pt。常态和悬浮共用的圆角半径，实际不超过当时宽高较小值的一半；默认两端圆润。实时生效。",
                |ui| {
                    ui.add(
                        egui::Slider::new(style.marker_radius.get_or_insert(2.0), 0.0..=6.0)
                            .suffix(" pt"),
                    );
                },
            );
            parameter(
                ui,
                "上下各内缩",
                "1～8 pt，每侧相对背景边缘的内缩量；常态高度 = 背景高度 − 2 × 内缩。实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.inset, 1.0..=8.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "颜色",
                "常态竖条的不透明颜色。实时生效；建议亮度低于悬浮颜色。",
                |ui| color(ui, &mut style.marker),
            );
        });
        style.hover_width = style.hover_width.max(style.width + 1.0);
        group(ui, "悬浮／拖动竖条", |ui| {
            parameter(
                ui,
                "宽度",
                "常态宽度 + 1～12 pt。悬浮时增粗；两端为最大宽度预留空间，避免截断。实时生效。",
                |ui| {
                    ui.add(
                        egui::Slider::new(&mut style.hover_width, style.width + 1.0..=12.0)
                            .suffix(" pt"),
                    );
                },
            );
            parameter(
                ui,
                "上下各外延",
                "0.5～6 pt，每侧伸出背景的高度；悬浮高度 = 背景高度 + 2 × 外延。行间预留空间，不推挤相邻控件。实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.outset, 0.5..=6.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "颜色",
                "悬浮和拖动期间的竖条颜色，默认比常态更亮。可自由调色对照，颜色不自动校正。实时生效。",
                |ui| color(ui, &mut style.hover_marker),
            );
            parameter(
                ui,
                "过渡时长",
                "0～300 毫秒（现实时间）。宽度、高度和颜色同步过渡；0 为立即切换。实时生效。",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.animation_ms, 0.0..=300.0).suffix(" ms"));
                },
            );
        });
        ui.add_space(12.0);
        if ui.button("恢复默认外观").clicked() {
            let defaults = Self::default();
            self.appearance = defaults.appearance;
            self.height = defaults.height;
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        charming_egui::h3(ui, "交互预览");
        parameter_table(ui, "preview sliders", |ui| {
            for (i, (name, range)) in [
                ("混合比例", 0.0..=1.0),
                ("波幅（米）", 0.0..=3.0),
                ("角度（度）", -180.0..=180.0),
                ("最小值端点", 0.0..=1.0),
                ("最大值端点", 0.0..=1.0),
            ]
            .into_iter()
            .enumerate()
            {
                parameter(
                    ui,
                    name,
                    "仅为外观示例，不控制模拟；与正式参数面板共用控件。点击轨道定位、拖动调整，或在右侧输入数值；Tab 聚焦后使用方向键微调，保留 egui 原生交互。",
                    |ui| {
                        slider::slider(
                            ui,
                            &mut self.values[i],
                            range,
                            &self.appearance,
                            self.height,
                        );
                    },
                );
            }
        });
        ui.add_space(18.0);
        charming_egui::h3(ui, "状态对照（静态示意，不可拖动）");
        parameter_table(ui, "state comparison", |ui| {
            parameter(
                ui,
                "常态",
                "固定显示常态，便于与悬浮样式直接比较。与上方真实交互使用同一绘制方法。",
                |ui| {
                    slider::swatch(ui, &self.appearance, self.height, false);
                },
            );
            parameter(
                ui,
                "悬浮／拖动",
                "固定显示悬浮终态，不依赖鼠标位置；实际触发请使用上方交互预览。",
                |ui| {
                    slider::swatch(ui, &self.appearance, self.height, true);
                },
            );
        });
        ui.add_space(18.0);
        charming_egui::h3(ui, "原生滑块对照（同一个混合比例）");
        parameter_table(ui, "native comparison", |ui| {
            parameter(
                ui,
                "混合比例",
                "原生 egui Slider，与上方第一行共享数值，主题保持不变。",
                |ui| {
                    ui.add(egui::Slider::new(&mut self.values[0], 0.0..=1.0));
                },
            );
        });
    }
}

fn color(ui: &mut egui::Ui, value: &mut egui::Color32) {
    let mut rgb = [value.r(), value.g(), value.b()];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *value = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    }
}

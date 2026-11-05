use crate::ocean::ui::gui::Gui;
use yummy_egui::{ParameterSlider, group, parameter};

impl Gui {
    pub(super) fn post_process_controls(&mut self, ui: &mut egui::Ui) {
        group(ui, "HDR 与 Bloom", |ui| {
            let config = &mut self.optics.post_process;
            parameter(
                ui,
                "启用 HDR 后处理",
                "启用线性 HDR 高光泛光和 ACES 近似色调映射，输出仍为 SDR。默认关闭以保留旧外观；调试视图自动跳过，UI 不参与泛光。实时生效，不重置模拟。",
                |ui| ui.checkbox(&mut config.enabled, ""),
            );
            if !config.enabled {
                return;
            }
            for (name, help, value, range) in [
                (
                    "曝光补偿（档）",
                    "在高光提取前将线性颜色乘以 2 的档数次方；每增加 1 档亮度翻倍。范围 -8～8，默认 0，实时生效。",
                    &mut config.exposure,
                    -8.0..=8.0,
                ),
                (
                    "Bloom 强度",
                    "泛光叠加强度，无量纲，范围 0～2，默认 0.1。设为 0 仅保留曝光和色调映射，并跳过模糊 pass。实时生效。",
                    &mut config.strength,
                    0.0..=2.0,
                ),
                (
                    "高光阈值（线性）",
                    "按曝光后的线性 RGB 最大分量提取高光，不是 sRGB 值。范围 0～20，默认 1；降低会让更多区域泛光。实时生效。",
                    &mut config.threshold,
                    0.0..=20.0,
                ),
                (
                    "阈值柔和度",
                    "过渡半宽占高光阈值的比例，无量纲，范围 0～1，默认 0.5。0 为硬阈值；非零允许阈值附近柔和过渡。实时生效。",
                    &mut config.soft_knee,
                    0.0..=1.0,
                ),
                (
                    "泛光半径",
                    "多尺度上采样滤波半径，单位为各层纹理像素，范围 0.5～2，默认 1。不是世界空间米数；随画面分辨率变化。实时生效。",
                    &mut config.radius,
                    0.5..=2.0,
                ),
            ] {
                parameter(ui, name, help, |ui| {
                    ui.add(ParameterSlider::new(value, range))
                });
            }
        });
    }
}

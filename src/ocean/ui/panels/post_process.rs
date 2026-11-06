use crate::ocean::ui::gui::Gui;
use yummy_egui::{ParameterSlider, group, parameter};

impl Gui {
    pub(super) fn post_process_controls(&mut self, ui: &mut egui::Ui) {
        group(ui, "HDR 与 Bloom", |ui| {
            let config = &mut self.optics.post_process;
            parameter(
                ui,
                "启用 HDR 后处理",
                "后处理总开关，默认关闭。开启后可独立控制 AgX 映射与 Bloom；输出仍为 SDR。调试视图自动旁路，UI 不参与处理。实时生效，不重置模拟；需手动保存。",
                |ui| ui.checkbox(&mut config.enabled, ""),
            );
            if !config.enabled {
                return;
            }
            parameter(
                ui,
                "AgX 色调映射",
                "默认开启。通用线性 Rec.709 HDR→SDR 映射，压缩亮部并使极亮颜色趋向白色；不针对海水调色。关闭后不压缩高光，SDR 输出可能截断。实时生效，不重置模拟；需手动保存。",
                |ui| ui.checkbox(&mut config.tonemapping_enabled, ""),
            );
            parameter(
                ui,
                "曝光补偿（档）",
                "在高光提取和映射之前将线性颜色乘以 2 的档数次方；每增加 1 档亮度翻倍。范围 -8～8，默认 0；独立于 AgX/Bloom 开关，实时生效，不重置模拟；需手动保存。",
                |ui| ui.add(ParameterSlider::new(&mut config.exposure, -8.0..=8.0)),
            );
            parameter(
                ui,
                "启用 Bloom",
                "默认开启。只控制高光扩散，与 AgX 映射独立。关闭时保留参数并跳过泛光 pass，曝光和映射仍可生效。实时生效，不重置模拟；需手动保存。",
                |ui| ui.checkbox(&mut config.bloom_enabled, ""),
            );
            if !config.bloom_enabled {
                return;
            }
            for (name, help, value, range) in [
                (
                    "Bloom 强度",
                    "泛光叠加强度，无量纲，范围 0～2，默认 0.1。设为 0 跳过泛光 pass；是否映射由 AgX 开关决定。实时生效，不重置模拟；需手动保存。",
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

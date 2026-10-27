use super::{
    gui::{parameter, parameter_table},
    spray::SprayConfig,
};

pub fn show(ui: &mut egui::Ui, config: &mut SprayConfig, error: Option<&str>) {
    ui.label(egui::RichText::new("GPU 浪花粒子").strong());
    parameter_table(ui, "spray parameters", |ui| {
        parameter(
            ui,
            "启用浪花",
            "近岸 SWE 纹理飞沫，默认关闭。实时切换；关闭清空粒子，不改变水体和泡沫。",
            |ui| ui.checkbox(&mut config.enabled, ""),
        );
        parameter(
            ui,
            "发射强度视图",
            "GPU 发射密度除以满权重密度，绿低红高；不读取泡沫密度。显示最近一次模拟帧的发射区域，暂停时保留。实时生效。",
            |ui| ui.checkbox(&mut config.debug, ""),
        );
        for (name, value, min, max, tip) in [
            (
                "发射密度",
                &mut config.rate,
                0.0,
                2000.0,
                "0～2000 粒子/平方米/模拟秒，默认 150；再乘坡度、上升和水深门控。实时影响新生粒子，0 停止生成。",
            ),
            (
                "坡度阈值",
                &mut config.slope,
                0.0,
                2.0,
                "水面高差/水平距离，0～2，默认 0.03；过渡宽度固定 0.1。不是水深梯度。实时影响发射。",
            ),
            (
                "上升阈值（米/秒）",
                &mut config.rise,
                0.0,
                5.0,
                "0～5 米/模拟秒，默认 0.02；过渡宽度 0.2 米/秒。仅上升水面生成，实时生效。",
            ),
            (
                "最小水深（米）",
                &mut config.depth,
                0.001,
                1.0,
                "0.001～1 米，默认 0.02；此水深以下不生成，到两倍水深达到完整权重。实时生效。",
            ),
            (
                "向上速度（米/秒）",
                &mut config.upward,
                0.0,
                10.0,
                "0～10 米/模拟秒，默认 2；另有少量确定性扰动。实时影响新生粒子。",
            ),
            (
                "前向速度（米/秒）",
                &mut config.forward,
                0.0,
                10.0,
                "0～10 米/模拟秒，默认 0.5；沿水流叠加，水流静止时不加前向分量。实时影响新生粒子。",
            ),
            (
                "最长寿命（秒）",
                &mut config.lifetime,
                0.05,
                5.0,
                "0.05～5 模拟秒，默认 1.2；实际为其 75%～100%，落水可提前消失。只影响新生粒子。",
            ),
            (
                "尺寸（米）",
                &mut config.size,
                0.01,
                1.0,
                "0.01～1 米，默认 0.2；四边形直径带 0.65～1.35 倍随机变化。只影响新生粒子。",
            ),
            (
                "空气阻力（每秒）",
                &mut config.drag,
                0.0,
                10.0,
                "0～10/模拟秒，默认 0.5；静止空气中的线性阻力，0 为纯重力弹道。实时影响所有粒子。",
            ),
            (
                "软交界距离（米）",
                &mut config.softness,
                0.001,
                1.0,
                "0.001～1 米，默认 0.15；粒子接近场景深度时淡出。只影响显示，实时生效。",
            ),
        ] {
            parameter(ui, name, tip, |ui| {
                ui.add(egui::Slider::new(value, min..=max).max_decimals(3))
            });
        }
    });
    if let Some(error) = error {
        ui.colored_label(egui::Color32::RED, error);
    }
}

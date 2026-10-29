use crate::ocean::parameters::spray::SprayConfig;
use charming_egui::{ParameterSlider, group, parameter};

fn sliders(ui: &mut egui::Ui, rows: &[(&str, &str)], values: &mut [(&mut f32, f32, f32)]) {
    for ((name, tip), (value, min, max)) in rows.iter().zip(values) {
        parameter(ui, name, tip, |ui| {
            ui.add(ParameterSlider::new(*value, *min..=*max).max_decimals(3))
        });
    }
}

pub fn show(ui: &mut egui::Ui, config: &mut SprayConfig, error: Option<&str>) {
    group(ui, "GPU 浪花粒子", |ui| {
        parameter(
            ui,
            "启用浪花",
            "近岸 SWE 纹理飞沫，默认关闭。实时切换；关闭清空粒子，不改变水体和泡沫。",
            |ui| ui.checkbox(&mut config.enabled, ""),
        );
    });
    if let Some(error) = error {
        ui.colored_label(egui::Color32::RED, error);
    }
    if !config.enabled {
        return;
    }
    group(ui, "发射条件", |ui| {
        sliders(
            ui,
            &[
                (
                    "发射密度",
                    "0～2000 粒子/平方米/模拟秒，默认 150；再乘坡度、上升和水深门控。实时影响新生粒子，0 停止生成。",
                ),
                (
                    "坡度阈值",
                    "水面高差/水平距离，0～2，默认 0.03；过渡宽度固定 0.1。不是水深梯度。实时影响发射。",
                ),
                (
                    "上升阈值（米/秒）",
                    "0～5 米/模拟秒，默认 0.02；过渡宽度 0.2 米/秒。仅上升水面生成，实时生效。",
                ),
                (
                    "最小水深（米）",
                    "0.001～1 米，默认 0.02；此水深以下不生成，到两倍水深达到完整权重。实时生效。",
                ),
            ],
            &mut [
                (&mut config.rate, 0.0, 2000.0),
                (&mut config.slope, 0.0, 2.0),
                (&mut config.rise, 0.0, 5.0),
                (&mut config.depth, 0.001, 1.0),
            ],
        );
    });
    group(ui, "运动与寿命", |ui| {
        sliders(
            ui,
            &[
                (
                    "向上速度（米/秒）",
                    "0～10 米/模拟秒，默认 2；另有少量确定性扰动。实时影响新生粒子。",
                ),
                (
                    "前向速度（米/秒）",
                    "0～10 米/模拟秒，默认 0.5；沿水流叠加，水流静止时不加前向分量。实时影响新生粒子。",
                ),
                (
                    "最长寿命（秒）",
                    "0.05～5 模拟秒，默认 1.2；实际为其 75%～100%，落水可提前消失。只影响新生粒子。",
                ),
                (
                    "空气阻力（每秒）",
                    "0～10/模拟秒，默认 0.5；静止空气中的线性阻力，0 为纯重力弹道。实时影响所有粒子。",
                ),
            ],
            &mut [
                (&mut config.upward, 0.0, 10.0),
                (&mut config.forward, 0.0, 10.0),
                (&mut config.lifetime, 0.05, 5.0),
                (&mut config.drag, 0.0, 10.0),
            ],
        );
    });
    group(ui, "浪花诊断", |ui| {
        parameter(
            ui,
            "发射强度视图",
            "GPU 发射密度除以满权重密度，绿低红高；不读取泡沫密度。显示最近一次模拟帧的发射区域，暂停时保留。实时生效。",
            |ui| ui.checkbox(&mut config.debug, ""),
        );
    });
}

pub fn appearance(ui: &mut egui::Ui, config: &mut SprayConfig) {
    group(ui, "尺寸与软交界", |ui| {
        sliders(
            ui,
            &[
                (
                    "尺寸（米）",
                    "基础四边形直径，0.01～2 米，默认 0.2。最终直径 = 此尺寸 × 随机尺寸倍率，因此最终尺寸可以超过 2 米。实时影响新生粒子，已有粒子不变；保存到配置。",
                ),
                (
                    "软交界距离（米）",
                    "0.001～1 米，默认 0.15；粒子接近场景深度时淡出。只影响显示，实时生效。",
                ),
            ],
            &mut [
                (&mut config.size, 0.01, 2.0),
                (&mut config.softness, 0.001, 1.0),
            ],
        );
        let [minimum, maximum] = &mut config.size_range;
        parameter(
            ui,
            "随机尺寸倍率下限",
            "无量纲，0～2，默认 0.65，不能超过上限。新生粒子在上下限之间均匀随机；上下限相同则为固定倍率。实时生效并保存，不重置模拟。",
            |ui| ui.add(ParameterSlider::new(&mut *minimum, 0.0..=*maximum).max_decimals(3)),
        );
        parameter(
            ui,
            "随机尺寸倍率上限",
            "无量纲，0～2，默认 1.35，不能低于下限。例如尺寸 2 米、倍率 0.5～1，实际为 1～2 米。只影响新生粒子，实时生效并保存。",
            |ui| ui.add(ParameterSlider::new(&mut *maximum, *minimum..=2.0).max_decimals(3)),
        );
    });
}

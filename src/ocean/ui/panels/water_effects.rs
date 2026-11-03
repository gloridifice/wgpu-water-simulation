use super::controls::{list, number};
use crate::ocean::{parameters::optics::OpticalMethod, ui::gui::Gui};
use yummy_egui::{group, parameter, segmented_control};

fn refraction_mode(ui: &mut egui::Ui, enabled: &mut bool, method: &mut OpticalMethod) {
    let mut selection = if !*enabled {
        0
    } else if *method == OpticalMethod::Fast {
        1
    } else {
        2
    };
    if segmented_control(ui,&mut selection,&[(0,"关闭"),(1,"快速"),(2,"物理")]).on_hover_text("关闭停止该效果；快速使用 UV 近似，物理使用波面与接收面求交。切换实时生效，手动保存，不重置模拟，保留各模式参数。").changed() {
        *enabled=selection!=0;
        if *enabled {*method=if selection==1 {OpticalMethod::Fast} else {OpticalMethod::Physical};}
    }
}
impl Gui {
    pub(in crate::ocean::ui) fn caustics_panel(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.optics;
        segmented_control(ui, &mut o.caustics, &[(false, "关闭"), (true, "物理")])
            .on_hover_text("实时切换物理焦散，保留参数，不重置模拟。");
        if o.caustics {
            group(ui, "焦散参数", |ui| {
                number(ui, "焦散强度", &mut o.caustics_strength, 0.0..=4.0, false);
                number(
                    ui,
                    "焦散覆盖宽度（米）",
                    &mut o.caustics_extent,
                    8.0..=512.0,
                    true,
                );
                number(
                    ui,
                    "焦散柔化（纹素）",
                    &mut o.caustics_softness,
                    0.0..=4.0,
                    false,
                );
                list(
                    ui,
                    "焦散光束网格",
                    &mut o.caustics_resolution,
                    &[(128, "128 × 128"), (256, "256 × 256"), (512, "512 × 512")],
                );
                number(ui, "水折射率", &mut o.water_ior, 1.0..=1.6, false);
            });
        }
        group(ui, "焦散诊断", |ui| {
            parameter(
                ui,
                "显示",
                "实时切换共享渲染视图；不重置。入射光不包含接收材质或观察路径消光。",
                |ui| {
                    segmented_control(ui, &mut o.view, &[(0, "正常"), (12, "入射光")]);
                },
            );
        });
    }
    pub(in crate::ocean::ui) fn refraction_panel(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.optics;
        refraction_mode(ui, &mut o.refraction, &mut o.refraction_method);
        if o.refraction {
            group(ui, "折射参数", |ui| {
                number(ui, "水折射率", &mut o.water_ior, 1.0..=1.6, false);
                if o.refraction_method == OpticalMethod::Fast {
                    number(
                        ui,
                        "折射扰动强度",
                        &mut o.fast_refraction_strength,
                        0.0..=2.0,
                        false,
                    );
                    number(
                        ui,
                        "折射充分扰动水深（米）",
                        &mut o.fast_refraction_depth,
                        0.01..=20.0,
                        true,
                    );
                    let mut percent = o.fast_refraction_limit * 100.0;
                    number(
                        ui,
                        "折射偏移上限（屏幕高度%）",
                        &mut percent,
                        0.0..=20.0,
                        false,
                    );
                    o.fast_refraction_limit = percent / 100.0;
                }
                number(
                    ui,
                    "折射最大光程（米）",
                    &mut o.refraction_distance,
                    1.0..=500.0,
                    true,
                );
            });
        }
        group(ui, "折射诊断", |ui| {
            parameter(
                ui,
                "显示",
                "实时切换共享渲染视图；绿色为有效采样，红色为回退，关闭时灰色。不重置。",
                |ui| {
                    segmented_control(ui, &mut o.view, &[(0, "正常"), (13, "方向"), (14, "命中")]);
                },
            );
        });
    }
}

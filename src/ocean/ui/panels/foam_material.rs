use super::controls::{list, number, toggle};
use crate::ocean::camera::Camera;
use crate::ocean::ui::controls::{PanelButton, group, parameter};
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;

impl Gui {
    pub(in crate::ocean::ui) fn foam_material_panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
    ) {
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
        if ui.add(PanelButton::new("恢复默认泡沫颜色")).clicked() {
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
                let names: Vec<_> = crate::ocean::simulation::nearshore::FOAM_TEXTURE_NAMES
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
                let names: Vec<_> = crate::ocean::simulation::nearshore::FOAM_MATERIAL_VIEWS
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
}

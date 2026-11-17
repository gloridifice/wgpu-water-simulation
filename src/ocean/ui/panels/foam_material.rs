use super::controls::{list, number, toggle};
use crate::ocean::camera::Camera;
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;
use yummy_egui::ParameterSlider;
use yummy_egui::{PanelButton, group, parameter, segmented_control};

impl Gui {
    pub(in crate::ocean::ui) fn foam_material_panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
    ) {
        group(ui, "Foam Display", |ui| {
            toggle(ui, "Show Foam", &mut camera.foam);
        });
        if !camera.foam {
            return;
        }
        let swe = self.optics.swe_enabled();
        let shore = &mut self.optics.shore;
        group(ui, "Shared Foam Color", |ui| {
            parameter(ui, "Water Foam", help("Water Foam"), |ui| {
                ui.color_edit_button_rgb(&mut shore.water_foam_color)
            });
        });
        if ui
            .add(PanelButton::new("Restore Default Foam Color"))
            .clicked()
        {
            shore.water_foam_color = [0.87, 0.92, 0.90];
        }
        if !swe {
            return;
        }
        group(ui, "Nearshore Foam Material", |ui| {
            toggle(
                ui,
                "Flow-Warped Texture",
                &mut shore.foam_texture_distortion,
            );
            if shore.foam_dual_phase {
                list(
                    ui,
                    "Foam Display Components",
                    &mut shore.foam_species_view,
                    &[
                        (0, "All"),
                        (1, "Fresh Foam N Only"),
                        (2, "Residual Foam R Only"),
                    ],
                );
            }
            if !shore.foam_dual_phase {
                toggle(ui, "Prototype Hole Appearance", &mut shore.foam_prototype);
            }
            if !shore.foam_dual_phase && !shore.foam_prototype {
                let names: Vec<_> = crate::ocean::parameters::shore::FOAM_TEXTURE_NAMES
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i as u32, *n))
                    .collect();
                list(ui, "Texture Source", &mut shore.foam_texture, &names);
            }
            parameter(
                ui,
                "Coordinate Reset Period (s)",
                help("Coordinate Reset Period (s)"),
                |ui| {
                    ui.add(
                        ParameterSlider::new(&mut shore.foam_texture_period, 0.0..=60.0)
                            .step_by(0.25),
                    )
                },
            );
            if shore.foam_texture > 0 && !shore.foam_prototype && !shore.foam_dual_phase {
                number(
                    ui,
                    "Texture Size (m)",
                    &mut shore.foam_texture_size,
                    0.25..=64.0,
                    true,
                );
                number(
                    ui,
                    "Foam Micro Normal Strength",
                    &mut shore.foam_micro_normal,
                    0.0..=3.0,
                    false,
                );
                number(
                    ui,
                    "Foam Roughness",
                    &mut shore.foam_roughness,
                    0.15..=1.0,
                    false,
                );
            }
            if shore.foam_texture > 0 || shore.foam_prototype || shore.foam_dual_phase {
                number(
                    ui,
                    "Opacity Density Scale",
                    &mut shore.foam_opacity_scale,
                    0.01..=20.0,
                    true,
                );
                let names: Vec<_> = crate::ocean::simulation::nearshore::FOAM_MATERIAL_VIEWS
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (i as u32, *n))
                    .collect();
                list(ui, "Material Debug", &mut shore.foam_material_view, &names);
            }
            number(
                ui,
                "Density Color Reference",
                &mut shore.foam_density_reference,
                0.01..=20.0,
                true,
            );
        });
        if shore.foam_dual_phase {
            group(ui, "Fresh Foam N Appearance", |ui| {
                parameter(ui, "Rendering Mode", help("N Rendering Mode"), |ui| {
                    segmented_control(
                        ui,
                        &mut shore.foam_fresh_threshold_mode,
                        &[(0, "Constant"), (1, "Noise"), (2, "Texture")],
                    );
                });
                match shore.foam_fresh_threshold_mode {
                    0 => number(
                        ui,
                        "N Constant Threshold",
                        &mut shore.foam_fresh_threshold,
                        0.0..=1.0,
                        false,
                    ),
                    1 => {
                        number(
                            ui,
                            "N Main Noise Frequency (per m)",
                            &mut shore.foam_fresh_noise_frequency,
                            0.05..=20.0,
                            true,
                        );
                        number(
                            ui,
                            "N Fine Noise Frequency (per m)",
                            &mut shore.foam_fresh_noise_detail_frequency,
                            0.05..=60.0,
                            true,
                        );
                        number(
                            ui,
                            "N Fine Noise Fraction",
                            &mut shore.foam_fresh_noise_detail_weight,
                            0.0..=1.0,
                            false,
                        );
                    }
                    _ => {
                        let names: Vec<_> = crate::ocean::parameters::shore::FOAM_TEXTURE_NAMES
                            .iter()
                            .enumerate()
                            .skip(1)
                            .map(|(i, n)| (i as u32, *n))
                            .collect();
                        list(
                            ui,
                            "N Texture Source",
                            &mut shore.foam_fresh_texture,
                            &names,
                        );
                        number(
                            ui,
                            "N Texture Size (m)",
                            &mut shore.foam_fresh_texture_size,
                            0.25..=64.0,
                            true,
                        );
                        number(
                            ui,
                            "N Micro Normal Strength",
                            &mut shore.foam_fresh_micro_normal,
                            0.0..=3.0,
                            false,
                        );
                        number(
                            ui,
                            "N Roughness",
                            &mut shore.foam_fresh_roughness,
                            0.15..=1.0,
                            false,
                        );
                        number(
                            ui,
                            "N Coverage Reference",
                            &mut shore.foam_fresh_coverage_reference,
                            0.01..=20.0,
                            true,
                        );
                    }
                }
                toggle(
                    ui,
                    "Concentration Controls Opacity",
                    &mut shore.foam_fresh_density_opacity,
                );
                if shore.foam_fresh_density_opacity {
                    number(
                        ui,
                        "N Concentration Range",
                        &mut shore.foam_fresh_opacity_range,
                        0.0..=10.0,
                        false,
                    );
                    number(
                        ui,
                        "N Concentration Exponent p",
                        &mut shore.foam_fresh_opacity_power,
                        0.1..=5.0,
                        false,
                    );
                }
            });
            group(ui, "Residual Foam R Appearance", |ui| {
                toggle(
                    ui,
                    "Concentration Controls Opacity",
                    &mut shore.foam_residual_density_opacity,
                );
                if shore.foam_residual_density_opacity {
                    number(
                        ui,
                        "R Concentration Range",
                        &mut shore.foam_residual_opacity_range,
                        0.0..=10.0,
                        false,
                    );
                    number(
                        ui,
                        "R Concentration Exponent p",
                        &mut shore.foam_residual_opacity_power,
                        0.1..=5.0,
                        false,
                    );
                }
            });
        }
        group(ui, "Foam Height (Appearance Only)", |ui| {
            number(
                ui,
                "Maximum Height (m)",
                &mut shore.foam_height_max,
                0.0..=1.0,
                false,
            );
            if shore.foam_height_max > 0.0 {
                number(
                    ui,
                    "Starting Density",
                    &mut shore.foam_height_threshold,
                    0.0..=20.0,
                    false,
                );
                number(
                    ui,
                    "Density Transition Range",
                    &mut shore.foam_height_transition,
                    0.001..=20.0,
                    true,
                );
            }
        });
    }
}

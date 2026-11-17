use super::controls::{list, number, toggle};
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::parameters::optics::VIEW_NAMES;
use crate::ocean::render::skylight::incident_reference;
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;
use yummy_egui::{PanelButton, ParameterSlider, choices, group, parameter, parameter_table};

impl Gui {
    pub(in crate::ocean::ui) fn optics_panel(&mut self, ui: &mut egui::Ui) {
        self.sigma_editor.ui(
            ui,
            "Extinction Coefficient (per meter)",
            &mut self.optics.sigma_t,
        );
        self.source_editor.ui(
            ui,
            "Manual Scattering Source (radiance / m)",
            &mut self.optics.source,
        );
        group(ui, "Sky Lighting and Reflections", |ui| {
            parameter(
                ui,
                "Sky Color (linear RGB)",
                help("Sky Color (linear RGB)"),
                |ui| ui.color_edit_button_rgb(&mut self.optics.sky.color),
            );
            number(
                ui,
                "Sky Intensity",
                &mut self.optics.sky.intensity,
                0.0..=20.0,
                true,
            );
            toggle(
                ui,
                "Sky Reflection and Highlights",
                &mut self.optics.reflections,
            );
        });
        group(ui, "Sky Scattering", |ui| {
            toggle(ui, "Enable Sky Scattering", &mut self.optics.sky.enabled);
            if self.optics.sky.enabled {
                parameter(
                    ui,
                    "Scattering Albedo RGB",
                    help("Scattering Albedo RGB"),
                    |ui| ui.color_edit_button_rgb(&mut self.optics.sky.albedo),
                );
                parameter(
                    ui,
                    "Sky Integration Steps",
                    help("Sky Integration Steps"),
                    |ui| {
                        choices(
                            ui,
                            &mut self.optics.sky.steps,
                            &[(8, "8"), (16, "16"), (32, "32")],
                        )
                    },
                );
            }
        });
        ui.collapsing("Exact Sky RGB Values", |ui| {
            parameter_table(ui, "sky exact", |ui| {
                for (c, name) in ["R", "G", "B"].iter().enumerate() {
                    parameter(
                        ui,
                        &format!("Color {name}"),
                        "Linear RGB component of the sky color, 0–1, applied immediately.",
                        |ui| {
                            ui.add(
                                ParameterSlider::new(&mut self.optics.sky.color[c], 0.0..=1.0)
                                    .clamping(egui::SliderClamping::Edits)
                                    .max_decimals(4),
                            )
                        },
                    );
                    if self.optics.sky.enabled {
                        parameter(
                            ui,
                            &format!("Albedo {name}"),
                            "Scattering albedo, 0–1, applied immediately.",
                            |ui| {
                                ui.add(
                                    ParameterSlider::new(&mut self.optics.sky.albedo[c], 0.0..=1.0)
                                        .clamping(egui::SliderClamping::Edits)
                                        .max_decimals(4),
                                )
                            },
                        );
                    }
                }
            });
        });
        if self.optics.sky.enabled {
            ui.collapsing("Sky Scattering Source by Depth", |ui| {
                depth_graph(ui, &self.optics)
            });
        }
        self.post_process_controls(ui);
        group(ui, "Optics Diagnostics", |ui| {
            let names: Vec<_> = VIEW_NAMES
                .iter()
                .enumerate()
                .map(|(i, n)| (i as u32, *n))
                .collect();
            list(ui, "Water Debug View", &mut self.optics.view, &names);
            if self.optics.view == 1 {
                number(
                    ui,
                    "Path Length White Reference (m)",
                    &mut self.optics.path_range,
                    1.0..=300.0,
                    true,
                );
            }
        });
        if ui
            .add(PanelButton::new("Reset Optics Parameters"))
            .on_hover_text(
                "As before, restore the entire optics configuration (including scene, SWE, materials, and spray), resetting the corresponding simulation history.",
            )
            .clicked()
        {
            self.optics = OpticsConfig::default();
        }
    }
}

pub fn depth_graph(ui: &mut egui::Ui, optics: &crate::ocean::parameters::optics::OpticsConfig) {
    let depth = if optics.lake.enabled {
        optics.lake.terrain.depth
    } else {
        optics.depth
    };
    let colors = [
        egui::Color32::LIGHT_RED,
        egui::Color32::LIGHT_GREEN,
        egui::Color32::LIGHT_BLUE,
    ];
    let source = |z: f32, c: usize| {
        if !optics.sky.enabled {
            return 0.0;
        }
        optics.sigma_t[c]
            * optics.sky.albedo[c]
            * incident_reference(&optics.sky, f64::from(z * optics.sigma_t[c]), c, 32) as f32
    };
    let peak = (0..3).map(|c| source(0.0, c)).fold(0.0_f32, f32::max);
    parameter_table(ui, "graph axes", |ui| {
        parameter(
            ui,
            "Depth Range",
            "Graph horizontal axis, in meters.",
            |ui| ui.label(format!("0 .. {depth:.2} m")),
        );
        parameter(
            ui,
            "Scattering Source Range",
            "Local sky scattering source, in radiance per meter, not yet integrated toward the observer.",
            |ui| ui.label(format!("0 .. {peak:.5}")),
        );
    });
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 90.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, egui::Color32::from_gray(18));
    for (c, color) in colors.into_iter().enumerate() {
        let points = (0..65)
            .map(|i| {
                let u = i as f32 / 64.0;
                egui::pos2(
                    rect.left() + u * rect.width(),
                    rect.bottom() - source(depth * u, c) / peak.max(1e-20) * rect.height(),
                )
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, egui::Stroke::new(1.5_f32, color)));
    }
}

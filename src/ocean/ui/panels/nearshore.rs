use super::controls::{list, number, toggle};
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;
use yummy_egui::{PanelButton, action_row, choices, group, parameter};

impl Gui {
    pub(in crate::ocean::ui) fn shore_panel(&mut self, ui: &mut egui::Ui) {
        let available = self.optics.swe_enabled();
        group(ui, "Shallow Water", |ui| {
            toggle(
                ui,
                "Enable Shallow Water Simulation",
                &mut self.optics.shore.enabled,
            );
            if !available {
                parameter(
                    ui,
                    "Requirements",
                    "Select the beach or enable a model in the Scene workspace, then enable Shallow Water.",
                    |ui| ui.label("Requires a coastal scene and SWE"),
                );
            }
        });
        if !self.optics.swe_enabled() {
            return;
        }
        let shore = &mut self.optics.shore;
        group(ui, "Solving and Sampling", |ui| {
            parameter(
                ui,
                "Grid Cells per Axis",
                "SWE resolution 64–1024; changing it immediately rebuilds and resets nearshore history without changing the domain.",
                |ui| {
                    egui::ComboBox::from_id_salt("swe resolution")
                        .width(ui.available_width())
                        .selected_text(format!("{} × {}", shore.resolution, shore.resolution))
                        .show_ui(ui, |ui| {
                            for n in crate::ocean::simulation::nearshore::RESOLUTIONS {
                                ui.selectable_value(&mut shore.resolution, n, format!("{n} × {n}"));
                            }
                        });
                },
            );
            parameter(
                ui,
                "Grid Spacing X/Z (m)",
                help("Grid Spacing X/Z (m)"),
                |ui| {
                    ui.label(format!(
                        "{:.3} / {:.3}",
                        self.swe_domain[2] / shore.resolution as f32,
                        self.swe_domain[3] / shore.resolution as f32
                    ))
                },
            );
            parameter(ui, "SWE Solver", help("SWE Solver"), |ui| {
                choices(
                    ui,
                    &mut shore.order,
                    &[(1, "First Order"), (2, "Second Order")],
                )
            });
            number(ui, "CFL Limit (C)", &mut shore.cfl, 0.05..=1.1, false);
        });
        group(ui, "FFT → SWE Input and Blending", |ui| {
            number(
                ui,
                "FFT → SWE Gain",
                &mut shore.input_gain,
                0.0..=3.0,
                false,
            );
            number(
                ui,
                "FFT Blend Depth (m)",
                &mut shore.breaking_depth,
                0.5..=5.0,
                false,
            );
            toggle(ui, "Filter Short Waves", &mut shore.filter_enabled);
            if shore.filter_enabled {
                number(
                    ui,
                    "Cutoff Wavelength (m)",
                    &mut shore.filter_cutoff,
                    0.0..=128.0,
                    false,
                );
                number(
                    ui,
                    "Filter Transition Width (m)",
                    &mut shore.filter_transition,
                    0.0..=128.0,
                    false,
                );
            }
        });
        let [default_filter, short_filter] = action_row(
            ui,
            [
                PanelButton::new("Default 12–24 m"),
                PanelButton::new("3–6 m"),
            ],
        );
        if default_filter.clicked() {
            shore.filter_enabled = true;
            shore.filter_cutoff = 12.0;
            shore.filter_transition = 12.0;
        }
        if short_filter.clicked() {
            shore.filter_enabled = true;
            shore.filter_cutoff = 3.0;
            shore.filter_transition = 3.0;
        }
        group(ui, "Sand History and Diagnostics", |ui| {
            if self.optics.beach {
                number(
                    ui,
                    "Sand Drying Time (s)",
                    &mut shore.drying_time,
                    1.0..=120.0,
                    false,
                );
            }
            list(
                ui,
                "Shallow Water Debug View",
                &mut shore.view,
                &crate::ocean::simulation::nearshore::VIEWS,
            );
        });
        if ui
            .add(PanelButton::new("Reset Shallow Water, Foam and Sand"))
            .clicked()
        {
            self.clear_foam = true;
        }
    }

    pub(in crate::ocean::ui) fn shore_foam_panel(&mut self, ui: &mut egui::Ui) {
        let shore = &mut self.optics.shore;
        group(ui, "Foam Generation", |ui| {
            toggle(ui, "Enable Foam Emission", &mut shore.foam_emission_enabled);
        });
        if shore.foam_emission_enabled {
            group(ui, "Front Generation", |ui| {
                toggle(
                    ui,
                    "Compression Ridge Experiment",
                    &mut shore.foam_ridge_enabled,
                );
                number(
                    ui,
                    "Front Generation Rate (per s)",
                    &mut shore.foam_front_generation,
                    0.0..=30.0,
                    false,
                );
                if shore.foam_ridge_enabled {
                    number(
                        ui,
                        "Narrow Band Width (m)",
                        &mut shore.foam_ridge_width,
                        0.05..=10.0,
                        true,
                    );
                    number(
                        ui,
                        "Peak Relative Prominence",
                        &mut shore.foam_ridge_prominence,
                        0.0..=1.0,
                        false,
                    );
                } else {
                    number(
                        ui,
                        "Front Sampling Distance (m)",
                        &mut shore.foam_front_distance,
                        0.05..=10.0,
                        false,
                    );
                }
                number(
                    ui,
                    "Velocity Change Start Threshold (per s)",
                    &mut shore.foam_velocity_threshold,
                    0.0..=20.0,
                    false,
                );
                number(
                    ui,
                    "Velocity Change Transition Range (per s)",
                    &mut shore.foam_velocity_transition,
                    0.0..=20.0,
                    false,
                );
            });
            if !shore.foam_ridge_enabled {
                group(ui, "Background Generation", |ui| {
                    number(
                        ui,
                        "Background Generation Rate (per s)",
                        &mut shore.generation,
                        0.0..=15.0,
                        false,
                    );
                    number(
                        ui,
                        "Background Generation Multiplier",
                        &mut shore.foam_background_multiplier,
                        0.0..=1.0,
                        false,
                    );
                    number(
                        ui,
                        "Background Generation Start Threshold",
                        &mut shore.foam_generation_threshold,
                        0.0..=2.0,
                        false,
                    );
                    number(
                        ui,
                        "Background Generation Transition Range",
                        &mut shore.foam_generation_transition,
                        0.0..=2.0,
                        false,
                    );
                    number(
                        ui,
                        "Convergence Enhancement Threshold (per s)",
                        &mut shore.foam_compression_threshold,
                        0.0..=4.0,
                        false,
                    );
                });
            }
        }
        group(ui, "Concentration Mode and Lifetime", |ui| {
            toggle(
                ui,
                "Dual-Concentration Experiment",
                &mut shore.foam_dual_phase,
            );
            if shore.foam_dual_phase {
                number(
                    ui,
                    "Fresh Half-Life (s)",
                    &mut shore.foam_fresh_half_life,
                    0.1..=60.0,
                    true,
                );
                number(
                    ui,
                    "Residual Half-Life (s)",
                    &mut shore.foam_residual_half_life,
                    0.1..=120.0,
                    true,
                );
                number(
                    ui,
                    "Exit-to-Residual Fraction",
                    &mut shore.foam_conversion,
                    0.0..=1.0,
                    false,
                );
            } else {
                number(
                    ui,
                    "Water Foam Decay (per s)",
                    &mut shore.decay,
                    0.0..=4.0,
                    false,
                );
            }
        });
        if !shore.foam_dual_phase {
            group(ui, "Foam Transport and Flow Drag", |ui| {
                if shore.order == 2 {
                    parameter(
                        ui,
                        "Foam Spatial Transport",
                        help("Foam Spatial Transport"),
                        |ui| {
                            choices(
                                ui,
                                &mut shore.foam_transport_order,
                                &[(1, "First Order"), (2, "Second Order")],
                            )
                        },
                    );
                }
                toggle(ui, "Enable Flow Drag", &mut shore.foam_drag_enabled);
                if shore.foam_drag_enabled {
                    number(
                        ui,
                        "Flow Following Multiplier",
                        &mut shore.foam_follow,
                        0.0..=1.0,
                        false,
                    );
                    number(
                        ui,
                        "Velocity Response Time (s)",
                        &mut shore.foam_response_time,
                        0.0..=20.0,
                        false,
                    );
                }
                number(
                    ui,
                    "New Foam Velocity Multiplier",
                    &mut shore.foam_birth_speed,
                    0.0..=1.0,
                    false,
                );
                parameter(
                    ui,
                    "Physical Reference Starting Point",
                    help("Physical Reference Starting Point"),
                    |ui| {
                        if ui
                            .add(PanelButton::new("Apply Recommended Values"))
                            .clicked()
                        {
                            shore.apply_foam_reference();
                        }
                    },
                );
            });
        }
        group(ui, "Foam Density Diagnostics", |ui| {
            number(
                ui,
                "Density Color Reference",
                &mut shore.foam_density_reference,
                0.01..=20.0,
                true,
            );
            list(
                ui,
                "Shallow Water Debug View",
                &mut shore.view,
                &crate::ocean::simulation::nearshore::VIEWS,
            );
        });
        let [debug, reset] = action_row(
            ui,
            [
                PanelButton::new("Density Debug: Low Green / High Red"),
                PanelButton::new("Reset Shallow Water History"),
            ],
        );
        if debug.clicked() {
            shore.view = 6;
        }
        if reset.clicked() {
            self.clear_foam = true;
        }
    }
}

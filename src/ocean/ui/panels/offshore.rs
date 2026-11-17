use super::controls::{number, toggle};
use crate::ocean::camera::Camera;
use crate::ocean::parameters::ocean::OceanConfig;
use crate::ocean::ui::gui::Gui;
use crate::ocean::ui::help::help;
use yummy_egui::ParameterSlider;
use yummy_egui::{PanelButton, action_row, choices, group, parameter};

impl Gui {
    pub(in crate::ocean::ui) fn ocean_panel(
        &mut self,
        ui: &mut egui::Ui,
        current: OceanConfig,
        camera: &mut Camera,
    ) -> Option<OceanConfig> {
        group(ui, "Phillips Spectrum", |ui| {
            number(
                ui,
                "Spectrum Strength A",
                &mut self.draft.amplitude,
                0.0..=0.1,
                true,
            );
            number(
                ui,
                "Spectrum Scale L (m)",
                &mut self.draft.phillips_length,
                0.1..=500.0,
                true,
            );
            let mut angle = self.draft.wind_angle.to_degrees();
            if parameter(
                ui,
                "Wind Direction (degrees)",
                help("Wind Direction (degrees)"),
                |ui| ui.add(ParameterSlider::new(&mut angle, -180.0..=180.0)),
            )
            .changed()
            {
                self.draft.wind_angle = angle.to_radians();
            }
            parameter(ui, "Random Seed", help("Random Seed"), |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.draft.seed));
                    if ui.button("Change").clicked() {
                        self.draft.seed = self.draft.seed.wrapping_add(1);
                    }
                });
            });
        });
        group(ui, "Domain and Sampling", |ui| {
            number(
                ui,
                "Patch Size (m)",
                &mut self.draft.length,
                8.0..=1024.0,
                true,
            );
            parameter(ui, "FFT Resolution", help("FFT Resolution"), |ui| {
                egui::ComboBox::from_id_salt("fft resolution")
                    .width(ui.available_width())
                    .selected_text(format!(
                        "{} × {}",
                        self.draft.resolution, self.draft.resolution
                    ))
                    .show_ui(ui, |ui| {
                        for n in [8, 16, 32, 64, 128, 256, 512] {
                            ui.selectable_value(
                                &mut self.draft.resolution,
                                n,
                                format!("{n} × {n}"),
                            );
                        }
                    });
            });
            ui.add_enabled_ui(!self.optics.lake.enabled, |ui| {
                parameter(
                    ui,
                    "Offshore Grid Density",
                    help("Offshore Grid Density"),
                    |ui| {
                        ui.add(ParameterSlider::new(
                            &mut self.draft.mesh_resolution,
                            8..=512,
                        ))
                    },
                );
            });
        });
        let mut apply = None;
        let [apply_button, defaults] = action_row(
            ui,
            [
                PanelButton::new("Apply Changes").enabled(self.draft != current),
                PanelButton::new("Restore Defaults"),
            ],
        );
        if apply_button.clicked() {
            match self.draft.validate() {
                Ok(()) => {
                    apply = Some(self.draft);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
        if defaults
            .on_hover_text(
                "Only restores the pending spectrum parameters; click Apply to take effect.",
            )
            .clicked()
        {
            self.draft = OceanConfig::default();
        }
        ui.label(if self.draft == current {
            "Parameters applied"
        } else {
            "Changes pending"
        });
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        if self.optics.lake.enabled {
            group(ui, "Lake FFT Waves", |ui| {
                parameter(
                    ui,
                    "FFT Overlay Strength",
                    "Dimensionless multiplier 0–1 for the shared spectrum height; 0 disables it. Takes effect immediately without resetting. Lakes do not use horizontal choppiness or the offshore mesh; applying spectrum changes rebuilds and restarts timing.",
                    |ui| {
                        ui.add(ParameterSlider::new(
                            &mut self.optics.lake.fft_strength,
                            0.0..=1.0,
                        ))
                    },
                );
                parameter(
                    ui,
                    "FFT Shore Fade",
                    "Takes effect immediately without resetting. When enabled, attenuates FFT within water depths of 0–0.5 m and shore distances of 0–1 m; when disabled, retains full strength while still masking dry land and keeping wave troughs above the lakebed.",
                    |ui| ui.checkbox(&mut self.optics.lake.fft_shore_fade, "Enable"),
                );
            });
            return apply;
        }
        group(ui, "Waves", |ui| {
            let mut triangular = current.triangular_mesh;
            toggle(ui, "Near-Equilateral Triangle Grid", &mut triangular);
            if triangular != current.triangular_mesh {
                self.draft.triangular_mesh = triangular;
                apply.get_or_insert(current).triangular_mesh = triangular;
            }
            number(ui, "Wave Height Gain", &mut camera.gain, 0.0..=3.0, false);
            toggle(ui, "Horizontal Choppiness", &mut camera.choppy);
            if camera.choppy {
                number(
                    ui,
                    "Choppiness Strength",
                    &mut camera.chop_amount,
                    0.0..=3.0,
                    false,
                );
            }
        });
        apply
    }

    pub(in crate::ocean::ui) fn offshore_foam_panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
    ) {
        group(ui, "Offshore Foam Generation and Decay", |ui| {
            number(
                ui,
                "Jacobian Threshold",
                &mut self.foam_settings.threshold,
                -0.5..=1.0,
                false,
            );
            number(
                ui,
                "Generation Rate (per s)",
                &mut self.foam_settings.generation,
                0.0..=20.0,
                false,
            );
            number(
                ui,
                "Decay Coefficient (per s)",
                &mut self.foam_settings.decay,
                0.0..=5.0,
                false,
            );
        });
        group(ui, "Offshore Diagnostics", |ui| {
            parameter(ui, "Foam View", help("Foam View"), |ui| {
                choices(
                    ui,
                    &mut camera.foam_debug,
                    &[(0, "Normal"), (1, "Jacobian"), (2, "Coverage")],
                )
            });
        });
        if ui
            .add(PanelButton::new("Clear Foam History"))
            .on_hover_text("Uses the global history reset, also resetting shallow water, foam and sand history.")
            .clicked()
        {
            self.clear_foam = true;
        }
    }
}

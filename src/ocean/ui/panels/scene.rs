use super::controls::{number, toggle};
use crate::ocean::camera::Camera;
use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::ui::gui::Gui;
use yummy_egui::{PanelButton, action_row, group, parameter};

impl Gui {
    pub(in crate::ocean::ui) fn scene_panel(
        &mut self,
        ui: &mut egui::Ui,
        camera: &mut Camera,
        length: f32,
    ) {
        let [beach, model, lake] = action_row(
            ui,
            [
                PanelButton::new("Beach Scene"),
                PanelButton::new("Rocky Coast Scene"),
                PanelButton::new("Lake Scene"),
            ],
        );
        if beach
            .on_hover_text(
                "Load the complete beach preset and reset nearshore history, not just the terrain.",
            )
            .clicked()
        {
            self.optics = OpticsConfig::beach_scene();
            *camera = Camera::beach(length);
        }
        if model
            .on_hover_text("Load the complete rocky coast preset and reset nearshore history.")
            .clicked()
        {
            self.optics = OpticsConfig::model_scene();
            *camera = Camera::model(length);
        }
        if lake
            .on_hover_text(
                "Generate a noise-shaped lake and rocks, run parametric ripples, and add the shared FFT at the selected strength. SWE is not used.",
            )
            .clicked()
        {
            self.optics = OpticsConfig::lake_scene();
            *camera = Camera::lake(self.optics.lake.terrain.radius);
            self.lake_panel.open = true;
            self.lake_panel.reset = true;
        }
        if self.optics.lake.enabled {
            let [terrain, ripples] = action_row(
                ui,
                [
                    PanelButton::new("Open Lake Terrain"),
                    PanelButton::new("Open Animated SDF Ripples"),
                ],
            );
            self.lake_panel.open |= terrain.clicked();
            self.lake_panel.open_ripples |= ripples.clicked();
            return;
        }
        group(ui, "Terrain and Seabed", |ui| {
            if !self.optics.model.enabled {
                toggle(ui, "Sloping Beach", &mut self.optics.beach);
            }
            if self.optics.model.enabled {
                parameter(
                    ui,
                    "Current Terrain",
                    "The model takes precedence over the sand slope. Enable and transform the model explicitly in the Model and Domain panel.",
                    |ui| ui.label("Model and supplemental seabed"),
                );
            }
            if self.optics.beach && !self.optics.model.enabled {
                number(
                    ui,
                    "Beach Slope (height/width)",
                    &mut self.optics.beach_slope,
                    0.01..=0.5,
                    false,
                );
                number(
                    ui,
                    "Shoreline X (m)",
                    &mut self.optics.shore_x,
                    -length..=length,
                    false,
                );
            }
            if !self.optics.beach || self.optics.model.enabled {
                number(
                    ui,
                    "Seabed Depth (m)",
                    &mut self.optics.depth,
                    0.25..=100.0,
                    true,
                );
            }
        });
        group(ui, "Local Simulation", |ui| {
            toggle(
                ui,
                "Enable Shallow Water Simulation",
                &mut self.optics.shore.enabled,
            );
            parameter(
                ui,
                "SWE Status",
                "Runs only in beach or model scenes with shallow water simulation enabled.",
                |ui| {
                    ui.label(if self.optics.swe_enabled() {
                        "Enabled"
                    } else {
                        "Requires a coastal scene and SWE"
                    })
                },
            );
        });
    }
}

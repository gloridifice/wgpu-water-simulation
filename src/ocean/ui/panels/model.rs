//! Apply-based model/domain editing: changing a draft never resets live water.
use crate::ocean::camera::Camera;
use crate::ocean::parameters::model::ModelConfig;
use crate::ocean::parameters::optics::OpticsConfig;
use yummy_egui::{PanelButton, ParameterSlider, action_row, parameter, parameter_table};
#[derive(Default)]
pub struct ModelPanel {
    draft: ModelConfig,
    applied: Option<ModelConfig>,
    pub source_bounds: Option<[[f32; 3]; 2]>,
    pub bed_range: Option<[f32; 2]>,
    pub error: Option<String>,
}
impl ModelPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        optics: &mut OpticsConfig,
        camera: &mut Camera,
        length: f32,
    ) {
        if self.applied != Some(optics.model) {
            self.draft = optics.model;
            self.applied = Some(optics.model);
        }
        if ui.add(PanelButton::new("Load Rocky Coast Scene")).clicked() {
            *optics = OpticsConfig::model_scene();
            *camera = Camera::model(length);
            self.draft = optics.model;
            self.applied = Some(optics.model);
            self.error = None;
        }
        yummy_egui::h3(ui, "Challaborough Pillbox");
        parameter_table(ui, "model asset status", |ui| {
            parameter(
                ui,
                "Heightfield Status",
                "Loads prebaked heights and coverage flags from assets/models/challaborough_pillbox.sweh. If missing, run terrain-bake first; triangles are not traversed in the frame loop.",
                |ui| {
                    ui.label(if self.source_bounds.is_some() {
                        "Disk asset loaded"
                    } else {
                        "Not loaded"
                    })
                },
            );
            parameter(
                ui,
                "Enable Model",
                "Applying switches to the model scene and resets nearshore history; disabling restores the original sand slope or flat bed.",
                |ui| ui.checkbox(&mut self.draft.enabled, ""),
            );
        });
        if self.draft.enabled {
            yummy_egui::h3(ui, "Model Transform and Water Level");
            parameter_table(ui, "model transform", |ui| {
                for (a, name) in ["Position X (m)", "Position Y (m)", "Position Z (m)"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "Model translation, -2000–2000 m. Y shares the sea level elevation datum; applying resamples the bed and resets nearshore history.",
                        |ui| {
                            ui.add(
                                ParameterSlider::new(&mut self.draft.position[a], -2000.0..=2000.0)
                                    .clamping(egui::SliderClamping::Edits)
                                    .max_decimals(3),
                            )
                        },
                    );
                }
                parameter(
                    ui,
                    "Uniform Scale",
                    "Meters per model unit, 0.01–100; the scan scale has not been calibrated against measurements. Applying resamples without rebaking the source asset.",
                    |ui| {
                        ui.add(
                            ParameterSlider::new(&mut self.draft.scale, 0.01..=100.0)
                                .logarithmic(true)
                                .clamping(egui::SliderClamping::Edits)
                                .max_decimals(3),
                        )
                    },
                );
                parameter(
                    ui,
                    "Y Rotation (degrees)",
                    "Rotation around the vertical axis, -180–180 degrees. Applying resamples and resets; pitch and roll require rebaking and are not available here.",
                    |ui| {
                        ui.add(
                            ParameterSlider::new(&mut self.draft.yaw_deg, -180.0..=180.0)
                                .clamping(egui::SliderClamping::Edits)
                                .max_decimals(2),
                        )
                    },
                );
                parameter(
                    ui,
                    "Sea Level Elevation (m)",
                    "-2000–2000 m. Rendering and solving use this level as Y=0; raising it is equivalent to a higher tide. Applying resets the simulation; this is not a dynamic tidal input.",
                    |ui| {
                        ui.add(
                            ParameterSlider::new(&mut self.draft.sea_level, -2000.0..=2000.0)
                                .clamping(egui::SliderClamping::Edits)
                                .max_decimals(3),
                        )
                    },
                );
            });
            yummy_egui::h3(ui, "SWE Bounds");
            parameter_table(ui, "model domain", |ui| {
                for (a, name) in ["Center X (m)", "Center Y (m)", "Center Z (m)"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "-2000–2000 m. XZ determines the computational domain and is rebuilt on apply; Y is only used for display and bounds checks, and does not move the water or clip terrain.",
                        |ui| {
                            ui.add(
                                ParameterSlider::new(&mut self.draft.center[a], -2000.0..=2000.0)
                                    .clamping(egui::SliderClamping::Edits)
                                    .max_decimals(3),
                            )
                        },
                    );
                }
                for (a, name) in ["Size X (m)", "Size Y (m)", "Size Z (m)"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "4–2048 m. XZ determines the computational domain; Y is visual only, and the top is not a closed lid. Applying XZ changes clears nearshore history; Y changes do not.",
                        |ui| {
                            ui.add(
                                ParameterSlider::new(&mut self.draft.size[a], 4.0..=2048.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .max_decimals(3),
                            )
                        },
                    );
                }
                parameter(
                    ui,
                    "Show Bounds",
                    "Show an orange 3D wireframe after applying. Visual only; does not reset the simulation.",
                    |ui| ui.checkbox(&mut self.draft.show_bounds, ""),
                );
                parameter(
                    ui,
                    "Pending Grid Spacing X/Z",
                    "In meters; select the grid count per axis in the SWE panel. Important channels should span at least 3–5 cells. Increasing resolution cannot restore detail missing from a coarse source heightfield.",
                    |ui| {
                        ui.label(format!(
                            "{:.3} / {:.3}",
                            self.draft.size[0] / optics.shore.resolution as f32,
                            self.draft.size[2] / optics.shore.resolution as f32
                        ))
                    },
                );
            });
            if ui
                .add_enabled(
                    self.source_bounds.is_some(),
                    PanelButton::new("Fit to Model Bounds"),
                )
                .clicked()
            {
                self.draft.fit(self.source_bounds.unwrap());
            }
            yummy_egui::h3(ui, "Boundaries and Supplemental Seabed");
            parameter_table(ui, "model boundaries", |ui| {
                for (a, name) in ["−X Open", "+X Open", "−Z Open", "+Z Open"]
                    .into_iter()
                    .enumerate()
                {
                    parameter(
                        ui,
                        name,
                        "On: FFT incoming waves and outflow boundary with a relaxation zone. Off: reflective wall. Applying resets nearshore history. FFT is the only external wave source.",
                        |ui| ui.checkbox(&mut self.draft.open_edges[a], ""),
                    );
                }
                parameter(
                    ui,
                    "Relaxation Width (m)",
                    "0.5–512 m, less than half the smaller X/Z extent. Check reflections against the actual wavelength; applying resets the simulation.",
                    |ui| {
                        ui.add(
                            ParameterSlider::new(&mut self.draft.sponge_width, 0.5..=512.0)
                                .logarithmic(true)
                                .clamping(egui::SliderClamping::Edits)
                                .max_decimals(3),
                        )
                    },
                );
                parameter(
                    ui,
                    "Seabed Blend Width (m)",
                    "0.5–256 m. The scan edge smoothly blends to the seabed depth from the Scene and Terrain panel over this distance. This is artificial supplemental terrain, not a measured seabed. Applying rebuilds and resets. Keep it inside the computational domain and relaxation zone.",
                    |ui| {
                        ui.add(
                            ParameterSlider::new(&mut self.draft.extension_width, 0.5..=256.0)
                                .logarithmic(true)
                                .clamping(egui::SliderClamping::Edits)
                                .max_decimals(3),
                        )
                    },
                );
            });
        }
        let [apply, cancel] = action_row(
            ui,
            [
                PanelButton::new("Apply Model and Domain"),
                PanelButton::new("Discard Pending Changes"),
            ],
        );
        if apply.clicked() {
            match self.draft.validate() {
                Ok(()) => {
                    optics.model = self.draft;
                    self.applied = Some(self.draft);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
        if cancel.clicked() {
            self.draft = optics.model;
            self.error = None;
        }
        if self.draft != optics.model {
            ui.label("Unapplied changes");
        }
        if let Some(range) = self.bed_range {
            let low = optics.model.center[1] - optics.model.size[1] * 0.5 - optics.model.sea_level;
            let high = optics.model.center[1] + optics.model.size[1] * 0.5 - optics.model.sea_level;
            if range[0] < low || range[1] > high {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "Bed extends outside the bounds in Y: warning only; it is not clipped.",
                );
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
}

//! Isolated, non-persistent slider appearance experiment.
mod slider;
mod window;

pub use window::run;

use slider::Appearance;
use yummy_egui::{group, parameter, parameter_table};

struct Demo {
    appearance: Appearance,
    height: f32,
    values: [f32; 5],
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            appearance: Appearance {
                background_radius: Some(2.0),
                marker_radius: Some(2.0),
                ..Default::default()
            },
            height: 24.0,
            values: [0.55, 1.2, 15.0, 0.0, 1.0],
        }
    }
}

impl Demo {
    fn ui(&mut self, root_ui: &mut egui::Ui) {
        egui::Panel::top("title").show_inside(root_ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                yummy_egui::h3(ui, "Rectangular Slider · Appearance Lab");
                ui.separator();
                ui.label("Standalone window · No settings read or written · No ocean simulation");
            });
        });
        egui::Panel::left("appearance")
            .default_size(340.0)
            .size_range(300.0..=420.0)
            .show_inside(root_ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
            });
        egui::CentralPanel::default().show_inside(root_ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.preview(ui));
        });
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        yummy_egui::h3(ui, "Appearance Settings (Live)");
        let style = &mut self.appearance;
        group(ui, "Background and Value Field", |ui| {
            parameter(
                ui,
                "Control height",
                "20–40 pt (logical pixels). The background matches the value field's actual height; minimum font height remains constrained by egui. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut self.height, 20.0..=40.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "Value field width",
                "48–100 pt. Adjusts only the right input field; the track uses the remaining control column width. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.value_width, 48.0..=100.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "End padding",
                "1–24 pt. Distance from the hovered handle's outer edge to the background edge at minimum/maximum values; normal spacing is slightly larger. The full value range is unchanged. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.edge_padding, 1.0..=24.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "Background rounding",
                "0–12 pt. Background corner radius; 0 is square. Appearance only; applies immediately.",
                |ui| {
                    ui.add(
                        egui::Slider::new(style.background_radius.get_or_insert(2.0), 0.0..=12.0)
                            .suffix(" pt"),
                    );
                },
            );
            parameter(
                ui,
                "Background color",
                "Uniform opaque color for the whole track, without progress fill. Applies immediately.",
                |ui| color(ui, &mut style.background),
            );
        });
        group(ui, "Normal Handle", |ui| {
            parameter(
                ui,
                "Width",
                "1–6 pt, a thin solid handle. Hover width is always at least 1 pt wider. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.width, 1.0..=6.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "Handle rounding",
                "0–6 pt. Shared normal and hover corner radius, limited to half the smaller dimension; rounded ends by default. Applies immediately.",
                |ui| {
                    ui.add(
                        egui::Slider::new(style.marker_radius.get_or_insert(2.0), 0.0..=6.0)
                            .suffix(" pt"),
                    );
                },
            );
            parameter(
                ui,
                "Vertical inset",
                "1–8 pt inset from each background edge; normal height = background height − 2 × inset. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.inset, 1.0..=8.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "Color",
                "Opaque normal handle color. Applies immediately; a lower brightness than the hover color is recommended.",
                |ui| color(ui, &mut style.marker),
            );
        });
        style.hover_width = style.hover_width.max(style.width + 1.0);
        group(ui, "Hover / Drag Handle", |ui| {
            parameter(
                ui,
                "Width",
                "Normal width + 1–12 pt. Widens on hover; space is reserved at both ends for the maximum width to avoid clipping. Applies immediately.",
                |ui| {
                    ui.add(
                        egui::Slider::new(&mut style.hover_width, style.width + 1.0..=12.0)
                            .suffix(" pt"),
                    );
                },
            );
            parameter(
                ui,
                "Vertical extension",
                "0.5–6 pt beyond each background edge; hover height = background height + 2 × extension. Space is reserved between rows without pushing adjacent controls. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.outset, 0.5..=6.0).suffix(" pt"));
                },
            );
            parameter(
                ui,
                "Color",
                "Handle color during hover and drag, brighter than normal by default. Adjust freely for comparison; colors are not automatically corrected. Applies immediately.",
                |ui| color(ui, &mut style.hover_marker),
            );
            parameter(
                ui,
                "Transition duration",
                "0–300 ms (wall time). Width, height and color transition together; 0 switches immediately. Applies immediately.",
                |ui| {
                    ui.add(egui::Slider::new(&mut style.animation_ms, 0.0..=300.0).suffix(" ms"));
                },
            );
        });
        ui.add_space(12.0);
        if ui.button("Restore Default Appearance").clicked() {
            let defaults = Self::default();
            self.appearance = defaults.appearance;
            self.height = defaults.height;
        }
    }

    fn preview(&mut self, ui: &mut egui::Ui) {
        yummy_egui::h3(ui, "Interactive Preview");
        parameter_table(ui, "preview sliders", |ui| {
            for (i, (name, range)) in [
                ("Blend ratio", 0.0..=1.0),
                ("Amplitude (m)", 0.0..=3.0),
                ("Angle (degrees)", -180.0..=180.0),
                ("Minimum endpoint", 0.0..=1.0),
                ("Maximum endpoint", 0.0..=1.0),
            ]
            .into_iter()
            .enumerate()
            {
                parameter(
                    ui,
                    name,
                    "Appearance example only; does not control the simulation. Uses the same control as the main parameter panels. Click or drag the track, or enter a value on the right; use Tab and arrow keys for fine adjustment with native egui interaction.",
                    |ui| {
                        slider::slider(
                            ui,
                            &mut self.values[i],
                            range,
                            &self.appearance,
                            self.height,
                        );
                    },
                );
            }
        });
        ui.add_space(18.0);
        yummy_egui::h3(ui, "State Comparison (Static, Not Draggable)");
        parameter_table(ui, "state comparison", |ui| {
            parameter(
                ui,
                "Normal",
                "Always shows the normal state for comparison with hover. Uses the same drawing method as the interactive preview above.",
                |ui| {
                    slider::swatch(ui, &self.appearance, self.height, false);
                },
            );
            parameter(
                ui,
                "Hover / Drag",
                "Always shows the final hover state regardless of cursor position; use the interactive preview above to trigger it.",
                |ui| {
                    slider::swatch(ui, &self.appearance, self.height, true);
                },
            );
        });
        ui.add_space(18.0);
        yummy_egui::h3(ui, "Native Slider Comparison (Same Blend Ratio)");
        parameter_table(ui, "native comparison", |ui| {
            parameter(
                ui,
                "Blend ratio",
                "Native egui Slider sharing the value of the first row above, with the same theme.",
                |ui| {
                    ui.add(egui::Slider::new(&mut self.values[0], 0.0..=1.0));
                },
            );
        });
    }
}

fn color(ui: &mut egui::Ui, value: &mut egui::Color32) {
    let mut rgb = [value.r(), value.g(), value.b()];
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *value = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    }
}

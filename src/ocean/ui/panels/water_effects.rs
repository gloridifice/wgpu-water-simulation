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
    if segmented_control(ui,&mut selection,&[(0,"Off"),(1,"Fast"),(2,"Physical")]).on_hover_text("Off disables the effect; Fast uses a UV approximation, while Physical intersects the wave surface with the receiving surface. Changes apply immediately, require manual saving, do not reset the simulation, and preserve each mode's parameters.").changed() {
        *enabled=selection!=0;
        if *enabled {*method=if selection==1 {OpticalMethod::Fast} else {OpticalMethod::Physical};}
    }
}
impl Gui {
    pub(in crate::ocean::ui) fn caustics_panel(&mut self, ui: &mut egui::Ui) {
        if self.optics.lake.enabled {
            ui.label("Lake mode does not yet support physical caustics; existing parameters are preserved. Configure wave reflections under Animated SDF Ripples.");
            return;
        }
        let o = &mut self.optics;
        segmented_control(ui, &mut o.caustics, &[(false, "Off"), (true, "Physical")])
            .on_hover_text("Toggle physical caustics immediately, preserving parameters without resetting the simulation.");
        if o.caustics {
            group(ui, "Caustics Parameters", |ui| {
                number(
                    ui,
                    "Caustics Intensity",
                    &mut o.caustics_strength,
                    0.0..=4.0,
                    false,
                );
                number(
                    ui,
                    "Caustics Coverage Width (m)",
                    &mut o.caustics_extent,
                    8.0..=512.0,
                    true,
                );
                number(
                    ui,
                    "Caustics Softening (texels)",
                    &mut o.caustics_softness,
                    0.0..=4.0,
                    false,
                );
                list(
                    ui,
                    "Caustics Beam Grid",
                    &mut o.caustics_resolution,
                    &[(128, "128 × 128"), (256, "256 × 256"), (512, "512 × 512")],
                );
                number(
                    ui,
                    "Water Refractive Index",
                    &mut o.water_ior,
                    1.0..=1.6,
                    false,
                );
            });
        }
        group(ui, "Caustics Diagnostics", |ui| {
            parameter(
                ui,
                "Display",
                "Switch the shared Render View immediately without resetting. Incident light excludes receiver material and viewing-path extinction.",
                |ui| {
                    segmented_control(ui, &mut o.view, &[(0, "Normal"), (12, "Incident Light")]);
                },
            );
        });
    }
    pub(in crate::ocean::ui) fn refraction_panel(&mut self, ui: &mut egui::Ui) {
        let o = &mut self.optics;
        refraction_mode(ui, &mut o.refraction, &mut o.refraction_method);
        if o.lake.enabled && o.refraction && o.refraction_method == OpticalMethod::Physical {
            ui.label("Lake mode does not yet support physical refraction; this setting is preserved but inactive. Fast refraction is available.");
            return;
        }
        if o.refraction {
            group(ui, "Refraction Parameters", |ui| {
                number(
                    ui,
                    "Water Refractive Index",
                    &mut o.water_ior,
                    1.0..=1.6,
                    false,
                );
                if o.refraction_method == OpticalMethod::Fast {
                    number(
                        ui,
                        "Refraction Distortion Strength",
                        &mut o.fast_refraction_strength,
                        0.0..=2.0,
                        false,
                    );
                    number(
                        ui,
                        "Full Refraction Distortion Depth (m)",
                        &mut o.fast_refraction_depth,
                        0.01..=20.0,
                        true,
                    );
                    let mut percent = o.fast_refraction_limit * 100.0;
                    number(
                        ui,
                        "Maximum Refraction Offset (% of screen height)",
                        &mut percent,
                        0.0..=20.0,
                        false,
                    );
                    o.fast_refraction_limit = percent / 100.0;
                }
                number(
                    ui,
                    "Maximum Refraction Path Length (m)",
                    &mut o.refraction_distance,
                    1.0..=500.0,
                    true,
                );
            });
        }
        group(ui, "Refraction Diagnostics", |ui| {
            parameter(
                ui,
                "Display",
                "Switch the shared Render View immediately; green indicates valid samples, red indicates fallback, and gray indicates disabled. No reset.",
                |ui| {
                    segmented_control(
                        ui,
                        &mut o.view,
                        &[(0, "Normal"), (13, "Direction"), (14, "Hit")],
                    );
                },
            );
        });
    }
}

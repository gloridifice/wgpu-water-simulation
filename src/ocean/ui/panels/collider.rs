use crate::ocean::parameters::optics::OpticsConfig;
use crate::ocean::scene::collider::Collider;
use crate::ocean::scene::collider::Shape;
use yummy_egui::{PanelButton, ParameterSlider, action_row, choices, parameter, parameter_table};

pub fn show(ui: &mut egui::Ui, optics: &mut OpticsConfig, domain: [f32; 4]) {
    let resolution = optics.shore.resolution;
    let model = optics.model;
    let interior = if model.enabled {
        model.interior(resolution)
    } else {
        [
            domain[0],
            domain[1],
            domain[0] + 0.78 * domain[2],
            domain[1] + domain[3],
        ]
    };
    yummy_egui::h3(ui, "Static Colliders");
    parameter_table(ui, "collider actions", |ui| {
        parameter(
            ui,
            "Status",
            "Simulated and displayed in beach or model scenes when SWE is enabled. Fixed to the bed; blocks water below its top and allows overtopping. Floating and rigid-body motion are not simulated.",
            |ui| {
                ui.label(if optics.swe_enabled() {
                    "Enabled"
                } else {
                    "Enable a coastal scene and SWE"
                });
            },
        );
        parameter(
            ui,
            "Valid X Range (m)",
            "The entire shape must lie inside the local SWE domain and outside the offshore FFT input band. One cell is also reserved at the edge.",
            |ui| ui.label(format!("{:.1} – {:.1}", interior[0], interior[2])),
        );
        parameter(
            ui,
            "Valid Z Range (m)",
            "Alongshore range, with one cell reserved at the edge. Outside this range the configuration is retained, but the collider is not displayed or simulated.",
            |ui| ui.label(format!("{:.1} – {:.1}", interior[1], interior[3])),
        );
        parameter(
            ui,
            "Grid Spacing X/Z (m)",
            "Colliders should span at least 3 cells in both directions. Shapes are sampled on the grid, so very small shapes may be missed.",
            |ui| {
                ui.label(format!(
                    "{:.2} / {:.2}",
                    domain[2] / resolution as f32,
                    domain[3] / resolution as f32
                ))
            },
        );
        parameter(
            ui,
            "Add",
            "Up to 8. By default, placed 8 m seaward of the shoreline. Edits apply immediately, require manual save, and do not reset SWE; only water where the bed changes is adjusted, preserving other history. These are not continuously moving rigid bodies.",
            |ui| {
                let can_add = optics.shore.colliders.iter().any(Option::is_none);
                let responses = action_row(
                    ui,
                    [
                        PanelButton::new("Cylinder").enabled(can_add),
                        PanelButton::new("Box").enabled(can_add),
                    ],
                );
                for (response, shape) in responses.into_iter().zip([Shape::Cylinder, Shape::Box]) {
                    if response.clicked()
                        && let Some((index, slot)) = optics
                            .shore
                            .colliders
                            .iter_mut()
                            .enumerate()
                            .find(|(_, c)| c.is_none())
                    {
                        *slot = Some(Collider {
                            shape,
                            position: if model.enabled {
                                [model.center[0], model.center[2] + index as f32 * 8.0]
                            } else {
                                [optics.shore_x + 8.0, index as f32 * 8.0]
                            },
                            ..Default::default()
                        });
                    }
                }
            },
        );
    });
    let mut remove = None;
    for (index, item) in optics.shore.colliders.iter_mut().enumerate() {
        let Some(c) = item else {
            continue;
        };
        ui.separator();
        ui.push_id(index, |ui| {
            yummy_egui::h3(ui, format!("Collider {}", index + 1));
            parameter_table(ui, "collider parameters", |ui| {
                parameter(ui, "Enabled", "Toggle immediately, even while paused. Save manually; does not reset SWE.", |ui| ui.checkbox(&mut c.enabled, ""));
                parameter(ui, "Shape", "Cylinder or rotatable box, anchored to the bed. Applies immediately, requires manual save, and does not reset SWE.", |ui| {
                    choices(ui, &mut c.shape, &[(Shape::Cylinder, "Cylinder"), (Shape::Box, "Box")]);
                });
                for (axis, label) in [(0, "Position X (m)"), (1, "Position Z (m)")] {
                    parameter(ui, label, "World coordinates, range ±1000 m. Increasing X points offshore; Z runs alongshore. Applies immediately, requires manual save, and does not reset SWE. The entire shape must lie in the valid range.", |ui| ui.add(ParameterSlider::new(&mut c.position[axis], -1000.0..=1000.0).clamping(egui::SliderClamping::Edits).max_decimals(3)));
                }
                if c.shape == Shape::Cylinder {
                    parameter(ui, "Radius (m)", "Range 0.25–30 m; applies immediately, requires manual save, and does not reset SWE. Shapes smaller than the grid spacing may not be resolved.", |ui| ui.add(ParameterSlider::new(&mut c.radius, 0.25..=30.0).logarithmic(true).clamping(egui::SliderClamping::Edits).max_decimals(3)));
                } else {
                    for (axis, label) in [(0, "Width (m)"), (1, "Length (m)")] {
                        parameter(ui, label, "Full size, range 0.5–60 m; applies immediately, requires manual save, and does not reset SWE.", |ui| ui.add(ParameterSlider::new(&mut c.size[axis], 0.5..=60.0).logarithmic(true).clamping(egui::SliderClamping::Edits).max_decimals(3)));
                    }
                    parameter(ui, "Rotation (degrees)", "Rotation around the vertical axis, range ±180 degrees; applies immediately, requires manual save, and does not reset SWE.", |ui| ui.add(ParameterSlider::new(&mut c.angle, -180.0..=180.0)));
                }
                parameter(ui, "Height (m)", "Range 0.1–30 m, measured from the original bed at the center to the horizontal top. Water can flow over the top. Applies immediately, requires manual save, and does not reset SWE.", |ui| ui.add(ParameterSlider::new(&mut c.height, 0.1..=30.0).logarithmic(true).clamping(egui::SliderClamping::Edits).max_decimals(3)));
                parameter(ui, "Top Elevation (m)", "Bed elevation at the center plus collider height, relative to sea level. Model scenes use the resampled bed, not the beach slope formula.", |ui| ui.label(if model.enabled {"Baked bed + height".to_string()} else {format!("{:.2}", -optics.beach_slope * (c.position[0] - optics.shore_x) + c.height)}));
                parameter(ui, "Simulation Status", "Colliders outside the valid region retain their configuration but are not displayed or simulated. The offshore FFT has no collision feedback.", |ui| {
                    let extent=c.extent();
                    let inside=if model.enabled {(0..2).all(|a|c.position[a]-extent[a]>=interior[a] && c.position[a]+extent[a]<=interior[a+2])} else {c.in_domain(domain,resolution)};
                    ui.label(if !c.enabled { "Disabled" } else if !inside { "Outside valid region" } else { "Inside valid region" });
                });
                parameter(ui, "Action", "Deletion applies immediately, requires manual save, and does not reset SWE; surrounding water fills the vacated area as simulation continues.", |ui| { if ui.add(PanelButton::new("Delete")).clicked() { remove = Some(index); } });
            });
            let size = if c.shape == Shape::Cylinder { c.radius * 2.0 } else { c.size[0].min(c.size[1]) };
            if size < 3.0 * domain[2].max(domain[3]) / resolution as f32 {
                ui.label("Small shape; increase its size for better grid resolution.");
            }
        });
    }
    if let Some(index) = remove {
        optics.shore.colliders[index] = None;
    }
}

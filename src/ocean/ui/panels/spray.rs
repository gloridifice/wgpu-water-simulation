use crate::ocean::parameters::spray::SprayConfig;
use yummy_egui::{ParameterSlider, group, parameter};

fn sliders(ui: &mut egui::Ui, rows: &[(&str, &str)], values: &mut [(&mut f32, f32, f32)]) {
    for ((name, tip), (value, min, max)) in rows.iter().zip(values) {
        parameter(ui, name, tip, |ui| {
            ui.add(ParameterSlider::new(*value, *min..=*max).max_decimals(3))
        });
    }
}

pub fn show(ui: &mut egui::Ui, config: &mut SprayConfig, error: Option<&str>) {
    group(ui, "GPU Spray Particles", |ui| {
        parameter(
            ui,
            "Enable Spray",
            "Textured nearshore SWE spray, disabled by default. Changes apply immediately; disabling clears particles without changing water or foam.",
            |ui| ui.checkbox(&mut config.enabled, ""),
        );
    });
    if let Some(error) = error {
        ui.colored_label(egui::Color32::RED, error);
    }
    if !config.enabled {
        return;
    }
    group(ui, "Emission", |ui| {
        sliders(
            ui,
            &[
                (
                    "Emission Density",
                    "0–2000 particles per square meter per simulation second, default 150; multiplied by slope, rise and depth gates. Affects new particles immediately; 0 stops emission.",
                ),
                (
                    "Slope Threshold",
                    "Surface height difference per horizontal distance, 0–2, default 0.03; transition width is fixed at 0.1. Not a depth gradient. Affects emission immediately.",
                ),
                (
                    "Rise Threshold (m/s)",
                    "0–5 m per simulation second, default 0.02; transition width 0.2 m/s. Emits only on rising surfaces. Applies immediately.",
                ),
                (
                    "Minimum Depth (m)",
                    "0.001–1 m, default 0.02; no emission below this depth, reaching full weight at twice this depth. Applies immediately.",
                ),
            ],
            &mut [
                (&mut config.rate, 0.0, 2000.0),
                (&mut config.slope, 0.0, 2.0),
                (&mut config.rise, 0.0, 5.0),
                (&mut config.depth, 0.001, 1.0),
            ],
        );
    });
    group(ui, "Motion and Lifetime", |ui| {
        sliders(
            ui,
            &[
                (
                    "Upward Speed (m/s)",
                    "0–10 m per simulation second, default 2, plus a small deterministic perturbation. Affects new particles immediately.",
                ),
                (
                    "Forward Speed (m/s)",
                    "0–10 m per simulation second, default 0.5; added along the flow, with no forward component in still water. Affects new particles immediately.",
                ),
                (
                    "Maximum Lifetime (s)",
                    "0.05–5 simulation seconds, default 1.2; actual lifetime is 75%–100% of this, and particles can disappear earlier on contact with water. Affects new particles only.",
                ),
                (
                    "Air Drag (per second)",
                    "0–10 per simulation second, default 0.5; linear drag in still air. At 0, particles follow gravity-only trajectories. Affects all particles immediately.",
                ),
            ],
            &mut [
                (&mut config.upward, 0.0, 10.0),
                (&mut config.forward, 0.0, 10.0),
                (&mut config.lifetime, 0.05, 5.0),
                (&mut config.drag, 0.0, 10.0),
            ],
        );
    });
    group(ui, "Spray Diagnostics", |ui| {
        parameter(
            ui,
            "Emission Intensity View",
            "GPU emission density divided by full-weight density: green is low, red is high. Does not read foam density. Shows the last simulation frame's emission area, retained while paused. Applies immediately.",
            |ui| ui.checkbox(&mut config.debug, ""),
        );
    });
}

pub fn appearance(ui: &mut egui::Ui, config: &mut SprayConfig) {
    group(ui, "Size and Soft Intersections", |ui| {
        sliders(
            ui,
            &[
                (
                    "Size (m)",
                    "Base quad diameter, 0.01–2 m, default 0.2. Final diameter = size × random size multiplier, so it may exceed 2 m. Affects new particles immediately, not existing particles; saved in the configuration.",
                ),
                (
                    "Soft Intersection Distance (m)",
                    "0.001–1 m, default 0.15; particles fade near scene depth. Affects rendering only, immediately.",
                ),
            ],
            &mut [
                (&mut config.size, 0.01, 2.0),
                (&mut config.softness, 0.001, 1.0),
            ],
        );
        let [minimum, maximum] = &mut config.size_range;
        parameter(
            ui,
            "Minimum Random Size Multiplier",
            "Dimensionless, 0–2, default 0.65; cannot exceed the maximum. New particles sample uniformly between the bounds; equal bounds give a fixed multiplier. Applies immediately, requires manual save, and does not reset the simulation.",
            |ui| ui.add(ParameterSlider::new(&mut *minimum, 0.0..=*maximum).max_decimals(3)),
        );
        parameter(
            ui,
            "Maximum Random Size Multiplier",
            "Dimensionless, 0–2, default 1.35; cannot be below the minimum. For example, size 2 m with multiplier 0.5–1 produces 1–2 m particles. Affects new particles only, applies immediately, and requires manual save.",
            |ui| ui.add(ParameterSlider::new(&mut *maximum, *minimum..=2.0).max_decimals(3)),
        );
    });
}

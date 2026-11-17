use crate::ocean::{
    parameters::lake::{LakeConfig, PropagationMode},
    ui::gui::Gui,
};
use yummy_egui::{PanelButton, ParameterSlider, action_row, group, parameter, segmented_control};

#[derive(Default)]
pub struct LakePanel {
    draft: LakeConfig,
    source: Option<LakeConfig>,
    pub emit: bool,
    pub clear: bool,
    pub reset: bool,
    pub open: bool,
    pub open_ripples: bool,
    pub error: Option<String>,
    pub status: (usize, u64, u32),
}
fn number(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    hint: &str,
) {
    parameter(ui, label, hint, |ui| {
        ui.add(ParameterSlider::new(value, range).clamping(egui::SliderClamping::Edits))
    });
}
impl Gui {
    pub(in crate::ocean::ui) fn lake_panel(&mut self, ui: &mut egui::Ui) {
        self.lake_panel.show_terrain(ui, &mut self.optics.lake);
    }
    pub(in crate::ocean::ui) fn animated_sdf_panel(&mut self, ui: &mut egui::Ui) {
        if !self.optics.lake.enabled {
            ui.label("Only available in the Lake scene; first select Lake in the Scene category.");
            return;
        }
        self.lake_panel.show_ripples(ui, &mut self.optics.lake);
    }
}
impl LakePanel {
    fn sync_draft(&mut self, config: &LakeConfig) {
        if let Some(source) = self.source {
            // Applying one panel must not discard the other panel's pending edits.
            if source.terrain != config.terrain {
                self.draft.terrain = config.terrain;
            }
            if source.mode != config.mode {
                self.draft.mode = config.mode;
            }
            if source.wave_seed != config.wave_seed {
                self.draft.wave_seed = config.wave_seed;
            }
        } else {
            self.draft = *config;
        }
        self.source = Some(*config);
    }
    fn apply(&mut self, config: &mut LakeConfig, next: LakeConfig) {
        match next.validate() {
            Ok(()) => {
                *config = next;
                self.source = Some(next);
                self.reset = true;
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn show_terrain(&mut self, ui: &mut egui::Ui, config: &mut LakeConfig) {
        self.sync_draft(config);
        let panel = self;
        let d = &mut panel.draft;
        group(ui, "Lake Terrain (reset on apply)", |ui| {
            number(
                ui,
                "Lake radius (m)",
                &mut d.terrain.radius,
                8.0..=100.0,
                "Base radius 8–100 m; shoreline noise varies around it. Applying rebuilds the terrain and clears ripples.",
            );
            number(
                ui,
                "Maximum depth (m)",
                &mut d.terrain.depth,
                0.5..=12.0,
                "Base depth of the bowl-shaped lake bed, 0.5–12 m; rebuilt on apply.",
            );
            number(
                ui,
                "Shoreline irregularity",
                &mut d.terrain.irregularity,
                0.0..=0.35,
                "Dimensionless radial periodic noise ratio, 0–0.35; rebuilt on apply.",
            );
            parameter(
                ui,
                "Scene seed",
                "Deterministic terrain and rock seed; rebuilt on apply.",
                |ui| ui.add(ParameterSlider::new(&mut d.terrain.seed, 0..=100000)),
            );
            parameter(
                ui,
                "Grid resolution",
                "Grid shared by the scene and wave field; rebuilt on apply. 512 is more expensive.",
                |ui| {
                    segmented_control(
                        ui,
                        &mut d.terrain.resolution,
                        &[(128, "128"), (256, "256"), (512, "512")],
                    )
                },
            );
            parameter(
                ui,
                "Rock count",
                "0–32; skipped if spacing is insufficient, with the actual count shown in the status. Every third rock attempts underwater placement. Rebuilt on apply.",
                |ui| ui.add(ParameterSlider::new(&mut d.terrain.rocks, 0..=32)),
            );
            number(
                ui,
                "Rock size (m)",
                &mut d.terrain.rock_size,
                0.5..=8.0,
                "Base horizontal rock radius, 0.5–8 m; rebuilt on apply. Rocks use single-layer height geometry without overhangs.",
            );
        });
        if ui
            .add(PanelButton::new("Apply terrain and reset"))
            .clicked()
        {
            let mut next = *config;
            next.terrain = d.terrain;
            panel.apply(config, next);
        }
        if let Some(e) = &panel.error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        group(ui, "Shared FFT waves", |ui| {
            number(
                ui,
                "FFT overlay strength",
                &mut config.fft_strength,
                0.0..=1.0,
                "Dimensionless multiplier for shared spectrum height, 0–1; takes effect immediately without resetting shore waves. 0 disables FFT. Edit and apply the spectrum in the Offshore category’s Spectrum and Waveform panel; both scenes share it. The lake overlays only vertical wave height, with optional shore and shallow-water fading; it does not move the shoreline or generate foam. Clearing ripples clears only shore waves, not FFT.",
            );
            parameter(
                ui,
                "FFT shore fade",
                "Takes effect immediately without resetting. When enabled, attenuates FFT at depths of 0–0.5 m and shore distances of 0–1 m; when disabled, retains full strength while still masking dry land and preventing troughs from penetrating the lake bed.",
                |ui| ui.checkbox(&mut config.fft_shore_fade, "Enabled"),
            );
        });
        group(ui, "Cartoon shore foam", |ui| {
            number(
                ui,
                "Foam strength",
                &mut config.foam_strength,
                0.0..=1.0,
                "Overlay opacity, dimensionless 0–1; 0 disables it. Takes effect immediately without resetting; shown only on shore or exposed-rock sides where SDF increases along the inward flow, without advecting simulated density.",
            );
            number(
                ui,
                "Foam width (m)",
                &mut config.foam_width,
                0.1..=10.0,
                "Width of the net-like foam band along shores and exposed rocks, 0.1–10 m; takes effect immediately without resetting and varies slightly with wave height.",
            );
            number(
                ui,
                "Mesh cell size (m)",
                &mut config.foam_size,
                0.1..=5.0,
                "Base size of the cellular mesh pattern, 0.1–5 m; takes effect immediately without resetting. Larger values produce larger holes; animation pauses with the lake.",
            );
            number(
                ui,
                "Inward flow speed (m/s)",
                &mut config.foam_flow_speed,
                0.0..=2.0,
                "Foam UVs flow from shore toward the lake center at 0–2 m per simulation second, slowing smoothly at the center. 0 disables UV flow; takes effect immediately without resetting. Edits may change the current pattern; this is not a physical flow speed.",
            );
            number(
                ui,
                "Flow field distortion",
                &mut config.foam_flow_distortion,
                0.0..=1.0,
                "Nearshore velocity gradient strength, dimensionless 0–1, default 1. Higher values slow flow near shore and accelerate it offshore; integrating texture coordinates along a simplified inward field produces stretching and shear. 0 restores approximately uniform flow, still inward. Takes effect immediately without resetting; edits change the current pattern but not wave height, foam density, or the direction mask.",
            );
            number(
                ui,
                "Distortion period (s)",
                &mut config.foam_flow_period,
                1.0..=20.0,
                "Cycle period for each of the two UV sets, 1–20 simulation seconds, offset by half a cycle and crossfaded with a smooth triangle wave. Longer periods increase maximum displacement. Takes effect immediately without resetting; edits remap the phase, and pausing freezes animation.",
            );
        });
        if ui
            .add(PanelButton::new("Open Animated SDF Ripples"))
            .clicked()
        {
            panel.open_ripples = true;
        }
    }
    fn show_ripples(&mut self, ui: &mut egui::Ui, config: &mut LakeConfig) {
        self.sync_draft(config);
        let panel = self;
        let d = &mut panel.draft;
        group(ui, "Propagation field (reset on apply)", |ui| {
            parameter(
                ui,
                "Propagation field",
                "Depth is normalized water depth; SDF is the normalized Euclidean distance to the outer shore and exposed rock boundaries. This is not a diffraction or travel-time solver. Applying clears ripples.",
                |ui| {
                    segmented_control(
                        ui,
                        &mut d.mode,
                        &[
                            (PropagationMode::Depth, "Depth"),
                            (PropagationMode::Sdf, "SDF"),
                        ],
                    )
                },
            );
            parameter(
                ui,
                "Ripple random seed",
                "Independent of the terrain seed; applying restarts the wave sequence.",
                |ui| ui.add(ParameterSlider::new(&mut d.wave_seed, 0..=100000)),
            );
        });
        if ui
            .add(PanelButton::new("Apply propagation field / seed and reset"))
            .clicked()
        {
            let mut next = *config;
            next.mode = d.mode;
            next.wave_seed = d.wave_seed;
            panel.apply(config, next);
        }
        if let Some(e) = &panel.error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        group(ui, "Ripple emission (affects new waves)", |ui| {
            parameter(
                ui,
                "Automatic emission",
                "Emits in simulation time and stops when paused; enabling or disabling does not clear existing waves.",
                |ui| ui.checkbox(&mut config.automatic, "Enabled"),
            );
            number(
                ui,
                "Emission interval (s)",
                &mut config.interval,
                0.1..=30.0,
                "0.1–30 simulation seconds; editing reschedules the next emission from the current time without making up missed waves.",
            );
            number(
                ui,
                "World direction angle (degrees)",
                &mut config.direction_deg,
                -180.0..=180.0,
                "0° points toward world +X, 90° toward +Z; marks a sector centered on the lake. Reflection does not flip the sector; affects new waves only.",
            );
            number(
                ui,
                "Angular half-width (degrees)",
                &mut config.spread_deg,
                5.0..=150.0,
                "Base half-width of the lake-centered sector, 5–150°, not a fixed world-space length. Clamped to the same range after sector width randomization. New waves are thick in the middle and thin at the ends.",
            );
            number(
                ui,
                "Direction random deviation (degrees)",
                &mut config.jitter_deg,
                0.0..=180.0,
                "Positive and negative random angular range for new wave directions, 0–180°; 0 disables it. Reproducible with a fixed seed; affects new waves only.",
            );
            number(
                ui,
                "Sector width randomness",
                &mut config.spread_jitter,
                0.0..=1.0,
                "Positive and negative random ratio for angular half-width, dimensionless 0–1, default 0.35; 0 disables it. Independently sampled and clamped to a final half-width of 5–150°; affects new waves only.",
            );
            number(
                ui,
                "Width randomness",
                &mut config.width_jitter,
                0.0..=1.0,
                "Positive and negative random ratio for propagation-coordinate width, dimensionless 0–1, default 0.35; 0 disables it. Final width is clamped to 0.005–0.15; affects new waves only.",
            );
            number(
                ui,
                "Start position random offset",
                &mut config.position_jitter,
                0.0..=0.3,
                "Positive and negative random offset of the nearshore emission coordinate, normalized 0–0.3, default 0.04; 0 disables it. The start is clamped between 0.005 and the inner turnaround coordinate minus 0.005, conforming to depth contours or SDF without shifting the lake center; affects new waves only.",
            );
        });
        group(
            ui,
            "Waveform and reflection (affects new waves and their descendants)",
            |ui| {
                let p = match config.mode {
                    PropagationMode::Depth => &mut config.depth_wave,
                    PropagationMode::Sdf => &mut config.sdf_wave,
                };
                number(
                    ui,
                    "Inner turnaround coordinate",
                    &mut p.start,
                    0.1..=0.95,
                    "Dimensionless 0.1–0.95; 0 is near the boundary and 1 is the field maximum. Waves propagate from shore toward increasing coordinates and can reflect back upon arrival. Must exceed the nearshore emission coordinate; affects new waves only, saved independently for each mode.",
                );
                number(
                    ui,
                    "Propagation progress / second",
                    &mut p.speed,
                    0.005..=0.5,
                    "Dimensionless coordinates per simulation second, 0.005–0.5, not constant horizontal meters per second; affects new waves only.",
                );
                number(
                    ui,
                    "Propagation coordinate width",
                    &mut p.width,
                    0.005..=0.15,
                    "Base thickness of the central ripple in normalized field coordinates, 0.005–0.15, not meters. Width randomness is applied and clamped to the same range; affects new waves only.",
                );
                number(
                    ui,
                    "Nearshore emission coordinate",
                    &mut p.reflection,
                    0.005..=0.3,
                    "Dimensionless 0.005–0.3; waves spawn randomly near this coordinate and propagate into the lake. Must be less than the inner turnaround coordinate; reflected waves return to their actual starting point. Affects new waves only.",
                );
                number(
                    ui,
                    "Wave height (m)",
                    &mut config.amplitude,
                    0.0..=0.5,
                    "Crest amplitude 0–0.5 m; boundaries and shallow areas limit final displacement. This is non-conservative animation; affects new waves only.",
                );
                number(
                    ui,
                    "Tip width ratio",
                    &mut config.tip_ratio,
                    0.05..=1.0,
                    "Tip-to-center width ratio 0.05–1; height smoothly reaches zero at both ends of the angular envelope. Affects new waves only.",
                );
                parameter(
                    ui,
                    "Ripple reflection",
                    "Each incident wave generates at most one reverse wave; reflection is not SSR. Affects new waves only; existing descendant parameters are frozen.",
                    |ui| ui.checkbox(&mut config.reflect, "Enabled"),
                );
                if config.reflect {
                    number(
                        ui,
                        "Reflection amplitude ratio",
                        &mut config.reflectivity,
                        0.0..=0.95,
                        "Reflected-to-incident amplitude ratio 0–0.95; reflections gradually decay and do not reflect again.",
                    );
                    number(
                        ui,
                        "Reflection half-life (s)",
                        &mut config.reflected_half_life,
                        0.1..=30.0,
                        "Simulation seconds for amplitude to halve, 0.1–30; affects only reflected descendants of new waves.",
                    );
                }
                number(
                    ui,
                    "Transition time (s)",
                    &mut config.transition,
                    0.05..=2.0,
                    "Simulation time for incident fade-out and reflection fade-in, 0.05–2 seconds, avoiding sudden weakening; new waves also fade in.",
                );
            },
        );
        let [emit, clear] = action_row(
            ui,
            [
                PanelButton::new("Emit one"),
                PanelButton::new("Clear ripples"),
            ],
        );
        if emit.clicked() {
            if let Err(e) = config.validate() {
                panel.error = Some(e);
            } else {
                panel.emit = true;
            }
        }
        panel.clear |= clear.clicked();
        group(ui, "Diagnostics", |ui| {
            parameter(
                ui,
                "Display",
                "Switch immediately between depth, signed SDF (m), propagation contours, or wave height; does not reset ripples.",
                |ui| {
                    egui::ComboBox::from_id_salt("lake view")
                        .selected_text(
                            ["Normal", "Depth", "SDF", "Contours", "Wave height"]
                                [config.view as usize],
                        )
                        .show_ui(ui, |ui| {
                            for (v, label) in ["Normal", "Depth", "SDF", "Contours", "Wave height"]
                                .iter()
                                .enumerate()
                            {
                                ui.selectable_value(&mut config.view, v as u32, *label);
                            }
                        });
                },
            );
            parameter(
                ui,
                "Current status",
                "Up to 128 active waves; incident waves reserve capacity for reflections. New waves are rejected when the pool is full rather than overwriting visible waves. Configurations do not save playback progress.",
                |ui| {
                    ui.label(format!(
                        "{} waves / {} dropped / {} rocks",
                        panel.status.0, panel.status.1, panel.status.2
                    ))
                },
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mode_selection_is_a_draft_until_apply_and_actions_are_explicit() {
        let context = egui::Context::default();
        yummy_egui::Theme::default().apply(&context);
        crate::ocean::ui::fonts::install(&context);
        let mut panel = LakePanel::default();
        let mut config = LakeConfig {
            enabled: true,
            ..Default::default()
        };
        let mut frame = |events| {
            context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 2400.0),
                    )),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default()
                        .show_inside(root, |ui| panel.show_ripples(ui, &mut config));
                },
            )
        };
        let mut output = frame(vec![]);
        fn locate(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
            match shape {
                egui::epaint::Shape::Text(t) if t.galley.text() == label => {
                    Some(t.visual_bounding_rect().center())
                }
                egui::epaint::Shape::Vec(v) => v.iter().rev().find_map(|s| locate(s, label)),
                _ => None,
            }
        }
        let mut click = |label: &str| {
            let pos = output
                .shapes
                .iter()
                .rev()
                .find_map(|s| locate(&s.shape, label))
                .unwrap_or_else(|| panic!("missing control {label}"));
            for pressed in [true, false] {
                output = frame(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]);
            }
        };
        click("SDF");
        // Observe state between clicks without introducing test-only production APIs.
        assert_eq!(config.mode, PropagationMode::Depth);
        assert!(!panel.reset);
        assert_eq!(panel.draft.mode, PropagationMode::Sdf);
        // A pending terrain edit belongs to the other panel, not this apply action.
        let original_radius = config.terrain.radius;
        panel.draft.terrain.radius = original_radius + 4.0;
        let pos = output
            .shapes
            .iter()
            .rev()
            .find_map(|s| locate(&s.shape, "Apply propagation field / seed and reset"))
            .unwrap();
        for pressed in [true, false] {
            let _ = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600.0, 2400.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default()
                        .show_inside(root, |ui| panel.show_ripples(ui, &mut config));
                },
            );
        }
        assert_eq!(config.mode, PropagationMode::Sdf);
        assert_eq!(config.terrain.radius, original_radius);
        assert!(panel.reset);
        assert!(!panel.emit && !panel.clear);
        panel.reset = false;
        let _ = context.run_ui(egui::RawInput::default(), |root| {
            egui::CentralPanel::default()
                .show_inside(root, |ui| panel.show_terrain(ui, &mut config));
        });
        assert_eq!(panel.draft.terrain.radius, original_radius + 4.0);
        assert!(!panel.reset);
    }
}

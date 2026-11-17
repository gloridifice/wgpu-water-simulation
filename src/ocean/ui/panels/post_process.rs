use crate::ocean::ui::gui::Gui;
use yummy_egui::{ParameterSlider, group, parameter};

impl Gui {
    pub(super) fn post_process_controls(&mut self, ui: &mut egui::Ui) {
        group(ui, "HDR and Bloom", |ui| {
            let config = &mut self.optics.post_process;
            parameter(
                ui,
                "Enable HDR Post-processing",
                "Master post-processing switch, off by default. AgX mapping and Bloom can be controlled independently when enabled; output remains SDR. Debug views bypass processing automatically, and the UI is excluded. Applies immediately without resetting the simulation; save manually.",
                |ui| ui.checkbox(&mut config.enabled, ""),
            );
            if !config.enabled {
                return;
            }
            parameter(
                ui,
                "AgX Tone Mapping",
                "Enabled by default. General linear Rec.709 HDR-to-SDR mapping compresses highlights and moves extremely bright colors toward white; it is not tuned for seawater. When disabled, highlights are not compressed and SDR output may clip. Applies immediately without resetting the simulation; save manually.",
                |ui| ui.checkbox(&mut config.tonemapping_enabled, ""),
            );
            parameter(
                ui,
                "Exposure Compensation (stops)",
                "Multiplies linear color by 2 raised to the number of stops before highlight extraction and mapping; each stop doubles brightness. Range -8–8, default 0. Independent of AgX and Bloom; applies immediately without resetting the simulation; save manually.",
                |ui| ui.add(ParameterSlider::new(&mut config.exposure, -8.0..=8.0)),
            );
            parameter(
                ui,
                "Enable Bloom",
                "Enabled by default. Controls highlight spread independently of AgX mapping. When disabled, parameters are retained and the bloom pass is skipped; exposure and mapping can still apply. Applies immediately without resetting the simulation; save manually.",
                |ui| ui.checkbox(&mut config.bloom_enabled, ""),
            );
            if !config.bloom_enabled {
                return;
            }
            for (name, help, value, range) in [
                (
                    "Bloom Strength",
                    "Dimensionless bloom compositing strength, range 0–2, default 0.1. At 0, the bloom pass is skipped; AgX controls whether mapping is applied. Applies immediately without resetting the simulation; save manually.",
                    &mut config.strength,
                    0.0..=2.0,
                ),
                (
                    "Highlight Threshold (linear)",
                    "Extracts highlights by the largest exposed linear RGB component, not sRGB. Range 0–20, default 1; lower values make more areas bloom. Applies immediately.",
                    &mut config.threshold,
                    0.0..=20.0,
                ),
                (
                    "Threshold Softness",
                    "Transition half-width as a fraction of the highlight threshold, dimensionless, range 0–1, default 0.5. Zero gives a hard threshold; nonzero values soften the transition. Applies immediately.",
                    &mut config.soft_knee,
                    0.0..=1.0,
                ),
                (
                    "Bloom Radius",
                    "Multiscale upsampling filter radius in pixels of each texture level, range 0.5–2, default 1. Not world-space meters; varies with screen resolution. Applies immediately.",
                    &mut config.radius,
                    0.5..=2.0,
                ),
            ] {
                parameter(ui, name, help, |ui| {
                    ui.add(ParameterSlider::new(value, range))
                });
            }
        });
    }
}

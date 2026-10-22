//! egui 参数面板与 GPU 合成；不参与流体计算。
use super::{
    color_editor::CoefficientEditor,
    foam::FoamConfig,
    gpu::Gpu,
    optics::{OpticsConfig, VIEW_NAMES},
    spectrum::OceanConfig,
    viewer::Camera,
};
use winit::window::Window;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Panel {
    View,
    Ocean,
    Foam,
    Shore,
    Optics,
}

fn default_dock() -> egui_dock::DockState<Panel> {
    use egui_dock::{DockState, NodeIndex};
    let mut dock = DockState::new(vec![Panel::View]);
    let [center, left] =
        dock.main_surface_mut()
            .split_left(NodeIndex::root(), 0.23, vec![Panel::Ocean]);
    dock.main_surface_mut()
        .split_below(left, 0.72, vec![Panel::Foam]);
    dock.main_surface_mut()
        .split_right(center, 0.68, vec![Panel::Shore, Panel::Optics]);
    dock
}

struct PanelViewer<F>(F);
impl<F: FnMut(Panel, &mut egui::Ui)> egui_dock::TabViewer for PanelViewer<F> {
    type Tab = Panel;
    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        match tab {
            Panel::View => "Ocean view",
            Panel::Ocean => "Ocean controls",
            Panel::Foam => "Jacobian foam",
            Panel::Shore => "Shallow water (SWE)",
            Panel::Optics => "Water optics",
        }
        .into()
    }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        (self.0)(*tab, ui);
    }
    fn is_closeable(&self, _: &Panel) -> bool {
        false
    }
    fn allowed_in_windows(&self, _: &mut Panel) -> bool {
        false
    }
    fn scroll_bars(&self, _: &Panel) -> [bool; 2] {
        [false, false]
    }
}

fn ocean_view(
    ui: &mut egui::Ui,
    texture: egui::TextureId,
    camera: &mut Camera,
    length: f32,
) -> [u32; 2] {
    let size = ui.available_size().max(egui::vec2(1.0, 1.0));
    let requested_size = [
        (size.x * ui.ctx().pixels_per_point()).round().max(1.0) as u32,
        (size.y * ui.ctx().pixels_per_point()).round().max(1.0) as u32,
    ];
    let response = ui.add(egui::Image::new((texture, size)).sense(egui::Sense::drag()));
    if response.dragged_by(egui::PointerButton::Primary) {
        let delta = ui.ctx().input(|input| input.pointer.delta());
        camera.yaw -= delta.x * 0.005;
        camera.pitch = (camera.pitch + delta.y * 0.005).clamp(0.07, 1.35);
    }
    if response.hovered() {
        let amount = ui.ctx().input(|input| input.smooth_scroll_delta.y) / 50.0;
        camera.distance =
            (camera.distance * (-amount * 0.1).exp()).clamp(length * 0.12, length * 0.95);
    }
    requested_size
}

const PARAMETER_NAME_RATIO: f32 = 0.42;
const PARAMETER_COLUMN_GAP: f32 = 8.0;

/// All groups use the same ratio; widget minimum sizes cannot widen a column.
pub(crate) fn parameter_table<R>(
    ui: &mut egui::Ui,
    id: &str,
    rows: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let usable = (ui.available_width() - PARAMETER_COLUMN_GAP).max(0.0);
    let widths = [
        usable * PARAMETER_NAME_RATIO,
        usable * (1.0 - PARAMETER_NAME_RATIO),
    ];
    let key = egui::Id::new("parameter table column widths");
    let previous = ui.data_mut(|data| {
        let old = data.get_temp::<[f32; 2]>(key);
        data.insert_temp(key, widths);
        old
    });
    let result = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = (widths[1] - 55.0).max(0.0);
            egui::Grid::new((id, widths[0].to_bits()))
                .num_columns(2)
                .min_col_width(widths[0])
                .spacing(egui::vec2(PARAMETER_COLUMN_GAP, 6.0))
                .striped(true)
                .show(ui, rows)
                .inner
        })
        .inner;
    ui.data_mut(|data| {
        if let Some(old) = previous {
            data.insert_temp(key, old);
        } else {
            data.remove::<[f32; 2]>(key);
        }
    });
    result
}

/// Reserve exactly the column width even if a control has a larger minimum size.
fn parameter_cell<R>(
    ui: &mut egui::Ui,
    id: (&str, &str),
    width: f32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let origin = ui.available_rect_before_wrap().min;
    let bounds = egui::Rect::from_min_size(origin, egui::vec2(width, 100000.0));
    let mut cell = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(bounds)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    cell.set_clip_rect(ui.clip_rect().intersect(bounds));
    cell.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    let value = contents(&mut cell);
    ui.allocate_space(egui::vec2(width, cell.min_size().y));
    value
}

/// Append a name/control row to the enclosing parameter table.
pub(crate) fn parameter<R>(
    ui: &mut egui::Ui,
    name: &str,
    description: &str,
    control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let widths = ui
        .data(|data| data.get_temp::<[f32; 2]>(egui::Id::new("parameter table column widths")))
        .expect("parameter rows must be inside a shared table");
    parameter_cell(ui, (name, "name"), widths[0], |ui| {
        ui.add(egui::Label::new(name).wrap())
            .on_hover_text(description);
    });
    let value = parameter_cell(ui, (name, "control"), widths[1], control);
    ui.end_row();
    value
}

fn help(name: &str) -> &'static str {
    match name {
        "A (power)" => {
            "Phillips spectrum power A. Wave height scales with sqrt(A). P(k) = A exp(-1/(kL)^2) / k^4 * alignment^2. Apply to rebuild."
        }
        "L (m)" => "Phillips length scale L = wind speed squared / gravity. Apply to rebuild.",
        "Wind angle (deg)" => {
            "Direction of the wind unit vector, not wave speed. Apply to rebuild the spectrum."
        }
        "Patch side (m)" => {
            "FFT tile side length. The rendered ocean spans 3 by 3 tiles. Apply to rebuild."
        }
        "FFT spectrum" => {
            "FFT samples per axis. Grid spacing = patch side / resolution. Apply to rebuild."
        }
        "Mesh cells / axis" => {
            "Coarse meshes may miss short waves. Geometry cells per tile axis; vertices = cells + 1. Mesh spacing = patch side / cells. Apply to rebuild."
        }
        "Paused" => "Freeze simulation, foam and texture flow. Render controls remain editable.",
        "Time speed" => "Simulation time multiplier. Zero freezes evolution.",
        "Height gain" => {
            "Scales FFT displacement and SWE input. Changing it reinitializes SWE history."
        }
        "Horizontal choppiness" | "Chop strength" => {
            "Horizontal FFT displacement sharpens crests and changes the Jacobian used to generate offshore foam."
        }
        "Show foam" => "Toggle foam shading without stopping simulation.",
        "J threshold" => {
            "Offshore foam is generated when the surface Jacobian falls below this threshold. Live; no Apply needed."
        }
        "Generation / s" => {
            "Offshore foam production rate per simulation second. Live; no Apply needed."
        }
        "Decay / s" => {
            "Offshore foam exponential decay rate. Smaller values keep foam longer; zero disables decay."
        }
        "Foam view" => "Show shaded water, the deformation Jacobian, or offshore foam coverage.",
        "Sloped sand" => {
            "Use sloped beach geometry. Land is toward -X; deeper water toward +X. Changing terrain resets SWE."
        }
        "Enable shore waves" => {
            "Enable local shallow-water simulation and foam/ground history. Changes reset SWE."
        }
        "SWE solver" => {
            "First order: Euler. Second order: MUSCL + RK2. Switching resets SWE. Set normal detail to zero to compare geometry."
        }
        "Water foam" => "Linear RGB for water foam, including offshore foam. Live shading only.",
        "Stranded foam (sand)" => {
            "Linear RGB for residue on exposed sand, not the wider water foam band. Live shading only."
        }
        "Normal detail strength" => {
            "FFT short-wave normal detail. 0 = off, 1 = default. Fades in thin water. No simulation reset; enable reflections to see it clearly."
        }
        "FFT to SWE gain" => {
            "Scales input elevation and momentum together: 0 = none, 1 = matched, 3 = triple. Reinitializes the whole SWE patch from current FFT and clears history."
        }
        "Filter short waves" => {
            "Apply the same short-wave filter to FFT elevation and momentum before SWE input. Far-ocean detail is unchanged. Changes reinitialize SWE."
        }
        "Reject wavelength <= (m)" => {
            "Wavelengths at or below this limit are rejected. Full transmission starts at cutoff + transition width."
        }
        "Transition width (m)" => {
            "Smooth transition above the rejection cutoff. Zero gives a hard cutoff. Changes reinitialize SWE."
        }
        "FFT blend depth (m)" => {
            "Blend from SWE to FFT between this depth and 2.5 times this depth. Changing it resets SWE."
        }
        "Foam generation / s" => {
            "Surface foam production rate. Zero lets existing foam transport, concentrate and decay without new production."
        }
        "Shore foam decay / s" => {
            "Water foam exponential decay rate per second. Smaller = longer life; 0 = no natural decay. Does not control sand residue."
        }
        "Foam compression threshold / s" => {
            "Threshold for compression-enhanced generation. Conservative transport still concentrates foam regardless of this setting."
        }
        "Density color reference" => {
            "Density reaching this value is red; dilute foam is green, intermediate yellow. Display only; does not clip simulated amount. Water and sand share the scale."
        }
        "Sand drying time (s)" => {
            "Wet sand drying time constant. Does not control foam lifetime. Retreat leaves foam on exposed sand."
        }
        "Shore view" => {
            "Diagnostic views bypass normal shading. Flow: blue = run-up, orange = backwash. Density: green = dilute, red = reference or higher, with no pore texture."
        }
        "Beach slope (rise/run)" | "Shore X (m)" => {
            "Beach geometry: land toward -X, deeper water toward +X. Changes reinitialize the shallow-water simulation."
        }
        "Seabed depth (m)" => {
            "Flat seabed depth. Disabled for sloped beach, where depth follows geometry."
        }
        "Enable sky scattering" => {
            "Isotropic sky scattering using the mean sea plane and straight light paths. Manual source is analytic; sky source is piecewise analytic."
        }
        "Sky tint (linear RGB)" => {
            "Linear RGB sky illumination; also controls the sky background and reflection."
        }
        "Sky intensity" => "Sky illumination radiance multiplier.",
        "Scattering albedo RGB" => "sigma_s = albedo * sigma_t; sigma_a = (1 - albedo) * sigma_t.",
        "Sky integration steps" => {
            "Number of integration segments for sky scattering. More steps improve accuracy at additional GPU cost."
        }
        "Water view" => {
            "Optical diagnostics bypass reflection and foam. T = exp(-sigma_t * distance). Absorption view displays background * T; loss includes out-scattering."
        }
        "Path view white at (m)" => "Distance mapped to white in the optical path diagnostic.",
        "Sky reflection and highlights" => "Enable surface reflection and sun highlights.",
        "Seabed RGB test bands" | "Checker size (m)" => {
            "Seabed calibration pattern for comparing wavelength-dependent absorption and optical path length."
        }
        _ => "Live parameter; changes take effect immediately.",
    }
}

pub struct Gui {
    pub context: egui::Context,
    pub input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pub draft: OceanConfig,
    pub speed: f32,
    pub swe_timing: String,
    pub error: Option<String>,
    pub foam_settings: FoamConfig,
    pub clear_foam: bool,
    pub optics: OpticsConfig,
    pub save_now: bool,
    pub save_status: String,
    pub save_failed: bool,
    pub settings_path: Option<std::path::PathBuf>,
    dock: egui_dock::DockState<Panel>,
    dock_style: egui_dock::Style,
    pub viewport_size: [u32; 2],
    pub viewport_view: wgpu::TextureView,
    viewport_texture: wgpu::Texture,
    viewport_id: egui::TextureId,
    sigma_editor: CoefficientEditor,
    source_editor: CoefficientEditor,
}

impl Gui {
    pub fn new(
        window: &Window,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        config: OceanConfig,
    ) -> Self {
        let context = egui::Context::default();
        let dock_style = super::theme::install(&context);
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let mut renderer = egui_wgpu::Renderer::new(&gpu.device, format, Default::default());
        let viewport_texture = Self::view_texture(gpu, format, [1, 1]);
        let viewport_view = viewport_texture.create_view(&Default::default());
        // egui samples gamma-encoded bytes, so use a non-sRGB sampling view.
        let sampled_view = viewport_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        let viewport_id =
            renderer.register_native_texture(&gpu.device, &sampled_view, wgpu::FilterMode::Linear);
        Self {
            dock: default_dock(),
            dock_style,
            viewport_size: [1, 1],
            viewport_texture,
            viewport_view,
            viewport_id,
            context,
            input,
            renderer,
            draft: config,
            speed: 1.0,
            swe_timing: "SWE GPU: waiting".into(),
            error: None,
            foam_settings: FoamConfig::default(),
            clear_foam: false,
            optics: OpticsConfig::default(),
            save_now: false,
            save_status: "Local saving disabled".into(),
            save_failed: false,
            settings_path: None,
            sigma_editor: CoefficientEditor::new(OpticsConfig::default().sigma_t, 0.1),
            source_editor: CoefficientEditor::new(OpticsConfig::default().source, 0.01),
        }
    }

    /// 参数先编辑后 Apply，避免拖动分辨率时连续重建 GPU 资源。
    pub fn frame(
        &mut self,
        window: &Window,
        current: OceanConfig,
        camera: &mut Camera,
        paused: &mut bool,
        time: &mut f32,
    ) -> (egui::FullOutput, Option<OceanConfig>) {
        let input = self.input.take_egui_input(window);
        let context = self.context.clone();
        let mut apply = None;
        let mut dock = std::mem::replace(&mut self.dock, egui_dock::DockState::new(Vec::new()));
        let mut requested_size = self.viewport_size;
        let output = context.run(input, |ctx| {
            egui::TopBottomPanel::top("workspace toolbar").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Ocean workspace").on_hover_text("Drag tabs to dock; drag dividers to resize. Drag inside Ocean view to orbit; scroll to zoom.");
                    if ui.button("Reset layout").clicked() { dock = default_dock(); }

                });
            });
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                egui_dock::DockArea::new(&mut dock)
                    .show_close_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .style(self.dock_style.clone())
                    .show_inside(ui, &mut PanelViewer(|tab: Panel, ui: &mut egui::Ui| {
                        let tab_id = &tab;
                        match tab {
                            Panel::View => {
                                requested_size = ocean_view(ui, self.viewport_id, camera, current.length);
                            },
Panel::Ocean => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let color = if self.save_failed {
                                    egui::Color32::LIGHT_RED
                                } else {
                                    ui.visuals().text_color()
                                };
                                ui.colored_label(color, &self.save_status).on_hover_text("Auto-save after 0.5 seconds idle and on exit.");
                                if ui
                                    .add_enabled(
                                        self.settings_path.is_some(),
                                        egui::Button::new("Save now"),
                                    )
                                    .clicked()
                                {
                                    self.save_now = true;
                                }
                            });
                            if let Some(path) = &self.settings_path {

                                ui.collapsing("Local settings file", |ui| {
                                    ui.label(path.display().to_string());
                                });
                            }
                            ui.separator();
                            ui.label(egui::RichText::new("Phillips spectrum").strong());

                            parameter_table(ui, "spectrum table", |ui| {
parameter(ui, "A (power)", help("A (power)"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.amplitude, 0.0..=0.1)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .smallest_positive(0.000001)
                                    ,
                            ));
                            parameter(ui, "L (m)", help("L (m)"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.phillips_length, 0.1..=500.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));
                            let mut angle = self.draft.wind_angle.to_degrees();
                            if parameter(ui, "Wind angle (deg)", help("Wind angle (deg)"), |ui|
                                ui.add(egui::Slider::new(&mut angle, -180.0..=180.0))).changed()
                            {
                                self.draft.wind_angle = angle.to_radians();
                            }




});
                            ui.separator();
                            ui.label(egui::RichText::new("Domain and sampling").strong());
                            parameter_table(ui, "domain table", |ui| {
parameter(ui, "Patch side (m)", help("Patch side (m)"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.length, 8.0..=1024.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));
                            parameter(ui, "FFT spectrum", help("FFT spectrum"), |ui| egui::ComboBox::from_id_salt("FFT spectrum")
                                .width(ui.available_width()).selected_text(format!(
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
                                }));
                            parameter(ui, "Mesh cells / axis", help("Mesh cells / axis"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.mesh_resolution, 8..=512)
                                    ,
                            ));
                            parameter(ui, "Seed", "Random spectrum seed. Apply to rebuild the ocean.", |ui| ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut self.draft.seed));
                                if ui.button("New").clicked() {
                                    self.draft.seed = self.draft.seed.wrapping_add(1);
                                }
                            }));

});
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        self.draft != current,
                                        egui::Button::new("Apply changes"),
                                    )
                                    .clicked()
                                {
                                    match self.draft.validate() {
                                        Ok(()) => {
                                            apply = Some(self.draft);
                                            self.error = None;
                                        }
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                                if ui.button("Defaults").clicked() {
                                    self.draft = OceanConfig::default();
                                }
                            });
                            parameter_table(ui, "status table", |ui| {
parameter(ui, "Changes", "Spectrum edits take effect after Apply.", |ui| ui.label(if self.draft == current {
                                "Parameters applied"
                            } else {
                                "Pending changes — click Apply"
                            }));

});
                            if let Some(error) = &self.error {
                                ui.colored_label(egui::Color32::LIGHT_RED, error);
                            }
                            ui.separator();
                            ui.label(egui::RichText::new("Playback and appearance").strong());
parameter_table(ui, "playback table", |ui| {
                            {
                                parameter(ui, "Paused", help("Paused"), |ui| ui.checkbox(paused, ""));
                                parameter(ui, "Time", "Elapsed simulation time.", |ui| ui.label(format!("{time:.2} s")));
                            }
                            parameter(ui, "Time speed", help("Time speed"), |ui| ui.add(
                                egui::Slider::new(&mut self.speed, 0.0..=3.0),
                            ));
                            parameter(ui, "Height gain", help("Height gain"), |ui| ui.add(
                                egui::Slider::new(&mut camera.gain, 0.0..=3.0),
                            ));
                            parameter(ui, "Horizontal choppiness", help("Horizontal choppiness"), |ui| ui.checkbox(&mut camera.choppy, ""));
                            parameter(ui, "Chop strength", help("Chop strength"), |ui| ui.add(
                                egui::Slider::new(&mut camera.chop_amount, 0.0..=3.0)
                                    ,
                            ));

});
                                if ui.button("Restart time").clicked() {
                                    *time = 0.0;
                                    self.clear_foam = true;
                                }
                            if ui.button("Reset camera").clicked() {
                                *camera = Camera::new(current.length);
                            }


}); },
Panel::Foam => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {

                            parameter_table(ui, "offshore foam table", |ui| {
parameter(ui, "Show foam", help("Show foam"), |ui| ui.checkbox(&mut camera.foam, ""));
                            parameter(ui, "J threshold", help("J threshold"), |ui| ui.add(
                                egui::Slider::new(&mut self.foam_settings.threshold, -0.5..=1.0)
                                    ,
                            ));
                            parameter(ui, "Generation / s", help("Generation / s"), |ui| ui.add(
                                egui::Slider::new(&mut self.foam_settings.generation, 0.0..=20.0)
                                    ,
                            ));
                            parameter(ui, "Decay / s", help("Decay / s"), |ui| ui.add(
                                egui::Slider::new(&mut self.foam_settings.decay, 0.0..=5.0)
                                    ,
                            ));
                            parameter(ui, "Foam view", help("Foam view"), |ui| egui::ComboBox::from_id_salt("Foam view")
                                .width(ui.available_width()).selected_text(match camera.foam_debug {
                                    1 => "Jacobian",
                                    2 => "Coverage",
                                    _ => "Shaded water",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut camera.foam_debug, 0, "Shaded water");
                                    ui.selectable_value(&mut camera.foam_debug, 1, "Jacobian");
                                    ui.selectable_value(&mut camera.foam_debug, 2, "Coverage");
                                }));



});
                            if ui.button("Clear foam history").clicked() {
                                self.clear_foam = true;
                            }
}); },
Panel::Shore => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {

                                if ui.button("Beach scene").clicked() {
                                    self.optics = OpticsConfig::beach_scene();
                                    *camera = Camera::beach(current.length);
                                }
                            let shore = &mut self.optics.shore;
                            parameter_table(ui, "solver table", |ui| {
parameter(ui, "Sloped sand", help("Sloped sand"), |ui| ui.checkbox(&mut self.optics.beach, ""));
                            parameter(ui, "Enable shore waves", help("Enable shore waves"), |ui| ui.checkbox(&mut shore.enabled, ""));
                            parameter(ui, "SWE solver", help("SWE solver"), |ui| egui::ComboBox::from_id_salt("SWE solver")
                                .width(ui.available_width()).selected_text(if shore.order == 2 {"Second order (MUSCL + RK2)"} else {"First order (Euler)"})
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut shore.order, 1, "First order (Euler)");
                                    ui.selectable_value(&mut shore.order, 2, "Second order (MUSCL + RK2)");
                                }));
                            parameter(ui, "GPU timing", "Includes SWE, foam and substep copies; excludes FFT and rendering.", |ui| ui.label(&self.swe_timing));




});
                            ui.separator();
                            ui.label(egui::RichText::new("Foam colors").strong());
                            parameter_table(ui, "foam colors table", |ui| {
parameter(ui, "Water foam", help("Water foam"), |ui| ui.color_edit_button_rgb(&mut shore.water_foam_color));
                            parameter(ui, "Stranded foam (sand)", help("Stranded foam (sand)"), |ui| ui.color_edit_button_rgb(&mut shore.stranded_foam_color));

});
                            ui.horizontal(|ui| {
                                if ui.button("Distinct defaults").clicked() {
                                    shore.water_foam_color=[0.87,0.92,0.90];
                                    shore.stranded_foam_color=[0.36,0.50,0.58];
                                }
                                if ui.button("Match previous colors").clicked() {
                                    shore.water_foam_color=[0.87,0.92,0.90];
                                    shore.stranded_foam_color=[0.82;3];
                                }
                            });
                            if ui.button("Identify foam: water green / sand red").clicked() {
                                shore.water_foam_color=[0.0,1.0,0.0];
                                shore.stranded_foam_color=[1.0,0.0,0.0];
                            }


                            ui.separator();
                            ui.label(egui::RichText::new("Small-wave normals").strong());
                            parameter_table(ui, "normal table", |ui| {
parameter(ui, "Normal detail strength", help("Normal detail strength"), |ui| ui.add(egui::Slider::new(&mut shore.normal_strength, 0.0..=3.0)));



});
                            ui.separator();
                            ui.label(egui::RichText::new("FFT input").strong());
                            parameter_table(ui, "FFT input table", |ui| {
parameter(ui, "FFT to SWE gain", help("FFT to SWE gain"), |ui| ui.add(egui::Slider::new(&mut shore.input_gain, 0.0..=3.0)));



                            parameter(ui, "Filter short waves", help("Filter short waves"), |ui| ui.checkbox(&mut shore.filter_enabled, ""));

                                parameter(ui, "Reject wavelength <= (m)", help("Reject wavelength <= (m)"), |ui| ui.add_enabled(shore.filter_enabled,egui::Slider::new(&mut shore.filter_cutoff, 0.0..=128.0)
                                    .clamping(egui::SliderClamping::Edits)));
                                parameter(ui, "Transition width (m)", help("Transition width (m)"), |ui| ui.add_enabled(shore.filter_enabled,egui::Slider::new(&mut shore.filter_transition, 0.0..=128.0)
                                    .clamping(egui::SliderClamping::Edits)));


});
                            ui.horizontal(|ui| {
                                if ui.button("12–24 m default").clicked() { shore.filter_enabled=true; shore.filter_cutoff=12.0; shore.filter_transition=12.0; }
                                if ui.button("3–6 m").clicked() { shore.filter_enabled=true; shore.filter_cutoff=3.0; shore.filter_transition=3.0; }
                            });


                            ui.separator();

                            parameter_table(ui, "shore foam table", |ui| {
parameter(ui, "FFT blend depth (m)", help("FFT blend depth (m)"), |ui| ui.add(
                                egui::Slider::new(&mut shore.breaking_depth, 0.5..=5.0)
                                    ,
                            ));
                            parameter(ui, "Foam generation / s", help("Foam generation / s"), |ui| ui.add(
                                egui::Slider::new(&mut shore.generation, 0.0..=15.0)
                                    ,
                            ));
                            parameter(ui, "Shore foam decay / s", help("Shore foam decay / s"), |ui| ui.add(
                                egui::Slider::new(&mut shore.decay, 0.0..=4.0)
                                    ,
                            ));
                            parameter(ui, "Foam compression threshold / s", help("Foam compression threshold / s"), |ui| ui.add(
                                egui::Slider::new(&mut shore.foam_compression_threshold, 0.0..=4.0)
                                    ,
                            ));
                            parameter(ui, "Density color reference", help("Density color reference"), |ui| ui.add(
                                egui::Slider::new(&mut shore.foam_density_reference, 0.01..=20.0)
                                    .logarithmic(true),
                            ));



                            parameter(ui, "Sand drying time (s)", help("Sand drying time (s)"), |ui| ui.add(
                                egui::Slider::new(&mut shore.drying_time, 1.0..=120.0)
                                    ,
                            ));
                            parameter(ui, "Shore view", help("Shore view"), |ui| egui::ComboBox::from_id_salt("Shore view")
                                .width(ui.available_width()).selected_text(
                                    super::shore::VIEW_NAMES[shore.view as usize],
                                )
                                .show_ui(ui, |ui| {
                                    for (i, name) in
                                        super::shore::VIEW_NAMES.iter().enumerate()
                                    {
                                        ui.selectable_value(
                                            &mut shore.view,
                                            i as u32,
                                            *name,
                                        );
                                    }
                                }));




});
                            if ui.button("Debug foam density: green / red").clicked() {
                                shore.view = 6;
                            }
                            if ui.button("Reset shallow water, foam and sand").clicked() {
                                self.clear_foam = true;
                            }

                            if self.optics.beach {
                                parameter_table(ui, "beach table", |ui| {
parameter(ui, "Beach slope (rise/run)", help("Beach slope (rise/run)"), |ui| ui.add(
                                    egui::Slider::new(&mut self.optics.beach_slope, 0.01..=0.5)
                                        .clamping(egui::SliderClamping::Edits)
                                        ,
                                ));
                                parameter(ui, "Shore X (m)", help("Shore X (m)"), |ui| ui.add(
                                    egui::Slider::new(
                                        &mut self.optics.shore_x,
                                        -current.length..=current.length,
                                    )
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                                ));


});
                                if ui.button("Beach camera").clicked() {
                                    *camera = Camera::beach(current.length);
                                }
                            }
}); },
Panel::Optics => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {

                            parameter_table(ui, "seabed table", |ui| {
parameter(ui, "Seabed depth (m)", help("Seabed depth (m)"), |ui| ui.add_enabled(
                                !self.optics.beach,
                                egui::Slider::new(&mut self.optics.depth, 0.25..=100.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));

});
                            self.sigma_editor.ui(
                                ui,
                                "Extinction sigma_t (1/m)",
                                &mut self.optics.sigma_t,
                            );
                            self.source_editor.ui(
                                ui,
                                "Manual source S (radiance/m)",
                                &mut self.optics.source,
                            );
                            ui.separator();
                            ui.label(egui::RichText::new("Sky scattering").strong());
                            parameter_table(ui, "sky table", |ui| {
parameter(ui, "Enable sky scattering", help("Enable sky scattering"), |ui| ui.checkbox(&mut self.optics.sky.enabled, ""));
                            parameter(ui, "Sky tint (linear RGB)", help("Sky tint (linear RGB)"), |ui| {
                                let mut color = self.optics.sky.color;
                                if ui.color_edit_button_rgb(&mut color).changed() {
                                    self.optics.sky.color = color;
                                }
                            });
                            parameter(ui, "Sky intensity", help("Sky intensity"), |ui| ui.add(
                                egui::Slider::new(&mut self.optics.sky.intensity, 0.0..=20.0)
                                    .logarithmic(true)
                                    ,
                            ));
                            parameter(ui, "Scattering albedo RGB", help("Scattering albedo RGB"), |ui| {
                                let mut albedo = self.optics.sky.albedo;
                                if ui.color_edit_button_rgb(&mut albedo).changed() {
                                    self.optics.sky.albedo = albedo;
                                }
                            });
                            parameter(ui, "Sky integration steps", help("Sky integration steps"), |ui| egui::ComboBox::from_id_salt("Sky integration steps")
                                .width(ui.available_width()).selected_text(self.optics.sky.steps.to_string())
                                .show_ui(ui, |ui| {
                                    for n in [8, 16, 32] {
                                        ui.selectable_value(
                                            &mut self.optics.sky.steps,
                                            n,
                                            n.to_string(),
                                        );
                                    }
                                }));

});
                            ui.collapsing("Precise sky RGB", |ui| {
parameter_table(ui, "sky channels table", |ui| {
                                for c in 0..3 {
                                    parameter(ui, &format!("Tint {}", ["R", "G", "B"][c]), "Linear RGB sky tint.", |ui|
                                        ui.add(egui::DragValue::new(&mut self.optics.sky.color[c]).range(0.0..=1.0).speed(0.001)));
                                    parameter(ui, &format!("Albedo {}", ["R", "G", "B"][c]), "Scattering fraction: sigma_s = albedo * sigma_t.", |ui|
                                        ui.add(egui::DragValue::new(&mut self.optics.sky.albedo[c]).range(0.0..=1.0).speed(0.001)));
                                }

});
                            });
                            ui.collapsing("Sky source vs depth", |ui| {
                                super::skylight::depth_graph(ui, &self.optics);
                            });


                            ui.separator();
                            parameter_table(ui, "water optics table", |ui| {
parameter(ui, "Water view", help("Water view"), |ui| egui::ComboBox::from_id_salt("Water view")
                                .width(ui.available_width()).selected_text(VIEW_NAMES[self.optics.view as usize])
                                .show_ui(ui, |ui| {
                                    for (i, name) in VIEW_NAMES.iter().enumerate() {
                                        ui.selectable_value(&mut self.optics.view, i as u32, *name);
                                    }
                                }));
                            parameter(ui, "Path view white at (m)", help("Path view white at (m)"), |ui| ui.add(
                                egui::Slider::new(&mut self.optics.path_range, 1.0..=300.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));
                            parameter(ui, "Sky reflection and highlights", help("Sky reflection and highlights"), |ui| ui.checkbox(
                                &mut self.optics.reflections,
                                "",
                            ));
                            parameter(ui, "Seabed RGB test bands", help("Seabed RGB test bands"), |ui| ui.checkbox(&mut self.optics.color_bands, ""));
                            parameter(ui, "Checker size (m)", help("Checker size (m)"), |ui| ui.add(
                                egui::Slider::new(&mut self.optics.checker_size, 0.5..=20.0)
                                    ,
                            ));





});
                            if ui.button("Reset optics").clicked() {
                                self.optics = OpticsConfig::default();
                            }
}); },
                        }
                    }));
            });
        });
        self.dock = dock;
        self.viewport_size = requested_size;
        (output, apply)
    }

    fn view_texture(gpu: &Gpu, format: wgpu::TextureFormat, size: [u32; 2]) -> wgpu::Texture {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("docked ocean view"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[format.remove_srgb_suffix()],
        })
    }

    pub fn resize_view(&mut self, gpu: &Gpu, format: wgpu::TextureFormat) {
        self.viewport_texture = Self::view_texture(gpu, format, self.viewport_size);
        self.viewport_view = self.viewport_texture.create_view(&Default::default());
        let sampled_view = self
            .viewport_texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(format.remove_srgb_suffix()),
                ..Default::default()
            });
        self.renderer.update_egui_texture_from_wgpu_texture(
            &gpu.device,
            &sampled_view,
            wgpu::FilterMode::Linear,
            self.viewport_id,
        );
    }

    pub fn encode(
        &mut self,
        window: &Window,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        output: egui::FullOutput,
    ) -> Vec<wgpu::CommandBuffer> {
        self.input
            .handle_platform_output(window, output.platform_output);
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&gpu.device, &gpu.queue, *id, delta);
        }
        let jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let size = window.inner_size();
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: output.pixels_per_point,
        };
        let commands =
            self.renderer
                .update_buffers(&gpu.device, &gpu.queue, encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.renderer.render(&mut pass, &jobs, &screen);
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        commands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_groups_share_ratio_even_with_oversized_controls() {
        let ctx = egui::Context::default();
        for width in [240.0, 400.0] {
            for _ in 0..2 {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width + 16.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_width(width);
                            let start = ui.cursor().left();
                            let expected = start
                                + (width - PARAMETER_COLUMN_GAP) * PARAMETER_NAME_RATIO
                                + PARAMETER_COLUMN_GAP;
                            let mut positions = Vec::new();
                            for group in ["short group", "long group"] {
                                parameter_table(ui, group, |ui| {
                                    for name in ["A", "A much longer name which wraps"] {
                                        parameter(ui, name, "Tooltip", |ui| {
                                            positions.push(ui.cursor().left());
                                            ui.allocate_space(egui::vec2(500.0, 20.0));
                                        });
                                    }
                                });
                            }
                            for x in positions {
                                assert!((x - expected).abs() < 1.0, "{x} != {expected}");
                            }
                        });
                    },
                );
            }
        }
    }

    #[test]
    fn dock_layout_keeps_view_between_controls_and_resizes() {
        let ctx = egui::Context::default();
        let dock_style = super::super::theme::install(&ctx);
        let mut dock = default_dock();
        let mut previous_width = 0.0;
        for width in [1200.0, 1600.0] {
            let mut rects = std::collections::HashMap::new();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 800.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui_dock::DockArea::new(&mut dock)
                            .style(dock_style.clone())
                            .show_inside(
                                ui,
                                &mut PanelViewer(|tab, ui: &mut egui::Ui| {
                                    rects.insert(tab, ui.available_rect_before_wrap());
                                }),
                            );
                    });
                },
            );
            let view = rects[&Panel::View];
            assert!(rects[&Panel::Ocean].right() <= view.left());
            assert!(view.right() <= rects[&Panel::Shore].left());
            assert!(rects[&Panel::Ocean].bottom() <= rects[&Panel::Foam].top());
            assert!(view.width() > rects[&Panel::Ocean].width());
            assert!(view.width() > rects[&Panel::Shore].width());
            assert!(view.width() > previous_width);
            previous_width = view.width();
        }
    }

    #[test]
    fn orbit_is_owned_by_view_drag_not_sidebar_drag() {
        let ctx = egui::Context::default();
        let mut camera = Camera::new(128.0);
        let initial = camera.yaw;
        let mut frame = |events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::SidePanel::left("controls")
                        .exact_width(200.0)
                        .show(ctx, |ui| {
                            ui.label("Controls");
                        });
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let size = ocean_view(ui, egui::TextureId::User(0), &mut camera, 128.0);
                        assert!(size[0] > 700 && size[0] < 800);
                        assert!(size[1] > 500 && size[1] <= 600);
                    });
                },
            );
            camera.yaw
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![]);
        frame(vec![
            egui::Event::PointerMoved(egui::pos2(50.0, 150.0)),
            button(egui::pos2(50.0, 150.0), true),
        ]);
        assert_eq!(
            frame(vec![egui::Event::PointerMoved(egui::pos2(100.0, 180.0))]),
            initial
        );
        frame(vec![button(egui::pos2(100.0, 180.0), false)]);
        frame(vec![
            egui::Event::PointerMoved(egui::pos2(400.0, 150.0)),
            button(egui::pos2(400.0, 150.0), true),
        ]);
        assert_ne!(
            frame(vec![egui::Event::PointerMoved(egui::pos2(460.0, 180.0))]),
            initial
        );
    }
}

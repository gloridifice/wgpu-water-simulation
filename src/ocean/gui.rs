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

pub struct Gui {
    pub context: egui::Context,
    pub input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pub draft: OceanConfig,
    pub speed: f32,
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
                    ui.label("Ocean workspace");
                    if ui.button("Reset layout").clicked() { dock = default_dock(); }
                    ui.small("Drag tabs to dock; drag dividers to resize. Orbit / zoom inside Ocean view.");
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
                                ui.colored_label(color, &self.save_status);
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
                                ui.small("Auto-save after 0.5 s idle; also saved on exit")
                                    .on_hover_text(path.display().to_string());
                                ui.collapsing("Local settings file", |ui| {
                                    ui.label(path.display().to_string());
                                });
                            }
                            ui.separator();
                            ui.heading("Phillips spectrum");
                            ui.label("P(k) = A exp(-1/(kL)²) / k⁴ · alignment²");
                            ui.add(
                                egui::Slider::new(&mut self.draft.amplitude, 0.0..=0.1)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .smallest_positive(0.000001)
                                    .text("A (power)"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.draft.phillips_length, 0.1..=500.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .text("L (m)"),
                            );
                            let mut angle = self.draft.wind_angle.to_degrees();
                            if ui
                                .add(
                                    egui::Slider::new(&mut angle, -180.0..=180.0)
                                        .text("Wind angle (deg)"),
                                )
                                .changed()
                            {
                                self.draft.wind_angle = angle.to_radians();
                            }
                            ui.small(format!(
                                "Wind unit vector = ({:.2}, {:.2})",
                                self.draft.wind_angle.cos(),
                                self.draft.wind_angle.sin()
                            ));
                            ui.small("omega-hat = wind direction; not wave speed.");
                            ui.small(format!(
                                "L = U²/g corresponds to U = {:.2} m/s",
                                (self.draft.phillips_length * 9.81).sqrt()
                            ));
                            ui.separator();
                            ui.heading("Domain and sampling");
                            ui.add(
                                egui::Slider::new(&mut self.draft.length, 8.0..=1024.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .text("Patch side (m)"),
                            );
                            egui::ComboBox::from_label("FFT spectrum")
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
                            ui.add(
                                egui::Slider::new(&mut self.draft.mesh_resolution, 8..=512)
                                    .text("Mesh cells / axis"),
                            );
                            let m = self.draft.mesh_resolution;
                            ui.small(format!(
                                "{} × {} vertices / tile; 3 × 3 tiles",
                                m + 1,
                                m + 1
                            ));
                            ui.small(format!(
                                "FFT spacing {:.3} m; mesh spacing {:.3} m",
                                self.draft.length / self.draft.resolution as f32,
                                self.draft.length / m as f32
                            ));
                            if m < self.draft.resolution {
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "Coarse mesh may miss short-wave geometry.",
                                );
                            }
                            ui.horizontal(|ui| {
                                ui.label("Seed");
                                ui.add(egui::DragValue::new(&mut self.draft.seed));
                                if ui.button("New").clicked() {
                                    self.draft.seed = self.draft.seed.wrapping_add(1);
                                }
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
                            ui.small(if self.draft == current {
                                "Parameters applied"
                            } else {
                                "Pending changes — click Apply"
                            });
                            if let Some(error) = &self.error {
                                ui.colored_label(egui::Color32::LIGHT_RED, error);
                            }
                            ui.separator();
                            ui.heading("Playback and appearance");
                            ui.horizontal(|ui| {
                                ui.checkbox(paused, "Paused");
                                if ui.button("Restart time").clicked() {
                                    *time = 0.0;
                                    self.clear_foam = true;
                                }
                                ui.label(format!("{time:.2} s"));
                            });
                            ui.add(
                                egui::Slider::new(&mut self.speed, 0.0..=3.0).text("Time speed"),
                            );
                            ui.add(
                                egui::Slider::new(&mut camera.gain, 0.0..=3.0).text("Height gain"),
                            );
                            ui.checkbox(&mut camera.choppy, "Horizontal choppiness");
                            ui.add(
                                egui::Slider::new(&mut camera.chop_amount, 0.0..=3.0)
                                    .text("Chop strength"),
                            );
                            if ui.button("Reset camera").clicked() {
                                *camera = Camera::new(current.length);
                            }
                            ui.small("Ocean view: drag to orbit, wheel to zoom.");
                            ui.small("A scales power. Wave height scales with sqrt(A).");
}); },
Panel::Foam => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {
                            ui.label("Live controls — no Apply needed");
                            ui.checkbox(&mut camera.foam, "Show foam");
                            ui.add(
                                egui::Slider::new(&mut self.foam_settings.threshold, -0.5..=1.0)
                                    .text("J threshold"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.foam_settings.generation, 0.0..=20.0)
                                    .text("Generation / s"),
                            );
                            ui.add(
                                egui::Slider::new(&mut self.foam_settings.decay, 0.0..=5.0)
                                    .text("Decay / s"),
                            );
                            egui::ComboBox::from_label("Foam view")
                                .selected_text(match camera.foam_debug {
                                    1 => "Jacobian",
                                    2 => "Coverage",
                                    _ => "Shaded water",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut camera.foam_debug, 0, "Shaded water");
                                    ui.selectable_value(&mut camera.foam_debug, 1, "Jacobian");
                                    ui.selectable_value(&mut camera.foam_debug, 2, "Coverage");
                                });
                            ui.small("J: red < 0, orange < 1, white = 1, blue > 1");
                            ui.small("Pause freezes foam. Rates use simulation seconds.");
                            if ui.button("Clear foam history").clicked() {
                                self.clear_foam = true;
                            }
}); },
Panel::Shore => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {
                            ui.label("Live controls — no Apply needed");
                            ui.horizontal(|ui| {
                                if ui.button("Beach scene").clicked() {
                                    self.optics = OpticsConfig::beach_scene();
                                    *camera = Camera::beach(current.length);
                                }
                                ui.checkbox(&mut self.optics.beach, "Sloped sand");
                            });
                            let shore = &mut self.optics.shore;
                            ui.checkbox(&mut shore.enabled, "Enable shore waves");
                            ui.small("Incoming waves follow FFT sea state and Height gain.");
                            ui.separator();
                            ui.heading("Small-wave normals");
                            ui.add(egui::Slider::new(&mut shore.normal_strength, 0.0..=3.0).text("Normal detail strength"));
                            ui.small("0 = off, 1 = default. Live shading only; no SWE reset.");
                            ui.small("Fades out in thin water. Enable reflections in Water optics.");
                            ui.separator();
                            ui.heading("FFT input filter");
                            ui.checkbox(&mut shore.filter_enabled, "Filter short waves");
                            ui.add_enabled_ui(shore.filter_enabled, |ui| {
                                ui.add(egui::Slider::new(&mut shore.filter_cutoff, 0.0..=128.0)
                                    .clamping(egui::SliderClamping::Edits).text("Reject wavelength <= (m)"));
                                ui.add(egui::Slider::new(&mut shore.filter_transition, 0.0..=128.0)
                                    .clamping(egui::SliderClamping::Edits).text("Transition width (m)"));
                            });
                            if shore.filter_enabled {
                                ui.small(format!("<= {:.1} m rejected; >= {:.1} m fully passed.", shore.filter_cutoff, shore.filter_cutoff + shore.filter_transition));
                                if shore.filter_transition == 0.0 { ui.small("Zero width: hard cutoff; wavelengths above cutoff pass."); }
                            } else {
                                ui.small("Filter off: all FFT wavelengths enter SWE.");
                            }
                            ui.horizontal(|ui| {
                                if ui.button("12–24 m default").clicked() { shore.filter_enabled=true; shore.filter_cutoff=12.0; shore.filter_transition=12.0; }
                                if ui.button("3–6 m").clicked() { shore.filter_enabled=true; shore.filter_cutoff=3.0; shore.filter_transition=3.0; }
                            });
                            ui.small("Live changes restart SWE history, not FFT time. Wait for waves to reach shore.");
                            ui.small("Height and momentum use the same filter. Far-ocean detail is unchanged.");
                            ui.separator();

                            ui.add(
                                egui::Slider::new(&mut shore.breaking_depth, 0.5..=5.0)
                                    .text("FFT blend depth (m)"),
                            );
                            ui.add(
                                egui::Slider::new(&mut shore.generation, 0.0..=15.0)
                                    .text("Shore foam / s"),
                            );
                            ui.add(
                                egui::Slider::new(&mut shore.decay, 0.0..=4.0)
                                    .text("Shore foam decay / s"),
                            );
                            ui.add(
                                egui::Slider::new(&mut shore.drying_time, 1.0..=120.0)
                                    .text("Sand drying time (s)"),
                            );
                            egui::ComboBox::from_label("Shore view")
                                .selected_text(
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
                                });
                            ui.small("Flow: blue = run-up; orange = backwash.");
                            ui.small("Retreat leaves foam on wet sand. Waves need time to reach shore.");
                            ui.small(
                                "FFT or terrain changes restart shore waves. Pause freezes them.",
                            );
                            if ui.button("Reset shallow water, foam and sand").clicked() {
                                self.clear_foam = true;
                            }

                            if self.optics.beach {
                                ui.add(
                                    egui::Slider::new(&mut self.optics.beach_slope, 0.01..=0.5)
                                        .clamping(egui::SliderClamping::Edits)
                                        .text("Beach slope (rise/run)"),
                                );
                                ui.add(
                                    egui::Slider::new(
                                        &mut self.optics.shore_x,
                                        -current.length..=current.length,
                                    )
                                    .clamping(egui::SliderClamping::Edits)
                                    .text("Shore X (m)"),
                                );
                                ui.small(
                                    "Land to -X; deeper water to +X. Depth comes from geometry.",
                                );
                                if ui.button("Beach camera").clicked() {
                                    *camera = Camera::beach(current.length);
                                }
                            }
}); },
Panel::Optics => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {
                            ui.label("Live controls / linear RGB");
                            ui.add_enabled(
                                !self.optics.beach,
                                egui::Slider::new(&mut self.optics.depth, 0.25..=100.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .text("Seabed depth (m)"),
                            );
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
                            ui.heading("Sky scattering");
                            ui.checkbox(&mut self.optics.sky.enabled, "Enable sky scattering");
                            ui.horizontal(|ui| {
                                ui.label("Sky tint (linear RGB)");
                                let mut color = self.optics.sky.color;
                                if ui.color_edit_button_rgb(&mut color).changed() {
                                    self.optics.sky.color = color;
                                }
                            });
                            ui.add(
                                egui::Slider::new(&mut self.optics.sky.intensity, 0.0..=20.0)
                                    .logarithmic(true)
                                    .text("Sky intensity"),
                            );
                            ui.horizontal(|ui| {
                                ui.label("Scattering albedo RGB");
                                let mut albedo = self.optics.sky.albedo;
                                if ui.color_edit_button_rgb(&mut albedo).changed() {
                                    self.optics.sky.albedo = albedo;
                                }
                            });
                            ui.collapsing("Precise sky RGB", |ui| {
                                for c in 0..3 {
                                    ui.horizontal(|ui| {
                                        ui.label(["R", "G", "B"][c]);
                                        ui.label("tint");
                                        ui.add(
                                            egui::DragValue::new(&mut self.optics.sky.color[c])
                                                .range(0.0..=1.0)
                                                .speed(0.001),
                                        );
                                        ui.label("albedo");
                                        ui.add(
                                            egui::DragValue::new(&mut self.optics.sky.albedo[c])
                                                .range(0.0..=1.0)
                                                .speed(0.001),
                                        );
                                    });
                                }
                            });
                            ui.small("sigma_s = albedo * sigma_t; sigma_a = (1-albedo) * sigma_t");
                            egui::ComboBox::from_label("Sky integration steps")
                                .selected_text(self.optics.sky.steps.to_string())
                                .show_ui(ui, |ui| {
                                    for n in [8, 16, 32] {
                                        ui.selectable_value(
                                            &mut self.optics.sky.steps,
                                            n,
                                            n.to_string(),
                                        );
                                    }
                                });
                            ui.collapsing("Sky source vs depth", |ui| {
                                super::skylight::depth_graph(ui, &self.optics);
                            });
                            ui.small("Isotropic / mean sea plane / straight light paths.");
                            ui.small("Sky tint also controls the sky background and reflection.");
                            ui.separator();
                            egui::ComboBox::from_label("Water view")
                                .selected_text(VIEW_NAMES[self.optics.view as usize])
                                .show_ui(ui, |ui| {
                                    for (i, name) in VIEW_NAMES.iter().enumerate() {
                                        ui.selectable_value(&mut self.optics.view, i as u32, *name);
                                    }
                                });
                            ui.add(
                                egui::Slider::new(&mut self.optics.path_range, 1.0..=300.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .text("Path view white at (m)"),
                            );
                            ui.checkbox(
                                &mut self.optics.reflections,
                                "Sky reflection and highlights",
                            );
                            ui.checkbox(&mut self.optics.color_bands, "Seabed RGB test bands");
                            ui.add(
                                egui::Slider::new(&mut self.optics.checker_size, 0.5..=20.0)
                                    .text("Checker size (m)"),
                            );
                            ui.small("Manual S: analytic; sky source: piecewise analytic.");
                            ui.small("T = exp(-sigma_t*d). Zero sigma uses limit S*d.");
                            ui.small("Water debug views bypass reflection and foam.");
                            ui.small("Absorption view shows Lbg*T; loss includes out-scattering.");
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

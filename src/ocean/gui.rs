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
        context.set_visuals(egui::Visuals::dark());
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        Self {
            context,
            input,
            renderer: egui_wgpu::Renderer::new(&gpu.device, format, Default::default()),
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
        let output = context.run(input, |ctx| {
            egui::Window::new("Ocean controls")
                .default_pos([16.0, 16.0])
                .default_width(330.0)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 100.0).max(120.0))
                        .show(ui, |ui| {
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
                            ui.small("Outside panel: drag to orbit, wheel to zoom.");
                            ui.small("A scales power. Wave height scales with sqrt(A).");
                        });
                });
            egui::Window::new("Jacobian foam")
                .default_pos([380.0, 16.0])
                .default_width(310.0)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 100.0).max(120.0))
                        .show(ui, |ui| {
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
                        });
                });
            egui::Window::new("Water optics")
                .default_pos([730.0, 16.0])
                .default_width(320.0)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 100.0).max(120.0))
                        .show(ui, |ui| {
                            ui.label("Live controls / linear RGB");
                            ui.horizontal(|ui| {
                                if ui.button("Beach scene").clicked() {
                                    self.optics = OpticsConfig::beach_scene();
                                    *camera = Camera::beach(current.length);
                                }
                                ui.checkbox(&mut self.optics.beach, "Sloped sand");
                            });
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
                                egui::CollapsingHeader::new("Shore waves and wet sand").default_open(true).show(ui, |ui| {
                                    ui.label("Local 2D shallow water (SWE)");
                                    let shore = &mut self.optics.shore;
                                    ui.checkbox(&mut shore.enabled, "Enable shore waves");
                                    ui.small("Incoming waves follow FFT sea state and Height gain.");
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
                                });
                            }
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
                        });
                });
        });
        (output, apply)
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
                            load: wgpu::LoadOp::Load,
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

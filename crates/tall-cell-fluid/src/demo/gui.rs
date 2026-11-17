//! Dock panels for the standalone demo; performance snapshots are read-only and actions return to the window thread.

use super::render::ViewMode;
use super::worker::{Frame, Scene};
use winit::{event::WindowEvent, window::Window};

pub struct PanelData<'a> {
    pub frame: Option<&'a Frame>,
    pub paused: bool,
    pub scene: Scene,
    pub view: ViewMode,
    pub resetting: bool,
    pub error: Option<&'a str>,
    pub finite_steps: bool,
    pub resolution: usize,
}

#[derive(Clone, Copy, Default)]
pub struct RenderTiming {
    pub supported: bool,
    pub failed: bool,
    pub latest_ms: Option<f64>,
}

pub enum GuiAction {
    SetPaused(bool),
    Step,
    Reset(Scene),
    View(ViewMode),
    ResetCamera,
    Orbit(egui::Vec2),
    Zoom(f32),
}

pub struct GuiFrame {
    pub output: egui::FullOutput,
    pub actions: Vec<GuiAction>,
}

#[derive(Clone, Copy)]
enum Panel {
    View,
    Performance,
}

fn default_dock() -> egui_dock::DockState<Panel> {
    let mut dock = egui_dock::DockState::new(vec![Panel::View]);
    dock.main_surface_mut().split_right(
        egui_dock::NodeIndex::root(),
        0.71,
        vec![Panel::Performance],
    );
    dock
}

pub struct Gui {
    context: egui::Context,
    input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    dock: egui_dock::DockState<Panel>,
    dock_style: egui_dock::Style,
    format: wgpu::TextureFormat,
    viewport_texture: wgpu::Texture,
    viewport_id: egui::TextureId,
    pub viewport_view: wgpu::TextureView,
    pub viewport_size: [u32; 2],
}

impl Gui {
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let context = egui::Context::default();
        let theme = yummy_egui::Theme::default();
        theme.apply(&context);
        let mut fonts = egui::FontDefinitions::default();
        // Reuse the repository font without depending on the working directory or Ocean module.
        fonts.font_data.insert(
            "MiSans".into(),
            egui::FontData::from_static(include_bytes!(
                "../../../../assets/fonts/MiSans-Normal.ttf"
            ))
            .into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "MiSans".into());
        context.set_fonts(fonts);
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let mut renderer = egui_wgpu::Renderer::new(device, format, Default::default());
        let viewport_texture = view_texture(device, format, [1, 1]);
        let viewport_view = viewport_texture.create_view(&Default::default());
        // egui samples gamma-encoded colors; use a non-sRGB sampling view for sRGB targets.
        let sampled_view = viewport_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        let viewport_id =
            renderer.register_native_texture(device, &sampled_view, wgpu::FilterMode::Linear);
        Self {
            context,
            input,
            renderer,
            dock: default_dock(),
            dock_style: theme.dock_style().clone(),
            format,
            viewport_texture,
            viewport_id,
            viewport_view,
            viewport_size: [1, 1],
        }
    }

    pub fn handle_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.input.on_window_event(window, event).consumed
    }

    pub fn prepare(
        &mut self,
        window: &Window,
        device: &wgpu::Device,
        data: &PanelData<'_>,
        timing: RenderTiming,
    ) -> GuiFrame {
        let input = self.input.take_egui_input(window);
        let mut actions = Vec::new();
        let mut requested_size = self.viewport_size;
        let output = self.context.run_ui(input, |root_ui| {
            egui::Panel::top("demo toolbar").show_inside(root_ui, |ui| {
                ui.horizontal(|ui| {
                    yummy_egui::h3(ui, "Tall Cell Fluid");
                    ui.label("Standalone Fluid Lab");
                    if ui.button("Reset Layout").clicked() {
                        self.dock = default_dock();
                    }
                });
            });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(root_ui, |ui| {
                    let mut viewer = Viewer {
                        data,
                        timing,
                        texture: self.viewport_id,
                        requested_size: &mut requested_size,
                        actions: &mut actions,
                    };
                    egui_dock::DockArea::new(&mut self.dock)
                        .style(self.dock_style.clone())
                        .show_close_buttons(false)
                        .show_leaf_close_all_buttons(false)
                        .show_inside(ui, &mut viewer);
                });
        });
        let max_size = device.limits().max_texture_dimension_2d;
        requested_size = requested_size.map(|n| n.clamp(1, max_size));
        if requested_size != self.viewport_size {
            self.viewport_size = requested_size;
            self.viewport_texture = view_texture(device, self.format, requested_size);
            self.viewport_view = self.viewport_texture.create_view(&Default::default());
            let sampled_view = self
                .viewport_texture
                .create_view(&wgpu::TextureViewDescriptor {
                    format: Some(self.format.remove_srgb_suffix()),
                    ..Default::default()
                });
            self.renderer.update_egui_texture_from_wgpu_texture(
                device,
                &sampled_view,
                wgpu::FilterMode::Linear,
                self.viewport_id,
            );
        }
        GuiFrame { output, actions }
    }

    pub fn encode(
        &mut self,
        window: &Window,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        output: egui::FullOutput,
    ) -> Vec<wgpu::CommandBuffer> {
        self.input
            .handle_platform_output(window, output.platform_output);
        for (id, delta) in &output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        let jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let size = window.inner_size();
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: output.pixels_per_point,
        };
        let commands = self
            .renderer
            .update_buffers(device, queue, encoder, &jobs, &screen);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("demo egui (excluded from scene timer)"),
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

fn view_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: [u32; 2],
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("demo dock viewport"),
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

struct Viewer<'a, 'b> {
    data: &'a PanelData<'b>,
    timing: RenderTiming,
    texture: egui::TextureId,
    requested_size: &'a mut [u32; 2],
    actions: &'a mut Vec<GuiAction>,
}

impl egui_dock::TabViewer for Viewer<'_, '_> {
    type Tab = Panel;

    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        match tab {
            Panel::View => "Fluid View",
            Panel::Performance => "Timing and Controls",
        }
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        match tab {
            Panel::View => {
                let size = ui.available_size().max(egui::vec2(1.0, 1.0));
                *self.requested_size = [
                    (size.x * ui.ctx().pixels_per_point()).round().max(1.0) as u32,
                    (size.y * ui.ctx().pixels_per_point()).round().max(1.0) as u32,
                ];
                let response =
                    ui.add(egui::Image::new((self.texture, size)).sense(egui::Sense::drag()));
                if response.dragged_by(egui::PointerButton::Primary) {
                    self.actions
                        .push(GuiAction::Orbit(ui.input(|i| i.pointer.delta())));
                }
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y) / 50.0;
                    if scroll != 0.0 {
                        self.actions.push(GuiAction::Zoom(scroll));
                    }
                }
            }
            Panel::Performance => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        performance_panel(ui, self.data, self.timing, self.actions);
                    });
            }
        }
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

/// Each group shares a two-column table; clip controls to preserve the 42% / 58% widths.
fn table(ui: &mut egui::Ui, id: &str, rows: impl FnOnce(&mut egui::Ui, [f32; 2])) {
    let available = (ui.available_width() - 8.0).max(0.0);
    let widths = [available * 0.42, available * 0.58];
    egui::Grid::new((id, widths[0].to_bits()))
        .num_columns(2)
        .spacing(egui::vec2(8.0, 7.0))
        .min_col_width(0.0)
        .show(ui, |ui| rows(ui, widths));
}

fn cell(ui: &mut egui::Ui, width: f32, f: impl FnOnce(&mut egui::Ui)) {
    let bounds = egui::Rect::from_min_size(
        ui.available_rect_before_wrap().min,
        egui::vec2(width, 100_000.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bounds)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(bounds));
    child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    f(&mut child);
    ui.allocate_space(egui::vec2(width, child.min_size().y));
}

fn row(ui: &mut egui::Ui, widths: [f32; 2], name: &str, tip: &str, f: impl FnOnce(&mut egui::Ui)) {
    cell(ui, widths[0], |ui| {
        ui.label(name).on_hover_text(tip);
    });
    cell(ui, widths[1], f);
    ui.end_row();
}

fn value_row(ui: &mut egui::Ui, widths: [f32; 2], name: &str, tip: &str, value: impl Into<String>) {
    row(ui, widths, name, tip, |ui| {
        ui.label(value.into());
    });
}

fn performance_panel(
    ui: &mut egui::Ui,
    data: &PanelData<'_>,
    timing: RenderTiming,
    actions: &mut Vec<GuiAction>,
) {
    let frame = data.frame.filter(|_| !data.resetting);
    let metric = |value: Option<f64>| {
        value.map_or_else(|| "Waiting for sample".into(), |v| format!("{v:.2}"))
    };
    yummy_egui::h3(ui, "Simulation Timing");
    table(ui, "simulation timing", |ui, widths| {
        value_row(
            ui,
            widths,
            "Wall clock (ms/step)",
            "Latest successful substep: CFL step selection, CPU topology, GPU solving and synchronous readback wait. Not a GPU timestamp; excludes initialization and retains the last sample while paused.",
            metric(frame.and_then(|f| f.simulation_ms)),
        );
    });
    ui.separator();
    yummy_egui::h3(ui, "Mesh Timing");
    table(ui, "mesh timing", |ui, widths| {
        value_row(
            ui,
            widths,
            "Wall clock (ms/update)",
            "Latest surface update: boundary halo, GPU triangle/normal extraction, readback and CPU processing. Excludes mesh upload to the window rendering device; initialization also generates a mesh.",
            metric(frame.map(|f| f.mesh_ms)),
        );
    });
    ui.separator();
    yummy_egui::h3(ui, "Rendering Timing");
    table(ui, "shading timing", |ui, widths| {
        let text = if !timing.supported {
            "Device unsupported".into()
        } else if timing.failed {
            "Timing readback failed".into()
        } else {
            metric(timing.latest_ms)
        };
        value_row(
            ui,
            widths,
            "GPU (ms/frame)",
            "Actual GPU timestamp for the complete scene render pass (water, ground and tank frame), including vertex processing, rasterization and fragment shading, not fragment shading alone. Excludes mesh upload, egui and presentation wait. The latest available sample is read asynchronously without blocking the window. The three timings have different sampling rates and cannot be added to obtain frame time.",
            text,
        );
    });
    ui.separator();
    yummy_egui::h3(ui, "Status");
    table(ui, "demo status", |ui, widths| {
        let state = if data.error.is_some() {
            "Computation failed"
        } else if data.resetting {
            "Resetting"
        } else if frame.is_none() {
            "Initializing"
        } else if data.paused && frame.is_some_and(|f| !f.paused) {
            "Waiting to pause"
        } else if data.paused {
            "Paused"
        } else {
            "Running"
        };
        value_row(
            ui,
            widths,
            "Status",
            "Simulation runs in the background; pause and reset wait for the current substep to finish.",
            state,
        );
        value_row(
            ui,
            widths,
            "Simulation time (s)",
            "Completed physical simulation time, not wall-clock time.",
            frame.map_or_else(|| "—".into(), |f| format!("{:.3}", f.time)),
        );
        value_row(
            ui,
            widths,
            "Completed substeps",
            "Latest displayed successful substep number.",
            frame.map_or_else(|| "—".into(), |f| f.steps.to_string()),
        );
        value_row(
            ui,
            widths,
            "Triangles",
            "Total surface triangles currently extracted by the GPU.",
            frame.map_or_else(|| "—".into(), |f| (f.mesh.indices.len() / 3).to_string()),
        );
        value_row(
            ui,
            widths,
            "Background grid",
            "Cells per axis, specified by the --resolution startup option.",
            format!("{}³", data.resolution),
        );
    });
    if let Some(error) = data.error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
    }
    ui.separator();
    yummy_egui::h3(ui, "Demo Controls");
    table(ui, "demo controls", |ui, widths| {
        row(
            ui,
            widths,
            "Scene",
            "Selecting a scene resets the fluid, time and timing samples.",
            |ui| {
                let mut scene = data.scene;
                egui::ComboBox::from_id_salt("scene")
                    .width(ui.available_width())
                    .selected_text(scene_name(scene))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut scene, Scene::Drop, "Droplet");
                        ui.selectable_value(&mut scene, Scene::Dam, "Dam Break");
                    });
                if scene != data.scene {
                    actions.push(GuiAction::Reset(scene));
                }
            },
        );
        row(
            ui,
            widths,
            "View mode",
            "Changes only the preview material, immediately and without resetting the simulation.",
            |ui| {
                let mut view = data.view;
                egui::ComboBox::from_id_salt("view")
                    .width(ui.available_width())
                    .selected_text(view_name(view))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut view, ViewMode::Water, "Water");
                        ui.selectable_value(&mut view, ViewMode::Mesh, "Mesh");
                        ui.selectable_value(&mut view, ViewMode::Normals, "Normals");
                    });
                if view != data.view {
                    actions.push(GuiAction::View(view));
                }
            },
        );
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                !data.finite_steps,
                egui::Button::new(if data.paused { "Play" } else { "Pause" }),
            )
            .on_hover_text("Space; playback toggling is disabled for finite-step runs.")
            .clicked()
        {
            actions.push(GuiAction::SetPaused(!data.paused));
        }
        if ui
            .add_enabled(
                data.paused && !data.resetting && data.error.is_none() && frame.is_some(),
                egui::Button::new("Step"),
            )
            .on_hover_text("N; advance one CFL-compliant substep.")
            .clicked()
        {
            actions.push(GuiAction::Step);
        }
        if ui
            .button("Reset")
            .on_hover_text("R; reinitialize the current scene.")
            .clicked()
        {
            actions.push(GuiAction::Reset(data.scene));
        }
        if ui
            .button("Reset Camera")
            .on_hover_text(
                "Home; drag with the left mouse button in the view to rotate, scroll to zoom.",
            )
            .clicked()
        {
            actions.push(GuiAction::ResetCamera);
        }
    });
}

fn scene_name(scene: Scene) -> &'static str {
    match scene {
        Scene::Drop => "Droplet",
        Scene::Dam => "Dam Break",
    }
}

fn view_name(view: ViewMode) -> &'static str {
    match view {
        ViewMode::Water => "Water",
        ViewMode::Mesh => "Mesh",
        ViewMode::Normals => "Normals",
    }
}

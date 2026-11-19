//! Fixed workspace navigation, docked editors and one shared GPU viewport.
use super::{
    color_editor::CoefficientEditor,
    panels::configuration::ConfigurationPanel,
    workspace::{Panel, WorkspaceId, Workspaces, store::WorkspaceStore},
};
use crate::ocean::{
    camera::Camera,
    gpu::context::Gpu,
    parameters::{foam::FoamConfig, ocean::OceanConfig, optics::OpticsConfig},
    settings::{ConfigurationLibrary, SavedSettings},
};
use winit::window::Window;
use yummy_egui::{IconSegmentedControl, ParameterSlider, parameter, parameter_table};

struct PanelViewer<F>(F);
impl<F: FnMut(Panel, &mut egui::Ui)> egui_dock::TabViewer for PanelViewer<F> {
    type Tab = Panel;
    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        tab.name().into()
    }
    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Panel) {
        (self.0)(*tab, ui);
    }
    fn is_closeable(&self, tab: &Panel) -> bool {
        *tab != Panel::View
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
    } else if response.dragged_by(egui::PointerButton::Middle) {
        let delta = if response.drag_started_by(egui::PointerButton::Middle) {
            response.total_drag_delta().unwrap_or_default()
        } else {
            response.drag_delta()
        };
        camera.pan_drag(glam::Vec2::new(delta.x, delta.y), size.y);
    }
    if response.hovered() {
        let amount = ui.ctx().input(|input| input.smooth_scroll_delta.y) / 50.0;
        camera.distance =
            (camera.distance * (-amount * 0.1).exp()).clamp(length * 0.12, length * 0.95);
        if !ui.ctx().egui_wants_keyboard_input() {
            ui.input(|input| {
                if input.focused && !input.modifiers.ctrl && !input.modifiers.command {
                    let right = i32::from(input.key_down(egui::Key::D))
                        - i32::from(input.key_down(egui::Key::A));
                    let forward = i32::from(input.key_down(egui::Key::W))
                        - i32::from(input.key_down(egui::Key::S));
                    camera.pan(
                        glam::Vec2::new(right as f32, forward as f32),
                        input.stable_dt.min(0.1),
                    );
                }
            });
        }
    }
    requested_size
}

pub(crate) enum ScreenshotStatus {
    Idle,
    Saving,
    Saved(std::path::PathBuf),
    Failed(String),
}

pub struct Gui {
    pub backend: common::ui::Egui,
    pub draft: OceanConfig,
    pub speed: f32,
    pub performance: crate::ocean::ui::performance::Performance,
    pub swe_domain: [f32; 4],
    pub model_panel: crate::ocean::ui::panels::model::ModelPanel,
    pub lake_panel: crate::ocean::ui::panels::lake::LakePanel,
    pub error: Option<String>,
    pub spray_error: Option<String>,
    pub foam_settings: FoamConfig,
    pub clear_foam: bool,
    pub optics: OpticsConfig,
    pub save_now: bool,
    pub save_status: String,
    pub save_failed: bool,
    pub(crate) screenshot_requested: bool,
    pub(crate) screenshot_status: ScreenshotStatus,
    pub settings_path: Option<std::path::PathBuf>,
    pub configuration_panel: ConfigurationPanel,
    pub load_configuration: Option<SavedSettings>,
    pub workspaces: Workspaces,
    pub workspace_store: Option<WorkspaceStore>,
    icons: [egui::Image<'static>; WorkspaceId::ALL.len()],
    dock_style: egui_dock::Style,
    show_dock_padding: bool,
    pub viewport_size: [u32; 2],
    pub viewport: common::ui::Viewport,
    pub(super) sigma_editor: CoefficientEditor,
    pub(super) source_editor: CoefficientEditor,
}

impl std::ops::Deref for Gui {
    type Target = common::ui::Egui;
    fn deref(&self) -> &Self::Target {
        &self.backend
    }
}
impl std::ops::DerefMut for Gui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.backend
    }
}
impl Gui {
    pub fn new(
        window: &Window,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        config: OceanConfig,
    ) -> Self {
        let mut backend = common::ui::Egui::new(window, gpu, format);
        let context = backend.context.clone();
        let theme = yummy_egui::Theme::default();
        super::fonts::install(&context);
        let dock_style = theme.dock_style().clone();
        egui_extras::install_image_loaders(&context);
        let sources = [
            include_str!("../../../assets/ui/workspaces/scene.svg"),
            include_str!("../../../assets/ui/workspaces/shallow.svg"),
            include_str!("../../../assets/ui/workspaces/offshore.svg"),
            include_str!("../../../assets/ui/workspaces/animated-sdf.svg"),
            include_str!("../../../assets/ui/workspaces/shading.svg"),
            include_str!("../../../assets/ui/workspaces/configurations.svg"),
        ];
        // Decode white once per requested raster size; tint follows the theme.
        let icons = std::array::from_fn(|i| {
            egui::Image::from_bytes(
                format!("bytes://workspace-{}.svg", WorkspaceId::ALL[i].key()),
                sources[i].replace("currentColor", "#ffffff").into_bytes(),
            )
        });
        let viewport = common::ui::Viewport::new(&mut backend, gpu, format);
        Self {
            backend,
            draft: config,
            speed: 1.0,
            performance: crate::ocean::ui::performance::Performance::default(),
            swe_domain: [-32.0, -192.0, 64.0, 384.0],
            model_panel: crate::ocean::ui::panels::model::ModelPanel::default(),
            lake_panel: Default::default(),
            error: None,
            spray_error: None,
            foam_settings: FoamConfig::default(),
            clear_foam: false,
            optics: OpticsConfig::default(),
            save_now: false,
            save_status: "Local saving disabled".into(),
            save_failed: false,
            screenshot_requested: false,
            screenshot_status: ScreenshotStatus::Idle,
            settings_path: None,
            configuration_panel: ConfigurationPanel::new(ConfigurationLibrary::new(
                SavedSettings::default(),
            )),
            load_configuration: None,
            workspaces: Workspaces::default(),
            workspace_store: None,
            icons,
            dock_style,
            show_dock_padding: false,
            viewport_size: [1, 1],
            viewport,
            sigma_editor: CoefficientEditor::new(OpticsConfig::default().sigma_t, 0.1),
            source_editor: CoefficientEditor::new(OpticsConfig::default().source, 0.01),
        }
    }

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
        // Move just the UI layouts out during callbacks, not shared editor state.
        let mut workspaces = std::mem::take(&mut self.workspaces);
        let output = context.run_ui(input, |root_ui| {
            let save_shortcut = root_ui.input_mut(|input| {
                input.consume_shortcut(&egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND,
                    egui::Key::S,
                ))
            });
            self.save_now |= save_shortcut && self.settings_path.is_some();
            egui::Panel::top("workspace header")
                .frame(egui::Frame::NONE)
                .show_inside(root_ui, |ui| {
                    egui::Frame::new().inner_margin(6.0).show(ui, |ui| {
                        let toolbar_bounds = ui.available_rect_before_wrap();
                        let controls = ui
                            .horizontal_wrapped(|ui| {
                                yummy_egui::h3(ui, format!("{} Workspace", workspaces.active.name()));
                                ui.separator();
                                if ui.button(if *paused { "Resume" } else { "Pause" }).clicked() {
                                    *paused = !*paused;
                                }
                                ui.label("Speed");
                                ui.add_sized(
                                    egui::vec2(140.0, ui.spacing().interact_size.y),
                                    ParameterSlider::new(&mut self.speed, 0.0..=3.0)
                                        .clamping(egui::SliderClamping::Edits)
                                        .max_decimals(2),
                                )
                                .on_hover_text("Simulation time scale, 0–3; applies immediately. 0 stops evolution.");
                                ui.menu_button("Time / Camera", |ui| {
                                    ui.label(format!("Simulation time {time:.2} s"));
                                    if ui.button("Reset Time").clicked() {
                                        *time = 0.0;
                                        self.clear_foam = true;
                                        ui.close();
                                    }
                                    if ui.button("Reset Camera").clicked() {
                                        *camera = if self.optics.lake.enabled {
                                            Camera::lake(self.optics.lake.terrain.radius)
                                        } else {
                                            Camera::new(current.length)
                                        };
                                        ui.close();
                                    }
                                    if self.optics.beach && ui.button("Beach Camera").clicked() {
                                        *camera = Camera::beach(current.length);
                                        ui.close();
                                    }
                                    if self.optics.model.enabled && ui.button("Model Camera").clicked()
                                    {
                                        *camera = Camera::model(current.length);
                                        ui.close();
                                    }
                                });
                                if ui
                                    .add_enabled(
                                        self.settings_path.is_some(),
                                        egui::Button::new("Save"),
                                    )
                                    .on_hover_text(
                                        "Save the working state and configuration library (Ctrl+S), without overwriting named snapshots.",
                                    )
                                    .on_disabled_hover_text(
                                        "Local saving disabled (read-only configuration or --no-settings)",
                                    )
                                    .clicked()
                                {
                                    self.save_now = true;
                                }
                                let status =
                                    self.save_status.split('：').next().unwrap_or_default();
                                ui.colored_label(
                                    if self.save_failed {
                                        egui::Color32::LIGHT_RED
                                    } else {
                                        ui.visuals().text_color()
                                    },
                                    status,
                                )
                                .on_hover_ui(|ui| {
                                    ui.label(&self.save_status);
                                    if let Some(path) = &self.settings_path {
                                        ui.label(path.display().to_string());
                                    }
                                });
                                ui.menu_button("Panels", |ui| {
                                    for panel in Panel::ALL
                                        .into_iter()
                                        .filter(|p| *p != Panel::View && p.available(&self.optics))
                                    {
                                        let present = workspaces.current().contains(panel);
                                        if ui
                                            .selectable_label(present, panel.name())
                                            .on_hover_text(if present {
                                                "Locate added panel"
                                            } else {
                                                "Add to current workspace"
                                            })
                                            .clicked()
                                        {
                                            workspaces.current().add(panel);
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("Dock Padding Debug").clicked() {
                                        self.show_dock_padding = true;
                                        ui.close();
                                    }
                                    if ui.button("Reset Current Layout").clicked() {
                                        workspaces.reset_current();
                                        if let Some(store) = &mut self.workspace_store {
                                            store.allow_repair();
                                        }
                                        ui.close();
                                    }
                                    if let Some(store) = &mut self.workspace_store
                                        && store.blocked
                                        && ui.button("Confirm Saving Repaired Layout").clicked()
                                    {
                                        store.allow_repair();
                                        ui.close();
                                    }
                                });
                                let saving_screenshot = self.screenshot_requested
                                    || matches!(self.screenshot_status, ScreenshotStatus::Saving);
                                if ui
                                    .add_enabled(!saving_screenshot, egui::Button::new("Screenshot"))
                                    .on_hover_text("Save the current Render View as a PNG in the screenshots folder, without UI panels.")
                                    .on_disabled_hover_text("Saving a screenshot…")
                                    .clicked()
                                {
                                    self.screenshot_requested = true;
                                }
                                match &self.screenshot_status {
                                    ScreenshotStatus::Saving => {
                                        ui.label("Saving PNG…");
                                    }
                                    ScreenshotStatus::Saved(path) => {
                                        ui.label("PNG saved")
                                            .on_hover_text(path.display().to_string());
                                    }
                                    ScreenshotStatus::Failed(error) => {
                                        ui.colored_label(egui::Color32::LIGHT_RED, "Screenshot failed")
                                            .on_hover_text(error);
                                    }
                                    ScreenshotStatus::Idle => {}
                                }
                                let errors: Vec<_> = [
                                    self.error.as_deref(),
                                    self.spray_error.as_deref(),
                                    self.model_panel.error.as_deref(),
                                    self.save_failed.then_some(self.save_status.as_str()),
                                    self.workspace_store
                                        .as_ref()
                                        .and_then(|s| s.error.as_deref()),
                                ]
                                .into_iter()
                                .flatten()
                                .collect();
                                if !errors.is_empty() {
                                    ui.menu_button(
                                        egui::RichText::new("Errors / Status")
                                            .color(egui::Color32::LIGHT_RED),
                                        |ui| {
                                            for e in errors {
                                                ui.label(e);
                                            }
                                        },
                                    );
                                }
                                ui.min_rect()
                            })
                            .inner;
                        let summary = self.performance.summary_text(self.optics.swe_enabled());
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let size = ui
                            .painter()
                            .layout_no_wrap(summary.clone(), font, ui.visuals().text_color())
                            .size();
                        let label = egui::Label::new(summary).halign(egui::Align::Max);
                        let response = if controls.right() + 16.0 + size.x <= toolbar_bounds.right()
                        {
                            let rect = egui::Rect::from_center_size(
                                egui::pos2(
                                    toolbar_bounds.right() - size.x * 0.5,
                                    controls.center().y,
                                ),
                                size,
                            );
                            ui.put(rect, label)
                        } else {
                            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                                ui.add(label.wrap())
                            })
                            .inner
                        };
                        response.on_hover_text(crate::ocean::ui::performance::SUMMARY_HELP);
                    });
                });
            egui::Panel::left("workspace navigation")
                .exact_size(54.0)
                .resizable(false)
                .frame(egui::Frame::side_top_panel(root_ui.style()).inner_margin(8.0))
                .show_inside(root_ui, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let segment_size = 38.0;
                        let items = WorkspaceId::ALL.map(|id| {
                            let color = if workspaces.active == id {
                                ui.visuals().selection.stroke.color
                            } else {
                                ui.visuals().text_color()
                            };
                            (id, self.icons[id.index()].clone().tint(color))
                        });
                        let response = ui.add(
                            IconSegmentedControl::new(&mut workspaces.active, items)
                                .vertical()
                                .size(segment_size)
                                .icon_size(24.0),
                        );
                        // The selector exposes one response; add per-workspace hover labels
                        // without replacing its native click and keyboard handling.
                        for id in WorkspaceId::ALL {
                            let rect = egui::Rect::from_min_size(
                                response.rect.min
                                    + egui::vec2(0.0, id.index() as f32 * segment_size),
                                egui::Vec2::splat(segment_size),
                            );
                            ui.interact(rect, ui.id().with(id), egui::Sense::hover())
                                .on_hover_text(id.name())
                                .widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::WidgetType::SelectableLabel,
                                        ui.is_enabled(),
                                        workspaces.active == id,
                                        id.name(),
                                    )
                                });
                        }
                    });
                });
            self.dock_padding_window(root_ui.ctx());
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(root_ui, |ui| {
                    let active = workspaces.active;
                    let workspace = workspaces.current();
                    workspace.prepare(&self.optics);
                    egui_dock::DockArea::new(&mut workspace.dock)
                        .id(egui::Id::new(("workspace dock", active)))
                        .show_close_buttons(true)
                        .show_leaf_close_all_buttons(false)
                        .style(self.dock_style.clone())
                        .show_inside(
                            ui,
                            &mut PanelViewer(|panel, ui: &mut egui::Ui| {
                                if panel == Panel::View {
                                    self.viewport_size = ocean_view(
                                        ui,
                                        self.viewport.id,
                                        camera,
                                        if self.optics.lake.enabled {
                                            self.optics.lake.terrain.radius * 3.0
                                        } else {
                                            current.length
                                        },
                                    );
                                    return;
                                }
                                ui.push_id((active, panel), |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_salt("parameters")
                                        .auto_shrink([false, false])
                                        .show(ui, |ui| {
                                            self.panel(
                                                panel, ui, current, camera, *paused, &mut apply,
                                            );
                                        });
                                });
                            }),
                        );
                    workspace.capture();
                });
        });
        if std::mem::take(&mut self.lake_panel.open) {
            workspaces.current().add(Panel::Lake);
        }
        if std::mem::take(&mut self.lake_panel.open_ripples) {
            workspaces.active = WorkspaceId::AnimatedSdf;
            workspaces.current().add(Panel::AnimatedSdf);
        }
        self.workspaces = workspaces;
        (output, apply)
    }

    fn dock_padding_window(&mut self, ctx: &egui::Context) {
        let mut body_padding = self.dock_style.tab.tab_body.inner_margin.left;
        let mut area_padding = self.dock_style.dock_area_padding.unwrap_or_default().left;
        egui::Window::new("Dock Padding Debug")
            .open(&mut self.show_dock_padding)
            .default_pos(egui::pos2(520.0, 80.0))
            .default_width(380.0)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                parameter_table(ui, "dock padding", |ui| {
                    parameter(
                        ui,
                        "Panel content padding",
                        "Padding on all sides of each Dock panel, including the Render View; 0–32 logical points (pt). Applies immediately without resetting the simulation.",
                        |ui| ui.add(ParameterSlider::new(&mut body_padding, 0..=32).suffix(" pt")),
                    );
                    parameter(
                        ui,
                        "Dock area padding",
                        "Left, right and bottom padding around the Dock area, not spacing between panels; top is fixed at 0. 0–32 logical points (pt); applies immediately without resetting the simulation.",
                        |ui| ui.add(ParameterSlider::new(&mut area_padding, 0..=32).suffix(" pt")),
                    );
                });
                ui.label("For this run only; not saved. At 100% scaling, 1 pt = 1 px.");
            });
        if body_padding != self.dock_style.tab.tab_body.inner_margin.left {
            self.dock_style.tab.tab_body.inner_margin = egui::Margin::same(body_padding);
        }
        if area_padding != self.dock_style.dock_area_padding.unwrap_or_default().left {
            self.dock_style.dock_area_padding = (area_padding != 0).then_some(egui::Margin {
                left: area_padding,
                right: area_padding,
                top: 0,
                bottom: area_padding,
            });
        }
    }

    fn panel(
        &mut self,
        panel: Panel,
        ui: &mut egui::Ui,
        current: OceanConfig,
        camera: &mut Camera,
        paused: bool,
        apply: &mut Option<OceanConfig>,
    ) {
        match panel {
            Panel::Scene => self.scene_panel(ui, camera, current.length),
            Panel::Lake => self.lake_panel(ui),
            Panel::AnimatedSdf => self.animated_sdf_panel(ui),
            Panel::Model => self
                .model_panel
                .show(ui, &mut self.optics, camera, current.length),
            Panel::Colliders => {
                crate::ocean::ui::panels::collider::show(ui, &mut self.optics, self.swe_domain)
            }
            Panel::Shore => self.shore_panel(ui),
            Panel::ShoreFoam => self.shore_foam_panel(ui),
            Panel::Spray => crate::ocean::ui::panels::spray::show(
                ui,
                &mut self.optics.spray,
                self.spray_error.as_deref(),
            ),
            Panel::Ocean => {
                if let Some(config) = self.ocean_panel(ui, current, camera) {
                    *apply = Some(config);
                }
            }
            Panel::OffshoreFoam => self.offshore_foam_panel(ui, camera),
            Panel::Optics => self.optics_panel(ui),
            Panel::Caustics => self.caustics_panel(ui),
            Panel::Refraction => self.refraction_panel(ui),
            Panel::Reflection => self.reflection_panel(ui),
            Panel::FoamMaterial => self.foam_material_panel(ui, camera),
            Panel::Surface => self.surface_panel(ui),
            Panel::SprayAppearance => {
                crate::ocean::ui::panels::spray::appearance(ui, &mut self.optics.spray)
            }
            Panel::Performance => self.performance.ui(ui),
            Panel::Configurations => {
                let settings = SavedSettings {
                    ocean: current,
                    draft: self.draft,
                    foam: self.foam_settings,
                    optics: self.optics,
                    camera: *camera,
                    speed: self.speed,
                    paused,
                    ..Default::default()
                };
                if let Some(settings) =
                    self.configuration_panel
                        .ui(ui, settings, self.settings_path.is_some())
                {
                    self.load_configuration = Some(settings);
                }
                self.save_now |= std::mem::take(&mut self.configuration_panel.save_now);
                if self.save_failed {
                    ui.colored_label(egui::Color32::LIGHT_RED, &self.save_status);
                }
            }
            Panel::View | Panel::Unknown => {}
        }
    }

    pub fn resize_view(&mut self, gpu: &Gpu, format: wgpu::TextureFormat) {
        self.viewport
            .resize(&mut self.backend, gpu, format, self.viewport_size);
    }
    pub fn encode(
        &mut self,
        window: &Window,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        output: egui::FullOutput,
    ) -> Vec<wgpu::CommandBuffer> {
        self.backend.encode(window, gpu, encoder, target, output)
    }
}

#[cfg(test)]
mod tests;

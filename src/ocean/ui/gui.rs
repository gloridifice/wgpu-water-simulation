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
    }
    if response.hovered() {
        let amount = ui.ctx().input(|input| input.smooth_scroll_delta.y) / 50.0;
        camera.distance =
            (camera.distance * (-amount * 0.1).exp()).clamp(length * 0.12, length * 0.95);
        if !ui.ctx().wants_keyboard_input() {
            ui.input(|input| {
                if input.focused {
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

pub struct Gui {
    pub context: egui::Context,
    pub input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    pub draft: OceanConfig,
    pub speed: f32,
    pub performance: crate::ocean::ui::performance::Performance,
    pub swe_domain: [f32; 4],
    pub model_panel: crate::ocean::ui::panels::model::ModelPanel,
    pub error: Option<String>,
    pub spray_error: Option<String>,
    pub foam_settings: FoamConfig,
    pub clear_foam: bool,
    pub optics: OpticsConfig,
    pub save_now: bool,
    pub save_status: String,
    pub save_failed: bool,
    pub settings_path: Option<std::path::PathBuf>,
    pub configuration_panel: ConfigurationPanel,
    pub load_configuration: Option<SavedSettings>,
    pub workspaces: Workspaces,
    pub workspace_store: Option<WorkspaceStore>,
    icons: [egui::Image<'static>; 5],
    dock_style: egui_dock::Style,
    show_dock_padding: bool,
    pub viewport_size: [u32; 2],
    pub viewport_view: wgpu::TextureView,
    viewport_texture: wgpu::Texture,
    viewport_id: egui::TextureId,
    pub(super) sigma_editor: CoefficientEditor,
    pub(super) source_editor: CoefficientEditor,
}

impl Gui {
    pub fn new(
        window: &Window,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        config: OceanConfig,
    ) -> Self {
        let context = egui::Context::default();
        let dock_style = crate::ocean::ui::theme::install(&context);
        egui_extras::install_image_loaders(&context);
        let sources = [
            include_str!("../../../assets/ui/workspaces/scene.svg"),
            include_str!("../../../assets/ui/workspaces/shallow.svg"),
            include_str!("../../../assets/ui/workspaces/offshore.svg"),
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
        let sampled_view = viewport_texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        let viewport_id =
            renderer.register_native_texture(&gpu.device, &sampled_view, wgpu::FilterMode::Linear);
        Self {
            context,
            input,
            renderer,
            draft: config,
            speed: 1.0,
            performance: crate::ocean::ui::performance::Performance::default(),
            swe_domain: [-32.0, -192.0, 64.0, 384.0],
            model_panel: crate::ocean::ui::panels::model::ModelPanel::default(),
            error: None,
            spray_error: None,
            foam_settings: FoamConfig::default(),
            clear_foam: false,
            optics: OpticsConfig::default(),
            save_now: false,
            save_status: "本地保存已禁用".into(),
            save_failed: false,
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
            viewport_texture,
            viewport_view,
            viewport_id,
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
        let output = context.run(input, |ctx| {
            egui::TopBottomPanel::top("workspace header")
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    egui::Frame::new().inner_margin(6.0).show(ui, |ui| {
                        let toolbar_bounds = ui.available_rect_before_wrap();
                        let controls = ui
                            .horizontal_wrapped(|ui| {
                                ui.strong(format!("{}工作区", workspaces.active.name()));
                                ui.separator();
                                if ui.button(if *paused { "继续" } else { "暂停" }).clicked() {
                                    *paused = !*paused;
                                }
                                ui.add(
                                    egui::DragValue::new(&mut self.speed)
                                        .range(0.0..=3.0)
                                        .speed(0.01)
                                        .prefix("倍率 "),
                                )
                                .on_hover_text("模拟时间倍率，0～3，实时生效；0 停止演化。");
                                ui.menu_button("时间 / 相机", |ui| {
                                    ui.label(format!("模拟时间 {time:.2} s"));
                                    if ui.button("重新计时").clicked() {
                                        *time = 0.0;
                                        self.clear_foam = true;
                                        ui.close();
                                    }
                                    if ui.button("重置相机").clicked() {
                                        *camera = Camera::new(current.length);
                                        ui.close();
                                    }
                                    if self.optics.beach && ui.button("沙滩相机").clicked() {
                                        *camera = Camera::beach(current.length);
                                        ui.close();
                                    }
                                    if self.optics.model.enabled && ui.button("模型相机").clicked()
                                    {
                                        *camera = Camera::model(current.length);
                                        ui.close();
                                    }
                                });
                                ui.menu_button("面板", |ui| {
                                    for panel in Panel::ALL
                                        .into_iter()
                                        .filter(|p| *p != Panel::View && p.available(&self.optics))
                                    {
                                        let present = workspaces.current().contains(panel);
                                        if ui
                                            .selectable_label(present, panel.name())
                                            .on_hover_text(if present {
                                                "定位已添加面板"
                                            } else {
                                                "添加到当前工作区"
                                            })
                                            .clicked()
                                        {
                                            workspaces.current().add(panel);
                                            ui.close();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("Dock 边距调试").clicked() {
                                        self.show_dock_padding = true;
                                        ui.close();
                                    }
                                    if ui.button("重置当前布局").clicked() {
                                        workspaces.reset_current();
                                        if let Some(store) = &mut self.workspace_store {
                                            store.allow_repair();
                                        }
                                        ui.close();
                                    }
                                    if let Some(store) = &mut self.workspace_store
                                        && store.blocked
                                        && ui.button("确认保存修复后的布局").clicked()
                                    {
                                        store.allow_repair();
                                        ui.close();
                                    }
                                });
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
                                        egui::RichText::new("错误 / 状态")
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
            egui::SidePanel::left("workspace navigation")
                .exact_width(54.0)
                .resizable(false)
                .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(8.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;
                        for id in WorkspaceId::ALL {
                            if id == WorkspaceId::Configurations {
                                ui.separator();
                            }
                            let selected = workspaces.active == id;
                            let mut button =
                                egui::Button::new("").selected(selected).corner_radius(8);
                            if !selected {
                                button = button
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE);
                            }
                            let response =
                                ui.add_sized([38.0, 38.0], button).on_hover_text(id.name());
                            response.widget_info(|| {
                                egui::WidgetInfo::selected(
                                    egui::WidgetType::SelectableLabel,
                                    true,
                                    selected,
                                    id.name(),
                                )
                            });
                            let color = ui
                                .style()
                                .interact_selectable(&response, selected)
                                .text_color();
                            let icon_rect = egui::Rect::from_center_size(
                                response.rect.center(),
                                egui::vec2(24.0, 24.0),
                            );
                            self.icons[id.index()]
                                .clone()
                                .tint(color)
                                .paint_at(ui, icon_rect);
                            if response.clicked() {
                                workspaces.active = id;
                            }
                        }
                    });
                });
            self.dock_padding_window(ctx);
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
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
                                    self.viewport_size =
                                        ocean_view(ui, self.viewport_id, camera, current.length);
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
        self.workspaces = workspaces;
        (output, apply)
    }

    fn dock_padding_window(&mut self, ctx: &egui::Context) {
        let mut body_padding = self.dock_style.tab.tab_body.inner_margin.left;
        let mut area_padding = self.dock_style.dock_area_padding.unwrap_or_default().left;
        egui::Window::new("Dock 边距调试")
            .open(&mut self.show_dock_padding)
            .default_pos(egui::pos2(520.0, 80.0))
            .default_width(380.0)
            .resizable(false)
            .collapsible(false)
            .show(ctx, |ui| {
                super::controls::parameter_table(ui, "dock padding", |ui| {
                    super::controls::parameter(
                        ui,
                        "面板内容 padding",
                        "每个 Dock 面板内容区的四边留白，包括渲染视图；0～32 逻辑点（pt），实时生效，不重置模拟。",
                        |ui| ui.add(egui::Slider::new(&mut body_padding, 0..=32).suffix(" pt")),
                    );
                    super::controls::parameter(
                        ui,
                        "Dock 区域 padding",
                        "整个 Dock 区域外围的左、右、底部留白，顶部固定为 0，不是面板间距；0～32 逻辑点（pt），实时生效，不重置模拟。",
                        |ui| ui.add(egui::Slider::new(&mut area_padding, 0..=32).suffix(" pt")),
                    );
                });
                ui.label("仅本次运行，不保存；100% 缩放时 1 pt = 1 px。");
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
mod tests;

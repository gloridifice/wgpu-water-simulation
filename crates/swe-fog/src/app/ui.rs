//! Fog business panels on the common egui/Dock and persistence mechanisms.
#[cfg(test)]
mod tests;
use crate::{
    interactions::MovementMode,
    parameters::{Reconstruction, Session},
    simulation::Diagnostics,
};
use common::{
    gpu::Gpu,
    settings::{ConfigurationLibrary, SettingsStore, sidecar::Sidecar},
    ui::{
        Egui, Viewport,
        layout::{Axis, Layout, PanelId, Workspace},
    },
};
use serde::{Deserialize, Serialize};
use winit::window::Window;
use yummy_egui::{
    IconButton, IconSegmentedControl, SelectionList, parameter, parameter_table, segmented_control,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Panel {
    View,
    Initialization,
    Dynamics,
    Interactions,
    Appearance,
    Configuration,
    Diagnostics,
    #[serde(other)]
    Unknown,
}
impl PanelId for Panel {
    const VIEW: Self = Self::View;
    const UNKNOWN: Self = Self::Unknown;
    const ALL: &'static [Self] = &[
        Self::View,
        Self::Initialization,
        Self::Dynamics,
        Self::Interactions,
        Self::Appearance,
        Self::Configuration,
        Self::Diagnostics,
    ];
}
impl Panel {
    fn name(self) -> &'static str {
        match self {
            Self::View => "渲染视图",
            Self::Initialization => "初始化",
            Self::Dynamics => "动力",
            Self::Interactions => "玩家交互",
            Self::Appearance => "外观",
            Self::Configuration => "配置",
            Self::Diagnostics => "诊断",
            Self::Unknown => "未知",
        }
    }
}
fn layout(category: usize) -> Layout<Panel> {
    let tabs = match category {
        0 => vec![Panel::Initialization, Panel::Dynamics, Panel::Interactions],
        1 => vec![Panel::Appearance, Panel::Diagnostics],
        _ => vec![Panel::Configuration],
    };
    Layout::split(
        Axis::Horizontal,
        0.32,
        Layout::tabs(tabs),
        Layout::tabs(vec![Panel::View]),
    )
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct LayoutDocument {
    version: u32,
    active: usize,
    layouts: Vec<Layout<Panel>>,
}
#[derive(Default)]
pub struct Actions {
    pub reset: bool,
    pub save: bool,
    pub capture: bool,
    pub load: Option<(Option<usize>, Session)>,
}
pub struct Status {
    pub resolution: [u32; 2],
    pub diagnostics: Diagnostics,
    pub fps: f32,
    pub gpu: String,
    pub save: String,
    pub save_failed: bool,
    pub save_path: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,
}
#[derive(Clone, Copy)]
enum Editor {
    New,
    Rename(usize),
    Delete(usize),
}
pub struct Interface {
    pub backend: Egui,
    pub viewport: Viewport,
    pub size: [u32; 2],
    pub library: ConfigurationLibrary<Session>,
    workspaces: Vec<Workspace<Panel>>,
    active: usize,
    icons: [egui::Image<'static>; 5],
    sidecar: Option<Sidecar<LayoutDocument>>,
    writable: bool,
    name: String,
    editor: Option<Editor>,
    configuration_error: Option<String>,
}
struct Tabs<F>(F);
impl<F: FnMut(Panel, &mut egui::Ui)> egui_dock::TabViewer for Tabs<F> {
    type Tab = Panel;
    fn title(&mut self, p: &mut Panel) -> egui::WidgetText {
        p.name().into()
    }
    fn ui(&mut self, ui: &mut egui::Ui, p: &mut Panel) {
        (self.0)(*p, ui)
    }
    fn is_closeable(&self, p: &Panel) -> bool {
        *p != Panel::View
    }
    fn allowed_in_windows(&self, _: &mut Panel) -> bool {
        false
    }
    fn scroll_bars(&self, _: &Panel) -> [bool; 2] {
        [false, false]
    }
}
impl Interface {
    pub fn new(
        window: &Window,
        gpu: &Gpu,
        format: wgpu::TextureFormat,
        session: &Session,
        store: Option<&SettingsStore<Session>>,
    ) -> Self {
        let mut backend = Egui::new(window, gpu, format);
        egui_extras::install_image_loaders(&backend.context);
        let sources = [
            include_str!("../../assets/icons/haze.svg"),
            include_str!("../../assets/icons/palette.svg"),
            include_str!("../../assets/icons/sliders-horizontal.svg"),
            include_str!("../../assets/icons/panel-left-open.svg"),
            include_str!("../../assets/icons/rotate-ccw.svg"),
        ];
        // Cache white SVGs once; tint follows the current theme and selection.
        let icons = std::array::from_fn(|i| {
            egui::Image::from_bytes(
                format!("bytes://fog-navigation-{i}.svg"),
                sources[i].replace("currentColor", "#ffffff").into_bytes(),
            )
        });
        let mut fonts = egui::FontDefinitions::default();
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
        backend.context.set_fonts(fonts);
        let viewport = Viewport::new(&mut backend, gpu, format);
        let mut workspaces: Vec<_> = (0..3).map(|i| Workspace::new(layout(i))).collect();
        let mut active = 0;
        let mut sidecar = store.map(|s| Sidecar::<LayoutDocument>::new(&s.path));
        if let Some(s) = &mut sidecar {
            match s.read_json().and_then(|v| {
                v.map(serde_json::from_value::<LayoutDocument>)
                    .transpose()
                    .map_err(|e| e.to_string())
            }) {
                Ok(Some(doc)) => {
                    if doc.version != 1 || doc.active >= 3 || doc.layouts.len() != 3 {
                        s.read_failed("不支持的雾布局".into());
                    } else {
                        let restored: Result<Vec<_>, _> =
                            doc.layouts.into_iter().map(Layout::validate).collect();
                        match restored {
                            Ok(layouts) => {
                                active = doc.active;
                                workspaces = layouts.iter().cloned().map(Workspace::new).collect();
                                s.loaded(LayoutDocument {
                                    version: 1,
                                    active,
                                    layouts,
                                });
                            }
                            Err(e) => s.read_failed(e),
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => s.read_failed(e),
            }
        }
        Self {
            backend,
            viewport,
            size: [1, 1],
            library: store
                .map(|s| s.configurations().clone())
                .unwrap_or_else(|| ConfigurationLibrary::new(session.clone())),
            workspaces,
            active,
            icons,
            sidecar,
            writable: store.is_some(),
            name: String::new(),
            editor: None,
            configuration_error: None,
        }
    }
    pub fn persist(&mut self, force: bool) {
        if let Some(store) = &mut self.sidecar {
            store.update(
                LayoutDocument {
                    version: 1,
                    active: self.active,
                    layouts: self.workspaces.iter().map(|w| w.layout.clone()).collect(),
                },
                std::time::Instant::now(),
                force,
            );
        }
    }
    pub fn frame(
        &mut self,
        window: &Window,
        session: &mut Session,
        status: &Status,
    ) -> (egui::FullOutput, Actions) {
        let input = self.backend.input.take_egui_input(window);
        let context = self.backend.context.clone();
        let mut actions = Actions::default();
        let mut workspaces = std::mem::take(&mut self.workspaces);
        let style = yummy_egui::Theme::default().dock_style().clone();
        let mut reset_layout = false;
        let output=context.run_ui(input,|root|{
            if root.input_mut(|i|i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND,egui::Key::S)))&&self.writable{actions.save=true;}
            egui::Panel::top("fog header").show_inside(root,|ui|{
                ui.horizontal(|ui|{ui.label(egui::RichText::new("SWE / FOG").strong());if ui.button(if session.paused{"继续"}else{"暂停"}).clicked(){session.paused = !session.paused;}
                    if ui.button("重置雾场").clicked(){actions.reset=true;}
                    if ui.add_enabled(self.writable,egui::Button::new("保存")).on_disabled_hover_text("只读配置或 --no-settings 模式不写盘").clicked(){actions.save=true;}
                    let label=if status.save_failed{ui.colored_label(egui::Color32::LIGHT_RED,&status.save)}else{ui.label(&status.save)};
                    label.on_hover_text(status.save_path.as_deref().unwrap_or("本地保存已禁用"));
                    ui.menu_button("操作",|ui|{ui.label("单击视图捕获鼠标；Esc 释放");ui.label("WASD 移动，空格跳跃，Q 退出");ui.label("左拖数值，Shift 精调，双击输入");ui.label("数字改动重置雾；平滑强度、积分精度与开关不重置");});ui.separator();
                    if let Some(e)=&status.error{ui.colored_label(egui::Color32::LIGHT_RED,"错误").on_hover_text(e);}
                    if let Some(w)=&status.warning{ui.colored_label(egui::Color32::YELLOW,"警告").on_hover_text(w);}
                    ui.label(format!("FPS {:.1}",status.fps)).on_hover_text(format!("GPU {}",status.gpu));
                    ui.menu_button("诊断",|ui|{
                        let d=status.diagnostics;
                        ui.label(format!("模拟网格 {} × {}",status.resolution[0],status.resolution[1]));
                        ui.label(format!("GPU {}",status.gpu));
                        ui.label(format!("模拟 {:.2} s · g={:.3} m/s²",d.simulated_time,if session.fog.gravity_enabled{session.fog.reduced_gravity}else{0.0}));
                        ui.label(format!("h {:.3}–{:.3} m · 最大速度 {:.2} cm/s",d.min_height,d.max_height,d.max_speed*100.0));
                    });
                });
            });
            egui::Panel::left("fog navigation").exact_size(54.0).resizable(false)
                .frame(egui::Frame::side_top_panel(root.style()).inner_margin(8.0))
                .show_inside(root,|ui|{
                let segment_size = 38.0;
                let items: [_; 3] = std::array::from_fn(|i| {
                    let color=if self.active==i{ui.visuals().selection.stroke.color}else{ui.visuals().text_color()};
                    (i, self.icons[i].clone().tint(color))
                });
                let response = ui.add(IconSegmentedControl::new(&mut self.active, items).vertical().size(segment_size).icon_size(24.0));
                // Add labels over the shared response without taking over the
                // selector's native click and keyboard handling.
                for (i,name) in ["模拟","外观","配置"].into_iter().enumerate(){
                    let rect = egui::Rect::from_min_size(response.rect.min + egui::vec2(0.0, i as f32 * segment_size), egui::Vec2::splat(segment_size));
                    ui.interact(rect, ui.id().with(("fog category",i)), egui::Sense::hover())
                        .on_hover_text(name)
                        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), self.active==i, name));
                }
                ui.separator();
                let add=ui.add(IconButton::new(self.icons[3].clone().tint(ui.visuals().text_color())).size(segment_size).icon_size(24.0).label("添加面板"));
                egui::Popup::menu(&add).show(|ui|{
                    for p in <Panel as PanelId>::ALL.iter().copied().filter(|p|*p!=Panel::View){if ui.button(p.name()).clicked(){workspaces[self.active].add(p);ui.close();}}
                });
                if ui.add(IconButton::new(self.icons[4].clone().tint(ui.visuals().text_color())).size(segment_size).icon_size(24.0).label("重排：重置当前布局；损坏的布局文件将允许修复。不重置模拟或相机。")).clicked(){reset_layout=true;}
            });
            egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(root,|ui|{
                let active=self.active;let workspace=&mut workspaces[active];workspace.prepare(|p|p!=Panel::Unknown);
                egui_dock::DockArea::new(&mut workspace.dock).id(egui::Id::new(("fog dock",active))).style(style.clone()).show_leaf_close_all_buttons(false).show_inside(ui,&mut Tabs(|panel,ui:&mut egui::Ui|{
                    if panel==Panel::View{let size=ui.available_size().max(egui::vec2(1.0,1.0));let response=ui.add(egui::Image::new((self.viewport.id,size)).sense(egui::Sense::click()));self.size=[(size.x*ui.ctx().pixels_per_point()).round().max(1.0) as u32,(size.y*ui.ctx().pixels_per_point()).round().max(1.0) as u32];if response.clicked(){actions.capture=true;}return;}
                    ui.push_id((active,panel),|ui|{egui::ScrollArea::vertical().auto_shrink([false,false]).show(ui,|ui|match panel{
                        Panel::Initialization=>initialization(ui,session,&mut actions),Panel::Dynamics=>dynamics(ui,session,&mut actions),Panel::Interactions=>interactions(ui,session,&mut actions),Panel::Appearance=>appearance(ui,session,&mut actions),Panel::Configuration=>self.configuration(ui,session,&mut actions),Panel::Diagnostics=>{
                            let d=status.diagnostics;ui.label(format!("接受子步 {} · 连续重试 {}\n积压 {:.4} s · 丢弃 {:.4} s\nGPU fault {}",d.accepted,d.retries,d.remaining,d.dropped_time,d.fault));ui.label("GPU 为真实 timestamp 异步采样，仅计雾模拟；不含呈现等待。");if let Some(sidecar)=&self.sidecar && let Some(e)=&sidecar.error {ui.colored_label(egui::Color32::LIGHT_RED,e);}
                        },_=>{}
                    });});
                }));workspace.capture();
            });
        });
        self.workspaces = workspaces;
        if reset_layout {
            self.workspaces[self.active] = Workspace::new(layout(self.active));
            if let Some(s) = &mut self.sidecar {
                s.allow_repair();
            }
        }
        (output, actions)
    }
    fn configuration(&mut self, ui: &mut egui::Ui, session: &Session, actions: &mut Actions) {
        yummy_egui::h3(ui, "命名配置");
        let mut chosen = None;
        SelectionList::new("fog configurations")
            .max_height(220.0)
            .show(ui, |list| {
                if list
                    .item(None::<usize>, self.library.active().is_none(), "默认配置")
                    .clicked()
                {
                    chosen = Some(None);
                }
                for (i, p) in self.library.profiles().iter().enumerate() {
                    if list
                        .item(Some(i), self.library.active() == Some(i), p.name())
                        .clicked()
                    {
                        chosen = Some(Some(i));
                    }
                }
            });
        if let Some(index) = chosen {
            match self.library.get(index) {
                Ok(s) => actions.load = Some((index, s.clone())),
                Err(e) => self.configuration_error = Some(e),
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.writable, egui::Button::new("新建"))
                .clicked()
            {
                self.name.clear();
                self.editor = Some(Editor::New);
            }
            if ui
                .add_enabled(
                    self.writable && self.library.active().is_some(),
                    egui::Button::new("重命名"),
                )
                .clicked()
            {
                let i = self.library.active().unwrap();
                self.name = self.library.profiles()[i].name().into();
                self.editor = Some(Editor::Rename(i));
            }
            if ui
                .add_enabled(
                    self.writable && self.library.active().is_some(),
                    egui::Button::new("删除"),
                )
                .clicked()
            {
                self.editor = Some(Editor::Delete(self.library.active().unwrap()));
            }
        });
        if let Some(editor) = self.editor {
            if matches!(editor, Editor::Delete(_)) {
                ui.label("确认删除所选配置？未手动保存前不写盘。");
            } else {
                ui.text_edit_singleline(&mut self.name);
            }
            ui.horizontal(|ui| {
                if ui.button("确定").clicked() {
                    let result = match editor {
                        Editor::New => self
                            .library
                            .save_as(&self.name, session.clone())
                            .map(|_| ()),
                        Editor::Rename(i) => self.library.rename(Some(i), &self.name),
                        Editor::Delete(i) => self.library.delete(Some(i)).map(|()| {
                            actions.load = Some((None, self.library.get(None).unwrap().clone()));
                        }),
                    };
                    match result {
                        Ok(()) => {
                            self.editor = None;
                            self.configuration_error = None;
                        }
                        Err(e) => self.configuration_error = Some(e),
                    }
                }
                if ui.button("取消").clicked() {
                    self.editor = None;
                }
            });
        }
        if ui
            .add_enabled(
                self.writable && self.library.active().is_some(),
                egui::Button::new("覆盖所选配置并保存"),
            )
            .clicked()
        {
            match self
                .library
                .overwrite(self.library.active(), session.clone())
            {
                Ok(()) => actions.save = true,
                Err(e) => self.configuration_error = Some(e),
            }
        }
        ui.label("顶部保存 / Ctrl+S 保存工作状态与配置库，不覆盖命名快照。布局独立自动保存。");
        if let Some(e) = &self.configuration_error {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
    }
}
fn number(
    ui: &mut egui::Ui,
    name: &str,
    help: &str,
    value: &mut f32,
    lo: f32,
    hi: f32,
    step: f64,
) -> bool {
    let mut changed = false;
    parameter(ui, name, help, |ui| {
        let r = ui.add(
            egui::DragValue::new(value)
                .range(lo..=hi)
                .speed(step)
                .max_decimals(4),
        );
        changed = r.changed();
        r
    });
    changed
}
fn integer(ui: &mut egui::Ui, name: &str, value: &mut u32, lo: u32, hi: u32) -> bool {
    let mut changed = false;
    parameter(
        ui,
        name,
        "左拖调节，Shift 精调，双击输入；改值实时重置雾场。",
        |ui| {
            let r = ui.add(egui::DragValue::new(value).range(lo..=hi).speed(1.0));
            changed = r.changed();
            r
        },
    );
    changed
}
const RESET: &str =
    "左拖调节，Shift 精调，双击输入；数值变化立即重置雾场，暂停状态与玩家位置保持不变。";
fn initialization(ui: &mut egui::Ui, s: &mut Session, a: &mut Actions) {
    yummy_egui::h3(ui, "初始化");
    parameter_table(ui, "fog initialization", |ui| {
        let before = s.grid_resolution;
        parameter(
            ui,
            "网格分辨率",
            "每轴格点数；可选 64、128、256、512，默认地形为 256×256。提高分辨率可表达更细的回卷，但增加计算量。切换后从静态场景重新生成内存地形、重建 GPU 网格并重置雾；场地尺寸、玩家、相机与暂停状态保持不变。选择会随会话手动保存。",
            |ui| {
                let label = s
                    .grid_resolution
                    .map_or_else(|| "地形原始网格".into(), |r| format!("{} × {}", r[0], r[1]));
                egui::ComboBox::from_id_salt("fog grid resolution")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut s.grid_resolution, None, "地形原始网格");
                        for n in [64, 128, 256, 512] {
                            ui.selectable_value(
                                &mut s.grid_resolution,
                                Some([n, n]),
                                format!("{n} × {n}"),
                            );
                        }
                    })
                    .response
            },
        );
        a.reset |= before != s.grid_resolution;
        a.reset |= number(
            ui,
            "基础高度 (m)",
            RESET,
            &mut s.fog.base_height,
            0.0,
            4.0,
            0.01,
        );
        a.reset |= number(
            ui,
            "噪波振幅 (m)",
            RESET,
            &mut s.fog.noise_amplitude,
            0.0,
            2.0,
            0.01,
        );
        a.reset |= number(
            ui,
            "噪波频率 (1/m)",
            RESET,
            &mut s.fog.noise_frequency,
            0.001,
            4.0,
            0.001,
        );
        a.reset |= integer(ui, "噪波层数", &mut s.fog.noise_octaves, 1, 6);
        a.reset |= integer(ui, "噪波种子", &mut s.fog.noise_seed, 0, 65535);
        a.reset |= number(
            ui,
            "高度 offset (m)",
            RESET,
            &mut s.fog.height_offset,
            -2.0,
            2.0,
            0.01,
        );
        a.reset |= number(
            ui,
            "采样偏移 X (m)",
            RESET,
            &mut s.fog.noise_offset[0],
            -1000.0,
            1000.0,
            0.5,
        );
        a.reset |= number(
            ui,
            "采样偏移 Z (m)",
            RESET,
            &mut s.fog.noise_offset[1],
            -1000.0,
            1000.0,
            0.5,
        );
    });
    ui.checkbox(&mut s.fog.uniform_top, "水平雾顶初态（下次重置）");
    if ui.button("重新初始化雾场").clicked() {
        a.reset = true;
    }
}
fn dynamics(ui: &mut egui::Ui, s: &mut Session, a: &mut Actions) {
    yummy_egui::h3(ui, "动力");
    ui.checkbox(&mut s.fog.gravity_enabled, "启用约化重力");
    ui.checkbox(&mut s.fog.decay_enabled, "启用缓慢消散");
    ui.checkbox(&mut s.fog.closed_boundary, "封闭边界");
    parameter_table(ui, "fog dynamics", |ui| {
        a.reset |= number(
            ui,
            "约化重力 (m/s²)",
            RESET,
            &mut s.fog.reduced_gravity,
            0.0,
            1.0,
            0.005,
        );
        a.reset |= number(
            ui,
            "微风 X (m/s)",
            RESET,
            &mut s.fog.wind[0],
            -0.5,
            0.5,
            0.01,
        );
        a.reset |= number(
            ui,
            "微风 Z (m/s)",
            RESET,
            &mut s.fog.wind[1],
            -0.5,
            0.5,
            0.01,
        );
        a.reset |= number(
            ui,
            "风响应时间 (s)",
            RESET,
            &mut s.fog.wind_response,
            0.1,
            600.0,
            0.5,
        );
        a.reset |= number(
            ui,
            "半衰期 (s)",
            RESET,
            &mut s.fog.half_life,
            1.0,
            3600.0,
            1.0,
        );
        a.reset |= number(
            ui,
            "浓度扩散 (m²/s)",
            RESET,
            &mut s.fog.diffusion,
            0.0,
            1.0,
            0.001,
        );
        a.reset |= integer(ui, "空间阶数", &mut s.fog.spatial_order, 1, 2);
    });
}
fn appearance(ui: &mut egui::Ui, s: &mut Session, a: &mut Actions) {
    yummy_egui::h3(ui, "白雾外观");
    ui.checkbox(&mut s.visible, "显示白雾");
    parameter_table(ui, "fog appearance", |ui| {
        parameter(
            ui,
            "重建方式",
            "双线性使用 2×2 邻格；SmoothStep 先以 t²(3−2t) 平滑 X/Z 插值权重，再读取同样的 2×2 原始邻格，保留格点值；预平滑生成 3×3 平滑结果后用双线性采样；B 样条逐点采样 4×4 邻格，成本较高。各模式均受障碍物约束，并共用积分精度。切换实时生效、不重置，随会话手动保存；原始格点诊断不受影响。",
            |ui| {
                let modes = [
                    (Reconstruction::Bilinear, "双线性"),
                    (Reconstruction::SmoothStep, "SmoothStep"),
                    (Reconstruction::PreSmoothed, "预平滑"),
                    (Reconstruction::CubicBSpline, "B 样条"),
                ];
                let segment_width = ui
                    .painter()
                    .layout_no_wrap(
                        "SmoothStep".into(),
                        egui::TextStyle::Button.resolve(ui.style()),
                        ui.visuals().text_color(),
                    )
                    .size()
                    .x
                    + 2.0 * ui.spacing().button_padding.x;
                if ui.available_width() >= segment_width * modes.len() as f32 {
                    segmented_control(ui, &mut s.reconstruction, &modes)
                } else {
                    ui.vertical(|ui| {
                        let first = segmented_control(ui, &mut s.reconstruction, &modes[..2]);
                        first | segmented_control(ui, &mut s.reconstruction, &modes[2..])
                    })
                    .inner
                }
            },
        );
        if s.reconstruction == Reconstruction::PreSmoothed {
            number(
                ui,
                "平滑强度",
                "无量纲，0–1；0 使用原始双线性，1 使用完整 3×3 平滑。只改变渲染厚度与雾量，不改变底高或模拟。实时生效、不重置；暂停时也能调节，随会话手动保存。",
                &mut s.reconstruction_smoothing,
                0.0,
                1.0,
                0.02,
            );
        }
        parameter(
            ui,
            "积分精度",
            "每个穿过的 XZ 网格内的基础积分段数，2–32 段/格，默认 2。可能穿雾的区间再自适应二分，最多 3 层；XZ 投影不超过 1/64 格时停止，均匀层和竖直视线直接积分。增加基础段数可减轻漏采与走样，但增加 GPU 耗时，B 样条成本更高；有限细分仍非严格求交。建议暂停后从 2 开始对比。实时生效、不重置模拟；随会话手动保存。",
            |ui| {
                ui.add(
                    yummy_egui::ParameterSlider::new(&mut s.integration_steps, 2..=32)
                        .suffix(" 段/格"),
                )
            },
        );
        a.reset |= number(ui, "消光强度", RESET, &mut s.fog.extinction, 0.0, 40.0, 0.1);
        a.reset |= number(ui, "白雾亮度", RESET, &mut s.fog.brightness, 0.0, 4.0, 0.02);
    });
    egui::ComboBox::from_label("诊断视图")
        .selected_text(
            ["关闭", "层厚", "面雾量", "速度", "雾顶", "体积不透明度"][s.debug_view as usize],
        )
        .show_ui(ui, |ui| {
            for (i, name) in ["关闭", "层厚", "面雾量", "速度", "雾顶", "体积不透明度"]
                .into_iter()
                .enumerate()
            {
                ui.selectable_value(&mut s.debug_view, i as u32, name);
            }
        });
}
fn interactions(ui: &mut egui::Ui, s: &mut Session, a: &mut Actions) {
    yummy_egui::h3(ui, "玩家拨雾");
    ui.checkbox(&mut s.interactions_enabled, "启用玩家交互");
    let p = &mut s.interaction;
    ui.checkbox(&mut p.move_enabled, "移动扰动");
    ui.checkbox(&mut p.jump_enabled, "起跳冲击");
    ui.checkbox(&mut p.landing_enabled, "落地冲击");
    parameter_table(ui, "fog player interactions", |ui| {
        parameter(
            ui,
            "移动模式",
            "持续拖带在角色周围带动雾，并沿路径留下反向尾涡；原有脉冲用于对照。切换立即生效并清理旧来源，不重置雾场。",
            |ui| {
                egui::ComboBox::from_id_salt("movement mode")
                    .selected_text(match p.movement_mode {
                        MovementMode::Wake => "拖带与回旋",
                        MovementMode::Impulse => "原有脉冲",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut p.movement_mode, MovementMode::Wake, "拖带与回旋");
                        ui.selectable_value(
                            &mut p.movement_mode,
                            MovementMode::Impulse,
                            "原有脉冲",
                        );
                    })
                    .response
            },
        );
        if p.movement_mode == MovementMode::Wake {
            for (name, help, value, lo, hi, step) in [
                (
                    "拖带半径 (m)",
                    "角色周围的作用半径，0.5–4 米；边缘平滑减弱。改值立即重置雾。",
                    &mut p.drag_radius,
                    0.5,
                    4.0,
                    0.05,
                ),
                (
                    "拖带响应时间 (s)",
                    "雾趋近角色目标速度的响应时间，0.05–3 模拟秒；越小跟随越快。改值立即重置雾。",
                    &mut p.drag_response,
                    0.05,
                    3.0,
                    0.02,
                ),
                (
                    "拖带速度比例",
                    "目标速度与实际角色速度的比例，无量纲，0–1。改值立即重置雾。",
                    &mut p.drag_velocity_ratio,
                    0.0,
                    1.0,
                    0.02,
                ),
                (
                    "尾涡半径 (m)",
                    "每个尾涡的作用半径，0.5–4 米；默认网格建议从 2 米左右开始。改值立即重置雾。",
                    &mut p.wake_radius,
                    0.5,
                    4.0,
                    0.05,
                ),
                (
                    "尾涡角速度 (rad/s)",
                    "正常步速下的旋转目标角速度，0–3 弧度/模拟秒；低速移动按比例减弱，零值关闭尾涡旋转。改值立即重置雾。",
                    &mut p.wake_angular_speed,
                    0.0,
                    3.0,
                    0.05,
                ),
                (
                    "尾涡响应时间 (s)",
                    "雾趋近尾涡目标速度的响应时间，0.05–3 模拟秒；越小施力越强。改值立即重置雾。",
                    &mut p.wake_response,
                    0.05,
                    3.0,
                    0.02,
                ),
                (
                    "尾涡后移 (m)",
                    "尾涡出生中心位于角色移动方向后方的距离，0–4 米。出生后固定在路径上。改值立即重置雾。",
                    &mut p.wake_offset,
                    0.0,
                    4.0,
                    0.05,
                ),
                (
                    "尾涡左右间距 (m)",
                    "一对反向尾涡两个中心之间的完整距离，0.5–6 米。改值立即重置雾。",
                    &mut p.wake_separation,
                    0.5,
                    6.0,
                    0.05,
                ),
                (
                    "尾涡发射间距 (m)",
                    "沿实际移动路径每隔 0.25–4 米生成一对尾涡，起步立即生成；越小来源越密。改值立即重置雾。",
                    &mut p.wake_spacing,
                    0.25,
                    4.0,
                    0.05,
                ),
                (
                    "尾涡施力时间 (s)",
                    "每对来源保留 0.05–3 个启用交互时的物理秒，响应逐渐减弱。到期只撤除外力，保留雾中惯性。暂停会清理来源。改值立即重置雾。",
                    &mut p.wake_lifetime,
                    0.05,
                    3.0,
                    0.02,
                ),
            ] {
                a.reset |= number(ui, name, help, value, lo, hi, step);
            }
        } else {
            a.reset |= number(
                ui,
                "移动纹理速度 (m/s)",
                RESET,
                &mut p.move_impulse_speed,
                0.0,
                6.0,
                0.05,
            );
            a.reset |= number(
                ui,
                "移动冲量强度",
                RESET,
                &mut p.move_impulse_strength,
                0.0,
                1.0,
                0.01,
            );
            a.reset |= number(
                ui,
                "冲量间隔 (s)",
                RESET,
                &mut p.move_impulse_interval,
                0.02,
                2.0,
                0.01,
            );
            a.reset |= number(
                ui,
                "扇形宽度 (m)",
                RESET,
                &mut p.move_width,
                0.25,
                8.0,
                0.05,
            );
            a.reset |= number(ui, "前伸距离 (m)", RESET, &mut p.move_reach, 0.1, 6.0, 0.05);
            a.reset |= number(
                ui,
                "中心前移 (m)",
                RESET,
                &mut p.move_forward_offset,
                0.0,
                2.0,
                0.05,
            );
        }
        a.reset |= number(
            ui,
            "起跳冲击强度",
            RESET,
            &mut p.jump_strength,
            0.0,
            3.0,
            0.05,
        );
        a.reset |= number(
            ui,
            "起跳半径 (m)",
            RESET,
            &mut p.jump_radius,
            0.25,
            6.0,
            0.05,
        );
        a.reset |= number(
            ui,
            "落地冲击强度",
            RESET,
            &mut p.landing_strength,
            0.0,
            4.0,
            0.05,
        );
        a.reset |= number(
            ui,
            "落地半径 (m)",
            RESET,
            &mut p.landing_radius,
            0.25,
            6.0,
            0.05,
        );
        a.reset |= number(
            ui,
            "作用中心高度 (m)",
            RESET,
            &mut p.center_height,
            -2.0,
            2.0,
            0.01,
        );
        a.reset |= number(
            ui,
            "竖直作用高度 (m)",
            RESET,
            &mut p.vertical_extent,
            0.05,
            4.0,
            0.05,
        );
        a.reset |= number(
            ui,
            "起跳速度 (m/s)",
            RESET,
            &mut s.player.jump_speed,
            1.0,
            10.0,
            0.1,
        );
    });
}

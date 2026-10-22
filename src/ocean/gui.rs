//! egui 参数面板与 GPU 合成；不参与流体计算。
use super::{
    color_editor::CoefficientEditor,
    configuration_panel::ConfigurationPanel,
    foam::FoamConfig,
    gpu::Gpu,
    optics::{OpticsConfig, VIEW_NAMES},
    settings::{ConfigurationLibrary, SavedSettings},
    spectrum::OceanConfig,
    viewer::Camera,
};
use winit::window::Window;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Panel {
    View,
    Ocean,
    Configurations,
    Foam,
    Shore,
    Optics,
}

fn default_dock() -> egui_dock::DockState<Panel> {
    use egui_dock::{DockState, NodeIndex};
    let mut dock = DockState::new(vec![Panel::View]);
    let [center, left] = dock.main_surface_mut().split_left(
        NodeIndex::root(),
        0.23,
        vec![Panel::Ocean, Panel::Configurations],
    );
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
            Panel::View => "渲染视图",
            Panel::Ocean => "海洋参数",
            Panel::Configurations => "配置保存",
            Panel::Foam => "远海泡沫",
            Panel::Shore => "浅水模拟（SWE）",
            Panel::Optics => "水体光学",
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
        "波谱强度 A" => "Phillips 波谱强度；波高与 A 的平方根成正比。点击应用后重建波谱。",
        "波谱尺度 L（米）" => {
            "Phillips 长度尺度，等于风速平方除以重力加速度。点击应用后生效。"
        }
        "风向（度）" => "风向单位向量的方向，不是波速。点击应用后重建波谱。",
        "区块边长（米）" => "FFT 周期区块的边长；海面由 3×3 个区块构成。点击应用后生效。",
        "FFT 分辨率" => "每轴 FFT 采样数；采样间距等于区块边长除以分辨率。点击应用后生效。",
        "每轴网格数" => {
            "每个区块每轴的几何网格数，顶点数为网格数加一。网格太粗会丢失短波形状。点击应用后生效。"
        }
        "暂停" => "冻结模拟、泡沫和纹理流动，仍可调整渲染参数。",
        "时间倍率" => "模拟时间的速度倍率，0 表示停止演化。",
        "波高增益" => "同时缩放 FFT 位移与 SWE 输入；修改后会重新初始化浅水和泡沫历史。",
        "水平挤浪" | "挤浪强度" => {
            "FFT 水平位移让浪峰更尖，并改变用于生成远海泡沫的雅可比值。"
        }
        "显示泡沫" => "只切换泡沫显示，不停止模拟。",
        "雅可比阈值" => "水面形变的雅可比值低于此阈值时生成远海泡沫。实时生效。",
        "生成速率（每秒）" => "每模拟秒的远海泡沫生成速率，实时生效。",
        "衰减系数（每秒）" => "远海泡沫的指数衰减系数；越小存活越久，0 表示不自然衰减。",
        "泡沫视图" => "选择正常水面、形变雅可比值或远海泡沫覆盖率。",
        "倾斜沙滩" => "启用倾斜沙滩：负 X 为陆地，正 X 为深水。修改地形会重置浅水模拟。",
        "启用浅水模拟" => "启用局部浅水模拟及泡沫、沙地历史。切换后重置 SWE。",
        "SWE 求解器" => {
            "一阶使用欧拉积分，二阶使用 MUSCL 与 RK2。切换会重置模拟；比较几何时可将法线细节设为 0。"
        }
        "水上泡沫" => "水上泡沫的线性 RGB 颜色，包含远海泡沫。只改变显示。",
        "沙地残留泡沫" => "暴露沙地上的残留泡沫颜色，采用线性 RGB，只改变显示。",
        "法线细节强度" => {
            "来自 FFT 的短波法线细节；0 为关闭，1 为默认。薄水区域逐渐减弱，不重置模拟。开启反射更容易观察。"
        }
        "FFT → SWE 增益" => {
            "同时缩放输入水位和动量：0 为无输入，1 为匹配，3 为三倍。修改后用当前 FFT 初始化整片浅水并清空历史。"
        }
        "过滤短波" => {
            "对输入 SWE 的 FFT 水位与动量使用相同短波过滤，远海细节不变。修改后重新初始化浅水。"
        }
        "截止波长（米）" => "不大于此波长的波被过滤；从截止波长加过渡宽度开始完整传入。",
        "过滤过渡宽度（米）" => {
            "截止波长以上的平滑过滤范围，0 表示硬切换。修改后重新初始化浅水。"
        }
        "FFT 混合水深（米）" => {
            "在此水深到其 2.5 倍之间，从 SWE 过渡到 FFT。修改后重置浅水。"
        }
        "背景生成速率（每秒）" => {
            "原有上涨、流速和汇聚判据的泡沫生成速率，还会乘以背景生成倍率。设为 0 仅关闭背景生成，前沿生成独立控制。"
        }
        "背景生成起始阈值" => {
            "水深上涨速度除以浅水波速超过此值时开始生成背景泡沫。无量纲，越低越容易生成；默认 0.25。实时生效，不重置。"
        }
        "背景生成过渡范围" => {
            "从起始阈值到完整生成权重的过渡宽度。默认 0.4，即 0.25 开始、0.65 达到完整权重。0 为硬阈值，不影响前沿生成。"
        }
        "水上泡沫衰减（每秒）" => {
            "水上泡沫的指数衰减系数；越小存活越久，0 为无自然衰减。不控制沙地残留寿命。"
        }
        "汇聚增强阈值（每秒）" => {
            "增强背景泡沫生成的水流汇聚阈值。已有泡沫仍会通过输送自然堆积，不受此阈值限制。"
        }
        "密度颜色参考值" => {
            "达到此密度显示红色，低密度绿色，中间黄色。仅改变色标，不限制模拟密度；水上和沙地共用。"
        }
        "泡沫沉积水深（毫米）" => {
            "水深不超过此值时，水上泡沫沉积到沙地；超过时冲淡沙地残留。越大越早沉积，0 要求水深为零。实时生效，不改变水面几何或沙地湿度。"
        }
        "沙地干燥时间（秒）" => "湿沙的干燥时间常数，不控制泡沫寿命。",
        "浅水调试视图" => {
            "调试视图跳过普通着色。流向：蓝色冲岸、橙色退水。密度：低绿高红，不混合孔隙纹理。"
        }
        "沙滩坡度（高／宽）" | "岸线 X（米）" => {
            "负 X 为陆地，正 X 为深水。修改后重新初始化浅水模拟。"
        }
        "海底深度（米）" => "水平海底的深度。倾斜沙滩模式下禁用，水深由地形决定。",
        "启用天空散射" => {
            "基于平均海平面与直线光路的各向同性天空散射。手动光源解析积分，天空光源分段解析积分。"
        }
        "天空颜色（线性 RGB）" => "天空照明的线性 RGB 颜色，同时影响背景和反射。",
        "天空强度" => "天空照明的辐亮度倍率。",
        "散射反照率 RGB" => {
            "散射系数 = 反照率 × 消光系数；吸收系数 =（1 − 反照率）× 消光系数。"
        }
        "天空积分步数" => "天空散射的积分分段数，越多越准确，GPU 开销也越高。",
        "水体调试视图" => {
            "光学调试跳过反射和泡沫。透射率按消光系数和光程指数衰减；吸收视图显示衰减后的背景，损失含散射出射。"
        }
        "光程白色参考值（米）" => "光程调试中显示为白色的距离。",
        "天空反射与高光" => "启用水面的天空反射与太阳高光。",
        "海底 RGB 测试色带" | "棋盘格尺寸（米）" => {
            "用于比较不同颜色吸收与光程长度的海底标定图案。"
        }
        _ => "实时参数，修改后立即生效。",
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
    pub configuration_panel: ConfigurationPanel,
    pub load_configuration: Option<SavedSettings>,
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
            swe_timing: "SWE GPU：等待采样".into(),
            error: None,
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
                    ui.label("海洋模拟").on_hover_text("拖动标签停靠，拖动分隔线调整大小。在渲染视图内拖动旋转相机，滚轮缩放。");
                    if ui.button("重置布局").clicked() { dock = default_dock(); }

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
Panel::Configurations => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let color = if self.save_failed {
                                    egui::Color32::LIGHT_RED
                                } else {
                                    ui.visuals().text_color()
                                };
                                ui.colored_label(color, &self.save_status).on_hover_text("停止调整 0.5 秒后自动保存，退出时也会保存。");
                                if ui
                                    .add_enabled(
                                        self.settings_path.is_some(),
                                        egui::Button::new("立即保存"),
                                    )
                                    .clicked()
                                {
                                    self.save_now = true;
                                }
                            });
                            if let Some(path) = &self.settings_path {

                                ui.collapsing("本地配置文件", |ui| {
                                    ui.label(path.display().to_string());
                                });
                            }
                            ui.separator();
                            let settings = SavedSettings {
                                ocean: current,
                                draft: self.draft,
                                foam: self.foam_settings,
                                optics: self.optics,
                                camera: *camera,
                                speed: self.speed,
                                paused: *paused,
                                ..Default::default()
                            };
                            if let Some(settings) = self.configuration_panel.ui(ui, settings, self.settings_path.is_some()) {
                                self.load_configuration = Some(settings);
                            }
                            self.save_now |= std::mem::take(&mut self.configuration_panel.save_now);
                        }); },
Panel::Ocean => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {
                            ui.label(egui::RichText::new("Phillips 波谱").strong());

                            parameter_table(ui, "spectrum table", |ui| {
parameter(ui, "波谱强度 A", help("波谱强度 A"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.amplitude, 0.0..=0.1)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    .smallest_positive(0.000001)
                                    ,
                            ));
                            parameter(ui, "波谱尺度 L（米）", help("波谱尺度 L（米）"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.phillips_length, 0.1..=500.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));
                            let mut angle = self.draft.wind_angle.to_degrees();
                            if parameter(ui, "风向（度）", help("风向（度）"), |ui|
                                ui.add(egui::Slider::new(&mut angle, -180.0..=180.0))).changed()
                            {
                                self.draft.wind_angle = angle.to_radians();
                            }




});
                            ui.separator();
                            ui.label(egui::RichText::new("范围与采样").strong());
                            parameter_table(ui, "domain table", |ui| {
parameter(ui, "区块边长（米）", help("区块边长（米）"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.length, 8.0..=1024.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));
                            parameter(ui, "FFT 分辨率", help("FFT 分辨率"), |ui| egui::ComboBox::from_id_salt("FFT 分辨率")
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
                            parameter(ui, "每轴网格数", help("每轴网格数"), |ui| ui.add(
                                egui::Slider::new(&mut self.draft.mesh_resolution, 8..=512)
                                    ,
                            ));
                            parameter(ui, "随机种子", "波谱随机种子，点击应用后重建海面。", |ui| ui.horizontal(|ui| {
                                ui.add(egui::DragValue::new(&mut self.draft.seed));
                                if ui.button("更换").clicked() {
                                    self.draft.seed = self.draft.seed.wrapping_add(1);
                                }
                            }));

});
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        self.draft != current,
                                        egui::Button::new("应用更改"),
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
                                if ui.button("恢复默认").clicked() {
                                    self.draft = OceanConfig::default();
                                }
                            });
                            parameter_table(ui, "status table", |ui| {
parameter(ui, "参数状态", "波谱参数需要点击应用后生效。", |ui| ui.label(if self.draft == current {
                                "已应用"
                            } else {
                                "有更改，需点击应用"
                            }));

});
                            if let Some(error) = &self.error {
                                ui.colored_label(egui::Color32::LIGHT_RED, error);
                            }
                            ui.separator();
                            ui.label(egui::RichText::new("播放与外观").strong());
parameter_table(ui, "playback table", |ui| {
                            {
                                parameter(ui, "暂停", help("暂停"), |ui| ui.checkbox(paused, ""));
                                parameter(ui, "模拟时间", "已经经过的模拟时间。", |ui| ui.label(format!("{time:.2} s")));
                            }
                            parameter(ui, "时间倍率", help("时间倍率"), |ui| ui.add(
                                egui::Slider::new(&mut self.speed, 0.0..=3.0),
                            ));
                            parameter(ui, "波高增益", help("波高增益"), |ui| ui.add(
                                egui::Slider::new(&mut camera.gain, 0.0..=3.0),
                            ));
                            parameter(ui, "水平挤浪", help("水平挤浪"), |ui| ui.checkbox(&mut camera.choppy, ""));
                            parameter(ui, "挤浪强度", help("挤浪强度"), |ui| ui.add(
                                egui::Slider::new(&mut camera.chop_amount, 0.0..=3.0)
                                    ,
                            ));

});
                                if ui.button("重新计时").clicked() {
                                    *time = 0.0;
                                    self.clear_foam = true;
                                }
                            if ui.button("重置相机").clicked() {
                                *camera = Camera::new(current.length);
                            }


}); },
Panel::Foam => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {

                            parameter_table(ui, "offshore foam table", |ui| {
parameter(ui, "显示泡沫", help("显示泡沫"), |ui| ui.checkbox(&mut camera.foam, ""));
                            parameter(ui, "雅可比阈值", help("雅可比阈值"), |ui| ui.add(
                                egui::Slider::new(&mut self.foam_settings.threshold, -0.5..=1.0)
                                    ,
                            ));
                            parameter(ui, "生成速率（每秒）", help("生成速率（每秒）"), |ui| ui.add(
                                egui::Slider::new(&mut self.foam_settings.generation, 0.0..=20.0)
                                    ,
                            ));
                            parameter(ui, "衰减系数（每秒）", help("衰减系数（每秒）"), |ui| ui.add(
                                egui::Slider::new(&mut self.foam_settings.decay, 0.0..=5.0)
                                    ,
                            ));
                            parameter(ui, "泡沫视图", help("泡沫视图"), |ui| egui::ComboBox::from_id_salt("泡沫视图")
                                .width(ui.available_width()).selected_text(match camera.foam_debug {
                                    1 => "雅可比值",
                                    2 => "覆盖率",
                                    _ => "正常着色",
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut camera.foam_debug, 0, "正常着色");
                                    ui.selectable_value(&mut camera.foam_debug, 1, "雅可比值");
                                    ui.selectable_value(&mut camera.foam_debug, 2, "覆盖率");
                                }));



});
                            if ui.button("清空泡沫历史").clicked() {
                                self.clear_foam = true;
                            }
}); },
Panel::Shore => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {

                                if ui.button("沙滩场景").clicked() {
                                    self.optics = OpticsConfig::beach_scene();
                                    *camera = Camera::beach(current.length);
                                }
                            let shore = &mut self.optics.shore;
                            parameter_table(ui, "solver table", |ui| {
parameter(ui, "倾斜沙滩", help("倾斜沙滩"), |ui| ui.checkbox(&mut self.optics.beach, ""));
                            parameter(ui, "启用浅水模拟", help("启用浅水模拟"), |ui| ui.checkbox(&mut shore.enabled, ""));
                            parameter(ui, "SWE 求解器", help("SWE 求解器"), |ui| egui::ComboBox::from_id_salt("SWE 求解器")
                                .width(ui.available_width()).selected_text(if shore.order == 2 {"二阶（MUSCL + RK2）"} else {"一阶（欧拉）"})
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut shore.order, 1, "一阶（欧拉）");
                                    ui.selectable_value(&mut shore.order, 2, "二阶（MUSCL + RK2）");
                                }));
                            parameter(ui, "GPU 耗时", "包含 SWE、泡沫和子步数据复制，不包含 FFT 与渲染。", |ui| ui.label(&self.swe_timing));




});
                            ui.separator();
                            ui.label(egui::RichText::new("泡沫颜色").strong());
                            parameter_table(ui, "foam colors table", |ui| {
parameter(ui, "水上泡沫", help("水上泡沫"), |ui| ui.color_edit_button_rgb(&mut shore.water_foam_color));
                            parameter(ui, "沙地残留泡沫", help("沙地残留泡沫"), |ui| ui.color_edit_button_rgb(&mut shore.stranded_foam_color));

});
                            ui.horizontal(|ui| {
                                if ui.button("默认区分色").clicked() {
                                    shore.water_foam_color=[0.87,0.92,0.90];
                                    shore.stranded_foam_color=[0.36,0.50,0.58];
                                }
                                if ui.button("恢复原配色").clicked() {
                                    shore.water_foam_color=[0.87,0.92,0.90];
                                    shore.stranded_foam_color=[0.82;3];
                                }
                            });
                            if ui.button("区分位置：水绿／沙红").clicked() {
                                shore.water_foam_color=[0.0,1.0,0.0];
                                shore.stranded_foam_color=[1.0,0.0,0.0];
                            }


                            ui.separator();
                            ui.label(egui::RichText::new("小波法线").strong());
                            parameter_table(ui, "normal table", |ui| {
parameter(ui, "法线细节强度", help("法线细节强度"), |ui| ui.add(egui::Slider::new(&mut shore.normal_strength, 0.0..=3.0)));



});
                            ui.separator();
                            ui.label(egui::RichText::new("FFT 输入").strong());
                            parameter_table(ui, "FFT input table", |ui| {
parameter(ui, "FFT → SWE 增益", help("FFT → SWE 增益"), |ui| ui.add(egui::Slider::new(&mut shore.input_gain, 0.0..=3.0)));



                            parameter(ui, "过滤短波", help("过滤短波"), |ui| ui.checkbox(&mut shore.filter_enabled, ""));

                                parameter(ui, "截止波长（米）", help("截止波长（米）"), |ui| ui.add_enabled(shore.filter_enabled,egui::Slider::new(&mut shore.filter_cutoff, 0.0..=128.0)
                                    .clamping(egui::SliderClamping::Edits)));
                                parameter(ui, "过滤过渡宽度（米）", help("过滤过渡宽度（米）"), |ui| ui.add_enabled(shore.filter_enabled,egui::Slider::new(&mut shore.filter_transition, 0.0..=128.0)
                                    .clamping(egui::SliderClamping::Edits)));


});
                            ui.horizontal(|ui| {
                                if ui.button("默认 12～24 米").clicked() { shore.filter_enabled=true; shore.filter_cutoff=12.0; shore.filter_transition=12.0; }
                                if ui.button("3～6 米").clicked() { shore.filter_enabled=true; shore.filter_cutoff=3.0; shore.filter_transition=3.0; }
                            });


                            ui.separator();

                            ui.label(egui::RichText::new("前沿泡沫").strong());
                            parameter_table(ui, "front foam table", |ui| {
                            parameter(ui, "前沿生成速率（每秒）", "沿流向发现前方水面更低时的独立泡沫生成速率。0 关闭前沿生成；不要求前方接近干地，也不使用背景上涨阈值。", |ui| ui.add(egui::Slider::new(&mut shore.foam_front_generation, 0.0..=30.0)));
                            parameter(ui, "前沿采样距离（米）", "沿当前水流方向，在此距离内均匀采样 4 个位置。越远通常覆盖越宽，默认 1 米。按世界距离采样，兼容非正方形网格。", |ui| ui.add(egui::Slider::new(&mut shore.foam_front_distance, 0.05..=10.0).max_decimals(2)));
                            parameter(ui, "前沿满强度水面落差（米）", "比较实际水面高度（地形高度＋水深）。当前水面减去前方最低水面，从 0 到此值平滑增强；达到或超过此值时权重保持为 1。干燥采样点跳过，不把沙坡高度当成水面。越小越敏感，默认 0.03 米。", |ui| ui.add(egui::Slider::new(&mut shore.foam_front_depth_drop, 0.001..=1.0).logarithmic(true).max_decimals(3)));
                            parameter(ui, "背景生成倍率", "缩放原有泡沫生成量，默认 0.15，使后方泡沫较弱。0 关闭背景生成。前沿速率设为 0、此倍率设为 1，可恢复原生成算法。", |ui| ui.add(egui::Slider::new(&mut shore.foam_background_multiplier, 0.0..=1.0).max_decimals(2)));
                            });
                            ui.separator();

                            ui.label(egui::RichText::new("泡沫随流运动").strong());
                            parameter_table(ui, "foam motion table", |ui| {
                                parameter(ui, "水流跟随倍率", "泡沫目标速度 = 水流速度 × 此倍率。默认 0.8；1 为等速跟随，0 时泡沫速度逐渐降到零。密度和孔隙纹理共用泡沫速度，水流本身不受影响。实时生效并保存，不清空历史。", |ui| ui.add(egui::Slider::new(&mut shore.foam_follow, 0.0..=1.0).max_decimals(2)));
                                parameter(ui, "速度响应时间（秒）", "泡沫速度追随目标速度的时间常数，默认 0.35 秒。越大越迟缓，转向后仍会短暂沿原方向漂移；0 为立即跟随。约经过此时间完成 63% 的速度变化。沙地残留不移动。", |ui| ui.add(egui::Slider::new(&mut shore.foam_response_time, 0.0..=5.0).max_decimals(2)));
                            });
                            ui.separator();

                            parameter_table(ui, "shore foam table", |ui| {
parameter(ui, "FFT 混合水深（米）", help("FFT 混合水深（米）"), |ui| ui.add(
                                egui::Slider::new(&mut shore.breaking_depth, 0.5..=5.0)
                                    ,
                            ));
                            parameter(ui, "背景生成速率（每秒）", help("背景生成速率（每秒）"), |ui| ui.add(
                                egui::Slider::new(&mut shore.generation, 0.0..=15.0)
                                    ,
                            ));
                            parameter(ui, "背景生成起始阈值", help("背景生成起始阈值"), |ui| ui.add(
                                egui::Slider::new(&mut shore.foam_generation_threshold, 0.0..=2.0).max_decimals(3),
                            ));
                            parameter(ui, "背景生成过渡范围", help("背景生成过渡范围"), |ui| ui.add(
                                egui::Slider::new(&mut shore.foam_generation_transition, 0.0..=2.0).max_decimals(3),
                            ));
                            parameter(ui, "水上泡沫衰减（每秒）", help("水上泡沫衰减（每秒）"), |ui| ui.add(
                                egui::Slider::new(&mut shore.decay, 0.0..=4.0)
                                    ,
                            ));
                            parameter(ui, "汇聚增强阈值（每秒）", help("汇聚增强阈值（每秒）"), |ui| ui.add(
                                egui::Slider::new(&mut shore.foam_compression_threshold, 0.0..=4.0)
                                    ,
                            ));
                            parameter(ui, "密度颜色参考值", help("密度颜色参考值"), |ui| ui.add(
                                egui::Slider::new(&mut shore.foam_density_reference, 0.01..=20.0)
                                    .logarithmic(true),
                            ));



                            parameter(ui, "泡沫沉积水深（毫米）", help("泡沫沉积水深（毫米）"), |ui| ui.add(egui::Slider::new(&mut shore.foam_deposit_depth_mm, 0.0..=100.0).max_decimals(2)));
                            parameter(ui, "沙地干燥时间（秒）", help("沙地干燥时间（秒）"), |ui| ui.add(
                                egui::Slider::new(&mut shore.drying_time, 1.0..=120.0)
                                    ,
                            ));
                            parameter(ui, "浅水调试视图", help("浅水调试视图"), |ui| egui::ComboBox::from_id_salt("浅水调试视图")
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
                            if ui.button("密度调试：低绿／高红").clicked() {
                                shore.view = 6;
                            }
                            if ui.button("重置浅水、泡沫和沙地").clicked() {
                                self.clear_foam = true;
                            }

                            if self.optics.beach {
                                parameter_table(ui, "beach table", |ui| {
parameter(ui, "沙滩坡度（高／宽）", help("沙滩坡度（高／宽）"), |ui| ui.add(
                                    egui::Slider::new(&mut self.optics.beach_slope, 0.01..=0.5)
                                        .clamping(egui::SliderClamping::Edits)
                                        ,
                                ));
                                parameter(ui, "岸线 X（米）", help("岸线 X（米）"), |ui| ui.add(
                                    egui::Slider::new(
                                        &mut self.optics.shore_x,
                                        -current.length..=current.length,
                                    )
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                                ));


});
                                if ui.button("沙滩相机").clicked() {
                                    *camera = Camera::beach(current.length);
                                }
                            }
}); },
Panel::Optics => { egui::ScrollArea::vertical().id_salt(*tab_id).auto_shrink([false, false]).show(ui, |ui| {

                            parameter_table(ui, "seabed table", |ui| {
parameter(ui, "海底深度（米）", help("海底深度（米）"), |ui| ui.add_enabled(
                                !self.optics.beach,
                                egui::Slider::new(&mut self.optics.depth, 0.25..=100.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));

});
                            self.sigma_editor.ui(
                                ui,
                                "消光系数（每米）",
                                &mut self.optics.sigma_t,
                            );
                            self.source_editor.ui(
                                ui,
                                "手动散射源（辐亮度／米）",
                                &mut self.optics.source,
                            );
                            ui.separator();
                            ui.label(egui::RichText::new("天空散射").strong());
                            parameter_table(ui, "sky table", |ui| {
parameter(ui, "启用天空散射", help("启用天空散射"), |ui| ui.checkbox(&mut self.optics.sky.enabled, ""));
                            parameter(ui, "天空颜色（线性 RGB）", help("天空颜色（线性 RGB）"), |ui| {
                                let mut color = self.optics.sky.color;
                                if ui.color_edit_button_rgb(&mut color).changed() {
                                    self.optics.sky.color = color;
                                }
                            });
                            parameter(ui, "天空强度", help("天空强度"), |ui| ui.add(
                                egui::Slider::new(&mut self.optics.sky.intensity, 0.0..=20.0)
                                    .logarithmic(true)
                                    ,
                            ));
                            parameter(ui, "散射反照率 RGB", help("散射反照率 RGB"), |ui| {
                                let mut albedo = self.optics.sky.albedo;
                                if ui.color_edit_button_rgb(&mut albedo).changed() {
                                    self.optics.sky.albedo = albedo;
                                }
                            });
                            parameter(ui, "天空积分步数", help("天空积分步数"), |ui| egui::ComboBox::from_id_salt("天空积分步数")
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
                            ui.collapsing("天空 RGB 精确值", |ui| {
parameter_table(ui, "sky channels table", |ui| {
                                for c in 0..3 {
                                    parameter(ui, &format!("颜色 {}", ["R", "G", "B"][c]), "天空颜色的线性 RGB 分量。", |ui|
                                        ui.add(egui::DragValue::new(&mut self.optics.sky.color[c]).range(0.0..=1.0).speed(0.001)));
                                    parameter(ui, &format!("反照率 {}", ["R", "G", "B"][c]), "散射系数等于反照率乘以消光系数。", |ui|
                                        ui.add(egui::DragValue::new(&mut self.optics.sky.albedo[c]).range(0.0..=1.0).speed(0.001)));
                                }

});
                            });
                            ui.collapsing("天空散射源随深度变化", |ui| {
                                super::skylight::depth_graph(ui, &self.optics);
                            });


                            ui.separator();
                            parameter_table(ui, "water optics table", |ui| {
parameter(ui, "水体调试视图", help("水体调试视图"), |ui| egui::ComboBox::from_id_salt("水体调试视图")
                                .width(ui.available_width()).selected_text(VIEW_NAMES[self.optics.view as usize])
                                .show_ui(ui, |ui| {
                                    for (i, name) in VIEW_NAMES.iter().enumerate() {
                                        ui.selectable_value(&mut self.optics.view, i as u32, *name);
                                    }
                                }));
                            parameter(ui, "光程白色参考值（米）", help("光程白色参考值（米）"), |ui| ui.add(
                                egui::Slider::new(&mut self.optics.path_range, 1.0..=300.0)
                                    .logarithmic(true)
                                    .clamping(egui::SliderClamping::Edits)
                                    ,
                            ));
                            parameter(ui, "天空反射与高光", help("天空反射与高光"), |ui| ui.checkbox(
                                &mut self.optics.reflections,
                                "",
                            ));
                            parameter(ui, "海底 RGB 测试色带", help("海底 RGB 测试色带"), |ui| ui.checkbox(&mut self.optics.color_bands, ""));
                            parameter(ui, "棋盘格尺寸（米）", help("棋盘格尺寸（米）"), |ui| ui.add(
                                egui::Slider::new(&mut self.optics.checker_size, 0.5..=20.0)
                                    ,
                            ));





});
                            if ui.button("重置光学参数").clicked() {
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

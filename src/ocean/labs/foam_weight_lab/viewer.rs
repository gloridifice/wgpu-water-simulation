use super::*;
use charming_egui::{parameter, parameter_table};
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

#[derive(Clone, Copy)]
enum Panel {
    Controls,
    Maps,
    Statistics,
}
struct Panels<F>(F);
impl<F: FnMut(Panel, &mut egui::Ui)> egui_dock::TabViewer for Panels<F> {
    type Tab = Panel;
    fn title(&mut self, tab: &mut Panel) -> egui::WidgetText {
        match tab {
            Panel::Controls => "试验参数",
            Panel::Maps => "FFT → J → 泡沫生成权重",
            Panel::Statistics => "数值与说明",
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
struct Pending {
    buffer: wgpu::Buffer,
    receiver: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    time: f32,
    params: Params,
}
struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    experiment: Experiment,
    controls: Controls,
    draft: OceanConfig,
    apply: bool,
    context: egui::Context,
    input: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    images: [egui::TextureId; 4],
    dock: egui_dock::DockState<Panel>,
    dock_style: egui_dock::Style,
    time: f32,
    paused: bool,
    speed: f32,
    last: Instant,
    frames: u32,
    pending: Option<Pending>,
    stats: Option<Statistics>,
    last_read: Instant,
    error: Option<String>,
    focus: Option<usize>,
}
fn views(maps: &Maps) -> [wgpu::TextureView; 4] {
    std::array::from_fn(|i| {
        maps.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: i as u32,
            array_layer_count: Some(1),
            ..Default::default()
        })
    })
}
impl State {
    async fn new(window: Arc<Window>, time: f32, paused: bool) -> Result<Self, Error> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let gpu = Gpu::new(&instance, Some(&surface)).await?;
        eprintln!(
            "GPU: {} | isolated J weight lab; no settings, no SWE",
            gpu.name
        );
        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("no window surface configuration")?;
        surface_config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &surface_config);
        let controls = Controls::default();
        let draft = OceanConfig {
            resolution: 256,
            ..Default::default()
        };
        let experiment = Experiment::new(&gpu, draft, controls.grid)?;
        let context = egui::Context::default();
        let theme = charming_egui::Theme::default();
        theme.apply(&context);
        crate::ocean::ui::fonts::install(&context);
        let dock_style = theme.dock_style().clone();
        let input = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let mut renderer =
            egui_wgpu::Renderer::new(&gpu.device, surface_config.format, Default::default());
        let images = views(&experiment.maps).map(|view| {
            renderer.register_native_texture(&gpu.device, &view, wgpu::FilterMode::Nearest)
        });
        let mut dock = egui_dock::DockState::new(vec![Panel::Maps]);
        let [center, _] = dock.main_surface_mut().split_left(
            egui_dock::NodeIndex::root(),
            0.24,
            vec![Panel::Controls],
        );
        dock.main_surface_mut()
            .split_right(center, 0.74, vec![Panel::Statistics]);
        Ok(Self {
            window,
            surface,
            surface_config,
            gpu,
            experiment,
            controls,
            draft,
            apply: false,
            context,
            input,
            renderer,
            images,
            dock,
            dock_style,
            time,
            paused,
            speed: 1.0,
            last: Instant::now(),
            frames: 0,
            pending: None,
            stats: None,
            last_read: Instant::now() - Duration::from_secs(1),
            error: None,
            focus: None,
        })
    }
    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface
            .configure(&self.gpu.device, &self.surface_config);
    }
    fn controls_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("lab controls").show(ui,|ui| {
            charming_egui::h3(ui, "播放");
            ui.horizontal_wrapped(|ui| {
                if ui.button(if self.paused {"播放"} else {"暂停"}).clicked(){self.paused = !self.paused;}
                if ui.add_enabled(self.paused,egui::Button::new("步进 1/60 秒")).clicked(){self.time+=1.0/60.0;}
                if ui.button("回到 0 秒").clicked(){self.time=0.0;self.paused=true;}
            });
            parameter_table(ui,"clock",|ui| {
                parameter(ui,"时间（秒）","模拟时间，0～3600 秒。修改立即暂停并查看该时刻；没有泡沫历史可清空。",|ui| {
                    if ui.add(egui::DragValue::new(&mut self.time).range(0.0..=3600.0).speed(0.02).max_decimals(3)).changed(){self.paused=true;}
                });
                parameter(ui,"时间倍率","0～3，无量纲。只改变播放速度，不改变同一模拟时刻的 J。实时生效。",|ui|ui.add(egui::Slider::new(&mut self.speed,0.0..=3.0)));
            });
            ui.separator();charming_egui::h3(ui, "FFT 波谱（应用后生效）");
            parameter_table(ui,"spectrum",|ui| {
                parameter(ui,"FFT 分辨率","64～512，每轴采样数。应用后重建现有 Ocean FFT，不新增另一套波谱；保留当前时间。",|ui| {
                    egui::ComboBox::from_id_salt("fft n").width(ui.available_width()).selected_text(self.draft.resolution.to_string()).show_ui(ui,|ui| {for n in [64,128,256,512] {ui.selectable_value(&mut self.draft.resolution,n,n.to_string());}});
                });
                parameter(ui,"周期边长（米）","16～256 米，世界空间周期。应用后重建 FFT，并按范围倍率调整试验范围。",|ui|ui.add(egui::Slider::new(&mut self.draft.length,16.0..=256.0)));
                parameter(ui,"波谱幅值","Phillips 波谱系数，0～0.03，非波高米数；0 得到平水。应用后重建 FFT。",|ui|ui.add(egui::Slider::new(&mut self.draft.amplitude,0.0..=0.03).max_decimals(5)));
                parameter(ui,"主波尺度（米）","Phillips 特征长度，1～100 米；不是最小波长。应用后重建 FFT。",|ui|ui.add(egui::Slider::new(&mut self.draft.phillips_length,1.0..=100.0)));
                let mut angle=self.draft.wind_angle.to_degrees();
                if parameter(ui,"风向（度）","−180～180 度。改变波谱方向；应用后重建 FFT。",|ui|ui.add(egui::Slider::new(&mut angle,-180.0..=180.0))).changed(){self.draft.wind_angle=angle.to_radians();}
                parameter(ui,"随机种子","无符号整数；固定种子用于可重复比较。应用后重建 FFT，不使用个人配置。",|ui|ui.add(egui::DragValue::new(&mut self.draft.seed)));
            });
            if ui.button("应用 FFT 参数").clicked(){self.apply=true;}
            ui.separator();charming_egui::h3(ui, "J 与目标采样网格");
            parameter_table(ui,"sampling",|ui| {
                parameter(ui,"横向位移倍率","0～3，无量纲。实际位移系数为负值，与海洋 choppiness 约定一致；高度增益固定为 1。0 时 J=1。实时生效，不重建 FFT。",|ui|ui.add(egui::Slider::new(&mut self.controls.chop,0.0..=3.0)));
                parameter(ui,"目标网格","64～512，每轴格数。只改变权重图采样，不改变 FFT；不运行 SWE。格内面积平均，不能凭空增加细节。实时重建诊断图。",|ui| {
                    egui::ComboBox::from_id_salt("target n").width(ui.available_width()).selected_text(self.controls.grid.to_string()).show_ui(ui,|ui| {for n in [64,128,256,512] {ui.selectable_value(&mut self.controls.grid,n,n.to_string());}});
                });
                for (axis,name) in ["范围 X／周期","范围 Z／周期"].into_iter().enumerate() {
                    parameter(ui,name,"0.25～3 倍 FFT 周期，世界范围居中。越大则每格越粗；跨周期重复采样。实时生效。",|ui|ui.add(egui::Slider::new(&mut self.controls.extent[axis],0.25..=3.0).max_decimals(2)));
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("单周期方形").clicked(){self.controls.extent=[1.0,1.0];}
                if ui.button("沿岸窄条 0.5 × 3").clicked(){self.controls.extent=[0.5,3.0];}
            });
            ui.separator();charming_egui::h3(ui, "生成权重（实时）");
            parameter_table(ui,"weights",|ui| {
                parameter(ui,"压缩起始阈值","0～1，无量纲。C=max(0,1−J)；超过阈值才有掩码。只改变示意权重，不改水体或现有泡沫。",|ui|ui.add(egui::Slider::new(&mut self.controls.threshold,0.0..=1.0).max_decimals(3)));
                parameter(ui,"压缩过渡宽度","0～1，无量纲。掩码在起始阈值至阈值＋宽度间平滑变为 1。0 为严格大于阈值的硬门限；平水不会误触发。",|ui|ui.add(egui::Slider::new(&mut self.controls.transition,0.0..=1.0).max_decimals(3)));
                parameter(ui,"调制强度","0～1，无量纲。W=(1−强度)+强度×增益×M。0 严格恢复 W=1；1 完全使用 J 掩码。实时生效，不自动归一。",|ui|ui.add(egui::Slider::new(&mut self.controls.strength,0.0..=1.0).max_decimals(2)));
                parameter(ui,"掩码增益","0～8，无量纲。改变峰值及平均权重；不逐帧拉伸对比度。用右侧平均 W 判断总生成量变化。",|ui|ui.add(egui::Slider::new(&mut self.controls.gain,0.0..=8.0).max_decimals(2)));
                parameter(ui,"示意基准生成率","0～10 泡沫量／面积／模拟秒（非实测质量单位）。全域均匀基准，S=基准×W；不是正式 SWE 生成率，也不是累计密度。",|ui|ui.add(egui::Slider::new(&mut self.controls.base_rate,0.0..=10.0).max_decimals(2)));
            });
        });
    }
    fn statistics_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("lab statistics").show(ui,|ui| {
            charming_egui::h3(ui, "独立权重试验 · 不保存配置");
            ui.label("未接入 SWE；不显示累计泡沫密度。");
            let config=self.experiment.ocean.config;
            let p=self.controls.params(config);
            ui.separator();charming_egui::h3(ui, "当前实际采样");
            ui.label(format!("FFT：{}²，{:.2} 米／采样",config.resolution,config.length/config.resolution as f32));
            ui.label(format!("目标：{}²，{:.1} × {:.1} 米",self.controls.grid,p.domain[1],p.domain[2]));
            ui.label(format!("每格：{:.3} × {:.3} 米",p.domain[1]/self.controls.grid as f32,p.domain[2]/self.controls.grid as f32));
            ui.label("范围按世界比例显示；+X 向右，+Z 向上。").on_hover_text("图像使用最近邻显示以暴露网格分辨率；缩小显示仍可能遮住细线，点击单图放大观察。计算使用面积平均，不是点采样。所有图使用未形变的 FFT 世界坐标。");
            ui.separator();charming_egui::h3(ui, "异步统计（约 4 次／秒）");
            if let Some(s)=&self.stats {
                ui.label(format!("统计时刻：{:.3} 模拟秒",s.time));
                if s.params!=p {ui.colored_label(egui::Color32::YELLOW,"参数已变动，等待新统计");}
                for (a,label) in ["面积变化比 J","生成掩码 M","生成权重 W","示意生成率 S"].iter().enumerate() {
                    charming_egui::h3(ui, *label);
                    ui.label(format!("{:.4} ～ {:.4}\n平均 {:.4}",s.min[a],s.max[a],s.mean[a]));
                }
                ui.label(format!("掩码 > 0.5：{:.1}%\n格均 J ≤ 0：{:.2}%",100.0*s.active,100.0*s.folded));
                ui.label(format!("相对基准总生成率：{:.3} 倍",s.mean[2])).on_hover_text("此实验基准在全域均匀，因此平均 W 等于积分生成率倍率（基准为 0 时仅表示权重倍率）。不自动保持总量；减少生成不能直接当作形态改善。");
                if s.max[2]>self.controls.weight_range || s.max[3]>self.controls.rate_range {ui.colored_label(egui::Color32::YELLOW,"部分颜色饱和；数值未截断");}
            } else {ui.label("等待读回…");}
            ui.separator();charming_egui::h3(ui, "固定色标（仅显示）");
            parameter_table(ui,"display scales",|ui| {
                parameter(ui,"J 偏离范围","0.01～2，无量纲。色标为 1±此值，橙色压缩／灰白 J=1／蓝色扩张；不影响计算。实时生效。",|ui|ui.add(egui::Slider::new(&mut self.controls.j_range,0.01..=2.0).logarithmic(true)));
                parameter(ui,"权重色标上限","1.1～8，无量纲。黑=0、青绿=1、黄=上限；超过仅颜色饱和，实际 W 不截断。实时生效。",|ui|ui.add(egui::Slider::new(&mut self.controls.weight_range,1.1..=8.0)));
                parameter(ui,"生成率白值","0.01～80 泡沫量／面积／秒。黑=0、白=此值；仅影响示意源项显示。实时生效。",|ui|ui.add(egui::Slider::new(&mut self.controls.rate_range,0.01..=80.0).logarithmic(true)));
            });
            ui.separator();
            ui.label("C = max(0, 1 − J)\nM = smoothstep(阈值, 阈值＋宽度, C)\nW = (1 − 强度) ＋ 强度 × 增益 × M\nS = 示意基准生成率 × W").on_hover_text("先在 FFT 原始采样上求掩码，再对目标格做面积平均。J 图也是格均值；不是用格均 J 再求掩码。J 是几何压缩代理，不是流速散度或精确破碎率。远海泡沫历史不参与权重。");
            if let Some(error)=&self.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        });
    }
    fn maps_panel(&mut self, ui: &mut egui::Ui) {
        const TITLES: [&str; 4] = [
            "J · 格均面积变化比",
            "M · 格均生成掩码",
            "W · 生成权重",
            "S · 示意加权生成率（非密度）",
        ];
        let legends = [
            format!(
                "橙：{:.2}  |  灰白：1  |  蓝：{:.2}",
                1.0 - self.controls.j_range,
                1.0 + self.controls.j_range
            ),
            "黑：0  |  白：1（只压缩区域）".into(),
            format!(
                "黑：0  |  青绿：1  |  黄：{:.1}",
                self.controls.weight_range
            ),
            format!("黑：0  |  白：{:.2} /s", self.controls.rate_range),
        ];
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.focus.is_none(), "四图对照")
                .clicked()
            {
                self.focus = None;
            }
            for (i, title) in ["J", "掩码", "权重", "源项"].iter().enumerate() {
                if ui.selectable_label(self.focus == Some(i), *title).clicked() {
                    self.focus = Some(i);
                }
            }
        });
        let plot = |ui: &mut egui::Ui, i: usize, size: egui::Vec2| {
            charming_egui::h3(ui, TITLES[i]);
            ui.label(&legends[i]);
            let available = egui::vec2(size.x.max(1.0), (size.y - 56.0).max(1.0));
            let aspect = self.controls.extent[0] / self.controls.extent[1];
            let h = available.y.min(available.x / aspect);
            let image_size = egui::vec2(h * aspect, h);
            ui.add(egui::Image::new((self.images[i], image_size)).sense(egui::Sense::click()))
                .clicked()
        };
        if let Some(i) = self.focus {
            plot(ui, i, ui.available_size());
        } else {
            let size = (ui.available_size() - egui::vec2(12.0, 12.0)) * 0.5;
            let mut clicked = None;
            for row in 0..2 {
                ui.horizontal(|ui| {
                    for col in 0..2 {
                        let i = row * 2 + col;
                        ui.allocate_ui_with_layout(
                            size,
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                if plot(ui, i, size) {
                                    clicked = Some(i);
                                }
                            },
                        );
                    }
                });
            }
            if clicked.is_some() {
                self.focus = clicked;
            }
        }
    }
    fn poll_statistics(&mut self) -> Result<(), Error> {
        self.gpu.device.poll(wgpu::PollType::Poll)?;
        let result = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.receiver.try_recv() {
                Ok(result) => Some(result.map_err(|e| e.to_string())),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(e) => Some(Err(e.to_string())),
            });
        if let Some(result) = result {
            let pending = self.pending.take().unwrap();
            match result {
                Ok(()) => {
                    let bytes = pending.buffer.slice(..).get_mapped_range();
                    self.stats = Some(Statistics::from_bytes(&bytes, pending.time, pending.params));
                    drop(bytes);
                }
                Err(error) => self.error = Some(format!("统计读回失败：{error}")),
            }
            pending.buffer.unmap();
        }
        Ok(())
    }
    fn draw(&mut self) -> Result<(), Error> {
        let now = Instant::now();
        let elapsed = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        if !self.paused {
            self.time = (self.time + elapsed * self.speed).min(3600.0);
        }
        self.poll_statistics()?;
        let input = self.input.take_egui_input(&self.window);
        let context = self.context.clone();
        let mut dock = std::mem::replace(&mut self.dock, egui_dock::DockState::new(Vec::new()));
        let output = context.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    egui_dock::DockArea::new(&mut dock)
                        .style(self.dock_style.clone())
                        .show_close_buttons(false)
                        .show_inside(
                            ui,
                            &mut Panels(|panel, ui: &mut egui::Ui| match panel {
                                Panel::Controls => self.controls_panel(ui),
                                Panel::Maps => self.maps_panel(ui),
                                Panel::Statistics => self.statistics_panel(ui),
                            }),
                        );
                });
        });
        self.dock = dock;
        if self.apply {
            self.apply = false;
            match Experiment::new(&self.gpu, self.draft, self.controls.grid) {
                Ok(experiment) => {
                    self.experiment = experiment;
                    self.error = None;
                    self.refresh_images();
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        } else if self.experiment.maps.grid != self.controls.grid {
            self.experiment.maps =
                Maps::new(&self.gpu, &self.experiment.foam.field, self.controls.grid);
            self.refresh_images();
        }
        self.input
            .handle_platform_output(&self.window, output.platform_output);
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.gpu.device, &self.gpu.queue, *id, delta);
        }
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.resize(self.surface_config.width, self.surface_config.height);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout | wgpu::SurfaceError::Other) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.experiment
            .encode(&self.gpu, &mut encoder, self.time, self.controls);
        let read = if self.pending.is_none() && now - self.last_read >= Duration::from_millis(250) {
            self.last_read = now;
            let values = &self.experiment.maps.values;
            let buffer = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("weight statistics staging"),
                size: values.size(),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(values, 0, &buffer, 0, values.size());
            Some(buffer)
        } else {
            None
        };
        let jobs = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: output.pixels_per_point,
        };
        let commands = self.renderer.update_buffers(
            &self.gpu.device,
            &self.gpu.queue,
            &mut encoder,
            &jobs,
            &screen,
        );
        {
            let view = frame.texture.create_view(&Default::default());
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("foam weight lab GUI"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        depth_slice: None,
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
        self.gpu
            .queue
            .submit(commands.into_iter().chain([encoder.finish()]));
        if let Some(buffer) = read {
            let (sender, receiver) = mpsc::channel();
            buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    let _ = sender.send(result);
                });
            self.pending = Some(Pending {
                buffer,
                receiver,
                time: self.time,
                params: self.controls.params(self.experiment.ocean.config),
            });
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        frame.present();
        self.frames += 1;
        Ok(())
    }
    fn refresh_images(&mut self) {
        for (id, view) in self.images.into_iter().zip(views(&self.experiment.maps)) {
            self.renderer.update_egui_texture_from_wgpu_texture(
                &self.gpu.device,
                &view,
                wgpu::FilterMode::Nearest,
                id,
            );
        }
        self.stats = None;
        // An older in-flight sample is explicitly tagged with its parameters;
        // let it finish instead of synchronously waiting or mapping twice.
        self.last_read = Instant::now() - Duration::from_secs(1);
    }
}
struct App {
    state: Option<State>,
    error: Option<String>,
    frames: Option<u32>,
    time: f32,
    paused: bool,
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let result = (|| -> Result<State, Error> {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_title("Foam Weight Lab | FFT J | Space: pause | Esc: exit")
                        .with_inner_size(winit::dpi::LogicalSize::new(1440.0, 960.0)),
                )?,
            );
            pollster::block_on(State::new(window, self.time, self.paused))
        })();
        match result {
            Ok(state) => self.state = Some(state),
            Err(error) => {
                self.error = Some(error.to_string());
                event_loop.exit();
            }
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else {
            return;
        };
        if state.window.id() != id {
            return;
        }
        let response = state.input.on_window_event(&state.window, &event);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size.width, size.height),
            WindowEvent::KeyboardInput { event, .. }
                if !response.consumed && event.state == ElementState::Pressed && !event.repeat =>
            {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => event_loop.exit(),
                    PhysicalKey::Code(KeyCode::Space) => state.paused = !state.paused,
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested
                if state.window.inner_size().width > 0 && state.window.inner_size().height > 0 =>
            {
                if let Err(error) = state.draw() {
                    self.error = Some(error.to_string());
                    event_loop.exit();
                }
                if self.frames.is_some_and(|limit| state.frames >= limit) {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}
pub fn run(frames: Option<u32>, time: f32, paused: bool) -> Result<(), Error> {
    if !time.is_finite() || !(0.0..=3600.0).contains(&time) || frames == Some(0) {
        return Err("time must be finite in 0..3600; frames must be positive".into());
    }
    let mut app = App {
        state: None,
        error: None,
        frames,
        time,
        paused,
    };
    EventLoop::new()?.run_app(&mut app)?;
    app.error.map_or(Ok(()), |error| Err(error.into()))
}

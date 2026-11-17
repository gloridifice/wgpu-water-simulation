use super::*;
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
use yummy_egui::{parameter, parameter_table};

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
            Panel::Controls => "Experiment Settings",
            Panel::Maps => "FFT → J → Foam Emission Weight",
            Panel::Statistics => "Values and Notes",
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
    instance: wgpu::Instance,
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
        let theme = yummy_egui::Theme::default();
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
            instance,
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
            yummy_egui::h3(ui, "Playback");
            ui.horizontal_wrapped(|ui| {
                if ui.button(if self.paused {"Play"} else {"Pause"}).clicked(){self.paused = !self.paused;}
                if ui.add_enabled(self.paused,egui::Button::new("Step 1/60 s")).clicked(){self.time+=1.0/60.0;}
                if ui.button("Return to 0 s").clicked(){self.time=0.0;self.paused=true;}
            });
            parameter_table(ui,"clock",|ui| {
                parameter(ui,"Time (s)","Simulation time, 0–3600 s. Editing pauses immediately at that time; there is no foam history to clear.",|ui| {
                    if ui.add(egui::DragValue::new(&mut self.time).range(0.0..=3600.0).speed(0.02).max_decimals(3)).changed(){self.paused=true;}
                });
                parameter(ui,"Time scale","0–3, dimensionless. Changes playback speed only, not J at a given simulation time. Applies immediately.",|ui|ui.add(egui::Slider::new(&mut self.speed,0.0..=3.0)));
            });
            ui.separator();yummy_egui::h3(ui, "FFT Spectrum (Apply to Update)");
            parameter_table(ui,"spectrum",|ui| {
                parameter(ui,"FFT resolution","64–512 samples per axis. Applying rebuilds the existing Ocean FFT, not a separate spectrum; current time is retained.",|ui| {
                    egui::ComboBox::from_id_salt("fft n").width(ui.available_width()).selected_text(self.draft.resolution.to_string()).show_ui(ui,|ui| {for n in [64,128,256,512] {ui.selectable_value(&mut self.draft.resolution,n,n.to_string());}});
                });
                parameter(ui,"Period length (m)","16–256 m, world-space period. Applying rebuilds the FFT and adjusts the experiment domain using the extent multiplier.",|ui|ui.add(egui::Slider::new(&mut self.draft.length,16.0..=256.0)));
                parameter(ui,"Spectrum amplitude","Phillips spectrum coefficient, 0–0.03, not wave height in meters; 0 gives flat water. Applying rebuilds the FFT.",|ui|ui.add(egui::Slider::new(&mut self.draft.amplitude,0.0..=0.03).max_decimals(5)));
                parameter(ui,"Dominant wave scale (m)","Phillips characteristic length, 1–100 m; not the minimum wavelength. Applying rebuilds the FFT.",|ui|ui.add(egui::Slider::new(&mut self.draft.phillips_length,1.0..=100.0)));
                let mut angle=self.draft.wind_angle.to_degrees();
                if parameter(ui,"Wind direction (degrees)","−180–180 degrees. Changes spectrum direction; applying rebuilds the FFT.",|ui|ui.add(egui::Slider::new(&mut angle,-180.0..=180.0))).changed(){self.draft.wind_angle=angle.to_radians();}
                parameter(ui,"Random seed","Unsigned integer; fix the seed for repeatable comparisons. Applying rebuilds the FFT; personal settings are not used.",|ui|ui.add(egui::DragValue::new(&mut self.draft.seed)));
            });
            if ui.button("Apply FFT Settings").clicked(){self.apply=true;}
            ui.separator();yummy_egui::h3(ui, "J and Target Sampling Grid");
            parameter_table(ui,"sampling",|ui| {
                parameter(ui,"Horizontal displacement scale","0–3, dimensionless. The actual displacement coefficient is negative, consistent with ocean choppiness; height gain is fixed at 1. At 0, J=1. Applies immediately without rebuilding FFT.",|ui|ui.add(egui::Slider::new(&mut self.controls.chop,0.0..=3.0)));
                parameter(ui,"Target grid","64–512 cells per axis. Changes only weight map sampling, not FFT; SWE is not run. Cell area averages cannot create extra detail. Diagnostic maps rebuild immediately.",|ui| {
                    egui::ComboBox::from_id_salt("target n").width(ui.available_width()).selected_text(self.controls.grid.to_string()).show_ui(ui,|ui| {for n in [64,128,256,512] {ui.selectable_value(&mut self.controls.grid,n,n.to_string());}});
                });
                for (axis,name) in ["X extent / period","Z extent / period"].into_iter().enumerate() {
                    parameter(ui,name,"0.25–3 times the FFT period, centered in world space. Larger extents mean coarser cells; samples repeat across periods. Applies immediately.",|ui|ui.add(egui::Slider::new(&mut self.controls.extent[axis],0.25..=3.0).max_decimals(2)));
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("Single-period square").clicked(){self.controls.extent=[1.0,1.0];}
                if ui.button("Alongshore strip 0.5 × 3").clicked(){self.controls.extent=[0.5,3.0];}
            });
            ui.separator();yummy_egui::h3(ui, "Emission Weight (Live)");
            parameter_table(ui,"weights",|ui| {
                parameter(ui,"Compression threshold","0–1, dimensionless. C=max(0,1−J); the mask appears only above the threshold. Changes illustrative weights only, not water or existing foam.",|ui|ui.add(egui::Slider::new(&mut self.controls.threshold,0.0..=1.0).max_decimals(3)));
                parameter(ui,"Compression transition width","0–1, dimensionless. The mask smoothly reaches 1 between the threshold and threshold + width. 0 is a hard threshold requiring a strictly greater value; flat water does not trigger it.",|ui|ui.add(egui::Slider::new(&mut self.controls.transition,0.0..=1.0).max_decimals(3)));
                parameter(ui,"Modulation strength","0–1, dimensionless. W=(1−strength)+strength×gain×M. 0 restores exactly W=1; 1 fully uses the J mask. Applies immediately; not automatically normalized.",|ui|ui.add(egui::Slider::new(&mut self.controls.strength,0.0..=1.0).max_decimals(2)));
                parameter(ui,"Mask gain","0–8, dimensionless. Changes peak and mean weight without stretching contrast per frame. Use mean W on the right to assess total emission changes.",|ui|ui.add(egui::Slider::new(&mut self.controls.gain,0.0..=8.0).max_decimals(2)));
                parameter(ui,"Illustrative baseline emission","0–10 foam amount/area/simulation s (not measured mass). Uniform baseline across the domain, S=baseline×W; neither the actual SWE emission rate nor accumulated density.",|ui|ui.add(egui::Slider::new(&mut self.controls.base_rate,0.0..=10.0).max_decimals(2)));
            });
        });
    }
    fn statistics_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("lab statistics").show(ui,|ui| {
            yummy_egui::h3(ui, "Standalone Weight Experiment · Settings Not Saved");
            ui.label("Not coupled to SWE; accumulated foam density is not shown.");
            let config=self.experiment.ocean.config;
            let p=self.controls.params(config);
            ui.separator();yummy_egui::h3(ui, "Current Sampling");
            ui.label(format!("FFT: {}², {:.2} m/sample",config.resolution,config.length/config.resolution as f32));
            ui.label(format!("Target: {}², {:.1} × {:.1} m",self.controls.grid,p.domain[1],p.domain[2]));
            ui.label(format!("Per cell: {:.3} × {:.3} m",p.domain[1]/self.controls.grid as f32,p.domain[2]/self.controls.grid as f32));
            ui.label("Domain shown to world scale; +X points right, +Z up.").on_hover_text("Images use nearest-neighbor display to reveal grid resolution; downscaling can still hide thin lines, so click an image to enlarge it. Calculations use area averages, not point samples. All plots use undeformed FFT world coordinates.");
            ui.separator();yummy_egui::h3(ui, "Asynchronous Statistics (~4 Hz)");
            if let Some(s)=&self.stats {
                ui.label(format!("Sample time: {:.3} simulation s",s.time));
                if s.params!=p {ui.colored_label(egui::Color32::YELLOW,"Parameters changed; awaiting new statistics");}
                for (a,label) in ["Area ratio J","Emission mask M","Emission weight W","Illustrative emission rate S"].iter().enumerate() {
                    yummy_egui::h3(ui, *label);
                    ui.label(format!("{:.4} – {:.4}\nMean {:.4}",s.min[a],s.max[a],s.mean[a]));
                }
                ui.label(format!("Mask > 0.5: {:.1}%\nCell-average J ≤ 0: {:.2}%",100.0*s.active,100.0*s.folded));
                ui.label(format!("Total emission relative to baseline: {:.3}×",s.mean[2])).on_hover_text("The baseline is uniform over the domain, so mean W equals the integrated emission rate multiplier (when the baseline is zero, it represents only the weight multiplier). Total emission is not automatically preserved; reduced emission does not by itself imply improved shape.");
                if s.max[2]>self.controls.weight_range || s.max[3]>self.controls.rate_range {ui.colored_label(egui::Color32::YELLOW,"Some colors are saturated; values are not clamped");}
            } else {ui.label("Awaiting readback…");}
            ui.separator();yummy_egui::h3(ui, "Fixed Color Scales (Display Only)");
            parameter_table(ui,"display scales",|ui| {
                parameter(ui,"J deviation range","0.01–2, dimensionless. Color scale is 1±this value: orange compression, off-white J=1, blue expansion. Does not affect calculations; applies immediately.",|ui|ui.add(egui::Slider::new(&mut self.controls.j_range,0.01..=2.0).logarithmic(true)));
                parameter(ui,"Weight color maximum","1.1–8, dimensionless. Black=0, teal=1, yellow=maximum; values above it saturate only the color, not W. Applies immediately.",|ui|ui.add(egui::Slider::new(&mut self.controls.weight_range,1.1..=8.0)));
                parameter(ui,"Emission rate white value","0.01–80 foam amount/area/s. Black=0, white=this value; affects only the illustrative source display. Applies immediately.",|ui|ui.add(egui::Slider::new(&mut self.controls.rate_range,0.01..=80.0).logarithmic(true)));
            });
            ui.separator();
            ui.label("C = max(0, 1 − J)\nM = smoothstep(threshold, threshold + width, C)\nW = (1 − strength) + strength × gain × M\nS = illustrative baseline emission rate × W").on_hover_text("The mask is calculated on raw FFT samples, then area-averaged over target cells. The J plot also shows cell averages; the mask is not calculated from cell-average J. J is a geometric compression proxy, not velocity divergence or an exact breaking rate. Offshore foam history does not affect the weight.");
            if let Some(error)=&self.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        });
    }
    fn maps_panel(&mut self, ui: &mut egui::Ui) {
        const TITLES: [&str; 4] = [
            "J · Cell-average area ratio",
            "M · Cell-average emission mask",
            "W · Emission weight",
            "S · Illustrative weighted emission rate (not density)",
        ];
        let legends = [
            format!(
                "Orange: {:.2}  |  Off-white: 1  |  Blue: {:.2}",
                1.0 - self.controls.j_range,
                1.0 + self.controls.j_range
            ),
            "Black: 0  |  White: 1 (compression only)".into(),
            format!(
                "Black: 0  |  Teal: 1  |  Yellow: {:.1}",
                self.controls.weight_range
            ),
            format!("Black: 0  |  White: {:.2} /s", self.controls.rate_range),
        ];
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.focus.is_none(), "Four-plot comparison")
                .clicked()
            {
                self.focus = None;
            }
            for (i, title) in ["J", "Mask", "Weight", "Source"].iter().enumerate() {
                if ui.selectable_label(self.focus == Some(i), *title).clicked() {
                    self.focus = Some(i);
                }
            }
        });
        let plot = |ui: &mut egui::Ui, i: usize, size: egui::Vec2| {
            yummy_egui::h3(ui, TITLES[i]);
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
                Err(error) => self.error = Some(format!("Statistics readback failed: {error}")),
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
        let output = context.run_ui(input, |root_ui| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show_inside(root_ui, |ui| {
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
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.surface_config.width, self.surface_config.height);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self.instance.create_surface(self.window.clone())?;
                self.resize(self.surface_config.width, self.surface_config.height);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("surface texture validation failed".into());
            }
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

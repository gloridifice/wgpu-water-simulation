//! One persistent, windowless Ocean scene driven by the common MCP control adapter.
use crate::ocean::{
    Error,
    parameters::ocean::OceanConfig,
    render::Renderer,
    settings::{SavedSettings, SettingsStore},
    simulation::{
        offshore::foam::{Foam, FoamStep},
        spectral::FftWaves as Ocean,
    },
};
use common::{
    control::{Command, Request, patch_settings as patched_settings},
    gpu::Gpu,
};
use serde_json::{Value, json};
use std::{path::Path, sync::mpsc::sync_channel};

const SIZE: [u32; 2] = [1280, 800];
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub(super) struct Scene {
    gpu: Gpu,
    format: wgpu::TextureFormat,
    size: [u32; 2],
    ocean: Ocean,
    foam: Foam,
    renderer: Renderer,
    settings: SavedSettings,
    store: Option<SettingsStore>,
    time: f32,
    reset: bool,
    emit: bool,
    clear: bool,
    version: u64,
}
impl Scene {
    async fn new(settings: SavedSettings, store: Option<SettingsStore>) -> Result<Self, Error> {
        let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
        eprintln!("Headless Ocean GPU: {}", gpu.name);
        Self::with_gpu(gpu, FORMAT, SIZE, settings, store)
    }
    pub(super) fn with_gpu(
        gpu: Gpu,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        settings: SavedSettings,
        store: Option<SettingsStore>,
    ) -> Result<Self, Error> {
        settings.validate()?;
        let ocean = Ocean::new(&gpu, settings.ocean)?;
        let foam = Foam::new(&gpu, &ocean.fields, settings.ocean.resolution);
        let mut renderer = Renderer::new_display(&gpu, &ocean, &foam, format, size[0], size[1]);
        renderer.set_shore_resolution(&gpu, &ocean, settings.optics.shore.resolution, format, size);
        renderer.prepare_scene(&gpu, &settings.optics, format)?;
        Ok(Self {
            gpu,
            format,
            size,
            ocean,
            foam,
            renderer,
            settings,
            store,
            time: 0.0,
            reset: true,
            emit: false,
            clear: false,
            version: 0,
        })
    }
    fn state(&self) -> Value {
        json!({"version":self.version,"time_seconds":self.time,"paused":self.settings.paused,
            "scene":if self.settings.optics.lake.enabled{"lake"}else if self.settings.optics.model.enabled{"model"}else if self.settings.optics.beach{"beach"}else{"ocean"}})
    }
    fn get(&self) -> Result<Value, String> {
        Ok(
            json!({"state":self.state(),"settings":serde_json::to_value(&self.settings).map_err(|e|e.to_string())?}),
        )
    }
    fn install(&mut self, next: SavedSettings) -> Result<(), String> {
        next.validate()?;
        if next.ocean != self.settings.ocean
            && next.optics == self.settings.optics
            && (OceanConfig {
                mesh_resolution: self.settings.ocean.mesh_resolution,
                triangular_mesh: self.settings.ocean.triangular_mesh,
                ..next.ocean
            }) == self.settings.ocean
        {
            self.renderer.set_ocean_mesh(&self.gpu.device, next.ocean);
            self.ocean.config = next.ocean;
        } else if next.ocean != self.settings.ocean {
            let ocean = Ocean::new(&self.gpu, next.ocean).map_err(|e| e.to_string())?;
            let foam = Foam::new(&self.gpu, &ocean.fields, next.ocean.resolution);
            let mut renderer = Renderer::new_display(
                &self.gpu,
                &ocean,
                &foam,
                self.format,
                self.size[0],
                self.size[1],
            );
            renderer.set_shore_resolution(
                &self.gpu,
                &ocean,
                next.optics.shore.resolution,
                self.format,
                self.size,
            );
            renderer.model_assets = self.renderer.model_assets.clone();
            renderer
                .prepare_scene(&self.gpu, &next.optics, self.format)
                .map_err(|e| e.to_string())?;
            self.ocean = ocean;
            self.foam = foam;
            self.renderer = renderer;
            self.reset = true;
        } else if next.optics != self.settings.optics {
            let regrid = next.optics.shore.resolution != self.settings.optics.shore.resolution;
            if regrid {
                self.renderer.set_shore_resolution(
                    &self.gpu,
                    &self.ocean,
                    next.optics.shore.resolution,
                    self.format,
                    self.size,
                );
            }
            if let Err(error) = self
                .renderer
                .prepare_scene(&self.gpu, &next.optics, self.format)
            {
                if regrid {
                    self.renderer.set_shore_resolution(
                        &self.gpu,
                        &self.ocean,
                        self.settings.optics.shore.resolution,
                        self.format,
                        self.size,
                    );
                }
                self.renderer
                    .prepare_scene(&self.gpu, &self.settings.optics, self.format)
                    .map_err(|restore| format!("{error}; rollback failed: {restore}"))?;
                return Err(error.to_string());
            }
            if next.optics.lake.terrain != self.settings.optics.lake.terrain
                || next.optics.lake.mode != self.settings.optics.lake.mode
                || next.optics.lake.wave_seed != self.settings.optics.lake.wave_seed
                || next.optics.lake.enabled != self.settings.optics.lake.enabled
            {
                self.reset = true;
            }
        }
        self.settings = next;
        self.version += 1;
        Ok(())
    }
    fn action(&mut self, id: &str) -> Result<Value, String> {
        match id {
            "ocean.apply_draft" => {
                let mut next = self.settings.clone();
                next.ocean = next.draft;
                self.install(next)?;
            }
            "simulation.reset" => {
                self.time = 0.0;
                self.reset = true;
                self.version += 1;
            }
            "camera.reset" => {
                let mut next = self.settings.clone();
                next.camera = if next.optics.lake.enabled {
                    crate::ocean::camera::Camera::lake(next.optics.lake.terrain.radius)
                } else if next.optics.beach {
                    crate::ocean::camera::Camera::beach(next.ocean.length)
                } else {
                    crate::ocean::camera::Camera::new(next.ocean.length)
                };
                self.install(next)?;
            }
            "lake.emit" if self.settings.optics.lake.enabled => {
                self.emit = true;
                self.step(0)?;
            }
            "lake.clear" if self.settings.optics.lake.enabled => {
                self.clear = true;
                self.step(0)?;
            }
            "configuration.save" => {
                let store = self
                    .store
                    .as_mut()
                    .ok_or("read-only scene; start with --settings PATH to enable save")?;
                store.update(self.settings.clone(), true);
                if store.failed {
                    return Err(store.status.clone());
                }
            }
            _ => return Err(format!("unknown or unavailable action: {id}")),
        }
        self.get()
    }
    fn step(&mut self, frames: u32) -> Result<Value, String> {
        self.step_with_dt(frames, 1.0 / 60.0)
    }
    pub(super) fn step_with_dt(&mut self, frames: u32, delta: f32) -> Result<Value, String> {
        if frames > 600 {
            return Err("frames must be 0..=600".into());
        }
        // A reset initializes without consuming one of the requested simulation steps.
        if frames > 0 && self.reset {
            self.step(0)?;
        }
        for _ in 0..frames.max(1) {
            let dt = if frames == 0 { 0.0 } else { delta };
            if frames != 0 {
                self.time += dt;
            }
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            let camera = self.settings.camera.uniform(
                self.settings.ocean,
                self.size[0] as f32 / self.size[1] as f32,
            );
            self.renderer
                .update_optics(&self.gpu, &self.settings.optics);
            if self.settings.optics.lake.enabled {
                if self.reset {
                    self.time = 0.0;
                }
                self.time = self.renderer.encode_lake(
                    &self.gpu,
                    &mut encoder,
                    f64::from(dt),
                    self.settings.optics.lake,
                    crate::ocean::simulation::animated_lake::LakeCommands {
                        emit: std::mem::take(&mut self.emit),
                        clear: std::mem::take(&mut self.clear),
                        reset: self.reset,
                    },
                ) as f32;
            } else {
                self.ocean.encode_filtered(
                    &self.gpu,
                    &mut encoder,
                    self.time,
                    &self.settings.optics.shore,
                );
                self.foam.encode(
                    &self.gpu,
                    &mut encoder,
                    FoamStep::new(
                        self.settings.ocean.resolution,
                        self.settings.ocean.length,
                        camera.ocean[2] * camera.ocean[3],
                        dt,
                        self.settings.foam,
                        self.reset,
                    ),
                );
                self.renderer.shore.encode(
                    &self.gpu,
                    &mut encoder,
                    &self.settings.optics,
                    self.time,
                    self.reset,
                    camera.ocean[3],
                );
                self.renderer.encode_spray(&self.gpu, &mut encoder);
                if let Some(error) = &self.renderer.shore.spray_error {
                    return Err(error.clone());
                }
            }
            self.gpu.queue.submit([encoder.finish()]);
            self.reset = false;
        }
        self.version += 1;
        Ok(self.state())
    }
    fn capture(&mut self, path: &str) -> Result<Value, String> {
        let path = Path::new(path);
        if self.reset {
            self.step(0)?;
        }
        self.renderer
            .update_optics(&self.gpu, &self.settings.optics);
        let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("MCP scene capture"),
            size: wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let stride = (self.size[0] * 4).div_ceil(256) * 256;
        let staging = self.gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("MCP capture readback"),
            size: u64::from(stride * self.size[1]),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        let camera = self.settings.camera.uniform(
            self.settings.ocean,
            self.size[0] as f32 / self.size[1] as f32,
        );
        self.renderer.encode(
            &self.gpu,
            &mut encoder,
            &texture.create_view(&Default::default()),
            &camera,
        );
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
        let data = self.gpu.map_buffer(&staging).map_err(|e| e.to_string())?;
        let file = std::fs::File::create_new(path).map_err(|e| e.to_string())?;
        let pixels: Vec<u8> = data
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..(self.size[0] * 4) as usize].iter().copied())
            .collect();
        let pixels = if matches!(
            self.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            let mut rgba = pixels;
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            rgba
        } else {
            pixels
        };
        let mut png = png::Encoder::new(std::io::BufWriter::new(file), self.size[0], self.size[1]);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .and_then(|mut writer| writer.write_image_data(&pixels))
            .map_err(|e| e.to_string())?;
        Ok(json!({"path":path.display().to_string(),"state":self.state()}))
    }
    pub(super) fn gpu(&self) -> &Gpu {
        &self.gpu
    }
    pub(super) fn settings(&self) -> &SavedSettings {
        &self.settings
    }
    pub(super) fn resize(&mut self, size: [u32; 2]) {
        self.size = size;
        self.renderer.resize(&self.gpu.device, size[0], size[1]);
    }
    pub(super) fn draw_into(&mut self, target: &wgpu::TextureView) {
        self.renderer
            .update_optics(&self.gpu, &self.settings.optics);
        let camera = self.settings.camera.uniform(
            self.settings.ocean,
            self.size[0] as f32 / self.size[1] as f32,
        );
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.renderer
            .encode(&self.gpu, &mut encoder, target, &camera);
        self.gpu.queue.submit([encoder.finish()]);
    }
    pub(super) fn handle(&mut self, cmd: Command) -> Result<Value, String> {
        match cmd {
            Command::Describe => {
                let mut actions = vec!["ocean.apply_draft", "simulation.reset", "camera.reset"];
                if self.settings.optics.lake.enabled {
                    actions.extend(["lake.emit", "lake.clear"]);
                }
                if self.store.is_some() {
                    actions.push("configuration.save");
                }
                Ok(
                    json!({"scene":"ocean","mode":"headless","step_seconds":1.0/60.0,
                    "parameter_root":["ocean","draft","foam","optics","camera","speed","paused"],
                    "actions":actions,
                    "notes":"Patch scalar fields (including numeric array indices); ocean applies directly, draft requires ocean.apply_draft. Explicit save requires --settings. Steps run even when paused."}),
                )
            }
            Command::Get => self.get(),
            Command::Patch(changes) => {
                let next = patched_settings(&self.settings, changes)?;
                self.install(next)?;
                self.get()
            }
            Command::Action(id) => self.action(&id),
            Command::Step(frames) => self.step(frames),
            Command::Capture(path) => self.capture(&path),
        }
    }
}

pub fn run(initial: SavedSettings, store: Option<SettingsStore>) -> Result<(), Error> {
    let (tx, rx) = sync_channel::<Request>(32);
    let (ready_tx, ready_rx) = sync_channel(1);
    let worker = std::thread::spawn(move || -> Result<(), String> {
        let initialized = pollster::block_on(Scene::new(initial, store))
            .map_err(|e| e.to_string())
            .and_then(|mut scene| {
                scene.step(0)?;
                Ok(scene)
            });
        let _ = ready_tx.send(initialized.as_ref().map(|_| ()).map_err(Clone::clone));
        let mut scene = initialized?;
        while let Ok(request) = rx.recv() {
            if request.reply.is_closed() {
                continue;
            }
            let response = scene.handle(request.command);
            let _ = request.reply.send(response);
        }
        Ok(())
    });
    if let Err(error) = ready_rx
        .recv()
        .map_err(|_| "scene worker stopped during startup")?
    {
        let _ = worker.join();
        return Err(error.into());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let served = runtime.block_on(common::control::serve_stdio(tx));
    let finished = worker.join().map_err(|_| "scene worker panicked")?;
    finished.map_err(|e| -> Error { e.into() })?;
    served
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patch_is_validated_as_a_whole_without_changing_the_original() {
        let original = SavedSettings::default();
        let candidate =
            patched_settings(&original, json!({"speed":2.0,"camera.gain":1.5})).unwrap();
        assert_eq!(candidate.speed, 2.0);
        assert_eq!(candidate.camera.gain, 1.5);
        let panned = patched_settings(&original, json!({"camera.target.0":3.0})).unwrap();
        assert_eq!(panned.camera.target[0], 3.0);
        assert_eq!(original.speed, 1.0);
        assert!(patched_settings(&original, json!({"speed":2.0,"optics.unknown":4})).is_err());
        assert!(patched_settings(&original, json!({"speed":-1.0})).is_err());
        assert!(patched_settings(&original, json!({"optics":{}})).is_err());
        assert!(patched_settings(&original, json!({"camera.target.99":3.0})).is_err());
    }
}

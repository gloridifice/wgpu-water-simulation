//! Persistent Fog GPU scene without window, GUI or a private solver implementation.
use super::fields;
use crate::{
    Error,
    parameters::Session,
    render::{Renderer, save_png},
    simulation::{Diagnostics, Simulation},
    terrain::{BakeSource, Terrain},
};
use common::{
    control::{Command, Request, patch_settings},
    gpu::Gpu,
    settings::{Settings, SettingsStore},
};
use serde_json::{Value, json};
use std::{path::Path, sync::mpsc::sync_channel};

const SIZE: [u32; 2] = [1280, 800];
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct Scene {
    gpu: Gpu,
    session: Session,
    native: Terrain,
    terrain: Terrain,
    source: BakeSource,
    simulation: Simulation,
    renderer: Renderer,
    extra_fields: Vec<fields::Field>,
    store: Option<SettingsStore<Session>>,
    reset: bool,
    requested_time: f32,
    last: Diagnostics,
    version: u64,
}
impl Scene {
    async fn new(
        mut session: Session,
        native: Terrain,
        source: BakeSource,
        store: Option<SettingsStore<Session>>,
    ) -> Result<Self, Error> {
        session.validate()?;
        session.player.place(&native)?;
        let gpu = Gpu::new(&wgpu::Instance::default(), None).await?;
        eprintln!("Headless Fog GPU: {}", gpu.name);
        let (terrain, simulation, renderer, extra_fields) =
            fields::build(&gpu, &native, &source, &session, FORMAT)?;
        if session.volumes.is_empty() {
            session.player.place(&terrain)?;
        }
        Ok(Self {
            gpu,
            session,
            native,
            terrain,
            source,
            simulation,
            renderer,
            extra_fields,
            store,
            reset: true,
            requested_time: 0.0,
            last: Diagnostics::default(),
            version: 0,
        })
    }
    fn state(&self) -> Value {
        json!({"scene":self.session.scene.label(),"version":self.version,
            "requested_time_seconds":self.requested_time,
            "accepted_time_seconds":self.last.simulated_time,
            "dropped_time_seconds":self.last.dropped_time,
            "fault":self.last.fault,
            "paused":self.session.paused,
            "resolution":self.simulation.resolution,
            "domain":self.simulation.domain,
            "field_count":1+self.extra_fields.len()})
    }
    fn get(&self) -> Result<Value, String> {
        Ok(
            json!({"state":self.state(),"settings":serde_json::to_value(&self.session).map_err(|e|e.to_string())?}),
        )
    }
    fn install(&mut self, mut next: Session) -> Result<(), String> {
        next.validate()?;
        let switched = next.scene != self.session.scene;
        let rebuild = switched
            || next.grid_resolution != self.session.grid_resolution
            || next.domain_scale != self.session.domain_scale
            || next.volumes != self.session.volumes;
        if rebuild {
            let (source, native) = if switched {
                next.scene.load().map_err(|e| e.to_string())?
            } else {
                (self.source.clone(), self.native.clone())
            };
            let (terrain, simulation, renderer, extra_fields) =
                fields::build(&self.gpu, &native, &source, &next, FORMAT)
                    .map_err(|e| e.to_string())?;
            if switched {
                next.player.feet = [0.0; 3];
            }
            next.player
                .place(if next.volumes.is_empty() {
                    &terrain
                } else {
                    &native
                })
                .map_err(|e| e.to_string())?;
            self.extra_fields = extra_fields;
            self.source = source;
            self.native = native;
            self.terrain = terrain;
            self.simulation = simulation;
            self.renderer = renderer;
            self.requested_time = 0.0;
            self.last = Diagnostics::default();
            self.reset = true;
        } else if next.fog != self.session.fog {
            // Automated edits use an explicit, conservative reset for any fog field.
            self.requested_time = 0.0;
            self.last = Diagnostics::default();
            self.reset = true;
        }
        self.session = next;
        self.version += 1;
        Ok(())
    }
    fn step(&mut self, frames: u32) -> Result<Value, String> {
        if frames > 600 {
            return Err("frames must be 0..=600".into());
        }
        if self.reset && frames > 0 {
            self.step(0)?;
        }
        for _ in 0..frames.max(1) {
            let dt = if frames == 0 { 0.0 } else { 1.0 / 30.0 };
            let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
            self.simulation
                .encode(
                    &self.gpu,
                    &mut encoder,
                    &self.session.fog,
                    dt,
                    self.reset,
                    &[],
                )
                .map_err(|e| e.to_string())?;
            for field in &mut self.extra_fields {
                field
                    .simulation
                    .encode(
                        &self.gpu,
                        &mut encoder,
                        &self.session.fog,
                        dt,
                        self.reset,
                        &[],
                    )
                    .map_err(|e| e.to_string())?;
            }
            self.gpu.queue.submit([encoder.finish()]);
            self.requested_time += dt;
            self.reset = false;
        }
        // Explicit debug step checks the actual GPU scheduler, not a CPU wall-clock estimate.
        let (diagnostics, _) = self
            .simulation
            .snapshot(&self.gpu)
            .map_err(|e| e.to_string())?;
        for field in &mut self.extra_fields {
            let (d, _) = field
                .simulation
                .snapshot(&self.gpu)
                .map_err(|e| e.to_string())?;
            if d.fault != 0 {
                return Err(format!("local fog GPU scheduler fault {}", d.fault));
            }
        }
        self.last = diagnostics;
        self.version += 1;
        if diagnostics.fault != 0 {
            return Err(format!("fog GPU scheduler fault {}", diagnostics.fault));
        }
        Ok(self.state())
    }
    fn action(&mut self, id: &str) -> Result<Value, String> {
        match id {
            "simulation.reset" => {
                self.requested_time = 0.0;
                self.reset = true;
                self.step(0)?;
            }
            "configuration.save" => {
                let store = self
                    .store
                    .as_mut()
                    .ok_or("read-only scene; start with --settings PATH")?;
                store.update(self.session.clone(), true);
                if store.failed {
                    return Err(store.status.clone());
                }
            }
            _ => return Err(format!("unknown or unavailable action: {id}")),
        }
        self.get()
    }
    fn capture(&mut self, path: &str) -> Result<Value, String> {
        if self.reset {
            self.step(0)?;
        }
        let pixels = Renderer::read_fields(
            &fields::renderers(&self.renderer, self.simulation.domain, &self.extra_fields),
            &self.gpu,
            &self.session,
            SIZE,
        )
        .map_err(|e| e.to_string())?;
        save_png(Path::new(path), &pixels, SIZE).map_err(|e| e.to_string())?;
        Ok(json!({"path":path,"state":self.state()}))
    }
    fn handle(&mut self, cmd: Command) -> Result<Value, String> {
        match cmd {
            Command::Describe => {
                let mut actions = vec!["simulation.reset"];
                if self.store.is_some() {
                    actions.push("configuration.save");
                }
                Ok(
                    json!({"scene":"fog","mode":"headless","step_seconds":1.0/30.0,
                    "parameter_root":["scene","grid_resolution","domain_scale","volumes","fog","camera","player","render_mode","paused"],
                    "actions":actions,
                    "notes":"Patching fog fields resets the field; rendering/player/camera changes do not. Built-in scene or grid changes rebuild. Explicit steps ignore paused. No input or player movement in headless mode."}),
                )
            }
            Command::Get => self.get(),
            Command::Patch(changes) => {
                let next = patch_settings(&self.session, changes)?;
                self.install(next)?;
                self.get()
            }
            Command::Action(id) => self.action(&id),
            Command::Step(frames) => self.step(frames),
            Command::Capture(path) => self.capture(&path),
        }
    }
}

pub fn run(
    initial: Session,
    terrain: Terrain,
    source: BakeSource,
    store: Option<SettingsStore<Session>>,
) -> Result<(), Error> {
    let (tx, rx) = sync_channel::<Request>(32);
    let (ready_tx, ready_rx) = sync_channel(1);
    let worker = std::thread::spawn(move || -> Result<(), String> {
        let initialized = pollster::block_on(Scene::new(initial, terrain, source, store))
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
            let _ = request.reply.send(scene.handle(request.command));
        }
        Ok(())
    });
    if let Err(error) = ready_rx
        .recv()
        .map_err(|_| "fog scene worker stopped during startup")?
    {
        let _ = worker.join();
        return Err(error.into());
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let served = runtime.block_on(common::control::serve_stdio(tx));
    worker
        .join()
        .map_err(|_| "fog scene worker panicked")?
        .map_err(|e| -> Error { e.into() })?;
    served
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_grid_patch_rejects_invalid_batch_without_affecting_original() {
        let original = Session::default();
        let changed = patch_settings(&original, json!({"grid_resolution":[128,256]})).unwrap();
        assert_eq!(changed.grid_resolution, Some([128, 256]));
        assert_eq!(original.grid_resolution, None);
        assert!(
            patch_settings(
                &original,
                json!({"grid_resolution":[128,0],"fog.half_life":10})
            )
            .is_err()
        );
        assert_eq!(
            patch_settings(&changed, json!({"grid_resolution":null}))
                .unwrap()
                .grid_resolution,
            None
        );
    }
}

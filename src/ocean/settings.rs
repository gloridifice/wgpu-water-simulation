//! Local sessions and named configurations, with validation and atomic autosave.
use super::{foam::FoamConfig, optics::OpticsConfig, spectrum::OceanConfig, viewer::Camera};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SavedSettings {
    pub version: u32,
    pub ocean: OceanConfig,
    pub draft: OceanConfig,
    pub foam: FoamConfig,
    pub optics: OpticsConfig,
    pub camera: Camera,
    pub speed: f32,
    pub paused: bool,
}

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            version: 1,
            ocean: OceanConfig::default(),
            draft: OceanConfig::default(),
            foam: FoamConfig::default(),
            optics: OpticsConfig::default(),
            camera: Camera::default(),
            speed: 1.0,
            paused: false,
        }
    }
}

impl SavedSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!("unsupported settings version {}", self.version));
        }
        self.ocean.validate()?;
        self.draft.validate()?;
        self.optics.validate()?;
        for (name, value, min, max) in [
            ("foam threshold", self.foam.threshold, -0.5, 1.0),
            ("foam generation", self.foam.generation, 0.0, 20.0),
            ("foam decay", self.foam.decay, 0.0, 5.0),
            ("time speed", self.speed, 0.0, 3.0),
            ("camera pitch", self.camera.pitch, 0.07, 1.35),
            ("height gain", self.camera.gain, 0.0, 3.0),
            ("chop strength", self.camera.chop_amount, 0.0, 3.0),
        ] {
            if !value.is_finite() || !(min..=max).contains(&value) {
                return Err(format!("invalid {name}"));
            }
        }
        if !self.camera.yaw.is_finite()
            || !self.camera.target.iter().all(|value| value.is_finite())
            || !self.camera.distance.is_finite()
            || self.camera.distance <= 0.0
            || self.camera.foam_debug > 2
        {
            return Err("invalid camera settings".into());
        }
        Ok(())
    }

    /// Explicit CLI values override persisted values; all other settings survive.
    pub fn apply_cli(&mut self, ocean: OceanConfig, optics: OpticsConfig, flags: &[String]) {
        let old_length = self.ocean.length;
        if flags.iter().any(|f| f == "--beach" || f == "--model") {
            self.optics = optics;
            self.camera = if optics.model.enabled {
                Camera::model(old_length)
            } else {
                Camera::beach(old_length)
            };
        }
        for flag in flags {
            match flag.as_str() {
                "--resolution" => {
                    self.ocean.resolution = ocean.resolution;
                    self.draft.resolution = ocean.resolution;
                }
                "--mesh-resolution" => {
                    self.ocean.mesh_resolution = ocean.mesh_resolution;
                    self.draft.mesh_resolution = ocean.mesh_resolution;
                }
                "--length" => {
                    self.ocean.length = ocean.length;
                    self.draft.length = ocean.length;
                }
                "--wind" | "--phillips-length" => {
                    self.ocean.phillips_length = ocean.phillips_length;
                    self.draft.phillips_length = ocean.phillips_length;
                }
                "--amplitude" => {
                    self.ocean.amplitude = ocean.amplitude;
                    self.draft.amplitude = ocean.amplitude;
                }
                "--wind-angle" => {
                    self.ocean.wind_angle = ocean.wind_angle;
                    self.draft.wind_angle = ocean.wind_angle;
                }
                "--seed" => {
                    self.ocean.seed = ocean.seed;
                    self.draft.seed = ocean.seed;
                }
                "--model-position" => self.optics.model.position = optics.model.position,
                "--model-scale" => self.optics.model.scale = optics.model.scale,
                "--model-yaw" => self.optics.model.yaw_deg = optics.model.yaw_deg,
                "--sea-level" => self.optics.model.sea_level = optics.model.sea_level,
                "--swe-center" => self.optics.model.center = optics.model.center,
                "--swe-size" => self.optics.model.size = optics.model.size,
                "--swe-resolution" => self.optics.shore.resolution = optics.shore.resolution,
                "--swe-sponge" => self.optics.model.sponge_width = optics.model.sponge_width,
                "--swe-open-edges" => self.optics.model.open_edges = optics.model.open_edges,
                "--hide-swe-box" => self.optics.model.show_bounds = false,
                "--seabed-depth" => self.optics.depth = optics.depth,
                "--sigma-t" => self.optics.sigma_t = optics.sigma_t,
                "--source" => self.optics.source = optics.source,
                "--sky-color" => self.optics.sky.color = optics.sky.color,
                "--sky-intensity" => self.optics.sky.intensity = optics.sky.intensity,
                "--scattering-albedo" => self.optics.sky.albedo = optics.sky.albedo,
                "--sky-steps" => self.optics.sky.steps = optics.sky.steps,
                "--no-skylight" | "--skylight" => self.optics.sky.enabled = optics.sky.enabled,
                "--water-view" => self.optics.view = optics.view,
                "--shore-view" => self.optics.shore.view = optics.shore.view,
                "--spray" => self.optics.spray.enabled = optics.spray.enabled,
                "--spray-debug" => {
                    self.optics.spray.enabled = optics.spray.enabled;
                    self.optics.spray.debug = optics.spray.debug;
                }
                "--foam-texture" => self.optics.shore.foam_texture = optics.shore.foam_texture,
                "--foam-opacity-scale" => {
                    self.optics.shore.foam_opacity_scale = optics.shore.foam_opacity_scale
                }
                "--foam-material-view" => {
                    self.optics.shore.foam_material_view = optics.shore.foam_material_view
                }
                "--swe-filter-cutoff" => {
                    self.optics.shore.filter_cutoff = optics.shore.filter_cutoff
                }
                "--swe-filter-transition" => {
                    self.optics.shore.filter_transition = optics.shore.filter_transition
                }
                "--water-foam-color" => {
                    self.optics.shore.water_foam_color = optics.shore.water_foam_color
                }
                "--swe-input-gain" => self.optics.shore.input_gain = optics.shore.input_gain,
                "--swe-order" => self.optics.shore.order = optics.shore.order,
                "--shore-normal-strength" => {
                    self.optics.shore.normal_strength = optics.shore.normal_strength
                }
                "--no-swe-filter" => self.optics.shore.filter_enabled = false,
                "--no-shore" => self.optics.shore.enabled = false,
                _ => {}
            }
        }
        self.camera.distance *= self.ocean.length / old_length;
    }
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("water-simulation").join("ocean-settings.json")
}

pub const DEFAULT_CONFIGURATION_NAME: &str = "默认配置";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NamedConfiguration {
    name: String,
    settings: SavedSettings,
}

impl NamedConfiguration {
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The original configuration is a snapshot, never an autosave target.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ConfigurationLibrary {
    default: SavedSettings,
    profiles: Vec<NamedConfiguration>,
    /// Session selection; independent of the manually saved parameter snapshots.
    #[serde(default)]
    active: Option<usize>,
}

impl ConfigurationLibrary {
    pub fn new(default: SavedSettings) -> Self {
        Self {
            default,
            profiles: Vec::new(),
            active: None,
        }
    }

    pub fn profiles(&self) -> &[NamedConfiguration] {
        &self.profiles
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    pub fn select(&mut self, index: Option<usize>) -> Result<(), String> {
        self.get(index)?;
        self.active = index;
        Ok(())
    }

    /// `None` identifies the immutable default; indices identify user profiles.
    pub fn get(&self, index: Option<usize>) -> Result<&SavedSettings, String> {
        match index {
            None => Ok(&self.default),
            Some(index) => self
                .profiles
                .get(index)
                .map(|p| &p.settings)
                .ok_or_else(|| "配置不存在".into()),
        }
    }

    fn check_name(&self, name: &str, except: Option<usize>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("请输入配置名称".into());
        }
        if name == DEFAULT_CONFIGURATION_NAME
            || self
                .profiles
                .iter()
                .enumerate()
                .any(|(i, p)| Some(i) != except && p.name == name)
        {
            return Err("配置名称已存在，请使用其他名称".into());
        }
        Ok(name.to_owned())
    }

    pub fn save_as(&mut self, name: &str, settings: SavedSettings) -> Result<usize, String> {
        let name = self.check_name(name, None)?;
        settings.validate()?;
        let index = self.profiles.len();
        self.profiles.push(NamedConfiguration { name, settings });
        self.active = Some(index);
        Ok(index)
    }

    pub fn rename(&mut self, index: Option<usize>, name: &str) -> Result<(), String> {
        let index = index.ok_or("默认配置不可重命名")?;
        let name = self.check_name(name, Some(index))?;
        self.profiles.get_mut(index).ok_or("配置不存在")?.name = name;
        Ok(())
    }

    pub fn overwrite(
        &mut self,
        index: Option<usize>,
        settings: SavedSettings,
    ) -> Result<(), String> {
        let index = index.ok_or("默认配置不可覆盖")?;
        settings.validate()?;
        self.profiles.get_mut(index).ok_or("配置不存在")?.settings = settings;
        Ok(())
    }

    pub fn delete(&mut self, index: Option<usize>) -> Result<(), String> {
        let index = index.ok_or("默认配置不可删除")?;
        if index >= self.profiles.len() {
            return Err("配置不存在".into());
        }
        self.profiles.remove(index);
        self.active = self.active.and_then(|active| match active.cmp(&index) {
            std::cmp::Ordering::Less => Some(active),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some(active - 1),
        });
        Ok(())
    }

    fn validate(&self) -> Result<(), String> {
        self.default.validate()?;
        for (index, profile) in self.profiles.iter().enumerate() {
            if self.check_name(&profile.name, Some(index))? != profile.name {
                return Err("配置名称不能包含首尾空格".into());
            }
            profile.settings.validate()?;
        }
        Ok(())
    }
}

/// Keep session fields at the top level for compatibility with existing files.
#[derive(Serialize, Deserialize)]
struct SettingsDocument {
    #[serde(flatten)]
    current: SavedSettings,
    #[serde(default)]
    configurations: Option<ConfigurationLibrary>,
}

fn read_document(path: &Path) -> Result<Option<SettingsDocument>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut document: SettingsDocument =
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    document.current.validate()?;
    if let Some(library) = &mut document.configurations {
        // A stale selection must not discard otherwise valid user configurations.
        if library
            .active
            .is_some_and(|index| index >= library.profiles.len())
        {
            library.active = None;
        }
        library.validate()?;
    }
    Ok(Some(document))
}

#[cfg(test)]
fn read(path: &Path) -> Result<Option<SavedSettings>, String> {
    Ok(read_document(path)?.map(|d| d.current))
}

#[cfg(test)]
fn write_atomic(path: &Path, settings: &SavedSettings) -> Result<(), String> {
    settings.validate()?;
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?;
    write_bytes_atomic(path, &bytes)
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(
        ".ocean-settings-{}-{nonce}.tmp",
        std::process::id()
    ));
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        // Same-directory replacement: the old JSON survives any failed write.
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
}

pub struct SettingsStore {
    pub path: PathBuf,
    pub status: String,
    pub failed: bool,
    observed: SavedSettings,
    saved: Option<SavedSettings>,
    configurations: ConfigurationLibrary,
    saved_configurations: Option<ConfigurationLibrary>,
    changed_at: Instant,
    attempted_at: Option<Instant>,
    backup_before_write: bool,
}

impl SettingsStore {
    pub fn open(path: PathBuf, now: Instant) -> (SavedSettings, Self) {
        let result = read_document(&path);
        let mut saved_configurations = None;
        let (initial, saved, status, failed, backup_before_write) = match result {
            Ok(Some(document)) => {
                saved_configurations = document.configurations;
                (
                    document.current.clone(),
                    Some(document.current),
                    "已载入本地配置".into(),
                    false,
                    false,
                )
            }
            Ok(None) => (
                SavedSettings::default(),
                None,
                "新配置，自动保存已启用".into(),
                false,
                false,
            ),
            Err(error) => (
                SavedSettings::default(),
                None,
                format!("载入失败：{error}；使用默认配置"),
                true,
                true,
            ),
        };
        let store = Self {
            path,
            status,
            failed,
            observed: initial.clone(),
            saved,
            configurations: saved_configurations
                .clone()
                .unwrap_or_else(|| ConfigurationLibrary::new(initial.clone())),
            saved_configurations,
            changed_at: now,
            attempted_at: None,
            backup_before_write,
        };
        (initial, store)
    }

    pub fn configurations(&self) -> &ConfigurationLibrary {
        &self.configurations
    }

    pub fn set_configurations(&mut self, library: &ConfigurationLibrary, now: Instant) {
        if &self.configurations != library {
            self.configurations = library.clone();
            self.changed_at = now;
            self.status = "更改等待自动保存".into();
        }
    }

    pub fn update(&mut self, current: SavedSettings, now: Instant, force: bool) {
        if current != self.observed {
            self.observed = current;
            self.changed_at = now;
            self.status = "更改等待自动保存".into();
        }
        if self.saved.as_ref() == Some(&self.observed)
            && self.saved_configurations.as_ref() == Some(&self.configurations)
        {
            if !self.failed {
                self.status = "已保存到本地".into();
            }
            return;
        }
        if !force
            && (now.duration_since(self.changed_at) < Duration::from_millis(500)
                || self
                    .attempted_at
                    .is_some_and(|last| now.duration_since(last) < Duration::from_secs(2)))
        {
            return;
        }
        self.attempted_at = Some(now);
        let result = self.save();
        match result {
            Ok(()) => {
                self.saved = Some(self.observed.clone());
                self.saved_configurations = Some(self.configurations.clone());
                self.status = "已保存到本地".into();
                self.failed = false;
                // A successful save shouldn't throttle the next real edit.
                self.attempted_at = None;
            }
            Err(error) => {
                self.status = format!("保存失败：{error}");
                self.failed = true;
            }
        }
    }

    fn save(&mut self) -> Result<(), String> {
        self.observed.validate()?;
        self.configurations.validate()?;
        if self.backup_before_write && self.path.exists() {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let backup = self.path.with_extension(format!("invalid-{nonce}.json"));
            let bytes = fs::read(&self.path).map_err(|e| e.to_string())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&backup)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            self.backup_before_write = false;
        }
        let document = SettingsDocument {
            current: self.observed.clone(),
            configurations: Some(self.configurations.clone()),
        };
        let bytes = serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?;
        write_bytes_atomic(&self.path, &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ocean-settings-test-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> PathBuf {
            self.0.join("settings.json")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            // Only files in this test's unique, explicitly created directory.
            if let Ok(entries) = fs::read_dir(&self.0) {
                for entry in entries.flatten() {
                    let _ = fs::remove_file(entry.path());
                }
            }
            let _ = fs::remove_dir(&self.0);
        }
    }
    #[test]
    fn round_trip_keeps_drafts_camera_and_linear_hdr_rgb() {
        let dir = Directory::new();
        let mut s = SavedSettings::default();
        s.draft.amplitude = 0.02;
        s.optics.sigma_t = [2.5, 0.000003, 0.0];
        s.optics.source = [0.0, 0.0089, 0.001];
        s.optics.sky.color = [0.5, 0.8, 0.9];
        s.optics.sky.intensity = 2.7;
        s.optics.sky.albedo = [0.1, 0.2, 0.3];
        s.optics.sky.steps = 32;
        s.optics.view = 9;
        s.optics.beach = true;
        s.optics.beach_slope = 0.23;
        s.optics.shore_x = -19.0;
        s.optics.model = super::super::terrain::ModelConfig {
            enabled: true,
            position: [8.0, 12.0, -3.0],
            scale: 0.75,
            yaw_deg: 31.0,
            sea_level: 1.3,
            center: [5.0, 10.0, -9.0],
            size: [160.0, 64.0, 96.0],
            open_edges: [false, true, true, false],
            sponge_width: 15.0,
            extension_width: 8.0,
            show_bounds: false,
        };
        s.optics.shore.water_foam_color = [0.7, 0.8, 0.9];
        s.optics.shore.order = 2;
        s.optics.shore.foam_velocity_threshold = 0.37;
        s.optics.shore.foam_velocity_transition = 1.25;
        s.optics.shore.foam_drag_enabled = false;
        s.optics.shore.foam_birth_speed = 0.65;
        s.optics.shore.foam_response_time = 12.0;
        s.optics.shore.foam_height_max = 0.12;
        s.optics.shore.foam_height_threshold = 0.3;
        s.optics.shore.foam_height_transition = 2.4;
        s.optics.shore.foam_texture = 3;
        s.optics.shore.foam_texture_size = 8.5;
        s.optics.shore.foam_micro_normal = 1.2;
        s.optics.shore.foam_opacity_scale = 6.5;
        s.optics.shore.foam_roughness = 0.75;
        s.optics.shore.foam_material_view = 4;
        s.optics.shore.colliders[3] = Some(super::super::collider::Collider {
            position: [-11.0, 3.0],
            height: 3.2,
            angle: 20.0,
            shape: super::super::collider::Shape::Box,
            ..Default::default()
        });
        s.optics.shore.filter_enabled = false;
        s.optics.shore.filter_cutoff = 3.0;
        s.optics.shore.filter_transition = 5.0;
        s.optics.shore.drying_time = 27.0;
        s.optics.shore.view = 4;
        s.camera.yaw = 1.6;
        s.camera.target = [-12.5, 8.0];
        s.camera.foam = false;
        s.paused = true;
        s.speed = 0.4;
        write_atomic(&dir.file(), &s).unwrap();
        assert_eq!(read(&dir.file()).unwrap().unwrap(), s);
        s.optics.depth = 17.0;
        write_atomic(&dir.file(), &s).unwrap();
        assert_eq!(read(&dir.file()).unwrap().unwrap(), s);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
    #[test]
    fn legacy_camera_defaults_to_origin_and_rejects_nonfinite_targets() {
        let dir = Directory::new();
        let mut legacy = serde_json::to_value(SavedSettings::default()).unwrap();
        legacy["camera"].as_object_mut().unwrap().remove("target");
        legacy["camera"]["yaw"] = serde_json::json!(1.2);
        fs::write(dir.file(), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut restored = read(&dir.file()).unwrap().unwrap();
        assert_eq!(restored.camera.target, [0.0; 2]);
        assert_eq!(restored.camera.yaw, 1.2);
        for target in [[f32::NAN, 0.0], [0.0, f32::INFINITY]] {
            restored.camera.target = target;
            assert!(restored.validate().is_err());
        }
    }

    #[test]
    fn idle_debounce_exit_flush_and_restart_restore() {
        let dir = Directory::new();
        let start = Instant::now();
        let (mut state, mut store) = SettingsStore::open(dir.file(), start);
        state.optics.depth = 8.0;
        store.update(state.clone(), start, false);
        assert!(!dir.file().exists());
        state.optics.depth = 9.0;
        store.update(state.clone(), start + Duration::from_millis(400), false);
        store.update(state.clone(), start + Duration::from_millis(700), false);
        assert!(!dir.file().exists());
        store.update(state.clone(), start + Duration::from_millis(901), false);
        assert_eq!(read(&dir.file()).unwrap().unwrap(), state);
        state.optics.depth = 12.0;
        store.update(state.clone(), start + Duration::from_millis(902), true);
        let (restored, _) = SettingsStore::open(dir.file(), start);
        assert_eq!(restored, state);
    }
    #[test]
    fn corrupt_or_unsupported_files_fall_back_and_are_backed_up() {
        let dir = Directory::new();
        let now = Instant::now();
        let corrupt = b"{unfinished json";
        fs::write(dir.file(), corrupt).unwrap();
        let (state, mut store) = SettingsStore::open(dir.file(), now);
        assert!(store.failed);
        assert_eq!(fs::read(dir.file()).unwrap(), corrupt);
        store.update(state.clone(), now, true);
        assert!(!store.failed);
        assert_eq!(read(&dir.file()).unwrap().unwrap(), state);
        let backup = fs::read_dir(&dir.0)
            .unwrap()
            .flatten()
            .find(|entry| entry.path() != dir.file())
            .unwrap();
        assert_eq!(fs::read(backup.path()).unwrap(), corrupt);
        let invalid = SavedSettings {
            version: 99,
            ..Default::default()
        };
        fs::write(dir.file(), serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(read(&dir.file()).is_err());
    }
    #[test]
    fn invalid_values_and_write_failure_preserve_last_saved_file() {
        let dir = Directory::new();
        let now = Instant::now();
        let valid = SavedSettings::default();
        write_atomic(&dir.file(), &valid).unwrap();
        let mut invalid = valid.clone();
        invalid.optics.sigma_t[0] = -1.0;
        assert!(write_atomic(&dir.file(), &invalid).is_err());
        assert_eq!(read(&dir.file()).unwrap().unwrap(), valid);
        let blocking_parent = dir.0.join("not-a-directory");
        fs::write(&blocking_parent, b"keep").unwrap();
        let (_, mut store) = SettingsStore::open(blocking_parent.join("settings.json"), now);
        store.update(valid, now, true);
        assert!(store.failed);
        assert_eq!(fs::read(blocking_parent).unwrap(), b"keep");
    }

    #[test]
    fn legacy_configuration_becomes_immutable_default_across_autosaves_and_restarts() {
        let dir = Directory::new();
        let now = Instant::now();
        let mut original = SavedSettings::default();
        original.optics.depth = 17.0;
        original.draft.seed = 42;
        original.camera.yaw = 1.2;
        write_atomic(&dir.file(), &original).unwrap();

        let (mut current, mut store) = SettingsStore::open(dir.file(), now);
        assert_eq!(current, original);
        assert_eq!(store.configurations().get(None).unwrap(), &original);
        current.optics.depth = 7.0;
        current.draft.seed = 99;
        store.update(current.clone(), now, true);
        assert!(!store.failed, "{}", store.status);

        let (restored, mut reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(restored, current);
        assert_eq!(reopened.configurations().get(None).unwrap(), &original);
        let default = reopened.configurations().get(None).unwrap().clone();
        reopened.update(default.clone(), now, true);
        let (restored, reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(restored, original);
        assert_eq!(reopened.configurations().get(None).unwrap(), &default);
    }

    #[test]
    fn named_profiles_rename_overwrite_and_persist_independently_of_session() {
        let dir = Directory::new();
        let now = Instant::now();
        let (initial, mut store) = SettingsStore::open(dir.file(), now);
        let mut library = store.configurations().clone();
        let mut beach = initial.clone();
        beach.optics.beach = true;
        beach.camera = Camera::beach(beach.ocean.length);
        beach.optics.shore.input_gain = 0.4;
        beach.draft.amplitude = 0.02;
        beach.paused = true;
        let first = library.save_as(" 沙滩方案 ", beach.clone()).unwrap();
        let second = library.save_as("远海", initial.clone()).unwrap();
        library.rename(Some(first), "浅水方案").unwrap();
        let mut revised = initial.clone();
        revised.optics.sky.intensity = 2.0;
        library.overwrite(Some(second), revised.clone()).unwrap();
        store.set_configurations(&library, now);
        // Named edits must be saved even when the session has not changed.
        store.update(initial.clone(), now, true);
        assert!(!store.failed, "{}", store.status);

        let (mut session, mut store) = SettingsStore::open(dir.file(), now);
        assert_eq!(session, initial);
        assert_eq!(store.configurations().profiles()[first].name(), "浅水方案");
        assert_eq!(store.configurations().get(Some(first)).unwrap(), &beach);
        assert_eq!(store.configurations().get(Some(second)).unwrap(), &revised);
        session.optics.shore.input_gain = 2.0;
        store.update(session.clone(), now, true);
        let (restored, store) = SettingsStore::open(dir.file(), now);
        assert_eq!(restored, session);
        assert_eq!(store.configurations(), &library);
    }

    #[test]
    fn default_and_invalid_profile_operations_never_modify_the_library() {
        let initial = SavedSettings::default();
        let mut library = ConfigurationLibrary::new(initial.clone());
        let index = library.save_as("方案一", initial.clone()).unwrap();
        let before = library.clone();
        assert!(library.rename(None, "新的默认").is_err());
        assert!(library.overwrite(None, initial.clone()).is_err());
        assert!(library.delete(None).is_err());
        assert!(library.delete(Some(99)).is_err());
        assert!(library.save_as("默认配置", initial.clone()).is_err());
        assert!(library.save_as(" 方案一 ", initial.clone()).is_err());
        assert!(library.save_as(" \t ", initial.clone()).is_err());
        assert!(library.rename(Some(index), "默认配置").is_err());
        assert!(library.rename(Some(99), "不存在").is_err());
        assert!(library.get(Some(99)).is_err());
        let mut invalid = initial;
        invalid.speed = 99.0;
        assert!(library.overwrite(Some(index), invalid.clone()).is_err());
        assert!(library.save_as("无效参数", invalid).is_err());
        assert_eq!(library, before);
        let other = library.save_as("方案二", SavedSettings::default()).unwrap();
        assert!(library.rename(Some(other), "方案一").is_err());
        assert_eq!(library.profiles()[other].name(), "方案二");
    }

    #[test]
    fn deleting_profile_preserves_other_snapshots_and_survives_restart() {
        let dir = Directory::new();
        let now = Instant::now();
        let (initial, mut store) = SettingsStore::open(dir.file(), now);
        let mut library = store.configurations().clone();
        library.save_as("删除的配置", initial.clone()).unwrap();
        let mut other = initial.clone();
        other.optics.depth = 17.0;
        library.save_as("保留的配置", other.clone()).unwrap();
        store.set_configurations(&library, now);
        store.update(initial.clone(), now, true);
        assert!(!store.failed);

        library.delete(Some(0)).unwrap();
        store.set_configurations(&library, now);
        store.update(initial.clone(), now, true);
        assert!(!store.failed);
        let (restored, reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(restored, initial);
        assert_eq!(reopened.configurations().profiles().len(), 1);
        assert_eq!(reopened.configurations().profiles()[0].name(), "保留的配置");
        assert_eq!(reopened.configurations().get(Some(0)).unwrap(), &other);
        assert_eq!(reopened.configurations().get(None).unwrap(), &initial);
        assert_eq!(reopened.configurations().active(), Some(0));
    }

    #[test]
    fn last_used_configuration_survives_restart_independently_of_unsaved_work() {
        let dir = Directory::new();
        let now = Instant::now();
        let (initial, mut store) = SettingsStore::open(dir.file(), now);
        let mut library = store.configurations().clone();
        let first = library.save_as("方案一", initial.clone()).unwrap();
        let second = library.save_as("方案二", initial.clone()).unwrap();
        store.set_configurations(&library, now);
        store.update(initial.clone(), now, true);
        assert!(!store.failed);

        // Identical parameters must not hide a change in the selected profile.
        library.select(Some(first)).unwrap();
        store.set_configurations(&library, now);
        store.update(initial.clone(), now, true);
        let (restored, mut reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(restored, initial);
        assert_eq!(reopened.configurations().active(), Some(first));

        // Remember edits to the working session without overwriting the named snapshot.
        let mut working = restored;
        working.optics.depth = 17.0;
        working.draft.seed = 123;
        library.rename(Some(first), "上次使用").unwrap();
        reopened.set_configurations(&library, now);
        reopened.update(working.clone(), now, true);
        let (restored, mut reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(restored, working);
        assert_eq!(reopened.configurations().active(), Some(first));
        assert_eq!(
            reopened.configurations().profiles()[first].name(),
            "上次使用"
        );
        assert_eq!(
            reopened.configurations().get(Some(first)).unwrap(),
            &initial
        );

        // Explicitly choosing the default must not select an identical named profile.
        library.select(None).unwrap();
        reopened.set_configurations(&library, now);
        reopened.update(initial.clone(), now, true);
        let (_, reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(reopened.configurations().active(), None);
        library.select(Some(second)).unwrap();
        library.delete(Some(second)).unwrap();
        assert_eq!(library.active(), None);
        let before = library.clone();
        assert!(library.select(Some(99)).is_err());
        assert_eq!(library, before);
    }

    #[test]
    fn missing_or_stale_last_selection_preserves_legacy_configurations() {
        let dir = Directory::new();
        let now = Instant::now();
        let initial = SavedSettings::default();
        let mut library = ConfigurationLibrary::new(initial.clone());
        library.save_as("原有配置", initial.clone()).unwrap();
        let mut document = serde_json::to_value(SettingsDocument {
            current: initial.clone(),
            configurations: Some(library),
        })
        .unwrap();
        document["configurations"]
            .as_object_mut()
            .unwrap()
            .remove("active");
        for stale in [false, true] {
            if stale {
                document["configurations"]["active"] = serde_json::json!(999);
            }
            fs::write(dir.file(), serde_json::to_vec(&document).unwrap()).unwrap();
            let (restored, store) = SettingsStore::open(dir.file(), now);
            assert!(!store.failed, "{}", store.status);
            assert_eq!(restored, initial);
            assert_eq!(store.configurations().active(), None);
            assert_eq!(store.configurations().profiles()[0].name(), "原有配置");
        }
    }

    #[test]
    fn profile_write_failure_is_reported_and_retry_preserves_profiles() {
        let dir = Directory::new();
        let now = Instant::now();
        let blocker = dir.0.join("blocker");
        fs::write(&blocker, b"keep").unwrap();
        let (initial, mut store) = SettingsStore::open(blocker.join("settings.json"), now);
        let mut library = store.configurations().clone();
        library.save_as("待保存方案", initial.clone()).unwrap();
        store.set_configurations(&library, now);
        store.update(initial.clone(), now, true);
        assert!(store.failed);
        assert_eq!(fs::read(&blocker).unwrap(), b"keep");
        // Retry on a writable path without losing the pending named snapshot.
        store.path = dir.file();
        store.update(initial, now + Duration::from_secs(3), false);
        assert!(!store.failed, "{}", store.status);
        let (_, reopened) = SettingsStore::open(dir.file(), now);
        assert_eq!(reopened.configurations(), &library);
    }

    #[test]
    fn cli_overrides_only_explicit_fields() {
        let mut s = SavedSettings::default();
        s.ocean.seed = 7;
        s.draft.seed = 9;
        s.optics.depth = 19.0;
        let ocean = OceanConfig {
            length: 256.0,
            ..Default::default()
        };
        s.apply_cli(
            ocean,
            OpticsConfig::default(),
            &["--length".into(), "--sigma-t".into()],
        );
        assert_eq!(s.ocean.seed, 7);
        assert_eq!(s.draft.seed, 9);
        assert_eq!(s.ocean.length, 256.0);
        assert_eq!(s.draft.length, 256.0);
        assert_eq!(s.optics.depth, 19.0);
        assert_eq!(s.camera.distance, Camera::default().distance * 2.0);
        let mut optics = OpticsConfig::default();
        optics.shore.order = 2;
        optics.shore.foam_opacity_scale = 8.0;
        s.optics.shore.foam_opacity_scale = 3.0;
        s.apply_cli(ocean, optics, &["--swe-order".into()]);
        assert_eq!(s.optics.shore.order, 2);
        assert_eq!(s.optics.shore.foam_opacity_scale, 3.0);
        s.apply_cli(ocean, optics, &["--foam-opacity-scale".into()]);
        assert_eq!(s.optics.shore.foam_opacity_scale, 8.0);
        s.optics.model.enabled = true;
        s.optics.model.center = [11.0, 2.0, -7.0];
        optics.model.size = [96.0, 48.0, 112.0];
        s.apply_cli(ocean, optics, &["--swe-size".into()]);
        assert!(s.optics.model.enabled);
        assert_eq!(s.optics.model.center, [11.0, 2.0, -7.0]);
        assert_eq!(s.optics.model.size, [96.0, 48.0, 112.0]);
        s.optics.spray.rate = 321.0;
        optics.spray.enabled = true;
        s.apply_cli(ocean, optics, &["--spray".into()]);
        assert!(s.optics.spray.enabled);
        assert_eq!(s.optics.spray.rate, 321.0);
        optics.spray.debug = true;
        s.apply_cli(ocean, optics, &["--spray-debug".into()]);
        assert!(s.optics.spray.debug);
        assert_eq!(s.optics.spray.rate, 321.0);
        s.apply_cli(ocean, OpticsConfig::beach_scene(), &["--beach".into()]);
        assert!(!s.optics.model.enabled);
    }
}

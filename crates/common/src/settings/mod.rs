//! Typed sessions and named snapshots. Paths and validation belong to the caller.
mod library;
pub mod sidecar;
pub use library::{ConfigurationLibrary, DEFAULT_CONFIGURATION_NAME, NamedConfiguration};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub trait Settings: Clone + Default + PartialEq + Serialize + DeserializeOwned {
    fn validate(&self) -> Result<(), String>;
}
#[derive(Serialize, Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned + Default"))]
pub struct SettingsDocument<T> {
    #[serde(flatten)]
    pub current: T,
    #[serde(default)]
    pub configurations: Option<ConfigurationLibrary<T>>,
}
pub fn default_path(app: &str, filename: &str) -> PathBuf {
    let repository = PathBuf::from("configs").join(filename);
    if repository.is_file() {
        return repository;
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(app).join(filename)
}
fn read_document<T: Settings>(path: &Path) -> Result<Option<SettingsDocument<T>>, String> {
    let bytes = match fs::read(path) {
        Ok(v) => v,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let mut doc: SettingsDocument<T> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    doc.current.validate()?;
    if let Some(library) = &mut doc.configurations {
        library.restore_selection();
        library.validate()?;
    }
    Ok(Some(doc))
}
/// Strict read-only loading never repairs or backs up a broken input.
pub fn load_config<T: Settings>(path: &Path) -> Result<T, String> {
    read_document(path)?
        .map(|d| d.current)
        .ok_or_else(|| format!("configuration does not exist: {}", path.display()))
}
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(".settings-{}-{nonce}.tmp", std::process::id()));
    let result = (|| -> io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(|e| e.to_string())
}
pub struct SettingsStore<T> {
    pub path: PathBuf,
    pub status: String,
    pub failed: bool,
    observed: T,
    saved: Option<T>,
    configurations: ConfigurationLibrary<T>,
    saved_configurations: Option<ConfigurationLibrary<T>>,
    backup_before_write: bool,
}
impl<T: Settings> SettingsStore<T> {
    pub fn open(path: PathBuf) -> (T, Self) {
        let mut saved_configurations = None;
        let (initial, saved, status, failed, backup_before_write) = match read_document::<T>(&path)
        {
            Ok(Some(doc)) => {
                saved_configurations = doc.configurations;
                (
                    doc.current.clone(),
                    Some(doc.current),
                    "已载入本地配置".into(),
                    false,
                    false,
                )
            }
            Ok(None) => (T::default(), None, "新配置，尚未保存".into(), false, false),
            Err(e) => (
                T::default(),
                None,
                format!("载入失败：{e}；使用默认配置"),
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
            backup_before_write,
        };
        (initial, store)
    }
    pub fn configurations(&self) -> &ConfigurationLibrary<T> {
        &self.configurations
    }
    pub fn set_configurations(&mut self, library: &ConfigurationLibrary<T>) {
        if &self.configurations != library {
            self.configurations = library.clone();
            self.refresh();
        }
    }
    fn refresh(&mut self) {
        if !self.failed {
            self.status = if self.saved.as_ref() == Some(&self.observed)
                && self.saved_configurations.as_ref() == Some(&self.configurations)
            {
                "无未保存的更改"
            } else {
                "有未保存的更改"
            }
            .into();
        }
    }
    pub fn update(&mut self, current: T, save_requested: bool) {
        if current != self.observed {
            self.observed = current;
            self.refresh();
        }
        if !save_requested {
            return;
        }
        match self.save() {
            Ok(()) => {
                self.saved = Some(self.observed.clone());
                self.saved_configurations = Some(self.configurations.clone());
                self.status = "保存成功".into();
                self.failed = false;
            }
            Err(e) => {
                self.status = format!("保存失败：{e}");
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
                .open(backup)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            self.backup_before_write = false;
        }
        let doc = SettingsDocument {
            current: self.observed.clone(),
            configurations: Some(self.configurations.clone()),
        };
        write_bytes_atomic(
            &self.path,
            &serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?,
        )
    }
}

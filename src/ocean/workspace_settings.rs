//! Layout sidecars are independent from simulation sessions and named presets.
use super::workspace::{Layout, Workspace, WorkspaceId, Workspaces};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Serialize)]
struct Document {
    version: u32,
    active: WorkspaceId,
    workspaces: BTreeMap<String, Layout>,
}
impl Document {
    fn capture(workspaces: &Workspaces) -> Self {
        Self {
            version: 1,
            active: workspaces.active,
            workspaces: WorkspaceId::ALL
                .into_iter()
                .map(|id| (id.key().into(), workspaces.items[id.index()].layout.clone()))
                .collect(),
        }
    }
}

pub struct WorkspaceStore {
    path: PathBuf,
    pub error: Option<String>,
    pub blocked: bool,
    saved: Option<Document>,
    observed: Option<Document>,
    checked_at: Instant,
    changed_at: Instant,
    attempted_at: Option<Instant>,
}
impl WorkspaceStore {
    pub fn open(session: &Path, workspaces: &mut Workspaces) -> Self {
        let stem = session.file_stem().unwrap_or_default().to_string_lossy();
        let path = session.with_file_name(format!("{stem}.workspaces.json"));
        let now = Instant::now();
        let mut store = Self {
            path,
            error: None,
            blocked: false,
            saved: None,
            observed: None,
            checked_at: now,
            changed_at: now,
            attempted_at: None,
        };
        match store.read(workspaces) {
            Ok(true) => {
                store.saved = Some(Document::capture(workspaces));
                store.observed = store.saved.clone();
            }
            Ok(false) => {}
            Err(error) => {
                store.error = Some(format!("布局读取失败：{error}；已保留原文件"));
                store.blocked = true;
            }
        }
        store
    }
    fn read(&self, workspaces: &mut Workspaces) -> Result<bool, String> {
        let metadata = match fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        };
        if metadata.len() > 1024 * 1024 {
            return Err("文件超过 1 MiB".into());
        }
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&self.path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if value["version"].as_u64() != Some(1) {
            return Err("不支持的布局版本".into());
        }
        let entries = value["workspaces"].as_object().ok_or("缺少工作区布局")?;
        let mut errors = Vec::new();
        for id in WorkspaceId::ALL {
            if let Some(value) = entries.get(id.key()) {
                match serde_json::from_value::<Layout>(value.clone())
                    .map_err(|e| e.to_string())
                    .and_then(Layout::validate)
                {
                    Ok(layout) => workspaces.items[id.index()] = Workspace::new(layout),
                    Err(error) => errors.push(format!("{}：{error}", id.name())),
                }
            }
        }
        if let Some(active) = value.get("active") {
            match serde_json::from_value(active.clone()) {
                Ok(id) => workspaces.active = id,
                Err(_) => errors.push("未知工作区，已选择场景".into()),
            }
        }
        if !errors.is_empty() {
            return Err(errors.join("；"));
        }
        Ok(true)
    }
    /// Only explicit reset/repair authorizes replacement of a damaged sidecar.
    pub fn allow_repair(&mut self) {
        self.blocked = false;
        self.error = None;
        self.saved = None;
        self.attempted_at = None;
    }
    pub fn update(&mut self, workspaces: &Workspaces, now: Instant, force: bool) {
        if self.blocked
            || (!force && now.duration_since(self.checked_at) < Duration::from_millis(500))
        {
            return;
        }
        self.checked_at = now;
        let current = Document::capture(workspaces);
        if self.observed.as_ref() != Some(&current) {
            self.observed = Some(current.clone());
            self.changed_at = now;
        }
        if self.saved.as_ref() == Some(&current) {
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
        match self.write(&current) {
            Ok(()) => {
                self.saved = Some(current);
                self.error = None;
                self.attempted_at = None;
            }
            Err(error) => self.error = Some(format!("布局保存失败：{error}")),
        }
    }
    fn write(&self, document: &Document) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp = parent.join(format!(
            ".ocean-workspaces-{}-{nonce}.tmp",
            std::process::id()
        ));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sidecar_roundtrip_isolated_from_session_and_corruption_requires_permission() {
        let dir = std::env::temp_dir().join(format!(
            "ocean-workspace-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let session = dir.join("personal.json");
        fs::write(&session, b"untouched simulation and profiles").unwrap();
        let mut workspaces = Workspaces {
            active: WorkspaceId::Shallow,
            ..Default::default()
        };
        workspaces
            .current()
            .add(super::super::workspace::Panel::Performance);
        let mut store = WorkspaceStore::open(&session, &mut workspaces);
        store.update(&workspaces, Instant::now(), true);
        assert!(store.error.is_none());
        let mut restored = Workspaces::default();
        let read = WorkspaceStore::open(&session, &mut restored);
        assert!(read.error.is_none());
        assert_eq!(restored.active, WorkspaceId::Shallow);
        assert!(
            restored
                .current()
                .contains(super::super::workspace::Panel::Performance)
        );
        assert_eq!(Document::capture(&restored), Document::capture(&workspaces));
        fs::write(&store.path, b"broken layout").unwrap();
        let mut damaged = WorkspaceStore::open(&session, &mut Workspaces::default());
        assert!(damaged.blocked);
        damaged.update(&restored, Instant::now(), true);
        assert_eq!(fs::read(&store.path).unwrap(), b"broken layout");
        damaged.allow_repair();
        damaged.update(&restored, Instant::now(), true);
        assert!(damaged.error.is_none());
        assert_eq!(
            fs::read(session).unwrap(),
            b"untouched simulation and profiles"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}

//! Ocean layout versions; generic sidecar I/O, throttling and errors live in common.
use super::{Layout, Workspace, WorkspaceId, Workspaces};
use serde::Serialize;
use std::{collections::BTreeMap, path::Path, time::Instant};
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Document {
    version: u32,
    active: WorkspaceId,
    workspaces: BTreeMap<String, Layout>,
}
impl Document {
    fn capture(w: &Workspaces) -> Self {
        Self {
            version: 3,
            active: w.active,
            workspaces: WorkspaceId::ALL
                .into_iter()
                .map(|id| (id.key().into(), w.items[id.index()].layout.clone()))
                .collect(),
        }
    }
}
pub struct WorkspaceStore {
    inner: common::settings::sidecar::Sidecar<Document>,
}
impl std::ops::Deref for WorkspaceStore {
    type Target = common::settings::sidecar::Sidecar<Document>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl std::ops::DerefMut for WorkspaceStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
impl WorkspaceStore {
    pub fn open(session: &Path, workspaces: &mut Workspaces) -> Self {
        let mut store = Self {
            inner: common::settings::sidecar::Sidecar::new(session),
        };
        match store.read(workspaces) {
            Ok(true) => store.inner.loaded(Document::capture(workspaces)),
            Ok(false) => {}
            Err(e) => store.inner.read_failed(e),
        }
        store
    }
    fn read(&self, workspaces: &mut Workspaces) -> Result<bool, String> {
        let Some(value) = self.inner.read_json()? else {
            return Ok(false);
        };
        let version = value["version"].as_u64();
        if !matches!(version, Some(1..=3)) {
            return Err("Unsupported layout version".into());
        }
        let entries = value["workspaces"]
            .as_object()
            .ok_or("Missing workspace layouts")?;
        let mut errors = Vec::new();
        for id in WorkspaceId::ALL {
            if let Some(value) = entries.get(id.key()) {
                match serde_json::from_value::<Layout>(value.clone())
                    .map_err(|e| e.to_string())
                    .and_then(Layout::validate)
                {
                    Ok(mut layout) => {
                        if version == Some(1) && id == WorkspaceId::Shading {
                            for p in [super::Panel::Caustics, super::Panel::Refraction] {
                                layout.insert_near(super::Panel::Optics, p);
                            }
                        }
                        if matches!(version, Some(1 | 2)) && id == WorkspaceId::Shading {
                            layout.insert_near(super::Panel::Optics, super::Panel::Reflection);
                        }
                        workspaces.items[id.index()] = Workspace::new(layout);
                    }
                    Err(e) => errors.push(format!("{}：{e}", id.name())),
                }
            }
        }
        if let Some(active) = value.get("active") {
            match serde_json::from_value(active.clone()) {
                Ok(id) => workspaces.active = id,
                Err(_) => errors.push("Unknown workspace; Scene selected".into()),
            }
        }
        if errors.is_empty() {
            Ok(true)
        } else {
            Err(errors.join("；"))
        }
    }
    pub fn update(&mut self, w: &Workspaces, now: Instant, force: bool) {
        self.inner.update(Document::capture(w), now, force);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
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
            active: WorkspaceId::AnimatedSdf,
            ..Default::default()
        };
        use crate::ocean::ui::workspace::Panel;
        workspaces.current().add(Panel::Performance);
        let mut store = WorkspaceStore::open(&session, &mut workspaces);
        store.update(&workspaces, Instant::now(), true);
        assert!(store.error.is_none());
        let mut restored = Workspaces::default();
        let read = WorkspaceStore::open(&session, &mut restored);
        assert!(read.error.is_none());
        assert_eq!(restored.active, WorkspaceId::AnimatedSdf);
        assert!(restored.current().contains(Panel::Performance));
        assert_eq!(Document::capture(&restored), Document::capture(&workspaces));
        let mut legacy = serde_json::to_value(Document::capture(&restored)).unwrap();
        legacy["version"] = serde_json::json!(1);
        legacy["active"] = serde_json::json!("shallow");
        legacy["workspaces"]
            .as_object_mut()
            .unwrap()
            .remove("animated_sdf");
        let shading = super::super::default_layout(WorkspaceId::Shading)
            .filtered(&|p| !matches!(p, Panel::Caustics | Panel::Refraction))
            .unwrap();
        legacy["workspaces"]["shading"] = serde_json::to_value(&shading).unwrap();
        fs::write(&store.path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut upgraded = Workspaces::default();
        let mut upgraded_store = WorkspaceStore::open(&session, &mut upgraded);
        assert!(!upgraded_store.blocked);
        assert_eq!(upgraded.active, WorkspaceId::Shallow);
        let animated = &upgraded.items[WorkspaceId::AnimatedSdf.index()];
        assert!(animated.contains(Panel::AnimatedSdf) && animated.contains(Panel::View));
        assert!(!animated.contains(Panel::Performance));
        let ws = &mut upgraded.items[WorkspaceId::Shading.index()];
        assert!(ws.contains(Panel::Caustics) && ws.contains(Panel::Refraction));
        assert_eq!(
            ws.layout
                .filtered(&|p| !matches!(p, Panel::Caustics | Panel::Refraction))
                .unwrap(),
            shading
        );
        ws.prepare(&crate::ocean::parameters::optics::OpticsConfig::default());
        let tab = ws.dock.find_tab(&Panel::Caustics).unwrap();
        ws.dock.remove_tab(tab);
        ws.capture();
        upgraded_store.update(&upgraded, Instant::now(), true);
        let mut reopened = Workspaces::default();
        WorkspaceStore::open(&session, &mut reopened);
        assert!(!reopened.items[WorkspaceId::Shading.index()].contains(Panel::Caustics));
        let mut legacy = serde_json::to_value(Document::capture(&reopened)).unwrap();
        legacy["version"] = serde_json::json!(2);
        let shading = reopened.items[WorkspaceId::Shading.index()]
            .layout
            .filtered(&|p| p != Panel::Reflection)
            .unwrap();
        legacy["workspaces"]["shading"] = serde_json::to_value(&shading).unwrap();
        fs::write(&store.path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let mut upgraded = Workspaces::default();
        let mut upgraded_store = WorkspaceStore::open(&session, &mut upgraded);
        let ws = &mut upgraded.items[WorkspaceId::Shading.index()];
        assert!(ws.contains(Panel::Reflection));
        assert_eq!(
            ws.layout.filtered(&|p| p != Panel::Reflection).unwrap(),
            shading
        );
        ws.prepare(&crate::ocean::parameters::optics::OpticsConfig::default());
        let tab = ws.dock.find_tab(&Panel::Reflection).unwrap();
        ws.dock.remove_tab(tab);
        ws.capture();
        upgraded_store.update(&upgraded, Instant::now(), true);
        let mut reopened = Workspaces::default();
        WorkspaceStore::open(&session, &mut reopened);
        assert!(!reopened.items[WorkspaceId::Shading.index()].contains(Panel::Reflection));
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

//! Throttled automatic layout saving, independent of manually saved sessions.
use super::write_bytes_atomic;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub struct Sidecar<T> {
    pub path: PathBuf,
    pub error: Option<String>,
    pub blocked: bool,
    saved: Option<T>,
    observed: Option<T>,
    checked_at: Instant,
    changed_at: Instant,
    attempted_at: Option<Instant>,
}
impl<T: Clone + PartialEq + Serialize> Sidecar<T> {
    pub fn new(session: &Path) -> Self {
        let stem = session.file_stem().unwrap_or_default().to_string_lossy();
        let now = Instant::now();
        Self {
            path: session.with_file_name(format!("{stem}.workspaces.json")),
            error: None,
            blocked: false,
            saved: None,
            observed: None,
            checked_at: now,
            changed_at: now,
            attempted_at: None,
        }
    }
    pub fn loaded(&mut self, value: T) {
        self.saved = Some(value.clone());
        self.observed = Some(value);
    }
    pub fn read_failed(&mut self, error: String) {
        self.error = Some(format!(
            "Failed to load layout: {error}; original file preserved"
        ));
        self.blocked = true;
    }
    pub fn read_json(&self) -> Result<Option<serde_json::Value>, String> {
        let metadata = match std::fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        if metadata.len() > 1024 * 1024 {
            return Err("File exceeds 1 MiB".into());
        }
        serde_json::from_slice(&std::fs::read(&self.path).map_err(|e| e.to_string())?)
            .map(Some)
            .map_err(|e| e.to_string())
    }
    pub fn allow_repair(&mut self) {
        self.blocked = false;
        self.error = None;
        self.saved = None;
        self.attempted_at = None;
    }
    pub fn update(&mut self, current: T, now: Instant, force: bool) {
        if self.blocked
            || (!force && now.duration_since(self.checked_at) < Duration::from_millis(500))
        {
            return;
        }
        self.checked_at = now;
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
                    .is_some_and(|t| now.duration_since(t) < Duration::from_secs(2)))
        {
            return;
        }
        self.attempted_at = Some(now);
        match serde_json::to_vec_pretty(&current)
            .map_err(|e| e.to_string())
            .and_then(|v| write_bytes_atomic(&self.path, &v))
        {
            Ok(()) => {
                self.saved = Some(current);
                self.error = None;
                self.attempted_at = None;
            }
            Err(e) => self.error = Some(format!("Failed to save layout: {e}")),
        }
    }
}

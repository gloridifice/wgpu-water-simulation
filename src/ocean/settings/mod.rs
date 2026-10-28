//! Working sessions and named configurations; Dock sidecars remain UI-owned.
mod library;
mod session;
mod store;
pub use library::{ConfigurationLibrary, DEFAULT_CONFIGURATION_NAME, NamedConfiguration};
pub use session::SavedSettings;
pub use store::{SettingsStore, default_path, load_config};

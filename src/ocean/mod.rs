//! GPU ocean organized by application, simulation, scene, rendering and UI; legacy public paths remain re-exported.
pub type Error = Box<dyn std::error::Error>;

pub mod app;
pub mod camera;
pub mod gpu;
pub mod labs;
pub mod parameters;
pub mod render;
pub mod scene;
pub mod settings;
pub mod simulation;
mod ui;
pub mod verify;

// Compatibility exports preserve the existing library API without runtime wrappers.
pub use labs::foam_weight_lab;
pub use render::optics;
pub mod skylight {
    pub use super::render::skylight::*;
    pub use super::ui::panels::optics::depth_graph;
}
pub use scene::{collider, model, terrain};
pub use simulation::{nearshore as shore, offshore::foam, spectral::spectrum, spray};

pub mod viewer {
    pub use super::{app::snapshot::snapshot, app::viewer::run, camera::Camera};
}

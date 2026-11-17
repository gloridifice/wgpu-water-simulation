//! Reusable desktop/GPU infrastructure for simulation applications.
//! No simulation parameters, business panels or application-specific paths live here.
pub type Error = Box<dyn std::error::Error>;
pub mod app;
pub mod camera;
#[cfg(feature = "mcp")]
pub mod control;
pub mod gpu;
pub mod settings;
pub mod ui;

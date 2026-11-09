//! Desktop/GPU infrastructure shared by the Ocean and fog applications.
//! No simulation parameters, business panels or application-specific paths live here.
pub type Error = Box<dyn std::error::Error>;
pub mod app;
pub mod camera;
pub mod gpu;
pub mod settings;
pub mod ui;

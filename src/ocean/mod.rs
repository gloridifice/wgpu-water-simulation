//! 频谱海洋：CPU 只初始化随机波谱，GPU 负责时间演化、二维逆 FFT 和绘制。
pub mod gpu;
pub mod spectrum;
pub mod verify;
pub mod viewer;

pub(crate) mod gpu_timer;
mod gui;
mod gui_controls;
mod panel_help;
mod parameter_panels;
mod performance;
mod theme;
mod workspace;
mod workspace_settings;

pub mod collider;
mod collider_panel;
mod color_editor;
mod configuration_panel;
pub mod foam;
mod foam_texture;
pub mod foam_weight_lab;
pub mod model;
mod model_panel;
mod model_render;
pub mod optics;
mod sand;
mod seabed;
pub mod settings;
pub mod shore;
pub mod skylight;
pub mod spray;
mod spray_panel;
pub mod terrain;

mod optics_verify;
mod skylight_verify;

//! 频谱海洋：CPU 只初始化随机波谱，GPU 负责时间演化、二维逆 FFT 和绘制。
pub mod gpu;
pub mod spectrum;
pub mod verify;
pub mod viewer;

mod gui;

mod color_editor;
pub mod foam;
pub mod optics;
mod seabed;
pub mod settings;
pub mod shore;
pub mod skylight;

mod optics_verify;
mod skylight_verify;

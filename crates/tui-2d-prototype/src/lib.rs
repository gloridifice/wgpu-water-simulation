//! 与界面无关的二维 PIC 与一维浅水方程求解库。
pub mod pic;
pub mod solver;

pub use solver::{Cell, Config, Flux, Simulation, rusanov_flux};

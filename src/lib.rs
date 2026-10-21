//! 与界面无关的二维 PIC 与一维浅水方程求解库。
pub mod pic;
pub mod solver;

// 保留原有导入路径；新代码也可以明确使用 water_simulation::solver。
pub use solver::{Cell, Config, Flux, Simulation, rusanov_flux};

#[cfg(feature = "ocean")]
pub mod ocean;

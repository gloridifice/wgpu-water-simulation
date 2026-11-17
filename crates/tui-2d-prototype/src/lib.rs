//! UI-independent two-dimensional PIC and one-dimensional shallow-water solvers.
pub mod pic;
pub mod solver;

pub use solver::{Cell, Config, Flux, Simulation, rusanov_flux};

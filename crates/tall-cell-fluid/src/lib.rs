//! Independent reference implementation of three-dimensional incompressible liquid.
//!
//! Explicit data structures connect MAC velocities, the liquid level set,
//! quadtree/tall cells, optical layers, pressure projection, and surface extraction.
//! This library does not depend on the main project's window or ocean modules.
//!
//! ```
//! use tall_cell_fluid::{Domain, Grid, Simulation, SimulationConfig};
//! use tall_cell_fluid::surface::extract_surface;
//!
//! let mut grid = Grid::new(Domain::new([8, 16, 8], 0.1).unwrap());
//! grid.set_liquid_sdf(|p| p[1] - 0.8);
//! let mut simulation = Simulation::from_grid(grid, SimulationConfig::default()).unwrap();
//! let report = simulation.step(0.01).unwrap();
//! assert!(report.projection.converged);
//! let mesh = extract_surface(&simulation.grid).unwrap();
//! assert!(!mesh.indices.is_empty());
//! ```

pub mod adaptivity;
#[cfg(feature = "demo")]
pub mod demo;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod grid;
pub mod optics;
pub mod pressure;
pub mod simulation;
pub mod surface;

pub use grid::{Domain, Grid, MacVelocity, Vec3};
pub use simulation::{Simulation, SimulationConfig, StepReport};

//! Device infrastructure. Legacy exports preserve existing library callers.
pub mod context;
pub(in crate::ocean) mod timer;
pub use crate::ocean::Error;
pub use crate::ocean::camera::CameraUniform;
pub use crate::ocean::render::Renderer;
pub use crate::ocean::simulation::offshore::Ocean;
pub use context::Gpu;

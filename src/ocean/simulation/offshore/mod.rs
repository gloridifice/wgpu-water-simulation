pub mod foam;
pub(in crate::ocean) mod verification;
// Compatibility only; internal wave consumers use simulation::spectral.
pub use super::spectral::{FftWaves as Ocean, spectrum};

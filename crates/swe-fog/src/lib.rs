//! Independent low-gravity 2D SWE fog. Desktop plumbing comes from common.
pub use common::Error;
pub mod app;
pub mod interactions;
pub mod parameters;
pub mod render;
pub mod scene;
pub mod simulation;
pub mod terrain;
pub mod verify;

//! Shared water material for open ocean and bounded procedural water.
//! Geometry, wet masks and foam remain scene adapters; optical composition is unique.
pub(in crate::ocean) const SHADER: &str = concat!(
    include_str!("shaders/lighting.wgsl"),
    "\n",
    include_str!("shaders/material.wgsl"),
);

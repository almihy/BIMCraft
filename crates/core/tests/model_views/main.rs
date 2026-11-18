//! The core's tests: the model's drawings driven through the Session: plans, sections, the 3D view, graphics, sheets. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod model_tests;

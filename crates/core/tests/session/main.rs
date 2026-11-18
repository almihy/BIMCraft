//! The core's tests: the Session driven through its commands: the engine's own tests, the control port, cameras, the samples and the self-checks. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod bridge_tests;
#[cfg(test)]
mod camera_tests;
#[cfg(test)]
mod commands_max_file_tests;
#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod sample_tests;
#[cfg(test)]
mod selfcheck_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tower_tests;

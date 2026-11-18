//! The core's tests: editing: warnings, the clipboard, pins, arrays, purge, read-only sessions, dry runs, live merge and its settings, autosave. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod align_tests;
#[cfg(test)]
mod array_tests;
#[cfg(test)]
mod autosave_tests;
#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod dry_run_tests;
#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod pin_tests;
#[cfg(test)]
mod purge_tests;
#[cfg(test)]
mod read_only_tests;
#[cfg(test)]
mod sync_room_tests;
#[cfg(test)]
mod sync_settings_tests;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tempdims_tests;
#[cfg(test)]
mod type_data_tests;
#[cfg(test)]
mod view3d_update_tests;
#[cfg(test)]
mod warnings_tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../common/graphics_testing.rs"]
#[cfg(test)]
mod graphics_testing;

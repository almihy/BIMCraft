//! The core's tests: the site, car parks, point clouds and rooms. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod cloud_tests;
#[cfg(test)]
mod georef_tests;
#[cfg(test)]
mod masterplan_tests;
#[cfg(test)]
mod parking_tests;
#[cfg(test)]
mod room_area_tests;
#[cfg(test)]
mod room_bounds_tests;
#[cfg(test)]
mod room_freeze_tests;
#[cfg(test)]
mod room_height_tests;
#[cfg(test)]
mod room_of_tests;
#[cfg(test)]
mod site_plan_tests;
#[cfg(test)]
mod site_revit_tests;
#[cfg(test)]
mod workplane_tests;

//! The core's tests: families, doors and windows, shapes and element solids, and IFC's classes and geometry. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod door_leaf_tests;
#[cfg(test)]
mod door_tests;
#[cfg(test)]
mod element_solids_tests;
#[cfg(test)]
mod family_drawing_tests;
#[cfg(test)]
mod family_engine_tests;
#[cfg(test)]
mod family_last_four_tests;
#[cfg(test)]
mod family_parametric_tests;
#[cfg(test)]
mod family_revit_tests;
#[cfg(test)]
mod family_rpc_tests;
#[cfg(test)]
mod family_sample_tests;
#[cfg(test)]
mod flip_tests;
#[cfg(test)]
mod ifc_colour_tests;
#[cfg(test)]
mod ifc_generic_tests;
#[cfg(test)]
mod ifc_geometry_tests;
#[cfg(test)]
mod ifc_member_tests;
#[cfg(test)]
mod shape_tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../common/graphics_testing.rs"]
#[cfg(test)]
mod graphics_testing;

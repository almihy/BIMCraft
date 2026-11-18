//! The core's tests: structure, reinforcement and MEP, their calculations, and the walls, floors and roofs they join. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod calc_tests;
#[cfg(test)]
mod connector_tests;
#[cfg(test)]
mod eberick_tests;
#[cfg(test)]
mod electrical_tests;
#[cfg(test)]
mod glue_tests;
#[cfg(test)]
mod mep_tests;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod openings_past_top_tests;
#[cfg(test)]
mod quantities_tests;
#[cfg(test)]
mod rebar_tests;
#[cfg(test)]
mod reinforce_tests;
#[cfg(test)]
mod roof_slopes_tests;
#[cfg(test)]
mod run_join_tests;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod wall_top_offset_tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../common/graphics_testing.rs"]
#[cfg(test)]
mod graphics_testing;

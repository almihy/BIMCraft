//! The core's tests: plans, sections, sheets, dimensions, text and the CAD, PDF and image imports, driven through commands. Each group of the core's tests is a test crate of its own (`tests/<group>/`), so the groups
//! compile side by side; this root names the engine's modules as the library's root does (`crate::model`,
//! `crate::engine`), so a test reads as it would inside the library.

pub use bimcraft_core::*;
// What the library keeps to itself at its root, which tests name by the same paths.
#[allow(unused_imports)]
pub use bimcraft_model::{component_geometry, constraints, fixtures, sync_settings, timber};

#[cfg(test)]
mod cad_tests;
#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod dxf_out_tests;
#[cfg(test)]
mod image_tests;
#[cfg(test)]
mod line_tests;
#[cfg(test)]
mod pdf_fidelity_tests;
#[cfg(test)]
mod pdf_layers_tests;
#[cfg(test)]
mod plan_view_tests;
#[cfg(test)]
mod section_box_tests;
#[cfg(test)]
mod section_cut_tests;
#[cfg(test)]
mod text_tests;
#[cfg(test)]
mod titleblock_tests;

#[cfg(test)]
#[allow(dead_code)]
#[path = "../common/graphics_testing.rs"]
#[cfg(test)]
mod graphics_testing;

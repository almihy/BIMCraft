//! The model's own modules driven through the Session: IFC, phasing, the library, sketches, rooms.

mod alignment_why_tests;
mod component_geometry;
mod constraints;
mod crosscheck;
mod derived;
mod ids_journal_tests;
mod ifc;
mod import;
mod insert;
mod library;
mod link;
mod model;
mod phasing;
mod rooms;
mod selection;
mod sketch_command_tests;
mod sketch_hostile_tests;
mod symbol_set_tests;
mod wallchain;
mod walls;
mod walls_index;

#[allow(dead_code)]
#[path = "../../common/graphics_testing.rs"]
pub(crate) mod graphics_testing;

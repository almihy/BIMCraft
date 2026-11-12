//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

use bimcraft_geom::{csg, font};
pub use bimcraft_geom::{files, geom, ifc_classes, language, prim, shared, skeleton, step, triangulation, units, xlsx};
pub use bimcraft_mep_calc::{conduit, demand, dps, dr, drainage, hydrant, loads, nbr5410, plumbing, reservoir, storm, water};
pub mod align;
#[cfg(test)]
mod align_tests;
pub mod alignment;
#[cfg(test)]
mod array_tests;
pub mod autosave;
#[cfg(test)]
mod autosave_tests;
pub mod background_grid;
pub mod bounds;
pub mod bridge;
pub mod cad;
#[cfg(test)]
mod cad_tests;
#[cfg(test)]
mod calc_tests;
pub mod camera;
#[cfg(test)]
mod clipboard_tests;
pub mod cloud;
#[cfg(test)]
mod cloud_tests;
mod commands;
mod component_geometry;
mod constraints;
pub mod crosscheck;
pub mod curves;
pub mod derived;
pub mod dims;
pub mod dims3d;
pub mod draw;
#[cfg(test)]
mod dry_run_tests;
pub mod dxf;
#[cfg(test)]
mod eberick_tests;
pub mod electrical;
#[cfg(test)]
mod electrical_tests;
pub mod engine;
pub mod falls;
pub mod family;
pub mod favorites;
#[cfg(test)]
mod files_tests;
pub mod fills;
mod fixtures;
#[cfg(test)]
mod georef_tests;
pub mod gltf;
pub mod glue;
#[cfg(test)]
mod glue_tests;
pub mod graphics;
pub mod groups;
pub mod ids;
pub mod ifc;
#[cfg(test)]
mod ifc_colour_tests;
#[cfg(test)]
mod ifc_generic_tests;
#[cfg(test)]
mod ifc_geometry_tests;
#[cfg(test)]
mod ifc_member_tests;
#[cfg(test)]
mod image_tests;
pub mod import;
pub mod inquiry;
pub mod insert;
pub mod json;
pub mod lenient;
pub mod library;
pub mod lines;
pub mod link;
pub mod massing;
pub mod masterplan;
#[cfg(test)]
mod masterplan_tests;
pub mod model;
pub mod motion;
pub mod network;
#[cfg(test)]
mod openings_past_top_tests;
#[cfg(test)]
mod oracle_tests;
pub mod paint;
pub mod pat;
pub mod pdf;
pub mod pen;
pub mod phasing;
#[cfg(test)]
mod pin_tests;
pub mod quantities;
#[cfg(test)]
mod quantities_tests;
pub mod raster;
#[cfg(test)]
mod read_only_tests;
pub mod rebar;
#[cfg(test)]
mod rebar_tests;
pub mod reinforce;
#[cfg(test)]
mod reinforce_tests;
pub mod render;
#[cfg(test)]
mod roof_slopes_tests;
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
pub mod rooms;
#[cfg(test)]
mod run_join_tests;
pub mod runs;
pub mod sample;
pub mod schedule;
#[cfg(test)]
mod section_box_tests;
pub mod selection;
#[cfg(test)]
mod shape_tests;
pub mod sheet;
pub mod site;
#[cfg(test)]
mod site_plan_tests;
pub mod sketch;
pub mod snap;
pub mod stairs;
pub mod standards;
pub mod styles;
pub mod symbol_set;
pub mod sync;
#[cfg(test)]
mod sync_room_tests;
mod sync_settings;
#[cfg(test)]
mod sync_settings_tests;
#[cfg(test)]
mod sync_tests;
pub mod tempdims;
#[cfg(test)]
mod tempdims_tests;
pub mod text;
pub mod texture;
mod timber;
pub mod titleblock;
#[cfg(test)]
mod titleblock_tests;
pub mod triad;
pub mod type_data;
#[cfg(test)]
mod type_data_tests;
pub mod view3d;
#[cfg(test)]
mod wall_top_offset_tests;
pub mod wallchain;
pub mod walls;
pub mod warnings;
#[cfg(test)]
mod warnings_tests;
pub mod workplane;
#[cfg(test)]
mod workplane_tests;

pub use commands::{command_of, floor_outline, opening_of, roof_footprint, run_category_of, runs_of, stood_of};
pub use json::{element_json, element_json_bare, element_json_with};

#[cfg(test)]
mod connector_tests;
#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod door_tests;
#[cfg(test)]
mod flip_tests;
#[cfg(test)]
mod line_tests;
#[cfg(test)]
mod mep_tests;
#[cfg(test)]
mod network_tests;
#[cfg(test)]
mod plan_view_tests;
#[cfg(test)]
mod purge_tests;
#[cfg(test)]
mod structure_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod text_tests;

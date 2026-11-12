//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

pub use bimcraft_geom::{files, geom, ifc_classes, language, prim, shared, skeleton, step, triangulation, units, xlsx};
pub use bimcraft_mep_calc::{conduit, demand, dps, dr, drainage, hydrant, loads, nbr5410, plumbing, reservoir, storm, water};
#[doc(hidden)]
pub use bimcraft_model::run_arm;
pub use bimcraft_model::{
    alignment, background_grid, bounds, cad, camera, cloud, crosscheck, curves, derived, dims, dims3d, draw, dxf, electrical, falls,
    family, favorites, fills, gltf, glue, graphics, groups, ids, ifc, import, inquiry, insert, json, lenient, library, lines, link,
    massing, masterplan, merge, model, motion, network, paint, pat, pdf, pen, phasing, quantities, raster, rebar, reinforce, render, rooms,
    runs, schedule, selection, selfcheck, sheet, site, sketch, snap, stairs, standards, styles, symbol_set, tempdims, text, texture,
    titleblock, triad, type_data, view3d, wallchain, walls, warnings, workplane,
};
use bimcraft_model::{constraints, sync_settings, timber};
// What only the tests here name by its old path.
#[cfg(test)]
use bimcraft_model::{component_geometry, fixtures};
pub mod align;
#[cfg(test)]
mod align_tests;
#[cfg(test)]
mod array_tests;
pub mod autosave;
#[cfg(test)]
mod autosave_tests;
pub mod bridge;
#[cfg(test)]
mod cad_tests;
#[cfg(test)]
mod calc_tests;
#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod cloud_tests;
mod commands;
#[cfg(test)]
mod dry_run_tests;
#[cfg(test)]
mod eberick_tests;
#[cfg(test)]
mod electrical_tests;
pub mod engine;
#[cfg(test)]
mod files_tests;
#[cfg(test)]
mod georef_tests;
#[cfg(test)]
mod glue_tests;
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
#[cfg(test)]
mod masterplan_tests;
#[cfg(test)]
mod model_tests;
#[cfg(test)]
mod openings_past_top_tests;
#[cfg(test)]
mod oracle_tests;
#[cfg(test)]
mod pin_tests;
#[cfg(test)]
mod quantities_tests;
#[cfg(test)]
mod read_only_tests;
#[cfg(test)]
mod rebar_tests;
#[cfg(test)]
mod reinforce_tests;
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
#[cfg(test)]
mod run_join_tests;
pub mod sample;
#[cfg(test)]
mod section_box_tests;
#[cfg(test)]
mod selfcheck_tests;
#[cfg(test)]
mod shape_tests;
#[cfg(test)]
mod site_plan_tests;
pub mod sync;
#[cfg(test)]
mod sync_room_tests;
#[cfg(test)]
mod sync_settings_tests;
#[cfg(test)]
mod sync_tests;
#[cfg(test)]
mod tempdims_tests;
#[cfg(test)]
mod titleblock_tests;
#[cfg(test)]
mod type_data_tests;
#[cfg(test)]
mod wall_top_offset_tests;
#[cfg(test)]
mod warnings_tests;
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

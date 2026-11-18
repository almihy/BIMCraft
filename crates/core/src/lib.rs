//! BIMCraft core: the building model, the command engine every front end drives, plan views
//! rendered to SVG and PNG, sections, elevations, the 3D view, sheets printed to PDF, schedules
//! and IFC4 export. Nothing here knows about a UI.

pub use bimcraft_geom::{files, geom, language, prim, shared, skeleton, solid, step, triangulation, units, xlsx};
pub use bimcraft_ifc::{ifc, import};
pub use bimcraft_mep_calc::{conduit, demand, dps, dr, drainage, hydrant, loads, nbr5410, plumbing, reservoir, storm, water};
#[doc(hidden)]
pub use bimcraft_model::run_arm;
pub use bimcraft_model::{
    alignment, background_grid, bounds, cad, camera, carpark, cloud, crosscheck, curves, derived, dims, draw, dxf, electrical, falls,
    family, favorites, fills, glue, graphics, ground, groups, ids, ifc_classes, insert, json, lenient, library, lines, link, massing,
    masterplan, merge, model, motion, network, paint, pat, pen, phasing, quantities, raster, rebar, reinforce, rooms, runs, schedule,
    selection, selfcheck, site, sketch, solids, stairs, standards, styles, symbol_set, tempdims, text, texture, titleblock, type_data,
    wallchain, walls, warnings, workplane,
};
use bimcraft_model::{constraints, sync_settings, timber};
pub use bimcraft_render::{dims3d, dxf_out, gltf, inquiry, pdf, render, sheet, snap, triad, view3d};
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
mod dxf_out_tests;
#[cfg(test)]
mod eberick_tests;
#[cfg(test)]
mod electrical_tests;
#[cfg(test)]
mod element_solids_tests;
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
mod parking_tests;
#[cfg(test)]
mod pdf_fidelity_tests;
#[cfg(test)]
mod pdf_layers_tests;
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
mod section_cut_tests;
#[cfg(test)]
mod selfcheck_tests;
#[cfg(test)]
mod shape_tests;
#[cfg(test)]
mod site_plan_tests;
#[cfg(test)]
mod site_revit_tests;
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
mod wall_join_tests;
#[cfg(test)]
mod wall_top_offset_tests;
#[cfg(test)]
mod wall_type_tests;
#[cfg(test)]
mod warnings_tests;
#[cfg(test)]
mod workplane_tests;

pub use commands::{command_of, floor_outline, opening_of, roof_footprint, run_category_of, runs_of, stood_of};
pub use json::element_json_bare;

/// Element `id`'s JSON ([`json::element_json`]) with what is found by drawing it: a viewport's size on its sheet, mm
/// ([`sheet::size`]).
pub fn element_json(m: &model::Model, id: model::Id) -> serde_json::Value {
    drawn(m, id, json::element_json(m, id))
}

/// [`element_json`], a room's boundary found among `rooms` ([`json::element_json_with`]).
pub fn element_json_with(m: &model::Model, id: model::Id, rooms: &rooms::Rooms) -> serde_json::Value {
    drawn(m, id, json::element_json_with(m, id, rooms))
}

/// `json`, element `id`'s, with what drawing it finds.
fn drawn(m: &model::Model, id: model::Id, mut json: serde_json::Value) -> serde_json::Value {
    if let (Some(model::Element::Viewport(v)), Some(o)) = (m.elements.get(id), json.as_object_mut()) {
        o.insert("size".into(), serde_json::json!(sheet::size(m, v.view, v.scale)));
    }
    json
}

#[cfg(test)]
mod connector_tests;
#[cfg(test)]
mod detail_tests;
#[cfg(test)]
mod dimension_tests;
#[cfg(test)]
mod door_leaf_tests;
#[cfg(test)]
mod door_tests;
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

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
pub mod align;
pub mod autosave;
pub mod bridge;
#[doc(hidden)]
pub mod commands;
pub mod engine;
pub mod sample;
pub mod sync;

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

//! BIMCraft's drawings of the model: plans, sections and elevations (`render`), the 3D view (`view3d`), sheets (`sheet`), 3D
//! dimensions (`dims3d`), the 3D view's axes (`triad`), snapping (`snap`), what is under the cursor (`inquiry`), and what a
//! drawing is written as (`pdf`, `dxf_out`, `gltf`). Above the model, beside IFC: it reads the model and its solids, never
//! changes them, and nothing under it draws. `bimcraft-core` re-exports each module under its old path.

// The crates under it, by the paths its modules have always named them by (`crate::model`, `crate::geom`).
pub use bimcraft_model::{
    bounds, cad, camera, carpark, cloud, component_geometry, curtain, curves, derived, dims, dxf, family, files, fills, font, geom,
    graphics, ground, groups, ids, json, language, lines, link, massing, masterplan, model, paint, pen, prim, raster, rooms, runs,
    schedule, selfcheck, shared, site, sketch, solid, solids, stairs, standards, styles, text, texture, titleblock, units, walls,
    workplane,
};

pub mod dims3d;
pub mod dxf_out;
pub mod gltf;
pub mod inquiry;
pub mod pdf;
pub mod render;
pub mod sheet;
pub mod snap;
pub mod triad;
pub mod view3d;

//! BIMCraft's site geometry (`site`): polygons with their properties, the shapes a site's plot, pads, roads and car parks
//! make, and the car park's layout. Under the model: it needs only geometry, so a change to the model does not rebuild it.
//! The model re-exports it as `crate::site`.

// The crate under it, by the path its modules have always named it by (`crate::geom`).
pub use bimcraft_geom::geom;

pub mod site;

//! BIMCraft's building model: the elements, their types, families and the library (`model`, `ids`, `library`), what each
//! element is in plan and in 3D (`solids`), the schedules and the title blocks' data. The commands that change it and the
//! Session that runs them are `bimcraft-core`'s; its drawings are `bimcraft-render`'s and IFC `bimcraft-ifc`'s, above it;
//! the site's geometry is `bimcraft-site`'s, under it. The core re-exports each module here under its old path. Nothing here
//! draws or knows about a UI.

// The crates under the model, by the paths its modules have always named them by (`crate::geom`, `crate::units`).
pub mod font;
pub use bimcraft_geom::{csg, files, geom, language, prim, shared, skeleton, solid, step, triangulation, units, xlsx};
pub use bimcraft_mep_calc::{conduit, demand, dps, dr, drainage, hydrant, loads, nbr5410, plumbing, reservoir, storm, water};
pub use bimcraft_site::site;

pub mod alignment;
pub mod background_grid;
pub mod bounds;
pub mod cad;
pub mod camera;
pub mod carpark;
pub mod cloud;
#[doc(hidden)]
pub mod component_geometry;
#[doc(hidden)]
pub mod constraints;
pub mod crosscheck;
pub mod curtain;
pub mod curves;
pub mod derived;
pub mod dims;
pub mod draw;
pub mod dxf;
pub mod electrical;
pub mod falls;
pub mod family;
pub mod favorites;
pub mod fills;
#[doc(hidden)]
pub mod fixtures;
pub mod glue;
pub mod graphics;
pub mod ground;
pub mod groups;
pub mod ids;
pub mod ifc_classes;
pub mod insert;
pub mod json;
pub mod lenient;
pub mod library;
pub mod lines;
pub mod link;
pub mod massing;
pub mod masterplan;
pub mod merge;
pub mod model;
pub mod motion;
pub mod network;
pub mod paint;
pub mod pat;
pub mod pen;
pub mod phasing;
pub mod quantities;
pub mod raster;
pub mod rebar;
pub mod reinforce;
pub mod rooms;
pub mod runs;
pub mod schedule;
pub mod selection;
pub mod selfcheck;
pub mod sketch;
pub mod solids;
pub mod stairs;
pub mod standards;
pub mod styles;
pub mod symbol_set;
#[doc(hidden)]
pub mod sync_settings;
pub mod tempdims;
pub mod text;
pub mod texture;
#[doc(hidden)]
pub mod timber;
pub mod titleblock;
pub mod type_data;
pub mod wallchain;
pub mod walls;
pub mod warnings;
pub mod workplane;

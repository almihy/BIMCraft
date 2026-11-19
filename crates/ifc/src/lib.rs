//! BIMCraft's IFC4: the model written as an IFC file (`ifc`), and an IFC file read into the model (`import`, over the
//! STEP reader in `bimcraft-geom`). Above the model and beside the drawings: nothing here draws, and nothing in the model
//! or the drawings needs it. `bimcraft-core` re-exports both modules under their old paths.

// The crates under it, by the paths its modules have always named them by (`crate::model`, `crate::geom`).
pub use bimcraft_geom::{geom, skeleton, step, triangulation};
pub use bimcraft_model::{
    curtain, family, graphics, ids, ifc_classes, library, massing, masterplan, model, phasing, quantities, rebar, rooms, runs, schedule, stairs,
    walls,
};

pub mod ifc;
pub mod import;

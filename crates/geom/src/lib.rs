//! BIMCraft's geometry and the plain helpers under the model: plan and 3D maths (`geom`), constrained triangulation, the straight
//! skeleton, solids cut from solids, STEP and XLSX writing, units and a project's language, the files a session reads and
//! writes, drawing primitives, our own lettering and IFC4's class list. Nothing here knows about the model.

#[doc(hidden)]
pub mod csg;
pub mod files;
#[doc(hidden)]
pub mod font;
pub mod geom;
pub mod language;
pub mod prim;
pub mod shared;
pub mod skeleton;
pub mod step;
pub mod triangulation;
pub mod units;
pub mod xlsx;

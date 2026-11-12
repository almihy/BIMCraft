# bimcraft-geom

The bottom of the engine: geometry and plain helpers that know nothing of the model, so they change rarely and every crate
above builds on them without waiting for the model. `bimcraft-core` re-exports each module under its old path
(`bimcraft_core::geom`, `crate::units` inside the core), so code above names them as it always has. Nothing here may depend
on another BIMCraft crate but `bimcraft-i18n`.

## Modules

- `geom.rs`: plan geometry (`P2`, rings, areas, the planar faces walls enclose), `TOL` and `MAX_COORD`, a point to the
  micron and a ring tidied; `geom/v3.rs`: 3D vector maths on `P3` arrays.
- `triangulation.rs`: constrained Delaunay triangulation in plan, exact on a 0.01 mm grid (shaped floors' tops:
  [shape editing](../../docs/shape-editing.md)). `skeleton.rs`: the straight skeleton (roofs by footprint). `csg.rs`: solids cut
  from solids.
- `step.rs`: the STEP files IFC export and import use. `xlsx.rs`: a one-sheet workbook schedules export to.
- `units.rs`, `language.rs`: a project's units and language (the project catalogs, compiled by `build.rs` from
  `translations/project`). `files.rs`: the files a session's commands read and write, and base64. `shared.rs`: shared parts
  of the spatial core ([crates/core/CLAUDE.md](../core/CLAUDE.md)).
- `prim.rs`: what a drawing is made of: `Rgb`, `Prim`, `Part` and the pixel `View`. `font.rs`: our own lettering.

Helpers that only tests use, here and in the crates above, are `#[cfg(debug_assertions)]` (`xlsx::unzip`): a debug build is
the same whichever crate's tests or build asks for it, and a release build leaves them out.

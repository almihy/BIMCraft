# bimcraft-model

The building model and everything that reads it: elements, types, families and the library, what each element is in plan
and 3D, the views drawn from it (plans, sections, the 3D view, sheets, PDF, glTF), schedules, IFC export and import, the
site, warnings and the model's half of live merge. Its module map is in the root [CLAUDE.md](../../CLAUDE.md); how a change
goes (a command, a rule in `Model::check`, ids and GUIDs) is in [crates/core/CLAUDE.md](../core/CLAUDE.md), and holds here.

- **Under the commands.** Nothing here knows about the `Session` or the commands (`bimcraft-core`); what runs after every
  command takes the model (`masterplan::keep`, `constraints`), and the engine applies it. The geometry under it is
  `bimcraft-geom`'s, re-exported here under the same paths (`crate::geom`, `crate::units`).
- **Tests.** A module's tests that need only the model stay with it. Those that drive the Session live in the core's
  `src/model_tests/`, one file per module (`render.rs`, `view3d_update.rs`), and reach the module's items through its
  public paths; an item only they need is `pub` and `#[doc(hidden)]` in spirit, not part of the engine's command API.
- **The `testing` feature** (the core's tests turn it on) keeps what tests count and check in the model's code when the
  core's tests run: the counters (`warnings::REVIEWS`, `cloud::LOOKED`, `model::FALLBACKS`, `render::regions::SPENT`) and
  the twins a cheap path is held to (`Image::same_as`, `Review::assert_whole`). Use `#[cfg(any(test, feature = "testing"))]`
  for anything a core test reads.
- `data/` holds the starter electrical settings and the AREI symbols; `testdata/` a texture's JPEG.

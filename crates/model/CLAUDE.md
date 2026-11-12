# bimcraft-model

The building model and everything that reads it: elements, types, families and the library, what each element is in plan
and 3D, the views drawn from it (plans, sections, the 3D view, sheets, PDF, glTF), schedules, IFC export and import, the
site, warnings and the model's half of live merge. Its module map is in the root [CLAUDE.md](../../CLAUDE.md); how a change
goes (a command, a rule in `Model::check`, ids and GUIDs) is in [crates/core/CLAUDE.md](../core/CLAUDE.md), and holds here.

- **Under the commands.** Nothing here knows about the `Session` or the commands (`bimcraft-core`); what runs after every
  command takes the model (`masterplan::keep`, `constraints`), and the engine applies it. The geometry under it is
  `bimcraft-geom`'s, re-exported here under the same paths (`crate::geom`, `crate::units`).
- **Tests.** A module's tests that need only the model stay with it (inline, or in `<module>_tests.rs` beside it through
  `#[path]`, as `model_tests.rs` and `render_tests.rs`). Those that drive the Session live in the core's
  `src/model_tests/`, one file per module (`render.rs`, `view3d_update.rs`), and reach the module's items through its
  public paths; an item only they need is `pub` and `#[doc(hidden)]` in spirit, not part of the engine's command API.
- **Self-checks and counters are debug builds'.** A fast path held to its twin (every element asked instead of the lookup's,
  the whole scene drawn) is `if selfcheck::on(Check::X) { ... }` (`selfcheck.rs`): compiled into every debug build and run
  by every crate's tests, a constant `false` in release builds (the browser's among them); the desktop and the command
  line call `selfcheck::off()` first, so their debug builds run at the fast paths' speed. A new twin adds a `Check`, and the
  core's `selfcheck_tests` fails until a model built by commands reaches it. What tests count and the helpers only they
  read (`warnings::REVIEWS`, `cloud::LOOKED`, `model::FALLBACKS`, `render::regions::SPENT`, `Review::assert_whole`) are
  `#[cfg(debug_assertions)]`, never `cfg(test)` (which the crates above don't see) or a feature (which would build this
  crate differently for each command).
- `data/` holds the starter electrical settings and the AREI symbols; `testdata/` a texture's JPEG.

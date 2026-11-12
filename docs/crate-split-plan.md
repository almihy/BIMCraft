# Splitting bimcraft-core: a plan

Status: steps 1 to 3 done, and the icons (step 6); step 4 (render, site and IFC out of the model) is next. What was done,
how it was done and the numbers are under *Where it stands*, at the end.

## Why

`bimcraft-core` is one crate of 7.5 MB of Rust: 4.6 MB of code and 2.9 MB of tests (the `*_tests.rs` files, `tests.rs`, the
`tests/` folders and the inline `mod tests`). Rust compiles a crate as one unit, its front end (parsing, name resolution, type
and borrow checking) on one thread. So:

- `cargo test -p bimcraft-core --lib` compiles all 7.5 MB in one rustc (7-9 GB; 10 m 35 s clean with the other helpers
  building, 1 m 24 s after a one-line change, before the profile changes of the same commit as this plan);
- `cargo test -p bimcraft-ui-egui --lib` first compiles the 4.6 MB library the same way, then ui-egui (2.8 MB of code and
  1.3 MB of tests) on top;
- every change anywhere in core redoes both, and nothing runs alongside, since everything waits for core.

Smaller crates compile in parallel (independent ones side by side, and a crate's dependants start as soon as its metadata is
ready), and a change rebuilds only its own crate and those above it. A test binary per crate tests that crate alone.

The numbers below are from `crates/core/src` at arqiflow-web 4ec760e3 (12 November 2026): sizes are bytes of code with
tests left out, dependencies are the `crate::module` paths each module's code (not its tests) names.

## The seams as they are

Today the modules form one cycle: 77 of the 80-odd modules, 4.3 MB of the 4.6 MB, all reach each other. Only `geom`,
`alignment` and the MEP calculations (`demand`, `nbr5410`, `dps`, `loads`, `storm`, `reservoir`, `dr`, `hydrant`, `conduit`,
`water`, `drainage`, `plumbing`) stand outside it. The cycle is held together by a few dozen downward-pointing uses, almost all
small helpers that live in the wrong place:

| Upward use | What it is | Where it goes |
| --- | --- | --- |
| `view3d::{P3, unit, sub, cross, extremes}` from bounds, component_geometry, ifc, massing, reinforce, workplane | 3D vector math | `geom` (a `geom::v3` module) |
| `render::Rgb` from model, cad, lines, masterplan, texture | a colour | the model crate (`model::Rgb`), re-exported by render |
| `render::{Prim, PAPER}` from lines, graphics, titleblock | drawing primitives | render's primitives (`Prim`, `Part`, `Rgb`) become their own small module the model crate owns, or titleblock's drawing moves up into render |
| `commands::base64` from cad, cloud, texture, render | an encoder | `files` or a `codec` module at the bottom |
| `commands::{category, micron, m2, colour}` from model, schedule, graphics | names and number formats | the model crate |
| `commands::{Motion, transform, move_by}` from groups, constraints | moving elements | groups' and constraints' moving code moves up with commands |
| `engine::Session` from constraints, masterplan, timber, view3d | running a command | these call sites are commands (or their tests) and move up |
| `view3d::{DisplayOptions, MIN_LENS, MAX_LENS, MAX_SILHOUETTE, solids, tops_at, Surface}` from model, graphics, family, rebar, runs, sheet, workplane | a view's settings (data) and two queries (`solids`, `tops_at`) | `DisplayOptions` and its limits to the model crate; the queries stay in render and `Model::check` stops calling them (below) |
| `dxf::{MAX_FILE_COORD, union, Shape}` from model, cad, titleblock | limits and a shape type | the model crate |
| `titleblock::check_values`, `schedule::check`, `sheet::DENSITY`, `graphics::scale_of`, `dims3d::length` from model | `Model::check`'s validation of each part | data and limits to the model crate; validation that needs drawing code moves with `Model::check` (below) |
| `background_grid::BackgroundGrid` from graphics | a view setting (data) | the model crate |
| `ifc::classes` from family | IFC4's class list (data) | the model crate, or a `bimcraft-ifc-schema` leaf |
| `sample::SAMPLE`, `sample::tower` from gltf, view3d | sample models (used by tests and the start page) | stay in the top crate; the callers' uses are tests |

`Model::check` is the one real knot: it validates every part of the model, and some of that validation asks drawing code
(`view3d::solids`, `tops_at`, `schedule::check`). It moves to the commands crate as `check(model)` (the engine already calls it
after every command), or the model crate gets a `Check` trait that the higher crates register into. The first is simpler and
keeps "validate in one place".

## The crates

Bottom to top. Each depends only on those above it in this list.

| Crate | Modules | Code | Notes |
| --- | --- | --- | --- |
| `bimcraft-geom` | geom (+ the 3D math from view3d), skeleton, triangulation, csg, step, xlsx, units, language, files, shared, font | ~0.15 MB | pure: no model; changes rarely |
| `bimcraft-mep-calc` | demand, nbr5410, dps, loads, storm, reservoir, dr, hydrant, conduit, water, drainage, plumbing | ~0.14 MB | already a leaf: no `crate::` uses at all; its commands stay in commands |
| `bimcraft-model` | model, ids, lenient, library, type_data, phasing, family (its types and formulas), schedule (its types), graphics (its types), and the element types: walls, runs, rebar, stairs, curves, sketch, massing, masterplan, cad, cloud, fills, lines, texture, paint, pen, styles, standards, text, dims, rooms, electrical, network, groups, link, insert, titleblock and sheet data, derived, falls, glue, wallchain, tempdims, workplane, quantities, bounds, component_geometry, alignment, constraints, reinforce, timber, crosscheck, pat, selection, favorites, symbol_set | ~1.7 MB | the model and its types; the biggest piece, and the one most changes touch |
| `bimcraft-site` | site, masterplan's planning | ~0.33 MB | site only needs geom and model |
| `bimcraft-render` | render, view3d, dxf, pdf, gltf, dims3d, background_grid, draw, triad, snap, inquiry, schedule tables, sheet and titleblock drawing | ~0.85 MB | plans, sections, the 3D view, sheets, picking |
| `bimcraft-ifc` | ifc, import, step reading | ~0.35 MB | IFC export and the importers; needs render's 3D solids |
| `bimcraft-core` | commands, engine, sync, sync_settings, warnings, autosave, sample, align, bridge, `Model::check` | ~1.15 MB | the commands and the Session; keeps its name and re-exports the crates below, so ui-egui, the CLI and web change only their `use` lines where they name a moved module |

Families: `family.rs` is model data plus its formula evaluator; it stays in `bimcraft-model`. A separate `bimcraft-family`
crate is not worth it at 0.12 MB unless the family editor grows.

ui-egui: its biggest module is `icons/drawn.rs` (0.57 MB of generated outlines) and `icons.rs` (0.1 MB). Moving those into a
`bimcraft-icons` crate, which almost never changes, takes 0.7 of its 2.8 MB out of every ui-egui rebuild. The rest
(canvas, properties, gpu, orbit) is one front end and stays one crate.

## What it gains

Rebuild after a one-line change, `cargo test -p <crate> --lib`, estimated from crate sizes (rustc time grows a little faster
than linearly with crate size, so this is if anything conservative):

| Change in | Today (core tests / ui-egui tests) | After the split |
| --- | --- | --- |
| a command (commands.rs, the most edited file) | all of core twice | `bimcraft-core` only: ~1.15 MB + its tests, about a quarter of today's core |
| render, view3d, sheets | all of core twice | render + ifc + core, the model and below reused |
| site | all of core twice | site + core |
| the model's types | all of core twice | everything above geom, but render, site and ifc compile side by side |
| ui-egui | ui-egui | ui-egui without its icons |

Tests: today `cargo test -p bimcraft-core --lib` builds 2.9 MB of tests into one binary. Split, a helper working in render
runs `cargo test -p bimcraft-render` (render's tests only), and the 0.4 MB of cross-cutting tests (`tests.rs`, the
`*_tests.rs` that drive the Session) stay in `bimcraft-core`.

A clean build gains less: the model crate (1.7 MB) is still one unit everything waits for, then render, site, ifc and mep-calc
build in parallel, then core. On a 24-thread PC that is roughly the model's time plus the largest of the middle crates plus
core's, against today's one serial crate.

## Order of work

1. Move the misplaced helpers down (the table above) inside core, with no new crates: `geom::v3`, `model::Rgb`,
   `files::base64`, the constants and view-settings types, `Model::check` up into commands. Each is a small, mechanical
   commit, and the cycle report (below) shrinks after each.
2. Split out the leaves: `bimcraft-geom`, `bimcraft-mep-calc`.
3. Split out `bimcraft-model`; `bimcraft-core` re-exports it.
4. Split out `bimcraft-render`, `bimcraft-site`, `bimcraft-ifc`.
5. Move each crate's tests with it; keep the Session-driven tests in core.
6. `bimcraft-icons` out of ui-egui.

Visibility: many `pub(crate)` items become `pub` across crates; mark them `#[doc(hidden)]` where they are internals.

To check the cycles at any point, list each module's `crate::<module>` uses outside tests (leaving out the `*_tests.rs`
files, `tests.rs`, `tests/` folders and `#[cfg(test)] mod` blocks) and find the strongly connected components (Tarjan's
algorithm): step 1 is done when the one big component is gone.

## Where it stands (12 November 2026, branch infra/crate-split)

Done, one commit each, every suite passing (`cargo test --workspace -- --skip gpu:: --skip retained::`: 2003 engine tests
across core, model, geom and mep-calc, as before the split; ui-egui's 1008) and both browser builds building:

1. The helpers moved down inside the core: `geom::v3`, `prim` (Rgb, Prim, Part, View), `files::base64`, the commands'
   name and number helpers (`micron`, `tidy`, `m2`, `category`, `colour`, `next_number`), `camera` (a 3D view's lens and
   display options), `ifc_classes`, `motion` (Motion, transform), `json` (element_json), and what runs after every command
   taking the model instead of the Session (`masterplan::keep`, `timber`; keeping constraints went up into the engine).
2. `bimcraft-geom` and `bimcraft-mep-calc` (its command tests in the core's `calc_tests/`).
3. `bimcraft-model`: here taken as everything under the commands (model, render, view3d, sheets, schedules, IFC, site,
   warnings, the model's half of sync as `merge`), so the commands came apart from the rest first, which is the change
   most edits make. Session-driven tests of its modules are the core's `model_tests/`; the counters and self-checks inside
   the model are on in every debug build (`selfcheck`: a fast path held to its twin, compiled out of release builds),
   and `bimcraft-workspace-hack` asks for every feature the workspace turns on in the model's dependencies, so the model
   builds alike for `cargo build`, `cargo test` of any crate or of the workspace, and clippy, and switching rebuilds
   nothing (on Windows, where it was measured; Linux and macOS may want their own lines in it).
6. `bimcraft-icons`: the Icon list and the generated drawings.

Left: step 4, splitting render (with view3d, sheet and titleblock drawing, pdf, gltf), site and IFC out of
`bimcraft-model`. The knots are as the table above says, with these found on the way: `Model::model_extent`,
`level_extent`, `roof_at`, `spot_host` and `spot_value` ask `view3d::solids` and `tops_at` (and `spot_value` is in an
element's JSON, which schedules and tags read), so the element solids either move into the model crate or those queries
move up; `graphics::dress` and `raster::draw` take a `Drawing`; `titleblock` and `sheet` hold both their data and their
drawing; `inquiry` calls `ifc::compress`. Step 5 beyond what moved already: the core's `*_tests.rs` that test one crate's
code alone could move down to it.

### Visibility, and what step 4 has to cut

Every item the split had widened was put back to what it was in the core (`pub(crate)`, `pub(super)` or private) and
made `pub` again only where the compiler asked for it. Of the 425 so widened, 37 are reached by the code of the crates
above (the core's commands and engine: `phasing`'s phase and option edits, `glue`'s footings, `walls::Index`,
`import::seed` and `from_step`, `render::plan_shows` and `section_heights`, `raster::PdfReads`, the `constraints`,
`sync_settings` and `timber` modules; geom's `csg`, `font`, `arrangement`, `crossing`), and about 150 only by the core's
tests that drive the Session (most of them `view3d`'s: `Solid`, `PreparedSolids`, `solids_in`; `component_geometry`'s,
`sheet::Table`). Both kinds are `#[doc(hidden)]`: internals, not the model's API. The counters and helpers only tests
read are, besides, `#[cfg(debug_assertions)]`. With the rest `pub(crate)` again, the dead-code lint sees them; it found
nothing unused.

What the future crates' code (not their tests) names of each other, today:

| From | To | What |
| --- | --- | --- |
| model | render | `view3d::{solids, tops_at, Surface, Camera}` (`Model::model_extent`, `level_extent`, `roof_at`, `spot_*`), `dims3d::length`, `render::Drawing` (`graphics::dress`), `sheet::{DENSITY, size, title_values}`, `titleblock::{TitleBlock, Scope, NONE, check_values}`, `dxf::{Dxf, Entity, Shape, Color, read, union, arc_points, MAX_BYTES, MAX_FILE_COORD}`, `raster::{ImageInstance, ImageType, MAX_HELD, MAX_TYPES, MIN_SIDE}`, `background_grid::BackgroundGrid` |
| model | site | `masterplan::{Site, PropertyLine, BuildingPad, MassSite, check_site, check_property_line, check_mass_site, transform_props}`, `site::Poly` |
| render | site | `masterplan::{Site, PropertyLine, BuildingPad, solids, plan_colours, road_colours, surface_colour}`, `site::{Poly, shapes}` |
| render | ifc | `ifc::{compress, expand}` (`inquiry`) |
| ifc | site | `masterplan::solids` |

IFC needs nothing of render: its 3D is its own and `masterplan::solids`. So: the site's element types and checks
(`masterplan`'s data) stay in the model, its shapes and solids (`site`, `masterplan`'s geometry) become `bimcraft-site`
under render and ifc; `compress`/`expand` (an IFC GUID as text) go down beside `ids`, and the model-to-render column is the knot the
table above names (the element solids, `Drawing`, the data halves of `sheet`, `titleblock`, `dxf` and `raster`).

Measured with `cargo test -p bimcraft-core -p bimcraft-ui-egui --no-run --timings`, target on the Dev Drive, sccache
warm for the dependencies, nothing else building; a one-line change is a new item at the end of the file:

| | before (5bda50ab) | after |
| --- | --- | --- |
| clean build | 5m 40s (core lib tests) | 5m 44s (both): model 104 s, then core 36 s and its tests 148 s |
| one line in `commands.rs` | 1m 11s / 1m 23s | 20 s |
| one line in `render.rs` | 1m 11s / 1m 23s | 1m 14s (model 28 s, core tests 44 s, ui-egui tests 37 s) |
| one line in `model.rs` | 1m 11s / 1m 23s | 56 s |
| one line in `geom.rs` | 1m 11s / 1m 23s | 40 s |

The before column was measured at 5bda50ab, not at this branch's base (94d57bea, 46 commits of other work later): core's
tests and ui-egui's tests each on its own; after is both in one command. A clean build gains
nothing yet: the model crate is still 3.4 MB everything waits for, and the core's test binary still holds the tests of
the model's modules that drive the Session.

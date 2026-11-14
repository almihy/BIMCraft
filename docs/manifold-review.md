# Review of geom/manifold (ce5521de..bc3b6742)

Reviewer: read the whole diff (geom::solid, slice, curved, mesh; family/cutting.rs and the check; view3d/element_solids.rs and
the Scene cache; quantities.rs; render/section_cut.rs and render.rs's `cut_model`; the vendored sys crate and workspace
wiring; the four docs). Ran on Windows (vcvars, `-j 4`, `CARGO_TARGET_DIR=D:\cargo-targets\manifold-elements`):

* `cargo test -p bimcraft-core --lib -- section_cut element_solids quantities`: 35 passed, 1 ignored.
* `cargo test -p bimcraft-geom --lib solid`: 23 passed.
* Four throwaway tests (not committed; described under each finding so the fixer can add them).

Findings, most severe first.

---

## 1. Every family component is now cut in sections, including categories Revit never cuts (Revit parity)

**Where:** `crates/model/src/render.rs:2275-2281` (`cut_model`: every element in `work.placed` but masses and deferred runs is
sliced through `solid_cache`), and `crates/model/src/view3d/element_solids.rs:189` (`solid_in` turns any `Solid::Mesh` into a
solid, ignoring its `cut: false` flag, which view3d.rs:945 and :1022 set on every component and family door/window form and
whose meaning, view3d.rs:428, was "shown beyond a section's line and never cut").

**What is wrong.** Before the branch no component was cut. Now every component is cut and filled, whatever its category. I
proved it with a throwaway test in `crates/core/src/section_cut_tests.rs`: one 600 x 600 x 800 box family per category, placed,
a section through its middle, count the rings `section_cut_rings` gives for it. Furniture, lighting, plumbing, outlet,
casework, generic: **one cut ring each**. In the sample house the sofa, chairs, table and bed are filled solid
(`house-sec2..6`).

What Revit does (Revit 2025 help, "About Cuttable Families" and "About Non-Cuttable Families"):

| Engine category (`family::Category`) | Revit category it stands for | Revit | Branch |
|---|---|---|---|
| Door, Window (and doors/windows with no family) | Doors, Windows | cut | cut: correct |
| Casework | Casework | cut | cut: correct |
| Generic | Generic Models | always cut | cut: correct |
| Furniture | Furniture (also Furniture Systems) | cuttable since Revit 2023, **only when the family's cut option is ticked** (a per-family setting; see below) | **cut always: wrong by default** |
| Plumbing (WC, basin, sink, shower, bath) | Plumbing Fixtures | not in the 2025 help's cuttable table: shown in projection (some notes say 2023 made it cuttable with the same per-family option; either way, not cut by default) | **cut: wrong** |
| Drain | Plumbing Fixtures | as above | **cut: wrong** |
| PipeFitting | Pipe Fittings | not cuttable | **cut: wrong** |
| PipeAccessory | Pipe Accessories | not cuttable | **cut: wrong** |
| Equipment (distribution board) | Electrical Equipment | not cuttable | **cut: wrong** |
| Outlet, Switch, DataOutlet, VoiceOutlet | Electrical Fixtures, Lighting Devices, Data Devices, Telephone Devices | not cuttable | **cut: wrong** |
| Lighting | Lighting Fixtures | not cuttable | **cut: wrong** |
| PatchPanel, Rack | Communication/Data Devices, Electrical Equipment | not cuttable | **cut: wrong** |
| AirTerminal, Earthing, InspectionBox (lightning protection) | Electrical Fixtures, or Generic Models in most content | not cuttable as fixtures | **cut: wrong** (unless treated as Generic) |

Revit's non-cuttable list also has Entourage, Planting, Parking and Mechanical Equipment; Speciality Equipment is cuttable with
the per-family option, like Furniture. The engine has no component category for those yet; site trees and cars are
`Element::Site` slabs and were already cut before this branch (a separate, older gap for the same table).

On Furniture's default: the help confirms the option is per family ("made available, and you can select it"); I could not read
a source that states its default for new families. Families made before 2023 have it off, and most furniture content is shown in
projection, so "not cut unless the family says so" is the safe default.

**Where the classification is:** `crates/model/src/graphics.rs:84` (`COMPONENTS`, the per-category keys) and `:181`
(`category_of(m, e)`, which maps a component to its family's category key). That is the place for the rule.

**Fix.**
1. In `graphics.rs`, next to `COMPONENTS`, add `pub fn cuttable(m: &Model, e: &Element) -> bool`: true for everything that is
   not a component; for a component, by `category_of`: `casework`, `generic` → true; `furniture` → the family's own cut option
   (step 2); every other component category → false. A doc comment should carry the table above.
2. Add to `Family` a `cut_in_views: bool` (Revit's per-family option for Furniture / Furniture Systems / Speciality
   Equipment), default false, read from the Revit add-in's definition when it says so, settable in the family editor. Only
   consulted for furniture.
3. In `cut_model` (render.rs:2275), skip `!graphics::cuttable(m, e)` elements, exactly where masses are skipped. They stay in
   the picture beyond the line, as before (the old behaviour for them was right).
4. Turn `house-sec2..6` and `components_show_in_plan_from_above_in_3d_and_beyond_a_section` back: the sofa, chairs, table, bed
   are not cut; the base cabinet (casework) is. Add the throwaway test above as a real one, asserting cut / not cut per category.

## 2. Family forms are cut whatever their Visibility Settings say (detail level, Front/Back, Left/Right)

**Where:** `crates/model/src/view3d/element_solids.rs:90-134` (`build`): every piece of a component or family door/window is
unioned into its material's layer solid; the piece's `vis` (`Vis::of(piece.show, x)`, view3d.rs:946 and :1023), which the
picture honours (`Vis::drawn(detail, looking)`), is dropped.

**What is wrong.** A form Revit draws only at Fine, or not in front/side views, is cut and filled in every section. Throwaway test
(section_cut_tests.rs): a generic family of two 600 x 600 x 800 forms, one with `"show": {"front": false, "side": false}`,
the other with `"show": {"coarse": false, "medium": false}`; a section along x through both. Result: **both are cut** (rings at
1000-1600 and 3000-3600 along the line); the first should never be cut, the second not at Coarse or Medium. The engine's own
door families hit this: family.rs:3618 gives frame forms `{"coarse": false, ...}`, so a Coarse section fills a frame the
picture does not draw. Typical Revit content (casework with Fine-only hardware over a Coarse/Medium block, doors with
detail-level frames) does the same, so the cut also double-counts: both representations are unioned and cut at once.

**Fix.** Make the visibility part of the layer key: in `build`, group pieces by `(paint, vis)` (or carry `vis` per layer:
`layers: Vec<(u16, Vis, Solid3)>`), and in `section_cut::cut_element` slice only the layers whose `vis.drawn(d.detail,
Some(looking))` is true, `looking` being the section's view direction in plan. The cache stays per element (visibility is a
property of the piece, not of the view). Add the throwaway test above at Fine and at Coarse.

## 3. Quantities and the element JSON now build Manifold solids on every call, uncached: a hot path

**Where:** `crates/model/src/quantities.rs:45` (`wall`: `wall_volume(m, id, false)` and `(m, id, true)`, two fresh solids per
wall, each with a fresh `Components` and no wall index), `:113` (`solid_volume` for floors, ceilings, roofs, columns), called
from `crates/model/src/json.rs:125, 244, 307, 320`. `element_json*` is what schedules (`schedule.rs:728, 917, 1167, 1172`),
tags' text (`field_text_with`), the agent's find/list commands (`commands.rs:15578`), Select All Instances
(`ui-egui/src/lib.rs:2740`), Match Properties and Create Similar (`lib.rs:2715, 2761`), the status bar (`selecting.rs:281`) and
IFC export call. None of them goes through the `Scene`'s `SolidCache`, which has no caller outside tests.

**Measured** (throwaway test, dev profile, `crate::sample::highrise(5)`, 2,381 elements): `element_json_with` of its 165 walls
and floors took **282 ms**; the old prism sums for the same walls took **4 ms**. About 1.7 ms per wall or floor against 0.025.
Scaled: a wall schedule of the 20-storey highrise or Select All Instances > In Entire Project on it is seconds per refresh, every
edit, in the browser too (single-threaded wasm, slower again). A tag showing a wall's Type Mark builds the wall's solids each time
the plan draws its text.

**Fix.** Two parts, both needed:
1. Don't compute volumes for JSON that does not ask for them: `element_json_bare` (status bar, type lookups, tags that read a
   mark) should not include `area_m2`/`volume_m3`; schedules should ask for the volume fields only when a column shows them
   (`Row::get` can compute them lazily).
2. Give the model snapshot one `SolidCache` (in the derived cache next to `m.lookup()`, keyed on the snapshot like the lookup),
   used by quantities, the section work (`SectionWork::solid_cache` would read it instead of its own) and the 3D scene; keep the
   gross (no openings) wall volume in it too. Then a model's solids are built once per snapshot, and the `Scene::update` /
   `SolidCache::kept` invalidation (which is sound, see "fine as is") can carry them across edits for every reader.

## 4. Section solids are rebuilt from scratch after every edit

**Where:** `crates/model/src/render.rs:1348/1371/1450` (`SectionWork::solid_cache: Default::default()` in every constructor);
a new `SectionWork` is made per model snapshot (`ui-egui/src/section_worker.rs:62-68`, `view3d/fragments.rs:177`).

**What is wrong.** The doc's own numbers: a section of the 20-storey highrise is 184 ms cold (release) / 265 ms (dev) against 6 /
50 ms warm, and "cold" is every edit, since every edit makes a new work. In the browser that is roughly half a second of
Manifold on every edit while a section is open, against ~12 ms before the branch (`Solid::cuts`).

**Fix.** Same as 3.2: one per-snapshot cache, carried over by `Scene::update`'s `made_again` set (the `kept` logic exists). Or,
smaller: put each element's `Arc<ElementSolids>` in `fragments::SectionCache`'s per-element entries, keyed by the `Key::of(m, id)`
it already uses for columns and runs, so `work_cancellable` reuses them for unchanged elements.

## 5. Near-coplanar void faces leave a skin on the kernel path (proven); Revit families do this

**Where:** `crates/model/src/family/cutting.rs:55-79` (`cut`: closed form minus closed voids goes to `Solid3::difference_all`,
which is exact). The old cutter treated faces within `COPLANAR = 0.1` mm as one (`crates/geom/src/csg.rs:12-15`, added the day
before as K13 because Revit families' voids sit 0.02-0.06 mm off the faces they are meant to be flush with).

**Proven** (throwaway test in `cutting.rs`'s tests): a 1000 x 1000 x 100 slab, a 200 x 200 void from z = 0.04 to 110 (its bottom
0.04 mm above the slab's underside). Kernel result: volume 96,001,600 mm3, i.e. a **0.04 mm skin of 1,600 mm3 closes the hole**
(should be 96,000,000). (`csg::subtract` on the same pair gave 96,001,066.7: it is not clean either in this case, but that is the
older cutter's own matter.) The skin is invisible in 3D but shows in a plan or section cut through the hole as a closed face, and
the hole is not a hole for anything that asks the solid.

No family in this repository hits it: there are no Revit families under `samples/` or the tests (the Revit families arrive at
runtime from the add-in), and the built-in families' voids are exact. The K13 fix says the owner's Revit families do.

**Fix (snap before the boolean).** In `cutting::cut`, before `difference_all`, for each closed void: collect the form's face
planes (normal, offset; group the form's triangles by plane). For each void vertex, for each form plane whose normal is parallel
(|dot| > 1 - 1e-9) to a void face plane through that vertex and whose distance is <= 0.1 mm, move the vertex onto the form's
plane, then push it 0.01 mm further out of the form along the void face's outward normal (so the void face goes cleanly through
the form face rather than being exactly coplanar, which Manifold handles but is fragile under the later transforms). Do it on
the void's vertices only, and only along the normal of a face that is parallel to the form's, so nothing else moves. Rebuild the
void's `Solid3` from the snapped triangles. Add the throwaway test (expect 96,000,000 to 1e-3), and the same with the void's top
0.04 mm short of a form's top face.

## 6. Volumes silently drop any piece that is not a solid

**Where:** `crates/model/src/view3d/element_solids.rs:56-58` (`ElementSolids::volume` sums only `layers`; `open` pieces are left
out) and `quantities.rs:45,113` which use it unconditionally.

**What is wrong.** A piece `Solid3::prism` refuses (`SolidError::Degenerate`: a ring the triangulation does not cover, as an
acute mitre can make, or a top below the bottom at a corner by more than 1e-6 mm, as a wall attached to a roof or floor that
dips below its base can make) is recorded in `open`, cut in sections by the fallback, but contributes **zero** to the wall's or
floor's volume. Before, `volume_of` summed every prism. I did not build a model that reproduces it; it is reachable from user
geometry by the refusals listed in `solid.rs:445-501`.

**Fix.** In `quantities`, when `!es.is_whole()`, add the open pieces' volumes the old way (for a wall,
`wall_volume_by_prisms` restricted to the open pieces, or simply fall back to `wall_volume_by_prisms` for that wall), and log it
in the family/model check so it is seen. A test: a wall attached to a floor above whose sloped underside passes below the wall's
base for part of its length; volume equals the prisms' sum.

## 7. Release workflow: the riscv64 cross build will not find a C++ cross compiler; macOS universal unverified

**Where:** `.github/workflows/release.yml:216-256` (`linux-riscv64` installs `gcc-riscv64-linux-gnu` only) and `:270-298`
(macOS universal builds `x86_64-apple-darwin` on an arm runner).

**What is wrong.** Every native build now runs CMake on Manifold (C++17). The riscv64 job has no `g++-riscv64-linux-gnu`, so the
`cmake` crate's `riscv64-linux-gnu-g++` is missing and `manifold-csg-sys`'s build script fails. macOS: cmake-rs cross to
x86_64 on arm64 usually works but has not been run. CI (`ci.yml`) is fine: ubuntu-22.04, windows-latest and macos-15 runners have
cmake, git and a C++17 compiler, and the build needs only network to github.com (+ about 1.5 min cold).

**Fix.** Add `g++-riscv64-linux-gnu` to that job's apt line (and `CXX_riscv64gc_unknown_linux_gnu=riscv64-linux-gnu-g++` if the
cmake crate does not infer it); run the release workflow once by `workflow_dispatch` before the next tag. Check whether
`packaging/linux/arch-package.sh` compiles (it runs in an Arch container with `CARGO_TARGET_DIR`): if it does, the container
needs `cmake` and `git`.

## 8. Build: clean builds now need github.com and are pinned by tag only

**Where:** `vendor/manifold-csg-sys/build/fetch.rs:36-58` (native: `git clone` Manifold, `git checkout v3.5.5`),
`build/wasm.rs` (wasm: clones wasm-cxx-shim; CMake FetchContent clones Manifold and Clipper2).

**What it means.** A native build for someone without LLVM 20 works (only cmake, git, a C++ compiler; checked: LLVM is needed
only for `wasm32-unknown-unknown`, through `unstable-wasm-uu`). But any clean build (new target dir, `cargo clean`, CI) clones
three repositories by tag: no offline build, and the C++ is not covered by `Cargo.lock` (a moved tag changes the engine). Also,
`crates/geom/Cargo.toml` sends every wasm target other than `wasm32-unknown-unknown` (wasip1/wasip2) down the native path, which
cannot work; no target in the workspace does that today.

**Fix (not blocking).** Pin by commit SHA instead of tag in the vendored copy (`MANIFOLD_VERSION`, the shim's and Clipper2's),
and document `MANIFOLD_CSG_LIB_DIR` for offline native builds. Narrow the geom dependency's cfg to say plainly that other wasm
targets are unsupported (a `compile_error!` in `geom::solid` under `cfg(all(target_arch = "wasm32", not(target_os =
"unknown")))`).

## 9. Docs out of date about the sample windows

**Where:** `docs/manifold-elements.md` (table: sample house "4: the casement and fixed windows' family forms ... not closed"),
`docs/manifold-sections.md` ("A window's family forms are open meshes in the sample ... still shows none of them cut", the
house-sec4 caption), and the comment in `section_cut_tests.rs` `a_section_cuts_the_samples_doors_and_windows`.

**What is wrong.** On the branch head every door and window of the sample house has closed layer solids and no open piece
(throwaway test printing `element_solids` of each: windows 100018-100022 have 1-2 layers, 0 open). So windows are cut now; the
docs and the pictures' captions say otherwise. Re-render house-sec4/6 after fix 1 and correct the text.

## 10. For the next step (issuable vector sections): two things in the way

* `crates/model/src/render/section_cut.rs:89-165`: a loop with a hole, or one that runs past a cropped line's end, is chopped into
  vertical trapezoids. Fine for poché, but a vector drawing wants the real outline and its holes (one closed polyline each, hatch
  aligned to the whole region, no seams to suppress). Keep `Loop`s (outer + holes) as the cut's data and do the trapezoids only
  where the raster fill needs simple rings.
* `Solid3::slice` calls `self.mesh()` (Manifold's `to_meshgl64`, a full copy) on every slice, and `volume_on_curves` does too.
  Elevations and plans will slice the same solids at several planes: cache the mesh once per `Solid3` (a `OnceLock<Mesh>` beside
  `m`). This is most of the "warm" 50 ms in the doc's dev timings.

---

## The door threshold (asked)

Revit draws no wall there. A door whose opening reaches the wall's base leaves no wall under it; a section through the opening
cuts the wall only above the head, and the line at floor level is the floor's own cut (its top edge), plus the door family's
threshold if the family models one (a door is cuttable, so a threshold form is cut). The old zero-area ring inked a line that was
the wall's, which Revit does not have. Removing it is right, as long as the floor is cut under the opening, which it is in the
pictures.

## Fine as is

* **Origins through booleans** (`solid.rs`): every constructor reserves its own Manifold original id and maps it; booleans merge
  the tables; `mesh()` maps Manifold's runs back; `transform` keeps the ids and maps the curve tags only for maps that keep vertical
  cylinders cylinders. Tables are per solid, as large as the element's input pieces: no global table to prune.
* **Arc recovery** (`slice.rs`, `curved.rs`): facets recognised by distance band (r cos 15° .. r) and all within the circle;
  radial jambs and wall ends fall outside it; when a band's two radii both qualify the nearer is taken; runs require the same
  origin, centre, radius and turn; ends snapped to where the next straight edge meets the circle within 2 sagittas. The
  closed-form sliver for `volume_on_curves` is right (checked the antiderivatives). Only level planes give arcs; sections are
  vertical, so today nothing flows from it into a drawing.
* **Slice topology**: one above/below classification per vertex and one cut point per edge make loops close exactly; a face on
  the plane is cut from behind, consistently, and `reaches()` uses the same side rule.
* **Scene cache invalidation** (`update.rs` + `SolidCache::kept`): solids are dropped exactly when `made_again` remakes the
  triangles, which covers joined neighbours (box reach + `walls_near_at`), hosts and hosted (refs/referrers in both models: a
  door moved to another wall remakes both walls), level edits (referrers of the level), undo/redo (just another pair of
  snapshots); library edits (types, layers, materials, families) are a whole scene, so an empty cache. `SectionWork` is per
  snapshot (`GeometryKey`), so it can never be stale; it is only slow (finding 4).
* **Panics from user data**: constructors reject NaN/huge coordinates, self-crossing or zero-area rings, tops under bottoms, open
  meshes, all as `SolidError`, and the callers fall back; `Frame::new` rejects a zero or NaN direction; no `unwrap`/index panics
  on the slice/arc paths (checked every index). The only `assert!` is behind the `SectionLayers` selfcheck. One hardening
  nicety: `cuboid_transformed` checks the corners but not the matrix; a NaN frame from a degenerate wall axis reaches Manifold
  (which, as far as I know, empties a non-finite manifold rather than throwing). Cheap to check `matrix.0` is finite.
* **C++ aborts in wasm**: Manifold 3.5 returns status codes on bad input; with `-fno-exceptions` only implicit STL throws (out of
  memory, `at()`) abort, and those already trap in Rust too. Nothing user-reachable found that makes Manifold throw.
* **Family voids** (`cutting.rs`): order kept (a void cuts only forms listed before it); open forms or voids fall back to
  `csg::subtract` exactly as before (test proves byte-identical output); a void that takes nothing keeps the form's own
  triangles; the family check's new warnings never refuse.
* **Section rings**: hole-to-outer assignment by the smallest containing outer; crop to the line's range; layer interface lines
  once per boundary; old `SectionWalls` geometry replaced by the solids' own rings; Coarse still skips layers.
* **Quantities' numbers**: where every piece is a solid, the tests hold walls to the prisms to 1e-9, curved walls and round
  columns to the circle, roofs and floors to their analytic values. (Speed: finding 3; open pieces: finding 6.)
* **Workspace wiring**: `[patch.crates-io]` with the vendored crate (excluded from the workspace, so its lints are its own);
  the native dependency has `parallel` off (no TBB); a plain native build does not need LLVM; wasm needs LLVM 20 + libc++
  headers + Ninja as documented in `docs/manifold-trial.md`; the Arqiflow server image downloads a built wasm and needs nothing.
* **Tests run**: `cargo test -p bimcraft-core --lib -- section_cut element_solids quantities` 35 passed / 1 ignored;
  `cargo test -p bimcraft-geom --lib solid` 23 passed.

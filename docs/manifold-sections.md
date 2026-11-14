# Section cuts from the element solids

Step 4 of `docs/superpowers/plans/2026-11-14-manifold-core.md` (in Arqiflow). Code: `crates/model/src/render/section_cut.rs` (the
cut of one element, the rings, the lines between layers) and `cut_model` in `crates/model/src/render.rs` (what a section's line cuts
of the model, and how `section_base_cancellable` draws it); tests in `crates/core/src/section_cut_tests.rs` and the unit tests at the
foot of `section_cut.rs`. It builds on `docs/manifold-solid.md` (`Solid3::slice`, `Frame::vertical`) and `docs/manifold-elements.md`
(`element_solids`).

## What changed

A section's cut was `Solid::cuts`, one scene piece at a time: analytic for prisms, slabs and boxes, convex hulls for the few
meshes that say they may be cut. A component, a family door or window, any family form was never cut.

It is now the section's vertical plane (`Frame::vertical(through, direction)`, through `v.start + u * first` along the section's
unit direction `u`, as the line was cut from before) sliced through every layer's closed solid. Per element the plane crosses:

1. `Scene`-style cached `element_solids` (`SectionWork::solid_cache`, a `view3d::SolidCache` that lives as long as the work does,
   which is of one model snapshot; the 3D view's `Scene` is not at hand in a section, only the work is; what the next snapshot's
   work takes over of it is under "Across edits"). An element is asked for only if it is shown by the view, is not a mass
   (outlined below, as before), is one Revit cuts (`graphics::cuttable`, below), and its box in plan is not all on one side of the
   line (the old `one_side` test, which stays); then a layer's solid is sliced only if its bounds meet the plane.
   Only what the section draws is cut: a family's form that says (Visibility Settings) it is not drawn at the section's detail
   level, or not in a view looking at the family's front or its side, is not part of the solid that is sliced
   (`ElementSolids::drawn_layers`, `docs/manifold-elements.md`), as it is not part of the picture beyond.
2. `Solid3::slice` gives closed loops, the solid on their left. Each loop with no hole, inside the line's range, becomes one ring
   as it is. A loop with a hole in it (a section lengthwise through a window) is cut into trapezoids that abut, by the vertical
   lines through every corner of it and of the hole, so every ring the drawing is handed is simple, as the prisms' were. A loop
   that runs past a cropped section's ends is cut at them the same way.
3. An arc a slice returns (only a level plane cuts a vertical cylinder in one; a section's plane is vertical, so none comes back
   from it today) is flattened to within 0.01 mm of its circle; the drawing has no arc primitive.
4. What is left open (`ElementSolids::open`: a pipe's or conduit's tube, a family form with holes in its skin) is cut as it was:
   `Solid::cuts` of that piece, so nothing that was cut before disappears. Round runs that the work has not made yet (deferred
   preparation) are cut as before, entirely.

The loops are also kept as they are, regions of an outline and the holes in it, for what draws vector sections:
`render::section_cut_regions(m, section)` (`CutRegions`: element, layer, `Region { outer, holes }`), with
`section_cut_regions_with(m, work, section)` for a work that has the solids. A window along a wall is a hole in the wall's
region, not four trapezoids. A region that runs past a cropped line's end is cut to it as trapezoids with no holes, as the rings
are; a bounded vector section would crop the solid before slicing it. The rings above are made from the regions.

Every `Solid3` keeps the surface Manifold gives it (`Solid3::surface`, a `OnceLock` shared by the solid's clones) once, where
`slice` and `triangles` copied Manifold's mesh on every call: a section's warm cut, and every plan or elevation that will slice the
same solids, reads it.

The rest is as it was: the poché fills all of an element's rings as one (nonzero fill, per colour), the edge is `cut_edges` (an
outline's stretches that another's lies against on the outside are not inked: where layers and elements meet), a bar's cuts are
dots. What changed in the drawing is how a compound wall, floor, roof or ceiling shows its layers:

* The material's cut pattern (or the light poché of a finish layer) goes over each layer's own rings (`cut_layer`), as it did over
  the part of the wall's ring between where the line crossed two boundaries. Its room (is it wide enough to hold a pattern) is the
  ring's own width, `2 area / perimeter`; its courses are level in a wall and along the slope of a floor, roof or ceiling.
* The thin line between two layers (`section_cut::interfaces`) is each ring's edge that a ring of the next layer lies against, as
  long as they lie together, drawn once. It replaces the lines worked out from the wall's layer geometry and from a floor's top
  and bottom points (`section_walls`, its selfcheck twin `SectionWalls`, the floors' four-corner block): a wall's layers at a join,
  a floor's shaped layers, a roof's, a hole through a floor now all come from the solids.

The selfcheck twin that replaces `SectionWalls` is `SectionLayers`: a layer solid whose bounds the plane passes by is cut by none
of it (its slice is empty).

## What is not changed

The picture of what lies beyond (`section_picture_in`, the hidden-line raster) is exactly as it was, and so is everything that
reads it (regions' ink, images). The cut is made in the same plane and at the same origin, so the poché sits where it did: the
pictures below are the proof. Plans, the 3D view, glTF and IFC are untouched. Sheets, PDF and SVG take the section's drawing from
the same `section_base_cancellable`, so they change with it, and with nothing else.

## Differences from before, and why

* **What Revit cuts is cut.** Components and family doors and windows were never cut before; they are now where Revit cuts their
  category (`graphics::cuttable`, from Revit 2025's help, "About Cuttable Families" and "About Non-Cuttable Families"). A door's
  or window's family, Casework, Generic Models, and every element that is not a family component are cut. Furniture (with
  Furniture Systems and Specialty Equipment) and Plumbing Fixtures (the engine's `Plumbing` and `Drain`) are cut only when the
  family has Revit's per-family "Enable Cutting in Views" on (`Family::cut_in_views`, off by default, set in the Family Editor's
  header or by the `family` command's `cut_in_views`, read from a Revit definition's `cut_in_views` or `enable_cutting_in_views`
  when it says; a project that has one is written as format 25). Pipe fittings and accessories, electrical equipment, outlets,
  switches, data and voice outlets, lighting, patch panels, racks, air terminals, earthing and inspection boxes are never cut: they
  stay in the picture beyond the line, as before the solids. In the sample house that is the sofa, desk, chairs, table and bed (in
  projection, as `house-sec2` to `house-sec6` show), and the base cabinet (casework), the family doors and the windows are cut:
  5 more elements for the lines tried than the old cut, all of them doors and the cabinet. (Revit's non-cuttable list also holds
  Entourage, Planting, Parking and Mechanical Equipment; the engine has no component category for them.) The old per-piece `cut`
  flag of a mesh (lofted masses, which are outlined, not filled, here) is not what decides a component any more.
* **A form is cut only where it is drawn.** A form that says it is drawn at Fine only is not cut in a Coarse or Medium section, one
  drawn in no front view is not cut in a section that looks at the family's front. Before, the pieces of a family were united and
  cut whatever they said, and a door's frame and handle were filled in a Coarse section that does not draw them.
* **No sliver across a door's threshold.** A wall's piece over an opening that reaches the floor is a prism whose top meets its
  bottom everywhere: the old cut was a ring of no area, which inked a line across the opening at floor level (`curved-sec1`
  before shows it under the door). Such a piece holds nothing and the solids leave it out.
* **No corners where a floor's triangles meet.** A floor's section was cut piece by piece, so a straight underside had a corner
  under each triangle's seam (`a_section_shows_the_screed_tapering...` asserted one at the drain). The layer is one solid, so its
  underside is one edge. The two `shape_tests` now ask whether the point lies on the edge or the line, not whether a corner is
  there; what they check (the top falls to the drain, the underside does not, the layers follow the top or stay level) is the same.
* **Interface lines are exact to the facets**, not recomputed from the layer geometry: in a curved wall, one pixel's difference at
  most (`curved-sec3`).
* `components_show_in_plan_from_above_in_3d_and_beyond_a_section`: its line runs through the table's top, which is now cut, so the
  test asks for a cut between 1000 and 2000 along the line where it asked for none.

## Timings

`crates/core/examples/section_cut_timings.rs` (`cargo run -p bimcraft-core --release --example section_cut_timings`): a section
through the middle of each model, along x. One thread. "Cold" is the first section of a fresh `SectionWork`: it makes the element solids of
every element the line crosses; "warm" is the same section again, the solids kept. The old figure is `Solid::cuts` of every piece
the scene holds, as the old loop did.

| Model | Elements | Elements cut | Rings | Work prepared | Cut, cold | Cut, warm | Cut before the solids | Whole overlay, cold | Whole overlay, warm |
|---|---|---|---|---|---|---|---|---|---|
| Highrise, 20 storeys | 9,522 | 500 | 620 | 6.6 ms | 189 ms | 4.7 ms | 13.3 ms (420 elements) | 191 ms | 10.7 ms |
| Tower, reference plate, one storey | 10,019 | 64 | 67 | 25 ms | 32 ms | 1.1 ms | 36.7 ms (56 elements) | 32 ms | 1.5 ms |
| Sample house | 75 | 7 | 13 | 2.4 ms | 5.1 ms | 0.05 ms | 2.2 ms (6 elements) | 5.2 ms | 0.23 ms |

Release build (`--release`, one thread, this PC, the C++ a release build either way), measured again after the cuttable rule and the
kept surface: the tower's 141 elements and 179 rings are 64 and 67 now (its furniture is in projection, not cut), and every warm cut is
a little faster for reading the solids' kept surfaces (6.2 to 4.7 ms in the highrise, 2.5 to 1.1 in the tower). The same figures in the
dev profile, which builds the engine's Rust at opt-level 1 (what `cargo test` runs; the same code, run as an ignored test before it was
moved into the example; not measured again: the cold and warm figures of the first cut, before the two changes above):

| Model | Cut, cold | Cut, warm | Cut before the solids | Whole overlay, cold | Whole overlay, warm |
|---|---|---|---|---|---|
| Highrise, 20 storeys | 265 ms | 50 ms | 20 ms | 389 ms | 183 ms |
| Tower, reference plate, one storey | 257 ms | 106 ms | 52 ms | 386 ms | 241 ms |
| Sample house | 25 ms | 7 ms | 6.7 ms | 39 ms | 21 ms |

*Whole overlay* is `SectionOverlay::new`: the cut, the datums, the grids, the layer patterns and the edges (`section_base`), with a
fit view of 1,600 x 900; *warm* is on a work that holds the solids already (the section was cut once on it). Cold is dominated by
Manifold making each crossed layer's solid (about 0.3 ms a layer solid: 500 elements, 620 rings); warm is the slice of each layer
(extracting Manifold's mesh and walking its triangles) and the drawing.

A section of the 9,521-element highrise builds the solids of the 500 elements it crosses, not the model's 9,521 (building all of
them is 4.4 s, `docs/manifold-elements.md`): the box tests (the old `one_side` test on the element's box, then the layer's bounds
against the plane) do that.

### Across edits

What `SectionWork::solid_cache` holds is for as long as the work is, and an edit makes a new work (one per model snapshot), so the
first section after every edit built every solid again: the cold column above. A new work now takes over from the old
(`SectionWork::solid_carry`, `inherit_solids`; the section worker, `ui-egui/src/section_worker.rs`, keeps the carry between its
snapshots, through a job cancelled part way) the solids of every element the edit left as it was. That is decided without a `Scene`
(there is none in a section) from what the new work has anyway, the pieces of every element:

* the model came from the old one by edits of elements only (`Model::changed_since`; a library or setting change, a type's layers,
  a material, a family, says everything, and nothing is taken over);
* the element is none of those edited, and none that points at or is pointed at by one (a door's wall, a level's elements);
* where the work holds the element's pieces (all but components, whose pieces are made only when asked for), they are the pieces the
  solids were made of. The pieces are what the 3D view makes of the element with its neighbours in it: a wall mitred where it
  joins, cut at its doors and windows, a floor less its shafts, attached to a roof. A wall that a new wall joins has other
  pieces, so it is made again though it was not edited, which is `Scene::update`'s `made_again` answered from the result instead
  of a simulation of it.

After an edit of one wall of the highrise's line (a move of 50 mm along x), release build, one thread (`section_cut_timings`
prints it for each model: the reference tower's plate 56 ms against 36 ms, 93 of 96 solids taken over; the sample house 9.0 ms
against 5.6 ms, 5 of 7; where there is little to build the preparation of the work, 25 ms in the tower, is most of it):

| | Work and cut |
|---|---|
| A fresh work (every solid again) | 189 ms |
| A work that took over the old solids | 17.4 ms (858 of 860 solids taken over, 2 made again) |

and the section is exactly what the fresh work cuts (`a_work_after_an_edit_takes_over_the_solids_of_what_the_edit_left_as_it_was`
holds every ring of the two to be equal, after a move, a join, an edit far from the line and a library edit).

## The pictures

`docs/manifold-sections/`, made by `bimcraft-cli run ... --cmd 'render {"view": "Section N", "path": ...}'` (the section's picture
with its drawing over it, as the editor shows it) before (the binary built at the head of step 3, `81e6db15`) and after:

* `house-sec1`: the sample house's own Section 1 (y = 3000, looking north): the walls' layers, the floor and roof as before; the
  two pictures differ in 36 pixels of ink.
* `house-sec2`: x = 3000: the sofa is in projection, as before the solids (Furniture is not cut unless its family says so).
* `house-sec3`: y = 1000: the desk against the east wall is in projection.
* `house-sec4`: x = 4500, through the plain door of the wall y = 4000 (the head cut over its opening, its leaf a thin cut slab), the
  casement window's family forms in the front wall (closed, cut: sill and head, the glass a thin cut) and the dining chairs and
  table (in projection).
  `house-sec5`: x = 2000, through the family door in the front wall (its leaf is a dark cut slab now) and the base cabinet at the
  back (Casework: cut, poché). `house-sec6`: y = 5250, through the door of the interior wall x = 6000 and the plain fixed window
  in the east wall (sill and head cut, the glass a thin cut), and the bed (in projection).
  The `after` pictures of the house were made again with the cuttable rule, by `bimcraft-cli run --sample` with the five sections
  added (x = 3000, y = 1000, x = 4500, x = 2000, y = 5250, each from -1500 to the far side of the house's 11500 or 8500) and `render`
  at 1600 x 900 (1800 x 900 for the three along x): `house-sec1` and `house-sec5` are pixel for pixel what they were, the other four
  differ from the pictures of the first cut only where the sofa (`house-sec2`), the desk (`house-sec3`), the chairs and the table
  (`house-sec4`) and the bed (`house-sec6`) were cut and are in projection again (4,396, 517, 1,441 and 7,221 pixels, in those
  places only). The curved pictures have no component and no family: they are as they were.
* `curved-sec1`: a plane at x = 2000 through the door at the apex of a half-circle wall of three layers (the sliver under the door
  is gone); `curved-sec3` and `curved-sec4`: a line across both legs of it at y = -1000, looking south and north, the layers cut on
  their facets, the window of the west leg cut across its glass; `curved-sec2` is empty before and after (the wall bulges south,
  the line looks north from y = 1000: nothing to cut).

The poché sits on the picture in every one: the cut's edges are the picture's silhouette edges, nothing is drawn twice, nothing that
was cut is missing but the sliver above.

## Tests

`crates/core/src/section_cut_tests.rs` (`cargo test -p bimcraft-core --lib section_cut_tests`), reading the rings with
`render::section_cut_rings` (exact coordinates, in the section's plane):

* A straight wall of three layers (20, 160, 20) with a door and a window; a plane across it through the door: one ring per layer,
  the core exactly 920 to 1080 by 2100 to 3000, the finishes 900 to 920 and 1080 to 1100, the opening open under the head; the
  door's leaf, which no section cut before, a rectangle from the floor to 2100 as thick as its leaf; through the window: two
  pieces per layer (under its sill, over its head) and the glass between.
* The same wall cut lengthwise (plane through the core): the window is a hole in the cut, the door a notch; the pieces' area is
  exactly 8 x 3 m less the door and the window (20,100,000 mm2 against 20,100,000), none of the finishes is cut, nothing covers
  the window or the door's opening.
* A cropped section: the cut stays between its line's ends (0 to 1000, area 1000 x 3000), the wall running past them.
* A half-circle wall of three layers with a door at its apex: a plane through the door (three upright rectangles over its head,
  abutting, the circles' radii to within the facets' 2.5 mm, every edge upright or level; the leaf a rectangle from the floor) and
  a line across both legs (three layers at each, each upright and level, widths from the circles).
* A family with a void inside it (a cavity): the cut is the cube's face less the cavity, four pieces of exactly 750,000 mm2; the
  middle of the cavity is not cut and has no fill in the drawing.
* A gable roof of 30 degrees with a wall attached under it: lengthwise through the wall, a pentagon with its corners at
  (500, 0), (6500, 0), (6500, 2577.35), (3500, 4309.40), (500, 2577.35); the roof's cut 8000 x 300 / cos 30 in area.
* The sample house, three lines of it: a family door, a plain door, the casement window's family forms and a plain window are cut
  (and walls, floor, roof, ceilings).
* Revit's cuttable categories: a 600 x 600 x 800 box family of every one of the 18 component categories, placed and cut through its
  middle: one ring each for Casework and Generic, none for the other 16; with `cut_in_views` on, one more each for Furniture, Plumbing
  and Drain (and none for the electrical, lighting, pipe, communication ones). The sample house's furniture is not cut by any of its
  lines and its base cabinet is.
* A form's Visibility Settings: a family of three boxes on a diagonal, the first drawn in no front or side view, the second at Fine
  only, the third in no front view: sections along x and along y through each, at Fine (none, the second, the second; none, the
  second, the second and the third) and at Medium and Coarse (none; only the third from the side). The engine's double door through
  a handle: both faces of the handle cut at Fine and Medium, none at Coarse, the leaf cut at all three.
* Across edits: a row of walls, the section of them and an edit (a move of one wall: all but that wall taken over; a wall joined to
  another's end: that one made again, the walls far away taken over; a wall far from the line: all taken over; a layer's thickness
  changed: none), each time the rings of the work that took solids over equal those of a fresh work, ring for ring.
* Regions: the same wall cut lengthwise through a window gives one outline of 8000 x 3000 with the window a hole of 1200 x 1500, where
  the rings are trapezoids round it. And in `bimcraft-geom`: a solid's surface is made once, shared by its clones, read by `slice`,
  and a solid made by an operation has its own.
* What was cut before is cut still: for seven lines through the sample house and three through the two-storey highrise, every
  element that `Solid::cuts` cut is cut now, with the area it had (no element differs by more than a millionth), and 5 and 16
  elements are cut that were not (5 in the house, over its seven lines: the family doors and the base cabinet; the highrise's are its family doors; the
  furniture is in projection). And every element a layer solid of which the plane crosses (its
  box a millimetre either side) has a cut: in the sample house 7, 8, 12, 9, 12, 10 and 11 elements cross its seven lines and as
  many are cut; in the highrise 42, 34 and 34 cross and 50, 42 and 34 are cut (the rest are open pieces and runs).
* `family_last_four_tests`: a family's `cut_in_views` alone writes the project as format 25 and reads back; `revit.rs`: a definition's
  `cut_in_views` or `enable_cutting_in_views` sets it, one that says nothing leaves it.
* Unit tests of the rings in `section_cut.rs`: a hole cut into four pieces; a ring cut to a range; an arc flattened within
  0.01 mm of its circle; a layer's width and courses; the line between two layers drawn once.

Numbers: `cargo test -p bimcraft-core --lib section_cut_tests`: 8 passed, 10.4 s (dev profile); `cargo test -p bimcraft-model --lib
section_cut`: 5 passed. The existing section, sheet, render, selfcheck, elevation, shape, graphics, regions, reinforce, rebar, mep,
structure, view3d, hatch, pattern, layer, element-solids and quantities tests (`cargo test -p bimcraft-core --lib -- section sheet render
selfcheck elevation shape_tests graphics regions reinforce rebar mep_tests structure_tests view3d hatch pattern layer element_solids
quantities`): 484 passed, 0 failed, 7 ignored.
`every_self_check_runs_under_the_core_tests` runs the new twin (`SectionLayers`) in the sample house and the highrise's sections.

## The browser

`bimcraft-web` built for wasm32-unknown-unknown in the `wasm` profile with the environment of `docs/manifold-trial.md`
(`unstable-wasm-uu`, LLVM 20), `wasm-bindgen --target nodejs`, and `web-demo/manifold-trial-node.cjs`: volume 803965.7193408788 (the
analytic value to the last digit), 272 triangles, 13.5 ms: the browser build still links and cuts with Manifold.

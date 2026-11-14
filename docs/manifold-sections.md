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
   which is of one model snapshot; the 3D view's `Scene` is not at hand in a section, only the work is). An element is asked for
   only if it is shown by the view, is not a mass (outlined below, as before), and its box in plan is not all on one side of the
   line (the old `one_side` test, which stays); then a layer's solid is sliced only if its bounds meet the plane.
2. `Solid3::slice` gives closed loops, the solid on their left. Each loop with no hole, inside the line's range, becomes one ring
   as it is. A loop with a hole in it (a section lengthwise through a window) is cut into trapezoids that abut, by the vertical
   lines through every corner of it and of the hole, so every ring the drawing is handed is simple, as the prisms' were. A loop
   that runs past a cropped section's ends is cut at them the same way.
3. An arc a slice returns (only a level plane cuts a vertical cylinder in one; a section's plane is vertical, so none comes back
   from it today) is flattened to within 0.01 mm of its circle; the drawing has no arc primitive.
4. What is left open (`ElementSolids::open`: a pipe's or conduit's tube, a family form with holes in its skin) is cut as it was:
   `Solid::cuts` of that piece, so nothing that was cut before disappears. Round runs that the work has not made yet (deferred
   preparation) are cut as before, entirely.

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

* **Everything is cut.** Components (furniture), family doors and windows, any family's closed forms are now cut and filled like the
  rest: in the sample house, 14 more elements for the lines tried (the sofa, the desk and the base cabinet are cut and filled
  where the line meets them; a family door's leaf is a thin cut slab). A window's family forms are open meshes in the sample
  (`docs/manifold-elements.md`), so a section through them still shows none of them cut: the fallback is `Solid::cuts`, which
  cuts a mesh only when its piece says it may be cut.
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
| Highrise, 20 storeys | 9,522 | 500 | 620 | 6.8 ms | 184 ms | 6.2 ms | 11.5 ms (420 elements) | 187 ms | 11.7 ms |
| Tower, reference plate, one storey | 10,019 | 141 | 179 | 23 ms | 92 ms | 2.5 ms | 33.9 ms (56 elements) | 93 ms | 3.0 ms |
| Sample house | 75 | 8 | 14 | 2.3 ms | 5.5 ms | 0.09 ms | 2.2 ms (6 elements) | 5.7 ms | 0.26 ms |

Release build (`--release`, one thread, this PC, the C++ a release build either way). The same figures in the dev profile, which
builds the engine's Rust at opt-level 1 (what `cargo test` runs; the same code, run as an ignored test before it was moved into the example):

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
against the plane) do that. What is kept in `SectionWork::solid_cache` is for as long as the work is; an edit makes a new work, so
the next section after an edit builds its solids again. Not done: keeping the solids across edits as `Scene::update` does
(`SolidCache::kept`); the work would have to know which elements an edit made again.

## The pictures

`docs/manifold-sections/`, made by `bimcraft-cli run ... --cmd 'render {"view": "Section N", "path": ...}'` (the section's picture
with its drawing over it, as the editor shows it) before (the binary built at the head of step 3, `81e6db15`) and after:

* `house-sec1`: the sample house's own Section 1 (y = 3000, looking north): the walls' layers, the floor and roof as before; the
  two pictures differ in 36 pixels of ink.
* `house-sec2`: x = 3000: the sofa is cut now (a stepped poché against the interior wall's, which is unchanged).
* `house-sec3`: y = 1000: the desk against the east wall is cut (its top and its side panel, in the cut pen).
* `house-sec4`: x = 4500, through the plain door of the wall y = 4000 (the head cut over its opening, its leaf a thin cut slab), the
  casement window's family form in the front wall (open, so still not cut) and the dining chairs and table (cut).
  `house-sec5`: x = 2000, through the family door in the front wall (its leaf is a dark cut slab now) and the base cabinet at the
  back (poché). `house-sec6`: y = 5250, through the door of the interior wall x = 6000 and the plain fixed window in the east wall
  (sill and head cut, the glass a thin cut), and the bed.
* `curved-sec1`: a plane at x = 2000 through the door at the apex of a half-circle wall of three layers (the sliver under the door
  is gone); `curved-sec3` and `curved-sec4`: a line across both legs of it at y = -1000, looking south and north, the layers cut on
  their facets, the window of the west leg cut across its glass; `curved-sec2` is empty before and after (the wall bulges south,
  the line looks north from y = 1000: nothing to cut).

The poché sits on the picture in every one: the cut's edges are the picture's silhouette edges, nothing is drawn twice, nothing that
was cut is missing but the sliver above.

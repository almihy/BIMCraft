# One closed solid per element: `view3d::element_solids`

Step 3 of `docs/superpowers/plans/2026-11-14-manifold-core.md` (in Arqiflow). Code: `crates/model/src/view3d/element_solids.rs`,
with `layered_indexed` in `view3d.rs`, the cache in `Scene` (`view3d/update.rs`), `Solid3::volume_on_curves` in
`crates/geom/src/solid/curved.rs`, the readers in `crates/model/src/quantities.rs`, and the tests in
`crates/core/src/element_solids_tests.rs` and `crates/geom/src/solid/tests.rs`. It builds on `docs/manifold-solid.md`.

**What changes and what does not.** Nothing the 3D view, glTF and IFC draw changes: they still make their triangles from the
scene's pieces (`Solid::Box`, `Slab`, `Prism`, `Mesh`) and group their faces by plane (`PlaneTable`). The element solids are a
second reading of the same pieces, for sections (step 4) and for quantities (here).

## The API

```rust
// bimcraft_model::view3d (re-exported by bimcraft-core)
pub fn element_solids(m: &Model, id: Id) -> ElementSolids;
pub fn element_solids_in(m: &Model, id: Id, index: Option<&walls::Index>, c: &mut Components) -> ElementSolids; // many elements
pub fn wall_volume(m: &Model, id: Id, openings: bool) -> Option<f64>;       // with its doors and windows cut out, or without

pub struct ElementSolids {
    pub layers: Vec<(u16, Solid3)>, // one closed solid per layer that has pieces
    pub open: Vec<OpenPiece>,       // pieces that are not solids, and why
    pub shown: Vec<ShownSolid>,     // where some pieces are drawn in some views only: the layers again, by the way they are shown
}
impl ElementSolids {
    fn volume(&self) -> f64; fn layer(&self, u16) -> Option<&Solid3>; fn is_whole(&self) -> bool;
    fn drawn_layers(&self, drawn: &dyn Fn(Vis) -> bool) -> Vec<(u16, Solid3)>; // what a view draws, for a section to cut
}
pub struct ShownSolid { pub layer: u16, pub vis: Vis, pub solid: Solid3 }
pub struct OpenPiece { pub layer: u16, pub why: SolidError, pub piece: Solid /* as the scene holds it */ }

// the cache, on the scene
Scene::element_solids(&self, m: &Model, id: Id) -> Arc<ElementSolids>;
Scene::solids_kept(&self) -> usize;

// bimcraft_geom::solid
Solid3::volume_on_curves(&self) -> f64;  // the volume with every tagged curved face the cylinder it lies on
```

Every `Solid3` carries `Origin { element: id, layer, curve }`, so any triangle of it and any edge of its section can be traced to
the element and the layer, and a curved face to its arc.

## What each element becomes

The pieces are today's (`solids_in_with`, then `layered_indexed`, the one function `layered` is made of); the pieces of a layer are
unioned with `Solid3::union_all` (one Manifold batch operation).

| Element | Pieces | Layers (`Origin.layer`) | Curve tag |
|---|---|---|---|
| Wall | `walls::prisms_in`: abutting prisms cut at the doors' and windows' jambs and heads and at an arc's chords, mitred at joins, top and bottom on the planes of what it is attached to | the layer of its type (finish, core, finish), counted as the type lists them, exterior first | a curved wall's layer: `Curve::band(centre, inner, outer)` with that layer's two radii |
| Floor, ceiling, roof, toposolid | slabs (a face and its rise) or prisms (under a variable layer), less the shafts through them, each type layer a stacked slab | the layer of its type, top down | none |
| Column, structural column, footing, building pad, mass, site areas | a slab of the outline over its span | 0 | a round column: `Curve::cylinder(at, width / 2)` |
| Stairs, ramps, railings | a box per riser or a slab per landing; run prisms; rail parts | 0 | none |
| Beams and rectangular runs | a prism of the outline | 0 | none |
| Door, window | a family's forms (meshes), or a plain type's leaf / glass (a box in the wall) | one layer per material, in the order they first appear | none |
| Component (furniture) | the family's forms, each a mesh | one layer per material | none |
| Round runs, bars (rebar), lofted masses | meshes (tubes, skins) | by material / 0 | none |

A prism becomes `Solid3::prism` (its ring between its bottom and top planes, either sloped), a box `Solid3::cuboid_transformed`
(the box's own axes turned into the plan), a slab a prism between its face's plane and that plane lifted by `rise`, a mesh
`Solid3::from_triangles`.

### Visibility Settings: `shown`

A family's form says which views draw it (Revit's Family Element Visibility Settings: the detail levels, and a view along the
family's front or back, left or right; `Vis`, carried by the piece). The picture beyond a section already honours that
(`Look::draws`). The layer solids unite every piece of a layer whatever it says, which is what a quantity wants, so where any
piece of an element is drawn in some views only, `build` keeps the layers again by the way they are shown: one `ShownSolid` per
layer and `Vis` (a layer drawn one way throughout is its own solid again, no second union). `ElementSolids::drawn_layers` gives
what a view draws, the pieces it draws united, for a section to cut; it is the layers themselves for every element whose pieces
are drawn everywhere, which is every element but a family's. The engine's door families draw their handles at Medium and Fine
only, a Coarse section through one does not cut it.

## What stays open, and why

A piece is left out of its layer's solid, and recorded in `ElementSolids::open` with the reason (`SolidError`) and the piece itself
(`Solid`), when it is not a closed solid:

- a **triangle mesh with holes or edges used three times** (some family forms, a file's soup): `SolidError::NotClosed` with the
  open and non-manifold edge counts. Step 4 falls back on `Solid::cuts` for it (which cuts a mesh only where its piece's `cut` is on:
  a pipe's or conduit's tube).
- a **prism with no thickness** (a roof piece whose top meets its bottom everywhere), a slab on an upright face:
  `SolidError::Degenerate`.

Also left out, and not recorded as open: a piece that holds nothing. A wall's piece over an opening that reaches its base (or
under one that reaches its top) is a prism whose two planes meet everywhere; the 3D view keeps it as a flat sliver, it has no
volume, and the sample house has seven of them. Such a piece (top no higher than bottom, to `tol::THICKNESS`) is simply not made.

What the sample models leave open (every element is otherwise closed, tested by `every_element_of_*_has_closed_solids_or_a_recorded_open_piece`):

| Model | Elements | With solids | Layer solids | Open pieces |
|---|---|---|---|---|
| Sample house (`sample::SAMPLE`) | 74 | 33 | 52 | none: every door and window, the family ones too, is closed (the casement and fixed windows' forms once were not; a test holds the house to none) |
| Tower, small plate, one storey | 2,953 | 2,050 | 2,475 | 645: 292 furniture and fixture forms (`component`), 210 conduit tubes, 143 pipe tubes (`mesh: not closed`) |
| Highrise, two storeys | 953 | 950 | 1,214 | none |

Walls, floors, ceilings, roofs, columns (round and square), structural columns, beams, stairs with railings, ramps, building pads,
doors and windows without a family: every piece closed (the samples hold most; `columns_stairs_ramps_beams_and_pads_are_solids...`
makes the others). A round column is a 24-gon on its circle, tagged so, and measures pi r2 through its height. A tube of a round run is an open sleeve (its ends are not capped), as the view draws it.
A closed family form is a solid; the open ones are not unioned (step 4 falls back on today's cuts for them, and the family
check of step 2 says which form is open).

Every element's wall volume agrees with the prisms'. Summed over every wall: the small tower 323,710,000,000 mm3 by the solids and
by the prisms (1.5e-15 apart), the two-storey highrise 401,760,000,000 mm3 (identical), the sample house 32,969,953,607.015 mm3
(identical); each wall to 1e-9.

## Quantities

`quantities::wall`, `floor`, `ceiling`, `roof` read their volumes from the element's solids (`floor_at`, `ceiling_at`, `roof_at`
take the id, which `floor` and the others look for by where the reference is); `quantities::column` is new. Gross volumes of
walls are the solids of the wall without its openings. Areas are what they were. A toposolid's volume and a floor's
gross volume are what they were (the ground's cut by pads is a different piece, the gross volume of a shaped floor comes from its
polyhedron).

`wall_volume_by_prisms` keeps the old figure (the prisms' plan through their height, a curved wall's plan true to its arc) for
the tests to hold the new one to.

### Why `volume_on_curves`, and not `Solid3::volume`

A curved wall is built as straight facets (a chord a millimetre off its arc, never turning more than 5 degrees), which Manifold
measures. The arcs' volume is larger by a segment per facet: 0.13 % of a half-circle wall. The quantities tests hold a curved
wall to the circle's volume to 1e-6, and Revit's is the circle's, so `volume_on_curves` adds to Manifold's volume, in closed form
and for the part of a facet that is left after a door has cut it, the volume between each facet triangle and its arc
(subtracting it on the inside face, where the solid is the far side of the arc). A facet is recognised as the sections recognise
one (`docs/manifold-solid.md`, "Arcs"): a vertical triangle of an origin with a curve tag, in a plane within `r cos 15 degrees`
and `r` of the axis, all of it within the cylinder.

## The cache

`Scene` holds a `SolidCache`: element id to `Arc<ElementSolids>`, made when first asked for (`Scene::element_solids(m, id)`, `m`
the model the scene is of) and kept for as long as the scene's triangles of that element are. `Scene::update` (`update.rs`) knows
exactly which elements an edit made again (`made_again`: those it touched, what points at them or at what they point at, and
those that touch them, a wall's joins, a slab a shaft cuts, until no more change; or the whole scene); the cache of the new scene
keeps the entries of every element not made again and still in the model, and drops the rest. A whole scene starts empty. An
element's solids are therefore recomputed exactly when its triangles are.

A section does not use the scene's cache (it has no scene: its `SectionWork` holds its own `SolidCache` per model snapshot), and
carries what an edit left as it was into the next snapshot's work from the pieces of each element instead (`SectionWork::inherit_solids`,
`docs/manifold-sections.md`, "Across edits").

`quantities` and the exporters do not use the cache (they take a `Model`, not a scene): each makes the element's solids
for the figure it asks.

## Timings

`element_solids_timings` (`cargo test -p bimcraft-core --lib element_solids_timings -- --ignored --nocapture`): every element of
each sample, one thread. A debug build of the engine's Rust (Manifold's C++ is a release build in either, `CMAKE_BUILD_TYPE`),
on a PC that was building another helper's branch at the time: the figures are good to the first digit.

| Model | Elements | Pieces (made in) | Layer solids | Triangles | Cold | First, through a scene | Warm (cache) | The 3D view's whole scene |
|---|---|---|---|---|---|---|---|---|
| Highrise, 20 storeys (9,521 elements) | 9,521 | 28,460 (38 ms) | 12,140 | 481,840 | 4.42 s | 4.73 s | 1.3 ms | 359 ms |
| Tower, reference plate, one storey | 10,018 | 16,610 (161 ms) | 8,336 | 684,776 | 6.34 s | 6.73 s | 1.2 ms | 933 ms |
| Sample house | 74 | 136 (3.2 ms) | 52 | 2,418 | 36 ms | 35 ms | 8 us | 6.5 ms |

*Cold* builds each element's solids, with the wall index and the families shared, and asks each solid for its triangles (Manifold's
booleans are lazy: the work is done by the first question). *First, through a scene* is the same through `Scene::element_solids`
(each asks for its own families; the wall index is made once, on the first). *Warm* is every element asked of a scene that holds
them all: a map lookup, 0.1 us each. About 0.35 ms per layer solid, 0.7 ms in the tower, nearly all of it in Manifold making a
solid of each piece and uniting them: making the pieces is under a tenth.

So a model's every solid is 4 to 6 s once, where its triangles are 0.4 to 0.9 s: the solids are for what a section or a schedule
asks of, one element at a time, not for a scene to make of the whole model. The cache makes that cost once per element and keeps it
across edits, except for the elements an edit made again. Not done: making them on the render pool in chunks of elements, as the scene
does (a Manifold solid is `Send + Sync`; its `parallel` feature stays off), which would take the whole-model figure down by the cores
the pool has.

## Tests

`crates/core/src/element_solids_tests.rs`: a straight three-layer wall (20, 160, 20) with a door and a window: each layer's volume
exact (0 off), the box of each the wall's length, height and the layer's thickness, the door's leaf and the window's glass; a
half-circle curved wall with a door: closed, 3,349,911,184.307763 mm3 against the analytic ...752 (3.4e-15 off), its facets alone
0.13 % short, the curve tag on its triangles (centre, radii 1900 and 2100), a plan slice at 1500 mm of four arcs of exactly those
radii, the one run of wall over the door's head at 2500; a holed floor of two layers less a shaft: exact, closed, and what the floor
measured before (`floor_volume`); a gable roof of 30 degrees with a wall attached under it: the roof 27,712,812,921.102047 mm3
against ...036 (4e-16), the wall 4,132,050,807.568878 against ...877; the cache (kept through an edit that touches one wall of
sixteen, remade for the neighbour a new wall joins, equal to a whole new scene's); a round column, a square one, a structural
column, a stair, a ramp, a beam and a pad, each sound; every element of the sample house, the tower
and the highrise sound; origins through a union. `crates/geom/src/solid/tests.rs`:
`case1_volume_on_curves_is_the_arcs_volume` (the bake-off's case 1 at 4.6e-11 where the facets were 2.1e-4 off, a 24-gon column to
the circle exactly, a sloped quarter ring to 1.5e-7).

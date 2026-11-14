# `bimcraft-geom::solid`: the one doorway to Manifold

Step 1 of `docs/superpowers/plans/2026-11-14-manifold-core.md` (in Arqiflow). Code: `crates/geom/src/solid.rs`, with
`solid/slice.rs` (sections), `solid/mesh.rs` (triangle soup in) and `solid/tests.rs`. The trial that gated this is in
`docs/manifold-trial.md`.

**This module is the only code in the engine that names `manifold_csg`.** Nothing outside it imports a Manifold type; the
workspace's `unsafe_code = "forbid"` still holds in every member crate (only the dependency has unsafe code). Everything below
is millimetres, x east, y north, z up.

## The API

### Types

| Type | What it is |
|---|---|
| `Solid3` | A closed solid (or nothing), cheap to clone (Manifold shares the mesh), `Send + Sync`. |
| `Origin { element: u64, layer: u16, curve: Option<Curve> }` | What a piece of a solid is of. `element` is the model's `Id` (geom cannot depend on model, so the caller fills it). `Copy`. |
| `Curve { centre: P2, radii: [f64; 2] }` | "This input's curved faces lie on vertical cylinders about `centre` of these radii." `Curve::cylinder(c, r)` for one, `Curve::band(c, inner, outer)` for a curved wall's two faces. |
| `Level { z, g: P2 }` | A plane over the plan, height `z + g . q` (the model's `walls::Plane`). |
| `Matrix([[f64; 4]; 3])` | An affine map, rows as `x' = m[0][0] x + m[0][1] y + m[0][2] z + m[0][3]`. `translation`, `rotation_z`, `scale`, `then`, `apply`. |
| `Frame` | A section plane and its 2D axes (below). |
| `Loop { segs: Vec<Seg> }`, `Seg::{Line, Arc}` | A section's closed loops. |
| `Tri { p: [P3; 3], origin, normal }`, `Mesh { verts, tris, origins }` | The surface out. |
| `SolidError` | `Degenerate(why)`, `NotClosed { open_edges, non_manifold_edges, triangles }`, `Manifold(why)`. `Display` says why. |
| `Bounds3 { min, max }` | A bounding box. |

### Constructors (each takes an `Origin`; each returns `Result<Solid3, SolidError>`, never panics)

- `Solid3::prism(ring: &[P2], bottom: Level, top: Level, origin)`: a plan ring (either winding) between two planes, either
  of which may slope (the same description as `view3d::Solid::Prism` and `walls::Plane`). Repeated corners and corners on a
  straight edge are dropped; a ring that crosses itself or has no area, a top below the bottom anywhere, and a prism with no
  thickness are refused. Where the top meets the bottom at a corner (a wedge, a wall running out at a roof) the two share
  that vertex.
- `Solid3::cuboid(a, b, origin)` and `Solid3::cuboid_transformed(a, b, &Matrix, origin)`.
- `Solid3::cylinder(centre, radius, z0, z1, segments, origin)`: a round column, `segments` facets inscribed in the circle
  (a vertex at angle 0). It tags the origin with `Curve::cylinder(centre, radius)` unless the origin already has a curve.
- `Solid3::from_triangles(&[[P3; 3]], origin)`: a triangle soup. Corners within `tol::WELD` are welded, triangles with a
  repeated corner dropped, a mesh wound inside out is turned over. When it is not a closed solid it is refused with the
  number of **open edges** (used by one triangle only) and **non-manifold edges** (used by more than two triangles, or twice
  in the same direction), in the error and its message. A vertex where two cones meet is refused by Manifold itself
  (`SolidError::Manifold`).
- `Solid3::empty()`.

### Operations

- `union`, `difference`, `intersection` (binary); `Solid3::union_all(iter)`, `Solid3::intersection_all(iter)` and
  `a.difference_all(cutters)` (Manifold's batch booleans: one operation, far faster than a chain: the tower's windows are one
  call per wall).
- `translate`, `transform(&Matrix)` (mirrors included).
- `volume()`, `surface_area()`, `bounds()`, `is_empty()`, `triangle_count()`, `is_closed()`.
- `triangles() -> Vec<Tri>` (each with its `Origin` and outward unit normal), `mesh() -> Mesh` (indexed, with an origin per
  triangle).
- `slice(&Frame) -> Vec<Loop>`.

## Origins

Every constructor registers its input with Manifold as an *original* (Manifold's `originalID`) and the `Solid3` keeps a table
`original id -> Origin` (an `Arc<HashMap>`). Manifold carries the original id of every triangle through every boolean, transform
and simplification; `mesh()` reads it from the `MeshGL64`'s runs (`run_original_id` with `run_index`, which gives each run's
first triangle) and looks each run up in the table. Every boolean merges its operands' tables (sharing the `Arc` when
nothing is new), so the table is always a superset of the ids in the mesh. A triangle whose id is not in the table (which
does not happen) would carry `Origin::NONE` (`element == u64::MAX`); the tests assert it never appears.

`transform` carries the curve tag along when the map keeps vertical cylinders cylinders: a turn about the vertical axis, a move,
a mirror, a uniform scale of the plan (any scale of z). The centre and radii are mapped. Under any other map (a non-uniform
plan scale, a tilt) the tag is dropped and the faces come back as lines.

The table is never pruned: a solid that has been cut and cut keeps the entries of inputs that have since vanished. It is a few
dozen bytes per input.

## Sections: `slice(&Frame)`

### The frame

`Frame { origin, u, v }`: unit, square axes. Loops are in `(x, y)` with `x` along `u` and `y` along `v`, measured from `origin`:
the model point is `origin + x u + y v`. The plane's normal is `u x v`, pointing **at the viewer**: the section is looked at
along `-normal`, `u` to the right and `v` up.

- `Frame::plan(z)`: the plan at height `z` from above; `(x, y)` are the model's own.
- `Frame::vertical(through: P2, direction: P2)`: the vertical plane through `through` along `direction`: `x` along `direction`
  from `through`, `y` is the model's z; for `direction` east it looks north (a south elevation).
- `Frame::new(origin, u, v)`: any plane (`v` is made square to `u`).

A plane that coincides with faces of the solid cuts **just behind itself**, away from the viewer: a vertex exactly on the plane
counts as in front of it. The solid behind a plane it touches is cut at its face (a cube's top face, seen from above, is a
full section); the solid in front of it is not (its bottom face, seen from above, is empty). One rule, applied to every vertex, so
neighbouring triangles always agree and the loops close exactly.

### The loops

Each loop is closed, its edges end to end, the solid on its **left**: an outer loop runs counter-clockwise, a hole clockwise
(`Loop::area()` is positive for the one and negative for the other; `is_hole()`). `Loop::area()` is exact for arcs.

`Seg::Line { a, b, origin }` or `Seg::Arc { centre, radius, start, end, ccw, origin }`. An arc's ends are points on the
circle, not angles (loops stay connected); `Seg::sweep()` gives the signed sweep. A circle all the way round comes back as two
arcs.

### How the edges and their origins are found (and why not Manifold's `slice`)

Manifold's `slice` returns polygons with no ids: an edge could not be traced to its input. So the surface's triangles are cut
against the plane here. A vertex is classified once (above the plane or not, `>= 0` counts as above); a triangle with vertices
on both sides gives one piece, from the edge where its surface goes down through the plane to the edge where it comes back up
(so the solid is on the piece's left); each cut edge's point is computed once, from its lower-indexed end, so the two triangles
that share it get the same bits; and pieces join by the *edge* they end on, not by comparing coordinates, so the loops close
exactly with no tolerance to guess. The origin of a piece is its triangle's.

Then, per loop: pieces of no length (`tol::ZERO_LENGTH`; a plane through a vertex leaves one) are dropped; consecutive pieces
of one origin on one line (`tol::LINE`) become one line; and, for a level plane, curved pieces become arcs.

### Arcs

For a **horizontal** plane (normal vertical, either way) a vertical cylinder is cut in a circle. A straight piece of an
origin that has a `Curve` is a facet (or part of one, where something else cut the facet) of the tagged cylinder when:

- both its ends are within the cylinder (at most `tol::CURVE` outside it),
- the line through it is at a distance from the axis within `[r cos(MAX_FACET_ANGLE/2), r]`: a chord of that circle turning no
  more than `tol::MAX_FACET_ANGLE` (30 degrees),
- for the radius of the two that its far end is closest to.

A radial line (an end of the wall, a door's jamb) is at distance zero from the axis, so it is never a facet. Consecutive facets of the
same origin, circle and sense of turning become one `Arc` with `radius` exactly the tag's. The arc's ends: a facet's end
vertex is on the circle already; where the run stops at a cut (a door's jamb through the middle of a facet, so the end is on the
chord, up to a sagitta inside the circle), the end is moved **along the straight edge that goes on from it** to where that edge
meets the circle (the jamb stays one exact line, from circle to circle); only if that edge is not near (or is not a line) it is
moved straight out from the centre. A run broken by the loop's start is handled (the loop is rotated to a break).

This is the bake-off's method (case 7: "Manifold keeps no curve information") made exact, and automatic: the bake-off had to
be told the circle by the caller and snapped ends radially; here the origin carries it and the ends follow their neighbours.

**Not recovered** (they come back as lines, correctly): any plane that is not horizontal. A vertical plane through the axis cuts
a cylinder in two lines; a vertical plane that misses it, also lines; a slanted plane cuts it in an ellipse, which is not an
arc and is left as the facets' lines. That follows the plan: "only recover circles".

An input's facets must have their vertices on the tagged cylinders (within `tol::CURVE`): the engine's curved walls
(`walls::prisms_in`) are built that way, with arc points on the true circle.

## Tolerances (all in `solid::tol`, millimetres)

| Constant | Value | Where |
|---|---|---|
| `WELD` | 1e-3 | triangle-mesh vertices closer than this are one vertex |
| `THICKNESS` | 1e-6 | a prism thinner than this at a ring corner has top and bottom meeting there |
| `RING` | 1e-7 | a ring corner this near a line through its neighbours, or to the corner before, is dropped |
| `LINE` | 1e-6 | section edges of one origin within this of a straight line are one line |
| `ZERO_LENGTH` | 1e-9 | section edges shorter than this are dropped |
| `CURVE` | 1e-4 | how far outside its cylinder a curved face's vertex may be and still be on it |
| `MAX_FACET_ANGLE` | 30 degrees | the most a facet of a curved face subtends at the axis |

Manifold's own epsilon is not set by this module: it takes it from the solid's size (about 1e-15 of the bounding box, which
is far below the engine's millimetre).

## What Manifold could not do, and what we do about it

- **It carries no curve information.** Its solids are meshes. The tag in `Origin` and the arc recovery above are ours.
- **Its slice has no ids.** Hence the section is cut here (above).
- **It requires a closed, oriented manifold in.** A mesh that is not (a Revit or file triangle soup with holes) cannot be
  cut by it: `from_triangles` refuses, with the reason, and the caller keeps today's `csg::subtract` for it (step 2 of the plan).
- **It does not check self-intersection.** A prism ring that crosses itself is refused here (its triangulation does not
  cover its area); a mesh that intersects itself is not detected.
- **Booleans are lazy**: the work happens at the first question (`triangle_count`, `volume`, `mesh`). Time a boolean with one.
- **`parallel` is off** (no TBB), natively and in the browser: single-threaded. See `docs/manifold-trial.md`.
- **C++ exceptions abort in the browser** (an out-of-memory is a wasm trap the page already handles by reloading).
- **Coplanar and touching faces**: `union` of boxes that touch on a face (the layers of a wall) works, and its volume is
  exact; the seam stays visible in `triangles()` as faces of both origins only where the faces are not interior. A section
  of the union shows only the outline; poché by layer comes from slicing each layer's own solid (step 4).

## Tests (`cargo test -p bimcraft-geom solid`)

The geometry bake-off's cases (`geometry-bakeoff/harness/cases.json`, `analytic.py`) built through the API and checked against
the analytic volumes, within the harness's tolerance, every result closed: case 1 (curved wall with door), 2 (walls trimmed by a
sloped plane, by intersection with a sloped-top prism), 3 (slab, shaft, three sleeves), 4 (L, T and X layered joins), 5 (straight and
arc curtain walls: 50 solids, the mullion groups unioned; the harness's 52 is wrong, 2 + 2 x 24 = 50), 6 (a three-storey tower).
Origins through a wall minus a door and through chains of booleans and transforms; the plan slice of case 1 at z = 1200
(four exact arcs, the bake-off's jamb point (4010.634, 4662.329) and its mirror); a vertical section through a box and through a
sloped-top prism; an oblique section; an open mesh refused with the reason; the wasm trial.

### Numbers (native, debug build, `cargo test -p bimcraft-geom solid -- --nocapture`; 22 tests pass, 0.08 s)

| Case | Volume | Analytic | Off by | Allowed (harness) |
|---|---|---|---|---|
| 1 curved wall with door | 7,913,614,801.8 | 7,915,300,164.7 | 2.1e-4 | 5e-3 |
| 2 walls under a sloped plane (intersection) | 44,430,212,701.289 | 44,430,212,701.289 | 0 | 1e-4 |
| 3 slab, shaft, three sleeves (32-gons) | 73,406,356,645.4 | 73,405,752,220.4 | 8.2e-6 | 5e-4 |
| 4 L, T, X layered joins | 26,718,750,000 | 26,718,750,000 | 0 | 1e-4 |
| 5 straight mullions / glass | 958,875,000 / 2,463,840,000 | same | 0 | 1e-4 |
| 5 arc mullions / glass / all 50 solids | 958,902,954.7 / 2,463,686,514.8 / 6,845,304,469.4 | 958,875,000 / 2,463,840,000 / 6,845,430,000 | 2.9e-5 / 6.2e-5 / 1.8e-5 | 5e-3 |
| 6 tower, 3 storeys, 123 solids | 1,049,832,000,000 | 1,049,832,000,000 | 0 | 1e-4 |

Case 1's plan slice at z = 1200: two loops, four arcs of radius 5850 and 6150 about (0, 0), the worst endpoint 9e-13 mm off its
circle; the jamb endpoints are (4010.6343784, 4662.3290191) and (4662.3290191, 4010.6343784) at the outer radius (the bake-off's
point and its mirror) and (3814.9936770, 4434.8983353) and its mirror at the inner; the loops' area is the exact
sector area (2,557,433.388231 mm2, to 1e-9).


### The browser

`bimcraft-web` built for wasm32-unknown-unknown in the `wasm` profile (5 m 27 s with a cold target dir for the Rust crates, the
C++ already built; `unstable-wasm-uu`, the environment of `docs/manifold-trial.md`), `wasm-bindgen 0.2.129 --target nodejs`, and
`web-demo/manifold-trial-node.cjs`: `manifold_trial()` (now a box minus `Solid3::cylinder`, through the doorway) gives volume
803965.7193408788 (the analytic value to the last digit) and 272 triangles, in 17 ms. `cargo check --workspace` and
`cargo clippy -p bimcraft-geom` are clean.

## Family voids (step 2)

`Family::build` cuts a form by its voids in `crates/model/src/family/cutting.rs`: a closed form and closed voids go to
`Solid3::from_triangles` and one `difference_all` (no triangle cap); a form or void that `from_triangles` refuses keeps
`csg::subtract` (cap 20,000), which stays for that alone. `Family::open_forms` names the parametric forms and voids that are not
closed and why; `check_type` puts them in `TypeCheck::warnings` (a warning, never a refusal). A piece carries one material, so
the cut faces take it without needing the origin map.

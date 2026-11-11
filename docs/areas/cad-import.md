# Importing and linking CAD drawings

On the roadmap under 0.9 Interoperability, as Revit's Import CAD and Link CAD: a consultant's or a surveyor's DXF drawing
brought into a project and drawn under the model in the plans of a level, to trace walls over and to check against. A project
holds each drawing once, an import element places it, plans draw and snap to it, and the desktop brings one in (Insert > Import
CAD, Link CAD). A DWG comes in as the DXF it becomes (below); DXF export, exploding an import into detail lines (which waits on detail lines, issue #58), and
images and PDFs as underlays are still to come. Walls, floors and the site's elements are made from its lines (Pick Lines, below).

## The reader

`dxf.rs` reads ASCII DXF of any version from Autodesk's public DXF reference: pairs of a group code and a value, read in
sections. From the header it takes `$INSUNITS` (the drawing's unit, `Dxf::unit` and `Dxf::scale`, mm to each of its units),
`$ACADVER` and `$LTSCALE`; from the tables its layers (colour, off when the colour is negative, frozen by flag, linetype, line
weight) and its linetypes' dash patterns; from the blocks section the blocks; and from the entities section what model space
draws (an entity with code 67 set is paper space's, and left out). Entities come in as a short list of `Shape`s, in the
drawing's own units: LINE, LWPOLYLINE and POLYLINE (bulges become arcs; a 3D mesh or polyface is left out), CIRCLE, ARC,
ELLIPSE (as a polyline), SPLINE (its fit points, else its control points), POINT, TEXT and MTEXT (its formatting codes taken
out, aligned as the file says), SOLID and TRACE, and HATCH (its boundary loops, solid or patterned; a patterned one is drawn as a
pale fill of its colour, since its pattern isn't kept). An INSERT places its block's entities, scaled, turned and arrayed, up to
`MAX_DEPTH` blocks deep; an entity on layer 0 of a block takes the insert's layer, and one by block its colour. DIMENSION draws
its anonymous block. What the reader doesn't know (3DFACE, a viewport, an image, a proxy) is counted by kind in
`Dxf::left_out`, and the command lists it. The text itself is read as UTF-8 where it is, and else by the code page the header names in
`$DWGCODEPAGE`: the East Asian pages and the Windows ones decode through `encoding_rs`, and a page without a decoder here (the DOS
ones) reads as Latin-1, as it always has.

A file is hostile: `MAX_BYTES` caps its size, `MAX_PAIRS` its pairs, `MAX_SHAPES` the shapes, `MAX_WORK` the points the blocks
may multiply into, and every coordinate is a finite number within `MAX_FILE_COORD` (one past that is left out as "past what a
drawing holds"). A file that ends after a group code, a value that isn't a number, a binary DXF and a DWG each say why.

## DWG

The core reads no DWG of its own. `crates/dwg` (`bimcraft-dwg`) is a bridge: the acadrust crate (pure Rust, MPL-2.0, used
unmodified as a separate library; its DWG code is ported from ACadSharp, MIT, after the Open Design Alliance's published DWG
specification; NOTICE carries both) reads the file in failsafe mode, and its DXF writer writes what it read as ASCII DXF, which
`dxf.rs` reads as any other. `Limits` holds a DWG to 64 MB before the decoder runs and its DXF to `dxf::MAX_BYTES` while it is
written; a read that left object records out says how many, of which kinds, in `import_cad`'s `notes` (the desktop puts them on
the status bar). The project holds the DXF, so a project file opens where no DWG reader is.

A session reads a DWG through the `DwgReader` it is given (`Session::dwg`; `read_dxf_file` in `commands.rs` is the one place
`import_cad`, `reload_cad` and opening a project read a drawing), and one without says to save it as DXF. The desktop and the
command line (so MCP too) hand theirs `bimcraft_dwg::reader()` behind their default `dwg` feature: it starts the program
itself as a child (`--dwg-to-dxf <path>`, answered by `child_main` before anything else runs) and reads the DXF from its
output. The decoder allocates what a file declares and can't be interrupted, so a hostile file could abort the process; in
the child it takes only the child, which is also stopped after `CHILD_TIME` or past the limits, and the reply says the file
couldn't be read. Tests make their DWG files from DXF text with the same library (`dwg_from_dxf`, the `testing` feature),
since Autodesk's are never committed; `BIMCRAFT_DWG_SAMPLES` names a folder of real ones for
`real_drawings_where_a_folder_of_them_is_named`, and `apps/bimcraft-cli/tests/dwg.rs` drives the child.

## The project

A project holds each drawing once, by name, as a `Cad` in `Model::cads` (a library entry of kind `Cad`, so it has an id and a
GUID; `cad.rs`): the bytes of its file, kept as they were and read again when the project opens (a file holds them deflated and in
base64, `CadFile`), the file's name, and, for a linked drawing, its path. `MAX_CADS` drawings and `MAX_HELD` bytes among them
are the most a project holds, and `cad::deserialize_cads` holds a file to them while it is read. `Cad::bulk` is the box round
what the drawing draws with what lies far from the rest left out (more than `FAR` times the spread of the middle of it from the
middle; a stray point at a survey's origin), which centring, zooming and the extents use; `far` counts what it left out.

A `CadImport` element places a drawing: on a `level`, in each of its plans or, with `view`, in that plan only (Revit's Current
view only), its `origin` (in the drawing's units) on `at` (mm), at a `scale` (mm to each of its units) and turned by `angle`
about `at`. It holds the layers it hides everywhere (`hidden`, by name) and how it colours (`colors`: preserve, invert, as on a
dark screen, or black). `cad_place` and `cad_unplace` take a point of the drawing to the model and back. `Model::check` holds it
to a drawing the project has, a level, a view of that level, a scale within `CAD_SCALE`, the names of layers it has, and its
bulk within `MAX_COORD` once placed (a survey's coordinates far off say to place it nearer the origin, with `position`: center).
The engine removes a drawing nothing places after each command, and forgets the layers views hide of it; undo brings both back.

## Commands

- `import_cad` reads a file (`path`) or places one the project holds again (`cad`). Its units are auto (the file's, else mm),
  a unit given, or a `scale`; `position` puts its middle (center, the default) or its origin on `at`; `layers` shows those its
  file has on (visible), all, or a list; `link` remembers the file. It returns what came in: the unit, the counts, the extents,
  the entities far off and what was left out.
- `cad_layers` lists an import's layers with their colours and linetypes and which are shown, and hides or shows them
  everywhere or, with `view`, in that view (its Visibility/Graphics, which `graphics` also sets with `layers`).
- `reload_cad` reads a linked drawing's file again; opening a project does so for each (`cads_not_reloaded` names those whose
  file is gone, which keep what they had).
- `cad_query` names the entity nearest a point: its layer, kind, colour and linetype (Revit's Query).
- `cad_outlines` lists the closed outlines an import's lines make, and `wall`, `floor`, `property_line`, `building_pad` and
  `toposolid` take a pick of its lines as `cad` (below).
- `move`, `rotate`, `copy` and `delete` take an import as any element; `mirror` is refused, since a drawing's text would read
  backwards; `modify` changes its level, `at`, `angle`, `scale` and `colors`.

## Building from a drawing (Pick Lines)

As Revit's Pick Lines, model elements are made from an import's lines rather than retyped (`cad/outlines.rs`, the commands
in `commands/cad_pick.rs`). A pick, `{id, entities?, layers?, box?}`, takes the import's entities `entities` names (their
index in `Dxf::entities`), on `layers`, lying wholly inside `box` (two corners, model mm), each only when given; with
neither `entities` nor `layers`, everything the import shows. Everything is placed by `cad_place` and in model mm.

- `cad_outlines {id, …the pick, min_area?}` (no undo step) lists the closed outlines the picked entities make. Lines,
  arcs and open polylines are joined where their ends meet within `outlines::SNAP` (5 mm) or one ends on another, and
  cut where they cross (`geom::arrangement`, the planar faces rooms are found with, with each face's segments kept);
  loose ends going nowhere are taken off and listed as `open` chains (`points`, `entities`). Each face is an outline,
  and when lines across a shape split it (a wall across a building) the whole shape is one too, marked `whole`. A closed
  polyline, a circle and each ring of a hatch are outlines as they are (`from`: lines, polyline, circle, hatch). An
  outline inside another is its hole (`holes`, clockwise); the same ring found twice, a hatch over the lines round it, is
  one outline of both. Each has `points` (counter-clockwise, to the hundredth), `area` (mm², less its holes), `area_m2`,
  `layer` (or `layers`), `entities` and `labels`: the drawing's texts, on any layer, whose point lies inside it (each
  line of an MTEXT is one). An arc is drawn in chords of 5°, none shorter than four snaps. Caps: `MAX_PICKED` entities,
  `MAX_SEGMENTS`, `MAX_RINGS` outlines, `MAX_LABELS` labels each; past them the pick is to be narrowed.
- `floor`, `property_line`, `building_pad` and `toposolid` take the pick as `cad`, instead of `boundary` (or `points`,
  `sketch`): their outline is the one outermost outline it makes, a floor's openings its holes. A pick making none says
  how many open chains it left; one making several says how many, and to pick one with `entities` or `box` as
  `cad_outlines` lists them. Their level is the import's unless `level` says.
- `wall` takes it as `cad` instead of `points`: one wall along each line, arc (a wall's `arc`, its sweep), polyline
  segment and half circle picked, with every other parameter of a wall; `skipped` counts what draws no line (a text, a
  point, a hatch). A polyline's bulges are read as short straight pieces, so each is a wall of its own.

The ground from the drawing (Revit's toposurface Create from Import) isn't built: the reader keeps every point in plan
(`Shape` is 2D; a LINE's, a POINT's and a polyline's z, codes 30 and 38, are not read), so a survey's 3D points and
contours have no heights here. It would need `dxf.rs` to keep z (an elevation on `Shape::Point`, `Line` and `Polyline`, or
a list of 3D points per entity), and then `toposolid` would take a pick as its `points`; spot heights written as text
(a TEXT of "123.45" by a point) could be read too, with a rule for which number belongs to which point.

## Drawing and snapping

`render::cad_imports` draws a plan's imports under all the rest but its scans, as Revit draws an import in the background: each
entity of a layer the import and the view show, in its colour (the view's ink when it has none or the import draws in black),
its pen (`Cad::pen`, the entity's or its layer's line weight) and its dashes (`Cad::dash`, its linetype by `$LTSCALE`, scaled to
the view, left solid when a dash would be under a pixel). Text is lettered with `font.rs` at its height, turned and aligned; a
solid hatch is filled. What lies outside the view is passed over, so a large site plan zoomed in draws only what's on screen. An
import is one element: picked by any of its lines, selected whole. Sections, elevations and the 3D view leave imports out, as
Revit's do for a drawing placed in a plan; IFC export leaves them out too. The category is `cadimport`, Revit's Imported
Categories: Visibility/Graphics hides it or halftones it in a view like any other.

`snap.rs` gathers a plan's imports' endpoints, midpoints and centres, and their segments for crossings and nearest points, so a
wall drawn from corner to corner of a drawing lies on its lines; a layer hidden isn't snapped to.

## The desktop

Insert > Link CAD (in the Link panel) and Import CAD (a panel of its own) ask for a file, then a box (`crates/ui-egui/src/cad.rs`)
for its name, colours, layers (visible or all), units (auto-detect, or one), positioning (center to center, or origin to origin)
and whether it's for the current view only; it runs `import_cad` on the plan on screen's level, selects what came in, and keeps
the box open with the reason when a file can't come in. The Properties palette shows an import's drawing, file, unit, counts,
level, position, angle and scale, and a drop-down for its colours. Visibility/Graphics has an Imported Categories tab: the
category's row, and under each drawing of the project a check box for each of its layers, cleared for those the view hides. A right click on linked drawings selected offers Reload CAD, which runs `reload_cad` for each.

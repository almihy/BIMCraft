# Importing and linking CAD drawings

On the roadmap under 0.9 Interoperability, as Revit's Import CAD and Link CAD: a consultant's or a surveyor's DXF drawing
brought into a project and drawn under the model in the plans of a level, to trace walls over and to check against. A project
holds each drawing once, an import element places it, plans draw and snap to it, and the desktop brings one in (Insert > Import
CAD, Link CAD). A DWG comes in as the DXF it becomes (below); DXF export and exploding an import into detail lines (which waits on detail lines, issue #58) are
still to come. Images and PDF pages come in as Revit's raster images (below). Walls, floors and the site's elements are made from its lines (Pick Lines, below).

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
`MAX_DEPTH` blocks deep; an entity on layer 0 of a block takes the insert's layer, and one by block its colour. Each placement
by model space (one for each cell of an array) is kept in `Dxf::inserts` as an `Insert`: the block's name, the INSERT's layer,
its insertion point (through the extrusion's mirror, when it has one), its turn in [0, 360), its scale, and the range of
`Dxf::entities` it placed, nested blocks included; a placement that placed nothing is not kept. DIMENSION draws
its anonymous block, which is no placement. What the reader doesn't know (3DFACE, a viewport, an image, a proxy) is counted by kind in
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
- `cad_read` gives an agent the drawing's raw contents as they lie in the model (mm, through `cad_place` and the import's
  angle, to the hundredth), every layer read whether shown or not. Without `layers`, `kinds`, `box` or `start` it sums the
  drawing up: its extents, its layers with their kinds counted (`block` counting the placements on a layer), its blocks by
  how often they're placed with their size, and the first 300 texts. With any of them it lists what matches in file order, a
  page at a time (`start`, `limit` up to 5000, `next`; a page also ends after 200,000 points): lines, polylines, circles,
  arcs, points, texts and hatches, each with `n`, its index in `Dxf::entities`, which stays the drawing's. A block placed in
  model space is one `block` item (`b`, its index in `Dxf::inserts`, its point, turn, scale, the corners of the box round
  what it placed in its own turned frame, and its range of `n`), unless `explode` lists its entities one by one.
  `cad::placed` and `cad::frame_corners` do the geometry.
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

## Images and PDFs

As Revit's Insert > Import PDF (2020) and Image, with Snap to PDF (2022). `raster.rs` holds them; `raster/pdf.rs` reads a
PDF through hayro (pure Rust, Apache-2.0 OR MIT, so the browser's engine reads one too; its stand-ins for the 14 standard
fonts are PDFium's, BSD, in NOTICE): a page's size (turned, its `/UserUnit` taken in), its pixels at a resolution, the scale
its viewport's measure states (`/VP` with a rectilinear `/Measure`, the real length of a point), and what it draws as
vectors: each stroked path (curves in chords of 0.05 mm), the outline of each fill, and each text it shows, in mm of the
paper from its bottom left. Only what shows is kept: what lies on the page (its crop box, as the picture has it) and inside
the clipping paths it sets (the box round each, within the one before), cut where it leaves them (strokes by Liang and
Barsky, fills' outlines by Sutherland and Hodgman). A hostile file can't hold the read up: a path whose control points lie
off the clip is dropped before it is worked, a curve far bigger than the page is halved until its parts near the page are
found (the rest become chords the clip drops), and the chords a page's curves may become (`MAX_WORK`), the points a path
(`MAX_PATH_POINTS`) and a page (`MAX_SEGMENTS`) may keep are capped; past them paths are left out with a note. Text set in a
character map BIMCraft doesn't hold (the CJK CMaps, left out of the build) is named in a note.

A project holds each image once, by name, as an `ImageType` in `Model::image_types` (a library entry of kind `image`; Manage
Images lists them): the file's name, the page and the resolution, its size in mm (a PDF's page; a picture's at 72 dpi, as
Revit takes every raster image), the scale the page states, its pixels as a PNG (grey when they are) and a PDF's vectors as a
`Cad` (a DXF in mm written of them: strokes on layer `Lines` in their colour and width, fills' outlines on `Fills`, texts on
`Text`; a DXF that can't be read back is said in the notes). The PDF itself is not kept. `MAX_PIXELS` (36 million) caps an
image: a page that would pass it at the resolution asked comes in at the finest that fits, said in the notes. Its pixels are
held in the channels they need (grey, RGB or RGBA), so a drawing's grey page holds a byte a pixel, encoded to PNG row by row
and decoded again only when first drawn. `MAX_TYPES` images and `MAX_HELD` (256 MB) of PNG among them are the most a project
holds: in base64, with the rest of the project, within the longest text a browser holds, which a project saved there is. A
file's image is checked as it opens (its size, resolution and stated scale), and one that says what no image is refuses the
file with what is wrong. An image stays when nothing places it, as Revit's does, until Manage Images deletes it. A project
with no images is written as format 19, so a build from before images opens it; one with images is format 20 (as is one
whose doors keep their From and To Room against their swing: [families](families.md)).

An `Element::Image` (`ImageInstance`, category `image`, Revit's Raster Images) places one in one view, as annotation is: a
plan (and its dependent views; a level's ceiling plan shares its plan's annotation), a section, an elevation or a sheet, in
the view's terms. It holds its middle `at`, `width` and `height`, `angle`, `mirrored`, `lock` (Lock Proportions), `snaps`
(Enable Snaps), `layer` (Draw Layer: background, behind the model, or foreground, in front of it; annotation draws over
both) and `order`, its place among its view's images (Revit's Arrange: "the draw order of raster images and PDF files can be
controlled in the same manner as detail elements"), which counts within its layer; a new one comes in front. `render::images` draws it (`raster::draw`: the image's nearest pixel at the level of its pyramid that is about the
view's, a PDF's levels keeping the darkest pixel so lines stay as it zooms out); a plan draws its background images after
its scans and CAD imports, its foreground ones before its regions; a section draws them first and last, its background ones
cleared where its picture shows the model (`behind_model`); a sheet under and over its views. It picks where nothing else
drawn near does (`Part::Picture`).

A PDF's lines are placed where the image shows them by `ImageInstance::lines_placed`, an import of the image's `Cad` (with
`CadImport::stretch` when its sides are scaled apart, and turned half round and flipped when mirrored), so everything an
import has works on them by the image's id: `cad_read`, `cad_query`, `cad_outlines`, the `cad` pick of `wall`, `floor` and
the rest (an image in a plan only), Align, and snapping (`snap.rs`, while `snaps`).

Commands (`commands/images.rs`): `import_pdf` and `import_image` (a file by `path`, or a held image by `image`, Revit's Place
Instance; in `view` at `at`), `pdf_pages`, `manage_images` (`delete`), `arrange` (Bring to Front, Bring Forward, Send Backward,
Send to Back), and `scale_elements` (Scale, RE, as Revit's for images, PDFs and CAD imports only: a factor, or two reference
points and a length). In a model view an image comes in at its paper times the view's scale, as Revit brings one in (its
help: set "the view scale to match the PDF before you place it", and scale it after by its Horizontal Scale or Scale); the
scale a page states is read, not taken (`stated_scale`, and `pdf_pages`), so the agent or the person gives it as `scale` to
bring the page in at true size. On a sheet it is its paper's size. `modify` takes `horizontal_scale` and `vertical_scale`, and sizes the other side in proportion
while `lock` is on (`images::sized`). `move`, `rotate`, `mirror`, `copy`, `pin` and `delete` take an image as any element.

A PDF is read where a hostile one can't take the editor down. A command reads it in its session (`raster::read_pdf`) unless
the session was given the page read elsewhere (`Session::give_pdf_page`, `give_pdf_outline`: a `PdfRead` holds the page's
PNG, its DXF and its notes, and comes over as bytes, checked as a file is); a session with `pdf.elsewhere` never reads one
itself. The window reads every PDF apart (`crates/ui-egui/src/pdfread.rs`, jobs of `raster::run_pdf_job`: the pages, a
thumbnail, a page): on the desktop in a thread of its own, where a panic is caught; in a page Arqiflow hosts (the browser,
where a panic aborts the WebAssembly instance) by the page, in a Web Worker with an instance of its own that calls the
build's `pdf_job` (`take_pdf_requests`, `answer_pdf_request`; `apps/bimcraft-web`'s `on_pdf` and `answer_pdf`), so a worker
that dies is lost alone and the window says the file could not be read. On the server, each session's engine runs in a
worker thread of its own already.

The desktop (`crates/ui-egui/src/images.rs`): Insert > Import PDF asks for a file (up to `MAX_FILE`), then a box of its pages'
thumbnails (read one at a time), Page to Import, Resolution (72, 150, 300 or 600 DPI, 150 to start) and Page Size; OK reads
the page apart while it follows the cursor, and a click before it is read places it once it is. Insert > Image asks for a
picture. Either follows the cursor (`Tool::PlaceImage`), its extents crossed corner to corner, until a click places it in the
plan, section, elevation or sheet on screen, selected. A selected image shows its frame and four corner grips (`image_grips`,
in every 2D canvas): a drag scales it about the corner across, in proportion while locked. The Arrange panel (an image
selected) has Bring to Front, Bring Forward, Send Backward and Send to Back; Insert > Manage Images lists the images with their count, Place Instance and
Delete. Properties shows Width, Height, Horizontal and Vertical Scale, Lock Proportions, Enable Snaps and Draw Layer. In a
page Arqiflow hosts, the file dialogs ask the page (`dialog.rs`), which gives the PDF or picture as bytes under a name.

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
the view, every dash, dot and gap of it: `Cad::dash`, left solid when its cycle would be under a few pixels). Text is lettered
with `font.rs` at its height and width factor (DXF group 41), turned, aligned and, backwards (group 71), mirrored; a hatch's
rings fill by odd parity, as DXF's do, so an island is a hole, and reading keeps the rings its style (group 75) fills, as
AutoCAD does (`dxf::styled`: Normal every island, Outermost the outer boundaries and their first islands, Ignore the outer
boundaries alone, nesting found by containment; a boundary written twice is kept once, so the two don't cancel); an entity's transparency (group 440) draws see-through. Only the
entities whose boxes reach the view are looked at (`Cad::entities_in`, a grid of the entities' boxes made the first time it is
asked, which snapping and `cad::nearest` use too), and a run's points under a pixel apart are drawn as one (`thinned`), so a
dense sheet draws quickly zoomed in and zoomed out. Each layer draws as the view's Imported Categories say (`ImportLook`:
the import's row and each layer's projection lines, colour, weight and pattern, and halftone; a layer's own win). An
import is one element: picked by any of its lines, selected whole. Sections, elevations and the 3D view leave imports out, as
Revit's do for a drawing placed in a plan; IFC export leaves them out too. The category is `cadimport`: hidden in a view
(Imported Categories' Show imported categories in this view cleared), every drawing and every PDF page's lines leave it
(`Look::draws_pdf_lines`), and it halftones like any other. `familyimport` is Imports in Families' row, a category hidden and
overridden as any; families bring in no CAD yet, so it draws nothing. An import's fills and hatches keep their own colours:
as in Revit, its Patterns can't be overridden (`ImportLook::check` takes lines and halftone only).

`snap.rs` gathers a plan's imports' endpoints, midpoints and centres, and their segments for crossings and nearest points, so a
wall drawn from corner to corner of a drawing lies on its lines; a layer hidden isn't snapped to.

## The desktop

Insert > Link CAD (in the Link panel) and Import CAD (a panel of its own) ask for a file, then a box (`crates/ui-egui/src/cad.rs`)
for its name, colours, layers (visible or all), units (auto-detect, or one), positioning (center to center, or origin to origin)
and whether it's for the current view only; it runs `import_cad` on the plan on screen's level, selects what came in, and keeps
the box open with the reason when a file can't come in. The Properties palette shows an import's drawing, file, unit, counts,
level, position, angle and scale, and a drop-down for its colours. Visibility/Graphics has an Imported Categories tab laid
out as Revit's (`graphics.rs` `imported`): Show imported categories in this view over it (`cadimport` hidden), then one table,
its columns Visibility, Projection/Surface (Lines, Patterns) and Halftone, of a row for each drawing of the project, each PDF
page drawn from its lines placed in the view, and Imports in Families, sorted by name. An import's row has its visibility,
its Projection Lines (Line Graphics) and its Halftone, the whole file's; folded under it, a row for each of its layers has its
visibility and Lines, its Halftone hatched, a layer's box cleared for those the view hides. Patterns is hatched on every row.
All, None and Invert under the table set the visibility of every row, an import's layers among them whether folded or not,
and Expand All unfolds every import; Object Styles… is in a box below. The box over the table and Imports in Families are
categories, which a view template carries when it controls `categories`; the imports' rows are what it carries when it
controls `imports` (V/G Overrides Import), and each row can be changed only where the view's template doesn't set it. A right click on linked drawings selected offers Reload CAD, which runs
`reload_cad` for each.

## Images and PDFs

As Revit's Insert > Import PDF (2020) and Image, with Snap to PDF (2022). `raster.rs` holds them; `raster/pdf.rs` reads a
PDF through hayro (pure Rust, Apache-2.0 OR MIT, so the browser's engine reads one too; its stand-ins for the 14 standard
fonts are PDFium's, BSD, in NOTICE): a page's size (turned, its `/UserUnit` taken in), its pixels at a resolution, the scale
its viewport's measure states (`/VP` with a rectilinear `/Measure`, the real length of a point), and what it draws as
vectors: each stroked path (curves in chords of 0.05 mm), the outline of each fill, and each text it shows, in mm of the
paper from its bottom left. Only what shows is kept: what lies on the page (its crop box, as the picture has it) and inside
the clipping paths it sets, whatever their shape and by their fill rule (`raster/pdf/shapes.rs`: a box clips by Liang and
Barsky and Sutherland and Hodgman, any other shape by `i_overlay`), so what is drawn, snapped to and read is only what shows.
A hostile file can't hold the read up: a path whose control points lie
off the clip is dropped before it is worked, a curve far bigger than the page is halved until its parts near the page are
found (the rest become chords the clip drops), and the chords a page's curves may become (`MAX_WORK`), the points a path
(`MAX_PATH_POINTS`) and a page (`MAX_SEGMENTS`) may keep are capped; past them paths are left out with a note. Text set in a
character map BIMCraft doesn't hold (the CJK CMaps, left out of the build) is named in a note.

A project holds each image once, by name, as an `ImageType` in `Model::image_types` (a library entry of kind `image`; Manage
Images lists them): the file's name, the page and the resolution, its size in mm (a PDF's page; a picture's at 72 dpi, as
Revit takes every raster image), the scale the page states, its pixels as a PNG (grey when they are) and a PDF's vectors as a
`Cad` held packed (`cad/packed.rs`: its layers, linetypes, polylines, fills and texts, every point to a micron as the step
from the one before, a few bytes each, deflated in the file; a dense A0 sheet's million and a half points are a few
megabytes, where their DXF would be forty; lines that can't be read back are said in the notes). The PDF itself is not kept. `MAX_PIXELS` (36 million) caps an
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

### A PDF's layers

Revit takes a PDF only as a picture and controls layers only for DWG (Visibility/Graphics > Imported Categories); here a PDF's
layers work as a DWG import's do. `pdf::Document` reads a file's optional content groups (`/OCProperties /OCGs`, their names,
and which its default configuration has off), and the vectors device (`Collect`) follows the page's marked content: each
stroke, fill and text records the layer it is drawn on (`Run::layer`, `Text::layer`). An `/OC` sequence inside another is on
its own group (the innermost wins); an OCMD is on its first group (of `/OCGs`, else of its visibility expression `/VE`);
what is drawn on no group is on `No layer`. A form XObject's own `/OC`, and an annotation's, are told to the device as a
`BDC /OC` is by hayro-interpret, vendored with that change (`vendor/hayro-interpret`, `[patch.crates-io]` in the root
`Cargo.toml`, its changes marked "BIMCraft's patch"), so what a form draws is on its layer. hayro skips what a group the
file has off draws, so the vectors are read from the file with one more revision appended (`Document::all_on`: a catalog
with no optional content, as an incremental update writes one); an encrypted file isn't revised (its key would be needed),
so what its off layers draw is left out, said in the notes. Every group the file lists is a layer of its drawing, drawn on
or not, each with the colour most of what is on it is drawn in. A page of a file with no layers has its lines grouped by
colour and weight instead (`colour_name`, the nearest of a few plain names: `Black 0.35`, `Red hairline`, `Grey fill`,
`Black text`).

The page is read as it draws (`Vectors::drawing`, in the page's order, `seq`): a fill by its rule, even-odd or non-zero, as
rings that fill by odd parity (`shapes::resolve`, so its holes stay holes and overlaps stay filled); paint's opacity (`ca`
and `CA`, and the transparency groups open) as the entity's; a gradient (a shading) in its mean colour over what it fills,
and a tiling pattern in the mean colour its cell paints, as opaque as the share of the cell it covers
(`raster/pdf/paint.rs`); a dash with all its dashes, its phase taken in; a line as wide as the page draws it, and where the
page stretches it more one way than the other, or makes it wider than the widest pen (`MAX_LINE_WIDTH`), its outline as a
fill; text at the width it runs (its squeeze, spacing and font: a width factor), from its first letter that isn't a space,
mirrored where the page mirrors it. A white fill over the page before anything else is drawn is the page's ground and is
left out: a drawing has no page. A letter with no Unicode is kept as its outline, filled; a Type 3 font's are counted
(`unlettered`).

A page with lines is drawn from them (`ImageType::drawn`): its pixels are then only its raster images, painted on nothing by
the `Pictures` device (each image's and stencil's nearest pixel, inside the box round each clip, no more than
`MAX_PAINTED` pixels) and cut to the part of the page they cover (`ImageType::covers`); a page with none has no picture, as a
DWG has no page. A small picture of the whole page (`thumb`, `THUMB` px a side) is kept for Manage Images, and the notes say
how many pictures there are, showing and hiding with the PDF and not by layer. A page keeps up to `MAX_SEGMENTS` (4 million)
points; one denser, or whose lines can't hold all it draws (text in a character map not held, Type 3 letters), keeps the
picture of it all, its notes saying so plainly, and its layers can't be shown and hidden (`cad_layers` says so); a page with
no lines (a scan) is its picture, as before. `render::images` draws a drawn page's picture, then its lines through
`cad_drawing` (the same code as an import's) at `Part::Picture`, so it still picks only where nothing else does, leaving out
the layers the image hides everywhere (`ImageInstance::hidden`) and those its view hides (`Graphics::pdf_layers`, by the
image's name, a library reference of kind `image`; `Look::hidden_pdf_layers`), as its view draws it
(`Graphics::pdf_imports`). The canvas keeps a drawing's triangles on the card even with pictures in it, each picture drawn
by egui in its place between them (`retained.rs`). A project with a drawn page, or a view or template that hides a page's
layers or says how an import draws, is written as format 21; one with images but none, 20. A template that controls `imports`
but holds none of that (every new one) doesn't force 21 by itself: written older, it leaves the group out of its `controls`,
which a build of 20 can't name, unless the drawings' layers it hides, or those of a view following it, would then show
differently (`Model::written_format`).

Commands: `import_pdf` lists the page's `layers`; those the file has off come in hidden in the view it is placed in, unticked
in its Imported Categories (on the instance, hidden wherever it shows, where that view has no graphics of its own);
`cad_layers` takes the image's `id` (hide and show wherever it shows, `show` showing it again in every view that hides it,
or with `view` in that view only) and gives each layer's colour; `graphics` takes `pdf_layers` (`{image: [layer, …] |
null}`) as it takes `layers`, and `pdf_imports` as `imports`; `cad_read` with `layers` gives one layer's lines and words;
`manage_images` lists each image's layers, deleting one (or taking its entry out of the library) forgets what views hid of
it, and `reload` reads one again from its file (Reload From, in Manage Images too) into the same image, its instances
staying, so a page that came in before its layers were kept comes in with them. As for a DWG, there is no Query or
right-click hide by layer in the window (an agent has `cad_query`).

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

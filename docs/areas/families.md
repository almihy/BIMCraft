# Families, forms, materials and textures

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A family (`Model::families`, `family.rs`) is parameters, types that set them and forms in its own axes: x along a
component's angle, y to its left, z up from its base; a form is a profile in a sketch plane (`xy` swept up, `xz` along y,
`yz` along x), its sizes numbers or formulas, or triangles as a file brings them. A component places a type;
`Model::component_pieces` are its forms where it stands. Plans draw them from above, a form's edges hidden under a higher
form of the same component; sections show them beyond their line and never cut them (a `Solid::Mesh` is cut only where its `cut`
says, as a round run's tube is); the
3D view takes a form's triangles that share an edge and turn under 20° there as one curved surface, so a curve draws no
edge between its facets (`view3d::facets`) and shades smoothly across them, each corner lit by the angle-weighted mean of
its own surface's normals there, so a crease keeps its step (`corner_normals`); IFC writes each form as a closed face set,
triangles turned out, and reads one family per family name the file gives.

A form is swept, or with an `axis` turned about it (`family::revolve`: `from` to `to` degrees, 32 steps to a turn, the profile to
one side of the axis); a `void` cuts the solid forms listed before it (`csg::subtract`: convex polygons in binary space
partitions, built and walked with a worklist rather than recursion and bounded by a `Meter`, so a hostile shape ends in an error;
a cut piece keeps its uncut outline in plan). A family of category `door` or `window` is hosted: its `Width` and `Height` (English
names, whatever the project's language) size the wall's opening, `Sill Height` is a window's default sill, its forms are drawn in
the opening's axes (x along the wall from the middle of the opening, y across it from the wall's middle plane and + toward its
exterior, z up from the sill) and its formulas read `Wall Thickness`; its `symbols` also draw in plan (a door's swing). Its `connectors` say what an electrical device draws ([structure and MEP](structure-and-mep.md)); a family file keeps them. An
`Opening` holds a `family`, or none (one of the project's own types): `Model::opening_dims` gives its size either way
(`opening_type` returns it by value), its flips are mirrors (`opening_local`), and `Element::family()` is the one way to ask what
family a component or an opening is of. Sections show a family opening's forms beyond their line and never cut them.
How a door or a window opens is its family's `operation` (`model::Operation`: swing, double-swing, sliding, double-sliding,
folding, fixed), or for one of the project's own types the type's (`OpeningType::operation`); none, a door swings. IFC writes
it as the door's `OperationType` (`Operation::ifc_door`, its side from `flip_hand`: `SLIDING_TO_RIGHT`) and a window's
`PartitioningType` (two leaves, `DOUBLE_PANEL_VERTICAL`); the import reads a door's back, or its `IfcDoorType`'s (IFC4) or
`IfcDoorStyle`'s (IFC2x3), onto the type it makes. Schedules have it as the `Operation` field, in the project's language.
A door of the project's own type is drawn in plan as Revit's door families draw theirs (`render::door_leaves`): each leaf a
panel as thick as the type's `thickness` (Revit's Thickness; none, or `null` given to `type`, `DOOR_THICKNESS`, 50 mm as the
Single-Flush's; `Model::check` refuses one on a window type, from a command or a file), outlined
in the door's cut pen, a swinging leaf open square to the wall with its arc in the projection pen, at every detail level (a
window, and a family's forms and symbols, are only their gap at coarse); a double door's two leaves from both jambs, a
sliding one's leaf in front of a fixed one, a double sliding one's two meeting in the wall's middle, a folding one's two
panels half folded toward the hinged jamb, a fixed one shut in the wall's middle. 3D views and IFC build the leaf shut, as
thick. Which way it is turned is `Model::opening_way`, which the plan reads: a door swings to its wall's left (start to
end) or, flipped facing, its right, hinged toward the wall's start or, flipped hand, its end; a window faces the wall's
left, or flipped its right. Its From Room and To Room are Revit's: set as it is placed, the To Room on the side a door
swings to or a window faces (its exterior, so a window faced out of a house has its room as From Room), and kept on the
element (`Opening::to_room_side`, the side of its wall) through `flip`, a wall's flip and `modify`, as Revit's do not
follow a flip; `flip_from_to_room` (Revit's Flip From/To Room, on the right-click menu too) swaps them. A file of format
19 has them from the swing, once, as it is read, and a project is written as 20 only when some door's or window's
To Room is not where its swing would put it (or it holds images: `Model::written_format`), so an older build opens the rest. `Rooms::from_to` finds the rooms on those sides (looked for `ROOM_PROBE`
past the faces at the middle, and on past any room-bounding wall alongside, a lining, as far again past its far face:
`Rooms::past_walls`), and `Rooms::swings_into` the room a door opens into now; both among the rooms standing in the
project's last phase, as Revit's FamilyInstance.FromRoom (`Rooms::room_in_last_phase`; a room stands from its Phase Created
up to its Phase Demolished, and only rooms standing in a phase together share a space, `rooms::share_a_phase`). Revit's
per-phase lookup (FromRoom[phase]) has no command yet. What wants an element's name, type or mark (the status bar, IDs of
Selection, a tag of its mark) reads `element_json_bare`, which finds none of its rooms; `element_json` says them as
`from_room`, `to_room` and `swings_into` ({id, guid, number, name}, null outside) with a door's `hinge`
(`Model::door_hinge`) and `swing` (left or right), Properties shows From Room and To Room read only under Phasing, and
door and window schedules have From Room: Number, From Room: Name, To Room: Number and To Room: Name. A
window's panel operation (`IfcWindowPanelProperties`, a sliding sash) needs an `IfcWindowType`, which the export doesn't write yet.
What BIMCraft has no element for comes in from IFC as a component of a generic family that remembers the class it came in as
(`Family::ifc`, an `IfcClass`: `{"class": "IFCPLATE", "predefined": "CURTAIN_PANEL"}`): a covering that is no ceiling, a curtain
wall's panels and mullions, members that don't frame as a beam ([IFC import](ifc-import.md)), plates, duct fittings, terrain (`IfcGeographicElement`), a lift, anything the file builds
that has a body BIMCraft can read. The import reads it, after every element of BIMCraft's own, through the same path as
furniture (`import::ifc`'s `component`, families settled by `settle`): its predefined type is its own, else its type object's,
`NOTDEFINED` left out; its object type is remembered where it says what it is (the predefined type is `USERDEFINED`, or it isn't
the name the family and type take), so a family is one per name and class, and one of another class takes the name numbered on
(`Fin (2)`). Never generic: openings and the other features, virtual elements, annotations, spatial elements, grids, ports,
the structural analysis model (`import::NOT_GENERIC`), a part of an element that came in (a wall's or a roof's), and an
assembly (`IfcCurtainWall`, `IfcElementAssembly`) whose parts came in, which is left out and listed as such; one whose parts
have no body comes in itself. What stays out is listed by class, with what has no body BIMCraft can read said so, and the
`open` reply's `generic` counts what came in. The export (`Family::ifc` and `ifc/classes.rs`, IFC4's element classes as
`ci/ifc4_classes.py` reads them from the schema) writes a generic family's components as the class remembered, its attributes
after `Tag` as IFC4 lays them out (an element assembly's place ahead of its predefined type, a door's sizes), the predefined
type where IFC4's class takes it (else `.NOTDEFINED.`; nothing for a class with none, `IfcFlowSegment`), the object type
remembered else the `Family:Type` name, and a type object only where IFC4 has a `<CLASS>TYPE` that isn't abstract (none for a
`*StandardCase` or a flow terminal), with the predefined type but `USERDEFINED`, which a type object would have to name. A
class IFC4 doesn't have (IFC4X3's `IfcKerb`) goes out as a proxy, `USERDEFINED`, its object type the class. The `family`
command sets it (`ifc`, written in capitals, `null` to forget it) and `Family::check` refuses what IFC can't write: a class that
isn't `IFC` and capitals, a predefined type of anything but capitals, digits and `_`, an object type of more than a line. A
family of another category keeps what it remembers but goes out as its category says. An older BIMCraft leaves `ifc` out of
a project and opens it, its components going out as proxies, so the field took no `FORMAT` step.
A material's `texture` (`texture.rs`) is a pattern drawn in code or a picture of `Model::images` (a PNG or JPEG comes in, is kept
as a PNG of at most 1024 px, its header checked before the decoder makes room for it), laid on a face at its `size` in mm by the
face's own axes (`view3d::basis`: across is level, up goes up it, east and north where it lies flat), sampled per pixel from the
depth buffer, in the 3D view alone, from a mip chain. A triangle carries a skin (`view3d::skin`): in its low 24 bits a texture id (one more than its material's place in the
library), in its high 8 its material's `transparency`; `Scene::new` resolves the textures. A material's `transparency` (percent,
0 to 100, 0 opaque and not written, the starter Glass 70) is how see-through the 3D view draws what is made of it, with no view
override: a layer's skin wins over its element's, paint gives a face its own, and a family-less window's glass takes the Glass
material's. IFC carries it in `IFCSURFACESTYLESHADING`'s second argument (0 opaque to 1 clear, `$` for opaque), read back from a
shading or a rendering. Only the nearest see-through surface is blended, so a glass wall's far face and stacked glass don't
accumulate; plans and sections ignore it. Paint (`paint.rs`) is `{face: material}` in an element's `params` under `Paint`, a reserved
name: a triangle's face is read from its winding, so every solid emits triangles turned out (`Solid::tris`, `prism`), and
`Image::face` says which face a pixel is on. Favorites (`favorites.rs`) are a file of the user's own (`default_path`:
`BIMCRAFT_FAVORITES`, else the configuration folder) with materials and their pictures, families and their materials; they change
a project only through the `favorite` command (`load` never overwrites without `replace`). The desktop's Family Editor
(`editor.rs`) edits text (`FormText`, `ParamText`, ...) and changes the project's family only through a `family` command; its
preview is a scratch `Session` of its own, and it says what's wrong inside the window (the status box, over the view's corner, would lie on its
buttons).

A type of a category or of a family is renamed with `rename_type` (`Model::move_entry` moves it in its map, and `rename_entry` the
library entry and every reference `Model::lib_refs_mut` visits, so it keeps its GUID). A type or a family goes with `delete_type` (one entry, refused while an element uses it, and for the last type of a category or of a family: a
tool needs one to place) and `purge` (every entry of the kinds asked that nothing uses). Use is what `Model::lib_refs_mut` visits, the one place
that names every reference to a library entry: `Model::library_uses` counts them on a copy of the model, `Model::unused_entries` is one pass of
what purge takes out (each category keeps its default's type, else its first; a family that goes takes its types), the command repeats it
for what a pass frees (a family's material, a type's), at most 16 times, on a copy that becomes the model only when it isn't a `dry_run`.
`Model::remove_entry` is the single place that knows where each kind lives. The desktop's Purge Unused window (`purge.rs`) lists a dry run
for the kinds checked and runs the same; the Project Browser's Families lists the system types and has the right-click Delete.

The `family` command works out the whole change before the model is touched (`commands.rs::family`), so reading one costs no undo step
and a family `Model::check` would refuse never enters it. `family {name}` alone reads: the definition with every form's and symbol's `id`
and `GUID`, each type's `values` worked out, and `warnings` — no `model_mut()`, no file. Without `name` it lists the project's families
with their category and how much of each there is; `save` still writes one to a file. `dry_run: true` builds the change it's given
locally and replies `ok`, `warnings`, `changes` (parameters and types added, taken away and changed; forms and symbols replaced, patched
and deleted by id; the rename) instead of doing it; a refusal comes back as an error exactly as it would without it, carrying every error
and warning found, not only the first. One form or symbol is patched by its `id` or `guid` (`form`/`symbol`: its fields merged over what
that one holds, a `null` taking one away, its place, id and GUID kept; `delete_form`/`delete_symbol` to take it away), where `forms` and
`symbols` still replace whole lists — `keep_parts` binds by place, so patching is what leaves the other parts' ids alone. Validation is
`family/validate.rs` (`Family::problems`): every formula read at once (`names`, unknown parameters besides a hosted family's
`Wall Thickness`, syntax errors with the character they stop at), then `Family::check`, plus geometry warnings best worked out around
what doesn't build — a void reaching no solid before it, a form wholly outside its opening, one thicker than its wall, a door's or
window's plan with no symbols — each aggregated as `(in type …)`. `Family::shapes` gives each form's box before any void cuts it, from
the same `solid` `build` uses. `render {family, type, mode}` draws a family on its own: a `Session::new()` of its own holding the
project's materials and images, a wall of `wall_thickness` mm for a hosted family (a floor of its own footprint otherwise) with the type
in it, then `render_view` on that model — `3d`, `plan` (what its symbols draw) or `elevation` (front-on) — and the project keeps no
element and no undo step from it, the way the Family Editor's own preview already worked (`editor.rs::scratch`).


## Plan symbols

A family's `symbols`, of any category, are what its plan draws besides its forms, in its own axes: each is exactly one of a
`line` (up to `MAX_PROFILE` points), an `arc`, a `text` or a `dot`, with `visible` (a yes/no parameter or a formula) for any.
`Family::marks` works them out, for a type's values, as `Mark`s: a `Mark::Line` (an arc as chords of `ARC_STEP` degrees; a
label as a `Mark::Label`, lettered with `font.rs` by `lettering` where it is drawn) or a `Mark::Dot`. A `text` is `{at, height, value, align?}`: capitals `height` mm
tall, `at` the middle of the line, started, centred or ended there (`Align`); `{Parameter Name}` in `value` reads that parameter
(`said_text`: a number as plainly as it reads, a material by its name), an unknown name or an unclosed `{` is refused by
`Family::check`, and the lettered text is at most `MAX_TEXT` characters. `at` is the middle of the line of text vertically, and `align`
sets the horizontal anchor. A `dot` is a `Circle`, drawn solid in ink. Everything
numeric is checked finite and within `MAX_COORD`; a label's height and a dot's radius must be above zero.

`Model::component_marks` places a component's marks (mirrored, then turned and moved) and `Model::opening_marks` an opening's,
and `render::marks` draws them: lines in the category's pen (`BY_CATEGORY`, so Object Styles and Visibility/Graphics apply), dots
as filled discs. Coarse detail leaves them out, unless a symbol's `show` says Coarse (below). A label's anchor is placed with the component (turned, mirrored, moved) but its
letters never are: they are always drawn at 0 degrees in the view, left to right with a horizontal baseline, however the component
stands (the simplest rule; no flipping by quadrant). Symbols are annotation of the component's level: they draw in that level's
plan and its ceiling plan whatever height the component's forms reach (a ceiling light shows its symbols in the floor plan) and
on no other level's; the forms are outlined by the old rule (whole while any of them is between the view's depth and cut plane,
and not in ceiling plans). A component with no forms draws its symbols the same way. IFC export ignores symbols (they are plan notation, not geometry); the Family Editor's Symbols tab lists
all four parts. Library parts: a symbol of any kind has its own id and GUID (`HasPart`, `keep_parts`). A file written with
`text` or `dot` symbols, or symbols on a category other than a door or a window, is read by an older version only with them left
out or refused, so the file format is unchanged.


## Families from Revit

A family exported from Revit by Arqiflow's add-in (the design: Arqiflow's `docs/superpowers/specs/2026-11-13-revit-families-design.md`,
section 1) is a BIMCraft family with fields a hand-made one leaves out, all optional and written only when not the default.
Revit's own parametric sketches aren't readable, so its geometry comes baked per type: a form or a symbol with `types` is
built only in those (`Form::of_type`, `Symbol::of_type`), so `Family::pieces`, `marks` and `shapes` take the type name
(`all_marks` checks every symbol of a type), and a family of many per-type forms builds only the type's own (`MAX_FORMS`
and `MAX_SYMBOLS` 4096, `MAX_PARAMETERS` 256, a symbol's line `MAX_SYMBOL_POINTS` 1024). `like: {new: old}` on `family`
gives a new type the old one's forms and symbols (`Family::type_like`; Type Properties' Duplicate sends it, and the Family
Editor's Duplicate, rename and Delete Type do the same to its lists); `rename_type` renames the type in the lists
(`Library::move_entry`, `Family::rename_type`) and a type taken away (`types: {name: null}`, `delete_type`, purge) takes
itself out of them, a form or symbol of it alone going with it (`Family::forget_type`), never left of no type and so of
every one. A form's `show` is Revit's Visibility Settings: the detail levels and the views that draw it, carried on its
`Piece` and, in the 3D scene, on its `Solid::Mesh` and every `Tri` as a `view3d::Vis` (its bits and the family's x axis in
plan). A plan draws a component's form when `show.plan` and its detail level say so (`Piece::in_plan`), a family opening's
at Coarse only where its `show` says Coarse (one without keeps the old rule, only its gap); a section or an elevation
draws one by `show.front` where it looks along the family's y within 45° and `show.side` along its x, at its detail level;
a 3D view by its own detail level, which a 3D view now has as Revit's do (`detail {view: "{3D}"}`, the view control bar;
Fine by default, as every BIMCraft view); IFC exports what Fine draws. `Look` carries the view's detail level and, for a
section or an elevation, the way it looks, and `Look::draws` is the one test a triangle passes (the picture, the section's
occluders, the card's mesh, whose placed copies fall back to triangles of their own where a copy has one not drawn). A
symbol's `view` is `plan`, `front` ((x, z) in the family's plane y = 0) or `side` ((y, z) in x = 0); `Model::elevation_lines`
places a component's or an opening's front or side lines in the model as its forms are (mirrored, flipped, turned), and
`render::elevation_symbols` draws them in the sections and elevations that see them past their line and within their
depth, in the category's projection pen, so the back or the left sees them mirrored (a door's Elevation Swing); they are
not hidden behind what stands in front of them. A symbol's `show` gives its detail levels, Coarse among them; one without
is drawn at Medium and Fine, as before. A mesh form's `outline` is its silhouette from above as the exporter traced it, in
place of the hull. A door's or window's `cut` (formulas over its parameters) is the wall's opening where it isn't
`Width` by `Height` (a frame outside the leaf): `OpeningType::hole`, worked out and never stored, read by `cut()` for the
wall's hole, its notches, the plan's gap and IFC's opening element, while the type's width and height stay the leaf's.
`flips` says which of Revit's flip controls it has, and the canvas shows only those arrows (`flip.rs`); `flip` and Space
flip it all the same, as Revit's API does. `source` (the firm library's entry and version, the application, its category,
the file), `host`, `subcategories` and a form's or symbol's `subcategory` are stored, checked (`Family::check_described`,
`check_part`) and shown, never acted on: the `family` list gives each family's `source`, and the Family Editor shows it
read only. A `text` parameter holds words (`Val::Text`), shown in Properties and in schedules (a door's or window's
schedule lists its families' parameters as a component's does), which no formula reads; a parameter's `unit` is said
beside a number and its `group` is the bar Properties and Type Properties list it under. `family {path}` and the inline
command take `materials: {name: {color, transparency?}}`, added where the project has none of that name, and the whitelist
of a file's keys takes all of the above; a caller's `source` is laid over the file's. A project is written as format 21
only when a family uses `types`, `show`, a symbol's `view`, `outline`, `cut` or a text parameter, which a build that reads
up to 20 would misread (`Model::written_format`); the rest an older build leaves out. `family_library` lists BIMCraft's own
families (the starter set, named in the project's language, and the symbol sets) for Load Family, and replies one's
definition as a family file holds it, at no undo step. The sample `crates/core/testdata/families/sample-two-type-flush-door.json`
(original) has all of it; `family_revit_tests.rs` drives it.

## Symbol sets

A symbol set (`symbol_set.rs`) is families that are a plan symbol and nothing else, loaded on demand and never part of a new
project: the starter fixtures (`fixtures.rs`) are in every project, and 76 more would crowd every family list. The
`symbol_set {set, names?}` command adds a set's families to the project (the Electrical tab's Symbols > Load AREI Symbols
button runs it); a family the project has by that name stays as it is, so loading twice changes nothing and an edited family is
never overwritten (`loaded` and `kept` in the reply). Each family has one type, `Standard`, no forms, and the symbols of
`crates/core/data/arei-symbols.json`, in the family `symbols` schema (mm, y up, the origin at the symbol's own origin): a
switch is in category `switch`, a socket in `outlet`, a data socket in `dataoutlet`, a light point or luminaire in `lighting`
and everything else (appliances, consumers, sources, domotics, protection) in `generic`, so the Device and Lighting Fixture
tools list their own kinds and the others are placed with the Component tool. They have no connectors, loads or 3D forms: they
are plan symbols only for now, and a family without a connector raises no warning in `inspect`.

The `arei` set is the graphical symbols of Table 2.23 of the AREI/RGIE (FOD Economie, Book 1), redrawn originally. The names
are those printed in its Dutch and French versions (`name_nl`, `name_fr` in the data), the English ones are working names. The
symbols are drawn at about 30 mm per drawing point, so a socket is about 25 cm wide on a plan. `names` (`en`, `nl`, `fr`) picks
the language of the family names; as given none, a French project gets the French wording, a Dutch project the Dutch
and every other language the English.

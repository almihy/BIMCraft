# Levels, walls, roofs, floors, ceilings, shafts, stairs

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

Levels have elevations, and each is a plane over its `extent` (`Model::level_extent`; without one, `Model::model_extent`):
sections and elevations draw a level over as much of its extent as they see, and the 3D view outlines it, depth-tested and
picked as the level; unselected, its two edges nearer the camera are left out. Its name goes on a `view3d::LevelHead`, an
unseen vertical plane past the right end of its far edges, where an elevation marker family will stand. Selected alone, a
level's ends in a section are grips (`Model::level_extent_along`) and so are its plane's edge middles in 3D
(`geom::edge_grips`, `Camera::on_plane`); both run `modify {extent}`.
A section or an elevation cropped shows its line's length and its crop's heights; uncropped, everything along its line
(`render::crop_region` is none, and there are no grips).
A level is also its plan: its view range (`Model::plan_range`)
cuts 1200 mm above it unless set, and its crop region bounds what the plan shows. Rooms find their walls at 1200 mm whatever
a plan cuts (`Model::bounding`). In a section or an elevation, points run along its line from its start, and up; on a sheet,
they're paper mm from its bottom left corner; notes and spot elevations live in those terms in their view.
A wall's `start` and `end` are on its location line: its centre line unless `location` says otherwise (`walls::faces`).
Its exterior is to the left of start to end, as in Revit, so walls drawn clockwise face out; a compound type lists its
layers from the exterior in, and a floor, roof or ceiling type from the top down (IFC import turns a wall round, or a slab's
layers over, by the way its layer set's usage stacks them). A layer's material is a name in the project's library
(`Model::materials`): its colour shades the layer in 3D, its cut pattern is a drafting pattern (`GAP` px apart on screen and
paper, where the layer is `PATTERN_ROOM` px wide), its surface pattern a model pattern (`MODULE` mm, its real size). A curved wall turns `arc` degrees, counter-clockwise positive (`walls::Axis`). Where walls meet,
their faces meet in mitred caps at any angle, and a wall ending on another stops at its face (`walls::cap`); plans, 3D,
sections and IFC all draw that outline (`walls::outline`, `walls::prisms`). A wall's `join` (on unless turned off, written in
a file only when it is off) says it joins at its ends, Revit's Allow Join: a wall refusing to join neither mitres against a
neighbour nor stretches with one whose end is dragged (`walls::arms` leaves it out of both; `relocate` passes it by), and a
footing under such a wall stops at its face as if it were a wall too. The desktop's right-click menu offers Disallow Join and
Allow Join for the walls and runs selected (`context::Action`, a `modify {join}` command).

Two straight walls join as Revit's (`walls/join.rs`). Where their ends meet they Butt, the default: one runs through to the
other's far face and the other stops against it, the wall made first (the lower id) running through until the Wall Joins tool
says otherwise. Where one ends on the other's side it stops against it, the other running on. Either way their layers clean
up by rank, the core before what is outside it and then by priority (`join::Rank`, `join::solve`): a layer runs from its own
wall through the other's layers of a worse rank, stops before one a better layer has taken, and where it reaches its like the
two join, the cell going to the wall running through. So finishes wrap a corner and cores join as an L, and a partition's core
passes the plaster of the wall it ends on to stop on its core. Each wall's end is then stepped, a layer at a time
(`join::end_cut`, `WallEnd::stepped`: its cap), and the wall running on is broken where the other's layers pass into it: in
plan each of its layers is its pieces (`join::gaps`, `LayerGeometry::bands`), and its solid gives up that part between the
other wall's base and top (`tongues`, in `walls::prisms`), so sections, 3D, quantities and IFC share nothing twice. A plan
draws a line between two layers only where their materials differ (`LayerGeometry::interfaces`). Quantities follow the Butt,
as Revit's: an L of two 4 m walls is 12.3 and 11.7 m² a side.
The `wall_join` command is the Wall Joins tool for two ends that meet: `kind` butt, mitre (both cut on the line from the inner
corner to the outer, the faces' crossings) or square-off (the first wall ends at 90 degrees to itself where the other's far
face meets its nearer face), `through`, `next` and `previous` for which wall is first, `display` (`dont-clean`: the layers
are left whole, the wall that stops ending on one line a plan draws at any detail, `LayerGeometry::butts`; a view's own
Wall Join Display is not read yet), `reset`. The desktop's Wall Joins tool (Modify tab; `Tool::WallJoins`, `wall_join_options`)
picks the join under the cursor, a square over it, and runs the command from its options. What it sets is kept with the model (`Model::wall_joins`, by the two ends' GUIDs; let go with its walls after
each command, as a pin is). Allow Join is each end's (`Wall::join_start`, `join_end`, `Wall::joins`; `join` off is both).
Layers wrap as Revit's (`walls/wrap.rs`, worked out along the wall and across it): at an end that joins nothing, on the
side the type's Wrapping at Ends says, and at the jambs of its doors and windows, on Wrapping at Inserts' side, the layers
outside the core ticked Wraps turn the corner and run across the core's end to its far boundary, one inside another, each
keeping its thickness; with both sides wrapping at an insert, each to the core's middle. What they cross stops short by
their thickness. A plan draws the wrapped pieces and the lines round each return (`layer_geometry`); a joined end stays as
its join cut it. Not yet: the solids (3D, sections and quantities keep the layers square at ends and jambs), an insert's own
Wall Closure plane, and turning the wrap off at one end.
Three ends at a point of which two are one wall carried on in line (one type, one location line: a wall split, or drawn in
two lengths; `in_line`) join as the third would the one wall: the two stand square against each other and each gives up its
part to the third's layers. Still as before: arc walls, other meetings of three or more walls, and walls within about 12
degrees of parallel, which mitre.

A wall type is Revit's basic wall type (file format 29, `WALL_FORMAT`; written only when a type needs it). Its layers are Edit
Assembly's rows, each a `Function` with Revit's priority (`Function::priority`: Structure 1, Substrate 2, Thermal/Air Layer 3,
Finish 1 `finish` 4, Finish 2 `finish2` 5, Membrane Layer none), a material, a thickness, `wraps` (outside the core) and
`structural` (the one layer whose material is the type's Structural Material, `WallType::structural_material`) and, when it is
not its function's, a `priority` of its own, 1 to 5 (Revit 2026's Priority column; `Layer::join_priority`). Its core lies
between the two Core Boundary rows, `core` {exterior, interior}: how many layers lie outside it on each side; a type that sets
none has it from its first structure layer to its last (`WallType::core_layers`, `core_span`, which the core location lines
and `walls::core_faces` read); it may hold no layer, as Revit's since 2026 (a finish wall). How two walls' layers clean up where
they join is `walls/join.rs` (`join::solve`: which wall's layer holds each cell of the patch the walls share, by rank: core
first, then priority), not yet read by the drawings. The type also holds `function` (Interior, Exterior, Foundation, Retaining, Soffit, Core-shaft:
IFC writes `IsExternal` in `Pset_WallCommon` by it, false for Interior and Core-shaft), `wrap_inserts` and `wrap_ends`
(Wrapping at Inserts and at Ends; stored, drawn with the joins), and `coarse_fill` with `coarse_color` (Coarse Scale Fill: a
plan of coarse detail draws a wall as its two lines, filled only with that pattern). `Model::check` refuses only core
boundaries past the layers there are, a priority that is not 1 to 5, wrapping at ends of both, a fill pattern the project lacks, two structural materials and a
layer that wraps in a floor, roof or ceiling. The desktop's Type Properties of a wall and its Edit Assembly window are
`crates/ui-egui/src/assembly.rs`; the Revit reader brings in Finish 2 but not yet the core boundaries or wraps.

Room areas are measured at the walls' centre
lines, run on or back to meet at joins (`walls::centre_line`). A room separator (`roomseparator`, made by `room_separator` a line per segment) bounds rooms as a wall's centre line
would and nothing else: it is no edge of a floor, a ceiling or a roof (`Model::bounding_lines` takes it, `wall_lines` and
`face_lines` don't; an end drawn to a wall's face runs on to its centre line there, so the line splits rooms measured at
the centre lines too), it is drawn in plan only, and it goes to IFC as an `IfcVirtualElement` of `ObjectType` `RoomSeparation`
with an `Axis` polyline on its storey, which the import reads back. The desktop's Room Separator, beside Room, lays one a
click from the point before until Esc. A room's height is Revit's: from `base_offset` over its level up to `limit_offset` over
its Upper Limit, `top_level` (none: Not Bounded, the offset then over its own level). A new room reaches 300 mm under the
level above (`ROOM_BELOW_ABOVE`), or with none above is Not Bounded and 2500 mm tall; a room read from a file without them is
Not Bounded, 2500 mm. `Model::room_span` and `room_height` give it, `elements` its `unbounded_height` and `volume_m3` (the area times that height, not cut by floors or roofs), and the IFC
space is swept that tall. A room's colour on plans (`Room::colour`) styles that solid in IFC as other elements' colours do: an
`IfcStyledItem` on the swept solid, its unnamed `IfcSurfaceStyle` shading in the colour (`IfcSurfaceStyleShading`, which IFC4's
rendering style extends; `Step::surface_style`, which a material's style shares), so a viewer shades the space in it. The import
reads it back from the colour the space's body is styled in (`import::item_colours`), so a round trip keeps it and a space
another program styled comes in filled in its colour; a room that isn't enclosed has no body, so no colour, in IFC. A frozen room (`frozen`, an instance parameter: `room {frozen}` or `modify {id, frozen}`) keeps the
boundary it has when frozen, both ways it can be measured (`Room::kept`, given after the command that froze it by
`Model::keep_rooms`, which the engine calls after every command, and let go when it is thawed): `room_region_at` returns it
and the plan draws it without finding the level's lines, so the walls are never searched for it again, which saves the time
of many rooms; moved, rotated or mirrored, its boundary goes with it, and only a bounded room can be frozen. Walls and columns (and structural columns) bound
rooms while their Room Bounding is on (`Wall::room_bounding`, `Column::room_bounding`, on unless set; `Model::room_bounding`,
`bounding_columns`, `room_lines`): a column's outline either way rooms are measured, a free one an island. A wall with it off
still bounds floors and roofs picked by their walls. Walls and columns bound a level's rooms where its
Computation Height (`Level::computation_height`, 1200 mm unless set) cuts them; room separation lines bound them whatever
it is. Two rooms can't share a space (`Model::room_bound`, `Unbound`): the one placed first (the lower id) keeps it, the
other is unbounded, and `room` and `inspect` warn. A room can be placed where nothing encloses it: unbounded, it covers a
`UNBOUNDED_ROOM` square round its point (`Model::room_outline`), as Revit's. A plan labels it NOT ENCLOSED, or REDUNDANT where another
room has its space. Its reference, a cross from the point it was placed at out along the diagonals to its boundary
(`geom::ray_hit`), drawn as the room's own lines (a click on it picks the room), is the subcategory `roomreference` under
Rooms, off unless shown (`graphics {show: ["roomreference"]}`), beside `roomseparator`, the separation lines.
Subcategories are general (`graphics::SUBCATEGORIES`: a key, its category, a name, whether it is off unless shown):
a view holds them as categories, with `Override::shown` for the ones off unless shown, `Graphics::hides` answers for
them (a category hidden hides its subcategories), and Visibility/Graphics folds them under their category. In a 3D
view rooms are off until shown (`graphics::OFF_IN_3D`, `Graphics::shows_in_3d`; `graphics {view: "{3D}", show: [rooms]}`,
or ticked in its Visibility/Graphics), every new 3D view so; shown, each is drawn as below, fainter than a selected one,
and a selected room is seen whatever the view says: a see-through solid in the selection's colour from its bottom to its top
(`orbit::room_solid`, drawn over the picture, so it shows through walls). With `room_area {at: "finish"}` (`Model::room_area`, Revit's Area and
Volume Computations) room areas are measured at the walls' faces instead (`face_lines` and the separators), the floor inside them; the IFC space
takes that outline, and its `Qto_SpaceBaseQuantities` give both whichever is set: `GrossFloorArea` and `GrossPerimeter` at
the centre lines, `NetFloorArea` and `NetPerimeter` at the faces, `Height` (the room's Unbounded Height) and `GrossVolume` and `NetVolume`. Walls, slabs, doors and windows
carry theirs too (`ifc::wall_quantities`): a wall its `Length` on its axis, `Width`, `Height` and side and volume, gross and less the
doors and windows in it (each cut to its height; joins aside), or only `Length` and `Width` when attached, as its height then varies;
a level slab its `Width` (depth), `Perimeter` and area and volume less its openings, a sloped one only its `Width`; a door or window
its `Width`, `Height`, `Perimeter` and `Area`. An attached top or base follows the roof's or floor's plane
over each part of the wall (a roof's slopes each over its own part of the plan); a mirrored wall runs back the other way.
A wall's top is `Model::own_top`, and 3D, sections, plans, IFC and what a wall is attached to all read it there (`walls::heights`
takes it, or what the wall is attached to): its `top_level`'s elevation plus its `top_offset` (file format 15, Revit's Top Offset,
mm, negative below the level), or its base plus its `height` when it has no top level, which an offset does not touch.
`Model::check` refuses a wall whose top does not come out above its base, saying how tall it comes out and by how much to raise it.
The Properties palette shows Top Offset under Top Constraint for a wall that has one.
A door or a window stands in its wall from its `sill` up its type's height (issue #215): `Model::check` keeps its width within the
wall's length, its sill at or over the wall's base and under its top all across it (`walls::top_at`, which a roof the wall is
attached to may raise; within `TOL` of the top is at it), and the openings in one wall apart; its head may rise past the top, as
Revit lets it. The wall is then cut up to its top, a notch open there: `walls::prisms` leaves no part over the opening, so 3D,
sections and elevations draw the notch, and the door or window at its full height, past the top. A plan cutting through the
wall opens it at the opening as ever; one cutting over the wall's top cuts neither, and sees the notch's sides across the wall
from above (`walls::notches`). IFC writes the opening's void at its full height (a boolean takes off only what is in the wall)
and the door or window at its own size, and the wall's net quantities take off only the part of each opening in the wall; the
IFC import brings such an opening in, and leaves out with a note naming the file's wall one whose sill is at or over its top.
A roof by footprint is the straight skeleton of its outline and its `courtyards` (`skeleton::faces`: the outline
counter-clockwise, the courtyards clockwise, the roof to the left of every edge): each edge moves in at speed 1, a gable's at
0, and its slope rises from it, reaching the roof's base `overhang` in from its eaves; a courtyard's edges number on after
the outline's, and every ring keeps its numbers whichever way round it runs, so `gables` stay put through a mirror.
`Roof::slopes` (file format 15) gives each edge a slope of its own, in degrees, numbered as `gables` are: 0 is the roof's `slope`,
and an empty list, every edge. `Model::roof_faces` then moves an edge in at the run over the rise of its slope, scaled so the
shallowest moves at 1 (every edge at one slope still moves at exactly 1), so edges of different slopes reach the same height
together and the slopes meet along their hips; the eaves stay level, the roof's base `overhang` in from them lowered by the roof's
`slope`, whatever the edges' own. A gable stays upright whatever its slope, and a flat roof (`slope` 0) keeps its `slopes` as it keeps
its `gables`, unused. `Model::check` refuses a list of the wrong length, saying how many edges there are, and a slope outside 0
to 75°, saying which edge. A sketch line may carry a `slope` (`SketchLine::slope`; the Slope row of Properties, with one line of a
roof's sketch picked, sets it): the roof keeps it as the `slopes` of the edges along that line (`sketch::along`), never in its
`sketch`, which `sketch::check` refuses to hold one. `edit_boundary` keeps the slope of an edge that stays or that a new
line cuts in two (an edge lying along an old one), the lines' own `slope` (0 sets the roof's) winning over it, and without
`slope` on the lines it keeps what the edges had; `edit_boundary` without a sketch gives the lines with the slope their edges share.
Over the walls (`floor_outline`, which `roof_footprint` grows by the overhang), a loop of them standing free inside the outer
ones, joined to none of them and with no room in it, is a courtyard, open through the floor and the roof; one joined to them,
or with a room in it, is rooms, and both run over it.
`Model::roof_faces` are its slopes, each the loops round it in plan (a slope can close round a courtyard whose far side is a
gable) with the plane of its underside, trimmed where it `join`s another; `Model::roof_pieces` are those less their holes
and its `openings`, convex where cut. A floor's top is at its level plus `offset`,
or with a slope arrow, there at the arrow's tail and rising `slope` degrees toward its head (`Model::floor_top`); it's its
type's thickness across the slope, so plumb it's thicker. Its `openings` are holes inside it, apart, and `Model::floor_slabs`
what's left, convex where cut. A floor's `shape` (IDEAS MOD-16, [design](../shape-editing.md)) folds its top into planes:
points `z` mm above (negative, below) where the top would be without them, and split lines it folds along, each a part
with an id and a GUID (`ShapePoint`, `SplitLine`). `Model::shape_planes` triangulates its outline, openings, points and
lines (`triangulation::constrained`), lifts each corner by the point there (a corner with none stays at 0) and merges
neighbours in one plane; `Model::floor_faces` are those planes in convex pieces, or the unshaped top, and `floor_slabs`,
the 3D view, sections and the element's details take them. In IFC a shaped slab is one closed `IfcFacetedBrep`
(`Model::shape_solid`: its top's planes, the same planes under them, plumb sides round its outline and openings, each
corner written once), and a sloped or shaped slab gives `GrossVolume` and `NetVolume`, its plan through its depth plumb;
every floor's details give `volume_m3`, what's left of it in plan (shafts too) through that depth. A shaped floor is its type's thickness plumb, so its planes meet
underneath too, and has no slope arrow (`Model::check`, `shape_ok`, refuses one, and points outside the outline or in an
opening, points within a millimetre of each other, a split line's end that is no point and no corner, and lines that
cross). A wall attached to one follows `Model::shape_triangles`, its top over the whole outline as if its openings were
closed, as a slope arrow's plane runs on over them. Plans draw each plane's edges as thin as the outline. `edit_shape`
gives the shape anew or says what it is, `add_point`, `add_split_line` and `reset_shape` change it a piece at a time (an
end of a line that is no point becomes one, at the height the top has there), `edit_boundary` keeps what stays inside the
new outline and its openings, cuts a split line where it leaves them, and gives each new end and each new corner a point at
the height the top had there, so a hole cut through a shaped top leaves it as it was round the hole; it names what it
`dropped`, `cut` and `added`. Moves and mirrors take the shape along, and snaps find its points. A flat roof by
footprint (`slope` 0, no extrusion, joining none) takes a shape too (`Roof::shape`, `Roof::unshapeable` says why another
can't): the commands, checks and both views' tools see its top as a floor's (`Model::shapeable`, `Model::roof_as_floor`:
its footprint, its courtyards and openings as openings, at its top), and `roof_faces` lays each plane's underside its
thickness below, plumb (`roof_rise`), so its slabs, IFC, sections and the walls attached under it follow its falls; plans
draw its creases thin. Under its type's variable layer a shaped roof's underside stays level at its base
(`Model::roof_under`, `roof_prisms`), as a floor's does: its layers in 3D and sections share `Variable::bounds` through
`Model::variable_under`, walls attached under it stop at that underside, IFC writes it as one closed faceted Brep
(`Model::roof_solid`), and `Model::check` refuses a point that leaves the layer thinner than nothing. `falls` (`falls.rs`) gives a floor or a flat roof its shape from its drains and the least fall:
catchments nearest each drain, ridges between them at the top's height, valleys from each drain to its catchment's corners,
each drain as deep as the fall takes its farthest corner; round an opening the valleys turn at its corners, which lie on
the drain's cone, and ridges drawn part the top first. A slab or flat roof whose top isn't one plane comes in from IFC
shaped (`import::shaped_top`), its variable layer marked by its `Description` or, under a level underside, guessed
(`import::guess_variable`); and a spot slope (`Spot::slope`) reads a floor's or roof's fall in a plan. A layer of a floor or roof type may be `variable` (one a type at most; never a wall's or a ceiling's): under a
shaped floor of that type, the underside stays level where the unshaped floor's was (`Model::floor_under`), the layers
above the variable one follow the top, those below it stay level, and the variable one takes up the difference
(`Variable::bounds`, which the 3D view's layers and sections share). Its pieces are then prisms from that level underside
(`Model::floor_prisms`), its volume is less where its top falls (`Model::floor_volume`, `model::volume` for IFC), a wall
attached under it stops at the level underside, and `Model::check` refuses a point that would leave the variable layer
thinner than nothing, saying by how much to raise it. Reinforcement arms a shaped floor only when a variable layer lies
over all its structure: the bars keep to the layers under it, as on a level floor that thick. Without one it refuses,
saying why. In the desktop, a floor selected shows Shape Editing in its contextual tab: Slope Arrow, Modify Sub Elements (a point, a corner or an edge's two ends,
`Floor::shape_pick`, to the Options Bar's Elevation), Add Point (a
click on the top, at the Options Bar's Elevation, above the unshaped top or, with Absolute, above the level; empty, the
top's height there), Add Split Line (two clicks, snapping to points, corners and edges) and Reset Shape, each a command.
While one is on, each point's height is labelled beside it (`Model::shape_labels`); under Modify Sub Elements a point's
grip drags it in plan at its height, its split lines with it (`Shape::moved`), in one `edit_shape`. In the 3D view the
same tools work on the floor's top: each point's and corner's grip drags its height up or down, and clicks land on the top
(`view3d::shape_handles`, `on_floor_top`, `shape_pick_3d`); both views run `shape.rs`. While a grip is held, either
view draws the creases its release will leave (`Model::shape_creases` of the floor with the shape it would have). Type Properties ticks a floor's or a roof's variable layer. A ceiling's `offset` is its underside; its `openings` are holes as a floor's
are, and `Model::ceiling_slabs` what's left. A shaft cuts each floor,
ceiling and roof its span reaches into over its outline, all the way through (`Model::less_shafts`). A stair or a ramp
is runs and landings laid out from its start (`stairs::stair`, `stairs::ramp`): a landing after `at` risers (a ramp's, `at`
mm up), as deep as the runs are wide, the next run turning by its `turn`; a railing on one follows its walk line moved out to
its side (`stairs::railing`), so it goes wherever its host goes. `modify` merges into fields that are objects (JSON Merge Patch), so `{"view_range": {"cut": 1500}}` keeps the rest; an element's own field can't be
called `category`, the tag every element carries.

Sketches (`sketch.rs`) are how an outline is drawn, the one solver everything with an outline and holes uses: floors,
ceilings, roofs by footprint, shafts and filled regions now, topography next. A sketch is `SketchLine`s, straight or
arcs (a point `through`), in any order, either way round; `sketch::solve` cuts arcs into chords no more than 5° round,
welds ends within `TOL`, then names every problem at once with the lines it's about, what to fix first first
(`Problem::rank`: a line too short or out of range; two lines crossing, or one ending part way along another; two along
each other; three or more at a point; an end no other meets; a loop with no area), and otherwise walks the loops and nests
them by how many others each is inside: even, an outline (counter-clockwise), odd, a hole (clockwise) of the innermost loop
round it, so an island in a hole is an outline again. `solve_one` wants one outline (a floor's, a roof's). The element
keeps the sketch it was drawn or last edited by (`sketch` on floors, ceilings, roofs and shafts; empty when it was made
over the walls or read from a file): each line is a part with an id and a GUID of its own (`HasPart`, listed in
`parts_now` by the element's GUID), going with the element when it moves and new on a copy; part ids are not given
again once a line is gone, for as long as the project is open (the library remembers the lowest it has held; a file holds the
ids in use, so one a line had before it was deleted and the file saved may be given again after the file is reopened). Commands take `sketch` (lines `[[x,y],[x,y]]`, arcs `[[x,y],[x,y],[x,y]]`, or
`{start, end, through?, id?}`) in place of `boundary` and its holes; `edit_boundary` without one gives the lines to edit
(the kept sketch while it still draws the outline, `sketch::draws`, else the outline's edges as new lines) and with one
redraws it, a line sent back with its `id` keeping it (keeping a roof's gables on edges that stay); the `sketch` command
solves without making anything. Messages name a line by its id, or its place in the list while it has none. A boundary
given as points is cleaned (a repeated point dropped, and one within a millimetre of the last), and every outline a command
makes or changes, whichever command (`floor`, `modify`, `edit_boundary`, `move`…), is judged as the sketch of its edges
(`Model::check_edits`, asked by the engine after each command but Insert from File): a floor's, ceiling's or roof's boundary,
an opening or a courtyard, a shaft's boundary, that crosses itself or lies along itself, has an edge ending part way along
another, or a spike or keyhole slit, is refused, saying which edges and where, and what to do: redraw it with `edit_boundary`; give
`boundary` or `sketch` (with the walls that touch at a corner named only when it was taken from walls), or simple `openings` or
`courtyards` for one of those rings, for one a command was making;
redraw the mass whose outline it is, for one made by face or by `mass_floors` (a mass itself may still have a bow tie outline,
a spike or a corner touching an edge, so the message says the outline isn't simple, not that it crosses itself);
and, for a roof's opening, which `edit_boundary` does not edit, give `modify` a simple `openings`, and from `dormer` change the
dormer's width, height or position. That is a change from the base, which let
`modify` and the commands that weren't given points set such an outline. `Model::check` doesn't judge the whole project by
it: a file, an IFC import or a Revit import with a keyhole outline still opens, and what leaves its outline alone (`modify`
of its offset, `delete`, `edit_boundary` with a sketch that is simple) still works, and so does what moves, copies, turns or mirrors
it, which makes it no worse (a copy is judged against the elements the command was given, so a new floor drawn with a bow tie
the file also holds is refused); what changes its shape is asked to redraw it first. One line that crosses itself (a closed polyline round a bow tie, a spline with a loop) is found as two lines
crossing are, and a sketch is refused at once past 200,000 straight pieces (a circle is up to 8,193 of them), or when its
pieces lie so close together that telling which cross would take six million steps.

The desktop's sketch mode (`crates/ui-egui/src/sketch.rs`) holds the lines until Finish Edit Mode, which solves them
first and says what's wrong for whoever drew it (`sketch::friendly`: what to fix, kind by kind, where, in the project's
units, and the lines by id) with those lines in the error colour; solved, it runs the command. Draw has every shape of
the Draw panel (`draw.rs`; lines, rectangles, polygons, circles, the arcs, Fillet Arc on two sketch lines, splines, ellipses,
polylines, Pick Lines: see [views and graphics](views-and-graphics.md)) and Pick Walls (a line along each wall's location
line, an arc along an arc wall); a sketch line is any curve, so a circle or an ellipse is a loop of its own. Modify picks
sketch lines (a click, a window box from the left, a crossing box from the right), a picked line's handles (its corners, a
spline's points, a circle's centre) are grips that take every one joined there with them, and Delete removes them; Ctrl+Z takes back a line; the plan's snaps take the
sketch's own ends, middles, crossings and curves as well as visible view-specific detail (`snap::in_sketch`). Floor, Ceiling, Roof and Shaft each start a new one (Boundary
Line, Sketch Ceiling); Edit Boundary redraws one selected. The Mode panel (Finish, Cancel; Edit Boundary) stands at the
ribbon's right end.

What the line tools hold to, and don't (`curves.rs`, `draw.rs`, `sketch.rs`). A curve is drawn as chords none further than `FLAT`
(0.25 mm) from it: a polyline's arcs, a circle, a spline and a fitted spline (a span of a spline is cut by the greatest
second derivative on it, so a ten metre arch is hundreds of chords), up to `MAX_DRAWN` extra points in all; a polyline whose
arcs would take more is refused, and a circle past about 3.4 km of radius (`MAX_DRAWN` chords stray `FLAT` from it there) is drawn coarser than `FLAT` rather than in more, 0.37 mm off at 5 km and 2 mm off at 27 km.
An arc, a circle or an ellipse reaches no further than `MAX_COORD` from the origin, as a wall's arc must; a spline lies inside
its control points, and a fitted spline is held to its points only, a cubic through them straying a little past. A curve's
start and end are the points it was given. Offset moves a polyline's lines along themselves and its arcs about their own
centres and makes each corner where the moved sides cross; it gives nothing where a side would vanish or turn round, or a
corner is so sharp its mitre would be 50 offsets long, and it does not trim a fold it makes in a concave shape (Revit's
does); an ellipse or a spline offsets to a fitted spline within 0.05 mm. Fillet on walls keeps their doors and windows
where they stand and refuses to cut one off, but does not stretch a third wall that ended on the corner. A detail line with
no style gets Thin Lines by the project's name, else the English one, else the thin black solid style. Of the line tools,
Revit has default shortcuts for Trim/Extend to Corner (TR), Offset (OF), Fillet (FL), Split (SL) and Detail Line (DL), which
`corner`, `offset`, `fillet`, `split` and `detail_line` have; it has none for Trim/Extend to a boundary, vertices, Convert
Spline, Edit Boundary, line patterns and styles or fill patterns, which have none here. Explode Polyline is EX, ours (PX is
the desktop's Plumbing Fixture).

## Curtain walls

A curtain wall is a wall whose type is a curtain type (`WallType.curtain`, `curtain.rs`, whose header holds the design):
no layers and no solid of its own, a grid with a panel in each cell and mullions on its lines' segments. The wall is
drawn, moved, attached and bounded by rooms as any wall; `WallType::is_curtain` is asked wherever a wall's layers, joins
or solid are read (it joins nothing, and hosts no ordinary door or window: the `door` and `window` commands refuse one,
and `Model::check` holds the rule).

**The model.** Panels (`Element::CurtainPanel`), mullions (`Element::Mullion`) and grid lines drawn by hand
(`Element::CurtainGrid`) are elements the wall hosts, each holding where it is in the wall: mm along its location line
from its start, and mm over its base. The type's own lines are worked out (`curtain::grid`), never stored. A line has a
key (a border `u0`/`u1`/`v0`/`v1`, the type's `ut3`, a hand-made line's `g<guid>`), a panel its lower left lines', a
mullion its line's and the line its segment starts at; an element's GUID is made from its wall's GUID and its key
(`curtain::child_guid`), so every copy of a project lays the same elements out. `curtain::keep` (run by
`Session::execute` after every undoable command) lays a wall out again when it, its type, a type its type names, a level
or one of its own changed; `curtain::lay` does one wall at once, for a command that answers with its panels. What a
person did is kept on the wall and its children: segments removed (`Wall.curtain.merged`), mullions deleted
(`no_mullion`), and panels and mullions unpinned or put on by hand (`free`, which keep their type across layouts).
Those are named by keys, and a wall copied, mirrored, flipped or split names its lines anew: `curtain::hand` takes what
was done off a wall by place, and `curtain::hand_to` puts it on the wall as it is now.

**Types** (the `type` command). Category `wall` with `curtain: {panel, join_condition, vertical: {layout, spacing,
adjust_for_mullion}, horizontal, vertical_mullions: {interior, border1, border2}, horizontal_mullions}` (layouts none,
fixed-distance, fixed-number, maximum-spacing, minimum-spacing; `curtain: true` a plain one, `null` a basic type again);
`curtainpanel` `{thickness, offset, material, empty}`; `mullion` `{thickness, side1, side2 | width, round, radius,
offset, material}`. Three starter wall types are made when first drawn with or given to a wall: Curtain Wall (one panel),
Exterior Glazing (a grid, no mullions) and Storefront (a grid with mullions), with the panel types Glazed, Solid and
Empty and four mullion types. A wall's own Number, Justification and Offset each way are `modify {id, curtain:
{vertical: {number, justification, offset}, horizontal}}`.

**The tools** (`commands/curtain.rs`), places given as `at: [s, z]` (mm along the wall, mm over its base) or a plan
`point` with an elevation `z`:

- `curtain_grid {wall, vertical, at}` (or `point`, `z`, `lines`): Revit's Curtain Grid, All Segments. A line is moved
  with `modify {id, at}` (a standing one with `move` too) and goes with `delete`. One Segment and All Except Picked are
  not built; the type's own lines are not elements yet, so they are not unpinned and moved.
- `add_remove_segments {wall, at, remove?, segments?}`: joins the panels either side of a segment, or parts them. A
  border's segment is refused, and so is a change that would leave a panel that is not a rectangle: the error lists the
  places to give together.
- `mullion {wall, type?, on: grid_line | segment | all, at}`: Revit's Mullion tool. `all` takes the borders too (the
  help does not say; it is what Revit does as far as we know). A segment that has a mullion keeps it.
- `delete`: a mullion goes and stays gone (pinned, the type's, it is unpinned first); a panel is refused, as Revit's
  (give it an empty type, or join it to its neighbour); a wall takes all its own with it.
- `unpin` frees a panel or a mullion the wall placed, which then takes a type with `modify {id, type}` or
  `match_properties`; `pin` gives it back to the wall's type. A mullion put on by hand where the type has none can't be
  pinned.
- A wall given a curtain type (`modify`, `match_properties`, or its type made one by `type`) loses its doors and windows,
  named in `deleted` with Revit's "Switching wall type to family curtain wall"; given a basic type it loses its panels,
  mullions and hand-made lines.
- `curtain_door {panel | wall, at | point, z?, type?, family?, flip_facing?, flip_hand?, mark?}`: a panel swapped for a
  door, a storefront's entrance: Revit's Unpin and Type Selector in one step (the panel may be pinned). The door is an
  `Element::Door` the curtain wall hosts, with two fields of its own: `panel`, the key of the cell it fills (the key the
  panel there had), and `cell`, that cell's span, which the layout writes as it writes a panel's (`curtain::door_fits`,
  from `relay`), with the door's `offset` (the cell's middle) and `sill` (its bottom). The layout places no panel in a
  cell a door fills, and `Model::opening_dims` gives the door its cell's width and height in place of its type's, so
  plans, solids, schedules, tags, rooms (From/To Room) and IFC read a door of that size and nothing else knows it is a
  curtain wall's. A family's door keeps its family's own size, standing at the middle of its cell's bottom. It follows
  its cell (a type's spacing changed, a line moved, cells joined); when its cell is gone it fills the cell that holds the
  place it was, and with none, or in a wall that stops being a curtain wall, it is deleted with its tags (`modify` and
  `type` say so in `deleted`). A wall flipped, mirrored, split or copied names its lines anew, so `curtain::hand_to`
  names each door's cell by where the door stands (`curtain::rehome_doors`). It is not slid along its wall (`move`
  refuses, naming what to do); pasted alone into a basic wall it is an ordinary door. Deleted, its cell has its panel
  again. Of two doors in one cell (two copies editing together) the one of the lower GUID stays. `Model::check` asks
  only that a curtain wall's door has its `panel` and a sound `cell`, and that no other door or window has them; an
  ordinary `door` or `window` in a curtain wall stays refused (a window in one is not built). IFC writes it as an
  `IfcDoor` aggregated under its `IfcCurtainWall` with the plates and members: no opening element, no void, no fill;
  reading that back as a curtain wall's door is not built.
- Copy, array, mirror, rotate, paste and Insert from File copy the wall with its hand-made lines and what was done by
  hand; its panels and mullions are never copied, the copy lays out its own. A panel or a mullion alone is not moved,
  copied, turned or mirrored. `flip` and a mirror keep the grid where it is in the model (a Beginning justification
  becomes End). `split` gives each half the lines and the hand work that fall in it, a lying line to both; a wall whose
  end is dragged, trimmed or filleted keeps its standing lines where they are in plan.
- Phases and design options: a panel, a mullion and a grid line are made, demolished and in an option as their wall
  (`phasing::value`).

# Project parameters and schedules

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

A project parameter (`Model::parameters`) is a value each element of its categories may hold in its `params`, of its kind;
a component's `params` hold its family's instance values too, so a parameter for components can't take a family
parameter's name, nor any category's own field's (`schedule::own_field`). A schedule's fields are its category's own
(`OWN` in `schedule.rs`, read from `element_json`), the project parameters for it and, for components, the families'
parameters, or calculated: a formula over the row's others, read as the family formula engine reads (lengths in mm, areas in
m²), a field with no value leaving it with none (an unset yes or no is no). Filters compare numbers as the schedule shows
them, so two that show the same are equal. The six schedules every project has (`schedule::VIEWS`) aren't elements
until changed or placed (`schedule::standard`), and a schedule named as one of them, in any language or case, is that one; renaming a parameter renames it in schedules, but a formula naming it holds
the rename back.
A component's Room Number and Room Name (as Revit's Room: Number and Room: Name) are the room it stands in, found where it is
read and never stored (`Rooms::room_at`): the room whose boundary at the walls' centre lines holds the
component's point on its level, so an outlet on a wall's face is in the room on that side whatever the project measures rooms
at. `element_json` gives it as `room` (the room's GUID), `room_number` and `room_name`; a component outside every room has none.
A door's or window's From Room: Number, From Room: Name, To Room: Number and To Room: Name are Revit's, found the same way either
side of its wall (`Rooms::from_to`, read from `from_room` and `to_room` in its JSON by `From::Of`; [families](families.md)):
set as it was placed and kept through its flips, of the project's last phase. A tag's text (`field_text_with`) finds an
element's rooms only for a field that reads them (`reads_rooms`).
A multi-category schedule (`of: "multicategory"`, `schedule::MULTI`, as Revit's Multi-Category Schedule) has a row for each
element of the categories it lists (`Schedule::categories`; none, every built one, `schedule::built`: not rooms, levels and
masses) and the fields they share (`SHARED`): Category (the element's, as elements name theirs), Family, Type, Level, Count,
Width, Height, Length, Thickness and Area, each its own category's field of that name where it has one holding the same and
empty where not (a wall's area is its own, one side less its openings, and a door's or window's is its width by its height; a
roof's is its surface's), and Material, its type's structure layer's (or its thickest's), a run's type's or a family's first material
parameter's. Its filters, sorting, grouping, totals and calculated fields are any schedule's; only it lists categories.
## Quantities and type identity data

**Area, Volume and Perimeter** are fields of the walls', floors', ceilings' and roofs' schedules, as Revit's (Area and Volume are
among a wall schedule's and a floor schedule's first fields). All of them come from `quantities.rs`, which IFC's
`Qto_*BaseQuantities` read too, so a schedule and an exported file never differ: a wall's numbers are those of its own solid (`walls::prisms`: each opening where it really is, held to the wall, overlapping ones cut once, the top where the wall is attached, its arc, its corners counted once): its Volume is the solid's, its Area the area of the middle of its thickness (its volume through its thickness: a straight wall's length by its height, a curved one's along its own middle, not its location line) less the openings; a floor's Area is its top's (a sloped one's stretched by its slope) less its openings and the shafts through it, its Volume its solid's (plumb depth through its plan; under a variable layer, less where its top falls), its Perimeter round its outline only (openings' edges not counted); a ceiling's Area is its outline less its openings and the shafts, its Volume that through its thickness; a roof's Area is its surface (each slope's plan stretched by its slope) less its openings and the shafts through it (the IFC `GrossArea` is before them), its Volume its solid's (a shaped flat roof's is its plan through its thickness plumb; `Plan Area` is the footprint less courtyards, openings and shafts). A field of volumes (`Kind::Volume`, m³ or ft³ by
`Units::volume`) is summed, filtered and charted as an area is; its heading says the unit. `elements` gives them as `area_m2`,
`volume_m3` and `perimeter_m` (a roof's surface as `surface_area_m2`).

**Type identity data** (`type_data.rs`: Type Mark, Description, Manufacturer, Model, Fire Rating, Cost, Keynote, Assembly Code, URL
and Type Comments, as Revit's Identity Data) is held by every library type (`identity` on a wall, floor, roof, ceiling, door,
window, column, railing or run type; a family's types in `Family::identity`, by type name), optional and written to a file only
where set, so a file from before reads as it was (a stair, a ramp and the types that aren't elements' have none). The `type`
command sets it (`type_mark`, `description`, `manufacturer`, `model`, `fire_rating`, `cost`, `keynote`, `assembly_code`, `url`,
`type_comments`; `""` or null clears one), `family`'s `identity` a family type's, and Edit Type's Identity Data group either.
An element's JSON carries its type's (`type_mark`…), so each is a schedule field of that name for every category whose elements
are of a type and for a multi-category schedule (not among the first fields unless chosen), and a tag's `label`. IFC writes it on
the type object (`HasPropertySets`, once for all the elements of the type; an element with no type object holds it): the
Type Mark as `Reference` and the Fire Rating as `FireRating` in its `Pset_*Common` where IFC has them there,
Manufacturer and Model as `Pset_ManufacturerTypeInformation`'s `Manufacturer` and `ModelLabel`, and the rest (and those two where
there's no common set, a component's) in `BIMCraft_TypeData`. A project parameter (or, for components, a family's) of the same
name takes the field's place in schedules, as it did before the field: `Fire Rating` as a project parameter of doors, with
a value for each door, still shows that. Area, Volume and Perimeter are fields of their own, which a project parameter can't be
named: a file from before that holds one by such a name reads with it renamed `Custom <name>` (`Custom <name> 2` where that
is taken), its values, tags' labels, schedules' fields, filters, sorts and formulas, view filters' rules
renamed with it (`Model::renamed_clashing_parameters`; a multi-category schedule's fields are the shared ones, never a project's).

A table goes to a file as CSV (`schedule::to_csv`, text a spreadsheet would read as a formula guarded with `'`) or, for a path
ending `.xlsx`, as a workbook of one sheet (`schedule::to_xlsx` over `xlsx.rs`, which writes the parts and the zip by hand on
`flate2`): headings in bold, numbers as numbers so they sum, text as inline strings, which a spreadsheet never takes for a formula.

## The same numbers as Revit

`crosscheck.rs` holds the numbers schedules show against Revit's ([tools/revit-crosscheck](../../tools/revit-crosscheck/README.md)):
`export_crosscheck` describes a project in plain numbers (walls by their centre lines, floors, shafts, doors and windows,
rooms by their points), a script of ours builds it again in Revit and writes what Revit reports, `kind,id,quantity,value`
by our element ids, and `compare_crosscheck` (and the test `the_small_tower_has_the_numbers_revit_gives_it`, once Revit's
table of the small two-storey tower is committed) compares them quantity by quantity within `crosscheck::tolerance`.
Compared: each room's area, perimeter, whether it is enclosed, its unbounded height and its volume, each wall's length
along its centre line and its doors and windows, each floor's area less its openings and shafts, and its volume. Revit
reports more than we work out yet: those come back as `only_theirs`, the list of what is still to do. A wall's net side area
and volume are worked out now (`quantities.rs`, as a schedule shows them); how Revit's agree, where it takes a join off, is for
the comparison to show. Where the two define a quantity differently, the difference goes here, with the rule the comparison applies.

- A room's volume is its area times its unbounded height (from its level and base offset up to its upper limit and limit
  offset), which is Revit's while nothing under its top bounds it. Revit also stops a room's volume at a room-bounding
  ceiling, floor or roof lower than its top, and a ceiling bounds rooms unless told not to; we don't yet. The description
  gives Revit each room's limits as ours. The tower's rooms reach 300 mm under the level above (3.7 m, the `room`
  command's default), over their ceilings at 2.7 m: Revit's volume of them is their area times 2.7 m where ours is times
  3.7 m, the one difference the comparison is expected to show until volumes stop at ceilings.
- Each level's computation height (where walls and columns are cut to bound rooms) goes to Revit as the level's.

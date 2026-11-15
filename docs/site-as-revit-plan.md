# The site as Revit has it: phase 1, the engine

The plan for phase 1 of Arqiflow's "the site and the car park as Revit has them" (Arqiflow's
`docs/superpowers/specs/2026-11-16-site-and-parking-as-revit.md`, agreed with the owner on 16 November), the engine's part:
Toposolid sub-elements, Sub-division, Excavate (Building Pad removed), Graded Region, Property Line with Edit Table and
setbacks, Survey Point and Project Base Point, Planting and Site Component families, their tools in the desktop, and the
commands Arqiflow's server drives. Parking, car parks and ramps are phase 2: untouched here but where a conversion must
leave them working. Revit (2024 to 2026) is the reference; where it is unclear, current Revit is followed and the choice
is written down below.

## The elements

### Toposolid (exists: `Element::Toposolid(Floor)`)

- **Modify Sub Elements** (Revit's Add Point, Add Split Line, Modify Sub Elements, Reset Shape): the floor commands
  `edit_shape`, `add_point`, `add_split_line`, `reset_shape` took floors and flat roofs only (`Model::shapeable`,
  `Element::shape_mut`); they take toposolids too. New: `remove_point {id, point? | at?}`, Revit's selecting a point and
  Delete (by its part id, or the point within a millimetre of `at`).
- Two new fields of `Floor`, used by toposolids only (`Model::check` refuses them on a floor):
  - `excavated_by: Vec<Guid>`: Revit 2024's Excavate, the floors, roofs and toposolids that cut it (Revit keeps the
    relationship on the toposolid; Remove Excavation takes one off). References, so copies, links and Insert from File
    carry them; an element deleted leaves the list (`delete`'s ties).
  - `graded_from: Option<GradedFrom>` `{toposolid: Guid, link?: Guid}`: a Graded Region's existing ground, in this model
    or in a linked one. Not a reference the model checks: the existing ground may be in a link, or deleted, and then the
    cut and fill is unknown (Revit too keeps a graded toposolid when its source goes).

### Sub-division (new: `Element::SubDivision`, category `subdivision`; replaces `Element::BuildingPad` in every match)

```rust
pub struct SubDivision {
    #[serde(rename = "type")] pub type_name: String, // a toposolid type: a floor type (its first layer's material)
    pub host: Guid,                                   // the toposolid it is a part of
    pub boundary: Vec<P2>,                            // counter-clockwise
    pub openings: Vec<Vec<P2>>,                       // islands it leaves out
    pub offset: f64,                                  // Revit's Subdivision Height: + raises it, - sinks it into its host
    pub name: String,                                 // what it is called in schedules (Revit's Comments-like name)
    pub role: String,                                 // our addition: what the masterplan called it (`lane/road`, `landscape/lawn`, `water/pond`, `pitch/tennis`...)
    pub sketch: Vec<SketchLine>,                      // Edit Boundary, as a floor's
    pub params: BTreeMap<String, Value>,
}
```

- Its region is its boundary less its openings, clipped to its host's outline less the host's openings
  (`Model::subdivision_region`, `site::poly::intersection`), so it never reaches past its host, as Revit's.
- It follows its host's surface: its 3D is its region cut into the host's planes (`Model::floor_faces` of the host as
  excavated), each piece a prism from the host's surface to `offset` above it (`offset` > 0); at `offset` 0 or below, a
  skin 20 mm thick (paint on the ground) at the surface (0) or `offset` down into it (< 0). Negative: it excavates its
  host as an Excavate does, to `offset` below the surface (Revit's negative Subdivision Height).
- Hosted: moved, rotated, mirrored or copied with its host (`relocate` and `duplicate_exactly` carry it, as a wall's
  doors), deleted with it (`delete` cascades), and re-shaped with it automatically (its geometry is worked out from the
  host every time).
- Plan: filled in its type's material colour, outlined, over its host's fill; contours are its host's. 3D: its material's
  colour. Schedulable: `schedule {category: subdivision}` with Type, Name, Area, Host, Height Offset, Role. IFC: an
  `IfcGeographicElement` `.USERDEFINED.` (a landscape, water) or an `IfcCivilElement` (the rest), as the site kinds were.
- Visibility/Graphics: `subdivision` is a model category and a subcategory of `toposolid` (as `roomseparator` is of
  `room`), named "Sub-divisions".
- Types: the project's floor types; `sub_division` takes `type` by name, or makes one of the built-in surfaces when the
  project hasn't it (`masterplan::SURFACES`: Asphalt, Paving, Block Paving, Gravel, Concrete, Grass, Meadow, Planting,
  Water, Sand, Sports Surface, Play Surface), 300 mm thick with one layer of a material of that name, made with the
  masterplan's colour (Arqiflow's palette) when the project hasn't it.

### Excavate (new; Building Pad removed)

- `excavate {toposolid, by: [ids]}` (Revit: Modify | Toposolid > Excavate, then the cutting elements) adds them to the
  toposolid's `excavated_by`; `remove_excavation {toposolid, by?: [ids]}` (all, without `by`).
- A cutter is a floor, a roof or another toposolid. It cuts the toposolid within its footprint (a floor's or toposolid's
  outline less its openings; a roof's footprint) down to its bottom (its lowest underside, mm): where the ground there is
  higher it comes down to that level; where the cutter's bottom is at or below the toposolid's bottom, a hole goes
  through. `Model::toposolid_cut` works this out (it already gave pads' holes): a hole through for a deep cutter (as
  before), else shape points: the ground's points inside the footprint lowered to the cutter's bottom, the footprint's
  corners at the bottom and a ring 10 mm outside them at the ground's height, a near-vertical cut face (a height field
  can't hold a vertical face; 10 mm is under any drawing's resolution). Revit cuts exactly; this is the closest a toposolid
  of one top surface holds.
- **Building Pad is removed.** On load (`Model::migrate`, format 26), each `buildingpad` becomes an excavating floor: a
  floor of its type, boundary and level at its offset (its top), added to the `excavated_by` of each toposolid on its
  level it lay in (`pads_in`'s rule), and the pad gone. Its GUID is kept by the floor, so what pointed at the pad points at
  the floor. `building_pad` (BP) is gone; `floor` and `excavate` are what Revit has.

### Graded Region (new)

- `graded_region {toposolid?, link?, phase?}` (Revit 2026's Graded Region): copies a toposolid (of this model, or of the
  linked model `link`, its element by `toposolid` GUID or id) into this model as the proposed ground: its type, outline,
  openings, shape points, contours and offset, on the level of the same name here (else the level nearest its elevation,
  its offset made up so it stands at the same height). Its Phase Created is the phase given (default: the project's last,
  as Revit makes it in the view's phase); a copy of this model's toposolid demolishes the original in that phase
  (Revit's Graded Region does: the existing is demolished, the new is created). Its `graded_from` remembers the source.
- Cut and fill (`Model::cut_fill`): within the new toposolid's outline, projected up (Revit 2026): the volume where the
  new surface is below the existing (cut) and above it (fill), integrated over the triangles of both surfaces overlaid on
  a grid of 0.5 m (or finer for small sites: at least 200 by 200 samples over the outline's box, at most 1,000,000), the
  difference of the two heights at each sample. `element_json` gives `cut_m3`, `fill_m3`, `net_m3` for a graded
  toposolid; the toposolid schedule has Cut, Fill and Net Cut/Fill fields (Revit's names).

### Property Line (extended)

- `PropertyLine` gains `edges: Vec<Edge>`, one per segment (from point i to i+1), empty when none was given:
  `Edge {setback: f64, kind: String (street|side|rear|corner|""), street: String (a street's name), street_width: f64}`.
- **Buildable line** (`PropertyLine::buildable`): the plot less a strip along each edge as deep as its setback (each
  segment grown by its setback with round ends, unioned, taken away): every point at least each edge's setback from it,
  as a planning setback reads. Rings, counter-clockwise; empty when the setbacks leave nothing. Drawn in plan as a thin
  dashed line (a `propertyline` subcategory, `buildableline`, "Buildable Line"); in `element_json` as `buildable` (rings)
  and `buildable_area_m2`; commands and checks read it from there or from `property_line_table`.
- **Edit Table** (`property_line {table: [...]}` and `property_line_edit {id, table}`): rows
  `{distance, bearing, type?: line|arc, radius?, side?: L|R}` as Revit's: `bearing` a survey bearing, `N 45°30'00" E` (or
  `N45-30-00E`, or a number: an azimuth in degrees clockwise from north), relative to true north (the Project Base
  Point's Angle to True North turns it into the project's axes; 0 without one), `distance` the segment's (an arc's chord),
  an arc's `radius` and which side its centre is (`L` or `R` of the chord, walking along it). From `start` [x, y] (the
  origin without). Not closing, it is closed by a last segment (Revit's Add Line to Close): the reply gives that
  segment's `closure` {distance, bearing}. `property_line_table {id}` reads any property line back as rows (Revit's
  Edit Table on one drawn by sketch converts it to the table).
- `property_line_edit {id, points? | table?, name?, edges?, level?}`: the points anew, or the table, and each edge's
  setback, kind, street and width (`edges` a list as long as the segments, or `{index: {...}}`).
- Tag: the existing `tag` command tags a property line with its area (`propertyline` added to the categories tags
  label, its label `Area`).

### Survey Point and Project Base Point (new elements; `Georeference` kept for the map's name)

- `Element::SurveyPoint(BasePoint)` (category `surveypoint`) and `Element::ProjectBasePoint(BasePoint)`
  (`projectbasepoint`), at most one of each (`Model::check`).

```rust
pub struct BasePoint {
    pub at: P2,          // where it is in the project, mm (from the internal origin)
    pub elevation: f64,  // its height in the project, mm
    pub east: f64, pub north: f64, pub height: f64, // the survey point's shared coordinates (E/W, N/S, Elev), mm
    pub angle: f64,      // the project base point's Angle to True North, degrees: project north is that much
                         // counter-clockwise from true north (true north is that much clockwise from project north)
    pub clipped: bool,   // the survey point's: clipped (default) or unclipped
}
```

- Shared coordinates (`Model::shared(p)`): `(east, north) + R(angle)·(p − sp.at)`, `height + (z − sp.elevation)`.
  Without a survey point, the origin with zeros; without a base point, angle 0.
- As Revit (2020.2 and on): moving the **clipped** survey point moves the shared coordinates with it (its own E/N/Elev
  stay; every other point's change: the project is relocated on the survey); moving it **unclipped** moves the marker
  alone (its E/N/Elev change, the project stays where it is on the survey). Moving the **project base point** changes
  only where it is in the project (its shared coordinates are shown, worked out); typing its E/N/Elev (`base_point
  {east, north, height}`) relocates the project so it reads them (Revit's Specify Coordinates at a Point). Its Angle to
  True North turns the shared coordinates (Revit's Rotate True North).
- `base_point {which: survey|project, at?, elevation?, east?, north?, height?, angle?, clipped?}` (made when missing,
  at the origin) and the Modify tools' `move` (with the clip rule above). Reply: both points, each with `at`,
  `elevation`, its shared `east`, `north`, `height`, the angle, clipped.
- Drawn in plan (when shown: the subcategories are off unless a view shows them, as Revit's): the survey point a
  triangle in a circle, the project base point a circle with a cross, each at its place, a clip mark beside a clipped
  survey point. Pickable and selectable; Visibility/Graphics `surveypoint`, `projectbasepoint` under the Site category.
- Project north and true north: a plan view's orientation (`Project North`, `True North`) as Revit's: the plan drawn
  turned by the angle in True North (`PlanView.orientation`, a view property; default Project North).
  **Note:** if `PlanView` has no orientation field yet, it is added (`#[serde(default)]`), and `render::plan_view` turns
  the drawing by the angle. If that reaches too far into the renderer for this phase, it is left as an open question.
- `Georeference`: a file's numbers (eastings, northings, height, rotation) are read into a survey point and a project base
  point on load when the project has none (`Model::migrate`), and from then the struct holds the map's name and datums
  (`crs`, …, `scale`) while its numbers mirror the points (`masterplan::keep` writes them after each command); `georeference`
  sets the points from the numbers given. IFC export reads the mirrored numbers, as now.

### Planting and Site Component (families)

- Two family categories, `planting` (Revit's Planting) and `site` → spelled `sitecomponent` in files (Revit's Site; the
  word `site` is the masterplan's element category).
- Built-in families (`family::site_library`, original, drawn in code): Planting: **Tree** (types Deciduous 6 m crown 8 m
  high, Small 4 m / 5 m, Conifer 3 m / 10 m; parameters Crown Diameter, Height, Trunk Diameter), **Shrub** (1.5 m, 1 m
  high), **Hedge** (a length of hedge: Length, Width, Height; 1 m by 1.5 m); Site Component: **Bike Rack** (Spaces,
  1 m a space), **Bench**, **Play Equipment** (Radius). Each with plan symbols (a crown's circle and a trunk dot; a
  rack's outline) and solids (a trunk and crown of stacked prisms).
- They come into a project when first placed (as Revit loads a family), not with every new project, so a project with
  none placed still opens in older builds; `planting` and `site_component` load them.
- `planting {family?: Tree, type?, at, host?, angle?, params?}` and `site_component {family, type?, at, host?, angle?,
  params?}`: a component on the toposolid under `at` (or `host`), on its level, its offset the ground's height there.
  After a command that moves one or changes the toposolid under it, it stands on the ground again (`masterplan::keep`).

### What the masterplan's `site` kinds become (`Model::migrate`, format 26)

| kind | becomes |
|---|---|
| `paving`, `terrace`, `landscape`, `water`, `stream`, `pitch`, `playground` | a sub-division of its outline (each hole an opening), type by its surface or kind (Paving, Grass, Meadow, Planting, Water, Sports Surface, Play Surface), offset: paving 40, terrace 150, landscape 20, water -300 (its `level`), stream -200, pitch 40, playground 40; role `kind/use` |
| `lane` (use road) | sub-divisions: carriageway (Asphalt, 0), each pavement and median (Paving / Grass, 150); role `lane/road`, `lane/footpath`, `lane/median` |
| `lane` (other uses) | one sub-division of its outline (Paving, 50; a surface given, that), role `lane/<use>` |
| `tree` | a Tree (Planting) at `at`, its type by its crown and height (a type made for sizes not in the family) |
| `hedge` | a Hedge for each straight piece of its path, as long as the piece, turned along it, its width and height |
| `bike_rack` | a Bike Rack at `at`, its angle, its spaces |
| `play_area` | a Play Equipment at `at` (a control ring: at its middle, its radius the ring's) |
| `car_park`, `parking_band`, `parking_stall`, `ramp` | unchanged (phase 2) |

- The host: the toposolid on the element's level whose outline holds the middle of the element; else the lowest
  toposolid of the project; else (no toposolid at all) one is made under the site: a flat toposolid of the type
  `Toposolid` round everything converted, 5 m past it, on the element's level, named `Ground`. Revit has no
  sub-division without a host.
- A converted element's name, phase (as its Phase Created and Demolished), and project parameters go to what it became.
- `site` refuses the converted kinds, saying which command makes each now (`sub_division`, `road`, `planting`,
  `site_component`). `site_kinds` lists the kinds `site` still takes. The car park still keeps clear of what stands on its
  level: sub-divisions there are its obstacles by their role (`lane/road` a road it is entered from, `water/…`, …), so
  its layout is as it was.

### Road (our addition)

- `road {host?, path, width?, use?, lanes?, ...}`: the masterplan's lane shaping (`site::shapes::normalise`, a `lane`) works
  out the road's carriageway, pavements and median from its centre line, and each is made a sub-division of the host
  (as the conversion above). Returns their ids.

## Commands (all in `ALL`, so the light wasm's `execute` has them)

New: `sub_division`, `excavate`, `remove_excavation`, `graded_region`, `remove_point`, `property_line_edit`,
`property_line_table`, `base_point`, `planting`, `site_component`, `road`. Changed: `property_line` (table, edges,
start), `site` (fewer kinds), `toposolid` (unchanged API; Modify Sub Elements works on it), `edit_shape`, `add_point`,
`add_split_line`, `reset_shape` (toposolids too), `georeference` (sets the points), `edit_boundary` (a sub-division
too), `modify` (a sub-division's type, offset, name, role; a base point). Gone: `building_pad`.

## Files

- `crates/model/src/model.rs`: the variants, `Floor`'s fields, `BasePoint`, `GradedFrom`, the checks, `FORMAT` 26
  (`SITE_FORMAT`), `migrate`'s step (calls `masterplan::convert`).
- `crates/model/src/masterplan.rs`: `SubDivision`, `Edge`, buildable line, surfaces, `toposolid_cut` with excavation,
  sub-division geometry, cut and fill, shared coordinates, the conversions (`convert`), keep steps (georeference mirror,
  planting on the ground).
- `crates/model/src/family.rs`: `Category::Planting`, `Category::Site`; `site_library`.
- `crates/model/src/render/site.rs`, `render.rs`, `view3d.rs`, `ifc.rs`, `json.rs`, `schedule.rs`, `graphics.rs`,
  `motion.rs`, `derived.rs`, `lenient.rs`, `library.rs`, `type_data.rs`, `view3d/update.rs`.
- `crates/core/src/commands/site.rs` (the commands), `commands.rs` (registry, move/copy/delete hosting),
  `commands/clipboard.rs`, `align.rs`.
- `crates/ui-egui/src/site.rs` and friends: Massing & Site's tools: Toposolid, Sub-divide (a type), Excavate, Graded
  Region, Property Line (Edit Table, setbacks in Properties), Site Component, Planting; Building Pad gone; Properties of a
  sub-division, a base point, a property line's edges.
- Tests: `crates/core/src/site_revit_tests.rs` (new), `masterplan_tests.rs` (kinds that went: their tests become the
  conversion's), `ui-egui/src/tests/site.rs`.
- Docs: `docs/areas/site.md` rewritten for what is now.

## Tests to write (names as sentences)

- a_toposolid_takes_points_added_moved_and_removed_as_revits_modify_sub_elements
- a_sub_division_follows_its_hosts_surface_raised_or_sunk_by_its_offset_and_clipped_to_its_host
- a_sub_division_moves_copies_and_goes_with_its_host
- a_floor_that_excavates_a_toposolid_cuts_it_down_to_its_bottom_and_through_when_deeper
- a_deleted_excavator_leaves_the_toposolids_list
- a_graded_region_copies_the_existing_ground_demolishes_it_and_works_out_cut_and_fill
- a_graded_region_from_a_linked_site_copies_its_ground
- a_property_line_from_a_table_of_bearings_and_distances_closes_and_reads_back
- setbacks_draw_the_buildable_line_each_edge_its_own_distance
- the_survey_point_clipped_moves_the_shared_coordinates_unclipped_moves_alone
- the_project_base_point_reads_its_shared_coordinates_and_turns_them_by_true_north
- an_old_georeference_becomes_the_survey_point_and_project_base_point
- trees_and_bike_racks_are_planted_as_families_on_the_ground
- an_old_masterplan_is_converted_on_load (paving, lawn, water, road, tree, hedge, bike rack, play area, pitch, building
  pad; a car park left as it was and still laid out)
- the_site_command_refuses_the_kinds_that_went_saying_what_makes_them_now
- egui: the Massing & Site tab has Revit's tools; Sub-divide sketches on a toposolid; Visibility/Graphics lists the
  site's categories.

## Open questions (decided here; the owner may say otherwise)

1. Property line bearings in the table are taken against **true north** (the base point's angle), as a surveyor's are;
   Revit's documentation is not explicit. Without an angle it is the same.
2. The Project Base Point has no clip (Revit 2020.2 and on); only the Survey Point is clipped or unclipped.
3. Excavation's cut face is 10 mm off vertical (a toposolid is one height field), and a cutter only cuts down: Revit's
   excavation doesn't fill either.
4. A sub-division at offset 0 or below is drawn as a 20 mm skin so it shows on the ground; Revit draws it as a face of
   the toposolid. Its area and schedule are its region's, not the skin's.
5. Planting stands on the ground by finding the toposolid under it on its level (no host field on a component); a
   planting placed off any toposolid keeps the offset it was given.
6. The built-in site families load on first use, not with a new project (an older build reads a project with none).
7. A Graded Region of a linked toposolid needs the link loaded to work out cut and fill; unloaded, it says unknown.
8. Project north / true north as a plan view's orientation (a view turned by the angle) is not built in this phase: the
   angle is kept and used for shared coordinates, the georeference and the property line table's bearings.
   **Follow-up (recorded in review, 16 November):** Revit's view property Orientation (Project North / True North) on
   plan views: `PlanView.orientation` (`#[serde(default)]`), `render::plan_view` turning the drawing by
   `Model::angle_to_true_north`, the egui's Properties row and the view's crop/annotation turned with it.
9. Graded Region keeps a copy of the existing ground as it was taken (`graded_from.ground`), so cut and fill holds when
   the source is a link not loaded or a project passed as text (`source`); `replace` takes it again into the same toposolid.
10. The desktop has Revit's Property Line dialogs (`crates/ui-egui/src/pltable.rs`): Create Property Line asks "by
   entering distances and bearings" or "by sketching"; the Edit Table (distance, bearing, line or arc, radius, L/R;
   Insert, Delete, Up, Down, Add Line to Close) and our Setbacks table (setback, faces street/side/rear/corner, street
   name and width) open from the ribbon's Mode panel with a property line selected. Revit places a table-made line where the next click is; here
   the table's Start (x, y) says where it begins.
11. Visibility/Graphics: the survey point and project base point are hidden unless a view shows them (Site > Survey
   Point / Project Base Point), as Revit's; Reveal Hidden Elements shows them in magenta; hiding Site hides them and the
   property lines.

# Ramps and parking as Revit has them: phase 2, the engine

The plan for phase 2 of Arqiflow's "the site and the car park as Revit has them" (Arqiflow's
`docs/superpowers/specs/2026-11-16-site-and-parking-as-revit.md`, agreed with the owner on 16 November), the engine's part:
one Ramp shown on every level it crosses (stairs too), the Parking category and its family, the car park generator on the
real building, firm sizes, the editor's tools and the commands Arqiflow's server drives. Phase 1 (`docs/site-as-revit-plan.md`)
is built; none of its commands is renamed or reshaped. Revit (2024 to 2026) is the reference; where it is unclear, current
Revit is followed and the choice is written down below.

## 1. One Ramp

### The element (`Element::Ramp`, extended)

- `path: Vec<[f64; 3]>` (new, `[x, y, bulge]` as a property line's points): Revit's ramp by sketch, its Run drawn as lines
  and arcs (Center-ends Arc) end to end, from its foot (on `level`) to its head. A car park's straight, curved and spiral
  ramps are paths: a spiral of more than one turn is a path that goes round again (two half-circle arcs a turn). With a path,
  `start` is its first point and `angle` its first direction (kept in step); `landings` are not used (a flat piece is a
  landing). Without one, the ramp is laid out from `start`, `angle` and `landings` as before.
- `slopes: Vec<f64>` (new): each run's own slope, percent, where it differs from `slope`. Without a path, one a run (the runs
  between its landings: a run's length is its rise over its slope). With a path, one a piece of it (a line or an arc): the
  ramp's rise is shared out along the pieces by their length times their slope, so the ramp reaches its top at the path's
  end (Revit's sketched ramp rises over its run as drawn); a piece of slope 0 is a landing, flat. A car park ramp's
  transitions are pieces of half the slope at each end.
- The runs (`stairs::ramp`): a path is cut in runs at its corners, its arcs in chords within `shapes::ARC_TOLERANCE` of the
  arc; each end of a run has its own cross line (`Run::across`), mitred where two runs meet without a landing, so the outline
  of a curved ramp has no gaps and 3D, IFC and railings work on runs as before. `Run::joined` marks a run that goes on from the
  one before (no line across where they meet in plan).
- `Model::check`: a path of 2 to `MAX_POINTS` points, finite, not of no length; as many slopes as runs or pieces at most, each
  0 to 100%; the top above the base. A steep ramp is allowed (tools, not rules); `element_json` gives each run's slope.
- Move, rotate, mirror and copy carry the path (a mirrored arc's bulge turns the other way).

### In plan, on every level it crosses (stairs too)

As Revit draws a stair or a ramp in a plan: the part from the level below up to the cut plane drawn, the part over the cut
plane dashed up to the view range's top (Revit's `<Above>` lines), nothing over that. Plan of level L (elevation e, the level
below at b, the cut plane at c, the view range's top at t): a stair or a ramp shows when its heights reach between b and t.
Its pieces from max(b, its base) to c are drawn solid; from c to min(t, its top) dashed; its walk line from max(e, its base)
up is marked UP, the part below e (coming up from the level below) marked DN. On its base level this is what was drawn; on
its top level the whole flight below, DN, as before; on a level a multi-storey ramp passes through, both. Its arrows and text
as before. The extents of the plan take what it draws.

### The masterplan "ramp" kind is converted (format 27, `PARKING_FORMAT`)

A `site` ramp (`path` from its open end, on the ground, down to `to_level`) becomes an `Element::Ramp`: its path reversed
(foot first), from `to_level` (by name; else the level below its own) up to its own level, its width, its gradient as its
slope; no level below, it climbs from its level by its length times its gradient. Its GUID, name and params go with it. `site`
refuses `ramp`, saying `ramp` makes it.

## 2. The Parking category and its family

- `family::Category::Parking` (`parking` in files), a row of Visibility/Graphics and Object Styles among the family
  categories, with two subcategories (`parkingline` "Stall Lines", `parkingmarking` "Markings", Revit's names) that a
  view hides apart from the rest; the family's symbols are in those subcategories (`Stall Lines`, `Markings`), and the plan
  leaves out a symbol whose subcategory the view hides. IFC: `IfcBuildingElementProxy` (as Revit exports Parking).
- The engine's own family **Parking Space** (`family/site.rs`, loaded on first use as phase 1's are), types **Standard**
  (2500 x 5000, 90°), **Accessible** (3700 x 5000: the bay and its access aisle), **EV** (2500 x 5000) and **Motorcycle**
  (1000 x 2200); type parameters Width, Depth, Angle (the stall lines' angle to the aisle, degrees); instance parameter Mark
  (text: the bay's number). Its origin is the middle of the bay; its x along the aisle, its y into the bay (the open side,
  the mouth, at y = -Depth / 2); an angled bay is a parallelogram whose sides lean at Angle. Symbols: the two stall lines and
  the back line (Stall Lines); a wheelchair mark or EV letters (Markings). A thin pad as its 3D form.
- Placed as Planting is: on the toposolid or sub-division under it (its offset the ground's there), else on the floor under it
  on its level, else at its level; it stays on the ground as the ground changes (`ground::keep`). Revit's Parking Component
  has no host field either: `element_json` names what it stands on (`host`).
- Selectable, taggable (`tag`'s label Mark), schedulable: a `parking` schedule category (Revit's Parking Schedule): Mark,
  Type, Level, Width, Depth, Angle, Count, Family, Phase Created/Demolished, Design Option; count by type and level by sorting
  and grouping as any schedule does.
- `parking_stall` and a `parking_band`'s bays are converted on load: each a Parking Space of the type its use says (Standard,
  Accessible, EV), at the middle of its outline, turned along its aisle, its Width and Depth what it was where they differ
  from the type's (a type made for them), Mark its number; a band's aisle a sub-division of type Drive Aisle on the ground.
  `site` refuses both kinds, saying what makes them now (`parking_component`, `car_park`).

## 3. The car park generator (`model/src/carpark.rs`)

The solver (`site/parking/*`) keeps its layouts; what it reads and writes changes.

- **The element**: the masterplan's `car_park` (a `Site` element of kind `car_park`, Arqiflow's, as phase 1 left it): its
  area (`control`) on a level and its settings (props as before). Its `layout` blob goes; its result is in `props.result`
  (counts, notes, `proposed_columns`, `ways`, `stalls`, `generated`). Phase 1's command API (`site` with kind `car_park`,
  `site_edit`) stays.
- **What it reads, as Revit's cut plane would show it on its level** (the storey from its level up to the level above, or
  2.5 m without one): walls, structural and architectural columns, shafts, stairs, ramps (their runs and landings that are in
  the storey, or under it down to the level below: a floor opening), floors' openings on the level, masses, components but
  its own bays and planting, sub-divisions by role (as phase 1), and the other car parks' bays. Building elements are kept
  clear exactly (no footway); masses and the masterplan's kept-clear roles with the footway as before. Street edges: the
  property lines' edges of kind street, and roads (sub-divisions `lane/road`), above ground.
- **Columns**: the real ones only. It never makes its own grid (`column_every` is 0 for it). On a level below ground with no
  column inside its area it works out, as well, the grid it would have laid (the solver's, a column every three bays) and
  returns it as `result.proposed_columns` ([x, y] centres): not placed.
- **Ways in**: below ground, the real ramps touching its level (a ramp whose foot or head is on the level: its end there is a
  way in, the rest of it kept clear); above ground, its `access` points and street sides as before. **It never invents a
  ramp**: below ground with no ramp reaching its level it lays out nothing and says so (`result.notes`: "No ramp reaches B1:
  add one, or say where (`access`)."). A basement way in placed by hand (`access`) is taken as a ramp's foot there.
- **What it writes**: Parking Space instances (Standard, Accessible, EV) on the car park's level, Mark numbered from 1, each
  with `car_park` (the car park's GUID) and `placed` (where and how it placed it) in its params (names no project parameter
  may take, as phase 1's `source`); drive aisles at ground level as sub-divisions of type Drive Aisle (asphalt) on the
  toposolid under the car park, role `car_park/aisle`; below ground nothing but the bays (the floor is the aisle; Arqiflow's
  aisle arrows are in `result.aisles`, drawn by the plan as annotation of the car park).
- **Pinned bays**: a bay of its own whose place, angle, type or level differs from its `placed` (the person moved or changed
  it), or that is pinned (Revit's Pin, `Model::pinned`), is kept as left and pinned; the bays it would place over it are left
  out. A bay of its own the person deleted adds its middle to the car park's `removed` (the solver plants a bay there). Its own
  unpinned bays and aisles are replaced (the same ids kept where a bay comes back where it was, so nothing churns).
- **When it runs**: after a command, when its area or settings changed, or any element on its level (in its storey) near it
  changed, or a level changed; by `car_park_run {id}`; deleted, its unpinned bays and aisles go with it, its pinned bays stay
  (their link cleared).
- The library is synced after the generator loads the family in a command's keep step (`engine.rs`).

### Firm sizes

`shapes::FIGURES` gains the firm's names: `parking.stall.width` (2500), `parking.stall.depth` (5000), `parking.aisle.width`
(6000), `parking.aisle.one_way` (3600 — the one-way aisle width), `parking.stall.angle` (90), `parking.accessible.width`
(3700), `parking.ev.share` (0), `parking.ramp.slope` (15%). A figure is read by its new name first, the old one
(`parking.bay_width`, `parking.bay_depth`, `parking.aisle`) as a fallback (`shapes::figure_of(&[new, old])`). The car park
takes them as its defaults where its own props say nothing; a parking band's and stall's defaults read them too.

### Conversion on load (format 27)

Each car park's `layout` is dropped and it is run again, keeping its `removed` points (the bays the person deleted).

## 4. Base points can't be deleted (review leftover)

`delete` refuses the survey point and the project base point, as Revit does ("can't be deleted; hide it with
Visibility/Graphics, or move it").

## 5. The editor (egui)

- Massing & Site: **Parking Component** places a Parking Space of the Options Bar's type (Standard, Accessible, EV,
  Motorcycle), Space turns it (was a masterplan stall). **Parking Area** sketches a car park's area (the row of stalls along a
  path goes: a row is a car park of one row, or Parking Components); its settings in Properties. A car park selected: **Run**
  on the ribbon (`car_park_run`).
- Ramp: the Options Bar takes Straight or Arc (the ramp by sketch, lines and Center-ends arcs end to end), its width and slope.
- Schedules: Parking is among the schedule categories. Visibility/Graphics lists Parking with Stall Lines and Markings.

## 6. Commands (crates/core, all in `ALL`, so crates/web's `execute` has them)

- New: `parking_component {at: [x,y], type?: Standard|Accessible|EV|Motorcycle, family?, host?, level?, angle?, mark?,
  params?}`; `car_park {control: [[x,y],...], level?, ...settings} ` (a car park made and run: the settings are the car park
  props); `car_park_run {id}` (run again; returns the result).
- Changed: `ramp {..., path?: [[x,y,bulge?],...], slopes?: [..]}` (with a path, `start` and `angle` may be left out);
  `modify` takes a ramp's `path`, `slopes`; `site` refuses `ramp`, `parking_stall`, `parking_band`; `delete` refuses the base
  points; `schedule {category: parking}`.

## Files

- `model/src/model.rs` (Ramp fields, check, `PARKING_FORMAT` 27, migrate step), `stairs.rs` (runs from a path, mitres),
  `render.rs` (stairs and ramps on every level; subcategory hiding), `motion.rs`, `json.rs`, `view3d.rs`, `ifc.rs`.
- `model/src/family.rs`, `family/site.rs` (Parking category and family), `graphics.rs`, `schedule.rs`, `ground.rs` (reserved
  params, conversions of stalls, bands and ramps), `carpark.rs` (new: the generator), `masterplan.rs` (the car park no longer
  laid out there), `site/parking/mod.rs`, `solve.rs` (no invented ramp, given feet, kept bays), `site/shapes.rs` (figures).
- `core/src/commands.rs`, `commands/ground.rs` (the commands), `engine.rs` (library sync after keep).
- `ui-egui/src/site.rs` and friends.
- Tests: `core/src/parking_tests.rs` (new), `ui-egui/src/tests/site.rs`.

## Tests to write

- a_ramp_by_path_climbs_along_lines_and_arcs_its_rise_shared_by_slope
- a_ramp_shows_on_every_level_it_crosses_dashed_over_the_cut_plane (and a stair)
- an_old_masterplan_ramp_becomes_a_ramp_element
- a_parking_component_stands_on_the_ground_or_the_floor_with_its_type_and_mark
- parking_stalls_and_bands_are_converted_to_parking_components
- the_parking_schedule_counts_bays_by_type_and_level
- a_car_park_keeps_clear_of_walls_columns_and_shafts_on_its_level
- a_car_park_below_ground_takes_the_real_ramps_as_ways_in_and_never_invents_one
- a_car_park_places_no_columns_and_proposes_a_grid_below_ground
- a_moved_bay_is_pinned_and_kept_a_deleted_bay_stays_removed
- a_car_park_runs_again_when_a_wall_on_its_level_changes
- firm_sizes_are_read_by_the_firms_names_and_the_old_ones
- an_old_car_park_is_run_again_on_load_keeping_its_removed_bays
- the_base_points_cant_be_deleted
- egui: Parking Component places a family instance; Parking Area makes a car park; Ramp by sketch.

## Open questions (decided here; the owner may say otherwise)

1. The car park stays a masterplan `Site` element of kind `car_park` (phase 1's API is kept, and Arqiflow's helper builds
   on it); it is the generator's settings and area, not a Revit element (Revit has none: its bays come into Revit as Parking
   Components and the area as nothing).
2. A Parking Component has no host field (Revit's has none: it is level-based and follows the toposolid's surface); `host`
   in `parking_component` picks the surface it stands on, and `element_json` gives the one under it.
3. Real columns are kept clear exactly; the layout is not shifted to fit a column grid (it picks the direction and
   arrangement that keep the most bays among them). Fitting bays between columns is a refinement for later.
4. Below ground, the aisle arrows are the car park's annotation (from `result.aisles`), not elements.

## Added during the build (asked for by Arqiflow's side)

1. **Project parameters on the site's elements**, as Revit allows: `toposolid`, `propertyline`, `subdivision`,
   `surveypoint` and `projectbasepoint` are categories a project parameter may be for (`parameter {categories}`), set by
   `modify {id, params}`.
2. **A toposolid's contours after it is made**: `modify {id, contours}` (the interval, mm; 0 none) and `modify {id,
   contours_hidden: true}` (Revit's Contour Display unticked: the interval kept, none drawn).
3. **IFC's map conversion from the base points alone**: IfcMapConversion and an IfcProjectedCRS are written from the survey
   point's and project base point's shared coordinates whenever the project has them; a `georeference` only names the map.
4. **A road keeps its definition**: each sub-division `road` makes holds `road: {of, use, name, props}` (`of` the road's first
   piece's GUID, the same on each piece; `props` its centre line `path` and its sizes with its defaults filled in);
   `road {replace: <a piece's id>, ...}` makes it again over what it had, its first piece keeping its id. A format-25 lane
   converted on load keeps its path and widths the same way.

## Review of phase 2 (16 November)

Fixed: a plan whose aisle runs through a column, a wall or a ramp is not taken (the ring layout's cross aisles ran through
columns: that was where a column grid's "good" yield came from); a field's aisles are set just clear of each row of posts and
its rows step round them; a way in off a ramp's foot (or a street) no longer runs through walls, columns or ramps (the solver's `clear` takes
what is kept clear exactly); a ramp's foot that can't be joined to an aisle is said; posts (holes of 2 m or less, real
columns) are stepped round, not cut along (a column grid laid out in milliseconds, not tens); a long wall across a car park
with no end near it is kept clear (nearness by boxes, not corners); far components' pieces aren't worked out; a moved bay
pinned has its `placed` brought to where it is, so unpinned it is the car park's again, and pinning or unpinning a bay runs
its car park; the help of `road`, `parking_component`, `car_park` and `ramp` had lost its line breaks.

Left (the first three, and the access aisles, ways below ground and the Mark, done in the solver rework below):
- **The solver round ramps inside the area.** A basement 60 x 40 m with its walls, a column grid and two Ramps inside it (one
  along a wall, one curved in a corner) gets a handful of bays where a designer lays some 60
  (`a_basement_..._is_filled_as_a_designer_would`, ignored until it passes). The solver cuts the plot once at an inside
  corner; ramps at two sides need cuts within cuts, and an aisle laid to meet each foot.
- **The solver round a column grid**: a 60 x 40 m basement with an 8.1 x 10 m grid gets a dozen bays where a designer fits
  some 60 (`below_ground_a_column_grid_is_stepped_round_...`). Fields of aisles across the grid, sized to its bays, are what
  it lacks.
- Bays are not fitted between real columns (a column in a row costs one or two bays): the field's bay grid could take the
  columns' pitch (`column_every`, `column_size`) and phase from the real grid.
- An accessible bay's access aisle is not placed (the solver's `access_aisles`): the Accessible type is the bay alone at the
  stall's width; Revit's accessible families draw the hatched aisle.
- Below ground `result.ways` stays empty; the ramps' throats are in `result.aisles` (no `kind`).
- `placed` holds the Mark under the English name only: in another language a renumbered bay isn't "changed".
- A masterplan ramp, stall or band of a file of 26 that has no path or point is dropped when converted, not kept.

## Solver rework (16 November): modules on the grid, as a designer lays them

The reviewer's basement (60 x 40 m in its walls, an 8.1 x 10 m column grid, a straight ramp along the north wall and a curved
one in the south-east corner) got 7 bays where a designer lays some 60: the ported solver cuts the plot once at an inside
corner, sets out fields between the cuts, and has no idea of a grid. The rework adds a planner that lays a car park out the
way a designer does (`site/parking/grid.rs`); the solver's other layouts (fields, ring) stay as candidates and the best by
bays reached wins, so a surface car park can only gain.

**The stack.** In each direction tried (the area's long sides and square to them, a column grid's axes, each ramp's way
off), the area is cut across into a stack of modules: an aisle with a row of bays on one side or both (double-loaded the
norm), and gaps where nothing fits. The stack is found by dynamic programming over the depth of the area (every 100 mm),
each module scored by the bays it gets less the paving it takes (as the solver scores: 100 m² of aisle a bay), so the aisles
fall where they serve most bays. What stands on the level is read exactly: a row's bays are set out in the spans its whole
depth is clear, so they fall between the columns (three 2.5 m bays between columns 8.1 m apart, the column within the row's
depth or at its back); an aisle is set where it is clear along its length, so it runs between the column lines, and an aisle
cut in pieces by what stands in it costs the joints it needs. A ramp's foot whose way off runs along the aisles is met by an
aisle in line with it (the stack is also tried with its phase set by each foot); one whose way off runs across them is met by
a short way straight off it into the first aisle, the bays it crosses given up.

**Circulation.** The stack's aisles are joined by cross aisles at right angles, chosen greedily: from the aisles a ramp's foot
(or, above ground, the biggest group) reaches, the cross aisle that brings in the most bays for the fewest it costs, set
between the columns, until none gains. Cross aisles that cost no bay are added where they close a loop at an aisle's end;
one-way car parks get a cross aisle at both ends of every aisle and their aisles' traffic runs round (each aisle the other
way to the one beside it). An aisle runs only as far as what it serves. Bays and aisles no car can reach are left out.

**Irregular areas** are laid out by the same stack: its rows and aisles take the spans of the area at their depth, so an L
or a notched plot gets modules in each part, joined by the cross aisles; each direction is tried. The fields' cut at an inside
corner stays a candidate.

**Kept**: never a ramp made up; columns proposed below ground when there are none, now from the chosen layout (a column after
every three bays, at the backs of the rows, so the grid proposed is the one that gave the most bays); bays removed or kept by
the person; ids stable (the bays' places are on the same lattice run to run). Angled bays stay with the fields and the ring
(the stack lays 90° bays).

**Also fixed**: an accessible bay's access aisle (the bay beside it, shared by two accessible bays where a third fits) is
written (`result.access_aisles`); the accessible bays go nearest the cores (shafts, stairs) below ground when no target is
given; below ground `result.ways` holds each ramp's way in (`{at, kind: "both", ramp: true}`); a bay's `placed` reads its Mark
by the model's language's name; a way off a ramp is the ramp's width (not a drive aisle's), so it no longer grazes the wall
beside the ramp; 90° bays on a one-way car park take a two-way aisle's width (a car needs it to turn in), not the one-way
figure meant for angled bays.

### What it came to (16 November)

- **The reviewer's basement** (`a_basement_with_walls_a_column_grid_and_two_ramps_is_filled_as_a_designer_would`, no longer
  ignored): 7 bays before, one ramp joined; now **36** on the standard's sizes (Saudi Arabia's 2.8 x 5.5 m bays on 7.5 m
  aisles: two bays fit between columns 8.1 m apart and the aisles only just fit between the column lines; a designer gets some
  35 to 40), and **57** on the 2.5 x 5 m bays and 6 m aisles its 8.1 m grid is drawn for (17 before); both ramps lead onto
  aisles. Pictures, the plan of B1: `docs/parking-real-basement-before.png` and `-after.png` (the standard's sizes),
  `-before-2500.png` and `-after-2500.png`. The 50 the reviewer asked for is reached on the sizes the grid was drawn for; on
  the standard's it can't be (the hand count is under 40).
- **A plain basement on a grid** (64.8 x 48 m, columns at 8.1 m and at the backs of the rows): 132, the hand count.
- **An L-shaped basement**: 67, both arms laid out, the drive off the ramp along the south wall.
- **An irregular surface lot with two street entrances**: 186 bays, both ways in joined.
- **Arqiflow's 49 layouts** (the fixtures): 9,460 bays as Arqiflow's solver laid them, 9,739 now; 18 laid out anew, each with
  more bays (e.g. 246 to 265, 161 to 191, 202 to 230), none with fewer; the other 31 exactly as before.
- **Time**: the reviewer's basement 60 to 105 ms in the solver (a test build, optimised at level 1), the whole `car_park`
  command under a second; a 100 x 60 m basement on a grid, 243 bays, 167 ms.

Left:
- Angled bays are laid out by the fields and the ring only (the stack lays 90° bays).
- One direction a layout: an L whose arms want their aisles at right angles to each other gets one direction for both (the
  fields' cut at the inside corner, which turns each part, is still a candidate).
- A dead end is not limited in length nor given a turning head; cross aisles that close a loop are added only where they
  cost no bay (or on a one-way car park, always).
- The accessible bay is a Parking Space of the stall's width beside its access aisle (`result.access_aisles`); the family
  draws no hatch for the aisle.
- A masterplan ramp, stall or band of a file of 26 that has no path or point is dropped when converted, not kept.

# The site and masterplan (Massing & Site)

Arqiflow's "site option B" (10 November): the site and the building are drawn in one model and one editor. Until now
Arqiflow kept its site and masterplan in an editor of its own and linked it under the building, halftone. Here they are
BIMCraft elements and tools, in the **Massing & Site** tab, after Revit's where Revit has one (Property Line, Toposolid,
Building Pad, Site Component, Parking Component, Planting) and after Arqiflow's masterplan where it does not (roads with
a cross-section, car parks laid out, pitches, ponds, streams).

The behaviour, the figures and the rules are Arqiflow's, ported: `server/arqiflow/kernel/site_kinds.py` and `roads.py`
(the server's), `web/components/editor/geom/site.ts` and `roads.ts` (the browser's, the same shapes within 5 mm, by the
fixtures in `server/tests/fixtures/site_kinds/`), and `web/lib/parking/` (car parks). Arqiflow's repository is at
`G:/Software Development/arqiflow`.

The code: `site/` is the pure geometry (`shapes.rs`: `normalise`, `catalog`; `parking/`: `lay_out`); `masterplan.rs`
holds the elements, keeps their shapes up to date after every command (`keep`), moves their truth with the Modify tools
(`transform_props`), checks them and builds them in 3D (`solids`); `commands/site.rs` the commands; `render/site.rs` the
plan. Tests: `masterplan_tests.rs`, and the site's payloads in `hostile_params_never_panic_or_break_the_model`.

## The elements

- **`site`** (`Element::Site`, `masterplan::Site`): one element of the masterplan. It holds
  - `kind` and `use`: what it is (below) and its use within the kind (kept: the kind's first when none was given);
  - `name`, `level` (GUID), `phase` (Arqiflow's: `new`, `existing`, `demolished`, `planned`), `source` and `ref` (where
    context came from: `site` for the plot's own streets, `osm`, `overture`, …, and its id there). An `existing`
    element from `site`, `osm` or `overture` is **context** (Arqiflow's `isContext`), not the design;
  - `props`: **what a person drew, its truth** (a lane's `path` and `width`, a pond's `control` points, a pitch's `at`,
    `angle` and size), lengths and points in **millimetres**; what the figures filled in (`site::shapes::fill_defaults`)
    is kept here too;
  - `outline` and `parts`: **its shape, worked out from the truth** (`site::shapes::normalise`) and kept in the file, so
    a reader never works it out again (Arqiflow's server reads them);
  - a car park's `layout` (`site::parking::lay_out`): its pieces (aisles, stalls, access aisles, columns, ramps, each
    with `band`, the car park's GUID, in its props, as Arqiflow's children carry their car park's id) and its `result`
    (stalls, accessible, ev, columns, arrangement, ways, notes…);
  - `notes`: what the person should be told of its shape (a pitch outside its type's sizes; why a shape couldn't be
    worked out);
  - `params`, as every element: its `Phase Created` and `Phase Demolished` (below) and the project's parameters.
- **`propertyline`** (`Element::PropertyLine`, `masterplan::PropertyLine`): Revit's Property Line, the plot's boundary:
  `points` [x, y, bulge] (the bulge DXF's, of the piece from that point to the next; 0 straight), closed, counter-
  clockwise. `PropertyLine::area` is exact (the polygon and each arc's segment); `ring` its line with the arcs in pieces
  within 20 mm.
- **`toposolid`** (`Element::Toposolid(Floor)`): Revit's Toposolid, the ground: a floor in a category of its own, so it
  is everything a floor is (its outline and openings, a floor type's thickness, `shape` points at their heights,
  `contours`, the Modify tools, Edit Boundary's sketch) and is drawn, scheduled and exported as the ground.
- **`buildingpad`** (`Element::BuildingPad`, `masterplan::BuildingPad`): Revit's Building Pad: a flat pad along
  `boundary`, its top `offset` over its `level`, a floor type's thickness under that (`Model::pad_span`).
- **`mass`**: a building on the masterplan is a mass (BIMCraft's), with the masterplan's figures for it under `site`
  (`masterplan::MassSite`: `use`, `phase`, `source`, `ref`, `props`: storeys, floor_height, attic, roof, pitch, ridge,
  overhang, steps, arc, dormers, windows, entrance, materials, unit_type, label, …), its height storeys × floor height
  when it has storeys (the `building.floor_height` figure where it gives none; `masterplan::mass_height`).

### What a building pad does

A pad cuts the toposolid on its level that it lies wholly inside (its corners inside the toposolid's outline, none of the
toposolid's corners inside the pad, apart from the toposolid's openings and from the other pads that cut it;
`Model::pads_in`). `Model::toposolid_cut` is the toposolid with each such pad's outline an opening, and with each of the
pad's corners a shape point at the ground's height there, so the ground round the pad keeps its slope. The plan and the
3D view draw that: the toposolid's fill and its contours stop at the pad's edge, the hole runs through the toposolid's
whole depth, and the pad is a slab in it at its own height. IFC writes the cut toposolid. What it does **not** do: it
doesn't grade the ground down or up to the pad's bottom round its edge (Revit cuts and fills the toposolid to the pad's
depth; here the sides of the hole are the toposolid's whole depth, under the pad and over it), it doesn't cut a
toposolid it only partly lies over, and it doesn't slope (Revit's pad can).

### In a file and in `element_json`

Each element as the file and `element_json` hold it (the element's serde, tagged by `category`; references are GUIDs;
`element_json` adds `id`, `guid` and what it measures, below). A road (context):

```json
{"category": "site", "kind": "lane", "use": "road", "name": "Street", "level": "<level GUID>",
 "phase": "existing", "source": "site", "ref": "street:edge 0",
 "props": {"path": [[-10000.0, -4000.0], [79950.0, -4000.0]], "width": 6000.0, "lanes": 2, "lane_width": 3000.0,
           "kerb": 150.0, "corner_radius": 6000.0, "surface": "block_paving", "heads": {"start": null, "end": null}, …},
 "outline": [{"outer": [[-10000.0, -7000.0], [79950.0, -7000.0], [79950.0, -1000.0], [-10000.0, -1000.0]]}],
 "parts": [{"role": "path", "lines": [[[-10000.0, -4000.0], [79950.0, -4000.0]]]},
           {"role": "carriageway", "polys": [{"outer": […]}]}, {"role": "pavement", "polys": […]},
           {"role": "kerb", "lines": […]}, {"role": "pavement_edge", "lines": […]}, {"role": "centre_line", "lines": […]}],
 "params": {"Phase Created": "Existing"}}
```

A car park:

```json
{"category": "site", "kind": "car_park", "use": "parking", "level": "<level GUID>",
 "props": {"control": [[0, 0], [60000, 0], [60000, 40000], [0, 40000]]},
 "outline": [{"outer": [[0.0, 0.0], [60000.0, 0.0], [60000.0, 40000.0], [0.0, 40000.0]]}],
 "layout": {"pieces": [{"kind": "aisle", "level": "Level 1", "props": {"part": "aisle", "band": "<car park GUID>"},
                        "outline": [{"outer": […]}]},
                       {"kind": "stall", "level": "Level 1", "props": {"index": 0, "mouth": [x, y], "band": "<car park GUID>"},
                        "outline": [{"outer": […]}]}, …],
            "result": {"stalls": 69, "accessible": 2, "accessible_needed": 2, "ev": 0, "columns": 0,
                       "arrangement": "fields", "directions": [0.0], "basement": false, "area_per_stall": 34.8,
                       "ways": [{"at": [x, y], "kind": "both"}], "notes": []}},
 "params": {"Phase Created": "New Construction"}}
```

A parking stall (Revit's Parking Component), a tree:

```json
{"category": "site", "kind": "parking_stall", "use": "accessible", "level": "<level GUID>",
 "props": {"at": [0, 0], "angle": 0, "width": 2500.0, "depth": 5000.0},
 "outline": [{"outer": [[-2500.0, -1250.0], [2500.0, -1250.0], [2500.0, 1250.0], [-2500.0, 1250.0]]}],
 "parts": [{"role": "accessible_stall", "polys": […]}, {"role": "mouth", "lines": [[[-2500.0, -1250.0], [-2500.0, 1250.0]]]},
           {"role": "point", "lines": [[[0.0, 0.0]]]}], "params": {"Phase Created": "New Construction"}}
{"category": "site", "kind": "tree", "use": "tree", "level": "<level GUID>", "phase": "existing", "source": "osm",
 "props": {"at": [0, 0], "crown": 6000.0, "height": 8000.0},
 "outline": [{"outer": [… 32 points …]}], "parts": [{"role": "crown", "polys": […]}, {"role": "point", "lines": [[[0.0, 0.0]]]}],
 "params": {"Phase Created": "Existing"}}
```

A property line, a toposolid and a building pad, a mass on the masterplan:

```json
{"category": "propertyline", "name": "Plot 4", "level": "<level GUID>",
 "points": [[0.0, 0.0, 0.0], [10000.0, 0.0, 0.0], [10000.0, 10000.0, 0.2], [0.0, 10000.0, 0.0]]}
{"category": "toposolid", "type": "Toposolid", "level": "<level GUID>",
 "boundary": [[0.0, 0.0], [40000.0, 0.0], [40000.0, 40000.0], [0.0, 40000.0]],
 "shape": {"points": [{"id": 100004, "guid": "…", "at": [40000.0, 0.0], "z": 4000.0}, …]}, "contours": 1000.0}
{"category": "buildingpad", "type": "Building Pad", "level": "<level GUID>",
 "boundary": [[15000.0, 15000.0], [25000.0, 15000.0], [25000.0, 25000.0], [15000.0, 25000.0]], "offset": 1500.0}
{"category": "mass", "name": "Mass", "level": "<level GUID>", "boundary": […], "top_level": null, "height": 9000.0,
 "base_offset": 0.0, "site": {"use": "housing", "phase": "existing", "source": "overture", "ref": "building:overture:17",
 "props": {"storeys": 3, "roof": "flat", "label": "B6"}}, "params": {"Phase Created": "Existing"}}
```

Left out when empty: a site element's `name`, `source`, `ref`, `outline`, `parts`, `layout` (only a car park has one),
`notes` and `params`; `phase` when `new`; a part's `polys` or `lines`; a pad's `offset` when 0. `element_json` adds:
a site element's `area_m2` (its outline's, less its holes) and a car park's `stalls`; a property line's `area_m2` and
`perimeter_m`; a pad's `area_m2`, `thickness` and `elevation` (its top); a toposolid what a floor's has (`area_m2`,
`thickness`, `volume_m3`, `top`…).

A site element's `phase` and its params are kept in step after every command (`masterplan::keep`): `existing` is made in
the project's first phase, `demolished` made in the first and taken down in the last, `new` and `planned` made in the last
(`masterplan::phase_params`). The phase given wins; where only the params changed (the Demolish tool's `modify {params:
{Phase Demolished}}`), the phase follows them. So phase filters and the Renovation menu show and hide them as any element.

### The kinds

| kind | Revit's | uses (the first is the default) | drawn from |
|---|---|---|---|
| `lane` | (roads: Civil's) | lane, footpath, road, promenade | `path`, `width`; a road its cross-section |
| `parking_band` | Parking Component | parking | `path` |
| `car_park` | Parking Component, laid out | parking | `control` |
| `parking_stall` | Parking Component | standard, accessible, ev | `at`, `angle` (the way its depth runs), `width`, `depth` |
| `paving` | Toposolid subdivision | forecourt, apron, square, court | `control` |
| `terrace` | Toposolid subdivision | terrace | `control` |
| `landscape` | Toposolid subdivision | lawn, meadow, planting, garden, woodland, orchard, bank | `control` |
| `tree` | Planting | tree | `at` |
| `hedge` | Planting | hedge | `path` |
| `water` | Site Component | pond, lake, wetland, basin | `control` |
| `stream` | Site Component | stream, rill, ditch | `path` |
| `pitch` | Site Component | five_a_side, futsal, football_7, football_11, basketball, tennis, padel, multi_sport | `at`, `angle` |
| `playground` | Site Component | playground | `control` |
| `play_area` | Site Component | equipment, toddlers, juniors | `at` and `radius`, or `control` |
| `bike_rack` | Site Component | residents, visitors | `at` |
| `ramp` | (a site ramp) | cars | `path` |

What each kind's props are, which are lengths and which are points, and the figures they default from: the
`site_kinds` command answers it (`site::shapes::catalog`), so Arqiflow converts between its metres and these
millimetres from the engine's own list. A parking stall's width and depth default from `parking.bay_width` and
`parking.bay_depth`.

### The tools (owner's decision, 10 November)

The ribbon tab is **Massing & Site**: BIMCraft's massing tools, then Revit's site tools under Revit's names, and two
of Arqiflow's beside them under familiar names. Buildings on the masterplan stay masses.

| Tool | Makes | Command |
|---|---|---|
| Toposolid | a `toposolid`: the ground, from the survey, with its terrain | `toposolid` |
| Property Line | a `propertyline`: the plot | `property_line` |
| Sub-divide / Graded Region | a `site` area cut into the ground: `paving`, `terrace`, `landscape` (lawn, meadow, planting…), `water`, `stream`, `pitch`, `playground` | `site` |
| Road (beside Sub-divide) | a `site` `lane`: a path with a width (lane, footpath, road with its cross-section, promenade) | `site` |
| Site Component | a `site` `tree`, `hedge`, `bike_rack`, `play_area` (trees, planting, furniture) | `site` |
| Parking Component | one stall: a `site` `parking_stall` (bay width by depth, at a point and an angle) | `site` |
| Parking Area (beside Parking Component) | a `site` `parking_band` (a row of stalls along a path) or `car_park` (an area laid out and counted) | `site` |
| Building Pad | a `buildingpad`: a flat pad cut into the toposolid at a height | `building_pad` |

In the Revit build each becomes Revit's own element: a toposolid, a property line, a sub-division, planting and site
components, parking components, a building pad.

### In the desktop (`crates/ui-egui/src/site.rs`)

The tab is **Massing & Site** (`Tab::Massing`): Conceptual Mass and Model by Face, then the panels **Model Site**
(Toposolid, Sub-divide with Road small beside it, Building Pad), **Modify Site** (Property Line) and **Components** (Site
Component, Parking Component with Parking Area small beside it). Every tool is `Tool::Site(SiteTool)`; BIMCraft's own
profile shows the same panels in its Tools palette. Each has its icon in `icons.rs` (`Icon::Toposolid`, `Subdivide`, `Road`,
`BuildingPad`, `PropertyLine`, `SiteComponent`, `ParkingComponent`, `ParkingArea`), and an entry in Keyboard Shortcuts and
Find (TS, BP and PL run the tools as the commands' aliases).

How each takes its clicks (`SiteChoices::taking`, by the shape of the kind it makes):

| Tool | Clicks | Options Bar | Makes |
|---|---|---|---|
| Toposolid | sketch mode at once (as Floor): the Draw panel's lines, arcs, rectangles…, Finish Edit Mode | | `toposolid {boundary}` |
| Sub-divide | sketch the area; a pitch, a click where its middle goes (its standard size), Space a quarter turn | Type (Paving, Terrace, Landscape, Water, Stream, Pitch, Playground), Use (the type's uses: lawn, meadow…; pond, lake…; tennis, padel…) | `site {kind, use, props: {control}}`; a stream `{path}`; a pitch `{at, angle}` |
| Road | sketch its path: lines and arcs end to end, Finish | Use (Lane, Footpath, Road, Promenade), Width, Lanes (a road's) | `site {kind: lane, props: {path, width?, lanes?}}` |
| Building Pad | sketch its outline, Finish | Height Offset | `building_pad {boundary, offset?}` |
| Property Line | sketch the plot's loop, arcs kept as bulges, Finish | | `property_line {points}` |
| Site Component | a tree, a bike rack or a play area: a click, Space a quarter turn; a hedge: sketch its path | Type (Tree, Hedge, Bike Rack, Play Area), Use, Crown / Width / Radius / Spaces | `site {kind, props: {at, angle?, crown?…}}` or `{path}` |
| Parking Component | a click, Space a quarter turn | Use (Standard, Accessible, EV) | `site {kind: parking_stall, props: {at, angle}}` |
| Parking Area | sketch an area (Area) or a path (Row), Finish | Area or Row | `site {kind: car_park, props: {control}}` laid out, or `parking_band {path}` |

A sketch is `sketch::Of::Site {level, tool}`: its contextual tab is the sketch's (Draw, Modify, Mode), its Options Bar the
tool's, and Finish (`finish_site_sketch`) solves an outline with the core's `sketch::solve_one` (one outline, no holes) or
joins a path with `sketch::chain` (lines end to end into `[x, y, bulge]` points; a property line's must close, a path's
must not). An Options Bar choice that changes how the tool takes its clicks (Sub-divide's Pitch, Site Component's Hedge,
Parking Area's Row) begins the tool again that way, while the sketch has no lines. Previews: a placing tool draws what a
click would make under the cursor (`site::ghost`, the element shaped alone by `shapes::normalise` and drawn by
`render::sketch`); a path being sketched draws the road, hedge or row its lines make so far (kept while the lines and choices
stay the same, `BimApp::site_ghost`).

Properties (`site_properties`): a site element's Level (read only) and Name, then a **Site** group: Kind (read only), Use
and Phase (drop-downs, by name), and each prop the catalogue gives its kind that is one value (a length in the project's
units, a count, an angle, yes or no, text; a surface a drop-down of its kind's surfaces); blank takes a prop back to the
figures' default. Read only under Dimensions: its area; a car park's stalls, accessible stalls and area per stall; a road's
carriageway and pavements (`roads::section_of`). Each edit is one `site_edit`. A mass has a **Masterplan** group: its use,
phase and the masterplan props (storeys, floor height, roof, pitch…), its height following its storeys. A toposolid's rows
are a floor's and its contour interval; a pad's its level, offset, area, thickness and elevation; a property line's its
name, area and perimeter.

Selected alone with Modify, a site element shows grips (`site_grips`): each point of its path, each control point of its
area, the point a tree, a stall, a pitch or a bike rack stands on. A drag moves the point, snapped, the element drawn as it
would be while it moves, and its release is one `site_edit`. **Edit Path** (a path's) and **Edit Boundary** (an area's, a
pad's, a property line's) in the Mode panel sketch its line again (`Of::EditSite`): Finish sets the path or control ring
(`site_edit`), a pad's boundary or a property line's points (`modify`). A toposolid's Edit Boundary is the core's
`edit_boundary`, as a floor's, its shape points inside kept.

## Commands

Every one checks its keys (a key it doesn't take is refused, naming the ones it does).

- `site {kind, use?, name?, level?, phase?, source?, ref?, props}` (SI): places one; its shape is worked out at once
  (with the lanes it meets), a car park laid out. Refused, saying why, when its shape can't be worked out (a lane with no
  `path`) or a prop isn't the catalogue's or of its type. Returns `id`, `area_m2`, `notes`, `filled` (the props the
  figures filled in) and `element` (its `element_json`).
- `site_edit {id, use?, name?, phase?, source?, ref?, level?, props?}`: changes one; each prop given is set, one given
  `null` is taken back to its default (filled in again from the figures, and returned in `filled`). On a mass, its
  masterplan figures (`site`), its height from its storeys; it returns the mass's `height`, `site` and `element`.
- `mass {…, site?: {use?, phase?, source?, ref?, props?}}`: a building on the masterplan; with `storeys` in its props,
  its height storeys × floor height.
- `site_figures {values?: {name: number | null}}`: the project's own figures (`Model::site_figures`, kept in the file),
  by Arqiflow's names (`lane.width`, `road.width`, `tree.crown`, `parking.bay_width`, `building.floor_height`…, lengths
  in mm); null takes one back to Arqiflow's. Every site element is shaped again with them. Returns every figure as
  `{value, type, own}`.
- `site_kinds {}`: the catalogue above (no undo step).
- `property_line {points: [[x, y] or [x, y, bulge], …], level?, name?}` (PL): closed; given clockwise, it is run round
  the other way (each bulge moved to the piece it now starts and turned). Returns `id`, `area_m2`.
- `toposolid {boundary, points?: [[x, y, z], …], contours?, type?, level?, offset?}` (TS): `points` the ground's
  heights over its level (shape points); `contours` mm apart (1000; 0 none); `type` a floor type (`Toposolid`, 1000 mm,
  made when the project hasn't one). Returns `id`, `area_m2`.
- `edit_boundary {id, sketch?}` on a toposolid: its outline and openings redrawn, as a floor's, its shape points inside kept; `sketch::chain` joins sketch lines end to end into a path's `[x, y, bulge]` points (what the desktop's path tools send).
- `building_pad {boundary, type?, level?, offset?}` (BP): `type` a floor type (`Building Pad`, 300 mm, made when
  missing). Returns `id`, `area_m2` and `cuts` (the toposolids it cuts).
- The Modify tools (move, copy, rotate, mirror, array, delete) move a site element's truth: its points, paths and
  control rings; its directions in the world (a pitch's, a bike rack's and a stall's `angle`, a car park's `direction`,
  a mass's `ridge` and arc); mirrored, an arc's bulge, a parking band's `sides` and its bays' sides and a lane's turning
  heads' sides go to the other side (`masterplan::transform_props`); its shape goes with it until it is worked out again.
  A property line's points (a mirror turns its bulges), a pad's outline and a toposolid (as a floor) go too.

### Keeping the shapes (`masterplan::keep`)

After every command that is an undo step (but `site` and `site_edit`, which do it themselves to answer with it), from what
the command changed (`Model::changed_since`'s ids, or everything when the site's figures, the phases or a level changed):

- each site element that changed is shaped again; with it, the lanes within 30 m of a changed lane (as it is, or was:
  a lane moved away or deleted leaves its old neighbours' junctions to be worked out again), given their own neighbours
  so their corners stay right; a changed ramp shapes the parking bands on its level again (their bays keep clear of
  ramps). Only what changed and its neighbours, level by level;
- a car park is laid out again when it changed, or when a site element or a mass changed within 10 m of it on its level
  (it keeps clear of buildings, cores, water…, and is entered from the lanes along it: `parking::Obstacle`'s kinds);
- the phase and its params are kept in step; a mass with storeys gets its height.

An element is written back only where what was worked out differs from what it holds.

## Rules (`Model::check`)

A site element's level exists; its `kind` is one of the kinds and its `use` one of its kind's (or none); its `phase`
one of the four; its `name` and `source` up to 256 characters, its `ref` up to 4096; each of its props is one the
catalogue gives its kind (else the error lists those it takes) and of its type: a length a finite number within 1000 km
(or a list of them, a pitch's margins), an angle, percent or number finite, a fraction 0 to 1, a count 0 to a million, a
point [x, y] finite within 1000 km, points and paths up to 100,000 of them (a path's bulge finite), text up to 4096
characters, yes or no; objects and lists by what they hold. Its shapes' points are finite, no more than a million in
all; only a car park has a layout; up to 100 notes. A mass's `site`: its use one of housing, office, retail, mixed,
community (or none), its phase one of the four, its props the catalogue's `mass` props. A property line: 3 to 10,000
points, finite, enclosing an area. A building pad: a floor type the project has, a simple outline, a finite offset. A
toposolid: a floor's rules.

## Drawing

**Plans** (`render/site.rs`): a site element draws in its level's plan, over the toposolids and under the building's
floors, in layers (ground: lawns, paving, water; hedges; lanes; roads; parking; trees). Each area is filled in its
surface's colour and outlined, Arqiflow's palette (palette.ts, draw-site.ts): lawn `#d6ebc6`, meadow `#e4eec4`,
planting `#c4dfae`, paving `#ebe6dd`, terrace `#ece4d6`, water and streams `#cfe6f3`, hedges `#b9d8a6`, pitches
`#cfe8c0` (their run-off paler and their markings white), playgrounds `#f6e3c4`, lanes `#ece6da`, ramps `#e7e2d8`
(their covered part dashed). A road draws its pavements `#e6e1d7`, carriageway `#a4aab2`, planted median, painted
lines, kerbs, the backs of its pavements, lane markings and centre lines (dashed) and double centre lines; a tree its
crown and a dot where it stands; a parking band its aisles and bays (accessible ones blue); a parking stall its bay; a
car park its area, its aisles, its stalls (accessible blue, EV green) and its columns. Existing elements draw halfway to
the paper (context halftone); demolished and planned ones faded and dashed, demolished crossed through. A property line
draws dash-dot; a building pad its outline over a pale fill; a toposolid its fill, outline and contours (as a floor that
draws contours), cut by its pads. Each primitive is its element's, so `Drawing::pick` picks it (inside an area, the area);
view overrides and phase filters dress them as any element's.

**3D** (`masterplan::solids`, `view3d`): each area a thin slab over its level (lawns 20 mm, paving and pitches 40, lanes
50, a road's carriageway 30 and its pavements and median 150, water at its `level` under the ground), in its surface's
colour (Arqiflow's view3d-look.ts); a tree a trunk to a third of its height and its crown up to its height; a hedge its
height; a car park its aisles, stalls and columns on their levels; a toposolid its terrain (as its pads cut it), a pad
a slab.

**IFC**: a toposolid is an `IfcGeographicElement` `.TERRAIN.` (its cut shape); a building pad an `IfcSlab` `.BASESLAB.`;
landscape, trees, hedges, water and streams `IfcGeographicElement` `.USERDEFINED.` and the rest of the site (lanes,
paving, parking, pitches, play) `IfcCivilElement`, each with its kind and use as its ObjectType (`lane: road`) and its 3D
slabs swept up; a property line an `IfcVirtualElement` of ObjectType `PropertyLine` with its closed line as an Axis
curve. They are written, not read back: an IFC import brings them in as generic components or leaves them out.

**Schedules**: `site` (Id, Kind, Use, Name, Level, Area, Phase, Stalls, Source…), `propertyline`, `toposolid` and
`buildingpad` schedules; a multi-category schedule of what's built lists site elements, toposolids and pads.

**Visibility/Graphics**: `toposolid`, `buildingpad`, `propertyline` and `site` are model categories views hide and
override.

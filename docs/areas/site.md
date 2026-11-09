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

## The elements

- **`site`** (`Element::Site`): one element of the masterplan. It holds
  - `kind` and `use`: what it is (below) and its use within the kind;
  - `name`, `level` (GUID), `phase` (Arqiflow's: `new`, `existing`, `demolished`, `planned`), `source` and `ref` (where
    context came from: `site` for the plot's own streets, `osm`, `overture`, …, and its id there);
  - `props`: **what a person drew, its truth** (a lane's `path` and `width`, a pond's `control` points, a pitch's `at`,
    `angle` and size), lengths and points in **millimetres**;
  - `outline` and `parts`: **its shape, worked out from the truth** (`site::shapes::normalise`) after every command that
    changes a site element, and kept in the file, so a reader never works it out again (Arqiflow's server reads them);
  - a car park's `layout` (`site::parking::lay_out`): its aisles, stalls, columns and ramps, and its figures;
  - `params`, as every element.
- **`propertyline`** (`Element::PropertyLine`): Revit's Property Line, the plot's boundary: points with bulges, closed,
  its area.
- **`toposolid`** (`Element::Toposolid`): Revit's Toposolid, the ground: a floor's shape (outline, shape points at
  their heights, contours) in a category of its own.
- **`mass`**: a building on the masterplan is a mass (BIMCraft's), with the masterplan's figures for it (use, storeys,
  floor height, attic, roof and pitch, label, entrance) beside its outline and height.

### In a file and in `element_json`

A site element, as the file and the live room hold it (the element's serde, tagged by `category`; references are
GUIDs):

```json
{"category": "site", "kind": "lane", "use": "road", "name": "Street", "level": "<level GUID>",
 "phase": "existing", "source": "site", "ref": "street:edge 0",
 "props": {"path": [[-10000, -4000], [79950, -4000]], "width": 8000, "lanes": 2, "lane_width": 3000, "kerb": 150},
 "outline": [{"outer": [[0, 0], [1000, 0], [1000, 1000]], "holes": []}],
 "parts": [{"role": "carriageway", "polys": [], "lines": []}],
 "layout": {"pieces": [{"kind": "stall", "level": "Level 1", "props": {}, "outline": []}], "result": {}}}
```

`layout` only on a car park; `parts`, `source`, `ref` and `name` left out when empty; `phase` left out when `new`.
A mass on the masterplan carries the same under `site`: `{"category": "mass", …, "site": {"use": "housing",
"phase": "existing", "source": "overture", "ref": "building:overture:17", "props": {"storeys": 2,
"floor_height": 3000, "roof": "flat", "label": "B6", …}}}`, its outline its `boundary` and its height worked out from
its storeys and floor height when it has them.

### The kinds

| kind | Revit's | uses (the first is the default) |
|---|---|---|
| `lane` | (roads: Civil's) | lane, footpath, road, promenade |
| `parking_band` | Parking Component | parking |
| `car_park` | Parking Component, laid out | parking |
| `paving` | Toposolid subdivision | forecourt, apron, square, court |
| `terrace` | Toposolid subdivision | terrace |
| `landscape` | Toposolid subdivision | lawn, meadow, planting, garden, woodland, orchard, bank |
| `tree` | Planting | tree |
| `hedge` | Planting | hedge |
| `water` | Site Component | pond, lake, wetland, basin |
| `stream` | Site Component | stream, rill, ditch |
| `pitch` | Site Component | five_a_side, futsal, football_7, football_11, basketball, tennis, padel, multi_sport |
| `playground`, `play_area`, `bike_rack` | Site Component | |
| `ramp` | (a site ramp) | cars |

What each kind's props are, which are lengths and which are points, and the figures they default from: the
`site_kinds` command answers it (`site::shapes::catalog`), so Arqiflow converts between its metres and these
millimetres from the engine's own list.

## Commands

- `site {kind, use?, name?, level?, phase?, source?, ref?, props}`: places one; its shape is worked out at once.
- `site_edit {id, use?, name?, phase?, props}`: changes one; a prop given `null` is taken back to its default.
- The Modify tools (move, copy, rotate, mirror, delete, …) move a site element's truth and work its shape out again.
- `site_figures {values}`: the project's own figures (a firm's lane width, a tree's crown…), by Arqiflow's names.
- `site_kinds`: the catalogue above.
- `property_line {points, name?}`, `toposolid {boundary, points?, contours?, …}`, and `mass` with the masterplan's
  figures.

## Drawing

Plans fill each area in its surface's colour (lawns green, roads grey, water blue), roads draw their carriageway,
pavements, median, kerbs and markings, trees their crowns; existing context draws halftone, demolished dashed. In 3D
each area is a thin slab over the ground, a tree a trunk and a crown, a car park its levels' slabs and columns.

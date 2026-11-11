# Review Warnings

Revit lets a person do what is questionable and lists it in Review Warnings; it refuses only what it cannot build. BIMCraft is
the same: `Model::check` (`model.rs`) refuses what would break the model, and a design judgment is a **warning**
(`crates/core/src/warnings.rs`), kept for as long as it stands, told to whoever made the change, and listed on request.

## What is refused and what is warned

Every rule of `Model::check` and `check_changed`, sorted.

**Integrity: still a refusal.** A reference to nothing (a level, a host wall, a type, a family, a view, a material, a name); a
reference by id not resolved to a GUID; a number that is not finite or is out of range (`coord`, `size`, `point`, `line`);
sizes over their caps (points, text, layers, balusters, risers, presets, scans, pictures, roof points); a shape the geometry
cannot build or that would corrupt rendering or IFC (a wall whose top is not above its base, an arc past 360 degrees, a
sketch outline that crosses itself, a column, shaft, mass, ramp or room with no height, a loft whose rings do not pair, a
family whose forms do not build, a type whose layers do not add to its thickness); duplicate names that views, sheets,
grids, levels and links are found by; a door or window with no wall, or of a type the project does not have.

**Design judgment: now a warning.** These were refusals (the first three) or were unlisted (the rest).

| Rule | What it says | Was |
|---|---|---|
| `openings_overlap` | doors or windows that overlap in plan and in height in one wall | refused |
| `opening_past_wall` | a door or window wider than its wall's run, or reaching past its start or end | refused |
| `opening_outside_height` | a sill under the wall's base, or at or over its top (nothing of it in the wall) | refused |
| `walls_overlap` | straight walls side by side whose bodies overlap along their length, and share a height | silent |
| `identical_instances` | walls, columns, structural columns, footings or components that are the same in the same place (a group is one warning) | silent |
| `wall_off_axis` | a straight wall or a grid more than 0.0001 and up to 1 degree off an axis (a note) | silent |
| `room_not_enclosed` | a room the walls and room separation lines do not close round | `inspect` |
| `room_redundant` | a room in the space an earlier room has | `inspect` |
| `device_no_connector` | a family of lighting, outlets or switches in use with no connector, counted as a load of zero | `inspect` |

A head that rises past the wall's top is no warning: Revit lets it, and the wall is notched.

## How it works

- `Model::review()` works out every warning, the worst first. `Model::review_since(before, prev)` brings the warnings of a model
  up to date from those of the model it was made of: the elements that changed, and what points at them, are looked at again;
  rooms again when a wall, column, separator or room changed; a changed level, roof, floor or any setting is a full review.
  The tests of every command assert it equals `review()` (`Session::report_warnings`).
- `Session::warnings()` is the warnings of the model now, kept with it. `Session::execute` puts the ones a command made, those
  that were not among the warnings before it, in its result as `new_warnings` (when the result is an object). Each is
  `{rule, severity: warning|note, message, elements: [ids], guids: [..]}`.
- The `warnings` command (`{rule?, severity?, element?}`) lists them all. `inspect` still lists their messages.
- `Model::warnings()` (the messages) is what `inspect` and a dry run return.
- Downstream copes with the allowed states: `walls::prisms` keeps a wall between its own base and top however an opening's edges
  fall; the plan clips an opening's gap to the wall; IFC wall quantities clip each opening to the wall's height.

## The window

The status bar shows a triangle and the count when there are warnings; a click opens Review Warnings, which Manage > Inquiry opens
too. A row picked and Show selects the elements and opens their plan zoomed to them. Properties has a
Warnings group for the selected elements (`crates/ui-egui/src/warnings.rs`).

## Adding a rule

A rule is a function in `warnings.rs` that takes the scope (every element, or those a change reached) and pushes `Warning`s about
the elements in it. Give it an id in `RULES` (the `warnings` command documents them), a test that it is allowed, warned of and
in the command's result, and, if it is about a pair, emit each pair once when either is in scope. If it can only be answered
against the whole project, add its kind of element to the list `review_since` reviews in full.

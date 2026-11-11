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
| `walls_overlap` | straight walls side by side whose bodies overlap along their length, and share a height (not walls that never stand together: options of one set; phases are ignored, as Revit's are, so a demolished wall and its replacement in one place still warn) | silent |
| `identical_instances` | walls, columns, structural columns, footings or components that are the same in the same place (a group is one warning). Elements in different options of one design option set are no twins; a main-model element and an option's are, as are options of two sets; so a group of one in option A, one in option B and one in the main model is two warnings (A with the main one, B with the main one), one for each maximal set that can stand together. Phases are ignored, as Revit's are | silent |
| `wall_off_axis` | a straight wall or a grid more than 0.0001 and up to 1 degree off an axis (a note) | silent |
| `room_not_enclosed` | a room the walls and room separation lines do not close round | `inspect` |
| `room_redundant` | a room in the space an earlier room has | `inspect` |
| `device_no_connector` | a family of lighting, outlets or switches in use with no connector, counted as a load of zero | `inspect` |
| `duplicate_type_mark` | types of one category (a door family's and the project's door types count as one) that share a non-empty Type Mark; about the types, by their library GUIDs, so `elements` is empty and Show has nothing to select | silent |

A head that rises past the wall's top is no warning: Revit lets it, and the wall is notched.

## How it works

- `Model::review()` works out every warning, the worst first. `Model::reviewed_since(before, prev)` brings the warnings of a
  model up to date from those of the model it was made of (`prev`, a `Review`: the warnings and what the rooms reach): the
  elements that changed, and what points at them, are looked at again; a changed level, roof, floor or any setting is a full
  review. `review_since(before, &[Warning])` is the same without the kept room reaches. The tests of every command assert the
  answer equals `review()` (`Session::report_warnings`, `Review::assert_whole`).
- What a change costs does not grow with the model. A change to a kind no rule looks at (a tag, a dimension, a view, a sheet, a
  note, a schedule) returns the warnings as they were. Twins are found among the elements near the changed ones (a plan tile and
  a height for a wall, a group and a height for a component, the few columns of a project), not among all. Device families are
  looked through only when a component changed, and only the families of those that did. Rooms: each room's boundary is kept
  with its warnings (`Reach`), and only the rooms a change reached are worked out again: those that changed themselves, and
  those whose boundary meets where a wall, column or separation line stood or stands (in the model before the change and the
  one after, grown by four wall thicknesses) at a height that wall or column spans. A room that is not enclosed, which has no
  boundary to meet, is worked out again by any change on its level.
- `Session::warnings()` is the warnings of the model now, kept with it. Kept on another model (after an undo, a redo, a change
  another copy merged, a transaction taken back), they are brought up to date from that model's, never worked out whole again.
  `Session::execute` puts the ones a command made, those that were not among the warnings before it, in its result as
  `new_warnings` (when the result is an object). Each is `{rule, severity: warning|note, message, elements: [ids], guids: [..]}`.
- A command made inside another (a script's lines, a transaction's, a plugin's) works out no warnings of its own: the
  outermost says what the whole made, once. A script's are in its last result that is an object (`new_warnings`); so are a
  plugin command's, in its own result; a transaction's come back from `Session::transaction_reporting` as a list, each as a
  command's result gives them (the window's multi-command actions, `run_all`, add them to their last result; `transaction` is
  the same without them). A dry run's are its `new_warnings`, structured as a command's, said once at its top level: the
  lines say none, and the warnings the session keeps stay those of the project, not of the model the dry run tried. A panic
  while the warnings are worked out refuses the command (an error naming it, the model as it was), as one in the command
  does, and leaves nothing under way: `Session::nested` is raised through a guard that lowers it however the work ends.
- Tests hold the warnings of every command made inside another to the whole model's too (`Session::shadow_review`, a
  test-only copy of the warnings kept one command at a time), so a script line, a transaction's command or a plugin's is as
  checked as a command of its own.
- The `warnings` command (`{rule?, severity?, element?}`) lists them all. `inspect` still lists their messages.
- `Model::warnings()` (the messages) is what `inspect` returns.
- Downstream copes with the allowed states: `walls::prisms` keeps a wall between its own base and top however an opening's edges
  fall (openings that overlap or sit inside one another cut once); the plan clips an opening's gap to the wall; wall quantities
  (`quantities::wall`) are those of that solid, so an opening past the wall's end or under its base cuts only what is in it.

## The window

The status bar shows a triangle and the count when there are warnings; a click opens Review Warnings, which Manage > Inquiry opens
too. A row picked and Show selects the elements and opens their plan zoomed to them. Properties has a
Warnings group for the selected elements (`crates/ui-egui/src/warnings.rs`).

## Adding a rule

A rule is a function in `warnings.rs` that takes the scope (every element, or those a change reached) and pushes `Warning`s about
the elements in it. Give it an id in `RULES` (the `warnings` command documents them), a test that it is allowed, warned of and
in the command's result, and, if it is about a pair, emit each pair once when either is in scope. If it can only be answered
against the whole project, add its kind of element to the list `review_since` reviews in full.

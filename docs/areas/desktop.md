# The desktop app: browser, docking, settings, picking, snapping, clicks, startup, tags

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

The Project Browser (`browser.rs`) builds its tree as `Node`s and draws it through its tab (`browser::Tab`) and its search
(`browser::spans`, wildcards and all, a pure function with its tests): a new kind of entry is a `Node` in `browser_tree`.
Palettes dock and float (`dock.rs`): `Docking` is where they are (stacks of tabs down the left or right side or along the
bottom, or floating windows), apart from its drawing, so tests and the settings file read it; a new palette is a row of
`Palette` and its body in `palette_body`. A view's tab floats it in a window (`BimApp::floating`; `views` are the tabs and
`front` the one the views' area shows): the active view (`BimApp::sheet`) takes the tools, any other on screen draws through
`view_pane` as it is, with the canvas `at_rest` and `passive` set so it takes no clicks, and a click makes it active. Widget
ids inside a view are per view (`push_id`), so two views on screen never clash. The start page is `start.rs` (`BimApp::home`; the desktop's `main` shows it when given no file, the quick access bar's Home and Ctrl+D bring it back over a project and go back to it, `BimApp::toggle_home`, and `Home::over` says there is one behind it): its tiles' pictures are drawn on a thread (`start::picture`, a `.bimcraft` file's 3D view) and a test waits for them with `Home::settled`. Its Sample Project button (`Pending::Sample`, `BimApp::open_sample`) builds the sample house, `bimcraft_core::sample::SAMPLE`, as a project with no file, so Save asks where to put it; it goes through `guard` like New and Open, and the script's steps are not undo steps, as with `--sample`. File > Settings is `settings.rs`, the user's
`settings.json` beside the favorites (`BimApp::use_settings`, which the desktop's `main` calls; tests never write it unless
they hand it a file): what it keeps is `Settings`, written when it changes, between drags; a setting still to come goes on
its page greyed out (`later`).

Two working aids are drawn over a plan's or a ceiling plan's canvas, in `canvas()` right after the drawing: the origin marker
(`origin_marker`: an arrow east for x in the canvas's red and one north for y in its green, lettered, at the screen point of the
project's 0, 0, `ORIGIN_ARM` points long at any zoom, left out when the origin is far off the screen) and the view's name in the
bottom left corner (`BimApp::corner_name`, over the view control bar where that floats over the corner, `bar_cover`; a ceiling plan's
reads `Level 1 (Ceiling Plan)`, as an image's does). They are the window's alone: `render::draw` has neither, so no PNG, sheet or
printout does. File > Settings > User Interface > Plans has a box for each (`BimApp::show_origin`, `show_view_name`; kept in
`settings.json` as `origin_marker` and `view_name`, on when the file doesn't say), and New and Open keep them. The 3D view, sections
and sheets have neither. Tests: `tests/origin.rs`.

The minimap (`minimap.rs`) is at the top of the Project Browser, under a header that folds
it away: folded at first, so the tree keeps its room (the tests that click the tree's entries where they are drawn rely on it), and
egui keeps the fold for the session. Unfolded, it is as wide as the palette and no taller than a third of what is left of it. For the
active plan, ceiling plan, section or elevation it draws the whole view small (`render::plan`, `render::ceiling_plan`, or a section's
picture and `render::section_over`, fitted into `SIZE`), drawn again only for another model, view or canvas theme, never for a pan or
a zoom; the 3D view, sheets and schedules have none. The part the view shows is outlined on it, from the canvas's camera
(`Canvas::view`). A click centres the view there and a drag pans it as it goes, both through `Canvas::centre`, which keeps the zoom: a
drag begun on the outline takes the view with it, one begun beside it moves the view there first. `tests/minimap.rs` clicks and drags
it (`Minimap::screen` is where a model point is on it).

View > Dark Mode opens a small theme menu: As the System, Light, Dark, or Dark UI, Light Canvas. The same choices are in
Settings > User Interface. The last choice keeps plans, ceiling plans, sections, elevations and 3D views on a light background
while the interface stays dark; sheets stay white paper in every mode. `Settings::light_canvas` is saved alongside `theme`
in `settings.json`, defaults to false for older files, and is only read with the dark theme (`Settings::from_json` drops it from
any other, so a hand-edited file never leaves the menu with no choice picked). `style::dark` chooses interface colours;
`style::canvas_dark` controls canvas ink, fill and background conversion, and the section/3D image cache keys. Both software
and GPU 3D painters use the canvas background. What the drawing paints in a palette (the fill of what is selected or hovered,
a crop region's outline) takes it from `style::canvas_pal`, which follows the canvas, not `pal`, which follows the interface:
`DARK`'s fills are near-black on a white drawing. A light canvas has a palette of its own (`CANVAS_LIGHT`: a deeper hover stroke and
selection tint than the interface's `LIGHT`, which keeps the tints it always had, so the plain Light window's chrome is unchanged). A scan is dimmed only on a dark canvas (`canvas_dark`), so a dark interface
around a light drawing leaves it as it is.

New and Open (`BimApp::reopened`) keep the look: a new `BimApp` would run `style::apply` again, which sets the theme
preference back to the system's (and `keep_settings` would then write that over the user's choice), so `reopened` carries
`styled`, `theme_wanted` and `light_canvas` over. A hosting page (`host.rs`, Arqiflow's editor) dresses the window with
`set_host_look`: it changes light or dark and the colours it sends (none, BIMCraft's own), never the person's Light Canvas, and
the layout (Compact) only when the page asks for a different one than it last did, so a page that sends its look with every
theme change wipes no choice the person made since (`tests/host.rs`).

A hosting page also reads what the person sees and can point at things for its agent (`hostview.rs`, `tests/host.rs`): `host_context` is the
selection (id, GUID, category, name), the active view (name, kind, a plan's level, a sheet's number) and the camera (a plan's, a
section's or an elevation's visible rectangle in the view's own millimetres, the 3D view's azimuth, elevation and target);
`host_context_key` is the cheap stand-in the web app (`apps/bimcraft-web`) compares each frame to tell the page, a quarter second after
the last change, that it moved (`on_context`). `host_select` and `host_open_view` move the window only, as the person would (Select by
ID with its zoom, the Project Browser's open): no command runs and the model does not change. `host::editor_takes_key` says which keys
the editor takes while its canvas has the keyboard, and the web app keeps the browser from acting on them (Tab, Space, Backspace, Delete, arrows, Home/End/Page Up/Down, Ctrl+A,
D, K, O, P, S, Y, Z, F1 to F4 and F6 to F10; never F5, F11, F12, or Ctrl+C, V, X, whose browser events carry the clipboard).

Application dialogs use `popup.rs`: a shared modal frame with a draggable title bar and a corner × button. Each dialog keeps its own
position while the application is open; dragging is constrained to the application window. The × button follows Cancel/Close, so it never
applies a draft; so does Escape. A click beside a dialog, on the dimmed window behind it, does nothing: only its buttons, its × and Escape
close it. Nested dialogs keep their own positions and close independently. The dialog body scrolls separately from its footer;
OK, Cancel, Apply and other closing actions stay at the bottom, including on shorter windows and when tabs have different amounts of content.
Imports and installations keep their close button disabled while they replace the project or application. Native file dialogs keep their system frames.

The system's file dialogs (Open, Save As, the exports, Load Family, Link Model, a material's picture) are asked for through
`BimApp::ask_file` (`dialog.rs`), never with a blocking `rfd::FileDialog`: a native dialog run inside the window's event
handler nests a run loop in it, and on macOS a drag reaching the window meanwhile re-enters the handler and aborts the app
(issue 43). The dialog (`rfd::AsyncFileDialog`) is begun on the window's thread, where AppKit wants it and where a sheet does not
block, and waited for on a thread of its own; `ask_file` is given what to do with the path, and `BimApp::dialog_answered` does it in a later frame (a cancel does nothing; one dialog is open at a time). A step
that goes on after a save (the unsaved-changes prompt's Save) is that `then`. A file dropped on the window is queued in
`opens`, as one the system asked for. A test hands the app a `Dialog` whose answer it sends itself (`tests/dialog.rs`).

The window's size (`window.rs`, issue 37) is `Settings::window` (its size in points and whether it was maximized, never its place:
Wayland tells an app none, and the window starts centred): `BimApp::watch_window` notes it once a frame, keeps it once it has held
still for half a second, and writes nothing for a window left as it started; `main` starts the window at the size the file holds
(`settings::window_in`). A window too big for its screen (the default 1440 x 900 on a laptop's, or a kept size after the screen
changed) is made smaller, once, in the first frame that knows the screen (`fitted`: 96% by 90% of it, never below the least a
window can be), and centred when it has taken the size.

The control port (`control.rs`, issue 41) is how an agent works on the project the window has open: `bimcraft --control PORT` binds
only 127.0.0.1, makes a token of 128 random bits and writes both to `bridge.json` beside the favorites (for the user alone; the
window takes it away when it closes), and `bimcraft-cli mcp --connect` runs its tools on the window's session. The protocol, and
what an agent may not do (`bridge::REFUSED`: `new`, `open`, `recover`, `highrise`, by any name, and a script is read through before it
runs), are `bimcraft_core::bridge`. A connection thread reads a line (at most 16 MB), checks the token, and hands the request to the
window through a channel; `BimApp::answer_agents` runs it in the frame as `BimApp::run` runs a button's command (`made_in`, then
`Session::execute`) and sends the reply back, up to sixteen a frame, and not while a file is being read or an update installed. A
request wakes the window (`Context::request_repaint`, from the context the first frame left in a `OnceLock`). Eight connections at
most; ten seconds, in all, to say the first line whole (a byte now and then does not count), and what is not a request, a web page's
HTTP say, is answered with an error and closed. A test (`tests/control.rs`) plays the agent on a real connection while the harness runs frames.

File > Settings > Agents opens the same port without `--control`: Let agents work on this project, and its Port (0 for any free one),
both kept in `settings.json` (`agents`), so the window opens it again at its next start once `--control` has had its say
(`listen_as_kept`), with the same bridge file and the same rules. Unticked, the port is closed: a `Control` dropped stops listening
(its accept thread is woken by a connection of its own and lets the port go) and hangs up on its agents, so it can be opened again.
Allow agents to edit this project (on by default, kept too) is `bridge::Access`: off, `answer_agents` runs each request read-only
(`Session::reading`), so an edit, an undo or a redo, a script holding one, or a save over the project's own file comes back as an
error that names the box, and what reads (inspect, schedules, render, exports to new files, the sheets to PDF) still answers; the
user's own commands are never read-only. An agent may ask to only read as well (`"read_only": true` in a request, which
`mcp --connect --read-only` sends). The view control bar ends with the port's state (`agents_indicator`): Agents:
off, Agents: port N, or Agents: N connected (a connection counts once it has said a request with the token), with read-only when edits
are off; a click opens Settings at Agents.
A script request may ask for a dry run (`"dry_run": true`, `Session::script_dry_run`): the window runs it all or nothing, takes it
back and replies with what it would make, change and remove and the warnings the model would have, so the user's project and
undo history are as they were. It only reads, so it answers where agents may only read too, and a line that names a file or
would save, undo, redo or replace the project is refused, as is a `favorite` other than `list`: saving or deleting one writes the
user's favorites file, outside the project, even with no `path` given.

The interface's language is `lang::Lang`: a project's language, or Simplified Chinese (issue 38), which only the window speaks
(a drawing's lettering, `font.rs`, is drawn in code and has no hanzi, so a project is never in Chinese and `Lang::project()` gives
it English). Its words, including the schedule headings shown in the window, are in `translations/ui/*.po`;
missing or unfinished translations show English. Project text has separate catalogs; see [translations](translations.md).
egui's own fonts have no hanzi, so the window in Chinese needs a font the system has: `lang::use_chinese_font` puts one behind egui's
(`lang/font.rs` tries each system's known places, then the Linux font folders, taking Noto Sans CJK's Simplified face) the first frame the
language is Chinese. Where there is none, `set_lang` leaves the language as it was and the status says why, and a settings file that says
Chinese on such a machine starts the window in English. The app holds the finder (`font_finder`) so a test can say there is none.

What a view hides or reveals for now (Temporary Hide/Isolate, Reveal Hidden Elements; `footer.rs`) is the window's, per `Sheet`, never the
project's: the plan, the section and the 3D view draw `BimApp::view_model`, a copy of the model whose view graphics went through the `graphics`
command on a scratch `Session`, made once for each model and each setting. A tool that needs the view's real model (select all, commands)
still reads `session.model`. The Door and Window tools preview with `opening_of`, which the `door` and `window` commands insert too, so a
sketch is the element the click makes (`render::sketch`). A selected door or window shows two temporary dimensions from its middle
to the faces of the walls meeting its wall either side of it, or to its wall's ends where none does (`Model::opening_span`), as Revit's
measure to wall faces; typing one counts from that face. With the project's Temporary Dimension Properties set to Openings for Doors and
Windows (`tempdims::opening_reach`, half the opening's width) they run from those faces to the jambs instead, and a typed one counts to the jamb. Placed or dragged alone within `SNAP` px of the middle between those faces, a
door or window lands on it (`Model::centred`): the click sends that `offset`, the drag slides by what reaches it.
A selected column or component, and the Column and Component tools' sketches, show up to four: how far it stands from the nearest wall centre
line or grid east, north, west and south (`Model::clearances`, which finds them in the core; `standing_dimensions` draws them); a typed one runs
`move` by the difference along that way.
A selected straight wall shows, besides its length, the distance square to it, on each side, to the nearest parallel wall or grid that
shares some of its run in the plan on screen, measured between the lines the project's Temporary Dimension Properties name (Manage > Additional Settings > Temporary Dimensions,
`tempdimprops.rs`, the `temporary_dimension_settings` command, kept in the file as `temporary_dimensions`: Walls are Centerlines, Faces, Center of Core or Faces of Core, as Revit's, and a wall with no core of its own measures its centre line or faces in their place; the core runs from a wall type's first structure layer to its last, and a wall has one of its own only when it does not fill all the thickness: `walls::core_faces`; the lines are `Face::CoreCentre`, `CoreLeft` and `CoreRight`, which a dimension may measure to too, new in file format 19), and only what that plan shows (its view range, phase, design option and hidden categories: `tempdims::wall` in the core, read by the agent through the `temporary_dimensions` command, whose `dimension` params carry the plan's `level`;
`canvas/wall_dims.rs` draws them). A typed one runs `move` on the wall by the difference toward that line. The dot where the dimension
starts on the wall is Revit's witness-line grip: a click cycles the line it measures from, the centre line, the exterior face (`Face::Left`),
the interior face (`Face::Right`) and, for a wall with a core, the core's centre and its two faces; the dot at the other end does the same
through the lines of the wall it ends at. A drag from either dot (`Sense::click_and_drag`), or Move Witness Line in the menu of a right click on
it and then a click (`Tool::MoveWitness`, `pick_witness`), puts the witness line on the reference nearest the cursor: at the wall, one of its own
lines (`tempdims::own_lines`); at the other end, a line of a parallel wall or grid the plan shows on that side (`tempdims::neighbour_lines`). A choice is
the wall's `Witness` (`BimApp::witnesses`), kept for the session as Revit keeps it, and beats the project's settings for that side and end only; the
core reads the same through `temporary_dimensions` (`left`, `right`, `to_left`, `to_right`). The icon beside the number runs `dimension` between
the same two lines, at the middle of the stretch they share.

Align (AL) highlights the exact edge or axis under the cursor before either pick, leaving the selected reference as an
extended dashed line. It searches the candidates under the pointer for a usable line rather than letting annotation mask
one. `align::lines` in the core supplies wall faces and ends, opening jambs, grids, column faces and axes, structural and
MEP runs, components, slab and roof boundaries, stairs, ramps, railings and detail geometry. Straight segments of a
polyline or sketch are usable; curved edges are not replaced with straight chords. The `align` command turns the target
by the smaller angle about the picked line's midpoint and translates it onto the reference, as one undo step through the
usual transform and validation path. Hosted doors/windows can slide along their wall, and hosted railings follow their
stair or ramp. A refusal keeps the reference selected. On the second pick a floor's or region's model pattern remains
alignable when its line is closer than the boundary; picking the boundary aligns the element instead.

While a sketch is active, Align takes its reference from either model geometry or another sketch line, with a coincident
draft line taking priority over the saved boundary. Its second pick edits a sketch line. The same exact segment highlights
on hover before either pick, including straight segments of polylines. `align::sketch_lines` applies the rigid alignment to
the draft curve, keeps its id, GUID and style, and moves joined endpoints so a closed boundary stays connected. A sketch
reference stays fixed. Changes use the sketch's Ctrl+Z history and reach the model only through Finish Edit Mode; Cancel
leaves the model as it was. Invalid geometry leaves the draft and reference intact, and undo or Escape drops stale picks.

A sketch line picked alone in Modify has a small palette over the view at its middle (`BimApp::edge_palette`): Add
Vertex (which no panel offers while sketching), Split Element, Offset and Trim/Extend to Corner, each starting its tool. The sketch's contextual ribbon includes Align, Trim/Extend to Corner, Trim/Extend Single/Multiple Elements, Split and
Offset. Trim highlights sketch candidates and keeps its first pick lit; its prompt says which line to pick and to click
the part to keep. A failed trim leaves that first pick available for another target; a successful edit clears the error.
Multiple Trim keeps the boundary across successive target edits while that indexed line is unchanged, and drops it if
another edit removes, renumbers or changes it. Escape clears the first pick; Ctrl+Z takes back each trim as one draft step.

A point picked in a plan lands where `snap::snap` says, the core's so every front end agrees: on the end, middle or centre of
what's drawn (`Scene::of`; a column, structural column or footing gives its centre, the ends of its outline's corners and the middles of its edges from `Model::column_outline`, a round one's four quadrants), a round detail line's quadrants (`rounds`: a circle's, an ellipse's or an arc's points along its axes),
where two straight lines (walls, grids; not arcs) or a detail line's chords cross (never a line's chords with each other), where a
line from the last point touches a circle or an arc (`tangents`), else a scan's point where the cut plane cuts it
([point clouds](point-clouds.md)), the foot of the perpendicular from the last point
to one, a guide through that point at
a multiple of `Snaps::angle` from east or from the line before it (where the guide crosses a wall or grid it lands there, and a
grid point lying on it beats a rounded length), a wall's centre line or the nearest point of a detail line, else the nearest 10 mm.
Annotation adds its anchors (`Kind::Anchor`, switched by `Snaps::annotation`): a note's insertion point and leader, a tag's head
and, with a leader, its host's anchor, a dimension's line and stations, a spot elevation, section and elevation marks, detail
groups and repeating details. A camera's mark is not a symbol to snap to: its body, lens and name all land on its eye, its target
is an endpoint, and its sight line gives no nearest point. Rotate's Place Centre runs its preview through `found` and
`show_snap`, so its cross and marker stand on the point a click takes.
Revit's Quadrants (SQ) and Tangents (ST) are snaps of their own, in Manage > Snaps with the rest. The desktop's `BimApp::snap` gives
it the reach in px (`SNAP`, `PICK`) and the snaps in force (Manage > Snaps, kept while the window is open, or for one point
a shortcut's override: `snap_once`, `Snaps::only`, `Snaps::none`); `found` and
`show_snap` draw what a pick would land on (a line's or a wall's length is typed at the cursor, and after Tab its angle, degrees counter-clockwise from east, `line_angle_field`;
Enter places the point along it; a point lined up with a wall says which: "Lines up with wall 100231", the core's
`AlignmentResult::wall_id`), and a point that moves with the cursor goes through them. A selected wall's ends are
grips (`grip_drag`): a drag runs `move` with `end`, Ctrl keeps the length; the angle beside each end runs `rotate` about it. A
wall joined at the end that moves keeps its joint, which slides along the wall it meets, so lengths change. At a corner where
a strip footing ends with it and another element also ends there, the footing's end goes with the wall's, as another wall's would.
A selected grid's ends are grips too (`move` with `end`).

A drag of the selection (`Canvas::drag`, `Drag::Move`) snaps as the Move tool does: `BimApp::drag_base` snaps the point pressed (the
way a base point snaps), `BimApp::dragged` snaps the cursor from it with `snap_skipping` (what is dragged attracts nothing; with Ctrl it is
a copy and does), Shift keeps it square, and a point with nothing to snap to is rounded to 10 mm. On release `drop_selection` runs `move`,
or `copy` with Ctrl (the copy is selected). Pin (PN) and Unpin (UP) are commands (`pin`, `unpin`; the model keeps `pinned` by GUID, `pin.rs` here
draws the pin on a selected pinned element, whose click unpins): a drag, a grip (refused at the press, with no preview), Move, Rotate or Mirror without Copy, a typed temporary dimension or angle, a length typed into a permanent dimension (it moves what the segment ends at, `Model::dimension_moves`), Align, Offset without Copy,
an arrow-key nudge and a drag on a sheet of a pinned element run nothing and the status bar says, in Revit's words, "Can't move pinned element." ("Can't rotate pinned element." for a turn; `BimApp::refuse_pinned`, `refuse_pinned_ids`, `refuse_pinned_turning`). The agent acts for the person, so the core refuses the same, with the same words and the `unpin` command named, and changes nothing: `move` (a grid's end too), `rotate`, `mirror`, `align`, `modify` of where that kind of element is (its ends, its `at` and `angle`, a re-sketched `boundary` or `sketch`, a door's `host` or `offset`; never a height or a vertical offset, which a pinned element keeps taking), `offset` without `copy`, `corner`, `trim_extend`, `split`, `fillet`, `dimension_value` when it would move a pinned element (`pin::refuse_pinned`, called by `relocate` and the others). Copies (`copy`, `array`, `paste`, and `rotate` or `mirror` with `copy`) of a pinned element are free, as in Revit. Delete is Revit's too: `delete` refuses a pinned element ("Pinned objects were not deleted. To delete them, unpin them before using delete."), the window (`BimApp::delete_selection`) deletes what is not pinned, warns the same for the rest (a warning, as Revit's PinnedObjectNotDeleted, under the warning sign in the status box: `BimApp::warn`, `warned`, until the next command works) and leaves nothing selected (the pinned ones neither, as Revit), and a pinned door or window goes with its deleted wall with a note ("Some pinned objects were deleted because their parent or host object was deleted."). Tests: `pin_tests.rs` in the core, `tests/pin.rs` here.

Delete on a type or a family in the Project Browser (`browser.rs`, `delete_type_asking`) asks `delete_type` with `dry_run` how many elements are of it: none, and it is deleted; some, and Revit's warning (`DeleteType`: "N instances of X will be deleted with it", OK and Cancel) comes first, and OK deletes the type and its instances (and what hangs on them) in one undo step. Live editing keeps its rule for the race where another copy places an instance of the type after it was deleted: in use wins over taking out (docs/areas/sync.md).
`dimgrips.rs`: the grips of a selected aligned or linear dimension, one on its line (`move`, square to the line) and one on each number
(`dimension_text_move`; the numbers of continuous and ordinate strings).

A click is made where the button was, not where the hand has got to. egui reads a click's position, and what lay under it, from the
pointer's last position in the frame, and counts two clicks in a frame as one, so a hand that moves on in the time between frames
(fast drawing) would click elsewhere, or once: `BimApp::pace` (run by `raw_input_hook`, with `show` asking for the frame that takes
the rest) cuts a window's events after each button press or release, and tests hand events over through `deliver`. egui also takes a
press and release more than six points apart for no click (a drag, if a frame falls between), which a hand clicking on the move
makes, so for every tool but Modify that is a click where the button went down (`BimApp::clicked_at`, where the plan, sections and
sheets take their clicks), and what lay under it is read there (`under_at`), not where the hand has got to. In Modify a drag is its
move and its box, so only a press let go within `QUICK` (0.2 s) is a click wherever the hand went: the drag it began is not
finished (`Canvas::quick`, `BimApp::finish_drag`: grips, crop regions, a sheet's views, the plan's move and box) and the selection
that drag's start replaced comes back (`BimApp::grab`) for the click to pick from. A slower press is the drag it is.

The selection's controls over a view take the pointer before the view does, as Revit's: the flip arrows, the pins, temporary
dimensions' numbers, witness dots and Make Permanent, a dimension's padlocks, EQ and grips, a wall's angles, the ends of a wall, grid
or run, a detail line's handles, a site element's grips, a crop region's and a level's grips, a shape's height labels and the
navigation bar. A press on one is that control's: the view picks nothing under it (a quick click included), draws no box and moves
nothing, nothing under it is lit, and the selection stays (`canvas::Controls`). A control that is an egui widget goes through
`BimApp::control` (it senses clicks and drags, so egui hands them to it alone); a grip the view's own drag works notes its square
with `BimApp::control_rect`. A press is tested against the pass before's controls, as egui tests its widgets. `tests/canvas_controls.rs`
presses and lets go of the pointer where each control is drawn, a frame apart. In the 3D view
a quick press is a click whatever the tool, and a camera the hand turned meanwhile goes back (`Orbit::before`). There, as in
Revit, Shift and the middle button orbit and the middle button alone pans; a primary drag orbits too, round the point under the
pointer, and with Shift held in Modify it is a selection box, as in a plan (`Orbit::select`, drawn by `canvas::box_shapes`), which picks what `Scene::in_box` finds: from the left what lies wholly
inside, from the right what it touches, by where the model is rather than what's in front, so what's behind is boxed too.
A 3D mouse (a 3Dconnexion SpaceMouse, SpaceNavigator, SpacePilot) moves the active 3D view as 3DxWare's object mode moves a
model (`spacemouse.rs`): the cap pans, pulling it toward you zooms in, twisting it turns the view about the vertical and tilting
it raises or lowers the eye, about the middle of the picture, while the window has focus. Like the hand it moves only the active
view, and only a 3D one: plans, sections and sheets it leaves alone, and a 3D view drawn beside the active one too. A camera
view's position is written back once the cap rests, and wheel turns 3DxWare may make of the cap are ignored while it moves. The
desktop app starts its reader (`BimApp::space_mouse`, Windows only for now), which reads the device's HID reports itself (with
3DxWare running too, while its profile for BIMCraft passes the axes on as `HIDMultiAxis_*`), the axes found in its report
descriptor (`spacemouse/descriptor.rs`). With `BIMCRAFT_SPACEMOUSE_LOG=<file>` the reader logs what it found and every report.

Startup (`apps/bimcraft/src/startup.rs`). A release build on Windows has no console, so a start that fails must say itself: `bimcraft.log`
and `graphics.json` sit beside the favorites (`startup::Files::beside`). `startup::begin` runs once the arguments are read (never for
`--version` or `--help`, which return before it), writes the log's first line and the panic hook, and picks the graphics for this start
(`Mark`: the choice, and whether the last start finished). A start is marked unfinished until its window has stayed up for `SURVIVE` (15 s)
or the app is closed the ordinary way (`startup::up`, `closed`); one that dies without a word, as a graphics driver can make it, leaves the
mark, and the next start takes the next choice on `LADDER` (wgpu's own, DirectX 12, Vulkan, OpenGL: Windows only), keeping the first that
survives. `graphics.json` keeps the backend's name (`{"backend": "DirectX 12", "starting": false}`), never its place on the ladder, so
reordering the ladder cannot change what a kept mark means; 0.8.0's marks (`"choice"`, a place on the old order) are read through that order. An error from `run_native` leaves the mark too, is logged, and on Windows shown in a message box (`startup::tell`: its words are
entries in the interface's PO catalogs). `WGPU_BACKEND` set by the user switches all of it off. A new way for the start to fail goes through `startup::note`, not `eprintln!` alone.

Tags. A `Tag` names an element at a `host` by a `label`: the name of one of the fields its category's schedule has (`schedule::field_text` reads it
as the schedule shows it, in the project's units), or, with none, the category's usual one (`Tag::usual`: a door's or window's `Mark`, a room's `Name`, else
the `Type`), `?` while that has no value. The host is anything `Model::anchor` finds a point for (a door or window's centre, the middle of a wall's or a run's
centre line, a column's, a component's or a room's point, the centre of a floor's, ceiling's or roof's outline, a stair's or ramp's start): a new kind of host is an arm there,
and `Model::check` refuses a tag whose host has none, or whose `label` isn't a field of it, saying which fields there are. A tag sits `offset` mm from that point (`Model::tag_offset`),
is drawn in plans only (a door's in a box, a window's in a hexagon, any other's as plain text, with a leader line when it has one), follows its host, and goes
with it. The Tag tool picks whatever `anchor` finds a point for, and the Options Bar's Label field is the `label` the click passes on; Tag All (`tag` without `ids`)
still takes doors and windows unless given a `category`.

Type Properties (`typeprops.rs`). Every kind of type opens in one dialog, after Revit's: the shared title bar, then `head` (Family, shown and not picked, and
Type, with Duplicate… and Rename… beside it; either opens a Name row under it, Enter or its button doing it), then `table` (Parameter and Value
under group bars that fold, `Table::row` and its helpers for a field, a check box, a drop-down, a dragged number, a colour, a value worked out, and
`Table::full` for what spans both columns, a type's layers or a sample), then `foot` (what stopped it, and OK, Cancel and Apply). Each dialog keeps its
state and its commands: the project's own types and a family's in `properties.rs` (`type`, `family`, and `rename_type` for Rename), text types in
`notes.rs` (`text_type`, with `rename`), filled region types in `fillpatterns.rs`, where Duplicate and Rename change the draft OK applies and a
pattern's … opens the Patterns tab. The project's own types and a family's end in an Identity Data group (`identity_group`: Type Mark,
Description, Manufacturer, Model, Fire Rating, Cost, Keynote, Assembly Code, URL, Type Comments), which sends the fields changed (all of them
for a Duplicate) as the `type` command's identity keys, or the `family` command's `identity`. The dialogs are one width, and the table's body one height where the window has room, known from the first frame:
a body that fitted its rows grew over a few frames, and a click meant for one row landed on the next. A refusal is said in the dialog.

The interface is Rev's, the ribbon tool its users know: the ribbon over the view, Properties over the Project Browser down the left
(`Docking::default`, which Settings' Reset Palette Layout puts back). Find (`find.rs`, Ctrl+K, and the search icon on the quick access bar)
lists every action a shortcut can be given (`shortcuts::ACTIONS`, with its keys) and every view the browser opens (`BimApp::openable`), by
every word typed (`find::matches`); Enter takes the first, a click the one clicked, Escape or a click beside it closes it. Rev copies a
layout, never another program's pictures or words: its icons are the ones `icons.rs` draws. `tests/interface.rs` drives it, and
`tests/smallest.rs` runs the least window in both layouts. (There was a second interface, BIMCraft's own, with its tools in a Tools
palette; it is gone, and a settings file that still names a `profile` or places the Tools palette opens Rev's: what it doesn't know is
left out.)

Scale, Edit Tools and presets (`presets.rs`) are window settings in `settings.json`, never in a project. The project's scale
(`presets::Scale`: Sketch, Studio, Team, Team the default so nobody loses a tool) leaves out panels and tools a project doesn't need yet
(`Scale::hides`, by English name: below Team design options, links and inquiry, in Sketch also sheets, phasing and view templates); a chip
on the quick access bar switches it at any time and lists the presets, Edit Tools and Manage Presets. Edit Tools turns every ribbon tool
into a box to tick (`edit_tool`): a click ticks or clears it and takes no tool, and what is cleared is `tools_hidden`, left off the ribbon
(Find still lists it). The ribbon reads both through `ribbon::tool_rules`, set each frame; `ribbon::panel` leaves out a panel the scale
does, `big_button`, `small_button`, `stacked_button` and `list_row` a tool. A preset (`presets::Preset`) keeps, by name, where the palettes
are, the tools shown, the scale and the layout; Settings > User Interface > Presets saves the layout now under a name, uses or deletes one,
and shows every tool again. A settings file's presets are read as hostile: unnamed, over-long or repeated ones are left out, 32 at most.

Themes. The window follows the system's light or dark until the user picks (Settings > User Interface >
Theme, or View > Dark Mode); beside the window's own light, dark and dark-with-a-light-canvas are three presets, dark ones (`style::Scheme`:
Dracula, after the open theme of that name, Midnight and AMOLED), and an accent colour of the user's own over any of them. `style::palette`
works out the frame's colours from light or dark, the scheme and the accent, `style::follow` puts them in force each frame (and
the widgets' look once they change) and `pal()` returns them, by value. An accent (`with_accent`) is the fills and strokes, a quarter of it
over the card for what is picked, and its lettering stepped toward the text's colour until it holds 4.5:1 on every ground it is read on, so
any accent stays legible; the contrast test runs every theme and an accent of every kind on each. The drawing keeps its own colours
(`canvas_pal`). `settings.json` keeps the scheme and the accent beside the theme.

Own colours over any theme. Beside the accent, Settings > User Interface > Theme takes a panel colour, a
canvas colour and a text colour of the user's own (`style::Look::panel_colour`, `canvas_colour`, `text_colour`), each with a Reset
that brings back the scheme's; `settings.json` keeps them as `panel_colour`, `canvas_colour` and `text_colour`, read as the accent is
(`colour()`), so one this version can't read is left out and the scheme's stands. Two grounds, because the window has two: the
cards' and the drawing's, which already part in Dark UI, Light Canvas. The panel colour (`with_ground`) is the cards' ground, with
their bands, buttons and hovered rows a step toward the ink that reads on it (black or white, `ink_on`, whichever holds more: one
always holds 4.5:1) and never so far that the ink stops reading; in Rev the chrome too, a step darker, as Rev's own is. What is read
on it (the text, the muted text, icons, errors, the contextual tabs) is stepped toward that ink until it holds, and the accent's
tints are made again over it (`with_accent`, whose tint of what is picked now steps back toward the card while the text would not
read on it). The text colour (`with_text`) is stepped the same way on every ground it is read on (in Rev the chrome's lettering is the
text's), and the dialog says so when it had to. The canvas colour is the paper the views draw on (`style::canvas_ground`, which
`canvas::color` returns for the drawing's white), and the drawing's ink follows it rather than the interface: dark ink on a light
one, light on a dark one (`canvas_dark`, from its luminance), so never black on black; a section's and the 3D view's pictures are
kept by their ground's colour, so a new one draws them again. `style::follow` restyles the widgets when anything in `Look` changes.
The contrast test runs every base with these colours at their worst (white, black, the grey both inks read worst on, a pale yellow,
a navy, pure red, and a text the panel's own colour).

The pictures in Properties. Under the filter row, the type the palette shows is drawn when it has layers
(`properties::layer_picture`, for walls, floors, roofs and ceilings, `LAYERED`): each layer to its thickness in its material's colour, a
type without layers as one band. A wall's run from its exterior face to its interior one, with the location line the selected walls share
dashed where it runs (`walls::depth`, the core's own rule) and named under it; a click on the picture sets the line nearest it, through
`modify`. A floor's, a roof's or a ceiling's run from the top down. A door's or a window's Properties have no flip rows or pictures,
as Revit's have none: it is flipped on the plan (the agent: `flip`, or `modify`'s `flip_hand` and `flip_facing`). On the plan (`flip.rs`) a door or window selected alone has Revit's flip arrows: across the wall beside
it, its facing, and along the wall on the side it doesn't open to, its hand (a window's mirrors its family's sash, `Model::opening_local`, in plan, 3D and IFC); a wall selected alone has one across it on its exterior side, along the wall from its middle (a quarter of its length, at least 56 px) so it never lies over the wall's pin.
A click is the `flip` command (`hand`, `facing`); the arrows are egui widgets labelled Flip facing, Flip hand and Flip exterior side. Space
in Modify flips the selected walls and steps each selected door or window through its four ways from its own state (the `flip` command's `step`: facing, hand, facing, hand);
with the Door or Window tool it flips the hand of the one about to be put down (`place_flip`), and the next ones put down with that tool keep it until Space again or the tool ends; the facing is always the cursor's side of the wall, live as the cursor crosses it; with the Wall tool it
draws the walls the other way round (`wall_flipped`, in `wall_params`: their exterior is to the left of the way they run, and it stays on for the chain until Space again or the tool ends) and the preview
marks the exterior side. A door or window being placed is sent the cursor as `toward`, so it opens to the cursor's side of the wall. A flip is not a move: a pinned door or wall is flipped like any other (Revit allows it, and a flipped wall's location line stays where it was), so `refuse_pinned` is never asked of a flip; do not add it. For rooms, their colour on plans (`room_colour`): swatches, No fill first, and any
other colour; a pick sets the room's `colour` (`Room::colour`, red, green and blue, none for no fill), which plans draw as a fill of
the room's region under its name (`render.rs`) and a file keeps (an optional field an older version leaves out, so no format step).
For floors, their slope (`floor_slope`): a short section of the slab at its true angle from its slope arrow's tail, a dashed line
level with the tail, and the angle written on it in degrees and percent (its accessible label too); under it the steps Flat, 1%, 2%
and 5%, the one they share lit, a click setting `slope` through `modify` as the Slope field does, a falling floor kept falling. A
floor with no slope arrow says it is flat and how to slope it, with a button that starts the Slope Arrow tool for it: the picture
never makes up an arrow, since `Model::check` refuses a sloped floor without one.

What an element is tied to (in Properties' ruled grid). Each tie is a link,
named by the other element's category and id, that makes it the selection, which every view shows. In the element's own rows: a
door's or a window's Host Wall, a railing's, a spot's and a tag's host (`Kind::Link`), and beside a wall's greyed Top and Base is
Attached, the roof or floor it is attached to. For one element, a Tied To group (`properties::ties`) lists the rest: a wall's
Openings (`Model::referring`, the elements whose `refs` hold its GUID), a roof's or a floor's Attached Walls, and the locked or
equal dimensions that hold it (`Model::held_by` in `constraints.rs`: one kept equal that measures to it, or one with a locked
segment that ends at it). egui gives a link's node a label's role when its text is selectable, so `properties::link` names it a
link again; a test clicks one with `click_accesskit`, since a wall's rows run below the palette's fold.

Properties' filter. With a selection of several categories, the line under the Type Selector is a drop-down of them, as Revit's is
(`BimApp::selection_categories`): Multiple Categories first, then each category with its count. One picked is what the palette shows
and edits, the selection staying as it is: `BimApp::palette_ids` is the selection narrowed to it, and the palette's subject, the Type
Selector, Edit Type (from the palette or the ribbon; Revit's right-click menu has none) and every edit the palette sends read it, never the selection.
`palette_filter` keeps the selection it was picked for, so a new selection starts it over.
What the palette reads of its elements, `element_json`, it reads several times a frame (the Type Selector, the rows, a family's
parameters), and a component's says the room it stands in, which costs its level's room boundaries: `BimApp::elements_json` keeps
each element's for the model it was found in (`palette_json`, by a `Weak<Model>` as the canvas keeps room outlines) and finds what it
lacks together, among one `Rooms`, so a level's rooms are found once for the whole selection. On the 20-storey highrise with two rooms
a storey, 50 selected components cost 180 ms a frame read afresh; kept, 0.004 ms, and the frame after a change 3 ms on one level, 27 ms
over all twenty. Select All Instances shares one `Rooms` over what the view shows the same way.

View Templates (`templates.rs`). VT and View > View Templates open it as Revit's does, in the look Type Properties has: on the left a tab for each
kind of view (`ViewKind`: Plan, Elevation/Section, 3D; a template of no kind is on every tab), the templates of the one showing, and Duplicate…,
Rename… and Delete, with New from This View… (enabled on the tab of the active view's kind) under them, each opening a Name row; they run at once
(`template` with `like`, `rename`, `delete`, `from`). On the right the template picked: a table of Parameter, Value and Include, its rows the groups
its kind holds (`ViewKind::groups`), each group's Include box named for the group. Its settings edit a draft (`TemplatesDialog::drafts`, one per
template, kept through Duplicate and Rename); OK and Apply send each template's difference as one `template` command, Cancel drops them.
V/G Overrides, V/G Filters and Design Options open Visibility/Graphics on the template's draft (`VgDialog::for_template`: no Elements tab nor a
drawing's layers, which are a view's own; its OK puts the categories, filters and options back in the draft), View Range a small dialog of
its four offsets, and Background Grids the grid editor. A view's Properties name its template on a button (`properties::wide_button`, as wide as the drop-downs, as Edit… is), which
opens the same dialog as Assign View Template: <None> first, the view's kind's tab showing, and OK also sets the template the view follows.

Visibility sets (`BimApp::visibility_sets`, `templates.rs`) are view templates, switched from the view control bar: no
layer is given to an element, and no new kind of entry was needed, because a template already holds what a view shows and its phase and a view
that stops following one has its own settings back as they were. The bar's Visibility Set menu names the template the view follows (<None> for
its own), lists <None> and the templates of the view's kind (`listed`), and a pick is one `graphics {template}`. Its Save as field makes one from
the view as it is that controls only what it shows and its phase (`SET`: categories, filters, phase with its phase filter; `template {from,
controls}`), and the view follows it, the two in one transaction and one undo step; a name a template has already is said and makes nothing, as
`template` would change that one. The Sketch scale leaves the menu out with View Templates, and a long name is cut short on the bar. The bar
runs onto a second row where the view is narrower than it, never under the palettes beside the view: its rows run down, so `footer.rs` starts
it as high as it was tall the frame before, and its foot stays on the view's.

The palette's rows are not striped: a stripe is `bar`, the group headers' colour. Buttons and drop-downs fill with `Palette::control`, a step past
`bar` (darker on light, lighter on dark), so a control never reads as a heading beside it.

The camera (`camera.rs`). The 3D view's Properties have a Camera group: Projection, a drop-down of Axonometric and Perspective that runs
`perspective` (off or on), and in perspective the Focal Length (a camera's mm, whatever the project's units) and the Eye Elevation, a length in the project's units. Focal
Length runs `perspective` with its `lens`; Eye Elevation is the camera's on screen, not the project's, and moves `Orbit::camera` by
`Camera::with_eye_elevation`. Each frame `orbit.rs` sizes the camera to the view through the view's lens (`orbit::lens_of`,
`Camera::sized`), so the eye stays where it stood as the lens or the window changes, and Home View and Zoom to Fit fit through it
(`Scene::fit_from`). Picking, Modify, the level grips and the section box grips work in perspective as they do parallel; a grip or an edge of
the box behind the eye isn't drawn.

The section box (`section_box.rs`). The 3D view's Properties have an Extents group: Section Box, a checkbox, and while it's on the
West, East, South, North, Bottom and Top faces as lengths in the project's units; BX turns it on or off (opening the 3D view). They run
`section_box`. In the view, `orbit.rs` draws the box's edges and, in Modify, an arrow on each face pointing out along its axis (a square
for a face square to the screen); a primary drag from one (`section_box::grip_at`, within `GRIP` points) moves that face by
`Camera::along` and `move_box_face`, the picture cut to the box as it goes (`Look::with_section_box`, part of the `Shot` it caches), and
runs `section_box` when the drag ends. The grip takes the drag, not the camera, and a click on a grip picks nothing behind it. A
plan's (and a plan view's) Properties have Section Box under Extents too, a `Flag` row: ticked, the plan shows what the box keeps.

Linked 3D View (VIEW-13; `section_box.rs`, a button on a plan's view control bar). On, the 3D view floats beside the plan
(`float_view`, the plan staying the view the tools work in) and is cut to what the plan shows (`plan_box`): its level, from its view range's
bottom (the level, unless the range says) up to its top (the model's top over the highest level, which has none), across the part of the plan
the canvas shows (`Canvas::view`). `BimApp::follow_plan`, run with the active plan's bar, sets it again when the hand rests (no button down,
no wheel turning), so the 3D view follows a pan when it ends and another level's plan when one is opened. The box is the window's, like
Temporary Hide/Isolate: `footer::Temp::section_box` on the 3D view's sheet, which `BimApp::view_model` puts through `section_box` on the copy
of the model the 3D view draws, so the project, its undo and its file never hold it, and the 3D view's own box stays as it was. Off, the 3D
view goes back where it was (`section_box::Was`: closed, its tab at its place, or floating) and is drawn as the project has it; closing its
window ends the link. The selection is the model's, so the plan and the 3D view share it already. The plan's own Section Box flag reads the
project's box, never the link's, so a plan that follows the box and a 3D view that follows the plan never chase each other.

Sketching in 3D (`sketch3d.rs`). Edit Boundary on a floor, ceiling, roof by footprint or shaft selected in the 3D view keeps the
sketch there, as Revit does: the lines are drawn over the picture on the element's own plane, `Model::sketch_plane` (a floor's
top, sloped with it; a ceiling's underside; a roof's base; a shaft's bottom), and `Sketch::plan` is
`Sheet::View3D`, so the window comes back to the 3D view until Finish or Cancel as it does to a plan (`BimApp::sketch_plan`,
while the element still has a plane). The view turns, pans and zooms as ever, and its clicks pick no elements: `sketch3d::Plane`
takes a point on screen to the plane (`Camera::on_slope`) and back. Modify picks a line where it shows (within `PICK` points of it
on screen, so a foreshortened line is as easy to hit as any), and a primary drag from a picked line's end holds that grip, not the
camera (`BimApp::sketch3d_grab`, before the camera moves), the ends there moved where it is let go; a Shift drag from anywhere else
is a box picking the sketch's lines (`sketch3d_box`: wholly inside from the left, touching from the right). The Draw panel's shapes, Pick Walls and Pick Lines (the element the picture shows under the
click), Split, Trim/Extend, Offset and Vertex take the plane's point as the plan takes its own, through `sketch_click`,
`sketch_modify` and `vertex_click`, snapped by `snap::in_sketch` in the element's level with reaches from the view's scale.
Align, which lines up with what a plan shows, says so. A view edge-on to the plane picks nothing on it, and says to turn it.

Work planes in 3D (`workplane.rs`; the core's in [views and graphics](views-and-graphics.md)). The plane has a patch of its own in
the view, as ArchiCAD's: a small square of it (`work_plane_ring`, the same size on screen at any zoom), by the model's south-west corner
on a level plane and about the point it was set through on another, lit with the plane's name under the cursor, that a click in Modify
opens the dialog from; so does the Architecture tab's Set (Revit's right-click menu has no Set Work Plane), in
its Datum panel. The 3D view's control bar has Work Plane, a `footer::menu_button` showing the plane (a level's name, `Surface, 90°` or
`Three points, 14°`, and `+ offset` when moved), which opens its dialog (`workplane::Draft`). It sets the plane three ways (`By`): Pick
from list, a level; Pick a surface, with Pick Surface; Pick three points, with Pick Points; and one Offset for each, along the plane's
normal (up from a level, out from a surface). Nothing is typed but the offset. A pick closes the dialog while it's made (`Picking`) and
brings it back with what it took, Esc going back without it: Pick Surface lights the face under the pointer (`Scene::face_at`, the
element's triangles in the plane of the one hit) and a click takes it, a point of it and the way it faces; Pick Points takes three
clicks, each snapping to the corner seen nearest the pointer within the snap's reach (`Scene::corner_near`, a square marking it, as a
dimension's ends snap in plan), else the surface's point there (a circle), the points taken drawn as dots (`workplane::aim`,
`draw_aim`). OK runs `work_plane`; what it refuses is said in the dialog. The plane is drawn as a grid over as much of it as the model
spans while the dialog is up and while a new outline is sketched on it (`workplane::draw_plane`, along the plane's own axes). Floor,
Ceiling, Roof and Shaft picked in the 3D view sketch a new one on the work plane at once (`set_tool`, `sketch_on_work_plane`):
`Sketch::surface` holds the plane, `sketch_surface` gives it to `sketch3d.rs`, and Finish makes it on the level below the plane
(`workplane::host`), as high above it as the plane is. Each of the four lies level, so on a plane that doesn't (`workplane::is_level`:
sloped, or upright as a wall's face) its tool says the work plane must be horizontal and makes nothing; the plane itself may lie at any
angle, for what comes to sketch on one later (an opening on a wall). A Line or Polyline sketched in 3D shows the line to come,
from the last point to the cursor, with its length (`sketch3d_preview`; the other shapes' previews are `shape_preview`'s).

The navigation triad (`triad.rs`; the core's `triad.rs` has what any front end needs of it). Left of the 3D view's navigation bar
(`BimApp::triad_rect`, which steps under the Options Bar as the bar does) stand the x, y and z axes in red, green and blue, turning
with the camera, each end a marker (the positive a disc with its letter, the negative a ring with a minus), the far ones fainter. It
is a triad, not a cube (#154 argues about a patent on clickable cubes). A click on an end turns the camera to look from that side
(`triad::turned`): +z the top, -z the bottom, -y the front, +y the back, -x the left, +x the right; the target, closeness and lens stay,
and a second click on the end the camera already looks from turns to the opposite side. The camera never turns straight down
(`Camera::orbit`), so the top and bottom views stand at 89 and -89 degrees, north up in the top view and the same turn in the bottom.
A camera view's new position is written back to it as any turn is. Alt and a click on an end sets the work plane square to that axis
(`triad::plane`, the `work_plane` command with a surface): through the point of the plane the view was on that is nearest the camera's
target, to the millimetre, facing the way the end points (a plane square to z always faces up); Alt and a click on the triad's empty
space clears the view's own plane, back to the lowest level (nothing is done, and there is no undo step, where it is there already).
Hovering an end names the view a click turns to (`Top`, or `Bottom` when the camera is on top already), and with Alt held the plane it
would set. `BimApp::triad_input` makes the widget over the view's, sensing clicks and drags, so a click or a drag that begins on it is
its own and never reaches the model behind (the view under it is not hovered either); `view3d` turns the camera with what it asks, and
`triad_draw` draws over the picture, so the processor's and the card's alike, and the pick uses the camera alone, so it picks the same
on both. A floating or linked 3D view beside the active one draws it but takes no clicks.

The Massing & Site tab's site tools (`site.rs`; [the site](site.md)) sketch an outline or a path at once in a plan, as Floor
does (`begin_site`, `sketch::Of::Site`), or place where clicked, Space turning what they place; their Options Bar holds the
kind, use and size, and a placing tool and a path being sketched preview what they make. A site element selected alone has
grips on its points and Edit Boundary or Edit Path in the Mode panel (`Of::EditSite`).

Floor goes into sketch mode at once, in a plan as in 3D, as Revit's does (`set_tool`); its sketch's ribbon has Walls' Outline
(`walls_outline_panel`), which puts the outline the level's walls enclose, and its courtyards, in the sketch as lines to edit
(`floor_outline`): what the tool's one click inside the walls made before.

Reveal Hidden Elements (RH, the view control bar's bulb; `footer.rs`) is Revit's mode: the view wears a magenta frame with
"Reveal Hidden Elements" in its top left corner (a temporary hide's frame is cyan, "Temporary Hide/Isolate"), what it hides is drawn
magenta and can be picked, and the rest is halftone (`shadowed` gives each shown element a halftone override in the view's copy). The
mode is not a temporary hide: Reset Temporary Hide/Isolate (HR) leaves it on. While it is on, the contextual tab is Modify | Reveal
Hidden Elements with nothing selected (`context`), and the Modify tab, whatever is selected, has the Reveal Hidden panel
(`reveal_hidden_panel`): Unhide Element (EU) and Unhide Category (VU), on only for what `BimApp::hidden_selection` finds the view
hiding of the selection (the elements hidden one by one, the categories hidden whole), and Toggle Reveal Hidden Elements Mode. They
run `graphics` with `show_elements` or `show` (`unhide_selection`); the selection stays, and with nothing of it hidden EU opens the
Elements tab as before and VU says so. The right-click menu offers the same as Unhide in View > Elements / Category in place of
Hide in View while a hidden element is selected in the mode.

The right-click menu (`context.rs`) is Revit's, in its order: Cancel, Repeat [last], Recent Commands, Select Previous (the
selection a change replaced, `note_selection`); Disallow Join or Allow Join when the right click lands on a selected wall's or
run's end grip (`end_under`; the flag is the element's, both ends alike, where Revit's is each end's); Hide in View > Elements /
Category / By Filter…, Override Graphics in View > By Element… / By Category… / By Filter… (the last two open Visibility/Graphics
on the category's row and on Filters), Create Similar and, for a loadable family's element, Edit Family; Select All Instances and
Delete; Find Referring Views (Go To View: the plans a section's line or a camera stands on); Zoom In Region, Zoom Out (2x), Zoom
To Fit, Previous Pan/Zoom and Next Pan/Zoom (`Canvas::next`, from what Previous went back from); Browsers > Project Browser and
Properties, ticked while they show. It shows no keys beside its items, as Revit's shows none. Revit's menu has no Edit Type, Set
Work Plane or Home View; the window's Create Group, Ungroup, Reload CAD and Open Camera View stay as groups of their own.

Filter. While something is selected, every tab's Select panel has Filter, and the funnel by the status bar's count opens it too
(`filter.rs`): Revit's Filter, the selection counted by category, each checked to keep or cleared to leave out, with Check All,
Check None and the total kept; OK keeps what is checked selected. Its Filter by list goes further: any parameter every selected
element has, which `selection::common` finds (the schedules' fields their categories share, Category first, Count left out; what
schedules aren't of, lines, regions, text, tags, grids, views and sheets, has its fields in `schedule::MORE`: a line's Line Style), and
the list becomes that parameter's values (`selection::groups`, as schedules show them, numbers by value, `(none)` last), to keep or
leave out the same way. The `filter_selection` command gives agents the same: the groups, the shared fields, and with `keep`, the ids kept.

Undo's list. The arrows beside Undo and Redo (`BimApp::steps_list`) list `Session::steps`, each a `Step` the core keeps with its
undo snapshot: the command's label and the elements it made and took away, by category, which `step_name` turns into Create Walls
or Delete Door (else the label). A click runs `undo` or `redo` with `count`.

Tabs. Ribbon and view tabs are pills (`tab_button_with`): the open one in the accent's soft tint, a contextual one solid green
when open and tinted green when not. A view's × is part of its tab (`tab_button_with`'s `room`). The ribbon's tab row scrolls
sideways with the wheel in a window too narrow for it.

The look. `style.rs` holds the tokens: the palettes, light and dark, the app bar in each the window's colour (light, with a hairline under it, in the light theme; dark in the dark ones); the
card frame (`style::card`: 8 px corners, a hairline border and a soft shadow, standing `GAP` off the edges it's told to); IBM Plex Sans
and Mono (`assets/fonts`, SIL OFL 1.1; built into the executable, so `NOTICE` names them with their versions and sha256 and every package
carries it and both OFL.txt, as `copy_docs` in `packaging/env.sh`, `bimcraft.wxs` and `portable.ps1` do: `tests/fonts.rs` checks), with
`style::semibold` for titles and tabs (the body's face until the window has loaded it, a frame after `apply`). The semibold face is a
family of its own, and egui does not fall back across families, so the fonts with the hanzi and the Hangul join every family in
`style::families` (`lang::use_cjk_font`), whatever language the window speaks. `BimApp::show` paints the active view's ground (`backdrop`) under everything, then the app bar (Home, File, the
Quick Access Toolbar and the title, light on dark, `style::on_app_bar`), and the ribbon, the Options Bar, the status bar and each docked
palette stack as cards, so the gaps between them read as the view going on beneath. A drawn view (a plan, a section, the 3D view, a
sheet) fills its pane from the top of the pane to its foot: its tabs float over its top left on a card of their own and the view control
bar over its bottom left, each drawn after the view in a child `Ui` that takes no room of it (`Ui::new_child`; `scope_builder` would
allocate it, and an auto-sized floating view window would grow a little every frame), so they are over it for the hand too. A schedule
keeps its tab strip, so its first rows aren't under the tabs. A plan or a section is fitted into what the two cards leave clear
(`Canvas::room`, `FLOATING_CARD`), the Options Bar comes below the tabs (`BimApp::tabs_foot`), and a palette docks along the view only below
them. The Options Bar (`BimApp::options_bar`) floats over the view's top left, an `Area` above it, while there's something on it (a
contextual tab's name, a tool's options): coming and going it takes no room, so the view never moves under a drag when a selection
fills it, and a click on it never reaches the view. File > Settings > Layout picks Floating (the cards) or Compact (`BimApp::compact`, kept in `settings.json` as
`layout`): the same pieces flush to the edges and square, the view's tabs and its control bar strips above and below it, `style::gap` and `style::card` reading it each frame (`style::follow`, given the `Look` the user kept: Compact and the Light Canvas).
Text holds 4.5:1 and a focus ring or an icon 3:1 on what it sits on, in both themes (`style::tests` computes WCAG's ratio for every pair the
window draws): `accent` is for fills, strokes and the drawing, `accent_ink` for the accent as text, an icon or a focus ring
(`style::focus_ring`, which on the app bar is the bar's lettering colour), `on_context` for the open contextual tab's lettering. A new pair of
colours goes in that test.

The title bar (`titlebar.rs`). On Windows and Linux the window draws its own (`BimApp::own_title_bar`, set by `main`, which opens
the window without the system's decorations): the app bar is the title bar. Its bare ground moves the window and a double-click
there maximizes or restores it (`titlebar::drag_area`, laid out before the bar's buttons so they keep their presses), Minimize,
Maximize (Restore Down when maximized) and Close sit flush in its right end (`caption_buttons`, 46 points each, Close red under the
pointer), and `titlebar::frame`, over everything, resizes the window from 4-point zones along its edges (`titlebar::EDGE`; corners twice as long,
winning where they meet) and draws a hairline round it; a maximized or full-screen window has neither. The zones sit over the window and win
the hit test, so they are as thin as the room each layout leaves round its controls (the app bar keeps 5 above its buttons, the cards stand off by
`GAP`, a Compact card 3), the top edge stops short of the caption buttons and the right edge starts under them (`titlebar::zones`, given
where `caption_buttons` put them this frame), and a Compact card docked right pads its scrollbar in by the same 4. The titlebar tests run in
the window's real geometry (`own` fills the window, with none of the margin `app()` keeps). Moving and resizing are the
system's (`StartDrag`, `BeginResize`), so Windows' snapping works, and Close is the system's Close, so unsaved work is asked about.
The start page has a slim bar of its own (`start_title_bar`). Settings > User Interface > Title bar asks for the system's instead
(`system_title_bar`, kept in `settings.json`, read by `main` before the window opens and switched at once with `Decorations`);
macOS keeps its own.

The least a window can be is `window::MIN_SIZE`, 900 by 560 points (`main` gives it as the window's minimum), and nothing runs under, over or
past anything else there (`tests/smallest.rs`, in the window's real geometry): the app bar's title sits between the quick access and the
caption buttons, in the window's middle if it fits there and cut short with an ellipsis if not; the ribbon's tab row scrolls, and a tab just
picked (a contextual tab comes last of all) is scrolled into view; the Options Bar wraps to a second line, and a view's navigation bar
steps under it where a drop-down (which doesn't wrap) still reaches over it (`BimApp::nav_corner`); and a stack of palettes standing over another leaves it `dock::LAST_MIN`, so its last card never runs under the status bar,
even when the window was brought down from a taller one. Nothing is fitted to a size below that: the window is asked not to go there.

The ribbon's panels (`ribbon.rs`). A panel's name, under its tools, is a button (named `Build ▾` for screen readers) whose list
(`ribbon::list`) draws the same tools again as rows: `ribbon::listing` turns `big_button` and `small_button` into `list_row`. A tab too
wide for the window makes room from the right, on the last frame's widths (`ribbon::tab`, `ribbon::fits`): a panel hands its last tools
over to its list (`ribbon::handed_over`: counted, not drawn), and when one tool left isn't room enough it folds to one button that lists
them all, and the panel before it is asked; what still doesn't fit scrolls sideways. A tool that drops a list of choices down (Project
Units, Schedules, Duplicate View, New Family...) goes through `ribbon::choices`, never `Popup::menu` on its own button: egui keeps one
popup open, so a popup of a row's own closes the panel's list that holds the row, and with it itself. In a list the choices are a
submenu of the row; a request to open them at once (UN) opens the panel's list when the tool has no button, and waits for a tab's second
frame, since its first lays out every tool (`a_tool_with_choices_of_its_own_opens_them_from_a_panels_list`).

Keyboard Shortcuts. Manage > Shortcuts (or KS) opens `shortcuts_window` (`shortcuts.rs`): every shortcut by what it does, from `shortcuts()`
(`SHORTCUTS`, the fixtures' letters, the commands' aliases), with Add, Edit and Load File. A shortcut is two letters or digits, all that `typed` holds,
and a custom one is tried before the defaults. Each name in `ACTIONS` is a tool or command the window runs (`execute_shortcut_action`), and a key never
carries Revit's name for a tool BIMCraft lacks (Sun Settings, Detail and Model Line, Split Face, Brace, Beam System, Reference Plane, the duct tools): it waits
for the tool. Load File reads JSON (`{"WA": "Wall"}`), Revit's `<Shortcut CommandName=… Shortcuts="WA#W1">` lines, or `KEY - Name` lines, up to 64 KiB;
a description is an action only by its whole name in one of the interface's languages (`resolve_action`), and a key that is not two characters, names no action or
is another action's is skipped and counted in the message. Custom shortcuts are kept in `settings.json` (`shortcuts`: key to the action's English name, as `Settings::shortcuts`), written when one is added, edited or loaded and
read into `BimApp::custom_shortcuts` at startup (`use_settings`), so they last past the window; Restore Defaults is kept too, as an empty object, and New and Open keep them.
`Settings::from_json` keeps only what the dialog could have made (a key of two letters or digits, in upper case whatever its case in the file, and a name in `ACTIONS`),
at most `MAX_SHORTCUTS`; a value that isn't an object leaves what the window had. Tests: `tests/shortcuts_kept.rs`. Match Properties (MA) is the `match_properties`
command; with one element selected the window remembers its type for the next one drawn, with several it gives the others of the first's category its type.

Annotation dialogs and draw tools. Manage > Additional Settings lists Line Weights, Line Styles, Line Patterns (Line Styles on its
Patterns tab) and Fill Patterns. Fill Patterns (`fillpatterns.rs`, also Edit Type of a filled region) and Line Styles
(`linestyles.rs`) edit a draft of the project's entries and send it as one transaction: what is deleted and made again under its name is changed in
place, and renames go through `renames::plan`, which runs a chain from its end and a swap through a spare name (the core refuses a rename onto a name
that is taken). A draft keeps the entries the project held when it opened and is refused over a project that has changed since (an agent's command),
so Apply never undoes or deletes what the draft never saw. A blank or repeated name is refused in the dialog. What an import or an export says
(`note`) is said in the dialog, not in the status bar, which can lie over its buttons. A PAT file is read by the `import_fill_patterns` command on a scratch
project that holds the draft's names, so the command's keep or replace rule and its caps are the dialog's too (Replace imported by same name is the command's
`replace`); an export to a name without `.pat` does not write over a file of that name with `.pat`. The preview draws a type as the plan does, in the canvas colours.
The note belongs to the Patterns tab (the types do not show it), a file's patterns are told in the singular for one, and a left-out pattern is named with its reason (the
core's message without the pattern's name it begins with), each cut at a word. In a test a file dialog is answered through `dialog::BEGUN`; no system dialog opens.

A dialog that holds a long list (these two, Edit Type, and Line Styles) never runs off the window: the list, and a fill pattern's families open under it, scroll in
the height the window has left (`style::dialog_room`: the window's height less the dialog's frame, what is drawn above the list, and what is drawn under it, the notes
and the row of buttons), so OK, Cancel and Apply stay in reach down to the least window, and the note and the error are boxes that scroll (`style::boxed_text`). When a
fill pattern's families are open they share the room with the list, half each. Line pattern segments scroll in their separate creator. `tests/smallest.rs` opens each with forty of everything and a long
note at the least window and asks that every button lie inside it. The room is worked out from the heights of the rows, not measured after layout, so a label that wraps
in a language with long words can take a row or two more than it reserved: the dialog then scrolls a little instead of fitting exactly.

The window is on one project, the `uid` of the model it was built on (`BimApp::project`): a model of another `uid` in the session's place (a script's or an embedder's
New or Open; the control port turns an agent's away, `bridge::REFUSED`) starts the window over (`reopened`), so a sketch or a dialog's draft of the old project cannot be
finished into the new one, and gives the new project a recovery copy of its own (`autosave_document`). The replaced project's own recovery copy is not asked about: it stays until the new project's first snapshot is written and is deleted then (`select_destination` retires it, `cleanup_retired_autosaves` removes it), where the old code wrote the new project over it; File > New asks about unsaved changes first and then retires the copy at once, and a copy found on disk and not yet answered (a candidate in the Recover dialog) is never deleted this way. Hover and selection are drawn in the canvas's palette, not the interface's (`canvas_pal`): on a light canvas inside a dark interface the dark palette's
blues were 2.5:1 on white; `style`'s tests keep the two told apart and 3:1 on the paper in `CANVAS_LIGHT`, and keep `LIGHT`'s tints as they were. A section's picture is drawn first and once a frame, and the drawing over it
(`render::section_over`) takes its regions' ink from that picture (`Cut::draw`), so a pan or a zoom costs one picture; a hover that changes along with the view draws it a
second time.

The Draw panel's tools leave nothing behind: `set_tool` drops Insulation and Revision Cloud (the Detail Line tool's) and the lines picked for Fillet Arc and
Trim/Extend (`forget_picks`), a sketch's picks are dropped when its lines change (`track_sketch`, once a frame), except an unchanged Multiple Trim boundary, and when it ends, and a shape's typed numbers are
dropped when the shape changes. A fillet that does not fit keeps its two lines picked and spends the digits; a click that sizes the arc clears them. A number of the
Options Bar that the shape in hand takes and cannot read (`options_error`) is red and makes nothing. A sketch keeps the lines as they were before each change
(`Sketch::track`), so Ctrl+Z takes back a fillet, a trim or a grip move whole; it belongs to the plan it began in, and the window comes back to that plan until Finish or
Cancel Edit Mode (`BimApp::sketch_plan`; a plan whose level was deleted is nowhere to come back to, and holds neither the window nor the tool: a view that cannot use the tool takes Modify, as for no sketch). egui draws a Dot (a dash of a hundredth of a millimetre) about a pixel and a half long, so it shows; `a_dot_of_a_hundredth_of_a_millimetre_still_draws_about_a_pixel_on_the_canvas`
fails if that changes.

Limits of the annotation stack, left as they are: the desktop has no group type rename, delete or edit (the `group_type` command does them), and a group taken
apart with Ungroup and made again with Create Group cannot take its old name (the core refuses a name in use), so what was placed from the old type does not follow;
detail groups and repeating details are placed in plans only; Properties' Group row is read only. Pick Lines and the polygons are Draw-panel tools that make
curves, not commands, so an agent draws the curves they would make. A sketch's Trim, Offset, Split, Fillet, Join, Vertex and Convert Spline (`sketch_modify` and its neighbours
in `draw.rs`) act on the sketch the window holds with the core's `draw` and `curves` functions and are not commands, so the CLI and MCP cannot reach them. The Align tool on a
pattern asks `Model::align_pattern` where the pattern then starts and sends the answer through `modify`, saying the core's refusal when it gives one. The patterns list is not virtualized: rows off the screen paint
no swatch, but each still lays out its widgets. A sketch's Ctrl+Z takes back at most the last 100 changes (`sketch::UNDO`); an older one is forgotten. The window tells a replaced project by its `uid`, so
`open` of the very file the window already holds, put in the session's place by a script or an embedder, is not noticed: the file keeps its `uid`, and a sketch or a dialog's draft survives it
(the core's `Session` counts its replacements in a private `resets`; a public reader of that count would let the window see this one too).

The Line Styles Patterns tab opens a separate Line Pattern Creator for New Pattern and Edit. Its preview uses the core
pattern geometry (including font outlines) and can follow a curve. The creator saves into the parent draft, with its own
Cancel, so OK/Apply remains one undoable Line Styles transaction. See [views and graphics](views-and-graphics.md) for the
seven segment kinds, paper units, text fitting and centerline semantics.

Annotate > Dimension has Aligned (DI) and Linear, and Angular, Radial, Diameter, Arc Length, Spot Elevation and Spot Slope small beside
them (`BimApp::dimension_panel`); Spot Slope (`Tool::SpotSlope`) runs `spot_slope` where a floor or a roof is clicked in a plan. Aligned and Linear are one tool, `Tool::Dimension(mode, refs)`: a click near an endpoint picks it before the line
it ends (`canvas::dimension_pick`), marked with a square; after two, a click away places the dimension the Options Bar's Place says (Auto,
Aligned, Horizontal, Vertical; Space steps them), drawn as it would be while the cursor moves. `Tool::Round(kind, picked)` is Radial, Diameter
and Arc Length; after its arc, Arc Length takes two ends on it to measure between (`dims::arc_point`), or a click places the whole arc's. A new dimension takes the type the Type Selector names, else its kind's (`dimtypes::dimension_type_for`); Edit Type on one opens
Dimension Type Properties (`dimtypes.rs`), and Properties edits its prefix, suffix, text above and below, Replace With Text and Equality Display.
A double click on one of its numbers opens Dimension Text for that segment (`DimensionTextDialog`). Selected, an aligned or linear one shows
a padlock under each segment and, over a string of two or more, EQ past its end (`dimension_constraints`): a click locks or unlocks, or makes
it equal or not.

## Array

Array (AR, Modify) is the core's `array` command (`crates/core/src/commands/array.rs`) behind `Tool::Array` (`array.rs` in the
desktop). The command makes member 1 to count-1 with `duplicate`, the machinery of `copy` and `rotate`, so doors and windows follow
their copied walls, tags and dimensions follow what they measure, and it returns `ids` (all the copies) and `members` (each
member's ids). `move_to: second` takes the distance or angle as the step between members, `last` as the span from the first to the last
(a 360 degree span shares the circle: no copy lands on the original). `count` counts the original. The tool: Options Bar Linear or
Radial, Number, Move To, Constrain; a linear array is two clicks through `snap_point`, a radial one is Rotate's two clicks or a typed
angle, about the selection's middle or Place Centre. The preview draws up to 60 members as ghosts. **Not here:** Revit's "Group and
Associate": an array's copies are independent elements. When model groups exist (step 4 of the Revit parity plan) the array makes its
members into one group, with a `group: true` parameter.

## Background grids

Each plan, section and elevation keeps its canvas grid in its `Graphics::background_grid`, saved with the project, copied
with independent view duplicates, and edited through the undoable `graphics` command. Reflected ceiling plans use their
plan key's graphics, like their scale and other view settings; dependent plans follow their parent's graphics.
The Graphics group in a view's instance properties has a Background Grids Edit… button. The grid menu beside Temporary
Hide/Isolate (the glasses) keeps Show grids, independent Main grid and Subgrid checkboxes, Grid snapping and Grid settings….
The editor uses a draft: Apply/OK save one undo step, Cancel discards unapplied changes. File > Settings has no grid page;
application settings files from the earlier global implementation no longer affect views.

View templates can control Background Grids as a group (`Group::BackgroundGrid`, JSON `background_grid`), for plans and
elevations and sections. New from This View copies the effective grid. View Templates has a Background Grids row whose Edit… opens
the grid editor on the template's draft (`open_template_background_grid`; its OK hands the grid back, the templates' OK sends it). Followers use the
template's values and gray out their instance Edit button, bottom menu grid settings and any open editor; removing template
control restores each view's own grid. The engine rejects bypasses through `graphics` and `modify` too.

`core::background_grid` owns the lattice geometry, shared by painting and snapping. Both lattices start at the fixed view
origin (0, 0), with one counter-clockwise rotation in degrees. Main spacing X/Y is entered in the project's actual units;
subgrid divisions X/Y divide that spacing independently. Each grid has its own colour and pen (0–16), with default colours [230,230,230] for main and [245,245,245] for subgrid,
following the project's line weights and the Thin Lines display toggle. Main lines take precedence over coincident subgrid lines.
Dense subgrids disappear below four screen pixels; main grids thin by whole multiples without moving the origin. Drawing
is bounded to 4096 lines. Section/elevation textures expose empty paper behind the model while retaining opaque model faces.
Grids are canvas aids; they do not create elements or appear in sheet/export drawings.

Grid snapping is a global application preference (`BimApp::grid_snapping`, saved as `Settings::grid_snapping`), available
in Manage > Snaps and the bottom grid menu. It is not part of view graphics or view templates: the checkbox stays editable
when a template controls the active view's grid, and changing it creates no project undo step. New/Open retain the setting.
It uses intersections of the active view's enabled subgrid, or main grid when the subgrid is disabled; with neither enabled,
there is nothing to snap to. Show grids only controls display: snapping can remain on while the grid is hidden. Object snaps, one-point snap overrides and Shift
constraints keep priority; grid snapping replaces the free-point fallback and automatic angle guides. Sections and
elevations snap annotation placement in their local horizontal/height coordinates. Model validation rejects invalid
spacing, divisions, pens or rotation, and commands roll back the whole edit.

## Copy and Paste

Ctrl+C (or Copy to Clipboard, the first row of the Modify panel's Paste list) runs `copy_to_clipboard`: the core keeps the GUIDs of what was copied on the session (`Session::clipboard_ids`), read from the model when pasting, so what changed since is pasted as it is and what was deleted is left out; the doors and windows of copied walls, the railings of copied stairs and the bars of a copied beam, column, footing or floor go along; the bars are those that were there when it was copied (a bar drawn since is not pasted, one deleted since is left out). Ctrl+V (or the window system's paste event, `clipboard::clipboard_keys`) starts `Tool::Paste`: `canvas::paste_ghost` outlines the copied elements where the plan draws them (those on another level have none) and a dashed box round all of them (`Session::clipboard_extent`), their middle (`Session::clipboard_anchor`) on the snapped cursor. One click runs `paste` with `by` (and `levels: [the plan's level]` only when what was copied stands on a level and not on this plan's, `Session::clipboard_level`, so what is pasted into a plan stands on its level; grids, sections and the like stand on none: with `levels` the core puts them down once, `by` from where they were, in the same step), selects what was pasted and returns to Modify; Esc cancels. The Paste button ends the second row of the Modify panel (`clipboard::clipboard_buttons`: a button there costs the ribbon no width, a panel of its own folds the tabs of a 1280 px window); its list has Copy to Clipboard, Paste from Clipboard and Aligned to Selected Levels (`PasteLevels`, a dialog of ticked levels), to Selected Views (`PasteViews`, a dialog of the views of the kind the annotation was copied in, a section's or an elevation's only those parallel to it, with a search box, `Session::clipboard_paste_views`; greyed while the clipboard holds no annotation, as Revit offers it only for view-specific elements; the `views` param: view-specific elements, that is dimensions, tags, notes, detail lines, filled regions, detail groups and spot elevations, go at the same place into each view of the kind they came from, one copy of each into each, and a model element goes to the level of each plan among them as Aligned to Selected Levels does and is left out of other kinds of view; a dimension or tag goes only into plans of its own level), to Current View (`view`; not offered in the plan the elements were copied in, as Revit's "The view must be different"), to a Picked Level (`Tool::PasteLevel`: only in an elevation or a section, where a click on a level line pastes onto that level, as Revit's "you must be in an elevation view"; the list item is greyed elsewhere and there is no dialog) and to Same Place (`same_place`). A door or window copied without its wall goes in the wall it lands on (Revit's), on the plan's level (a wall based on that level before a lower one running up through it; at a join the nearer axis, kept within its run; a window's sill is measured from its wall's base, so it stays as high above the level), and is left out with its reason when no wall is there. An element that has to be raised where no level stands at the new height is raised by its offset (a room's base and limit offsets, a floor's, a roof's, a pad's, a cloud's, a bar's heights) or, a stair, ramp, CAD import, site element or room separation line having none, left out. The status line carries `notes` and `left_out` of the reply. Tests: `tests/clipboard.rs`.

## Moving about, selecting and repeating

`canvas/navigation.rs` is how a hand moves a plan, a section or a sheet (all three go through `Canvas::navigate`). The wheel is
judged once for each gesture: the first wheel event after 0.3 s of quiet decides (`first_hand`), and the gesture's other events
are the same hand's. A mouse's first event turns one way and is a notch (at least 50 pixels, whatever the browser's zoom, the
system's lines to a notch and the display's scaling make of 100 or 120) or a whole number of lines; a trackpad's is a gesture's
start or end, both ways at once, or small steps. What was judged decides what `smooth_scroll_delta` means: a mouse zooms about
the cursor (Shift and the wheel pans along) and a trackpad pans, when Auto is on, and a pinch (`Event::Zoom`, or Ctrl and the wheel, which egui turns
into one) zooms either way. Where the two cannot be told apart (a smooth wheel's small steps, a Mac mouse's) it reads as a
trackpad: Settings > User Interface > Mouse wheel (`Wheel`, kept as `mouse_wheel`) is Zoom (Revit's way, and the default: the wheel always zooms, and so does a
trackpad's two-finger scroll), Auto (the judgement above, for a laptop) or Pan (always). `Nav::mode` is Pan (the left button drags the view, the canvas senses hover only so no tool sees the drag) or Region
(Zoom Region: a box dragged, or one corner clicked and then the other, then the view fills with it); both end with Esc or a tool.
`Canvas::remember_now` keeps the view before a fit, a zoom button, a region, and the first of a run of wheel or pan steps; ZP
(`previous`) gives the last back.

`selecting.rs` holds `Flow`: `pending` is the Move, Copy, Rotate or Mirror asked for with nothing selected (the app is in Modify
meanwhile, so picking, boxing and Tab are Modify's; Enter calls `set_tool` with it), `last` is the tool last started (Enter with
none running, RC and the menu's Repeat start it again; only what the person starts is remembered: the ribbon, a shortcut or a menu go through `BimApp::start_tool`, and `set_tool` alone, which is how the app starts a tool for itself (a dialog's pick, Paste from Ctrl+V), remembers nothing), and
`preview`, the chain Tab highlights (`canvas/picking.rs`, over the core's `wallchain::wall_chain`, which the agent reads as the
`wall_chain` command). Tab only highlights, as Revit's does: it never touches the selection; a click on the chain selects it,
Ctrl+click adds it, Shift+click takes it away (`click_set`, then `pick_up`), a press and drag on one of its walls selects and moves the whole chain (`Canvas::pressed_chain`, kept at the press because the highlight goes as the cursor leaves the wall), and the highlight goes when the cursor moves on to
something else. Revit has no default Ctrl+A for the model, and neither has the app (in a text field it is the field's, as everywhere); its way is Select All Instances (SA, and the right-click menu's Select All Instances > Visible in View or In Entire Project: every element of the selected elements' types, shown in the view or not; the shortcut list has "Select All Instances in Entire Project" with no default keys). The menu's Recent Commands submenu lists the last five commands used, each once, newest first (`Flow::recent`; `repeat_recent` starts one and makes it the newest), beside Repeat. Arrow keys call `move` with a step of about three pixels at the view's zoom (`nudge_step`), ten times
that with Shift. The status bar names the element under the cursor from `Canvas::hovered`; the cursor shape is asked for in
`plan_cursor` and kept in `Nav::icon` for tests.

# Editing together (sync)

How copies of one project (two people's pages, the server's copy for the agent) tell each other what changed. Code:
`crates/core/src/sync.rs` (changes, merging, undo history) and `sync_settings.rs` (the settings entry by entry); tests in
`sync_tests.rs` and `sync_settings_tests.rs`. The transport (a shared Yjs document per project) is Arqiflow's.

## What travels

[`Changes`](../../crates/core/src/sync.rs) is two maps of JSON text, `elements` (by element GUID) and `project` (by key), and
`complete` for "everything there is". Copies agree on GUIDs, never on ids.

- **Elements** travel as a file holds them: whatever an element points at in the library (its type, a layer's material, a view's
  template, its project parameters) is a GUID, not a name (`Model::element_to_file`, and `element_from_file` back).
- **Settings** travel entry by entry. A map of entries (types, materials, families, text types, view templates, phases…) sends
  `<map>/<guid>` for an entry's body (`{"entry": …, "subs"?: […]}`) and `<map>/<guid>/name` for its name. A fixed vocabulary
  (`phase_overrides`, `object_styles`, `site_figures`, `title_values`) sends `<map>/<key>`. Everything else is one key.
- `sync::FORMAT` says which of these a copy speaks. A page and a room on different numbers do not edit together.

## Library GUIDs are the project's

Each library entry (type, material, family, family type and parameter, phase, option set and option, view template…) has a GUID
that a file stores and every copy shares, as an element's. Where a file holds none (it is from before the library, or before a
kind of entry), reading it gives each entry `stable_guid(uid, kind, name)`, the standard an IFC import already follows for its
elements: two copies reading one old file agree. An entry a command makes gets a random GUID, and its copies learn it from the
room. `sync_library` no longer makes the library again from names per copy.

## A rename

A rename is one change: the entry's `name` key. Nothing that points at the entry changes (bodies and elements hold its GUID), so
there is no note, no timer and no chain of follow-ups. A copy told a new name renames its entry (`Library::rebuilt`) and its own
elements, which hold names in memory, follow by going through their file form: `element_to_file` with the old library, then
`element_from_file` with the new (`Model::follow`). Only elements need it: a told body already holds GUIDs and is named by the
new library when it is read.

Name and body are separate keys so that a rename and an edit of one entry made at the same moment both stand: a shared room keeps
the last write of each key. (An element, and an entry's body, are replaced whole: last write wins per element and per entry body.)

An entry taken out by one copy while another changes its body leaves a body with no name in a shared room: a copy that has no such
entry makes none from it (the entry stays out; a rename of an entry taken out is dropped the same way), and tells the room to take
the body out (a `fixes` entry that says none), so that the room does not keep a difference that no copy can settle (Arqiflow's
server saves the room by what differs from the stored model: it would open the engine at every save).

## An entry in use is not taken out

On one copy, `delete_type` does what Revit's Delete does: a type with instances is deleted with them (the window warns with their
number first). This is for the race between copies: an entry that something *another copy* names is not taken out, **in use wins
over taking out**, so a wall one copy placed of a type while another deleted it is not lost (a material, a template, a phase… too).
When changes take an entry out that a copy still names (a wall this copy placed meanwhile is of the type; a type here is made of
the material; the changes themselves bring a wall of it), `Model::merged` keeps the entry. It leaves the taking out out of
`Merged::taken`, names it in `Merged::kept` (`{entry, kind, name, uses}`: `wall type`, `Beta`, `walls`), and returns the entry's body and name keys, as this copy has them, in `Merged::fixes`. The page (and Arqiflow's
server, for what the agent and the room change) puts them in the room, so the copy that took the type out hears the type again,
and every copy ends with the type and the wall, nothing lost. What a kept entry names is kept with it (a few rounds, until
nothing more is). A family's type or a design option that is in use keeps its owner's entry.

What counts as naming: the elements this copy sends (its own text links never count, they are never sent), the elements the changes
bring, this copy's own changes not sent yet (they will be sent, so the room then holds the entry and what names it, and nothing
loops), and other entries (a type's layers, a view's template).

Who is told: Arqiflow's page shows `kept` as a notice ("Wall type Beta was kept: walls in the model use it."), and the page of the
one who took the entry out, which hears its two keys come back, says the same without `uses` ("it is in use"). Nothing is refused.

## Elements that wait

An element that names a library entry this copy has not heard of yet (the wall came before its type) is not refused: it waits in the
session (`Session::waiting`; settings: `waiting_project`), in `Merged::waiting` and nowhere else. Only that error waits (the check
that names what an element holds that the library has not); any other (a door whose wall is gone) is refused as ever.

When the library changes (an entry made, renamed, taken out, or a whole room), the waiting are offered again, **as a batch of their
own after the changes that came**, so that one that still cannot be taken holds nothing else back. Changes that touch no library entry
do not offer them again. The order the changes come in does not matter: the copy that took the type out hears the wall first and the
type after, and places the wall.

Each waiting item is offered again at most 20 times (`WAITING_TRIES`); then it is given up on and named in `Merged::refused` once. The
queue holds at most 10,000 elements and 10,000 settings and 8 MiB of JSON together; an item that does not fit is named the same way.
What the room says of a key (or of an element) later is what counts: it replaces what waited for it.

## One key that cannot apply

A change of one entry that cannot apply (a body of the wrong shape, a key no entry has, a GUID that is another kind here) is
refused alone, named by its key, and the rest of the batch is taken (`Settled::take`: all together where they pass; else halved
until each one that does not is alone). Only a whole room (`complete`), which replaces everything, is taken whole or not at all.

A page offline for any time, or an element that arrives after a rename, needs nothing: the element names the entry by GUID. An
element naming an entry that was taken out meanwhile waits (above) and stays in the room.

## Two entries, one name

Two copies making a type of one name at once make two entries of two GUIDs. Of the entries of one kind asking for one name the
first by GUID keeps it, and the others become `Name (2)`, `Name (3)` in the order of their GUIDs, in every copy alike. The count is
of the name without a count of its own: two asking for `Brick (2)` end as `Brick (2)` and `Brick (3)`, never `Brick (2) (2)`. A copy
that had to rename an entry returns the key in `Merged::fixes`, and Arqiflow's page puts it in the room, so that a copy made from
the room later agrees (identical writes by every copy; there is nothing to loop over once the room says it).

## Changes made outside a room

The server's agent changes the stored project, not a page's model. What it did is `Engine.changes(before, after)` (`crates/web`, a host op of Arqiflow's engine host): the engine's own `sync::changes`, so the agent's library changes (a type, a rename) travel exactly as a page's do, with no second comparison written elsewhere. What the server keeps of a room is the same `merge`: what
it settles (`fixes`: a type in use that the room took out, a body left with no name) it tells the open room, as a page does, but a key
only if the room still holds what the server's save saw of it (`bim_live.say`): a copy may have edited the entry in the moment since,
and a fix made from the old body would write over that edit, or bring back what a copy took out meanwhile. A key dropped for that is
logged, and the room says what the others made.

## Phases

A phase's body is its place. The phases stand in the order of places, ties by name. A copy takes the place of a phase it is not
told of from its own order, so whoever tells a change of a phase tells every phase the room holds (Arqiflow's page does).

## Not here

Old whole maps (`materials: {…}`, a page on format 1): only a complete load takes one, the entries told by key overruling it.
Rename notes, `expand` and `follow(notes)` are gone; a page of another format is refused by Arqiflow's ticket.

## Tests

`sync_settings_tests.rs` has the cases that matter: rename with a concurrent edit, rename with a wall of the old name placed
meanwhile, a copy offline across a rename, two copies making one name, an old file read twice, the undo history across a rename.
`sync_room_tests.rs` puts a room between the copies (the last write of a key stands; each copy keeps what it last sent and heard,
as Arqiflow's page does) and takes the changes in different orders: a type taken out while a wall of it is placed (and the server's
copy; three copies and the server, with a wall that waits for a type that never comes), a type taken out while another edits it,
a bad key in a batch, a body left with no name, `Brick (2)` asked for twice, and the waiting queue (not refused, offered again only when
the library changes, given up on after 20 tries, held to 8 MiB).

//! Editing together, the settings entry by entry (`sync_settings`): two copies add and change different entries at once and
//! both keep theirs; the same entry changed by both is the last to be told; names with a slash travel; a whole room makes a
//! copy say every entry; what is hostile is refused.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::ids::Guid;
use crate::library::Kind;
use crate::model::{Element, Model};
use crate::sync::{self, Changes};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Two copies of one project, as two people's windows open it: read from the same file.
fn two() -> (Session, Session) {
    let mut s = Session::new();
    run(&mut s, "new", json!({}));
    let text = s.model.to_json().unwrap();
    let open = |t: &str| {
        let mut x = Session::new();
        x.reset(Model::read(t).unwrap().0, None);
        x
    };
    (open(&text), open(&text))
}

fn send_model(before: &Model, after: &Model, to: &mut Session) -> sync::Merged {
    let c = sync::changes(before, after).unwrap();
    // As the page sends it: JSON, read back strictly.
    let c = Changes::read(&c.to_json()).unwrap();
    to.merge(&c, &BTreeSet::new()).unwrap()
}

fn by_guid(m: &Model) -> BTreeMap<Guid, String> {
    m.elements
        .iter()
        .filter(|(_, e)| !sync::own(e))
        .map(|(id, e)| (m.elements.guid(*id).unwrap(), serde_json::to_string(e).unwrap()))
        .collect()
}

fn walls(m: &Model) -> usize {
    m.elements.values().filter(|e| matches!(e, Element::Wall(_))).count()
}

fn wall_types(m: &Model) -> Vec<String> {
    m.types.wall.keys().cloned().collect()
}

fn thickness(m: &Model, name: &str) -> f64 {
    m.types.wall.get(name).map(|t| t.thickness).unwrap_or(-1.0)
}

/// Both copies do something at once, then each sends the other what it did, as two pages do.
fn crossed(a: &mut Session, b: &mut Session, fa: impl FnOnce(&mut Session), fb: impl FnOnce(&mut Session)) {
    let (a0, b0) = (a.model.clone(), b.model.clone());
    fa(a);
    fb(b);
    let (a1, b1) = (a.model.clone(), b.model.clone());
    let got_b = send_model(&b0, &b1, a);
    let got_a = send_model(&a0, &a1, b);
    assert!(got_a.refused.is_empty() && got_b.refused.is_empty(), "{:?} {:?}", got_a.refused, got_b.refused);
}

/// `a`'s changes since `before`, in the copy `b`.
fn send(before: &Model, a: &Session, b: &mut Session) {
    let got = send_model(before, &a.model, b);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
}

#[test]
fn each_type_travels_on_its_own_key_so_a_change_names_only_what_changed() {
    let (mut a, _) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Party wall 250", "thickness": 250}));
    run(&mut a, "material", json!({"name": "Plaster", "color": "#eeeeee"}));
    let c = sync::changes(&before, &a.model).unwrap();
    let keys: Vec<&str> = c.project.keys().map(String::as_str).collect();
    assert_eq!(keys, ["materials/Plaster", "types/wall/Party wall 250"], "only the entries that changed");
    // Removing it is that key alone, none.
    let after = a.model.clone();
    run(&mut a, "delete_type", json!({"category": "wall", "type": "Party wall 250"}));
    let c = sync::changes(&after, &a.model).unwrap();
    assert_eq!(c.project.len(), 1, "{:?}", c.project.keys());
    assert_eq!(c.project.get("types/wall/Party wall 250"), Some(&None));
}

#[test]
fn two_people_adding_different_types_materials_and_text_types_at_once_both_keep_theirs() {
    let (mut a, mut b) = two();
    crossed(
        &mut a,
        &mut b,
        |a| {
            run(a, "type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
            run(a, "material", json!({"name": "Brick", "color": "#aa5533"}));
            run(a, "text_type", json!({"name": "Notes A", "size": 3.5}));
        },
        |b| {
            run(b, "type", json!({"category": "wall", "name": "Block 200", "thickness": 200}));
            run(b, "type", json!({"category": "door", "name": "Door 900", "width": 900, "height": 2100}));
            run(b, "material", json!({"name": "Block", "color": "#999999"}));
            run(b, "text_type", json!({"name": "Notes B", "size": 5}));
        },
    );
    for m in [&a.model, &b.model] {
        assert!(
            wall_types(m).contains(&"Brick 300".to_string()) && wall_types(m).contains(&"Block 200".to_string()),
            "{:?}",
            wall_types(m)
        );
        assert!(m.types.door.contains_key("Door 900"));
        assert!(m.materials.contains_key("Brick") && m.materials.contains_key("Block"));
        assert!(m.text_types.contains_key("Notes A") && m.text_types.contains_key("Notes B"));
        assert_eq!(m.check(), Ok(()));
    }
    assert_eq!(a.model.types, b.model.types);
    assert_eq!(a.model.materials, b.model.materials);
    assert_eq!(a.model.text_types, b.model.text_types);
}

#[test]
fn editing_different_entries_at_once_keeps_both_edits() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Alpha", "thickness": 200}));
    run(&mut a, "type", json!({"category": "wall", "name": "Beta", "thickness": 300}));
    send(&before, &a, &mut b);
    crossed(
        &mut a,
        &mut b,
        |a| {
            run(a, "type", json!({"category": "wall", "name": "Alpha", "thickness": 210}));
        },
        |b| {
            run(b, "type", json!({"category": "wall", "name": "Beta", "thickness": 310}));
        },
    );
    for m in [&a.model, &b.model] {
        assert_eq!((thickness(m, "Alpha"), thickness(m, "Beta")), (210.0, 310.0));
    }
    assert_eq!(a.model.types, b.model.types);
}

/// `m` with Alpha's thickness set.
fn b0_after(m: &Model, t: f64) -> Model {
    let mut x = m.clone();
    if let Some(w) = x.types.wall.get_mut("Alpha") {
        w.thickness = t;
    }
    x
}

#[test]
fn the_same_entry_edited_by_both_is_the_last_to_say_it_and_the_other_entries_are_not_touched() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Alpha", "thickness": 200}));
    run(&mut a, "type", json!({"category": "wall", "name": "Beta", "thickness": 300}));
    send(&before, &a, &mut b);
    let (a0, b0) = (a.model.clone(), b.model.clone());
    run(&mut a, "type", json!({"category": "wall", "name": "Alpha", "thickness": 250}));
    run(&mut b, "type", json!({"category": "wall", "name": "Alpha", "thickness": 260}));
    // What each says is that entry alone.
    let said = sync::changes(&a0, &a.model).unwrap();
    assert_eq!(said.project.keys().map(String::as_str).collect::<Vec<_>>(), ["types/wall/Alpha"]);
    // B says it later: a copy told both ends with the last it was told, whichever it is.
    send_model(&a0, &a.model.clone(), &mut b);
    assert_eq!(thickness(&b.model, "Alpha"), 250.0, "B was told A's after its own");
    send_model(&b0, &b0_after(&b0, 260.0), &mut a);
    assert_eq!(thickness(&a.model, "Alpha"), 260.0, "A was told B's after its own");
    assert_eq!(thickness(&a.model, "Beta"), 300.0);
    assert_eq!(thickness(&b.model, "Beta"), 300.0);
}

#[test]
fn an_entry_removed_by_one_and_edited_by_the_other_is_as_an_element_is() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Beta", "thickness": 300}));
    let wall = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].clone();
    send(&before, &a, &mut b);
    // An entry nobody uses to take away.
    let (a0, b0) = (a.model.clone(), b.model.clone());
    let wall_b = b.model.elements.id(a.model.elements.guid(wall.as_u64().unwrap()).unwrap()).unwrap();
    run(&mut a, "delete_type", json!({"category": "wall", "type": "Beta"}));
    run(&mut b, "type", json!({"category": "wall", "name": "Beta", "thickness": 320}));
    // The same for an element: A removes it, B moves it.
    run(&mut a, "delete", json!({"ids": [wall]}));
    run(&mut b, "move", json!({"ids": [wall_b], "by": [0, 100]}));
    let (a1, b1) = (a.model.clone(), b.model.clone());
    send_model(&b0, &b1, &mut a);
    send_model(&a0, &a1, &mut b);
    // A is told B's edit and has the entry (and the element) again; B is told the removal and has neither: each applies what
    // it is told, in the order told, for entries as for elements. (A shared room settles it for everyone: there the edit wins.)
    assert_eq!(thickness(&a.model, "Beta"), 320.0);
    assert_eq!(walls(&a.model), 1);
    assert!(!b.model.types.wall.contains_key("Beta"));
    assert_eq!(walls(&b.model), 0);
}

#[test]
fn walls_of_a_type_added_at_the_same_moment_find_their_types_in_both_copies() {
    let (mut a, mut b) = two();
    crossed(
        &mut a,
        &mut b,
        |a| {
            run(a, "type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
            run(a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Brick 300"}));
        },
        |b| {
            run(b, "type", json!({"category": "wall", "name": "Block 200", "thickness": 200}));
            run(b, "wall", json!({"points": [[0, 5000], [6000, 5000]], "type": "Block 200"}));
        },
    );
    for m in [&a.model, &b.model] {
        assert_eq!(walls(m), 2);
        assert_eq!(m.check(), Ok(()));
        for e in m.elements.values() {
            let Element::Wall(w) = e else { continue };
            let kind = Kind::Type("wall".into());
            // The type is held by GUID in a file, and the wall finds it by that GUID.
            let guid = m.library.guid(&kind, &w.type_name).expect("the type has a GUID here");
            assert_eq!(m.library_name(&kind, &guid.to_string()), Some(w.type_name.clone()));
        }
    }
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    // And a file of either reads back whole.
    let reread = Model::read(&a.model.to_json().unwrap()).unwrap().0;
    assert_eq!(reread.check(), Ok(()));
    assert_eq!(wall_types(&reread), wall_types(&a.model));
}

#[test]
fn a_name_with_a_slash_or_a_percent_travels_and_comes_back_as_it_was() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Wall 200/50 100%", "thickness": 250}));
    run(&mut a, "material", json!({"name": "A/B %2F", "color": "#123456"}));
    let c = sync::changes(&before, &a.model).unwrap();
    assert!(c.project.contains_key("types/wall/Wall 200%2F50 100%25"), "{:?}", c.project.keys());
    send(&before, &a, &mut b);
    assert_eq!(thickness(&b.model, "Wall 200/50 100%"), 250.0);
    assert!(b.model.materials.contains_key("A/B %2F"));
    assert_eq!(b.model.types, a.model.types);
}

#[test]
fn a_whole_room_makes_a_copy_say_every_entry_and_none_more() {
    let (mut a, mut b) = two();
    run(&mut a, "type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
    run(&mut a, "phase", json!({"name": "Phase A"}));
    // B has an entry the room doesn't, which it would lose.
    run(&mut b, "type", json!({"category": "wall", "name": "Only in B", "thickness": 100}));
    let all = sync::entries(&a.model).unwrap();
    assert!(all.complete && all.project.contains_key("types/wall/Brick 300") && all.project.contains_key("phases/Phase A"));
    let c = Changes::read(&all.to_json()).unwrap();
    let got = b.merge(&c, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(b.model.types, a.model.types);
    assert_eq!(b.model.phases, a.model.phases);
    assert_eq!(b.model.materials, a.model.materials);
    assert_eq!(b.model.check(), Ok(()));
    assert!(!b.model.types.wall.contains_key("Only in B"));
}

#[test]
fn a_map_emptied_stays_empty_in_a_whole_room() {
    let (mut a, mut b) = two();
    let mut m = (*a.model).clone();
    m.line_styles.clear();
    m.line_patterns.clear();
    let all = sync::entries(&m).unwrap();
    assert!(all.project.contains_key("line_patterns"), "an empty map is a key of its own");
    assert!(!b.model.line_patterns.is_empty());
    let got = b.merge(&all, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert!(b.model.line_patterns.is_empty());
    let _ = &mut a;
}

#[test]
fn hostile_setting_keys_are_refused_without_a_change() {
    let (mut a, _) = two();
    let before = a.model.clone();
    for k in ["library/x", "elements/x", "next_id/1", "library%2Fx", "elements%2Fx"] {
        let text = json!({"project": {k: "1"}}).to_string();
        assert!(Changes::read(&text).is_err(), "{k}");
    }
    for k in ["units/x", "types/wall", "types/nothing/x/y/z", "materials/a/b", "bogus/x"] {
        let c = Changes::read(&json!({"project": {k: "{}"}}).to_string()).unwrap();
        let got = a.merge(&c, &BTreeSet::new()).unwrap();
        assert!(!got.refused.is_empty(), "{k} is refused");
    }
    assert!(a.model.same_settings(&before));
}

/// A third copy of the same project.
fn third(a: &Session) -> Session {
    let mut x = Session::new();
    x.reset(Model::read(&a.model.to_json().unwrap()).unwrap().0, None);
    x
}

/// `c` told to `to`, as a page sends it (read back strictly).
fn told(c: &Changes, to: &mut Session) {
    let got = to.merge(&Changes::read(&c.to_json()).unwrap(), &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
}

/// `c` as a page tells it where it changes a phase: with every phase the room holds (`room`, its keys), so that the places are the
/// room's and not the order the copy happens to have them in.
fn told_with_the_room_phases(c: &Changes, room: &BTreeMap<String, Option<String>>, to: &mut Session) {
    let mut c = c.clone();
    for (k, v) in room.iter().filter(|(k, _)| k.starts_with("phases/")) {
        c.project.entry(k.clone()).or_insert_with(|| v.clone());
    }
    told(&c, to);
}

#[test]
fn phases_added_at_the_same_moment_end_in_one_order_whichever_order_they_are_told_in() {
    let (mut a, mut b) = two();
    let mut c = third(&a);
    let base = a.model.clone();
    run(&mut a, "phase", json!({"name": "Phase A"}));
    run(&mut b, "phase", json!({"name": "Phase B"}));
    run(&mut c, "phase", json!({"name": "Phase C"}));
    let (ca, cb, cc) =
        (sync::changes(&base, &a.model).unwrap(), sync::changes(&base, &b.model).unwrap(), sync::changes(&base, &c.model).unwrap());
    // No list of the order is sent: each phase says its own place, so there is no list for the last writer to win.
    assert!(!ca.project.contains_key("phases") && ca.project.contains_key("phases/Phase A"), "{:?}", ca.project.keys());
    // What the room holds of the phases: the base's, and the three added. Each copy is told the other two, in an order of its own.
    let mut room = sync::entries(&base).unwrap();
    for x in [&ca, &cb, &cc] {
        room.project.extend(x.project.clone());
    }
    let places = room.project.clone();
    told_with_the_room_phases(&cc, &places, &mut a);
    told_with_the_room_phases(&cb, &places, &mut a);
    told_with_the_room_phases(&ca, &places, &mut b);
    told_with_the_room_phases(&cc, &places, &mut b);
    told_with_the_room_phases(&cb, &places, &mut c);
    told_with_the_room_phases(&ca, &places, &mut c);
    let want: Vec<String> = ["Existing", "New Construction", "Phase A", "Phase B", "Phase C"].map(String::from).to_vec();
    assert_eq!(a.model.phases, want);
    assert_eq!(b.model.phases, want);
    assert_eq!(c.model.phases, want);
    // And a copy made from the whole room (the base and the three's entries together) agrees.
    let mut d = third(&a);
    told(&room, &mut d);
    assert_eq!(d.model.phases, want);
    assert_eq!(a.model.check(), Ok(()));
}

#[test]
fn deleting_a_phase_and_adding_one_in_the_middle_keep_the_order_of_the_rest() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "phase", json!({"name": "Phase A"}));
    run(&mut a, "phase", json!({"name": "Phase B"}));
    send(&before, &a, &mut b);
    assert_eq!(a.model.phases, b.model.phases);
    // Deleting one is that entry alone, and the order of the rest stays.
    let before = a.model.clone();
    run(&mut a, "phase", json!({"name": "Phase A", "delete": true, "into": "New Construction"}));
    send(&before, &a, &mut b);
    assert_eq!(a.model.phases, b.model.phases);
    assert!(!b.model.phases.contains(&"Phase A".to_string()) && b.model.phases.contains(&"Phase B".to_string()));
    // A phase added in the middle takes its place there.
    let before = a.model.clone();
    run(&mut a, "phase", json!({"name": "Phase C", "before": "New Construction"}));
    send(&before, &a, &mut b);
    assert_eq!(a.model.phases, b.model.phases);
    assert_eq!(b.model.phases.iter().position(|p| p == "Phase C"), Some(1));
}

/// The materials of `m` as an old page sent them: one whole map.
fn whole_materials(m: &Model) -> Changes {
    let mut project = BTreeMap::new();
    project.insert("materials".to_string(), Some(serde_json::to_string(&m.materials).unwrap()));
    Changes { project, ..Changes::default() }
}

#[test]
fn an_old_whole_map_never_brings_a_deleted_entry_back_in_a_change() {
    let (mut a, mut b) = two();
    run(&mut a, "material", json!({"name": "Gone", "color": "#010101"}));
    run(&mut a, "material", json!({"name": "Kept", "color": "#020202"}));
    let old_page = whole_materials(&a.model);
    send(&b.model.clone(), &a, &mut b);
    let before = a.model.clone();
    run(&mut a, "material", json!({"name": "Gone", "delete": true}));
    send(&before, &a, &mut b);
    assert!(!b.model.materials.contains_key("Gone"));
    // A page still on the old engine put its whole map in the room: every copy, and the server at every save, is told it
    // again; none of it takes an entry back.
    for _ in 0..2 {
        for s in [&mut a, &mut b] {
            let got = s.merge(&old_page, &BTreeSet::new()).unwrap();
            assert!(got.refused.is_empty(), "{:?}", got.refused);
            assert!(!s.model.materials.contains_key("Gone") && s.model.materials.contains_key("Kept"));
        }
    }
}

#[test]
fn an_old_whole_map_is_taken_in_a_whole_room_and_the_entries_told_overrule_it() {
    let (mut a, mut b) = two();
    run(&mut a, "material", json!({"name": "Mine", "color": "#010101"}));
    let theirs = a.model.materials.get("Mine").cloned().unwrap();
    let mut old = (*a.model).clone();
    old.materials.insert("Theirs".to_string(), theirs.clone());
    old.materials.insert("Mine".to_string(), theirs);
    // The room: everything a copy says, but its materials are an old page's whole map, and an entry of its own that overrules it.
    run(&mut a, "material", json!({"name": "Mine", "color": "#999999"}));
    let mut room = sync::entries(&a.model).unwrap();
    room.project.retain(|k, _| !k.starts_with("materials/"));
    room.project.extend(whole_materials(&old).project);
    room.project.insert("materials/Mine".into(), Some(serde_json::to_string(a.model.materials.get("Mine").unwrap()).unwrap()));
    told(&room, &mut b);
    assert!(b.model.materials.contains_key("Theirs") && b.model.materials.len() == old.materials.len());
    assert_eq!(b.model.materials.get("Mine"), a.model.materials.get("Mine"), "the entry told is the later");
}

#[test]
fn a_whole_map_is_made_entries_for_a_room_that_keeps_it_and_its_whole_key_goes() {
    let (mut a, _) = two();
    run(&mut a, "material", json!({"name": "Alpha", "color": "#010101"}));
    run(&mut a, "type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
    let mut room: BTreeMap<String, String> = BTreeMap::new();
    room.insert("materials".into(), serde_json::to_string(&a.model.materials).unwrap());
    room.insert("types".into(), serde_json::to_string(&a.model.types).unwrap());
    // An entry the room has says its own; a map with no entries and a single-valued setting are left alone.
    room.insert("materials/Alpha".into(), "{}".into());
    room.insert("line_patterns".into(), "{}".into());
    room.insert("units".into(), "{}".into());
    let out = crate::sync_settings::expand(&room).unwrap();
    assert_eq!(out.get("materials"), Some(&None));
    assert_eq!(out.get("types"), Some(&None));
    assert!(out.get("types/wall/Brick 300").is_some_and(Option::is_some));
    assert!(!out.contains_key("materials/Alpha"), "the entry the room has is not put over");
    assert!(!out.contains_key("line_patterns") && !out.contains_key("units"));
    // The entries it makes are what a copy takes as a change.
    let project: BTreeMap<String, Option<String>> = out.into_iter().filter(|(_, v)| v.is_some()).collect();
    let (mut b, _) = two();
    told(&Changes { project, ..Changes::default() }, &mut b);
    assert!(b.model.types.wall.contains_key("Brick 300"));
}

#[test]
fn the_sync_format_is_a_number_a_room_and_a_page_compare() {
    assert_eq!(sync::FORMAT, 2);
}

/// What a change says under `key`, a JSON text.
fn said(c: &Changes, key: &str) -> Option<String> {
    c.project.get(key).cloned().flatten()
}

#[test]
fn a_rename_travels_with_a_note_naming_the_old_and_the_new() {
    let (mut a, _) = two();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    run(&mut a, "material", json!({"name": "Clay", "color": "#aa5533"}));
    let before = a.model.clone();
    run(&mut a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
    run(&mut a, "material", json!({"name": "Clay", "rename": "Terracotta"}));
    let c = sync::changes(&before, &a.model).unwrap();
    assert_eq!(said(&c, "renames/types/wall/Old"), Some("\"New\"".into()), "{:?}", c.project.keys());
    assert_eq!(said(&c, "renames/materials/Clay"), Some("\"Terracotta\"".into()));
    // A type only removed and another only added are no rename.
    let before = a.model.clone();
    run(&mut a, "delete_type", json!({"category": "wall", "type": "New"}));
    run(&mut a, "type", json!({"category": "wall", "name": "Else", "thickness": 100}));
    let c = sync::changes(&before, &a.model).unwrap();
    assert!(!c.project.keys().any(|k| k.starts_with("renames/")), "{:?}", c.project.keys());
    // A model of the undo history is told what changed, not the notes (they are for the others' copies).
    let mut with = Changes::default();
    with.project.insert("renames/types/wall/Old".into(), Some("\"New\"".into()));
    with.project.insert("types/wall/New".into(), Some("{}".into()));
    assert_eq!(with.without_notes().project.keys().collect::<Vec<_>>(), ["types/wall/New"]);
}

#[test]
fn a_rename_by_one_and_an_edit_of_the_old_name_by_another_end_as_one_entry_with_the_edit() {
    let (mut a, mut b) = two();
    let base = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    send(&base, &a, &mut b);
    let (a0, b0) = (a.model.clone(), b.model.clone());
    run(&mut a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
    run(&mut b, "type", json!({"category": "wall", "name": "Old", "thickness": 260}));
    let (from_a, from_b) = (sync::changes(&a0, &a.model).unwrap(), sync::changes(&b0, &b.model).unwrap());
    let note = from_a.project["renames/types/wall/Old"].clone();
    // The room has B's edit (an edit wins over a removal), A's New, and A's note. A is told B's edit (the note stands in the
    // room); B is told what A did, and not the removal that its edit beat.
    let mut to_a = Changes::default();
    to_a.project.insert("types/wall/Old".into(), from_b.project["types/wall/Old"].clone());
    to_a.project.insert("renames/types/wall/Old".into(), note.clone());
    let mut to_b = Changes::default();
    to_b.project.insert("types/wall/New".into(), from_a.project["types/wall/New"].clone());
    to_b.project.insert("renames/types/wall/Old".into(), note.clone());
    told(&to_a, &mut a);
    told(&to_b, &mut b);
    for m in [&a.model, &b.model] {
        assert!(!m.types.wall.contains_key("Old"), "{:?}", wall_types(m));
        assert_eq!(thickness(m, "New"), 260.0, "the edit lands on the new name");
        assert_eq!(m.check(), Ok(()));
    }
    assert_eq!(a.model.types, b.model.types);
    // A copy made from the whole room (Old with the edit, New, and the note) says the same.
    let mut whole = sync::entries(&base).unwrap();
    whole.project.remove("types/wall/Old");
    whole.project.insert("types/wall/Old".into(), from_b.project["types/wall/Old"].clone());
    whole.project.insert("types/wall/New".into(), from_a.project["types/wall/New"].clone());
    whole.project.insert("renames/types/wall/Old".into(), note);
    let mut d = third(&a);
    told(&whole, &mut d);
    assert_eq!(d.model.types, a.model.types);
}

#[test]
fn an_element_of_the_old_name_placed_while_it_was_renamed_follows_the_rename_in_every_copy() {
    let (mut a, mut b) = two();
    let base = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    send(&base, &a, &mut b);
    let (a0, b0) = (a.model.clone(), b.model.clone());
    run(&mut a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
    let wall = run(&mut b, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Old"}))["ids"][0].as_u64().unwrap();
    let guid = b.model.elements.guid(wall).unwrap();
    let from_a = sync::changes(&a0, &a.model).unwrap();
    let from_b = sync::changes(&b0, &b.model).unwrap();
    assert_eq!(from_b.elements.len(), 1);
    // B has not seen the rename: its wall is its own change, not sent yet (`keep`), and the rename comes to it.
    let got = b.merge(&from_a, &BTreeSet::from([guid])).unwrap();
    assert!(got.refused.is_empty(), "the rename is taken and B's wall follows it: {:?}", got.refused);
    // A is told the wall, and the note stands in the room.
    let mut to_a = from_b.clone();
    to_a.project.insert("renames/types/wall/Old".into(), from_a.project["renames/types/wall/Old"].clone());
    let got = a.merge(&to_a, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "A takes the wall: {:?}", got.refused);
    for m in [&a.model, &b.model] {
        assert_eq!(walls(m), 1, "the wall is in every copy");
        let Some(Element::Wall(w)) = m.elements.get(guid) else { panic!("no wall") };
        assert_eq!(w.type_name, "New");
        assert!(!m.types.wall.contains_key("Old"));
        assert_eq!(m.check(), Ok(()));
    }
    // What B sends next is its wall of the new name (the baseline was told the rename without its note).
    let baseline = b0.with_changes(&from_a.without_notes(), &BTreeMap::new()).unwrap();
    assert!(sync::changes(&baseline, &b.model).unwrap().elements.contains_key(&guid));
}

#[test]
fn a_note_that_names_no_entry_is_refused_without_a_change() {
    let (mut a, _) = two();
    let before = a.model.clone();
    for (k, v) in [
        ("renames/units", json!("x")),
        ("renames/types/wall", json!("x")),
        ("renames/bogus/x", json!("x")),
        ("renames/materials/Clay", json!(7)),
    ] {
        let c = Changes::read(&json!({"project": {k: v.to_string()}}).to_string()).unwrap();
        assert!(!a.merge(&c, &BTreeSet::new()).unwrap().refused.is_empty(), "{k}");
    }
    assert!(a.model.same_settings(&before));
}

#[test]
fn the_undo_history_keeps_a_rename_without_its_note() {
    let (mut a, _) = two();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    run(&mut a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
    let h = a.kept_history().unwrap();
    let keys: Vec<&String> = h.undo.iter().flat_map(|s| s.changes.project.keys()).chain(h.now.project.keys()).collect();
    assert!(!keys.is_empty() && !keys.iter().any(|k| k.starts_with("renames/")), "{keys:?}");
    // And it goes back on the project opened again.
    let mut again = third(&a);
    assert!(again.restore_history(&h).is_ok());
}

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
        assert!(wall_types(m).contains(&"Brick 300".to_string()) && wall_types(m).contains(&"Block 200".to_string()), "{:?}", wall_types(m));
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
    // B's edit is the later one: A's reaches B first, and B's reaches A last.
    send_model(&a0, &a.model.clone(), &mut b);
    send_model(&b0, &b.model.clone(), &mut a);
    assert_eq!(thickness(&a.model, "Alpha"), 260.0, "the last edit to reach a copy is the entry's");
    assert_eq!(thickness(&b.model, "Alpha"), 260.0);
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
fn phases_added_at_the_same_moment_are_both_kept_in_one_order() {
    let (mut a, mut b) = two();
    crossed(
        &mut a,
        &mut b,
        |a| {
            run(a, "phase", json!({"name": "Phase A"}));
        },
        |b| {
            run(b, "phase", json!({"name": "Phase B"}));
        },
    );
    assert!(a.model.phases.contains(&"Phase A".to_string()) && a.model.phases.contains(&"Phase B".to_string()), "{:?}", a.model.phases);
    assert_eq!(a.model.phases, b.model.phases);
    assert_eq!(a.model.check(), Ok(()));
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
    assert_eq!(b.model.phases.iter().position(|p| p == "Phase C"), a.model.phases.iter().position(|p| p == "Phase C"));
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
    assert!(all.complete && all.project.contains_key("types/wall/Brick 300") && all.project.contains_key("phases"));
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
    m.line_patterns.clear();
    let all = sync::entries(&m).unwrap();
    assert!(all.project.contains_key("line_patterns"), "an empty map is a key of its own");
    assert!(!b.model.line_patterns.is_empty());
    b.merge(&all, &BTreeSet::new()).unwrap();
    assert!(b.model.line_patterns.is_empty());
    let _ = &mut a;
}

#[test]
fn a_whole_map_from_before_this_puts_its_entries_in_and_takes_none_out() {
    let (mut a, _) = two();
    run(&mut a, "material", json!({"name": "Mine", "color": "#010101"}));
    let mut other = a.model.materials.clone();
    let theirs = other.get("Mine").cloned().unwrap();
    other.clear();
    other.insert("Theirs".to_string(), theirs);
    let mut project = BTreeMap::new();
    project.insert("materials".to_string(), Some(serde_json::to_string(&*other).unwrap()));
    let got = a.merge(&Changes { project, ..Changes::default() }, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert!(a.model.materials.contains_key("Theirs") && a.model.materials.contains_key("Mine"));
}

#[test]
fn hostile_setting_keys_are_refused_without_a_change() {
    let (mut a, _) = two();
    let before = a.model.clone();
    for k in ["library/x", "elements/x", "next_id/1"] {
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

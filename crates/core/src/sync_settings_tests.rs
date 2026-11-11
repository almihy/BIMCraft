//! Editing together, the settings entry by entry (`sync_settings`): two copies add and change different entries at once and
//! both keep theirs; entries are told by GUID, a rename as a name of its own, so a rename and an edit of one entry both stand; two
//! entries made with one name at once end as `Name` and `Name (2)` in every copy; a whole room makes a copy say every entry;
//! what is hostile is refused.
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

/// The GUID of the wall type `name` in `m`.
fn wall_guid(m: &Model, name: &str) -> Guid {
    m.library.guid(&Kind::Type("wall".into()), name).unwrap_or_else(|| panic!("no wall type {name}"))
}

/// The key of the wall type `name`'s body in `m`, and of its name.
fn wall_key(m: &Model, name: &str) -> String {
    format!("types/wall/{}", wall_guid(m, name))
}

fn name_key(m: &Model, name: &str) -> String {
    format!("{}/name", wall_key(m, name))
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
fn each_entry_travels_on_two_keys_of_its_guid_a_body_and_a_name() {
    let (mut a, _) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Party wall 250", "thickness": 250}));
    run(&mut a, "material", json!({"name": "Lime plaster", "color": "#eeeeee"}));
    let c = sync::changes(&before, &a.model).unwrap();
    let wall = wall_guid(&a.model, "Party wall 250");
    let plaster = a.model.library.guid(&Kind::Material, "Lime plaster").unwrap();
    let mut keys: Vec<String> = c.project.keys().cloned().collect();
    keys.sort();
    let mut want = vec![
        format!("materials/{plaster}"),
        format!("materials/{plaster}/name"),
        format!("types/wall/{wall}"),
        format!("types/wall/{wall}/name"),
    ];
    want.sort();
    assert_eq!(keys, want, "only the entries that changed");
    assert_eq!(c.project[&format!("types/wall/{wall}/name")], Some("\"Party wall 250\"".into()));
    // Removing it is its two keys, none.
    let after = a.model.clone();
    run(&mut a, "delete_type", json!({"category": "wall", "type": "Party wall 250"}));
    let c = sync::changes(&after, &a.model).unwrap();
    assert_eq!(c.project.len(), 2, "{:?}", c.project.keys());
    assert_eq!(c.project.get(&format!("types/wall/{wall}")), Some(&None));
    assert_eq!(c.project.get(&format!("types/wall/{wall}/name")), Some(&None));
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
    // And the entries are the same entries in both copies: each has the GUID the copy that made it gave it.
    for (kind, name) in [(Kind::Type("wall".into()), "Brick 300"), (Kind::Material, "Block"), (Kind::TextType, "Notes B")] {
        assert_eq!(a.model.library.guid(&kind, name), b.model.library.guid(&kind, name), "{name}");
        assert!(a.model.library.guid(&kind, name).is_some());
    }
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
    // What each says is that entry's body alone: its name did not change.
    let said = sync::changes(&a0, &a.model).unwrap();
    assert_eq!(said.project.keys().cloned().collect::<Vec<_>>(), [wall_key(&a0, "Alpha")]);
    // B says it later: a copy told both ends with the last it was told, whichever it is.
    send_model(&a0, &a.model.clone(), &mut b);
    assert_eq!(thickness(&b.model, "Alpha"), 250.0, "B was told A's after its own");
    send_model(&b0, &b0_after(&b0, 260.0), &mut a);
    assert_eq!(thickness(&a.model, "Alpha"), 260.0, "A was told B's after its own");
    assert_eq!(thickness(&a.model, "Beta"), 300.0);
    assert_eq!(thickness(&b.model, "Beta"), 300.0);
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
    // Names are values, not parts of keys: no key has a slash of a name in it.
    assert!(c.project.keys().all(|k| !k.contains("200")), "{:?}", c.project.keys());
    assert_eq!(c.project[&name_key(&a.model, "Wall 200/50 100%")], Some("\"Wall 200/50 100%\"".into()));
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
    let phase = a.model.library.guid(&Kind::Phase, "Phase A").unwrap();
    assert!(
        all.complete && all.project.contains_key(&wall_key(&a.model, "Brick 300")) && all.project.contains_key(&format!("phases/{phase}"))
    );
    let c = Changes::read(&all.to_json()).unwrap();
    let got = b.merge(&c, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(b.model.types, a.model.types);
    assert_eq!(b.model.phases, a.model.phases);
    assert_eq!(b.model.materials, a.model.materials);
    assert_eq!(b.model.check(), Ok(()));
    assert!(!b.model.types.wall.contains_key("Only in B"));
    assert_eq!(b.model.library.guid(&Kind::Phase, "Phase A"), Some(phase), "the room's GUIDs are the copy's now");
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
    let g = Guid::new();
    for k in [
        "units/x".to_string(),
        "types/wall".to_string(),
        "types/nothing/x/y/z".to_string(),
        "materials/a/b".to_string(),
        "bogus/x".to_string(),
        "materials/Concrete".to_string(),
        format!("materials/{g}/other"),
        format!("types/runs/nothing/{g}"),
        format!("types/nothing/{g}"),
    ] {
        let c = Changes::read(&json!({"project": {k.clone(): "{}"}}).to_string()).unwrap();
        let got = a.merge(&c, &BTreeSet::new()).unwrap();
        assert!(!got.refused.is_empty(), "{k} is refused");
    }
    // A body of the wrong shape, a name that is not text, a phase with no place, and a GUID that is another kind's here.
    let wall = wall_guid(&a.model, &wall_types(&a.model)[0]);
    for (k, v) in [
        (format!("materials/{g}"), json!(7)),
        (format!("materials/{g}/name"), json!(7)),
        (format!("phases/{g}"), json!({"entry": "x"})),
        (format!("materials/{wall}"), json!({"entry": {"color": [1, 2, 3]}})),
    ] {
        let c = Changes::read(&json!({"project": {k.clone(): v.to_string()}}).to_string()).unwrap();
        let got = a.merge(&c, &BTreeSet::new()).unwrap();
        assert!(!got.refused.is_empty(), "{k} is refused");
    }
    assert!(a.model.same_settings(&before));
    // An entry with a body and no name that this copy does not have (what is left of one taken out while another copy changed it)
    // makes no entry, and is no refusal; a name alone makes none either.
    for (k, v) in [(format!("materials/{g}"), json!({"entry": {"color": [1, 2, 3]}})), (format!("materials/{g}/name"), json!("Ghost"))] {
        let c = Changes::read(&json!({"project": {k.clone(): v.to_string()}}).to_string()).unwrap();
        let got = a.merge(&c, &BTreeSet::new()).unwrap();
        assert!(got.refused.is_empty(), "{k}: {:?}", got.refused);
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
    let phase_a = a.model.library.guid(&Kind::Phase, "Phase A").unwrap();
    assert!(!ca.project.contains_key("phases") && ca.project.contains_key(&format!("phases/{phase_a}")), "{:?}", ca.project.keys());
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
    // A whole map told as a change again and again takes nothing back.
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
    // The room: everything a copy says, but its materials are a whole map, and an entry of its own that overrules it.
    run(&mut a, "material", json!({"name": "Mine", "color": "#999999"}));
    let mut room = sync::entries(&a.model).unwrap();
    let mine = a.model.library.guid(&Kind::Material, "Mine").unwrap();
    let entry: BTreeMap<String, Option<String>> =
        room.project.iter().filter(|(k, _)| k.starts_with(&format!("materials/{mine}"))).map(|(k, v)| (k.clone(), v.clone())).collect();
    assert_eq!(entry.len(), 2);
    room.project.retain(|k, _| !k.starts_with("materials/"));
    room.project.extend(whole_materials(&old).project);
    room.project.extend(entry);
    told(&room, &mut b);
    assert!(b.model.materials.contains_key("Theirs") && b.model.materials.len() == old.materials.len());
    assert_eq!(b.model.materials.get("Mine"), a.model.materials.get("Mine"), "the entry told is the later");
    assert_eq!(b.model.library.guid(&Kind::Material, "Mine"), Some(mine), "and has its GUID");
}

#[test]
fn the_sync_format_is_a_number_a_room_and_a_page_compare() {
    assert_eq!(sync::FORMAT, 3);
}

#[test]
fn a_rename_is_the_name_key_alone_and_no_element_says_otherwise() {
    let (mut a, _) = two();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    run(&mut a, "material", json!({"name": "Clay", "color": "#aa5533"}));
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Old"}));
    let before = a.model.clone();
    run(&mut a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
    run(&mut a, "material", json!({"name": "Clay", "rename": "Terracotta"}));
    let c = sync::changes(&before, &a.model).unwrap();
    let mut keys: Vec<String> = c.project.keys().cloned().collect();
    keys.sort();
    let mut want =
        vec![name_key(&a.model, "New"), format!("materials/{}/name", a.model.library.guid(&Kind::Material, "Terracotta").unwrap())];
    want.sort();
    assert_eq!(keys, want, "{keys:?}");
    assert_eq!(c.project[&name_key(&a.model, "New")], Some("\"New\"".into()));
    assert!(c.elements.is_empty(), "the wall of the renamed type says what it did: {:?}", c.elements.keys());
    // A type only removed and another only added are no rename: the one's two keys taken away, the other's two put in.
    run(&mut a, "type", json!({"category": "wall", "name": "Else", "thickness": 100}));
    let before = a.model.clone();
    let was = wall_guid(&a.model, "Else");
    run(&mut a, "delete_type", json!({"category": "wall", "type": "Else"}));
    run(&mut a, "type", json!({"category": "wall", "name": "Else", "thickness": 100}));
    let c = sync::changes(&before, &a.model).unwrap();
    assert_ne!(wall_guid(&a.model, "Else"), was);
    assert_eq!(c.project.len(), 4, "{:?}", c.project.keys());
    assert_eq!(c.project.get(&format!("types/wall/{was}")), Some(&None));
    assert!(c.project.get(&wall_key(&a.model, "Else")).is_some_and(Option::is_some));
}

#[test]
fn a_rename_by_one_and_an_edit_of_the_same_entry_by_another_end_as_one_entry_with_both() {
    let (mut a, mut b) = two();
    let base = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    send(&base, &a, &mut b);
    let guid = wall_guid(&a.model, "Old");
    crossed(
        &mut a,
        &mut b,
        |a| {
            run(a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
        },
        |b| {
            run(b, "type", json!({"category": "wall", "name": "Old", "thickness": 260}));
        },
    );
    for m in [&a.model, &b.model] {
        assert_eq!(wall_types(m).iter().filter(|n| ["Old", "New"].contains(&n.as_str())).count(), 1, "{:?}", wall_types(m));
        assert!(!m.types.wall.contains_key("Old"));
        assert_eq!(thickness(m, "New"), 260.0, "the rename and the edit both stand");
        assert_eq!(wall_guid(m, "New"), guid);
        assert_eq!(m.check(), Ok(()));
    }
    assert_eq!(a.model.types, b.model.types);
    // The same in a shared room, which keeps the last write of each key: the name from A, the body from B.
    let mut room = sync::entries(&base).unwrap();
    let (from_a, from_b) = (sync::changes(&base, &a.model).unwrap(), sync::changes(&base, &b.model).unwrap());
    let _ = from_b;
    room.project.extend(from_a.project.clone().into_iter().filter(|(k, _)| k.ends_with("/name")));
    room.project.extend(from_a.project.into_iter().filter(|(k, _)| !k.ends_with("/name")));
    let mut d = third(&a);
    told(&room, &mut d);
    assert_eq!(d.model.types, a.model.types);
}

#[test]
fn a_wall_of_the_old_name_placed_while_the_type_was_renamed_ends_with_the_new_type_everywhere() {
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
    // The wall names its type by GUID, so what B says of it does not depend on what the type is called.
    let text = from_b.elements[&guid].clone().unwrap();
    assert!(text.contains(&wall_guid(&b.model, "Old").to_string()) && !text.contains("\"Old\""), "{text}");
    // B has not seen the rename: its wall is its own change, not sent yet (`keep`), and the rename comes to it.
    let got = b.merge(&from_a, &BTreeSet::from([guid])).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    let got = a.merge(&from_b, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "A takes the wall: {:?}", got.refused);
    for m in [&a.model, &b.model] {
        assert_eq!(walls(m), 1, "the wall is in every copy");
        let Some(Element::Wall(w)) = m.elements.get(guid) else { panic!("no wall") };
        assert_eq!(w.type_name, "New");
        assert!(!m.types.wall.contains_key("Old"));
        assert_eq!(m.check(), Ok(()));
    }
    // What B says next of its wall is the same: there is nothing to say again.
    let baseline = b0.with_changes(&from_a, &BTreeMap::new()).unwrap();
    assert!(!sync::changes(&baseline, &b.model).unwrap().elements.contains_key(&guid) || baseline.elements.get(guid).is_none());
}

#[test]
fn a_copy_away_through_renames_takes_them_in_and_its_walls_follow() {
    let (mut a, mut b) = two();
    let base = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    send(&base, &a, &mut b);
    let (a0, b0) = (a.model.clone(), b.model.clone());
    // A is away (a page that lost its connection, however long) and goes on drawing in the type.
    for y in [0, 3000, 6000] {
        run(&mut a, "wall", json!({"points": [[0, y], [6000, y]], "type": "Old"}));
    }
    // Meanwhile the type is renamed twice and edited.
    run(&mut b, "rename_type", json!({"category": "wall", "type": "Old", "name": "Middle"}));
    run(&mut b, "type", json!({"category": "wall", "name": "Middle", "thickness": 240}));
    run(&mut b, "rename_type", json!({"category": "wall", "type": "Middle", "name": "Final"}));
    let (a1, b1) = (a.model.clone(), b.model.clone());
    // It comes back: what it drew goes out, and what the room did meanwhile comes in.
    let got_b = send_model(&a0, &a1, &mut b);
    let got_a = send_model(&b0, &b1, &mut a);
    assert!(got_a.refused.is_empty() && got_b.refused.is_empty(), "{:?} {:?}", got_a.refused, got_b.refused);
    for m in [&a.model, &b.model] {
        assert_eq!(walls(m), 3);
        assert_eq!(wall_types(m).iter().filter(|n| ["Old", "Middle", "Final"].contains(&n.as_str())).collect::<Vec<_>>(), ["Final"]);
        assert_eq!(thickness(m, "Final"), 240.0);
        for e in m.elements.values() {
            if let Element::Wall(w) = e {
                assert_eq!(w.type_name, "Final");
            }
        }
        assert_eq!(m.check(), Ok(()));
    }
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
}

#[test]
fn two_types_made_with_one_name_at_once_end_as_name_and_name_2_in_every_copy() {
    let (mut a, mut b) = two();
    let mut c = third(&a);
    let base = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Plain", "thickness": 200}));
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Plain"}));
    run(&mut b, "type", json!({"category": "wall", "name": "Plain", "thickness": 300}));
    run(&mut b, "wall", json!({"points": [[0, 5000], [6000, 5000]], "type": "Plain"}));
    let (ga, gb) = (wall_guid(&a.model, "Plain"), wall_guid(&b.model, "Plain"));
    assert_ne!(ga, gb, "two copies made two entries");
    let (first, later) = if ga < gb { (ga, gb) } else { (gb, ga) };
    let (from_a, from_b) = (sync::changes(&base, &a.model).unwrap(), sync::changes(&base, &b.model).unwrap());
    let got_b = a.merge(&Changes::read(&from_b.to_json()).unwrap(), &BTreeSet::new()).unwrap();
    let got_a = b.merge(&Changes::read(&from_a.to_json()).unwrap(), &BTreeSet::new()).unwrap();
    assert!(got_a.refused.is_empty() && got_b.refused.is_empty(), "{:?} {:?}", got_a.refused, got_b.refused);
    // A third copy is told both, in the other order.
    told(&from_b, &mut c);
    told(&from_a, &mut c);
    for m in [&a.model, &b.model, &c.model] {
        assert_eq!(wall_guid(m, "Plain"), first, "the first GUID keeps the name");
        assert_eq!(wall_guid(m, "Plain (2)"), later, "the later is Plain (2)");
        assert_eq!(walls(m), 2);
        assert_eq!(m.check(), Ok(()));
    }
    assert_eq!(a.model.types, b.model.types);
    assert_eq!(b.model.types, c.model.types);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(by_guid(&b.model), by_guid(&c.model));
    // Each wall is of the entry its copy made it of, whatever it is called now.
    for m in [&a.model, &b.model] {
        for e in m.elements.values() {
            let Element::Wall(w) = e else { continue };
            let t = m.library.guid(&Kind::Type("wall".into()), &w.type_name).unwrap();
            assert!([ga, gb].contains(&t));
        }
    }
    // The copies that renamed an entry tell the room what it is called now, and told it, there is nothing more to say.
    let key = format!("types/wall/{later}/name");
    for got in [&got_a, &got_b] {
        assert_eq!(got.fixes.get(&key), Some(&Some("\"Plain (2)\"".to_string())), "{:?}", got.fixes);
    }
    let fix = Changes { project: got_a.fixes.clone(), ..Changes::default() };
    for s in [&mut a, &mut b, &mut c] {
        let got = s.merge(&Changes::read(&fix.to_json()).unwrap(), &BTreeSet::new()).unwrap();
        assert!(got.refused.is_empty() && got.fixes.is_empty(), "{:?} {:?}", got.refused, got.fixes);
    }
    // And a copy made from the whole room, with the fix in it, agrees.
    let mut room = sync::entries(&base).unwrap();
    room.project.extend(from_a.project.clone());
    room.project.extend(from_b.project.clone());
    room.elements.extend(from_a.elements.clone());
    room.elements.extend(from_b.elements.clone());
    room.project.extend(fix.project.clone());
    let mut d = third(&a);
    told(&room, &mut d);
    assert_eq!(d.model.types, a.model.types);
    assert_eq!(by_guid(&d.model), by_guid(&a.model));
}

#[test]
fn a_file_from_before_the_library_read_in_two_copies_gives_the_same_guids() {
    let (a, _) = two();
    let mut s = Session::new();
    run(&mut s, "new", json!({}));
    run(&mut s, "type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Brick 300"}));
    run(&mut s, "phase", json!({"name": "Phase A"}));
    // As a file from before the library has it: names, and no library.
    let Value::Object(mut file) = serde_json::to_value(&*s.model).unwrap() else { panic!("not an object") };
    file.remove("library");
    let text = Value::Object(file).to_string();
    let (one, other) = (Model::read(&text).unwrap().0, Model::read(&text).unwrap().0);
    let entries = |m: &Model| m.library.entries().map(|(_, e)| (e.kind.clone(), e.name.clone(), e.guid)).collect::<BTreeSet<_>>();
    assert!(!entries(&one).is_empty());
    assert_eq!(entries(&one), entries(&other), "the same file reads the same");
    assert!(entries(&one).iter().all(|(_, _, g)| !g.is_nil() && g.pending().is_none()));
    // The parts they hold (a layer of a type) agree too, and so what the copies tell each other.
    assert_eq!(
        one.library.parts().map(|(_, p)| p.guid).collect::<BTreeSet<_>>(),
        other.library.parts().map(|(_, p)| p.guid).collect::<BTreeSet<_>>()
    );
    let mut x = Session::new();
    x.reset(one, None);
    let mut y = Session::new();
    y.reset(other, None);
    assert_eq!(sync::entries(&x.model).unwrap(), sync::entries(&y.model).unwrap());
    // A copy of another project has its own, as two projects' entries are not one.
    assert_ne!(
        a.model.library.guid(&Kind::Type("wall".into()), "Brick 300"),
        x.model.library.guid(&Kind::Type("wall".into()), "Brick 300")
    );
}

#[test]
fn a_familys_types_and_parameters_are_entries_with_the_guids_of_every_copy() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(
        &mut a,
        "family",
        json!({
            "name": "Trap", "category": "plumbing", "kind": "sink",
            "parameters": [{"name": "DN", "kind": "length", "value": 50}],
            "types": {"50": {}, "75": {"DN": 75}},
        }),
    );
    run(&mut a, "option", json!({"set": "Facade", "options": ["Brick", "Glass"]}));
    send(&before, &a, &mut b);
    for (kind, name) in [
        (Kind::Family, "Trap"),
        (Kind::FamilyType, "Trap: 75"),
        (Kind::FamilyParameter, "Trap: DN"),
        (Kind::OptionSet, "Facade"),
        (Kind::DesignOption, "Facade: Glass"),
    ] {
        let g = a.model.library.guid(&kind, name);
        assert!(g.is_some(), "{name}");
        assert_eq!(b.model.library.guid(&kind, name), g, "{name}");
    }
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn an_entry_taken_out_and_made_again_under_the_same_name_is_another_entry_and_its_elements_say_so() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Brick", "thickness": 200}));
    let wall = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Brick"}))["ids"][0].as_u64().unwrap();
    send(&before, &a, &mut b);
    let guid = a.model.elements.guid(wall).unwrap();
    let old = wall_guid(&a.model, "Brick");
    // The wall goes to another type, the type is taken out and made again, and the wall comes back to it.
    let before = a.model.clone();
    let other = wall_types(&a.model).into_iter().find(|n| n != "Brick").unwrap();
    run(&mut a, "modify", json!({"ids": [wall], "type": other}));
    run(&mut a, "delete_type", json!({"category": "wall", "type": "Brick"}));
    run(&mut a, "type", json!({"category": "wall", "name": "Brick", "thickness": 250}));
    run(&mut a, "modify", json!({"ids": [wall], "type": "Brick"}));
    assert_ne!(wall_guid(&a.model, "Brick"), old);
    send(&before, &a, &mut b);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(wall_guid(&b.model, "Brick"), wall_guid(&a.model, "Brick"));
    assert!(matches!(b.model.elements.get(guid), Some(Element::Wall(w)) if w.type_name == "Brick"));
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn the_undo_history_keeps_a_rename_and_goes_back_on_the_project_opened_again() {
    let (mut a, _) = two();
    run(&mut a, "type", json!({"category": "wall", "name": "Old", "thickness": 200}));
    run(&mut a, "rename_type", json!({"category": "wall", "type": "Old", "name": "New"}));
    let h = a.kept_history().unwrap();
    let keys: Vec<&String> = h.undo.iter().flat_map(|s| s.changes.project.keys()).chain(h.now.project.keys()).collect();
    assert!(keys.iter().any(|k| k.ends_with("/name")), "{keys:?}");
    // And it goes back on the project opened again, where undo takes the rename back.
    let mut again = third(&a);
    assert!(again.restore_history(&h).is_ok());
    run(&mut again, "undo", json!({}));
    assert!(again.model.types.wall.contains_key("Old") && !again.model.types.wall.contains_key("New"));
}

#[test]
fn a_type_deleted_with_its_instances_goes_from_the_other_copy_with_them() {
    let (mut a, mut b) = two();
    let base = a.model.clone();
    run(&mut a, "type", json!({"category": "wall", "name": "Brick", "thickness": 200}));
    let wall = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Brick"}))["ids"][0].as_u64().unwrap();
    send(&base, &a, &mut b);
    assert_eq!((walls(&a.model), walls(&b.model)), (1, 1));
    // Revit's Delete on a type in use: the type and its wall go, in one change that the other copy takes whole.
    let before = a.model.clone();
    let r = run(&mut a, "delete_type", json!({"category": "wall", "type": "Brick"}));
    assert_eq!(r["elements"], json!([wall]));
    send(&before, &a, &mut b);
    assert_eq!((walls(&b.model), wall_types(&b.model).contains(&"Brick".to_string())), (0, false), "the wall and its type, both gone");
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(b.model.check(), Ok(()));
}

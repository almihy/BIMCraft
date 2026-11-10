//! Editing together (`sync`): two copies of one project send each other what their commands changed, by GUID, and end
//! the same; what would break a copy is refused; undo stays each copy's; a link from text is each copy's own.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::ids::Guid;
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

/// What `s` changed since `before`, sent to `to`.
fn send(before: &Model, s: &Session, to: &mut Session) -> sync::Merged {
    send_model(before, &s.model, to)
}

fn send_model(before: &Model, after: &Model, to: &mut Session) -> sync::Merged {
    let c = sync::changes(before, after).unwrap();
    // As the page sends it: JSON, read back strictly.
    let c = Changes::read(&c.to_json()).unwrap();
    to.merge(&c, &BTreeSet::new()).unwrap()
}

/// The elements by GUID, as JSON: what two copies that agree have alike (their ids may differ).
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

#[test]
fn a_wall_and_its_door_drawn_in_one_copy_come_to_the_other_by_guid() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    let wall = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    run(&mut a, "door", json!({"wall": wall, "offset": 3000}));
    let c = sync::changes(&before, &a.model).unwrap();
    assert_eq!(c.elements.len(), 2, "the wall and the door: {:?}", c.elements.keys());
    assert!(c.project.is_empty(), "no setting changed: {:?}", c.project.keys());
    let got = send(&before, &a, &mut b);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(b.model.check(), Ok(()));
    // The door points at its wall by GUID, which is the same in both.
    assert_eq!(walls(&b.model), 1);
}

#[test]
fn both_draw_at_once_and_end_the_same_though_their_ids_cross() {
    let (mut a, mut b) = two();
    let (a0, b0) = (a.model.clone(), b.model.clone());
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    run(&mut b, "wall", json!({"points": [[0, 5000], [6000, 5000]]}));
    // Each gave its wall the same id: each copy gives the other's another.
    let (a1, b1) = (a.model.clone(), b.model.clone());
    send_model(&b0, &b1, &mut a);
    send_model(&a0, &a1, &mut b);
    assert_eq!(walls(&a.model), 2);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(a.model.check(), Ok(()));
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn a_change_and_a_removal_come_across_and_settings_too() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    let ids = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0], [6000, 4000]]}))["ids"].clone();
    send(&before, &a, &mut b);
    let before = a.model.clone();
    run(&mut a, "move", json!({"ids": [ids[0]], "by": [0, 500]}));
    run(&mut a, "delete", json!({"ids": [ids[1]]}));
    run(&mut a, "type", json!({"category": "wall", "name": "Party wall 250", "thickness": 250}));
    run(&mut a, "units", json!({"length": "m"}));
    let c = sync::changes(&before, &a.model).unwrap();
    assert!(c.elements.values().any(Option::is_none), "the removal: {c:?}");
    assert!(c.project.contains_key("types") && c.project.contains_key("units"), "{:?}", c.project.keys());
    let got = send(&before, &a, &mut b);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert!(serde_json::to_string(&*b.model.types).unwrap().contains("Party wall 250"));
    assert_eq!(b.model.units, a.model.units);
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn what_would_break_the_model_is_refused_and_the_rest_taken() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    let wall = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].clone();
    send(&before, &a, &mut b);
    // B puts a door in the wall while A takes the wall away and draws another.
    let b_id = b.model.elements.id(a.model.elements.guid(wall.as_u64().unwrap()).unwrap()).unwrap();
    run(&mut b, "door", json!({"wall": b_id, "offset": 3000}));
    let before = a.model.clone();
    run(&mut a, "delete", json!({"ids": [wall]}));
    run(&mut a, "wall", json!({"points": [[0, 8000], [6000, 8000]]}));
    let got = send(&before, &a, &mut b);
    assert_eq!(got.refused.len(), 1, "the wall B's door stands in stays there: {:?}", got.refused);
    assert_eq!(walls(&b.model), 2, "the new wall came, the old one stayed");
    assert_eq!(b.model.check(), Ok(()));
    // Hostile changes break nothing.
    for bad in [
        json!({"elements": {"not a guid": "{}"}}),
        json!({"elements": {Guid::new().to_string(): "{\"category\": \"wall\"}"}}),
        json!({"elements": {Guid::new().to_string(): "not json"}}),
        json!({"project": {"next_id": "1"}}),
        json!({"project": {"units": "{\"length\": \"furlongs\"}"}}),
    ] {
        let was = by_guid(&b.model);
        match Changes::read(&bad.to_string()) {
            Err(_) => {}
            Ok(c) => {
                let got = b.merge(&c, &BTreeSet::new()).unwrap();
                assert!(!got.refused.is_empty(), "{bad}");
            }
        }
        assert_eq!(by_guid(&b.model), was, "{bad}");
        assert_eq!(b.model.check(), Ok(()));
    }
}

#[test]
fn undo_takes_back_ones_own_command_and_leaves_the_others() {
    let (mut a, mut b) = two();
    run(&mut b, "wall", json!({"points": [[0, 5000], [6000, 5000]]}));
    let mine = b.model.elements.guid(*b.model.elements.keys().next_back().unwrap()).unwrap();
    let before = a.model.clone();
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let theirs = sync::changes(&before, &a.model).unwrap();
    let theirs_guid = *theirs.elements.keys().next().unwrap();
    b.merge(&theirs, &BTreeSet::new()).unwrap();
    assert_eq!(walls(&b.model), 2);
    run(&mut b, "undo", json!({}));
    assert!(b.model.elements.get(mine).is_none(), "B's own wall is taken back");
    assert!(b.model.elements.get(theirs_guid).is_some(), "A's stays");
    run(&mut b, "redo", json!({}));
    assert!(b.model.elements.get(mine).is_some() && b.model.elements.get(theirs_guid).is_some());
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn ones_own_changes_not_sent_yet_stay_and_a_whole_model_makes_a_copy_say_it() {
    let (mut a, mut b) = two();
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    run(&mut a, "link", json!({"text": Session::new().model.to_json().unwrap(), "name": "Site"}));
    let all = sync::entries(&a.model).unwrap();
    assert!(all.complete);
    assert!(all.elements.values().flatten().all(|e| !e.contains("text:Site")), "a link from text is each copy's own, never sent");
    // B has a wall of its own not sent yet, and its own site.
    run(&mut b, "link", json!({"text": Session::new().model.to_json().unwrap(), "name": "Site"}));
    let before = b.model.clone();
    run(&mut b, "wall", json!({"points": [[0, 9000], [6000, 9000]]}));
    let pending: BTreeSet<Guid> = sync::changes(&before, &b.model).unwrap().elements.into_keys().collect();
    b.merge(&all, &pending).unwrap();
    assert_eq!(walls(&b.model), 2, "A's wall came, B's not-yet-sent one stayed");
    let links = b.model.elements.values().filter(|e| sync::own(e)).count();
    assert_eq!(links, 1, "B keeps its own site, once");
    // Without it, the whole model makes B say what A says.
    b.merge(&all, &BTreeSet::new()).unwrap();
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(b.model.check(), Ok(()));
}

/// `s`'s project opened again from its file, as a page reloaded opens it, with the site linked again (each copy's own).
fn reopened(s: &Session) -> Session {
    let mut x = Session::new();
    x.reset(Model::read(&s.model.to_json().unwrap()).unwrap().0, None);
    run(&mut x, "link", json!({"text": Session::new().model.to_json().unwrap(), "name": "Site"}));
    x
}

/// The history kept as the page keeps it: JSON, read back.
fn kept(s: &Session) -> sync::History {
    sync::History::read(&s.kept_history().unwrap().to_json()).unwrap()
}

#[test]
fn a_kept_undo_history_goes_back_on_the_project_opened_again() {
    let (mut a, _) = two();
    run(&mut a, "link", json!({"text": Session::new().model.to_json().unwrap(), "name": "Site"}));
    let m0 = by_guid(&a.model);
    let wall = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let m1 = by_guid(&a.model);
    run(&mut a, "door", json!({"wall": wall, "offset": 3000}));
    let m2 = by_guid(&a.model);
    run(&mut a, "wall", json!({"points": [[0, 4000], [6000, 4000]]}));
    let m3 = by_guid(&a.model);
    // The last wall undone: a redo step, kept too.
    run(&mut a, "undo", json!({}));
    let h = kept(&a);
    assert_eq!(h.undo.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Wall", "Door"], "the site's link is no step of the person's");
    assert_eq!(h.redo.len(), 1);

    let mut b = reopened(&a);
    assert_eq!(b.restore_history(&h), Ok((2, 1)));
    assert_eq!(b.history(), ["Wall", "Door"]);
    run(&mut b, "redo", json!({}));
    assert_eq!(by_guid(&b.model), m3);
    run(&mut b, "undo", json!({"count": 2}));
    assert_eq!(by_guid(&b.model), m1, "the door undone, by GUID");
    run(&mut b, "undo", json!({}));
    assert_eq!(by_guid(&b.model), m0);
    assert_eq!(b.model.elements.values().filter(|e| sync::own(e)).count(), 1, "the site stays linked through it all");
    run(&mut b, "redo", json!({"count": 2}));
    assert_eq!(by_guid(&b.model), m2);
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn a_kept_history_leaves_the_others_work_since_in_place() {
    let (mut a, mut other) = two();
    let mine = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let mine = a.model.elements.guid(mine).unwrap();
    // The room has A's wall; another person draws one after A's page went.
    let start = other.model.clone();
    send_model(&start, &a.model, &mut other);
    let theirs = run(&mut other, "wall", json!({"points": [[0, 8000], [6000, 8000]]}))["ids"][0].as_u64().unwrap();
    let theirs = other.model.elements.guid(theirs).unwrap();
    let h = kept(&a);
    let mut b = reopened(&other);
    assert_eq!(b.restore_history(&h), Ok((1, 0)));
    run(&mut b, "undo", json!({}));
    assert!(b.model.elements.get(mine).is_none(), "A's own wall is taken back");
    assert!(b.model.elements.get(theirs).is_some(), "the other's stays");
    assert_eq!(b.model.check(), Ok(()));
}

#[test]
fn a_history_goes_back_only_on_its_own_project_and_under_the_steps_taken_since() {
    let (mut a, _) = two();
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    let h = kept(&a);
    let mut stranger = Session::new();
    run(&mut stranger, "new", json!({}));
    assert!(stranger.restore_history(&h).is_err());
    assert!(stranger.history().is_empty());
    // A step taken before the history came back stays on top of it.
    let mut b = reopened(&a);
    run(&mut b, "wall", json!({"points": [[0, 3000], [6000, 3000]]}));
    // Under the site's link and the new wall: three steps.
    assert_eq!(b.restore_history(&h), Ok((3, 0)));
    run(&mut b, "undo", json!({"count": 3}));
    assert_eq!(walls(&b.model), 0);
    run(&mut b, "redo", json!({"count": 3}));
    assert_eq!(walls(&b.model), 2);
}

/// The wall type of `m`'s first wall.
fn wall_type(m: &Model) -> String {
    m.elements.values().find_map(|e| if let Element::Wall(w) = e { Some(w.type_name.clone()) } else { None }).unwrap()
}

#[test]
fn a_kept_history_takes_back_drawn_walls_a_door_and_a_delete_though_the_room_says_them_otherwise_since() {
    let (mut a, mut other) = two();
    run(&mut a, "link", json!({"text": Session::new().model.to_json().unwrap(), "name": "Site"}));
    let m0 = by_guid(&a.model);
    let w1 = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let m1 = by_guid(&a.model);
    let w2 = run(&mut a, "wall", json!({"points": [[0, 4000], [6000, 4000]]}))["ids"][0].as_u64().unwrap();
    let m2 = by_guid(&a.model);
    run(&mut a, "door", json!({"wall": w1, "offset": 3000}));
    let m3 = by_guid(&a.model);
    let g2 = a.model.elements.guid(w2).unwrap();
    let w3 = run(&mut a, "wall", json!({"points": [[0, 8000], [6000, 8000]]}))["ids"][0].as_u64().unwrap();
    run(&mut a, "delete", json!({"ids": [w3]}));
    let m5 = by_guid(&a.model);
    let h = kept(&a);
    assert_eq!(h.undo.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(), ["Wall", "Wall", "Door", "Wall", "Delete"]);

    // The room has A's work; after A's page went, another moves A's second wall (and the server's copy may say A's
    // walls a little otherwise than A's page did: the same to the history).
    send_model(&other.model.clone(), &a.model, &mut other);
    let og2 = other.model.elements.id(g2).unwrap();
    run(&mut other, "move", json!({"ids": [og2], "by": [500, 0]}));
    let moved = by_guid(&other.model);
    let mut b = reopened(&other);
    assert_eq!(b.restore_history(&h), Ok((5, 0)));
    run(&mut b, "undo", json!({"count": 2}));
    assert_eq!(by_guid(&b.model), moved, "the deleted wall back and gone again");
    run(&mut b, "undo", json!({}));
    let mut want = m2.clone();
    want.insert(g2, moved[&g2].clone());
    assert_eq!(by_guid(&b.model), want, "the door taken back; the other's move of the wall stays");
    run(&mut b, "undo", json!({}));
    assert!(b.model.elements.get(g2).is_none(), "the second wall taken back, moved as it was");
    assert_eq!(by_guid(&b.model), m1);
    run(&mut b, "undo", json!({}));
    assert_eq!(by_guid(&b.model), m0, "the first wall taken back: nothing of A's is left");
    assert_eq!(walls(&b.model), 0);
    run(&mut b, "redo", json!({"count": 3}));
    let mut want = m3.clone();
    want.insert(g2, moved[&g2].clone());
    assert_eq!(by_guid(&b.model), want);
    assert_eq!(b.model.check(), Ok(()));
    let _ = m5;
}

#[test]
fn undo_takes_back_a_drawn_wall_the_others_changed_since() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    let w = run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let g = a.model.elements.guid(w).unwrap();
    send(&before, &a, &mut b);
    // B moves A's wall; A takes it in, then undoes drawing it.
    let before = b.model.clone();
    let bw = b.model.elements.id(g).unwrap();
    run(&mut b, "move", json!({"ids": [bw], "by": [0, 500]}));
    send(&before, &b, &mut a);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    run(&mut a, "undo", json!({}));
    assert!(a.model.elements.get(g).is_none(), "A's wall is taken back, moved or not");
    run(&mut a, "redo", json!({}));
    assert_eq!(by_guid(&a.model), by_guid(&b.model), "and comes back as B left it");
    assert_eq!(a.model.check(), Ok(()));
}

#[test]
fn settings_and_the_elements_that_need_them_come_across_together() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]]}));
    send(&before, &a, &mut b);
    // A renames the wall's type: the type list and the wall change together, and neither passes B's check alone.
    let before = a.model.clone();
    let was = wall_type(&a.model);
    run(&mut a, "rename_type", json!({"category": "wall", "type": was, "name": "Renamed wall"}));
    let got = send(&before, &a, &mut b);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(wall_type(&b.model), "Renamed wall");
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(b.model.check(), Ok(()));
    // A whole other project opened in A (its own types, its own walls) comes to B whole, as the room keeps it.
    let mut other = Session::new();
    run(&mut other, "new", json!({}));
    run(&mut other, "type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
    run(&mut other, "wall", json!({"points": [[0, 0], [9000, 0]], "type": "Brick 300"}));
    run(&mut other, "delete_type", json!({"category": "wall", "type": was}));
    let all = sync::entries(&other.model).unwrap();
    let got = b.merge(&all, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(by_guid(&b.model), by_guid(&other.model));
    assert_eq!(wall_type(&b.model), "Brick 300");
    assert_eq!(b.model.check(), Ok(()));
}

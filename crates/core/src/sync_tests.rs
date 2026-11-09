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

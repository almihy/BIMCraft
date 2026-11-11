//! Editing together through a shared room (`docs/areas/sync.md`): copies that take each other's changes in any order and tell the
//! room what they settled end as one, and what a person placed is never lost to a taking out. The room here is what Arqiflow's is:
//! a map of keys where the last write of a key stands, and every copy keeps what it last sent and what it has heard of the room.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::ids::Guid;
use crate::library::Kind;
use crate::model::{Element, Model};
use crate::sync::{self, Changes, Merged};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// What a room holds: the elements by GUID and the settings by key, each JSON text.
#[derive(Clone, Default, PartialEq, Debug)]
struct Held {
    elements: BTreeMap<Guid, String>,
    project: BTreeMap<String, String>,
}

impl Held {
    fn put(&mut self, c: &Changes) {
        for (g, v) in &c.elements {
            match v {
                Some(t) => self.elements.insert(*g, t.clone()),
                None => self.elements.remove(g),
            };
        }
        for (k, v) in &c.project {
            match v {
                Some(t) => self.project.insert(k.clone(), t.clone()),
                None => self.project.remove(k),
            };
        }
    }

    fn of(m: &Model) -> Held {
        let mut h = Held::default();
        h.put(&sync::entries(m).unwrap());
        h
    }
}

/// A copy of the project in a person's page (or the server's), bound to the room as the page binds its engine.
struct Copy {
    s: Session,
    /// What it last sent: what it changed since is its own to send.
    sent: Arc<Model>,
    /// What it has heard of the room (its own writes included).
    heard: Held,
}

impl Copy {
    fn new(s: Session, room: &Held) -> Copy {
        Copy { sent: Arc::clone(&s.model), s, heard: room.clone() }
    }

    fn model(&self) -> &Model {
        &self.s.model
    }

    fn run(&mut self, id: &str, p: Value) -> Value {
        run(&mut self.s, id, p)
    }

    /// What this person changed since last time goes into the room.
    fn push(&mut self, room: &mut Held) {
        let c = sync::changes(&self.sent, &self.s.model).unwrap();
        // Through JSON, as the page sends it.
        let c = Changes::read(&c.to_json()).unwrap();
        room.put(&c);
        self.heard.put(&c);
        self.sent = Arc::clone(&self.s.model);
    }

    /// What the room says differently from what this copy heard, where `only` allows the key, taken in as one batch. What the copy
    /// settled that the room does not say (`fixes`) goes into the room.
    fn pull(&mut self, room: &mut Held, only: impl Fn(&str) -> bool) -> Merged {
        let mut c = Changes::default();
        for g in self.heard.elements.keys().chain(room.elements.keys()).collect::<BTreeSet<_>>() {
            let (was, now) = (self.heard.elements.get(g), room.elements.get(g));
            if was != now && only(&g.to_string()) {
                c.elements.insert(*g, now.cloned());
            }
        }
        for k in self.heard.project.keys().chain(room.project.keys()).collect::<BTreeSet<_>>() {
            let (was, now) = (self.heard.project.get(k), room.project.get(k));
            if was != now && only(k) {
                c.project.insert(k.clone(), now.cloned());
            }
        }
        self.heard.put(&c);
        let c = Changes::read(&c.to_json()).unwrap();
        // This person's own changes not sent yet stay as they are.
        let keep: BTreeSet<Guid> = sync::changes(&self.sent, &self.s.model).unwrap().elements.into_keys().collect();
        let merged = self.s.merge(&c, &keep).unwrap();
        self.sent = self.sent.with_changes(&merged.taken, &merged.ids).map(Arc::new).unwrap_or_else(|_| Arc::clone(&self.s.model));
        let fixes = Changes { project: merged.fixes.clone(), ..Changes::default() };
        room.put(&fixes);
        self.heard.put(&fixes);
        merged
    }

    fn pull_all(&mut self, room: &mut Held) -> Merged {
        self.pull(room, |_| true)
    }
}

/// Every copy takes in the room until none has anything more to say.
fn settle(copies: &mut [&mut Copy], room: &mut Held) {
    for _ in 0..8 {
        let before = (room.clone(), copies.iter().map(|c| c.heard.clone()).collect::<Vec<_>>());
        for c in copies.iter_mut() {
            c.pull_all(room);
            c.push(room);
        }
        if before == (room.clone(), copies.iter().map(|c| c.heard.clone()).collect::<Vec<_>>()) {
            return;
        }
    }
    panic!("the copies did not settle in eight rounds");
}

/// Two copies of one project, read from one file, and the room that says it.
fn two() -> (Copy, Copy, Held) {
    let mut s = Session::new();
    run(&mut s, "new", json!({}));
    let text = s.model.to_json().unwrap();
    let open = |t: &str| {
        let mut x = Session::new();
        x.reset(Model::read(t).unwrap().0, None);
        x
    };
    let (a, b) = (open(&text), open(&text));
    let room = Held::of(&a.model);
    (Copy::new(a, &room), Copy::new(b, &room), room)
}

fn wall_type() -> Kind {
    Kind::Type("wall".into())
}

fn walls_of(m: &Model, type_name: &str) -> usize {
    m.elements.values().filter(|e| matches!(e, Element::Wall(w) if w.type_name == type_name)).count()
}

/// `text` (JSON) without the ids each copy gives the parts of an entry (a layer has an id of its copy's and a GUID of the project's).
fn without_ids(text: &str) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(o) => {
                if o.contains_key("guid") {
                    o.remove("id");
                }
                o.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut v = serde_json::from_str::<Value>(text).unwrap_or(Value::Null);
    strip(&mut v);
    v
}

fn same(a: &Model, b: &Model) {
    let (x, y) = (sync::entries(a).unwrap(), sync::entries(b).unwrap());
    let say = |c: &Changes, k: &String| c.project.get(k).cloned().flatten().map(|t| without_ids(&t));
    let differ: Vec<String> = x
        .project
        .keys()
        .chain(y.project.keys())
        .filter(|k| say(&x, k) != say(&y, k))
        .map(|k| format!("{k}: {:?} / {:?}", say(&x, k), say(&y, k)))
        .chain(x.elements.keys().chain(y.elements.keys()).filter(|k| x.elements.get(*k) != y.elements.get(*k)).map(|k| k.to_string()))
        .collect();
    assert!(differ.is_empty(), "the two copies say different things: {differ:#?}");
}

/// A type Beta that both copies have, nothing placed in it.
fn both_have_beta() -> (Copy, Copy, Held, Guid) {
    let (mut a, mut b, mut room) = two();
    a.run("type", json!({"category": "wall", "name": "Beta", "thickness": 300}));
    a.push(&mut room);
    b.pull_all(&mut room);
    let beta = a.model().library.guid(&wall_type(), "Beta").unwrap();
    assert_eq!(b.model().library.guid(&wall_type(), "Beta"), Some(beta));
    (a, b, room, beta)
}

fn assert_beta_and_the_wall_everywhere(copies: &[&Copy], room: &Held, beta: Guid, wall: Guid) {
    for c in copies {
        assert!(c.model().types.wall.contains_key("Beta"), "the type in use stays");
        assert_eq!(c.model().library.guid(&wall_type(), "Beta"), Some(beta));
        assert_eq!(walls_of(c.model(), "Beta"), 1, "the wall is there");
        assert!(c.model().elements.contains_key(wall));
        assert_eq!(c.model().check(), Ok(()));
    }
    for pair in copies.windows(2) {
        same(pair[0].model(), pair[1].model());
    }
    // The room says the same: a copy made from it is the others.
    assert!(room.project.contains_key(&format!("types/wall/{beta}")) && room.project.contains_key(&format!("types/wall/{beta}/name")));
    assert!(room.elements.contains_key(&wall));
    let mut fresh = Session::new();
    fresh.reset(Model::read(&copies[0].model().to_json().unwrap()).unwrap().0, None);
    let all = Changes {
        elements: room.elements.iter().map(|(g, t)| (*g, Some(t.clone()))).collect(),
        project: room.project.iter().map(|(k, t)| (k.clone(), Some(t.clone()))).collect(),
        complete: true,
    };
    let got = fresh.merge(&all, &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    same(&fresh.model, copies[0].model());
}

#[test]
fn a_type_taken_out_by_one_while_another_placed_a_wall_of_it_ends_in_every_copy_with_the_type_and_the_wall() {
    // The copy that placed the wall hears the taking out last, and the copy that took the type out hears the wall first.
    let (mut a, mut b, mut room, beta) = both_have_beta();
    a.run("delete_type", json!({"category": "wall", "type": "Beta"}));
    let wall = b.run("wall", json!({"points": [[0, 0], [6000, 0]], "type": "Beta"}))["ids"][0].as_u64().unwrap();
    let wall = b.model().elements.guid(wall).unwrap();
    a.push(&mut room);
    b.push(&mut room);
    // A hears B's wall: its type is gone here, so the wall cannot be taken yet. It is not refused: it waits for the type.
    let got = a.pull_all(&mut room);
    assert!(got.refused.is_empty(), "a waiting wall is not a refused one: {:?}", got.refused);
    assert!(got.waiting.contains(&wall) && a.s.waiting.contains_key(&wall));
    assert!(!a.model().elements.contains_key(wall));
    // B hears A's taking out: its wall is of the type, so the type stays, and B tells the room so; the one who took it out is told why.
    let got = b.pull_all(&mut room);
    assert!(got.refused.is_empty(), "nothing is refused: {:?}", got.refused);
    assert_eq!(got.kept.len(), 1, "{:?}", got.kept);
    assert_eq!(
        (got.kept[0].kind.as_str(), got.kept[0].name.as_str(), got.kept[0].uses.clone()),
        ("wall type", "Beta", vec!["walls".to_string()])
    );
    assert_eq!(got.kept[0].entry, beta);
    assert_eq!(got.to_json()["kept"][0], json!({"entry": beta.to_string(), "kind": "wall type", "name": "Beta", "uses": ["walls"]}));
    assert!(b.model().types.wall.contains_key("Beta"));
    assert_eq!(got.fixes.len(), 2, "its body and its name: {:?}", got.fixes.keys());
    assert!(got.taken.project.is_empty(), "the taking out is not taken: {:?}", got.taken.project.keys());
    // A hears the type again, and the wall that waited comes with it.
    a.pull_all(&mut room);
    settle(&mut [&mut a, &mut b], &mut room);
    assert_beta_and_the_wall_everywhere(&[&a, &b], &room, beta, wall);
}

#[test]
fn a_type_taken_out_by_one_while_another_placed_a_wall_of_it_ends_the_same_whichever_order_each_hears_it_in() {
    let (mut a, mut b, mut room, beta) = both_have_beta();
    a.run("delete_type", json!({"category": "wall", "type": "Beta"}));
    let wall = b.run("wall", json!({"points": [[0, 0], [6000, 0]], "type": "Beta"}))["ids"][0].as_u64().unwrap();
    let wall = b.model().elements.guid(wall).unwrap();
    a.push(&mut room);
    b.push(&mut room);
    // B keeps the type; then A is told the type and the wall in one batch, which is what a copy that was slow to answer hears.
    b.pull_all(&mut room);
    let got = a.pull_all(&mut room);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    settle(&mut [&mut a, &mut b], &mut room);
    assert_beta_and_the_wall_everywhere(&[&a, &b], &room, beta, wall);
}

#[test]
fn a_type_taken_out_while_a_wall_of_it_is_placed_is_kept_by_the_server_too() {
    // The server's copy is the stored model, and what it is told is what the room differs from it in: the type's two keys gone and
    // the wall in. It keeps the type, says so in `fixes`, and the room is put right by them.
    let (mut a, mut b, mut room, beta) = both_have_beta();
    let mut server = Session::new();
    server.reset(Model::read(&a.model().to_json().unwrap()).unwrap().0, None);
    let stored = Held::of(server.model.as_ref());
    a.run("delete_type", json!({"category": "wall", "type": "Beta"}));
    let wall = b.run("wall", json!({"points": [[0, 0], [6000, 0]], "type": "Beta"}))["ids"][0].as_u64().unwrap();
    let wall = b.model().elements.guid(wall).unwrap();
    a.push(&mut room);
    b.push(&mut room);
    let mut c = Changes::default();
    for g in stored.elements.keys().chain(room.elements.keys()).collect::<BTreeSet<_>>() {
        if stored.elements.get(g) != room.elements.get(g) {
            c.elements.insert(*g, room.elements.get(g).cloned());
        }
    }
    for k in stored.project.keys().chain(room.project.keys()).collect::<BTreeSet<_>>() {
        if stored.project.get(k) != room.project.get(k) {
            c.project.insert(k.clone(), room.project.get(k).cloned());
        }
    }
    assert!(c.project.get(&format!("types/wall/{beta}")) == Some(&None) && c.elements.contains_key(&wall));
    let got = server.merge(&Changes::read(&c.to_json()).unwrap(), &BTreeSet::new()).unwrap();
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert!(server.model.types.wall.contains_key("Beta") && server.model.elements.contains_key(wall));
    assert_eq!(server.model.check(), Ok(()));
    // What it says puts the room right, and then the room says what the server has: nothing differs.
    let fixes = Changes { project: got.fixes.clone(), ..Changes::default() };
    assert!(fixes.project.keys().all(|k| k.starts_with(&format!("types/wall/{beta}"))) && fixes.project.len() == 2, "{:?}", got.fixes);
    room.put(&fixes);
    settle(&mut [&mut a, &mut b], &mut room);
    assert_beta_and_the_wall_everywhere(&[&a, &b], &room, beta, wall);
    same(&server.model, a.model());
}

#[test]
fn a_material_taken_out_while_a_type_made_of_it_is_edited_is_kept() {
    let (mut a, mut b, mut room) = two();
    a.run("material", json!({"name": "Clay", "color": "#aa5533"}));
    a.push(&mut room);
    b.pull_all(&mut room);
    a.run("material", json!({"name": "Clay", "delete": true}));
    b.run(
        "type",
        json!({"category": "wall", "name": "Fired", "layers": [{"function": "structure", "material": "Clay", "thickness": 200}]}),
    );
    a.push(&mut room);
    b.push(&mut room);
    settle(&mut [&mut a, &mut b], &mut room);
    for c in [&a, &b] {
        assert!(c.model().materials.contains_key("Clay"), "the material in use stays");
        assert!(c.model().types.wall.contains_key("Fired"), "and the type made of it");
        assert_eq!(c.model().check(), Ok(()));
    }
    same(a.model(), b.model());
}

#[test]
fn a_batch_with_one_key_that_cannot_apply_takes_the_other_keys() {
    let (mut a, mut b, mut room) = two();
    a.run("material", json!({"name": "Plaster", "color": "#eeeeee"}));
    a.run("type", json!({"category": "wall", "name": "Brick 300", "thickness": 300}));
    let mut c = sync::changes(&b.sent, a.model()).unwrap();
    let (material, wall) =
        (a.model().library.guid(&Kind::Material, "Plaster").unwrap(), a.model().library.guid(&wall_type(), "Brick 300").unwrap());
    // A type whose body is not one (`thickness` is a number), a key that is no entry's, and a body that is not JSON.
    let bad = Guid::new();
    c.project.insert(format!("types/wall/{bad}"), Some(json!({"entry": {"thickness": "thick"}}).to_string()));
    c.project.insert(format!("types/wall/{bad}/name"), Some("\"Broken\"".into()));
    c.project.insert("types/nothing/x".into(), Some("{}".into()));
    c.project.insert(format!("materials/{}", Guid::new()), Some("not json".into()));
    let got = b.s.merge(&Changes::read(&c.to_json()).unwrap(), &BTreeSet::new()).unwrap();
    assert!(got.refused.len() >= 3, "{:?}", got.refused);
    for key in [format!("types/wall/{bad}"), "types/nothing/x".to_string()] {
        assert!(got.refused.iter().any(|(what, _)| *what == key), "{key} is named: {:?}", got.refused);
    }
    assert!(b.model().materials.contains_key("Plaster") && b.model().types.wall.contains_key("Brick 300"), "the others are taken");
    assert!(!b.model().types.wall.contains_key("Broken"));
    assert_eq!(b.model().library.guid(&Kind::Material, "Plaster"), Some(material));
    assert_eq!(b.model().library.guid(&wall_type(), "Brick 300"), Some(wall));
    assert_eq!(b.model().check(), Ok(()));
    let _ = &mut room;
}

#[test]
fn a_type_taken_out_by_one_and_edited_by_another_ends_without_it_everywhere_and_without_a_body_left_in_the_room() {
    // The room keeps the last write of each key: whichever of the two writes lands last, a copy is told a body with no name, or both gone.
    for edit_last in [true, false] {
        let (mut a, mut b, mut room, beta) = both_have_beta();
        a.run("delete_type", json!({"category": "wall", "type": "Beta"}));
        b.run("type", json!({"category": "wall", "name": "Beta", "thickness": 320}));
        if edit_last {
            a.push(&mut room);
            b.push(&mut room);
            // The name is taken out and the body edited: what is left is a body alone.
            assert!(
                room.project.contains_key(&format!("types/wall/{beta}")) && !room.project.contains_key(&format!("types/wall/{beta}/name"))
            );
        } else {
            b.push(&mut room);
            a.push(&mut room);
            assert!(!room.project.keys().any(|k| k.starts_with(&format!("types/wall/{beta}"))));
        }
        settle(&mut [&mut a, &mut b], &mut room);
        for c in [&a, &b] {
            assert!(!c.model().types.wall.contains_key("Beta"), "taken out everywhere (edit last: {edit_last})");
            assert_eq!(c.model().check(), Ok(()));
        }
        same(a.model(), b.model());
        assert!(
            !room.project.keys().any(|k| k.starts_with(&format!("types/wall/{beta}"))),
            "no key of it is left in the room: {:?}",
            room.project.keys()
        );
    }
}

#[test]
fn a_body_left_with_no_name_is_taken_out_of_the_room_by_whoever_meets_it() {
    let (mut a, _, mut room) = two();
    let ghost = Guid::new();
    let key = format!("materials/{ghost}");
    room.project.insert(key.clone(), json!({"entry": {"color": [1, 2, 3]}}).to_string());
    let got = a.pull_all(&mut room);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(got.fixes.get(&key), Some(&None), "{:?}", got.fixes);
    assert!(!room.project.contains_key(&key));
    // And once it is out there is nothing more to say.
    let again = a.pull_all(&mut room);
    assert!(again.fixes.is_empty());
}

#[test]
fn two_entries_asking_for_a_name_that_already_has_a_count_become_the_next_count_not_a_count_of_a_count() {
    let (mut a, mut b, mut room) = two();
    let mut c = {
        let mut x = Session::new();
        x.reset(Model::read(&a.model().to_json().unwrap()).unwrap().0, None);
        Copy::new(x, &room)
    };
    a.run("material", json!({"name": "Brick (2)", "color": "#aa0000"}));
    b.run("material", json!({"name": "Brick (2)", "color": "#00aa00"}));
    c.run("material", json!({"name": "Brick", "color": "#0000aa"}));
    a.push(&mut room);
    b.push(&mut room);
    c.push(&mut room);
    settle(&mut [&mut a, &mut b, &mut c], &mut room);
    let names = |m: &Model| m.materials.keys().filter(|n| n.starts_with("Brick")).cloned().collect::<Vec<_>>();
    for x in [&a, &b, &c] {
        assert_eq!(names(x.model()), ["Brick", "Brick (2)", "Brick (3)"], "{:?}", names(x.model()));
        assert_eq!(x.model().check(), Ok(()));
    }
    same(a.model(), b.model());
    same(b.model(), c.model());
}

/// A wall that names a type by GUID, as it goes to a room, and the GUID of that type; the type is in no copy of `two()`.
fn wall_of_a_type_nobody_has() -> (Guid, String, Guid) {
    let (mut a, _, _) = two();
    let before = Arc::clone(&a.s.model);
    a.run("type", json!({"category": "wall", "name": "Ghost", "thickness": 250}));
    a.run("wall", json!({"points": [[0, 0], [6000, 0]], "type": "Ghost"}));
    let ghost = a.model().library.guid(&wall_type(), "Ghost").unwrap();
    let c = sync::changes(&before, a.model()).unwrap();
    let (g, text) = c.elements.into_iter().next().unwrap();
    (g, text.unwrap(), ghost)
}

fn a_new_material(a: &mut Copy, b: &mut Copy, room: &mut Held, name: &str) -> Merged {
    a.run("material", json!({"name": name, "color": "#aa5533"}));
    a.push(room);
    b.pull_all(room)
}

fn count_walls(m: &Model) -> usize {
    m.elements.values().filter(|e| matches!(e, Element::Wall(_))).count()
}

#[test]
fn a_wall_of_a_type_that_never_comes_waits_and_is_given_up_on_after_twenty_changes_to_the_library_and_said_once() {
    let (mut a, mut b, mut room) = two();
    let (wall, text, _) = wall_of_a_type_nobody_has();
    room.elements.insert(wall, text);
    let got = b.pull_all(&mut room);
    assert!(got.refused.is_empty(), "it waits, it is not refused: {:?}", got.refused);
    assert_eq!(b.s.waiting.get(&wall).map(|w| w.tried), Some(0));
    let mut said = 0;
    for i in 1..=25u32 {
        let got = a_new_material(&mut a, &mut b, &mut room, &format!("Clay {i}"));
        said += got.refused.len();
        if i < 20 {
            assert!(got.refused.is_empty(), "{i}: {:?}", got.refused);
            assert_eq!(b.s.waiting.get(&wall).map(|w| w.tried), Some(i), "tried once more each time the library changes");
        }
    }
    assert_eq!(said, 1, "given up on once, and said");
    assert!(b.s.waiting.is_empty());
    assert!(!b.model().elements.contains_key(wall));
}

#[test]
fn an_element_refused_for_something_other_than_a_missing_entry_is_refused_not_kept_waiting() {
    let (mut a, mut b, mut room) = two();
    let wall = a.run("wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap();
    let before = Arc::clone(&a.s.model);
    a.run("door", json!({"wall": wall, "offset": 3000}));
    // Only the door comes, to a copy that has no wall for it: nothing in the library is missing, the model would break.
    let c = sync::changes(&before, a.model()).unwrap();
    for (g, t) in c.elements.iter().filter(|(_, t)| t.as_deref().is_some_and(|t| t.contains("\"door\""))) {
        room.elements.insert(*g, t.clone().unwrap());
    }
    assert_eq!(room.elements.len(), 1 + Held::of(b.model()).elements.len(), "the door and nothing of the wall");
    let got = b.pull_all(&mut room);
    assert_eq!(got.refused.len(), 1, "{:?}", got.refused);
    assert!(b.s.waiting.is_empty() && got.waiting.is_empty());
}

#[test]
fn what_waits_is_offered_again_only_when_the_library_changes_and_apart_from_the_changes_that_came() {
    let (mut a, mut b, mut room) = two();
    let (junk, text, _) = wall_of_a_type_nobody_has();
    room.elements.insert(junk, text);
    b.pull_all(&mut room);
    // A wall comes and the library is the same: nothing waiting is offered again.
    a.run("wall", json!({"points": [[0, 5000], [6000, 5000]]}));
    a.push(&mut room);
    let got = b.pull_all(&mut room);
    assert!(got.refused.is_empty() && !got.taken.elements.is_empty(), "{:?}", got.refused);
    assert_eq!(b.s.waiting.get(&junk).map(|w| w.tried), Some(0), "not offered again: the library did not change");
    // A material and a wall come together: the wall is taken, the waiting one still fails on its own and refuses nothing.
    a.run("material", json!({"name": "Pigeon grey", "color": "#99bbcc"}));
    a.run("wall", json!({"points": [[0, 9000], [6000, 9000]]}));
    a.push(&mut room);
    let got = b.pull_all(&mut room);
    assert!(got.refused.is_empty(), "{:?}", got.refused);
    assert_eq!(count_walls(b.model()), 2);
    assert_eq!(b.s.waiting.get(&junk).map(|w| w.tried), Some(1), "offered again once, apart from the batch");
    assert_eq!(b.model().check(), Ok(()));
}

#[test]
fn what_waits_is_held_to_a_number_of_bytes_and_what_does_not_fit_is_said() {
    let (_, mut b, mut room) = two();
    let (_, text, ghost) = wall_of_a_type_nobody_has();
    for i in 0..9 {
        let long = format!("{i}{}", "x".repeat(1 << 20));
        room.elements.insert(Guid::new(), text.replace(&ghost.to_string(), &long));
    }
    let got = b.pull_all(&mut room);
    let held = b.s.waiting.len();
    assert!((5..9).contains(&held), "about eight MiB wait: {held}");
    assert_eq!(got.refused.len(), 9 - held, "what did not fit is said: {:?}", got.refused.iter().map(|r| &r.1).collect::<Vec<_>>());
    assert!(got.refused.iter().all(|(_, why)| why.contains("too many changes are waiting")), "{:?}", got.refused);
}

#[test]
fn three_copies_and_the_server_end_alike_when_one_takes_a_type_out_another_places_a_wall_of_it_and_a_wall_waits_for_a_type_that_never_comes()
 {
    let (mut a, mut b, mut room, beta) = both_have_beta();
    let text = a.model().to_json().unwrap();
    let open = |t: &str| {
        let mut x = Session::new();
        x.reset(Model::read(t).unwrap().0, None);
        x
    };
    let mut c = Copy::new(open(&text), &room);
    let mut server = Copy::new(open(&text), &room);
    let (junk, junk_text, _) = wall_of_a_type_nobody_has();
    a.run("delete_type", json!({"category": "wall", "type": "Beta"}));
    let wall = b.run("wall", json!({"points": [[0, 0], [6000, 0]], "type": "Beta"}))["ids"][0].as_u64().unwrap();
    let wall = b.model().elements.guid(wall).unwrap();
    a.push(&mut room);
    room.elements.insert(junk, junk_text);
    // B hears the taking out before it has sent its wall: the wall is its own change not sent yet, and it keeps the type for it.
    let got = b.pull_all(&mut room);
    assert!(b.model().types.wall.contains_key("Beta") && got.kept.len() == 1 && got.refused.is_empty(), "{:?} {:?}", got.kept, got.refused);
    b.push(&mut room);
    let mut refused = Vec::new();
    for _ in 0..6 {
        for x in [&mut a, &mut b, &mut c, &mut server] {
            refused.extend(x.pull_all(&mut room).refused);
            x.push(&mut room);
        }
    }
    assert!(refused.is_empty(), "nothing is refused on the way, what waits is not refused: {refused:?}");
    assert_beta_and_the_wall_everywhere(&[&a, &b, &c, &server], &room, beta, wall);
    for x in [&a, &b, &c, &server] {
        assert!(!x.model().elements.contains_key(junk), "the wall that waits is in none");
        assert!(x.s.waiting.contains_key(&junk), "and waits in each");
    }
}

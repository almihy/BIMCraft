//! Revit's flip controls as commands: `flip` turns walls round and flips a door's or window's hand and facing.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id, Opening};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn opening(s: &Session, id: Id) -> Opening {
    match s.model.elements.get(id) {
        Some(Element::Door(o) | Element::Window(o)) => o.clone(),
        e => panic!("{id} is {e:?}"),
    }
}

fn flips(s: &Session, id: Id) -> (bool, bool) {
    let o = opening(s, id);
    (o.flip_hand, o.flip_facing)
}

fn wall_with_openings() -> (Session, Id, Id, Id) {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap_or_default();
    let door = run(&mut s, "door", json!({"wall": w, "offset": 1500}))["id"].as_u64().unwrap_or_default();
    let window = run(&mut s, "window", json!({"wall": w, "offset": 4500}))["id"].as_u64().unwrap_or_default();
    (s, w, door, window)
}

#[test]
fn flip_turns_a_doors_or_windows_facing_by_default_and_its_hand_when_asked() {
    let (mut s, _, door, window) = wall_with_openings();
    let at = (opening(&s, door).offset, opening(&s, window).offset);
    // Nothing said: the facing, the swing to the wall's other side.
    run(&mut s, "flip", json!({"ids": [door, window]}));
    assert_eq!((flips(&s, door), flips(&s, window)), ((false, true), (false, true)));
    // The hand alone, then both.
    run(&mut s, "flip", json!({"ids": [door], "hand": true}));
    assert_eq!(flips(&s, door), (true, true));
    run(&mut s, "flip", json!({"ids": [door], "hand": true, "facing": true}));
    assert_eq!(flips(&s, door), (false, false));
    // Flipping twice is no flip, and an opening stays where it stands.
    run(&mut s, "flip", json!({"ids": [window], "facing": true}));
    assert_eq!(flips(&s, window), (false, false));
    assert_eq!((opening(&s, door).offset, opening(&s, window).offset), at);
}

#[test]
fn flip_takes_walls_and_openings_together_and_refuses_anything_else() {
    let (mut s, w, door, _) = wall_with_openings();
    let before = opening(&s, door);
    // The wall turns round (its openings keep their places and turn with it); the door asked for turns its hand too.
    run(&mut s, "flip", json!({"ids": [w, door], "hand": true}));
    let after = opening(&s, door);
    assert_eq!((after.flip_hand, after.flip_facing), (before.flip_hand, !before.flip_facing));
    assert_eq!(after.offset, 6000.0 - before.offset);
    let level = s.model.elements.iter().find(|(_, e)| e.category() == "level").map(|(id, _)| *id).unwrap_or_default();
    let e = s.execute("flip", &json!({"ids": [level]})).unwrap_err();
    assert!(e.contains("wall") && e.contains("door"), "{e}");
    assert!(s.execute("flip", &json!({"ids": [door], "hand": "yes"})).is_err());
}

#[test]
fn a_door_or_window_asked_toward_a_point_faces_the_side_of_the_wall_it_is_on() {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [20000, 0]]}))["ids"][0].as_u64().unwrap_or_default();
    // The wall runs east: its exterior, the default side, is to its left, north.
    let facing = |s: &mut Session, cat: &str, p: Value| {
        let id = run(s, cat, p)["id"].as_u64().unwrap_or_default();
        opening(s, id).flip_facing
    };
    assert!(!facing(&mut s, "door", json!({"wall": w, "offset": 2000, "toward": [2000, 900]})));
    assert!(facing(&mut s, "door", json!({"wall": w, "offset": 5000, "toward": [5000, -900]})));
    assert!(facing(&mut s, "window", json!({"at": [8000, 0], "toward": [8000, -10]})));
    // Asked to flip too, it goes the other way from the point's side.
    assert!(facing(&mut s, "door", json!({"wall": w, "offset": 11000, "toward": [11000, 900], "flip_facing": true})));
    assert!(s.execute("door", &json!({"wall": w, "offset": 15000, "toward": "north"})).is_err());
}

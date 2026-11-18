//! Revit's flip controls as commands: `flip` turns walls round and flips a door's or window's hand and facing.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, Opening};
use crate::walls::{self, Axis};

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

#[test]
fn a_facing_said_to_be_false_is_not_flipped_and_only_a_facing_left_out_defaults_to_flipping() {
    let (mut s, _, door, _) = wall_with_openings();
    // `facing: false` with no `hand` flips nothing.
    run(&mut s, "flip", json!({"ids": [door], "facing": false}));
    assert_eq!(flips(&s, door), (false, false));
    run(&mut s, "flip", json!({"ids": [door], "facing": false, "hand": false}));
    assert_eq!(flips(&s, door), (false, false));
    // Left out, or null, it is the facing.
    run(&mut s, "flip", json!({"ids": [door], "facing": null}));
    assert_eq!(flips(&s, door), (false, true));
    run(&mut s, "flip", json!({"ids": [door], "hand": false}));
    assert_eq!(flips(&s, door), (false, false));
    // The hand alone leaves the facing as it was.
    run(&mut s, "flip", json!({"ids": [door], "hand": true}));
    assert_eq!(flips(&s, door), (true, false));
}

#[test]
fn toward_on_a_curved_wall_is_the_side_of_its_curve_at_the_door_not_of_its_chord() {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [10000, 0]], "arc": 90}))["ids"][0].as_u64().unwrap_or_default();
    // Where the door goes, and which way the wall's left is there: the chord's left is north, the curve's here is not.
    let (at, left) = match s.model.wall(w) {
        Some(wall) => {
            let axis = Axis::of(wall);
            (axis.at(1000.0, walls::centre(&s.model, wall)), axis.tangent(1000.0).perp())
        }
        None => panic!("no wall {w}"),
    };
    let q = at + P2(-1000.0, 500.0);
    assert!(q.1 > at.1 && (q - at).dot(left) < 0.0, "the point is on the chord's left and the curve's right");
    let id = run(&mut s, "door", json!({"wall": w, "offset": 1000, "toward": [q.0, q.1]}))["id"].as_u64().unwrap_or_default();
    assert!(opening(&s, id).flip_facing, "swings to the curve's right");
    let q = at + left * 300.0;
    let id = run(&mut s, "door", json!({"wall": w, "offset": 1000, "toward": [q.0, q.1]}))["id"].as_u64().unwrap_or_default();
    assert!(!opening(&s, id).flip_facing, "swings to the curve's left");
}

#[test]
fn flip_step_takes_each_door_and_window_through_its_four_ways_from_its_own_state() {
    let (mut s, w, door, window) = wall_with_openings();
    let door2 = run(&mut s, "door", json!({"wall": w, "offset": 3000}))["id"].as_u64().unwrap_or_default();
    // The second door is a step ahead of the first.
    run(&mut s, "flip", json!({"ids": [door2]}));
    assert_eq!((flips(&s, door), flips(&s, door2)), ((false, false), (false, true)));
    run(&mut s, "flip", json!({"ids": [door, door2, window], "step": true}));
    // Each stepped from its own state: (false,false) -> facing; (false,true) -> hand; a window as a door.
    assert_eq!((flips(&s, door), flips(&s, door2), flips(&s, window)), ((false, true), (true, true), (false, true)));
    run(&mut s, "flip", json!({"ids": [door, door2, window], "step": true}));
    assert_eq!((flips(&s, door), flips(&s, door2), flips(&s, window)), ((true, true), (true, false), (true, true)));
    // A window goes round the same four ways: facing, hand, facing back, hand back.
    let mut seen = Vec::new();
    for _ in 0..4 {
        run(&mut s, "flip", json!({"ids": [window], "step": true}));
        seen.push(flips(&s, window));
    }
    assert_eq!(seen, [(true, false), (false, false), (false, true), (true, true)]);
    // `step` is the whole instruction: `hand` and `facing` beside it are refused.
    assert!(s.execute("flip", &json!({"ids": [door], "step": true, "hand": true})).is_err());
}

#[test]
fn a_family_windows_hand_mirrors_it_on_the_plan_in_3d_and_in_ifc() {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]]}))["ids"][0].as_u64().unwrap_or_default();
    let window = run(&mut s, "window", json!({"wall": w, "offset": 3000, "family": "Casement Window"}))["id"].as_u64().unwrap_or_default();
    let shape = |s: &Session| {
        let e = s.model.elements.get(window).cloned().unwrap_or_else(|| panic!("no window"));
        let (pieces, marks) = s.model.opening_local(&e).unwrap_or_else(|| panic!("a family's window builds"));
        format!("{:?} {:?}", pieces.iter().map(|p| &p.outline).collect::<Vec<_>>(), marks)
    };
    let ifc = |s: &Session| crate::ifc::export(&s.model);
    let (before, before_ifc) = (shape(&s), ifc(&s));
    run(&mut s, "flip", json!({"ids": [window], "hand": true}));
    assert!(flips(&s, window).0);
    assert_ne!(shape(&s), before, "its sash on the other jamb");
    assert_ne!(ifc(&s), before_ifc, "the exported body mirrored too");
    // And it is kept in the file.
    let back = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    assert!(matches!(back.elements.get(window), Some(Element::Window(o)) if o.flip_hand));
}

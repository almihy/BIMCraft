use crate::engine::Session;
use crate::geom::P2;
use crate::model::Face;
use crate::tempdims::{self, Side, Witness};
use serde_json::{Value, json};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// An 8 x 6 m box: walls 100002 (south, running east), 100003, 100004 (north, running west) and 100005.
fn house(s: &mut Session) {
    run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
}

fn wall_at(s: &mut Session, from: [f64; 2], to: [f64; 2]) -> u64 {
    run(s, "wall", json!({"points": [from, to]}))["ids"][0].as_u64().unwrap()
}

/// Each dimension of wall `id` as its side, what it ends at (an id) and how far.
fn dims(s: &Session, id: u64, witness: Witness) -> Vec<(Side, u64, f64)> {
    let level = s.model.wall(id).unwrap().level;
    tempdims::wall(&s.model, id, level, witness).into_iter().map(|d| (d.side, s.model.elements.id(d.to.id).unwrap(), d.distance)).collect()
}

#[test]
fn a_wall_measures_to_the_nearest_parallel_wall_on_each_side() {
    let mut s = Session::new();
    house(&mut s);
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5000.0, 2000.0]);
    // The inner wall, running east: the north wall is 4000 off on its left, the south wall 2000 on its right.
    assert_eq!(dims(&s, inner, Witness::default()), [(Side::Left, 100004, 4000.0), (Side::Right, 100002, 2000.0)]);
    // The south wall sees the inner wall, nearer than the north one, and nothing on its other side.
    assert_eq!(dims(&s, 100002, Witness::default()), [(Side::Left, inner, 2000.0)]);
    // The east wall runs across the others, so only the west wall is parallel to it.
    assert_eq!(dims(&s, 100003, Witness::default()), [(Side::Left, 100005, 8000.0)]);
    // Its line goes square to the wall, through the middle of what both share.
    let level = s.model.wall(inner).unwrap().level;
    let d = tempdims::wall(&s.model, inner, level, Witness::default());
    assert_eq!((d[0].start, d[0].end, d[0].toward), (P2(3000.0, 2000.0), P2(3000.0, 6000.0), P2(0.0, 1.0)));
    assert_eq!((d[1].start, d[1].end, d[1].toward), (P2(3000.0, 2000.0), P2(3000.0, 0.0), P2(0.0, -1.0)));
}

#[test]
fn a_grid_is_measured_to_as_a_wall_is_when_it_shares_some_of_the_run() {
    let mut s = Session::new();
    let w = wall_at(&mut s, [0.0, 0.0], [4000.0, 0.0]);
    run(&mut s, "grid", json!({"start": [-1000, -1500], "end": [9000, -1500]}));
    let near = run(&mut s, "grid", json!({"start": [-1000, -600], "end": [2000, -600]}))["id"].as_u64().unwrap();
    // Alongside the wall's far end only, and across it: neither counts.
    run(&mut s, "grid", json!({"start": [5000, -300], "end": [7000, -300]}));
    run(&mut s, "grid", json!({"start": [1000, -2000], "end": [1000, 2000]}));
    assert_eq!(dims(&s, w, Witness::default()), [(Side::Right, near, 600.0)]);
    // A wall on the same line is no distance away, and is passed over.
    wall_at(&mut s, [4500.0, 0.0], [6000.0, 0.0]);
    assert_eq!(dims(&s, w, Witness::default()).len(), 1);
}

#[test]
fn a_wall_with_nothing_parallel_beside_it_has_no_temporary_dimensions() {
    let mut s = Session::new();
    let lone = wall_at(&mut s, [0.0, 0.0], [4000.0, 0.0]);
    assert!(dims(&s, lone, Witness::default()).is_empty());
    // A wall across it, and one that only meets its end, are none either.
    wall_at(&mut s, [2000.0, 500.0], [2000.0, 3000.0]);
    wall_at(&mut s, [4000.0, 1000.0], [8000.0, 1000.0]);
    assert!(dims(&s, lone, Witness::default()).is_empty());
    let door = run(&mut s, "door", json!({"wall": lone, "offset": 1000}))["id"].as_u64().unwrap();
    let level = s.model.wall(lone).unwrap().level;
    assert!(tempdims::wall(&s.model, door, level, Witness::default()).is_empty(), "a door isn't a wall");
    let curved = run(&mut s, "wall", json!({"points": [[0, 3000], [4000, 3000]], "arc": 90}))["ids"][0].as_u64().unwrap();
    assert!(dims(&s, curved, Witness::default()).is_empty(), "a curved wall has no parallel lines");
}

#[test]
fn an_angled_wall_is_measured_square_to_itself() {
    let mut s = Session::new();
    // Along (3, 4)/5; its left is (-0.8, 0.6).
    let w = wall_at(&mut s, [0.0, 0.0], [3000.0, 4000.0]);
    let beside = wall_at(&mut s, [-800.0, 600.0], [2200.0, 4600.0]);
    // Not parallel, but near enough to cross: left out.
    wall_at(&mut s, [-2000.0, 1500.0], [2000.0, 3500.0]);
    let on = dims(&s, w, Witness::default());
    assert_eq!(on.len(), 1);
    assert_eq!((on[0].0, on[0].1), (Side::Left, beside));
    assert!((on[0].2 - 1000.0).abs() < 1e-6, "{}", on[0].2);
    let back = dims(&s, beside, Witness::default());
    assert_eq!((back[0].0, back[0].1), (Side::Right, w));
    assert!((back[0].2 - 1000.0).abs() < 1e-6);
}

#[test]
fn the_witness_line_cycles_from_the_centre_to_the_exterior_face_to_the_interior_face() {
    let mut s = Session::new();
    house(&mut s);
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5000.0, 2000.0]);
    let half = s.model.thickness(s.model.wall(inner).unwrap()) / 2.0;
    let mut witness = Witness::default();
    assert_eq!(dims(&s, inner, witness)[0].2, 4000.0);
    // The exterior face is on the left: nearer the north wall.
    witness = witness.cycled(Side::Left);
    assert_eq!(witness.of(Side::Left), Face::Left);
    assert_eq!(witness.of(Side::Right), Face::Centre, "the other side is its own");
    assert_eq!(dims(&s, inner, witness)[0].2, 4000.0 - half);
    witness = witness.cycled(Side::Left);
    assert_eq!(witness.of(Side::Left), Face::Right);
    assert_eq!(dims(&s, inner, witness)[0].2, 4000.0 + half);
    assert_eq!(witness.cycled(Side::Left).of(Side::Left), Face::Centre);
    // The interior face, on the south side, is nearer the south wall.
    let right = Witness { right: Face::Right, ..Witness::default() };
    assert_eq!(dims(&s, inner, right)[1].2, 2000.0 - half);
}

#[test]
fn the_temporary_dimensions_command_reads_them_for_the_agent_and_changes_nothing() {
    let mut s = Session::new();
    house(&mut s);
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5000.0, 2000.0]);
    let before = s.model.clone();
    let r = run(&mut s, "temporary_dimensions", json!({"id": inner}));
    assert_eq!(s.model.elements, before.elements);
    assert_eq!(r["length"], 4000.0);
    let ds = r["dimensions"].as_array().unwrap();
    assert_eq!(ds.len(), 2);
    assert_eq!((ds[0]["side"].as_str(), ds[0]["to"]["id"].as_u64(), ds[0]["distance"].as_f64()), (Some("left"), Some(100004), Some(4000.0)));
    assert_eq!(ds[1]["toward"], json!([0.0, -1.0]));
    // The ready-made params of a permanent dimension make one that reads what the temporary one did.
    let made = run(&mut s, "dimension", ds[0]["dimension"].clone());
    assert_eq!(made["values"], json!([4000.0]));
    // Typing is a move: toward the wall by the distance less what was typed.
    let by: Vec<f64> = ds[1]["toward"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() * (2000.0 - 500.0)).collect();
    run(&mut s, "move", json!({"ids": [inner], "by": by}));
    assert_eq!(s.model.wall(inner).unwrap().start, P2(1000.0, 500.0));
    assert_eq!(run(&mut s, "temporary_dimensions", json!({"id": inner}))["dimensions"][1]["distance"], 500.0);
    // The faces a person cycles through, by name.
    let r = run(&mut s, "temporary_dimensions", json!({"id": inner, "left": "left", "right": "right"}));
    let half = s.model.thickness(s.model.wall(inner).unwrap()) / 2.0;
    assert_eq!(r["dimensions"][0]["from"]["face"], "left");
    assert_eq!(r["dimensions"][1]["distance"].as_f64(), Some(500.0 - half));
}

#[test]
fn the_temporary_dimensions_command_refuses_what_is_not_a_wall_and_what_it_cannot_read() {
    let mut s = Session::new();
    house(&mut s);
    let grid = run(&mut s, "grid", json!({"start": [0, -500], "end": [8000, -500]}))["id"].as_u64().unwrap();
    for p in [
        json!({"id": grid}),
        json!({"id": 999999}),
        json!({}),
        json!({"id": 100002, "left": "up"}),
        json!({"id": 100002, "level": "Nowhere"}),
        json!({"id": 100002, "extra": 1}),
    ] {
        assert!(s.execute("temporary_dimensions", &p).is_err(), "{p}");
    }
    let before = s.model.clone();
    assert!(s.execute("temporary_dimensions", &json!({"id": 100002, "level": "Level 2"})).is_ok());
    assert_eq!(s.model.elements, before.elements);
}

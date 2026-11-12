//! `constraints`'s tests that drive the Session (moved out of `constraints.rs`).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Id;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// Two walls running north, apart and not joined: the west one's centre at x = 0, the east one's at `x`; and a dimension
/// between their centre lines, south of them.
fn two_walls(s: &mut Session, x: f64) -> (Id, Id, Id) {
    let one = |s: &mut Session, p: Value| run(s, "wall", p)["ids"][0].as_u64().unwrap_or_default();
    let a = one(s, json!({"points": [[0, 0], [0, 4000]]}));
    let b = one(s, json!({"points": [[x, 0], [x, 4000]]}));
    let d = run(s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [x / 2.0, -1000]}))["id"].as_u64().unwrap_or_default();
    (a, b, d)
}

fn start_x(s: &Session, id: Id) -> f64 {
    s.model.wall(id).map_or(f64::NAN, |w| w.start.0)
}

#[test]
fn a_length_typed_into_a_dimension_moves_its_second_reference_and_a_locked_segment_is_locked_at_it() {
    let mut s = Session::new();
    let (a, b, d) = two_walls(&mut s, 2000.0);
    let out = run(&mut s, "dimension_value", json!({"id": d, "value": 3000}));
    assert_eq!((out["moved"].as_u64(), start_x(&s, a), start_x(&s, b)), (Some(b), 0.0, 3000.0), "the second wall moved");
    assert_eq!(
        s.model.dimension_values(match s.model.elements.get(d) {
            Some(crate::model::Element::Dimension(d)) => d,
            other => panic!("{other:?}"),
        }),
        vec![3000.0]
    );
    // Locked, typing changes the lock: the first wall moved, the second keeps 2500 from it.
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
    run(&mut s, "dimension_value", json!({"id": d, "value": 2500}));
    run(&mut s, "move", json!({"ids": [a], "by": [-400, 0]}));
    assert_eq!((start_x(&s, a), start_x(&s, b)), (-400.0, 2100.0));
    // Refused: no such segment, nothing long, not a dimension.
    for p in [json!({"id": d, "value": 1000, "segment": 1}), json!({"id": d, "value": 0}), json!({"id": a, "value": 1000})] {
        assert!(s.execute("dimension_value", &p).is_err(), "{p}");
    }
}

#[test]
fn align_with_its_lock_keeps_the_two_lines_on_one_line_when_the_first_wall_moves() {
    let mut s = Session::new();
    let one = |s: &mut Session, p: Value| run(s, "wall", p)["ids"][0].as_u64().unwrap_or_default();
    let a = one(&mut s, json!({"points": [[0, 0], [0, 4000]]}));
    let b = one(&mut s, json!({"points": [[500, 6000], [500, 9000]]}));
    let out = run(&mut s, "align", json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[500, 6000], [500, 9000]], "lock": true}));
    assert_eq!((start_x(&s, b), out["line"].clone()), (0.0, json!([[0.0, 6000.0], [0.0, 9000.0]])));
    let lock = out["lock"].as_u64().unwrap_or_default();
    assert!(matches!(s.model.elements.get(lock), Some(crate::model::Element::Dimension(d)) if d.alignment && d.refs.len() == 2));
    assert_eq!(s.model.held_by(b), vec![lock]);
    // The first wall moved, then turned back: the second follows it, each time.
    run(&mut s, "move", json!({"ids": [a], "by": [300, 0]}));
    assert_eq!((start_x(&s, a), start_x(&s, b)), (300.0, 300.0));
    run(&mut s, "move", json!({"ids": [a], "by": [-1300, 0]}));
    assert_eq!(start_x(&s, b), -1000.0);
    // It isn't drawn: a plan shows nothing of it.
    let level = s.model.levels()[0].0;
    let view = crate::prim::View { scale: 0.1, cx: 0.0, cy: 4000.0, width: 800, height: 1200 };
    assert!(!crate::render::draw(&s.model, level, view).owners.contains(&Some(lock)), "a lock is a front end's to show");
    // Unlocked (deleted), the walls are free again.
    run(&mut s, "delete", json!({"ids": [lock]}));
    run(&mut s, "move", json!({"ids": [a], "by": [500, 0]}));
    assert_eq!((start_x(&s, a), start_x(&s, b)), (-500.0, -1000.0));
}

#[test]
fn a_lock_that_cant_hold_with_the_others_is_refused_and_the_project_is_as_it_was() {
    let mut s = Session::new();
    // The second wall north of the first, so aligned they lie end to end.
    let one = |s: &mut Session, p: Value| run(s, "wall", p)["ids"][0].as_u64().unwrap_or_default();
    let a = one(&mut s, json!({"points": [[0, 0], [0, 4000]]}));
    let b = one(&mut s, json!({"points": [[2000, 6000], [2000, 9000]]}));
    let d = run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}], "at": [1000, -1000]}))["id"].as_u64().unwrap_or_default();
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
    let before = s.model.elements.len();
    // Locked 2000 apart, and locked on one line: they can't be both.
    let e = s
        .execute("align", &json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[2000, 6000], [2000, 9000]], "lock": true}))
        .unwrap_err();
    assert!(e.contains("constraints"), "{e}");
    assert_eq!((s.model.elements.len(), start_x(&s, a), start_x(&s, b)), (before, 0.0, 2000.0), "as it was");
    // Without the lock it aligns, and the locked dimension takes the first wall along.
    run(&mut s, "align", json!({"id": b, "reference": [[0, 0], [0, 4000]], "line": [[2000, 6000], [2000, 9000]]}));
    assert_eq!((start_x(&s, a), start_x(&s, b)), (-2000.0, 0.0));
}

#[test]
fn an_element_is_held_by_the_dimensions_whose_locked_or_equal_segments_end_at_it() {
    let mut s = Session::new();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let w: Vec<Id> = w["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    let g = run(&mut s, "grid", json!({"start": [2000, -2000], "end": [2000, 8000]}))["id"].as_u64().unwrap_or_default();
    // West wall to grid, then grid to east wall: the first segment locked.
    let d = run(&mut s, "dimension", json!({"refs": [{"id": w[3]}, {"id": g}, {"id": w[1]}], "at": [0, -1000]}))["id"].as_u64();
    let d = d.unwrap_or_default();
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [0]}));
    let held = |s: &Session, id: Id| s.model.held_by(id);
    let none: Vec<Id> = Vec::new();
    assert_eq!((held(&s, w[3]), held(&s, g)), (vec![d], vec![d]));
    assert_eq!(
        (held(&s, w[1]), held(&s, w[0])),
        (none.clone(), none.clone()),
        "the east wall's segment is free; the south wall isn't measured"
    );
    // Equal, it holds all it measures to; with no constraint, nothing.
    run(&mut s, "dimension_constraint", json!({"id": d, "locked": [], "equal": true}));
    assert_eq!(held(&s, w[1]), vec![d]);
    run(&mut s, "dimension_constraint", json!({"id": d, "equal": false}));
    assert_eq!(held(&s, g), none);
}

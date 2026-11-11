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
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5500.0, 2000.0]);
    // The inner wall, running east: the north wall is 4000 off on its left, the south wall 2000 on its right.
    assert_eq!(dims(&s, inner, Witness::default()), [(Side::Left, 100004, 4000.0), (Side::Right, 100002, 2000.0)]);
    // The south wall sees the inner wall, nearer than the north one, and nothing on its other side.
    assert_eq!(dims(&s, 100002, Witness::default()), [(Side::Left, inner, 2000.0)]);
    // The east wall runs across the others, so only the west wall is parallel to it.
    assert_eq!(dims(&s, 100003, Witness::default()), [(Side::Left, 100005, 8000.0)]);
    // Each goes square to the wall, a third and two thirds of the way along what it shares with the walls it measures to.
    let level = s.model.wall(inner).unwrap().level;
    let d = tempdims::wall(&s.model, inner, level, Witness::default());
    assert_eq!((d[0].start, d[0].end, d[0].toward), (P2(2500.0, 2000.0), P2(2500.0, 6000.0), P2(0.0, 1.0)));
    assert_eq!((d[1].start, d[1].end, d[1].toward), (P2(4000.0, 2000.0), P2(4000.0, 0.0), P2(0.0, -1.0)));
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
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5500.0, 2000.0]);
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
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5500.0, 2000.0]);
    let before = s.model.clone();
    let r = run(&mut s, "temporary_dimensions", json!({"id": inner}));
    assert_eq!(s.model.elements, before.elements);
    assert_eq!(r["length"], 4500.0);
    let ds = r["dimensions"].as_array().unwrap();
    assert_eq!(ds.len(), 2);
    assert_eq!(
        (ds[0]["side"].as_str(), ds[0]["to"]["id"].as_u64(), ds[0]["distance"].as_f64()),
        (Some("left"), Some(100004), Some(4000.0))
    );
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

#[test]
fn the_ready_made_dimension_of_the_command_is_in_the_plan_it_was_read_in() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "level": "Level 2"}));
    run(&mut s, "wall", json!({"points": [[0, 3000], [8000, 3000]], "level": "Level 2"}));
    let walls = s.model.elements.iter().filter(|(_, e)| matches!(e, crate::model::Element::Wall(_))).map(|(id, _)| *id).collect::<Vec<_>>();
    // Read without a level: the wall's own, whichever that is.
    let r = run(&mut s, "temporary_dimensions", json!({"id": walls[0]}));
    let made = run(&mut s, "dimension", r["dimensions"][0]["dimension"].clone());
    let Some(crate::model::Element::Dimension(d)) = s.model.elements.get(made["id"].as_u64().unwrap()) else { panic!("a dimension") };
    let level_2 = s.model.levels().into_iter().find(|(_, l)| l.name == "Level 2").map(|(id, _)| id).unwrap();
    assert_eq!(d.level, s.model.guid(level_2), "on Level 2, not the lowest level");
    // Asked in a level by name, the ready-made params name it too.
    let r = run(&mut s, "temporary_dimensions", json!({"id": walls[0], "level": "Level 2"}));
    let made = run(&mut s, "dimension", r["dimensions"][0]["dimension"].clone());
    let Some(crate::model::Element::Dimension(d)) = s.model.elements.get(made["id"].as_u64().unwrap()) else { panic!("a dimension") };
    assert_eq!(d.level, s.model.guid(level_2));
}

#[test]
fn a_wall_is_measured_only_to_what_the_plan_shows() {
    let mut s = Session::new();
    let me = wall_at(&mut s, [0.0, 0.0], [8000.0, 0.0]);
    let gone = wall_at(&mut s, [0.0, 1000.0], [8000.0, 1000.0]);
    let far = wall_at(&mut s, [0.0, 3000.0], [8000.0, 3000.0]);
    let grid = run(&mut s, "grid", json!({"start": [0, 2000], "end": [8000, 2000]}))["id"].as_u64().unwrap();
    let to = |s: &Session| dims(s, me, Witness::default()).into_iter().map(|(_, id, d)| (id, d)).collect::<Vec<_>>();
    assert_eq!(to(&s), [(gone, 1000.0)]);
    // Taken down in the first phase, the nearer wall isn't in the plan of the last: the next one is measured to.
    run(&mut s, "modify", json!({"id": gone, "params": {"Phase Created": "Existing", "Phase Demolished": "Existing"}}));
    assert_eq!(to(&s), [(grid, 2000.0)]);
    // A grid the plan hides is not measured to either.
    run(&mut s, "graphics", json!({"view": "Level 1", "categories": {"grids": {"hidden": true}}}));
    assert_eq!(to(&s), [(far, 3000.0)]);
    // And a wall the plan doesn't show has none.
    run(&mut s, "modify", json!({"id": me, "params": {"Phase Created": "Existing", "Phase Demolished": "Existing"}}));
    assert!(to(&s).is_empty());
}

#[test]
fn a_tall_walls_dimensions_in_the_plan_of_the_level_above_measure_what_that_plan_shows() {
    let mut s = Session::new();
    // A Level 1 wall 8000 tall, cut by Level 2's plan too; a Level 1 wall 3000 north, which that plan does not cut; and a
    // Level 2 wall 5000 north, which the first plan does not show.
    let tall = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "height": 8000}))["ids"][0].as_u64().unwrap();
    let low = run(&mut s, "wall", json!({"points": [[0, 3000], [8000, 3000]]}))["ids"][0].as_u64().unwrap();
    let up = run(&mut s, "wall", json!({"points": [[0, 5000], [8000, 5000]], "level": "Level 2"}))["ids"][0].as_u64().unwrap();
    let reads = |s: &mut Session, level: Option<&str>| {
        let p = match level {
            Some(l) => json!({"id": tall, "level": l}),
            None => json!({"id": tall}),
        };
        let r = run(s, "temporary_dimensions", p);
        r["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| (d["to"]["id"].as_u64().unwrap(), d["distance"].as_f64().unwrap()))
            .collect::<Vec<_>>()
    };
    assert_eq!(reads(&mut s, None), [(low, 3000.0)], "in its own level's plan");
    assert_eq!(reads(&mut s, Some("Level 2")), [(up, 5000.0)], "in Level 2's plan");
    // The window passes the plan it views: the same measure.
    let level_2 = s.model.levels().into_iter().find(|(_, l)| l.name == "Level 2").map(|(id, _)| id).unwrap();
    let d = tempdims::wall(&s.model, tall, level_2, Witness::default());
    assert_eq!(d.iter().map(|d| (s.model.elements.id(d.to.id).unwrap(), d.distance)).collect::<Vec<_>>(), [(up, 5000.0)]);
}

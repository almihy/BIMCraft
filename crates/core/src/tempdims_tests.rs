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
    let core = tempdims::has_core(&s.model, inner);
    assert!(!core, "a wall of one thickness has no core of its own");
    let mut witness = Witness::default();
    let now = |s: &Session, w: Witness| tempdims::wall(&s.model, inner, s.model.wall(inner).unwrap().level, w)[0].from.face;
    assert_eq!(dims(&s, inner, witness)[0].2, 4000.0);
    // The exterior face is on the left: nearer the north wall.
    witness = witness.cycled(Side::Left, now(&s, witness), core);
    assert_eq!(witness.of(Side::Left), Some(Face::Left));
    assert_eq!(witness.of(Side::Right), None, "the other side is its own");
    assert_eq!(dims(&s, inner, witness)[0].2, 4000.0 - half);
    witness = witness.cycled(Side::Left, now(&s, witness), core);
    assert_eq!(witness.of(Side::Left), Some(Face::Right));
    assert_eq!(dims(&s, inner, witness)[0].2, 4000.0 + half);
    assert_eq!(witness.cycled(Side::Left, now(&s, witness), core).of(Side::Left), Some(Face::Centre));
    // The interior face, on the south side, is nearer the south wall.
    let right = Witness { right: Some(Face::Right), ..Witness::default() };
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

/// Two "Exterior - 300mm" walls (brick 100, insulation 50, a 140 mm block core, plaster 10, from the exterior in), running east,
/// the first on y = 0 and the second on y = 4000, location lines at their centres: a wall's exterior face is 150 on its left, its
/// interior face 150 on its right, its core runs from 0 to 140 on the right of the location line.
fn cored(s: &mut Session) -> (u64, u64) {
    let wall = |s: &mut Session, y: f64| {
        run(s, "wall", json!({"points": [[0.0, y], [6000.0, y]], "type": "Exterior - 300mm"}))["ids"][0].as_u64().unwrap()
    };
    (wall(s, 0.0), wall(s, 4000.0))
}

#[test]
fn a_wall_with_a_core_has_its_centre_and_faces_to_cycle_through() {
    let mut s = Session::new();
    let (a, b) = cored(&mut s);
    assert!(tempdims::has_core(&s.model, a));
    let level = s.model.wall(a).unwrap().level;
    let left = |s: &Session, w: Witness| tempdims::wall(&s.model, a, level, w).into_iter().find(|d| d.side == Side::Left).unwrap();
    // Centre line to the neighbour's centre line, as the settings start: 4000.
    let mut witness = Witness::default();
    let mut seen = Vec::new();
    for _ in 0..7 {
        let d = left(&s, witness);
        seen.push((d.from.face, d.distance));
        witness = witness.cycled(Side::Left, d.from.face, true);
    }
    // The neighbour's centre line is on y = 4000; the wall's lines are on y = 0 (centre), 150 (exterior face), -150 (interior
    // face), -70 (core centre), 0 (core exterior face) and -140 (core interior face).
    assert_eq!(
        seen,
        [
            (Face::Centre, 4000.0),
            (Face::Left, 3850.0),
            (Face::Right, 4150.0),
            (Face::CoreCentre, 4070.0),
            (Face::CoreLeft, 4000.0),
            (Face::CoreRight, 4140.0),
            (Face::Centre, 4000.0),
        ]
    );
    // The far witness line stays at the neighbour's centre line, whichever the near one is.
    assert_eq!(left(&s, witness).to, crate::model::Reference::line(s.model.guid(b), Face::Centre));
}

#[test]
fn the_projects_temporary_dimension_settings_say_which_lines_a_wall_measures_when_none_was_chosen() {
    let mut s = Session::new();
    let (a, _) = cored(&mut s);
    let level = s.model.wall(a).unwrap().level;
    let left = |s: &Session| tempdims::wall(&s.model, a, level, Witness::default()).into_iter().find(|d| d.side == Side::Left).unwrap();
    let as_set = |s: &mut Session, walls: &str| {
        run(s, "temporary_dimension_settings", json!({"walls": walls}));
        let d = left(s);
        (d.from.face, d.to.face, d.distance)
    };
    // Centrelines: the default, as before. Faces: the face of each toward the other (a's exterior face, 3850 on to b's interior).
    assert_eq!(as_set(&mut s, "centerlines"), (Face::Centre, Face::Centre, 4000.0));
    assert_eq!(as_set(&mut s, "faces"), (Face::Left, Face::Right, 3700.0));
    // The core's centre lines, the core's faces: b's core is the same distance off, its nearer face 3860.
    assert_eq!(as_set(&mut s, "core-centre"), (Face::CoreCentre, Face::CoreCentre, 4000.0));
    assert_eq!(as_set(&mut s, "core-faces"), (Face::CoreLeft, Face::CoreRight, 3860.0));
    // A wall with no core of its own measures its centre line or its faces in their place.
    let mut plain = Session::new();
    for y in [0.0, 3000.0] {
        run(&mut plain, "wall", json!({"points": [[0.0, y], [6000.0, y]], "type": "Generic - 200mm"}));
    }
    let id = plain.model.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let lvl = plain.model.wall(id).unwrap().level;
    let of = |s: &mut Session, walls: &str| {
        run(s, "temporary_dimension_settings", json!({"walls": walls}));
        let d = tempdims::wall(&s.model, id, lvl, Witness::default()).remove(0);
        (d.from.face, d.to.face, d.distance)
    };
    assert_eq!(of(&mut plain, "core-centre"), (Face::Centre, Face::Centre, 3000.0));
    assert_eq!(of(&mut plain, "core-faces"), (Face::Left, Face::Right, 2800.0));
    // A person's own choice beats the setting for that side.
    run(&mut s, "temporary_dimension_settings", json!({"walls": "faces"}));
    let chosen = Witness { left: Some(Face::Centre), ..Witness::default() };
    assert_eq!(tempdims::wall(&s.model, a, level, chosen)[0].from.face, Face::Centre);
}

#[test]
fn the_settings_are_kept_in_the_project_and_read_back() {
    let mut s = Session::new();
    // Revit's defaults, and nothing in the file for them.
    let r = run(&mut s, "temporary_dimension_settings", json!({}));
    assert_eq!((r["walls"].as_str(), r["openings"].as_str()), (Some("centerlines"), Some("centerlines")));
    assert!(serde_json::to_value(&*s.model).unwrap().get("temporary_dimensions").is_none());
    let r = run(&mut s, "temporary_dimension_settings", json!({"walls": "core-faces", "openings": "openings"}));
    assert_eq!((r["walls"].as_str(), r["openings"].as_str()), (Some("core-faces"), Some("openings")));
    let saved = serde_json::to_value(&*s.model).unwrap();
    assert_eq!(saved["temporary_dimensions"], json!({"walls": "core-faces", "openings": "openings"}));
    assert_eq!(saved["format"], crate::model::FORMAT);
    let (back, left_out) = crate::model::Model::read(&saved.to_string()).unwrap();
    assert_eq!((back.temporary_dimensions, left_out.len()), (s.model.temporary_dimensions, 0));
    // One step to undo, and a mistake is refused and changes nothing.
    run(&mut s, "undo", json!({}));
    assert!(s.model.temporary_dimensions.is_default());
    for p in [json!({"walls": "nowhere"}), json!({"openings": "faces"}), json!({"walls": 3}), json!({"extra": 1})] {
        assert!(s.execute("temporary_dimension_settings", &p).is_err(), "{p}");
    }
    assert!(s.model.temporary_dimensions.is_default());
}

#[test]
fn a_far_witness_line_can_be_moved_to_another_parallel_reference_and_the_settings_follow_the_openings() {
    let mut s = Session::new();
    house(&mut s);
    let inner = wall_at(&mut s, [1000.0, 2000.0], [5500.0, 2000.0]);
    let level = s.model.wall(inner).unwrap().level;
    let north = s.model.guid(100004);
    let half = s.model.thickness(s.model.wall(100004).unwrap()) / 2.0;
    // The north wall is the nearest on the left. Moved to its left face (on its south side, as it runs west), the dimension is shorter.
    let mut witness = Witness::default();
    assert_eq!(tempdims::wall(&s.model, inner, level, witness)[0].distance, 4000.0);
    witness = witness.with_target(Side::Left, Some(crate::model::Reference::line(north, Face::Left)));
    let d = tempdims::wall(&s.model, inner, level, witness);
    assert_eq!((d[0].to.face, d[0].distance), (Face::Left, 4000.0 - half));
    assert_eq!(d[1].distance, 2000.0, "the other side is its own");
    // Moved on to a farther wall: any parallel line the plan shows, not only the nearest's.
    let far = wall_at(&mut s, [0.0, 9000.0], [8000.0, 9000.0]);
    let far_line = crate::model::Reference::line(s.model.guid(far), Face::Centre);
    let d = tempdims::wall(&s.model, inner, level, witness.with_target(Side::Left, Some(far_line)));
    assert_eq!((d[0].to, d[0].distance), (far_line, 7000.0));
    // To what is behind the wall (south of it, for the left side), or not a line the plan shows, it stays where it was.
    let behind = crate::model::Reference::line(s.model.guid(100002), Face::Centre);
    let d = tempdims::wall(&s.model, inner, level, witness.with_target(Side::Left, Some(behind)));
    assert_eq!(d[0].to.id, s.model.guid(100004));
    // The lines it can be moved to: the faces of each wall beside it and a grid's line.
    let grid = run(&mut s, "grid", json!({"start": [0, 2600], "end": [8000, 2600]}))["id"].as_u64().unwrap();
    let lines = tempdims::neighbour_lines(&s.model, inner, level);
    assert!(lines.iter().any(|(r, _)| *r == crate::model::Reference::line(s.model.guid(grid), Face::Centre)));
    assert!(lines.iter().any(|(r, _)| r.id == north && r.face == Face::Left));
    assert!(lines.iter().all(|(r, _)| r.id != s.model.guid(inner)), "never the wall's own");
    // A door's dimensions stop at its opening when the settings say so.
    let door = run(&mut s, "door", json!({"wall": 100002, "offset": 3000}))["id"].as_u64().unwrap();
    let e = s.model.elements.get(door).unwrap().clone();
    assert_eq!(tempdims::opening_reach(&s.model, &e), 0.0);
    run(&mut s, "temporary_dimension_settings", json!({"openings": "openings"}));
    assert_eq!(tempdims::opening_reach(&s.model, &e), s.model.opening_type(&e).unwrap().width / 2.0);
}

#[test]
fn the_command_reads_the_witness_lines_a_person_set_and_a_core_is_a_line_a_dimension_can_measure_to() {
    let mut s = Session::new();
    let (a, b) = cored(&mut s);
    let r = run(&mut s, "temporary_dimensions", json!({"id": a, "left": "core-left"}));
    assert_eq!(r["core"], true);
    assert_eq!((r["dimensions"][0]["from"]["face"].as_str(), r["dimensions"][0]["distance"].as_f64()), (Some("coreleft"), Some(4000.0)));
    assert_eq!(r["settings"], json!({"walls": "centerlines", "openings": "centerlines"}));
    // The far line moved to b's exterior-face-side line, by its reference.
    let r = run(&mut s, "temporary_dimensions", json!({"id": a, "to_left": {"id": b, "face": "left"}}));
    assert_eq!(r["dimensions"][0]["to"]["face"], "left");
    assert_eq!(r["dimensions"][0]["distance"].as_f64(), Some(4150.0));
    // Its ready-made dimension measures to the core as it says.
    let r = run(&mut s, "temporary_dimensions", json!({"id": a, "left": "core-right", "right": "centre"}));
    let made = run(&mut s, "dimension", r["dimensions"][0]["dimension"].clone());
    assert_eq!(made["values"], json!([4140.0]), "{made}");
    // A wall with no core of its own has none to measure to; a misspelt line or reference is refused.
    let plain = wall_at(&mut s, [0.0, 9000.0], [6000.0, 9000.0]);
    let before = s.model.elements.clone();
    let e = s.execute("dimension", &json!({"refs": [{"id": plain, "face": "corecentre"}, {"id": a}], "at": [100, 5000]})).unwrap_err();
    assert!(e.contains("no core"), "{e}");
    for p in [json!({"id": a, "left": "core"}), json!({"id": a, "to_left": 3}), json!({"id": a, "to_left": {"id": b, "face": "up"}})] {
        assert!(s.execute("temporary_dimensions", &p).is_err(), "{p}");
    }
    assert_eq!(s.model.elements, before);
    // A flipped wall's core faces swap, as its faces do.
    assert_eq!(Face::CoreLeft.flipped(), Face::CoreRight);
    assert_eq!(Face::CoreCentre.flipped(), Face::CoreCentre);
}

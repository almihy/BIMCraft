//! Review Warnings: what Revit lets a person do and lists, which the engine used to refuse. A design judgment is a warning in
//! the model's list and in the result of the command that made it; what would break the model is still refused.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id};
use crate::render;
use crate::view3d::{P3, Scene};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn id_of(r: &Value) -> Id {
    r["id"].as_u64().unwrap_or_else(|| panic!("no id in {r}"))
}

fn wall(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Id {
    run(s, "wall", json!({"points": [a, b]}))["ids"][0].as_u64().unwrap()
}

/// The rules of the warnings a command's result lists as new.
fn new_rules(r: &Value) -> Vec<String> {
    r["new_warnings"].as_array().map_or_else(Vec::new, |a| a.iter().map(|w| w["rule"].as_str().unwrap().to_string()).collect())
}

fn listed(s: &mut Session, rule: &str) -> Vec<Value> {
    run(s, "warnings", json!({"rule": rule}))["warnings"].as_array().unwrap().clone()
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

#[test]
fn doors_that_overlap_are_made_and_the_result_says_so() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    let first = id_of(&run(&mut s, "door", json!({"wall": w, "offset": 1000})));
    let r = run(&mut s, "door", json!({"wall": w, "offset": 1500}));
    let second = id_of(&r);
    assert_eq!(new_rules(&r), ["openings_overlap"], "{r}");
    let warning = &r["new_warnings"][0];
    assert_eq!(warning["elements"], json!([first, second]), "{warning}");
    assert_eq!(warning["guids"].as_array().unwrap().len(), 2);
    assert_eq!(warning["severity"], "warning");
    assert!(warning["message"].as_str().unwrap().contains(&format!("wall {w}")), "{warning}");
    assert_eq!(s.model.check(), Ok(()), "it is only a warning");
    // The read command lists it, until the second is moved clear of the first.
    assert_eq!(listed(&mut s, "openings_overlap").len(), 1);
    let r = run(&mut s, "modify", json!({"id": second, "offset": 3000}));
    assert!(r.get("new_warnings").is_none(), "nothing new: {r}");
    assert!(listed(&mut s, "openings_overlap").is_empty());
}

#[test]
fn an_opening_past_its_walls_end_or_wider_than_its_run_is_made_and_warned_of() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    let r = run(&mut s, "door", json!({"wall": w, "offset": 100}));
    assert_eq!(new_rules(&r), ["opening_past_wall"], "{r}");
    assert!(r["new_warnings"][0]["message"].as_str().unwrap().contains("past the start"), "{r}");
    let r = run(&mut s, "door", json!({"wall": w, "offset": 5900}));
    assert!(r["new_warnings"][0]["message"].as_str().unwrap().contains("past the end"), "{r}");
    // A wall shorter than the door is wide.
    let short = wall(&mut s, [0.0, 3000.0], [600.0, 3000.0]);
    let r = run(&mut s, "door", json!({"wall": short, "offset": 300}));
    assert!(r["new_warnings"][0]["message"].as_str().unwrap().contains("wider than"), "{r}");
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn what_would_break_the_model_is_still_refused() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    let before = s.model.clone();
    for (cmd, p) in [
        ("door", json!({"wall": 424_242, "offset": 1000})),
        ("door", json!({"wall": w, "offset": 1e300})),
        ("door", json!({"wall": w, "offset": 1000, "type": "No Such Door"})),
        ("wall", json!({"points": [[0, 0], [0.2, 0]]})),
        ("modify", json!({"id": w, "type": "No Such Wall Type"})),
    ] {
        assert!(s.execute(cmd, &p).is_err(), "{cmd} {p}");
        assert_eq!(*s.model, *before, "a refused {cmd} changes nothing");
    }
}

#[test]
fn walls_that_overlap_or_sit_on_each_other_are_warned_of() {
    let mut s = Session::new();
    wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    // Walls that meet end to end, or at a corner, or lie side by side without their bodies touching, are fine.
    let r = run(&mut s, "wall", json!({"points": [[6000, 0], [6000, 4000]]}));
    assert!(r.get("new_warnings").is_none(), "{r}");
    let r = run(&mut s, "wall", json!({"points": [[0, 300], [6000, 300]]}));
    assert!(r.get("new_warnings").is_none(), "{r}");
    // Along the same line, over half its length: they overlap.
    let r = run(&mut s, "wall", json!({"points": [[3000, 0], [9000, 0]]}));
    assert_eq!(new_rules(&r), ["walls_overlap"], "{r}");
    assert!(r["new_warnings"][0]["message"].as_str().unwrap().contains("3000 mm"), "{r}");
    // The same wall twice is identical, not just overlapping.
    let r = run(&mut s, "wall", json!({"points": [[0, 300], [6000, 300]]}));
    assert_eq!(new_rules(&r), ["identical_instances"], "{r}");
    assert_eq!(s.model.check(), Ok(()));
    // A wall of another level's height doesn't overlap the walls of this one.
    run(&mut s, "level", json!({"name": "Roof", "elevation": 6000}));
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "level": "Roof"}));
    assert!(new_rules(&r).iter().all(|k| k != "walls_overlap" && k != "identical_instances"), "{r}");
}

#[test]
fn identical_columns_are_warned_of_once_for_the_group() {
    let mut s = Session::new();
    run(&mut s, "column", json!({"at": [1000, 1000]}));
    run(&mut s, "column", json!({"at": [2000, 1000]}));
    let r = run(&mut s, "column", json!({"at": [1000, 1000]}));
    assert_eq!(new_rules(&r), ["identical_instances"], "{r}");
    assert_eq!(r["new_warnings"][0]["elements"].as_array().unwrap().len(), 2);
    // A third in the same place joins the same warning: the group, not another pair.
    let r = run(&mut s, "column", json!({"at": [1000, 1000]}));
    assert_eq!(r["new_warnings"][0]["elements"].as_array().unwrap().len(), 3, "{r}");
    assert_eq!(listed(&mut s, "identical_instances").len(), 1);
    // Taking one away leaves the others warned of.
    let group = ids(&listed(&mut s, "identical_instances")[0]["elements"]);
    run(&mut s, "delete", json!({"ids": [group[2]]}));
    assert_eq!(listed(&mut s, "identical_instances")[0]["elements"].as_array().unwrap().len(), 2);
    run(&mut s, "delete", json!({"ids": [group[1]]}));
    assert!(listed(&mut s, "identical_instances").is_empty());
}

#[test]
fn a_wall_or_grid_a_hair_off_axis_is_a_note_and_an_exact_or_a_clear_angle_is_not() {
    let mut s = Session::new();
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [5000, 20]]}));
    assert_eq!(new_rules(&r), ["wall_off_axis"], "{r}");
    assert_eq!(r["new_warnings"][0]["severity"], "note");
    let r = run(&mut s, "grid", json!({"start": [0, -1000], "end": [30, 9000]}));
    assert_eq!(new_rules(&r), ["wall_off_axis"], "{r}");
    for points in [json!([[0, 2000], [5000, 7000]]), json!([[0, 3000], [5000, 3600]]), json!([[0, 4000], [5000, 4000]])] {
        let r = run(&mut s, "wall", json!({"points": points}));
        assert!(r.get("new_warnings").is_none(), "{points}: {r}");
    }
}

#[test]
fn a_room_the_walls_do_not_close_round_is_warned_of_and_one_in_anothers_space_is_redundant() {
    let mut s = Session::new();
    // Three walls of a square: the room has no loop.
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}));
    let r = run(&mut s, "room", json!({"point": [2000, 2000], "name": "Hall"}));
    assert_eq!(new_rules(&r), ["room_not_enclosed"], "{r}");
    // Closing the square answers it, and the walls' change is what tells the room's warning.
    run(&mut s, "wall", json!({"points": [[0, 4000], [0, 0]]}));
    assert!(listed(&mut s, "room_not_enclosed").is_empty());
    let r = run(&mut s, "room", json!({"point": [1500, 1500], "name": "Again"}));
    assert_eq!(new_rules(&r), ["room_redundant"], "{r}");
    assert_eq!(r["new_warnings"][0]["elements"].as_array().unwrap().len(), 2);
    // Taking a wall out opens the first room again.
    let walls: Vec<u64> = run(&mut s, "elements", json!({"category": "wall"}))["elements"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["id"].as_u64().unwrap())
        .collect();
    let r = run(&mut s, "delete", json!({"ids": [walls[3]]}));
    assert!(new_rules(&r).contains(&"room_not_enclosed".to_string()), "{r}");
}

#[test]
fn a_warning_is_new_only_once_and_goes_with_an_undo() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    run(&mut s, "door", json!({"wall": w, "offset": 1000}));
    let r = run(&mut s, "door", json!({"wall": w, "offset": 1400}));
    assert_eq!(new_rules(&r), ["openings_overlap"]);
    // A command that leaves it standing doesn't say it again.
    let r = run(&mut s, "wall", json!({"points": [[0, 3000], [6000, 3000]]}));
    assert!(r.get("new_warnings").is_none(), "{r}");
    assert_eq!(listed(&mut s, "openings_overlap").len(), 1);
    // Undo takes the second wall, then the second door, and its warning with it.
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert!(s.warnings().is_empty());
    run(&mut s, "redo", json!({}));
    run(&mut s, "redo", json!({}));
    assert_eq!(s.warnings().len(), 1);
    // Said again after it was taken away and made again.
    let doors = ids(&listed(&mut s, "openings_overlap")[0]["elements"]);
    run(&mut s, "delete", json!({"ids": [doors[1]]}));
    assert!(s.warnings().is_empty());
    let r = run(&mut s, "door", json!({"wall": w, "offset": 1400}));
    assert_eq!(new_rules(&r), ["openings_overlap"]);
}

#[test]
fn the_warnings_command_filters_and_refuses_what_it_does_not_know() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 20.0]);
    let door = id_of(&run(&mut s, "door", json!({"wall": w, "offset": 100})));
    let all = run(&mut s, "warnings", json!({}));
    assert_eq!(all["count"], 2, "{all}");
    assert_eq!(run(&mut s, "warnings", json!({"severity": "note"}))["count"], 1);
    assert_eq!(run(&mut s, "warnings", json!({"element": door}))["warnings"][0]["rule"], "opening_past_wall");
    assert_eq!(run(&mut s, "warnings", json!({"element": door, "rule": "wall_off_axis"}))["count"], 0);
    for p in [json!({"rule": "nonsense"}), json!({"severity": "dire"}), json!({"element": 999_999_999}), json!({"colour": 1})] {
        assert!(s.execute("warnings", &p).is_err(), "{p}");
    }
    // `inspect` carries the messages as it did.
    let i = run(&mut s, "inspect", json!({"brief": true}));
    assert_eq!(i["warnings"].as_array().unwrap().len(), 2, "{i}");
}

#[test]
fn two_types_of_one_category_sharing_a_type_mark_are_warned_of() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "A", "thickness": 200, "type_mark": "W1"}));
    assert!(listed(&mut s, "duplicate_type_mark").is_empty(), "one type holds the mark");
    let r = run(&mut s, "type", json!({"category": "wall", "name": "B", "thickness": 150, "type_mark": "W1"}));
    assert_eq!(new_rules(&r), ["duplicate_type_mark"], "{r}");
    let warning = &r["new_warnings"][0];
    let message = warning["message"].as_str().unwrap();
    assert!(message.contains("`A`") && message.contains("`B`") && message.contains("`W1`") && message.contains("wall"), "{message}");
    // About the types: their library GUIDs, which no element has.
    assert_eq!(warning["guids"].as_array().unwrap().len(), 2, "{warning}");
    assert_eq!(s.model.check(), Ok(()), "a warning, not a refusal");
    // A type of another category with the mark, and types that hold none, are no duplicates.
    let r = run(&mut s, "type", json!({"category": "floor", "name": "F", "thickness": 200, "type_mark": "W1"}));
    assert!(new_rules(&r).is_empty(), "{r}");
    run(&mut s, "type", json!({"category": "wall", "name": "C", "thickness": 100}));
    run(&mut s, "type", json!({"category": "wall", "name": "D", "thickness": 100}));
    assert_eq!(listed(&mut s, "duplicate_type_mark").len(), 1);
    // A change to elements keeps it, and is not its news.
    let r = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "A"}));
    assert!(!new_rules(&r).contains(&"duplicate_type_mark".to_string()), "{r}");
    assert_eq!(listed(&mut s, "duplicate_type_mark").len(), 1);
    // A third type with the mark joins the one warning; a mark of its own takes a type out of it.
    run(&mut s, "type", json!({"category": "wall", "name": "C", "type_mark": "W1"}));
    let all = listed(&mut s, "duplicate_type_mark");
    assert_eq!((all.len(), all[0]["guids"].as_array().unwrap().len()), (1, 3), "{all:?}");
    run(&mut s, "type", json!({"category": "wall", "name": "C", "type_mark": "W3"}));
    run(&mut s, "type", json!({"category": "wall", "name": "B", "type_mark": "W2"}));
    assert!(listed(&mut s, "duplicate_type_mark").is_empty());
}

fn volume(tris: &[[P3; 3]]) -> f64 {
    tris.iter()
        .map(|[a, b, c]| a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0]) + a[2] * (b[0] * c[1] - b[1] * c[0]))
        .sum::<f64>()
        / 6.0
}

/// A wall 6 m long and 1 m tall (200 thick) with every kind of opening that is only a warning: two overlapping doors, a window
/// past the wall's end, one whose sill is over the wall's top, one wholly under its base, and one half under it.
fn awkward() -> (Session, Id) {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "window", "name": "Tall", "width": 1000, "height": 800, "sill": 0}));
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 1000}))["ids"][0].as_u64().unwrap();
    for (cmd, offset, sill) in [
        ("door", 800.0, 0.0),
        ("door", 1200.0, 0.0),
        ("window", 5900.0, 100.0),
        ("window", 2500.0, 1500.0),
        ("window", 3500.0, -2000.0),
        ("window", 4500.0, -400.0),
    ] {
        run(
            &mut s,
            cmd,
            json!({"wall": w, "offset": offset, "sill": sill, "type": if cmd == "window" { "Tall" } else { "Single 900 x 2100" }}),
        );
    }
    (s, w)
}

#[test]
fn every_awkward_opening_is_listed_and_the_wall_keeps_between_its_base_and_top() {
    let (mut s, w) = awkward();
    let rules: Vec<String> = s.warnings().iter().map(|w| w.rule.to_string()).collect();
    for rule in ["openings_overlap", "opening_past_wall", "opening_outside_height"] {
        assert!(rules.iter().any(|r| r == rule), "{rule} in {rules:?}");
    }
    assert_eq!(s.model.check(), Ok(()));
    // In 3D: the wall's solid stays within 0 to 1000 whatever the openings do, and holds what the cuts leave.
    let scene = Scene::new(&s.model);
    let look = crate::graphics::Look::of_3d(&s.model);
    let tris: Vec<[P3; 3]> = scene.facets(&look).filter(|f| f.owner == w).map(|f| f.corners).collect();
    assert!(!tris.is_empty());
    let (lo, hi) = tris.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
    assert!(lo >= -1e-6 && hi <= 1000.0 + 1e-6, "the wall runs from {lo} to {hi}");
    let whole = 6000.0 * 200.0 * 1000.0;
    // Cut: the doors (350 to 1650, 1300 wide) the wall's whole height, the window past the end (5400 to 6000, 600 wide, from
    // 100 to 900), the window half under the base (4000 to 5000, up to 400): and nothing for the two wholly over or under.
    let cut = 1300.0 * 200.0 * 1000.0 + 600.0 * 200.0 * 800.0 + 1000.0 * 200.0 * 400.0;
    let held = volume(&tris);
    assert!((held - (whole - cut)).abs() < 1.0, "{held} against {}", whole - cut);
}

#[test]
fn rendering_ifc_and_schedules_cope_with_every_awkward_opening() {
    let (mut s, _) = awkward();
    // A curved wall with openings at and past its ends, and a door in a wall shorter than the door is wide.
    let arc = run(&mut s, "wall", json!({"points": [[0, 5000], [4000, 5000]], "arc": 90}))["ids"][0].as_u64().unwrap();
    run(&mut s, "door", json!({"wall": arc, "offset": 100}));
    run(&mut s, "door", json!({"wall": arc, "offset": 6000}));
    let stub = wall(&mut s, [0.0, 9000.0], [600.0, 9000.0]);
    run(&mut s, "door", json!({"wall": stub, "offset": 100}));
    assert_eq!(s.model.check(), Ok(()));
    // Plans, 3D, sections and elevations, the IFC, the schedules, and a save and open again.
    let d = render::plan(&s.model, 100_000, 800, 600);
    assert!(render::png(&d).is_ok());
    let _ = Scene::new(&s.model).facets(&crate::graphics::Look::of_3d(&s.model)).count();
    let elevation = id_of(&run(&mut s, "elevation", json!({"facing": "south"})));
    let view = render::section_fit(&s.model, elevation, 800, 600);
    assert!(render::section_png(&s.model, elevation, view).is_ok());
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains("IFCOPENINGELEMENT"));
    assert!(!ifc.contains("NaN") && !ifc.contains("inf"), "numbers stay numbers");
    for category in ["door", "window", "wall"] {
        run(&mut s, "schedule", json!({"category": category}));
    }
    let dir = std::env::temp_dir().join(format!("bimcraft-warnings-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("awkward.bimcraft");
    run(&mut s, "save", json!({"path": path}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": path}));
    assert_eq!(t.model.review().len(), s.model.review().len(), "a file brings the same warnings back");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_warnings_brought_up_to_date_are_those_of_the_whole_model_after_every_edit() {
    // `Session` asserts it after each command in tests; this runs a mixed set of edits through it.
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    let a = id_of(&run(&mut s, "door", json!({"wall": w, "offset": 1000})));
    run(&mut s, "door", json!({"wall": w, "offset": 1300}));
    run(&mut s, "move", json!({"ids": [w], "by": [0, 500]}));
    run(&mut s, "copy", json!({"ids": [w], "by": [0, 0]}));
    run(&mut s, "modify", json!({"id": a, "offset": 4000}));
    run(&mut s, "delete", json!({"ids": [w]}));
    run(&mut s, "undo", json!({}));
    assert_eq!(*s.warnings(), s.model.review());
    assert!(matches!(s.model.elements.get(a), Some(Element::Door(_))));
}

/// How many whole reviews and how many reviews brought up to date `f` made, and how many rooms it worked out.
fn counts(f: impl FnOnce()) -> (usize, usize, usize) {
    let was = crate::warnings::REVIEWS.with(std::cell::Cell::get);
    f();
    let now = crate::warnings::REVIEWS.with(std::cell::Cell::get);
    (now.0 - was.0, now.1 - was.1, now.2 - was.2)
}

/// [`counts`] without the rooms.
fn reviews(f: impl FnOnce()) -> (usize, usize) {
    let (whole, since, _) = counts(f);
    (whole, since)
}

/// A wall with two doors that overlap and a room, so that there are warnings to keep.
fn awkward_house() -> (Session, Id) {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    run(&mut s, "door", json!({"wall": w, "offset": 1000}));
    run(&mut s, "door", json!({"wall": w, "offset": 1300}));
    (s, w)
}

#[test]
fn after_an_undo_the_next_command_does_not_review_the_whole_model() {
    let (mut s, w) = awkward_house();
    run(&mut s, "door", json!({"wall": w, "offset": 4000}));
    run(&mut s, "undo", json!({}));
    let (whole, _) = reviews(|| {
        run(&mut s, "move", json!({"ids": [w], "by": [0, 10]}));
    });
    assert_eq!(whole, 0, "the kept warnings are of the model before the undo, a step from this one");
    assert_eq!(*s.warnings(), s.model.review_whole());
    // And an undo and a redo, then the next command.
    run(&mut s, "undo", json!({}));
    run(&mut s, "redo", json!({}));
    let (whole, _) = reviews(|| {
        run(&mut s, "door", json!({"wall": w, "offset": 5000}));
    });
    assert_eq!(whole, 0);
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn after_a_change_another_copy_made_the_next_command_does_not_review_the_whole_model() {
    let (mut a, w) = awkward_house();
    let mut b = Session::new();
    b.reset((*a.model).clone(), None);
    b.warnings();
    let before = a.model.clone();
    let door = id_of(&run(&mut a, "door", json!({"wall": w, "offset": 4500})));
    let changes = crate::sync::changes(&before, &a.model).unwrap();
    // `b` had its own command last, so its kept warnings are of the model it had.
    run(&mut b, "move", json!({"ids": [w], "by": [0, 10]}));
    let before_move = b.model.clone();
    assert!(b.merge(&changes, &std::collections::BTreeSet::new()).unwrap().refused.is_empty());
    assert!(b.model.elements.get(door).is_some(), "the door came in");
    let (whole, _) = reviews(|| {
        run(&mut b, "move", json!({"ids": [w], "by": [0, 10]}));
    });
    assert_eq!(whole, 0, "a merge leaves the kept warnings on the model before it");
    assert_eq!(*b.warnings(), b.model.review_whole());
    let _ = before_move;
}

#[test]
fn after_a_transaction_taken_back_the_next_command_does_not_review_the_whole_model() {
    let (mut s, w) = awkward_house();
    let r = s.transaction("Try", |s| {
        s.execute("door", &json!({"wall": w, "offset": 4000}))?;
        Err::<(), String>("no".into())
    });
    assert!(r.is_err());
    let (whole, _) = reviews(|| {
        run(&mut s, "move", json!({"ids": [w], "by": [0, 10]}));
    });
    assert_eq!(whole, 0);
}

#[test]
fn a_script_works_out_its_warnings_once_and_says_them_in_its_last_result() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    s.warnings();
    let script = |a: u32, b: u32| {
        format!(
            "door {{\"wall\": {w}, \"offset\": {a}}}
door {{\"wall\": {w}, \"offset\": {b}}}
wall {{\"points\": [[0, 5000], [6000, 5000]]}}
             door {{\"wall\": {w}, \"offset\": 4500}}"
        )
    };
    let text = script(1000, 1300);
    let mut out = Vec::new();
    let (whole, since) = reviews(|| out = s.script(&text).unwrap());
    assert_eq!((whole, since), (0, 1), "not one for each of the four lines");
    assert_eq!(out.len(), 4);
    assert!(out[..3].iter().all(|r| r.get("new_warnings").is_none()), "{out:?}");
    assert_eq!(out[3]["new_warnings"][0]["rule"], "openings_overlap", "{}", out[3]);
    assert_eq!(*s.warnings(), s.model.review_whole());
    // All or nothing, and a dry run, the same.
    let (whole, since) = reviews(|| out = s.script_atomic(&script(2500, 3000)).unwrap());
    assert_eq!((whole, since), (0, 1));
    assert_eq!(out.len(), 4);
}

#[test]
fn a_dry_run_says_its_new_warnings_as_a_command_does_and_works_them_out_once() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let text = format!(
        "door {{\"wall\": {w}, \"offset\": 1100}}
door {{\"wall\": {w}, \"offset\": 5000}}
move {{\"ids\": [{w}], \"by\": [0, 5]}}"
    );
    let mut r = Value::Null;
    let (whole, since) = reviews(|| r = s.script_dry_run(&text).unwrap());
    assert_eq!(whole, 0, "{r}");
    assert!(since <= 4, "the script's once, and the answer's: {since}");
    let new = r["new_warnings"].as_array().unwrap();
    assert!(!new.is_empty() && new.iter().all(|w| w["rule"].is_string() && w["guids"].is_array()), "{r}");
    assert!(r.get("warnings").is_none(), "{r}");
}

/// The small tower of two storeys in a session: rooms on each, in a layout of every kind of room.
fn small_tower() -> Session {
    let mut s = Session::new();
    run(&mut s, "tower", json!({"storeys": 2, "plate": "small"}));
    s
}

#[test]
fn a_change_works_out_only_the_rooms_it_reached() {
    let mut s = small_tower();
    let all = s.model.elements.values().filter(|e| matches!(e, Element::Room(_))).count();
    assert!(all > 20, "{all} rooms");
    s.warnings();
    let column = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Column(_) | Element::StructuralColumn(_))).map(|(id, _)| *id);
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).unwrap();
    for id in column.into_iter().chain([wall]) {
        let (whole, _, judged) = counts(|| {
            run(&mut s, "move", json!({"ids": [id], "by": [20, 0]}));
        });
        assert_eq!(whole, 0);
        assert!(judged < all / 3, "moving {id} worked out {judged} of {all} rooms");
        // (Each command held the rooms' reach to the whole model's, as every command does in a test.)
    }
}

#[test]
fn what_the_rooms_reach_stays_true_through_every_kind_of_edit() {
    let mut s = small_tower();
    s.warnings();
    let pick =
        |s: &Session, f: &dyn Fn(&Element) -> bool| s.model.elements.iter().filter(|(_, e)| f(e)).map(|(id, _)| *id).collect::<Vec<Id>>();
    let walls = pick(&s, &|e| matches!(e, Element::Wall(_)));
    let rooms = pick(&s, &|e| matches!(e, Element::Room(_)));
    let at = s.model.elements.values().find_map(|e| if let Element::Room(r) = e { Some(r.point) } else { None }).unwrap();
    // Each edit's warnings are held to the whole model's by the session.
    for i in [0, walls.len() / 3, walls.len() / 2, walls.len() - 1] {
        run(&mut s, "move", json!({"ids": [walls[i]], "by": [0, 700]}));
        run(&mut s, "move", json!({"ids": [walls[i]], "by": [0, -700]}));
    }
    run(&mut s, "modify", json!({"id": rooms[1], "name": "Renamed"}));
    run(&mut s, "move", json!({"ids": [rooms[2]], "by": [300, 300]}));
    // Two rooms in one space, a separation line through it, a room taken away, a wall deleted and brought back.
    run(&mut s, "room", json!({"point": [at.0 + 5.0, at.1 + 5.0]}));
    run(&mut s, "delete", json!({"ids": [walls[walls.len() / 2]]}));
    run(&mut s, "undo", json!({}));
    run(&mut s, "delete", json!({"ids": [rooms[3]]}));
    run(&mut s, "undo", json!({}));
    run(&mut s, "copy", json!({"ids": [walls[3]], "by": [0, 0]}));
    run(&mut s, "undo", json!({}));
    run(&mut s, "redo", json!({}));
    assert_eq!(*s.warnings(), s.model.review_whole());
}

/// What the warnings cost on the reference tower, next to the check an edit pays anyway: `cargo test --release -p bimcraft-core
/// warnings_cost -- --ignored --nocapture`. Each edit's warnings are brought up to date from those before it, the median of
/// seven, as `Session::report_warnings` does after every command (whose own time, here, holds the oracles the test build adds).
#[test]
#[ignore = "timing, not a check"]
fn warnings_cost_on_the_reference_tower_next_to_the_check() {
    use std::time::{Duration, Instant};
    fn median(mut v: Vec<Duration>) -> Duration {
        v.sort();
        v[v.len() / 2]
    }
    let mut s = Session::new();
    run(&mut s, "tower", json!({"storeys": 3}));
    let started = Instant::now();
    let mut kept = s.model.reviewed();
    let full = started.elapsed();
    let started = Instant::now();
    s.model.check().unwrap();
    let check = started.elapsed();
    let first = |s: &Session, f: &dyn Fn(&Element) -> bool| s.model.elements.iter().find(|(_, e)| f(e)).map(|(id, _)| *id).unwrap();
    let column = first(&s, &|e| matches!(e, Element::Column(_) | Element::StructuralColumn(_)));
    let wall = first(&s, &|e| matches!(e, Element::Wall(_)));
    let door = first(&s, &|e| matches!(e, Element::Door(_)));
    let component = first(&s, &|e| matches!(e, Element::Component(_)));
    eprintln!(
        "tower, 3 storeys, {} elements, {} warnings: whole review {full:?}, check {check:?}",
        s.model.elements.len(),
        kept.warnings.len()
    );
    for (what, id) in [("a column", column), ("a wall", wall), ("a door", door), ("a component", component)] {
        let mut times = Vec::new();
        let mut warnings = 0;
        for i in 0..7 {
            let before = s.model.clone();
            let by = if i % 2 == 0 { [10, 0] } else { [-10, 0] };
            let moved = s.execute("move", &json!({"ids": [id], "by": by}));
            if moved.is_err() {
                break;
            }
            let started = Instant::now();
            kept = s.model.reviewed_since(&before, &kept);
            times.push(started.elapsed());
            warnings = kept.warnings.len();
        }
        if !times.is_empty() {
            eprintln!("  moving {what}: warnings brought up to date {:?} ({warnings} warnings)", median(times));
        }
    }
}

// ---- Every command made inside another, a transaction or a plugin is held to the whole model's warnings too ----

/// A square of four walls on `level` with a room in it.
fn square(s: &mut Session, level: &str, at: f64) -> [Id; 4] {
    let points = json!([[at, at], [at + 4000.0, at], [at + 4000.0, at + 4000.0], [at, at + 4000.0], [at, at]]);
    let made = run(s, "wall", json!({"points": points, "level": level}));
    let walls = made["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect::<Vec<_>>();
    run(s, "room", json!({"point": [at + 2000.0, at + 2000.0], "level": level, "name": "Hall"}));
    [walls[0], walls[1], walls[2], walls[3]]
}

#[test]
fn what_a_script_line_leaves_is_held_to_the_whole_model_as_a_command_is() {
    let mut s = Session::new();
    let w = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    s.shadowed = 0;
    let text = format!(
        "door {{\"wall\": {w}, \"offset\": 1000}}
door {{\"wall\": {w}, \"offset\": 1300}}
wall {{\"points\": [[0, 5000], [6000, 5000]]}}"
    );
    s.script(&text).unwrap();
    assert_eq!(s.shadowed, 3, "each of the lines");
    // A transaction's commands, an all-or-nothing script's and a dry run's, the same.
    s.transaction("Two", |s| {
        s.execute("door", &json!({"wall": w, "offset": 4000}))?;
        s.execute("door", &json!({"wall": w, "offset": 4200})).map(|_| ())
    })
    .unwrap();
    assert_eq!(s.shadowed, 5, "a transaction's");
    s.script_atomic(&text).unwrap();
    assert_eq!(s.shadowed, 8, "an atomic script's");
    s.script_dry_run(&text).unwrap();
    assert_eq!(s.shadowed, 11, "a dry run's");
    // A plugin's command: the commands it runs.
    let plugin = crate::engine::ExternalCommand {
        id: "plug_in".into(),
        label: "Plug".into(),
        aliases: Vec::new(),
        params: "{}".into(),
        menu: None,
        plugin: "test".into(),
        run: std::sync::Arc::new(move |s: &mut Session, _: &Value| {
            s.transaction("Plug", |s| s.execute("door", &json!({"wall": w, "offset": 2500})))
        }),
    };
    s.register(plugin).unwrap();
    run(&mut s, "plug_in", json!({}));
    assert_eq!(s.shadowed, 12, "a plugin's");
}

#[test]
fn a_transaction_says_the_warnings_it_made_at_its_end_and_its_commands_say_none() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let mut inside = Vec::new();
    let (_, new) = s
        .transaction_reporting("Doors", |s| {
            inside.push(s.execute("door", &json!({"wall": w, "offset": 4000}))?);
            inside.push(s.execute("door", &json!({"wall": w, "offset": 4300}))?);
            Ok(())
        })
        .unwrap();
    assert!(inside.iter().all(|r| r.get("new_warnings").is_none()), "{inside:?}");
    let rules: Vec<&str> = new.iter().map(|w| w["rule"].as_str().unwrap()).collect();
    assert_eq!(rules, ["openings_overlap"], "{new:?}");
    assert_eq!(new[0]["guids"].as_array().unwrap().len(), 2);
    assert_eq!(*s.warnings(), s.model.review_whole());
    // Nothing made, nothing said; one that fails says nothing and changes nothing.
    let (_, new) = s.transaction_reporting("Nothing", |_| Ok(())).unwrap();
    assert!(new.is_empty());
    let kept = s.model.clone();
    let failed = s.transaction_reporting::<()>("Fails", |s| {
        s.execute("door", &json!({"wall": w, "offset": 5000}))?;
        Err("no".into())
    });
    assert!(failed.is_err());
    assert_eq!(*s.model, *kept);
}

#[test]
fn a_plugins_command_says_the_warnings_it_made_in_its_result() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let plugin = crate::engine::ExternalCommand {
        id: "two_doors".into(),
        label: "Two doors".into(),
        aliases: Vec::new(),
        params: "{}".into(),
        menu: None,
        plugin: "test".into(),
        run: std::sync::Arc::new(move |s: &mut Session, _: &Value| {
            s.transaction("Two doors", |s| {
                s.execute("door", &json!({"wall": w, "offset": 4000}))?;
                s.execute("door", &json!({"wall": w, "offset": 4300}))
            })
        }),
    };
    s.register(plugin).unwrap();
    let r = run(&mut s, "two_doors", json!({}));
    assert_eq!(new_rules(&r), ["openings_overlap"], "{r}");
    assert_eq!(*s.warnings(), s.model.review_whole());
    // The same plugin's command run in a script: the script says them, once.
    let out = s.script("two_doors").unwrap();
    assert_eq!(out.iter().filter(|r| r.get("new_warnings").is_some()).count(), 1, "{out:?}");
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn a_panic_after_a_script_line_leaves_the_next_command_the_outermost_it_is() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let text = format!("door {{\"wall\": {w}, \"offset\": 4000}}\ndoor {{\"wall\": {w}, \"offset\": 4100}}");
    crate::engine::FORCED_PANIC.with(|f| f.set(Some("post")));
    let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.script(&text)));
    crate::engine::FORCED_PANIC.with(|f| f.set(None));
    assert!(ran.is_err(), "the panic came out of the script");
    assert_eq!(s.nested, 0, "what the script began is not left under way");
    let r = run(&mut s, "door", json!({"wall": w, "offset": 5000}));
    let _ = r;
    let r = run(&mut s, "door", json!({"wall": w, "offset": 5100}));
    assert_eq!(new_rules(&r), ["openings_overlap"], "the command says its warnings: {r}");
}

#[test]
fn a_panic_working_out_the_warnings_refuses_the_command_and_leaves_the_session_as_it_was() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let before = s.model.clone();
    let steps = s.history().len();
    crate::engine::FORCED_PANIC.with(|f| f.set(Some("report")));
    let e = s.execute("door", &json!({"wall": w, "offset": 4000})).unwrap_err();
    crate::engine::FORCED_PANIC.with(|f| f.set(None));
    assert!(e.contains("warnings") && e.contains("forced panic"), "{e}");
    assert_eq!((*s.model == *before, s.history().len(), s.nested), (true, steps, 0));
    // And the next works, its warnings as they should be.
    let r = run(&mut s, "door", json!({"wall": w, "offset": 4000}));
    assert!(r.get("id").is_some());
    assert_eq!(*s.warnings(), s.model.review_whole());
    // The same at the end of a transaction.
    crate::engine::FORCED_PANIC.with(|f| f.set(Some("report")));
    let e = s.transaction_reporting("Doors", |s| s.execute("door", &json!({"wall": w, "offset": 5000})).map(|_| ())).unwrap_err();
    crate::engine::FORCED_PANIC.with(|f| f.set(None));
    assert!(e.contains("forced panic"), "{e}");
    assert_eq!(s.nested, 0);
}

#[test]
fn a_panic_working_out_a_scripts_warnings_refuses_an_atomic_script_but_not_one_whose_lines_are_done() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let text = format!(
        "door {{\"wall\": {w}, \"offset\": 4000}}
door {{\"wall\": {w}, \"offset\": 4300}}"
    );
    let doors = |s: &Session| s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Door(_))).count();
    let before = doors(&s);
    crate::engine::FORCED_PANIC.with(|f| f.set(Some("report")));
    let out = s.script(&text).expect("the lines went through, so the script is not refused");
    crate::engine::FORCED_PANIC.with(|f| f.set(None));
    assert_eq!((out.len(), doors(&s)), (2, before + 2), "the lines are done and said");
    assert!(out.iter().all(|r| r.get("new_warnings").is_none()), "{out:?}");
    assert_eq!(s.nested, 0);
    assert_eq!(*s.warnings(), s.model.review_whole());
    // All or nothing: the same failure takes the lines back.
    let kept = s.model.clone();
    crate::engine::FORCED_PANIC.with(|f| f.set(Some("report")));
    let e = s.script_atomic(&text.replace("4000", "5000").replace("4300", "5300")).unwrap_err();
    crate::engine::FORCED_PANIC.with(|f| f.set(None));
    assert!(e.contains("forced panic"), "{e}");
    assert_eq!(*s.model, *kept);
}

#[test]
fn a_transaction_works_out_no_warnings_and_a_panic_in_doing_so_cannot_refuse_it() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let made = reviews(|| {
        s.transaction("Doors", |s| s.execute("door", &json!({"wall": w, "offset": 4000})).map(|_| ())).unwrap();
    });
    assert_eq!(made, (0, 0), "a transaction is not asked for its warnings");
    crate::engine::FORCED_PANIC.with(|f| f.set(Some("report")));
    let done = s.transaction("Doors", |s| s.execute("door", &json!({"wall": w, "offset": 5000})).map(|_| ()));
    crate::engine::FORCED_PANIC.with(|f| f.set(None));
    assert!(done.is_ok(), "{done:?}");
    // Asked for, it does: one review brought up to date.
    let asked = reviews(|| {
        s.transaction_reporting("Doors", |s| s.execute("door", &json!({"wall": w, "offset": 5500})).map(|_| ())).unwrap();
    });
    assert!(asked.0 == 0 && asked.1 >= 1, "asked for, it brings the warnings up to date, never from nothing: {asked:?}");
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn a_dry_run_leaves_the_kept_warnings_on_the_project_and_says_its_new_warnings_once() {
    let (mut s, w) = awkward_house();
    s.warnings();
    let text = format!("door {{\"wall\": {w}, \"offset\": 4000}}\ndoor {{\"wall\": {w}, \"offset\": 4200}}");
    let r = s.script_dry_run(&text).unwrap();
    assert_eq!(r["new_warnings"].as_array().unwrap().len(), 1, "{r}");
    let lines = r["results"].as_array().unwrap();
    assert!(lines.iter().all(|l| l.get("new_warnings").is_none()), "not again in the last line's result: {lines:?}");
    // The warnings kept are the project's: asking costs nothing.
    let (whole, since) = reviews(|| {
        s.warnings();
    });
    assert_eq!((whole, since), (0, 0), "the dry run left the kept warnings on its hypothetical model");
}

#[test]
fn a_wall_or_column_moved_to_another_level_is_worked_out_where_it_went_and_where_it_was() {
    let mut s = Session::new();
    let levels: Vec<String> = s.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    let (low, high) = (levels[0].clone(), levels[1].clone());
    let first = |r: &Value| r["ids"][0].as_u64().unwrap();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "level": low, "height": 3000}));
    let b = first(&run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "level": high, "height": 3000})));
    assert!(s.warnings().is_empty());
    let r = run(&mut s, "modify", json!({"id": b, "level": low}));
    assert!(new_rules(&r).contains(&"identical_instances".to_string()), "{r}");
    let r = run(&mut s, "modify", json!({"id": b, "level": high}));
    assert!(r.get("new_warnings").is_none(), "{r}");
    assert!(s.warnings().is_empty());
    // A room on the lower level, whose wall the other one then replaces.
    let walls = square(&mut s, &low, 20_000.0);
    let top = first(&run(&mut s, "wall", json!({"points": [[20000, 20000], [24000, 20000]], "level": high})));
    run(&mut s, "modify", json!({"id": top, "level": low}));
    run(&mut s, "delete", json!({"ids": [walls[0]]}));
    run(&mut s, "undo", json!({}));
    // Columns: the same.
    let c1 = id_of(&run(&mut s, "column", json!({"at": [1000, 1000], "level": low, "height": 3000})));
    let c2 = id_of(&run(&mut s, "column", json!({"at": [1000, 1000], "level": high, "height": 3000})));
    let r = run(&mut s, "modify", json!({"id": c2, "level": low}));
    assert!(new_rules(&r).contains(&"identical_instances".to_string()), "{r}");
    assert_eq!(listed(&mut s, "identical_instances").iter().filter(|w| ids(&w["elements"]).contains(&c1)).count(), 1);
    run(&mut s, "modify", json!({"id": c2, "level": high}));
    assert!(listed(&mut s, "identical_instances").iter().all(|w| !ids(&w["elements"]).contains(&c1)));
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn a_component_changing_family_is_worked_out_in_both_families() {
    let mut s = Session::new();
    run(&mut s, "family", json!({"name": "Plain", "types": {"T": {}}}));
    run(&mut s, "family", json!({"name": "Same", "types": {"T": {}}}));
    run(&mut s, "family", json!({"name": "Bare", "category": "outlet", "types": {"T": {}}}));
    let a = id_of(&run(&mut s, "component", json!({"family": "Plain", "type": "T", "at": [1000, 1000]})));
    let b = id_of(&run(&mut s, "component", json!({"family": "Same", "type": "T", "at": [1000, 1000]})));
    assert!(listed(&mut s, "identical_instances").is_empty());
    let r = run(&mut s, "modify", json!({"id": b, "family": "Plain"}));
    assert!(new_rules(&r).contains(&"identical_instances".to_string()), "{r}");
    run(&mut s, "modify", json!({"id": b, "family": "Same"}));
    assert!(listed(&mut s, "identical_instances").is_empty());
    // To a family of devices with no connector, and back.
    let r = run(&mut s, "modify", json!({"id": a, "family": "Bare"}));
    assert!(new_rules(&r).contains(&"device_no_connector".to_string()), "{r}");
    run(&mut s, "modify", json!({"id": a, "family": "Plain"}));
    assert!(listed(&mut s, "device_no_connector").is_empty());
}

#[test]
fn a_wall_spanning_several_levels_bounds_the_rooms_of_each() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"name": "Third", "elevation": 6000}));
    let levels: Vec<String> = s.model.levels().iter().map(|(_, l)| l.name.clone()).collect();
    let (low, mid) = (levels[0].clone(), levels[1].clone());
    let walls = square(&mut s, &low, 0.0);
    square(&mut s, &mid, 0.0);
    assert!(s.warnings().iter().all(|w| !w.rule.starts_with("room_")), "{:?}", s.warnings());
    // A wall as tall as two storeys over the one side of both rooms: it replaces the walls there, and bounds both.
    run(&mut s, "delete", json!({"ids": [walls[0]]}));
    let tall = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": low, "height": 6000}))["ids"][0].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": tall, "height": 2000}));
    run(&mut s, "modify", json!({"id": tall, "height": 6000}));
    run(&mut s, "move", json!({"ids": [tall], "by": [0, 300]}));
    run(&mut s, "undo", json!({}));
    run(&mut s, "delete", json!({"ids": [tall]}));
    run(&mut s, "undo", json!({}));
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn room_separators_close_rooms_and_their_changes_are_worked_out() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 4000], [0, 4000], [0, 0]]}));
    run(&mut s, "room", json!({"point": [2000, 2000], "name": "Left"}));
    run(&mut s, "room", json!({"point": [6000, 2000], "name": "Right"}));
    assert_eq!(listed(&mut s, "room_redundant").len(), 1, "two rooms in one space");
    let sep = run(&mut s, "room_separator", json!({"points": [[4000, 0], [4000, 4000]]}))["ids"][0].as_u64().unwrap();
    assert!(listed(&mut s, "room_redundant").is_empty(), "the line parts them");
    run(&mut s, "move", json!({"ids": [sep], "by": [1000, 0]}));
    assert!(listed(&mut s, "room_redundant").is_empty(), "still between the two points");
    run(&mut s, "move", json!({"ids": [sep], "by": [-1000, 0]}));
    run(&mut s, "delete", json!({"ids": [sep]}));
    assert_eq!(listed(&mut s, "room_redundant").len(), 1);
    run(&mut s, "undo", json!({}));
    assert!(listed(&mut s, "room_redundant").is_empty());
    // A line that only part-closes a space leaves its room open.
    run(&mut s, "wall", json!({"points": [[10000, 0], [14000, 0], [14000, 4000]]}));
    run(&mut s, "room", json!({"point": [12000, 2000], "name": "Open"}));
    run(&mut s, "room_separator", json!({"points": [[14000, 4000], [10000, 4000]]}));
    assert_eq!(listed(&mut s, "room_not_enclosed").len(), 1);
    run(&mut s, "room_separator", json!({"points": [[10000, 4000], [10000, 0]]}));
    assert!(listed(&mut s, "room_not_enclosed").is_empty());
}

#[test]
fn a_rooms_area_from_the_finish_faces_is_worked_out_on_the_incremental_path() {
    let mut s = Session::new();
    run(&mut s, "room_area", json!({"at": "finish"}));
    let walls = square(&mut s, "Level 1", 0.0);
    square(&mut s, "Level 1", 6000.0);
    // A wall moved into the room's point puts it in the wall's body: not enclosed there, at the finish faces.
    run(&mut s, "move", json!({"ids": [walls[3]], "by": [2000, 0]}));
    run(&mut s, "move", json!({"ids": [walls[3]], "by": [-2000, 0]}));
    run(&mut s, "room_area", json!({"at": "centre"}));
    run(&mut s, "room_area", json!({"at": "finish"}));
    run(&mut s, "delete", json!({"ids": [walls[1]]}));
    run(&mut s, "undo", json!({}));
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn a_change_merged_then_undone_is_worked_out_from_the_model_it_was_made_of() {
    let (mut a, w) = awkward_house();
    let mut b = Session::new();
    b.reset((*a.model).clone(), None);
    b.warnings();
    let before = a.model.clone();
    run(&mut a, "door", json!({"wall": w, "offset": 4500}));
    let changes = crate::sync::changes(&before, &a.model).unwrap();
    run(&mut b, "door", json!({"wall": w, "offset": 2000}));
    assert!(b.merge(&changes, &std::collections::BTreeSet::new()).unwrap().refused.is_empty());
    run(&mut b, "undo", json!({}));
    run(&mut b, "door", json!({"wall": w, "offset": 4600}));
    run(&mut b, "undo", json!({}));
    run(&mut b, "redo", json!({}));
    assert_eq!(*b.warnings(), b.model.review_whole());
}

#[test]
fn elements_in_different_options_of_one_set_do_not_conflict_but_a_main_model_one_and_other_sets_do() {
    let mut s = Session::new();
    run(&mut s, "option", json!({"set": "Entry", "options": ["A", "B"]}));
    run(&mut s, "option", json!({"set": "Roof", "options": ["X", "Y"]}));
    let a = id_of(&run(&mut s, "column", json!({"at": [1000, 1000]})));
    run(&mut s, "modify", json!({"id": a, "params": {"Design Option": "Entry: A"}}));
    let b = id_of(&run(&mut s, "column", json!({"at": [1000, 1000]})));
    assert!(listed(&mut s, "identical_instances").len() == 1, "one in an option, one in the main model: both are there");
    let r = run(&mut s, "modify", json!({"id": b, "params": {"Design Option": "Entry: B"}}));
    assert!(r.get("new_warnings").is_none(), "{r}");
    assert!(listed(&mut s, "identical_instances").is_empty(), "options of one set are never there together");
    let r = run(&mut s, "modify", json!({"id": b, "params": {"Design Option": "Roof: X"}}));
    assert_eq!(new_rules(&r), ["identical_instances"], "options of two sets are: {r}");
    run(&mut s, "modify", json!({"id": b, "params": {"Design Option": "Entry: B"}}));
    assert!(listed(&mut s, "identical_instances").is_empty());
    // Walls that overlap, and doors that overlap in a wall.
    let w1 = wall(&mut s, [0.0, 3000.0], [6000.0, 3000.0]);
    let w2 = wall(&mut s, [3000.0, 3000.0], [9000.0, 3000.0]);
    assert_eq!(listed(&mut s, "walls_overlap").len(), 1);
    run(&mut s, "modify", json!({"id": w1, "params": {"Design Option": "Entry: A"}}));
    run(&mut s, "modify", json!({"id": w2, "params": {"Design Option": "Entry: B"}}));
    assert!(listed(&mut s, "walls_overlap").is_empty());
    let d1 = id_of(&run(&mut s, "door", json!({"wall": w1, "offset": 1500})));
    let d2 = id_of(&run(&mut s, "door", json!({"wall": w1, "offset": 1800})));
    assert_eq!(listed(&mut s, "openings_overlap").len(), 1);
    run(&mut s, "modify", json!({"id": d1, "params": {"Design Option": "Entry: B"}}));
    assert!(listed(&mut s, "openings_overlap").is_empty(), "{d2}");
}

#[test]
fn phases_do_not_hide_identical_or_overlapping_elements() {
    let mut s = Session::new();
    run(&mut s, "phase", json!({"name": "New work"}));
    let old = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    let new = wall(&mut s, [0.0, 0.0], [6000.0, 0.0]);
    assert_eq!(listed(&mut s, "identical_instances").len(), 1);
    let r = run(&mut s, "modify", json!({"id": old, "params": {"Phase Demolished": "New work"}}));
    assert!(r.get("new_warnings").is_none());
    assert_eq!(listed(&mut s, "identical_instances").len(), 1, "both stand in the first phase");
    run(&mut s, "modify", json!({"id": new, "params": {"Phase Created": "New work"}}));
    assert_eq!(
        listed(&mut s, "identical_instances").len(),
        1,
        "Revit's warning ignores phases: a demolished wall and its replacement still warn"
    );
    let longer = wall(&mut s, [3000.0, 0.0], [9000.0, 0.0]);
    assert_eq!(listed(&mut s, "walls_overlap").len(), 2, "{old} {new} {longer}");
    run(&mut s, "modify", json!({"id": longer, "params": {"Phase Created": "New work"}}));
    assert_eq!(listed(&mut s, "walls_overlap").len(), 2, "phases do not part overlapping walls");
    assert_eq!(*s.warnings(), s.model.review_whole());
}

#[test]
fn a_group_of_twins_in_two_options_and_the_main_model_is_a_warning_for_each_option_with_the_main_one() {
    let mut s = Session::new();
    run(&mut s, "option", json!({"set": "Entry", "options": ["A", "B"]}));
    let a = id_of(&run(&mut s, "column", json!({"at": [1000, 1000]})));
    run(&mut s, "modify", json!({"id": a, "params": {"Design Option": "Entry: A"}}));
    let b = id_of(&run(&mut s, "column", json!({"at": [1000, 1000]})));
    run(&mut s, "modify", json!({"id": b, "params": {"Design Option": "Entry: B"}}));
    let c = id_of(&run(&mut s, "column", json!({"at": [1000, 1000]})));
    let found = listed(&mut s, "identical_instances");
    let mut sets: Vec<Vec<Id>> = found.iter().map(|w| ids(&w["elements"]).into_iter().collect()).collect();
    for set in &mut sets {
        set.sort_unstable();
    }
    sets.sort();
    assert_eq!(sets, vec![vec![a, c], vec![b, c]], "A and B are never there together: {found:?}");
    assert_eq!(*s.warnings(), s.model.review_whole());
    // The same after a change to one of them, worked out from the warnings before.
    run(&mut s, "modify", json!({"id": a, "params": {"Design Option": "Entry: B"}}));
    assert_eq!(listed(&mut s, "identical_instances").len(), 1, "now a and b are in one option, with the main one's: one group");
    assert_eq!(*s.warnings(), s.model.review_whole());
}

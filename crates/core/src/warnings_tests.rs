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

/// What the warnings cost on the reference tower, next to the check an edit pays anyway: `cargo test -p bimcraft-core warnings_cost
/// -- --ignored --nocapture`.
#[test]
#[ignore = "timing, not a check"]
fn warnings_cost_on_the_reference_tower_next_to_the_check() {
    use std::time::Instant;
    let mut s = Session::new();
    run(&mut s, "tower", json!({"storeys": 3}));
    let started = Instant::now();
    let all = s.model.review();
    let full = started.elapsed();
    let started = Instant::now();
    s.model.check().unwrap();
    let check = started.elapsed();
    let column =
        s.model.elements.iter().find(|(_, e)| matches!(e, Element::Column(_) | Element::StructuralColumn(_))).map(|(id, _)| *id).unwrap();
    let before = s.model.clone();
    let started = Instant::now();
    let r = run(&mut s, "move", json!({"ids": [column], "by": [10, 0]}));
    let edit = started.elapsed();
    let started = Instant::now();
    let now = s.model.review_since(&before, &all);
    let since = started.elapsed();
    eprintln!(
        "tower, 3 storeys, {} elements, {} warnings: review {full:?}, check {check:?}, a move with its check and warnings {edit:?} \
         (new: {}), review_since alone {since:?} ({} warnings)",
        s.model.elements.len(),
        all.len(),
        r.get("new_warnings").map_or(0, |w| w.as_array().map_or(0, Vec::len)),
        now.len()
    );
}

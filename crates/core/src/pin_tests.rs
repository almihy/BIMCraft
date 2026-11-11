//! Pin and Unpin, as Revit's: a pin is kept in the file and goes with its element; a command from a script or the agent
//! still moves a pinned element and says it was pinned (the window is what refuses, see the desktop's tests).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A 4000 mm wall, each one 3000 mm north of the last, so that none touches another.
fn wall(s: &mut Session) -> u64 {
    let y = 3000 * s.model.elements.iter().filter(|(_, e)| e.category() == "wall").count();
    run(s, "wall", json!({"points": [[0, y], [4000, y]]}))["ids"][0].as_u64().unwrap()
}

#[test]
fn pin_and_unpin_hold_and_free_elements() {
    let mut s = Session::new();
    let (a, b) = (wall(&mut s), wall(&mut s));
    let r = run(&mut s, "pin", json!({"ids": [a]}));
    assert_eq!((r["pinned"].clone(), r["already"].clone()), (json!([a]), json!([])));
    assert!(s.model.is_pinned(a) && !s.model.is_pinned(b));
    assert_eq!(s.model.pinned_ids(), [a]);
    // The agent sees it on the element.
    let shown = run(&mut s, "elements", json!({"category": "wall"}));
    let of = |id: u64| shown["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).cloned().unwrap();
    assert_eq!(of(a)["pinned"], true);
    assert!(of(b).get("pinned").is_none());
    // Pinned twice is pinned once.
    assert_eq!(run(&mut s, "pin", json!({"ids": [a, b]}))["already"], json!([a]));
    assert_eq!(s.model.pinned_ids(), [a, b]);
    let r = run(&mut s, "unpin", json!({"ids": [a, b]}));
    assert_eq!(r["unpinned"], json!([a, b]));
    assert!(s.model.pinned_ids().is_empty());
    assert_eq!(run(&mut s, "unpin", json!({"ids": [a]}))["unpinned"], json!([]));
}

#[test]
fn the_shortcuts_are_pn_and_up() {
    assert_eq!(crate::engine::find("PN").map(|c| c.id), Some("pin"));
    assert_eq!(crate::engine::find("UP").map(|c| c.id), Some("unpin"));
}

#[test]
fn pin_refuses_what_is_not_there_and_what_it_does_not_take() {
    let mut s = Session::new();
    assert!(s.execute("pin", &json!({"ids": [999_999]})).is_err());
    assert!(s.execute("pin", &json!({})).is_err());
    let a = wall(&mut s);
    assert!(s.execute("pin", &json!({"ids": [a], "nonsense": 1})).is_err());
    assert!(s.model.pinned_ids().is_empty());
}

#[test]
fn a_command_moves_a_pinned_element_and_says_it_was_pinned() {
    let mut s = Session::new();
    let (a, b) = (wall(&mut s), wall(&mut s));
    run(&mut s, "pin", json!({"ids": [a]}));
    for (command, p, key) in [
        ("move", json!({"ids": [a, b], "by": [0, 500]}), "moved"),
        ("rotate", json!({"ids": [a], "about": [0, 0], "angle": 90}), "rotated"),
        ("mirror", json!({"ids": [a], "axis": [[0, 0], [0, 1000]]}), "mirrored"),
    ] {
        let r = run(&mut s, command, p);
        assert_eq!(r["pinned"], json!([a]), "{command}");
        assert!(r["note"].as_str().is_some_and(|n| n.contains("pinned") && n.contains(&a.to_string())), "{command}: {r}");
        assert!(r.get(key).is_some(), "{command} did what it does: {r}");
    }
    // The pinned wall did move.
    let at = run(&mut s, "elements", json!({"category": "wall"}));
    assert_ne!(at["elements"][0]["start"], json!([0.0, 0.0]));
    // Free, it says nothing.
    run(&mut s, "unpin", json!({"ids": [a]}));
    let r = run(&mut s, "move", json!({"ids": [a], "by": [0, 500]}));
    assert!(r.get("pinned").is_none() && r.get("note").is_none(), "{r}");
}

#[test]
fn a_copy_of_a_pinned_element_is_free() {
    let mut s = Session::new();
    let a = wall(&mut s);
    run(&mut s, "pin", json!({"ids": [a]}));
    let made = run(&mut s, "copy", json!({"ids": [a], "by": [0, 1000]}));
    assert!(made.get("pinned").is_none());
    let copy = made["ids"][0].as_u64().unwrap();
    assert!(!s.model.is_pinned(copy) && s.model.is_pinned(a));
}

#[test]
fn a_pinned_element_can_be_deleted_and_the_pin_goes_with_it() {
    let mut s = Session::new();
    let (a, b) = (wall(&mut s), wall(&mut s));
    run(&mut s, "pin", json!({"ids": [a, b]}));
    let r = run(&mut s, "delete", json!({"ids": [a]}));
    assert_eq!(r["deleted"], json!([a]));
    assert_eq!(r["pinned"], json!([a]), "it says the element was pinned");
    assert_eq!(s.model.pinned_ids(), [b]);
    assert!(s.model.pinned.len() == 1, "no pin left for what is gone");
    // Undo brings the element back, pinned.
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.pinned_ids(), [a, b]);
    // A free element's delete says nothing.
    run(&mut s, "unpin", json!({"ids": [a]}));
    assert!(run(&mut s, "delete", json!({"ids": [a]})).get("pinned").is_none());
}

#[test]
fn a_pin_is_kept_in_the_file_and_a_file_without_pins_is_as_before() {
    let mut s = Session::new();
    let a = wall(&mut s);
    let plain = serde_json::to_string(&*s.model).unwrap();
    assert!(!plain.contains("pinned"), "nothing pinned, nothing written: a file stays as it was");
    run(&mut s, "pin", json!({"ids": [a]}));
    let text = serde_json::to_string(&*s.model).unwrap();
    assert!(text.contains("pinned"));
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.pinned_ids(), [a]);
    assert_eq!(Model::read(&plain).unwrap().0.pinned_ids(), Vec::<u64>::new());
}

#[test]
fn grids_and_levels_can_be_pinned() {
    let mut s = Session::new();
    let grid = run(&mut s, "grid", json!({"start": [0, 0], "end": [0, 8000]}))["id"].as_u64().unwrap();
    let level = run(&mut s, "elements", json!({"category": "level"}))["elements"][0]["id"].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [grid, level]}));
    let mut both = vec![grid, level];
    both.sort_unstable();
    assert_eq!(s.model.pinned_ids(), both);
    let r = run(&mut s, "move", json!({"ids": [grid], "by": [100, 0]}));
    assert_eq!(r["pinned"], json!([grid]));
}

#[test]
fn a_grids_end_moves_alone_and_the_other_stays() {
    let mut s = Session::new();
    let grid = run(&mut s, "grid", json!({"start": [0, 0], "end": [0, 8000]}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [grid], "by": [0, 1500], "end": "end"}));
    let g = run(&mut s, "elements", json!({"category": "grid"}));
    assert_eq!((g["elements"][0]["start"].clone(), g["elements"][0]["end"].clone()), (json!([0.0, 0.0]), json!([0.0, 9500.0])));
    run(&mut s, "move", json!({"ids": [grid], "by": [0, -500], "end": "start"}));
    let g = run(&mut s, "elements", json!({"category": "grid"}));
    assert_eq!((g["elements"][0]["start"].clone(), g["elements"][0]["end"].clone()), (json!([0.0, -500.0]), json!([0.0, 9500.0])));
    // One grid at a time.
    let other = run(&mut s, "grid", json!({"start": [5000, 0], "end": [5000, 8000]}))["id"].as_u64().unwrap();
    assert!(s.execute("move", &json!({"ids": [grid, other], "by": [0, 1], "end": "end"})).is_err());
}

#[test]
fn a_dimensions_number_moves_along_and_across_its_line_and_goes_back() {
    let mut s = Session::new();
    // Two walls, each measured to at a point of its own centre line.
    run(&mut s, "wall", json!({"points": [[0, 0], [0, 3000]]}));
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 3000]]}));
    let d = run(&mut s, "dimension", json!({"refs": [[0, 1000], [4000, 1000]], "at": [0, -1000]}))["id"].as_u64().unwrap();
    let shifts = |s: &mut Session| run(s, "elements", json!({"category": "dimension"}))["elements"][0].get("text_shifts").cloned();
    assert_eq!(shifts(&mut s), None);
    let r = run(&mut s, "dimension_text_move", json!({"id": d, "along": 300, "across": -200}));
    assert_eq!((r["along"].clone(), r["across"].clone(), r["segment"].clone()), (json!(300.0), json!(-200.0), json!(0)));
    assert_eq!(shifts(&mut s), Some(json!([{"segment": 0, "along": 300.0, "across": -200.0}])));
    let spots = s.model.dimension_text_spots(match s.model.elements.get(d) {
        Some(crate::model::Element::Dimension(d)) => d,
        _ => panic!("a dimension"),
    });
    assert_eq!(spots.len(), 1);
    assert!((spots[0].1 - crate::geom::P2(2000.0, -1000.0)).length() < 1e-6, "the number's place is the middle of its segment");
    // It draws where it was put: the plan differs from the unmoved one.
    let view = crate::render::View { scale: 0.1, cx: 2000.0, cy: -500.0, width: 800, height: 600 };
    let points = |d: &crate::render::Drawing| d.prims.iter().flat_map(|p| p.points()).collect::<Vec<_>>();
    let moved = crate::render::draw(&s.model, 100000, view);
    run(&mut s, "dimension_text_move", json!({"id": d, "reset": true}));
    assert_eq!(shifts(&mut s), None);
    let home = crate::render::draw(&s.model, 100000, view);
    assert_ne!(points(&moved), points(&home));
    assert!(s.execute("dimension_text_move", &json!({"id": d, "segment": 5})).is_err());
    assert!(s.execute("dimension_text_move", &json!({"id": d, "along": "far"})).is_err());
}

#[test]
fn a_dragged_element_does_not_snap_to_itself() {
    use crate::snap::{Kind, Reach, Snaps, snap, snap_skipping};
    let mut s = Session::new();
    let a = wall(&mut s);
    let at = crate::geom::P2(4010.0, 5.0);
    let reach = Reach { point: 50.0, line: 20.0 };
    let with = snap(&s.model, 100000, at, reach, &[], false, &Snaps::default());
    assert_eq!((with.kind, with.source), (Kind::Endpoint, Some(a)));
    let without = snap_skipping(&s.model, 100000, at, reach, &[], false, &Snaps::default(), &[a]);
    assert_ne!(without.source, Some(a));
}

#[test]
fn the_commands_that_change_where_a_pinned_element_is_say_it_was_pinned() {
    let mut s = Session::new();
    let said = |r: &Value, id: u64, what: &str| {
        assert_eq!(r["pinned"], json!([id]), "{what}: {r}");
        assert!(r["note"].as_str().is_some_and(|n| n.contains("pinned")), "{what}: {r}");
    };
    // Align turns and moves it; `modify` of its ends moves it too, but its height is no move.
    let a = wall(&mut s);
    run(&mut s, "pin", json!({"ids": [a]}));
    let r = run(&mut s, "align", json!({"id": a, "reference": [[0, 100], [4000, 100]], "line": [[0, 0], [4000, 0]]}));
    said(&r, a, "align");
    let r = run(&mut s, "modify", json!({"id": a, "start": [0, 300]}));
    assert!(r["note"].as_str().is_some_and(|n| n.contains("pinned")) && r["pinned"] == json!(true), "modify a wall's start: {r}");
    let r = run(&mut s, "modify", json!({"id": a, "end": [4000, 300]}));
    assert!(r["note"].as_str().is_some_and(|n| n.contains("pinned")), "modify a wall's end: {r}");
    let r = run(&mut s, "modify", json!({"id": a, "height": 2800}));
    assert!(r.get("note").is_none(), "a height is no move: {r}");
    // A door slid along its wall.
    let door = run(&mut s, "door", json!({"wall": a, "offset": 1000}))["id"].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [door]}));
    let r = run(&mut s, "modify", json!({"id": door, "offset": 1500}));
    assert!(r["note"].as_str().is_some_and(|n| n.contains("pinned")), "modify a door's offset: {r}");
    // A detail line offset in place; its copy is free.
    let line = run(&mut s, "detail_line", json!({"start": [0, 8000], "end": [4000, 8000]}))["id"].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [line]}));
    let r = run(&mut s, "offset", json!({"id": line, "distance": 100, "toward": [0, 9000], "copy": true}));
    assert!(r.get("pinned").is_none(), "{r}");
    said(&run(&mut s, "offset", json!({"id": line, "distance": 100, "toward": [0, 9000], "copy": false})), line, "offset");
    // Trimmed to a boundary, split, and trimmed to a corner.
    let boundary = run(&mut s, "detail_line", json!({"start": [2000, 7000], "end": [2000, 9000]}))["id"].as_u64().unwrap();
    let r = run(&mut s, "trim_extend", json!({"boundary": boundary, "ids": [line], "keep": [[500, 8100]]}));
    said(&r, line, "trim_extend");
    let w = run(&mut s, "wall", json!({"points": [[10000, 0], [10000, 3000]]}))["ids"][0].as_u64().unwrap();
    let across = run(&mut s, "wall", json!({"points": [[9000, 4000], [12000, 4000]]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [w]}));
    said(&run(&mut s, "corner", json!({"ids": [w, across]})), w, "corner");
    let r = run(&mut s, "split", json!({"id": w, "at": [10000, 2000]}));
    said(&r, w, "split");
}

#[test]
fn a_dimensions_typed_length_says_when_it_moved_a_pinned_wall() {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [0, 3000]]}));
    let far = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 3000]]}))["ids"][0].as_u64().unwrap();
    let d = run(&mut s, "dimension", json!({"refs": [[0, 1000], [4000, 1000]], "at": [0, -1000]}))["id"].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [far]}));
    let r = run(&mut s, "dimension_value", json!({"id": d, "value": 3000}));
    assert_eq!(r["pinned"], json!([far]), "{r}");
}

#[test]
fn a_baseline_dimensions_numbers_have_places_to_drag_them_from() {
    let mut s = Session::new();
    let line = |s: &mut Session, x: f64| run(s, "detail_line", json!({"start": [x, 0], "end": [x, 3000]}))["id"].as_u64().unwrap();
    let (a, b, c) = (line(&mut s, 0.0), line(&mut s, 4000.0), line(&mut s, 8000.0));
    run(&mut s, "dimension_type", json!({"name": "Baseline", "string": "baseline", "baseline_spacing": 10}));
    let d = run(&mut s, "dimension", json!({"refs": [{"id": a}, {"id": b}, {"id": c}], "at": [2000, -2000], "type": "Baseline"}))["id"]
        .as_u64()
        .unwrap();
    let Some(crate::model::Element::Dimension(dim)) = s.model.elements.get(d) else { panic!("a dimension") };
    let spots = s.model.dimension_text_spots(dim);
    assert_eq!(spots.len(), 2, "one number on each of its two lines");
    // A mm of paper, in model mm, as the plan draws it.
    let level = s.model.levels().first().map(|l| l.0).unwrap_or_default();
    let drawn = crate::render::plan(&s.model, level, 800, 600);
    let mm = f64::from(drawn.k) / drawn.view.scale * crate::sheet::DENSITY;
    let (first, second) = (spots[0].1, spots[1].1);
    assert!((first - crate::geom::P2(2000.0, -2000.0)).length() < 1e-6, "{first:?}: the middle of a to b, on the first line");
    assert!((second - crate::geom::P2(6000.0, -2000.0 - 10.0 * mm)).length() < 1e-3, "{second:?}: the middle of b to c, a spacing out");
    // A number moved by its spot is a number moved: the command takes the segment.
    assert!(s.execute("dimension_text_move", &json!({"id": d, "segment": 1, "along": 100})).is_ok());
}

//! `sketch`'s tests that drive the Session (moved out of `sketch.rs`).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Element;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A square's lines, `[x, y]` its south-west corner, drawn clockwise.
fn square(x: f64, y: f64, size: f64) -> Vec<Value> {
    let c = [[x, y], [x, y + size], [x + size, y + size], [x + size, y]];
    (0..4).map(|i| json!([c[i], c[(i + 1) % 4]])).collect()
}

fn with_hole() -> Value {
    let mut ls = square(0.0, 0.0, 6000.0);
    ls.extend(square(2000.0, 2000.0, 1000.0));
    Value::Array(ls)
}

#[test]
fn a_sketch_is_solved_without_changing_the_model_and_says_what_is_wrong_line_by_line() {
    let mut s = Session::new();
    let steps = s.history().len();
    let r = run(&mut s, "sketch", json!({"lines": with_hole()}));
    assert_eq!(r["regions"].as_array().map(Vec::len), Some(1));
    assert_eq!(r["regions"][0]["holes"].as_array().map(Vec::len), Some(1));
    assert_eq!(r["area_m2"], 35.0);
    assert_eq!(r["regions"][0]["outline"][1], json!([6000.0, 0.0]), "counter-clockwise whichever way it was drawn");
    assert_eq!(s.history().len(), steps, "no undo step");
    // Open, crossing: every problem, with the lines it's about.
    let mut ls = square(0.0, 0.0, 4000.0);
    ls.pop();
    ls.push(json!([[-1000, 2000], [1000, 2000]]));
    let e = s.execute("sketch", &json!({"lines": ls})).unwrap_err();
    assert!(e.contains("line 0 and line 3 intersect at [0, 2000]"), "{e}");
    assert!(e.contains("line 2 ends at [4000, 0] without meeting another"), "{e}");
    let e = s.execute("sketch", &json!({"lines": [[[0, 0], [1]]]})).unwrap_err();
    assert!(e.contains("`lines` is"), "{e}");
}

#[test]
fn floors_ceilings_roofs_and_shafts_take_their_outline_and_holes_as_a_sketch() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"sketch": with_hole()}));
    assert_eq!(f["area_m2"], 35.0);
    let Some(Element::Floor(fl)) = s.model.elements.get(f["id"].as_u64().unwrap()).cloned() else { panic!() };
    assert_eq!((fl.boundary.len(), fl.openings.len()), (4, 1));
    assert!(crate::geom::area(&fl.boundary) > 0.0);
    let c = run(&mut s, "ceiling", json!({"sketch": with_hole()}));
    assert_eq!(c["area_m2"], 35.0);
    let r = run(&mut s, "roof", json!({"sketch": with_hole(), "slope": 0}));
    let Some(Element::Roof(rf)) = s.model.elements.get(r["id"].as_u64().unwrap()) else { panic!() };
    assert_eq!(rf.courtyards.len(), 1, "a roof's holes are its courtyards");
    run(&mut s, "shaft", json!({"sketch": square(0.0, 0.0, 1000.0)}));
    let e = s.execute("shaft", &json!({"sketch": with_hole()})).unwrap_err();
    assert!(e.contains("no holes"), "{e}");
    // One outline: two apart, or a sketch with a boundary too, is refused.
    let mut two = square(0.0, 0.0, 1000.0);
    two.extend(square(5000.0, 0.0, 1000.0));
    let e = s.execute("floor", &json!({"sketch": two})).unwrap_err();
    assert!(e.contains("2 separate outlines"), "{e}");
    let e = s.execute("floor", &json!({"sketch": with_hole(), "boundary": [[0, 0], [1000, 0], [0, 1000]]})).unwrap_err();
    assert!(e.contains("`sketch` or `boundary`"), "{e}");
}

#[test]
fn a_boundary_given_as_points_is_judged_as_the_sketch_of_its_edges() {
    let mut s = Session::new();
    let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 4000]]})).unwrap_err();
    assert!(e.contains("edge 0 and edge 2 intersect at [2000, 2000]"), "{e}");
    let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "openings": [[[1000, 1000], [2000, 2000], [2000, 1000], [1000, 2000]]]}))
        .unwrap_err();
    assert!(e.contains("opening 0") && e.contains("intersect"), "{e}");
    let e = s.execute("roof", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 4000]]})).unwrap_err();
    assert!(e.contains("intersect"), "{e}");
    // Closed back to its first point, or a point twice, it's the same outline.
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 0], [4000, 4000], [0, 4000], [0, 0]]}));
    let Some(Element::Floor(fl)) = s.model.elements.get(f["id"].as_u64().unwrap()) else { panic!() };
    assert_eq!(fl.boundary.len(), 4);
}

/// A bow tie with a quarter of its area left: no edge across another but the first and third.
const BOW: [[i32; 2]; 4] = [[0, 0], [4000, 4000], [4000, 0], [0, 2000]];

#[test]
fn a_simple_outline_is_asked_of_whatever_a_command_makes_or_changes_and_not_only_of_floor() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
    let c = run(&mut s, "ceiling", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
    let r = run(&mut s, "roof", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "slope": 0}))["id"].clone();
    let h = run(&mut s, "shaft", json!({"boundary": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]]}))["id"].clone();
    let before = s.model.clone();
    // Modify set it too, where only `floor` refused it: the outline, an opening, a courtyard.
    for (id, key, value) in [
        (&f, "boundary", json!(BOW)),
        (&c, "boundary", json!(BOW)),
        (&r, "boundary", json!(BOW)),
        (&h, "boundary", json!(BOW)),
        (&f, "openings", json!([[[1000, 1000], [2000, 2000], [2000, 1000], [1000, 2000]]])),
        (&r, "courtyards", json!([[[1000, 1000], [2000, 2000], [2000, 1000], [1000, 2000]]])),
    ] {
        let e = s.execute("modify", &json!({"id": id, key: value})).unwrap_err();
        assert!(e.contains("isn't a simple outline") && e.contains("edge"), "{key}: {e}");
    }
    assert_eq!(*s.model, *before);
    // A keyhole slit, a point on another edge and a spike are not simple either.
    let slit = json!([[0, 0], [4000, 0], [4000, 4000], [2000, 4000], [2000, 2000], [2000, 4000], [0, 4000]]);
    assert!(s.execute("modify", &json!({"id": h, "boundary": slit})).unwrap_err().contains("simple"));
    // A simple one is as before.
    run(&mut s, "modify", json!({"id": f, "boundary": [[0, 0], [5000, 0], [5000, 3000], [0, 3000]]}));
}

/// A floor whose outline is `pts` as an old file would hold it: the check of the whole project lets it by.
fn old_floor(s: &mut Session, pts: &[[f64; 2]]) -> u64 {
    let f = run(s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].as_u64().unwrap();
    if let Some(Element::Floor(fl)) = s.model_mut().elements.get_mut(f) {
        fl.boundary = pts.iter().map(|p| crate::geom::P2(p[0], p[1])).collect();
    }
    assert_eq!(s.model.check(), Ok(()));
    f
}

#[test]
fn an_outline_as_bad_as_it_was_moves_copies_turns_and_mirrors_and_one_made_worse_is_refused() {
    let bad: [(&str, &[[f64; 2]]); 3] = [
        ("a bow tie", &[[0.0, 0.0], [4000.0, 4000.0], [4000.0, 0.0], [0.0, 2000.0]]),
        ("a half-millimetre edge", &[[0.0, 0.0], [4000.0, 0.0], [4000.0, 0.5], [4000.0, 4000.0], [0.0, 4000.0]]),
        ("a closing point repeated", &[[0.0, 0.0], [4000.0, 0.0], [4000.0, 4000.0], [0.0, 4000.0], [0.0, 0.0]]),
    ];
    for (what, pts) in bad {
        let mut s = Session::new();
        let f = old_floor(&mut s, pts);
        // A rigid motion leaves its defects what they were: moved, copied, turned (also as a copy), mirrored (also as a copy).
        for (cmd, p) in [
            ("move", json!({"ids": [f], "by": [1000, 500]})),
            ("copy", json!({"ids": [f], "by": [1000, 500]})),
            ("rotate", json!({"ids": [f], "about": [100, 200], "angle": 33})),
            ("rotate", json!({"ids": [f], "about": [100, 200], "angle": 90, "copy": true})),
            ("mirror", json!({"ids": [f], "axis": [[1000, 0], [1000, 1000]]})),
            ("mirror", json!({"ids": [f], "axis": [[0, 0], [1000, 1000]], "copy": true})),
        ] {
            if let Err(e) = s.execute(cmd, &p) {
                panic!("{what}: {cmd} {p}: {e}");
            }
        }
        // Its shape changed it is not: one corner moved, another outline, or a clean one made bad.
        let mut other = pts.to_vec();
        other[1][1] += 700.0;
        for boundary in [json!(other), json!([[0, 0], [100, 4000], [4000, 0], [0, 2000]])] {
            let e = s.execute("modify", &json!({"id": f, "boundary": boundary})).unwrap_err();
            assert!(e.contains("isn't a simple outline"), "{what}: {e}");
        }
        let g = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
        let e = s.execute("modify", &json!({"id": g, "boundary": BOW})).unwrap_err();
        assert!(e.contains("isn't a simple outline"), "{what}: {e}");
        // Nor does a copy that stretches it pass as a copy: scaled by two it is a different outline.
        let big: Vec<[f64; 2]> = pts.iter().map(|p| [p[0] * 2.0, p[1] * 2.0]).collect();
        let e = s.execute("modify", &json!({"id": f, "boundary": big})).unwrap_err();
        assert!(e.contains("isn't a simple outline"), "{what}: {e}");
    }
}

#[test]
fn a_new_floor_with_the_outline_of_a_bad_one_in_the_file_is_refused_and_a_copy_of_that_one_is_not() {
    let bow: Vec<[f64; 2]> = BOW.iter().map(|p| [f64::from(p[0]), f64::from(p[1])]).collect();
    let mut s = Session::new();
    let f = old_floor(&mut s, &bow);
    let clean = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
    // A brand new element is no copy of it, however alike: the same bow tie drawn by hand, or on a floor of another place.
    for boundary in [json!(BOW), json!([[1000, 0], [5000, 4000], [5000, 0], [1000, 2000]])] {
        let before = s.model.clone();
        let e = s.execute("floor", &json!({ "boundary": boundary })).unwrap_err();
        assert!(e.contains("isn't a simple outline"), "{e}");
        assert_eq!(s.model, before);
    }
    // A copy of it passes, named by id, by GUID or among others; so do a mirrored and a turned one.
    let guid = json!(s.model.guid(f));
    for (cmd, p) in [
        ("copy", json!({"ids": [f], "by": [9000, 0]})),
        ("copy", json!({"id": f, "by": [9000, 0]})),
        ("copy", json!({"ids": [guid], "by": [9000, 0]})),
        ("copy", json!({"ids": [clean, f], "by": [9000, 0]})),
        ("mirror", json!({"ids": [f], "axis": [[0, 0], [1000, 1000]], "copy": true})),
        ("rotate", json!({"ids": [f], "about": [100, 200], "angle": 33, "copy": true})),
    ] {
        if let Err(e) = s.execute(cmd, &p) {
            panic!("{cmd} {p}: {e}");
        }
    }
}

/// A regular `n`-gon 10 m across to the micron, with the corners `swap` and the next swapped: a small bow tie in a ring of
/// `n - 3` edges of one length.
fn regular_bow(n: usize, at: [f64; 2], swap: usize) -> Vec<[f64; 2]> {
    let micron = |v: f64| (v * 1000.0).round() / 1000.0;
    let mut ring: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [at[0] + micron(5000.0 * a.cos()), at[1] + micron(5000.0 * a.sin())]
        })
        .collect();
    ring.swap(swap, swap + 1);
    ring
}

#[test]
fn an_outline_of_thousands_of_equal_edges_as_bad_as_it_was_is_let_by_when_mirrored_turned_or_copied() {
    // Its edges are all the one length: every corner is a place to start comparing it with its image.
    let n = 5000;
    let mut s = Session::new();
    let f = old_floor(&mut s, &regular_bow(n, [0.0, 0.0], 10));
    for (cmd, p) in [
        ("mirror", json!({"ids": [f], "axis": [[1000, 0], [1000, 1000]]})),
        ("mirror", json!({"ids": [f], "axis": [[0, 0], [1000, 1000]], "copy": true})),
        ("rotate", json!({"ids": [f], "about": [100, 200], "angle": 33.3, "copy": true})),
        ("copy", json!({"ids": [f], "by": [20000, 0]})),
    ] {
        if let Err(e) = s.execute(cmd, &p) {
            panic!("{cmd} {p}: {e}");
        }
    }
    // Twenty of them, the bow tie of each at another corner, copied at once: each is compared with those before it.
    let mut s = Session::new();
    let ids: Vec<u64> = (0..20).map(|i| old_floor(&mut s, &regular_bow(n, [f64::from(i) * 20000.0, 0.0], 10 + 250 * i as usize))).collect();
    let made = run(&mut s, "copy", json!({"ids": ids, "by": [0, 20000]}));
    assert_eq!(made["ids"].as_array().map(Vec::len), Some(20));
    // And one made worse is still refused.
    let e = s.execute("modify", &json!({"id": ids[0], "boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 2000]]})).unwrap_err();
    assert!(e.contains("isn't a simple outline"), "{e}");
}

#[test]
fn a_floor_over_boxes_of_walls_that_touch_at_a_corner_says_what_to_give_instead_of_naming_nothing() {
    let mut s = Session::new();
    for x in [0, 4000] {
        run(&mut s, "wall", json!({"points": [[x, x], [x + 4000, x], [x + 4000, x + 4000], [x, x + 4000]], "closed": true}));
    }
    let before = s.model.clone();
    let e = s.execute("floor", &json!({})).unwrap_err();
    // Nothing was made, so nothing is named, and `edit_boundary` has nothing to edit.
    assert!(!e.contains("floor 1") && !e.contains("edit_boundary"), "{e}");
    assert!(e.contains("boundary"), "{e}");
    assert_eq!(s.model, before);
    // The same words for a boundary that is given and not simple.
    let e = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 4000], [4000, 0], [0, 2000]]})).unwrap_err();
    assert!(!e.contains("floor 1") && !e.contains("edit_boundary"), "{e}");
    // And a floor that is there is still sent to `edit_boundary` when a change breaks it.
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
    let e = s.execute("modify", &json!({"id": f, "boundary": BOW})).unwrap_err();
    assert!(e.contains(&format!("floor {f}")) && e.contains("edit_boundary"), "{e}");
}

#[test]
fn a_dormer_that_makes_a_hole_cross_itself_is_told_what_to_change_not_to_edit_the_boundary() {
    let mut s = Session::new();
    let roof = run(&mut s, "roof", json!({"boundary": [[0, 0], [12000, 0], [12000, 8000], [0, 8000]], "slope": 60, "offset": 1000}));
    // A low dormer on a steep roof: the hole its cheeks cut crosses itself.
    let before = s.model.clone();
    let e = s.execute("dormer", &json!({"host": roof["id"], "at": [6000, 1500], "width": 1600, "height": 100, "slope": 30})).unwrap_err();
    assert!(e.contains("opening 0 isn't a simple outline") && e.contains("dormer's width, height or position"), "{e}");
    // `edit_boundary` redraws a roof's footprint and courtyards, not its openings.
    assert!(!e.contains("edit_boundary"), "{e}");
    assert_eq!(s.model, before);
    // An opening given to `modify` is told to give a simple one, and nothing of a dormer, which wasn't made;
    // a courtyard still goes to `edit_boundary`.
    let e = s.execute("modify", &json!({"id": roof["id"], "openings": [BOW_HOLE]})).unwrap_err();
    assert!(e.contains("give `modify` a simple `openings`") && !e.contains("edit_boundary") && !e.contains("dormer"), "{e}");
    let e = s.execute("modify", &json!({"id": roof["id"], "courtyards": [BOW_HOLE]})).unwrap_err();
    assert!(e.contains("courtyard 0") && e.contains("edit_boundary"), "{e}");
}

/// A spike (the outline goes up an edge's line and comes back down it), and a corner that touches another edge.
const SPIKE: [[i32; 2]; 6] = [[0, 0], [4000, 0], [4000, 4000], [4000, 6000], [4000, 5000], [0, 4000]];
const TOUCH: [[i32; 2]; 5] = [[0, 0], [4000, 0], [4000, 4000], [2000, 0], [0, 4000]];

#[test]
fn what_a_mass_with_an_outline_that_is_not_simple_makes_is_refused_saying_the_mass_outline_is_not_simple() {
    let mut s = Session::new();
    // A mass is let have the outline (as it always was), and what it makes is refused. The bow tie crosses itself, the
    // spike and the touching corner do not, so none is told it does.
    for (what, outline) in [("bow tie", json!(BOW)), ("spike", json!(SPIKE)), ("touch", json!(TOUCH))] {
        let mass = run(&mut s, "mass", json!({"boundary": outline, "height": 3000}))["id"].clone();
        for (cmd, p) in
            [("mass_floors", json!({"mass": mass})), ("floor_by_face", json!({"mass": mass})), ("roof_by_face", json!({"mass": mass}))]
        {
            let before = s.model.clone();
            let e = s.execute(cmd, &p).unwrap_err();
            let (named, said) = (format!("mass {mass}"), "outline isn't simple");
            assert!(e.contains(&named) && e.contains(said) && e.contains("redraw the mass"), "{what} {cmd}: {e}");
            assert!(!e.contains("crosses itself"), "{what} {cmd}: {e}");
            // No `boundary` or `sketch` was given to `mass_floors`, `floor_by_face` or `roof_by_face`, and no walls to touch.
            assert!(!e.contains("`boundary`") && !e.contains("walls"), "{what} {cmd}: {e}");
            assert_eq!(s.model, before);
        }
    }
}

#[test]
fn a_new_element_with_openings_or_courtyards_that_are_not_simple_is_told_to_give_simple_ones() {
    let square = json!([[0, 0], [4000, 0], [4000, 4000], [0, 4000]]);
    for (cmd, key, ask) in [
        ("floor", "openings", "give simple `openings`"),
        ("ceiling", "openings", "give simple `openings`"),
        ("roof", "openings", "give simple `openings`"),
        ("roof", "courtyards", "give simple `courtyards`"),
    ] {
        let mut p = json!({"boundary": square, key: [BOW_HOLE]});
        if cmd == "roof" {
            p["slope"] = json!(0);
        }
        let e = Session::new().execute(cmd, &p).unwrap_err();
        assert!(e.contains(ask) && !e.contains("`boundary`") && !e.contains("`sketch`"), "{cmd} {key}: {e}");
    }
}

#[test]
fn the_walls_that_touch_at_a_corner_are_named_only_for_an_outline_taken_from_walls() {
    let mut s = Session::new();
    for x in [0, 4000] {
        run(&mut s, "wall", json!({"points": [[x, x], [x + 4000, x], [x + 4000, x + 4000], [x, x + 4000]], "closed": true}));
    }
    let aside = "that touch at a corner";
    let e = s.execute("floor", &json!({})).unwrap_err();
    assert!(e.contains(aside), "an outline from walls: {e}");
    for (cmd, p) in [("floor", json!({"boundary": BOW})), ("ceiling", json!({"boundary": BOW})), ("shaft", json!({"boundary": BOW}))] {
        let e = s.execute(cmd, &p).unwrap_err();
        assert!(e.contains("give a simple `boundary` or `sketch`") && !e.contains(aside), "{cmd} {p}: {e}");
    }
}

/// A bow tie inside a 4 m square, for a hole.
const BOW_HOLE: [[i32; 2]; 4] = [[1000, 1000], [3000, 3000], [3000, 1000], [1000, 2000]];

#[test]
fn every_ring_a_floor_a_ceiling_a_roof_or_a_shaft_takes_is_asked_to_be_simple_made_or_modified() {
    let square = json!([[0, 0], [4000, 0], [4000, 4000], [0, 4000]]);
    // Each ring a command takes: floor and ceiling `boundary` and `openings`, roof `boundary`, `courtyards` and `openings`,
    // shaft `boundary`; a bow tie in each, as the command makes the element and as `modify` changes it.
    let rings: [(&str, &str, Option<&str>); 8] = [
        ("floor", "boundary", None),
        ("floor", "openings", Some("opening 0")),
        ("ceiling", "boundary", None),
        ("ceiling", "openings", Some("opening 0")),
        ("roof", "boundary", None),
        ("roof", "courtyards", Some("courtyard 0")),
        ("roof", "openings", Some("opening 0")),
        ("shaft", "boundary", None),
    ];
    for (cmd, key, name) in rings {
        let (outline, bow) = (json!(square), if name.is_some() { json!([BOW_HOLE]) } else { json!(BOW) });
        let mut made = json!({ key: bow });
        if name.is_some() {
            made["boundary"] = outline.clone();
        }
        if cmd == "roof" {
            made["slope"] = json!(0);
        }
        let mut s = Session::new();
        let blank = s.model.clone();
        let e = s.execute(cmd, &made).unwrap_err();
        assert!(e.contains("intersect") || e.contains("simple"), "{cmd} {key}: {e}");
        assert_eq!(s.model, blank, "{cmd} {key}: nothing was made");
        let mut ok = json!({"boundary": outline});
        if cmd == "roof" {
            ok["slope"] = json!(0);
        }
        let id = run(&mut s, cmd, ok)["id"].clone();
        let before = s.model.clone();
        let e = s.execute("modify", &json!({"id": id, key: made[key]})).unwrap_err();
        assert!(e.contains("isn't a simple outline") && e.contains(name.unwrap_or("boundary")), "modify {cmd} {key}: {e}");
        assert_eq!(s.model, before, "modify {cmd} {key}: nothing changed");
    }
}

#[test]
fn a_file_with_an_outline_that_is_not_simple_still_opens_and_what_changes_it_is_asked_to_redraw_it() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].as_u64().unwrap();
    // Written as an old file would have it: a boundary the check of the whole project lets by.
    if let Some(Element::Floor(fl)) = s.model_mut().elements.get_mut(f) {
        fl.boundary = BOW.iter().map(|p| crate::geom::P2(f64::from(p[0]), f64::from(p[1]))).collect();
    }
    assert_eq!(s.model.check(), Ok(()));
    let path = std::env::temp_dir().join(format!("bimcraft-bow-{}.bimcraft", std::process::id()));
    std::fs::write(&path, s.model.to_json().unwrap()).unwrap();
    let mut t = Session::new();
    let opened = t.execute("open", &json!({"path": path.to_string_lossy()}));
    let inserted = Session::new().execute("insert", &json!({"path": path.to_string_lossy()}));
    std::fs::remove_file(&path).ok();
    opened.unwrap();
    inserted.unwrap();
    // What leaves its outline alone goes through, and so does a move, which makes it no worse; what changes its shape is
    // asked to redraw it, and a redrawn one is fine.
    run(&mut t, "modify", json!({"id": f, "offset": 100}));
    run(&mut t, "move", json!({"ids": [f], "by": [1000, 0]}));
    let e = t.execute("modify", &json!({"id": f, "boundary": [[0, 0], [100, 4000], [4000, 0], [0, 2000]]})).unwrap_err();
    assert!(e.contains("isn't a simple outline") && e.contains("edit_boundary"), "{e}");
    run(&mut t, "edit_boundary", json!({"id": f, "sketch": square(0.0, 0.0, 4000.0)}));
    run(&mut t, "move", json!({"ids": [f], "by": [1000, 0]}));
    // Or it goes.
    let g = run(&mut t, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
    run(&mut t, "delete", json!({"ids": [g]}));
}

#[test]
fn one_line_crossing_itself_is_refused_as_the_same_points_in_four_lines_are() {
    let mut s = Session::new();
    let bow = json!([[0, 0], [4000, 4000], [4000, 0], [0, 2000]]);
    // A closed polyline is one sketch line: it used to pass, and report the shoelace area, 4 m², where the loop of
    // the two triangles it makes is not one outline at all.
    let one = json!([{"points": bow, "closed": true}]);
    for cmd in ["floor", "ceiling", "shaft", "sketch", "filled_region"] {
        let p = if cmd == "sketch" { json!({"lines": one}) } else { json!({"sketch": one}) };
        let e = s.execute(cmd, &p).unwrap_err();
        assert!(e.contains("line 0 crosses itself at [1333, 1333]"), "{cmd}: {e}");
    }
    let four = Value::Array((0..4).map(|i| json!([bow[i], bow[(i + 1) % 4]])).collect());
    let e = s.execute("floor", &json!({"sketch": four})).unwrap_err();
    assert!(e.contains("line 0 and line 2 intersect at [1333, 1333]"), "{e}");
    assert!(s.model.elements.is_empty() || s.model.elements.values().all(|e| !matches!(e, Element::Floor(_))));
}

#[test]
fn edit_boundary_redraws_an_outline_in_one_step_or_leaves_it_as_it_was() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]}))["id"].clone();
    // What it is now, as lines to edit.
    let now = run(&mut s, "edit_boundary", json!({"id": f}));
    assert_eq!(now["sketch"].as_array().map(Vec::len), Some(4));
    let r = run(&mut s, "edit_boundary", json!({"id": f, "sketch": with_hole()}));
    assert_eq!(r["area_m2"], 35.0);
    let Some(Element::Floor(fl)) = s.model.elements.get(f.as_u64().unwrap()) else { panic!() };
    assert_eq!(fl.openings.len(), 1);
    assert_eq!(s.history().last(), Some(&"Edit Boundary"));
    let mut open = square(0.0, 0.0, 4000.0);
    open.pop();
    let before = s.model.clone();
    let e = s.execute("edit_boundary", &json!({"id": f, "sketch": open})).unwrap_err();
    assert!(e.contains("open"), "{e}");
    assert_eq!(s.model, before);
    run(&mut s, "undo", json!({}));
    let Some(Element::Floor(fl)) = s.model.elements.get(f.as_u64().unwrap()) else { panic!() };
    assert!(fl.openings.is_empty());
    // A wall has no boundary.
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].clone();
    let e = s.execute("edit_boundary", &json!({"id": w, "sketch": with_hole()})).unwrap_err();
    assert!(e.contains("floor, a ceiling, a roof's footprint, a shaft or a filled region"), "{e}");
}

#[test]
fn a_roofs_gable_edges_stay_gables_where_the_edited_footprint_keeps_them() {
    let mut s = Session::new();
    let r =
        run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "gables": [1, 3], "slope": 30}))["id"].clone();
    // Its south edge moved down: the east and west edges are new lines, the north edge the same.
    let lines = json!([[[0, -1000], [8000, -1000]], [[8000, -1000], [8000, 6000]], [[8000, 6000], [0, 6000]], [[0, 6000], [0, -1000]]]);
    run(&mut s, "edit_boundary", json!({"id": r, "sketch": lines}));
    let Some(Element::Roof(rf)) = s.model.elements.get(r.as_u64().unwrap()) else { panic!() };
    assert!(rf.gables.is_empty(), "{:?}", rf.gables);
    // Back, and the gables on the east and west ends again, then the north edge in two lines.
    run(&mut s, "undo", json!({}));
    let lines = json!([
        [[0, 0], [8000, 0]],
        [[8000, 0], [8000, 6000]],
        [[8000, 6000], [4000, 6000]],
        [[4000, 6000], [0, 6000]],
        [[0, 6000], [0, 0]]
    ]);
    run(&mut s, "edit_boundary", json!({"id": r, "sketch": lines}));
    let Some(Element::Roof(rf)) = s.model.elements.get(r.as_u64().unwrap()) else { panic!() };
    assert_eq!(rf.gables, vec![1, 4]);
    // An extrusion roof is edited by its profile.
    let x =
        run(&mut s, "roof", json!({"extrusion": {"line": [[0, 0], [4000, 0]], "profile": [[0, 0], [2000, 1000], [4000, 0]], "to": 3000}}))
            ["id"]
            .clone();
    let e = s.execute("edit_boundary", &json!({"id": x, "sketch": with_hole()})).unwrap_err();
    assert!(e.contains("extrusion"), "{e}");
}

/// A floor's sketch lines, as `elements` gives them.
fn lines(s: &mut Session, id: &Value) -> Vec<Value> {
    let r = run(s, "elements", json!({"category": "floor"}));
    let found = r["elements"].as_array().and_then(|es| es.iter().find(|e| e["id"] == *id).cloned()).unwrap_or_default();
    found["sketch"].as_array().cloned().unwrap_or_default()
}

#[test]
fn each_sketch_line_has_an_id_and_a_guid_of_its_own_kept_through_edits_and_moves() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"sketch": square(0.0, 0.0, 6000.0)}))["id"].clone();
    let kept = lines(&mut s, &f);
    assert_eq!(kept.len(), 4);
    let ids: Vec<u64> = kept.iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(ids.len(), 4, "{kept:?}");
    assert!(ids.iter().all(|&i| (100_000..=999_999).contains(&i)), "{ids:?}");
    assert!(kept.iter().all(|l| l["guid"].is_string()));
    // Edit Boundary gives them back; three sent again by id keep theirs, the fourth redrawn as two is new.
    let now = run(&mut s, "edit_boundary", json!({"id": f}))["sketch"].clone();
    assert_eq!(now.as_array().map(Vec::len), Some(4));
    let mut again: Vec<Value> =
        now.as_array().unwrap()[..3].iter().map(|l| json!({"id": l["id"], "start": l["start"], "end": l["end"]})).collect();
    let last = &now[3];
    let mid = json!([3000.0, 0.0]);
    again.push(json!([last["start"], mid]));
    again.push(json!([mid, last["end"]]));
    run(&mut s, "edit_boundary", json!({"id": f, "sketch": again}));
    let after = lines(&mut s, &f);
    let after_ids: Vec<u64> = after.iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(after_ids[..3], ids[..3]);
    assert!(after_ids[3..].iter().all(|i| !ids.contains(i)) && after_ids.len() == 5, "{after_ids:?}");
    // An id that isn't one of its lines is refused.
    let e = s.execute("edit_boundary", &json!({"id": f, "sketch": [{"id": 123_456, "start": [0, 0], "end": [1, 1]}]})).unwrap_err();
    assert!(e.contains("123456 isn't a line of this sketch"), "{e}");
    // Moved, the sketch goes with it and keeps its ids; copied, the copy's lines are new.
    run(&mut s, "move", json!({"ids": [f], "by": [1000, 0]}));
    let moved = lines(&mut s, &f);
    assert_eq!(moved[0]["id"], after[0]["id"]);
    assert_eq!(moved[0]["start"][0].as_f64(), after[0]["start"][0].as_f64().map(|x| x + 1000.0));
    let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 10_000]}))["ids"][0].clone();
    let copied: Vec<u64> = lines(&mut s, &copy).iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(copied.len(), 5);
    assert!(copied.iter().all(|i| !after_ids.contains(i)), "{copied:?}");
    // A floor made over the walls keeps none until its boundary is edited; its lines then come as new ones.
    run(&mut s, "wall", json!({"points": [[20000, 0], [26000, 0], [26000, 6000], [20000, 6000]], "closed": true}));
    let w = run(&mut s, "floor", json!({}))["id"].clone();
    assert!(lines(&mut s, &w).is_empty());
    let fresh = run(&mut s, "edit_boundary", json!({"id": w}))["sketch"].clone();
    assert!(fresh.as_array().unwrap().iter().all(|l| l.get("id").is_none()));
}

#[test]
fn a_boundary_line_sent_back_by_its_id_keeps_its_style_and_its_invisibility_unless_given_them_again() {
    let mut s = Session::new();
    let sketch = json!([
        {"start": [0, 0], "end": [4000, 0], "style": "Hidden Lines"},
        {"start": [4000, 0], "end": [4000, 3000], "invisible": true},
        [[4000, 3000], [0, 0]]
    ]);
    let r = run(&mut s, "filled_region", json!({"sketch": sketch}))["id"].clone();
    let now = run(&mut s, "edit_boundary", json!({"id": r}))["sketch"].clone();
    let (a, b, c) = (&now[0], &now[1], &now[2]);
    // Only where they go, by id.
    let back = json!([
        {"id": a["id"], "start": [0, 0], "end": [4000, 0]},
        {"id": b["id"], "start": [4000, 0], "end": [4000, 3000]},
        {"id": c["id"], "start": [4000, 3000], "end": [0, 0]}
    ]);
    run(&mut s, "edit_boundary", json!({"id": r, "sketch": back}));
    let after = run(&mut s, "edit_boundary", json!({"id": r}))["sketch"].clone();
    assert_eq!(after[0]["style"], "Hidden Lines");
    assert_eq!(after[1]["invisible"], true);
    assert!(after[2].get("style").is_none() && after[2].get("invisible").is_none());
    // Given again, it is what it is told: visible, and in another style.
    let told = json!([
        {"id": a["id"], "start": [0, 0], "end": [4000, 0], "style": "Wide Lines"},
        {"id": b["id"], "start": [4000, 0], "end": [4000, 3000], "invisible": false},
        {"id": c["id"], "start": [4000, 3000], "end": [0, 0]}
    ]);
    run(&mut s, "edit_boundary", json!({"id": r, "sketch": told}));
    let after = run(&mut s, "edit_boundary", json!({"id": r}))["sketch"].clone();
    assert_eq!(after[0]["style"], "Wide Lines");
    assert!(after[1].get("invisible").is_none());
}

#[test]
fn a_sketch_lines_id_is_not_given_again_while_the_project_is_open_after_the_line_has_gone() {
    let mut s = Session::new();
    let f = run(&mut s, "floor", json!({"sketch": square(0.0, 0.0, 4000.0)}))["id"].clone();
    let first = lines(&mut s, &f);
    let ids: Vec<u64> = first.iter().filter_map(|l| l["id"].as_u64()).collect();
    // Down to a triangle on the first three lines' ids: the fourth's goes.
    let tri = [([0, 0], [4000, 0]), ([4000, 0], [4000, 4000]), ([4000, 4000], [0, 0])];
    let kept: Vec<Value> = tri.iter().zip(&ids).map(|((a, b), id)| json!({"id": id, "start": a, "end": b})).collect();
    run(&mut s, "edit_boundary", json!({"id": f, "sketch": kept}));
    let now: Vec<u64> = lines(&mut s, &f).iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(now, ids[..3], "the three keep theirs");
    // A line added next time: its id is nobody's before, the fourth's included.
    let mut more: Vec<Value> = kept.clone();
    more[2] = json!({"id": ids[2], "start": [2000, 2000], "end": [0, 0]});
    more.push(json!({"start": [4000, 4000], "end": [2000, 2000]}));
    run(&mut s, "edit_boundary", json!({"id": f, "sketch": more}));
    let after: Vec<u64> = lines(&mut s, &f).iter().filter_map(|l| l["id"].as_u64()).collect();
    assert_eq!(after.len(), 4);
    assert!(!ids[3..].contains(&after[3]) && after[3] < ids[3], "{after:?} against {ids:?}");
    // A copy's lines are new ones too, lower still.
    let copy = run(&mut s, "copy", json!({"ids": [f], "by": [0, 9000]}))["ids"][0].clone();
    let copied: Vec<u64> = lines(&mut s, &copy).iter().filter_map(|l| l["id"].as_u64()).collect();
    assert!(copied.iter().all(|c| *c < after[3] && !ids.contains(c)), "{copied:?}");
}

#[test]
fn an_arc_sketched_rounds_the_floor() {
    let mut s = Session::new();
    let half = json!([[[0, 0], [4000, 0]], [[4000, 0], [0, 0], [2000, 2000]]]);
    let f = run(&mut s, "floor", json!({"sketch": half}));
    let a = f["area_m2"].as_f64().unwrap();
    assert!((a - std::f64::consts::TAU).abs() < 0.1, "{a}");
    // Kept by how far it turns, counter-clockwise as a wall's arc: west from (4000, 0), bulging north to its right.
    let turn = lines(&mut s, &f["id"])[1]["arc"].as_f64().unwrap_or_default();
    assert!((turn - 180.0).abs() < 1e-6, "{turn}");
}

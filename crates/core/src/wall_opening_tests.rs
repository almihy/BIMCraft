//! Revit's Wall Opening (`wall_opening`, `WallOpening`) and Automatically Embed (a curtain wall type's `embed`): a
//! rectangle cut through a wall, and a curtain wall drawn in a basic wall cutting its own opening there.

use serde_json::{Value, json};

use crate::curtain_tests::{fails, run, wall};
use crate::engine::Session;
use crate::model::{Element, Id};

/// The element `id` as `elements` lists it.
fn listed(s: &mut Session, id: Id) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"].as_u64() == Some(id)).cloned().expect("listed")
}

fn volume(s: &mut Session, id: Id) -> f64 {
    listed(s, id)["volume_m3"].as_f64().unwrap()
}

/// A basic wall 10 m long, 200 thick and 3 m tall along x from the origin.
fn basic(s: &mut Session) -> Id {
    run(s, "type", json!({"category": "wall", "name": "Generic - 200mm", "thickness": 200}));
    wall(s, json!({"points": [[0, 0], [10000, 0]], "type": "Generic - 200mm", "height": 3000}))
}

#[test]
fn a_wall_opening_cuts_its_wall_and_goes_with_it() {
    let mut s = Session::new();
    let w = basic(&mut s);
    assert!((volume(&mut s, w) - 6.0).abs() < 1e-6);
    let r = run(&mut s, "wall_opening", json!({"wall": w, "offset": 3000, "width": 1000, "height": 2440}));
    let o = r["id"].as_u64().unwrap();
    assert_eq!(r["host"].as_u64(), Some(w));
    // 1 m by 2.44 m through 0.2 m
    assert!((volume(&mut s, w) - (6.0 - 0.488)).abs() < 1e-6, "{}", volume(&mut s, w));
    let e = listed(&mut s, o);
    assert_eq!(e["category"], "wallopening");
    assert_eq!((e["from"].as_f64(), e["to"].as_f64(), e["bottom"].as_f64(), e["top"].as_f64()), (Some(2500.0), Some(3500.0), Some(0.0), Some(2440.0)));
    // raised off the floor, as Revit's Base Offset
    run(&mut s, "modify", json!({"id": o, "base_offset": 900, "height": 1200}));
    assert!((volume(&mut s, w) - (6.0 - 0.24)).abs() < 1e-6);
    // the wall moved: the opening goes with it; it slides along its wall alone
    run(&mut s, "move", json!({"ids": [w], "by": [0, 5000]}));
    assert_eq!(listed(&mut s, o)["at"], json!([3000.0, 5000.0]));
    run(&mut s, "move", json!({"ids": [o], "by": [1000, 0]}));
    assert_eq!(listed(&mut s, o)["from"].as_f64(), Some(3500.0));
    // copied with its wall, and deleted with it
    run(&mut s, "copy", json!({"ids": [w], "by": [0, 3000]}));
    let count = |s: &Session| s.model.elements.values().filter(|e| matches!(e, Element::WallOpening(_))).count();
    assert_eq!(count(&s), 2);
    run(&mut s, "delete", json!({"ids": [w]}));
    assert_eq!(count(&s), 1);
    assert!(s.model.elements.get(o).is_none());
}

#[test]
fn a_wall_opening_is_refused_where_it_cannot_cut() {
    let mut s = Session::new();
    let w = basic(&mut s);
    assert!(fails(&mut s, "wall_opening", json!({"wall": w, "offset": 12000, "width": 1000, "height": 2000})).contains("past its ends"));
    assert!(fails(&mut s, "wall_opening", json!({"wall": w, "offset": 1000, "height": 2000})).contains("`width`"));
    assert!(fails(&mut s, "wall_opening", json!({"wall": w, "offset": 1000, "width": -5, "height": 2000})).contains("more than 0"));
    let cw = wall(&mut s, json!({"points": [[0, 5000], [6000, 5000]], "type": "Curtain Wall", "height": 3000}));
    assert!(fails(&mut s, "wall_opening", json!({"wall": cw, "offset": 1000, "width": 500, "height": 2000})).contains("curtain wall"));
}

#[test]
fn a_wall_opening_cuts_a_curved_wall_across() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "thickness": 200}));
    let w = wall(&mut s, json!({"points": [[0, 0], [10000, 0]], "arc": 90, "type": "Generic - 200mm", "height": 3000}));
    let before = volume(&mut s, w);
    let o = run(&mut s, "wall_opening", json!({"wall": w, "at": [5000, -2071], "width": 1000, "height": 2000}))["id"].as_u64().unwrap();
    let e = listed(&mut s, o);
    // at the arc's middle, its own length along it
    assert!((e["from"].as_f64().unwrap() + 500.0 - std::f64::consts::FRAC_PI_2 * 7071.0678 / 2.0).abs() < 2.0, "{e}");
    let cut = before - volume(&mut s, w);
    assert!((cut - 0.4).abs() < 0.005, "{cut}");
}

#[test]
fn glazing_of_a_type_that_embeds_cuts_the_basic_wall_it_stands_in() {
    let mut s = Session::new();
    let w = basic(&mut s);
    run(&mut s, "type", json!({"category": "wall", "name": "Embedded", "curtain": {"embed": true}}));
    run(&mut s, "type", json!({"category": "wall", "name": "Loose", "curtain": true}));
    // a window, 900 over the floor to 2200, in the wall
    let g = wall(&mut s, json!({"points": [[2000, 0], [6000, 0]], "type": "Embedded", "height": 1300, "base_offset": 900}));
    assert!((volume(&mut s, w) - (6.0 - 4.0 * 1.3 * 0.2)).abs() < 1e-6, "{}", volume(&mut s, w));
    assert_eq!(listed(&mut s, g)["embedded_in"], json!([w]));
    // embedded, it stands in the opening it cuts: no warning that the two overlap
    let warnings = run(&mut s, "warnings", json!({}));
    assert!(!warnings.to_string().contains("walls_overlap"), "{warnings}");
    // one of a type that does not embed stands in the wall uncut, and is warned of
    let l = wall(&mut s, json!({"points": [[7000, 0], [9000, 0]], "type": "Loose", "height": 2000}));
    assert!((volume(&mut s, w) - (6.0 - 4.0 * 1.3 * 0.2)).abs() < 1e-6);
    assert!(listed(&mut s, l).get("embedded_in").is_none());
    assert!(run(&mut s, "warnings", json!({})).to_string().contains("walls_overlap"));
    // Revit's Storefront embeds itself
    let sf = wall(&mut s, json!({"points": [[7000, 0], [9000, 0]], "type": "Storefront", "height": 2000}));
    assert_eq!(listed(&mut s, sf)["embedded_in"], json!([w]));
    // the type set not to embed any more: the wall is whole again where its glazing stands
    run(&mut s, "type", json!({"category": "wall", "name": "Embedded", "curtain": {"embed": false}}));
    run(&mut s, "delete", json!({"ids": [sf]}));
    assert!((volume(&mut s, w) - 6.0).abs() < 1e-6, "{}", volume(&mut s, w));
}

#[test]
fn glazing_beside_a_wall_or_across_it_cuts_nothing() {
    let mut s = Session::new();
    let w = basic(&mut s);
    run(&mut s, "type", json!({"category": "wall", "name": "Embedded", "curtain": {"embed": true}}));
    // beside its face, and across it
    let beside = wall(&mut s, json!({"points": [[2000, 300], [6000, 300]], "type": "Embedded", "height": 2000}));
    let across = wall(&mut s, json!({"points": [[8000, -2000], [8000, 2000]], "type": "Embedded", "height": 2000}));
    assert!((volume(&mut s, w) - 6.0).abs() < 1e-6);
    assert_eq!(listed(&mut s, beside)["embedded_in"], json!([]));
    assert_eq!(listed(&mut s, across)["embedded_in"], json!([]));
}

#[test]
fn curved_glazing_embeds_in_a_curved_wall_round_the_same_centre() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "thickness": 200}));
    let w = wall(&mut s, json!({"points": [[0, 0], [10000, 0]], "arc": 90, "type": "Generic - 200mm", "height": 3000}));
    let before = volume(&mut s, w);
    run(&mut s, "type", json!({"category": "wall", "name": "Embedded", "curtain": {"embed": true}}));
    // the same circle, centre (5000, 5000), from -120 to -60 degrees
    let r = 50_000_000f64.sqrt();
    let at = |deg: f64| json!([5000.0 + r * deg.to_radians().cos(), 5000.0 + r * deg.to_radians().sin()]);
    let g = wall(&mut s, json!({"points": [at(-120.0), at(-60.0)], "arc": 60, "type": "Embedded", "height": 3000}));
    assert_eq!(listed(&mut s, g)["embedded_in"], json!([w]));
    let cut = before - volume(&mut s, w);
    let expected = r * std::f64::consts::PI / 3.0 * 3000.0 * 200.0 / 1e9;
    assert!((cut - expected).abs() < 0.01, "{cut} {expected}");
}

/// The volume of every wall of the model, m3.
fn all_walls(s: &mut Session) -> f64 {
    let all = run(s, "elements", json!({"category": "wall"}));
    all["elements"].as_array().unwrap().iter().map(|e| e["volume_m3"].as_f64().unwrap()).sum()
}

#[test]
fn a_wall_put_on_its_face_keeps_its_joins_as_revits_does() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "thickness": 200}));
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 300mm", "thickness": 300}));
    // a room's corner and a tee, on their centre lines
    let a = wall(&mut s, json!({"points": [[0, 0], [5000, 0]], "type": "Generic - 200mm", "height": 3000}));
    let b = wall(&mut s, json!({"points": [[0, 5000], [0, 0]], "type": "Generic - 300mm", "height": 3000}));
    let c = wall(&mut s, json!({"points": [[2500, 3000], [2500, 0]], "type": "Generic - 200mm", "height": 3000}));
    let before = all_walls(&mut s);
    let solid = |s: &Session, id: Id| crate::walls::outline(&s.model, id);
    let outlines: Vec<_> = [a, b, c].iter().map(|id| solid(&s, *id)).collect();
    // each put on a face: the walls stay where they are, their lines move, and every join stays
    run(&mut s, "modify", json!({"id": a, "location": "exterior-finish"}));
    run(&mut s, "modify", json!({"id": b, "location": "interior-finish"}));
    run(&mut s, "modify", json!({"id": c, "location": "exterior-finish"}));
    assert!((all_walls(&mut s) - before).abs() < 1e-6, "{} {before}", all_walls(&mut s));
    let ring = |r: &Vec<crate::geom::P2>| {
        let mut v: Vec<(i64, i64)> = r.iter().map(|p| ((p.0 * 10.0).round() as i64, (p.1 * 10.0).round() as i64)).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    for (id, was) in [a, b, c].iter().zip(&outlines) {
        assert_eq!(ring(&solid(&s, *id)), ring(was), "wall {id}");
    }
    assert_eq!(crate::walls::joined_at(&s.model, a, false), vec![b]);
    // and moved, the wall joined at its centre line stretches with it
    run(&mut s, "move", json!({"ids": [b], "by": [-1000, 0]}));
    let wa = s.model.wall(a).unwrap().clone();
    assert!((crate::walls::Axis::of(&wa).length() - 6000.0).abs() < 1e-6, "{wa:?}");
}

//! The family engine's own rules, on original families built here (never Autodesk content): an arc that follows its ends, a
//! nested part drawn as Revit made it that follows the values its parent hands it, a nested part's symbols in every view,
//! a traced outline that crosses itself, and a void that cuts only the forms it names.

use serde_json::{Value, json};

use crate::family::{Family, Mark, SymbolView};
use crate::geom::P2;

fn family(v: Value) -> Family {
    serde_json::from_value(v).unwrap_or_else(|e| panic!("a family that reads: {e}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.1
}

/// The points of every line mark `f`'s type `t` draws in `view` at its own values.
fn lines(f: &Family, t: &str, view: SymbolView) -> Vec<Vec<P2>> {
    let v = f.values(t, &Default::default()).unwrap();
    f.marks(t, &v, view, None)
        .unwrap()
        .into_iter()
        .filter_map(|m| match m {
            Mark::Line(pts) => Some(pts),
            _ => None,
        })
        .collect()
}

/// A door of two types, 762 and 915 wide, whose leaf is 51 thick and hinged on the right jamb: its swing arc is centred on the
/// hinge and starts at the leaf's free corner, which moves with the width; the stored angles are
/// the first type's, as numbers.
fn swinging(first: f64) -> Family {
    let corner = |w: &str| json!([format!("{w} / 2 - 51"), w]);
    family(json!({
        "category": "door",
        "parameters": [{"name": "Width", "kind": "length", "value": first}, {"name": "Height", "kind": "length", "value": 2032}],
        "types": {"0762": {"Width": 762}, "0915": {"Width": 915}},
        "symbols": [{
            "arc": {"centre": ["Width / 2", 0], "radius": "sqrt(Width * Width + 51 * 51)", "from": 94.0, "to": 180,
                    "start": corner("Width"), "end": ["-Width / 2", 0]}
        }]
    }))
}

#[test]
fn a_swing_arc_starts_where_its_leaf_ends_in_every_type() {
    let door = swinging(762.0);
    for (t, w) in [("0762", 762.0), ("0915", 915.0)] {
        let arcs = lines(&door, t, SymbolView::Plan);
        let first = arcs[0][0];
        // The leaf's free corner: 51 in from the hinge's side of the opening, a leaf's width out.
        assert!(near(first.0, w / 2.0 - 51.0) && near(first.1, w), "{t}: the arc starts at the leaf's corner: {first:?}");
        // It turns counter-clockwise (the stored `to` is above `from`) to the shut leaf's end, across the opening.
        let last = arcs[0][arcs[0].len() - 1];
        assert!(last.1.abs() < 0.1 && last.0 < w / 2.0 - 51.0, "{t}: round to the shut leaf's end: {last:?}");
    }
}

#[test]
fn an_arc_with_one_end_only_is_refused_and_one_with_both_is_a_whole_turn_when_they_meet() {
    let mut half = swinging(762.0);
    half.symbols[0].arc.as_mut().unwrap().end = None;
    assert!(half.check(&|_| true).is_err_and(|e| e.contains("both `start` and `end`")), "start without end");
    let round = family(json!({
        "category": "generic", "types": {"A": {}},
        "symbols": [{"arc": {"centre": [0, 0], "radius": 100, "from": 0, "to": 1, "start": [100, 0], "end": [100, 0]}}]
    }));
    let pts = &lines(&round, "A", SymbolView::Plan)[0];
    assert!(pts.len() > 30 && pts[0].dist(pts[pts.len() - 1]) < 1e-6, "a circle: {} points", pts.len());
}

/// A box as twelve triangles turned out, as a snapshot mesh holds it.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Value {
    let p =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut tris = Vec::new();
    for [a, b, c, d] in quads {
        tris.push(json!([p(a), p(b), p(c)]));
        tris.push(json!([p(a), p(c), p(d)]));
    }
    json!(tris)
}

/// The box a family's type builds to, its nested parts' of `others`: its least and most corner.
fn extent(f: &Family, t: &str, others: crate::family::Others) -> ([f64; 3], [f64; 3]) {
    let v = f.values(t, &Default::default()).unwrap();
    let pieces = f.pieces_with(t, &v, others).unwrap();
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for p in pieces.iter().flat_map(|p| p.tris.iter().flatten()) {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    (lo, hi)
}

/// A post whose Revit result for its one type stands 1000 tall and 100 across, and whose definition, which didn't match it (its
/// check failed), stands `Height` tall and 120 across.
fn post() -> Family {
    family(json!({
        "category": "generic",
        "parameters": [{"name": "Height", "kind": "length", "value": 1000}],
        "types": {"Standard": {}},
        "forms": [
            {"mesh": cuboid([0.0, 0.0, 0.0], [100.0, 100.0, 1000.0]), "snapshot": true},
            {"profile": [[0, 0], [120, 0], [120, 120], [0, 120]], "from": 0, "to": "Height"}
        ],
        "check": {"Standard": {"matched": false, "reason": "its shape is off"}}
    }))
}

fn holder(given: Value) -> Family {
    family(json!({
        "category": "furniture", "types": {"Standard": {}},
        "nested": [{"family": "Sample Post", "type": "Standard", "at": [0, 0, 0], "params": given}]
    }))
}

#[test]
fn a_nested_part_drawn_as_revit_made_it_follows_the_parameters_its_parent_gives() {
    let post = post();
    let others = |n: &str| (n == "Sample Post").then_some(&post);
    // The parent hands it its own value: Revit's result stands, 100 across.
    let (_, hi) = extent(&holder(json!({"Height": 1000})), "Standard", &others);
    assert!(near(hi[2], 1000.0) && near(hi[0], 100.0), "Revit's result at its own values: {hi:?}");
    let (_, hi) = extent(&holder(json!({})), "Standard", &others);
    assert!(near(hi[0], 100.0), "handed nothing it is Revit's: {hi:?}");
    // The parent hands it another: Revit regenerates it, so its definition draws it, 900 tall and 120 across.
    let (_, hi) = extent(&holder(json!({"Height": 900})), "Standard", &others);
    assert!((hi[2] - 900.0).abs() < 1.0 && near(hi[0], 120.0), "its definition at what the parent hands it: {hi:?}");
}

/// A leg whose own symbols are a line in its side view, (y, z) from (0, 0) up to (0, 500), and one in its plan.
fn leg() -> Family {
    family(json!({
        "category": "generic", "types": {"Standard": {}},
        "symbols": [
            {"line": [[0, 0], [0, 500]], "view": "side"},
            {"line": [[0, 0], [100, 50]]}
        ]
    }))
}

fn points(marks: &[Mark]) -> Vec<P2> {
    marks
        .iter()
        .flat_map(|m| match m {
            Mark::Line(p) => p.clone(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn a_nested_parts_side_symbols_draw_in_the_parents_side_view() {
    let leg = leg();
    let others = |n: &str| (n == "Sample Leg").then_some(&leg);
    let parent = |extra: Value| {
        let mut n = json!({"family": "Sample Leg", "type": "Standard", "at": [1000, 200, 0]});
        n.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        family(json!({"category": "furniture", "types": {"Standard": {}}, "nested": [n]}))
    };
    let marks = |f: &Family, view| {
        let v = f.values("Standard", &Default::default()).unwrap();
        points(&f.marks_with("Standard", &v, view, None, &others).unwrap())
    };
    let f = parent(json!({}));
    // Its side view is (y, z): the leg's line, up from where it stands (y 200).
    let side = marks(&f, SymbolView::Side);
    assert!(side.iter().any(|p| near(p.0, 200.0) && near(p.1, 0.0)) && side.iter().any(|p| near(p.0, 200.0) && near(p.1, 500.0)), "{side:?}");
    // Its plan has the leg's plan line and not its side one; its front has neither.
    let plan = marks(&f, SymbolView::Plan);
    assert!(plan.iter().any(|p| near(p.0, 1100.0) && near(p.1, 250.0)) && !plan.iter().any(|p| near(p.1, 700.0)), "{plan:?}");
    assert!(marks(&f, SymbolView::Front).is_empty(), "nothing of the leg faces the front");
    // Turned a quarter about z, the leg's side faces the parent's front, and its plan stays in plan.
    let turned = parent(json!({"rotation": 90}));
    assert!(marks(&turned, SymbolView::Side).is_empty());
    let front = marks(&turned, SymbolView::Front);
    assert!(front.iter().any(|p| near(p.0, 1000.0) && near(p.1, 500.0)), "its side line faces the front: {front:?}");
    // Its Visibility Settings leave the side view out.
    let hidden = parent(json!({"show": {"side": false}}));
    assert!(marks(&hidden, SymbolView::Side).is_empty());
}

#[test]
fn a_nested_part_turned_onto_a_face_draws_its_plan_symbols_in_the_parents_front_view() {
    let leg = leg();
    let others = |n: &str| (n == "Sample Leg").then_some(&leg);
    // Its x along the parent's x, its y up, its z (the way its plan looks) toward the parent's -y: stood on a wall's face.
    let f = family(json!({
        "category": "furniture", "types": {"Standard": {}},
        "nested": [{"family": "Sample Leg", "type": "Standard", "at": [1000, 0, 300], "axes": [[1, 0, 0], [0, 0, 1], [0, -1, 0]]}]
    }));
    let v = f.values("Standard", &Default::default()).unwrap();
    let front = points(&f.marks_with("Standard", &v, SymbolView::Front, None, &others).unwrap());
    // The leg's plan line, (0, 0) to (100, 50), in the parent's (x, z): from (1000, 300) to (1100, 350).
    assert!(front.iter().any(|p| near(p.0, 1000.0) && near(p.1, 300.0)) && front.iter().any(|p| near(p.0, 1100.0) && near(p.1, 350.0)), "{front:?}");
    // Nothing of it looks down, so the parent's plan has none of its plan symbols.
    assert!(points(&f.marks_with("Standard", &v, SymbolView::Plan, None, &others).unwrap()).is_empty());
}


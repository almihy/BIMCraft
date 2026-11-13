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

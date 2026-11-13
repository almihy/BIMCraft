//! A Revit RPC planting (a tree): its 3D is a render placeholder no rules can rebuild, so its check compares only its plan
//! and it always draws Revit's own 3D; its plan symbol is its base family nested in it, scaled by the tree's Height over the
//! base's own. All original content, made here (never Autodesk's).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::check::RPC_3D;
use crate::model::FORMAT;

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A box from `lo` to `hi` as twelve triangles, as a snapshot mesh holds it.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Value {
    let p =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    json!(quads.iter().flat_map(|[a, b, c, d]| [json!([p(*a), p(*b), p(*c)]), json!([p(*a), p(*c), p(*d)])]).collect::<Vec<_>>())
}

/// A circle of radius `r` as Revit draws one in plan, in the types `types`.
fn circle(r: f64, types: &[&str]) -> Vec<Value> {
    [(0, 180), (180, 360)]
        .iter()
        .map(|(a, b)| json!({"snapshot": true, "arc": {"centre": [0, 0], "radius": r, "from": a, "to": b}, "types": types}))
        .collect()
}

/// An export package of a two-type tree: Revit's box for its 3D, a circle in plan scaled by Height / 6096 (snapped to 6096 within a foot; the base's top
/// plane), the base nested in it; `rpc` whether the package says it is an RPC planting.
fn package(rpc: bool) -> Value {
    let base = json!({
        "category": "generic", "types": {"Base": {}},
        "references": [{"name": "Ground", "axis": "z", "at": 0, "origin": true}, {"name": "Top", "axis": "z", "at": 6096}],
        "symbols": [{"arc": {"centre": [0, 0], "radius": 1500, "from": 0, "to": 180}},
                    {"arc": {"centre": [0, 0], "radius": 1500, "from": 180, "to": 360}}]
    });
    let mut symbols = circle(750.0, &["Short"]);
    symbols.extend(circle(1500.0, &["Tall"]));
    let tree = json!({
        "category": "generic", "source": {"app": "Revit 2025", "category": "Planting"},
        "parameters": [{"name": "Height", "kind": "length", "value": 3048}],
        "types": {"Short": {"Height": 3048}, "Tall": {"Height": 6096}},
        "forms": [{"snapshot": true, "mesh": cuboid([-200.0, -200.0, 0.0], [200.0, 200.0, 3000.0]),
                   "show": {"plan": false, "plan_top": false, "plan_cut": false}}],
        "symbols": symbols
    });
    let definition = json!({"to_bimcraft": {}, "nested": [{"id": "1", "family": "Base", "type": "Base", "at": [0, 0, 0], "rotation": 0,
                                                         "mirrored": false, "params": {}}]});
    let mut revit = json!({"version": "2025", "category": "Planting"});
    if rpc {
        revit["rpc"] = json!(true);
    }
    json!({"format": 2, "family": tree, "definition": definition, "nested_families": {"Base": {"family": base}}, "revit": revit})
}

fn load(s: &mut Session, rpc: bool) -> crate::family::Family {
    s.files.give("Sample Tree.arqiflow-family.json", package(rpc).to_string().into_bytes());
    run(s, "family", json!({"path": "Sample Tree.arqiflow-family.json"}));
    s.model.families["Sample Tree"].clone()
}

#[test]
fn a_package_that_says_rpc_is_checked_in_plan_only_and_counts_as_editable() {
    let mut s = Session::new();
    let f = load(&mut s, true);
    assert!(f.rpc);
    // The base is scaled by the tree's Height over the base's top plane, from the base the tree holds.
    assert_eq!(f.nested[0].scale, Some(crate::family::Expr::Formula("if(abs(Height - 6096) <= 304.8, 6096, Height) / 6096".into())));
    for t in ["Short", "Tall"] {
        assert!(f.check[t].matched, "{t}: {:?}", f.check[t].reason);
        assert_eq!(f.check[t].reason.as_deref(), Some(RPC_3D));
        // The 3D is Revit's whatever the plan was drawn from.
        assert!(f.draws_snapshot_forms(t));
    }
    assert!(f.editable.as_ref().is_some_and(|e| e.ok), "{:?}", f.editable);
    // Its 3D is Revit's box.
    let values = f.values("Tall", &Default::default()).unwrap();
    let others = |n: &str| s.model.families.get(n);
    let pieces = f.pieces_with("Tall", &values, &others).unwrap();
    assert!(pieces.len() == 1 && pieces[0].top > 2999.0, "{}", pieces.len());
}

#[test]
fn a_package_that_does_not_say_rpc_is_checked_in_3d_too_and_fails() {
    let mut s = Session::new();
    let f = load(&mut s, false);
    assert!(!f.rpc);
    assert!(f.check.values().all(|c| !c.matched), "{:?}", f.check);
    assert!(f.editable.as_ref().is_some_and(|e| !e.ok));
}

#[test]
fn an_rpc_family_is_written_and_read_back_as_it_was() {
    let mut s = Session::new();
    load(&mut s, true);
    let json = s.model.to_json().unwrap();
    let v: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["format"], json!(FORMAT));
    assert_eq!(v["families"]["Sample Tree"]["rpc"], json!(true));
    let (m, left_out) = crate::model::Model::read(&json).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.families["Sample Tree"], s.model.families["Sample Tree"]);
}

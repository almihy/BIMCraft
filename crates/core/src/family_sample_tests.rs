//! Families from Revit checked as the exporter brings them, beyond the small cases of `family_parametric_tests`: asymmetric
//! families (a handle on one side, a desk with a return), a door in walls of several thicknesses, a rebuilt door flipped
//! with its nested handle, each pinned to whether it matches Revit's result. All original content, made here, never
//! Autodesk's: the real exported samples (Autodesk's own families) may not be kept in the repository (CLAUDE.md,
//! "Clean-room"), so their match is checked only where a folder of them is given (`BIMCRAFT_FAMILY_SAMPLES`), against the
//! record of what each gives (`testdata/families/sample-matches.json`).

use serde_json::{Value, json};

use crate::engine::Session;

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

fn line(a: Value, b: Value) -> Value {
    json!({"line": [a, b]})
}

/// A rectangle loop in plan between four bound numbers.
fn rect(x0: Value, x1: Value, y0: Value, y1: Value) -> Value {
    let p = |x: &Value, y: &Value| json!([x, y, 0]);
    json!([line(p(&x0, &y0), p(&x1, &y0)), line(p(&x1, &y0), p(&x1, &y1)), line(p(&x1, &y1), p(&x0, &y1)), line(p(&x0, &y1), p(&x0, &y0))])
}

fn r(id: &str, offset: f64) -> Value {
    json!({"ref": id, "offset": offset})
}

/// A flush door, original: Width between Left and Right, a lining as deep as its wall (bound to the wall's faces), a leaf
/// 40 mm thick, and a handle, a nested family it holds, 60 mm in from Right on the interior side, 1000 up. Its types are
/// `widths`, each exported in the wall `walls` says. Revit's result is given per type with its handle at `handle_x` (+1: by
/// Right, as the definition puts it; -1: by Left).
fn door(widths: &[f64], walls: &[f64], handle_x: f64) -> Value {
    let types: serde_json::Map<String, Value> = widths.iter().map(|w| (format!("{w}"), json!({"Width": w}))).collect();
    let per = |f: &dyn Fn(f64, f64) -> f64| -> Value {
        json!(widths.iter().zip(walls).map(|(w, t)| (format!("{w}"), json!(f(*w, *t)))).collect::<serde_json::Map<String, Value>>())
    };
    let mut forms = Vec::new();
    for (w, t) in widths.iter().zip(walls) {
        let (h, d) = (w / 2.0, t / 2.0);
        let ty = format!("{w}");
        forms.push(json!({"mesh": cuboid([-h - 50.0, -d, 0.0], [-h, d, 2100.0]), "types": [ty], "snapshot": true}));
        forms.push(json!({"mesh": cuboid([h, -d, 0.0], [h + 50.0, d, 2100.0]), "types": [ty], "snapshot": true}));
        forms.push(json!({"mesh": cuboid([-h, -40.0, 0.0], [h, 0.0, 2100.0]), "types": [ty], "snapshot": true}));
        let x = handle_x * (h - 60.0);
        forms.push(json!({"mesh": cuboid([x - 60.0, -100.0, 990.0], [x + 60.0, -40.0, 1010.0]), "types": [ty], "snapshot": true}));
    }
    let wall: serde_json::Map<String, Value> = widths.iter().zip(walls).map(|(w, t)| (format!("{w}"), json!(t))).collect();
    let handle = json!({"category": "generic", "types": {"Lever": {}},
                        "forms": [{"profile": [[-60, -60], [60, -60], [60, 0], [0 - 60, 0]], "from": -10, "to": 10}]});
    json!({
        "format": 2,
        "family": {"category": "door", "host": "wall",
                   "parameters": [{"name": "Width", "kind": "length", "value": widths[0]}, {"name": "Height", "kind": "length", "value": 2100}],
                   "types": types, "forms": forms},
        "definition": {
            "to_bimcraft": {"x_offset": 0, "y_sign": 1, "z_minus": 0, "wall": wall},
            "reference_planes": [
                {"id": "c", "name": "Center", "normal": [1, 0, 0], "defines_origin": true, "at": per(&|_, _| 0.0)},
                {"id": "l", "name": "Left", "normal": [1, 0, 0], "at": per(&|w, _| -w / 2.0)},
                {"id": "r", "name": "Right", "normal": [1, 0, 0], "at": per(&|w, _| w / 2.0)},
                {"id": "e", "name": "Exterior", "normal": [0, 1, 0], "is_reference": "host_exterior", "at": per(&|_, t| t / 2.0)},
                {"id": "i", "name": "Interior", "normal": [0, 1, 0], "is_reference": "host_interior", "at": per(&|_, t| -t / 2.0)},
                {"id": "z", "name": "Ref Level", "normal": [0, 0, 1], "defines_origin": true, "at": per(&|_, _| 0.0)}
            ],
            "dimensions": [{"refs": ["l", "r"], "label": "Width"}, {"refs": ["l", "c", "r"], "eq": true}],
            "forms": [
                {"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 2100, "loops": [rect(r("l", -50.0), r("l", 0.0), r("i", 0.0), r("e", 0.0))]},
                {"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 2100, "loops": [rect(r("r", 0.0), r("r", 50.0), r("i", 0.0), r("e", 0.0))]},
                {"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 2100, "loops": [rect(r("l", 0.0), r("r", 0.0), json!(-40), json!(0))]}
            ],
            "nested": [{"id": "h", "family": "Sample Handle", "type": "Lever", "shared": false, "at": [r("r", -60.0), -40, 1000]}]
        },
        "nested_families": {"Sample Handle": {"family": handle}}
    })
}

fn load(s: &mut Session, name: &str, package: &Value) -> Value {
    let file = format!("{name}.arqiflow-family.json");
    s.files.give(&file, package.to_string().into_bytes());
    run(s, "family", json!({"path": file}))
}

/// Matched in every wall it was exported in (100, 200 and 300 mm), its handle where Revit put it; Revit's handle by the other
/// jamb fails though everything else agrees.
#[test]
fn an_asymmetric_door_in_walls_of_several_thicknesses_matches_and_a_mirrored_handle_fails() {
    let mut s = Session::new();
    let r = load(&mut s, "Sample Lever Door", &door(&[800.0, 900.0, 1000.0], &[100.0, 200.0, 300.0], 1.0));
    for (t, wall) in [("800", 100.0), ("900", 200.0), ("1000", 300.0)] {
        assert_eq!((r["check"][t]["matched"].clone(), r["check"][t]["wall"].clone()), (json!(true), json!(wall)), "{t}: {}", r["check"]);
    }
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    let mut s = Session::new();
    let r = load(&mut s, "Sample Lever Door", &door(&[900.0], &[200.0], -1.0));
    assert_eq!(r["check"]["900"]["matched"], false, "{}", r["check"]);
}

/// A rebuilt door flipped: its nested handle goes to the other side with it, as Revit's flip does.
#[test]
fn a_rebuilt_door_flipped_takes_its_nested_handle_with_it() {
    let mut s = Session::new();
    load(&mut s, "Sample Lever Door", &door(&[900.0], &[200.0], 1.0));
    assert!(!s.model.families["Sample Lever Door"].draws_snapshot("900"));
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let d = run(&mut s, "door", json!({"at": [2000, 0], "family": "Sample Lever Door", "type": "900"}))["id"].as_u64().unwrap();
    // The handle: the piece that stands 1000 up and no higher than 1010.
    let handle_x = |s: &Session| {
        let (pieces, _) = s.model.opening_local(&s.model.elements[&d]).unwrap();
        let p = pieces.iter().find(|p| p.tris.iter().flatten().all(|q| q[2] >= 989.0 && q[2] <= 1011.0)).expect("the handle");
        let xs: Vec<f64> = p.tris.iter().flatten().map(|q| q[0]).collect();
        (xs.iter().copied().fold(f64::MAX, f64::min) + xs.iter().copied().fold(f64::MIN, f64::max)) / 2.0
    };
    let before = handle_x(&s);
    assert!((before - 390.0).abs() < 0.5, "{before}");
    run(&mut s, "modify", json!({"id": d, "flip_hand": true}));
    let after = handle_x(&s);
    assert!((after + before).abs() < 0.5, "{before} -> {after}");
}

/// A desk with a return on one side (an L in plan): the same L matches, the L mirrored fails.
#[test]
fn an_l_shaped_desk_matches_only_the_right_way_round() {
    let desk = |leg: f64| {
        json!({"category": "furniture", "types": {"A": {}}, "parameters": [],
               "forms": [{"mesh": cuboid([-800.0, 0.0, 700.0], [800.0, 700.0, 730.0]), "snapshot": true},
                         {"mesh": cuboid(if leg > 0.0 { [300.0, 700.0, 700.0] } else { [-800.0, 700.0, 700.0] },
                                         if leg > 0.0 { [800.0, 1500.0, 730.0] } else { [-300.0, 1500.0, 730.0] }), "snapshot": true}]})
    };
    let definition = json!({"forms": [
        {"kind": "extrusion", "normal": [0, 0, 1], "start": 700, "end": 730, "loops": [rect(json!(-800), json!(800), json!(0), json!(700))]},
        {"kind": "extrusion", "normal": [0, 0, 1], "start": 700, "end": 730, "loops": [rect(json!(300), json!(800), json!(700), json!(1500))]}
    ]});
    for (leg, matched) in [(1.0, true), (-1.0, false)] {
        let mut s = Session::new();
        let r = load(&mut s, "Sample Return Desk", &json!({"format": 2, "family": desk(leg), "definition": definition}));
        assert_eq!(r["check"]["A"]["matched"], matched, "{}", r["check"]);
    }
}

/// The real exported samples, where a folder of them is given (`BIMCRAFT_FAMILY_SAMPLES`; they are Autodesk's and are never
/// kept here): each family's types match Revit's result as recorded, so a change that makes one match or fail says so.
#[test]
fn the_exported_samples_match_as_recorded() {
    let Ok(dir) = std::env::var("BIMCRAFT_FAMILY_SAMPLES") else { return };
    let expected: Value = serde_json::from_str(include_str!("../testdata/families/sample-matches.json")).unwrap();
    let mut differ = Vec::new();
    let mut matched_families = 0;
    for (file, want) in expected["families"].as_object().unwrap() {
        let path = std::path::Path::new(&dir).join(file);
        let mut s = Session::new();
        let got = match s.execute("family", &json!({"path": path.to_string_lossy()})) {
            Ok(r) => {
                let check = r["check"].as_object().cloned().unwrap_or_default();
                let n = check.values().filter(|c| c["matched"] == true).count();
                matched_families += usize::from(r["editable"]["ok"] == true);
                json!({"editable": r["editable"]["ok"] == true, "matched": n, "types": check.len()})
            }
            Err(e) => json!({"error": e.lines().next().unwrap_or_default()}),
        };
        let mine = json!({"editable": want["editable"], "matched": want["matched"], "types": want["types"]});
        if got != mine {
            differ.push(format!("{file}: recorded {mine}, now {got}"));
        }
    }
    assert!(differ.is_empty(), "{}", differ.join("\n"));
    assert_eq!(json!(matched_families), expected["editable"], "families that match Revit's result in every type");
}

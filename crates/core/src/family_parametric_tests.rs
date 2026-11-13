//! Families from Revit that stay parametric (round 2): Revit's definition (original, synthetic ones written here: never
//! Autodesk content) translated into references, dimensions and forms, the references placed per type by labelled, locked
//! and EQ dimensions, flexed and duplicated; blends, revolves, sweeps and swept blends; nested parts; Revit's own result
//! kept as the check and the fallback; and the package shape of a file, with the type to place.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family::Family;
use crate::model::{Element, Id};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A box from `lo` to `hi` as twelve triangles turned out, as a snapshot mesh holds it.
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

fn line(a: Value, b: Value) -> Value {
    json!({"line": [a, b]})
}

/// A rectangle loop in plan between four bound numbers.
fn rect(x0: Value, x1: Value, y0: Value, y1: Value) -> Value {
    let p = |x: &Value, y: &Value| json!([x, y, 0]);
    json!([line(p(&x0, &y0), p(&x1, &y0)), line(p(&x1, &y0), p(&x1, &y1)), line(p(&x1, &y1), p(&x0, &y1)), line(p(&x0, &y1), p(&x0, &y0))])
}

fn r(id: &str) -> Value {
    json!({"ref": id})
}

/// Planes of a box: a centre that defines the origin and two sides placed by a labelled dimension and EQ, on x and y; a
/// level and a top on z.
fn box_planes(widths: [f64; 2], depths: [f64; 2], height: f64) -> Value {
    let at = |a: f64, b: f64| json!({"Small": a, "Large": b});
    json!([
        {"id": "1", "name": "Center Left Right", "normal": [1, 0, 0], "defines_origin": true, "is_reference": "center_lr", "at": at(0.0, 0.0)},
        {"id": "2", "name": "Left", "normal": [-1, 0, 0], "is_reference": "left", "at": at(-widths[0] / 2.0, -widths[1] / 2.0)},
        {"id": "3", "name": "Right", "normal": [1, 0, 0], "is_reference": "right", "at": at(widths[0] / 2.0, widths[1] / 2.0)},
        {"id": "4", "name": "Center Front Back", "normal": [0, 1, 0], "defines_origin": true, "at": at(0.0, 0.0)},
        {"id": "5", "name": "Front", "normal": [0, -1, 0], "at": at(-depths[0] / 2.0, -depths[1] / 2.0)},
        {"id": "6", "name": "Back", "normal": [0, 1, 0], "at": at(depths[0] / 2.0, depths[1] / 2.0)},
        {"id": "7", "name": "Ref Level", "normal": [0, 0, 1], "defines_origin": true, "at": at(0.0, 0.0)},
        {"id": "8", "name": "Top", "normal": [0, 0, 1], "at": at(height, height)}
    ])
}

fn box_dimensions() -> Value {
    json!([
        {"id": "d1", "kind": "linear", "refs": ["2", "3"], "label": "Width", "value": {"Small": 1000, "Large": 1400}},
        {"id": "d2", "kind": "linear", "refs": ["2", "1", "3"], "eq": true},
        {"id": "d3", "kind": "linear", "refs": ["5", "6"], "label": "Depth"},
        {"id": "d4", "kind": "linear", "refs": ["5", "4", "6"], "eq": true},
        {"id": "d5", "kind": "linear", "refs": ["7", "8"], "label": "Height"}
    ])
}

/// A two-type box whose Revit result for `Large` is `large_top` mm tall (750 is right), with its definition.
fn table(large_top: f64) -> Value {
    json!({
        "name": "Sample Box Table",
        "category": "furniture",
        "parameters": [
            {"name": "Width", "kind": "length", "value": 1000, "group": "Dimensions"},
            {"name": "Depth", "kind": "length", "value": 600, "group": "Dimensions"},
            {"name": "Height", "kind": "length", "value": 750, "group": "Dimensions"}
        ],
        "types": {"Small": {}, "Large": {"Width": 1400, "Depth": 800}},
        "forms": [
            {"mesh": cuboid([-500.0, -300.0, 0.0], [500.0, 300.0, 750.0]), "types": ["Small"], "snapshot": true},
            {"mesh": cuboid([-700.0, -400.0, 0.0], [700.0, 400.0, large_top]), "types": ["Large"], "snapshot": true}
        ],
        "revit": {
            "to_bimcraft": {"x_offset": 0, "y_sign": 1, "z_minus": 0},
            "reference_planes": box_planes([1000.0, 1400.0], [600.0, 800.0], 750.0),
            "dimensions": box_dimensions(),
            "forms": [{"id": "f1", "kind": "extrusion", "void": false, "normal": [0, 0, 1], "start": r("7"), "end": r("8"),
                       "loops": [rect(r("2"), r("3"), r("5"), r("6"))]}]
        }
    })
}

/// The box `f`'s type `t` builds to: its least and most corner over every form.
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

fn volume(f: &Family, t: &str) -> f64 {
    let v = f.values(t, &Default::default()).unwrap();
    let pieces = f.pieces(t, &v).unwrap();
    pieces.iter().flat_map(|p| p.tris.iter()).map(|[a, b, c]| crate::geom::v3::dot(*a, crate::geom::v3::cross(*b, *c))).sum::<f64>() / 6.0
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn a_box_from_revit_matches_revits_result_is_editable_and_flexes_with_its_parameters() {
    let mut s = Session::new();
    let r = run(&mut s, "family", table(750.0));
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    assert_eq!(r["check"]["Small"]["matched"], true);
    assert_eq!(r["check"]["Large"]["matched"], true);
    let f = s.model.families["Sample Box Table"].clone();
    assert_eq!(f.references.len(), 8);
    assert_eq!(f.dimensions.len(), 5);
    assert!(!f.draws_snapshot("Large"), "a matched type draws its definition");
    let none = |_: &str| None;
    let (lo, hi) = extent(&f, "Large", &none);
    assert!(near(lo[0], -700.0) && near(hi[0], 700.0) && near(hi[1], 400.0) && near(hi[2], 750.0), "{lo:?} {hi:?}");
    // The references solved: Left half the width from the centre, as the EQ says.
    let v = f.values("Large", &Default::default()).unwrap();
    assert_eq!(v["ref(Left)"], crate::family::Val::Number(-700.0));
    // Flexed: the type's Width changed, its box follows, and it stays parametric (Revit's result no longer says).
    let r = run(&mut s, "family", json!({"name": "Sample Box Table", "types": {"Small": {"Width": 1600, "Height": 900}}}));
    assert_eq!(r["check"]["Small"]["flexed"], true, "{}", r["check"]);
    let f = s.model.families["Sample Box Table"].clone();
    let (lo, hi) = extent(&f, "Small", &none);
    assert!(near(lo[0], -800.0) && near(hi[0], 800.0) && near(hi[2], 900.0), "{lo:?} {hi:?}");
    // Duplicated (Type Properties' Duplicate) and flexed: a parametric type of its own.
    let r = run(
        &mut s,
        "family",
        json!({"name": "Sample Box Table", "types": {"Huge": {"Width": 2000, "Depth": 800}}, "like": {"Huge": "Large"}}),
    );
    assert_eq!(r["check"]["Huge"]["matched"], true, "{}", r["check"]);
    let f = s.model.families["Sample Box Table"].clone();
    let (lo, hi) = extent(&f, "Huge", &none);
    assert!(near(lo[0], -1000.0) && near(hi[0], 1000.0), "{lo:?} {hi:?}");
    // Placed, the component is the parametric box.
    let c = run(&mut s, "component", json!({"family": "Sample Box Table", "type": "Huge", "at": [5000, 5000]}));
    let id: Id = c["id"].as_u64().unwrap();
    let Some(Element::Component(comp)) = s.model.elements.get(id) else { panic!("no component") };
    let pieces = s.model.component_local_pieces(comp);
    let xs: Vec<f64> = pieces.iter().flat_map(|p| p.tris.iter().flatten()).map(|p| p[0]).collect();
    assert!(near(xs.iter().copied().fold(f64::MIN, f64::max), 1000.0));
}

#[test]
fn a_type_that_does_not_match_draws_revits_result_and_says_why() {
    let mut s = Session::new();
    let r = run(&mut s, "family", table(900.0));
    assert_eq!(r["check"]["Small"]["matched"], true);
    assert_eq!(r["check"]["Large"]["matched"], false, "{}", r["check"]);
    let why = r["check"]["Large"]["reason"].as_str().unwrap();
    assert!(why.contains("volume") || why.contains("box"), "{why}");
    assert_eq!(r["editable"]["ok"], false);
    assert!(r["editable"]["reason"].as_str().unwrap().contains("Large"), "{}", r["editable"]);
    let f = s.model.families["Sample Box Table"].clone();
    assert!(f.draws_snapshot("Large") && !f.draws_snapshot("Small"));
    let none = |_: &str| None;
    assert!(near(extent(&f, "Large", &none).1[2], 900.0), "Revit's result, as it came");
    assert!(near(extent(&f, "Small", &none).1[2], 750.0));
    // A duplicate of it is Revit's result too.
    let r = run(
        &mut s,
        "family",
        json!({"name": "Sample Box Table", "types": {"Copy": {"Width": 1400, "Depth": 800}}, "like": {"Copy": "Large"}}),
    );
    assert_eq!(r["check"]["Copy"]["matched"], false);
    // The list says it.
    let list = run(&mut s, "family", json!({}));
    let mine = list["families"].as_array().unwrap().iter().find(|f| f["name"] == "Sample Box Table").unwrap().clone();
    assert_eq!(mine["editable"]["ok"], false);
}

#[test]
fn an_over_constrained_family_is_not_editable_and_draws_revits_result() {
    let mut s = Session::new();
    let mut fam = table(750.0);
    // Left to Right also labelled Depth: it can't be both.
    fam["revit"]["dimensions"].as_array_mut().unwrap().push(json!({"refs": ["2", "3"], "label": "Depth"}));
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], false);
    assert!(r["check"]["Small"]["reason"].as_str().unwrap().contains("over-constrained"), "{}", r["check"]);
    let f = s.model.families["Sample Box Table"].clone();
    assert!(f.draws_snapshot("Small"));
}

/// A door: frame and leaf bound to Left and Right, its Top to Height, mirrored across its wall (`y_sign` -1), a plan swing.
fn door() -> Value {
    let at = |a: f64, b: f64| json!({"900": a, "1000": b});
    let planes = json!([
        {"id": "1", "name": "Center Left Right", "normal": [1, 0, 0], "defines_origin": true, "at": at(0.0, 0.0)},
        {"id": "2", "name": "Left", "normal": [-1, 0, 0], "at": at(-450.0, -500.0)},
        {"id": "3", "name": "Right", "normal": [1, 0, 0], "at": at(450.0, 500.0)},
        {"id": "7", "name": "Ref Level", "normal": [0, 0, 1], "defines_origin": true, "at": at(0.0, 0.0)},
        {"id": "8", "name": "Top", "normal": [0, 0, 1], "at": at(2100.0, 2100.0)}
    ]);
    let off = |id: &str, o: f64| json!({"ref": id, "offset": o});
    let jamb = |x0: Value, x1: Value| {
        json!({"kind": "extrusion", "normal": [0, 0, 1], "start": r("7"), "end": off("8", 50.0), "subcategory": "Frame",
               "loops": [rect(x0, x1, json!(-50), json!(50))]})
    };
    let snapshot = |w: f64| {
        let h = w / 2.0;
        json!([
            {"mesh": cuboid([-h - 50.0, -50.0, 0.0], [-h, 50.0, 2150.0]), "types": [format!("{w}")], "snapshot": true},
            {"mesh": cuboid([h, -50.0, 0.0], [h + 50.0, 50.0, 2150.0]), "types": [format!("{w}")], "snapshot": true},
            {"mesh": cuboid([-h, -40.0, 0.0], [h, 0.0, 2100.0]), "types": [format!("{w}")], "snapshot": true}
        ])
    };
    let swing = |w: f64| {
        let h = w / 2.0;
        json!([
            {"line": [[h, 0], [h, w]], "types": [format!("{w}")], "snapshot": true, "show": {}},
            {"arc": {"centre": [h, 0], "radius": w, "from": 90, "to": 180}, "types": [format!("{w}")], "snapshot": true, "show": {}}
        ])
    };
    let mut forms = snapshot(900.0).as_array().unwrap().clone();
    forms.extend(snapshot(1000.0).as_array().unwrap().clone());
    let mut symbols = swing(900.0).as_array().unwrap().clone();
    symbols.extend(swing(1000.0).as_array().unwrap().clone());
    json!({
        "name": "Sample Flexing Door",
        "category": "door",
        "host": "wall",
        "subcategories": [{"name": "Frame"}],
        "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 2100}],
        "types": {"900": {}, "1000": {"Width": 1000}},
        "cut": {"width": "Width + 100", "height": "Height + 50"},
        "forms": forms,
        "symbols": symbols,
        "revit": {
            "to_bimcraft": {"x_offset": 0, "y_sign": -1, "z_minus": 0},
            "reference_planes": planes,
            "dimensions": [
                {"refs": ["2", "3"], "label": "Width"},
                {"refs": ["2", "1", "3"], "eq": true},
                {"refs": ["7", "8"], "label": "Height"}
            ],
            "forms": [
                jamb(off("2", -50.0), r("2")),
                jamb(r("3"), off("3", 50.0)),
                // The leaf, 40 mm on the document's +y: on the family's -y, mirrored.
                {"kind": "extrusion", "normal": [0, 0, -1], "start": r("8"), "end": r("7"), "loops": [rect(r("2"), r("3"), json!(0), json!(40))]}
            ],
            "lines": [{"view": "plan", "show": {"coarse": true, "medium": true, "fine": true, "plan": true, "front": true, "side": true},
                       "curves": [
                           line(json!([r("3"), 0, 0]), json!([r("3"), {"param": "Width", "scale": -1}, 0])),
                           {"arc": {"center": [r("3"), 0, 0], "radius": {"param": "Width"}, "start": [r("2"), 0, 0],
                                    "end": [r("3"), {"param": "Width", "scale": -1}, 0], "normal": [0, 0, 1]}}
                       ]}]
        }
    })
}

#[test]
fn a_door_from_revit_flexes_its_frame_leaf_and_swing_as_in_revit() {
    let mut s = Session::new();
    let r = run(&mut s, "family", door());
    assert_eq!(r["editable"]["ok"], true, "{} {}", r["editable"], r["check"]);
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let d: Id = run(&mut s, "door", json!({"at": [2000, 0], "family": "Sample Flexing Door", "type": "900"}))["id"].as_u64().unwrap();
    let (pieces, marks) = s.model.opening_local(&s.model.elements[&d]).unwrap();
    assert_eq!(pieces.len(), 3, "the frame's two jambs and the leaf");
    assert_eq!(marks.len(), 2, "the swing's line and arc");
    // Flexed wider: the frame and the swing follow.
    run(&mut s, "family", json!({"name": "Sample Flexing Door", "types": {"900": {"Width": 1200}}}));
    let (pieces, marks) = s.model.opening_local(&s.model.elements[&d]).unwrap();
    let xs: Vec<f64> = pieces.iter().flat_map(|p| p.tris.iter().flatten()).map(|p| p[0]).collect();
    let most = xs.iter().copied().fold(f64::MIN, f64::max);
    assert!(near(most.abs(), 650.0), "{most}");
    let reach = marks
        .iter()
        .filter_map(|m| match m {
            crate::family::Mark::Line(pts) => Some(pts.iter().map(|p| p.1.abs()).fold(0.0, f64::max)),
            _ => None,
        })
        .fold(0.0, f64::max);
    assert!(near(reach, 1200.0), "{reach}");
}

#[test]
fn revolves_blends_sweeps_and_swept_blends_regenerate() {
    let mut s = Session::new();
    let p = |x: f64, y: f64, z: f64| json!([x, y, z]);
    let lp = |pts: &[Value]| -> Value {
        json!((0..pts.len()).map(|i| line(pts[i].clone(), pts[(i + 1) % pts.len()].clone())).collect::<Vec<_>>())
    };
    let plan_square = |h: f64, z: f64| lp(&[p(-h, -h, z), p(h, -h, z), p(h, h, z), p(-h, h, z)]);
    let fam = json!({
        "name": "Shapes", "category": "generic",
        "parameters": [{"name": "Width", "kind": "length", "value": 400}],
        "types": {"A": {}},
        "revit": {
            "forms": [
                {"kind": "revolution", "loops": [lp(&[p(100.0, 0.0, 0.0), p(200.0, 0.0, 0.0), p(200.0, 0.0, 300.0), p(100.0, 0.0, 300.0)])],
                 "axis": [p(0.0, 0.0, 0.0), p(0.0, 0.0, 1000.0)], "start_angle": 0, "end_angle": 360},
                {"kind": "blend", "normal": [0, 0, 1], "start": 1000, "end": 1300, "bottom": [plan_square(200.0, 1000.0)], "top": [plan_square(100.0, 1300.0)]},
                {"kind": "sweep", "path": [line(p(0.0, 0.0, 2000.0), p(1000.0, 0.0, 2000.0)), line(p(1000.0, 0.0, 2000.0), p(1000.0, 1000.0, 2000.0))],
                 "profile": [lp(&[p(0.0, -25.0, 1975.0), p(0.0, 25.0, 1975.0), p(0.0, 25.0, 2025.0), p(0.0, -25.0, 2025.0)])]},
                {"kind": "swept_blend", "path": [line(p(0.0, 0.0, 0.0), p(0.0, 0.0, 1000.0))],
                 "profile": [lp(&[p(-25.0, -25.0, 0.0), p(25.0, -25.0, 0.0), p(25.0, 25.0, 0.0), p(-25.0, 25.0, 0.0)])],
                 "profile_end": [lp(&[p(-50.0, -50.0, 1000.0), p(50.0, -50.0, 1000.0), p(50.0, 50.0, 1000.0), p(-50.0, 50.0, 1000.0)])]},
                {"kind": "sweep", "path": [{"arc": {"center": p(5000.0, 0.0, 0.0), "radius": 500, "start": p(5500.0, 0.0, 0.0), "end": p(5000.0, 500.0, 0.0), "normal": [0, 0, 1]}}],
                 "profile": [lp(&[p(5475.0, 0.0, -25.0), p(5525.0, 0.0, -25.0), p(5525.0, 0.0, 25.0), p(5475.0, 0.0, 25.0)])]}
            ]
        }
    });
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    let f = s.model.families["Shapes"].clone();
    assert_eq!(f.forms.len(), 5);
    let alone = |i: usize| Family { forms: vec![f.forms[i].clone()], ..f.clone() };
    let pi = std::f64::consts::PI;
    let within = |got: f64, want: f64, share: f64| assert!((got - want).abs() <= want * share, "{got} against {want}");
    within(volume(&alone(0), "A"), pi * (200.0f64.powi(2) - 100.0f64.powi(2)) * 300.0, 0.01);
    within(volume(&alone(1), "A"), 100.0 * (160000.0 + 40000.0 + 80000.0), 0.001);
    within(volume(&alone(2), "A"), 2.0 * 1000.0 * 2500.0, 0.001);
    within(volume(&alone(3), "A"), 1000.0 / 3.0 * (2500.0 + 10000.0 + 5000.0), 0.001);
    // A quarter turn of a true arc, radius 500: Pappus.
    within(volume(&alone(4), "A"), 2500.0 * pi / 2.0 * 500.0, 0.01);
    assert!(f.forms[4].arcs.iter().any(|b| b.ring == crate::family::Ring::Path), "the path keeps its arc");
}

#[test]
fn a_package_loads_its_nested_families_and_gives_back_the_type_to_place() {
    let mut s = Session::new();
    let bracket = json!({"category": "generic", "parameters": [{"name": "Depth", "kind": "length", "value": 200}], "types": {"Standard": {}},
                         "forms": [{"profile": [[-10, 0], [10, 0], [10, "Depth"], [-10, "Depth"]], "from": 0, "to": 100}]});
    let mut shelf = table(750.0);
    shelf.as_object_mut().unwrap().remove("name");
    let definition = shelf.as_object_mut().unwrap().remove("revit").unwrap();
    let mut definition = definition;
    definition["nested"] = json!([{"id": "n1", "family": "Sample Bracket", "type": "Standard", "shared": false,
                                   "at": [{"ref": "3", "offset": -50}, {"ref": "5"}, 750], "rotation": 0, "mirrored": false,
                                   "params": {"Depth": {"param": "Depth"}}}]);
    let package = json!({"format": 2, "family": shelf, "definition": definition, "place": "Large",
                         "nested_families": {"Sample Bracket": {"family": bracket}}, "revit": {"version": "2025"}});
    s.files.give("Sample Shelf.arqiflow-family.json", package.to_string().into_bytes());
    let r = run(&mut s, "family", json!({"path": "Sample Shelf.arqiflow-family.json"}));
    assert_eq!(r["place"], "Large");
    assert_eq!(r["name"], "Sample Shelf", "named after its file, as any family file");
    assert!(s.model.families.contains_key("Sample Bracket"));
    let name = r["name"].as_str().unwrap().to_string();
    let f = s.model.families[&name].clone();
    assert_eq!(f.nested.len(), 1);
    // Its check sees the bracket, which Revit's result here hasn't: not a match, so Revit's result is drawn.
    let others = |n: &str| s.model.families.get(n);
    let (lo, hi) = extent(
        &Family { check: Default::default(), forms: f.forms.iter().filter(|x| !x.snapshot).cloned().collect(), ..f.clone() },
        "Large",
        &others,
    );
    assert!(near(hi[0], 700.0) && near(hi[2], 850.0) && near(lo[1], -400.0), "{lo:?} {hi:?}");
    // The bracket follows the shelf's width.
    let mut wider = Family { check: Default::default(), forms: f.forms.iter().filter(|x| !x.snapshot).cloned().collect(), ..f.clone() };
    wider.types.get_mut("Large").unwrap().insert("Width".into(), json!(2000));
    let (_, hi) = extent(&wider, "Large", &others);
    assert!(near(hi[0], 1000.0), "{hi:?}");
}

#[test]
fn a_project_with_a_parametric_family_is_written_as_22() {
    use crate::model::{FORMAT, Model};
    let mut s = Session::new();
    run(&mut s, "family", table(750.0));
    let v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(v["format"], json!(FORMAT));
    assert_eq!(FORMAT, 22);
    let (m, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.families["Sample Box Table"].references, s.model.families["Sample Box Table"].references);
    assert_eq!(m.families["Sample Box Table"].check, s.model.families["Sample Box Table"].check);
    // Saved to a file and read back, it is as it was.
    let saved = run(&mut s, "family", json!({"name": "Sample Box Table"}));
    let mut file = saved["family"].clone();
    file["name"] = json!("Again");
    run(&mut s, "family", file);
    assert_eq!(s.model.families["Again"].check, s.model.families["Sample Box Table"].check);
}

#[test]
fn hostile_definitions_never_panic() {
    let hostile = [
        json!(5),
        json!({"to_bimcraft": 5, "reference_planes": [{"id": 1, "name": "", "normal": [0, 0, 0], "at": {"A": 1e300}}]}),
        json!({"reference_planes": [{"id": "a", "name": "A", "normal": [1, 0, 0], "at": {"A": 0}}, {"id": "b", "name": "A", "normal": [1, 0, 0]}],
               "dimensions": [{"refs": ["a", "a"], "label": "Nope", "locked": true, "eq": true, "value": "x"}, {"refs": ["a", "b"], "locked": true, "value": {"A": 1e300}}]}),
        json!({"forms": [{"kind": "sweep", "path": [{"arc": {"center": [0, 0, 0], "radius": -1, "start": [0, 0, 0], "end": [0, 0, 0], "normal": [0, 0, 0]}}], "profile": [[]]},
                         {"kind": "extrusion", "normal": [0, 0, 1], "loops": [[{"line": [[0, 0, 0], [1e300, 0, 0]]}]], "start": {"ref": "x"}, "end": {"param": "Nope"}},
                         {"kind": "blend", "normal": [1, 1, 0], "bottom": [], "top": 5},
                         {"kind": "revolution", "loops": [[{"line": [[0, 0, 0], [0, 0, 0]]}]], "axis": [[0, 0, 0], [0, 0, 0]], "start_angle": 0, "end_angle": 1e9},
                         {"kind": "swept_blend", "path": [{"line": [[0, 0, 0], [0, 0, 0]]}], "profile": [[{"line": [[0, 0, 0], [1, 0, 0]]}]], "profile_end": []},
                         {"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 10, "loops": [[{"arc": {"center": [0, 0, 0], "radius": 5, "start": [5, 0, 0], "end": [5, 0, 0], "normal": [0, 0, 1]}}]]},
                         {"kind": "nope"}],
               "lines": [{"view": "plan", "curves": [{"arc": {"center": [0, 0, 0], "start": [0, 0, 0], "end": [0, 0, 0], "normal": [0, 0, 1]}}]}, {"view": 5}],
               "nested": [{"family": "Odd", "type": "A", "at": [0, 0, 0], "params": {"X": {"formula": "1/0"}}}, {"family": "Odd", "type": "A", "at": [1, 2, 3]}],
               "type_catalog": {"columns": 5}}),
    ];
    for def in hostile {
        let mut s = Session::new();
        let p = json!({"name": "Odd", "category": "generic", "types": {"A": {}}, "parameters": [{"name": "W", "kind": "length", "value": 10}], "revit": def});
        // Either it loads (what can't be translated said) or it's refused; it never panics, and the model stays sound.
        if s.execute("family", &p).is_ok() {
            let _ = s.execute("component", &json!({"family": "Odd", "type": "A", "at": [0, 0]}));
            let _ = crate::render::plan(&s.model, 100000, 400, 300);
        }
        assert!(s.model.check().is_ok());
    }
}

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
    assert!(why.contains("Revit's") && !why.contains("along"), "{why}");
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
    // Not shared: the shelf holds it, and the project's list doesn't show it.
    assert!(!s.model.families.contains_key("Sample Bracket"));
    let name = r["name"].as_str().unwrap().to_string();
    let f = s.model.families[&name].clone();
    assert_eq!(f.nested.len(), 1);
    assert!(f.embedded.contains_key("Sample Bracket"));
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

// Round 3: what the translator reads beside round 2's.

/// A closed loop through `pts`, as lines.
fn ring_of(pts: &[Value]) -> Value {
    json!((0..pts.len()).map(|i| line(pts[i].clone(), pts[(i + 1) % pts.len()].clone())).collect::<Vec<_>>())
}

#[test]
fn a_model_line_draws_in_each_view_its_visibility_settings_say() {
    let mut s = Session::new();
    let show = json!({"coarse": true, "medium": true, "fine": true, "plan": true, "front": false, "side": true});
    let fam = json!({
        "name": "Bay", "category": "generic", "types": {"A": {}},
        "revit": {"lines": [{"view": "model", "show": show, "curves": [line(json!([0, 0, 0]), json!([2400, 0, 0])), line(json!([0, 0, 0]), json!([0, 4800, 0]))]}]}
    });
    run(&mut s, "family", fam);
    let f = &s.model.families["Bay"];
    let views: Vec<crate::family::SymbolView> = f.symbols.iter().map(|y| y.view).collect();
    use crate::family::SymbolView::{Front, Plan, Side};
    assert_eq!(views.iter().filter(|v| **v == Plan).count(), 2, "{views:?}");
    assert_eq!(views.iter().filter(|v| **v == Side).count(), 2, "{views:?}");
    assert!(!views.contains(&Front), "its Front/Back is off");
}

/// A window whose sill plane stands under a fixed head by its height (Revit's Default Head Height less Height), which
/// no parameter says alone: its Sill Height is 0 in every type, as Revit's families of the kind have it.
fn head_hung_window() -> Value {
    let at = |a: f64, b: f64| json!({"Tall": a, "Short": b});
    let planes = json!([
        {"id": "1", "name": "Center Left Right", "normal": [1, 0, 0], "defines_origin": true, "at": at(0.0, 0.0)},
        {"id": "2", "name": "Left", "normal": [-1, 0, 0], "at": at(-400.0, -400.0)},
        {"id": "3", "name": "Right", "normal": [1, 0, 0], "at": at(400.0, 400.0)},
        {"id": "4", "name": "Sill", "normal": [0, 0, 1], "is_reference": "bottom", "at": at(900.0, 1200.0)},
        {"id": "5", "name": "Head", "normal": [0, 0, 1], "is_reference": "top", "at": at(2100.0, 2100.0)}
    ]);
    let snapshot = |t: &str, h: f64| json!({"mesh": cuboid([-400.0, -50.0, 0.0], [400.0, 50.0, h]), "types": [t], "snapshot": true});
    json!({
        "name": "Head Hung", "category": "window", "host": "wall",
        "parameters": [
            {"name": "Width", "kind": "length", "value": 800}, {"name": "Height", "kind": "length", "value": 1200},
            {"name": "Default Head Height", "kind": "length", "value": 2100}, {"name": "Sill Height", "kind": "length", "value": 0}
        ],
        "types": {"Tall": {"Sill Height": 0}, "Short": {"Height": 900, "Sill Height": 0}},
        "forms": [snapshot("Tall", 1200.0), snapshot("Short", 900.0)],
        "revit": {
            "to_bimcraft": {"x_offset": 0, "y_sign": 1, "z_minus": {"ref": "4"}},
            "reference_planes": planes,
            "dimensions": [
                {"refs": ["2", "3"], "label": "Width", "locked": true},
                {"refs": ["2", "1", "3"], "eq": true},
                {"refs": ["4", "5"], "label": "Height", "locked": true, "value": {"Tall": 1200, "Short": 900}},
                // To the level, which isn't a reference plane.
                {"refs": ["5", "21"], "label": "Default Head Height", "locked": true, "value": {"Tall": 2100, "Short": 2100}}
            ],
            "forms": [{"kind": "extrusion", "normal": [0, 0, 1], "start": r("4"), "end": r("5"), "loops": [rect(r("2"), r("3"), json!(-50), json!(50))]}]
        }
    })
}

#[test]
fn a_window_hung_from_its_head_takes_its_sill_from_the_dimensions_and_is_placed_there() {
    let mut s = Session::new();
    let r = run(&mut s, "family", head_hung_window());
    assert_eq!(r["editable"]["ok"], true, "{} {}", r["editable"], r["check"]);
    let f = s.model.families["Head Hung"].clone();
    let sill = |t: &str| f.values(t, &Default::default()).unwrap()[crate::family::SILL].clone();
    assert_eq!(sill("Tall"), crate::family::Val::Number(900.0));
    assert_eq!(sill("Short"), crate::family::Val::Number(1200.0));
    let none = |_: &str| None;
    assert!(near(extent(&f, "Short", &none).1[2], 900.0), "its z from its sill in every type");
    // Placed with no sill said, it stands where Revit puts it.
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let w: Id = run(&mut s, "window", json!({"at": [2000, 0], "family": "Head Hung", "type": "Short"}))["id"].as_u64().unwrap();
    let Some(Element::Window(o)) = s.model.elements.get(w) else { panic!("no window") };
    assert!(near(o.sill, 1200.0), "{}", o.sill);
}

#[test]
fn a_plane_kept_its_distance_in_every_type_by_an_unlocked_dimension_moves_with_the_other() {
    let mut s = Session::new();
    let mut fam = table(750.0);
    // An inner face 10 mm in from Right, measured but not locked in Revit: it still moves with Right in each type.
    fam["revit"]["reference_planes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "9", "name": "Inner", "normal": [1, 0, 0], "at": {"Small": 490, "Large": 690}}));
    fam["revit"]["dimensions"].as_array_mut().unwrap().push(json!({"refs": ["9", "3"], "value": {"Small": 10, "Large": 10}}));
    fam["revit"]["forms"][0]["loops"] = json!([rect(r("2"), r("9"), r("5"), r("6"))]);
    fam["forms"] = json!([
        {"mesh": cuboid([-500.0, -300.0, 0.0], [490.0, 300.0, 750.0]), "types": ["Small"], "snapshot": true},
        {"mesh": cuboid([-700.0, -400.0, 0.0], [690.0, 400.0, 750.0]), "types": ["Large"], "snapshot": true}
    ]);
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{} {}", r["editable"], r["check"]);
}

#[test]
fn a_ring_of_two_half_circles_is_a_hole_and_an_axis_drawn_in_a_sketch_is_no_loop() {
    let mut s = Session::new();
    let p = |x: f64, y: f64, z: f64| json!([x, y, z]);
    let half = |r: f64, from: f64| json!({"arc": {"center": p(0.0, 0.0, 0.0), "radius": r, "start": p(from * r, 0.0, 0.0), "end": p(-from * r, 0.0, 0.0), "normal": [0, 0, 1]}});
    let circle = |r: f64| json!([half(r, 1.0), half(r, -1.0)]);
    let fam = json!({
        "name": "Collar", "category": "generic", "types": {"A": {}},
        "revit": {"forms": [
            {"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 100, "loops": [circle(80.0), circle(70.0)]},
            {"kind": "revolution", "axis": [p(0.0, 0.0, 0.0), p(0.0, 0.0, 1000.0)], "start_angle": 0, "end_angle": 360,
             "loops": [ring_of(&[p(100.0, 0.0, 200.0), p(150.0, 0.0, 200.0), p(150.0, 0.0, 300.0), p(100.0, 0.0, 300.0)]),
                       [line(p(0.0, 0.0, 0.0), p(0.0, 0.0, 1000.0))]]}
        ]}
    });
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    let f = s.model.families["Collar"].clone();
    assert_eq!(f.forms.len(), 2, "one ring with its hole, one revolve");
    assert_eq!(f.forms[0].holes.len(), 1);
    let alone = Family { forms: vec![f.forms[0].clone()], ..f.clone() };
    let want = std::f64::consts::PI * (80.0f64.powi(2) - 70.0f64.powi(2)) * 100.0;
    assert!((volume(&alone, "A") - want).abs() < want * 0.02, "{} against {want}", volume(&alone, "A"));
}

#[test]
fn a_sweep_of_a_profile_with_a_loop_inside_is_a_tube() {
    let mut s = Session::new();
    let p = |x: f64, y: f64, z: f64| json!([x, y, z]);
    let square = |h: f64| ring_of(&[p(0.0, -h, -h), p(0.0, h, -h), p(0.0, h, h), p(0.0, -h, h)]);
    let fam = json!({
        "name": "Rail", "category": "generic", "types": {"A": {}},
        "revit": {"forms": [{"kind": "sweep", "path": [line(p(0.0, 0.0, 0.0), p(1000.0, 0.0, 0.0)), line(p(1000.0, 0.0, 0.0), p(1000.0, 1000.0, 0.0))],
                             "profile": [square(25.0), square(15.0)]}]}
    });
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    let f = s.model.families["Rail"].clone();
    assert_eq!(f.forms.len(), 1, "the inside is a hole of the one sweep");
    assert_eq!(f.forms[0].section_holes.len(), 1);
    let want = 2.0 * 1000.0 * (2500.0 - 900.0);
    assert!((volume(&f, "A") - want).abs() < want * 0.001, "{} against {want}", volume(&f, "A"));
    let v = f.values("A", &Default::default()).unwrap();
    let pieces = f.pieces("A", &v).unwrap();
    let tris: Vec<[[f64; 3]; 3]> = pieces.iter().flat_map(|p| p.tris.iter().copied()).collect();
    // Closed: every edge met by one running the other way.
    let key = |p: [f64; 3]| p.map(|x| (x * 1000.0).round() as i64);
    let mut edges = std::collections::HashMap::new();
    for t in &tris {
        for i in 0..3 {
            *edges.entry((key(t[i]), key(t[(i + 1) % 3]))).or_insert(0i32) += 1;
        }
    }
    let open = edges.iter().filter(|((a, b), n)| edges.get(&(*b, *a)).copied().unwrap_or(0) != **n).count();
    assert_eq!(open, 0, "a tube closed at its ends round its inside");
}

#[test]
fn a_nested_parts_plan_symbols_are_drawn_where_it_stands() {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Sample Mark", "category": "generic", "types": {"Standard": {}},
               "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 10}],
               "symbols": [{"line": [[0, 0], [100, 0]]}]}),
    );
    run(
        &mut s,
        "family",
        json!({"name": "Sample Holder", "category": "furniture", "types": {"Standard": {}},
               "forms": [{"profile": [[0, 0], [500, 0], [500, 500], [0, 500]], "from": 0, "to": 10}],
               "nested": [{"family": "Sample Mark", "type": "Standard", "at": [1000, 0, 0], "rotation": 90}]}),
    );
    let c: Id = run(&mut s, "component", json!({"family": "Sample Holder", "type": "Standard", "at": [0, 0]}))["id"].as_u64().unwrap();
    let Some(Element::Component(comp)) = s.model.elements.get(c) else { panic!("no component") };
    let marks = s.model.component_marks(comp, crate::graphics::Detail::Medium);
    let ends: Vec<crate::geom::P2> = marks
        .iter()
        .flat_map(|m| match m {
            crate::family::Mark::Line(pts) => pts.clone(),
            _ => Vec::new(),
        })
        .collect();
    assert!(ends.iter().any(|p| near(p.0, 1000.0) && near(p.1, 100.0)), "turned a quarter where it stands: {ends:?}");
}

#[test]
fn a_shared_nested_family_comes_into_the_project_and_one_not_shared_is_held_by_its_parent() {
    let mut s = Session::new();
    let part = |w: f64| {
        json!({"category": "generic", "types": {"Standard": {}},
               "forms": [{"profile": [[0, 0], [w, 0], [w, 50], [0, 50]], "from": 0, "to": 50}],
               "symbols": [{"line": [[0, 0], [w, 0]]}]})
    };
    let mut shelf = table(750.0);
    shelf.as_object_mut().unwrap().remove("name");
    let mut definition = shelf.as_object_mut().unwrap().remove("revit").unwrap();
    definition["nested"] = json!([
        {"id": "n1", "family": "Sample Knob", "type": "Standard", "shared": true, "at": [0, 0, 750], "rotation": 0, "mirrored": false},
        {"id": "n2", "family": "Sample Rail", "type": "Standard", "shared": false, "at": [0, 0, 750], "rotation": 0, "mirrored": false}
    ]);
    let package = json!({"format": 2, "family": shelf, "definition": definition,
                         "nested_families": {"Sample Knob": {"family": part(50.0)}, "Sample Rail": {"family": part(300.0)}}});
    s.files.give("Sample Unit.arqiflow-family.json", package.to_string().into_bytes());
    run(&mut s, "family", json!({"path": "Sample Unit.arqiflow-family.json"}));
    assert!(s.model.families.contains_key("Sample Knob"), "shared: the project's");
    assert!(!s.model.families.contains_key("Sample Rail"), "not shared: its parent's");
    let f = s.model.families["Sample Unit"].clone();
    assert!(f.embedded.contains_key("Sample Rail") && !f.embedded.contains_key("Sample Knob"));
    // The project written and read back keeps what the family holds.
    let (m, _) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(m.families["Sample Unit"].embedded, f.embedded);
    // Its parts, built with the project's families: the rail it holds among them.
    let mut parametric =
        Family { check: Default::default(), forms: f.forms.iter().filter(|x| !x.snapshot).cloned().collect(), ..f.clone() };
    parametric.symbols.retain(|y| !y.snapshot);
    let others = |n: &str| s.model.families.get(n);
    assert!(near(extent(&parametric, "Small", &others).1[2], 800.0), "the parts stand on the top");
    let v = parametric.values("Small", &Default::default()).unwrap();
    let marks = parametric.marks_with("Small", &v, crate::family::SymbolView::Plan, None, &others).unwrap();
    assert_eq!(marks.len(), 2, "the knob's and the rail's plan lines");
}

#[test]
fn a_sweep_of_a_round_profile_drawn_as_two_half_circles_is_a_rod() {
    let mut s = Session::new();
    let p = |x: f64, y: f64, z: f64| json!([x, y, z]);
    let half = |from: f64| json!({"arc": {"center": p(0.0, 0.0, 0.0), "radius": 10, "start": p(0.0, 0.0, 10.0 * from), "end": p(0.0, 0.0, -10.0 * from), "normal": [-1, 0, 0]}});
    let fam = json!({
        "name": "Rod", "category": "generic", "types": {"A": {}},
        "revit": {"forms": [{"kind": "sweep", "path": [line(p(0.0, 0.0, 0.0), p(500.0, 0.0, 0.0))], "profile": [[half(1.0), half(-1.0)]]}]}
    });
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    let f = s.model.families["Rod"].clone();
    let want = std::f64::consts::PI * 100.0 * 500.0;
    assert!((volume(&f, "A") - want).abs() < want * 0.02, "{} against {want}", volume(&f, "A"));
}

#[test]
fn an_extrusion_on_a_sloped_plane_is_swept_along_its_normal() {
    let mut s = Session::new();
    // A back leaning 10 degrees: its sketch on a plane whose normal is (0, cos 10, sin 10), 400 by 300, 38 thick.
    let (c, si) = (10f64.to_radians().cos(), 10f64.to_radians().sin());
    let n = [0.0, c, si];
    // Corners on the plane through (0, -200, 600): across it x, and up it (0, -sin, cos).
    let corner = |x: f64, up: f64| json!([x, -200.0 - si * up, 600.0 + c * up]);
    let at = -200.0 * c + 600.0 * si;
    let fam = json!({
        "name": "Leaning Back", "category": "furniture", "types": {"A": {}},
        "revit": {
            "reference_planes": [{"id": "b", "name": "back", "normal": n, "at": {"A": at}}],
            "forms": [{"kind": "extrusion", "normal": n, "start": at, "end": at + 38.0,
                       "loops": [ring_of(&[corner(-200.0, 0.0), corner(200.0, 0.0), corner(200.0, 300.0), corner(-200.0, 300.0)])]}]
        }
    });
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{}", r["editable"]);
    let f = s.model.families["Leaning Back"].clone();
    let want = 400.0 * 300.0 * 38.0;
    assert!((volume(&f, "A") - want).abs() < want * 0.001, "{} against {want}", volume(&f, "A"));
    let none = |_: &str| None;
    let (lo, hi) = extent(&f, "A", &none);
    // It leans back from its sketch, the far face 38 along the normal.
    assert!(near(lo[2], 600.0) && near(hi[1], -200.0 + 38.0 * c), "{lo:?} {hi:?}");
}

#[test]
fn a_windows_board_below_the_plan_cut_is_seen_in_plan_whatever_its_plan_setting() {
    let mut s = Session::new();
    // A board on the sill, 25 thick, Plan/RCP off: Revit's floor plan, cutting 1200 over the level, sees it from above.
    let board = |t: &str| {
        json!({"mesh": cuboid([-450.0, -160.0, 0.0], [450.0, 0.0, 25.0]), "types": [t], "snapshot": true,
                                 "show": {"coarse": false, "medium": true, "fine": true, "plan": false, "front": true, "side": true}})
    };
    let outline = |t: &str| {
        let ring = [[-450.0, -160.0], [450.0, -160.0], [450.0, 0.0], [-450.0, 0.0], [-450.0, -160.0]];
        json!({"line": ring, "types": [t], "snapshot": true, "show": {"coarse": false, "medium": true, "fine": true}})
    };
    let fam = json!({
        "name": "Boarded", "category": "window", "host": "wall",
        "parameters": [{"name": "Width", "kind": "length", "value": 800}, {"name": "Height", "kind": "length", "value": 1200},
                       {"name": "Sill Height", "kind": "length", "value": 900}],
        "types": {"A": {}},
        "forms": [board("A")],
        "symbols": [outline("A")],
        "revit": {"forms": [{"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 25,
                             "show": {"coarse": false, "medium": true, "fine": true, "plan": false, "front": true, "side": true},
                             "loops": [rect(json!(-450), json!(450), json!(-160), json!(0))]}]}
    });
    let r = run(&mut s, "family", fam);
    assert_eq!(r["editable"]["ok"], true, "{} {}", r["editable"], r["check"]);
}

#[test]
fn a_sweep_starting_on_a_bend_keeps_its_profile_at_the_start() {
    let mut s = Session::new();
    let p = |x: f64, y: f64, z: f64| json!([x, y, z]);
    // A tube's bend up from the floor, then a long run back along -y high above: the profile at the start faces y, as the
    // run does, but it belongs at the start.
    let half = |from: f64| {
        json!({"arc": {"center": p(0.0, 0.0, 12.7), "radius": 12.7, "start": p(0.0, 0.0, 12.7 + 12.7 * from),
                                          "end": p(0.0, 0.0, 12.7 - 12.7 * from), "normal": [0, -1, 0]}})
    };
    let fam = json!({
        "name": "Frame", "category": "furniture", "types": {"A": {}},
        "revit": {"forms": [{"kind": "sweep", "profile": [[half(1.0), half(-1.0)]],
                             "path": [{"arc": {"center": p(0.0, 0.0, 63.5), "radius": 50.8, "start": p(0.0, 0.0, 12.7), "end": p(0.0, 50.8, 63.5), "normal": [1, 0, 0]}},
                                      line(p(0.0, 50.8, 63.5), p(0.0, 50.8, 400.0)),
                                      {"arc": {"center": p(0.0, 0.0, 400.0), "radius": 50.8, "start": p(0.0, 50.8, 400.0), "end": p(0.0, 0.0, 450.8), "normal": [1, 0, 0]}},
                                      line(p(0.0, 0.0, 450.8), p(0.0, -400.0, 450.8))]}]}
    });
    run(&mut s, "family", fam);
    let f = s.model.families["Frame"].clone();
    let none = |_: &str| None;
    let (lo, hi) = extent(&f, "A", &none);
    assert!(lo[1] > -401.0 && hi[1] < 64.0 && lo[2] > -0.5, "{lo:?} {hi:?}");
}

#[test]
fn an_arch_through_a_bound_point_follows_its_width_where_its_centre_cant() {
    let mut s = Session::new();
    // A segmental arch over a 1000 wide opening rising 200: its centre and radius aren't sums of Width; its ends and its
    // crown are.
    let w = |k: f64| json!({"param": "Width", "scale": k});
    let fam = json!({
        "name": "Arch", "category": "generic", "types": {"Narrow": {}, "Wide": {"Width": 1600}},
        "parameters": [{"name": "Width", "kind": "length", "value": 1000}],
        "revit": {"forms": [{"kind": "extrusion", "normal": [0, 1, 0], "start": 0, "end": 100, "loops": [[
            line(json!([w(-0.5), 0, 0]), json!([w(0.5), 0, 0])),
            line(json!([w(0.5), 0, 0]), json!([w(0.5), 0, 1000])),
            {"arc": {"start": [w(0.5), 0, 1000], "end": [w(-0.5), 0, 1000], "center": [0, 0, 375], "radius": 725, "normal": [0, -1, 0],
                     "mid": [0, 0, 1200]}},
            line(json!([w(-0.5), 0, 1000]), json!([w(-0.5), 0, 0]))
        ]]}]}
    });
    run(&mut s, "family", fam);
    let f = s.model.families["Arch"].clone();
    let none = |_: &str| None;
    let (lo, hi) = extent(&f, "Wide", &none);
    assert!(near(lo[0], -800.0) && near(hi[0], 800.0) && (hi[2] - 1200.0).abs() < 10.0, "{lo:?} {hi:?}");
    // The segment over 1600 rising 200: radius (800² + 200²) / 400 = 1700.
    let (r, half) = (1700.0f64, 800.0f64);
    let segment = r * r * (half / r).asin() - half * (r - 200.0);
    let want = (1600.0 * 1000.0 + segment) * 100.0;
    assert!((volume(&f, "Wide") - want).abs() < want * 0.005, "{} against {want}", volume(&f, "Wide"));
}

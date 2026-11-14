//! What the last four Revit samples taught the engine (a shared nested part drawn by its own settings, a nested window's wall,
//! a side region's edges, a face on the cut plane, curves to within half a millimetre) and what a package may now say (a revolve's
//! turn, voids whose cuts were measured, regions whose edges carry their own styles, values set on nested parts, a string
//! dimension's locked segments, subcategories' line weights and patterns). All original content, made here (never Autodesk's).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::prim::Prim;
use crate::render::{self, Drawing};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A box from `lo` to `hi` as twelve triangles, as a snapshot mesh holds it.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Value {
    let p =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    json!(quads.iter().flat_map(|[a, b, c, d]| [json!([p(*a), p(*b), p(*c)]), json!([p(*a), p(*c), p(*d)])]).collect::<Vec<_>>())
}

/// The closed outlines `id` draws in `d`, in model mm.
fn rings(d: &Drawing, id: Id) -> Vec<Vec<P2>> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, closed: true, .. } if *o == Some(id) => Some(pts.iter().map(|q| d.view.mm(*q)).collect::<Vec<_>>()),
            _ => None,
        })
        .collect()
}

fn extent(ring: &[P2]) -> (f64, f64) {
    let (lo, hi) = ring.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
        (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
    });
    (hi.0 - lo.0, hi.1 - lo.1)
}

/// A generic family of one box, 100 by 40 and 300 tall, centred on its origin: a trim.
fn trim() -> Value {
    json!({"category": "generic", "types": {"Standard": {}},
           "forms": [{"profile": [[-50, -20], [50, -20], [50, 20], [-50, 20]], "from": 0, "to": 300, "show": {}}]})
}

/// An export package of a window in a 200 wall: a jamb Revit's plan cuts, and two trims standing below the cut, each its
/// nested family, the first `shared`. Revit draws the shared trim (its own settings) and not the other (its host's say it is
/// seen only where cut).
fn trimmed() -> Value {
    let at = |v: f64| json!({"900": v});
    let everything = json!({"coarse": true, "medium": true, "fine": true, "plan": true, "front": true, "side": true,
                            "plan_top": true, "plan_cut": true});
    let below_the_cut = json!({"coarse": true, "medium": true, "fine": true, "plan": true, "front": true, "side": true,
                               "plan_top": false, "plan_cut": true});
    let side = |a: Value, b: Value| json!({"line": [a, b]});
    let corner = |x: f64, y: f64| json!([x, y, 0]);
    let ring = json!([
        side(corner(-500.0, -100.0), corner(-450.0, -100.0)),
        side(corner(-450.0, -100.0), corner(-450.0, 100.0)),
        side(corner(-450.0, 100.0), corner(-500.0, 100.0)),
        side(corner(-500.0, 100.0), corner(-500.0, -100.0)),
    ]);
    json!({
        "format": 2,
        "family": {"category": "window", "host": "wall",
                   "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 1500},
                                  {"name": "Sill Height", "kind": "length", "value": 0}],
                   "types": {"900": {"Width": 900}},
                   "forms": [
                       {"mesh": cuboid([-500.0, -100.0, 0.0], [-450.0, 100.0, 1500.0]), "types": ["900"], "snapshot": true},
                       {"mesh": cuboid([100.0, -100.0, 0.0], [200.0, -60.0, 300.0]), "types": ["900"], "snapshot": true},
                       {"mesh": cuboid([-200.0, -100.0, 0.0], [-100.0, -60.0, 300.0]), "types": ["900"], "snapshot": true,
                        "show": {"plan": false, "plan_top": false, "plan_cut": false}}]},
        "definition": {
            "to_bimcraft": {"x_offset": 0, "y_sign": 1, "z_minus": 0, "wall": at(200.0)},
            "reference_planes": [
                {"id": "c", "name": "Center", "normal": [1, 0, 0], "defines_origin": true, "at": at(0.0)},
                {"id": "e", "name": "Exterior", "normal": [0, 1, 0], "is_reference": "host_exterior", "at": at(100.0)},
                {"id": "i", "name": "Interior", "normal": [0, 1, 0], "is_reference": "host_interior", "at": at(-100.0)},
                {"id": "z", "name": "Ref Level", "normal": [0, 0, 1], "defines_origin": true, "at": at(0.0)}
            ],
            "forms": [{"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 1500, "loops": [ring], "show": everything}],
            "nested": [
                {"id": "s", "family": "Sample Shared Trim", "type": "Standard", "shared": true, "at": [150, -80, 0], "show": below_the_cut},
                {"id": "o", "family": "Sample Own Trim", "type": "Standard", "shared": false, "at": [-150, -80, 0], "show": below_the_cut}]
        },
        "nested_families": {"Sample Shared Trim": {"family": trim()}, "Sample Own Trim": {"family": trim()}}
    })
}

#[test]
fn a_shared_trim_below_the_cut_is_drawn_in_a_windows_plan() {
    let mut s = Session::new();
    s.files.give("Sample Trimmed Window.arqiflow-family.json", trimmed().to_string().into_bytes());
    let r = run(&mut s, "family", json!({"path": "Sample Trimmed Window.arqiflow-family.json"}));
    assert_eq!(r["check"]["900"]["matched"], true, "{}", r["check"]);
    assert!(!s.model.families["Sample Trimmed Window"].draws_snapshot("900"), "drawn from its rules, which is what this is about");
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let w = id_of(&run(&mut s, "window", json!({"at": [2000, 0], "family": "Sample Trimmed Window", "type": "900"})));
    for detail in ["coarse", "medium", "fine"] {
        run(&mut s, "detail", json!({"view": 100000, "detail": detail}));
        let d = render::plan(&s.model, 100000, 3000, 2000);
        let trims = rings(&d, w).into_iter().filter(|r| {
            let (x, y) = extent(r);
            (x - 100.0).abs() < 1.0 && (y - 40.0).abs() < 1.0
        });
        let at: Vec<f64> = trims.map(|r| r.iter().map(|p| p.0).sum::<f64>() / r.len() as f64).collect();
        assert_eq!(at.len(), 1, "{detail}: one trim, the shared one, is drawn: {at:?}");
        // Window at x 2000: the shared trim stands 150 to the right of it.
        assert!((at[0] - 2150.0).abs() < 1.0, "{detail}: {at:?}");
    }
}

#[test]
fn a_family_saying_its_line_weights_or_patterns_is_written_as_25_and_read_back() {
    let family = |extra: Value| {
        let mut f = json!({"name": "Sample Block", "category": "furniture", "types": {"A": {}},
            "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}], "subcategories": [{"name": "Edge"}]});
        for (k, v) in extra.as_object().into_iter().flatten() {
            f[k] = v.clone();
        }
        f
    };
    let written = |f: Value| {
        let mut s = Session::new();
        run(&mut s, "family", f);
        let json = s.model.to_json().unwrap();
        let v: Value = serde_json::from_str(&json).unwrap();
        let (m, left_out) = crate::model::Model::read(&json).unwrap();
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(m.families["Sample Block"], s.model.families["Sample Block"]);
        v["format"].as_u64().unwrap_or_default()
    };
    let plain = written(family(json!({})));
    assert!(plain < u64::from(crate::model::STYLES_FORMAT), "no line weights: {plain}");
    let weighted = written(family(json!({"subcategories": [{"name": "Edge", "projection": 2}]})));
    assert_eq!(weighted, u64::from(crate::model::STYLES_FORMAT));
    let patterned = written(family(json!({"subcategories": [{"name": "Edge", "pattern": "Sample Dash"}],
        "line_patterns": {"Sample Dash": [{"dash": 3.0}, {"space": 1.5}]}})));
    assert_eq!(patterned, u64::from(crate::model::STYLES_FORMAT));
}

#[test]
fn a_hostile_subcategory_weight_is_refused_by_the_family_command() {
    let mut s = Session::new();
    for sub in [json!({"name": "Edge", "projection": 0}), json!({"name": "Edge", "cut": 17}), json!({"name": "Edge", "projection": 2.5})] {
        let e = s
            .execute(
                "family",
                &json!({"name": "Sample Block", "category": "furniture", "types": {"A": {}},
                        "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}], "subcategories": [sub]}),
            )
            .unwrap_err();
        assert!(e.contains("pen") || e.contains("line weight") || e.contains("whole number"), "{e}");
    }
    assert!(!s.model.families.contains_key("Sample Block") && s.model.subcategory_styles.is_empty());
}

/// What a build that reads up to 24 refuses as a field it doesn't know (the project's `subcategory_styles`, a family's
/// `cuts_measured` and `line_patterns`, a subcategory's pens and pattern) is written as 25, each alone, and the file reads back.
#[test]
fn each_new_field_alone_is_written_as_25_and_a_subcategory_style_outlives_its_family() {
    use crate::model::{Model, PREVIOUS_FORMAT, STYLES_FORMAT};
    assert_eq!(STYLES_FORMAT, 25);
    let block = |extra: Value| {
        let mut f = json!({"name": "Sample Block", "category": "furniture", "types": {"A": {}},
            "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}]});
        for (k, v) in extra.as_object().into_iter().flatten() {
            f[k] = v.clone();
        }
        f
    };
    let format = |s: &Session| {
        let json = s.model.to_json().unwrap();
        let (m, left_out) = Model::read(&json).unwrap();
        assert!(left_out.is_empty(), "{left_out:?}");
        assert_eq!(m.subcategory_styles, s.model.subcategory_styles);
        serde_json::from_str::<Value>(&json).unwrap()["format"].as_u64().unwrap_or_default()
    };
    let only = |extra: Value| {
        let mut s = Session::new();
        run(&mut s, "family", block(extra));
        (format(&s), s)
    };
    assert_eq!(only(json!({})).0, u64::from(PREVIOUS_FORMAT), "the same family with none of them");
    // A subcategory that has only a colour: what the project's Object Styles take.
    let (f, mut s) = only(json!({"subcategories": [{"name": "Edge", "color": [200, 0, 0]}]}));
    assert_eq!((f, s.model.subcategory_styles.len()), (25, 1), "a colour the project draws it in");
    // The family deleted, what the project drew it in stays, and so does the format.
    run(&mut s, "family", json!({"name": "Sample Block", "delete": true}));
    assert!(!s.model.families.contains_key("Sample Block") && s.model.subcategory_styles.len() == 1);
    assert_eq!(format(&s), 25, "a style outlives its family");
    // A family's line patterns, its void cuts' having been measured, and a subcategory's pen, each alone.
    assert_eq!(only(json!({"line_patterns": {"Sample Dash": [{"dash": 3.0}, {"space": 1.5}]}})).0, 25);
    assert_eq!(only(json!({"cuts_measured": true})).0, 25);
    assert_eq!(only(json!({"subcategories": [{"name": "Edge", "cut": 3}]})).0, 25);
}

#[test]
fn a_region_whose_levels_were_read_keeps_them_and_one_whose_levels_were_not_learns_them_from_revits_plan() {
    let mut s = Session::new();
    let all = json!({"coarse": true, "medium": true, "fine": true, "plan": true, "front": false, "side": false});
    let region = |from: f64, to: f64, extra: Value| {
        let mut r = json!({"view": "plan", "filled": true, "masking": true, "show": all,
                           "curves": [{"line": [[from, 0, 0], [to, 0, 0]]}]});
        for (k, v) in extra.as_object().into_iter().flatten() {
            r[k] = v.clone();
        }
        r
    };
    // Revit's plan draws both regions' edges at Medium and Fine only, and a package says each is shown at every level (the
    // second says it read that: `levels_read`; or read Medium and Fine only).
    let said = |extra: Value| {
        let mut r = region(0.0, 100.0, extra);
        r["show"] = json!({"coarse": false, "medium": true, "fine": true, "plan": true, "front": false, "side": false});
        r
    };
    let fam = json!({
        "name": "Sample Sill", "category": "door", "host": "wall", "types": {"A": {}},
        "symbols": [{"line": [[0, 0], [100, 0]], "types": ["A"], "snapshot": true, "show": {"coarse": false}},
                    {"line": [[0, 50], [100, 50]], "types": ["A"], "snapshot": true, "show": {"coarse": false}},
                    {"line": [[0, 80], [100, 80]], "types": ["A"], "snapshot": true, "show": {"coarse": false}}],
        "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 2100}],
        "revit": {"reference_planes": [{"id": "c", "name": "Center", "normal": [1, 0, 0], "defines_origin": true, "at": {"A": 0}}],
                  "lines": [region(0.0, 100.0, json!({})),
                            // Read, and drawn at every level though Revit's plan draws it at Medium and Fine (something else hides it).
                            {"view": "plan", "filled": true, "masking": true, "levels_read": true, "show": all,
                             "curves": [{"line": [[0, 50, 0], [100, 50, 0]]}]},
                            said(json!({"levels_read": true, "curves": [{"line": [[0, 80, 0], [100, 80, 0]]}]}))]}
    });
    run(&mut s, "family", fam);
    let f = &s.model.families["Sample Sill"];
    let masks: Vec<_> = f.symbols.iter().filter(|y| !y.snapshot).collect();
    assert_eq!(masks.len(), 3, "{masks:?}");
    let levels = |i: usize| masks[i].show.map(|l| (l.coarse, l.medium, l.fine));
    assert_eq!(levels(0), Some((false, true, true)), "not read: learnt from Revit's plan");
    assert_eq!(levels(1), Some((true, true, true)), "read: kept, whatever Revit's plan draws");
    assert_eq!(levels(2), Some((false, true, true)), "read: Medium and Fine");
}

#[test]
fn a_line_pattern_the_engine_cannot_hold_is_said_and_left_out_not_a_reason_to_refuse_the_family() {
    let long: Vec<Value> = (0..40).map(|_| json!({"dash": 1.0})).collect();
    let package = |patterns: Value| {
        json!({"name": "Sample Block", "category": "furniture", "types": {"A": {}},
            "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}],
            "subcategories": [{"name": "Edge", "pattern": "Too Long"}, {"name": "Rim", "pattern": "Sample Dash"},
                              {"name": "Hidden", "pattern": "Odd Segment"}],
            "line_patterns": patterns, "revit": {}})
    };
    let mut s = Session::new();
    let patterns = json!({
        "Sample Dash": [{"dash": 3.0}, {"space": 1.5}],
        "Too Long": long,
        "Too Short": [{"dash": 0.001}, {"space": 1.5}],
        "Odd Segment": [{"squiggle": 3.0}],
    });
    let r = run(&mut s, "family", package(patterns.clone()));
    let f = &s.model.families["Sample Block"];
    assert_eq!(f.line_patterns.keys().map(String::as_str).collect::<Vec<_>>(), ["Sample Dash"], "{:?}", f.line_patterns);
    let pattern_of = |n: &str| f.subcategories.iter().find(|s| s.name == n).and_then(|s| s.pattern.clone());
    assert_eq!((pattern_of("Edge"), pattern_of("Rim"), pattern_of("Hidden")), (None, Some("Sample Dash".to_string()), None));
    let said = &f.editable.as_ref().expect("said").untranslated;
    for name in ["Too Long", "Too Short", "Odd Segment"] {
        assert!(said.iter().any(|n| n.contains(name) && n.contains("drawn solid")), "{name}: {said:?} {r}");
    }
    assert!(!said.iter().any(|n| n.contains("Sample Dash")), "{said:?}");
    // The project drew the one it can hold.
    assert!(s.model.line_patterns.contains_key("Sample Dash") && !s.model.line_patterns.contains_key("Too Long"));
    // The same patterns given directly, not by a package from Revit, are still refused: a person is told what to fix.
    let mut plain = package(patterns);
    plain.as_object_mut().map(|o| o.remove("revit"));
    let e = Session::new().execute("family", &plain).unwrap_err();
    assert!(e.contains("line_patterns") || e.contains("line pattern"), "{e}");
}

#[test]
fn object_styles_lists_a_familys_subcategories_under_their_category_and_edits_them() {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Sample Block", "category": "furniture", "types": {"A": {}},
               "forms": [{"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "from": 0, "to": 100}],
               "subcategories": [{"name": "Edge", "color": [1, 2, 3]}, {"name": "Rim"}]}),
    );
    let row = |v: &Value, key: &str| v["subcategories"].as_array().unwrap().iter().find(|r| r["key"] == key).cloned();
    let all = run(&mut s, "object_styles", json!({}));
    let edge = row(&all, "furniture/Edge").expect("a row for each subcategory");
    assert_eq!(
        (edge["category"].clone(), edge["name"].clone(), edge["color"].clone(), edge["own"].clone()),
        (json!("furniture"), json!("Edge"), json!([1, 2, 3]), json!(true))
    );
    let rim = row(&all, "furniture/Rim").expect("a plain one too");
    assert_eq!((rim["projection"].clone(), rim["own"].clone()), (json!(null), json!(false)));
    // Set, as Revit's dialog sets a row: its weights, colour, pattern and material.
    let material = s.model.materials.keys().next().expect("the project has a material").clone();
    let set = run(
        &mut s,
        "object_styles",
        json!({"subcategories": {"furniture/Rim": {"projection": 3, "cut": 5, "color": [9, 8, 7], "pattern": "Dash", "material": material}}}),
    );
    let rim = row(&set, "furniture/Rim").unwrap();
    assert_eq!(
        (rim["projection"].clone(), rim["cut"].clone(), rim["color"].clone(), rim["pattern"].clone(), rim["material"].clone()),
        (json!(3), json!(5), json!([9, 8, 7]), json!("Dash"), json!(material))
    );
    let style = s.model.subcategory_style("furniture", "Rim").expect("the project's style");
    assert_eq!((style.projection, style.cut), (Some(crate::pen::Pen(3)), Some(crate::pen::Pen(5))));
    // What isn't given stays; null takes it back to none, and a style with nothing is no style.
    let part = run(&mut s, "object_styles", json!({"subcategories": {"furniture/Rim": {"projection": null}}}));
    let rim = row(&part, "furniture/Rim").unwrap();
    assert_eq!((rim["projection"].clone(), rim["cut"].clone()), (json!(null), json!(5)));
    run(
        &mut s,
        "object_styles",
        json!({"subcategories": {"furniture/Rim": {"cut": null, "color": null, "pattern": null, "material": null}}}),
    );
    assert!(s.model.subcategory_style("furniture", "Rim").is_none());
    // Refused: not a subcategory of the project's, a pen that isn't one, a pattern or a material it hasn't.
    for (given, says) in [
        (json!({"furniture/Nothing": {"cut": 3}}), "isn't a subcategory"),
        (json!({"furniture/Rim": {"cut": 0}}), "pen"),
        (json!({"furniture/Rim": {"cut": 17}}), "pen"),
        (json!({"furniture/Rim": {"pattern": "No Such"}}), "line pattern"),
        (json!({"furniture/Rim": {"material": "No Such"}}), "material"),
        (json!({"furniture/Rim": {"weight": 3}}), "weight"),
    ] {
        let e = s.execute("object_styles", &json!({"subcategories": given})).unwrap_err();
        assert!(e.contains(says), "{given}: {e}");
    }
    assert!(s.model.subcategory_style("furniture", "Rim").is_none(), "a refusal changes nothing");
}

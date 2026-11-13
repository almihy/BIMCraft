//! A placed door's and window's drawing follows what the check found: a plan draws what its cut plane crosses as the section
//! (a frame's rebate), an elevation draws its nested parts' symbols, and a type drawn as Revit made it cuts the opening
//! Revit measured. All original content, made here (never Autodesk's).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family_revit_tests::jamb_package;
use crate::geom::P2;
use crate::graphics::Detail;
use crate::model::Id;
use crate::prim::Prim;
use crate::render::{self, Drawing};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A box from `lo` to `hi` as twelve triangles, as a mesh form holds it.
fn cuboid(lo: [f64; 3], hi: [f64; 3]) -> Vec<Value> {
    let p =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    quads.iter().flat_map(|[a, b, c, d]| [json!([p(*a), p(*b), p(*c)]), json!([p(*a), p(*c), p(*d)])]).collect()
}

/// The points of every line `id` draws in `d`, in model mm.
fn points(d: &Drawing, id: Id) -> Vec<P2> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, .. } if *o == Some(id) => Some(pts.iter().map(|q| d.view.mm(*q)).collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect()
}

#[test]
fn a_placed_door_draws_its_frame_cut_with_its_rebate() {
    // A jamb 50 across the wall's line and 200 deep, with a stop 20 by 40 standing on its inner face: one mesh, so its outline
    // from above is the hull round both, which fills the rebate in.
    let mut tris = cuboid([-450.0, -100.0, 0.0], [-400.0, 100.0, 2100.0]);
    tris.extend(cuboid([-400.0, -100.0, 0.0], [-380.0, -60.0, 2100.0]));
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Sample Rebated Door", "category": "door", "host": "wall",
               "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 2100}],
               "types": {"900": {"Width": 900}}, "forms": [{"mesh": tris}]}),
    );
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let d = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Sample Rebated Door", "type": "900"})));
    run(&mut s, "detail", json!({"view": 100000, "detail": "medium"}));
    let drawn = points(&render::plan(&s.model, 100000, 3000, 2000), d);
    // The cut plane (1200) crosses it: the corner where the stop meets the jamb is drawn, which the hull has no vertex at.
    let rebate = |p: &P2| (p.0 - 1600.0).abs() < 1.0 && (p.1.abs() - 60.0).abs() < 1.0;
    assert!(drawn.iter().any(rebate), "the rebate's inner corner is in the section: {drawn:?}");
}

#[test]
fn an_elevation_of_a_window_draws_its_nested_parts_side_lines() {
    let leg = json!({"category": "generic", "types": {"Standard": {}}, "symbols": [{"line": [[0, 0], [0, 500]], "view": "side"}]});
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Sample Sash Window", "category": "window", "host": "wall",
               "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 1200},
                              {"name": "Sill Height", "kind": "length", "value": 900}],
               "types": {"900": {}}, "forms": [{"profile": [[-450, -50], [450, -50], [450, 50], [-450, 50]], "from": 0, "to": 1200}],
               "embedded": {"Sample Leg": leg},
               "nested": [{"family": "Sample Leg", "type": "Standard", "at": [0, 0, 200]}]}),
    );
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let w = id_of(&run(&mut s, "window", json!({"at": [2000, 0], "family": "Sample Sash Window", "type": "900"})));
    // Looking along the wall, a side view: the sash's line stands in the window's x = 0 plane, from 200 to 700 over its sill.
    let lines = s.model.elevation_lines(&s.model.elements[&w], P2(1.0, 0.0), Detail::Fine);
    let up: Vec<f64> = lines.iter().flatten().map(|p| p[2]).collect();
    assert!(
        up.iter().any(|z| (z - 1100.0).abs() < 1.0) && up.iter().any(|z| (z - 1600.0).abs() < 1.0),
        "the nested part's line is drawn: {lines:?}"
    );
}

#[test]
fn a_section_along_a_wall_draws_a_windows_nested_parts_lines_though_the_window_has_no_elevation_symbols_of_its_own() {
    let leg = json!({"category": "generic", "types": {"Standard": {}}, "symbols": [{"line": [[0, 0], [0, 500]], "view": "side"}]});
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Sample Sash Window", "category": "window", "host": "wall",
               "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 1200},
                              {"name": "Sill Height", "kind": "length", "value": 900}],
               "types": {"900": {}}, "forms": [{"profile": [[-450, -50], [450, -50], [450, 50], [-450, 50]], "from": 0, "to": 1200}],
               "embedded": {"Sample Leg": leg},
               "nested": [{"family": "Sample Leg", "type": "Standard", "at": [0, 0, 200]}]}),
    );
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let w = id_of(&run(&mut s, "window", json!({"at": [2000, 0], "family": "Sample Sash Window", "type": "900"})));
    // A section across the wall's line at x = 500, looking along it towards the window: one of its two directions sees it.
    let mut found = Vec::new();
    for (start, end) in [([500, -3000], [500, 3000]), ([500, 3000], [500, -3000])] {
        let id = id_of(&run(&mut s, "section", json!({"start": start, "end": end})));
        let d = render::section(&s.model, id, render::section_fit(&s.model, id, 1600, 1000));
        let heights: Vec<f64> = points(&d, w).iter().map(|p| p.1).collect();
        found.push((heights.iter().any(|z| (z - 1100.0).abs() < 1.0), heights.iter().any(|z| (z - 1600.0).abs() < 1.0)));
    }
    assert!(found.contains(&(true, true)), "the sash's line is in the drawing, from 1100 to 1600: {found:?}");
}

#[test]
fn a_type_drawn_as_revit_made_it_cuts_revits_opening() {
    let hole = |openings: Value| {
        let mut s = Session::new();
        s.files.give("jamb.arqiflow-family.json", jamb_package(Some(openings)).to_string().into_bytes());
        run(&mut s, "family", json!({"path": "jamb.arqiflow-family.json"}));
        run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
        let d = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "jamb", "type": "900"})));
        let matched = s.model.families["jamb"].check["900"].matched;
        (matched, s.model.opening_type(&s.model.elements[&d]).map(|t| t.cut()))
    };
    // Revit's opening is wider than the door's rules cut: the type fails its check, draws Revit's result, and cuts its opening.
    let (matched, cut) = hole(json!({"900": {"width": 1000.0, "height": 2100.0, "bottom": 0.0}}));
    assert!(!matched);
    assert_eq!(cut, Some((1000.0, 2100.0)));
    // Where the opening is the same, the type matches and cuts what its rules say: its width and height.
    let (matched, cut) = hole(json!({"900": {"width": 900.0, "height": 2100.0, "bottom": 0.0}}));
    assert!(matched);
    assert_eq!(cut, Some((900.0, 2100.0)));
}

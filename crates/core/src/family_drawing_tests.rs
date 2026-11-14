//! A placed door's and window's drawing follows what the check found: a plan draws what its cut plane crosses as the section
//! (a frame's rebate), an elevation draws its nested parts' symbols, and a type drawn as Revit made it cuts the opening
//! Revit measured. All original content, made here (never Autodesk's).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::family_revit_tests::jamb_package;
use crate::geom::P2;
use crate::graphics::Detail;
use crate::model::Id;
use crate::pen::Pen;
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
    let up: Vec<f64> = lines.iter().flat_map(|(line, _)| line).map(|p| p[2]).collect();
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

/// A door family of one 900 x 2100 box form (in `subcategory` where given), with the subcategories `subs` and the line patterns
/// `patterns`, loaded as `name`.
fn styled_door(s: &mut Session, name: &str, subs: Value, patterns: Value, symbols: Value, subcategory: Option<&str>) {
    let mut form = json!({"profile": [[-450, -100], [-400, -100], [-400, 100], [-450, 100]], "from": 0, "to": 2100});
    if let Some(sub) = subcategory {
        form["subcategory"] = json!(sub);
    }
    run(
        s,
        "family",
        json!({"name": name, "category": "door", "host": "wall",
               "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 2100}],
               "types": {"900": {}}, "forms": [form], "subcategories": subs, "line_patterns": patterns, "symbols": symbols}),
    );
}

/// The lines `id` draws in `d`, as (width, dashed, closed, points in model mm).
fn strokes(d: &Drawing, id: Id) -> Vec<(f32, bool, bool, Vec<P2>)> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, width, dash, closed, .. } if *o == Some(id) => {
                Some((*width, dash.is_some(), *closed, pts.iter().map(|q| d.view.mm(*q)).collect()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn an_elevation_swing_draws_dashed_in_its_subcategorys_weight() {
    let swing = json!([{"line": [[-400, 0], [0, 2000], [400, 0]], "view": "front", "subcategory": "Elevation Swing"}]);
    let mut s = Session::new();
    styled_door(
        &mut s,
        "Sample Dashed Swing",
        json!([{"name": "Elevation Swing", "projection": 1, "pattern": "Sample Dash"}]),
        json!({"Sample Dash": [{"dash": 3.0}, {"space": 1.5}]}),
        swing.clone(),
        None,
    );
    styled_door(&mut s, "Sample Plain Swing", json!([{"name": "Elevation Swing"}]), json!({}), swing, None);
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let dashed = id_of(&run(&mut s, "door", json!({"at": [1000, 0], "family": "Sample Dashed Swing", "type": "900"})));
    let plain = id_of(&run(&mut s, "door", json!({"at": [3000, 0], "family": "Sample Plain Swing", "type": "900"})));
    // The family's pattern is the project's now.
    assert!(s.model.line_patterns.contains_key("Sample Dash"));
    // A section along the wall, looking at its face from one side or the other: one of the two sees the doors' front symbols.
    let mut seen = false;
    for (start, end) in [([0, -3000], [4000, -3000]), ([4000, 3000], [0, 3000])] {
        let id = id_of(&run(&mut s, "section", json!({"start": start, "end": end})));
        let d = render::section(&s.model, id, render::section_fit(&s.model, id, 1600, 1000));
        let (a, b) = (strokes(&d, dashed), strokes(&d, plain));
        // The swing: a line of three points, the apex 2000 up.
        let swing_of = |l: &[(f32, bool, bool, Vec<P2>)]| {
            l.iter().find(|(_, _, closed, pts)| !closed && pts.iter().any(|p| (p.1 - 2000.0).abs() < 1.0)).cloned()
        };
        if let (Some(a), Some(b)) = (swing_of(&a), swing_of(&b)) {
            seen = true;
            assert!(a.1, "the Dash pattern draws it dashed");
            assert!((a.0 - d.width(Pen(1))).abs() < 1e-3, "in the width of its subcategory's pen 1: {} not {}", a.0, d.width(Pen(1)));
            assert!(!b.1, "with no pattern, solid");
            assert!(b.0 > a.0, "in its category's projection pen, heavier: {} against {}", b.0, a.0);
        }
    }
    assert!(seen, "an elevation draws the doors' Elevation Swing");
}

#[test]
fn a_door_frame_cut_draws_in_its_subcategorys_cut_pen() {
    let mut s = Session::new();
    styled_door(&mut s, "Sample Light Frame", json!([{"name": "Frame", "cut": 1}]), json!({}), json!([]), Some("Frame"));
    styled_door(&mut s, "Sample Heavy Frame", json!([{"name": "Frame"}]), json!({}), json!([]), Some("Frame"));
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let light = id_of(&run(&mut s, "door", json!({"at": [1000, 0], "family": "Sample Light Frame", "type": "900"})));
    let heavy = id_of(&run(&mut s, "door", json!({"at": [3000, 0], "family": "Sample Heavy Frame", "type": "900"})));
    run(&mut s, "detail", json!({"view": 100000, "detail": "medium"}));
    let d = render::plan(&s.model, 100000, 3000, 2000);
    // The jamb the plane cuts: a ring 50 across and 200 deep.
    let jamb = |id: Id| {
        strokes(&d, id)
            .into_iter()
            .find(|(_, _, closed, pts)| {
                let xs = pts.iter().map(|p| p.0);
                let (lo, hi) = (xs.clone().fold(f64::MAX, f64::min), xs.fold(f64::MIN, f64::max));
                *closed && (hi - lo - 50.0).abs() < 1.0
            })
            .map(|(w, ..)| w)
    };
    let (a, b) = (jamb(light).expect("a cut jamb"), jamb(heavy).expect("a cut jamb"));
    assert!((a - d.width(Pen(1))).abs() < 1e-3, "its subcategory's cut pen 1: {a} not {}", d.width(Pen(1)));
    assert!((b - d.width(Pen::MEDIUM)).abs() < 1e-3, "else its category's cut pen: {b} not {}", d.width(Pen::MEDIUM));
}

#[test]
fn a_project_subcategory_style_wins_over_the_familys_on_load() {
    let mut s = Session::new();
    // The project has an Object Style for door/Frame (a pen 9 where seen) and a pattern of its own named as the family's.
    s.model_mut()
        .subcategory_styles
        .insert("door/Frame".into(), crate::styles::SubcategoryStyle { projection: Some(Pen(9)), ..Default::default() });
    let mine = crate::lines::LinePattern(vec![crate::lines::Segment::Dash(8.0), crate::lines::Segment::Space(2.0)]);
    s.model_mut().line_patterns.insert("Sample Dash".into(), mine.clone());
    styled_door(
        &mut s,
        "Sample Styled Door",
        json!([{"name": "Frame", "projection": 1, "cut": 2, "pattern": "Sample Dash"}, {"name": "Panel", "projection": 3}]),
        json!({"Sample Dash": [{"dash": 3.0}, {"space": 1.5}], "Sample Dot": ["dot", {"space": 2.0}]}),
        json!([]),
        Some("Frame"),
    );
    let styles = &s.model.subcategory_styles;
    // What the project has is kept whole; what it doesn't have is added.
    assert_eq!(styles["door/Frame"], crate::styles::SubcategoryStyle { projection: Some(Pen(9)), ..Default::default() });
    assert_eq!(styles["door/Panel"].projection, Some(Pen(3)));
    assert_eq!(s.model.line_patterns["Sample Dash"], mine, "the project's pattern of the name stays");
    assert!(s.model.line_patterns.contains_key("Sample Dot"), "the family's other pattern is added");
    // The project's style is what a plan draws it in: a cut jamb in the category's cut pen, not the family's `cut` 2.
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Sample Styled Door", "type": "900"})));
    run(&mut s, "detail", json!({"view": 100000, "detail": "medium"}));
    let d = render::plan(&s.model, 100000, 3000, 2000);
    let widths: Vec<f32> = strokes(&d, door).into_iter().filter(|(_, _, closed, _)| *closed).map(|(w, ..)| w).collect();
    assert!(widths.iter().any(|w| (w - d.width(Pen::MEDIUM)).abs() < 1e-3), "{widths:?}");
    assert!(widths.iter().all(|w| (w - d.width(Pen(2))).abs() > 1e-3), "the family's own cut pen is not used: {widths:?}");
    // Loaded again, nothing the project has changes.
    let before = s.model.subcategory_styles.clone();
    styled_door(&mut s, "Sample Styled Door", json!([{"name": "Frame", "projection": 5}]), json!({}), json!([]), Some("Frame"));
    assert_eq!(s.model.subcategory_styles["door/Frame"], before["door/Frame"]);
}

#[test]
fn a_walls_face_lines_stop_at_a_doors_opening() {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({"name": "Sample Wide Door", "category": "door", "host": "wall",
               "parameters": [{"name": "Width", "kind": "length", "value": 1010}, {"name": "Height", "kind": "length", "value": 2100}],
               "types": {"1010": {}}, "forms": [{"profile": [[-505, -20], [505, -20], [505, 20], [-505, 20]], "from": 0, "to": 50}]}),
    );
    let wall = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})));
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "Sample Wide Door", "type": "1010"})));
    let half = s.model.thickness(
        s.model
            .wall(match &s.model.elements[&door] {
                crate::model::Element::Door(o) => o.host,
                _ => panic!("a door"),
            })
            .unwrap(),
    ) / 2.0;
    for detail in ["coarse", "medium", "fine"] {
        run(&mut s, "detail", json!({"view": 100000, "detail": detail}));
        let d = render::plan(&s.model, 100000, 3000, 2000);
        let across: Vec<P2> = strokes(&d, wall).into_iter().flat_map(|(_, _, _, pts)| pts).collect();
        assert!(across.iter().any(|p| p.0 < 1400.0) && across.iter().any(|p| p.0 > 2600.0), "{detail}: the wall is drawn either side");
        // Between the jambs, no point of the wall's lines at its faces: they end at the jambs. (Its hatch and the lines between its
        // layers run across the opening under the gap's white; what stays is the faces.)
        let stray: Vec<&P2> = across.iter().filter(|p| p.0 > 1500.0 && p.0 < 2500.0 && (p.1.abs() - half).abs() < 20.0).collect();
        assert!(stray.is_empty(), "{detail}: the wall's lines cross the opening: {stray:?}");
        // And the jambs are the wall's own, at the opening's two ends.
        for x in [1495.0, 2505.0] {
            assert!(across.iter().any(|p| (p.0 - x).abs() < 1.0 && (p.1.abs() - half).abs() < 1.0), "{detail}: a jamb at {x}");
        }
    }
}

#[test]
fn a_screens_lines_do_not_thicken_as_it_zooms_in() {
    // Revit keeps a line's width at its weight's for the view's scale whatever the zoom: zooming in makes the drawing bigger, not
    // its lines heavier. The screen's plan at a tenth of a pixel to the mm and at four times that draws the wall's cut outline as
    // wide as each other, and as wide as pen 10 is at 1 : 100 (0.35 mm, 5 pixels to a mm of paper).
    let mut s = Session::new();
    let wall = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]})));
    let widest = |scale: f64| {
        let view = crate::render::View { scale, cx: 2000.0, cy: 0.0, width: 1000, height: 800 };
        let d = render::draw(&s.model, 100000, view);
        let width = strokes(&d, wall).into_iter().filter(|(_, dashed, ..)| !dashed).map(|(w, ..)| w).fold(0.0, f32::max);
        (width, d.width(Pen(10)))
    };
    let ((far, pen_far), (near, pen_near)) = (widest(0.1), widest(0.4));
    assert!(far > 0.0, "the wall is drawn");
    assert!((far - near).abs() < 1e-4, "{far} px zoomed out, {near} px zoomed in");
    assert!((far - pen_far).abs() < 1e-4 && (near - pen_near).abs() < 1e-4, "each is its pen's width: {far} {pen_far}, {near} {pen_near}");
    assert!((pen_far - pen_near).abs() < 1e-4, "a pen's width is the same at every zoom of a screen");
}

//! The site and masterplan's elements driven through their commands (docs/areas/site.md): placed and shaped, edited and
//! moved and shaped again with what they meet, checked, drawn in plan and 3D, written to IFC and read back from a file.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::render::{self, Prim, Rgb};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> Id {
    serde_json::from_value(v["id"].clone()).unwrap_or_else(|_| panic!("no id in {v}"))
}

fn site(s: &Session, id: Id) -> crate::masterplan::Site {
    match s.model.elements.get(id) {
        Some(Element::Site(st)) => (**st).clone(),
        other => panic!("{id} is {other:?}"),
    }
}

fn outline_area(st: &crate::masterplan::Site) -> f64 {
    st.outline.iter().map(|p| crate::geom::area(&p.outer).abs()).sum()
}

fn level(s: &Session) -> Id {
    s.model.levels()[0].0
}

/// A plan of the first level, 800 px square round what it shows, and its PNG.
fn plan(s: &Session) -> (render::Drawing, Vec<u8>) {
    let d = render::plan(&s.model, level(s), 800, 800);
    let png = render::png(&d).unwrap();
    (d, png)
}

fn pixel(d: &render::Drawing, png: &[u8], at: P2) -> Rgb {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let (x, y) = d.view.px(at);
    let c = pm.pixel(x as u32, y as u32).unwrap();
    Rgb(c.red(), c.green(), c.blue())
}

fn lane(s: &mut Session, path: Value, use_: &str) -> Id {
    id(&run(s, "site", json!({"kind": "lane", "use": use_, "props": {"path": path}})))
}

#[test]
fn a_lane_is_placed_and_shaped_its_width_filled_in_and_a_prop_taken_back_by_null() {
    let mut s = Session::new();
    let r = run(&mut s, "site", json!({"kind": "lane", "props": {"path": [[0, 0], [20000, 0]]}}));
    let lane = id(&r);
    assert_eq!(r["filled"]["width"], json!(5500.0), "{r}");
    let st = site(&s, lane);
    assert_eq!(st.use_, "lane", "its kind's first use");
    assert!((outline_area(&st) - 20000.0 * 5500.0).abs() < 1e3, "a band 5.5 m wide: {}", outline_area(&st));
    assert!(st.parts.iter().any(|p| p.role == "path"));
    // Edited, it is shaped again; null takes a prop back to the figure.
    run(&mut s, "site_edit", json!({"id": lane, "props": {"width": 3000}}));
    assert!((outline_area(&site(&s, lane)) - 20000.0 * 3000.0).abs() < 1e3);
    let r = run(&mut s, "site_edit", json!({"id": lane, "props": {"width": null}}));
    assert_eq!(r["filled"]["width"], json!(5500.0), "{r}");
    assert!((outline_area(&site(&s, lane)) - 20000.0 * 5500.0).abs() < 1e3);
    // One undo step each.
    assert_eq!(s.history(), ["Site Element", "Edit Site Element", "Edit Site Element"]);
    run(&mut s, "undo", json!({}));
    assert!((outline_area(&site(&s, lane)) - 20000.0 * 3000.0).abs() < 1e3);
}

#[test]
fn what_cant_be_shaped_or_isnt_in_the_catalogue_is_refused_saying_what_to_fix() {
    let mut s = Session::new();
    for (p, says) in [
        (json!({"kind": "lane", "props": {}}), "path"),
        (json!({"kind": "castle", "props": {}}), "one of"),
        (json!({"kind": "lane", "use": "motorway", "props": {"path": [[0, 0], [1000, 0]]}}), "motorway"),
        (json!({"kind": "lane", "props": {"path": [[0, 0], [1000, 0]], "colour": "red"}}), "colour"),
        (json!({"kind": "lane", "props": {"path": [[0, 0], [1e300, 0]]}}), "path"),
        (json!({"kind": "tree", "props": {"at": [0, 0], "crown": "big"}}), "crown"),
        (json!({"kind": "tree", "phase": "ancient", "props": {"at": [0, 0]}}), "phase"),
        (json!({"kind": "pitch", "props": {"at": [0, 0]}, "colour": 1}), "colour"),
    ] {
        let e = s.execute("site", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Site(_))).count(), 0);
    // A file's element that breaks the rules is refused by the check too.
    let tree = id(&run(&mut s, "site", json!({"kind": "tree", "props": {"at": [0, 0]}})));
    let mut m = (*s.model).clone();
    if let Some(Element::Site(st)) = m.elements.get_mut(tree) {
        st.props.insert("height".into(), json!(f64::MAX));
    }
    assert!(m.check().unwrap_err().contains("height"));
}

#[test]
fn roads_that_meet_are_worked_out_together_and_again_when_one_moves_away() {
    let mut s = Session::new();
    let a = lane(&mut s, json!([[-30000, 0], [30000, 0]]), "road");
    let own = site(&s, a).parts;
    let b = lane(&mut s, json!([[0, 0], [0, 30000]]), "road");
    let met = site(&s, a);
    assert!(met.parts.iter().any(|p| p.role == "carriageway") && met.parts.iter().any(|p| p.role == "kerb"));
    // Its kerb opens where the other road comes in.
    let kerbs = |parts: &[crate::site::shapes::Part]| parts.iter().filter(|p| p.role == "kerb").map(|p| p.lines.len()).sum::<usize>();
    assert!(kerbs(&met.parts) > kerbs(&own), "{} → {}", kerbs(&own), kerbs(&met.parts));
    let with = met.parts.clone();
    // Moved away, the other road's junction goes.
    run(&mut s, "move", json!({"ids": [b], "by": [0, 100000]}));
    let moved = site(&s, b);
    assert_eq!(moved.props["path"], json!([[0.0, 100000.0], [0.0, 130000.0]]), "its truth moved");
    assert_eq!(site(&s, a).parts, own, "the junction gone");
    // Moved back, they meet again.
    run(&mut s, "move", json!({"ids": [b], "by": [0, -100000]}));
    assert_eq!(site(&s, a).parts, with);
    // Deleted, the road it met is shaped again on its own.
    run(&mut s, "delete", json!({"ids": [b]}));
    assert_eq!(site(&s, a).parts, own);
}

#[test]
fn the_modify_tools_move_turn_and_mirror_a_site_elements_truth() {
    let mut s = Session::new();
    let pitch = id(&run(&mut s, "site", json!({"kind": "pitch", "use": "tennis", "props": {"at": [10000, 0]}})));
    run(&mut s, "rotate", json!({"ids": [pitch], "about": [0, 0], "angle": 90}));
    let st = site(&s, pitch);
    let at = st.props["at"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert!(at[0].abs() < 0.01 && (at[1] - 10000.0).abs() < 0.01, "{at:?}");
    assert!((st.props["angle"].as_f64().unwrap() - 90.0).abs() < 1e-6, "{}", st.props["angle"]);
    // The outline turned with it: long north-south now.
    let ys: Vec<f64> = st.outline[0].outer.iter().map(|p| p.1).collect();
    let span = ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min);
    assert!((span - 23770.0).abs() < 1.0, "{span}");
    // A copy is an element of its own, shaped.
    let before = s.model.elements.len();
    run(&mut s, "copy", json!({"ids": [pitch], "by": [50000, 0]}));
    assert_eq!(s.model.elements.len(), before + 1);
    // A parking band's sides go to the other side of a mirror.
    let band = id(&run(&mut s, "site", json!({"kind": "parking_band", "props": {"path": [[0, -30000], [30000, -30000]], "sides": "left"}})));
    run(&mut s, "mirror", json!({"ids": [band], "axis": [[0, -40000], [1000, -40000]]}));
    assert_eq!(site(&s, band).props["sides"], json!("right"));
    // A lane's arc bulges the other way once mirrored.
    let curve = lane(&mut s, json!([[0, 50000], [10000, 50000, 0.5], [20000, 60000]]), "lane");
    run(&mut s, "mirror", json!({"ids": [curve], "axis": [[0, 0], [1000, 0]]}));
    assert_eq!(site(&s, curve).props["path"][1], json!([10000.0, -50000.0, -0.5]));
}

#[test]
fn a_car_park_is_laid_out_its_pieces_carrying_its_band_and_laid_out_again_round_a_building() {
    let mut s = Session::new();
    let ring = json!([[0, 0], [60000, 0], [60000, 40000], [0, 40000]]);
    let r = run(&mut s, "site", json!({"kind": "car_park", "props": {"control": ring}}));
    let cp = id(&r);
    let st = site(&s, cp);
    let layout = st.layout.clone().expect("laid out");
    let stalls = layout.result["stalls"].as_u64().unwrap();
    assert!(stalls > 50, "{:?}", layout.result);
    assert_eq!(r["element"]["stalls"], json!(stalls));
    let guid = s.model.guid(cp).to_string();
    assert!(layout.pieces.iter().all(|p| p.props["band"] == json!(guid)));
    assert!(layout.pieces.iter().any(|p| p.kind == "aisle") && layout.pieces.iter().any(|p| p.kind == "stall"));
    // A building put on it: laid out round it, fewer stalls.
    run(&mut s, "mass", json!({"boundary": [[20000, 10000], [40000, 10000], [40000, 30000], [20000, 30000]], "height": 6000}));
    let fewer = site(&s, cp).layout.unwrap().result["stalls"].as_u64().unwrap();
    assert!(fewer < stalls, "{fewer} < {stalls}");
    // Its plan draws its stalls.
    let (d, _) = plan(&s);
    let fills = d.prims.iter().zip(&d.owners).filter(|(p, o)| matches!(p, Prim::Fill { .. }) && **o == Some(cp)).count();
    assert!(fills as u64 > fewer, "{fills}");
}

#[test]
fn a_parking_stall_is_one_bay_from_the_figures_at_a_point_and_an_angle() {
    let mut s = Session::new();
    let r = run(&mut s, "site", json!({"kind": "parking_stall", "use": "accessible", "props": {"at": [0, 0], "angle": 0}}));
    let st = site(&s, id(&r));
    assert_eq!(r["filled"], json!({"width": 2500.0, "depth": 5000.0}));
    assert!((outline_area(&st) - 2500.0 * 5000.0).abs() < 1.0);
    let xs: Vec<f64> = st.outline[0].outer.iter().map(|p| p.0).collect();
    assert!((xs.iter().cloned().fold(f64::MIN, f64::max) - 2500.0).abs() < 0.01, "its depth along its angle: {xs:?}");
    assert!(st.parts.iter().any(|p| p.role == "accessible_stall") && st.parts.iter().any(|p| p.role == "mouth"));
    let kinds = run(&mut s, "site_kinds", json!({}));
    assert_eq!(kinds["kinds"]["parking_stall"]["revit"], json!("Parking Component"));
    assert_eq!(kinds["kinds"]["parking_stall"]["props"]["depth"], json!("length"));
}

#[test]
fn a_site_elements_phase_is_its_phase_created_and_demolished_and_the_demolish_tool_reaches_it() {
    let mut s = Session::new();
    let tree = id(&run(&mut s, "site", json!({"kind": "tree", "phase": "existing", "source": "osm", "props": {"at": [0, 0]}})));
    let st = site(&s, tree);
    assert_eq!(st.params["Phase Created"], json!("Existing"));
    assert!(st.context(), "existing and fetched: context");
    run(&mut s, "site_edit", json!({"id": tree, "phase": "demolished"}));
    let st = site(&s, tree);
    assert_eq!((st.params["Phase Created"].clone(), st.params["Phase Demolished"].clone()), (json!("Existing"), json!("New Construction")));
    // Its params changed with the Demolish tool's `modify`: its phase follows them.
    let lawn =
        id(&run(&mut s, "site", json!({"kind": "landscape", "use": "lawn", "phase": "existing", "props": {"control": [[0, 0], [9000, 0], [9000, 9000]]}})));
    run(&mut s, "modify", json!({"id": lawn, "params": {"Phase Demolished": "New Construction"}}));
    assert_eq!(site(&s, lawn).phase, "demolished");
    // A phase filter hides it as any element: Show New leaves out what stood before.
    let shows = |s: &Session, id: Id| crate::graphics::Look::of(&s.model, level(s)).shows(id);
    assert!(shows(&s, tree));
    let view = level(&s);
    run(&mut s, "graphics", json!({"view": view, "phase": "New Construction", "phase_filter": "Show New"}));
    assert!(!shows(&s, tree) && !shows(&s, lawn));
}

#[test]
fn site_figures_are_the_projects_own_and_what_is_drawn_next_takes_them() {
    let mut s = Session::new();
    let r = run(&mut s, "site_figures", json!({"values": {"lane.width": 4000}}));
    assert_eq!(r["figures"]["lane.width"], json!({"value": 4000.0, "type": "length", "own": true}));
    let r = run(&mut s, "site", json!({"kind": "lane", "props": {"path": [[0, 0], [10000, 0]]}}));
    assert_eq!(r["filled"]["width"], json!(4000.0));
    assert!(s.execute("site_figures", &json!({"values": {"lane.colour": 1}})).unwrap_err().contains("isn't a figure"));
    assert!(s.execute("site_figures", &json!({"values": {"lane.width": -5}})).is_err());
    run(&mut s, "site_figures", json!({"values": {"lane.width": null}}));
    assert!(s.model.site_figures.is_empty());
    // Kept in the file.
    run(&mut s, "site_figures", json!({"values": {"tree.crown": 8000}}));
    let (m, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(m.site_figures.get("tree.crown"), Some(&8000.0));
}

#[test]
fn a_mass_on_the_masterplan_takes_its_height_from_its_storeys() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "mass",
        json!({"boundary": [[0, 0], [10000, 0], [10000, 8000], [0, 8000]], "site": {"use": "housing", "phase": "existing", "source": "overture",
               "ref": "building:overture:17", "props": {"storeys": 3, "roof": "flat", "label": "B6"}}}),
    );
    let ms = id(&r);
    assert_eq!(r["height"], json!(9000.0), "3 × the 3 m figure: {r}");
    let e = run(&mut s, "site_edit", json!({"id": ms, "props": {"storeys": 4, "floor_height": 3500}}));
    assert_eq!(e["height"], json!(14000.0));
    assert_eq!(e["site"]["props"]["label"], json!("B6"));
    assert_eq!(e["site"]["phase"], json!("existing"));
    assert!(s.execute("site_edit", &json!({"id": ms, "use": "castle"})).unwrap_err().contains("castle"));
    assert!(s.execute("mass", &json!({"boundary": [[0, 0], [1, 0], [1, 1]], "site": {"props": {"towers": 3}}})).unwrap_err().contains("towers"));
    // Its entrance moves with it.
    run(&mut s, "site_edit", json!({"id": ms, "props": {"entrance": [5000, 0]}}));
    run(&mut s, "move", json!({"ids": [ms], "by": [1000, 2000]}));
    let Some(Element::Mass(m)) = s.model.elements.get(ms) else { panic!() };
    assert_eq!(m.site.as_ref().unwrap().props["entrance"], json!([6000.0, 2000.0]));
}

#[test]
fn the_plan_fills_each_area_in_its_surfaces_colour_and_picks_it() {
    let mut s = Session::new();
    let lawn = id(&run(&mut s, "site", json!({"kind": "landscape", "use": "lawn", "props": {"control": [[0, 0], [20000, 0], [20000, 20000], [0, 20000]]}})));
    let pond = id(&run(&mut s, "site", json!({"kind": "water", "props": {"control": [[30000, 0], [50000, 0], [50000, 20000], [30000, 20000]]}})));
    let road = lane(&mut s, json!([[-10000, 30000], [60000, 30000]]), "road");
    let context = id(&run(
        &mut s,
        "site",
        json!({"kind": "landscape", "use": "lawn", "phase": "existing", "source": "osm", "props": {"control": [[0, 40000], [20000, 40000], [20000, 60000], [0, 60000]]}}),
    ));
    let (d, png) = plan(&s);
    assert_eq!(pixel(&d, &png, P2(10000.0, 10000.0)), Rgb(214, 235, 198), "a lawn's green");
    assert_eq!(pixel(&d, &png, P2(40000.0, 10000.0)), Rgb(207, 230, 243), "water's blue");
    assert_eq!(pixel(&d, &png, P2(25000.0, 30000.0 + 2000.0)), Rgb(164, 170, 178), "a carriageway's asphalt");
    let halftone = pixel(&d, &png, P2(10000.0, 50000.0));
    assert!(halftone.1 > 235 && halftone != Rgb(214, 235, 198), "existing context halftone: {halftone:?}");
    // Its kerbs and markings: lines of the road's.
    let road_lines = d.prims.iter().zip(&d.owners).filter(|(p, o)| matches!(p, Prim::Line { .. }) && **o == Some(road)).count();
    assert!(road_lines >= 2, "{road_lines}");
    for (at, want) in [(P2(10000.0, 10000.0), lawn), (P2(40000.0, 10000.0), pond), (P2(10000.0, 50000.0), context)] {
        assert_eq!(d.pick(d.view.px(at), 3.0), Some(want));
    }
}

#[test]
fn trees_draw_their_crowns_and_what_comes_down_is_dashed() {
    let mut s = Session::new();
    let tree = id(&run(&mut s, "site", json!({"kind": "tree", "props": {"at": [0, 0], "crown": 8000}})));
    let gone = id(&run(&mut s, "site", json!({"kind": "paving", "phase": "demolished", "props": {"control": [[10000, 0], [20000, 0], [20000, 10000], [10000, 10000]]}})));
    let (d, png) = plan(&s);
    assert_eq!(pixel(&d, &png, P2(2000.0, 0.0)), Rgb(169, 209, 142), "a crown's green");
    let dashed = d.prims.iter().zip(&d.owners).any(|(p, o)| matches!(p, Prim::Line { dash: Some(_), .. }) && *o == Some(gone));
    assert!(dashed);
    let solid = d.prims.iter().zip(&d.owners).all(|(p, o)| !matches!(p, Prim::Line { dash: Some(_), .. }) || *o != Some(tree));
    assert!(solid);
}

#[test]
fn a_property_line_closes_its_points_measures_its_arcs_and_draws_dash_dot() {
    let mut s = Session::new();
    let r = run(&mut s, "property_line", json!({"points": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]], "name": "Plot 4"}));
    assert_eq!(r["area_m2"], json!(100.0));
    let pl = id(&r);
    // Given clockwise, it is run round the other way; a half circle out of one side adds its half disc (a bulge to the
    // right of the way it runs, clockwise: negative).
    let half = run(&mut s, "property_line", json!({"points": [[0, 20000], [0, 30000], [10000, 30000], [10000, 20000, -1.0]]}));
    let want = 100.0 + std::f64::consts::PI * 25.0 / 2.0;
    assert!((half["area_m2"].as_f64().unwrap() - want).abs() < 0.01, "{half} vs {want}");
    assert!(s.execute("property_line", &json!({"points": [[0, 0], [1000, 0]]})).is_err());
    let (d, _) = plan(&s);
    let dash = d.prims.iter().zip(&d.owners).find_map(|(p, o)| match p {
        Prim::Line { dash: Some(dash), .. } if *o == Some(pl) => Some(dash.len()),
        _ => None,
    });
    assert_eq!(dash, Some(4), "dash-dot");
    let e = run(&mut s, "elements", json!({"category": "propertyline"}));
    assert_eq!(e["elements"].as_array().map(Vec::len), Some(2));
}

/// A toposolid 40 m square rising 4 m from west to east, contours a metre apart, and a pad 10 m square in its middle.
fn ground(s: &mut Session) -> (Id, Id) {
    let topo = id(&run(
        s,
        "toposolid",
        json!({"boundary": [[0, 0], [40000, 0], [40000, 40000], [0, 40000]],
               "points": [[0, 0, 0], [0, 40000, 0], [40000, 0, 4000], [40000, 40000, 4000]], "contours": 1000}),
    ));
    let pad = run(s, "building_pad", json!({"boundary": [[15000, 15000], [25000, 15000], [25000, 25000], [15000, 25000]], "offset": 1500}));
    assert_eq!(pad["cuts"], json!([topo]));
    (topo, id(&pad))
}

#[test]
fn a_toposolid_draws_its_contours_and_a_building_pad_cuts_them_in_plan_and_3d() {
    let mut s = Session::new();
    let topo = id(&run(
        &mut s,
        "toposolid",
        json!({"boundary": [[0, 0], [40000, 0], [40000, 40000], [0, 40000]],
               "points": [[0, 0, 0], [0, 40000, 0], [40000, 0, 4000], [40000, 40000, 4000]], "contours": 1000}),
    ));
    assert!(matches!(s.model.elements.get(topo), Some(Element::Toposolid(_))));
    // Its contours: lines north-south at 10, 20 and 30 m, each crossing the whole of it.
    let contour_lines = |s: &Session| -> Vec<[f32; 4]> {
        let d = render::plan(&s.model, level(s), 800, 800);
        d.prims
            .iter()
            .zip(&d.owners)
            .zip(&d.parts)
            .filter_map(|((p, o), part)| match p {
                Prim::Line { pts, .. } if *o == Some(topo) && *part != render::Part::Pattern && pts.len() == 2 && (pts[0].0 - pts[1].0).abs() < 0.5 => {
                    let m0 = d.view.mm(pts[0]);
                    let m1 = d.view.mm(pts[1]);
                    Some([m0.0 as f32, m0.1 as f32, m1.0 as f32, m1.1 as f32])
                }
                _ => None,
            })
            .collect()
    };
    let whole = contour_lines(&s);
    let at_20m = |ls: &[[f32; 4]]| ls.iter().filter(|l| (l[0] - 20000.0).abs() < 100.0).map(|l| (l[1] - l[3]).abs()).sum::<f32>();
    assert!((at_20m(&whole) - 40000.0).abs() < 200.0, "the 2 m contour runs right across: {whole:?}");
    // The pad: the contour stops at its edge.
    let pad = run(&mut s, "building_pad", json!({"boundary": [[15000, 15000], [25000, 15000], [25000, 25000], [15000, 25000]], "offset": 1500}));
    assert_eq!(pad["cuts"], json!([topo]));
    let pad = id(&pad);
    let cut = contour_lines(&s);
    assert!((at_20m(&cut) - 30000.0).abs() < 200.0, "less the pad's 10 m: {cut:?}");
    // In 3D: the pad a slab at its height, 300 mm deep; the toposolid's top open over it.
    let solids = crate::view3d::solids(&s.model);
    let pad_solids: Vec<_> = solids.iter().filter(|x| x.owner() == pad).collect();
    assert_eq!(pad_solids.len(), 1);
    assert_eq!(s.model.pad_span(match s.model.elements.get(pad) {
        Some(Element::BuildingPad(p)) => p,
        _ => panic!(),
    }), (1200.0, 1500.0));
    let Some(Element::Toposolid(t)) = s.model.elements.get(topo) else { panic!() };
    let cut = s.model.toposolid_cut(t);
    assert_eq!(cut.openings.len(), 1);
    // The ground round the pad keeps its height: its corners are on the slope.
    let corner = cut.shape.points.iter().find(|p| p.at == P2(25000.0, 25000.0)).unwrap();
    assert!((corner.z - 2500.0).abs() < 1.0, "{}", corner.z);
    // Moved off the toposolid, it cuts nothing.
    run(&mut s, "move", json!({"ids": [pad], "by": [100000, 0]}));
    let Some(Element::Toposolid(t)) = s.model.elements.get(topo) else { panic!() };
    assert!(s.model.toposolid_cut(t).openings.is_empty());
}

#[test]
fn the_3d_view_builds_areas_as_thin_slabs_and_trees_a_trunk_and_a_crown() {
    let mut s = Session::new();
    let tree = id(&run(&mut s, "site", json!({"kind": "tree", "props": {"at": [0, 0], "height": 10000}})));
    let lawn = id(&run(&mut s, "site", json!({"kind": "landscape", "use": "lawn", "props": {"control": [[0, 0], [9000, 0], [9000, 9000]]}})));
    let road = lane(&mut s, json!([[-10000, 20000], [10000, 20000]]), "road");
    let st = site(&s, tree);
    let pieces = crate::masterplan::solids(&s.model, &st);
    assert_eq!(pieces.len(), 2, "a trunk and a crown");
    assert!(pieces.iter().any(|(_, lo, hi, _)| *lo == 3500.0 && *hi == 10000.0), "the crown over the trunk");
    let solids = crate::view3d::solids(&s.model);
    for want in [tree, lawn, road] {
        assert!(solids.iter().any(|x| x.owner() == want), "{want}");
    }
    let lawn_piece = crate::masterplan::solids(&s.model, &site(&s, lawn));
    assert_eq!(lawn_piece.iter().map(|(_, lo, hi, _)| hi - lo).collect::<Vec<_>>(), [20.0]);
    // Seen from above, its green.
    let scene = crate::view3d::Scene::new(&s.model);
    let cam = scene.fit(0.0, 89.0, 200, 200);
    let img = scene.render(&cam, &[]);
    let greens = (0..200).flat_map(|x| (0..200).map(move |y| (x, y))).filter(|&(x, y)| {
        let c = img.color(f64::from(x) + 0.5, f64::from(y) + 0.5);
        i32::from(c.1) > i32::from(c.0) + 20 && i32::from(c.1) > i32::from(c.2) + 20
    });
    assert!(greens.count() > 50);
}

#[test]
fn site_elements_go_out_to_ifc_as_geographic_civil_and_virtual_elements() {
    let mut s = Session::new();
    ground(&mut s);
    run(&mut s, "site", json!({"kind": "tree", "props": {"at": [5000, 5000]}}));
    run(&mut s, "site", json!({"kind": "paving", "props": {"control": [[0, 0], [5000, 0], [5000, 5000]]}}));
    run(&mut s, "property_line", json!({"points": [[0, 0], [40000, 0], [40000, 40000], [0, 40000]]}));
    let f = crate::ifc::export(&s.model);
    assert!(f.contains(".TERRAIN.)"), "the toposolid");
    assert!(f.contains("IFCSLAB(") && f.contains(".BASESLAB.)"), "the pad");
    assert!(f.contains("IFCGEOGRAPHICELEMENT(") && f.contains("'tree: tree'"), "the tree");
    assert!(f.contains("IFCCIVILELEMENT(") && f.contains("'paving: forecourt'"), "the paving");
    assert!(f.contains("'PropertyLine'"), "the property line");
}

#[test]
fn a_file_keeps_site_elements_as_the_doc_says_and_an_older_reader_leaves_out_what_it_doesnt_know() {
    let mut s = Session::new();
    ground(&mut s);
    let road = lane(&mut s, json!([[-10000, -4000], [79950, -4000]]), "road");
    run(&mut s, "site_edit", json!({"id": road, "name": "Street", "phase": "existing", "source": "site", "ref": "street:edge 0"}));
    run(&mut s, "site", json!({"kind": "car_park", "props": {"control": [[0, 50000], [40000, 50000], [40000, 80000], [0, 80000]]}}));
    run(&mut s, "property_line", json!({"points": [[0, 0], [10000, 0], [10000, 10000, 0.2], [0, 10000]]}));
    let text = s.model.to_json().unwrap();
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.elements.len(), s.model.elements.len());
    for (a, b) in m.elements.values().zip(s.model.elements.values()) {
        assert_eq!(a, b);
    }
    // As the doc's JSON has it.
    let v: Value = serde_json::from_str(&text).unwrap();
    let elements = v["elements"].as_object().map(|o| o.values().cloned().collect::<Vec<_>>()).or_else(|| v["elements"].as_array().cloned()).unwrap();
    let street = elements.iter().find(|e| e["category"] == "site" && e["kind"] == "lane").unwrap();
    assert_eq!((street["use"].clone(), street["phase"].clone(), street["ref"].clone()), (json!("road"), json!("existing"), json!("street:edge 0")));
    assert!(street["outline"][0]["outer"].is_array() && street["parts"].as_array().is_some_and(|p| p.iter().any(|p| p["role"] == "carriageway")));
    assert!(elements.iter().any(|e| e["category"] == "site" && e["layout"]["pieces"].is_array()));
    assert!(elements.iter().any(|e| e["category"] == "toposolid") && elements.iter().any(|e| e["category"] == "buildingpad"));
    // A key of a newer version inside a site element is left out and named.
    let newer = text.replacen("\"kind\": \"lane\"", "\"kind\": \"lane\", \"glow\": 1", 1);
    assert_ne!(newer, text);
    let (_, left_out) = Model::read(&newer).unwrap();
    assert!(left_out.iter().any(|k| k.contains("glow")), "{left_out:?}");
}

#[test]
fn a_site_schedule_lists_its_elements_by_kind_use_and_area() {
    let mut s = Session::new();
    run(&mut s, "site", json!({"kind": "landscape", "use": "meadow", "props": {"control": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]]}}));
    run(&mut s, "site", json!({"kind": "tree", "props": {"at": [0, 0]}}));
    let r = run(&mut s, "schedule", json!({"name": "Site Schedule", "category": "site"}));
    let text = r.to_string();
    assert!(text.contains("meadow") && text.contains("tree"), "{text}");
}

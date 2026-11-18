//! The site and masterplan's elements driven through their commands (docs/areas/site.md): placed and shaped, edited and
//! moved and shaped again with what they meet, checked, drawn in plan and 3D, written to IFC and read back from a file.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::prim::Prim;
use crate::render;

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

fn level(s: &Session) -> Id {
    s.model.levels()[0].0
}

/// A plan of the first level, 800 px square round what it shows, and its PNG.
fn plan(s: &Session) -> (render::Drawing, Vec<u8>) {
    let d = render::plan(&s.model, level(s), 800, 800);
    let png = render::png(&d).unwrap();
    (d, png)
}

/// A small car park's area: a site element to try the masterplan's rules on.
const SMALL: [[f64; 2]; 3] = [[0.0, 0.0], [5000.0, 0.0], [5000.0, 5000.0]];

#[test]
fn what_cant_be_shaped_or_isnt_in_the_catalogue_is_refused_saying_what_to_fix() {
    let mut s = Session::new();
    for (p, says) in [
        (json!({"kind": "castle", "props": {}}), "one of"),
        (json!({"kind": "car_park", "use": "valet", "props": {"control": SMALL}}), "valet"),
        (json!({"kind": "car_park", "props": {"control": SMALL, "colour": "red"}}), "colour"),
        (json!({"kind": "car_park", "props": {"control": [[0, 0], [1e300, 0], [0, 1000]]}}), "control"),
        (json!({"kind": "car_park", "phase": "ancient", "props": {"control": SMALL}}), "phase"),
        (json!({"kind": "car_park", "props": {"control": SMALL}, "colour": 1}), "colour"),
        // Ramps and parking are Revit's now.
        (json!({"kind": "parking_stall", "props": {"at": [0, 0]}}), "parking_component"),
        // What Revit makes as a sub-division, planting or a site component is made so now.
        (json!({"kind": "lane", "props": {"path": [[0, 0], [1000, 0]]}}), "road"),
        (json!({"kind": "tree", "props": {"at": [0, 0]}}), "planting"),
        (json!({"kind": "paving", "props": {"control": [[0, 0], [1000, 0], [0, 1000]]}}), "sub_division"),
    ] {
        let e = s.execute("site", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Site(_))).count(), 0);
    // A file's element that breaks the rules is refused by the check too.
    let stall = id(&run(&mut s, "site", json!({"kind": "car_park", "props": {"control": SMALL}})));
    let mut m = (*s.model).clone();
    if let Some(Element::Site(st)) = m.elements.get_mut(stall) {
        st.props.insert("stall_width".into(), json!(f64::MAX));
    }
    assert!(m.check().unwrap_err().contains("stall_width"));
}

#[test]
fn the_modify_tools_move_turn_and_mirror_a_site_elements_truth() {
    let mut s = Session::new();
    let stall = id(&run(
        &mut s,
        "site",
        json!({"kind": "car_park", "props": {"control": [[10000, 0], [15000, 0], [15000, 5000]], "direction": 0}}),
    ));
    run(&mut s, "rotate", json!({"ids": [stall], "about": [0, 0], "angle": 90}));
    let st = site(&s, stall);
    let at = st.props["control"][0].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    assert!(at[0].abs() < 0.01 && (at[1] - 10000.0).abs() < 0.01, "{at:?}");
    assert!((st.props["direction"].as_f64().unwrap() - 90.0).abs() < 1e-6, "{}", st.props["direction"]);
    // A copy is an element of its own, shaped.
    let before = s.model.elements.values().filter(|e| matches!(e, Element::Site(_))).count();
    run(&mut s, "copy", json!({"ids": [stall], "by": [50000, 0]}));
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Site(_))).count(), before + 1);
}

#[test]
fn a_car_park_is_laid_out_as_parking_spaces_and_laid_out_again_round_a_building() {
    let mut s = Session::new();
    let ring = json!([[0, 0], [60000, 0], [60000, 40000], [0, 40000]]);
    let r = run(&mut s, "site", json!({"kind": "car_park", "props": {"control": ring}}));
    let cp = id(&r);
    let stalls = site(&s, cp).props["result"]["stalls"].as_u64().unwrap();
    assert!(stalls > 50, "{stalls}");
    assert_eq!(r["element"]["stalls"], json!(stalls));
    let guid = s.model.guid(cp).to_string();
    let own = |s: &Session| {
        s.model.elements.values().filter(|e| matches!(e, Element::Component(c) if c.params.get("car_park") == Some(&json!(guid)))).count()
    };
    assert_eq!(own(&s) as u64, stalls);
    // A building put on it: laid out round it, fewer stalls.
    run(&mut s, "mass", json!({"boundary": [[20000, 10000], [40000, 10000], [40000, 30000], [20000, 30000]], "height": 6000}));
    let fewer = site(&s, cp).props["result"]["stalls"].as_u64().unwrap();
    assert!(fewer < stalls, "{fewer} < {stalls}");
    assert_eq!(own(&s) as u64, fewer);
}

#[test]
fn a_site_elements_phase_is_its_phase_created_and_demolished_and_the_demolish_tool_reaches_it() {
    let mut s = Session::new();
    let stall = id(&run(&mut s, "site", json!({"kind": "car_park", "phase": "existing", "source": "osm", "props": {"control": SMALL}})));
    let st = site(&s, stall);
    assert_eq!(st.params["Phase Created"], json!("Existing"));
    assert!(st.context(), "existing and fetched: context");
    run(&mut s, "site_edit", json!({"id": stall, "phase": "demolished"}));
    let st = site(&s, stall);
    assert_eq!((st.params["Phase Created"].clone(), st.params["Phase Demolished"].clone()), (json!("Existing"), json!("New Construction")));
    // Its params changed with the Demolish tool's `modify`: its phase follows them.
    let other = id(&run(
        &mut s,
        "site",
        json!({"kind": "car_park", "phase": "existing", "props": {"control": [[9000, 0], [14000, 0], [14000, 5000]]}}),
    ));
    run(&mut s, "modify", json!({"id": other, "params": {"Phase Demolished": "New Construction"}}));
    assert_eq!(site(&s, other).phase, "demolished");
    // A phase filter hides it as any element: Show New leaves out what stood before.
    let shows = |s: &Session, id: Id| crate::graphics::Look::of(&s.model, level(s)).shows(id);
    assert!(shows(&s, stall));
    let view = level(&s);
    run(&mut s, "graphics", json!({"view": view, "phase": "New Construction", "phase_filter": "Show New"}));
    assert!(!shows(&s, stall) && !shows(&s, other));
}

#[test]
fn site_figures_are_the_projects_own_and_what_is_drawn_next_takes_them() {
    let mut s = Session::new();
    let r = run(&mut s, "site_figures", json!({"values": {"parking.bay_width": 2700}}));
    assert_eq!(r["figures"]["parking.bay_width"], json!({"value": 2700.0, "type": "length", "own": true}));
    assert!(s.execute("site_figures", &json!({"values": {"lane.colour": 1}})).unwrap_err().contains("isn't a figure"));
    assert!(s.execute("site_figures", &json!({"values": {"lane.width": -5}})).is_err());
    run(&mut s, "site_figures", json!({"values": {"parking.bay_width": null}}));
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
    assert!(
        s.execute("mass", &json!({"boundary": [[0, 0], [1, 0], [1, 1]], "site": {"props": {"towers": 3}}})).unwrap_err().contains("towers")
    );
    // Its entrance moves with it.
    run(&mut s, "site_edit", json!({"id": ms, "props": {"entrance": [5000, 0]}}));
    run(&mut s, "move", json!({"ids": [ms], "by": [1000, 2000]}));
    let Some(Element::Mass(m)) = s.model.elements.get(ms) else { panic!() };
    assert_eq!(m.site.as_ref().unwrap().props["entrance"], json!([6000.0, 2000.0]));
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
    let pad = id(&run(s, "floor", json!({"boundary": [[15000, 15000], [25000, 15000], [25000, 25000], [15000, 25000]], "offset": 1500})));
    run(s, "excavate", json!({"toposolid": topo, "by": [pad]}));
    (topo, pad)
}

#[test]
fn a_toposolid_draws_its_contours_and_a_floor_that_excavates_it_through_cuts_them_in_plan_and_3d() {
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
                Prim::Line { pts, .. }
                    if *o == Some(topo) && *part != render::Part::Pattern && pts.len() == 2 && (pts[0].0 - pts[1].0).abs() < 0.5 =>
                {
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
    // A floor 10 m square under the toposolid's bottom, excavating it: the contour stops at its edge.
    let pad = id(&run(
        &mut s,
        "floor",
        json!({"boundary": [[15000, 15000], [25000, 15000], [25000, 25000], [15000, 25000]], "offset": -1500, "type": "Generic - 200mm"}),
    ));
    run(&mut s, "excavate", json!({"toposolid": topo, "by": [pad]}));
    let cut = contour_lines(&s);
    assert!((at_20m(&cut) - 30000.0).abs() < 200.0, "less the floor's 10 m: {cut:?}");
    let Some(Element::Toposolid(t)) = s.model.elements.get(topo) else { panic!() };
    let cut = s.model.toposolid_cut(t);
    assert_eq!(cut.openings.len(), 1, "a hole through it");
    // Moved off the toposolid, it cuts nothing; deleted, it leaves the toposolid's list.
    run(&mut s, "move", json!({"ids": [pad], "by": [100000, 0]}));
    let Some(Element::Toposolid(t)) = s.model.elements.get(topo) else { panic!() };
    assert!(s.model.toposolid_cut(t).openings.is_empty());
    run(&mut s, "delete", json!({"ids": [pad]}));
    let Some(Element::Toposolid(t)) = s.model.elements.get(topo) else { panic!() };
    assert!(t.excavated_by.is_empty());
}

#[test]
fn site_elements_go_out_to_ifc_as_geographic_civil_and_virtual_elements() {
    let mut s = Session::new();
    ground(&mut s);
    run(&mut s, "planting", json!({"at": [5000, 5000]}));
    run(&mut s, "sub_division", json!({"boundary": [[1000, 1000], [5000, 1000], [5000, 5000]], "type": "Paving"}));
    run(&mut s, "sub_division", json!({"boundary": [[6000, 1000], [9000, 1000], [9000, 5000]], "type": "Grass", "role": "landscape/lawn"}));
    run(&mut s, "property_line", json!({"points": [[0, 0], [40000, 0], [40000, 40000], [0, 40000]]}));
    let f = crate::ifc::export(&s.model);
    assert!(f.contains(".TERRAIN.)"), "the toposolid");
    assert!(f.contains("IFCGEOGRAPHICELEMENT(") && f.contains("'Grass: landscape/lawn'"), "the lawn");
    assert!(f.contains("IFCCIVILELEMENT(") && f.contains("'Paving'"), "the paving");
    assert!(f.contains("'PropertyLine'"), "the property line");
}

#[test]
fn a_file_keeps_site_elements_as_the_doc_says_and_an_older_reader_leaves_out_what_it_doesnt_know() {
    let mut s = Session::new();
    ground(&mut s);
    let stall = id(&run(
        &mut s,
        "site",
        json!({"kind": "car_park", "props": {"control": [[0, 50000], [40000, 50000], [40000, 80000], [0, 80000]]}}),
    ));
    run(&mut s, "site_edit", json!({"id": stall, "name": "Bay", "phase": "existing", "source": "site", "ref": "street:edge 0"}));
    run(&mut s, "sub_division", json!({"boundary": [[1000, 1000], [5000, 1000], [5000, 5000]]}));
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
    let elements =
        v["elements"].as_object().map(|o| o.values().cloned().collect::<Vec<_>>()).or_else(|| v["elements"].as_array().cloned()).unwrap();
    let street = elements.iter().find(|e| e["category"] == "site" && e["kind"] == "car_park").unwrap();
    assert_eq!(
        (street["use"].clone(), street["phase"].clone(), street["ref"].clone()),
        (json!("parking"), json!("existing"), json!("street:edge 0"))
    );
    assert!(street["outline"][0]["outer"].is_array());
    assert!(street["props"]["result"]["stalls"].is_number(), "its result in its props, its bays elements");
    assert!(elements.iter().any(|e| e["category"] == "toposolid") && elements.iter().any(|e| e["category"] == "subdivision"));
    // A key of a newer version inside a site element is left out and named.
    let newer = text.replacen("\"kind\": \"car_park\"", "\"kind\": \"car_park\", \"glow\": 1", 1);
    assert_ne!(newer, text);
    let (_, left_out) = Model::read(&newer).unwrap();
    assert!(left_out.iter().any(|k| k.contains("glow")), "{left_out:?}");
}

#[test]
fn a_site_schedule_lists_its_elements_by_kind_use_and_area() {
    let mut s = Session::new();
    run(&mut s, "site", json!({"kind": "car_park", "props": {"control": SMALL}}));
    let r = run(&mut s, "schedule", json!({"name": "Site Schedule", "category": "site"}));
    let text = r.to_string();
    assert!(text.contains("parking") && text.contains("car_park"), "{text}");
}

#[test]
fn a_site_element_draws_in_its_own_levels_plan_only() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"name": "Deck", "elevation": 3000}));
    let up = id(&run(&mut s, "site", json!({"kind": "car_park", "level": "Deck", "props": {"control": SMALL}})));
    let down = id(&run(&mut s, "site", json!({"kind": "car_park", "props": {"control": SMALL}})));
    let owners = |l: Id| render::plan(&s.model, l, 200, 200).owners.into_iter().flatten().collect::<std::collections::BTreeSet<Id>>();
    let deck = s.model.levels().iter().find(|(_, l)| l.name == "Deck").map(|(id, _)| *id).unwrap();
    assert!(owners(level(&s)).contains(&down) && !owners(level(&s)).contains(&up));
    assert!(owners(deck).contains(&up) && !owners(deck).contains(&down));
}

#[test]
fn edit_boundary_redraws_a_toposolid_and_keeps_its_heights_inside() {
    let mut s = Session::new();
    let topo = id(&run(
        &mut s,
        "toposolid",
        json!({"boundary": [[0, 0], [40000, 0], [40000, 40000], [0, 40000]], "points": [[20000, 20000, 1500]]}),
    ));
    let lines = run(&mut s, "edit_boundary", json!({"id": topo}))["sketch"].clone();
    assert_eq!(lines.as_array().map(Vec::len), Some(4), "{lines}");
    run(
        &mut s,
        "edit_boundary",
        json!({"id": topo, "sketch": [[[0, 0], [30000, 0]], [[30000, 0], [30000, 30000]], [[30000, 30000], [0, 30000]], [[0, 30000], [0, 0]]]}),
    );
    let Some(Element::Toposolid(f)) = s.model.elements.get(topo) else { panic!("still a toposolid") };
    assert!((crate::geom::area(&f.boundary).abs() - 9.0e8).abs() < 1.0);
    assert_eq!(f.shape.points.iter().filter(|p| p.at == P2(20000.0, 20000.0)).count(), 1, "the point inside keeps its height");
    assert_eq!(f.sketch.len(), 4, "it keeps its sketch");
}

#[test]
fn site_edit_gives_a_plain_mass_its_masterplan_figures_new() {
    let mut s = Session::new();
    let mass = id(&run(&mut s, "mass", json!({"boundary": [[0, 0], [4000, 0], [4000, 3000], [0, 3000]], "height": 3000})));
    let r = run(&mut s, "site_edit", json!({"id": mass, "props": {"storeys": 4}}));
    assert_eq!(r["height"], json!(12000.0), "{r}");
    let Some(Element::Mass(ms)) = s.model.elements.get(mass) else { panic!() };
    assert_eq!(ms.site.as_ref().map(|x| x.phase.as_str()), Some("new"));
}

#[test]
fn a_site_element_a_tool_previews_is_drawn_as_its_plan_draws_it() {
    let mut s = Session::new();
    let stall = id(&run(&mut s, "site", json!({"kind": "car_park", "props": {"control": SMALL}})));
    let e = s.model.elements.get(stall).cloned().unwrap();
    let v = render::plan(&s.model, level(&s), 400, 400).view;
    assert!(!render::sketch(&s.model, v, s.model.next_id, &e).prims.is_empty(), "a car park's area");
}

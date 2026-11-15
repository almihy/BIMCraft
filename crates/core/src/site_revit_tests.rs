//! The site as Revit has it (docs/areas/site.md, docs/site-as-revit-plan.md), driven through its commands: a toposolid's
//! points, sub-divisions on it, excavation, a graded region's cut and fill, the property line's table and setbacks, the
//! survey point and project base point, planting and site components, and a file of before converted as it is read.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Id, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> Id {
    serde_json::from_value(v["id"].clone()).unwrap_or_else(|_| panic!("no id in {v}"))
}

/// A toposolid 40 m square rising 4 m from west to east.
fn slope(s: &mut Session) -> Id {
    id(&run(
        s,
        "toposolid",
        json!({"boundary": [[0, 0], [40000, 0], [40000, 40000], [0, 40000]],
               "points": [[0, 0, 0], [0, 40000, 0], [40000, 0, 4000], [40000, 40000, 4000]], "contours": 1000}),
    ))
}

fn topo(s: &Session, id: Id) -> crate::model::Floor {
    match s.model.elements.get(id) {
        Some(Element::Toposolid(f)) => f.clone(),
        other => panic!("{id} is {other:?}"),
    }
}

/// The height of toposolid `id`'s top at `q`, as what cuts it cuts it, mm.
fn ground(s: &Session, id: Id, q: P2) -> f64 {
    let f = s.model.toposolid_cut(&topo(s, id));
    crate::ground::Surface3::of(&s.model, &f).at(q).unwrap_or(f64::NAN)
}

#[test]
fn a_toposolid_takes_points_added_moved_and_removed_as_revits_modify_sub_elements() {
    let mut s = Session::new();
    let t = id(&run(&mut s, "toposolid", json!({"boundary": [[0, 0], [20000, 0], [20000, 20000], [0, 20000]]})));
    run(&mut s, "add_point", json!({"id": t, "at": [10000, 10000], "z": 2000}));
    assert!((ground(&s, t, P2(10000.0, 10000.0)) - 2000.0).abs() < 1.0);
    // The whole list at once.
    let r = run(&mut s, "edit_shape", json!({"id": t, "points": [{"at": [10000, 10000], "z": 3000}, {"at": [5000, 5000], "z": 1000}]}));
    assert!(r["faces"].as_u64().is_some_and(|n| n > 1), "{r}");
    assert_eq!(topo(&s, t).shape.points.len(), 2);
    let listed = run(&mut s, "edit_shape", json!({"id": t}));
    let pid = listed["points"][1]["id"].as_u64().unwrap();
    run(&mut s, "remove_point", json!({"id": t, "point": pid}));
    run(&mut s, "remove_point", json!({"id": t, "at": [10000, 10000]}));
    assert!(topo(&s, t).shape.points.is_empty());
    assert!(s.execute("remove_point", &json!({"id": t, "at": [1, 1]})).unwrap_err().contains("no such point"));
}

#[test]
fn a_sub_division_follows_its_hosts_surface_raised_or_sunk_by_its_offset_and_clipped_to_its_host() {
    let mut s = Session::new();
    let t = slope(&mut s);
    // Half on the toposolid, half off it: clipped to it, on the ground there.
    let r = run(&mut s, "sub_division", json!({"boundary": [[30000, 10000], [50000, 10000], [50000, 20000], [30000, 20000]], "type": "Asphalt"}));
    let road = id(&r);
    assert_eq!(r["host"], json!(t));
    assert!((r["area_m2"].as_f64().unwrap() - 100.0).abs() < 0.5, "{r}");
    let Some(Element::SubDivision(sd)) = s.model.elements.get(road).cloned() else { panic!() };
    let pieces = s.model.subdivision_pieces(&sd);
    assert!(!pieces.is_empty());
    for (ring, bottom, top) in &pieces {
        let q = crate::geom::centroid(ring);
        assert!((bottom.at(q) - q.0 / 10.0).abs() < 2.0, "on the ground: {} at {q:?}", bottom.at(q));
        assert!((top.at(q) - bottom.at(q) - crate::ground::SKIN).abs() < 1e-6);
    }
    // Raised 150: its top is 150 over the ground.
    run(&mut s, "modify", json!({"id": road, "offset": 150}));
    let Some(Element::SubDivision(sd)) = s.model.elements.get(road).cloned() else { panic!() };
    let (ring, bottom, top) = &s.model.subdivision_pieces(&sd)[0];
    let q = crate::geom::centroid(ring);
    assert!((top.at(q) - bottom.at(q) - 150.0).abs() < 1e-6);
    // A pond sunk 500: its host comes down that far under the ground within it, and stays outside.
    let pond = id(&run(&mut s, "sub_division", json!({"boundary": [[5000, 5000], [15000, 5000], [15000, 15000], [5000, 15000]], "type": "Water", "offset": -500})));
    assert!((ground(&s, t, P2(10000.0, 10000.0)) - (1000.0 - 500.0)).abs() < 2.0, "{}", ground(&s, t, P2(10000.0, 10000.0)));
    assert!((ground(&s, t, P2(20000.0, 30000.0)) - 2000.0).abs() < 2.0);
    // Drawn on the plan in its surface's colour; scheduled by type and area.
    let d = crate::render::plan(&s.model, s.model.levels()[0].0, 400, 400);
    assert!(d.owners.contains(&Some(pond)));
    let sched = run(&mut s, "schedule", json!({"name": "Sub-divisions", "category": "subdivision"})).to_string();
    assert!(sched.contains("Water") && sched.contains("Asphalt"), "{sched}");
}

#[test]
fn a_sub_division_moves_copies_and_goes_with_its_host() {
    let mut s = Session::new();
    let t = slope(&mut s);
    let lawn = id(&run(&mut s, "sub_division", json!({"boundary": [[1000, 1000], [5000, 1000], [5000, 5000], [1000, 5000]], "type": "Grass"})));
    run(&mut s, "move", json!({"ids": [t], "by": [100000, 0]}));
    let Some(Element::SubDivision(sd)) = s.model.elements.get(lawn) else { panic!() };
    assert_eq!(sd.boundary[0], P2(101000.0, 1000.0), "it went with its host");
    let r = run(&mut s, "copy", json!({"ids": [t], "by": [0, 100000]}));
    let copies: Vec<Id> = serde_json::from_value(r["ids"].clone()).unwrap();
    let copy_host = copies.iter().copied().find(|id| matches!(s.model.elements.get(*id), Some(Element::Toposolid(_)))).unwrap();
    let hosted: Vec<_> = s.model.subdivisions_of(s.model.guid(copy_host));
    assert_eq!(hosted.len(), 1, "the copy brings its sub-division");
    run(&mut s, "delete", json!({"ids": [t]}));
    assert!(s.model.elements.get(lawn).is_none(), "deleted with its host");
}

#[test]
fn a_floor_that_excavates_a_toposolid_cuts_it_down_to_its_bottom() {
    let mut s = Session::new();
    let t = slope(&mut s);
    // A basement floor 10 m square, its top at -200 and 200 thick: its bottom at -400, over the toposolid's bottom.
    let f = id(&run(&mut s, "floor", json!({"boundary": [[15000, 15000], [25000, 15000], [25000, 25000], [15000, 25000]], "offset": -200, "type": "Generic - 200mm"})));
    let r = run(&mut s, "excavate", json!({"toposolid": t, "by": [f]}));
    assert_eq!(r["excavated_by"], json!([f]), "{r}");
    assert!((ground(&s, t, P2(20000.0, 20000.0)) + 400.0).abs() < 2.0, "down to its bottom: {}", ground(&s, t, P2(20000.0, 20000.0)));
    assert!((ground(&s, t, P2(5000.0, 20000.0)) - 500.0).abs() < 2.0, "the ground round it as it was");
    // A wall can't excavate.
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}));
    let wid = w["ids"][0].as_u64().unwrap();
    assert!(s.execute("excavate", &json!({"toposolid": t, "by": [wid]})).unwrap_err().contains("doesn't excavate"));
    run(&mut s, "remove_excavation", json!({"toposolid": t}));
    assert!((ground(&s, t, P2(20000.0, 20000.0)) - 2000.0).abs() < 2.0);
}

#[test]
fn a_graded_region_copies_the_existing_ground_demolishes_it_and_works_out_cut_and_fill() {
    let mut s = Session::new();
    let existing = id(&run(&mut s, "toposolid", json!({"boundary": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]]})));
    let r = run(&mut s, "graded_region", json!({"toposolid": existing}));
    let new = id(&r);
    assert_eq!(r["cut_m3"], json!(0.0), "{r}");
    let old = s.model.elements.get(existing).and_then(Element::params).cloned().unwrap_or_default();
    assert_eq!(old.get("Phase Demolished"), Some(&json!("New Construction")), "the existing ground goes in the new phase");
    // Half of it raised a metre (fill), the rest lowered half a metre (cut).
    run(
        &mut s,
        "edit_shape",
        json!({"id": new, "points": [{"at": [0, 0], "z": 1000}, {"at": [0, 10000], "z": 1000}, {"at": [10000, 0], "z": -1000}, {"at": [10000, 10000], "z": -1000}]}),
    );
    let e = crate::json::element_json(&s.model, new);
    let (cut, fill) = (e["cut_m3"].as_f64().unwrap(), e["fill_m3"].as_f64().unwrap());
    assert!((cut - 25.0).abs() < 1.0 && (fill - 25.0).abs() < 1.0, "{cut} {fill}");
    // Taken again: the existing ground anew into the same toposolid, its sub-divisions kept.
    let lawn = id(&run(&mut s, "sub_division", json!({"boundary": [[1000, 1000], [3000, 1000], [3000, 3000]], "host": new, "type": "Grass"})));
    let again = run(&mut s, "graded_region", json!({"toposolid": existing, "replace": new}));
    assert_eq!(id(&again), new);
    assert!(topo(&s, new).shape.points.is_empty() && s.model.elements.get(lawn).is_some());
    // From a project given as its text.
    let text = s.model.to_json().unwrap();
    let mut other = Session::new();
    let r = run(&mut other, "graded_region", json!({"source": text, "toposolid": s.model.guid(existing).to_string()}));
    assert!(matches!(other.model.elements.get(id(&r)), Some(Element::Toposolid(f)) if f.graded_from.is_some()));
}

#[test]
fn a_property_line_from_a_table_of_bearings_and_distances_closes_and_reads_back() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "property_line",
        json!({"table": [{"distance": 30000, "bearing": "N 90°00'00\" E"}, {"distance": 20000, "bearing": "N0E"}, {"distance": 30000, "bearing": 270}]}),
    );
    assert!((r["area_m2"].as_f64().unwrap() - 600.0).abs() < 0.01, "{r}");
    assert_eq!(r["closure"]["distance"], json!(20000.0), "Add Line to Close");
    assert_eq!(r["closure"]["bearing"], json!("S 0°00'00\" E"));
    let pl = id(&r);
    let t = run(&mut s, "property_line_table", json!({"id": pl}));
    assert_eq!(t["table"].as_array().map(Vec::len), Some(4));
    assert_eq!(t["table"][0]["bearing"], json!("N 90°00'00\" E"));
    // An arc: a chord, a radius and a side.
    let r = run(
        &mut s,
        "property_line",
        json!({"table": [{"distance": 20000, "bearing": "N 90 E"}, {"distance": 20000, "bearing": "N 0 E", "type": "arc", "radius": 10000, "side": "L"}, {"distance": 20000, "bearing": "S 90 W"}]}),
    );
    let area = r["area_m2"].as_f64().unwrap();
    assert!((area - (400.0 + std::f64::consts::PI * 100.0 / 2.0)).abs() < 0.5, "a half circle on its side: {area}");
    let t = run(&mut s, "property_line_table", json!({"id": id(&r)}));
    assert!(t["table"].as_array().unwrap().iter().any(|row| row["type"] == "arc" && (row["radius"].as_f64().unwrap() - 10000.0).abs() < 1.0));
}

#[test]
fn setbacks_draw_the_buildable_line_each_edge_its_own_distance() {
    let mut s = Session::new();
    let r = run(
        &mut s,
        "property_line",
        json!({"points": [[0, 0], [30000, 0], [30000, 20000], [0, 20000]],
               "edges": [{"setback": 5000, "kind": "street", "street": "High Street", "street_width": 12000}, {"setback": 3000, "kind": "side"}, {"setback": 6000, "kind": "rear"}, {"setback": 3000, "kind": "side"}]}),
    );
    // 24 m by 9 m.
    assert!((r["buildable_area_m2"].as_f64().unwrap() - 216.0).abs() < 0.5, "{r}");
    let pl = id(&r);
    let r = run(&mut s, "property_line_edit", json!({"id": pl, "edges": {"2": {"setback": 0}}}));
    assert!((r["buildable_area_m2"].as_f64().unwrap() - 24.0 * 15.0).abs() < 0.5, "{r}");
    let e = crate::json::element_json(&s.model, pl);
    assert!(e["buildable"].as_array().is_some_and(|a| a.len() == 1));
    assert_eq!(e["edges"][0]["street"], json!("High Street"));
    assert!(s.execute("property_line_edit", &json!({"id": pl, "edges": [{"kind": "moat"}]})).unwrap_err().contains("moat"));
}

#[test]
fn the_survey_point_clipped_moves_the_shared_coordinates_unclipped_moves_alone() {
    let mut s = Session::new();
    let r = run(&mut s, "base_point", json!({"which": "survey", "east": 500000000, "north": 7000000000_i64, "height": 800000}));
    let sp = r["survey"]["id"].as_u64().unwrap();
    assert_eq!(r["survey"]["east"], json!(500000000.0));
    // Clipped: moved, its own coordinates stay, the project's origin moves on the survey.
    run(&mut s, "move", json!({"ids": [sp], "by": [10000, 0]}));
    let r = run(&mut s, "base_point", json!({}));
    assert_eq!(r["survey"]["east"], json!(500000000.0));
    assert_eq!(s.model.shared(P2(0.0, 0.0), 0.0)[0], 500000000.0 - 10000.0);
    // Unclipped: it moves alone and reads where it is.
    run(&mut s, "base_point", json!({"which": "survey", "clipped": false}));
    run(&mut s, "move", json!({"ids": [sp], "by": [10000, 0]}));
    let r = run(&mut s, "base_point", json!({}));
    assert_eq!(r["survey"]["east"], json!(500000000.0 + 10000.0));
    assert_eq!(s.model.shared(P2(0.0, 0.0), 0.0)[0], 500000000.0 - 10000.0);
}

#[test]
fn the_project_base_point_reads_its_shared_coordinates_and_turns_them_by_true_north() {
    let mut s = Session::new();
    run(&mut s, "base_point", json!({"which": "project", "angle": 90}));
    // Project north is 90° counter-clockwise from true north: a point east in the project is north on the survey.
    let e = s.model.shared(P2(1000.0, 0.0), 0.0);
    assert!((e[0]).abs() < 1e-6 && (e[1] - 1000.0).abs() < 1e-6, "{e:?}");
    // Specify Coordinates at a Point: the project base point reads what it's given.
    let r = run(&mut s, "base_point", json!({"which": "project", "east": 1000, "north": 2000, "height": 30}));
    assert_eq!((r["project"]["east"].clone(), r["project"]["north"].clone(), r["project"]["height"].clone()), (json!(1000.0), json!(2000.0), json!(30.0)));
    assert!(s.execute("base_point", &json!({"which": "project", "clipped": true})).unwrap_err().contains("no clip"));
    // The georeference reads its numbers from them.
    run(&mut s, "georeference", json!({"crs": "EPSG:31983"}));
    let g = s.model.georeference.clone().unwrap();
    assert_eq!((g.eastings, g.northings, g.rotation), (1000.0, 2000.0, 90.0));
    run(&mut s, "georeference", json!({"eastings": 5000}));
    assert_eq!(s.model.shared(P2(0.0, 0.0), 0.0)[0], 5000.0);
}

#[test]
fn an_old_georeference_becomes_the_survey_point_and_project_base_point() {
    let mut m = Model::new("old");
    m.georeference = Some(crate::model::Georeference { crs: "EPSG:31983".into(), eastings: 1000.0, northings: 2000.0, rotation: 30.0, scale: 1.0, ..Default::default() });
    m.format = 25;
    let (read, _) = Model::read(&m.to_json().unwrap()).unwrap();
    let (sp, pb) = (read.survey_point().unwrap().1.clone(), read.project_base_point().unwrap().1.clone());
    assert_eq!((sp.east, sp.north, pb.angle), (1000.0, 2000.0, 30.0));
}

#[test]
fn trees_and_bike_racks_are_planted_as_families_on_the_ground() {
    let mut s = Session::new();
    let t = slope(&mut s);
    let r = run(&mut s, "planting", json!({"at": [20000, 20000]}));
    assert!((r["offset"].as_f64().unwrap() - 2000.0).abs() < 1.0, "on the ground: {r}");
    let tree = id(&r);
    let Some(Element::Component(c)) = s.model.elements.get(tree) else { panic!() };
    assert_eq!(s.model.families.get(&c.family).map(|f| f.category), Some(crate::family::Category::Planting));
    // Moved, it stands on the ground again; the ground raised, it rises with it.
    run(&mut s, "move", json!({"ids": [tree], "by": [10000, 0]}));
    let Some(Element::Component(c)) = s.model.elements.get(tree) else { panic!() };
    assert!((c.offset - 3000.0).abs() < 1.0, "{}", c.offset);
    run(&mut s, "modify", json!({"id": t, "offset": 500}));
    let Some(Element::Component(c)) = s.model.elements.get(tree) else { panic!() };
    assert!((c.offset - 3500.0).abs() < 1.0, "{}", c.offset);
    let r = run(&mut s, "site_component", json!({"family": "Bike Rack", "at": [5000, 5000], "params": {"Spaces": 4}}));
    let Some(Element::Component(c)) = s.model.elements.get(id(&r)) else { panic!() };
    assert_eq!(s.model.families.get(&c.family).map(|f| f.category), Some(crate::family::Category::SiteComponent));
    assert!(!crate::view3d::solids(&s.model).iter().all(|x| x.owner() != tree), "a tree in 3D");
    assert!(s.execute("planting", &json!({"family": "Unicorn", "at": [0, 0]})).unwrap_err().contains("no family"));
    // Every site family reads.
    assert_eq!(crate::family::site_library(s.model.language).len(), 6);
}

/// A project of format 25 with the masterplan's kinds of before, a building pad among them.
fn old_project() -> String {
    let mut s = Session::new();
    let t = slope(&mut s);
    let level = s.model.guid(s.model.levels()[0].0);
    let mut m = (*s.model).clone();
    let figures = crate::masterplan::figures(&m);
    let site = |kind: &str, use_: &str, props: Value| -> Element {
        let mut st = crate::masterplan::Site {
            kind: kind.into(),
            use_: use_.into(),
            name: String::new(),
            level,
            phase: "existing".into(),
            source: "osm".into(),
            reference: "way/1".into(),
            props: props.as_object().cloned().unwrap_or_default(),
            outline: Vec::new(),
            parts: Vec::new(),
            layout: None,
            notes: Vec::new(),
            params: Default::default(),
        };
        let key = "k".to_string();
        let item = crate::site::shapes::Item { key: &key, kind: &st.kind, use_: &st.use_, props: &st.props, phase: "existing", context: true };
        let shaped = crate::site::shapes::normalise(std::slice::from_ref(&item), &figures).pop().unwrap();
        (st.outline, st.parts) = (shaped.outline, shaped.parts);
        Element::Site(Box::new(st))
    };
    m.insert(site("landscape", "lawn", json!({"control": [[1000, 1000], [9000, 1000], [9000, 9000], [1000, 9000]]})));
    m.insert(site("water", "pond", json!({"control": [[20000, 20000], [26000, 20000], [26000, 26000], [20000, 26000]]})));
    m.insert(site("lane", "road", json!({"path": [[0, 35000], [40000, 35000]]})));
    m.insert(site("tree", "tree", json!({"at": [12000, 12000], "crown": 5000, "height": 9000})));
    m.insert(site("hedge", "hedge", json!({"path": [[0, 30000], [10000, 30000], [10000, 33000]]})));
    m.insert(site("bike_rack", "residents", json!({"at": [15000, 5000], "spaces": 8})));
    m.insert(site("play_area", "equipment", json!({"at": [30000, 5000], "radius": 3000})));
    m.insert(site("pitch", "tennis", json!({"at": [30000, 15000]})));
    m.insert(Element::Site(Box::new(crate::masterplan::Site {
        kind: "parking_stall".into(),
        use_: "standard".into(),
        name: String::new(),
        level,
        phase: "new".into(),
        source: String::new(),
        reference: String::new(),
        props: json!({"at": [38000, 2000]}).as_object().cloned().unwrap_or_default(),
        outline: Vec::new(),
        parts: Vec::new(),
        layout: None,
        notes: Vec::new(),
        params: Default::default(),
    })));
    let _ = t;
    // A building pad as a file of 25 holds one: a floor's fields, its own category.
    let pad = m.insert(Element::Floor(crate::model::Floor {
        type_name: "Generic - 200mm".into(),
        level,
        boundary: vec![P2(14000.0, 14000.0), P2(18000.0, 14000.0), P2(18000.0, 18000.0), P2(14000.0, 18000.0)],
        openings: Vec::new(),
        offset: 1500.0,
        slope: 0.0,
        arrow: None,
        structural: false,
        pattern_origin: P2(0.0, 0.0),
        pattern_angle: 0.0,
        sketch: Vec::new(),
        shape: Default::default(),
        contours: 0.0,
        excavated_by: Vec::new(),
        graded_from: None,
        params: Default::default(),
    }));
    let mut v: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
    v["format"] = json!(25);
    v["elements"][pad.to_string()]["category"] = json!("buildingpad");
    v.to_string()
}

#[test]
fn an_old_masterplan_is_converted_on_load() {
    let text = old_project();
    let (m, left_out) = Model::read(&text).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    m.check().unwrap_or_else(|e| panic!("{e}"));
    let count = |cat: &str| m.elements.values().filter(|e| e.category() == cat).count();
    let kinds: Vec<String> = m.elements.values().filter_map(|e| if let Element::Site(st) = e { Some(st.kind.clone()) } else { None }).collect();
    assert_eq!(kinds, vec!["parking_stall".to_string()], "parking stays");
    // The lawn, the pond, the pitch, and the road's carriageway (and its pavements, where it has them).
    let roles: Vec<String> = m.elements.values().filter_map(|e| if let Element::SubDivision(sd) = e { Some(sd.role.clone()) } else { None }).collect();
    assert!(count("subdivision") >= 4, "{roles:?}");
    for r in ["landscape/lawn", "water/pond", "pitch/tennis", "lane/road"] {
        assert!(roles.iter().any(|x| x == r), "{r}: {roles:?}");
    }
    // Where they came from is kept.
    assert!(m.elements.values().any(|e| matches!(e, Element::SubDivision(sd) if sd.params.get("source") == Some(&json!("osm")) && sd.params.get("ref") == Some(&json!("way/1")))));
    // The tree, the hedge's two pieces, the bike rack and the play equipment are family components on the ground.
    let comps: Vec<&crate::model::Component> = m.elements.values().filter_map(|e| if let Element::Component(c) = e { Some(c) } else { None }).collect();
    assert_eq!(comps.len(), 5, "{comps:?}");
    let tree = comps.iter().find(|c| c.family == "Tree").unwrap();
    assert_eq!(tree.params.get("Crown Diameter"), Some(&json!(5000.0)));
    assert!((tree.offset - 1200.0).abs() < 1.0, "on the ground at x 12 m: {}", tree.offset);
    // The pad is a floor that excavates the toposolid.
    let Some(pad) = m.elements.values().find_map(|e| match e {
        Element::Floor(f) if f.offset == 1500.0 => Some(f),
        _ => None,
    }) else {
        panic!("the pad is a floor")
    };
    assert_eq!(pad.offset, 1500.0);
    assert!(m.elements.values().any(|e| matches!(e, Element::Toposolid(t) if t.excavated_by.len() == 1)));
    // Written again, it is what it read.
    let (again, _) = Model::read(&m.to_json().unwrap()).unwrap();
    assert_eq!(again.elements.len(), m.elements.len());
}

#[test]
fn the_new_site_reads_back_from_a_file() {
    let mut s = Session::new();
    let t = slope(&mut s);
    run(&mut s, "sub_division", json!({"boundary": [[1000, 1000], [5000, 1000], [5000, 5000]], "type": "Grass"}));
    let f = id(&run(&mut s, "floor", json!({"boundary": [[15000, 15000], [25000, 15000], [25000, 25000], [15000, 25000]], "offset": -200, "type": "Generic - 200mm"})));
    run(&mut s, "excavate", json!({"toposolid": t, "by": [f]}));
    run(&mut s, "base_point", json!({"which": "survey", "east": 1}));
    run(&mut s, "planting", json!({"at": [3000, 3000]}));
    run(&mut s, "property_line", json!({"points": [[0, 0], [9000, 0], [9000, 9000]], "edges": [{"setback": 1000}]}));
    let text = s.model.to_json().unwrap();
    assert!(text.contains("\"format\": 26") || text.contains("\"format\":26"));
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.elements.len(), s.model.elements.len());
    for (a, b) in m.elements.values().zip(s.model.elements.values()) {
        assert_eq!(a, b);
    }
}

#[test]
fn a_road_is_its_carriageway_and_pavements_as_sub_divisions_on_the_ground() {
    let mut s = Session::new();
    let t = slope(&mut s);
    let r = run(&mut s, "road", json!({"path": [[0, 20000], [40000, 20000]], "width": 14000, "lanes": 2}));
    let ids: Vec<Id> = serde_json::from_value(r["ids"].clone()).unwrap();
    assert!(!ids.is_empty(), "{r}");
    assert_eq!(r["host"], json!(t));
    let roles: Vec<String> =
        ids.iter().filter_map(|id| if let Some(Element::SubDivision(sd)) = s.model.elements.get(*id) { Some(sd.role.clone()) } else { None }).collect();
    assert!(roles.iter().any(|x| x == "lane/road"), "{roles:?}");
    assert!(s.execute("road", &json!({"path": [[0, 0], [10, 0]], "use": "motorway"})).unwrap_err().contains("motorway"));
}

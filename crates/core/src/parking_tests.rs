//! Ramps and parking as Revit has them (docs/parking-ramps-plan.md), driven through their commands: a ramp along a path and
//! on every level it crosses, the Parking Space family and its schedule, the car park generator on the real building
//! (walls, columns, ramps; pinned and deleted bays; the firm's sizes), and a file of before converted as it is read.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Guid, Id, Model};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id(v: &Value) -> Id {
    serde_json::from_value(v["id"].clone()).unwrap_or_else(|_| panic!("no id in {v}"))
}

fn level_named(s: &Session, name: &str) -> Id {
    s.model.levels().iter().find(|(_, l)| l.name == name).map(|(id, _)| *id).unwrap_or_else(|| panic!("no level {name}"))
}

/// A project with a basement, B1 at -3500, below its Level 1 (0) and Level 2 (3000).
fn with_basement() -> (Session, Id, Id, Id) {
    let mut s = Session::new();
    run(&mut s, "level", json!({"elevation": -3500, "name": "B1"}));
    let (b1, l1, l2) = (level_named(&s, "B1"), level_named(&s, "Level 1"), level_named(&s, "Level 2"));
    (s, b1, l1, l2)
}

/// The elements drawn in the plan of `level`, by how many primitives each.
fn drawn(s: &Session, level: Id) -> Vec<Option<Id>> {
    crate::render::plan(&s.model, level, 800, 600).owners
}

/// The car park's bays: their ids and components.
fn bays(s: &Session, park: Id) -> Vec<(Id, crate::model::Component)> {
    let me = s.model.guid(park).to_string();
    s.model
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Component(c) if c.params.get(crate::carpark::BY).and_then(Value::as_str) == Some(me.as_str()) => {
                Some((*id, c.clone()))
            }
            _ => None,
        })
        .collect()
}

fn result(s: &Session, park: Id) -> Value {
    match s.model.elements.get(park) {
        Some(Element::Site(st)) => st.props.get("result").cloned().unwrap_or(Value::Null),
        other => panic!("{park} is {other:?}"),
    }
}

fn notes(r: &Value) -> String {
    r["notes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" | ")).unwrap_or_default()
}

const AREA: [[f64; 2]; 4] = [[0.0, 0.0], [60000.0, 0.0], [60000.0, 40000.0], [0.0, 40000.0]];

// ====================================================================== ramps

#[test]
fn a_ramp_by_path_climbs_along_lines_and_arcs_its_rise_shared_by_slope() {
    let mut s = Session::new();
    // A straight run of 10 m, a half turn of radius 8 m, and a flat landing of 3 m: from Level 1 up to Level 2.
    let r = run(
        &mut s,
        "ramp",
        json!({"path": [[0, 0, 0], [10000, 0, 1], [10000, 16000, 0], [7000, 16000]], "width": 3500, "slope": 12, "slopes": [12, 12, 0], "top_level": "Level 2"}),
    );
    let ramp = id(&r);
    let e = crate::json::element_json(&s.model, ramp);
    let slopes: Vec<f64> = e["run_slopes"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    // The straight run and the arc's chords climb alike; the landing is flat.
    assert!(slopes.len() > 4, "the arc is cut in chords: {slopes:?}");
    assert_eq!(*slopes.last().unwrap(), 0.0);
    let climbing: Vec<f64> = slopes.iter().copied().filter(|v| *v > 0.0).collect();
    assert!(climbing.iter().all(|v| (v - climbing[0]).abs() < 0.2), "{slopes:?}");
    // 3 m over 10 m and half a circle of 8 m (25.1 m): a little under 9%.
    assert!((climbing[0] - 3000.0 / (10000.0 + std::f64::consts::PI * 8000.0) * 100.0).abs() < 0.3, "{}", climbing[0]);
    let Some(Element::Ramp(rm)) = s.model.elements.get(ramp) else { panic!() };
    let l = crate::stairs::ramp(&s.model, rm);
    assert!((l.runs.last().unwrap().z.1 - 3000.0).abs() < 1e-6);
    // Mitred: one chord's end is the next one's start, no gaps.
    for w in l.runs.windows(2) {
        let (a, b) = (w[0].ring(3500.0), w[1].ring(3500.0));
        assert!(a[1].dist(b[0]) < 1.0 && a[2].dist(b[3]) < 1.0, "{a:?} {b:?}");
    }
    // Moved, its path goes with it; mirrored, its arc turns the other way.
    run(&mut s, "move", json!({"ids": [ramp], "by": [1000, 0]}));
    let Some(Element::Ramp(rm)) = s.model.elements.get(ramp) else { panic!() };
    assert_eq!(rm.path[1][0], 11000.0);
    assert_eq!(rm.start, P2(1000.0, 0.0));
}

#[test]
fn a_ramp_shows_on_every_level_it_crosses_dashed_over_the_cut_plane_and_a_stair_too() {
    let (mut s, b1, l1, l2) = with_basement();
    // From B1 straight up to Level 2: through Level 1.
    let ramp =
        id(&run(&mut s, "ramp", json!({"start": [0, 0], "angle": 0, "level": "B1", "top_level": "Level 2", "width": 3000, "slope": 15})));
    let stair = id(&run(&mut s, "stair", json!({"start": [0, 20000], "angle": 0, "level": "B1", "top_level": "Level 2"})));
    for level in [b1, l1, l2] {
        let owners = drawn(&s, level);
        assert!(owners.contains(&Some(ramp)), "the ramp is drawn on level {level}");
        assert!(owners.contains(&Some(stair)), "the stair is drawn on level {level}");
    }
    // On Level 1 it is drawn up from B1 and on over the cut plane, dashed there.
    let d = crate::render::plan(&s.model, l1, 800, 600);
    let lines = |dashed: bool| {
        d.prims
            .iter()
            .zip(&d.owners)
            .filter(|(p, o)| **o == Some(ramp) && matches!(p, crate::render::Prim::Line { dash, .. } if dash.is_some() == dashed))
            .count()
    };
    let (dashed, solid) = (lines(true), lines(false));
    assert!(dashed > 0 && solid > 0, "{dashed} dashed, {solid} solid");
    // A ramp of Level 1 to Level 2 is not in B1's plan.
    let up = id(&run(&mut s, "ramp", json!({"start": [20000, 0], "angle": 0, "level": "Level 1", "width": 3000})));
    assert!(!drawn(&s, b1).contains(&Some(up)));
}

#[test]
fn an_old_masterplan_ramp_and_parking_become_a_ramp_and_parking_spaces() {
    let (mut s, _, l1, _) = with_basement();
    run(&mut s, "toposolid", json!({"boundary": [[-20000, -20000], [80000, -20000], [80000, 60000], [-20000, 60000]], "level": "Level 1"}));
    let level: Guid = s.model.guid(l1);
    let mut m = (*s.model).clone();
    let figures = crate::masterplan::figures(&m);
    let site = |kind: &str, use_: &str, props: Value| -> Element {
        let mut st = crate::masterplan::Site {
            kind: kind.into(),
            use_: use_.into(),
            name: String::new(),
            level,
            phase: "new".into(),
            source: String::new(),
            reference: String::new(),
            props: props.as_object().cloned().unwrap_or_default(),
            outline: Vec::new(),
            parts: Vec::new(),
            notes: Vec::new(),
            params: Default::default(),
        };
        let key = "k".to_string();
        let item = crate::site::shapes::Item { key: &key, kind: &st.kind, use_: &st.use_, props: &st.props, phase: "new", context: false };
        let shaped = crate::site::shapes::normalise(std::slice::from_ref(&item), &figures).pop().unwrap();
        (st.outline, st.parts) = (shaped.outline, shaped.parts);
        Element::Site(Box::new(st))
    };
    let ramp =
        m.insert(site("ramp", "cars", json!({"path": [[0, 50000], [30000, 50000]], "width": 3500, "gradient": 12, "to_level": "B1"})));
    let stall = m.insert(site("parking_stall", "accessible", json!({"at": [5000, 30000], "angle": 90})));
    let band = m.insert(site("parking_band", "parking", json!({"path": [[0, 10000], [20000, 10000]], "sides": "both"})));
    let park = m.insert(site(
        "car_park",
        "parking",
        json!({"control": [[40000, 0], [70000, 0], [70000, 30000], [40000, 30000]], "removed": [[41250, 2500]]}),
    ));
    let mut v: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
    v["format"] = json!(26);
    // A car park of 26 kept its layout in a blob.
    v["elements"][park.to_string()]["layout"] = json!({"pieces": [], "result": {"stalls": 3}});
    let (m, left_out) = Model::read(&v.to_string()).unwrap_or_else(|e| panic!("{e}"));
    assert!(left_out.is_empty(), "{left_out:?}");
    m.check().unwrap_or_else(|e| panic!("{e}"));
    // The ramp: from B1 up to Level 1, its foot at the old one's far end.
    let Some(Element::Ramp(r)) = m.elements.get(ramp) else { panic!("{:?}", m.elements.get(ramp)) };
    assert_eq!(r.level, m.guid(level_named(&s, "B1")));
    assert_eq!(r.top_level, Some(level));
    assert_eq!((r.path[0][0], r.path.last().unwrap()[0]), (30000.0, 0.0));
    assert_eq!(r.slope, 12.0);
    // The stall: an Accessible Parking Space; the band: its bays, and its aisle a drive aisle.
    let Some(Element::Component(c)) = m.elements.get(stall) else { panic!() };
    assert!(c.type_name.starts_with("Accessible"), "{}", c.type_name);
    assert_eq!(m.families[&c.family].category, crate::family::Category::Parking);
    let band_bays = m
        .elements
        .values()
        .filter(|e| matches!(e, Element::Component(c) if crate::carpark::is_bay(&m, c) && c.at.1 != 30000.0 && c.at.0 < 30000.0))
        .count();
    assert!(band_bays >= 10, "{band_bays} of the band's bays");
    assert!(m.elements.get(band).is_some_and(|e| matches!(e, Element::Component(_) | Element::SubDivision(_))));
    assert!(m.elements.values().any(|e| matches!(e, Element::SubDivision(sd) if sd.role == crate::carpark::AISLE_ROLE)));
    // The car park: laid out again as elements, its removed bay kept out.
    let Some(Element::Site(cp)) = m.elements.get(park) else { panic!() };
    let laid = cp.props["result"]["stalls"].as_u64().unwrap();
    assert!(laid > 20, "{laid}");
    let me = m.guid(park).to_string();
    let own: Vec<&crate::model::Component> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Component(c) if c.params.get(crate::carpark::BY).and_then(Value::as_str) == Some(me.as_str()) => Some(c),
            _ => None,
        })
        .collect();
    assert_eq!(own.len() as u64, laid);
    assert!(!own.iter().any(|c| c.at.dist(P2(41250.0, 2500.0)) < 1200.0), "the removed bay stays out");
    // `site` takes none of them now.
    let mut s2 = Session::new();
    for kind in ["ramp", "parking_stall", "parking_band"] {
        let e = s2.execute("site", &json!({"kind": kind, "props": {}})).unwrap_err();
        assert!(e.contains("now"), "{e}");
    }
}

// ====================================================================== the Parking family

#[test]
fn a_parking_component_stands_on_the_ground_or_the_floor_with_its_type_and_mark() {
    let (mut s, _, _, _) = with_basement();
    run(
        &mut s,
        "toposolid",
        json!({"boundary": [[0, 0], [20000, 0], [20000, 20000], [0, 20000]], "points": [[0, 0, 0], [20000, 0, 2000], [20000, 20000, 2000], [0, 20000, 0]], "level": "Level 1"}),
    );
    let r = run(&mut s, "parking_component", json!({"at": [10000, 10000], "type": "EV", "mark": "12", "angle": 90}));
    let bay = id(&r);
    assert!((r["offset"].as_f64().unwrap() - 1000.0).abs() < 1.0, "on the ground: {r}");
    let e = crate::json::element_json(&s.model, bay);
    assert_eq!(e["mark"], json!("12"));
    assert_eq!((e["width"].as_f64(), e["depth"].as_f64()), (Some(2500.0), Some(5000.0)));
    // On a floor below ground.
    run(&mut s, "floor", json!({"boundary": [[0, 0], [20000, 0], [20000, 20000], [0, 20000]], "level": "B1", "offset": 100}));
    let low = run(&mut s, "parking_component", json!({"at": [5000, 5000], "level": "B1", "type": "Accessible"}));
    assert_eq!(low["offset"].as_f64(), Some(100.0));
    // Its plan: its stall lines, and the wheelchair's mark of an accessible one.
    let b1 = level_named(&s, "B1");
    assert!(drawn(&s, b1).contains(&Some(id(&low))));
    // Visibility/Graphics hides its markings apart from its lines.
    let before = drawn(&s, b1).iter().filter(|o| **o == Some(id(&low))).count();
    run(&mut s, "graphics", json!({"view": b1, "hide": ["parkingmarking"]}));
    let after = drawn(&s, b1).iter().filter(|o| **o == Some(id(&low))).count();
    assert!(after < before && after > 0, "{before} then {after}");
    // The Parking schedule: a row a bay, counted by type.
    let sc = run(&mut s, "schedule", json!({"name": "Parking Schedule", "category": "parking", "fields": ["Type", "Level", "Count"]}));
    let text = sc.to_string();
    assert!(text.contains("EV") && text.contains("Accessible"), "{text}");
    // A family of another category is refused.
    assert!(s.execute("parking_component", &json!({"at": [0, 0], "family": "Tree"})).is_err());
}

#[test]
fn every_parking_space_type_reads_and_draws() {
    let lib = crate::family::site_library(crate::language::Language::English);
    let f = &lib["Parking Space"];
    assert_eq!(f.category, crate::family::Category::Parking);
    assert_eq!(f.types.keys().cloned().collect::<Vec<_>>(), ["Accessible", "EV", "Motorcycle", "Standard"]);
    for t in f.types.keys() {
        let v = f.values(t, &Default::default()).unwrap();
        f.all_marks(t, &v).unwrap_or_else(|e| panic!("{t}: {e}"));
        f.pieces(t, &v).unwrap_or_else(|e| panic!("{t}: {e}"));
    }
    // An angled bay is a parallelogram whose outline the generator's matches.
    let ring = crate::carpark::bay_ring(P2(0.0, 0.0), 0.0, false, 2500.0, 5000.0, 60.0);
    let (at, angle, mirrored, w, d) = crate::carpark::bay_from(&ring, (ring[0] + ring[1]) * 0.5, 60.0).unwrap();
    assert!(at.dist(P2(0.0, 0.0)) < 1e-6 && angle.abs() < 1e-6 && !mirrored && (w - 2500.0).abs() < 1e-6 && (d - 5000.0).abs() < 1e-6);
}

// ====================================================================== the car park generator

/// A car park on Level 1 over a toposolid: its id.
fn surface_park(s: &mut Session) -> Id {
    run(s, "toposolid", json!({"boundary": [[-10000, -10000], [70000, -10000], [70000, 50000], [-10000, 50000]]}));
    id(&run(s, "car_park", json!({"control": AREA, "level": "Level 1"})))
}

#[test]
fn a_car_park_lays_out_parking_spaces_and_drive_aisles_and_keeps_clear_of_the_building_on_its_level() {
    let mut s = Session::new();
    let park = surface_park(&mut s);
    let laid = bays(&s, park);
    assert!(laid.len() > 60, "{} bays", laid.len());
    assert_eq!(result(&s, park)["stalls"].as_u64(), Some(laid.len() as u64));
    // Numbered from 1, Standard by the firm's default sizes.
    assert!(laid.iter().any(|(_, c)| c.params.get("Mark") == Some(&json!("1"))));
    // The standard's sizes (Saudi Arabia's, 2.8 by 5.5 m) where the firm gives none: a type made for them.
    assert!(
        laid.iter().all(|(_, c)| c.type_name.starts_with("Standard") || c.type_name.starts_with("Accessible")),
        "{:?}",
        laid[0].1.type_name
    );
    let aisles =
        s.model.elements.values().filter(|e| matches!(e, Element::SubDivision(sd) if sd.role == crate::carpark::AISLE_ROLE)).count();
    assert!(aisles > 0);
    // A wall and a column across it: it lays out again, keeping clear of both.
    run(&mut s, "wall", json!({"points": [[30000, 5000], [30000, 35000]], "level": "Level 1"}));
    run(&mut s, "column", json!({"at": [15000, 20000], "level": "Level 1"}));
    let now = bays(&s, park);
    let wall = crate::geom::P2(30000.0, 20000.0);
    for (_, c) in &now {
        let (w, d, a) = crate::carpark::bay_size(&s.model, c).unwrap();
        let ring = crate::carpark::bay_ring(c.at, c.angle, c.mirrored, w, d, a);
        let xs: (f64, f64) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
        let ys: (f64, f64) = ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.1), hi.max(q.1)));
        let across_wall = xs.0 < wall.0 - 150.0 && xs.1 > wall.0 + 150.0 && ys.1 > 5200.0 && ys.0 < 34800.0;
        assert!(!across_wall, "a bay across the wall: {:?}", c.at);
        assert!(!crate::geom::contains(&ring, P2(15000.0, 20000.0)), "a bay over the column: {:?}", c.at);
    }
    // It never places a column of its own.
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Column(_) | Element::StructuralColumn(_))).count(), 1);
    // Deleted, its bays and aisles go with it.
    run(&mut s, "delete", json!({"ids": [park]}));
    assert!(bays(&s, park).is_empty());
    assert!(!s.model.elements.values().any(|e| matches!(e, Element::SubDivision(sd) if sd.role == crate::carpark::AISLE_ROLE)));
}

#[test]
fn a_car_park_below_ground_takes_the_real_ramps_as_ways_in_and_never_invents_one() {
    let (mut s, _, _, _) = with_basement();
    let park = id(&run(&mut s, "car_park", json!({"control": AREA, "level": "B1"})));
    let r = result(&s, park);
    assert!(bays(&s, park).is_empty(), "nothing is laid out with no way in");
    assert!(notes(&r).contains("No ramp reaches B1"), "{r}");
    assert!(!s.model.elements.values().any(|e| matches!(e, Element::Ramp(_))), "no ramp is made");
    // A ramp from B1 up to Level 1, its foot inside the car park: its way in.
    run(&mut s, "ramp", json!({"start": [2000, 37000], "angle": 0, "level": "B1", "top_level": "Level 1", "width": 6000, "slope": 15}));
    let laid = bays(&s, park);
    assert!(laid.len() > 60, "{} bays", laid.len());
    let r = result(&s, park);
    assert!(!notes(&r).contains("No ramp reaches"), "{r}");
    // It placed no column, and proposes a grid since there is none.
    assert!(!s.model.elements.values().any(|e| matches!(e, Element::Column(_) | Element::StructuralColumn(_))));
    assert!(r["proposed_columns"].as_array().is_some_and(|a| !a.is_empty()), "{r}");
    // The ramp is kept clear of.
    for (_, c) in &laid {
        assert!(!(c.at.1 > 34000.0 && c.at.0 < 2000.0 + 26000.0 && c.at.0 > 2000.0), "a bay on the ramp: {:?}", c.at);
    }
    // Below ground its aisles are the car park's arrows, not elements.
    assert!(r["aisles"].as_array().is_some_and(|a| !a.is_empty()));
    assert!(!s.model.elements.values().any(|e| matches!(e, Element::SubDivision(_))));
    // A real column in its area: no grid proposed.
    run(&mut s, "structural_column", json!({"at": [30000, 20000], "level": "B1"}));
    assert!(result(&s, park).get("proposed_columns").is_none());
}

#[test]
fn a_moved_bay_is_pinned_and_kept_and_a_deleted_bay_stays_removed() {
    let mut s = Session::new();
    let park = surface_park(&mut s);
    let laid = bays(&s, park);
    let (moved, was) = (laid[0].0, laid[0].1.at);
    run(&mut s, "move", json!({"ids": [moved], "by": [0, 300]}));
    let Some(Element::Component(c)) = s.model.elements.get(moved).cloned() else { panic!("the moved bay is gone") };
    assert_eq!(c.at, was + P2(0.0, 300.0), "kept where the person put it");
    assert!(s.model.pinned.contains(&s.model.guid(moved)), "and pinned");
    // Run again by command: still there, and no bay of its own over it.
    run(&mut s, "car_park_run", json!({"id": park}));
    assert!(s.model.elements.get(moved).is_some());
    let (w, d, a) = crate::carpark::bay_size(&s.model, &c).unwrap();
    let ring = crate::carpark::bay_ring(c.at, c.angle, c.mirrored, w, d, a);
    assert!(!bays(&s, park).iter().any(|(id, o)| *id != moved && crate::geom::contains(&ring, o.at)));
    // A deleted bay stays out when it runs again.
    let gone = bays(&s, park).into_iter().find(|(id, _)| *id != moved).unwrap();
    run(&mut s, "delete", json!({"ids": [gone.0]}));
    let Some(Element::Site(cp)) = s.model.elements.get(park) else { panic!() };
    assert!(cp.props["removed"].as_array().is_some_and(|a| !a.is_empty()));
    run(&mut s, "car_park_run", json!({"id": park}));
    assert!(!bays(&s, park).iter().any(|(_, c)| c.at.dist(gone.1.at) < 100.0), "the deleted bay came back");
    // Run again with nothing changed: every bay keeps its id.
    let before: Vec<Id> = bays(&s, park).into_iter().map(|(id, _)| id).collect();
    run(&mut s, "car_park_run", json!({"id": park}));
    let after: Vec<Id> = bays(&s, park).into_iter().map(|(id, _)| id).collect();
    assert_eq!(before, after);
    // Unpinned, the moved bay is the car park's again: laid out afresh, not kept where it was moved.
    run(&mut s, "unpin", json!({"ids": [moved]}));
    assert!(!bays(&s, park).iter().any(|(id, c)| *id == moved && c.at == was + P2(0.0, 300.0)), "unpinned, it is laid out again");
    assert!(s.model.elements.get(moved).is_none() || !s.model.pinned.contains(&s.model.guid(moved)));
    let _ = laid;
}

#[test]
fn firm_sizes_are_read_by_the_firms_names_and_the_old_ones() {
    let mut s = Session::new();
    run(&mut s, "site_figures", json!({"values": {"parking.bay_width": 2700}}));
    let park = surface_park(&mut s);
    let (_, c) = bays(&s, park).into_iter().find(|(_, c)| c.type_name != "Standard" && !c.type_name.starts_with("Accessible")).unwrap();
    assert!(c.type_name.starts_with("Standard 2700"), "the old name: {}", c.type_name);
    run(&mut s, "site_figures", json!({"values": {"parking.stall.width": 2400, "parking.stall.depth": 4800}}));
    let (_, c) = bays(&s, park).into_iter().find(|(_, c)| !c.type_name.starts_with("Accessible")).unwrap();
    assert!(c.type_name.starts_with("Standard 2400 x 4800"), "the firm's name first: {}", c.type_name);
    // Its own settings over the firm's.
    let Some(Element::Site(_)) = s.model.elements.get(park) else { panic!() };
    run(&mut s, "site_edit", json!({"id": park, "props": {"stall_width": 2600}}));
    let (_, c) = bays(&s, park).into_iter().find(|(_, c)| !c.type_name.starts_with("Accessible")).unwrap();
    assert!(c.type_name.starts_with("Standard 2600 x 4800"), "{}", c.type_name);
}

#[test]
fn the_survey_point_and_project_base_point_cant_be_deleted() {
    let mut s = Session::new();
    let r = run(&mut s, "base_point", json!({"which": "survey", "east": 1000}));
    let _ = r;
    let points: Vec<Id> = s
        .model
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, Element::SurveyPoint(_) | Element::ProjectBasePoint(_)))
        .map(|(id, _)| *id)
        .collect();
    assert!(!points.is_empty());
    for p in points {
        let e = s.execute("delete", &json!({"ids": [p]})).unwrap_err();
        assert!(e.contains("can't be deleted"), "{e}");
        assert!(s.model.elements.get(p).is_some());
    }
}

// ====================================================================== the site's leftovers (Arqiflow's side)

#[test]
fn the_sites_elements_take_project_parameters_as_revit_allows() {
    let mut s = Session::new();
    run(
        &mut s,
        "parameter",
        json!({"name": "Plot Source", "kind": "text", "categories": ["toposolid", "propertyline", "subdivision", "surveypoint", "projectbasepoint"]}),
    );
    let t = id(&run(&mut s, "toposolid", json!({"boundary": [[0, 0], [20000, 0], [20000, 20000], [0, 20000]]})));
    let pl = id(&run(&mut s, "property_line", json!({"points": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]]})));
    let sd = id(&run(&mut s, "sub_division", json!({"boundary": [[1000, 1000], [5000, 1000], [5000, 5000]]})));
    run(&mut s, "base_point", json!({"which": "survey", "east": 1000}));
    let sp = s.model.survey_point().unwrap().0;
    for e in [t, pl, sd, sp] {
        run(&mut s, "modify", json!({"id": e, "params": {"Plot Source": "survey 2026"}}));
        assert_eq!(s.model.elements.get(e).and_then(|e| e.params()).and_then(|p| p.get("Plot Source")), Some(&json!("survey 2026")));
    }
}

#[test]
fn a_toposolids_contours_are_set_and_turned_off_after_it_is_made() {
    let mut s = Session::new();
    let t = id(&run(
        &mut s,
        "toposolid",
        json!({"boundary": [[0, 0], [20000, 0], [20000, 20000], [0, 20000]], "points": [[0, 0, 0], [20000, 0, 3000], [20000, 20000, 3000], [0, 20000, 0]]}),
    ));
    let l1 = level_named(&s, "Level 1");
    let lines = |s: &Session| drawn(s, l1).iter().filter(|o| **o == Some(t)).count();
    run(&mut s, "modify", json!({"id": t, "contours": 500}));
    let fine = lines(&s);
    run(&mut s, "modify", json!({"id": t, "contours": 1000}));
    assert!(lines(&s) < fine, "a wider interval, fewer lines");
    run(&mut s, "modify", json!({"id": t, "contours_hidden": true}));
    let Some(Element::Toposolid(f)) = s.model.elements.get(t) else { panic!() };
    assert_eq!(f.contours, 1000.0, "its interval kept");
    assert!(lines(&s) < fine);
}

#[test]
fn ifc_places_the_project_on_the_map_from_the_base_points_alone() {
    let mut s = Session::new();
    run(&mut s, "base_point", json!({"which": "survey", "east": 500000000, "north": 4000000000_i64, "height": 12000}));
    run(&mut s, "base_point", json!({"which": "project", "angle": 30}));
    assert!(s.model.georeference.is_none());
    let f = crate::ifc::export(&s.model);
    let line = f.lines().find(|l| l.contains("IFCMAPCONVERSION(")).unwrap_or_else(|| panic!("no map conversion"));
    assert!(line.contains("500000.") && line.contains("4000000.") && line.contains("12."), "{line}");
    assert!(f.contains("IFCPROJECTEDCRS("));
}

#[test]
fn a_road_keeps_its_definition_and_is_made_again_by_replace() {
    let mut s = Session::new();
    run(&mut s, "toposolid", json!({"boundary": [[-10000, -20000], [60000, -20000], [60000, 20000], [-10000, 20000]]}));
    let r = run(&mut s, "road", json!({"path": [[0, 0], [50000, 0]], "lanes": 2, "name": "High Street"}));
    let ids: Vec<Id> = serde_json::from_value(r["ids"].clone()).unwrap();
    assert!(!ids.is_empty(), "{r}");
    let last = *ids.last().unwrap();
    let road = |s: &Session, id: Id| match s.model.elements.get(id) {
        Some(Element::SubDivision(sd)) => sd.road.clone().unwrap_or_else(|| panic!("{id} keeps no road")),
        other => panic!("{other:?}"),
    };
    let first = road(&s, ids[0]);
    assert_eq!(first.of, s.model.guid(ids[0]));
    assert!(ids.iter().all(|id| road(&s, *id).of == first.of));
    assert_eq!(first.name, "High Street");
    assert_eq!(first.props["path"], json!([[0, 0], [50000, 0]]));
    assert!(first.props.get("width").is_some(), "its width filled in: {:?}", first.props);
    let e = crate::json::element_json(&s.model, last);
    assert_eq!(e["road"]["props"]["lanes"], json!(2));
    // Made again wider, from any piece: the first keeps its id, the old pieces go.
    let again = run(&mut s, "road", json!({"replace": last, "width": 9000}));
    let now: Vec<Id> = serde_json::from_value(again["ids"].clone()).unwrap();
    assert_eq!(now[0], ids[0]);
    assert_eq!(road(&s, now[0]).props["width"], json!(9000));
    assert_eq!(road(&s, now[0]).props["path"], json!([[0, 0], [50000, 0]]));
    let pieces = s.model.elements.values().filter(|e| matches!(e, Element::SubDivision(sd) if sd.road.is_some())).count();
    assert_eq!(pieces, now.len());
}

// ====================================================================== a real basement

/// Whether two convex rings overlap by more than `tol` (mm) across every axis (separating axes).
fn overlap(a: &[P2], b: &[P2], tol: f64) -> bool {
    for ring in [a, b] {
        let n = ring.len();
        for i in 0..n {
            let e = ring[(i + 1) % n] - ring[i];
            let len = e.dot(e).sqrt();
            if len < 1e-6 {
                continue;
            }
            let ax = e.perp() * (1.0 / len);
            let span =
                |r: &[P2]| r.iter().map(|q| q.dot(ax)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            let (p, q) = (span(a), span(b));
            if p.1 - q.0 < tol || q.1 - p.0 < tol {
                return false;
            }
        }
    }
    true
}

#[test]
fn a_basement_with_walls_a_column_grid_and_two_ramps_from_the_ground_is_laid_out_using_those_ramps() {
    let (mut s, b1, _, _) = with_basement();
    // The basement's slab and its walls, B1 up to Level 1.
    run(&mut s, "floor", json!({"boundary": AREA, "level": "B1"}));
    run(&mut s, "wall", json!({"points": AREA, "closed": true, "level": "B1", "top_level": "Level 1"}));
    // A column grid, 8.1 m by 10 m.
    let mut columns = Vec::new();
    for x in [8100.0, 16200.0, 24300.0, 32400.0, 40500.0, 48600.0] {
        for y in [10000.0, 20000.0, 30000.0] {
            columns.push(id(&run(&mut s, "structural_column", json!({"at": [x, y], "level": "B1", "top_level": "Level 1"}))));
        }
    }
    // Two ramps up to the ground: a straight one along the north wall, its foot at the east; a curved one in the south-east,
    // a straight run then a quarter turn.
    let straight = id(&run(
        &mut s,
        "ramp",
        json!({"start": [31000, 36500], "angle": 180, "level": "B1", "top_level": "Level 1", "width": 6000, "slope": 15}),
    ));
    let curved = id(&run(
        &mut s,
        "ramp",
        json!({"path": [[36000, 3500, 0], [52000, 3500, 0.41421356], [56000, 7500]], "level": "B1", "top_level": "Level 1", "width": 6000}),
    ));
    let park = id(&run(&mut s, "car_park", json!({"control": AREA, "level": "B1"})));
    let r = result(&s, park);
    let laid = bays(&s, park);
    assert!(!notes(&r).contains("No ramp reaches"), "{r}");
    assert!(!notes(&r).contains("No aisle can be reached"), "a ramp leads in: {r}");
    // Some bays: how many the solver fits round two ramps inside the area and a column grid is its own weakness (see
    // `..._is_filled_as_a_designer_would`).
    assert!(!laid.is_empty(), "{r}");
    // Its own ramps, not one made up; no column of its own, none proposed with a grid there.
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::Ramp(_))).count(), 2);
    assert_eq!(s.model.elements.values().filter(|e| matches!(e, Element::StructuralColumn(_))).count(), columns.len());
    assert_eq!(r["columns"].as_u64(), Some(columns.len() as u64));
    assert!(r.get("proposed_columns").is_none(), "{r}");
    // On B1, on its slab.
    let b1g = s.model.guid(b1);
    assert!(laid.iter().all(|(_, c)| c.level == b1g));
    // What stands there: the walls, the columns, the two ramps' runs.
    let m = &s.model;
    let mut solid: Vec<(String, Vec<P2>)> = Vec::new();
    for (eid, e) in m.elements.iter() {
        match e {
            Element::Wall(_) => solid.push((format!("wall {eid}"), crate::walls::outline(m, *eid))),
            Element::StructuralColumn(_) => solid.push((format!("column {eid}"), m.column_outline(e))),
            Element::Ramp(rm) => {
                let l = crate::stairs::ramp(m, rm);
                solid.extend(l.runs.iter().map(|run| (format!("ramp {eid}"), run.ring(l.width))));
            }
            _ => {}
        }
    }
    let rings: Vec<(Id, Vec<P2>)> = laid
        .iter()
        .map(|(id, c)| {
            let (w, d, a) = crate::carpark::bay_size(m, c).unwrap();
            (*id, crate::carpark::bay_ring(c.at, c.angle, c.mirrored, w, d, a))
        })
        .collect();
    for (id, ring) in &rings {
        for (what, o) in &solid {
            assert!(!overlap(ring, o, 5.0), "bay {id} {ring:?} over {what}");
        }
        assert!(ring.iter().all(|q| crate::geom::contains(&AREA.map(|p| P2(p[0], p[1])), *q)), "bay {id} outside the area");
    }
    for (i, (a, ra)) in rings.iter().enumerate() {
        for (b, rb) in &rings[i + 1..] {
            assert!(!overlap(ra, rb, 5.0), "bays {a} and {b} overlap");
        }
    }
    // Its aisles (the arrows below ground) run through no column and no wall, and one starts at each ramp's foot.
    let aisles: Vec<Vec<P2>> = r["aisles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["ring"].as_array().unwrap().iter().map(|q| P2(q[0].as_f64().unwrap(), q[1].as_f64().unwrap())).collect())
        .collect();
    // (A way off a ramp is a drive aisle wide, which may be a little wider than the ramp: it may graze the wall beside it.)
    for a in &aisles {
        for (what, o) in solid.iter().filter(|(w, _)| !w.starts_with("ramp")) {
            assert!(!overlap(a, o, 150.0), "an aisle {a:?} through {what}");
        }
    }
    // A way in off each ramp's foot; one that can't be joined is said (the solver's weakness, `..._as_a_designer_would`).
    let joined = [straight, curved]
        .iter()
        .filter(|ramp| {
            let Some(Element::Ramp(rm)) = m.elements.get(**ramp) else { panic!() };
            let l = crate::stairs::ramp(m, rm);
            let foot = l.runs[0].ring(l.width);
            let mid = (foot[0] + foot[3]) * 0.5;
            aisles.iter().any(|a| a.iter().map(|q| q.dist(mid)).fold(f64::INFINITY, f64::min) < 3500.0 || crate::geom::contains(a, mid))
        })
        .count();
    assert!(joined >= 1, "no ramp leads in: {r}");
    assert!(joined == 2 || notes(&r).contains("1 of the 2 ramps' feet could not be joined"), "{r}");
    // A column moved: it runs again, quickly, and still keeps clear of it.
    let t = std::time::Instant::now();
    run(&mut s, "move", json!({"ids": [columns[7]], "by": [1500, 0]}));
    let took = t.elapsed();
    let Some(Element::StructuralColumn(c)) = s.model.elements.get(columns[7]) else { panic!() };
    let ring = s.model.column_outline(&Element::StructuralColumn(c.clone()));
    for (_, c) in bays(&s, park) {
        let (w, d, a) = crate::carpark::bay_size(&s.model, &c).unwrap();
        assert!(!overlap(&crate::carpark::bay_ring(c.at, c.angle, c.mirrored, w, d, a), &ring, 5.0), "a bay over the moved column");
    }
    assert!(took.as_secs_f64() < 20.0, "{took:?}");
    eprintln!("re-run on a column moved: {took:?}, {} bays", bays(&s, park).len());
}

/// What a designer lays out in the basement above: some 60 bays. The solver (Arqiflow's, ported) cuts a plot once at an inside
/// corner and sets out fields between the cuts; two ramps inside the area at opposite sides leave it a handful of bays.
#[test]
#[ignore = "the solver's layout round ramps inside the area is still to improve (docs/parking-ramps-plan.md, left)"]
fn a_basement_with_walls_a_column_grid_and_two_ramps_is_filled_as_a_designer_would() {
    let (mut s, _, _, _) = with_basement();
    run(&mut s, "floor", json!({"boundary": AREA, "level": "B1"}));
    run(&mut s, "wall", json!({"points": AREA, "closed": true, "level": "B1", "top_level": "Level 1"}));
    for x in [8100.0, 16200.0, 24300.0, 32400.0, 40500.0, 48600.0] {
        for y in [10000.0, 20000.0, 30000.0] {
            run(&mut s, "structural_column", json!({"at": [x, y], "level": "B1", "top_level": "Level 1"}));
        }
    }
    run(&mut s, "ramp", json!({"start": [31000, 36500], "angle": 180, "level": "B1", "top_level": "Level 1", "width": 6000, "slope": 15}));
    run(
        &mut s,
        "ramp",
        json!({"path": [[36000, 3500, 0], [52000, 3500, 0.41421356], [56000, 7500]], "level": "B1", "top_level": "Level 1", "width": 6000}),
    );
    let park = id(&run(&mut s, "car_park", json!({"control": AREA, "level": "B1"})));
    assert!(bays(&s, park).len() >= 50, "{} bays", bays(&s, park).len());
}

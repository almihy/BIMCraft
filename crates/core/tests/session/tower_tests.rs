//! The benchmark towers (`sample::tower`).

use crate::sample::tower::*;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom;
use crate::model::Room;
use crate::model::{Element, Guid, Id, Model};

fn rooms(m: &Model) -> Vec<&Room> {
    m.elements
        .values()
        .filter_map(|e| match e {
            Element::Room(r) => Some(r),
            _ => None,
        })
        .collect()
}

#[test]
fn a_tower_storey_is_about_ten_thousand_elements_and_every_room_bounds() {
    let m = tower(2).unwrap();
    m.check().unwrap();
    assert_eq!(m.levels().len(), 3);
    let n = m.elements.len();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in m.elements.values() {
        *counts.entry(e.category()).or_default() += 1;
    }
    assert!((19_800..20_400).contains(&n), "{n} elements in two storeys: {counts:?}");
    for category in [
        "wall",
        "window",
        "door",
        "room",
        "ceiling",
        "floor",
        "structuralcolumn",
        "beam",
        "shaft",
        "component",
        "pipe",
        "conduit",
        "cabletray",
        "tag",
    ] {
        assert!(m.elements.values().any(|e| e.category() == category), "{category}");
    }
    let all = rooms(&m);
    assert_eq!(all.len(), 2 * 195);
    for r in &all {
        assert!(m.room_region(r).is_some(), "room {} ({}) isn't enclosed", r.number, r.name);
    }
    let numbers: std::collections::BTreeSet<&str> = all.iter().map(|r| r.number.as_str()).collect();
    assert_eq!(numbers.len(), all.len(), "room numbers are the project's own");
    assert!(numbers.contains("1001") && numbers.contains("2195"));
    // The corridor runs round the inner block, an island of it: between the two rings' centre lines, less the columns
    // standing on its outer ring, which bound rooms (as Revit's do): half of each along its sides, a quarter at its
    // corners.
    let corridor = all.iter().find(|r| r.name == "Corridor").unwrap();
    let area = m.room_region(corridor).unwrap().area();
    let Plate { width, depth } = Plate::REFERENCE;
    let ring = (width - 12_000) * (depth - 12_000) - (width - 16_000) * (depth - 16_000);
    let column = m.elements.values().find(|e| e.category() == "structuralcolumn").map(|e| geom::area(&m.column_outline(e)).abs());
    let halves = 2 * ((width - 12_000) / 6000 - 1) + 2 * ((depth - 12_000) / 6000 - 1);
    let expected = ring as f64 - column.unwrap() * (halves as f64 / 2.0 + 1.0);
    assert!((area - expected).abs() < 1e3, "{area} mm² against {expected}");
}

/// Whether `e` is drawn in one view only: annotation and detailing.
fn two_d(e: &Element) -> bool {
    matches!(e, Element::Text(_) | Element::DetailLine(_) | Element::FilledRegion(_) | Element::Dimension(_))
}

#[test]
fn an_annotated_storey_has_two_thousand_2d_elements_in_its_own_plan_and_dimensions_on_its_own_walls() {
    let plain = tower(2).unwrap();
    let m = annotated(Plate::REFERENCE, 2).unwrap();
    m.check().unwrap();
    // The same building, with the 2D on top of it.
    assert_eq!(m.elements.values().filter(|e| !two_d(e)).count(), plain.elements.len());
    let levels: Vec<Guid> = m.levels().iter().map(|(id, _)| m.guid(*id)).collect();
    for (storey, level) in levels.iter().take(2).enumerate() {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for e in m.elements.values() {
            let view = match e {
                Element::Text(n) => n.view,
                Element::DetailLine(l) => l.view,
                Element::FilledRegion(r) => r.view,
                Element::Dimension(d) => d.view.unwrap_or(d.level),
                _ => continue,
            };
            if view == *level {
                *counts.entry(e.category()).or_default() += 1;
            }
        }
        let all: usize = counts.values().sum();
        assert!(all >= 1800, "storey {storey}: {counts:?}");
        for (category, least) in [("line", 1000), ("text", 250), ("region", 100), ("dimension", 90)] {
            assert!(counts.get(category).copied().unwrap_or(0) >= least, "storey {storey}: {category} in {counts:?}");
        }
    }
    assert!(levels.get(2).is_some_and(|top| !m.elements.values().any(|e| match e {
        Element::Text(n) => n.view == *top,
        _ => false,
    })));
    // A dimension measures the walls of its own storey, as the storey it was copied from did.
    for e in m.elements.values() {
        let Element::Dimension(d) = e else { continue };
        assert!(d.refs.len() >= 2);
        for r in &d.refs {
            let wall = m.wall(r.id).unwrap_or_else(|| panic!("a dimension measures {r:?}, not a wall"));
            assert_eq!(wall.level, d.level);
        }
    }
}

#[test]
fn a_drawn_tower_has_each_storeys_sheets_and_the_projects_once() {
    let plain = tower_on(Plate::SMALL, 3).unwrap();
    let m = drawn(Plate::SMALL, 3, Drawings { annotation: false, sheets: true }).unwrap();
    m.check().unwrap();
    let sheets: BTreeMap<&str, Id> =
        m.elements.iter().filter_map(|(id, e)| if let Element::Sheet(sh) = e { Some((sh.number.as_str(), *id)) } else { None }).collect();
    let mut numbers: Vec<&str> = sheets.keys().copied().collect();
    numbers.sort_unstable();
    let mut expected = vec!["A0-000", "A5-001", "A5-002", "A6-001", "A7-001", "A7-002"];
    for series in 1..=4 {
        for storey in 1..=3 {
            expected.push(Box::leak(format!("A{series}-{storey:03}").into_boxed_str()));
        }
    }
    expected.sort_unstable();
    assert_eq!(numbers, expected);
    // What's on a sheet: storey 2's floor plan is Level 2's, its halves Level 2's dependents; the project's once.
    let on = |number: &str| -> Vec<String> {
        let g = m.guid(sheets[number]);
        m.elements
            .values()
            .filter_map(|e| match e {
                Element::Viewport(v) if v.sheet == g => m.view_name(v.view).map(str::to_string),
                _ => None,
            })
            .collect()
    };
    assert_eq!(on("A1-002"), ["Level 2"]);
    assert_eq!(on("A2-003"), ["Level 3 - Ceiling Plan"]);
    assert_eq!(on("A3-002"), ["Level 2 - North Half"]);
    assert_eq!(on("A5-001"), ["North Elevation", "South Elevation"]);
    assert_eq!(on("A7-002"), ["Door Schedule", "Window Schedule"]);
    for e in m.elements.values() {
        if let Element::PlanView(v) = e {
            let level = m.levels().into_iter().find(|(id, _)| m.guid(*id) == v.level).unwrap().1.name.clone();
            assert!(v.name.starts_with(&format!("{level} ")), "{} of {level}", v.name);
        }
    }
    assert_eq!(m.elements.values().filter(|e| matches!(e, Element::Schedule(_))).count(), 3);
    let notes = m.elements.values().filter(|e| matches!(e, Element::Text(n) if sheets.values().any(|s| m.guid(*s) == n.view))).count();
    assert_eq!(notes, 8 * sheets.len());
    // The same building under the drawings.
    let drawing = |e: &Element| {
        matches!(
            e,
            Element::Sheet(_)
                | Element::Viewport(_)
                | Element::Text(_)
                | Element::PlanView(_)
                | Element::Section(_)
                | Element::Elevation(_)
                | Element::Schedule(_)
                | Element::View3D(_)
        )
    };
    assert_eq!(m.elements.values().filter(|e| !drawing(e)).count(), plain.elements.values().filter(|e| !drawing(e)).count());
    // A storey's sheet draws its plan.
    let d = crate::sheet::draw(&m, sheets["A1-003"]).unwrap();
    assert!(d.prims.len() > 1000, "{} primitives", d.prims.len());
}

#[test]
fn sheets_views_and_viewports_are_checked_with_the_drawing_set_not_the_whole_model() {
    let mut s = Session::new();
    s.reset(drawn(Plate::SMALL, 2, Drawings { annotation: false, sheets: true }).unwrap(), None);
    let fallbacks = || crate::model::FALLBACKS.with(std::cell::Cell::get);
    let before = fallbacks();
    let viewport = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Viewport(_)).then_some(*id)).unwrap();
    // The engine holds each to the full check's answer (`check_changed differs from check`).
    s.execute("sheet", &json!({"number": "X-001", "name": "Extra"})).unwrap();
    s.execute("place", &json!({"sheet": "X-001", "view": "Wall Schedule"})).unwrap();
    s.execute("move", &json!({"ids": [viewport], "by": [10, 10]})).unwrap();
    s.execute("duplicate_view", &json!({"view": "Level 2", "name": "Level 2 - Furniture"})).unwrap();
    s.execute("text", &json!({"text": "Revised", "at": [100, 100], "view": "X-001"})).unwrap();
    let err = |s: &mut Session, cmd: &str, p: Value| s.execute(cmd, &p).unwrap_err();
    assert!(err(&mut s, "sheet", json!({"number": "A1-002"})).contains("already numbered `A1-002`"));
    assert!(err(&mut s, "place", json!({"sheet": "X-001", "view": "Level 1"})).contains("is on sheet A1-001 already"));
    let e = err(&mut s, "duplicate_view", json!({"view": "Level 2", "name": "Level 1"}));
    assert!(e.contains("Level 1"), "{e}");
    for id in s.model.sheets() {
        s.execute("delete", &json!({"ids": [id]})).unwrap();
        s.execute("undo", &json!({})).unwrap();
    }
    assert_eq!(fallbacks(), before, "a sheet's edit checked the whole model");
}

#[test]
fn every_plan_finds_from_the_lookup_what_reaches_it_from_other_storeys_and_after_a_type_grows() {
    let mut s = Session::new();
    s.reset(drawn(Plate::SMALL, 3, Drawings { annotation: true, sheets: true }).unwrap(), None);
    let plans = |s: &Session| -> Vec<Id> {
        s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Level(_) | Element::PlanView(_))).map(|(id, _)| *id).collect()
    };
    // Each plan's elements are asked of every element too (the twin in `render::plan_elements`, and `plan_shows`').
    let draw_all = |s: &Session| {
        for id in plans(s) {
            crate::render::plan(&s.model, id, 400, 300);
        }
    };
    draw_all(&s);
    for (cmd, p) in [
        // A luminaire raised into the storey over its level, and one let down into the storey under.
        ("component", json!({"family": "Ceiling Luminaire", "at": [3000, 3000], "level": "Level 1", "offset": 5200})),
        ("component", json!({"family": "Ceiling Luminaire", "at": [3000, 9000], "level": "Level 3", "offset": -3000})),
        // A riser through two storeys, and a pipe too steep to band.
        ("pipe", json!({"points": [[1000, 1000], [1000, 1000]], "level": "Level 1", "offset": 0, "end_offset": 9000})),
        ("pipe", json!({"points": [[2000, 2000], [2100, 2000]], "level": "Level 1", "offset": 0, "end_offset": 6000})),
        // A beam deep enough to hang into the storey under its level.
        ("type", json!({"category": "beam", "name": "Transfer", "width": 400, "depth": 2500})),
        ("beam", json!({"points": [[0, 18_000], [36_000, 18_000]], "level": "Level 3", "type": "Transfer"})),
    ] {
        s.execute(cmd, &p).unwrap();
    }
    draw_all(&s);
    // The library changes; what the lookup keeps of the elements doesn't.
    s.execute("type", &json!({"category": "beam", "name": "Transfer", "width": 400, "depth": 5000})).unwrap();
    draw_all(&s);
    s.execute("move", &json!({"ids": [plans(&s)[0]], "by": [0, 0]})).ok();
    draw_all(&s);
}

#[test]
fn a_tower_is_one_to_a_hundred_storeys_on_a_plate_its_layout_fits() {
    assert!(tower(0).is_err() && tower(MAX_STOREYS + 1).is_err());
    for (width, depth) in [(30_000, 36_000), (36_000, 42_000), (36_000, 30_000), (0, 0), (-36_000, 36_000), (606_000, 36_000)] {
        assert!(tower_on(Plate { width, depth }, 1).is_err(), "{width} × {depth}");
    }
}

#[test]
fn a_small_tower_has_the_same_layout() {
    let m = tower_on(Plate::SMALL, 2).unwrap();
    m.check().unwrap();
    let all = rooms(&m);
    // Offices: 12 a side and 8 an end; the corridor; meeting rooms: 5 a side, 2 an end; 2 in the core.
    assert_eq!(all.len(), 2 * (40 + 1 + 14 + 2));
    for r in &all {
        assert!(m.room_region(r).is_some(), "room {} ({}) isn't enclosed", r.number, r.name);
    }
    assert_eq!(m.elements.values().filter(|e| e.category() == "shaft").count(), 2 * 2);
}

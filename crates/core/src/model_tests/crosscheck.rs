//! `crosscheck`'s tests that drive the Session (moved out of `crosscheck.rs`).

use crate::geom::P2;
use crate::model::{Element, Model};
use crate::rooms::Rooms;
use crate::walls::{self, Axis};
use serde_json::{Value, json};

use crate::crosscheck::*;
use crate::sample::tower::{Plate, tower_on};

fn rows(m: &Model, kind: &str) -> usize {
    m.elements.values().filter(|e| e.category() == kind).count()
}

#[test]
fn a_description_holds_what_another_tool_builds_the_model_again_from() {
    let m = tower_on(Plate::SMALL, 1).unwrap();
    let d = describe(&m);
    assert_eq!((d["format"].as_str(), d["version"].as_u64(), d["units"].as_str()), (Some(FORMAT), Some(1), Some("mm")));
    let len = |k: &str| d[k].as_array().map_or(0, Vec::len);
    assert_eq!(len("walls"), rows(&m, "wall"));
    assert_eq!(len("floors"), rows(&m, "floor"));
    assert_eq!(len("rooms"), rows(&m, "room"));
    assert_eq!(len("openings"), rows(&m, "door") + rows(&m, "window"));
    assert!(len("rooms") > 50 && len("openings") > 100, "{} rooms, {} openings", len("rooms"), len("openings"));
    // Every wall's type, level and every opening's host are in it.
    let guids = |k: &str| -> Vec<String> { d[k].as_array().unwrap().iter().map(|v| v["guid"].as_str().unwrap().into()).collect() };
    let (levels, walls) = (guids("levels"), guids("walls"));
    let types: Vec<&str> = d["wall_types"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for w in d["walls"].as_array().unwrap() {
        assert!(levels.contains(&w["level"].as_str().unwrap().to_string()));
        assert!(types.contains(&w["type"].as_str().unwrap()));
    }
    for o in d["openings"].as_array().unwrap() {
        assert!(walls.contains(&o["host"].as_str().unwrap().to_string()));
        assert!(o["width"].as_f64().unwrap() > 0.0);
    }
    // A wall's centre line is its location line moved to its middle: as long, and its type's half thickness from
    // each face.
    let (_, g, e) = m.elements.with_guids().find(|(_, _, e)| matches!(e, Element::Wall(_))).unwrap();
    let Element::Wall(w) = e else { panic!("a wall") };
    let dw = d["walls"].as_array().unwrap().iter().find(|v| v["guid"] == json!(g)).unwrap();
    let p = |v: &Value| P2(v[0].as_f64().unwrap(), v[1].as_f64().unwrap());
    let (s, t) = (p(&dw["start"]), p(&dw["end"]));
    assert!((s.dist(t) - w.start.dist(w.end)).abs() < 1e-6);
    let (ext, int) = walls::faces(&m, w);
    let off = Axis::of(w).project(s).1;
    assert!((off - (ext + int) / 2.0).abs() < 1e-6, "{off} {ext} {int}");
    assert!(d["left_out"]["tag"].as_u64().unwrap() > 0, "{}", d["left_out"]);
}

#[test]
fn our_numbers_are_those_the_model_shows() {
    let m = tower_on(Plate::SMALL, 1).unwrap();
    let rows = ours(&m);
    let rooms = Rooms::new(&m);
    let mut n = 0;
    for (id, _, e) in m.elements.with_guids() {
        let shown = crate::json::element_json_with(&m, id, &rooms);
        let of = |q: &str| rows.iter().find(|r| r.id == id && r.quantity == q).map(|r| r.value);
        match e {
            Element::Room(_) => {
                assert!((of("area_m2").unwrap() - shown["area_m2"].as_f64().unwrap()).abs() < 0.006);
                assert!((of("perimeter_m").unwrap() - shown["perimeter_m"].as_f64().unwrap()).abs() < 0.006);
                assert_eq!(of("enclosed"), Some(if shown["enclosed"] == json!(true) { 1.0 } else { 0.0 }));
                assert!((of("volume_m3").unwrap() - shown["volume_m3"].as_f64().unwrap()).abs() < 0.006);
                assert!((of("height_m").unwrap() * 1e3 - shown["unbounded_height"].as_f64().unwrap()).abs() < 1e-6);
                n += 1;
            }
            Element::Floor(_) => {
                assert!((of("area_m2").unwrap() - shown["area_m2"].as_f64().unwrap()).abs() < 0.006);
                n += 1;
            }
            Element::Wall(w) => assert!((of("length_m").unwrap() * 1e3 - w.start.dist(w.end)).abs() < 1e-6),
            _ => {}
        }
    }
    assert!(n > 50);
    // Every room of the tower bounds.
    assert!(rows.iter().filter(|r| r.quantity == "enclosed").all(|r| r.value == 1.0));
}

/// The small tower, two storeys, as Revit reports it (`tools/revit-crosscheck`): every quantity both have the same,
/// within [`tolerance`].
#[test]
#[ignore = "needs Revit's numbers: run tools/revit-crosscheck and commit its table as tests/revit/small-tower-2.csv"]
fn the_small_tower_has_the_numbers_revit_gives_it() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/revit/small-tower-2.csv");
    let theirs = read(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let m = tower_on(Plate::SMALL, 2).unwrap();
    let report = compare(&ours(&m), &theirs);
    assert!(report.compared > 1000, "{}", report.to_json(5));
    assert!(report.differ.is_empty(), "{}", report.to_json(20));
}

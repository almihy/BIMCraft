//! `inquiry`'s tests that drive the Session (moved out of `inquiry.rs`).

use crate::model::Id;

use serde_json::json;

use crate::engine::Session;
use crate::inquiry::*;

fn house() -> (Session, Vec<Id>) {
    let mut s = Session::new();
    let r = s.execute("wall", &json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true})).unwrap();
    let walls = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    (s, walls)
}

#[test]
fn an_element_is_found_by_its_id_its_guid_however_written_or_its_ifc_globalid() {
    let (s, walls) = house();
    let w = walls[1];
    let g = s.model.guid(w).to_string();
    let ifc = crate::ifc::compress(s.model.guid(w).as_u128());
    for word in [w.to_string(), g.clone(), g.to_uppercase(), format!("{{{g}}}"), g.replace('-', ""), ifc, format!("\"{w}\"")] {
        assert_eq!(one(&s.model, &word), Some(w), "{word}");
    }
    for word in ["", "x", "99", "100099", "00000000-0000-4000-8000-000000000000", "0123456789abcdefghijkl"] {
        assert_eq!(one(&s.model, word), None, "{word}");
    }
}

#[test]
fn a_list_finds_each_element_once_in_order_and_says_what_named_none() {
    let (s, walls) = house();
    let g = s.model.guid(walls[2]).to_string();
    let text = format!("{}, {g};{}\n{}  nowhere 100099 {}", walls[3], walls[0], walls[3], walls[2]);
    assert_eq!(
        find(&s.model, &text),
        Found { ids: vec![walls[3], walls[2], walls[0]], library: Vec::new(), missing: vec!["nowhere".into(), "100099".into()] }
    );
    assert_eq!(find(&s.model, " \n,, ;"), Found::default());
    // A hostile list: long, and of many names, is cut short, never a hang or a panic.
    let long = "100000 ".repeat(MAX_LIST);
    assert_eq!(find(&s.model, &long).ids, [100_000]);
    let many = (0..2 * MAX_NAMES).map(|i| i.to_string()).collect::<Vec<_>>().join(" ");
    assert_eq!(find(&s.model, &many).missing.len(), MAX_NAMES);
    assert!(find(&s.model, &"é".repeat(MAX_LIST)).ids.is_empty(), "cut on a character's edge");
}

#[test]
fn an_element_is_named_by_its_id_guid_and_ifc_globalid() {
    let (s, walls) = house();
    let n = names(&s.model, walls[0]).unwrap();
    assert_eq!((n.id, n.guid, n.category.as_str(), n.name.as_str()), (walls[0], s.model.guid(walls[0]), "wall", "Generic - 200mm"));
    assert_eq!(n.ifc.len(), 22);
    assert_eq!(crate::ifc::expand(&n.ifc), Some(n.guid.as_u128()));
    assert_eq!(names(&s.model, 100_000).unwrap().name, "Level 1");
    assert_eq!(names(&s.model, 5), None);
}

#[test]
fn elements_show_in_the_plan_of_their_level_where_they_are_drawn() {
    let (mut s, walls) = house();
    // The east wall, on Level 1: from its outer face to its inner one, south to north.
    let (level, [lo, hi]) = shown(&s.model, &[walls[1]]).unwrap();
    assert_eq!(level, 100_000);
    assert!((lo.0 - 7900.0).abs() < 50.0 && (hi.0 - 8100.0).abs() < 50.0 && lo.1 < 0.0 && hi.1 > 6000.0, "{lo:?} {hi:?}");
    let up = s.execute("wall", &json!({"points": [[0, 0], [3000, 0]], "level": "Level 2"})).unwrap()["ids"][0].as_u64().unwrap();
    assert_eq!(shown(&s.model, &[up, walls[0]]).unwrap().0, 100_001, "the first's level");
    assert_eq!(shown(&s.model, &[100_001]).map(|(l, _)| l), None, "a level draws nothing in its own plan");
    let sheet = s.execute("sheet", &json!({})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(shown(&s.model, &[sheet]), None);
}

#[test]
fn the_commands_find_and_name_elements_and_change_nothing() {
    let (mut s, walls) = house();
    let (steps, before) = (s.history().len(), s.model.clone());
    let g = s.model.guid(walls[0]).to_string();
    let r = s.execute("select_by_id", &json!({"text": format!("{g} {} nope", walls[1])})).unwrap();
    assert_eq!((r["ids"].clone(), r["missing"].clone(), r["plan"].clone()), (json!([walls[0], walls[1]]), json!(["nope"]), json!(100_000)));
    assert!(r["extent"][0][0].as_f64().unwrap() < 0.0 && r["extent"][1][0].as_f64().unwrap() > 8000.0, "{r}");
    let r = s.execute("select_by_id", &json!({"text": [walls[2], g]})).unwrap();
    assert_eq!(r["ids"], json!([walls[2], walls[0]]), "a list of them too");
    let r = s.execute("ids_of", &json!({"ids": [walls[0], 100_000]})).unwrap();
    assert_eq!(r["elements"][0]["guid"], json!(g));
    assert_eq!(r["elements"][1]["category"], "level");
    assert_eq!(r["elements"][0]["ifc"].as_str().map(str::len), Some(22));
    for bad in [json!({}), json!({"text": 5}), json!(null)] {
        assert!(s.execute("select_by_id", &bad).is_err(), "{bad}");
    }
    assert!(s.execute("ids_of", &json!({"ids": [5]})).is_err());
    assert_eq!((s.history().len(), &s.model), (steps, &before), "reading changes nothing and is no undo step");
}

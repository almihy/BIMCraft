//! Type identity data (Type Mark, Manufacturer, Fire Rating…) and the quantities schedules show (area, volume, perimeter):
//! set by the `type` and `family` commands, kept by a file, read by schedules and tags.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::graphics::testing::{house, run};
use crate::model::{Element, Model};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * b.abs().max(1.0)
}

/// A house (an 8 x 6 m box of walls) with a door and a window in its first wall.
fn with_openings() -> (Session, Vec<u64>, u64, u64) {
    let mut s = Session::new();
    let walls = house(&mut s);
    let door = run(&mut s, "door", json!({"at": [2000, 0]}))["id"].as_u64().unwrap_or_default();
    let window = run(&mut s, "window", json!({"at": [4000, 0]}))["id"].as_u64().unwrap_or_default();
    (s, walls, door, window)
}

fn first_type(m: &Model, category: &str) -> String {
    m.type_names(category).first().map(|n| n.to_string()).unwrap_or_default()
}

#[test]
fn a_type_holds_identity_data_and_a_file_keeps_only_what_is_set() {
    let (mut s, ..) = with_openings();
    let wall = first_type(&s.model, "wall");
    // Nothing set: nothing in the file, so a file from before reads the same.
    let before = serde_json::to_string(&*s.model).unwrap();
    assert!(!before.contains("type_mark") && !before.contains("identity"));
    let r = run(
        &mut s,
        "type",
        json!({"category": "wall", "name": wall, "type_mark": " W1 ", "description": "Brick, rendered", "manufacturer": "Acme",
               "model": "X-200", "fire_rating": "EI 60", "cost": 120.5, "keynote": "04 21 13", "assembly_code": "B2010",
               "url": "https://example.com/w1", "type_comments": "see detail 4"}),
    );
    assert_eq!(r["type"]["identity"]["type_mark"], "W1", "{r}");
    assert_eq!(r["type"]["identity"]["cost"], 120.5);
    // A later change to the type keeps it, and a field cleared goes from the file.
    run(&mut s, "type", json!({"category": "wall", "name": wall, "thickness": 250}));
    assert_eq!(s.model.types.wall[&wall].identity.fire_rating, "EI 60");
    run(&mut s, "type", json!({"category": "wall", "name": wall, "fire_rating": "", "cost": null}));
    let identity = &s.model.types.wall[&wall].identity;
    assert!(identity.fire_rating.is_empty() && identity.cost.is_none() && identity.type_mark == "W1");
    let text = serde_json::to_string(&*s.model).unwrap();
    assert!(text.contains("\"type_mark\":\"W1\"") && !text.contains("fire_rating") && !text.contains("\"cost\""));
    // Read back from a file, whole.
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.types.wall[&wall], s.model.types.wall[&wall]);
    // Any other category's types hold it too.
    for (category, extra) in [
        ("door", json!({})),
        ("window", json!({})),
        ("floor", json!({})),
        ("roof", json!({})),
        ("ceiling", json!({})),
        ("column", json!({})),
        ("railing", json!({})),
        ("beam", json!({})),
    ] {
        let name = first_type(&s.model, category);
        let mut p = json!({"category": category, "name": name, "type_mark": "T1", "manufacturer": "Acme"});
        if let (Some(p), Some(extra)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(extra.clone());
        }
        if name.is_empty() {
            // A category with no type yet: a new one.
            p["name"] = json!("New");
            p["width"] = json!(300);
            p["depth"] = json!(500);
            p["thickness"] = json!(200);
            p["height"] = json!(1000);
        }
        let r = run(&mut s, "type", p);
        assert_eq!(r["type"]["identity"]["manufacturer"], "Acme", "{category}: {r}");
        let name = r["name"].as_str().unwrap_or_default().to_string();
        assert_eq!(s.model.identity_of(category, &name).map(|i| i.type_mark.as_str()), Some("T1"), "{category}");
    }
}

#[test]
fn identity_data_that_cannot_be_held_is_refused() {
    let (mut s, ..) = with_openings();
    let wall = first_type(&s.model, "wall");
    for (bad, says) in [
        (json!({"cost": -1}), "cost"),
        (json!({"cost": "cheap"}), "number"),
        (json!({"manufacturer": 4}), "text"),
        (json!({"model": "a\nb"}), "one line"),
        (json!({"url": "x".repeat(2000)}), "characters"),
    ] {
        let mut p = json!({"category": "wall", "name": wall});
        if let Some(o) = p.as_object_mut() {
            o.extend(bad.as_object().cloned().unwrap_or_default());
        }
        let e = s.execute("type", &p).unwrap_err();
        assert!(e.contains(says), "{bad}: {e}");
    }
    assert!(s.model.types.wall[&wall].identity.is_empty());
}

#[test]
fn a_types_elements_read_its_identity_data_as_schedule_fields_and_in_tags() {
    let (mut s, walls, door, window) = with_openings();
    let wall_type = match s.model.elements.get(walls[0]) {
        Some(Element::Wall(w)) => w.type_name.clone(),
        _ => panic!("no wall"),
    };
    // The door the tool places is of one of the project's own types or of a family's.
    let (family, door_type) = match s.model.elements.get(door) {
        Some(Element::Door(o)) => (o.family.clone(), o.type_name.clone()),
        _ => panic!("no door"),
    };
    let data = json!({"type_mark": "D1", "fire_rating": "EI 30", "cost": 310});
    match family {
        Some(f) => run(&mut s, "family", json!({"name": f, "identity": {door_type: data}})),
        None => run(&mut s, "type", json!({"category": "door", "name": door_type, "type_mark": "D1", "fire_rating": "EI 30", "cost": 310})),
    };
    run(&mut s, "type", json!({"category": "wall", "name": wall_type, "type_mark": "W1", "manufacturer": "Acme"}));
    // The element's JSON says it, for agents.
    let j = crate::json::element_json(&s.model, door);
    assert_eq!((j["type_mark"].as_str(), j["fire_rating"].as_str(), j["cost"].as_f64()), (Some("D1"), Some("EI 30"), Some(310.0)));
    assert!(crate::json::element_json(&s.model, window).get("type_mark").is_none());
    // Fields of a door schedule, and of a multi-category one.
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Door Schedule", "fields": ["Mark", "Type Mark", "Fire Rating", {"name": "Cost", "total": true}]}),
    );
    assert_eq!(r["columns"][1], "Type Mark", "{r}");
    assert_eq!(r["rows"][0][1], "D1");
    assert_eq!(r["rows"][0][2], "EI 30");
    assert_eq!(r["totals"][3], 310.0);
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Everything", "category": "multicategory", "categories": ["wall", "door"], "fields": ["Category", "Type Mark", "Manufacturer"]}),
    );
    let rows: Vec<(String, String, String)> = r["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row[0].as_str().unwrap_or_default().into(),
                row[1].as_str().unwrap_or_default().into(),
                row[2].as_str().unwrap_or_default().into(),
            )
        })
        .collect();
    assert!(rows.contains(&("door".into(), "D1".into(), String::new())), "{rows:?}");
    assert!(rows.contains(&("wall".into(), "W1".into(), "Acme".into())), "{rows:?}");
    // A tag shows one, as it shows a door's mark.
    let tag = run(&mut s, "tag", json!({"ids": [door], "label": "Type Mark"}));
    let t = tag["ids"][0].as_u64().unwrap_or_default();
    assert_eq!(crate::json::element_json(&s.model, t)["text"], "D1", "{tag}");
    // Filtering and sorting by one.
    let r = run(
        &mut s,
        "schedule",
        json!({"name": "Walls", "category": "wall", "fields": ["Id", "Type Mark"], "filters": [{"field": "Type Mark", "op": "equals", "value": "W1"}]}),
    );
    assert_eq!(r["rows"].as_array().map(Vec::len), Some(walls.len()));
    // A project parameter of the same name takes a field's place in a schedule, as it did before the field.
    run(&mut s, "parameter", json!({"name": "Fire Rating", "categories": ["wall"], "kind": "number"}));
    run(&mut s, "modify", json!({"id": walls[0], "params": {"Fire Rating": 60}}));
    let r = run(&mut s, "schedule", json!({"name": "Walls again", "category": "wall", "fields": ["Id", "Fire Rating"]}));
    assert!(
        r["rows"].as_array().unwrap().iter().any(|row| row[0].as_f64() == Some(walls[0] as f64) && row[1].as_f64() == Some(60.0)),
        "{r}"
    );
}

#[test]
fn a_family_types_identity_data_follows_the_type_and_reaches_the_schedule() {
    let mut s = Session::new();
    house(&mut s);
    run(
        &mut s,
        "family",
        json!({"name": "Chair", "category": "furniture", "parameters": [{"name": "Width", "kind": "length", "value": 500}],
               "types": {"A": {"Width": 500}, "B": {"Width": 600}},
               "forms": [{"plane": "xy", "profile": [[0, 0], [500, 0], [500, 500], [0, 500]], "from": 0, "to": 400}],
               "identity": {"A": {"type_mark": "C1", "manufacturer": "Ikea", "cost": 49.9}}}),
    );
    let e = s.execute("family", &json!({"name": "Chair", "identity": {"Nope": {"type_mark": "x"}}})).unwrap_err();
    assert!(e.contains("no type `Nope`"), "{e}");
    let id = run(&mut s, "component", json!({"family": "Chair", "type": "A", "at": [1000, 1000]}))["id"].as_u64().unwrap_or_default();
    let j = crate::json::element_json(&s.model, id);
    assert_eq!((j["type_mark"].as_str(), j["manufacturer"].as_str()), (Some("C1"), Some("Ikea")), "{j}");
    let r = run(&mut s, "schedule", json!({"name": "Chairs", "category": "component", "fields": ["Type", "Type Mark", "Manufacturer"]}));
    assert_eq!(r["rows"], json!([["A", "C1", "Ikea"]]), "{r}");
    // A file keeps it; a rename follows it; a type that goes takes it along.
    let (back, left_out) = Model::read(&serde_json::to_string(&*s.model).unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(back.families["Chair"].identity["A"].type_mark, "C1");
    run(&mut s, "rename_type", json!({"family": "Chair", "type": "A", "name": "Armless"}));
    assert_eq!(s.model.families["Chair"].identity["Armless"].manufacturer, "Ikea");
    assert!(!s.model.families["Chair"].identity.contains_key("A"));
    run(&mut s, "family", json!({"name": "Chair", "identity": {"Armless": {"type_mark": ""}, "B": {"model": "Plain"}}}));
    assert_eq!(s.model.families["Chair"].identity["Armless"].type_mark, "");
    run(&mut s, "family", json!({"name": "Chair", "types": {"B": null}}));
    assert!(!s.model.families["Chair"].identity.contains_key("B"), "a type that goes takes its identity data");
}

#[test]
fn a_wall_schedule_has_area_and_volume_less_its_openings() {
    let (mut s, walls, door, window) = with_openings();
    let model = std::sync::Arc::clone(&s.model);
    let m = &*model;
    let Some(Element::Wall(wl)) = m.elements.get(walls[0]) else { panic!("no wall") };
    let (height, thick) = (m.top_z(wl) - m.base_z(wl), m.thickness(wl));
    let size = |id: u64| m.opening_type(m.elements.get(id).unwrap()).unwrap();
    let sill = match m.elements.get(window) {
        Some(Element::Window(o)) => o.sill,
        _ => panic!("no window"),
    };
    let (d, wn) = (size(door), size(window));
    let net = (8000.0 * height - d.width * d.height - wn.width * wn.height.min(height - sill)) / 1e6;
    let r = run(&mut s, "schedule", json!({"name": "Wall Schedule"}));
    let columns: Vec<&str> = r["columns"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    let (a, v) = (columns.iter().position(|c| c.starts_with("Area")), columns.iter().position(|c| c.starts_with("Volume")));
    assert!(a.is_some() && v.is_some(), "the wall schedule has Area and Volume: {columns:?}");
    let (a, v) = (a.unwrap_or_default(), v.unwrap_or_default());
    let rows = r["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    // Rows come in the walls' order: the bottom wall holds the door and the window; the others, nothing.
    let by_id: Vec<(f64, f64)> =
        rows.iter().map(|row| (row[a].as_f64().unwrap_or_default(), row[v].as_f64().unwrap_or_default())).collect();
    assert!(
        by_id.iter().any(|&(area, volume)| (area - net).abs() < 0.006 && (volume - net * thick / 1e3).abs() < 0.006),
        "{by_id:?} vs {net}"
    );
    let full = 6000.0 * height / 1e6;
    assert!(by_id.iter().any(|&(area, _)| (area - full).abs() < 0.006), "{by_id:?}");
    // One quantity function: the schedule's cells are the IFC's numbers, to the schedule's rounding.
    let q = crate::quantities::wall(m, walls[0]).unwrap_or_else(|| panic!("a wall"));
    assert!(near(q.net_side / 1e6, net) && near(q.net_volume / 1e9, net * thick / 1e3));
    // A wall's JSON says them, for agents.
    let j = crate::json::element_json(&s.model, walls[0]);
    assert!((j["area_m2"].as_f64().unwrap_or_default() - net).abs() < 0.006, "{j}");
}

#[test]
fn floors_ceilings_and_roofs_schedule_their_area_volume_and_perimeter() {
    let mut s = Session::new();
    house(&mut s);
    let boundary = json!([[0, 0], [8000, 0], [8000, 6000], [0, 6000]]);
    let hole = json!([[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]);
    run(&mut s, "floor", json!({"boundary": boundary, "openings": hole}));
    run(&mut s, "ceiling", json!({"boundary": boundary, "openings": hole}));
    run(&mut s, "level", json!({"name": "Roof level", "elevation": 3000}));
    run(&mut s, "roof", json!({"boundary": boundary, "overhang": 0, "slope": 30}));
    let model = std::sync::Arc::clone(&s.model);
    let thick = |category: &str| {
        let n = first_type(&model, category);
        match category {
            "floor" => model.types.floor[&n].thickness,
            "ceiling" => model.types.ceiling[&n].thickness,
            _ => model.types.roof[&n].thickness,
        }
    };
    let r = run(&mut s, "schedule", json!({"name": "Floors", "category": "floor", "fields": ["Area", "Volume", "Perimeter"]}));
    let row = &r["rows"][0];
    assert!(near(row[0].as_f64().unwrap_or_default(), 47.0), "{r}");
    assert!((row[1].as_f64().unwrap_or_default() - 47.0 * thick("floor") / 1e3).abs() < 0.001, "{r}");
    assert!(near(row[2].as_f64().unwrap_or_default(), 28000.0), "{r}");
    let r = run(&mut s, "schedule", json!({"name": "Ceilings", "category": "ceiling", "fields": ["Area", "Volume", "Perimeter"]}));
    let row = &r["rows"][0];
    assert!(near(row[0].as_f64().unwrap_or_default(), 47.0), "{r}");
    assert!((row[1].as_f64().unwrap_or_default() - 47.0 * thick("ceiling") / 1e3).abs() < 0.001, "{r}");
    assert!(near(row[2].as_f64().unwrap_or_default(), 28000.0), "{r}");
    // A roof's Area is its surface: its plan stretched by its slope, every face of a hip roof at 30°.
    let r = run(&mut s, "schedule", json!({"name": "Roofs", "category": "roof", "fields": ["Plan Area", "Area", "Volume", "Perimeter"]}));
    let row = &r["rows"][0];
    let (plan, surface) = (row[0].as_f64().unwrap_or_default(), row[1].as_f64().unwrap_or_default());
    assert!(near(plan, 48.0), "{r}");
    assert!((surface - 48.0 / 30f64.to_radians().cos()).abs() < 0.01, "{r}");
    assert!((row[2].as_f64().unwrap_or_default() - surface * thick("roof") / 1e3).abs() < 0.01, "{r}");
    assert!(near(row[3].as_f64().unwrap_or_default(), 28000.0), "{r}");
}

#[test]
fn a_parameter_a_file_names_as_a_field_that_is_new_is_renamed_not_refused() {
    let mut s = Session::new();
    house(&mut s);
    // A file from before the Volume field: a project parameter of that name for walls, one wall holding a value.
    run(&mut s, "parameter", json!({"name": "Sound Rating", "categories": ["wall"], "kind": "text"}));
    let id = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).unwrap_or_default();
    run(&mut s, "modify", json!({"id": id, "params": {"Sound Rating": "45"}}));
    let mut v: Value = serde_json::to_value(&*s.model).unwrap();
    let rename = |v: &mut Value| {
        let text = serde_json::to_string(v).unwrap().replace("Sound Rating", "Volume");
        *v = serde_json::from_str(&text).unwrap();
    };
    rename(&mut v);
    let (back, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert!(back.parameters.contains_key("Custom Volume") && !back.parameters.contains_key("Volume"), "{:?}", back.parameters.keys());
    assert!(back.check().is_ok(), "{:?}", back.check());
    let held = back.elements.values().filter_map(|e| e.params()).any(|p| p.get("Custom Volume").and_then(Value::as_str) == Some("45"));
    assert!(held, "its value moved with it");
}

/// A file from before the Volume field, where `Sound Rating` was a number parameter of walls named `Volume`: `s`'s model written,
/// that name changed in it, and read.
fn read_as_volume(s: &Session) -> Model {
    let text = serde_json::to_string(&*s.model).unwrap().replace("Sound Rating", "Volume");
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    back
}

#[test]
fn a_renamed_parameter_takes_its_tags_and_formulas_with_it() {
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "parameter", json!({"name": "Sound Rating", "categories": ["wall"], "kind": "number"}));
    run(&mut s, "parameter", json!({"name": "Sound Rating Extra", "categories": ["wall"], "kind": "number"}));
    run(&mut s, "modify", json!({"id": walls[0], "params": {"Sound Rating": 45, "Sound Rating Extra": 3}}));
    run(&mut s, "tag", json!({"ids": [walls[0]], "label": "Sound Rating"}));
    run(
        &mut s,
        "schedule",
        json!({"name": "Acoustics", "category": "wall", "filters": [{"field": "Sound Rating", "op": "greater", "value": 30}],
               "sort": [{"field": "Sound Rating"}],
               "fields": ["Sound Rating", "Sound Rating Extra",
                          {"name": "Double", "formula": "Sound Rating * 2"},
                          {"name": "Mixed", "formula": "max(Sound Rating Extra, Sound Rating) + Sound Rating Extra + 1e3"}]}),
    );
    let back = read_as_volume(&s);
    assert!(back.parameters.contains_key("Custom Volume") && !back.parameters.contains_key("Volume"));
    assert!(back.check().is_ok(), "{:?}", back.check());
    let tag = back.elements.values().find_map(|e| if let Element::Tag(t) = e { Some(t.label.clone()) } else { None });
    assert_eq!(tag, Some(Some("Custom Volume".to_string())), "a tag shows the parameter by its new name");
    let Some(Element::Schedule(sc)) = back.elements.values().find(|e| matches!(e, Element::Schedule(_))) else { panic!("no schedule") };
    assert_eq!(sc.fields[0].name, "Custom Volume");
    assert_eq!((sc.filters[0].field.as_str(), sc.sort[0].field.as_str()), ("Custom Volume", "Custom Volume"));
    // Its calculated fields read it by the new name, and not the parameter whose name starts with the old one.
    assert_eq!(sc.fields[2].formula.as_deref(), Some("Custom Volume * 2"));
    assert_eq!(sc.fields[3].formula.as_deref(), Some("max(Volume Extra, Custom Volume) + Volume Extra + 1e3"));
    assert!(crate::schedule::table(&back, sc).is_ok(), "{:?}", crate::schedule::table(&back, sc).err());
    // A multi-category schedule's Area is the one its categories share, never a parameter: it keeps its name.
    run(&mut s, "schedule", json!({"name": "Everything", "category": "multicategory", "categories": ["wall"], "fields": ["Type", "Area"]}));
    let text = serde_json::to_string(&*s.model).unwrap().replace("Sound Rating", "Area");
    let (back, _) = Model::read(&text).unwrap();
    assert!(back.parameters.contains_key("Custom Area"));
    let Some(Element::Schedule(sc)) = back.elements.values().find(|e| matches!(e, Element::Schedule(sc) if sc.name == "Everything")) else {
        panic!("no schedule")
    };
    assert_eq!(sc.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Type", "Area"]);
    assert!(back.check().is_ok(), "{:?}", back.check());
}

#[test]
fn a_parameter_renamed_where_the_new_name_is_taken_counts_up() {
    let mut s = Session::new();
    house(&mut s);
    for name in ["Sound Rating", "Custom Volume", "Custom Volume 2"] {
        run(&mut s, "parameter", json!({"name": name, "categories": ["wall"], "kind": "number"}));
    }
    let back = read_as_volume(&s);
    let names: Vec<&str> = back.parameters.keys().map(String::as_str).collect();
    assert!(names.contains(&"Custom Volume 3") && !names.contains(&"Custom Volume 2 2") && !names.contains(&"Volume"), "{names:?}");
    assert!(back.check().is_ok(), "{:?}", back.check());
}

#[test]
fn a_calculated_field_named_as_a_field_the_category_has_since_is_renamed_where_the_file_is_read() {
    let mut s = Session::new();
    let walls = house(&mut s);
    run(&mut s, "parameter", json!({"name": "Sound Rating", "categories": ["wall"], "kind": "number"}));
    run(&mut s, "modify", json!({"id": walls[0], "params": {"Sound Rating": 45}}));
    run(
        &mut s,
        "schedule",
        json!({"name": "Acoustics", "category": "wall", "filters": [{"field": "Wall Vol", "op": "greater", "value": 30}],
               "sort": [{"field": "Wall Vol"}],
               "fields": ["Sound Rating", {"name": "Wall Vol", "formula": "Sound Rating * 2"},
                          {"name": "Twice", "formula": "Wall Vol * 2 + Sound Rating"}]}),
    );
    // The file of a version from before walls had a Volume of their own: a calculated field of that name.
    let text = serde_json::to_string(&*s.model).unwrap().replace("Wall Vol", "Volume");
    let (back, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert!(back.check().is_ok(), "{:?}", back.check());
    let Some(Element::Schedule(sc)) = back.elements.values().find(|e| matches!(e, Element::Schedule(_))) else { panic!("no schedule") };
    assert_eq!(sc.fields[1].name, "Custom Volume");
    assert_eq!(sc.fields[1].heading.as_deref(), Some("Volume"), "its column is headed as it was");
    assert_eq!(sc.fields[2].formula.as_deref(), Some("Custom Volume * 2 + Sound Rating"), "the others read it by its new name");
    assert_eq!((sc.filters[0].field.as_str(), sc.sort[0].field.as_str()), ("Custom Volume", "Custom Volume"));
    assert!(crate::schedule::table(&back, sc).is_ok(), "{:?}", crate::schedule::table(&back, sc).err());
    // Where the new name is taken, it counts up.
    let mut t = Session::new();
    t.reset((*s.model).clone(), None);
    run(&mut t, "parameter", json!({"name": "Custom Volume", "categories": ["wall"], "kind": "number"}));
    run(&mut t, "parameter", json!({"name": "Custom Volume 2", "categories": ["wall"], "kind": "number"}));
    let text = serde_json::to_string(&*t.model).unwrap().replace("Wall Vol", "Volume");
    let (back, _) = Model::read(&text).unwrap();
    assert!(back.check().is_ok(), "{:?}", back.check());
    let Some(Element::Schedule(sc)) = back.elements.values().find(|e| matches!(e, Element::Schedule(_))) else { panic!("no schedule") };
    assert_eq!(sc.fields[1].name, "Custom Volume 3");
}

//! Curtain walls in schedules, quantities, element JSON and warnings: panels and mullions are categories of their own
//! (Revit's Curtain Panels and Curtain Wall Mullions), and the wall itself stays a wall.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, Id};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A storefront 6 m long and 3 m tall: four panels along (1500 each) by two up (2400 and 600), a 50 x 150 mullion on
/// every segment of its five standing lines and its three lying ones.
fn storefront(s: &mut Session) -> Id {
    run(s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}))["ids"][0].as_u64().unwrap()
}

fn of(s: &Session, category: &str) -> Vec<Id> {
    s.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect()
}

fn column(table: &Value, name: &str) -> Vec<Value> {
    let at = table["columns"].as_array().unwrap().iter().position(|c| c == name).unwrap_or_else(|| panic!("no column {name}: {table}"));
    table["rows"].as_array().unwrap().iter().map(|r| r[at].clone()).collect()
}

#[test]
fn curtain_panels_have_a_schedule_of_their_own() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let t = run(&mut s, "schedule", json!({"name": "Curtain Panel Schedule", "category": "curtain panels"}));
    assert_eq!(t["columns"], json!(["Id", "Type", "Level", "Host", "Width", "Height", "Area (m²)"]));
    assert_eq!(t["rows"].as_array().unwrap().len(), 8);
    assert!(column(&t, "Type").iter().all(|v| v == "Glazed"));
    assert!(column(&t, "Level").iter().all(|v| v == "Level 1"));
    assert!(column(&t, "Host").iter().all(|v| v.as_f64() == Some(w as f64)));
    assert!(column(&t, "Width").iter().all(|v| v == 1500.0));
    let fields = ["Height", "Area", "Thickness", "Material", "Volume", "Count", "Phase Created", "Type Mark"];
    let t = run(&mut s, "schedule", json!({"name": "Curtain Panel Schedule", "fields": fields, "sort": ["Height"]}));
    // The low row is 2400 tall, the one over it 600: 3.6 m² and 0.9 m² of 25 mm glass.
    assert_eq!(t["rows"][0], json!([600.0, 0.9, 25.0, "Glass", 0.0225, 1.0, "Existing", null]));
    assert_eq!(t["rows"][7], json!([2400.0, 3.6, 25.0, "Glass", 0.09, 1.0, "Existing", null]));
    // Its type's identity data, and a project parameter for the category.
    run(&mut s, "type", json!({"category": "curtainpanel", "name": "Glazed", "type_mark": "GL1"}));
    run(&mut s, "parameter", json!({"name": "Finish", "categories": ["curtainpanel", "mullion"]}));
    let panel = of(&s, "curtainpanel")[0];
    run(&mut s, "modify", json!({"id": panel, "params": {"Finish": "Fritted"}}));
    let t = run(&mut s, "schedule", json!({"name": "Curtain Panel Schedule", "fields": ["Id", "Type Mark", "Finish"]}));
    assert!(column(&t, "Type Mark").iter().all(|v| v == "GL1"));
    assert_eq!(column(&t, "Finish").iter().filter(|v| *v == "Fritted").count(), 1);
    // A panel is made and taken down with its wall, unless it says otherwise.
    run(&mut s, "phase", json!({"name": "Fit-out"}));
    run(&mut s, "modify", json!({"id": w, "params": {"Phase Created": "Fit-out"}}));
    let t = run(&mut s, "schedule", json!({"name": "Curtain Panel Schedule", "fields": ["Id", "Phase Created"]}));
    assert!(column(&t, "Phase Created").iter().all(|v| v == "Fit-out"), "{t}");
}

#[test]
fn mullions_have_a_schedule_of_their_own() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let t = run(&mut s, "schedule", json!({"name": "Mullion Schedule", "category": "mullion"}));
    assert_eq!(t["columns"], json!(["Id", "Type", "Level", "Host", "Length", "Thickness", "Width"]));
    assert_eq!(t["rows"].as_array().unwrap().len(), 22);
    assert!(column(&t, "Host").iter().all(|v| v.as_f64() == Some(w as f64)));
    assert!(column(&t, "Thickness").iter().all(|v| v == 150.0));
    assert!(column(&t, "Width").iter().all(|v| v == 50.0));
    let t = run(
        &mut s,
        "schedule",
        json!({"name": "Mullion Schedule", "fields": ["Type", "Material", "Count", {"name": "Length", "total": true}, {"name": "Volume", "total": true}], "grouped": true}),
    );
    // One row for the one type; its lengths and volumes add up to those of each mullion's own.
    let (length, volume): (f64, f64) = of(&s, "mullion")
        .iter()
        .map(|id| match s.model.elements.get(*id) {
            Some(Element::Mullion(x)) => (x.length(), x.length() * 50.0 * 150.0 / 1e9),
            _ => (0.0, 0.0),
        })
        .fold((0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    assert_eq!(t["rows"].as_array().unwrap().len(), 1);
    assert_eq!(t["rows"][0][0], "50 x 150mm");
    assert_eq!(t["rows"][0][1], "Metal");
    assert_eq!(t["rows"][0][2], 22.0);
    assert!((t["totals"][3].as_f64().unwrap() - length).abs() < 1e-3, "{t} {length}");
    assert!((t["totals"][4].as_f64().unwrap() - volume).abs() < 1e-3, "{t} {volume}");
}

#[test]
fn a_curtain_wall_stays_in_the_wall_schedule_with_its_depth_and_its_face() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let basic = run(&mut s, "wall", json!({"points": [[0, 8000], [6000, 8000]], "height": 3000}))["ids"][0].as_u64().unwrap();
    let t =
        run(&mut s, "schedule", json!({"category": "wall", "fields": ["Id", "Type", "Length", "Height", "Thickness", "Area", "Volume"]}));
    let row = t["rows"].as_array().unwrap().iter().find(|r| r[0].as_f64() == Some(w as f64)).unwrap();
    // As deep as its mullions; its length by its height; nothing of its own to hold (its panels and mullions hold theirs).
    assert_eq!(*row, json!([w as f64, "Storefront", 6000.0, 3000.0, 150.0, 18.0, 0.0]));
    let other = t["rows"].as_array().unwrap().iter().find(|r| r[0].as_f64() == Some(basic as f64)).unwrap();
    assert!(other[6].as_f64().unwrap() > 0.0);
    let q = crate::quantities::wall(&s.model, w).unwrap();
    assert_eq!((q.length, q.width, q.height), (6000.0, 150.0, Some(3000.0)));
    assert_eq!((q.gross_side, q.net_side, q.gross_volume, q.net_volume), (18.0e6, 18.0e6, 0.0, 0.0));
    // Every category together: each panel and mullion a row, with what it is made of.
    let t = run(
        &mut s,
        "schedule",
        json!({"name": "All", "category": "multicategory", "fields": ["Category", "Type", "Material", "Area", "Length"]}),
    );
    let rows = t["rows"].as_array().unwrap();
    assert_eq!(rows.iter().filter(|r| r[0] == "curtainpanel" && r[2] == "Glass" && r[3].as_f64().is_some_and(|a| a > 0.0)).count(), 8);
    assert_eq!(rows.iter().filter(|r| r[0] == "mullion" && r[2] == "Metal" && r[4].as_f64().is_some_and(|l| l > 0.0)).count(), 22);
}

#[test]
fn an_agent_reads_a_facade_from_its_wall_and_a_part_names_its_host() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let j = crate::commands::element_json(&s.model, w);
    assert_eq!(j["curtain_wall"], true);
    assert_eq!((j["panels"].clone(), j["mullions"].clone()), (json!(8), json!(22)));
    assert_eq!(j["vertical_grid"], json!([0.0, 1500.0, 3000.0, 4500.0, 6000.0]));
    assert_eq!(j["horizontal_grid"], json!([0.0, 2400.0, 3000.0]));
    assert_eq!(j["area_m2"], 18.0);
    let level = j["level"].clone();
    let panel = crate::commands::element_json(&s.model, of(&s, "curtainpanel")[0]);
    assert_eq!(panel["host"], j["guid"]);
    assert_eq!(panel["level"], level);
    assert_eq!((panel["thickness"].clone(), panel["material"].clone()), (json!(25.0), json!("Glass")));
    let mullion = crate::commands::element_json(&s.model, of(&s, "mullion")[0]);
    assert_eq!(mullion["host"], j["guid"]);
    assert_eq!(mullion["level"], level);
    assert_eq!((mullion["thickness"].clone(), mullion["width"].clone()), (json!(150.0), json!(50.0)));
    // A basic wall says nothing of a grid.
    let basic = run(&mut s, "wall", json!({"points": [[0, 8000], [6000, 8000]]}))["ids"][0].as_u64().unwrap();
    let j = crate::commands::element_json(&s.model, basic);
    assert!(j.get("curtain").is_none() && j.get("vertical_grid").is_none());
}

#[test]
fn a_curtain_wall_warns_about_a_wall_it_overlaps_and_never_about_its_own() {
    let mut s = Session::new();
    storefront(&mut s);
    let all = run(&mut s, "warnings", json!({}));
    assert_eq!(all["count"], 0, "its panels and mullions are no walls over it, and no twins of one another: {all}");
    // A second storefront on the first: the two walls are twins; their panels and mullions say nothing more.
    storefront(&mut s);
    let all = run(&mut s, "warnings", json!({}));
    let rules: Vec<&str> = all["warnings"].as_array().unwrap().iter().filter_map(|w| w["rule"].as_str()).collect();
    assert!(!rules.is_empty() && rules.iter().all(|r| *r == "identical_instances" || *r == "walls_overlap"), "{all}");
    assert!(all["warnings"].as_array().unwrap().iter().all(|w| w["elements"].as_array().is_some_and(|e| e.len() == 2)), "{all}");
    // A basic wall along a curtain wall of a type that does not embed overlaps it; along a Storefront, which embeds itself
    // as Revit's does, it holds the storefront in the opening it cuts, and nothing is amiss.
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Exterior Glazing", "height": 3000}));
    run(&mut s, "wall", json!({"points": [[1000, 0], [5000, 0]], "height": 3000}));
    let found = run(&mut s, "warnings", json!({"rule": "walls_overlap"}));
    assert_eq!(found["count"], 1, "{found}");
    let mut s = Session::new();
    storefront(&mut s);
    run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "height": 3000}));
    let found = run(&mut s, "warnings", json!({"rule": "walls_overlap"}));
    assert_eq!(found["count"], 0, "{found}");
}

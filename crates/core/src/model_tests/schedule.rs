//! `schedule`'s tests that drive the Session (moved out of `schedule.rs`).

use crate::model::Schedule;
use crate::units::Units;
use serde_json::Value;

use crate::engine::Session;
use crate::schedule::*;
use serde_json::json;

fn house() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           wall {"points": [[5000,0],[5000,6000]], "type": "Interior - 100mm"}
           room {"point": [6500, 3000], "name": "Kitchen"}
           room {"point": [2000, 3000], "name": "Living", "number": "10"}
           door {"at": [6000, 0], "mark": "D2"}
           door {"at": [2000, 0]}"#,
    )
    .unwrap();
    s
}

fn of(cat: &str) -> Schedule {
    Schedule { name: String::new(), of: cat.into(), ..Schedule::default() }
}

fn headings(t: &Table) -> Vec<String> {
    t.columns.iter().map(|c| heading(Units::Millimetres, c)).collect()
}

#[test]
fn room_schedules_list_number_name_level_area_and_perimeter_in_number_order() {
    let t = table(&house().model, &of("rooms")).unwrap();
    assert_eq!(headings(&t), ["Number", "Name", "Level", "Area (m²)", "Perimeter"]);
    assert_eq!(
        t.rows,
        [
            vec![json!("1"), json!("Kitchen"), json!("Level 1"), json!(18.0), json!(18000.0)],
            vec![json!("10"), json!("Living"), json!("Level 1"), json!(30.0), json!(22000.0)],
        ]
    );
    assert!(t.totals.is_none());
}

#[test]
fn door_schedules_read_sizes_from_the_type() {
    let t = table(&house().model, &of("Door")).unwrap();
    assert_eq!(headings(&t), ["Mark", "Type", "Width", "Height", "Level", "Wall"]);
    assert_eq!(t.rows.len(), 2);
    assert_eq!(t.rows[0][0], "1", "numbers before other marks");
    assert_eq!((&t.rows[1][0], &t.rows[1][2], &t.rows[1][3]), (&json!("D2"), &json!(900.0), &json!(2100.0)));
}

#[test]
fn unknown_categories_name_the_schedules_there_are() {
    let e = table(&house().model, &of("clouds")).err().unwrap();
    assert!(e.contains("room") && e.contains("stair"), "{e}");
}

#[test]
fn schedules_show_lengths_and_areas_in_the_project_units() {
    let (ft, m) = (Units::FeetInches, Units::Metres);
    let area = Column { heading: "Area".into(), kind: Kind::Area };
    assert_eq!((heading(ft, &area), shown(ft, Kind::Area, &json!(18.0))), ("Area (ft²)".into(), "193.75".into()));
    assert_eq!((heading(m, &area), shown(m, Kind::Area, &json!(18.0))), ("Area (m²)".into(), "18.00".into()));
    assert_eq!([shown(m, Kind::Length, &json!(900.0)), shown(ft, Kind::Length, &json!(900.0))], ["0.90", "2'-11 7/16\""]);
    assert_eq!([shown(ft, Kind::Text, &json!("D2")), shown(ft, Kind::Number, &json!(4))], ["D2", "4"]);
    assert_eq!([shown(m, Kind::YesNo, &json!(true)), shown(m, Kind::Number, &json!(2.0 / 3.0))], ["Yes", "0.67"]);
}

#[test]
fn filters_compare_values_as_the_schedule_shows_them() {
    let (ft, feet) = (Units::FeetInches, Units::Feet);
    let filter = |op, value| Filter { field: String::new(), op, value };
    // 2'-11 7/16" typed is 900.1125 mm: a 900 mm door shows the same.
    assert!(keeps(&filter(Op::Equals, json!(900.1125)), &json!(900.0), Some(Kind::Length), ft));
    assert!(!keeps(&filter(Op::Equals, json!(910.0)), &json!(900.0), Some(Kind::Length), ft));
    // 18 m² shows 193.75 ft²; 193.75 typed is a hair under it.
    assert!(keeps(&filter(Op::LessOrEqual, json!(193.75 * 0.09290304)), &json!(18.0), Some(Kind::Area), feet));
    assert!(!keeps(&filter(Op::Less, json!(193.75 * 0.09290304)), &json!(18.0), Some(Kind::Area), feet));
}

#[test]
fn csv_leaves_no_text_a_spreadsheet_would_take_for_a_formula() {
    let t = Table {
        columns: vec![Column { heading: "Name".into(), kind: Kind::Text }, Column { heading: "Offset".into(), kind: Kind::Length }],
        rows: vec![vec![json!("=1+1"), json!(-5.0)], vec![json!("@SUM(A1)"), json!(5.0)]],
        ids: Vec::new(),
        totals: None,
    };
    assert_eq!(to_csv(&t), "Name,Offset\n'=1+1,-5.0\n'@SUM(A1),5.0\n");
}

#[test]
fn a_schedule_goes_to_a_workbook_with_its_numbers_as_numbers() {
    let t = table(&house().model, &of("rooms")).unwrap();
    let parts = crate::xlsx::unzip(&to_xlsx(&t, "Room Schedule"));
    assert!(parts["xl/workbook.xml"].contains(r#"name="Room Schedule""#));
    let sheet = &parts["xl/worksheets/sheet1.xml"];
    assert!(sheet.contains(r#"<t xml:space="preserve">Area (m²)</t>"#), "{sheet}");
    assert!(sheet.contains(r#"<t xml:space="preserve">Kitchen</t></is></c><c r="C2" t="inlineStr"><is><t xml:space="preserve">Level 1</t></is></c><c r="D2"><v>18</v></c><c r="E2"><v>18000</v></c>"#), "{sheet}");
}

fn multi(cats: &[&str], fields: &[&str]) -> Schedule {
    Schedule {
        of: MULTI.into(),
        categories: cats.iter().map(|c| c.to_string()).collect(),
        fields: fields.iter().map(|f| Field { name: f.to_string(), ..Field::default() }).collect(),
        ..Schedule::default()
    }
}

#[test]
fn a_multi_category_schedule_lists_its_categories_elements_with_the_fields_they_share() {
    let m = house().model;
    let sc = multi(&["walls", "door"], &["Category", "Type", "Width", "Height", "Length", "Area", "Count"]);
    check(&m, &sc).unwrap();
    let t = table(&m, &sc).unwrap();
    assert_eq!(headings(&t), ["Category", "Type", "Width", "Height", "Length", "Area (m²)", "Count"]);
    assert_eq!(t.rows.len(), 7, "five walls and two doors, no rooms");
    let doors: Vec<&Vec<Value>> = t.rows.iter().filter(|r| r[0] == "door").collect();
    assert_eq!(doors.len(), 2, "sorted by its first column, the doors first");
    assert_eq!((&doors[0][2], &doors[0][3], &doors[0][4]), (&json!(900.0), &json!(2100.0), &Value::Null), "a door has no length");
    assert_eq!(doors[0][5], json!(1.89), "a door's area is its width by its height");
    for wall in t.rows.iter().filter(|r| r[0] == "wall") {
        assert_eq!(wall[2], Value::Null, "a wall has a thickness, not a width");
        let (l, h) = (wall[4].as_f64().unwrap(), wall[3].as_f64().unwrap());
        assert!(l > 0.0 && h > 0.0, "{wall:?}");
        // One side of it, less the doors and windows in it.
        let gross = fine(l * h / 1e6);
        assert!(wall[5].as_f64().unwrap() <= gross, "a wall's area is its length by its height, less its openings: {wall:?}");
    }
}

#[test]
fn a_multi_category_schedule_filters_groups_and_totals_as_any_other() {
    let m = house().model;
    let mut sc = multi(&[], &["Category", "Type", "Count"]);
    sc.sort = vec![Sort { field: "Category".into(), descending: false }];
    sc.grouped = true;
    let t = table(&m, &sc).unwrap();
    assert_eq!(
        t.rows,
        [vec![json!("door"), json!("Single 900 x 2100"), json!(2.0)], vec![json!("wall"), Value::Null, json!(5.0)]],
        "rooms aren't built"
    );
    assert_eq!(t.ids.iter().map(Vec::len).collect::<Vec<_>>(), [2, 5]);
    sc.grouped = false;
    sc.filters = vec![Filter { field: "Category".into(), op: Op::Equals, value: json!("Door") }];
    assert_eq!(table(&m, &sc).unwrap().rows.len(), 2);
}

#[test]
fn a_multi_category_schedule_reads_its_materials_from_its_types() {
    let mut s = house();
    s.script(
        r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "material": "Brick", "thickness": 100}, {"function": "structure", "material": "Concrete block", "thickness": 140}]}
           type {"category": "wall", "name": "Lined", "layers": [{"function": "finish", "material": "Plaster", "thickness": 10}, {"function": "finish", "material": "Brick", "thickness": 100}]}
           wall {"points": [[0,-2000],[4000,-2000]], "type": "Cavity"}
           wall {"points": [[0,-4000],[4000,-4000]], "type": "Lined"}"#,
    )
    .unwrap();
    let mut sc = multi(&["wall"], &["Type", "Material"]);
    sc.grouped = true;
    sc.sort = vec![Sort { field: "Type".into(), descending: false }];
    let t = table(&s.model, &sc).unwrap();
    let material = |ty: &str| t.rows.iter().find(|r| r[0] == ty).map(|r| r[1].clone());
    assert_eq!(material("Cavity"), Some(json!("Concrete block")), "its structure layer's");
    assert_eq!(material("Lined"), Some(json!("Brick")), "with none, its thickest layer's");
    assert_eq!(material("Interior - 100mm"), Some(Value::Null), "a type of no layers says none");
}

#[test]
fn only_a_multi_category_schedule_lists_categories_and_only_their_shared_fields() {
    let m = house().model;
    let e = check(&m, &Schedule { categories: vec!["door".into()], ..of("wall") }).unwrap_err();
    assert!(e.contains("multi-category"), "{e}");
    let e = check(&m, &multi(&["door", "clouds"], &[])).unwrap_err();
    assert!(e.contains("cloud") && e.contains("stair"), "{e}");
    let e = check(&m, &multi(&["door"], &["Sill"])).unwrap_err();
    assert!(e.contains("Sill") && e.contains("Category"), "{e}");
    assert_eq!(defaults(MULTI).len(), 11);
}

#[test]
fn csv_quotes_the_cells_that_need_it_and_ends_with_the_totals() {
    let column = |h: &str, kind| Column { heading: h.into(), kind };
    let t = Table {
        columns: vec![column("Name", Kind::Text), column("Area", Kind::Area)],
        rows: vec![vec![json!("A, B"), json!(1.5)], vec![json!("say \"hi\""), Value::Null]],
        ids: Vec::new(),
        totals: Some(vec![Value::Null, json!(1.5)]),
    };
    assert_eq!(to_csv(&t), "Name,Area (m²)\n\"A, B\",1.5\n\"say \"\"hi\"\"\",\n,1.5\n");
}

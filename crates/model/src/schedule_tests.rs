//! `schedule`'s tests that need only the model.

use crate::schedule::*;
use crate::units::Units;
use serde_json::Value;
use serde_json::json;

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

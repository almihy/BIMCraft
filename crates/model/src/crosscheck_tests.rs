//! `crosscheck`'s tests that need only the model.
use crate::crosscheck::*;
use crate::model::Id;

#[test]
fn a_comparison_names_each_difference_and_what_only_one_side_has() {
    let r = |kind: &str, id: Id, q: &str, v: f64| Row { kind: kind.into(), id, quantity: q.into(), value: v };
    let ours = [r("room", 1, "area_m2", 20.0), r("room", 1, "perimeter_m", 18.0), r("wall", 2, "length_m", 6.0)];
    let theirs = [
        r("room", 1, "area_m2", 20.00004),
        r("room", 1, "perimeter_m", 18.2),
        r("wall", 3, "length_m", 6.0),
        r("room", 1, "volume_m3", 50.0),
    ];
    let report = compare(&ours, &theirs);
    assert_eq!(report.compared, 2);
    assert_eq!(report.differ, vec![(ours[1].clone(), 18.2)]);
    assert_eq!(report.only_ours, vec![ours[2].clone()]);
    assert_eq!(report.only_theirs.len(), 2);
    assert_eq!(report.to_json(10)["differ"][0]["theirs"], 18.2);
}

#[test]
fn a_table_of_results_is_read_strictly() {
    let rows = read("\u{feff}kind, id, quantity, value\nroom,100001,area_m2,20.5\n\nwall,100002,length_m,6\n").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].value, 20.5);
    let head = "kind,id,quantity,value\n";
    for bad in [
        String::new(),
        "kind,id\nroom,1".into(),
        format!("{head}room,1,area_m2,NaN"),
        format!("{head}room,1,area_m2"),
        format!("{head},1,area_m2,1"),
        format!("{head}room,a,area_m2,1"),
        format!("{head}room,-1,area_m2,1"),
    ] {
        assert!(read(&bad).is_err(), "{bad:?}");
    }
}

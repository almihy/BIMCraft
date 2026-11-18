//! `selection`'s tests that drive the Session (moved out of `selection.rs`).

use crate::model::Id;
use crate::schedule::Kind;

use serde_json::json;

use crate::engine::Session;
use crate::selection::*;

/// Four walls, two of them of another type, and a door in the first.
fn house() -> (Session, Vec<Id>, Id) {
    let mut s = Session::new();
    let r = s.execute("wall", &json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true})).unwrap();
    let walls: Vec<Id> = r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
    s.execute("type", &json!({"category": "wall", "name": "Thick", "thickness": 300})).unwrap();
    s.execute("modify", &json!({"ids": [walls[1], walls[3]], "type": "Thick"})).unwrap();
    let door = s.execute("door", &json!({"wall": walls[0], "offset": 2000})).unwrap()["id"].as_u64().unwrap();
    (s, walls, door)
}

#[test]
fn a_selection_counts_by_category_and_keeps_what_is_checked() {
    let (s, walls, door) = house();
    let mut ids = walls.clone();
    ids.insert(1, door);
    let g = groups(&s.model, &ids, CATEGORY);
    assert_eq!(g.iter().map(|g| (g.value.as_str(), g.ids.len())).collect::<Vec<_>>(), [("door", 1), ("wall", 4)]);
    assert_eq!(kept(&s.model, &ids, CATEGORY, &["wall".into()]), walls);
    assert_eq!(kept(&s.model, &ids, CATEGORY, &[]), Vec::<Id>::new());
}

#[test]
fn walls_share_their_parameters_and_are_kept_by_a_value() {
    let (s, walls, door) = house();
    let fields: Vec<String> = common(&s.model, &walls).into_iter().map(|(n, _)| n).collect();
    assert_eq!(fields[0], CATEGORY);
    assert!(fields.iter().any(|f| f == "Type") && fields.iter().any(|f| f == "Length"), "{fields:?}");
    assert!(!fields.iter().any(|f| f == "Count"), "{fields:?}");
    let g = groups(&s.model, &walls, "Type");
    assert_eq!(g.iter().map(|g| (g.value.as_str(), g.ids.len())).collect::<Vec<_>>(), [("Generic - 200mm", 2), ("Thick", 2)]);
    assert_eq!(kept(&s.model, &walls, "Type", &["Thick".into()]), [walls[1], walls[3]]);
    // A door and walls share what both categories have, and only that.
    let mixed: Vec<String> = common(&s.model, &[walls[0], door]).into_iter().map(|(n, _)| n).collect();
    assert!(mixed.iter().any(|f| f == "Type") && !mixed.iter().any(|f| f == "Length"), "{mixed:?}");
    assert_eq!(common(&s.model, &[]), [(CATEGORY.to_string(), Kind::Text)]);
}

#[test]
fn lengths_go_by_value_and_what_has_none_comes_last() {
    let (s, walls, door) = house();
    let lengths: Vec<String> = groups(&s.model, &walls, "Length").into_iter().map(|g| g.value).collect();
    assert_eq!(lengths.len(), 2, "two lengths, each twice: {lengths:?}");
    let marks = groups(&s.model, &[door, walls[0]], "Mark");
    assert_eq!(marks.last().map(|g| g.value.as_str()), Some(""), "{marks:?}");
}

#[test]
fn annotation_has_its_parameters_too_a_line_its_line_style() {
    let mut s = Session::new();
    let line = |s: &mut Session, style: &str| {
        s.execute("detail_line", &json!({"start": [0, 0], "end": [1000, 0], "style": style})).unwrap()["id"].as_u64().unwrap()
    };
    let lines = [line(&mut s, "Thin Lines"), line(&mut s, "Wide Lines"), line(&mut s, "Thin Lines")];
    let fields: Vec<String> = common(&s.model, &lines).into_iter().map(|(n, _)| n).collect();
    assert!(fields.iter().any(|f| f == "Line Style") && fields.iter().any(|f| f == "Length"), "{fields:?}");
    assert_eq!(kept(&s.model, &lines, "Line Style", &["Wide Lines".into()]), [lines[1]]);
    let sketch = json!([[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]]);
    let region = s.execute("filled_region", &json!({"sketch": sketch})).unwrap()["id"].as_u64().unwrap();
    let g = groups(&s.model, &[region], "Type");
    assert_eq!(g.iter().map(|g| g.value.as_str()).collect::<Vec<_>>(), ["Solid Black"]);
    let text = s.execute("text", &json!({"at": [0, 0], "text": "hi"})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(groups(&s.model, &[text], "Text")[0].value, "hi");
}

#[test]
fn the_command_lists_the_groups_and_what_is_kept_and_changes_nothing() {
    let (mut s, walls, door) = house();
    let (steps, before) = (s.history().len(), s.model.clone());
    let ids = [walls[0], walls[1], door];
    let r = s.execute("filter_selection", &json!({"ids": ids})).unwrap();
    assert_eq!(r["by"], CATEGORY);
    assert_eq!(
        r["groups"],
        json!([{"value": "door", "count": 1, "ids": [door]}, {"value": "wall", "count": 2, "ids": [walls[0], walls[1]]}])
    );
    assert_eq!(r["fields"][0], CATEGORY);
    assert!(r.get("ids").is_none(), "nothing kept unless asked");
    let r = s.execute("filter_selection", &json!({"ids": ids, "by": "Type", "keep": ["Thick"]})).unwrap();
    assert_eq!(r["ids"], json!([walls[1]]));
    for bad in [
        json!({}),
        json!({"ids": ids, "by": "Nonsense"}),
        json!({"ids": ids, "keep": "wall"}),
        json!({"ids": ids, "keep": [5]}),
        json!({"ids": [5]}),
    ] {
        assert!(s.execute("filter_selection", &bad).is_err(), "{bad}");
    }
    assert_eq!((s.history().len(), &s.model), (steps, &before), "reading changes nothing and is no undo step");
}

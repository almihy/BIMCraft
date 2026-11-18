//! The sample house, built and checked.

use crate::engine::Session;
use serde_json::json;

#[test]
fn highrise_is_an_original_valid_twenty_storey_project() {
    let mut s = Session::new();
    s.execute("highrise", &json!({})).unwrap();
    assert_eq!(s.model.levels().len(), 21);
    assert!((9_000..11_000).contains(&s.model.elements.len()));
    s.model.check().unwrap();
    for category in ["wall", "window", "floor", "ceiling", "structuralcolumn", "beam", "component"] {
        assert!(s.model.elements.values().any(|e| e.category() == category), "{category}");
    }
    let text = s.model.to_json().unwrap();
    let reopened = crate::model::Model::from_json(&text).unwrap();
    reopened.check().unwrap();
    assert_eq!(reopened.elements.len(), s.model.elements.len());
    assert!(s.path.is_none());
    assert!(s.history().is_empty());
}

#[test]
fn highrise_rejects_unbounded_sizes_without_replacing_the_project() {
    let mut s = Session::new();
    let before = s.model.clone();
    for size in [json!(0), json!(81), json!(-1), json!(1.5), json!("20")] {
        assert!(s.execute("highrise", &json!({"storeys": size})).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, &s.model));
    }
}

#[test]
fn tower_rejects_unbounded_sizes_without_replacing_the_project() {
    let mut s = Session::new();
    let before = s.model.clone();
    for size in [json!(0), json!(101), json!(-1), json!(1.5), json!("20")] {
        assert!(s.execute("tower", &json!({"storeys": size})).is_err(), "{size}");
        assert!(std::sync::Arc::ptr_eq(&before, &s.model));
    }
    let e = s.execute("tower", &json!({"floors": 2})).unwrap_err();
    assert!(e.contains("floors"), "{e}");
    let e = s.execute("tower", &json!({"storeys": 1, "plate": "huge"})).unwrap_err();
    assert!(e.contains("`plate` must be `reference` or `small`"), "{e}");
    assert!(std::sync::Arc::ptr_eq(&before, &s.model));
    // The small plate: the tower the cross-check with Revit is of.
    let small = s.execute("tower", &json!({"storeys": 1, "plate": "small"})).unwrap();
    let built = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap();
    assert_eq!(small["elements"].as_u64(), Some(built.elements.len() as u64));
    assert_eq!(s.model.elements.keys().collect::<Vec<_>>(), built.elements.keys().collect::<Vec<_>>(), "the same ids");
    // Annotated: its plans detailed, 2D in thousands.
    let annotated = s.execute("tower", &json!({"storeys": 1, "plate": "small", "annotation": true})).unwrap();
    let built = crate::sample::tower::annotated(crate::sample::tower::Plate::SMALL, 1).unwrap();
    assert_eq!(annotated["elements"].as_u64(), Some(built.elements.len() as u64));
    assert!(built.elements.len() > small["elements"].as_u64().unwrap_or(0) as usize + 300);
    let e = s.execute("tower", &json!({"storeys": 1, "annotation": "yes"})).unwrap_err();
    assert!(e.contains("annotation"), "{e}");
    assert!(crate::bridge::REFUSED.contains(&"tower"), "a window's project isn't replaced from outside");
}

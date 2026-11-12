//! `model`'s tests that drive the Session (moved out of `model.rs`).

use crate::ids::Id;

use crate::model::*;

#[test]
fn components_sharing_a_shape_are_each_checked_and_the_first_that_fails_is_named() {
    use serde_json::json;
    let mut s = crate::engine::Session::new();
    let width = json!({"name": "Width", "instance": true, "value": 500});
    let form = json!({"profile": [[0, 0], ["Width", 0], ["Width", 100], [0, 100]], "to": 100});
    s.execute("family", &json!({"name": "Slab", "parameters": [width], "forms": [form], "types": {"A": {}}})).unwrap();
    for _ in 0..3 {
        s.execute("component", &json!({"family": "Slab", "at": [0, 0]})).unwrap();
    }
    s.model.check().unwrap();
    // Two with a width no profile has, the same one: each its own error, the first named.
    let ids: Vec<Id> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Component(_))).map(|(id, _)| *id).collect();
    let m = s.model_mut();
    for id in &ids[1..] {
        if let Some(Element::Component(c)) = m.elements.get_mut(*id) {
            c.params.insert("Width".into(), json!(-1e300));
        }
    }
    let first = m.check().unwrap_err();
    assert!(first.starts_with(&format!("component {}:", ids[1])), "{first}");
    m.elements.remove(ids[1]);
    let second = m.check().unwrap_err();
    assert!(second.starts_with(&format!("component {}:", ids[2])), "{second}");
    assert_eq!(first.split_once(':').map(|p| p.1), second.split_once(':').map(|p| p.1));
}

//! `background_grid`'s tests that drive the Session (moved out of `background_grid.rs`).

#[test]
fn background_grids_are_independent_saved_view_settings_with_undo() {
    use crate::{engine::Session, model::Model};
    use serde_json::json;
    let mut s = Session::new();
    let grid = json!({"main": {"enabled": true}, "spacing": [1200, 2400], "rotation": 25});
    s.execute("graphics", &json!({"view": "Level 1", "background_grid": grid})).unwrap();
    let read = |s: &mut Session, view: &str| s.execute("graphics", &json!({"view": view})).unwrap()["background_grid"].clone();
    let first = read(&mut s, "Level 1");
    assert_eq!(first["spacing"], json!([1200.0, 2400.0]));
    assert_eq!(read(&mut s, "Level 2")["main"]["enabled"], false);
    s.execute("duplicate_view", &json!({"view":"Level 1", "name":"Independent"})).unwrap();
    assert_eq!(read(&mut s, "Independent"), first);
    s.execute("graphics", &json!({"view":"Independent", "background_grid":{"rotation":70}})).unwrap();
    assert_eq!(read(&mut s, "Independent")["rotation"], 70.0);
    assert_eq!(read(&mut s, "Level 1"), first);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(read(&mut s, "Independent"), first);
    s.execute("section", &json!({"start":[0,0], "end":[8000,0]})).unwrap();
    s.execute("graphics", &json!({"view":"Section 1", "background_grid":{"sub":{"enabled":true}}})).unwrap();
    assert_eq!(read(&mut s, "Section 1")["sub"]["enabled"], true);
    assert_eq!(read(&mut s, "Level 1"), first);
    let saved = s.model.to_json().unwrap();
    let (back, left_out) = Model::read(&saved).unwrap();
    assert!(left_out.is_empty());
    assert_eq!(back, *s.model);
}

#[test]
fn background_grid_templates_control_followers_and_reject_bypasses_and_bad_values() {
    use crate::engine::Session;
    use serde_json::json;
    let mut s = Session::new();
    s.execute("graphics", &json!({"view":"Level 1", "background_grid":{"main":{"enabled":true}, "rotation":20}})).unwrap();
    let template = s.execute("template", &json!({"name":"Grid template", "from":"Level 1", "controls":["background_grid"]})).unwrap();
    assert_eq!(template["background_grid"]["rotation"], 20.0);
    s.execute("graphics", &json!({"view":"Level 2", "template":"Grid template"})).unwrap();
    for (command, params) in [
        ("graphics", json!({"view":"Level 2", "background_grid":{"rotation":30}})),
        ("modify", json!({"id":100001, "graphics":{"background_grid":{"rotation":30}}})),
    ] {
        let err = s.execute(command, &params).unwrap_err();
        assert!(err.contains("Grid template") && err.contains("background"), "{err}");
    }
    s.execute("template", &json!({"name":"Grid template", "background_grid":{"rotation":45}})).unwrap();
    assert_eq!(s.execute("graphics", &json!({"view":"Level 2"})).unwrap()["background_grid"]["rotation"], 45.0);
    s.execute("graphics", &json!({"view":"Level 2", "template":null})).unwrap();
    assert_eq!(s.execute("graphics", &json!({"view":"Level 2"})).unwrap()["background_grid"]["main"]["enabled"], false);
    for bad in [
        json!({"spacing":[0,1000]}),
        json!({"divisions":[0,5]}),
        json!({"rotation":361}),
        json!({"main":{"pen":17}}),
        json!({"typo":true}),
        json!({"snapping":true}),
    ] {
        assert!(s.execute("graphics", &json!({"view":"Level 1", "background_grid":bad})).is_err(), "{bad}");
        assert!(s.execute("template", &json!({"name":"Grid template", "background_grid":bad})).is_err(), "{bad}");
    }
}

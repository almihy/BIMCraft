//! `background_grid`'s tests that drive the Session (moved out of `background_grid.rs`).

use crate::background_grid::*;
use crate::geom::P2;
use crate::prim::View;

#[test]
fn background_grid_defaults_use_the_requested_neutral_greys() {
    let grid = BackgroundGrid::default();
    assert_eq!(grid.main.color, [230, 230, 230]);
    assert_eq!(grid.sub.color, [245, 245, 245]);
}

#[test]
fn grid_rotation_spacing_and_negative_coordinates_share_the_origin() {
    let mut g = BackgroundGrid::default();
    g.main.enabled = true;
    g.spacing = [1000.0, 2000.0];
    assert_eq!(g.nearest(P2(-610.0, 1300.0)), Some(P2(-1000.0, 2000.0)));
    g.rotation = 90.0;
    let p = g.nearest(P2(-1700.0, 850.0)).unwrap();
    assert!(p.dist(P2(-2000.0, 1000.0)) < 1e-8);
    assert_eq!(g.nearest(P2(0.0, 0.0)), Some(P2(0.0, 0.0)));
    g.main.enabled = false;
    assert_eq!(g.nearest(P2(10.0, 10.0)), None);
}

#[test]
fn subgrid_is_independent_and_drawing_is_bounded_at_extreme_zoom() {
    let mut g = BackgroundGrid::default();
    g.sub.enabled = true;
    g.spacing = [1000.0, 2000.0];
    g.divisions = [4, 5];
    assert_eq!(g.nearest(P2(370.0, -610.0)), Some(P2(250.0, -800.0)));
    let v = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600 };
    let lines = g.lines(v);
    assert!(!lines.is_empty());
    assert!(lines.iter().any(|l| l.points[0].0 == 0.0 && l.points[1].0 == 0.0));
    g.main.enabled = true;
    g.rotation = 33.0;
    assert!(g.lines(View { scale: 1e-6, ..v }).len() <= 4096);
    g.spacing[0] = 0.0;
    assert!(!g.valid());
    assert!(g.lines(v).is_empty());
    assert_eq!(g.nearest(P2(0.0, 0.0)), None);
}

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

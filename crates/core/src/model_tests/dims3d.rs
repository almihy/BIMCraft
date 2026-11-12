//! `dims3d`'s tests that drive the Session (moved out of `dims3d.rs`).

use crate::geom::v3::{add, mul, sub};
use crate::graphics::Look;
use crate::model::{Dimension3D, Element, Id};
use crate::prim::Rgb;

use serde_json::{Value, json};

use crate::dims3d::*;
use crate::engine::Session;
use crate::view3d::Scene;

fn boxed() -> Session {
    let mut s = Session::new();
    s.script(r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}"#).unwrap();
    s
}

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// The south wall's outer top corners, its line 1500 mm south of them at the ground.
fn across_the_south_wall(s: &mut Session) -> Id {
    let d = run(s, "dimension_3d", json!({"refs": [{"at": [-100, -100, 3000]}, {"at": [8100, -100, 3000]}], "at": [4000, -1600, 0]}));
    assert_eq!(d["length"], 8200.0);
    d["id"].as_u64().unwrap()
}

fn the(s: &Session, id: Id) -> &Dimension3D {
    match s.model.elements.get(id) {
        Some(Element::Dimension3D(d)) => d,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_dimension_in_a_3d_view_measures_between_points_or_square_to_an_edge_and_reports_its_length() {
    let mut s = boxed();
    let id = across_the_south_wall(&mut s);
    let e = crate::element_json(&s.model, id);
    assert_eq!((e["category"].as_str(), e["length"].as_f64()), (Some("dimension3d"), Some(8200.0)));
    assert_eq!(s.model.elements.id(the(&s, id).view), s.model.default_3d(), "kept by the default 3D view, made for it");
    // A point to an edge: square to the edge's line, the north wall's top.
    let d = run(
        &mut s,
        "dimension_3d",
        json!({"refs": [{"at": [0, 0, 0]}, {"at": [3000, 6000, 3000], "edge": [[0, 6000, 3000], [8000, 6000, 3000]]}], "at": [0, 3000, 4000]}),
    );
    assert_eq!(d["length"], 6708.2, "the hypotenuse of 6000 north and 3000 up");
    // Two parallel edges: how far apart they are.
    let d = run(
        &mut s,
        "dimension_3d",
        json!({"refs": [{"at": [1000, 0, 3000], "edge": [[0, 0, 3000], [8000, 0, 3000]]}, {"at": [5000, 6000, 3000], "edge": [[0, 6000, 3000], [8000, 6000, 3000]]}],
               "at": [-500, 3000, 3000]}),
    );
    assert_eq!(d["length"], 6000.0);
    // Refused: nothing between them, a plan as its view, a point out of the project.
    let refused = [
        json!({"refs": [{"at": [0, 0, 0]}, {"at": [0, 0, 0]}], "at": [0, 0, 100]}),
        json!({"refs": [{"at": [0, 0, 0]}, {"at": [1000, 0, 0]}], "at": [0, 0, 100], "view": "Level 1"}),
        json!({"refs": [{"at": [0, 0, 0]}, {"at": [1e12, 0, 0]}], "at": [0, 0, 100]}),
        json!({"refs": [{"at": [0, 0, 0]}], "at": [0, 0, 100]}),
    ];
    let before = s.model.elements.len();
    for p in refused {
        assert!(s.execute("dimension_3d", &p).is_err(), "{p}");
    }
    assert_eq!(s.model.elements.len(), before);
    // A move takes it along; its view deleted takes it too.
    run(&mut s, "move", json!({"ids": [id], "by": [0, 1000]}));
    assert_eq!(the(&s, id).refs[0].at, [-100.0, 900.0, 3000.0]);
    let view = s.model.default_3d().unwrap();
    run(&mut s, "delete", json!({"ids": [view]}));
    assert!(s.model.elements.values().all(|e| !matches!(e, Element::Dimension3D(_))));
}

#[test]
fn its_line_and_number_are_painted_into_the_processors_picture_where_the_camera_sees_them() {
    let mut s = boxed();
    let id = across_the_south_wall(&mut s);
    let m = &s.model;
    let view = m.default_3d().unwrap();
    let (scene, look) = (Scene::new(m), Look::of_3d(m));
    let cam = scene.fit_in(crate::camera::AZIMUTH, crate::camera::ELEVATION, 800, 600, &look);
    let (a, b) = line(the(&s, id));
    // Its line, a third of the way along: off the model, where the picture is paper.
    let along = add(a, mul(sub(b, a), 1.0 / 3.0));
    for cam in [cam, cam.orbit(40.0, 10.0)] {
        let mut image = scene.render_in(&cam, &[], &look);
        let (x, y, _) = cam.project(along);
        assert_eq!(image.color(x, y), Rgb(255, 255, 255), "nothing there before");
        let marks = marks(m, view, &cam, &look, SCREEN);
        assert!(marks.len() > 6, "witness lines, its line, ticks and letters: {}", marks.len());
        paint(&mut image, &marks);
        assert_eq!(image.color(x, y), Rgb(0, 0, 0), "its line, in its type's colour, where the camera sees it");
        assert_eq!(pick(&marks, (x + 1.0, y - 1.0), 6.0), Some(id), "picked where it's drawn");
    }
    // The render of the view has it.
    let with = run(&mut s, "render", json!({"view": "{3D}"}))["pngBase64"].as_str().map(String::from);
    run(&mut s, "delete", json!({"ids": [id]}));
    let without = run(&mut s, "render", json!({"view": "{3D}"}))["pngBase64"].as_str().map(String::from);
    assert_ne!(with, without, "the 3D view's PNG draws it");
    // Hidden with the Dimensions category in the view's Visibility/Graphics.
    let id = across_the_south_wall(&mut s);
    run(&mut s, "graphics", json!({"view": "{3D}", "hide": ["dimension"]}));
    let m = &s.model;
    assert!(marks(m, view, &cam, &Look::of_3d(m), SCREEN).iter().all(|mk| mk.owner != id));
}

#[test]
fn a_3d_views_dimension_prints_on_the_sheet_that_holds_the_view() {
    let mut s = boxed();
    let sheet = run(&mut s, "sheet", json!({}))["id"].as_u64().unwrap();
    let id = across_the_south_wall(&mut s);
    let view = s.model.default_3d().unwrap();
    run(&mut s, "place", json!({"sheet": sheet, "view": view}));
    let paper = crate::model::paper(&s.model.sheet(sheet).unwrap().size).unwrap();
    // The sheet's lines, and the strokes of its PDF page.
    let strokes = |s: &Session| {
        let d = crate::sheet::draw(&s.model, sheet).unwrap();
        let lines = d.prims.iter().filter(|p| matches!(p, crate::prim::Prim::Line { .. })).count();
        (lines, crate::pdf::contents(&d, paper).0.matches(" S\n").count())
    };
    let (lines, with) = strokes(&s);
    run(&mut s, "delete", json!({"ids": [id]}));
    let (fewer, without) = strokes(&s);
    assert!(lines - fewer > 6, "its witness lines, line, ticks and letters on the sheet: {}", lines - fewer);
    assert_eq!(with - without, lines - fewer, "each a stroke of the sheet's PDF");
}

//! `styles`'s tests that drive the Session (moved out of `styles.rs`).

use crate::graphics::MODEL;
use crate::model::Model;
use crate::pen::Pen;

use crate::engine::Session;
use crate::prim::{Part, Prim, View};
use crate::render::Drawing;
use crate::styles::*;
use serde_json::json;

/// The widest solid line `id` draws of its `part` in `d`.
fn widest(d: &Drawing, id: crate::model::Id, part: Part) -> f32 {
    (d.prims.iter().zip(&d.owners).zip(&d.parts))
        .filter_map(|((p, o), q)| match p {
            Prim::Line { width, dash: None, .. } if *o == Some(id) && *q == part => Some(*width),
            _ => None,
        })
        .fold(0.0, f32::max)
}

#[test]
fn object_styles_are_the_projects_pens_by_category_saved_and_undone() {
    let mut s = Session::new();
    // Asking says each model category's, and changes nothing.
    let r = s.execute("object_styles", &json!({})).unwrap();
    assert_eq!(r["categories"].as_array().map(Vec::len), Some(MODEL.len()));
    assert_eq!(r["categories"][0], json!({"category": "wall", "projection": 4, "cut": 10, "own": false}));
    assert!(s.history().is_empty());
    // Given, by category, a pen or both, in one undo step.
    s.execute("object_styles", &json!({"categories": {"wall": {"cut": 12}, "door": {"projection": 2, "cut": 5}}})).unwrap();
    assert_eq!(s.history(), ["Object Styles"]);
    assert_eq!(s.model.object_style("wall"), ObjectStyle { projection: Pen(4), cut: Pen(12) });
    assert_eq!(s.model.object_style("door"), ObjectStyle { projection: Pen(2), cut: Pen(5) });
    // Saved with the project and read back.
    let (m, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(m.object_styles, s.model.object_styles);
    // Given back what it starts with, a category is the project's start again: nothing of its own to keep.
    s.execute("object_styles", &json!({"categories": {"wall": {"cut": 10}}})).unwrap();
    assert_eq!(s.model.object_styles.keys().collect::<Vec<_>>(), ["door"]);
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.object_style("wall").cut, Pen(12));
    s.execute("object_styles", &json!({"reset": true})).unwrap();
    assert!(s.model.object_styles.is_empty());
    // What isn't an object style is refused, saying why, and changes nothing.
    s.execute("object_styles", &json!({"categories": {"floor": {"cut": 9}}})).unwrap();
    let before = s.model.clone();
    for (p, says) in [
        (json!({"categories": {"grid": {"cut": 3}}}), "isn't a model category"),
        (json!({"categories": {"walls": {"cut": 3}}}), "Object Styles has: wall"),
        (json!({"categories": {"wall": {"cut": 17}}}), "0 (a hairline) to 16"),
        (json!({"categories": {"wall": {"cut": 2.5}}}), "whole number"),
        (json!({"categories": {"wall": {"colour": 3}}}), "colour"),
        (json!({"categories": ["wall"]}), "categories"),
        (json!({"reset": "yes"}), "reset"),
    ] {
        let e = s.execute("object_styles", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(*s.model, *before);
}

#[test]
fn a_category_draws_its_object_styles_pens_cut_and_seen_in_plan_and_section_under_the_views_overrides() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,5000],[0,5000]], "closed": true}
           floor {}
           section {"start": [-1000, 2500], "end": [9000, 2500]}"#,
    )
    .unwrap();
    let (west, floor, section) = (100005, 100006, 100007);
    assert!(matches!(s.model.elements.get(floor), Some(crate::model::Element::Floor(_))));
    let plan = |s: &Session| crate::render::draw(&s.model, 100000, View { scale: 0.1, cx: 4000.0, cy: 2500.0, width: 1000, height: 800 });
    let cut = |s: &Session| {
        let view = crate::render::section_fit(&s.model, section, 800, 600);
        crate::render::section(&s.model, section, view)
    };
    // As a project starts: cut pen 10, the floor seen in pen 4.
    let d = plan(&s);
    assert!((widest(&d, west, Part::Cut) - d.width(Pen::CUT)).abs() < 1e-4);
    assert!((widest(&d, floor, Part::Projection) - d.width(Pen::PROJECTION)).abs() < 1e-4);
    // Walls cut in pen 13 and floors seen in pen 1: in plan, and in section where it cuts them.
    s.execute("object_styles", &json!({"categories": {"wall": {"cut": 13}, "floor": {"projection": 1}}})).unwrap();
    let d = plan(&s);
    assert!((widest(&d, west, Part::Cut) - d.width(Pen(13))).abs() < 1e-4, "{}", widest(&d, west, Part::Cut));
    assert!((widest(&d, floor, Part::Projection) - d.width(Pen(1))).abs() < 1e-4);
    let d = cut(&s);
    assert!((widest(&d, west, Part::Cut) - d.width(Pen(13))).abs() < 1e-4);
    // A wall's layers keep their own pen: the cut's weight is its outline's.
    assert!(d.pens.iter().all(|p| *p != Some(BY_CATEGORY)), "every mark replaced");
    // The view's Visibility/Graphics prevails.
    s.execute("graphics", &json!({"view": 100000, "categories": {"wall": {"cut": {"weight": 2}}}})).unwrap();
    let d = plan(&s);
    assert!((widest(&d, west, Part::Cut) - d.width(Pen(2))).abs() < 1e-4);
}

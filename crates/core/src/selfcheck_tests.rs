//! The model's self-checks (`bimcraft_model::selfcheck`) run under the core's tests: each fast path is held to its twin in a
//! model built by commands, as it was when the model's code was the core's.

use crate::engine::Session;
use crate::model::Element;
use crate::render;
use crate::selfcheck::{self, Check};
use serde_json::json;

#[test]
fn every_self_check_runs_under_the_core_tests() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    // A pipe the section cuts, and a region over the south elevation that keeps the model's ink above it.
    s.execute("pipe", &json!({"points": [[1000, 2000], [1000, 5000]], "offset": 1000})).unwrap();
    let fill = crate::fills::SOLID_FILL;
    s.execute("filled_region_type", &json!({"name": "Blue", "foreground": fill, "foreground_color": [0, 0, 255], "masking": false}))
        .unwrap();
    let ring = [[-2000, -600], [14000, -600], [14000, 6000], [-2000, 6000]];
    let sketch: Vec<_> = (0..4).map(|i| json!([ring[i], ring[(i + 1) % 4]])).collect();
    s.execute("filled_region", &json!({"view": "South", "type": "Blue", "line_style": null, "sketch": sketch})).unwrap();
    // Every view drawn: the plans, the sections and elevations, the sheets, the 3D view.
    let views: Vec<String> = s
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Level(l) => Some(l.name.clone()),
            Element::Section(v) | Element::Elevation(v) => Some(v.name.clone()),
            Element::Sheet(sh) => Some(sh.number.clone()),
            _ => None,
        })
        .chain(["{3D}".to_owned()])
        .collect();
    for v in &views {
        s.execute("render", &json!({"view": v, "width": 200, "height": 150})).unwrap();
    }
    // A section cut through work whose round runs are made only as the cut needs them, as a big model's is.
    let section = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Section(_))).map(|(id, _)| *id).unwrap();
    let work = render::SectionWork::new_deferred(&s.model);
    let view = render::section_fit_with(&s.model, &work, section, 400, 300);
    render::SectionOverlay::new(&s.model, &work, section, view);
    // What points at a wall, and the dimensions that hold it.
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).unwrap();
    assert!(!s.model.referring(wall).is_empty(), "the doors in it, the dimensions to it");
    assert!(s.model.held_by(wall).is_empty(), "nothing locked");
    let missing: Vec<Check> = Check::ALL.into_iter().filter(|c| !selfcheck::ran(*c)).collect();
    assert!(missing.is_empty(), "these self-checks never ran under the core's tests: {missing:?}");
}

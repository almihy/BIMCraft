//! `lines`'s tests that need only the model.

use crate::lines::*;
use crate::model::Model;

#[test]
fn a_models_own_serialization_holds_its_line_and_fill_styles_even_when_they_are_the_starting_ones() {
    // A file is written by `Model::to_json`, which has named them by GUID by then, so they were never equal to the starting
    // ones there and a rule skipping them when they are was dead; none is left.
    let v = serde_json::to_value(Model::new("x")).unwrap();
    for key in ["line_patterns", "line_styles", "fill_patterns", "region_types"] {
        assert!(v.get(key).is_some_and(|k| k.as_object().is_some_and(|o| !o.is_empty())), "{key}");
    }
    let file: serde_json::Value = serde_json::from_str(&Model::new("x").to_json().unwrap()).unwrap();
    for key in ["line_patterns", "line_styles", "fill_patterns", "region_types"] {
        assert!(file.get(key).is_some(), "{key}");
    }
}

#[test]
fn a_pattern_too_small_to_see_draws_solid() {
    let p = LinePattern(vec![Segment::Dash(1.0), Segment::Space(0.5)]);
    assert_eq!(p.dash(2.0), Some(vec![2.0, 1.0]));
    assert_eq!(p.dash(1.0), None);
    assert_eq!(p.dash(f32::INFINITY), None);
}

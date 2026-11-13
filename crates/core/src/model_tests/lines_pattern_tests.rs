use crate::{
    engine::Session,
    geom::P2,
    model::Model,
    render::{self, Prim, Rgb},
};
use serde_json::json;

#[test]
fn free_segments_roundtrip_and_follow_curves_without_moving_the_axis() {
    let mut s = Session::new();
    let segments = json!([
        {"space": 1}, {"offset": {"length": 8, "count": 4, "distance": 0.5}},
        {"wave": {"length": 6, "frequency": 2, "amplitude": 1}},
        {"zigzag": {"length": 6, "frequency": 3, "amplitude": 1}},
        {"text": {"length": 12, "size": 2, "text": "Gas"}}, {"dot_length": 1}, {"dash": 4}
    ]);
    s.execute("line_pattern", &json!({"name":"Utility", "segments":segments})).unwrap();
    s.execute("line_style", &json!({"name":"Utility", "pen":5, "pattern":"Utility"})).unwrap();
    let id = s.execute("detail_line", &json!({"start":[0,0], "end":[4000,0], "style":"Utility"})).unwrap()["id"].as_u64().unwrap();
    let p = &s.model.line_patterns["Utility"];
    let out = p.draw(&[(0.0, 0.0), (500.0, 0.0), (500.0, 500.0)], 5.0, 2.0, Rgb(0, 0, 0));
    assert!(out.iter().any(|p| matches!(p, Prim::Fill { .. })), "text and round dots are filled outlines");
    assert!(out.iter().flat_map(Prim::points).any(|(x, y)| x > 500.0 && y > 0.0), "offsets follow the second leg");
    let (read, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(read.line_patterns, s.model.line_patterns);
    let (a, b) = render::edge_at(&s.model, 100000, id, P2(2000.0, 0.0), 100.0).unwrap();
    assert_eq!((a.1, b.1), (0.0, 0.0), "dimensions and alignment see the centerline");
}

#[test]
fn new_segment_parameters_are_validated_atomically() {
    let mut s = Session::new();
    for segment in [
        json!({"offset":{"length":2,"count":1,"distance":0.5}}),
        json!({"offset":{"length":2,"count":10,"distance":0.5}}),
        json!({"offset":{"length":2,"count":3,"distance":-1}}),
        json!({"wave":{"length":2,"frequency":0,"amplitude":1}}),
        json!({"zigzag":{"length":2,"frequency":2,"amplitude":-1}}),
        json!({"text":{"length":8,"size":0,"text":"Gas"}}),
        json!({"text":{"length":8,"size":2,"text":"a\nb"}}),
        json!({"dot_length":0}),
    ] {
        assert!(s.execute("line_pattern", &json!({"name":"Bad", "segments":[segment]})).is_err());
        assert!(!s.model.line_patterns.contains_key("Bad"));
    }
    s.execute("line_pattern", &json!({"name":"Wave", "segments":[{"wave":{"length":5,"frequency":2,"amplitude":1}}]})).unwrap();
}

#[test]
fn pattern_dots_and_lettering_take_line_overrides_and_exports_keep_outlines() {
    let mut s = Session::new();
    s.execute(
        "line_pattern",
        &json!({"name":"Gas", "segments":[
            {"dot_length":2},{"text":{"length":12,"size":2,"text":"Gas"}},{"dash":6}
        ]}),
    )
    .unwrap();
    s.execute("line_style", &json!({"name":"Gas", "pen":3,"pattern":"Gas"})).unwrap();
    let id = s.execute("detail_line", &json!({"start":[0,0],"end":[4000,0],"style":"Gas"})).unwrap()["id"].as_u64().unwrap();
    s.execute("graphics", &json!({"view":100000,"elements":{id.to_string():{"projection":{"color":[200,0,0],"weight":12}}}})).unwrap();
    let d = render::draw(&s.model, 100000, render::View { scale: 0.2, cx: 2000.0, cy: 0.0, width: 1000, height: 300 });
    let mut dot = None;
    let mut stroke = None;
    for ((prim, owner), pen) in d.prims.iter().zip(&d.owners).zip(&d.pens) {
        if *owner != Some(id) {
            continue;
        }
        match prim {
            Prim::Fill { rings, color, .. } => {
                assert_eq!(*color, Rgb(200, 0, 0), "both text and dots take line colour");
                if pen.is_some() {
                    let (lo, hi) = rings.iter().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
                    dot = Some(hi - lo);
                }
            }
            Prim::Line { width, .. } => stroke = Some(*width),
            _ => {}
        }
    }
    assert!((dot.unwrap() - stroke.unwrap()).abs() < 1e-3, "dot diameter follows the overridden pen");
    assert!(render::svg(&d).contains("#c80000"));
    let pdf = crate::pdf::pdf(&[(&d, P2(250.0, 75.0))]);
    assert!(pdf.starts_with(b"%PDF"));
    assert!(!pdf.windows(5).any(|w| w == b"/Font"), "glyph outlines export without system font dependencies");
}

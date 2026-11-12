//! `pen`'s tests that drive the Session (moved out of `pen.rs`).

use crate::units::Units;

use crate::pen::*;

#[test]
fn pens_widen_from_a_hairline_and_a_width_finds_the_nearest() {
    assert_eq!(Pen::HAIRLINE.mm(), 0.0);
    assert!((1..=MAX_PEN).all(|p| Pen(p).mm() > Pen(p - 1).mm()), "each wider than the one before");
    assert!(Pen(MAX_PEN).is_valid() && !Pen(MAX_PEN + 1).is_valid());
    assert_eq!(Pen(MAX_PEN + 1).mm(), 0.0);
    // A sheet's drawing has 5 px to the mm: the weights from before pens find the pen nearest as wide.
    for (px, pen) in [(0.5, 1), (0.75, 3), (0.9, 4), (1.0, 5), (1.5, 9), (2.0, 11), (2.5, 12), (3.5, 13), (5.0, 14), (20.0, 16)] {
        assert_eq!(Pen::nearest(px / 5.0), Pen(pen), "{px} px");
    }
    assert_eq!(Pen::nearest(0.0), Pen(1), "a width is never a hairline");
}

#[test]
fn line_weights_are_the_projects_set_whole_saved_and_undone() {
    use crate::engine::Session;
    use serde_json::json;
    let mut s = Session::new();
    // Asking says what they are, as the units name the scales, and changes nothing.
    let r = s.execute("line_weights", &json!({})).unwrap();
    assert_eq!(r["model"][3], json!({"scale": 100.0, "name": "1 : 100", "widths": WIDTHS[1..].to_vec()}));
    assert_eq!((r["annotation"][9].clone(), r["own"].clone()), (json!(0.35), json!(false)));
    assert!(s.history().is_empty());
    // Given, whole, in one undo step; the scales kept finest first.
    let model = json!([{"scale": 200, "widths": vec![0.1; 16]}, {"scale": 20, "widths": vec![0.5; 16]}]);
    let r = s.execute("line_weights", &json!({"model": model, "annotation": vec![0.3; 16]})).unwrap();
    assert_eq!((r["model"][0]["scale"].clone(), r["own"].clone()), (json!(20.0), json!(true)));
    assert_eq!(s.history(), ["Line Weights"]);
    let lw = s.model.line_weights();
    assert_eq!((lw.at(30.0).mm(Pen(4), false), lw.at(1000.0).mm(Pen(4), false), lw.at(1000.0).mm(Pen(4), true)), (0.5, 0.1, 0.3));
    // Only the annotation: the model's stay.
    s.execute("line_weights", &json!({"annotation": vec![0.2; 16]})).unwrap();
    assert_eq!(s.model.line_weights().model.len(), 2);
    // Saved with the project and read back.
    let (m, _) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(m.line_weights, s.model.line_weights);
    // Undone, and reset to the project's own start.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(s.model.line_weights().annotation, [0.3; 16]);
    s.execute("line_weights", &json!({"reset": true})).unwrap();
    assert_eq!(s.model.line_weights, None);
    // What isn't line weights is refused, saying why, and changes nothing.
    let before = s.model.clone();
    for (p, says) in [
        (json!({"model": []}), "1 to 32 scales"),
        (json!({"annotation": vec![0.2; 15]}), "16"),
        (json!({"annotation": vec![-1.0; 16]}), "annotation pen 1 is -1 mm wide"),
        (json!({"model": [{"scale": 50, "widths": vec![0.2; 16]}, {"scale": 50, "widths": vec![0.2; 16]}]}), "twice"),
        (json!({"model": [{"scale": 50, "widths": vec![0.2; 16], "pen": 3}]}), "pen"),
        (json!({"reset": "yes"}), "reset"),
    ] {
        let e = s.execute("line_weights", &p).unwrap_err();
        assert!(e.contains(says), "{p}: {e}");
    }
    assert_eq!(*s.model, *before);
}

#[test]
fn a_line_draws_its_pen_as_the_line_weights_say_the_model_by_scale_annotation_at_any() {
    use crate::engine::Session;
    use crate::prim::{Prim, View};
    use serde_json::json;
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0]]}
           grid {"start": [0, -3000], "end": [0, 3000]}
           grid {"start": [8000, -3000], "end": [8000, 3000]}
           dimension {"refs": [{"id": 100003}, {"id": 100004}], "at": [4000, -1500]}"#,
    )
    .unwrap();
    let (wall, dim) = (100002, 100005);
    let widest = |s: &Session, id| {
        let view = View { scale: 0.1, cx: 4000.0, cy: 0.0, width: 1000, height: 800 };
        let d = crate::render::draw(&s.model, 100000, view);
        (d.prims.iter().zip(&d.owners))
            .filter_map(|(p, o)| match p {
                Prim::Line { width, dash: None, .. } if *o == Some(id) => Some(*width),
                _ => None,
            })
            .fold(0.0, f32::max)
    };
    // 1 : 50 at a tenth of a pixel to the mm: 5 px to a mm of paper. The wall's cut outline is model pen 10, the
    // dimension's lines annotation pens.
    s.execute("scale", &json!({"view": 100000, "scale": 50})).unwrap();
    let w50 = s.model.line_weights().at(50.0).mm(Pen(10), false) as f32 * 5.0;
    assert!((widest(&s, wall) - w50).abs() < 1e-4, "{} {w50}", widest(&s, wall));
    let dim_before = widest(&s, dim);
    let mut model = s.model.line_weights().model.clone();
    for col in &mut model {
        col.widths = [1.0; 16];
    }
    s.execute("line_weights", &json!({"model": model})).unwrap();
    assert!((widest(&s, wall) - 5.0).abs() < 1e-4, "1 mm");
    assert_eq!(widest(&s, dim), dim_before, "annotation keeps its own");
    s.execute("line_weights", &json!({"annotation": vec![0.6; 16]})).unwrap();
    assert!((widest(&s, dim) - 3.0).abs() < 1e-4, "{}", widest(&s, dim));
    // An override's pen draws as the line weights say too.
    s.execute("graphics", &json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 3}}}})).unwrap();
    assert!((widest(&s, wall) - 5.0).abs() < 1e-4);
}

#[test]
fn a_projects_line_weights_start_as_the_pens_at_1_100_wider_finer_and_take_the_nearest_scale() {
    let lw = LineWeights::default_for(Units::Millimetres);
    assert_eq!(lw.model.iter().map(|s| s.scale).collect::<Vec<_>>(), [10.0, 20.0, 50.0, 100.0, 200.0, 500.0]);
    assert_eq!(LineWeights::default_for(Units::FeetInches).model[3].scale, 96.0, "1/8\" = 1'-0\" among an imperial project's");
    assert_eq!(lw.at(100.0), Weights::default(), "at 1 : 100, model and annotation are the pens' own widths");
    assert!(lw.at(50.0).mm(Pen(10), false) > 0.35 && lw.at(200.0).mm(Pen(10), false) < 0.35);
    assert_eq!(lw.at(50.0).mm(Pen(10), true), 0.35, "annotation, whatever the scale");
    assert_eq!((lw.at(50.0).mm(Pen(0), false), lw.at(50.0).mm(Pen(0), true)), (0.0, 0.0), "a hairline");
    // Between two, the nearer by how many times: 1 : 70 is nearer 1 : 50 (1.4 times) than 1 : 100 (1.43).
    assert_eq!(lw.at(70.0), lw.at(50.0));
    assert_eq!(lw.at(75.0), lw.at(100.0));
    assert_eq!(lw.at(5000.0), lw.at(500.0));
    assert!(lw.check().is_ok());
    let bad = |f: &dyn Fn(&mut LineWeights)| {
        let mut l = lw.clone();
        f(&mut l);
        l.check().unwrap_err()
    };
    assert!(bad(&|l| l.model.clear()).contains("1 to 32 scales"));
    assert!(bad(&|l| l.model[1].scale = 10.0).contains("given twice"));
    assert!(bad(&|l| l.model[0].scale = 0.0).contains("a scale is"));
    assert!(bad(&|l| l.model[2].widths[4] = 0.0).contains("model pen 5 at 1 : 50 is 0 mm wide"));
    assert!(bad(&|l| l.annotation[0] = f64::NAN).contains("annotation pen 1"));
}

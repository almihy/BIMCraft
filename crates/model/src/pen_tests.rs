//! `pen`'s tests that need only the model.

use crate::pen::*;
use crate::units::Units;

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

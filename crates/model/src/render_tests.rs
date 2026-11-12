//! `render`'s tests that need only the model.

use crate::geom::P2;
use crate::pen::Pen;
use crate::prim::View;
use crate::render::*;
use crate::units::Units;

#[test]
fn spot_elevations_read_signed_heights_in_the_project_units() {
    assert_eq!(
        [height(Units::Millimetres, 0.3), height(Units::Millimetres, 3000.0), height(Units::Millimetres, -600.0)],
        ["±0", "+3000", "-600"]
    );
    assert_eq!([height(Units::Metres, 0.0), height(Units::FeetInches, 3000.0)], ["±0.00", "+9'-10 1/8\""]);
}

#[test]
fn lettering_turns_its_corners_round_while_a_pens_lines_turn_them_square() {
    let view = View { scale: 1.0, cx: 0.0, cy: 0.0, width: 400, height: 200 };
    let mut d = Drawing::new(view);
    d.text("VW", (200.0, 100.0), 80.0, INK);
    d.line(&[P2(-150.0, 0.0), P2(-100.0, 50.0), P2(-50.0, 0.0)], Pen::MEDIUM, INK, false);
    let svg = svg(&d);
    let joins: Vec<&str> = svg.split("stroke-linejoin=\"").skip(1).filter_map(|s| s.split('"').next()).collect();
    assert_eq!(joins.last(), Some(&"miter"), "the pen's line");
    assert!(joins.len() > 2 && joins[..joins.len() - 1].iter().all(|j| *j == "round"), "{joins:?}");
    let (pdf, _) = crate::pdf::contents(&d, P2(100.0, 50.0));
    assert!(pdf.contains(" 1 j ") && pdf.trim_end().rsplit('\n').next().is_some_and(|l| l.contains(" 0 j ")), "{pdf}");
}

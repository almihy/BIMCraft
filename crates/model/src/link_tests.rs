//! `link`'s tests that need only the model.

use crate::ids::LINKED;

#[test]
fn a_line_keeps_its_pen_when_a_link_goes_under_the_projects() {
    use crate::pen::Pen;
    use crate::prim::{Rgb, View};
    use crate::render::Drawing;
    let mut d = Drawing::new(View { scale: 1.0, cx: 0.0, cy: 0.0, width: 10, height: 10 });
    d.own(Some(7));
    d.stroke(vec![(0.0, 0.0), (1.0, 1.0)], Pen::CUT, Rgb(0, 0, 0), None, false);
    d.own(Some(LINKED));
    d.stroke(vec![(2.0, 2.0), (3.0, 3.0)], Pen::PROJECTION, Rgb(0, 0, 0), None, false);
    d.linked_under();
    assert_eq!(d.owners, [Some(LINKED), Some(7)]);
    assert_eq!(d.pens, [Some(Pen::PROJECTION), Some(Pen::CUT)], "each line's pen goes with it");
}

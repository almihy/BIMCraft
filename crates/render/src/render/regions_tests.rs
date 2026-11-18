//! The ink a non-masking region keeps above it: in elevations, on sheets, along a region's edge, and what it costs.

use crate::geom::P2;
use crate::render::regions::*;

/// Whether `pieces` (lines along y = 50, as `Area::clip` gives them) draw at `x`, by their own dashes.
fn drawn_at(pieces: &[Prim], x: f32) -> bool {
    pieces.iter().any(|p| {
        let Prim::Line { pts, dash, .. } = p else { return false };
        let (a, b) = (pts[0].0, pts[pts.len() - 1].0);
        if x < a || x > b {
            return false;
        }
        match dash {
            None => true,
            Some(d) => {
                let (cycle, on) = (d.iter().sum::<f32>(), d[0]);
                (x - a).rem_euclid(cycle) < on
            }
        }
    })
}

#[test]
fn a_dashed_line_a_region_takes_part_of_keeps_its_dashes_where_they_were() {
    // 10 px on and 5 off from the line's start; a region from x = 103 on takes the rest of it.
    let line = Prim::line(vec![(0.0, 50.0), (400.0, 50.0)], 1.0, Rgb(0, 0, 0), Some(vec![10.0, 5.0]), false);
    let view = View { scale: 1.0, cx: 200.0, cy: 0.0, width: 400, height: 100 };
    let ring = vec![vec![P2(103.0, -100.0), P2(500.0, -100.0), P2(500.0, 100.0), P2(103.0, 100.0)]];
    let masks: Vec<Mask> = Vec::new();
    let pieces = Area::new(&ring, &masks).unwrap().clip(&line, view, 1.0);
    assert!(!pieces.is_empty());
    for k in 104..399 {
        let x = k as f32 + 0.5;
        assert_eq!(drawn_at(&pieces, x), x.rem_euclid(15.0) < 10.0, "x = {x}");
    }
}

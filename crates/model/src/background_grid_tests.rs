//! `background_grid`'s tests that need only the model.

use crate::background_grid::*;
use crate::geom::P2;
use crate::prim::View;

#[test]
fn background_grid_defaults_use_the_requested_neutral_greys() {
    let grid = BackgroundGrid::default();
    assert_eq!(grid.main.color, [230, 230, 230]);
    assert_eq!(grid.sub.color, [245, 245, 245]);
}

#[test]
fn grid_rotation_spacing_and_negative_coordinates_share_the_origin() {
    let mut g = BackgroundGrid::default();
    g.main.enabled = true;
    g.spacing = [1000.0, 2000.0];
    assert_eq!(g.nearest(P2(-610.0, 1300.0)), Some(P2(-1000.0, 2000.0)));
    g.rotation = 90.0;
    let p = g.nearest(P2(-1700.0, 850.0)).unwrap();
    assert!(p.dist(P2(-2000.0, 1000.0)) < 1e-8);
    assert_eq!(g.nearest(P2(0.0, 0.0)), Some(P2(0.0, 0.0)));
    g.main.enabled = false;
    assert_eq!(g.nearest(P2(10.0, 10.0)), None);
}

#[test]
fn subgrid_is_independent_and_drawing_is_bounded_at_extreme_zoom() {
    let mut g = BackgroundGrid::default();
    g.sub.enabled = true;
    g.spacing = [1000.0, 2000.0];
    g.divisions = [4, 5];
    assert_eq!(g.nearest(P2(370.0, -610.0)), Some(P2(250.0, -800.0)));
    let v = View { scale: 0.1, cx: 0.0, cy: 0.0, width: 800, height: 600 };
    let lines = g.lines(v);
    assert!(!lines.is_empty());
    assert!(lines.iter().any(|l| l.points[0].0 == 0.0 && l.points[1].0 == 0.0));
    g.main.enabled = true;
    g.rotation = 33.0;
    assert!(g.lines(View { scale: 1e-6, ..v }).len() <= 4096);
    g.spacing[0] = 0.0;
    assert!(!g.valid());
    assert!(g.lines(v).is_empty());
    assert_eq!(g.nearest(P2(0.0, 0.0)), None);
}

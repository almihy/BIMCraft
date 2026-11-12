//! `snap`'s tests that need only the model.
use crate::curves::Curve;
use crate::geom::P2;
use crate::model::Model;
use crate::snap::*;

const REACH: Reach = Reach { point: 100.0, line: 60.0 };

#[test]
fn sketch_snaps_find_crossings_perpendiculars_and_nearest_points_and_obey_switches() {
    use crate::sketch::SketchLine;
    let m = Model::new("Snapping");
    let lines = [SketchLine::new(P2(1000.0, 1000.0), P2(5000.0, 1000.0)), SketchLine::new(P2(2000.0, 0.0), P2(2000.0, 4000.0))];
    let at = |p, from: &[P2], o: Snaps| in_sketch(&m, Sketch { view: 100000, lines: &lines, editing: None }, p, REACH, from, false, &o);
    let f = at(P2(2030.0, 1020.0), &[], Snaps::default());
    assert_eq!((f.kind, f.at), (Kind::Intersection, P2(2000.0, 1000.0)));
    let f = at(P2(3530.0, 1020.0), &[P2(3500.0, 3000.0)], Snaps::default());
    assert_eq!((f.kind, f.at), (Kind::Perpendicular, P2(3500.0, 1000.0)));
    let f = at(P2(3523.0, 1020.0), &[], Snaps::only(Kind::Nearest));
    assert_eq!((f.kind, f.at), (Kind::Nearest, P2(3523.0, 1000.0)));
    let f = at(P2(3523.0, 1023.0), &[], Snaps::none());
    assert_eq!((f.kind, f.at), (Kind::Free, P2(3520.0, 1020.0)));
}

#[test]
fn region_sketch_curves_retain_centres_quadrants_tangents_and_exact_nearest_snaps() {
    use crate::sketch::SketchLine;
    let m = Model::new("Curved region");
    let lines = [SketchLine::from(Curve::Circle { centre: P2(0.0, 0.0), radius: 1000.0 })];
    let at = |p, from: &[P2], o: Snaps| in_sketch(&m, Sketch { view: 100000, lines: &lines, editing: None }, p, REACH, from, false, &o);
    let f = at(P2(20.0, 30.0), &[], Snaps::default());
    assert_eq!((f.kind, f.at), (Kind::Centre, P2(0.0, 0.0)));
    let f = at(P2(20.0, 1040.0), &[], Snaps::only(Kind::Quadrant));
    assert_eq!((f.kind, f.at), (Kind::Quadrant, P2(0.0, 1000.0)));
    let f = at(P2(500.0, 880.0), &[P2(2000.0, 0.0)], Snaps::only(Kind::Tangent));
    assert_eq!(f.kind, Kind::Tangent);
    assert!(f.at.dist(P2(500.0, 866.025)) < 0.01);
    let f = at(P2(707.0, -700.0), &[], Snaps::only(Kind::Nearest));
    assert_eq!(f.kind, Kind::Nearest);
    assert!((f.at.length() - 1000.0).abs() < 0.01);
    assert_eq!(at(P2(20.0, 30.0), &[], Snaps::none()).kind, Kind::Free);
}

#[test]
fn every_step_of_an_angle_gets_its_guides_round_to_the_last() {
    let from = [P2(0.0, 0.0)];
    // 7° doesn't go into 360°: the last guide is at 357°, and 170° has one at 340°.
    assert_eq!(rays(&from, 7.0).last(), Some(&357.0));
    assert_eq!(rays(&from, 170.0), [0.0, 170.0, 340.0]);
    assert_eq!(rays(&from, 15.0).len(), 24);
    assert_eq!(rays(&from, 90.0), [0.0, 90.0, 180.0, 270.0]);
}

#[test]
fn the_ruler_reads_length_parts_and_angle_and_its_graduations_are_round() {
    let r = ruler(P2(0.0, 0.0), P2(3000.0, 4000.0));
    assert_eq!((r.length, r.dx, r.dy), (5000.0, 3000.0, 4000.0));
    assert!((r.angle - 53.130102354).abs() < 1e-6);
    assert_eq!(ruler(P2(0.0, 0.0), P2(-1000.0, 0.0)).angle, 180.0);
    assert_eq!(ruler(P2(0.0, 0.0), P2(0.0, -1000.0)).angle, 270.0);
    assert_eq!(ruler(P2(5.0, 5.0), P2(5.0, 5.0)).angle, 0.0);
    // 1, 2 or 5 times a power of ten, at least 10 px apart.
    assert_eq!(
        [graduation(1.0), graduation(0.5), graduation(0.1), graduation(0.02), graduation(0.004)],
        [10.0, 20.0, 100.0, 500.0, 5000.0]
    );
    assert_eq!(graduation(0.0), 100.0);
    assert_eq!(graduation(f64::NAN), 100.0);
}

#[test]
fn a_polylines_middles_are_each_segments() {
    use crate::sketch::SketchLine;
    let m = Model::new("Box");
    let lines = [SketchLine::from(Curve::Poly {
        points: vec![P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 3000.0), P2(0.0, 3000.0)],
        arcs: Vec::new(),
        closed: true,
    })];
    let at =
        |p: P2| in_sketch(&m, Sketch { view: 100000, lines: &lines, editing: None }, p, REACH, &[], false, &Snaps::only(Kind::Midpoint));
    for (near, middle) in
        [(P2(2030.0, 20.0), P2(2000.0, 0.0)), (P2(4020.0, 1530.0), P2(4000.0, 1500.0)), (P2(2020.0, 3030.0), P2(2000.0, 3000.0))]
    {
        let f = at(near);
        assert_eq!((f.kind, f.at), (Kind::Midpoint, middle), "{f:?}");
    }
}

#[test]
fn the_nearest_chords_are_the_ones_tried_when_there_are_too_many_near() {
    use crate::sketch::SketchLine;
    let m = Model::new("Many");
    // A line of a hundred 10 mm chords, and one across it at 950: more chords near the cursor than are tried.
    let line = Curve::Poly { points: (0..=100).map(|i| P2(f64::from(i) * 10.0, 0.0)).collect(), arcs: Vec::new(), closed: false };
    let lines = [SketchLine::from(line), SketchLine::new(P2(955.0, -500.0), P2(955.0, 500.0))];
    let reach = Reach { point: 800.0, line: 60.0 };
    let f = in_sketch(
        &m,
        Sketch { view: 100000, lines: &lines, editing: None },
        P2(958.0, 4.0),
        reach,
        &[],
        false,
        &Snaps::only(Kind::Intersection),
    );
    assert_eq!((f.kind, f.at), (Kind::Intersection, P2(955.0, 0.0)), "{f:?}");
}

//! `walls`'s tests that need only the model.

use crate::geom::P2;
use crate::walls::*;
use std::f64::consts::PI;

fn near(a: P2, b: P2) -> bool {
    a.dist(b) < 1e-6
}

#[test]
fn an_arc_through_three_points_turns_toward_the_middle_one() {
    assert!((sweep_through(P2(0.0, 0.0), P2(2000.0, -2000.0), P2(4000.0, 0.0)) - 180.0).abs() < 1e-9);
    assert!((sweep_through(P2(0.0, 0.0), P2(2000.0, 2000.0), P2(4000.0, 0.0)) + 180.0).abs() < 1e-9);
    let a = Axis::new(P2(100.0, 50.0), P2(2100.0, -900.0), -75.0);
    assert!((sweep_through(a.start, a.at(700.0, 0.0), a.end) + 75.0).abs() < 1e-9);
    assert_eq!(sweep_through(P2(0.0, 0.0), P2(1000.0, 0.0), P2(4000.0, 0.0)), 0.0);
}

#[test]
fn arcs_turn_left_for_a_positive_sweep_and_measure_along_themselves() {
    // From west to east turning left a half circle: it bulges south.
    let a = Axis::new(P2(0.0, 0.0), P2(4000.0, 0.0), 180.0);
    assert!(near(a.centre().unwrap(), P2(2000.0, 0.0)) && (a.radius().unwrap() - 2000.0).abs() < 1e-9);
    assert!(near(a.at(a.length() / 2.0, 0.0), P2(2000.0, -2000.0)));
    assert!(near(a.tangent(0.0), P2(0.0, -1.0)), "it sets off south");
    // Its left is toward the centre; a point projects to how far along it lies and how far left.
    assert!(near(a.at(a.length() / 2.0, 100.0), P2(2000.0, -1900.0)));
    let (s, left) = a.project(P2(2000.0, -2100.0));
    assert!((s - a.length() / 2.0).abs() < 1e-6 && (left + 100.0).abs() < 1e-9);
    assert_eq!(a.project(P2(-500.0, 300.0)).0, 0.0, "past its start, its start");
    // Past half a circle its centre is to the right of the chord.
    let big = Axis::new(P2(0.0, 0.0), P2(2000.0, 0.0), 270.0);
    assert!(big.centre().unwrap().1 < 0.0);
    assert!((big.length() - 1.5 * PI * big.radius().unwrap()).abs() < 1e-6);
    let points = a.points(0.0, a.length(), 0.0);
    assert!(points.len() > 30 && points.iter().all(|p| (p.dist(P2(2000.0, 0.0)) - 2000.0).abs() < 1e-6));
}

//! `walls`'s tests that drive the Session (moved out of `walls.rs`).

use crate::geom::{self, P2};
use crate::model::Element;
use std::f64::consts::PI;

use crate::engine::Session;
use crate::walls::*;

fn session(script: &str) -> Session {
    let mut s = Session::new();
    s.script(script).unwrap();
    s
}

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

/// Two 200 mm walls leaving the origin, east and `degrees` round from it.
fn corner(degrees: f64) -> Session {
    let (x, y) = (4000.0 * degrees.to_radians().cos(), 4000.0 * degrees.to_radians().sin());
    session(&format!(r#"wall {{"points": [[4000, 0], [0, 0], [{x}, {y}]]}}"#))
}

#[test]
fn walls_meet_at_any_angle_in_a_mitre() {
    for degrees in [90.0, 60.0, 120.0, 150.0] {
        let s = corner(degrees);
        let (a, b) = (outline(&s.model, 100002), outline(&s.model, 100003));
        // The faces cross on the bisector, 100 / sin(half the angle) from the corner, both ways.
        let half = (degrees / 2.0f64).to_radians();
        let inner = P2(half.cos(), half.sin()) * (100.0 / half.sin());
        for o in [&a, &b] {
            assert!(o.iter().any(|p| near(*p, inner)) && o.iter().any(|p| near(*p, inner * -1.0)), "{degrees}°: {o:?}");
            // Mitred, each is as big as its length times its thickness: nothing overlaps, nothing's missing.
            assert!((geom::area(o).abs() - 800_000.0).abs() < 1e-3, "{degrees}°: {}", geom::area(o));
        }
    }
}

#[test]
fn sharp_corners_square_off_rather_than_spike() {
    let s = corner(20.0);
    let o = outline(&s.model, 100003);
    // Outside the corner the faces would cross 5.7 thicknesses back: the end goes square there.
    assert!(o.iter().all(|p| p.length() < 700.0 || p.length() > 3000.0), "{o:?}");
    assert!(o.iter().any(|p| near(*p, P2(-20f64.to_radians().sin(), 20f64.to_radians().cos()) * 100.0)), "{o:?}");
}

#[test]
fn walls_meeting_at_a_shallow_angle_square_off_rather_than_run_on_metres() {
    // 7.5° apart: their faces would cross 1.5 m along each wall, which is no corner but a wedge. A wall's end goes
    // square instead, no farther from the node than the thickest wall there is thick.
    let s = corner(7.5);
    for id in [100002, 100003] {
        let o = outline(&s.model, id);
        assert!(o.iter().all(|p| p.length() < 300.0 || p.length() > 3000.0), "{id}: {o:?}");
    }
}

#[test]
fn a_wall_ending_on_another_stops_at_its_face() {
    let s = session(
        r#"wall {"points": [[-2000, 0], [2000, 0]], "type": "Exterior - 300mm"}
           wall {"points": [[0, 0], [1000, 3000]], "type": "Interior - 100mm"}"#,
    );
    let t = outline(&s.model, 100003);
    assert!(t.iter().filter(|p| p.1 < 1000.0).all(|p| (p.1 - 150.0).abs() < 1e-6), "{t:?}");
    assert_eq!(cap(&s.model, 100003, false).len(), 2, "the through wall fills the node");
    // The through wall goes on, square at its ends.
    assert_eq!(outline(&s.model, 100002).len(), 4);
}

#[test]
fn three_walls_meeting_share_the_node() {
    let s = session(
        r#"wall {"points": [[0, 0], [3000, 0]]}
           wall {"points": [[0, 0], [-1500, 2598.0762]]}
           wall {"points": [[0, 0], [-1500, -2598.0762]]}"#,
    );
    let total: f64 = [100_002, 100_003, 100_004].iter().map(|id| geom::area(&outline(&s.model, *id)).abs()).sum();
    // Three 3 m arms 200 mm thick, less the triangle they'd all three cover where they meet.
    let hub = 3f64.sqrt() / 4.0 * 200.0 * 200.0;
    assert!((total - (3.0 * 600_000.0 - hub)).abs() < 1.0, "{total}");
    for id in [100_002, 100_003, 100_004] {
        assert!(cap(&s.model, id, false).iter().any(|p| near(*p, P2(0.0, 0.0))), "{id}");
    }
}

#[test]
fn curved_walls_outline_their_arc_and_join_at_their_ends() {
    let s = session(
        r#"wall {"points": [[0, 0], [4000, 0]], "arc": 180}
           wall {"points": [[4000, 0], [4000, 3000]]}"#,
    );
    // Half a ring 2 m out, 200 mm thick: its length on the centre line times its thickness.
    let o = outline(&s.model, 100002);
    let ring = PI * 2000.0 * 200.0;
    assert!((geom::area(&o).abs() - ring).abs() < 0.005 * ring, "{}", geom::area(&o));
    // At its east end it runs north, on into the straight wall: their ends meet square.
    let end = cap(&s.model, 100002, true);
    assert!(end.iter().any(|p| near(*p, P2(3900.0, 0.0))) && end.iter().any(|p| near(*p, P2(4100.0, 0.0))), "{end:?}");
}

#[test]
fn faces_and_layers_lie_where_the_location_line_puts_them() {
    let s = session(
        r#"type {"category": "wall", "name": "C", "layers": [{"function": "finish", "thickness": 100}, {"function": "thermal", "thickness": 50}, {"function": "structure", "thickness": 140}, {"function": "finish", "thickness": 10}]}
           wall {"points": [[0, 0], [5000, 0]], "type": "C"}
           wall {"points": [[0, 2000], [5000, 2000]], "type": "C", "location": "core-centre"}"#,
    );
    let (a, b) = (s.model.wall(100002).unwrap(), s.model.wall(100003).unwrap());
    assert_eq!(boundaries(&s.model, a), [150.0, 50.0, 0.0, -140.0, -150.0]);
    assert_eq!(boundaries(&s.model, b), [220.0, 120.0, 70.0, -70.0, -80.0]);
    assert_eq!((faces(&s.model, b), centre(&s.model, b)), ((220.0, -80.0), 70.0));
    let lines = layer_lines(&s.model, 100003);
    assert_eq!(lines.len(), 5);
    assert!(near(lines[2][0], P2(0.0, 2070.0)) && near(lines[2][1], P2(5000.0, 2070.0)));
}

#[test]
fn walls_split_into_prisms_around_their_openings_and_under_what_they_follow() {
    let s = session(
        r#"wall {"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}
           window {"at": [8000, 3000]}
           roof {"gable": true}"#,
    );
    // Unattached: below and above the window, and either side of it.
    assert_eq!(prisms(&s.model, 100003, true).len(), 4);
    assert_eq!(prisms(&s.model, 100003, false).len(), 1);
    let mut m = (*s.model).clone();
    let roof = m.guid(100007);
    if let Some(Element::Wall(w)) = m.elements.get_mut(100003) {
        w.attach_top = Some(roof);
    }
    // Attached under a gable: the parts either side of the ridge, under the two slopes.
    let whole = prisms(&m, 100003, false);
    assert_eq!(whole.len(), 2);
    let tan = 30f64.to_radians().tan();
    assert!(whole.iter().all(|p| (p.top.at(P2(8000.0, 3000.0)) - (3000.0 + 3100.0 * tan)).abs() < 1e-6));
}

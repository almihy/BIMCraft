//! `snap`'s tests that drive the Session (moved out of `snap.rs`).

use crate::curves::Curve;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::walls::Axis;

use crate::engine::Session;
use crate::snap::*;

const REACH: Reach = Reach { point: 100.0, line: 60.0 };

fn model(script: &str) -> Model {
    let mut s = Session::new();
    s.script(script).unwrap();
    (*s.model).clone()
}

fn at(m: &Model, p: P2, from: &[P2], o: &Snaps) -> Found {
    snap(m, 100000, p, REACH, from, false, o)
}

fn degrees(from: P2, to: P2) -> f64 {
    (to.1 - from.1).atan2(to.0 - from.0).to_degrees()
}

#[test]
fn drafting_snaps_follow_the_active_view_and_its_visibility() {
    let mut s = Session::new();
    s.script(
        r#"detail_line {"points": [[1000,1000],[5000,1000]]}
        duplicate_view {"view": "Level 1", "name": "Other"}"#,
    )
    .unwrap();
    let view = s.model.elements.iter().find(|(_, e)| matches!(e, Element::PlanView(v) if v.name == "Other")).unwrap().0;
    let p = P2(1030.0, 1020.0);
    assert_eq!(at(&s.model, p, &[], &Snaps::default()).kind, Kind::Endpoint);
    assert_eq!(snap(&s.model, *view, p, REACH, &[], false, &Snaps::default()).kind, Kind::Free);
    s.execute("graphics", &serde_json::json!({"view": "Level 1", "hide": ["line"]})).unwrap();
    assert_eq!(at(&s.model, p, &[], &Snaps::default()).kind, Kind::Free);
}

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
fn points_snap_to_the_corners_of_a_mass_where_the_plan_cuts_it() {
    let m = model(r#"mass {"boundary": [[1000,1000],[5000,1000],[5000,4000],[1000,4000]], "height": 2000}"#);
    let o = Snaps::default();
    let f = at(&m, P2(5030.0, 4020.0), &[], &o);
    assert_eq!((f.at, f.kind), (P2(5000.0, 4000.0), Kind::Endpoint));
    // Level 2's plan cuts at 4200 mm, over its top at 2000: nothing there to snap to.
    let f = snap(&m, 100001, P2(5030.0, 4020.0), REACH, &[], false, &o);
    assert_eq!((f.at, f.kind), (P2(5030.0, 4020.0), Kind::Free));
}

#[test]
fn points_snap_to_the_corners_of_a_lofted_mass_at_the_height_the_plan_cuts_it() {
    let m = model(
        r#"mass {"boundary": [[0,0],[10000,0],[10000,6000],[0,6000]], "height": 6000, "top": [[2000,2000],[8000,2000],[8000,4000],[2000,4000]]}"#,
    );
    // Level 1's plan cuts 1200 mm up, a fifth of the way to the top outline: the corner is at 400, 400.
    let f = at(&m, P2(430.0, 380.0), &[], &Snaps::default());
    assert_eq!((f.at.0.round(), f.at.1.round(), f.kind), (400.0, 400.0, Kind::Endpoint));
}

#[test]
fn points_snap_to_the_ends_middles_and_centres_of_walls() {
    let m = model(
        r#"wall {"points": [[0,0],[6000,0]]}
           wall {"points": [[0,5000],[4000,5000]], "arc": 180}"#,
    );
    let o = Snaps::default();
    let f = at(&m, P2(30.0, -20.0), &[], &o);
    assert_eq!((f.at, f.kind), (P2(0.0, 0.0), Kind::Endpoint));
    let f = at(&m, P2(3040.0, 30.0), &[], &o);
    assert_eq!((f.at, f.kind), (P2(3000.0, 0.0), Kind::Midpoint));
    // An arc's centre, and the middle of its length.
    let f = at(&m, P2(2030.0, 4980.0), &[], &o);
    assert_eq!((f.at, f.kind), (P2(2000.0, 5000.0), Kind::Centre));
    let f = at(&m, P2(2030.0, 3020.0), &[], &o);
    assert_eq!((f.kind, (f.at.dist(P2(2000.0, 5000.0)) * 1e3).round() / 1e3), (Kind::Midpoint, 2000.0));
    // Beyond their reach, a wall's centre line, then the nearest 10 mm.
    let f = at(&m, P2(1234.0, 40.0), &[], &o);
    assert_eq!((f.at, f.kind), (P2(1230.0, 0.0), Kind::Nearest));
    let f = at(&m, P2(1234.0, 900.0), &[], &o);
    assert_eq!((f.at, f.kind), (P2(1230.0, 900.0), Kind::Free));
}

/// A project with a 300 x 300 column centred at 2000, 1000 (its left edge at x = 1850), as issue #88 draws one.
const COLUMN: &str = r#"type {"category": "column", "name": "Square 300", "width": 300, "depth": 300}
    column {"at": [2000,1000], "type": "Square 300"}"#;

#[test]
fn points_snap_to_the_corners_and_edge_midpoints_of_a_column() {
    let m = model(COLUMN);
    let o = Snaps::default();
    let column = m.elements.iter().find(|(_, e)| matches!(e, Element::Column(_))).map(|(id, _)| *id);
    // The corners are its outline's: ends.
    for (near, corner) in [
        (P2(1830.0, 840.0), P2(1850.0, 850.0)),
        (P2(2170.0, 860.0), P2(2150.0, 850.0)),
        (P2(2160.0, 1140.0), P2(2150.0, 1150.0)),
        (P2(1840.0, 1160.0), P2(1850.0, 1150.0)),
    ] {
        let f = at(&m, near, &[], &o);
        assert_eq!((f.at, f.kind), (corner, Kind::Endpoint), "near {near:?}");
        assert_eq!(f.source, column, "the column's");
    }
    // The middles of its edges, so a 250 mm wall can run along the left edge or start from it.
    for (near, middle) in [
        (P2(1840.0, 1020.0), P2(1850.0, 1000.0)),
        (P2(2020.0, 840.0), P2(2000.0, 850.0)),
        (P2(2160.0, 980.0), P2(2150.0, 1000.0)),
        (P2(1980.0, 1160.0), P2(2000.0, 1150.0)),
    ] {
        let f = at(&m, near, &[], &o);
        assert_eq!((f.at, f.kind), (middle, Kind::Midpoint), "near {near:?}");
    }
    // The centre is still there, and each snap obeys its switch.
    assert_eq!((at(&m, P2(2030.0, 1020.0), &[], &o).at, at(&m, P2(2030.0, 1020.0), &[], &o).kind), (P2(2000.0, 1000.0), Kind::Centre));
    assert_ne!(at(&m, P2(1830.0, 840.0), &[], &Snaps::only(Kind::Midpoint)).at, P2(1850.0, 850.0));
    assert_eq!(at(&m, P2(1830.0, 840.0), &[], &Snaps::only(Kind::Endpoint)).at, P2(1850.0, 850.0));
}

#[test]
fn a_turned_column_snaps_at_its_turned_corners_and_a_round_one_at_its_quadrants() {
    let m = model(
        r#"type {"category": "column", "name": "Square 300", "width": 300, "depth": 300}
           column {"at": [0,0], "type": "Square 300", "angle": 45}
           column {"at": [5000,0], "type": "Round 300"}"#,
    );
    let o = Snaps::default();
    // A square turned 45 degrees stands on a corner: 150 * sqrt 2 east of its centre.
    let reach = 150.0 * std::f64::consts::SQRT_2;
    let f = at(&m, P2(reach + 20.0, 20.0), &[], &o);
    assert_eq!((f.kind, (f.at.0 * 1e3).round() / 1e3, f.at.1.abs() < 1e-6), (Kind::Endpoint, (reach * 1e3).round() / 1e3, true));
    // A round column has its four quadrant points, not corners.
    for (near, quadrant) in [
        (P2(5170.0, 20.0), P2(5150.0, 0.0)),
        (P2(5020.0, 170.0), P2(5000.0, 150.0)),
        (P2(4830.0, -20.0), P2(4850.0, 0.0)),
        (P2(4980.0, -170.0), P2(5000.0, -150.0)),
    ] {
        let f = at(&m, near, &[], &o);
        assert_eq!((f.kind, f.at.dist(quadrant) < 1e-6), (Kind::Quadrant, true), "near {near:?}: {f:?}");
    }
    assert_eq!(at(&m, P2(5170.0, 20.0), &[], &Snaps::only(Kind::Endpoint)).kind, Kind::Free, "no corners on a round one");
}

#[test]
fn structural_columns_and_footings_snap_at_their_edges_too() {
    let m = model(
        r#"structural_column {"at": [0,0]}
           footing {"at": [6000,0]}"#,
    );
    let o = Snaps::default();
    let ids: Vec<Id> = m.elements.iter().filter(|(_, e)| e.column().is_some()).map(|(id, _)| *id).collect();
    assert_eq!(ids.len(), 2);
    for id in ids {
        let Some(e) = m.elements.get(id) else { panic!("{id} is there") };
        let outline = m.column_outline(e);
        assert_eq!(outline.len(), 4);
        let corner = outline[2];
        let f = at(&m, corner + P2(15.0, 15.0), &[], &o);
        assert_eq!((f.at.dist(corner) < 1e-6, f.kind, f.source), (true, Kind::Endpoint, Some(id)));
    }
}

#[test]
fn a_wall_drawn_from_a_columns_edge_middle_starts_there() {
    let mut s = Session::new();
    s.script(COLUMN).unwrap();
    // Picked near the left edge's middle: it lands on the edge, 150 mm from the centre.
    let start = at(&s.model, P2(1842.0, 1013.0), &[], &Snaps::default());
    assert_eq!((start.at, start.kind), (P2(1850.0, 1000.0), Kind::Midpoint));
    // And a wall to the west of it ends flush with the column, on the edge itself.
    let end = at(&s.model, P2(-1000.0, 1000.0), &[start.at], &Snaps::default());
    s.execute("wall", &serde_json::json!({"points": [[end.at.0, end.at.1], [start.at.0, start.at.1]]})).unwrap();
    let wall = s.model.elements.values().find_map(|e| if let Element::Wall(w) = e { Some(w.clone()) } else { None }).unwrap();
    assert_eq!(wall.end, P2(1850.0, 1000.0));
}

#[test]
fn walls_snap_where_they_cross_and_the_points_picked_so_far_are_ends() {
    let m = model(
        r#"wall {"points": [[0,0],[6000,0]]}
           wall {"points": [[2000,-1000],[2000,3000]]}"#,
    );
    let f = at(&m, P2(2040.0, 30.0), &[], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(2000.0, 0.0), Kind::Intersection));
    // Closing a loop: the first point picked.
    let f = at(&m, P2(5030.0, 5020.0), &[P2(5000.0, 5000.0), P2(5000.0, 4000.0), P2(3000.0, 4000.0)], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(5000.0, 5000.0), Kind::Endpoint));
}

#[test]
fn a_point_snaps_to_the_foot_of_the_perpendicular_from_the_last_one() {
    let m = model(r#"wall {"points": [[0,0],[6000,2000]]}"#);
    // 14e6 / 40e6 along the wall: (2100, 700), straight down no multiple of 15°.
    let f = at(&m, P2(2130.0, 680.0), &[P2(1000.0, 4000.0)], &Snaps::default());
    assert_eq!(f.kind, Kind::Perpendicular);
    assert!(f.at.dist(P2(2100.0, 700.0)) < 1e-6, "{:?}", f.at);
    let g = f.guide.unwrap();
    assert_eq!(g.from, P2(1000.0, 4000.0));
    assert!((g.angle - degrees(P2(1000.0, 4000.0), P2(2100.0, 700.0)).rem_euclid(360.0)).abs() < 1e-6, "{}", g.angle);
    // Not there once it's off.
    let f = at(&m, P2(2130.0, 680.0), &[P2(1000.0, 4000.0)], &Snaps { perpendicular: false, ..Snaps::default() });
    assert_ne!(f.kind, Kind::Perpendicular);
}

#[test]
fn directions_snap_to_the_angle_from_east_and_from_the_line_before() {
    let m = model("");
    let o = Snaps::default();
    // 30.2° from the first point: onto 30°, its length to the nearest 10 mm.
    let f = at(&m, P2(3470.0, 2020.0), &[P2(0.0, 0.0)], &o);
    assert_eq!(f.kind, Kind::Angle);
    assert!((degrees(P2(0.0, 0.0), f.at) - 30.0).abs() < 1e-4, "{:?}", f.at);
    assert!((f.at.length() / 10.0 - (f.at.length() / 10.0).round()).abs() < 1e-3, "{}", f.at.length());
    let g = f.guide.unwrap();
    assert_eq!((g.from, g.angle.round(), g.corner), (P2(0.0, 0.0), 30.0, None));
    // 22.5° is between guides: free.
    let f = at(&m, P2(3000.0, 1243.0), &[P2(0.0, 0.0)], &o);
    assert_eq!((f.kind, f.guide), (Kind::Free, None));
    // Off: free.
    let f = at(&m, P2(3470.0, 2020.0), &[P2(0.0, 0.0)], &Snaps { angle: 0.0, ..o });
    assert_eq!((f.at, f.kind), (P2(3470.0, 2020.0), Kind::Free));
    // From the line before: after a wall at 20°, 90° to it is 110°, no multiple of 15°.
    let (prev, last) = (P2(0.0, 0.0), P2(20f64.to_radians().cos(), 20f64.to_radians().sin()) * 3000.0);
    let want = last + P2(110f64.to_radians().cos(), 110f64.to_radians().sin()) * 2000.0;
    let f = at(&m, want + P2(25.0, 10.0), &[prev, last], &o);
    assert_eq!(f.kind, Kind::Angle);
    assert!((degrees(last, f.at) - 110.0).abs() < 1e-3, "{:?}", f.at);
    assert!(f.guide.and_then(|g| g.corner).is_some_and(|c| (c - 90.0).abs() < 1e-9), "{:?}", f.guide);
}

#[test]
fn a_point_on_the_grid_that_lies_on_the_guide_stays_where_it_is() {
    let m = model("");
    // 225° from (4000, 0) runs through (2000, -2000), a whole 2828.4 mm out: not rounded to 2830.
    let f = at(&m, P2(2000.0, -2000.0), &[P2(4000.0, 0.0)], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(2000.0, -2000.0), Kind::Angle));
    // Near it, the same; where no grid point lies on the guide, its length goes to the nearest 10 mm.
    let f = at(&m, P2(2003.0, -2003.0), &[P2(4000.0, 0.0)], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(2000.0, -2000.0), Kind::Angle));
    let f = at(&m, P2(3470.0, 2020.0), &[P2(0.0, 0.0)], &Snaps::default());
    assert!((f.at.length() % 10.0).min(10.0 - f.at.length() % 10.0) < 1e-3, "{:?}", f.at);
}

#[test]
fn a_guide_ends_where_it_crosses_a_wall() {
    let m = model(r#"wall {"points": [[3000,-3000],[3000,3000]]}"#);
    // 30° from the origin meets x = 3000 at y = 1732.05.
    let f = at(&m, P2(3010.0, 1700.0), &[P2(0.0, 0.0)], &Snaps::default());
    assert_eq!(f.kind, Kind::Intersection);
    assert!(f.at.dist(P2(3000.0, 3000.0 * 30f64.to_radians().tan())) < 1e-2, "{:?}", f.at);
    assert!(f.guide.is_some());
}

#[test]
fn a_snap_names_the_element_it_came_from() {
    let m = model(
        r#"wall {"points": [[0,0],[6000,0]]}
           wall {"points": [[2000,-1000],[2000,3000]]}"#,
    );
    let walls: Vec<Id> = m.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect();
    let o = Snaps::default();
    let f = at(&m, P2(5970.0, -20.0), &[], &o);
    assert_eq!((f.kind, f.source), (Kind::Endpoint, Some(walls[0])));
    let f = at(&m, P2(2030.0, 2020.0), &[], &o);
    assert_eq!((f.kind, f.source), (Kind::Nearest, Some(walls[1])));
    // Where two cross, the one nearer the cursor.
    let f = at(&m, P2(2040.0, 30.0), &[], &o);
    assert_eq!((f.kind, f.source), (Kind::Intersection, Some(walls[0])));
    let f = at(&m, P2(4000.0, 2000.0), &[], &o);
    assert_eq!((f.kind, f.source), (Kind::Free, None));
    // In a sketch, a wall's outline is the wall's; the sketch's own lines are no element's.
    use crate::sketch::SketchLine;
    let lines = [SketchLine::new(P2(4000.0, 2000.0), P2(5000.0, 2000.0))];
    let sketch = |p| in_sketch(&m, Sketch { view: 100000, lines: &lines, editing: None }, p, REACH, &[], false, &o);
    let f = sketch(P2(4030.0, 2020.0));
    assert_eq!((f.kind, f.source), (Kind::Endpoint, None));
    let f = sketch(P2(3500.0, 120.0));
    assert_eq!((f.kind, f.source), (Kind::Nearest, Some(walls[0])));
}

#[test]
fn each_snap_switches_off_on_its_own() {
    let m = model(r#"wall {"points": [[0,0],[6000,0]]}"#);
    let o = Snaps::default();
    assert_eq!(at(&m, P2(30.0, -20.0), &[], &Snaps { endpoint: false, ..o }).kind, Kind::Nearest);
    assert_eq!(at(&m, P2(3040.0, 30.0), &[], &Snaps { midpoint: false, ..o }).kind, Kind::Nearest);
    let none = Snaps { nearest: false, ..Snaps { endpoint: false, ..o } };
    assert_eq!(at(&m, P2(34.0, -20.0), &[], &none).at, P2(30.0, -20.0));
}

#[test]
fn nothing_a_cursor_or_a_chain_holds_makes_it_panic() {
    let m = model(r#"wall {"points": [[0,0],[6000,0]]}"#);
    let (nan, inf) = (f64::NAN, f64::INFINITY);
    let chains: [&[P2]; 5] = [&[], &[P2(0.0, 0.0)], &[P2(nan, 1.0), P2(1.0, 1.0)], &[P2(5.0, 5.0), P2(5.0, 5.0)], &[P2(inf, -inf)]];
    for p in [P2(nan, 0.0), P2(inf, -inf), P2(1e300, 1e300), P2(0.0, 0.0), P2(-0.0, 1e-300)] {
        for from in chains {
            for o in [
                Snaps::default(),
                Snaps { angle: nan, ..Snaps::default() },
                Snaps { angle: 1.0, ..Snaps::default() },
                Snaps { angle: 1e9, ..Snaps::default() },
            ] {
                for square in [true, false] {
                    let _ = snap(&m, 100000, p, REACH, from, square, &o);
                }
            }
        }
    }
    for reach in [Reach { point: 0.0, line: 0.0 }, Reach { point: nan, line: -1.0 }, Reach { point: inf, line: inf }] {
        let _ = snap(&m, 100000, P2(10.0, 10.0), reach, &[P2(0.0, 0.0)], false, &Snaps::default());
    }
}

#[test]
fn a_wall_is_found_far_from_both_its_ends_and_an_arc_where_it_bulges() {
    let m = model(
        r#"wall {"points": [[0,0],[60000,0]]}
           wall {"points": [[0,30000],[10000,30000]], "arc": 180}"#,
    );
    let f = at(&m, P2(31234.0, 40.0), &[], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(31230.0, 0.0), Kind::Nearest));
    // Out where the arc bulges, 5000 mm from its chord: past the reach and the thickest wall.
    let want = m
        .elements
        .values()
        .find_map(|e| match e {
            Element::Wall(w) if w.arc != 0.0 => {
                let a = Axis::of(w);
                Some(a.at(a.length() / 2.0, 0.0))
            }
            _ => None,
        })
        .unwrap_or_default();
    assert!((want.1 - 30000.0).abs() > 4999.0, "{want:?}");
    let f = at(&m, want + P2(30.0, 20.0), &[], &Snaps::default());
    assert_eq!(f.kind, Kind::Midpoint, "{f:?}");
    assert!(f.at.dist(want) < 1e-3, "{:?}", f.at);
}

#[test]
fn a_guides_corner_is_read_whichever_way_it_runs() {
    let m = model("");
    // A right angle off a line along an axis: the step the corner is read from is 1 mm long.
    let f = at(&m, P2(1000.0, 2000.0), &[P2(0.0, 0.0), P2(1000.0, 0.0)], &Snaps::default());
    assert_eq!(f.kind, Kind::Angle);
    assert!(f.guide.and_then(|g| g.corner).is_some_and(|c| (c - 90.0).abs() < 1e-9), "{:?}", f.guide);
    // Every one of the 24 guides, off lines along each axis and at 20°.
    for line in [P2(1000.0, 0.0), P2(0.0, 1000.0), P2(-1000.0, 0.0), P2(0.0, -1000.0), P2(940.0, 342.0)] {
        for k in 0..24 {
            let (a, last) = (f64::from(k) * 15f64.to_radians(), line);
            let to = last + P2(a.cos(), a.sin()) * 2000.0;
            let f = at(&m, to, &[P2(0.0, 0.0), last], &Snaps::default());
            let corner = f.guide.and_then(|g| g.corner);
            assert!(corner.is_some(), "off {line:?} at {}°: {f:?}", k * 15);
        }
    }
}

#[test]
fn shift_keeps_a_step_square_from_a_point_off_the_grid() {
    let m = model("");
    let from = [P2(2501.5, 1000.25)];
    let f = snap(&m, 100000, P2(5000.0, 1003.0), REACH, &from, true, &Snaps::none());
    assert_eq!(f.at, P2(5000.0, 1000.25));
    let f = snap(&m, 100000, P2(2503.0, 4000.0), REACH, &from, true, &Snaps::none());
    assert_eq!(f.at, P2(2501.5, 4000.0));
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
fn at_the_same_distance_an_end_wins_over_a_middle() {
    // A wall 100 mm long: its middle is 50 mm from either end; the cursor in the middle of it.
    let m = model(r#"wall {"points": [[0,0],[100,0]]}"#);
    let f = at(&m, P2(25.0, 0.0), &[], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(0.0, 0.0), Kind::Endpoint));
    let f = at(&m, P2(50.0, 0.0), &[], &Snaps::default());
    assert_eq!((f.at, f.kind), (P2(50.0, 0.0), Kind::Midpoint));
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

/// A 4 x 3 m box of 1 m walls, and a plan view of its level, "Low", cut at 500 mm: its level is cut at 1200.
fn low() -> (Session, Id) {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[4000,0],[4000,3000],[0,3000]], "closed": true, "height": 1000}
           duplicate_view {"view": "Level 1", "name": "Low"}"#,
    )
    .unwrap();
    let view = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::PlanView(v) if v.name == "Low")).unwrap().0;
    s.execute("modify", &serde_json::json!({"id": view, "view_range": {"cut": 500}})).unwrap();
    (s, view)
}

#[test]
fn a_plan_view_snaps_to_what_its_own_cut_plane_cuts_not_its_levels() {
    let (s, view) = low();
    let near = P2(4030.0, 20.0);
    // The wall is 1000 tall: the level's plan cuts at 1200, over it, and Low's at 500, through it.
    assert_eq!(snap(&s.model, 100000, near, REACH, &[], false, &Snaps::default()).kind, Kind::Free);
    assert_eq!(snap(&s.model, view, near, REACH, &[], false, &Snaps::default()).kind, Kind::Endpoint);
    let sketch = |view| in_sketch(&s.model, Sketch { view, lines: &[], editing: None }, near, REACH, &[], false, &Snaps::default());
    assert_eq!(sketch(100000).kind, Kind::Free);
    // The outline's corner, where the walls' faces meet, a wall thickness out from the centre lines.
    assert_eq!(sketch(view).kind, Kind::Endpoint);
}

#[test]
fn a_sketch_snaps_to_the_edges_of_an_opening_far_from_its_floors_outer_edge_too() {
    // A 20 m floor with a metre-wide opening at its middle, the cursor 9.5 m from the nearest outer edge.
    let mut s = Session::new();
    s.script(
        r#"floor {"boundary": [[0,0],[20000,0],[20000,20000],[0,20000]], "openings": [[[9500,9500],[10500,9500],[10500,10500],[9500,10500]]]}
           ceiling {"boundary": [[0,0],[20000,0],[20000,20000],[0,20000]], "openings": [[[9500,9500],[10500,9500],[10500,10500],[9500,10500]]]}"#,
    )
    .unwrap();
    let sketch = |p, o: &Snaps| in_sketch(&s.model, Sketch { view: 100000, lines: &[], editing: None }, p, REACH, &[], false, o);
    let f = sketch(P2(9800.0, 10530.0), &Snaps::only(Kind::Nearest));
    assert_eq!((f.kind, f.at), (Kind::Nearest, P2(9800.0, 10500.0)), "{f:?}");
    let f = sketch(P2(10030.0, 10520.0), &Snaps::only(Kind::Midpoint));
    assert_eq!((f.kind, f.at), (Kind::Midpoint, P2(10000.0, 10500.0)), "{f:?}");
    // The opening's corner, as before, and the outer edge, near it.
    assert_eq!(sketch(P2(10520.0, 10530.0), &Snaps::default()).kind, Kind::Endpoint);
    assert_eq!(sketch(P2(5000.0, 30.0), &Snaps::only(Kind::Nearest)).kind, Kind::Nearest);
}

#[test]
fn two_walls_that_meet_snap_where_they_cross() {
    let m = model(r#"wall {"points": [[0,0],[4000,0],[4000,3000]]}"#);
    let f = at(&m, P2(4030.0, 20.0), &[], &Snaps::only(Kind::Intersection));
    assert_eq!((f.kind, f.at), (Kind::Intersection, P2(4000.0, 0.0)));
}

#[test]
fn a_section_has_no_plan_to_snap_to() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[4000,0],[4000,3000]]}
           elevation {"facing": "south"}"#,
    )
    .unwrap();
    let elevation = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Elevation(_))).unwrap().0;
    let sketch = |view, p| in_sketch(&s.model, Sketch { view, lines: &[], editing: None }, p, REACH, &[], false, &Snaps::default());
    assert_eq!(sketch(100000, P2(4030.0, 20.0)).kind, Kind::Endpoint, "the plan has the corner");
    assert_eq!(sketch(elevation, P2(4030.0, 20.0)).kind, Kind::Free, "the elevation looks at it from the side");
    assert_eq!(snap(&s.model, elevation, P2(4030.0, 20.0), REACH, &[], false, &Snaps::default()).kind, Kind::Free);
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

#[test]
fn a_sketch_builds_the_outlines_of_the_nearest_walls_and_no_more_than_it_can_try() {
    // Forty walls 100 mm apart, all within the window of the cursor: more than are tried, which are the nearest.
    let mut s = Session::new();
    for i in 0..40 {
        s.execute("wall", &serde_json::json!({"points": [[0, i * 100], [3000, i * 100]]})).unwrap_or_default();
    }
    let level = s.model.levels()[0].0;
    let before = BUILT.with(std::cell::Cell::get);
    let sk = Sketch { view: level, lines: &[], editing: None };
    in_sketch(&s.model, sk, P2(1500.0, 20.0), REACH, &[], false, &Snaps::default());
    let built = BUILT.with(std::cell::Cell::get) - before;
    assert_eq!(built, MAX_OUTLINES, "{built} outlines built of 40 walls");
}

#[test]
fn a_sketch_in_a_big_plan_snaps_as_quickly_as_it_has_walls_near_it() {
    let m = crate::sample::highrise(20).unwrap();
    let level = m.levels()[0].0;
    // By the service core's corner, where walls of every storey meet.
    let p = P2(18030.0, 12020.0);
    let sk = |_: ()| Sketch { view: level, lines: &[], editing: None };
    let before = BUILT.with(std::cell::Cell::get);
    let f = in_sketch(&m, sk(()), p, REACH, &[], false, &Snaps::default());
    let built = BUILT.with(std::cell::Cell::get) - before;
    let walls = m.elements.values().filter(|e| matches!(e, Element::Wall(_))).count();
    eprintln!("{walls} walls: in_sketch built {built} outlines -> {f:?}");
    assert!(f.kind == Kind::Endpoint && f.at.dist(P2(18000.0, 12000.0)) < 400.0, "a corner of the core's walls: {f:?}");
    // The outlines of the walls near the cursor, not of the hundreds there are.
    assert!(built <= MAX_OUTLINES && built * 10 < walls, "{built} outlines built of {walls} walls");
}

#[test]
fn a_note_a_section_and_a_camera_snap_where_they_are_placed() {
    let mut s = Session::new();
    s.script(
        r#"text {"text": "Note", "at": [1000, 1000], "leader": [2000, 1500]}
           section {"start": [-1000, 3000], "end": [5000, 3000]}
           camera {"eye": [5000, 5000, 1500], "target": [6000, 5000, 1200]}"#,
    )
    .unwrap();
    let o = Snaps::default();
    let text_id = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Text(_))).unwrap().0;
    let section_id = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Section(_))).unwrap().0;
    let camera_id = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::View3D(_))).unwrap().0;
    assert_eq!(at(&s.model, P2(1030.0, 1020.0), &[], &o).kind, Kind::Anchor);
    assert_eq!(
        at(&s.model, P2(2030.0, 1520.0), &[], &o),
        Found { at: P2(2000.0, 1500.0), kind: Kind::Anchor, guide: None, source: Some(text_id) }
    );
    assert_eq!(
        at(&s.model, P2(-980.0, 3020.0), &[], &o),
        Found { at: P2(-1000.0, 3000.0), kind: Kind::Anchor, guide: None, source: Some(section_id) }
    );
    assert_eq!(
        at(&s.model, P2(5030.0, 5020.0), &[], &o),
        Found { at: P2(5000.0, 5000.0), kind: Kind::Endpoint, guide: None, source: Some(camera_id) }
    );
    assert_eq!(
        at(&s.model, P2(6030.0, 5020.0), &[], &o),
        Found { at: P2(6000.0, 5000.0), kind: Kind::Endpoint, guide: None, source: Some(camera_id) }
    );
    // The annotation's switch turns its anchors off; endpoints keep the camera's eye and target.
    let off = Snaps { annotation: false, ..o };
    assert_eq!(at(&s.model, P2(1030.0, 1020.0), &[], &off).kind, Kind::Free);
    assert_eq!(at(&s.model, P2(2030.0, 1520.0), &[], &off).kind, Kind::Free);
    assert_eq!(at(&s.model, P2(5030.0, 5020.0), &[], &off).kind, Kind::Endpoint);
    assert_eq!(at(&s.model, P2(1030.0, 1020.0), &[], &Snaps::only(Kind::Anchor)).kind, Kind::Anchor);
    assert_eq!(at(&s.model, P2(5030.0, 5020.0), &[], &Snaps::only(Kind::Anchor)).kind, Kind::Free);
    assert_eq!(at(&s.model, P2(5030.0, 5020.0), &[], &Snaps::only(Kind::Endpoint)).kind, Kind::Endpoint);
    assert_eq!(at(&s.model, P2(1030.0, 1020.0), &[], &Snaps::none()).kind, Kind::Free);
}

#[test]
fn a_dimension_snaps_at_its_line_and_its_stations() {
    let m = model(
        r#"wall {"points": [[0, 0], [0, 4000]]}
           wall {"points": [[4000, 0], [4000, 4000]]}
           dimension {"refs": [[0, 3000], [4000, 3000]], "at": [2000, 5000]}"#,
    );
    let o = Snaps::default();
    let dimension_id = *m.elements.iter().find(|(_, e)| matches!(e, Element::Dimension(_))).unwrap().0;
    assert_eq!(
        at(&m, P2(30.0, 5020.0), &[], &o),
        Found { at: P2(0.0, 5000.0), kind: Kind::Anchor, guide: None, source: Some(dimension_id) }
    );
    assert_eq!(
        at(&m, P2(2000.0, 5010.0), &[], &o),
        Found { at: P2(2000.0, 5000.0), kind: Kind::Anchor, guide: None, source: Some(dimension_id) }
    );
    assert_eq!(
        at(&m, P2(4030.0, 5020.0), &[], &o),
        Found { at: P2(4000.0, 5000.0), kind: Kind::Anchor, guide: None, source: Some(dimension_id) }
    );
    assert_eq!(at(&m, P2(2000.0, 5010.0), &[], &Snaps { annotation: false, ..o }).kind, Kind::Free);
}

#[test]
fn a_tag_snaps_at_its_head_and_where_its_leader_holds() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0, 1000], [6000, 1000]]}
               door {"wall": 100002, "offset": 2000}"#,
    )
    .unwrap();
    let door = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).unwrap().0;
    s.execute("tag", &serde_json::json!({"ids": [door], "leader": true})).unwrap();
    let (tag_id, tag) = s
        .model
        .elements
        .iter()
        .find_map(|(&id, e)| match e {
            Element::Tag(t) => Some((id, t)),
            _ => None,
        })
        .expect("the tag the command made");
    let host = s.model.anchor(tag.host).unwrap();
    let head = host + tag.offset;
    let o = Snaps::default();
    assert_eq!(at(&s.model, host + P2(20.0, 20.0), &[], &o), Found { at: host, kind: Kind::Anchor, guide: None, source: Some(tag_id) });
    assert_eq!(at(&s.model, head + P2(20.0, 20.0), &[], &o), Found { at: head, kind: Kind::Anchor, guide: None, source: Some(tag_id) });
    // Without a leader the place it holds is not drawn, so it is no anchor.
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0, 1000], [6000, 1000]]}
               door {"wall": 100002, "offset": 2000}"#,
    )
    .unwrap();
    let door = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).unwrap().0;
    s.execute("tag", &serde_json::json!({"ids": [door]})).unwrap();
    let host = s.model.anchor(door).unwrap();
    assert_ne!(at(&s.model, host + P2(20.0, 20.0), &[], &o).kind, Kind::Anchor);
}

#[test]
fn a_spot_a_group_and_a_repeating_detail_snap_where_they_are_placed() {
    let mut s = Session::new();
    s.script(
        r#"floor {"boundary": [[0, 0], [6000, 0], [6000, 4000], [0, 4000]]}
           spot {"at": [3000, 2000]}"#,
    )
    .unwrap();
    let member = crate::groups::Member {
        id: 0,
        guid: crate::ids::Guid::default(),
        element: Element::DetailLine(crate::lines::DetailLine {
            view: crate::ids::Guid::default(),
            curve: Curve::line(P2(0.0, 0.0), P2(0.0, 5.0)),
            style: "Thin Lines".into(),
            insulation: None,
            centre_mark: false,
            cloud: None,
            revision: None,
        }),
    };
    s.model_mut().detail_groups.insert("G".into(), crate::groups::GroupType { members: vec![member] });
    s.model_mut().sync_library();
    s.execute("detail_group", &serde_json::json!({"type": "G", "at": [1000, 1000]})).unwrap();
    s.execute("repeating_detail", &serde_json::json!({"type": "G", "start": [2000, 3000], "end": [4000, 3000], "spacing": 1000})).unwrap();
    let o = Snaps::default();
    let group_id = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::DetailGroup(_))).unwrap().0;
    let repeating_id = *s.model.elements.iter().find(|(_, e)| matches!(e, Element::RepeatingDetail(_))).unwrap().0;
    assert_eq!(at(&s.model, P2(3020.0, 2020.0), &[], &o).kind, Kind::Anchor);
    assert_eq!(
        at(&s.model, P2(1020.0, 1020.0), &[], &o),
        Found { at: P2(1000.0, 1000.0), kind: Kind::Anchor, guide: None, source: Some(group_id) }
    );
    assert_eq!(
        at(&s.model, P2(2020.0, 3020.0), &[], &o),
        Found { at: P2(2000.0, 3000.0), kind: Kind::Anchor, guide: None, source: Some(repeating_id) }
    );
    assert_eq!(
        at(&s.model, P2(3020.0, 3020.0), &[], &o),
        Found { at: P2(3000.0, 3000.0), kind: Kind::Anchor, guide: None, source: Some(repeating_id) }
    );
}

#[test]
fn a_camera_on_another_level_has_no_points_here() {
    let mut s = Session::new();
    s.script(
        r#"level {"name": "Roof", "elevation": 3000}
               camera {"eye": [5000, 5000, 3500], "target": [6000, 5000, 3200]}"#,
    )
    .unwrap();
    assert_eq!(at(&s.model, P2(5030.0, 5020.0), &[], &Snaps::default()).kind, Kind::Free);
    let level = s.model.levels().last().unwrap().0;
    assert_eq!(snap(&s.model, level, P2(5030.0, 5020.0), REACH, &[], false, &Snaps::default()).kind, Kind::Endpoint);
}

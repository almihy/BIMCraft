//! `triad`'s tests that drive the Session (moved out of `triad.rs`).

use crate::camera::{AZIMUTH, ELEVATION};
use crate::engine::Session;
use crate::triad::*;
use crate::view3d::Camera;
use crate::workplane::Frame;

fn cam(azimuth: f64, elevation: f64) -> Camera {
    Camera {
        azimuth,
        elevation,
        scale: 0.1,
        target: [1000.0, 2000.0, 500.0],
        width: 640,
        height: 480,
        lens: None,
        shift: [0.0; 2],
        clip: None,
    }
}

fn at(placed: &[Placed; 6], end: End) -> Placed {
    *placed.iter().find(|p| p.end == end).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn each_end_shows_where_the_camera_sees_that_axis_and_the_nearest_is_last() {
    // From the south, level: x to the right, z up, and the south end of y pointing at the viewer.
    let front = layout(&cam(-90.0, 0.0));
    let (x, z, south) = (at(&front, End::PlusX), at(&front, End::PlusZ), at(&front, End::MinusY));
    assert!(near(x.x, 1.0) && near(x.y, 0.0) && near(x.depth, 0.0), "{x:?}");
    assert!(near(z.x, 0.0) && near(z.y, -1.0) && near(z.depth, 0.0), "{z:?}: up the screen is negative y");
    assert!(near(south.depth, 1.0) && near(south.x, 0.0) && near(south.y, 0.0), "{south:?}");
    assert_eq!(front[5].end, End::MinusY, "drawn last, over the others");
    assert_eq!(front[0].end, End::PlusY, "the far end first");
    // The home view: the three positive ends face the viewer, the negative ones turn away, and up is up.
    let home = layout(&cam(AZIMUTH, ELEVATION));
    assert!(at(&home, End::PlusZ).y < 0.0 && at(&home, End::MinusZ).y > 0.0);
    assert!(at(&home, End::PlusZ).depth > 0.0 && at(&home, End::MinusZ).depth < 0.0);
    for p in home {
        assert!(p.x.hypot(p.y) <= 1.0 + 1e-9 && p.depth.abs() <= 1.0 + 1e-9, "{p:?}");
    }
    assert!(home.windows(2).all(|w| w[0].depth <= w[1].depth), "far to near");
}

#[test]
fn the_end_in_front_takes_the_point_where_two_ends_overlap_and_the_other_shows_beside_it() {
    // Looking along x from the east: both ends of x are at the middle, the east one nearer.
    let right = layout(&cam(0.0, 0.0));
    assert_eq!(pick(&right, (0.0, 0.0), 0.2), Some(End::PlusX));
    assert_eq!(pick(&layout(&cam(180.0, 0.0)), (0.0, 0.0), 0.2), Some(End::MinusX));
    assert_eq!(pick(&right, (1.0, 0.0), 0.2), Some(End::PlusY), "y to the right, seen from the east");
    assert_eq!(pick(&right, (0.0, -1.0), 0.2), Some(End::PlusZ));
    assert_eq!(pick(&right, (0.5, 0.5), 0.2), None, "empty space between the ends");
    // Out of reach.
    assert_eq!(pick(&right, (0.0, -0.7), 0.2), None);
    assert_eq!(pick(&right, (0.0, -0.7), 0.31), Some(End::PlusZ));
}

#[test]
fn a_click_turns_the_camera_to_look_from_that_end_and_a_second_turns_to_the_opposite_side() {
    let home = cam(AZIMUTH, ELEVATION);
    let looking = |c: Camera| (c.azimuth, c.elevation);
    for (end, side, angles) in [
        (End::PlusZ, Side::Top, (-90.0, 89.0)),
        (End::MinusZ, Side::Bottom, (-90.0, -89.0)),
        (End::MinusY, Side::Front, (-90.0, 0.0)),
        (End::PlusY, Side::Back, (90.0, 0.0)),
        (End::MinusX, Side::Left, (180.0, 0.0)),
        (End::PlusX, Side::Right, (0.0, 0.0)),
    ] {
        assert_eq!(end.side(), side);
        let there = turned(home, end);
        assert_eq!(looking(there), angles, "{end:?}");
        assert!(looks_from(&there, end), "{end:?}");
        // What the camera keeps: where it looks, how close, the picture and the lens.
        assert_eq!((there.target, there.scale, there.width, there.height, there.lens), (home.target, 0.1, 640, 480, None));
        // The second click on the same end turns to the opposite side.
        let back = turned(there, end);
        assert_eq!(looking(back), end.opposite().side().angles(), "{end:?}");
        assert!(looks_from(&back, end.opposite()) && !looks_from(&back, end), "{end:?}");
        assert_eq!(side_after(&there, end), end.opposite().side());
        assert_eq!(side_after(&home, end), side);
        // And a third click is the first again.
        assert_eq!(looking(turned(back, end)), angles, "{end:?}");
    }
    // The other end of the axis, from the first view, is its own side, not the opposite of the one just left.
    assert_eq!(side_after(&turned(home, End::PlusZ), End::MinusZ), Side::Bottom);
    assert_eq!(side_after(&turned(home, End::PlusX), End::PlusZ), Side::Top);
}

#[test]
fn a_camera_a_degree_or_so_short_of_an_end_is_there_but_two_degrees_away_is_not() {
    // The camera never turns straight down: the top view stands one degree short.
    assert!(looks_from(&cam(-90.0, 89.0), End::PlusZ));
    assert!(looks_from(&cam(37.0, 89.0), End::PlusZ), "turned about, still above");
    assert!(!looks_from(&cam(-90.0, 87.0), End::PlusZ));
    assert_eq!(side_after(&cam(-90.0, 87.0), End::PlusZ), Side::Top, "a click on it from near turns to it");
    assert!(looks_from(&cam(1.0, 0.0), End::PlusX) && !looks_from(&cam(3.0, 0.0), End::PlusX));
}

#[test]
fn the_sides_have_names_and_the_ends_theirs() {
    let sides = [Side::Top, Side::Bottom, Side::Front, Side::Back, Side::Left, Side::Right];
    let names: std::collections::BTreeSet<_> = sides.iter().map(|s| s.name()).collect();
    assert_eq!(names.len(), 6);
    assert_eq!(names, ["Back", "Bottom", "Front", "Left", "Right", "Top"].into());
    let axes: Vec<_> = ENDS.iter().map(|e| e.axis_name()).collect();
    assert_eq!(axes, ["X", "-X", "Y", "-Y", "Z", "-Z"]);
    assert!(ENDS.iter().all(|e| e.opposite().opposite() == *e && e.opposite() != *e));
    assert!(ENDS.iter().all(|e| (dot_of(e.direction(), e.opposite().direction()) + 1.0).abs() < 1e-12));
}

fn dot_of(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn the_work_plane_square_to_an_end_goes_through_the_point_of_the_current_plane_nearest_the_target() {
    let level = Frame::flat(0.0);
    let target = [1234.4, 2000.6, 500.0];
    let s = plane(Some(&level), target, End::PlusX);
    assert_eq!((s.point, s.normal), ([1234.0, 2001.0, 0.0], [1.0, 0.0, 0.0]), "down to the level, to the millimetre");
    assert_eq!(plane(Some(&level), target, End::MinusX).normal, [-1.0, 0.0, 0.0], "facing the way the end points");
    assert_eq!(plane(Some(&level), target, End::MinusY).normal, [0.0, -1.0, 0.0]);
    // A level plane stays level, facing up, whichever end of z.
    for end in [End::PlusZ, End::MinusZ] {
        assert_eq!(plane(Some(&level), target, end).normal, [0.0, 0.0, 1.0], "{end:?}");
    }
    // From an upright plane, the point on it nearest the target: the new plane crosses the old one there.
    let wall = Frame { origin: [5000.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0] };
    assert_eq!(plane(Some(&wall), target, End::PlusY).point, [5000.0, 2001.0, 500.0]);
    // With no plane to start from, through the target itself.
    assert_eq!(plane(None, target, End::PlusZ).point, [1234.0, 2001.0, 500.0]);
    // Never a negative zero in what a command is given.
    let s = plane(None, [-0.2, 0.0, 0.0], End::PlusX);
    assert!(s.point.iter().all(|v| v.is_sign_positive()), "{:?}", s.point);
}

#[test]
fn the_surface_the_command_is_given_sets_the_plane_square_to_the_axis() {
    let mut s = Session::new();
    s.script("level {\"name\": \"Level 1\", \"elevation\": 0}").ok();
    for end in ENDS {
        let m = &s.model;
        let current = m.work_plane(m.default_3d()).resolve(m).ok();
        let surface = plane(current.as_ref(), [3000.0, 4000.0, 1500.0], end);
        s.execute("work_plane", &serde_json::json!({"surface": surface})).unwrap();
        let m = &s.model;
        let frame = m.work_plane(m.default_3d()).resolve(m).unwrap();
        assert_eq!(frame.normal, surface.normal, "{end:?}");
        assert!(frame.normal.iter().zip(surface.normal).all(|(a, b)| near(*a, b)), "{end:?}");
    }
}

#[test]
fn a_camera_or_a_target_that_is_not_a_number_never_breaks_it() {
    let hostile = Camera { azimuth: f64::NAN, elevation: f64::INFINITY, scale: f64::NAN, target: [f64::NAN; 3], ..cam(0.0, 0.0) };
    let placed = layout(&hostile);
    assert_eq!(pick(&placed, (0.0, 0.0), 0.3), None);
    let _ = turned(hostile, End::PlusZ);
    let _ = looks_from(&hostile, End::PlusX);
    let s = plane(Some(&Frame::flat(f64::NAN)), [f64::INFINITY, f64::NAN, 1.0], End::PlusX);
    assert!(s.point.iter().all(|v| v.is_finite()), "{:?}", s.point);
}

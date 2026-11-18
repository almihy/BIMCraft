//! `triad`'s tests that drive the Session (moved out of `triad.rs`).

use crate::engine::Session;
use crate::triad::*;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
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

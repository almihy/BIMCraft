//! `workplane`'s tests that drive the Session (moved out of `workplane.rs`).

use crate::camera::{AZIMUTH, ELEVATION};
use crate::engine::Session;
use crate::graphics::Look;
use crate::view3d::{Scene, Surface};
use crate::workplane::*;

#[test]
fn a_level_face_is_drawn_on_at_its_height_over_its_elements_level_and_a_face_that_isnt_level_is_not() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           floor {"offset": 500}"#,
    )
    .unwrap();
    let m = &s.model;
    let levels = m.levels();
    let (one, two) = (levels[0].0, levels[1].0);
    let floor = m.elements.iter().find(|(_, e)| e.category() == "floor").map(|(id, _)| *id).unwrap();
    let wall = m.elements.iter().find(|(_, e)| e.category() == "wall").map(|(id, _)| *id).unwrap();
    let (scene, look) = (Scene::new(m), Look::of_3d(m));
    let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let image = scene.render_in(&cam, &[], &look);
    // The floor's top, picked where the picture shows it: on its level, 500 up.
    let at = cam.project([2000.0, 5000.0, 500.0]);
    let under = scene.surface_in(&image, &cam, (at.0, at.1));
    assert_eq!(under.map(|u| u.id), Some(floor));
    let plane = cursor_plane(m, &cam, (at.0, at.1), under).unwrap();
    assert_eq!((plane.on, plane.level, plane.offset, plane.z), (floor, one, 500.0, 500.0));
    // A wall's face stands up: nothing to draw on.
    let at = cam.project([5000.0, -100.0, 1500.0]);
    let under = scene.surface_in(&image, &cam, (at.0, at.1));
    assert_eq!(under.map(|u| u.id), Some(wall));
    assert_eq!(cursor_plane(m, &cam, (at.0, at.1), under), None);
    // Nothing under the cursor: the ground, Level 1's plane.
    let plane = cursor_plane(m, &cam, (3.0, 3.0), None).unwrap();
    assert_eq!((plane.on, plane.level, plane.offset), (one, one, 0.0));
    // Level 2's plane, and a face of nothing in the model (a linked one's) 200 above it: each over Level 2.
    let (on, up) = (|id, z| Surface { id, at: [1000.0, 1000.0, z], normal: [0.0, 0.0, 1.0] }, m.elevation(two));
    let plane = cursor_plane(m, &cam, (0.0, 0.0), Some(on(two, up))).unwrap();
    assert_eq!((plane.level, plane.offset), (two, 0.0));
    let plane = cursor_plane(m, &cam, (0.0, 0.0), Some(on(1, up + 200.0))).unwrap();
    assert_eq!((plane.on, plane.level, plane.offset), (1, two, 200.0));
    // A floor's bottom, seen from under it, is turned down.
    assert_eq!(cursor_plane(m, &cam, (0.0, 0.0), Some(Surface { normal: [0.0, 0.0, -1.0], ..on(floor, 300.0) })), None);
}

//! Cameras as a person frames a shot: the camera's cone and grips on the plan (Revit's Show Camera), its Properties as Revit's
//! Camera group and 3ds Max's Physical Camera rollouts, and in the camera view itself flying with the right button held and
//! W, A, S and D, Walk (Shift+W), the safe frame (Shift+F) and two-point perspective; every 3D view keeping where it is left.

use super::*;

/// A camera looking east along the box's middle, placed with the Camera tool, its view open.
fn placed() -> (Harness<'static, BimApp>, Id) {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Camera");
    click(&mut h, P2(-1000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    let id = ids(&h, "view3d")[0];
    (h, id)
}

fn cam(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::ViewCamera {
    match h.state().session.model.elements.get(id) {
        Some(Element::View3D(v)) => v.camera.unwrap(),
        _ => panic!("no camera view"),
    }
}

fn lens(h: &Harness<BimApp>, id: Id) -> f64 {
    match h.state().session.model.elements.get(id) {
        Some(Element::View3D(v)) => v.perspective.unwrap().lens,
        _ => panic!("no camera view"),
    }
}

/// The plan with the camera selected, as Show Camera leaves it.
fn shown_on_plan(h: &mut Harness<BimApp>, id: Id) -> bimcraft_core::render::Cone {
    topmost(h, "Level 1");
    h.state_mut().selection = vec![id];
    h.run();
    let m = &h.state().session.model;
    match m.elements.get(id) {
        Some(Element::View3D(v)) => bimcraft_core::render::camera_cone(v).unwrap(),
        _ => panic!("no camera view"),
    }
}

#[test]
fn show_camera_in_the_browser_selects_the_camera_so_the_plan_shows_its_cone() {
    let (mut h, id) = placed();
    topmost(&mut h, "Level 1");
    h.state_mut().selection.clear();
    h.run();
    // The browser's entry, right-clicked: Show Camera.
    let entry = h.get_all_by_label("Camera 1").min_by(|a, b| b.rect().top().total_cmp(&a.rect().top())).map(|n| n.rect().center()).unwrap();
    h.hover_at(entry);
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton { pos: entry, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
    }
    h.run();
    press(&mut h, "Show Camera");
    assert_eq!(h.state().selection, vec![id]);
}

#[test]
fn the_eye_s_grip_moves_the_camera_and_the_target_s_aims_it_each_one_undo_step() {
    let (mut h, id) = placed();
    let cone = shown_on_plan(&mut h, id);
    assert_eq!(cone.eye, P2(-1000.0, 3000.0));
    // The eye dragged south: it moves, the target stays.
    drag(&mut h, cone.eye, P2(-1000.0, 1000.0));
    let c = cam(&h, id);
    assert!(same_point(c.eye, [-1000.0, 1000.0, 1600.0]), "{:?}", c.eye);
    assert!(same_point(c.target, [4000.0, 3000.0, 1200.0]), "{:?}", c.target);
    // The target dragged north: the camera aims there.
    drag(&mut h, P2(4000.0, 3000.0), P2(4000.0, 5000.0));
    let c2 = cam(&h, id);
    assert!(same_point(c2.target, [4000.0, 5000.0, 1200.0]), "{:?}", c2.target);
    assert!(same_point(c2.eye, c.eye));
    // Each drag was one undo step.
    shortcut_undo(&mut h);
    assert!(same_point(cam(&h, id).target, [4000.0, 3000.0, 1200.0]));
    shortcut_undo(&mut h);
    assert!(same_point(cam(&h, id).eye, [-1000.0, 3000.0, 1600.0]));
}

fn shortcut_undo(h: &mut Harness<BimApp>) {
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
}

#[test]
fn the_far_clip_s_grip_sets_the_far_clip_and_a_side_s_the_field_of_view() {
    let (mut h, id) = placed();
    let cone = shown_on_plan(&mut h, id);
    // The far line's middle, dragged back toward the eye: Clip Manually on, the far plane where it was let go.
    drag(&mut h, cone.far, P2(2000.0, 3000.0));
    let c = cam(&h, id);
    assert!(c.physical.clip_on);
    let along = 3000.0 / (c.target_distance() / P2(-1000.0, 3000.0).dist(P2(4000.0, 3000.0)));
    assert!((c.physical.far - along).abs() < 30.0, "far {} vs {along}", c.physical.far);
    // A side dragged outward widens the field of view: a shorter focal length.
    let before = lens(&h, id);
    let cone = shown_on_plan(&mut h, id);
    let side = cone.sides[0];
    drag(&mut h, side, P2(side.0, side.1 + 1500.0));
    assert!(lens(&h, id) < before, "wider: {} < {before}", lens(&h, id));
}

#[test]
fn properties_show_revit_s_camera_group_and_3ds_max_s_rollouts_and_apply_as_typed() {
    let (mut h, id) = placed();
    for label in [
        "Projection Mode",
        "Eye Elevation",
        "Target Elevation",
        "Camera Position",
        "Targeted",
        "Target Distance",
        "Film/Sensor",
        "Focal Length",
        "Field of View",
        "Zoom",
        "Aperture",
        "Exposure Gain",
        "Lens Shift Vertical",
        "Auto Vertical Tilt Correction",
        "Clip Manually",
        "Lock Image Aspect",
    ] {
        assert!(h.query_by_label(label).is_some(), "Properties shows {label}");
    }
    assert!(h.query_by_label("Eye").is_none() && h.query_by_label("Target").is_none(), "no raw triples");
    // The field of view typed sets the focal length across the film.
    type_into(&mut h, "Field of View", "90");
    h.key_press(Key::Enter);
    h.run();
    assert!((lens(&h, id) - 18.0).abs() < 0.01, "{}", lens(&h, id));
    type_into(&mut h, "Target Elevation", "3000");
    h.key_press(Key::Enter);
    h.run();
    assert_eq!(cam(&h, id).target[2], 3000.0);
    pick(&mut h, Role::CheckBox, "Auto Vertical Tilt Correction");
    assert!(cam(&h, id).physical.auto_tilt);
}

#[test]
fn holding_the_right_button_flies_with_wasd_and_typing_wa_still_makes_a_wall_elsewhere() {
    let (mut h, id) = placed();
    assert_eq!(h.state().sheet, Sheet::View3D);
    let before = cam(&h, id);
    let middle = h.state().orbit.rect.center();
    h.hover_at(middle);
    h.run();
    h.event(Event::PointerButton { pos: middle, button: PointerButton::Secondary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    // W held for a few frames: forward, toward the target.
    h.event(Event::Key { key: Key::W, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.event(Event::Text("w".into()));
    for _ in 0..4 {
        h.step();
    }
    h.event(Event::Key { key: Key::W, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    // The pointer moved right with the button held: it turns right, about the eye.
    h.hover_at(middle + vec2(40.0, 0.0));
    h.step();
    h.event(Event::PointerButton { pos: middle + vec2(40.0, 0.0), button: PointerButton::Secondary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    let after = cam(&h, id);
    assert!(after.eye[0] > before.eye[0] + 100.0, "it flew forward: {:?} -> {:?}", before.eye, after.eye);
    let way = |c: &bimcraft_core::model::ViewCamera| (c.target[1] - c.eye[1]).atan2(c.target[0] - c.eye[0]);
    assert!(way(&after) < way(&before) - 0.05, "it turned right");
    assert!(h.state().tool == Tool::Modify, "the W typed while flying was no shortcut");
    assert!(h.query_by_label("Show Camera").is_none(), "letting go after a flight opens no menu");
    // One undo step for the flight.
    shortcut_undo(&mut h);
    assert!(same_point(cam(&h, id).eye, before.eye), "undone at once");
    // Not flying, WA is the Wall tool as ever.
    shortcut(&mut h, "WA");
    assert!(matches!(h.state().tool, Tool::Wall(_)), "{:?}", h.state().tool);
}

#[test]
fn walk_toggles_with_shift_w_and_the_safe_frame_with_shift_f() {
    let (mut h, _) = placed();
    assert!(h.state().orbit.safe_frame, "a camera view shows its frame");
    h.key_press_modifiers(Modifiers::SHIFT, Key::F);
    h.run();
    assert!(!h.state().orbit.safe_frame);
    h.key_press_modifiers(Modifiers::SHIFT, Key::W);
    h.run();
    assert!(h.state().orbit.walk);
    shortcut(&mut h, "WA");
    assert_eq!(h.state().tool, Tool::Modify, "walking, the letters are the walk's");
    key(&mut h, Key::Escape);
    assert!(!h.state().orbit.walk);
}

#[test]
fn every_3d_view_keeps_where_it_is_left_as_one_undo_step_a_gesture() {
    let mut h = app(BOX);
    press(&mut h, "Default 3D View");
    let undo = h.state().session.history().len();
    let rect = h.state().orbit.rect;
    drag_screen(&mut h, rect.center(), rect.center() + vec2(60.0, 0.0), PointerButton::Primary);
    h.run();
    let m = &h.state().session.model;
    let v = match m.default_3d().and_then(|id| m.elements.get(id)) {
        Some(Element::View3D(v)) => v.clone(),
        _ => panic!("the default 3D view is made"),
    };
    assert!(v.framing.is_some(), "where it was left is kept");
    assert!((v.azimuth - bimcraft_core::view3d::AZIMUTH).abs() > 1.0, "turned: {}", v.azimuth);
    assert_eq!(h.state().session.history().len(), undo + 1, "one undo step for the drag");
    // Rendered by the core, it is the view the person left.
    let r = h.state_mut().session.execute("view_orientation", &json!({"view": "{3D}"})).unwrap();
    assert_eq!(r["azimuth"].as_f64(), Some(v.azimuth));
}

#[test]
fn in_a_section_the_eye_and_the_target_are_raised_by_their_grips() {
    let mut h = app_at(&format!("{BOX}\nsection {{\"start\": [-3000, -1000], \"end\": [9000, -1000]}}"), 0.05);
    press(&mut h, "View");
    press(&mut h, "Camera");
    click(&mut h, P2(-1000.0, 3000.0));
    click(&mut h, P2(4000.0, 3000.0));
    let id = ids(&h, "view3d")[0];
    browse(&mut h, "Section 1");
    h.state_mut().selection = vec![id];
    h.run();
    // Along the section the eye stands 2000 mm in, at 1600; dragged up to 2600.
    drag(&mut h, P2(2000.0, 1600.0), P2(2000.0, 2600.0));
    assert!((cam(&h, id).eye[2] - 2600.0).abs() < 20.0, "{:?}", cam(&h, id).eye);
    // The target, 7000 mm in at 1200, down to 900.
    drag(&mut h, P2(7000.0, 1200.0), P2(7000.0, 900.0));
    assert!((cam(&h, id).target[2] - 900.0).abs() < 20.0, "{:?}", cam(&h, id).target);
    assert!(same_point([cam(&h, id).eye[0], cam(&h, id).eye[1], 0.0], [-1000.0, 3000.0, 0.0]), "only heights change there");
}

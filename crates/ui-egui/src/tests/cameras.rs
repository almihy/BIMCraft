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
    use egui_kittest::kittest::NodeT;
    let ids: Vec<_> = h.get_all_by_label("Camera 1").map(|n| n.accesskit_node().id()).collect();
    for id in ids {
        h.get_by(|n| n.id() == id).scroll_to_me();
        h.run();
        let entry = h.get_by(|n| n.id() == id).rect().center();
        h.hover_at(entry);
        h.run();
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos: entry, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        }
        h.run();
        if h.query_by_label("Show Camera").is_some() {
            break;
        }
        key(&mut h, Key::Escape);
    }
    press(&mut h, "Show Camera");
    assert_eq!(h.state().selection, vec![id]);
    assert!(h.query_by_label("Rotate").is_some(), "its Modify tab is up, as Revit's");
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
    h.get_by_role_and_label(Role::CheckBox, "Auto Vertical Tilt Correction").click_accesskit();
    h.run();
    assert!(cam(&h, id).physical.auto_tilt);
}

#[test]
fn holding_the_right_button_flies_with_wasd_and_typing_wa_still_makes_a_wall_elsewhere() {
    let (mut h, id) = placed();
    assert_eq!(h.state().sheet, Sheet::View3D);
    let before = cam(&h, id);
    let undo = h.state().session.history().len();
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
    h.event(Event::PointerButton {
        pos: middle + vec2(40.0, 0.0),
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    let after = cam(&h, id);
    assert!(after.eye[0] > before.eye[0] + 100.0, "it flew forward: {:?} -> {:?}", before.eye, after.eye);
    let way = |c: &bimcraft_core::model::ViewCamera| (c.target[1] - c.eye[1]).atan2(c.target[0] - c.eye[0]);
    assert!(way(&after) < way(&before) - 0.05, "it turned right");
    assert!(h.state().tool == Tool::Modify, "the W typed while flying was no shortcut");
    assert!(!egui::Popup::is_any_open(&h.ctx), "letting go after a flight opens no menu");
    // Navigation is no undo step, as Revit's.
    assert_eq!(h.state().session.history().len(), undo, "no undo step for the flight");
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
fn every_3d_view_keeps_where_it_is_left_and_navigating_makes_no_undo_step() {
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
    assert_eq!(h.state().session.history().len(), undo, "no undo step for the drag, as Revit's navigation");
    assert!(h.state().dirty(), "but the project is changed: the view is saved with it");
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

/// A key's press or release over the view: `repeat` for the presses a held key repeats.
fn key_event(h: &mut Harness<BimApp>, key: Key, pressed: bool, repeat: bool, modifiers: Modifiers) {
    h.event(Event::Key { key, physical_key: None, pressed, repeat, modifiers });
}

fn right_button(h: &mut Harness<BimApp>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
}

#[test]
fn shift_w_while_the_right_button_flies_is_the_flight_s_and_a_held_shift_w_toggles_walk_once() {
    let (mut h, _) = placed();
    let middle = h.state().orbit.rect.center();
    h.hover_at(middle);
    h.run();
    right_button(&mut h, middle, true);
    h.run();
    // Shift and W held while the right button flies: forward, faster; Walk stays off.
    key_event(&mut h, Key::W, true, false, Modifiers::SHIFT);
    h.step();
    key_event(&mut h, Key::W, true, true, Modifiers::SHIFT);
    h.step();
    key_event(&mut h, Key::W, false, false, Modifiers::SHIFT);
    h.step();
    assert!(!h.state().orbit.walk, "Shift+W in a flight is no Walk");
    right_button(&mut h, middle, false);
    h.run();
    // Not flying, Shift+W held down turns Walk on once: its repeats don't turn it off again.
    key_event(&mut h, Key::W, true, false, Modifiers::SHIFT);
    h.step();
    for _ in 0..3 {
        key_event(&mut h, Key::W, true, true, Modifiers::SHIFT);
        h.step();
    }
    key_event(&mut h, Key::W, false, false, Modifiers::SHIFT);
    h.run();
    assert!(h.state().orbit.walk, "one press, one toggle");
}

#[test]
fn walking_ctrl_s_ctrl_z_and_delete_still_work_and_ctrl_held_moves_nothing() {
    let (mut h, id) = placed();
    h.key_press_modifiers(Modifiers::SHIFT, Key::W);
    h.run();
    assert!(h.state().orbit.walk);
    // Ctrl+S saves: a project never saved asks where.
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    assert!(crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).is_some(), "Ctrl+S while walking saves");
    assert!(h.state().orbit.walk, "and the walk goes on");
    // A key held with Ctrl is a shortcut's, not a step.
    let before = cam(&h, id);
    h.event(Event::ModifiersChanged(Modifiers::COMMAND));
    key_event(&mut h, Key::A, true, false, Modifiers::COMMAND);
    for _ in 0..4 {
        h.step();
    }
    key_event(&mut h, Key::A, false, false, Modifiers::COMMAND);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
    assert!(same_point(cam(&h, id).eye, before.eye), "Ctrl+A moved the camera: {:?}", cam(&h, id).eye);
    // Delete deletes what is selected, and Ctrl+Z brings it back.
    let wall = ids(&h, "wall")[0];
    h.state_mut().selection = vec![wall];
    key(&mut h, Key::Delete);
    assert!(h.state().session.model.elements.get(wall).is_none(), "Delete while walking deletes");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert!(h.state().session.model.elements.get(wall).is_some(), "Ctrl+Z while walking undoes");
}

#[test]
fn walking_the_keys_typed_in_a_text_field_are_the_field_s_and_move_nothing() {
    let (mut h, id) = placed();
    h.key_press_modifiers(Modifiers::SHIFT, Key::W);
    h.run();
    assert!(h.state().orbit.walk);
    let before = cam(&h, id);
    h.get_by_label("Target Distance").focus();
    h.run();
    key_event(&mut h, Key::W, true, false, Modifiers::NONE);
    h.event(Event::Text("w".into()));
    for _ in 0..4 {
        h.step();
    }
    key_event(&mut h, Key::W, false, false, Modifiers::NONE);
    h.run();
    assert!(same_point(cam(&h, id).eye, before.eye), "typing in a field walked the camera: {:?}", cam(&h, id).eye);
}

/// What a camera view's physical settings are, as the model holds them.
fn physical(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::Physical {
    cam(h, id).physical
}

#[test]
fn with_a_lens_shift_a_section_box_s_grip_is_found_where_the_picture_draws_it() {
    let (mut h, id) = placed();
    // The safe frame off: the film is the view's width.
    h.key_press_modifiers(Modifiers::SHIFT, Key::F);
    h.run();
    let s = &mut h.state_mut().session;
    s.execute("section_box", &json!({"view": id, "min": [0.0, 0.0, 0.0], "max": [4000.0, 6000.0, 3000.0]})).unwrap();
    s.execute("camera", &json!({"view": id, "shift": [0.0, 30.0]})).unwrap();
    h.run();
    h.run();
    let (rect, ppp) = (h.state().orbit.rect, h.ctx.pixels_per_point());
    let bare = h.state().orbit.camera.unwrap();
    let seen = bare.physical(&physical(&h, id), bare.target, f64::from(bare.width));
    let b = bimcraft_core::graphics::Look::of(&h.state().session.model, id).section_box().unwrap();
    let on = |c: &bimcraft_core::view3d::Camera, p: [f64; 3]| {
        let (x, y, _) = c.project(p);
        rect.min + vec2(x as f32 / ppp, y as f32 / ppp)
    };
    let grips = bimcraft_core::view3d::box_grips(b);
    let (k, at) = grips
        .iter()
        .enumerate()
        .map(|(k, g)| (k, on(&seen, *g)))
        .find(|(k, at)| rect.shrink(20.0).contains(*at) && on(&bare, grips[*k]).distance(*at) > 40.0)
        .expect("a grip the shift moves, in the view");
    h.hover_at(at);
    h.run();
    assert_eq!(h.state().orbit.hot_grip, Some(k), "the grip under the pointer is the one drawn there");
}

#[test]
fn opening_a_steep_camera_view_changes_nothing() {
    let mut h = app(BOX);
    let r = h.state_mut().session.execute("camera", &json!({"eye": [2000.0, 3000.0, 30000.0], "target": [2050.0, 3000.0, 0.0]})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let (model, undo) = (Arc::clone(&h.state().session.model), h.state().session.history().len());
    h.state_mut().open_3d(Some(id));
    for _ in 0..4 {
        h.run();
    }
    assert!(Arc::ptr_eq(&model, &h.state().session.model), "opening a view writes nothing back: {:?}", cam(&h, id));
    assert_eq!(h.state().session.history().len(), undo, "and makes no undo step");
}

#[test]
fn a_flight_is_kept_with_the_view_and_no_undo_step_as_revit_s_navigation() {
    let (mut h, id) = placed();
    let undo = h.state().session.history().len();
    let before = cam(&h, id);
    let middle = h.state().orbit.rect.center();
    h.hover_at(middle);
    h.run();
    right_button(&mut h, middle, true);
    h.run();
    key_event(&mut h, Key::W, true, false, Modifiers::NONE);
    for _ in 0..4 {
        h.step();
    }
    key_event(&mut h, Key::W, false, false, Modifiers::NONE);
    h.step();
    right_button(&mut h, middle, false);
    h.run();
    let flown = cam(&h, id);
    assert!(!same_point(flown.eye, before.eye), "it flew");
    assert!(h.state().dirty(), "the project is changed");
    assert_eq!(h.state().session.history().len(), undo, "navigation is no undo step");
    // Undo takes back the last edit (the camera placed), not the flight.
    assert!(!egui::Popup::is_any_open(&h.ctx), "letting go after a flight opens no menu");
}

#[test]
fn a_right_click_without_a_flight_opens_the_view_s_menu() {
    let (mut h, _) = placed();
    let middle = h.state().orbit.rect.center();
    h.hover_at(middle);
    h.run();
    right_button(&mut h, middle, true);
    h.run();
    right_button(&mut h, middle, false);
    h.run();
    assert!(h.query_by_label("Zoom to Fit").is_some(), "the canvas's own menu opens");
}

#[test]
fn select_all_instances_in_a_camera_view_takes_only_what_that_view_shows() {
    let (mut h, id) = placed();
    let walls = ids(&h, "wall");
    assert!(walls.len() >= 2);
    h.state_mut().selection = vec![walls[0]];
    h.state_mut().act(Action::HideElement);
    h.run();
    assert!(!bimcraft_core::graphics::Look::of(&h.state().session.model, id).shows(walls[0]), "hidden in the camera view");
    h.state_mut().selection = vec![walls[1]];
    h.state_mut().act(Action::SelectAllInstances);
    h.run();
    assert!(!h.state().selection.contains(&walls[0]), "what the camera view hides isn't taken: {:?}", h.state().selection);
    assert!(h.state().selection.contains(&walls[1]));
}

#[test]
fn a_camera_shows_on_the_plan_only_while_its_view_is_selected() {
    let (mut h, id) = placed();
    topmost(&mut h, "Level 1");
    h.state_mut().selection.clear();
    h.run();
    // Not selected, it isn't drawn and a click where its eye is picks nothing.
    click(&mut h, P2(-1000.0, 3000.0));
    assert!(h.state().selection.is_empty(), "a hidden camera is not picked: {:?}", h.state().selection);
    let shown = h.state().canvas.drawing.shown_drawing().unwrap();
    assert!(!shown.owners.iter().flatten().any(|o| *o == id), "not drawn while not selected");
    // Selected (Show Camera), its mark is there and picks.
    h.state_mut().selection = vec![id];
    h.run();
    click(&mut h, P2(-1000.0, 3000.0));
    assert_eq!(h.state().selection, vec![id], "its mark picks while shown");
}

#[test]
fn the_field_of_view_spans_the_safe_frame_and_the_window_draws_what_render_makes() {
    let (mut h, id) = placed();
    h.state_mut().session.execute("camera", &json!({"view": id, "shift": [5.0, 8.0], "tilt": 0.2})).unwrap();
    h.run();
    h.run();
    assert!(h.state().orbit.safe_frame);
    let (rect, ppp) = (h.state().orbit.rect, h.ctx.pixels_per_point());
    let c = cam(&h, id);
    let frame = crate::fly::safe_frame(rect, c.physical.aspect());
    let seen = h.state().orbit.seen.expect("the camera the picture is drawn through");
    let px = |p: [f64; 3]| {
        let (x, y, _) = seen.project(p);
        (rect.min.x + x as f32 / ppp, rect.min.y + y as f32 / ppp)
    };
    // `render` of the view at its output size, and the window's picture, scaled from the frame: the same points.
    let v = match h.state().session.model.elements.get(id) {
        Some(Element::View3D(v)) => v.clone(),
        _ => panic!(),
    };
    let [ow, oh] = c.physical.output;
    let rendered = bimcraft_core::view3d::as_left(&v, ow, oh).unwrap();
    for p in [[0.0, 0.0, 0.0], [4000.0, 6000.0, 3000.0], [2000.0, 1000.0, 2500.0], c.target] {
        let (x, y) = px(p);
        let (fx, fy) = ((x - frame.min.x) / frame.width() * ow as f32, (y - frame.min.y) / frame.height() * oh as f32);
        let (rx, ry, _) = rendered.project(p);
        assert!((f64::from(fx) - rx).abs() < 2.0 && (f64::from(fy) - ry).abs() < 2.0, "{p:?}: window {fx} {fy}, render {rx} {ry}");
    }
    // Without shift or tilt, the field of view's edge at the target lands on the frame's edge.
    h.state_mut().session.execute("camera", &json!({"view": id, "shift": [0.0, 0.0], "tilt": 0.0})).unwrap();
    h.run();
    h.run();
    let seen = h.state().orbit.seen.unwrap();
    let c = cam(&h, id);
    let fov = bimcraft_core::view3d::field_of_view(lens(&h, id), &c.physical).to_radians();
    let d = c.target_distance();
    let way = [(c.target[0] - c.eye[0]) / d, (c.target[1] - c.eye[1]) / d];
    let right = [way[1], -way[0]];
    let edge = [c.target[0] + right[0] * d * (fov / 2.0).tan(), c.target[1] + right[1] * d * (fov / 2.0).tan(), c.target[2]];
    let (x, ..) = seen.project(edge);
    let x = rect.min.x + x as f32 / ppp;
    assert!((x - frame.max.x).abs() < 1.5, "the field of view's edge {x} is the frame's {}", frame.max.x);
}

#[test]
fn with_auto_vertical_tilt_the_vertical_tilt_correction_shows_as_set_for_it_and_there_is_no_horizontal() {
    use egui_kittest::kittest::NodeT;
    let (mut h, id) = placed();
    assert!(h.query_by_label("Tilt Correction Horizontal").is_none(), "only the vertical tilt correction is applied, so only it shows");
    h.get_by_role_and_label(Role::CheckBox, "Auto Vertical Tilt Correction").click_accesskit();
    h.run();
    assert!(physical(&h, id).auto_tilt);
    assert!(h.get_by_label("Tilt Correction Vertical").accesskit_node().is_disabled(), "greyed: Auto sets it");
}

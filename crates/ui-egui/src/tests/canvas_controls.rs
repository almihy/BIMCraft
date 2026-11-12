//! The selection's controls on the plan take the pointer before the plan does, as Revit's do: a click on a flip arrow, a
//! pin, a temporary dimension's number, a witness line's dot, Make Permanent, a padlock, EQ, an angle, a grip or the
//! navigation bar does what the control says and never also picks what lies under it, and a drag from one never moves the
//! selection or draws a box. Each is pressed and let go with the pointer where the control is drawn, a frame apart and
//! quickly, as a hand clicks.

use super::*;

/// The app with frames 50 ms apart, so a press and its release a frame later are a quick click.
fn quick(script: &str) -> Harness<'static, BimApp> {
    app_at(script, 0.05)
}

/// A hand's click at `at`: the pointer goes there, the button goes down, and a frame later comes up.
fn tap(h: &mut Harness<BimApp>, at: Pos2) {
    h.hover_at(at);
    h.run();
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run();
    // Let the click age, so the next one is no double click.
    h.run_steps(10);
}

/// Where the control named `label` is drawn.
fn spot(h: &Harness<BimApp>, label: &str) -> Pos2 {
    h.get_by_label(label).rect().center()
}

/// Selects `ids` as a click would.
fn select(h: &mut Harness<BimApp>, ids: &[Id]) {
    h.state_mut().selection = ids.to_vec();
    h.run();
}

const WITH_DOOR: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
door {"at": [2000, 0], "type": "Single 800 x 2100"}
window {"at": [5000, 0]}"#;

#[test]
fn a_click_on_a_flip_arrow_flips_and_keeps_the_selection_whatever_lies_under_the_arrow() {
    let mut h = quick(WITH_DOOR);
    let (door, window, south) = (ids(&h, "door")[0], ids(&h, "window")[0], ids(&h, "wall")[0]);
    let flips = |h: &Harness<BimApp>, id| (field(h, id, "flip_hand") == json!(true), field(h, id, "flip_facing") == json!(true));
    select(&mut h, &[door]);
    // The facing arrow stands beside the door, over its wall.
    let facing = spot(&h, "Flip facing");
    tap(&mut h, facing);
    assert_eq!(flips(&h, door), (false, true), "{}", h.state().status.text);
    assert_eq!(h.state().selection, [door], "the arrow took the click: the wall under it was not picked");
    let at = spot(&h, "Flip hand");
    tap(&mut h, at);
    assert_eq!(flips(&h, door), (true, true));
    assert_eq!(h.state().selection, [door]);
    select(&mut h, &[window]);
    let at = spot(&h, "Flip facing");
    tap(&mut h, at);
    assert_eq!(flips(&h, window), (false, true));
    assert_eq!(h.state().selection, [window]);
    select(&mut h, &[south]);
    let before = field(&h, south, "start");
    let at = spot(&h, "Flip exterior side");
    tap(&mut h, at);
    assert_eq!(field(&h, south, "end"), before, "the wall turned round");
    assert_eq!(h.state().selection, [south]);
}

#[test]
fn a_drag_from_a_flip_arrow_moves_nothing_and_draws_no_box() {
    let mut h = quick(WITH_DOOR);
    let door = ids(&h, "door")[0];
    select(&mut h, &[door]);
    let was = Arc::clone(&h.state().session.model);
    let at = spot(&h, "Flip facing");
    drag_screen(&mut h, at, at + vec2(60.0, 40.0), PointerButton::Primary);
    assert!(Arc::ptr_eq(&was, &h.state().session.model), "nothing moved: {:?}", h.state().session.history().last());
    assert_eq!(h.state().selection, [door], "no box was drawn from the arrow");
}

#[test]
fn a_click_on_a_pin_unpins_and_keeps_the_selection() {
    let mut h = quick("column {\"at\": [2000, 2000]}");
    let col = ids(&h, "column")[0];
    select(&mut h, &[col]);
    h.state_mut().session.execute("pin", &json!({"ids": [col]})).unwrap();
    h.run();
    let at = spot(&h, &format!("Unpin element {col}"));
    tap(&mut h, at);
    assert!(!h.state().session.model.is_pinned(col));
    assert_eq!(h.state().selection, [col]);
}

#[test]
fn a_click_on_a_temporary_dimensions_number_opens_it_to_type_and_keeps_the_selection() {
    let mut h = quick(BOX);
    let south = ids(&h, "wall")[0];
    select(&mut h, &[south]);
    let at = spot(&h, "Temporary dimension 8000");
    tap(&mut h, at);
    assert_eq!(h.state().selection, [south]);
    assert!(h.state().dim_edit.is_some(), "the number is a field to type in");
}

#[test]
fn a_click_on_a_walls_angle_opens_it_to_type_and_keeps_the_selection() {
    let mut h = quick(BOX);
    let south = ids(&h, "wall")[0];
    select(&mut h, &[south]);
    let at = h.get_by_label_contains("Turn the wall about its start").rect().center();
    tap(&mut h, at);
    assert_eq!(h.state().selection, [south]);
    assert!(h.state().angle_edit.is_some(), "the angle is a field to type in");
}

/// The box with a wall inside it, 2000 from the south wall and 4000 from the north one, selected.
fn with_inner_wall() -> (Harness<'static, BimApp>, Id) {
    let mut h = quick(&format!("{BOX}\nwall {{\"points\": [[1000, 2000], [5700, 2000]]}}"));
    let inner = ids(&h, "wall")[4];
    select(&mut h, &[inner]);
    (h, inner)
}

#[test]
fn a_click_on_make_permanent_or_a_witness_dot_does_that_and_keeps_the_selection() {
    let (mut h, inner) = with_inner_wall();
    let at = h.query_all_by_label_contains("Make this temporary dimension permanent").next().map(|n| n.rect().center()).unwrap();
    tap(&mut h, at);
    assert_eq!(ids(&h, "dimension").len(), 1, "{}", h.state().status.text);
    assert_eq!(h.state().selection, [inner]);
    let dot = h.query_all_by_label_contains("Witness line: click").next().map(|n| n.rect().center()).unwrap();
    tap(&mut h, dot);
    assert!(h.state().witnesses.contains_key(&inner), "the witness line went to the wall's next line");
    assert_eq!(h.state().selection, [inner]);
}

#[test]
fn a_click_on_a_dimensions_padlock_eq_or_grip_does_that_and_keeps_the_selection() {
    let mut h = quick(&format!(
        "{BOX}\ngrid {{\"start\": [1000, -2000], \"end\": [1000, 8000]}}\ndimension {{\"refs\": [[0, 3000], [1000, 3000], [8000, 3000]], \"at\": [4000, -1000]}}"
    ));
    let dim = ids(&h, "dimension")[0];
    select(&mut h, &[dim]);
    let at = spot(&h, "Lock segment 2");
    tap(&mut h, at);
    assert_eq!(field(&h, dim, "locked").as_array().map(Vec::len), Some(1), "{}", h.state().status.text);
    assert_eq!(h.state().selection, [dim]);
    let at = spot(&h, "Equal");
    tap(&mut h, at);
    assert_eq!(field(&h, dim, "equal"), json!(true), "{}", h.state().status.text);
    assert_eq!(h.state().selection, [dim]);
    let was = Arc::clone(&h.state().session.model);
    let at = spot(&h, &format!("Dimension line of element {dim}"));
    tap(&mut h, at);
    assert!(Arc::ptr_eq(&was, &h.state().session.model), "a click on a grip moves nothing");
    assert_eq!(h.state().selection, [dim]);
}

#[test]
fn a_click_on_a_walls_or_a_grids_end_grip_keeps_the_selection() {
    let mut h = quick(&format!("{BOX}\ngrid {{\"start\": [-2000, -1000], \"end\": [-2000, 7000]}}"));
    let (south, grid) = (ids(&h, "wall")[0], ids(&h, "grid")[0]);
    select(&mut h, &[south]);
    // At the corner, where the west wall's end is too.
    let was = Arc::clone(&h.state().session.model);
    let at = h.state().screen(P2(0.0, 0.0));
    tap(&mut h, at);
    assert_eq!(h.state().selection, [south], "the grip took the click: the west wall was not picked");
    assert!(Arc::ptr_eq(&was, &h.state().session.model));
    select(&mut h, &[grid]);
    let at = h.state().screen(P2(-2000.0, 7000.0));
    tap(&mut h, at);
    assert_eq!(h.state().selection, [grid]);
}

#[test]
fn a_click_on_a_detail_lines_grip_keeps_the_selection() {
    let mut h = quick("detail_line {\"centre\": [5000, 3000], \"radius\": 800}");
    let line = ids(&h, "line")[0];
    select(&mut h, &[line]);
    // A circle's grip is its centre, on empty floor.
    let at = h.state().screen(P2(5000.0, 3000.0));
    tap(&mut h, at);
    assert_eq!(h.state().selection, [line]);
}

#[test]
fn a_click_on_a_crop_regions_grip_keeps_the_selection() {
    let mut h = quick(BOX);
    let south = ids(&h, "wall")[0];
    h.state_mut().session.execute("crop", &json!({"view": 100000, "region": [[-2000, -1000], [10000, 7000]]})).unwrap();
    select(&mut h, &[south]);
    let at = h.state().screen(P2(10000.0, 3000.0));
    tap(&mut h, at);
    assert_eq!(h.state().selection, [south]);
}

#[test]
fn a_click_on_the_navigation_bar_keeps_the_selection() {
    let mut h = quick(BOX);
    let south = ids(&h, "wall")[0];
    select(&mut h, &[south]);
    let at = spot(&h, "Zoom In");
    tap(&mut h, at);
    assert_eq!(h.state().selection, [south]);
}

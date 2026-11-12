//! Revit's flip controls: the arrows beside a selected door, window or wall, and Space while one is selected, a door or window
//! is placed or a wall is drawn; a door or window put down faces the side of the wall the cursor is on.

use super::*;

const WITH_DOOR: &str = r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
door {"at": [2000, 0], "type": "Single 800 x 2100"}
window {"at": [5000, 0]}"#;

fn flips(h: &Harness<BimApp>, id: Id) -> (bool, bool) {
    (field(h, id, "flip_hand") == json!(true), field(h, id, "flip_facing") == json!(true))
}

/// Click the arrow named `label`, where the harness draws it.
fn arrow(h: &mut Harness<BimApp>, label: &str) {
    let at = h.get_by_label(label).rect().center();
    click_at(h, at);
}

#[test]
fn a_selected_door_shows_two_arrows_and_a_click_on_one_flips_what_it_says() {
    let mut h = app(WITH_DOOR);
    let door = ids(&h, "door")[0];
    assert!(h.query_by_label("Flip hand").is_none(), "nothing selected, no arrows");
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    arrow(&mut h, "Flip facing");
    assert_eq!(flips(&h, door), (false, true));
    assert_eq!(h.state().selection, [door], "the arrow's click is no pick");
    arrow(&mut h, "Flip hand");
    assert_eq!(flips(&h, door), (true, true));
    arrow(&mut h, "Flip hand");
    arrow(&mut h, "Flip facing");
    assert_eq!(flips(&h, door), (false, false));
    // Nothing else moved.
    assert_eq!(field(&h, door, "offset"), json!(2000.0));
}

#[test]
fn a_selected_window_has_both_arrows_as_a_door_and_a_wall_has_one_on_its_exterior_side() {
    let mut h = app(WITH_DOOR);
    let window = ids(&h, "window")[0];
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(h.state().selection, [window]);
    arrow(&mut h, "Flip facing");
    assert_eq!(flips(&h, window), (false, true));
    arrow(&mut h, "Flip hand");
    assert_eq!(flips(&h, window), (true, true));
    assert_eq!(h.state().selection, [window], "the arrows' clicks are no picks");
    // A wall: its arrow turns it about its location line.
    let south = ids(&h, "wall")[0];
    click(&mut h, P2(7000.0, 0.0));
    assert_eq!(h.state().selection, [south]);
    assert!(h.query_by_label("Flip hand").is_none(), "a wall has the one arrow");
    let before = field(&h, south, "start");
    arrow(&mut h, "Flip exterior side");
    assert_ne!(field(&h, south, "start"), before);
    assert_eq!(field(&h, south, "end"), before);
}

/// How far, on the screen, `at` is from the model point `from` in the direction of `to` from it.
fn side_of(h: &Harness<BimApp>, at: Pos2, from: P2, to: P2) -> f32 {
    let (a, b) = (h.state().screen(from), h.state().screen(to));
    (at - a).dot((b - a).normalized())
}

#[test]
fn a_walls_arrow_stands_on_its_exterior_side_and_a_doors_hand_arrow_on_the_side_it_does_not_open_to() {
    let mut h = app(WITH_DOOR);
    // The south wall runs east: its exterior, its left, is the north.
    let south = ids(&h, "wall")[0];
    click(&mut h, P2(7000.0, 0.0));
    assert_eq!(h.state().selection, [south]);
    assert_eq!(field(&h, south, "start"), json!([0.0, 0.0]));
    let at = h.get_by_label("Flip exterior side").rect().center();
    assert!(side_of(&h, at, P2(4000.0, 0.0), P2(4000.0, 500.0)) > 10.0, "on the north side, the exterior");
    // Turned round, its exterior is the south, and the arrow goes there.
    click_at(&mut h, at);
    let at = h.get_by_label("Flip exterior side").rect().center();
    assert!(side_of(&h, at, P2(4000.0, 0.0), P2(4000.0, 500.0)) < -10.0, "on the south side now");
    // A door opens north to start with: its hand arrow is on the south side; with its facing flipped, the north.
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    let hand = h.get_by_label("Flip hand").rect().center();
    assert!(side_of(&h, hand, P2(2000.0, 0.0), P2(2000.0, 500.0)) < -10.0, "south of the wall, away from the swing");
    arrow(&mut h, "Flip facing");
    let hand = h.get_by_label("Flip hand").rect().center();
    assert!(side_of(&h, hand, P2(2000.0, 0.0), P2(2000.0, 500.0)) > 10.0, "north now: the door swings south");
}

/// The wheel turned `times` notches out over the middle of the box.
fn zoom_out(h: &mut Harness<BimApp>, times: usize) {
    let at = h.state().screen(P2(4000.0, 3000.0));
    h.hover_at(at);
    for _ in 0..times {
        h.event(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -120.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
}

#[test]
fn a_pinned_walls_flip_arrow_never_lies_over_its_pin_however_far_out_the_view_is_zoomed() {
    let mut h = app(BOX);
    let south = ids(&h, "wall")[0];
    for notches in [0, 3, 3, 3] {
        zoom_out(&mut h, notches);
        let length = (h.state().screen(P2(8000.0, 0.0)) - h.state().screen(P2(0.0, 0.0))).length();
        click(&mut h, P2(4000.0, 0.0));
        assert_eq!(h.state().selection, [south], "the wall is {length} px long");
        h.state_mut().session.execute("pin", &json!({"ids": [south]})).unwrap();
        h.run();
        let pin = h.get_by_label(&format!("Unpin element {south}")).rect();
        let flip = h.get_by_label("Flip exterior side").rect();
        assert!(!pin.intersects(flip), "the wall is {length} px long: pin {pin:?}, arrow {flip:?}");
        // Each takes its own click: the arrow flips the wall (it stays pinned), the pin unpins it.
        let before = field(&h, south, "start");
        click_at(&mut h, flip.center());
        assert_ne!(field(&h, south, "start"), before, "the arrow's click flipped the wall ({length} px long)");
        assert!(h.state().session.model.is_pinned(south), "a flip is no unpin");
        let pin = h.get_by_label(&format!("Unpin element {south}")).rect().center();
        click_at(&mut h, pin);
        assert!(!h.state().session.model.is_pinned(south), "the pin's click unpinned it ({length} px long)");
    }
}

#[test]
fn a_pinned_door_or_wall_still_flips() {
    let mut h = app(WITH_DOOR);
    let (door, south) = (ids(&h, "door")[0], ids(&h, "wall")[0]);
    h.state_mut().session.execute("pin", &json!({"ids": [door, south]})).unwrap();
    click(&mut h, P2(2000.0, 0.0));
    assert_eq!(h.state().selection, [door]);
    key(&mut h, Key::Space);
    assert_eq!(flips(&h, door), (false, true), "Revit lets a pinned door be flipped");
    arrow(&mut h, "Flip hand");
    assert_eq!(flips(&h, door), (true, true));
    click(&mut h, P2(7000.0, 0.0));
    let before = field(&h, south, "start");
    arrow(&mut h, "Flip exterior side");
    assert_ne!(field(&h, south, "start"), before, "a pinned wall flips about its location line, which stays");
}

#[test]
fn no_arrows_for_a_selection_of_several_or_outside_modify() {
    let mut h = app(WITH_DOOR);
    click(&mut h, P2(2000.0, 0.0));
    click_with(&mut h, P2(5000.0, 0.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    assert!(h.query_by_label("Flip facing").is_none());
}

#[test]
fn space_steps_a_selected_door_through_its_four_ways() {
    let mut h = app(WITH_DOOR);
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    let mut seen = vec![flips(&h, door)];
    for _ in 0..4 {
        key(&mut h, Key::Space);
        seen.push(flips(&h, door));
    }
    assert_eq!(seen, [(false, false), (false, true), (true, true), (true, false), (false, false)]);
}

#[test]
fn a_door_put_down_opens_to_the_side_of_the_wall_the_cursor_is_on() {
    let mut h = app(BOX);
    shortcut(&mut h, "DR");
    assert!(matches!(h.state().tool, Tool::Door));
    // The south wall runs east: its left, north, is the way a door opens by default.
    click(&mut h, P2(2000.0, 40.0));
    click(&mut h, P2(4000.0, -40.0));
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 2);
    assert_eq!((flips(&h, doors[0]), flips(&h, doors[1])), ((false, false), (false, true)));
}

#[test]
fn space_while_placing_a_door_flips_its_hand_the_next_door_keeps_it_and_the_cursors_side_is_its_facing() {
    let mut h = app(BOX);
    shortcut(&mut h, "DR");
    key(&mut h, Key::Space);
    // The cursor north of the south wall (its left): facing as placed; Space: the hand.
    click(&mut h, P2(2000.0, 40.0));
    // The next door keeps the hand; the cursor on the south side turns the facing.
    click(&mut h, P2(4000.0, -40.0));
    // Space again: the hand back.
    key(&mut h, Key::Space);
    click(&mut h, P2(6000.0, 40.0));
    let got: Vec<(bool, bool)> = ids(&h, "door").iter().map(|d| flips(&h, *d)).collect();
    assert_eq!(got, [(true, false), (true, true), (false, false)]);
}

#[test]
fn a_window_being_placed_faces_the_cursors_side_of_the_wall_and_space_flips_its_hand_for_the_next_ones_too() {
    let mut h = app(BOX);
    shortcut(&mut h, "WN");
    // The cursor moved across the wall and back: the side it ends on is the facing.
    for (y, x) in [(40.0, 1500.0), (-40.0, 3000.0)] {
        h.hover_at(h.state().screen(P2(x, -y)));
        h.run();
        h.hover_at(h.state().screen(P2(x, y)));
        h.run();
        click(&mut h, P2(x, y));
    }
    key(&mut h, Key::Space);
    click(&mut h, P2(4500.0, 40.0));
    click(&mut h, P2(6000.0, -40.0));
    let got: Vec<(bool, bool)> = ids(&h, "window").iter().map(|w| flips(&h, *w)).collect();
    assert_eq!(got, [(false, false), (false, true), (true, false), (true, true)]);
    // Leaving the tool forgets the hand.
    key(&mut h, Key::Escape);
    shortcut(&mut h, "DR");
    key(&mut h, Key::Space);
    key(&mut h, Key::Escape);
    shortcut(&mut h, "DR");
    click(&mut h, P2(7200.0, 40.0));
    assert_eq!(flips(&h, ids(&h, "door")[0]), (false, false), "Esc ended the flip");
}

#[test]
fn space_while_drawing_a_wall_puts_its_exterior_side_on_the_other_side() {
    let mut h = app(BOX);
    shortcut(&mut h, "WA");
    assert!(matches!(h.state().tool, Tool::Wall(_)));
    key(&mut h, Key::Space);
    click(&mut h, P2(1000.0, 2000.0));
    click(&mut h, P2(5000.0, 2000.0));
    let made = walls(&h).into_iter().find(|(a, _)| a.1 == 2000.0).unwrap();
    assert_eq!(made, (P2(5000.0, 2000.0), P2(1000.0, 2000.0)), "drawn east, built west: its exterior on the right of the drawing");
    // The flip stays on for the whole chain, until Space again or the tool ends. Enter ends the chain and the tool goes on:
    // Space again turns it off, and the next is as drawn.
    key(&mut h, Key::Enter);
    key(&mut h, Key::Space);
    click(&mut h, P2(1000.0, 4000.0));
    click(&mut h, P2(5000.0, 4000.0));
    let made = walls(&h).into_iter().find(|(a, _)| a.1 == 4000.0).unwrap();
    assert_eq!(made, (P2(1000.0, 4000.0), P2(5000.0, 4000.0)));
}

#[test]
fn space_steps_a_selected_window_as_a_door_and_each_selected_opening_from_its_own_state() {
    let mut h = app(WITH_DOOR);
    let (door, window) = (ids(&h, "door")[0], ids(&h, "window")[0]);
    // A window alone: the four ways, as a door's.
    click(&mut h, P2(5000.0, 0.0));
    let mut seen = Vec::new();
    for _ in 0..4 {
        key(&mut h, Key::Space);
        seen.push(flips(&h, window));
    }
    assert_eq!(seen, [(false, true), (true, true), (true, false), (false, false)]);
    // The door ahead of the window by a step: each goes on from its own state.
    click(&mut h, P2(2000.0, 0.0));
    key(&mut h, Key::Space);
    assert_eq!(flips(&h, door), (false, true));
    click_with(&mut h, P2(5000.0, 0.0), Modifiers::COMMAND);
    assert_eq!(h.state().selection.len(), 2);
    key(&mut h, Key::Space);
    assert_eq!((flips(&h, door), flips(&h, window)), ((true, true), (false, true)));
    key(&mut h, Key::Space);
    assert_eq!((flips(&h, door), flips(&h, window)), ((true, false), (true, true)));
}

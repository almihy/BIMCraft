//! Revit's flip controls: the arrows beside a selected door, window or wall, and Space while one is selected, a door or window
//! is placed or a wall is drawn; a door put down opens to the side of the wall the cursor is on.

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
fn a_selected_window_has_the_arrows_too_and_a_wall_has_one_on_its_exterior_side() {
    let mut h = app(WITH_DOOR);
    let window = ids(&h, "window")[0];
    click(&mut h, P2(5000.0, 0.0));
    assert_eq!(h.state().selection, [window]);
    arrow(&mut h, "Flip facing");
    assert_eq!(flips(&h, window), (false, true));
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
fn space_while_placing_a_door_flips_it_before_it_is_put_down_and_only_that_one() {
    let mut h = app(BOX);
    shortcut(&mut h, "DR");
    key(&mut h, Key::Space);
    click(&mut h, P2(2000.0, 40.0));
    assert_eq!(flips(&h, ids(&h, "door")[0]), (false, true), "the facing, from the cursor's side");
    // The next is as the cursor says; two Spaces take the hand too.
    click(&mut h, P2(3000.0, 40.0));
    key(&mut h, Key::Space);
    key(&mut h, Key::Space);
    click(&mut h, P2(4000.0, 40.0));
    let doors = ids(&h, "door");
    assert_eq!(doors.len(), 3);
    assert_eq!((flips(&h, doors[1]), flips(&h, doors[2])), ((false, false), (true, true)));
}

#[test]
fn space_while_placing_a_window_flips_its_facing_and_leaving_the_tool_forgets_it() {
    let mut h = app(BOX);
    shortcut(&mut h, "WN");
    key(&mut h, Key::Space);
    key(&mut h, Key::Escape);
    shortcut(&mut h, "WN");
    click(&mut h, P2(2000.0, 40.0));
    assert_eq!(flips(&h, ids(&h, "window")[0]), (false, false), "Esc ended the flip");
    key(&mut h, Key::Space);
    click(&mut h, P2(4000.0, 40.0));
    assert_eq!(flips(&h, ids(&h, "window")[1]), (false, true));
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
    // Enter ends the chain; Space again, and the next is as drawn.
    key(&mut h, Key::Enter);
    key(&mut h, Key::Space);
    click(&mut h, P2(1000.0, 4000.0));
    click(&mut h, P2(5000.0, 4000.0));
    let made = walls(&h).into_iter().find(|(a, _)| a.1 == 4000.0).unwrap();
    assert_eq!(made, (P2(1000.0, 4000.0), P2(5000.0, 4000.0)));
}

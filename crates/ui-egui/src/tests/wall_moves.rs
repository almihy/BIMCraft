//! Walls dragged as Revit's: one joined at an end to a wall that is not dragged goes only square to itself, and Shift frees
//! it; one joined to nothing goes anywhere, and Shift holds it square to itself.
use super::*;

/// Two walls: east from the origin to (4000, 0), then north from there. The first is joined at its end.
const ELL: &str = r#"wall {"points": [[0,0],[4000,0],[4000,3000]]}"#;

/// A drag with Shift held from the press to the release, as a hand holds it.
fn shift_drag(h: &mut Harness<BimApp>, from: P2, to: P2) {
    let (a, b) = (h.state().screen(from), h.state().screen(to));
    let held = Modifiers::SHIFT;
    h.event(Event::ModifiersChanged(held));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: held });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: held });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

#[test]
fn a_joined_wall_drags_square_to_itself_and_shift_frees_it() {
    let mut h = app(ELL);
    click(&mut h, P2(2000.0, 0.0));
    // Dragged down and to the right, it goes down only: the corner slides down its neighbour's line.
    drag(&mut h, P2(2000.0, 0.0), P2(2600.0, -1000.0));
    assert_eq!(walls(&h)[0], (P2(0.0, -1000.0), P2(4000.0, -1000.0)));
    assert_eq!(walls(&h)[1], (P2(4000.0, -1000.0), P2(4000.0, 3000.0)), "its neighbour stretches to stay joined");
    // With Shift it goes where the cursor does; its joined end still meets its neighbour, on that wall's line.
    shift_drag(&mut h, P2(2000.0, -1000.0), P2(2600.0, -1000.0));
    assert_eq!(walls(&h)[0].0, P2(600.0, -1000.0), "its free end went along with it");
}

#[test]
fn a_wall_joined_to_nothing_drags_anywhere_and_shift_holds_it_square() {
    let mut h = app(r#"wall {"points": [[0,0],[4000,0]]}"#);
    click(&mut h, P2(2000.0, 0.0));
    drag(&mut h, P2(2000.0, 0.0), P2(2600.0, -1000.0));
    assert_eq!(walls(&h)[0], (P2(600.0, -1000.0), P2(4600.0, -1000.0)));
    shift_drag(&mut h, P2(2600.0, -1000.0), P2(3000.0, -1500.0));
    assert_eq!(walls(&h)[0], (P2(600.0, -1500.0), P2(4600.0, -1500.0)), "square to itself only");
}

#[test]
fn the_move_tool_has_disjoin_which_leaves_the_neighbour_as_it_is() {
    let mut h = app(ELL);
    click(&mut h, P2(2000.0, 0.0));
    topmost(&mut h, "Modify");
    press(&mut h, "Move");
    press(&mut h, "Disjoin");
    click(&mut h, P2(2000.0, 0.0));
    click(&mut h, P2(2000.0, -1000.0));
    assert_eq!(walls(&h)[0], (P2(0.0, -1000.0), P2(4000.0, -1000.0)));
    assert_eq!(walls(&h)[1], (P2(4000.0, 0.0), P2(4000.0, 3000.0)), "left where it was");
}

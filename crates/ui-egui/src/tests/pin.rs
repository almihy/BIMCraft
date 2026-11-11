//! Pin and Unpin, dragging with snaps, Ctrl and Shift, and the grips on grids and dimensions, driven with real clicks and drags.

use super::*;

/// A drag on the plan between model points holding `mods` (Ctrl: copy; Shift: square), in steps, as a hand makes it.
fn drag_holding(h: &mut Harness<BimApp>, from: P2, to: P2, mods: Modifiers) {
    let (a, b) = (h.state().screen(from), h.state().screen(to));
    h.event(Event::ModifiersChanged(mods));
    h.hover_at(a);
    h.run();
    h.event(Event::PointerButton { pos: a, button: PointerButton::Primary, pressed: true, modifiers: mods });
    h.run();
    for t in [0.25, 0.5, 0.75, 1.0] {
        h.hover_at(a + (b - a) * t);
        h.run();
    }
    h.event(Event::PointerButton { pos: b, button: PointerButton::Primary, pressed: false, modifiers: mods });
    h.run();
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

const ONE: &str = "column {\"at\": [2000, 2000]}";
const TWO: &str = "column {\"at\": [2000, 2000]}
column {\"at\": [6000, 3000]}";

fn at(h: &Harness<BimApp>, id: Id) -> P2 {
    let v = field(h, id, "at");
    P2(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}

#[test]
fn a_dragged_element_snaps_as_the_move_tool_does() {
    let mut h = app(&format!(
        "{BOX}
{TWO}"
    ));
    let (col, other) = (ids(&h, "column")[0], ids(&h, "column")[1]);
    // Near the south wall's middle (4000, 0), a few pixels off it: the middle, to the millimetre, not the nearest 10.
    drag(&mut h, P2(2000.0, 2000.0), P2(3990.0, 12.0));
    assert_eq!(at(&h, col), P2(4000.0, 0.0), "{:?}", h.state().status);
    // Far from anything it keeps to 10 mm, as before.
    drag(&mut h, P2(6000.0, 3000.0), P2(7003.0, 4103.0));
    assert_eq!(at(&h, other), P2(7000.0, 4100.0));
}

#[test]
fn a_dragged_element_pressed_a_few_pixels_off_its_centre_lands_exactly_on_the_snap() {
    let mut h = app(&format!(
        "{BOX}
{ONE}"
    ));
    let col = ids(&h, "column")[0];
    // Pressed 3 px beside the centre: the grab point is the centre's snap, so the centre goes onto the wall's middle exactly.
    let a = h.state().screen(P2(2000.0, 2000.0)) + vec2(3.0, 2.0);
    let b = h.state().screen(P2(4000.0, 0.0)) + vec2(2.0, -3.0);
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert_eq!(at(&h, col), P2(4000.0, 0.0), "{:?}", h.state().status);
}

#[test]
fn a_dragged_element_does_not_snap_to_its_own_points() {
    let mut h = app(ONE);
    let col = ids(&h, "column")[0];
    // Grabbed off its centre and moved a little: it moves by the hand's distance, not onto its own centre.
    drag(&mut h, P2(2100.0, 2100.0), P2(2600.0, 2100.0));
    assert_eq!(at(&h, col), P2(2500.0, 2000.0));
}

#[test]
fn ctrl_drag_copies_and_selects_the_copy() {
    let mut h = app(ONE);
    let col = ids(&h, "column")[0];
    drag_holding(&mut h, P2(2000.0, 2000.0), P2(5000.0, 2000.0), Modifiers::COMMAND);
    let cols = ids(&h, "column");
    assert_eq!(cols.len(), 2, "{:?}", h.state().status);
    assert_eq!(at(&h, col), P2(2000.0, 2000.0), "the original stays");
    let copy = cols.into_iter().find(|c| *c != col).unwrap();
    assert_eq!(at(&h, copy), P2(5000.0, 2000.0));
    assert_eq!(h.state().selection, [copy], "the copy is selected");
    assert_eq!(h.state().session.history().last(), Some(&"Copy"));
}

#[test]
fn shift_drag_keeps_the_move_square() {
    let mut h = app(ONE);
    let col = ids(&h, "column")[0];
    drag_holding(&mut h, P2(2000.0, 2000.0), P2(5000.0, 2700.0), Modifiers::SHIFT);
    assert_eq!(at(&h, col), P2(5000.0, 2000.0), "along x only: {:?}", h.state().status);
    drag_holding(&mut h, P2(5000.0, 2000.0), P2(5600.0, 4000.0), Modifiers::SHIFT);
    assert_eq!(at(&h, col), P2(5000.0, 4000.0), "along y only");
}

#[test]
fn a_pinned_element_refuses_a_drag_and_the_tools_and_says_so() {
    let mut h = app(ONE);
    let col = ids(&h, "column")[0];
    click(&mut h, P2(2000.0, 2000.0));
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(col), "{:?}", h.state().status);
    // A drag moves nothing and the status bar says it is pinned.
    let before = Arc::clone(&h.state().session.model);
    drag(&mut h, P2(2000.0, 2000.0), P2(3000.0, 2000.0));
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "no command ran");
    assert!(h.state().status.error && h.state().status.text.contains("pinned"), "{:?}", h.state().status);
    assert_eq!(at(&h, col), P2(2000.0, 2000.0));
    // Move and Rotate refuse to begin.
    for letters in ["MV", "RO"] {
        h.state_mut().status = crate::Status::default();
        shortcut(&mut h, letters);
        assert_eq!(h.state().tool, Tool::Modify, "{letters}");
        assert!(h.state().status.text.contains("pinned"), "{letters}: {:?}", h.state().status);
    }
    // Copy is no move: it goes on.
    shortcut(&mut h, "CO");
    assert!(matches!(h.state().tool, Tool::Copy(_)));
    key(&mut h, Key::Escape);
    // The agent's command is refused as the window's drag is, in Revit's words, and says how to free it.
    let e = h.state_mut().session.execute("move", &json!({"ids": [col], "by": [100, 0]})).unwrap_err();
    assert!(e.contains("Can't move pinned element.") && e.contains("`unpin`"), "{e}");
    assert_eq!(at(&h, col), P2(2000.0, 2000.0));
}

#[test]
fn a_pinned_element_is_not_turned_and_the_status_says_rotate() {
    let mut h = app(ONE);
    click(&mut h, P2(2000.0, 2000.0));
    shortcut(&mut h, "PN");
    h.state_mut().status = crate::Status::default();
    shortcut(&mut h, "RO");
    assert!(h.state().status.text.contains("Can't rotate pinned element."), "{:?}", h.state().status);
}

#[test]
fn delete_takes_what_is_not_pinned_and_says_what_it_left() {
    let mut h = app(TWO);
    let (a, b) = (ids(&h, "column")[0], ids(&h, "column")[1]);
    h.state_mut().session.execute("pin", &json!({"ids": [a]})).unwrap();
    h.state_mut().selection = vec![a, b];
    h.run();
    key(&mut h, Key::Delete);
    assert_eq!(ids(&h, "column"), [a], "the free column went, the pinned one stayed");
    let status = &h.state().status;
    assert!(
        status.error && status.text.contains("Pinned objects were not deleted. To delete them, unpin them before using Delete."),
        "{status:?}"
    );
    // Alone, a pinned element is refused whole, and Unpin then lets it go.
    h.state_mut().selection = vec![a];
    h.run();
    key(&mut h, Key::Delete);
    assert_eq!(ids(&h, "column"), [a]);
    assert!(h.state().status.text.contains("Pinned objects were not deleted"), "{:?}", h.state().status);
    shortcut(&mut h, "UP");
    key(&mut h, Key::Delete);
    assert!(ids(&h, "column").is_empty());
}

#[test]
fn the_pin_on_a_selected_element_unpins_it_and_the_drag_then_moves_it() {
    let mut h = app(ONE);
    let col = ids(&h, "column")[0];
    click(&mut h, P2(2000.0, 2000.0));
    press(&mut h, "Pin");
    assert!(h.state().session.model.is_pinned(col), "{:?} {:?}", h.state().status, h.state().selection);
    h.run();
    press(&mut h, &format!("Unpin element {col}"));
    assert!(!h.state().session.model.is_pinned(col), "the pin icon unpinned it");
    drag(&mut h, P2(2000.0, 2000.0), P2(3000.0, 2000.0));
    assert_eq!(at(&h, col), P2(3000.0, 2000.0));
    // Pin and Unpin on the Modify panel, and by their shortcuts.
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(col));
    shortcut(&mut h, "UP");
    assert!(!h.state().session.model.is_pinned(col));
    press(&mut h, "Pin");
    press(&mut h, "Unpin");
    assert!(!h.state().session.model.is_pinned(col));
}

#[test]
fn a_pin_shows_only_while_the_element_is_selected() {
    let mut h = app(ONE);
    let col = ids(&h, "column")[0];
    h.state_mut().session.execute("pin", &json!({"ids": [col]})).unwrap();
    h.run();
    assert!(h.query_by_label(&format!("Unpin element {col}")).is_none(), "nothing selected, no pin icon");
    click(&mut h, P2(2000.0, 2000.0));
    assert!(h.query_by_label(&format!("Unpin element {col}")).is_some());
}

#[test]
fn a_grids_end_is_a_grip_that_lengthens_it() {
    let mut h = app(&format!("{BOX}\ngrid {{\"start\": [-2000, -1000], \"end\": [-2000, 7000]}}"));
    let grid = ids(&h, "grid")[0];
    click(&mut h, P2(-2000.0, 3000.0));
    assert_eq!(h.state().selection, [grid]);
    drag(&mut h, P2(-2000.0, 7000.0), P2(-2000.0, 9000.0));
    let g = &h.state().session.model;
    let Some(Element::Grid(g)) = g.elements.get(grid) else { panic!("a grid") };
    assert_eq!(g.start, P2(-2000.0, -1000.0), "the other end stays");
    assert!(g.end.0 == -2000.0 && (g.end.1 - 9000.0).abs() <= 20.0, "{:?}", g.end);
    // The start end too.
    drag(&mut h, P2(-2000.0, -1000.0), P2(-2000.0, -2500.0));
    let Some(Element::Grid(g)) = h.state().session.model.elements.get(grid) else { panic!("a grid") };
    assert!((g.start.1 + 2500.0).abs() <= 20.0, "{:?}", g.start);
    // Pinned, it stays.
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(grid), "{:?}", h.state().status);
    let was = Arc::clone(&h.state().session.model);
    drag(&mut h, P2(-2000.0, 9000.0), P2(-2000.0, 9500.0));
    assert!(Arc::ptr_eq(&was, &h.state().session.model), "a pinned grid's end does not move");
    assert!(h.state().status.text.contains("pinned"));
}

#[test]
fn a_dimensions_line_and_text_have_grips() {
    let mut h = app(&format!("{BOX}\ndimension {{\"refs\": [[0, 3000], [8000, 3000]], \"at\": [0, 3000]}}"));
    let dim = ids(&h, "dimension")[0];
    click(&mut h, P2(4000.0, 3000.0));
    assert_eq!(h.state().selection, [dim]);
    // The line's grip drags the line square to itself: the offset changes, along the line it does not.
    let line = h.get_by_label(&format!("Dimension line of element {dim}")).rect().center();
    drag_screen(&mut h, line, line + vec2(0.0, 40.0), PointerButton::Primary);
    let now = field(&h, dim, "at");
    assert_eq!(now[0], json!(0.0), "{now}");
    assert!(now[1].as_f64().unwrap() < 3000.0, "dragged down the screen, south: {now}");
    // The number's grip moves the number.
    let text = h.get_by_label(&format!("Dimension text 1 of element {dim}")).rect().center();
    drag_screen(&mut h, text, text + vec2(40.0, 0.0), PointerButton::Primary);
    let shifts = field(&h, dim, "text_shifts");
    assert!(shifts[0]["along"].as_f64().unwrap() > 0.0, "{shifts}");
    assert_eq!(h.state().session.history().last(), Some(&"Move Dimension Text"));
}

#[test]
fn a_pinned_column_is_not_moved_by_a_typed_dimension_or_the_arrow_keys() {
    let mut h = app(&format!("{BOX}\ncolumn {{\"at\": [3000, 2000]}}"));
    let col = ids(&h, "column")[0];
    click(&mut h, P2(3000.0, 2000.0));
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(col), "{:?}", h.state().status);
    let before = Arc::clone(&h.state().session.model);
    h.state_mut().status = crate::Status::default();
    retype_dimension(&mut h, "3000", "1000");
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "a typed dimension moved a pinned column");
    assert!(h.state().status.error && h.state().status.text.contains("pinned"), "{:?}", h.state().status);
    h.state_mut().status = crate::Status::default();
    key(&mut h, Key::ArrowRight);
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "an arrow key moved a pinned column");
    assert!(h.state().status.text.contains("pinned"), "{:?}", h.state().status);
}

#[test]
fn a_pinned_wall_is_not_changed_by_its_length_its_angle_its_ends_align_or_offset() {
    let mut h = app(&format!("{BOX}\nwall {{\"points\": [[1000, 2000], [5700, 2000]]}}"));
    let inner = ids(&h, "wall")[4];
    click(&mut h, P2(3000.0, 2000.0));
    assert_eq!(h.state().selection, [inner]);
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(inner), "{:?}", h.state().status);
    let before = Arc::clone(&h.state().session.model);
    let refused = |h: &Harness<BimApp>, what: &str| {
        assert!(Arc::ptr_eq(&before, &h.state().session.model), "{what}: a pinned wall changed");
        assert!(h.state().status.error && h.state().status.text.contains("pinned"), "{what}: {:?}", h.state().status);
    };
    // Its length, typed; its distance to a neighbour, typed.
    h.state_mut().status = crate::Status::default();
    retype_dimension(&mut h, "4700", "5000");
    refused(&h, "length");
    h.state_mut().status = crate::Status::default();
    retype_dimension(&mut h, "2000", "1500");
    refused(&h, "neighbour distance");
    // Its angle, typed.
    h.state_mut().status = crate::Status::default();
    h.get_by_label_contains("Turn the wall about its start").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("30".into()));
    h.run();
    key(&mut h, Key::Enter);
    refused(&h, "angle");
    // Dragging its end.
    h.state_mut().status = crate::Status::default();
    drag(&mut h, P2(5700.0, 2000.0), P2(5700.0, 3000.0));
    refused(&h, "end grip");
    // Align and Offset.
    h.state_mut().status = crate::Status::default();
    shortcut(&mut h, "AL");
    click(&mut h, P2(4000.0, 6000.0));
    click(&mut h, P2(3000.0, 2000.0));
    refused(&h, "align");
    key(&mut h, Key::Escape);
    h.state_mut().status = crate::Status::default();
    shortcut(&mut h, "OF");
    h.state_mut().offset_copy = false;
    click(&mut h, P2(3000.0, 2000.0));
    refused(&h, "offset");
}

#[test]
fn a_grids_end_slides_along_its_own_line_whichever_way_it_is_dragged() {
    let mut h = app(&format!("{BOX}\ngrid {{\"start\": [-2000, -1000], \"end\": [-2000, 7000]}}"));
    let grid = ids(&h, "grid")[0];
    click(&mut h, P2(-2000.0, 3000.0));
    assert_eq!(h.state().selection, [grid]);
    // Dragged off its line, to the side and up: it goes up, and stays on the line.
    drag(&mut h, P2(-2000.0, 7000.0), P2(-900.0, 9000.0));
    let Some(Element::Grid(g)) = h.state().session.model.elements.get(grid) else { panic!("a grid") };
    assert_eq!(g.start, P2(-2000.0, -1000.0), "the other end stays");
    assert!(g.end.0 == -2000.0 && (g.end.1 - 9000.0).abs() <= 20.0, "still vertical, at x -2000: {:?}", g.end);
    // Dragged square across its line, it does not turn.
    let end = g.end;
    drag(&mut h, end, P2(2000.0, end.1));
    let Some(Element::Grid(g)) = h.state().session.model.elements.get(grid) else { panic!("a grid") };
    assert_eq!(g.start.0, g.end.0, "{g:?}");
}

#[test]
fn a_pinned_dimensions_grips_refuse_as_they_are_pressed() {
    let mut h = app(&format!("{BOX}\ndimension {{\"refs\": [[0, 3000], [8000, 3000]], \"at\": [0, 3000]}}"));
    let dim = ids(&h, "dimension")[0];
    click(&mut h, P2(4000.0, 3000.0));
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(dim), "{:?}", h.state().status);
    let before = Arc::clone(&h.state().session.model);
    h.state_mut().status = crate::Status::default();
    let line = h.get_by_label(&format!("Dimension line of element {dim}")).rect().center();
    h.hover_at(line);
    h.run();
    h.event(Event::PointerButton { pos: line, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run();
    h.hover_at(line + vec2(0.0, 20.0));
    h.run();
    assert!(h.state().status.error && h.state().status.text.contains("pinned"), "said at the press: {:?}", h.state().status);
    h.event(Event::PointerButton {
        pos: line + vec2(0.0, 20.0),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "a pinned dimension did not move");
}

#[test]
fn a_pinned_wall_is_not_moved_by_a_length_typed_into_a_permanent_dimension() {
    let mut h = app(TWO_WALLS_AND_DIMENSION);
    let (dim, walls) = (ids(&h, "dimension")[0], ids(&h, "wall"));
    click(&mut h, P2(2000.0, 2000.0));
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(walls[1]), "{:?}", h.state().status);
    h.state_mut().selection = vec![dim];
    h.run();
    let before = Arc::clone(&h.state().session.model);
    h.state_mut().status = crate::Status::default();
    h.get_by_label("Dimension 2000").click();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.event(Event::Text("3000".into()));
    h.run();
    key(&mut h, Key::Enter);
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "a pinned wall moved: {:?}", h.state().status);
    assert!(h.state().status.error && h.state().status.text.contains("pinned"), "{:?}", h.state().status);
}

const TWO_WALLS_AND_DIMENSION: &str = r#"wall {"points": [[0, 0], [0, 4000]]}
wall {"points": [[2000, 0], [2000, 4000]]}
dimension {"refs": [[0, 2000], [2000, 2000]], "at": [1000, -1000]}"#;

#[test]
fn a_pinned_viewport_refuses_a_drag_on_its_sheet_and_says_so() {
    let mut h = app_at(BOX, 0.05);
    press(&mut h, "View");
    press(&mut h, "Sheet");
    press(&mut h, "View");
    press(&mut h, "Place View");
    press(&mut h, "Floor Plan: Level 1");
    let vp = ids(&h, "viewport")[0];
    h.state_mut().selection = vec![vp];
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(vp), "{:?}", h.state().status);
    let at = field(&h, vp, "at");
    let (x, y) = (at[0].as_f64().unwrap(), at[1].as_f64().unwrap());
    let before = Arc::clone(&h.state().session.model);
    h.state_mut().status = crate::Status::default();
    drag(&mut h, P2(x, y), P2(x + 40.0, y));
    assert_eq!(field(&h, vp, "at"), json!([x, y]), "the viewport stayed");
    assert!(Arc::ptr_eq(&before, &h.state().session.model));
    assert!(h.state().status.error && h.state().status.text.contains("pinned"), "{:?}", h.state().status);
}

#[test]
fn a_pinned_doors_temporary_dimension_refuses_a_typed_offset_and_says_so() {
    let mut h = app(&format!("{BOX}\ndoor {{\"at\": [2000, 0]}}"));
    let door = ids(&h, "door")[0];
    click(&mut h, P2(2000.0, 0.0));
    shortcut(&mut h, "PN");
    assert!(h.state().session.model.is_pinned(door), "{:?}", h.state().status);
    let before = Arc::clone(&h.state().session.model);
    h.state_mut().status = crate::Status::default();
    retype_dimension(&mut h, "1900", "2400");
    assert!(Arc::ptr_eq(&before, &h.state().session.model), "a pinned door slid along its wall");
    assert!(h.state().status.error && h.state().status.text.contains("pinned"), "{:?}", h.state().status);
}

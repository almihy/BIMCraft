//! Copy to Clipboard and Paste: Ctrl+C and Ctrl+V, the Paste list's aligned ways, and the levels dialog.

use super::*;
use egui_kittest::kittest::NodeT;

/// A wall from (0, 0) to (8000, 0) with a door in it, on Level 1, in a project with Levels 1 and 2 (the new project's) and a
/// Level 3 at 6000.
const WALL: &str = "level {\"elevation\": 6000}\nwall {\"points\": [[0, 0], [8000, 0]]}\ndoor {\"at\": [2000, 0]}";

fn copy(h: &mut Harness<BimApp>) {
    click(h, P2(6000.0, 0.0));
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    key(h, Key::Escape);
}

#[test]
fn ctrl_c_copies_the_selection_and_ctrl_v_holds_it_at_the_cursor_until_a_click_puts_it_there() {
    let mut h = app(WALL);
    copy(&mut h);
    assert_eq!(h.state().session.clipboard_ids().len(), 2, "the wall and its door");
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    assert_eq!(h.state().tool, Tool::Paste);
    // The outline follows the cursor: a frame of it draws.
    let at = h.state().screen(P2(4000.0, -3000.0));
    h.hover_at(at);
    h.run();
    // The wall's middle (4000, 0) goes where the click is.
    click(&mut h, P2(4000.0, -3000.0));
    let made = walls(&h);
    assert_eq!(made.len(), 2);
    let new = made.iter().find(|(a, _)| a.1 < -1.0).expect("a new wall at y = -3000");
    assert!((new.0.1 + 3000.0).abs() < 1.0 && (new.0.0).abs() < 1.0 && (new.1.0 - 8000.0).abs() < 1.0, "{new:?}");
    assert_eq!(ids(&h, "door").len(), 2, "the door went with its wall");
    assert_eq!(h.state().tool, Tool::Modify, "one click pastes, as Revit's");
    assert_eq!(h.state().selection.len(), 2, "what was pasted is selected");
    // One undo takes the paste back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!((walls(&h).len(), ids(&h, "door").len()), (1, 1));
}

#[test]
fn escape_leaves_paste_and_pastes_nothing() {
    let mut h = app(WALL);
    copy(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    assert_eq!(walls(&h).len(), 1);
}

#[test]
fn ctrl_v_with_nothing_copied_says_so() {
    let mut h = app(WALL);
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.state().status.error, "{:?}", h.state().status.text);
}

#[test]
fn ctrl_c_with_nothing_selected_says_so() {
    let mut h = app(WALL);
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    assert!(h.state().status.error);
    assert!(h.state().session.clipboard_ids().is_empty());
}

#[test]
fn the_windows_copy_and_paste_events_do_the_same_as_the_keys() {
    let mut h = app(WALL);
    click(&mut h, P2(6000.0, 0.0));
    h.event(Event::Copy);
    h.run();
    assert_eq!(h.state().session.clipboard_ids().len(), 2);
    h.event(Event::Paste("clipboard text".into()));
    h.run();
    assert_eq!(h.state().tool, Tool::Paste);
}

/// The Paste list's item, from the Modify tab.
fn paste_list(h: &mut Harness<BimApp>, item: &str) {
    topmost(h, "Modify");
    press(h, "Paste");
    press(h, item);
}

fn level(h: &Harness<BimApp>, name: &str) -> Id {
    let m = &h.state().session.model;
    m.levels().into_iter().find(|(_, l)| l.name == name).map(|(id, _)| id).unwrap()
}

#[test]
fn paste_aligned_to_selected_levels_asks_which_levels_and_pastes_to_each_at_the_same_place() {
    let mut h = app(WALL);
    copy(&mut h);
    click(&mut h, P2(-500.0, -500.0));
    paste_list(&mut h, "Aligned to Selected Levels…");
    assert!(h.state().paste_levels.is_some(), "the levels dialog");
    // OK waits for a level.
    for name in ["Level 2 (", "Level 3 ("] {
        h.get_by_label_contains(name).click();
        h.run();
    }
    press(&mut h, "OK");
    assert!(h.state().paste_levels.is_none());
    let (l2, l3) = (h.state().session.model.guid(level(&h, "Level 2")), h.state().session.model.guid(level(&h, "Level 3")));
    let on = |g| ids(&h, "wall").into_iter().filter(|id| h.state().session.model.wall(*id).is_some_and(|w| w.level == g)).count();
    assert_eq!((on(l2), on(l3)), (1, 1), "a wall on each level ticked");
    assert_eq!(ids(&h, "door").len(), 3);
    for id in ids(&h, "wall") {
        let w = h.state().session.model.wall(id).unwrap().clone();
        assert_eq!((w.start, w.end), (P2(0.0, 0.0), P2(8000.0, 0.0)), "at the same place in plan");
    }
}

#[test]
fn paste_aligned_to_current_view_puts_it_on_the_level_of_the_plan_on_screen() {
    let mut h = app(WALL);
    copy(&mut h);
    let l2 = level(&h, "Level 2");
    h.state_mut().sheet = Sheet::Plan(l2);
    h.run();
    paste_list(&mut h, "Aligned to Current View");
    let g = h.state().session.model.guid(l2);
    let new: Vec<Id> = ids(&h, "wall").into_iter().filter(|id| h.state().session.model.wall(*id).is_some_and(|w| w.level == g)).collect();
    assert_eq!(new.len(), 1);
    assert_eq!(h.state().selection.len(), 2);
}

#[test]
fn paste_aligned_to_same_place_makes_the_copies_on_top_of_the_originals() {
    let mut h = app(WALL);
    copy(&mut h);
    paste_list(&mut h, "Aligned to Same Place");
    assert_eq!((walls(&h).len(), ids(&h, "door").len()), (2, 2));
    assert_eq!(walls(&h)[0], walls(&h)[1]);
}

#[test]
fn paste_aligned_to_a_picked_level_takes_one_level_from_the_list() {
    let mut h = app(WALL);
    copy(&mut h);
    paste_list(&mut h, "Aligned to Picked Level…");
    assert!(h.state().paste_levels.is_some());
    h.get_by_label_contains("Level 3 (").click();
    h.run();
    press(&mut h, "OK");
    let g = h.state().session.model.guid(level(&h, "Level 3"));
    assert_eq!(ids(&h, "wall").into_iter().filter(|id| h.state().session.model.wall(*id).is_some_and(|w| w.level == g)).count(), 1);
}

#[test]
fn the_paste_list_is_closed_while_nothing_is_copied() {
    let mut h = app(WALL);
    topmost(&mut h, "Modify");
    assert!(h.state().session.clipboard_ids().is_empty());
    // Nothing to open: the button is disabled.
    assert!(h.get_by_label("Paste").accesskit_node().is_disabled());
}

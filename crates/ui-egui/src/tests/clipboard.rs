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

/// A section along the wall, cutting it: points run along its line from its start at x = -1000, and up from Level 1.
const SECTION: &str = "section {\"start\": [-1000, 0], \"end\": [9000, 0]}";

#[test]
fn paste_aligned_to_a_picked_level_is_a_click_on_a_level_line_in_a_section() {
    let mut h = app(&format!(
        "{WALL}
{SECTION}"
    ));
    copy(&mut h);
    // In a plan the item waits: a level is picked by its line in an elevation or a section, and there is no list.
    topmost(&mut h, "Modify");
    press(&mut h, "Paste");
    assert!(h.get_by_label("Aligned to Picked Level").accesskit_node().is_disabled(), "not in a plan");
    key(&mut h, Key::Escape);
    browse(&mut h, "Section 1");
    paste_list(&mut h, "Aligned to Picked Level");
    assert_eq!(h.state().tool, Tool::PasteLevel);
    assert!(h.state().paste_levels.is_none(), "no dialog of levels: the line is the pick");
    // A click on empty paper is not a level: it says so, and the tool waits for one.
    click(&mut h, P2(5000.0, 4500.0));
    assert!(h.state().status.error, "{:?}", h.state().status);
    assert_eq!(h.state().tool, Tool::PasteLevel);
    assert_eq!(walls(&h).len(), 1);
    // Level 3's line, at 6000, above the wall: the copy goes there at the same place in plan.
    click(&mut h, P2(5000.0, 6000.0));
    assert_eq!(h.state().tool, Tool::Modify, "one click pastes: {:?}, selected {:?}", h.state().status, h.state().selection);
    let g = h.state().session.model.guid(level(&h, "Level 3"));
    let on: Vec<Id> = ids(&h, "wall").into_iter().filter(|id| h.state().session.model.wall(*id).is_some_and(|w| w.level == g)).collect();
    assert_eq!(on.len(), 1, "{:?}", h.state().status);
    let w = h.state().session.model.wall(on[0]).unwrap().clone();
    assert_eq!((w.start, w.end), (P2(0.0, 0.0), P2(8000.0, 0.0)), "the same place in plan");
    assert_eq!(ids(&h, "door").len(), 2, "its door went with it");
    // Escape leaves the tool without pasting.
    paste_list(&mut h, "Aligned to Picked Level");
    key(&mut h, Key::Escape);
    assert_eq!((h.state().tool.clone(), walls(&h).len()), (Tool::Modify, 2));
}

#[test]
fn the_paste_list_has_nothing_to_paste_until_something_is_copied_and_copies_the_selection_from_its_first_row() {
    let mut h = app(WALL);
    topmost(&mut h, "Modify");
    press(&mut h, "Paste");
    assert!(h.get_by_label("Aligned to Same Place").accesskit_node().is_disabled(), "nothing copied");
    assert!(h.get_by_label("Copy to Clipboard").accesskit_node().is_disabled(), "nothing selected");
    key(&mut h, Key::Escape);
    click(&mut h, P2(6000.0, 0.0));
    press(&mut h, "Paste");
    press(&mut h, "Copy to Clipboard");
    assert_eq!(h.state().session.clipboard_ids().len(), 2);
}

fn grids(h: &Harness<BimApp>) -> Vec<(P2, P2)> {
    let mut out: Vec<(P2, P2)> = h
        .state()
        .session
        .model
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Grid(g) => Some((g.start, g.end)),
            _ => None,
        })
        .collect();
    out.sort_by(|a, b| a.0.0.total_cmp(&b.0.0).then(a.0.1.total_cmp(&b.0.1)));
    out
}

const GRIDS: &str = "grid {\"start\": [1000, -2000], \"end\": [1000, 4000]}\ngrid {\"start\": [3000, -2000], \"end\": [3000, 4000]}";

#[test]
fn grids_copied_and_pasted_by_a_click_make_new_grids_where_it_was_clicked() {
    let mut h = app(GRIDS);
    click(&mut h, P2(1000.0, 1000.0));
    click_with(&mut h, P2(3000.0, 1000.0), Modifiers::COMMAND);
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    assert_eq!(h.state().session.clipboard_ids().len(), 2, "{:?}", h.state().status.text);
    key(&mut h, Key::Escape);
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    assert_eq!(h.state().tool, Tool::Paste);
    // Their middle is (2000, 1000); the click puts it at (7000, 1000).
    click(&mut h, P2(7000.0, 1000.0));
    assert_eq!(grids(&h).len(), 4, "two new grids: {:?}", h.state().status.text);
    assert!(!h.state().status.error, "{:?}", h.state().status.text);
    let xs: Vec<f64> = grids(&h).iter().map(|g| g.0.0).collect();
    assert_eq!(xs, vec![1000.0, 3000.0, 6000.0, 8000.0]);
    assert_eq!(h.state().selection.len(), 2, "what was pasted is selected");
}

#[test]
fn a_wall_and_a_grid_copied_together_are_both_pasted_by_a_click_on_the_same_level() {
    let mut h = app(&format!("{WALL}\ngrid {{\"start\": [1000, 2000], \"end\": [1000, 6000]}}"));
    click(&mut h, P2(6000.0, 0.0));
    click_with(&mut h, P2(1000.0, 5000.0), Modifiers::COMMAND);
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    key(&mut h, Key::Escape);
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    click(&mut h, P2(4000.0, -1500.0));
    assert_eq!((walls(&h).len(), grids(&h).len()), (2, 2), "{:?}", h.state().status.text);
}

#[test]
fn a_wall_and_a_grid_copied_together_are_both_pasted_by_a_click_on_another_levels_plan() {
    let mut h = app(&format!("{WALL}\ngrid {{\"start\": [1000, 2000], \"end\": [1000, 6000]}}"));
    click(&mut h, P2(6000.0, 0.0));
    click_with(&mut h, P2(1000.0, 5000.0), Modifiers::COMMAND);
    h.key_press_modifiers(Modifiers::COMMAND, Key::C);
    h.run();
    key(&mut h, Key::Escape);
    let l2 = level(&h, "Level 2");
    h.state_mut().open_sheet(Sheet::Plan(l2));
    h.state_mut().canvas.show(l2, P2(-1000.0, -3000.0), P2(9000.0, 7000.0));
    h.run();
    h.run();
    h.key_press_modifiers(Modifiers::COMMAND, Key::V);
    h.run();
    assert_eq!(h.state().tool, Tool::Paste, "{:?}", h.state().status);
    click(&mut h, P2(4000.0, -1500.0));
    let g = h.state().session.model.guid(l2);
    let on_l2 = ids(&h, "wall").into_iter().filter(|id| h.state().session.model.wall(*id).is_some_and(|w| w.level == g)).count();
    assert_eq!((on_l2, grids(&h).len()), (1, 2), "the wall came to Level 2 and the grid beside: {:?}", h.state().status.text);
    // One undo takes both back.
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!((walls(&h).len(), grids(&h).len()), (1, 1));
}

#[test]
fn paste_aligned_to_current_view_is_not_offered_in_the_view_the_elements_were_copied_in() {
    let mut h = app(WALL);
    copy(&mut h);
    // Revit: "The view must be different from the view where the elements were cut or copied."
    topmost(&mut h, "Modify");
    press(&mut h, "Paste");
    assert!(h.get_by_label("Aligned to Current View").accesskit_node().is_disabled(), "the plan it was copied in");
    assert!(!h.get_by_label("Aligned to Same Place").accesskit_node().is_disabled());
    key(&mut h, Key::Escape);
    let l2 = level(&h, "Level 2");
    h.state_mut().sheet = Sheet::Plan(l2);
    h.run();
    topmost(&mut h, "Modify");
    press(&mut h, "Paste");
    assert!(!h.get_by_label("Aligned to Current View").accesskit_node().is_disabled(), "another plan");
}

//! How a hand picks and repeats: the tools that take a selection asking for it first, Enter repeating the last tool, Create
//! Similar and Repeat in the right-click menu, Tab selecting a chain of walls, Ctrl+A, the arrow keys nudging, the status bar
//! naming what is under the cursor, and the cursor showing the tool.

use super::*;
use crate::selecting::nudge_step;
use egui::CursorIcon;

const ASK: &str = "Select elements, then press Enter";

fn south(h: &Harness<BimApp>) -> Id {
    find(h, |e| matches!(e, Element::Wall(w) if w.start.1 == 0.0 && w.end.1 == 0.0))
}

fn key_of(h: &Harness<BimApp>) -> Id {
    match h.state().sheet {
        Sheet::Plan(k) => k,
        other => panic!("not a plan: {other:?}"),
    }
}

fn wall_ids(h: &Harness<BimApp>) -> Vec<Id> {
    ids(h, "wall")
}

#[test]
fn move_copy_rotate_and_mirror_with_nothing_selected_ask_for_elements_and_go_on_at_enter() {
    for (letters, label) in [("MV", "Move"), ("CO", "Copy"), ("RO", "Rotate"), ("MM", "Mirror")] {
        let mut h = app(BOX);
        assert!(h.state().selection.is_empty());
        shortcut(&mut h, letters);
        assert_eq!(h.state().tool, Tool::Modify, "{label}: the elements are picked as in Modify");
        assert!(h.state().flow.pending.is_some(), "{label} waits for its elements");
        assert!(h.query_by_label(ASK).is_some(), "{label}: the status bar asks");
        assert!(!h.state().status.error, "{label} is not refused: {:?}", h.state().status);
        // Enter before anything is picked changes nothing.
        key(&mut h, Key::Enter);
        assert!(h.state().flow.pending.is_some(), "{label}: still waiting");
        // Pick a wall, and Enter starts the tool on it.
        click(&mut h, P2(4000.0, 0.0));
        assert_eq!(h.state().selection, [south(&h)]);
        key(&mut h, Key::Enter);
        assert!(h.state().flow.pending.is_none());
        let started = std::mem::discriminant(&h.state().tool);
        let wanted = match letters {
            "MV" => Tool::Move(None),
            "CO" => Tool::Copy(None),
            "RO" => Tool::Rotate { centre: P2(0.0, 0.0), start: None, place: false },
            _ => Tool::Mirror { draw: false, start: None },
        };
        assert_eq!(started, std::mem::discriminant(&wanted), "{label} runs");
        assert_eq!(h.state().selection, [south(&h)], "on what was picked");
        assert!(h.query_by_label(ASK).is_none());
    }
}

#[test]
fn a_tool_that_waits_for_its_elements_takes_a_box_and_does_not_drag_what_is_under_it_and_esc_ends_it() {
    let mut h = app(BOX);
    shortcut(&mut h, "MV");
    let before = walls(&h);
    // A drag from a wall draws a selection box, not a move.
    let (a, b) = (h.state().screen(P2(3000.0, 0.0)), h.state().screen(P2(5000.0, 1500.0)));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert_eq!(walls(&h), before, "nothing moved");
    assert!(h.state().flow.pending.is_some());
    // A box right to left takes what it touches, and Enter goes on.
    let (a, b) = (h.state().screen(P2(5000.0, 1500.0)), h.state().screen(P2(3000.0, -500.0)));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert_eq!(h.state().selection, [south(&h)]);
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Move(None));
    let mut h = app(BOX);
    shortcut(&mut h, "MV");
    assert!(h.state().flow.pending.is_some());
    key(&mut h, Key::Escape);
    assert!(h.state().flow.pending.is_none() && h.state().tool == Tool::Modify, "Esc ends it");
    // With no tool running, Enter repeats the last one: Move asks again.
    key(&mut h, Key::Enter);
    assert!(h.state().flow.pending.is_some(), "Enter repeated Move");
}

#[test]
fn a_tool_with_a_selection_already_starts_at_once() {
    let mut h = app(BOX);
    click(&mut h, P2(4000.0, 0.0));
    shortcut(&mut h, "MV");
    assert_eq!(h.state().tool, Tool::Move(None));
    assert!(h.state().flow.pending.is_none());
}

#[test]
fn enter_with_no_tool_running_repeats_the_last_one_and_so_does_rr() {
    let mut h = app(BOX);
    // Nothing to repeat yet.
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Modify);
    shortcut(&mut h, "WA");
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify, "the tool ended");
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()), "Enter starts the wall tool again");
    // While it runs, Enter is its own: it builds the chain and does not start anything else.
    click(&mut h, P2(10000.0, 0.0));
    click(&mut h, P2(12000.0, 0.0));
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()));
    key(&mut h, Key::Escape);
    // RR is the same, and the last tool is the last one started.
    shortcut(&mut h, "DR");
    key(&mut h, Key::Escape);
    shortcut(&mut h, "WN");
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    shortcut(&mut h, "RR");
    assert_eq!(h.state().tool, Tool::Window);
}

#[test]
fn repeat_remembers_only_the_tools_the_user_started() {
    let mut h = app(BOX);
    // A tool the app starts for a dialog is nobody's last command: with nothing before it there is nothing to repeat.
    h.state_mut().set_tool(Tool::PatternCorner(None));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Modify, "Enter has nothing to repeat");
    let at = h.state().screen(P2(4000.0, 3000.0));
    assert!(offered(&mut h, at, &["Repeat ["]).is_empty());
    key(&mut h, Key::Escape);
    // After a wall, the app's own tool leaves Repeat at the wall, in Enter and in the menu.
    shortcut(&mut h, "WA");
    key(&mut h, Key::Escape);
    h.state_mut().set_tool(Tool::PatternCorner(None));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    assert_eq!(offered(&mut h, at, &["Repeat [Wall]"]), ["Repeat [Wall]"]);
    key(&mut h, Key::Escape);
    key(&mut h, Key::Enter);
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()), "Enter repeats the wall");
}

#[test]
fn the_right_click_menu_offers_repeat_and_create_similar() {
    let mut h = app(BOX);
    let at = h.state().screen(P2(4000.0, 3000.0));
    assert!(offered(&mut h, at, &["Repeat ["]).is_empty(), "nothing to repeat at first");
    key(&mut h, Key::Escape);
    shortcut(&mut h, "WA");
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // Repeat [Wall] is on the menu while no tool runs, and starts it.
    assert_eq!(offered(&mut h, at, &["Repeat [Wall]"]), ["Repeat [Wall]"]);
    choose_item(&mut h, "Repeat [Wall]");
    assert_eq!(h.state().tool, Tool::Wall(Vec::new()));
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    // A door selected: Create Similar starts the door tool with its type.
    let mut h = app(r#"wall {"points": [[0,0],[8000,0]]}
door {"at": [2000, 0], "type": "Single 800 x 2100"}
door {"at": [5000, 0], "type": "Single 900 x 2100"}"#);
    let door = ids(&h, "door")[0];
    let ty = element_json(&h.state().session.model, door)["type"].as_str().unwrap().to_string();
    h.state_mut().selection = vec![door];
    h.run();
    let at = h.state().screen(P2(4000.0, 3000.0));
    assert_eq!(offered(&mut h, at, &["Create Similar"]), ["Create Similar"]);
    h.state_mut().types.insert("door", "something else".into());
    choose_item(&mut h, "Create Similar");
    assert_eq!(h.state().tool, Tool::Door);
    assert_eq!(h.state().types.get("door"), Some(&ty), "with the type of the one selected");
    // Not for a lone click on nothing, nor for two elements: the item is there but off.
    key(&mut h, Key::Escape);
    key(&mut h, Key::Escape);
    let both = ids(&h, "door");
    h.state_mut().selection = both;
    h.run();
    let items = h.state().context_items();
    let similar = items.iter().flatten().find(|i| i.action == Action::CreateSimilar).unwrap();
    assert!(!similar.enabled, "one element at a time");
}

fn sorted(mut v: Vec<Id>) -> Vec<Id> {
    v.sort_unstable();
    v
}

#[test]
fn tab_over_a_wall_only_highlights_its_chain_and_a_click_selects_it() {
    let mut h = app(&format!(
        "{BOX}
wall {{\"points\": [[20000, 0], [26000, 0]]}}"
    ));
    let all = wall_ids(&h);
    assert_eq!(all.len(), 5);
    let (room, lone) = (sorted(all[..4].to_vec()), all[4]);
    // Something is selected already: Tab leaves it alone.
    click(&mut h, P2(23000.0, 0.0));
    assert_eq!(h.state().selection, [lone]);
    let on_room = h.state().screen(P2(4000.0, 0.0));
    h.hover_at(on_room);
    h.run();
    key(&mut h, Key::Tab);
    assert_eq!(sorted(h.state().flow.preview.clone()), room, "Tab highlights the walls of the room");
    assert_eq!(h.state().selection, [lone], "and selects nothing");
    assert_eq!(h.state_mut().hover_text().as_deref(), Some("Chain of walls"), "the status bar names the chain");
    // Tab again goes on to what else is under the cursor: here only the wall, so the highlight goes, the selection as it was.
    key(&mut h, Key::Tab);
    assert!(h.state().flow.preview.is_empty());
    assert_eq!(h.state().selection, [lone]);
    assert_ne!(h.state_mut().hover_text().as_deref(), Some("Chain of walls"));
    // Shift+Tab goes back to the chain.
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run();
    assert_eq!(sorted(h.state().flow.preview.clone()), room, "Shift+Tab came back to the chain");
    // Moving off the walls drops the highlight and leaves the selection.
    let away = h.state().screen(P2(4000.0, 3000.0));
    h.hover_at(away);
    h.run();
    assert!(h.state().flow.preview.is_empty(), "the highlight went with the cursor");
    assert_eq!(h.state().selection, [lone]);
    // A click on the highlighted chain selects it, and the highlight is done.
    h.hover_at(on_room);
    h.run();
    key(&mut h, Key::Tab);
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(sorted(h.state().selection.clone()), room, "the click took the chain");
    assert!(h.state().flow.preview.is_empty());
    // Ctrl+click adds the chain to what is selected; Shift+click takes it away.
    click(&mut h, P2(23000.0, 0.0));
    h.hover_at(on_room);
    h.run();
    key(&mut h, Key::Tab);
    click_with(&mut h, P2(4000.0, 0.0), Modifiers::COMMAND);
    let mut both = room.clone();
    both.push(lone);
    assert_eq!(sorted(h.state().selection.clone()), sorted(both), "Ctrl+click added the chain");
    h.hover_at(on_room);
    h.run();
    key(&mut h, Key::Tab);
    click_with(&mut h, P2(4000.0, 0.0), Modifiers::SHIFT);
    assert_eq!(h.state().selection, [lone], "Shift+click took the chain off");
    // Without Tab a click is one wall, as before; a lone wall has no chain, and Tab leaves it to what is next.
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection.len(), 1);
    let lone_at = h.state().screen(P2(23000.0, 0.0));
    h.hover_at(lone_at);
    h.run();
    key(&mut h, Key::Tab);
    assert!(h.state().flow.preview.is_empty(), "nothing to chain: {:?}", h.state().flow.preview);
}

#[test]
fn ctrl_a_selects_everything_the_view_shows() {
    let mut h = app(BOX);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    let mut got = h.state().selection.clone();
    got.sort_unstable();
    let mut walls = wall_ids(&h);
    walls.sort_unstable();
    assert!(walls.iter().all(|w| got.contains(w)), "the four walls are in {got:?}");
    // In a text field it is the field's: select all the text, not the plan.
    key(&mut h, Key::Escape);
    assert!(h.state().selection.is_empty());
    h.state_mut().selection = vec![walls[0]];
    h.run();
}

#[test]
fn ctrl_a_leaves_out_the_views_marks_so_delete_cannot_take_a_view_with_it() {
    let mut h = app(&format!("{BOX}\nsection {{\"start\": [-1000, 3000], \"end\": [9000, 3000]}}\nelevation {{\"at\": [4000, 3000]}}"));
    let marks = [ids(&h, "section"), ids(&h, "elevation")].concat();
    assert!(!marks.is_empty(), "the plan has marks to leave out");
    let walls = wall_ids(&h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.run();
    let got = h.state().selection.clone();
    assert!(walls.iter().all(|w| got.contains(w)), "the walls are in {got:?}");
    assert!(marks.iter().all(|m| !got.contains(m)), "no section or elevation mark is in {got:?}");
    // Delete then takes the model's elements and no view.
    key(&mut h, Key::Delete);
    assert!(wall_ids(&h).is_empty());
    assert!(marks.iter().all(|m| h.state().session.model.elements.contains_key(m)), "the views are still there");
}

#[test]
fn the_arrow_keys_nudge_the_selection_a_step_that_follows_the_zoom_and_shift_takes_ten() {
    let mut h = app(r#"wall {"points": [[0,0],[8000,0]]}"#);
    let wall = wall_ids(&h)[0];
    click(&mut h, P2(4000.0, 0.0));
    assert_eq!(h.state().selection, [wall]);
    let ends = |h: &Harness<BimApp>| match h.state().session.model.elements.get(wall) {
        Some(Element::Wall(w)) => (w.start, w.end),
        _ => panic!("no wall"),
    };
    let (a, _) = ends(&h);
    let step = nudge_step(h.state().canvas.view(key_of(&h)).unwrap().scale);
    key(&mut h, Key::ArrowRight);
    let (b, _) = ends(&h);
    assert!(
        (b.0 - a.0 - step).abs() < 1e-6 && (b.1 - a.1).abs() < 1e-6,
        "a step right: {a:?} to {b:?}, step {step}, {:?}",
        h.state().status
    );
    key(&mut h, Key::ArrowUp);
    let (c, _) = ends(&h);
    assert!((c.1 - b.1 - step).abs() < 1e-6, "a step up (north): {b:?} to {c:?}");
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowLeft);
    h.run();
    let (d, _) = ends(&h);
    assert!((c.0 - d.0 - 10.0 * step).abs() < 1e-6 && (d.1 - c.1).abs() < 1e-6, "ten steps left: {c:?} to {d:?}");
    key(&mut h, Key::ArrowDown);
    let (e, _) = ends(&h);
    assert!((d.1 - e.1 - step).abs() < 1e-6, "a step down: {d:?} to {e:?}");
    // Zoomed in, a step is smaller: a few pixels all the same.
    let plan = key_of(&h);
    h.state_mut().canvas.zoom(plan, 20.0);
    h.run();
    let small = nudge_step(h.state().canvas.view(plan).unwrap().scale);
    assert!(small < step);
    let before = ends(&h).0;
    key(&mut h, Key::ArrowRight);
    assert!((ends(&h).0.0 - before.0 - small).abs() < 1e-6, "the zoomed step is {small}");
    // With nothing selected the keys do nothing to the model.
    key(&mut h, Key::Escape);
    let walls_before = walls(&h);
    key(&mut h, Key::ArrowRight);
    assert_eq!(walls(&h), walls_before);
}

#[test]
fn the_status_bar_names_the_element_under_the_cursor() {
    let mut h = app(BOX);
    let wall = south(&h);
    let json = element_json(&h.state().session.model, wall);
    let ty = json["type"].as_str().unwrap().to_string();
    assert!(h.query_by_label_contains("Walls : ").is_none(), "over empty floor it names nothing");
    let at = h.state().screen(P2(4000.0, 0.0));
    h.hover_at(at);
    h.run();
    let words = format!("Walls : Basic Wall : {ty}");
    assert!(h.query_by_label(&words).is_some(), "the bar says {words:?}");
    // Off the wall again, it is gone.
    let off = h.state().screen(P2(4000.0, 3000.0));
    h.hover_at(off);
    h.run();
    assert!(h.query_by_label(&words).is_none());
}

#[test]
fn the_canvas_shows_the_tool_in_the_cursor() {
    let mut h = app(BOX);
    let off = h.state().screen(P2(4000.0, 3000.0));
    let on_wall = h.state().screen(P2(4000.0, 0.0));
    let icon = |h: &Harness<BimApp>| h.state().canvas.nav.icon.get();
    h.hover_at(off);
    h.run();
    assert_eq!(icon(&h), None, "Modify over nothing: the arrow");
    // A drawing tool: crosshair.
    shortcut(&mut h, "WA");
    h.hover_at(off);
    h.run();
    assert_eq!(icon(&h), Some(CursorIcon::Crosshair));
    key(&mut h, Key::Escape);
    // Over a selected element, the move cross.
    click(&mut h, P2(4000.0, 0.0));
    h.hover_at(on_wall);
    h.run();
    assert_eq!(icon(&h), Some(CursorIcon::Move), "over what a drag would move");
    // Over a grip of the wall selected alone, too.
    let grip = h.state().screen(P2(0.0, 0.0));
    h.hover_at(grip);
    h.run();
    assert_eq!(icon(&h), Some(CursorIcon::Move), "over a grip");
    // A tool that picks shows a hand over what it would take.
    key(&mut h, Key::Escape);
    shortcut(&mut h, "SL");
    h.hover_at(on_wall);
    h.run();
    assert_eq!(icon(&h), Some(CursorIcon::PointingHand), "Split over a wall");
    h.hover_at(off);
    h.run();
    assert_eq!(icon(&h), Some(CursorIcon::Crosshair), "and a crosshair over nothing");
}

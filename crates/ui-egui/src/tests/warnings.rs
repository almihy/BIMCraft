//! Review Warnings: the count in the status bar, the list with Show, Manage's button, and the warnings Properties lists for what
//! is selected, driven as a user drives it.

use super::*;
use egui_kittest::kittest::NodeT;

/// The box's four walls with two doors that overlap in the south one and a third past its end: the doors' ids.
fn crowded() -> (Harness<'static, BimApp>, Vec<Id>, Vec<Id>) {
    let mut h = app_at(BOX, 0.05);
    let mut walls = ids(&h, "wall");
    walls.sort_unstable();
    let s = &mut h.state_mut().session;
    let doors: Vec<Id> = [2000.0, 2600.0, 7900.0]
        .iter()
        .map(|at| s.execute("door", &json!({"wall": walls[0], "offset": at})).unwrap()["id"].as_u64().unwrap())
        .collect();
    h.run();
    (h, walls, doors)
}

#[test]
fn the_status_bar_counts_the_warnings_and_opens_the_list_where_show_selects_and_zooms() {
    let (mut h, _, doors) = crowded();
    // Two warnings: the pair that overlap, and the one past the wall's end.
    h.get_by_label("2 warnings");
    press(&mut h, "2 warnings");
    assert!(h.state().warnings_dialog.is_some());
    h.get_by_label("2 warnings. Show selects what one is about, and zooms to it.");
    // Show waits for a row.
    assert!(h.get_by_label("Show").accesskit_node().is_disabled(), "Show waits for a row");
    let overlap = format!("door {} and door {} overlap in wall", doors[0], doors[1]);
    let row = h.get_all_by_label_contains(&overlap).next().expect("the overlap is listed").rect().center();
    click_at(&mut h, row);
    assert!(h.state().selection.is_empty(), "a click on a row picks it, it doesn't show it");
    press(&mut h, "Show");
    assert!(h.state().warnings_dialog.is_none());
    assert_eq!(h.state().selection, [doors[0], doors[1]]);
    assert_eq!(h.state().sheet, Sheet::Plan(100_000));
    h.run();
    let at = h.state().screen(P2(2300.0, 0.0));
    assert!(h.ctx.content_rect().contains(at), "zoomed to them: {at:?}");
}

#[test]
fn manages_review_warnings_opens_the_list_and_a_project_without_warnings_says_so() {
    let mut h = app(BOX);
    h.set_size(vec2(1800.0, 900.0));
    assert!(h.query_by_label_contains("warnings").is_none(), "no count in the status bar when there are none");
    press(&mut h, "Manage");
    press(&mut h, "Review Warnings");
    h.get_by_label("The project has no warnings.");
    press(&mut h, "Close");
    assert!(h.state().warnings_dialog.is_none());
    // A door past the wall's end: the count, the same list from Manage.
    let wall = ids(&h, "wall")[0];
    h.state_mut().session.execute("door", &json!({"wall": wall, "offset": 100})).unwrap();
    h.run();
    h.get_by_label("1 warning");
    press(&mut h, "Review Warnings");
    assert!(h.query_by_label_contains("past the start of wall").is_some());
}

#[test]
fn a_selected_element_with_a_warning_shows_it_in_properties_and_one_without_shows_none() {
    let (mut h, walls, doors) = crowded();
    h.state_mut().selection = vec![doors[2]];
    h.run();
    h.get_by_label("Warnings");
    assert!(h.query_by_label_contains("past the end of wall").is_some());
    // A wall nobody warns of has no such group.
    h.state_mut().selection = vec![walls[1]];
    h.run();
    assert!(h.query_by_label("Warnings").is_none());
    // Moving the door in clears it, and the count follows.
    h.state_mut().session.execute("modify", &json!({"id": doors[2], "offset": 6000})).unwrap();
    h.run();
    h.get_by_label("1 warning");
}

#[test]
fn a_batch_of_commands_from_the_window_says_the_warnings_it_made_in_its_last_result() {
    let mut h = app_at(BOX, 0.05);
    let mut walls = ids(&h, "wall");
    walls.sort_unstable();
    let params: Vec<Value> = [2000.0, 2300.0].iter().map(|at| json!({"wall": walls[0], "offset": at})).collect();
    let results = h.state_mut().run_all("Doors", "door", params);
    assert_eq!(results.len(), 2);
    assert!(results[0].get("new_warnings").is_none(), "{:?}", results[0]);
    assert_eq!(results[1]["new_warnings"][0]["rule"], "openings_overlap", "{:?}", results[1]);
}

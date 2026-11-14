//! The ribbon folded to its tabs and the panels that don't fit: a tab hovered or clicked, a panel's name listing every tool, a
//! two-row panel handing tools over by columns, a narrow window's panels as small icons.

use super::*;

/// The app in a window `w` points wide, frames `step` seconds apart.
fn window(w: f32, step: f32) -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    let mut h = Harness::builder()
        .with_size([w, 800.0])
        .with_max_steps(32)
        .with_step_dt(step)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), BimApp::new(s));
    h.run();
    h
}

/// A folded ribbon, in a window `w` wide.
fn folded(w: f32) -> Harness<'static, BimApp> {
    let mut h = window(w, 0.1);
    h.state_mut().ribbon_min = true;
    h.run();
    h
}

/// The middle of the highest widget named `label`: a ribbon tab over a tool of the same name.
fn tab_at(h: &Harness<BimApp>, label: &str) -> Pos2 {
    h.get_all_by_label(label).min_by(|a, b| a.rect().top().total_cmp(&b.rect().top())).map(|n| n.rect().center()).unwrap()
}

/// A click on the tab named `label`.
fn pick_tab(h: &mut Harness<BimApp>, label: &str) {
    let at = tab_at(h, label);
    click_at(h, at);
}

/// The ribbon's tab strip stands above this, a docked ribbon's tools below it.
fn on_the_ribbon(h: &Harness<BimApp>, label: &str) -> bool {
    h.query_all_by_label(label).any(|n| n.rect().top() > 80.0 && n.rect().bottom() < 190.0)
}

#[test]
fn the_pointer_on_a_folded_tab_shows_its_tools_and_leaving_it_and_them_closes_them() {
    let mut h = folded(1280.0);
    assert!(h.query_by_label("Beam").is_none(), "only the tabs");
    let tab = tab_at(&h, "Structure");
    h.hover_at(tab);
    h.run();
    assert!(h.query_by_label("Beam").is_some(), "the tab's tools float under the tabs");
    assert!(!h.state().ribbon_peek, "on the pointer's say alone, not pinned");
    // Down into them, through the gap between: they stay while the pointer is in them.
    h.hover_at(tab + vec2(0.0, 14.0));
    h.run();
    let beam = h.get_by_label("Beam").rect().center();
    h.hover_at(beam);
    h.run_steps(12);
    assert!(h.query_by_label("Beam").is_some(), "the pointer is on them");
    // Off them, they go: not at once but within a second.
    h.hover_at(pos2(700.0, 600.0));
    h.step();
    assert!(h.query_by_label("Beam").is_some(), "a moment's grace for a pointer on its way");
    h.run_steps(8);
    assert!(h.query_by_label("Beam").is_none(), "gone");
    // Off the tab, not into them, they go as well.
    h.hover_at(tab);
    h.run();
    assert!(h.query_by_label("Beam").is_some());
    h.hover_at(pos2(700.0, 600.0));
    h.run_steps(8);
    assert!(h.query_by_label("Beam").is_none());
    assert_eq!(h.state().tab, Tab::Architecture, "hovering shows a tab's tools, it doesn't pick the tab");
}

#[test]
fn a_click_on_a_folded_tab_pins_its_tools_until_a_tool_is_picked_or_a_click_lands_elsewhere() {
    let mut h = folded(1280.0);
    let tab = tab_at(&h, "Structure");
    click_at(&mut h, tab);
    assert!(h.state().ribbon_peek && h.query_by_label("Beam").is_some(), "pinned");
    // The pointer away, they stay.
    h.hover_at(pos2(700.0, 600.0));
    h.run_steps(12);
    assert!(h.query_by_label("Beam").is_some(), "pinned means pinned");
    // Another tab, hovered, shows its own.
    let plumbing = tab_at(&h, "Plumbing");
    h.hover_at(plumbing);
    h.run();
    assert!(h.state().ribbon_peek, "still pinned");
    assert_eq!(h.state().tab, Tab::Plumbing);
    assert!(h.query_by_label("Beam").is_none() && h.query_by_label("Pipe").is_some(), "the other tab's tools");
    // A click elsewhere puts them away.
    click_at(&mut h, pos2(700.0, 600.0));
    assert!(!h.state().ribbon_peek && h.query_by_label("Pipe").is_none());
    // Esc puts them away, and they stay away while the pointer is still on the tab.
    click_at(&mut h, plumbing);
    assert!(h.query_by_label("Pipe").is_some());
    key(&mut h, Key::Escape);
    h.run_steps(6);
    assert!(h.query_by_label("Pipe").is_none(), "Esc, the pointer still on the tab");
    // A tool picked closes them too.
    h.hover_at(pos2(700.0, 600.0));
    h.run();
    let structure = tab_at(&h, "Structure");
    click_at(&mut h, structure);
    press(&mut h, "Beam");
    assert!(h.query_by_label("Beam").is_none(), "folded again");
    assert_eq!(h.state().tool, Tool::Run("beam", None));
}

#[test]
fn the_floating_tools_show_every_panel_in_full_when_the_window_has_room() {
    // The View tab is 1230 points wide: in full in a 1280 window, none folded, handed over or made small.
    let mut h = folded(1280.0);
    let tab = tab_at(&h, "View");
    h.hover_at(tab);
    h.run_steps(3);
    for name in ["Visibility/Graphics", "Duplicate View", "Sheet Composition ▾", "Close Inactive", "User Interface"] {
        assert!(h.query_by_label(name).is_some(), "{name} is shown");
    }
    for big in ["Visibility/Graphics", "Duplicate View", "Schedules", "Title Blocks", "Close Inactive"] {
        assert!(h.get_by_label(big).rect().height() > 45.0, "{big} is a large button, with its label");
    }
}

#[test]
fn a_window_too_narrow_for_a_tab_draws_its_last_panels_as_small_icons_not_a_folded_button() {
    let mut h = window(1100.0, 0.1);
    h.run_steps(4);
    // Datum, the last panel of Architecture: its tools two high as icons, its name and arrow under them as always.
    let level = h.get_by_label("Level").rect();
    assert!((level.width(), level.height()) == (30.0, 25.0), "a small icon: {level:?}");
    let name = h.get_by_label("Datum ▾").rect();
    assert!(name.height() < 30.0 && name.top() > level.bottom(), "the name under the icons: {name:?}");
    // Two to a column, down then across.
    let grid = h.get_by_label("Grid").rect();
    assert_eq!(grid.top(), level.top() + 27.0, "the second of the first column: {grid:?} {level:?}");
    // The panels that have room keep their large buttons.
    assert!(h.get_by_label("Wall").rect().height() > 45.0, "Build is still large");
    // Nothing is in a list that could be on the ribbon: every tool of the tab is where it can be clicked.
    for tool in ["Wall", "Door", "Stair", "Room", "Shaft", "Dormer", "Grid"] {
        assert!(on_the_ribbon(&h, tool), "{tool} is on the ribbon");
    }
}

/// The Modify tab's tools, row by row, as the Modify panel draws them: the second row's last two are the clipboard's.
const ROW_ONE: [&str; 8] =
    ["Align", "Offset", "Mirror Pick Axis", "Mirror Draw Axis", "Split Element", "Linework", "Scale", "Trim/Extend Single Element"];
const ROW_TWO: [&str; 10] =
    ["Move", "Copy", "Rotate", "Array", "Trim/Extend to Corner", "Trim/Extend Multiple Elements", "Pin", "Unpin", "Delete", "Paste"];

#[test]
fn the_modify_panel_keeps_both_rows_while_there_is_room() {
    let mut h = window(1280.0, 0.1);
    pick_tab(&mut h, "Modify");
    h.run_steps(3);
    for tool in ROW_ONE.iter().chain(&ROW_TWO) {
        assert!(on_the_ribbon(&h, tool), "{tool} is on the ribbon");
    }
}

#[test]
fn the_modify_panel_hands_its_tools_over_by_columns_and_only_what_does_not_fit() {
    // A window too narrow for the tab: the tools last in their rows go, a column at a time, never one row's and not the other's.
    let mut h = window(560.0, 0.1);
    h.state_mut().tab = Tab::Modify;
    h.run_steps(6);
    let shown = |name: &str| on_the_ribbon(&h, name);
    let first = ROW_ONE.iter().filter(|n| shown(n)).count();
    let second = ROW_TWO.iter().filter(|n| shown(n)).count();
    // Row one has 8 tools and row two 10, the clipboard's two ending it: a column of both rows goes at once.
    assert!(first < ROW_ONE.len(), "a window this narrow hands some over: {first}");
    assert!(first >= 2, "and keeps what fits: {first}");
    assert_eq!(second, first, "the same columns of both rows: {first} {second}");
    for (a, b) in ROW_ONE.iter().zip(&ROW_TWO) {
        assert_eq!(shown(a), shown(b), "{a} and {b} are in one column");
    }
    // What was handed over is in the panel's list, every tool of it.
    press(&mut h, "Modify ▾");
    let under = h.get_by_label("Modify ▾").rect().bottom();
    for tool in ROW_ONE.iter().chain(&ROW_TWO) {
        assert!(h.query_all_by_label(tool).any(|n| n.rect().top() > under), "{tool} is in the list");
    }
}

#[test]
fn a_panels_name_lists_every_tool_one_under_the_next_as_wide_as_the_widest() {
    let mut h = window(1280.0, 0.1);
    pick_tab(&mut h, "Modify");
    h.run_steps(3);
    press(&mut h, "Modify ▾");
    let under = h.get_by_label("Modify ▾").rect().bottom();
    // Each tool's row in the list, in the panel's order: the highest of those below the name is the list's.
    let mut rows = Vec::new();
    for tool in ROW_ONE.iter().chain(&ROW_TWO) {
        let row = h.query_all_by_label(tool).map(|n| n.rect()).filter(|r| r.top() > under).min_by(|a, b| a.top().total_cmp(&b.top()));
        rows.push(row.unwrap_or_else(|| panic!("{tool} is not in the list")));
    }
    // The panel's own order: its first row's tools, then its second's.
    assert!(rows.windows(2).all(|w| w[1].top() > w[0].top()), "one under the next, in order: {rows:?}");
    assert!(rows.iter().all(|r| r.left() == rows[0].left() && r.width() == rows[0].width()), "a column of equal rows");
    assert!(rows[0].width() > 150.0 && rows[0].width() < 400.0, "as wide as its words, not a window or a fixed 220: {}", rows[0].width());
}

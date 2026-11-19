//! A curtain wall worked on by hand: the Curtain Grid tool, Add/Remove Segments, the Mullion tool, a hand-made grid line
//! selected, pins, Tab in the 3D view and in an elevation, and what the core gives the tools (`render::curtain_grid`).

use super::*;
use crate::curtain::tools::{Among, On};
use bimcraft_core::curtain;
use bimcraft_core::render::curtain_grid::{self, Snap};

const PLAIN: &str = r#"wall {"points": [[0,0],[6000,0]], "type": "Curtain Wall", "height": 3000}"#;
const STORE: &str = r#"wall {"points": [[0,0],[6000,0]], "type": "Storefront", "height": 3000}"#;
const GLAZING: &str = r#"wall {"points": [[0,0],[6000,0]], "type": "Exterior Glazing", "height": 3000}"#;
/// An elevation of the wall from the south: the wall runs from 1000 to 7000 along it.
const SOUTH: &str = r#"section {"start": [-1000,-1000], "end": [7000,-1000]}"#;

/// The app on an elevation of the curtain wall `wall` alone, and the wall.
fn elevation(wall: &str) -> (Harness<'static, BimApp>, Id) {
    let mut h = app(&format!("{wall}\n{SOUTH}"));
    let section = ids(&h, "section")[0];
    h.state_mut().open_sheet(Sheet::Section(section));
    h.run();
    let wall = ids(&h, "wall")[0];
    (h, wall)
}

/// The place `s` mm along the wall and `z` mm over its base, in the elevation.
fn on(s: f64, z: f64) -> P2 {
    P2(1000.0 + s, z)
}

fn hover(h: &mut Harness<BimApp>, p: P2) {
    let pos = h.state().screen(p);
    h.hover_at(pos);
    h.run();
}

/// The hand-made grid lines of the model: which way each runs and where it is.
fn lines(h: &Harness<BimApp>) -> Vec<(bool, f64)> {
    let m = &h.state().session.model;
    ids(h, "curtaingrid")
        .into_iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::CurtainGrid(l)) => Some((l.vertical, l.at)),
            _ => None,
        })
        .collect()
}

fn mullions(h: &Harness<BimApp>) -> Vec<curtain::Mullion> {
    let m = &h.state().session.model;
    ids(h, "mullion")
        .into_iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Mullion(x)) => Some(x.clone()),
            _ => None,
        })
        .collect()
}

fn grid_line(h: &mut Harness<BimApp>, wall: Id, vertical: bool, at: f64) -> Id {
    let made = h.state_mut().run("curtain_grid", json!({"wall": wall, "vertical": vertical, "at": at})).unwrap();
    h.run();
    made["ids"][0].as_u64().unwrap()
}

#[test]
fn the_ribbon_has_the_curtain_grid_and_mullion_tools_after_window() {
    let mut h = app(PLAIN);
    press(&mut h, "Architecture");
    let x = |h: &Harness<BimApp>, label: &str| h.get_by_label(label).rect().left();
    assert!(x(&h, "Window") < x(&h, "Curtain Grid") && x(&h, "Curtain Grid") < x(&h, "Mullion") && x(&h, "Mullion") < x(&h, "Column"));
    press(&mut h, "Curtain Grid");
    assert_eq!(h.state().tool, Tool::CurtainGrid);
    assert_eq!(h.state().context().as_deref(), Some("Modify | Place Curtain Grid"));
    assert!(h.query_by_label("All Segments").is_some(), "its Placement");
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
}

#[test]
fn the_curtain_grid_tool_previews_a_line_square_to_the_edge_under_the_cursor_snaps_it_and_a_click_places_it() {
    let (mut h, _) = elevation(PLAIN);
    h.state_mut().start_tool(Tool::CurtainGrid);
    h.run();
    // Along the base: a standing line, at the third of the panel.
    hover(&mut h, on(2010.0, 40.0));
    let aim = h.state().curtain_tools.aimed.expect("a line previewed");
    assert!(aim.vertical && aim.at == 2000.0 && aim.snap == Some(Snap::Third), "{aim:?}");
    assert_eq!((aim.before, aim.after), (0.0, 6000.0), "its temporary dimensions' ends");
    click(&mut h, on(2010.0, 40.0));
    assert_eq!(lines(&h), [(true, 2000.0)], "{}", h.state().status.text);
    assert_eq!(h.state().tool, Tool::CurtainGrid, "the tool stays for the next line");
    // Beside the wall's end: a lying line, at the middle.
    hover(&mut h, on(30.0, 1490.0));
    let aim = h.state().curtain_tools.aimed.expect("a line previewed");
    assert!(!aim.vertical && aim.at == 1500.0 && aim.snap == Some(Snap::Midpoint), "{aim:?}");
    click(&mut h, on(30.0, 1490.0));
    assert_eq!(lines(&h), [(true, 2000.0), (false, 1500.0)]);
    // Between the line placed and the wall's end, away from any snap: where the cursor is, to the millimetre.
    hover(&mut h, on(3170.0, 2980.0));
    let aim = h.state().curtain_tools.aimed.expect("a line previewed");
    assert!(aim.vertical && aim.snap.is_none() && (aim.at - 3170.0).abs() < 30.0, "{aim:?}");
    assert_eq!((aim.before, aim.after), (2000.0, 6000.0));
    // Off the wall: no line, and a click says so.
    hover(&mut h, on(-600.0, 1500.0));
    assert!(h.state().curtain_tools.aimed.is_none());
    key(&mut h, Key::Escape);
    assert_eq!(h.state().tool, Tool::Modify);
    assert_eq!(ids(&h, "curtainpanel").len(), 4, "a panel for each cell");
}

#[test]
fn in_a_plan_the_curtain_grid_tool_places_a_standing_line_on_the_wall_clicked() {
    let mut h = app(PLAIN);
    h.state_mut().start_tool(Tool::CurtainGrid);
    h.run();
    hover(&mut h, P2(3020.0, 0.0));
    let aim = h.state().curtain_tools.aimed.expect("a line previewed");
    assert!(aim.vertical && aim.at == 3000.0 && aim.snap == Some(Snap::Midpoint), "{aim:?}");
    click(&mut h, P2(3020.0, 0.0));
    assert_eq!(lines(&h), [(true, 3000.0)], "{}", h.state().status.text);
}

#[test]
fn in_the_3d_view_the_curtain_grid_tool_places_a_line_on_the_panel_under_the_cursor() {
    let mut h = app(PLAIN);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run_steps(4);
    h.state_mut().start_tool(Tool::CurtainGrid);
    h.run();
    let at = h.state().screen3([2000.0, -12.5, 60.0]);
    h.hover_at(at);
    h.run();
    let aim = h.state().curtain_tools.aimed.expect("a line previewed");
    assert!(aim.vertical && aim.at == 2000.0, "{aim:?}");
    click_at(&mut h, at);
    assert_eq!(lines(&h), [(true, 2000.0)], "{}", h.state().status.text);
}

#[test]
fn the_mullion_tool_puts_mullions_on_the_grid_line_the_segment_or_every_grid_line() {
    let (mut h, wall) = elevation(PLAIN);
    grid_line(&mut h, wall, true, 2000.0);
    grid_line(&mut h, wall, false, 1500.0);
    h.state_mut().start_tool(Tool::Mullion);
    h.run();
    assert_eq!(h.state().context().as_deref(), Some("Modify | Place Mullion"));
    for label in ["Grid Line", "Grid Line Segment", "All Grid Lines"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    // The Type Selector lists the mullion types under their families.
    press(&mut h, "Type Selector");
    for label in ["Rectangular Mullion", "Circular Mullion", "50 x 150mm"] {
        assert!(h.query_all_by_label(label).count() >= 1, "{label}");
    }
    key(&mut h, Key::Escape);
    h.state_mut().start_tool(Tool::Mullion);
    h.run();
    // Grid Line: the whole line clicked.
    click(&mut h, on(2000.0, 700.0));
    let all = mullions(&h);
    assert_eq!(all.len(), 2, "{}", h.state().status.text);
    assert!(all.iter().all(|x| x.vertical && x.at == 2000.0), "{all:?}");
    assert_eq!(h.state().tool, Tool::Mullion);
    // Grid Line Segment: the one segment clicked.
    press(&mut h, "Grid Line Segment");
    assert_eq!(h.state().curtain_tools.on, On::Segment);
    click(&mut h, on(1000.0, 1500.0));
    let lying: Vec<_> = mullions(&h).into_iter().filter(|x| !x.vertical).collect();
    assert_eq!(lying.len(), 1, "{}", h.state().status.text);
    assert!(lying[0].at == 1500.0 && lying[0].to <= 2000.0, "{lying:?}");
    // All Grid Lines: every segment of the wall, its borders too.
    press(&mut h, "All Grid Lines");
    click(&mut h, on(4000.0, 2200.0));
    let m = h.state().session.model.clone();
    assert_eq!(mullions(&h).len(), curtain::grid(&m, wall).unwrap().segments().len(), "{}", h.state().status.text);
}

#[test]
fn shift_puts_a_mullion_on_the_one_segment_and_ctrl_on_all_grid_lines() {
    let (mut h, wall) = elevation(PLAIN);
    grid_line(&mut h, wall, true, 2000.0);
    grid_line(&mut h, wall, false, 1500.0);
    h.state_mut().start_tool(Tool::Mullion);
    h.run();
    click_with(&mut h, on(2000.0, 700.0), Modifiers::SHIFT);
    assert_eq!(mullions(&h).len(), 1, "{}", h.state().status.text);
    click_with(&mut h, on(4000.0, 2200.0), Modifiers::COMMAND);
    let m = h.state().session.model.clone();
    assert_eq!(mullions(&h).len(), curtain::grid(&m, wall).unwrap().segments().len());
}

#[test]
fn add_remove_segments_joins_two_panels_puts_the_segment_back_and_says_why_a_panel_must_be_a_rectangle() {
    let (mut h, wall) = elevation(PLAIN);
    let line = grid_line(&mut h, wall, true, 2000.0);
    grid_line(&mut h, wall, false, 1500.0);
    assert_eq!(ids(&h, "curtainpanel").len(), 4);
    // A grid line selected: Modify | Curtain Wall Grids, with Add/Remove Segments.
    click(&mut h, on(2000.0, 2300.0));
    assert_eq!(h.state().selection, [line]);
    assert_eq!(h.state().context().as_deref(), Some("Modify | Curtain Wall Grids"));
    press(&mut h, "Add/Remove Segments");
    assert_eq!(h.state().tool, Tool::GridSegments(wall));
    let merged = |h: &Harness<BimApp>| h.state().session.model.wall(wall).unwrap().curtain.merged.len();
    // The lower segment of the standing line: gone, the two lower panels one.
    click(&mut h, on(2000.0, 700.0));
    assert_eq!((merged(&h), ids(&h, "curtainpanel").len()), (1, 3), "{}", h.state().status.text);
    // Removing the left segment of the lying line alone would leave an L: the engine's own words, in the status bar.
    click(&mut h, on(1000.0, 1500.0));
    assert!(h.state().status.error && h.state().status.text.contains("not a rectangle"), "{}", h.state().status.text);
    assert_eq!(merged(&h), 1);
    // The removed segment clicked again is put back.
    click(&mut h, on(2000.0, 700.0));
    assert_eq!((merged(&h), ids(&h, "curtainpanel").len()), (0, 4), "{}", h.state().status.text);
    assert_eq!(h.state().tool, Tool::GridSegments(wall), "the tool stays");
    // A click in empty space finishes.
    click(&mut h, on(-700.0, 1500.0));
    assert_eq!(h.state().tool, Tool::Modify);
    // The wall's own lines are no elements: the button is there for the wall and for a panel too.
    for id in [wall, ids(&h, "curtainpanel")[0]] {
        h.state_mut().selection = vec![id];
        h.state_mut().raise_context();
        h.run();
        assert!(h.query_by_label("Add/Remove Segments").is_some());
    }
}

#[test]
fn a_selected_grid_line_moves_by_its_temporary_dimension_typed_by_a_drag_and_goes_with_delete() {
    let (mut h, wall) = elevation(PLAIN);
    let line = grid_line(&mut h, wall, true, 2000.0);
    let at = |h: &Harness<BimApp>| lines(h).first().map(|l| l.1);
    // A click on it selects it, along its whole length.
    click(&mut h, on(2000.0, 2300.0));
    assert_eq!(h.state().selection, [line]);
    // Its distance to the wall's start, and to its end.
    assert!(h.query_by_label("Grid line distance 2000").is_some() && h.query_by_label("Grid line distance 4000").is_some());
    press(&mut h, "Grid line distance 2000");
    h.run();
    // The dimension is a field now, with the keyboard.
    use egui_kittest::kittest::NodeT as _;
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    h.query_all_by_role(Role::TextInput).find(|n| n.accesskit_node().is_focused()).expect("the dimension, typed").type_text("2500");
    h.run();
    key(&mut h, Key::Enter);
    assert_eq!(at(&h), Some(2500.0), "{}", h.state().status.text);
    // Dragged along its wall.
    let (a, b) = (h.state().screen(on(2500.0, 1000.0)), h.state().screen(on(3500.0, 1100.0)));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert!(at(&h).is_some_and(|x| (x - 3500.0).abs() < 40.0), "{:?} {}", at(&h), h.state().status.text);
    assert_eq!(h.state().selection, [line], "still selected");
    key(&mut h, Key::Delete);
    assert!(lines(&h).is_empty(), "{}", h.state().status.text);
    assert_eq!(ids(&h, "curtainpanel").len(), 1);
}

#[test]
fn a_pinned_panel_or_mullion_shows_its_pin_in_an_elevation_and_unpinned_takes_a_type_or_is_deleted() {
    let (mut h, _) = elevation(STORE);
    let panel = ids(&h, "curtainpanel")[0];
    h.state_mut().selection = vec![panel];
    h.run();
    let pin = format!("Unpin element {panel}");
    assert!(h.state().session.model.is_pinned(panel));
    press(&mut h, &pin);
    assert!(!h.state().session.model.is_pinned(panel), "{}", h.state().status.text);
    assert!(h.query_by_label(&pin).is_none(), "no pin on what is free");
    press(&mut h, "Type Selector");
    press(&mut h, "Solid");
    assert_eq!(field(&h, panel, "type"), json!("Solid"), "{}", h.state().status.text);
    // A mullion: pinned it stays, unpinned Delete takes it.
    let mullion = ids(&h, "mullion")[0];
    let count = ids(&h, "mullion").len();
    h.state_mut().selection = vec![mullion];
    h.run();
    key(&mut h, Key::Delete);
    assert_eq!(ids(&h, "mullion").len(), count, "pinned");
    h.state_mut().selection = vec![mullion];
    h.run();
    press(&mut h, &format!("Unpin element {mullion}"));
    key(&mut h, Key::Delete);
    assert_eq!(ids(&h, "mullion").len(), count - 1, "{}", h.state().status.text);
}

#[test]
fn in_the_3d_view_a_curtain_wall_is_picked_first_and_tab_goes_on_to_the_panel_under_the_cursor() {
    let mut h = app(STORE);
    let wall = ids(&h, "wall")[0];
    h.state_mut().open_sheet(Sheet::View3D);
    h.run_steps(4);
    // The middle of a pane of glass.
    let m = h.state().session.model.clone();
    let panel = ids(&h, "curtainpanel")[0];
    let Some(Element::CurtainPanel(x)) = m.elements.get(panel) else { panic!() };
    let [s0, s1, z0, z1] = x.span;
    let at = h.state().screen3([(s0 + s1) / 2.0, -12.5, (z0 + z1) / 2.0]);
    h.hover_at(at);
    h.run();
    assert_eq!(h.state().canvas.hovered, Some(wall), "the wall first, as in a plan");
    key(&mut h, Key::Tab);
    assert_eq!(h.state().canvas.hovered, Some(panel), "Tab: the panel");
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [panel]);
    // Moved off and back, a click takes the wall.
    key(&mut h, Key::Escape);
    h.hover_at(at + vec2(300.0, 300.0));
    h.run();
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [wall]);
}

#[test]
fn in_the_3d_view_tab_reaches_a_hand_made_grid_line_and_a_drag_moves_it_along_its_wall() {
    let mut h = app(PLAIN);
    let wall = ids(&h, "wall")[0];
    let line = grid_line(&mut h, wall, true, 2000.0);
    h.state_mut().open_sheet(Sheet::View3D);
    h.run_steps(4);
    let camera = h.state().orbit.camera;
    let at = h.state().screen3([2000.0, -12.5, 1500.0]);
    h.hover_at(at);
    h.run();
    assert_eq!(h.state().canvas.hovered, Some(wall), "the wall first");
    key(&mut h, Key::Tab);
    assert_eq!(h.state().canvas.hovered, Some(line), "Tab: the grid line, before the panel");
    click_at(&mut h, at);
    assert_eq!(h.state().selection, [line]);
    // A press on the line and a drag along the wall moves it, and the view is not turned.
    let to = h.state().screen3([3200.0, -12.5, 1500.0]);
    drag_screen(&mut h, at, to, PointerButton::Primary);
    let now = lines(&h)[0].1;
    assert!((now - 3200.0).abs() < 60.0, "{now} {}", h.state().status.text);
    assert_eq!(h.state().orbit.camera, camera, "the drag was the line's, not the view's");
}

#[test]
fn in_an_elevation_tab_reaches_the_grid_line_under_a_mullion() {
    let (mut h, wall) = elevation(PLAIN);
    let line = grid_line(&mut h, wall, true, 2000.0);
    h.state_mut().run("mullion", json!({"wall": wall, "on": "grid_line", "at": [2000.0, 1500.0], "vertical": true})).unwrap();
    h.run();
    let mullion = ids(&h, "mullion")[0];
    hover(&mut h, on(2000.0, 1500.0));
    assert_eq!(h.state().canvas.hovered, Some(mullion), "the mullion the picture shows");
    key(&mut h, Key::Tab);
    assert_eq!(h.state().canvas.hovered, Some(line), "Tab: the grid line it lies on");
    click(&mut h, on(2000.0, 1500.0));
    assert_eq!(h.state().selection, [line]);
}

#[test]
fn the_right_click_menu_selects_the_panels_and_the_mullions_of_a_curtain_wall() {
    let mut h = app(STORE);
    let wall = ids(&h, "wall")[0];
    let (panels, mullions) = (ids(&h, "curtainpanel"), ids(&h, "mullion"));
    let offered = |h: &Harness<BimApp>| -> Vec<Among> {
        h.state()
            .context_items()
            .into_iter()
            .flatten()
            .filter_map(|i| match i.action {
                Action::SelectCurtain(among) if i.enabled => Some(among),
                _ => None,
            })
            .collect()
    };
    h.state_mut().selection = vec![panels[0]];
    assert_eq!(offered(&h), [Among::PanelsVertical, Among::PanelsHorizontal, Among::PanelsHost]);
    h.state_mut().act(Action::SelectCurtain(Among::PanelsHost));
    assert_eq!(h.state().selection, panels);
    // Along a grid: the panels in the first one's column, and in its row.
    let span = |h: &Harness<BimApp>, id: Id| match h.state().session.model.elements.get(id) {
        Some(Element::CurtainPanel(x)) => x.span,
        _ => panic!(),
    };
    let first = span(&h, panels[0]);
    for (among, k) in [(Among::PanelsVertical, 0), (Among::PanelsHorizontal, 2)] {
        h.state_mut().selection = vec![panels[0]];
        h.state_mut().act(Action::SelectCurtain(among));
        let want: Vec<Id> = panels.iter().copied().filter(|id| span(&h, *id)[k] == first[k]).collect();
        assert_eq!(h.state().selection, want, "{among:?}");
    }
    // A wall: its mullions, all, inside or on its border.
    h.state_mut().selection = vec![wall];
    assert!(offered(&h).contains(&Among::MullionsHost) && offered(&h).contains(&Among::PanelsHost));
    h.state_mut().act(Action::SelectCurtain(Among::MullionsHost));
    assert_eq!(h.state().selection, mullions);
    let border =
        |h: &Harness<BimApp>, id: Id| matches!(h.state().session.model.elements.get(id), Some(Element::Mullion(x)) if x.border != 0);
    h.state_mut().act(Action::SelectCurtain(Among::MullionsBorder));
    assert!(!h.state().selection.is_empty() && h.state().selection.iter().all(|id| border(&h, *id)));
    h.state_mut().selection = vec![wall];
    h.state_mut().act(Action::SelectCurtain(Among::MullionsInside));
    assert!(!h.state().selection.is_empty() && h.state().selection.iter().all(|id| !border(&h, *id)));
}

#[test]
fn pick_walls_takes_a_curtain_wall_by_its_panel_along_its_centre_line() {
    let mut h = app(&format!("{BOX}\n{STORE}"));
    let m = h.state().session.model.clone();
    let panel = ids(&h, "curtainpanel")[0];
    let level = m.levels()[0].0;
    h.state_mut().start_sketch(crate::sketch::Of::Floor(level), Vec::new());
    h.state_mut().tool = Tool::SketchPick;
    h.state_mut().sketch_click(&m, P2(3000.0, 0.0), P2(3000.0, 0.0), 50.0, Some(panel), Modifiers::NONE);
    let sk = h.state().sketch.as_ref().unwrap();
    assert_eq!(sk.lines.len(), 1, "a line along the curtain wall");
}

// ---------- what the core gives the tools ----------

#[test]
fn every_grid_line_of_a_curtain_wall_is_given_in_the_model_and_a_hand_made_one_is_found_along_its_whole_length() {
    let mut h = app(STORE);
    let wall = ids(&h, "wall")[0];
    let line = grid_line(&mut h, wall, true, 700.0);
    let m = h.state().session.model.clone();
    let marks = curtain_grid::marks(&m, wall);
    let g = curtain::grid(&m, wall).unwrap();
    assert_eq!(marks.len(), g.u.len() + g.v.len(), "the type's lines, the borders and the hand-made one");
    let made: Vec<_> = marks.iter().filter(|k| k.line.is_some()).collect();
    assert_eq!(made.len(), 1);
    assert_eq!((made[0].line, made[0].vertical, made[0].at), (Some(line), true, 700.0));
    assert_eq!(made[0].pts, [[700.0, 0.0, 0.0], [700.0, 0.0, 3000.0]], "from the wall's base to its top");
    assert_eq!(curtain_grid::outline(&m, wall).len(), 4);
    // Seen from the south, a px a mm: found anywhere along it, a mullion on it or not, and before the type's lines.
    let project = |q: [f64; 3]| (q[0], -q[2]);
    for z in [10.0, 1500.0, 2990.0] {
        let found = curtain_grid::line_near(&marks, &project, (703.0, -z), 6.0);
        assert_eq!(found.and_then(|k| k.line), Some(line), "at {z}");
    }
    assert!(curtain_grid::line_near(&marks, &project, (760.0, -1500.0), 6.0).is_none(), "nothing near");
    assert!(curtain_grid::line_near(&marks, &project, (0.0, -1500.0), 6.0).is_none(), "a border is no grid line to pick");
    // A place of the wall in an elevation of it.
    let v = bimcraft_core::model::Section {
        name: String::new(),
        start: P2(-1000.0, -1000.0),
        end: P2(7000.0, -1000.0),
        depth: None,
        crop: None,
        graphics: Default::default(),
    };
    let look = bimcraft_core::graphics::Look::of_3d(&m);
    let place = curtain_grid::in_section(&m, &v, &look, P2(1700.0, 1200.0), 50.0).unwrap();
    assert_eq!((place.wall, place.s, place.z), (wall, 700.0, 1200.0));
    assert!(curtain_grid::in_section(&m, &v, &look, P2(500.0, 1200.0), 50.0).is_none(), "beside the wall");
    assert_eq!(curtain_grid::to_section(&v, [700.0, 0.0, 3000.0]), P2(1700.0, 3000.0));
    // Dragged 100 px to the right in that view: 100 mm along the wall, and never past the lines either side.
    assert_eq!(curtain_grid::dragged(&m, line, &project, (100.0, 3.0)), Some(800.0));
    let far = curtain_grid::dragged(&m, line, &project, (9000.0, 0.0)).unwrap();
    assert!(far < g.u.iter().map(|l| l.at).find(|x| *x > 700.5).unwrap(), "{far}");
}

#[test]
fn panels_joined_with_no_mullion_between_them_show_the_joint_in_an_elevation_and_in_the_3d_view() {
    use bimcraft_core::view3d::Scene;
    let h = app(&format!("{GLAZING}\n{SOUTH}"));
    let m = h.state().session.model.clone();
    let (wall, section) = (ids(&h, "wall")[0], ids(&h, "section")[0]);
    let g = curtain::grid(&m, wall).unwrap();
    assert!(g.u.len() > 2 && ids(&h, "mullion").is_empty(), "panels side by side, no mullions");
    let joint = g.u[1].at;
    // The elevation: ink down the joint between two panels, none in the middle of a panel.
    let view = bimcraft_core::render::section_fit(&m, section, 900, 600);
    let picture = bimcraft_core::render::section_picture(&m, section, view, &[]).unwrap();
    let dark = |x: f64, z: f64| {
        let (px, py) = view.px(P2(1000.0 + x, z));
        (-2..=2).any(|d| {
            let c = picture.color(f64::from(px) + f64::from(d), f64::from(py));
            u32::from(c.0) + u32::from(c.1) + u32::from(c.2) < 300
        })
    };
    assert!(dark(joint, 1500.0), "the joint is drawn");
    assert!(!dark(joint / 2.0, 1500.0), "no line across a panel");
    // The 3D view: two panels' triangles in one plane are told apart, a panel's own are not.
    let scene = Scene::new(&m);
    let panels = ids(&h, "curtainpanel");
    let front = |panel: Id| {
        (0..scene.tris.len())
            .filter(|i| scene.tris[*i].owner == panel)
            .filter(|i| scene.tris[*i].p.iter().all(|q| (q[1] + 12.5).abs() < 0.1))
            .map(|i| (scene.planes[i], scene.edge_plane(i).unwrap()))
            .collect::<Vec<_>>()
    };
    let (a, b) = (front(panels[0]), front(panels[1]));
    assert!(a.len() == 2 && b.len() == 2, "{a:?} {b:?}");
    assert_eq!(a[0].0, b[0].0, "one plane");
    assert_eq!(a[0].1, a[1].1, "a panel's face is one");
    assert_ne!(a[0].1, b[0].1, "two panels' faces are two");
}

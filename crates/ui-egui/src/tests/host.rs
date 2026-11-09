//! The window inside a page that keeps the project (`host.rs`): no files, start page, plugins or control port of its own; Save
//! asks the page; the page puts a newer model in place and runs its agent's commands; it dresses the window in its colours.

use super::*;
use crate::host::{HostLook, Skin};

fn hosted(script: &str) -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(script).unwrap();
    let app = BimApp::hosted_on(s);
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .with_step_dt(0.25)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
    h.run();
    h
}

#[test]
fn a_hosted_window_opens_on_the_first_floor_plan_with_none_of_the_desktops_files_start_page_or_port() {
    let mut h = hosted(BOX);
    assert!(!h.state().home.shown, "no start page");
    assert!(matches!(h.state().sheet, Sheet::Plan(_)), "straight onto a floor plan");
    for gone in ["Home", "Open", "Team"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} is the desktop's");
    }
    assert!(h.query_all_by_label_contains("Agents:").next().is_none(), "no control port");
    press(&mut h, "File");
    for gone in ["New", "Open…", "Open Recent", "Save As…", "Exit"] {
        assert!(h.query_all_by_label_contains(gone).next().is_none(), "File > {gone} is the desktop's");
    }
    assert!(h.query_all_by_label_contains("Settings").next().is_some(), "the window's own settings stay");
    // The desktop's window keeps all of it.
    let mut d = bimcraft(BOX);
    assert!(d.query_by_label("Open").is_some() && d.query_by_label("Team").is_some());
    press(&mut d, "File");
    assert!(d.query_all_by_label_contains("Save As").next().is_some());
}

#[test]
fn save_in_a_hosted_window_asks_the_page_and_opens_no_dialog() {
    let mut h = hosted(BOX);
    assert!(!h.state_mut().take_save_request());
    h.key_press_modifiers(Modifiers::COMMAND, Key::S);
    h.run();
    assert!(h.state_mut().take_save_request(), "Ctrl+S asks the page");
    assert!(!h.state_mut().take_save_request(), "once");
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::S);
    h.run();
    assert!(h.state_mut().take_save_request(), "Save As is Save");
    press(&mut h, "Save");
    assert!(h.state_mut().take_save_request(), "the quick access bar's Save");
    assert!(crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).is_none(), "no file dialog");
    // Saved, the model is no longer unsaved.
    h.state_mut().execute_for_agent("wall", &json!({"points": [[0, 0], [0, 3000]]})).unwrap();
    assert!(h.state().dirty());
    let model = Arc::clone(&h.state().session.model);
    h.state_mut().mark_saved(&model);
    assert!(!h.state().dirty());
}

#[test]
fn the_agents_commands_are_undo_steps_and_a_newer_model_from_the_page_keeps_the_views_open() {
    let mut h = hosted(SAMPLE);
    let walls = |h: &Harness<BimApp>| h.state().session.model.elements.values().filter(|e| matches!(e, Element::Wall(_))).count();
    let before = walls(&h);
    h.state_mut().execute_for_agent("wall", &json!({"points": [[20000, 0], [26000, 0]]})).unwrap();
    h.run();
    assert_eq!(walls(&h), before + 1);
    assert!(h.state_mut().execute_for_agent("no_such_command", &json!({})).is_err());
    assert!(!h.state().status.error, "a refusal is the agent's to read");
    h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
    h.run();
    assert_eq!(walls(&h), before, "the agent's command is undone as one step");

    // The page's newer model: the same project, with a wall more. The 3D view open stays open and in front.
    h.state_mut().open_3d(None);
    h.run();
    assert_eq!(h.state().sheet, Sheet::View3D);
    let mut newer = (*h.state().session.model).clone();
    let mut s = Session::new();
    s.reset(newer.clone(), None);
    s.execute("wall", &json!({"points": [[30000, 0], [36000, 0]]})).unwrap();
    newer = (*s.model).clone();
    h.state_mut().replace_model(newer);
    h.run();
    assert_eq!(walls(&h), before + 1);
    assert_eq!(h.state().sheet, Sheet::View3D, "the view stays");
    assert!(h.state().views.iter().any(|v| matches!(v, Sheet::Plan(_))), "and the plan's tab");
    assert!(!h.state().dirty(), "the page's model is the saved one");
    assert!(h.state().hosted(), "still hosted");
}

#[test]
fn the_page_dresses_the_window_in_its_colours() {
    let mut h = hosted(BOX);
    let look = HostLook {
        dark: false,
        panel: Some([255, 255, 255]),
        text: Some([16, 24, 40]),
        accent: Some([43, 80, 216]),
        canvas: Some([245, 246, 248]),
        skin: Skin { app_bar: Some([247, 248, 250]), app_bar_text: Some([16, 24, 40]), border: Some([228, 231, 236]), ..Skin::default() },
        compact: Some(true),
    };
    h.state_mut().set_host_look(look);
    h.run();
    let p = crate::style::pal();
    assert_eq!(p.panel, egui::Color32::from_rgb(255, 255, 255));
    assert_eq!(p.app_bar, egui::Color32::from_rgb(247, 248, 250));
    assert_eq!(p.border, egui::Color32::from_rgb(228, 231, 236));
    assert_eq!(p.accent, egui::Color32::from_rgb(43, 80, 216));
    assert!(crate::style::contrast(p.app_bar_text, p.app_bar) >= 4.5);
    assert!(crate::style::compact());
    // Dark, the page's dark colours.
    h.state_mut().set_host_look(HostLook { dark: true, panel: Some([23, 28, 37]), ..HostLook::default() });
    h.run();
    assert!(crate::style::dark());
    assert_eq!(crate::style::pal().panel, egui::Color32::from_rgb(23, 28, 37));
}

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
    h.run();
    assert!(h.query_by_label("Unsaved changes").is_none(), "the page says whether it is saved");
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

#[test]
fn the_pages_look_changes_only_what_it_sends_and_keeps_the_persons_own_choices() {
    let mut h = hosted(BOX);
    h.state_mut().set_host_look(HostLook { dark: true, compact: Some(true), ..HostLook::default() });
    h.run();
    assert!(crate::style::compact());
    // The person picks the Light Canvas and the floating cards for themselves.
    (h.state_mut().light_canvas, h.state_mut().compact) = (true, false);
    // The page sends its look again, light now, the same layout as before: the person's choices stay.
    h.state_mut().set_host_look(HostLook { dark: false, compact: Some(true), ..HostLook::default() });
    h.run();
    assert!(!crate::style::dark(), "light or dark is the page's");
    assert!(h.state().light_canvas, "the Light Canvas is the person's");
    assert!(!h.state().compact, "the layout the page asked for already does not undo the person's");
    h.state_mut().set_host_look(HostLook { dark: true, ..HostLook::default() });
    h.run();
    assert!(h.state().light_canvas && !h.state().compact, "a look with no layout keeps it");
    // A layout the page had not asked for is the page's change.
    h.state_mut().set_host_look(HostLook { dark: true, compact: Some(false), ..HostLook::default() });
    h.state_mut().set_host_look(HostLook { dark: true, compact: Some(true), ..HostLook::default() });
    assert!(h.state().compact);
}

/// A room drawn in metres on WALLS, as a DXF.
fn room_dxf() -> Vec<u8> {
    let pairs = |list: &[(i32, &str)]| list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect::<String>();
    let lines: String = [((0, 0), (10, 0)), ((10, 0), (10, 6)), ((10, 6), (0, 6)), ((0, 6), (0, 0))]
        .iter()
        .map(|((ax, ay), (bx, by))| {
            pairs(&[(0, "LINE"), (8, "WALLS"), (10, &ax.to_string()), (20, &ay.to_string()), (11, &bx.to_string()), (21, &by.to_string())])
        })
        .collect();
    format!("0\nSECTION\n2\nENTITIES\n{lines}0\nENDSEC\n0\nEOF\n").into_bytes()
}

/// The ask open now.
fn asked(h: &Harness<BimApp>) -> crate::host::FileRequest {
    h.state().file_request().cloned().expect("the page is asked for a file")
}

#[test]
fn import_cad_in_a_hosted_window_asks_the_page_and_reads_the_drawing_it_gives() {
    let mut h = hosted(BOX);
    let ctx = h.ctx.clone();
    h.state_mut().import_cad_dialog(&ctx, false);
    h.run();
    let r = asked(&h);
    assert!(!r.save && r.multiple, "{r:?}");
    assert_eq!(r.extensions, ["dxf", "dwg"]);
    assert_eq!(r.title.as_deref(), Some("Import CAD"), "the page's picker names what it is for");
    assert!(crate::dialog::BEGUN.with(|b| b.borrow_mut().pop()).is_none(), "no system dialog");
    h.state_mut().give_file("files/doc_1/Survey.dxf", room_dxf());
    h.state_mut().answer_file_request(r.id, Some(vec!["files/doc_1/Survey.dxf".into()]));
    h.run();
    assert!(h.state().file_request().is_none(), "answered");
    press(&mut h, "OK");
    let cads = h.state().session.model.elements.values().filter(|e| matches!(e, Element::CadImport(_))).count();
    assert_eq!(cads, 1, "{}", h.state().status.text);
}

#[test]
fn a_model_the_page_gives_is_linked_and_read_again_after_the_page_puts_the_model_in_place() {
    let mut house = Session::new();
    house.script(SAMPLE).unwrap();
    let text = house.model.to_json().unwrap();
    let mut h = hosted(BOX);
    let ctx = h.ctx.clone();
    h.state_mut().link_model_dialog(&ctx);
    h.run();
    let r = asked(&h);
    assert!(r.extensions.contains(&"rvt".to_string()), "the page reads Revit models itself: {r:?}");
    assert_eq!(r.title.as_deref(), Some("Link Model"));
    // A cancel does nothing.
    h.state_mut().answer_file_request(r.id, None);
    h.run();
    assert!(h.state().file_request().is_none());
    h.state_mut().link_model_dialog(&ctx);
    h.run();
    let r = asked(&h);
    h.state_mut().give_file("files/doc_2/House.bimcraft", text.clone().into_bytes());
    h.state_mut().answer_file_request(r.id, Some(vec!["files/doc_2/House.bimcraft".into()]));
    h.run();
    assert!(h.state().files_wanted().is_empty(), "read: {}", h.state().status.text);
    assert_eq!(h.state().session.model.loaded.len(), 1);
    // The page's model, from its text: the link is there, what it read is not, until the page gives it again.
    let (m, _) = bimcraft_core::model::Model::read(&h.state().session.model.to_json().unwrap()).unwrap();
    h.state_mut().replace_model(m);
    h.run();
    assert_eq!(h.state().files_wanted(), ["files/doc_2/House.bimcraft"]);
    h.state_mut().give_file("files/doc_2/House.bimcraft", text.into_bytes());
    let model = Arc::clone(&h.state().session.model);
    h.state_mut().mark_saved(&model);
    let r = h.state_mut().reload_links().unwrap();
    assert_eq!(r["reloaded"], json!(["House"]), "{r}");
    assert!(h.state().files_wanted().is_empty());
    assert!(!h.state().dirty(), "reading the links is no change of the person's");
}

#[test]
fn export_in_a_hosted_window_asks_the_page_where_and_leaves_what_it_wrote_for_the_page() {
    let mut h = hosted(BOX);
    press(&mut h, "File");
    assert!(h.query_by_label("Open IFC or Revit Model…").is_some(), "the page picks what to open");
    // The submenu's label carries its arrow too.
    h.get_by_label_contains("Export").click();
    h.run();
    press(&mut h, "IFC…");
    let r = asked(&h);
    assert!(r.save, "{r:?}");
    assert_eq!(r.extensions, ["ifc"]);
    assert_eq!(r.title.as_deref(), Some("Export"));
    let name = r.name.clone().unwrap();
    assert!(name.ends_with(".ifc"), "{name}");
    h.state_mut().answer_file_request(r.id, Some(vec![format!("out/{name}")]));
    h.run();
    let written = h.state_mut().take_written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, format!("out/{name}"));
    assert!(String::from_utf8_lossy(&written[0].1).starts_with("ISO-10303-21"));
    assert!(!h.state().has_written(), "taken");
}

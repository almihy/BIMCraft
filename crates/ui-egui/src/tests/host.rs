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
    let mut d = app(BOX);
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

fn first_wall(h: &Harness<BimApp>) -> Id {
    h.state().session.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).unwrap()
}

#[test]
fn the_page_reads_what_the_person_sees_the_selection_the_view_and_where_it_looks() {
    let mut h = hosted(SAMPLE);
    let wall = first_wall(&h);
    h.state_mut().selection = vec![wall];
    h.run();
    let c = h.state().host_context();
    assert_eq!(c["count"], json!(1));
    assert_eq!(c["selection"][0]["id"], json!(wall));
    assert_eq!(c["selection"][0]["category"], json!("wall"));
    assert_eq!(c["selection"][0]["guid"], json!(h.state().session.model.elements.guid(wall).unwrap().to_string()));
    assert_eq!(c["view"]["kind"], json!("floor plan"));
    assert_eq!(c["view"]["name"], json!("Level 1"));
    assert_eq!(c["view"]["level"]["name"], json!("Level 1"));
    // The part of the plan on screen: a box round the house, in its own millimetres, with the zoom.
    assert_eq!(c["camera"]["kind"], json!("floor plan"), "the view's kind");
    let (lo, hi) = (&c["camera"]["visible"]["min"], &c["camera"]["visible"]["max"]);
    assert!(lo[0].as_f64().unwrap() < 0.0 && hi[0].as_f64().unwrap() > 10000.0, "{c}");
    assert!(lo[1].as_f64().unwrap() < hi[1].as_f64().unwrap(), "north is up: {c}");
    assert!(c["camera"]["scale"].as_f64().unwrap() > 0.0);
}

#[test]
fn the_context_changes_with_the_selection_the_view_and_the_camera_and_not_while_nothing_moves() {
    let mut h = hosted(SAMPLE);
    h.run();
    let at_rest = h.state().host_context_key();
    h.run();
    assert_eq!(h.state().host_context_key(), at_rest, "nothing moved");
    let wall = first_wall(&h);
    h.state_mut().selection = vec![wall];
    let picked = h.state().host_context_key();
    assert_ne!(picked, at_rest, "the selection");
    let level = h.state().plan_key().unwrap();
    h.state_mut().canvas.zoom(level, 2.0);
    h.run();
    let zoomed = h.state().host_context_key();
    assert_ne!(zoomed, picked, "the camera");
    h.state_mut().open_3d(None);
    h.run();
    let three = h.state().host_context();
    assert_eq!(three["view"]["kind"], json!("3D view"));
    assert_eq!(three["camera"]["kind"], json!("3D view"));
    assert!(three["camera"]["target"].is_array());
    assert_ne!(h.state().host_context_key(), zoomed, "the view");
}

#[test]
fn the_agent_selects_what_it_means_by_id_or_guid_and_the_window_shows_it() {
    let mut h = hosted(SAMPLE);
    let model = Arc::clone(&h.state().session.model);
    let wall = first_wall(&h);
    let guid = h.state().session.model.elements.guid(wall).unwrap().to_string();
    let r = h.state_mut().host_select(&format!("{wall} {guid} 999999999 not-an-id"), false, false);
    assert_eq!(h.state().selection, vec![wall], "the id and its GUID are one element; what names nothing is left");
    assert_eq!(r["ids"], json!([wall]));
    assert_eq!(r["missing"].as_array().unwrap().len(), 2, "{r}");
    h.run();
    // In place of what was selected, unless added.
    let other =
        h.state().session.model.elements.iter().find(|(id, e)| matches!(e, Element::Door(_)) && **id != wall).map(|(id, _)| *id).unwrap();
    h.state_mut().host_select(&other.to_string(), false, false);
    assert_eq!(h.state().selection, vec![other]);
    h.state_mut().host_select(&wall.to_string(), true, false);
    assert_eq!(h.state().selection, vec![other, wall]);
    // Shown: the plan that has it opens.
    h.state_mut().open_3d(None);
    h.run();
    h.state_mut().host_select(&wall.to_string(), false, true);
    h.run();
    assert!(matches!(h.state().sheet, Sheet::Plan(_)), "the plan opened over the 3D view");
    // Nothing clears it.
    let r = h.state_mut().host_select("", false, false);
    assert!(h.state().selection.is_empty());
    assert_eq!(r["selected"], json!(0));
    assert!(Arc::ptr_eq(&h.state().session.model, &model), "selecting changes nothing in the model");
}

#[test]
fn a_selected_element_deleted_since_is_not_counted() {
    let mut h = hosted(SAMPLE);
    let wall = first_wall(&h);
    h.state_mut().selection = vec![wall, 999_999_999];
    let c = h.state().host_context();
    assert_eq!(c["selection"].as_array().unwrap().len(), 1, "{c}");
    assert_eq!(c["count"], json!(1), "what survives, not what the selection still names: {c}");
}

#[test]
fn a_section_and_a_ceiling_plan_say_their_own_kind_in_their_camera() {
    let mut h = hosted(SAMPLE);
    let section = h.state().session.model.rare("section").into_iter().next().unwrap();
    h.state_mut().host_open_view(&section.to_string(), false).unwrap();
    h.run();
    let c = h.state().host_context();
    assert_eq!(c["view"]["kind"], json!("section"), "{c}");
    assert_eq!(c["camera"]["kind"], json!("section"), "{c}");
    h.state_mut().host_open_view("Level 2", true).unwrap();
    h.run();
    let c = h.state().host_context();
    assert_eq!(c["camera"]["kind"], json!("ceiling plan"), "{c}");
}

#[test]
fn a_selection_that_found_nothing_is_what_it_says_whatever_was_selected_before() {
    let mut h = hosted(SAMPLE);
    let wall = first_wall(&h);
    h.state_mut().selection = vec![wall];
    // Added to: what was selected stays, and the reply says so.
    let r = h.state_mut().host_select("999999999", true, false);
    assert_eq!(h.state().selection, vec![wall]);
    assert_eq!(r["selected"], json!(1), "{r}");
    assert_eq!(r["ids"], json!([]), "{r}");
    assert_eq!(r["missing"], json!(["999999999"]), "{r}");
    // In place of: nothing was found, so nothing is selected, and the reply says that and not the old count.
    let r = h.state_mut().host_select("999999999", false, false);
    assert!(h.state().selection.is_empty());
    assert_eq!(r["selected"], json!(0), "{r}");
    assert_eq!(r["missing"], json!(["999999999"]), "{r}");
    // What only the library has is no element: said in `missing` too, and the selection is cleared all the same.
    let ty = h.state().session.model.library.entries().next().map(|(id, _)| id).unwrap();
    h.state_mut().selection = vec![wall];
    let r = h.state_mut().host_select(&ty.to_string(), false, false);
    assert!(h.state().selection.is_empty());
    assert_eq!(r["selected"], json!(0), "{r}");
    assert_eq!(r["ids"], json!([]), "{r}");
    assert_eq!(r["missing"].as_array().unwrap().len(), 1, "the library entry: {r}");
}

#[test]
fn the_agent_opens_a_view_by_its_name_number_or_id() {
    let mut h = hosted(SAMPLE);
    let model = Arc::clone(&h.state().session.model);
    let c = h.state_mut().host_open_view("3d", false).unwrap();
    h.run();
    assert_eq!(h.state().sheet, Sheet::View3D);
    assert_eq!(c["view"]["kind"], json!("3D view"));
    let c = h.state_mut().host_open_view("level 2", true).unwrap();
    assert!(matches!(h.state().sheet, Sheet::Ceiling(_)), "{c}");
    assert_eq!(c["view"]["kind"], json!("ceiling plan"));
    assert_eq!(c["view"]["name"], json!("Level 2"));
    let c = h.state_mut().host_open_view("Level 2", false).unwrap();
    assert_eq!(c["view"]["kind"], json!("floor plan"));
    let section = h.state().session.model.rare("section").into_iter().next().unwrap();
    let c = h.state_mut().host_open_view(&section.to_string(), false).unwrap();
    assert_eq!(c["view"]["kind"], json!("section"), "{c}");
    let c = h.state_mut().host_open_view("A101", false).unwrap();
    assert_eq!(c["view"]["kind"], json!("sheet"), "{c}");
    assert!(c["view"]["number"].is_string());
    assert_eq!(c["camera"], Value::Null, "a sheet has no camera");
    let e = h.state_mut().host_open_view("No such view", false).unwrap_err();
    assert!(e.contains("No view is named `No such view`") && e.contains("Level 1"), "{e}");
    h.run();
    assert!(Arc::ptr_eq(&h.state().session.model, &model), "opening views changes nothing in the model");
}

#[test]
fn a_page_holds_back_the_keys_the_editor_takes_and_leaves_the_rest_to_the_browser() {
    use crate::host::editor_takes_key as takes;
    for k in ["Tab", " ", "Backspace", "Delete", "ArrowLeft", "ArrowDown", "Home", "PageUp", "F1", "F8", "F10"] {
        assert!(takes(k, false), "{k}");
    }
    for k in ["a", "A", "d", "z", "y", "s", "k", ","] {
        assert!(takes(k, true), "Ctrl+{k}");
        assert!(!takes(k, false), "{k} alone is typed");
    }
    for k in ["c", "v", "x", "r", "w", "t", "l", "F5", "F11", "F12", "Escape", "Enter"] {
        assert!(!takes(k, true), "Ctrl+{k} stays the browser's");
    }
    assert!(!takes("F5", false) && !takes("F11", false) && !takes("F12", false));
    assert!(!takes("Escape", false) && !takes("Enter", false));
}

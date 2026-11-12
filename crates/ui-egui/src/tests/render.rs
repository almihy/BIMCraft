//! Render (`host.rs`): a hosted window's Project Browser offers Render… on each 3D view and camera, and the View tab's
//! Presentation panel offers Render for the 3D view in front; each asks the page to render that view (the page opens its
//! own Render dialog). The desktop's window has no renderer to ask, and offers neither.

use super::*;
use egui_kittest::kittest::NodeT;

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

/// A right click on a view's entry in the Project Browser, found by its name first: the 3D views are below the browser's fold.
fn right_click_in_browser(h: &mut Harness<BimApp>, label: &str) {
    type_into(h, "Search the Project Browser", label);
    let entry = h
        .get_all_by_label(label)
        .filter(|n| n.rect().center().x < 290.0 && n.accesskit_node().role() == Role::Button)
        .min_by(|a, b| a.rect().center().y.total_cmp(&b.rect().center().y))
        .map(|n| n.accesskit_node().id())
        .unwrap();
    h.get_by(|n| n.id() == entry).click_secondary();
    h.run();
}

/// A camera named `name` looking into the box, made as the agent makes one: its id.
fn camera(h: &mut Harness<BimApp>, name: &str) -> Id {
    let made = h
        .state_mut()
        .execute_for_agent("camera", &json!({"eye": [-5000.0, -4000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0], "name": name}))
        .unwrap();
    h.run();
    made["id"].as_u64().unwrap()
}

#[test]
fn render_in_the_project_browser_asks_the_page_to_render_that_camera_or_the_default_3d_view() {
    let mut h = hosted(BOX);
    let id = camera(&mut h, "Entrance");
    assert_eq!(h.state_mut().take_render_request(), None);
    right_click_in_browser(&mut h, "Entrance");
    press(&mut h, "Render…");
    let guid = h.state().session.model.elements.guid(id);
    assert_eq!(
        h.state_mut().take_render_request(),
        Some(crate::host::RenderRequest { view: "Entrance".into(), id: Some(id), guid, camera: true })
    );
    assert_eq!(h.state_mut().take_render_request(), None, "once");
    // The default {3D}, before it is ever made: by its name alone, and nothing is made for it.
    right_click_in_browser(&mut h, "{3D}");
    press(&mut h, "Render…");
    assert_eq!(
        h.state_mut().take_render_request(),
        Some(crate::host::RenderRequest { view: "{3D}".into(), id: None, guid: None, camera: false })
    );
    assert!(h.state().session.model.default_3d().is_none());
    // A camera's menu keeps its Delete beside Render….
    right_click_in_browser(&mut h, "Entrance");
    assert!(h.query_by_label("Delete").is_some() && h.query_by_label("Render…").is_some());
}

#[test]
fn the_view_tabs_render_asks_for_the_3d_view_in_front() {
    let mut h = hosted(BOX);
    let id = camera(&mut h, "Entrance");
    press(&mut h, "View");
    assert!(h.query_all_by_label_contains("Presentation").next().is_some(), "Revit's panel");
    // On a plan there is no 3D view to render: the button is there, and does nothing.
    press(&mut h, "Render");
    assert_eq!(h.state_mut().take_render_request(), None);
    // The camera open in the 3D tab: Render asks for it.
    browse(&mut h, "Entrance");
    assert_eq!(h.state().sheet, Sheet::View3D);
    press(&mut h, "View");
    press(&mut h, "Render");
    let asked = h.state_mut().take_render_request().unwrap();
    assert_eq!((asked.view.as_str(), asked.id, asked.camera), ("Entrance", Some(id), true));
}

#[test]
fn the_desktops_window_offers_no_render() {
    let mut d = app(BOX);
    d.state_mut().execute_for_agent("camera", &json!({"eye": [-5000.0, -4000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0]})).unwrap();
    d.run();
    right_click_in_browser(&mut d, "Camera 1");
    assert!(d.query_by_label("Delete").is_some() && d.query_by_label("Render…").is_none());
    d.key_press(Key::Escape);
    d.run();
    press(&mut d, "View");
    assert!(d.query_all_by_label_contains("Presentation").next().is_none() && d.query_by_label("Render").is_none());
}

#[test]
fn render_is_named_in_every_interface_language() {
    let words = [
        "Presentation",
        "Render",
        "Render…",
        "Render: a picture of this 3D view or camera, made by the renderer of the page the model is kept in",
    ];
    for lang in crate::lang::Lang::ALL.into_iter().filter(|l| *l != crate::lang::Lang::English) {
        for english in words {
            assert!(crate::lang::translated_in(lang, english).is_some(), "{english:?} has no {} word", lang.code());
        }
    }
    assert_eq!(crate::lang::translated_in(crate::lang::Lang::German, "Render…"), Some("Rendern…"));
}

//! The Checks palette (`checks.rs`): the hosting page's checks of the model, flagged first; a click on a flag selects what it
//! names and zooms to it; the status bar counts the flags and opens the palette; Open Rule asks the page.

use super::*;
use crate::dock::{Palette, Side};
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

/// A box on Level 1 and a wall far off on Level 2.
fn two_levels() -> String {
    format!("{BOX}\nwall {{\"points\": [[20000, 20000], [23000, 20000]], \"level\": \"Level 2\"}}")
}

/// Checks shaped as Arqiflow's `GET /api/proposals/{id}/checks` answers: a firm rule passing, a client requirement failing on
/// the far wall, a design check's warning, a rule nobody checks automatically, and a waived one.
fn checks(far: Id) -> String {
    json!({
        "proposal": "Option A", "ref": "draft",
        "summary": {"pass": 1, "fail": 1, "warn": 1, "guidance": 1, "waived": 1, "error": 0, "clean": false},
        "integrity": [], "value_conflicts": [],
        "results": [
            {"rule_id": "kn_pass", "version": 1, "title": "Corridor width", "scope": "company", "hard": true,
             "text": "Corridors are at least 1.2 m wide.", "check_summary": "", "findings": [], "status": "pass"},
            {"rule_id": "kn_fail", "version": 1, "title": "Bedroom area", "scope": "client", "hard": true,
             "text": "Each bedroom is at least 12 m².", "check_summary": "",
             "findings": [{"message": "Bedroom 2 is 9.6 m², under 12 m²", "elements": [format!("w{far}")]}], "status": "fail"},
            {"rule_id": "parking", "version": 1, "title": "Parking can be driven to", "scope": "design", "hard": false,
             "text": "", "check_summary": "", "findings": [{"message": "Two stalls have no way out", "elements": []}], "status": "warn"},
            {"rule_id": "kn_guide", "version": 1, "title": "Daylight in living rooms", "scope": "company", "hard": false,
             "text": "Living rooms face south where they can.", "check_summary": "", "findings": [], "status": "guidance"},
            {"rule_id": "kn_waived", "version": 1, "title": "Balcony depth", "scope": "project", "hard": true,
             "text": "Balconies are at least 1.5 m deep.", "check_summary": "", "findings": [], "status": "waived",
             "waiver": {"by": "Sara", "reason": "The client asked for Juliet balconies", "at": "2026-11-10T10:00:00Z"}}
        ]
    })
    .to_string()
}

/// A click on what is named, scrolled into the palette's view first: the palette under Properties is short.
fn tap(h: &mut Harness<BimApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

/// The Checks in front of their stack, as a click on the status bar's count puts them.
fn front(h: &mut Harness<BimApp>) {
    h.state_mut().docking.activate(Palette::Checks);
    h.run();
}

fn docking<'a>(h: &'a Harness<'_, BimApp>) -> &'a crate::dock::Docking {
    &h.state().docking
}

#[test]
fn a_hosted_window_has_the_checks_palette_open_and_the_desktops_has_none() {
    let mut h = hosted(BOX);
    // A sidebar of its own down the right side, the whole height: no tab of anything's, and nothing else there.
    assert_eq!(docking(&h).docked(Palette::Checks), Some(Side::Right));
    assert_eq!(docking(&h).side(Side::Right).len(), 1);
    assert_eq!(docking(&h).stack_of(Palette::Checks).unwrap().tabs, [Palette::Checks]);
    assert_eq!(docking(&h).stack_of(Palette::Browser).unwrap().tabs, [Palette::Browser], "the Project Browser as it was");
    assert!(h.query_by_label("Checking the model…").is_some(), "until the page gives its checks");
    assert!(h.query_all_by_label_contains("Checks:").next().is_none(), "no count before there are checks");
    // The desktop's window has no page to check its model: no palette, and none to switch on.
    let mut d = app(BOX);
    assert!(!d.state().docking.shown(Palette::Checks));
    press(&mut d, "View");
    press(&mut d, "User Interface");
    assert!(d.query_by_role_and_label(Role::CheckBox, "Checks").is_none());
    assert!(d.query_by_role_and_label(Role::CheckBox, "Properties").is_some());
    // The hosted window's View > User Interface switches it like the others.
    press(&mut h, "View");
    press(&mut h, "User Interface");
    pick(&mut h, Role::CheckBox, "Checks");
    assert!(!docking(&h).shown(Palette::Checks));
}

#[test]
fn the_flags_come_first_and_a_click_on_one_selects_and_zooms_to_what_it_names() {
    let mut h = hosted(&two_levels());
    let far = *ids(&h, "wall").last().unwrap();
    h.state_mut().set_checks(&checks(far)).unwrap();
    front(&mut h);
    // Flagged first, failed before the warning, then waived, not checked and passed.
    let order: Vec<&str> = h.state().checks().unwrap().rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(order, ["Bedroom area", "Parking can be driven to", "Balcony depth", "Daylight in living rooms", "Corridor width"]);
    assert!(h.query_by_label("2 flags").is_some() && h.query_by_label("1 passed.").is_some());
    assert!(h.query_by_label("Checks: 2 flags").is_some(), "the status bar's count");
    let level1 = match h.state().sheet {
        Sheet::Plan(l) => l,
        s => panic!("{s:?}"),
    };
    // A click on the flag: the far wall selected, its level's plan open and zoomed to it.
    tap(&mut h, "Bedroom area — Failed");
    assert_eq!(h.state().selection, [far]);
    assert!(matches!(h.state().sheet, Sheet::Plan(l) if l != level1), "Level 2's plan");
    h.run();
    let at = h.state().screen(P2(21500.0, 20000.0));
    assert!(h.state().view_rect.contains(at), "the wall in the view: {at:?} in {:?}", h.state().view_rect);
    // Its detail, one click away: the rule as written, what it found, whose it is, and Open Rule, which asks the page.
    assert!(h.query_by_label("Each bedroom is at least 12 m².").is_some());
    assert!(h.query_by_label("Client requirement").is_some());
    assert_eq!(h.state_mut().take_rule_request(), None);
    tap(&mut h, "Open Requirement");
    assert_eq!(h.state_mut().take_rule_request().as_deref(), Some("kn_fail"));
    assert_eq!(h.state_mut().take_rule_request(), None, "once");
    // Arqiflow's own check has no rule in the project's setup to open; it names nothing to select, and the selection stays.
    tap(&mut h, "Bedroom area — Failed");
    tap(&mut h, "Parking can be driven to — Warning");
    assert!(h.query_by_label("Two stalls have no way out").is_some());
    assert!(h.query_by_label("Open Rule").is_none() && h.query_by_label("Open Requirement").is_none());
    assert_eq!(h.state().selection, [far]);
    // A waived rule says who accepted it and why.
    tap(&mut h, "Balcony depth — Waived");
    assert!(h.query_by_label("Waived by Sara: The client asked for Juliet balconies").is_some());
}

#[test]
fn the_status_bars_count_opens_the_palette() {
    let mut h = hosted(&two_levels());
    let far = *ids(&h, "wall").last().unwrap();
    h.state_mut().set_checks(&checks(far)).unwrap();
    h.run();
    assert!(h.query_by_label("Bedroom area — Failed").is_some(), "open from the start");
    // Closed by its ×, the count opens it again where it was: its own sidebar on the right.
    press(&mut h, "Close Checks");
    assert!(!docking(&h).shown(Palette::Checks) && h.query_by_label("Bedroom area — Failed").is_none());
    press(&mut h, "Checks: 2 flags");
    assert_eq!(docking(&h).docked(Palette::Checks), Some(Side::Right));
    assert!(h.query_by_label("Bedroom area — Failed").is_some());
    // Tabbed behind another palette by the person, the count brings it to the front.
    h.state_mut().docking.put(Palette::Checks, crate::dock::Target::With(Palette::Browser));
    h.state_mut().docking.activate(Palette::Browser);
    h.run();
    press(&mut h, "Checks: 2 flags");
    assert_eq!(docking(&h).stack_of(Palette::Checks).unwrap().shown(), Some(Palette::Checks));
}

#[test]
fn no_rules_all_passing_and_new_checks_each_say_so() {
    let mut h = hosted(BOX);
    h.state_mut().set_checks(r#"{"results": [], "integrity": []}"#).unwrap();
    front(&mut h);
    assert!(h.query_by_label("No rules to check").is_some());
    assert!(h.query_by_label("No rules or client requirements apply to this project yet.").is_some());
    assert!(h.query_by_label("Checks: no flags").is_some());
    let passing = json!({"results": [
        {"rule_id": "a", "title": "Corridor width", "scope": "company", "status": "pass", "findings": []},
        {"rule_id": "b", "title": "Stair width", "scope": "code", "status": "pass", "findings": []}
    ]});
    h.state_mut().set_checks(&passing.to_string()).unwrap();
    h.run();
    assert!(h.query_by_label("No flags").is_some() && h.query_by_label("Every check passed.").is_some());
    assert!(h.query_by_label("Checks: no flags").is_some());
    // A problem in the model itself is a flag too, before the rules'; and the page checking again shows it is.
    let wall = ids(&h, "wall")[0];
    front(&mut h);
    let broken = json!({"integrity": [{"message": "Two walls overlap", "elements": [format!("w{wall}")]}], "results": []});
    h.state_mut().set_checks(&broken.to_string()).unwrap();
    h.run();
    assert!(h.query_by_label("Checks: 1 flag").is_some());
    tap(&mut h, "Modelling problem — Failed");
    assert_eq!(h.state().selection, [wall]);
    h.state_mut().set_checks("null").unwrap();
    h.run();
    assert!(h.query_by_label("Checking the model…").is_some());
    // What the page sends is read kindly, and refused only when it isn't checks at all.
    assert!(h.state_mut().set_checks("[1, 2]").is_err() && h.state_mut().set_checks("{").is_err());
    assert!(h.state_mut().set_checks(r#"{"results": [{"status": 7, "findings": "x"}, 3], "extra": true}"#).is_ok());
}

#[test]
fn a_flag_on_a_room_shows_the_whole_room_not_only_its_tag() {
    // A long room: 30 m by 6 m, its tag in the middle.
    let mut h = hosted(
        r#"wall {"points": [[0,0],[30000,0],[30000,6000],[0,6000]], "closed": true}
room {"point": [15000, 3000], "name": "Hall"}"#,
    );
    let room = ids(&h, "room")[0];
    let flag = json!({"results": [{"rule_id": "kn_1", "title": "Hall area", "scope": "client", "status": "fail",
        "findings": [{"message": "Too small", "elements": [format!("r{room}")]}]}]});
    h.state_mut().set_checks(&flag.to_string()).unwrap();
    front(&mut h);
    tap(&mut h, "Hall area — Failed");
    assert_eq!(h.state().selection, [room]);
    h.run();
    let view = h.state().view_rect;
    for corner in [P2(500.0, 500.0), P2(29500.0, 5500.0)] {
        let at = h.state().screen(corner);
        assert!(view.contains(at), "the room's corner {corner:?} in the view: {at:?} in {view:?}");
    }
}

#[test]
fn only_failed_rules_and_warnings_are_counted() {
    let mut h = hosted(BOX);
    let some = json!({"results": [
        {"rule_id": "a", "title": "Crashed", "scope": "company", "status": "error", "error": "Traceback", "findings": []},
        {"rule_id": "b", "title": "Waived", "scope": "company", "status": "waived", "findings": [], "waiver": {"by": "A", "reason": "B"}},
        {"rule_id": "c", "title": "Unchecked", "scope": "company", "status": "guidance", "findings": []},
        {"rule_id": "d", "title": "Warned", "scope": "company", "status": "warn", "findings": []}
    ]});
    h.state_mut().set_checks(&some.to_string()).unwrap();
    front(&mut h);
    assert!(h.query_by_label("Checks: 1 flag").is_some() && h.query_by_label("1 flag").is_some());
    let order: Vec<&str> = h.state().checks().unwrap().rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(order, ["Warned", "Waived", "Crashed", "Unchecked"], "a check that could not run is not checked, not a flag");
}

#[test]
fn a_flag_is_waived_with_a_reason_through_the_page_and_a_waiver_undone() {
    let mut h = hosted(&two_levels());
    let far = *ids(&h, "wall").last().unwrap();
    h.state_mut().set_checks(&checks(far)).unwrap();
    front(&mut h);
    tap(&mut h, "Bedroom area — Failed");
    tap(&mut h, "Waive");
    // The reason first: one line, needed.
    assert!(h.get_by_role_and_label(Role::Button, "Waive").accesskit_node().is_disabled(), "no reason, no waiver");
    type_into(&mut h, "Reason", "The client accepted a smaller second bedroom");
    tap(&mut h, "Waive");
    let asked = h.state_mut().take_waive_request();
    assert_eq!(
        asked,
        Some(crate::checks::Waive::Add { rule: "kn_fail".into(), reason: "The client accepted a smaller second bedroom".into() })
    );
    assert!(h.query_by_label("Saving…").is_some());
    // The server would not: its reason, under the check.
    h.state_mut().waive_refused("kn_fail", "Only a project manager can waive a rule.");
    h.run();
    assert!(h.query_by_label("Only a project manager can waive a rule.").is_some());
    // A building code's flag has no Waive.
    let code = json!({"results": [{"rule_id": "k", "title": "Fire exit width", "scope": "code", "status": "fail", "findings": []}]});
    h.state_mut().set_checks(&code.to_string()).unwrap();
    h.run();
    tap(&mut h, "Fire exit width — Failed");
    assert!(h.query_by_label("Waive").is_none());
    // A waived one: who and why, and Undo Waive.
    tap(&mut h, "Fire exit width — Failed");
    h.state_mut().set_checks(&checks(far)).unwrap();
    h.run();
    tap(&mut h, "Balcony depth — Waived");
    assert!(h.query_by_label("Waived by Sara: The client asked for Juliet balconies").is_some());
    tap(&mut h, "Undo Waive");
    assert_eq!(h.state_mut().take_waive_request(), Some(crate::checks::Waive::Remove { rule: "kn_waived".into() }));
}

#[test]
fn nothing_verified_and_rules_that_disagree_are_said() {
    let mut h = hosted(BOX);
    let c = json!({
        "summary": {"checked": 0},
        "value_conflicts": [{"key": "max_height", "used": 12, "claims": [
            {"rule_id": "a", "title": "Zoning", "scope": "code", "value": 12}, {"rule_id": "b", "title": "Brief", "scope": "client", "value": 15}]}],
        "results": [{"rule_id": "x", "title": "Daylight", "scope": "company", "status": "guidance", "findings": []}]
    });
    h.state_mut().set_checks(&c.to_string()).unwrap();
    front(&mut h);
    assert!(h.query_by_label("No active rule has a check, so nothing was verified.").is_some());
    let text = "Rules disagree on a value. The highest-ranking rule wins:\nmax_height is taken as 12: Zoning says 12; Brief says 15.";
    assert!(h.query_by_label(text).is_some());
}

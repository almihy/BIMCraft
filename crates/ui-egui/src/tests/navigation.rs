//! Moving about a view on a laptop and with a mouse: the trackpad's two-finger pan and pinch, the wheel's zoom, Pan (the hand),
//! Zoom Region and Previous Zoom, and what the cursor shows while they are on.

use super::*;
use crate::canvas::navigation::Mode;
use egui::CursorIcon;

fn key_of(h: &Harness<BimApp>) -> Id {
    match h.state().sheet {
        Sheet::Plan(k) => k,
        other => panic!("not a plan: {other:?}"),
    }
}

/// The plan's zoom, px per mm, and the model point at the middle of it.
fn camera(h: &Harness<BimApp>) -> (f64, P2) {
    let v = h.state().canvas.view(key_of(h)).unwrap();
    (v.scale, P2(v.cx, v.cy))
}

fn wheel(h: &mut Harness<BimApp>, unit: MouseWheelUnit, delta: egui::Vec2, modifiers: Modifiers) {
    h.event(Event::MouseWheel { unit, delta, phase: TouchPhase::Move, modifiers });
    h.run();
}

/// A few quiet frames, so that a wheel's last turn is long past.
fn rest(h: &mut Harness<BimApp>) {
    h.run_steps(6);
}

fn over_plan(h: &mut Harness<BimApp>) -> Pos2 {
    let at = h.state().screen(P2(4000.0, 3000.0));
    h.hover_at(at);
    h.run();
    at
}

/// The window's wheel as a laptop has it: the trackpad told from the mouse (Settings > Mouse wheel > Auto).
fn auto(h: &mut Harness<BimApp>) {
    h.state_mut().canvas.nav.wheel = crate::canvas::navigation::Wheel::Auto;
}

#[test]
fn the_wheel_zooms_by_default_as_revits_does_whatever_it_sends() {
    use crate::canvas::navigation::Wheel;
    let mut h = app(BOX);
    assert_eq!(h.state().canvas.nav.wheel, Wheel::Zoom);
    let p = P2(4000.0, 3000.0);
    let at = over_plan(&mut h);
    // A smooth wheel's small steps, a trackpad's two-finger scroll and a notch: all zoom, about the cursor, and none pans.
    for delta in [vec2(0.0, 15.0), vec2(0.0, 120.0), vec2(0.0, -40.0)] {
        let before = camera(&h).0;
        gesture(&mut h, MouseWheelUnit::Point, &[delta], Modifiers::NONE);
        let now = camera(&h).0;
        assert!((now > before) == (delta.y > 0.0) && (now - before).abs() > 1e-9, "{delta:?}: {before} to {now}");
        assert!((h.state().screen(p) - at).length() < 1.5, "{delta:?}: about the cursor, not a pan");
    }
    // A settings file that says nothing of it starts a window with the same.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    std::fs::write(&file, "{}").unwrap();
    let mut again = app(BOX);
    again.state_mut().use_settings(file);
    assert_eq!(again.state().canvas.nav.wheel, Wheel::Zoom);
}

#[test]
fn a_trackpads_two_finger_scroll_pans_the_plan_and_a_mouse_wheel_still_zooms() {
    let mut h = app(BOX);
    auto(&mut h);
    let p = P2(4000.0, 3000.0);
    let at = over_plan(&mut h);
    let (scale, _) = camera(&h);
    // Two fingers dragged down and left by (-30, 40) points: the drawing follows them.
    wheel(&mut h, MouseWheelUnit::Point, vec2(-30.0, 40.0), Modifiers::NONE);
    rest(&mut h);
    let moved = h.state().screen(p) - at;
    assert!((moved - vec2(-30.0, 40.0)).length() < 1.5, "the drawing went {moved:?}, with the fingers");
    assert!((camera(&h).0 - scale).abs() < 1e-12, "panning does not zoom");
    // The same scroll again, a bit at a time, as a trackpad sends it.
    for _ in 0..5 {
        wheel(&mut h, MouseWheelUnit::Point, vec2(0.0, -6.0), Modifiers::NONE);
    }
    rest(&mut h);
    let moved = h.state().screen(p) - at;
    assert!((moved - vec2(-30.0, 10.0)).length() < 1.5, "five small steps: {moved:?}");
    assert!((camera(&h).0 - scale).abs() < 1e-12);
    // A mouse's notch, 120 pixels of it, zooms about the cursor as it always did.
    rest(&mut h);
    wheel(&mut h, MouseWheelUnit::Point, vec2(0.0, 120.0), Modifiers::NONE);
    rest(&mut h);
    assert!(camera(&h).0 > scale * 1.2, "the notch zoomed in");
}

#[test]
fn shift_and_the_wheel_do_what_the_wheel_does_alone() {
    // Revit gives Shift nothing to do with the wheel: a notch zooms about the cursor all the same, and nothing pans.
    let p = P2(4000.0, 3000.0);
    let mut turned = Vec::new();
    for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
        let mut h = app(BOX);
        let at = over_plan(&mut h);
        let (scale, _) = camera(&h);
        wheel(&mut h, MouseWheelUnit::Point, vec2(0.0, 120.0), modifiers);
        rest(&mut h);
        assert!((h.state().screen(p) - at).length() < 1.5, "{modifiers:?}: about the cursor, no pan");
        turned.push(camera(&h).0 / scale);
    }
    assert!(turned[0] > 1.2 && (turned[0] - turned[1]).abs() < 1e-9, "{turned:?}");
}

#[test]
fn a_mouse_that_counts_lines_zooms_and_a_hostile_scroll_leaves_the_view_usable() {
    let mut h = app(BOX);
    over_plan(&mut h);
    let (scale, _) = camera(&h);
    wheel(&mut h, MouseWheelUnit::Line, vec2(0.0, 3.0), Modifiers::NONE);
    rest(&mut h);
    assert!(camera(&h).0 > scale, "three lines of wheel zoomed in");
    for delta in [vec2(0.0, 1e30), vec2(0.0, -1e30), vec2(1e30, -1e30)] {
        rest(&mut h);
        wheel(&mut h, MouseWheelUnit::Point, delta, Modifiers::NONE);
        let (s, c) = camera(&h);
        assert!(s.is_finite() && c.0.is_finite() && c.1.is_finite(), "{delta:?}: {s} {c:?}");
    }
}

#[test]
fn a_pinch_zooms_about_the_cursor() {
    let mut h = app(BOX);
    let p = P2(4000.0, 3000.0);
    let at = over_plan(&mut h);
    let (scale, _) = camera(&h);
    // The pinch of a trackpad, as a window reports it.
    h.event(Event::Zoom(1.5));
    h.run();
    let (now, _) = camera(&h);
    assert!((now / scale - 1.5).abs() < 0.01, "zoomed by {}", now / scale);
    assert!((h.state().screen(p) - at).length() < 1.0, "the point under the cursor stays put");
    // A browser sends a pinch as the wheel with Ctrl: it zooms the same way, and never pans.
    rest(&mut h);
    wheel(&mut h, MouseWheelUnit::Point, vec2(0.0, -20.0), Modifiers::COMMAND);
    rest(&mut h);
    assert!(camera(&h).0 < now, "pinching in zoomed out");
    assert!((h.state().screen(p) - at).length() < 1.0, "and the point stays put");
}

#[test]
fn pan_is_a_button_in_the_navigation_bar_and_while_it_is_on_the_left_button_drags_the_view() {
    let mut h = app(BOX);
    let p = P2(4000.0, 3000.0);
    let from = h.state().screen(P2(1000.0, 1000.0));
    let to = from + vec2(120.0, -70.0);
    // Without Pan a drag on empty floor is a selection box: the view stays.
    let before = h.state().screen(p);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert!((h.state().screen(p) - before).length() < 0.5);
    press(&mut h, "Pan");
    assert_eq!(h.state().canvas.nav.mode, Mode::Pan);
    // Over a wall too: a drag pans and moves nothing.
    let walls_before = walls(&h);
    let on_wall = h.state().screen(P2(4000.0, 0.0));
    drag_screen(&mut h, on_wall, on_wall + vec2(80.0, 30.0), PointerButton::Primary);
    assert_eq!(walls(&h), walls_before, "the hand moves the view, not the walls");
    assert!(h.state().selection.is_empty(), "and picks nothing");
    let moved = h.state().screen(p) - before;
    assert!((moved - vec2(80.0, 30.0)).length() < 1.0, "the view followed the hand: {moved:?}");
    // The cursor is a hand, closed while it drags.
    h.hover_at(from);
    h.run();
    assert_eq!(h.state().canvas.nav.icon.get(), Some(CursorIcon::Grab));
    h.event(button(from, true));
    h.run();
    h.hover_at(from + vec2(5.0, 5.0));
    h.run();
    assert_eq!(h.state().canvas.nav.icon.get(), Some(CursorIcon::Grabbing));
    h.event(button(from + vec2(5.0, 5.0), false));
    h.run();
    // Esc puts the hand down, and a drag is a selection box again.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().canvas.nav.mode, Mode::Off);
    let before = h.state().screen(p);
    drag_screen(&mut h, from, to, PointerButton::Primary);
    assert!((h.state().screen(p) - before).length() < 0.5, "a drag no longer pans");
    // The button toggles it, and a tool puts it down.
    press(&mut h, "Pan");
    press(&mut h, "Pan");
    assert_eq!(h.state().canvas.nav.mode, Mode::Off);
    press(&mut h, "Pan");
    shortcut(&mut h, "WA");
    assert_eq!(h.state().canvas.nav.mode, Mode::Off, "starting a tool ends Pan");
}

#[test]
fn zoom_region_zooms_to_the_box_dragged_and_previous_zoom_goes_back() {
    let mut h = app(BOX);
    let plan = key_of(&h);
    let (scale, centre) = camera(&h);
    shortcut(&mut h, "ZR");
    assert_eq!(h.state().canvas.nav.mode, Mode::Region);
    // A box round the south-west corner, 2 m by 1.5 m.
    let (lo, hi) = (P2(-500.0, -500.0), P2(1500.0, 1000.0));
    let (a, b) = (h.state().screen(lo), h.state().screen(hi));
    drag_screen(&mut h, a, b, PointerButton::Primary);
    assert_eq!(h.state().canvas.nav.mode, Mode::Off, "one box, then back to the tool");
    let (now, mid) = camera(&h);
    assert!(now > scale * 3.0, "zoomed in from {scale} to {now}");
    assert!(mid.dist(P2(500.0, 250.0)) < 5.0, "the box is in the middle: {mid:?}");
    let rect = h.state().canvas.view(plan).map(|v| (f64::from(v.width), f64::from(v.height))).unwrap();
    let fits = (rect.0 / 2000.0).min(rect.1 / 1500.0);
    assert!((now / fits - 1.0).abs() < 0.02, "the box fills the view: {now} against {fits}");
    assert!(h.state().selection.is_empty(), "the box selected nothing");
    // Previous Zoom: the view as it was.
    shortcut(&mut h, "ZP");
    let (back, there) = camera(&h);
    assert!((back / scale - 1.0).abs() < 1e-9 && there.dist(centre) < 1e-6, "back at {back} {there:?}");
    // Nothing earlier now: it says so and the view stays.
    shortcut(&mut h, "ZP");
    assert!(!h.state().status.text.is_empty() && !h.state().status.error, "{:?}", h.state().status);
    assert!((camera(&h).0 / scale - 1.0).abs() < 1e-9);
    // A click with Zoom Region on is no box: the mode waits for one. Esc ends it.
    shortcut(&mut h, "ZR");
    let mid = h.state().screen(P2(4000.0, 3000.0));
    click_at(&mut h, mid);
    assert_eq!(h.state().canvas.nav.mode, Mode::Region);
    h.hover_at(mid);
    h.run();
    assert_eq!(h.state().canvas.nav.icon.get(), Some(CursorIcon::ZoomIn));
    key(&mut h, Key::Escape);
    assert_eq!(h.state().canvas.nav.mode, Mode::Off);
    // The navigation bar has it too.
    press(&mut h, "Zoom Region");
    assert_eq!(h.state().canvas.nav.mode, Mode::Region);
}

#[test]
fn previous_zoom_also_undoes_a_wheel_turn_a_pan_and_a_fit() {
    let mut h = app(BOX);
    let (scale, centre) = camera(&h);
    over_plan(&mut h);
    wheel(&mut h, MouseWheelUnit::Point, vec2(0.0, 120.0), Modifiers::NONE);
    rest(&mut h);
    let zoomed = camera(&h);
    assert!(zoomed.0 > scale);
    // Zoom to Fit is a step too.
    shortcut(&mut h, "ZF");
    shortcut(&mut h, "ZP");
    let (s, c) = camera(&h);
    assert!((s / zoomed.0 - 1.0).abs() < 1e-9 && c.dist(zoomed.1) < 1e-6, "back from the fit");
    // And the wheel's whole run of turns is one step.
    shortcut(&mut h, "ZP");
    let (s, c) = camera(&h);
    assert!((s / scale - 1.0).abs() < 1e-9 && c.dist(centre) < 1e-6, "back from the wheel: {s} {c:?}");
}

#[test]
fn zoom_out_by_half_and_the_shortcuts_are_in_the_list() {
    let mut h = app(BOX);
    let (scale, _) = camera(&h);
    shortcut(&mut h, "ZO");
    assert!((camera(&h).0 / scale - 0.5).abs() < 1e-9);
    // Revit's own keys: ZR and ZZ, ZP and ZC, ZO and ZV, ZE, ZF and ZX; Repeat Last Command is RC.
    shortcut(&mut h, "ZV");
    assert!((camera(&h).0 / scale - 0.25).abs() < 1e-9, "ZV zooms out by half too");
    shortcut(&mut h, "ZC");
    assert!((camera(&h).0 / scale - 0.5).abs() < 1e-9, "ZC is Previous Zoom");
    shortcut(&mut h, "ZX");
    assert!((camera(&h).0 / scale - 1.0).abs() < 0.05, "ZX fits as ZF does: {}", camera(&h).0 / scale);
    for (keys, name) in [
        ("ZR", "Zoom Region"),
        ("ZZ", "Zoom Region"),
        ("ZP", "Previous Zoom"),
        ("ZC", "Previous Zoom"),
        ("ZO", "Zoom Out"),
        ("ZV", "Zoom Out"),
        ("RC", "Repeat Last Command"),
    ] {
        assert!(crate::shortcuts::ACTIONS.contains(&name), "{name}");
        assert!(crate::shortcuts().iter().any(|(k, _)| *k == keys), "{keys}");
    }
}

#[test]
fn zoom_region_takes_one_corner_clicked_and_then_the_other() {
    let mut h = app(BOX);
    let (scale, _) = camera(&h);
    shortcut(&mut h, "ZR");
    let (lo, hi) = (P2(-500.0, -500.0), P2(1500.0, 1000.0));
    let (a, b) = (h.state().screen(lo), h.state().screen(hi));
    // The first click leaves the box open, and it follows the cursor.
    click_at(&mut h, a);
    assert_eq!(h.state().canvas.nav.mode, Mode::Region, "one click is not a box");
    h.hover_at(b);
    h.run();
    assert!(!h.state().canvas.region_shapes(egui::Rect::EVERYTHING).is_empty(), "the box is drawn to the cursor");
    // A second click at the same spot is no box either.
    click_at(&mut h, a);
    assert_eq!(h.state().canvas.nav.mode, Mode::Region);
    // The click at the far corner finishes it.
    click_at(&mut h, b);
    assert_eq!(h.state().canvas.nav.mode, Mode::Off, "the second corner ends Zoom Region");
    let (now, mid) = camera(&h);
    assert!(now > scale * 3.0 && mid.dist(P2(500.0, 250.0)) < 5.0, "zoomed to the box: {now} {mid:?}");
    assert!(h.state().selection.is_empty(), "and nothing was selected");
    // Esc between the corners drops the first one: the next ZR starts afresh.
    shortcut(&mut h, "ZP");
    shortcut(&mut h, "ZR");
    click_at(&mut h, a);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().canvas.nav.mode, Mode::Off);
    shortcut(&mut h, "ZR");
    click_at(&mut h, b);
    assert_eq!(h.state().canvas.nav.mode, Mode::Region, "the old corner is forgotten: this click is a first corner");
}

/// A wheel gesture's events as they come in one frame, then a quiet.
fn gesture(h: &mut Harness<BimApp>, unit: MouseWheelUnit, deltas: &[egui::Vec2], modifiers: Modifiers) {
    rest(h);
    for delta in deltas {
        h.event(Event::MouseWheel { unit, delta: *delta, phase: TouchPhase::Move, modifiers });
    }
    h.run();
    rest(h);
}

#[test]
fn every_browsers_notch_zooms_and_a_two_finger_scroll_pans() {
    let mut h = app(BOX);
    auto(&mut h);
    let p = P2(4000.0, 3000.0);
    let at = over_plan(&mut h);
    // What browsers send for one notch: 100 and 120; at 110 % and 90 % browser zoom; five lines to a notch under Windows;
    // two and a half; a notch counted in lines.
    for (unit, dy) in [
        (MouseWheelUnit::Point, 100.0),
        (MouseWheelUnit::Point, 120.0),
        (MouseWheelUnit::Point, 90.909),
        (MouseWheelUnit::Point, 111.111),
        (MouseWheelUnit::Point, 166.67),
        (MouseWheelUnit::Point, 80.0),
        (MouseWheelUnit::Line, 3.0),
    ] {
        let (before, _) = camera(&h);
        gesture(&mut h, unit, &[vec2(0.0, dy)], Modifiers::NONE);
        let (now, _) = camera(&h);
        assert!(now > before * 1.05, "{unit:?} {dy}: the notch zoomed ({before} to {now})");
        assert!((h.state().screen(p) - at).length() < 1.5, "{dy}: about the cursor, not a pan");
    }
    // A fast wheel: several notches at once, then smaller ones within the same gesture, all zoom.
    let before = camera(&h).0;
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(0.0, -240.0), vec2(0.0, -20.0), vec2(0.0, -30.0)], Modifiers::NONE);
    assert!(camera(&h).0 < before, "spinning back zoomed out");
    assert!((h.state().screen(p) - at).length() < 1.5, "and never panned");
    // A trackpad: both ways at once, or small steps first, pans; a big step in the middle of its gesture is still its own.
    let scale = camera(&h).0;
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(-30.0, 40.0)], Modifiers::NONE);
    let moved = h.state().screen(p) - at;
    assert!((moved - vec2(-30.0, 40.0)).length() < 1.5, "two fingers panned: {moved:?}");
    let at = h.state().screen(p);
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(0.0, 6.5), vec2(0.0, 7.5), vec2(0.0, 110.0)], Modifiers::NONE);
    let moved = h.state().screen(p) - at;
    assert!((moved - vec2(0.0, 124.0)).length() < 2.0, "small steps first: a pan all through ({moved:?})");
    assert!((camera(&h).0 - scale).abs() < 1e-12, "and no zoom");
    // A pinch zooms whatever the hand.
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(0.0, 12.0)], Modifiers::COMMAND);
    assert!(camera(&h).0 > scale, "ctrl and the wheel zoomed");
}

#[test]
fn mouse_wheel_in_settings_makes_the_wheel_always_zoom_or_always_pan() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("settings.json");
    let mut h = app(BOX);
    h.state_mut().use_settings(file.clone());
    let p = P2(4000.0, 3000.0);
    over_plan(&mut h);
    auto(&mut h);
    // A smooth wheel's small steps look like a trackpad's: Auto pans.
    let scale = camera(&h).0;
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(0.0, 15.0), vec2(0.0, 15.0)], Modifiers::NONE);
    assert!((camera(&h).0 - scale).abs() < 1e-12, "Auto read small steps as a trackpad");
    // Settings > User Interface > Mouse wheel > Zoom: Revit's way.
    h.state_mut().settings_page = Some(crate::settings::Page::Interface);
    h.run();
    h.get_by_role_and_label(Role::RadioButton, "Zoom").scroll_to_me();
    h.run();
    pick(&mut h, Role::RadioButton, "Zoom");
    assert_eq!(h.state().canvas.nav.wheel, crate::canvas::navigation::Wheel::Zoom);
    press(&mut h, "Close");
    over_plan(&mut h);
    let before = h.state().screen(p);
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(0.0, 15.0), vec2(0.0, 15.0)], Modifiers::NONE);
    assert!(camera(&h).0 > scale, "the small steps zoomed");
    assert!((h.state().screen(p) - before).length() < 1.5);
    // A trackpad's two-finger scroll zooms too, along its up and down.
    let before = camera(&h).0;
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(-30.0, 40.0)], Modifiers::NONE);
    assert!(camera(&h).0 > before);
    // The file keeps it, and the next window starts with it.
    let kept: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(kept["mouse_wheel"], "zoom");
    let mut again = app(BOX);
    again.state_mut().use_settings(file);
    assert_eq!(again.state().canvas.nav.wheel, crate::canvas::navigation::Wheel::Zoom);
    // Pan: even a notch scrolls.
    h.state_mut().canvas.nav.wheel = crate::canvas::navigation::Wheel::Pan;
    let (scale, at) = (camera(&h).0, h.state().screen(p));
    gesture(&mut h, MouseWheelUnit::Point, &[vec2(0.0, 120.0)], Modifiers::NONE);
    assert!((camera(&h).0 - scale).abs() < 1e-12, "no zoom");
    assert!((h.state().screen(p) - at).length() > 20.0, "the view scrolled");
    // A pinch zooms in every setting.
    let before = camera(&h).0;
    h.event(Event::Zoom(1.5));
    h.run();
    assert!(camera(&h).0 > before * 1.4);
}

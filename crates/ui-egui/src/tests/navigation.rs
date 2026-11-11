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

#[test]
fn a_trackpads_two_finger_scroll_pans_the_plan_and_a_mouse_wheel_still_zooms() {
    let mut h = app(BOX);
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
    // Shift and the wheel pan along, as a mouse has no other way to.
    let before = h.state().screen(p);
    rest(&mut h);
    wheel(&mut h, MouseWheelUnit::Point, vec2(120.0, 0.0), Modifiers::SHIFT);
    rest(&mut h);
    assert!((h.state().screen(p).x - before.x).abs() > 20.0, "a horizontal turn panned");
}

#[test]
fn a_mouse_that_counts_lines_zooms_and_a_hostile_scroll_leaves_the_view_usable() {
    let mut h = app(BOX);
    over_plan(&mut h);
    let (scale, _) = camera(&h);
    wheel(&mut h, MouseWheelUnit::Line, vec2(0.0, 3.0), Modifiers::NONE);
    rest(&mut h);
    assert!(camera(&h).0 > scale, "three lines of wheel zoomed in");
    for delta in [vec2(0.0, 1e30), vec2(f32::NAN, 5.0), vec2(1e30, -1e30)] {
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
    for (keys, name) in [("ZR", "Zoom Region"), ("ZP", "Previous Zoom"), ("ZO", "Zoom Out"), ("RR", "Repeat Last Command")] {
        assert!(crate::shortcuts::ACTIONS.contains(&name), "{name}");
        assert!(crate::shortcuts().iter().any(|(k, _)| *k == keys), "{keys}");
    }
}

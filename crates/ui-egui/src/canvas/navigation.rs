//! Moving about a plan, a section or a sheet, for a laptop as much as for a mouse. The wheel zooms about the cursor, as it
//! always did; a trackpad's two-finger scroll pans and its pinch (Ctrl and the wheel, in a browser) zooms. A wheel and a
//! trackpad are told apart by what they send ([`hand_of`]). The navigation bar's Pan is the hand: while it is on, the left
//! button drags the view about. Zoom Region (ZR) zooms to a box dragged, Previous Zoom (ZP) goes back to where the view was
//! before its last zoom or pan, and Zoom to Fit (ZF, ZE) is the view's own.
//!
//! A drag with the middle button pans in every mode, as before.

use bimcraft_core::model::Id;
use egui::{CursorIcon, Event, MouseWheelUnit, PointerButton, Pos2, Rect, Sense, Shape, TouchPhase, Ui, UiBuilder, Vec2, vec2};

use super::{Camera, Canvas, MAX_COORD, SCALE, box_shapes};
use crate::icons::Icon;
use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Status};

/// How many earlier views Previous Zoom keeps.
const HISTORY: usize = 32;

/// A pan or zoom that starts this long after the last one is a new step of Previous Zoom, not a continuation of it, seconds.
const GESTURE: f64 = 0.5;

/// A wheel event this soon after a trackpad's is the trackpad's, seconds: a flick ends with events that look like a notch.
const SETTLE: f64 = 0.3;

/// A box dragged for Zoom Region smaller than this (px) is a click, not a box.
const BOX: f32 = 6.0;

/// What the left button does on a view besides what its tool says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Off,
    /// Revit's hand: a drag pans.
    Pan,
    /// Zoom Region: a box dragged is what the view then shows.
    Region,
}

/// What turns the wheel: a mouse's notches, or a trackpad's two-finger scroll.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Hand {
    #[default]
    Mouse,
    Trackpad,
}

/// Whether `v` is a mouse notch in browser pixels: 100 or 120 (Windows, Linux), or several at once when the wheel spins fast.
fn notch(v: f32) -> bool {
    let a = v.abs();
    a >= 100.0 && [100.0_f32, 120.0].iter().any(|n| (a / n - (a / n).round()).abs() < 0.01)
}

/// Whether `v` is a whole number of lines: a mouse's notch in a window or a browser that counts lines.
fn lines(v: f32) -> bool {
    v != 0.0 && (v - v.round()).abs() < 1e-3
}

/// What sent one wheel event. A trackpad is the only thing that reports a gesture's start and end, scrolls both ways at once,
/// or scrolls in pixels that are not a notch (a notch being 100 or 120 of them, or whole lines); a mouse's wheel turns one
/// notch at a time, one way. Where the two cannot be told apart (a Mac's mouse wheel reports small pixel steps, as a trackpad
/// does) it reads as a trackpad.
pub(crate) fn hand_of(unit: MouseWheelUnit, delta: Vec2, phase: TouchPhase) -> Hand {
    if phase != TouchPhase::Move {
        return Hand::Trackpad;
    }
    let one_way = delta.x == 0.0 || delta.y == 0.0;
    let along = if delta.x == 0.0 { delta.y } else { delta.x };
    let mouse = match unit {
        MouseWheelUnit::Point => !delta.is_finite() || delta.abs().max_elem() > 5000.0 || (one_way && notch(along)),
        MouseWheelUnit::Line | MouseWheelUnit::Page => !delta.is_finite() || (one_way && lines(along)),
    };
    if mouse { Hand::Mouse } else { Hand::Trackpad }
}

/// A view's navigation state, kept with the canvas.
#[derive(Default)]
pub(crate) struct Nav {
    pub(crate) mode: Mode,
    /// What the wheel was last heard to be.
    pub(crate) hand: Hand,
    /// When a trackpad was last heard, seconds.
    heard: Option<f64>,
    /// Zoom Region's box, once begun: where it began and where the hand is, on screen.
    region: Option<(Pos2, Pos2)>,
    /// The views the view had before its last pans and zooms, by level, the latest last.
    history: Vec<(Id, Camera)>,
    /// When the last pan or zoom by hand or wheel went on, seconds.
    gesture: Option<f64>,
    /// The cursor shape the canvas asked for this frame.
    pub(crate) icon: std::cell::Cell<Option<CursorIcon>>,
}

impl Nav {
    /// Listen to this frame's wheel events and note what is turning the wheel.
    fn hear(&mut self, ui: &Ui, now: f64) {
        let events: Vec<(MouseWheelUnit, Vec2, TouchPhase)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    // With Ctrl the wheel is a zoom, whichever hand it is.
                    Event::MouseWheel { unit, delta, phase, modifiers } if !modifiers.command && !modifiers.ctrl => {
                        Some((*unit, *delta, *phase))
                    }
                    _ => None,
                })
                .collect()
        });
        for (unit, delta, phase) in events {
            match hand_of(unit, delta, phase) {
                Hand::Trackpad => {
                    self.hand = Hand::Trackpad;
                    self.heard = Some(now);
                }
                Hand::Mouse if self.heard.is_none_or(|t| now - t > SETTLE) => self.hand = Hand::Mouse,
                Hand::Mouse => {}
            }
        }
    }

    /// The scroll of this frame as what it does: how much to zoom (the wheel's, in points), and how far to pan (points). A
    /// trackpad's scroll pans both ways; a wheel zooms, and Shift and the wheel pans along.
    fn split(&self, scroll: Vec2) -> (f32, Vec2) {
        let (zoom, pan) = match self.hand {
            Hand::Trackpad => (0.0, scroll),
            Hand::Mouse => (scroll.y, vec2(scroll.x, 0.0)),
        };
        let pan = if pan.is_finite() { pan.clamp(vec2(-3000.0, -3000.0), vec2(3000.0, 3000.0)) } else { Vec2::ZERO };
        (if zoom.is_finite() { zoom } else { 0.0 }, pan)
    }

    /// Ask for cursor shape `icon` over the view.
    pub(crate) fn cursor(&self, ui: &Ui, icon: CursorIcon) {
        ui.ctx().set_cursor_icon(icon);
        self.icon.set(Some(icon));
    }
}

impl Canvas {
    /// What a view's canvas senses: clicks and drags, but while the hand or Zoom Region is on, nothing the tool would take the
    /// drag for.
    pub(crate) fn sense(&self) -> Sense {
        if self.nav.mode == Mode::Off { Sense::CLICK | Sense::DRAG } else { Sense::hover() }
    }

    /// Keep where view `id` is now as a step of Previous Zoom, unless it is already the last.
    pub(crate) fn remember_now(&mut self, id: Id) {
        let Some(&now) = self.cams.get(&id) else { return };
        let same = |a: &Camera, b: &Camera| a.scale == b.scale && a.cx == b.cx && a.cy == b.cy;
        if self.nav.history.last().is_some_and(|(k, c)| *k == id && same(c, &now)) {
            return;
        }
        self.nav.history.push((id, now));
        if self.nav.history.len() > HISTORY {
            self.nav.history.remove(0);
        }
    }

    /// Keep it as a step too, but only when this is the first of a run of pans and zooms: a wheel turning, a hand dragging.
    fn remember_gesture(&mut self, id: Id, now: f64) {
        if self.nav.gesture.is_none_or(|t| now - t > GESTURE) {
            self.remember_now(id);
        }
        self.nav.gesture = Some(now);
    }

    /// Previous Zoom: view `id` as it was before its last zoom or pan. Whether there was one.
    pub(crate) fn previous(&mut self, id: Id) -> bool {
        let Some(at) = self.nav.history.iter().rposition(|(k, _)| *k == id) else { return false };
        let (_, cam) = self.nav.history.remove(at);
        self.cams.insert(id, cam);
        self.nav.gesture = None;
        true
    }

    /// View `id` moved by `by` pixels, as a hand moves paper: what was under the cursor follows it.
    fn pan(&mut self, id: Id, by: Vec2) {
        if let Some(v) = self.view(id) {
            let mid = self.rect.size() / 2.0;
            self.hold(id, v.mm((mid - by).into()), mid, v.scale);
        }
    }

    /// View `id` showing the box between screen points `a` and `b` (in the view's own pixels), as large as it goes.
    fn zoom_box(&mut self, id: Id, a: Vec2, b: Vec2) {
        let Some(v) = self.view(id) else { return };
        let (p, q) = (v.mm(a.into()), v.mm(b.into()));
        let (w, h) = (f64::from(self.rect.width().max(1.0)), f64::from(self.rect.height().max(1.0)));
        let (dx, dy) = ((p.0 - q.0).abs().max(1.0), (p.1 - q.1).abs().max(1.0));
        let scale = (w / dx).min(h / dy).clamp(SCALE.0, SCALE.1);
        let (cx, cy) = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
        if scale.is_finite() && cx.is_finite() && cy.is_finite() {
            self.remember_now(id);
            self.cams.insert(id, Camera { scale, cx: cx.clamp(-MAX_COORD, MAX_COORD), cy: cy.clamp(-MAX_COORD, MAX_COORD) });
        }
    }

    /// What the hands do to view `id` this frame, in `rect`: the wheel, a trackpad's scroll and pinch, the middle button, the
    /// left one in Pan, and Zoom Region's box. `hover` is the cursor over the view, in screen points.
    pub(super) fn handle(&mut self, ui: &Ui, rect: Rect, id: Id, hover: Option<Pos2>) {
        let now = ui.input(|i| i.time);
        self.nav.icon.set(None);
        self.nav.hear(ui, now);
        let (scroll, pinch, middle, primary, latest, released) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                (i.pointer.button_pressed(PointerButton::Middle), i.pointer.button_down(PointerButton::Middle)),
                (i.pointer.button_pressed(PointerButton::Primary), i.pointer.button_down(PointerButton::Primary)),
                i.pointer.latest_pos(),
                i.pointer.button_released(PointerButton::Primary),
            )
        });
        let (wheel, pan) = self.nav.split(scroll);
        let mode = self.nav.mode;
        if let (Some(screen), Some(v)) = (hover, self.view(id)) {
            let at = screen - rect.min;
            // A wheel notch (about 50 points) zooms by a fifth.
            let factor = (f64::from(wheel) * 0.004).exp() * f64::from(pinch);
            if factor.is_finite() && factor != 1.0 {
                self.remember_gesture(id, now);
                self.hold(id, v.mm(at.into()), at, v.scale * factor.clamp(0.2, 5.0));
            }
            if pan != Vec2::ZERO {
                self.remember_gesture(id, now);
                self.pan(id, pan);
            }
            if middle.0 || (mode == Mode::Pan && primary.0) {
                self.remember_gesture(id, now);
                if let Some(v) = self.view(id) {
                    self.grab = Some(v.mm(at.into()));
                }
            }
            if mode == Mode::Region && primary.0 {
                self.nav.region = Some((screen, screen));
            }
        }
        let dragging = middle.1 || (mode == Mode::Pan && primary.1);
        match (self.grab, ui.input(|i| i.pointer.hover_pos()), self.view(id)) {
            (Some(g), Some(at), Some(v)) if dragging => self.hold(id, g, at - rect.min, v.scale),
            _ => self.grab = None,
        }
        if let Some((from, _)) = self.nav.region {
            let to = latest.unwrap_or(from);
            self.nav.region = Some((from, to));
            if released {
                self.nav.region = None;
                if (to - from).abs().max_elem() >= BOX {
                    self.zoom_box(id, from - rect.min, to - rect.min);
                    self.nav.mode = Mode::Off;
                }
            }
        }
        if hover.is_some() {
            match (self.nav.mode, dragging) {
                (Mode::Pan, true) => self.nav.cursor(ui, CursorIcon::Grabbing),
                (Mode::Pan, false) => self.nav.cursor(ui, CursorIcon::Grab),
                (Mode::Region, _) => self.nav.cursor(ui, CursorIcon::ZoomIn),
                (Mode::Off, true) => self.nav.cursor(ui, CursorIcon::Grabbing),
                (Mode::Off, false) => {}
            }
        }
    }

    /// Zoom Region's box as dragged so far, in `rect`.
    pub(crate) fn region_shapes(&self, rect: Rect) -> Vec<Shape> {
        match self.nav.region {
            Some((a, b)) => box_shapes(rect, (a.x - rect.min.x, a.y - rect.min.y), (b.x - rect.min.x, b.y - rect.min.y)),
            None => Vec::new(),
        }
    }
}

/// A navigation button: its icon, lit while `active`, named `label` for the screen reader and the tooltip.
fn button(ui: &mut Ui, icon: Icon, label: &str, active: bool) -> egui::Response {
    let label = t(label);
    let (rect, r) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::click());
    let enabled = ui.is_enabled();
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, active, label));
    let (ink, hover) = crate::style::icon_inks();
    if active {
        ui.painter().rect(rect, 6.0, pal().select_fill, egui::Stroke::new(1.0, pal().accent), egui::StrokeKind::Inside);
    } else if r.hovered() {
        ui.painter().rect_filled(rect, 6.0, hover);
    }
    if r.has_focus() {
        ui.painter().rect_stroke(rect, 6.0, crate::style::focus_ring(), egui::StrokeKind::Inside);
    }
    let ink = if active { pal().accent_ink } else { ink };
    crate::icons::paint(ui.painter(), icon, Rect::from_center_size(rect.center(), vec2(18.0, 18.0)), ink);
    r.on_hover_text(label)
}

impl BimApp {
    /// The navigation bar's buttons, top down: Pan (the hand), Zoom Region, then fit, in and out.
    pub(crate) fn nav_buttons(&mut self, ui: &mut Ui, bar: Rect, id: Id) {
        ui.scope_builder(UiBuilder::new().max_rect(bar.shrink(3.0)).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let mode = self.canvas.nav.mode;
            if button(ui, Icon::Pan, "Pan", mode == Mode::Pan).clicked() {
                self.toggle_pan();
            }
            if button(ui, Icon::ZoomRegion, "Zoom Region", mode == Mode::Region).clicked() {
                self.zoom_region();
            }
            if button(ui, Icon::Fit, "Zoom to Fit", false).clicked() {
                self.canvas.fit(id);
            }
            if button(ui, Icon::ZoomIn, "Zoom In", false).clicked() {
                self.canvas.zoom(id, 1.25);
            }
            if button(ui, Icon::ZoomOut, "Zoom Out", false).clicked() {
                self.canvas.zoom(id, 0.8);
            }
        });
    }

    /// Pan: the hand. The left button drags the view about until Pan is pressed again or Esc.
    pub(crate) fn toggle_pan(&mut self) {
        let nav = &mut self.canvas.nav;
        nav.mode = if nav.mode == Mode::Pan { Mode::Off } else { Mode::Pan };
        nav.region = None;
    }

    /// Zoom Region (ZR): drag a box on the view and it fills the view.
    pub(crate) fn zoom_region(&mut self) {
        let nav = &mut self.canvas.nav;
        nav.mode = if nav.mode == Mode::Region { Mode::Off } else { Mode::Region };
        nav.region = None;
    }

    /// Previous Zoom (ZP): the view as it was before its last zoom or pan.
    pub(crate) fn previous_zoom(&mut self) {
        let went = match self.sheet {
            crate::Sheet::Plan(id) | crate::Sheet::Ceiling(id) | crate::Sheet::Section(id) | crate::Sheet::Paper(id) => {
                self.canvas.previous(id)
            }
            _ => true,
        };
        if !went {
            self.status = Status { text: t("There is no earlier zoom to go back to").into(), error: false };
        }
    }

    /// Leave Pan or Zoom Region, when one is on. Whether one was.
    pub(crate) fn end_navigation(&mut self) -> bool {
        let nav = &mut self.canvas.nav;
        nav.region = None;
        std::mem::replace(&mut nav.mode, Mode::Off) != Mode::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOVE: TouchPhase = TouchPhase::Move;

    #[test]
    fn a_wheel_turning_notch_by_notch_is_a_mouse_and_a_trackpads_scroll_is_not() {
        let point = MouseWheelUnit::Point;
        // The notches of Windows and Linux browsers, and several at once.
        for dy in [120.0, -120.0, 100.0, -300.0, 360.0] {
            assert_eq!(hand_of(point, vec2(0.0, dy), MOVE), Hand::Mouse, "{dy}");
        }
        // Shift and the wheel: a notch along.
        assert_eq!(hand_of(point, vec2(120.0, 0.0), MOVE), Hand::Mouse);
        // A mouse that counts lines.
        assert_eq!(hand_of(MouseWheelUnit::Line, vec2(0.0, -3.0), MOVE), Hand::Mouse);
        // A trackpad: small steps, both ways at once, a precision touchpad's fractions of a line, any gesture's start.
        for d in [vec2(0.0, 7.0), vec2(0.0, -33.5), vec2(12.0, 40.0), vec2(-3.0, 0.0), vec2(0.0, 130.0)] {
            assert_eq!(hand_of(point, d, MOVE), Hand::Trackpad, "{d:?}");
        }
        assert_eq!(hand_of(MouseWheelUnit::Line, vec2(0.0, 0.2), MOVE), Hand::Trackpad);
        assert_eq!(hand_of(MouseWheelUnit::Line, vec2(0.5, 1.0), MOVE), Hand::Trackpad);
        assert_eq!(hand_of(point, vec2(0.0, 120.0), TouchPhase::Start), Hand::Trackpad);
        assert_eq!(hand_of(point, vec2(0.0, 120.0), TouchPhase::End), Hand::Trackpad);
        // Nonsense is no hand: it zooms as a notch does, and the view clamps it.
        assert_eq!(hand_of(point, vec2(0.0, 1e30), MOVE), Hand::Mouse);
        assert_eq!(hand_of(point, vec2(0.0, f32::NAN), MOVE), Hand::Mouse);
    }
}

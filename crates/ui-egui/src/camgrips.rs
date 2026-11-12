//! A camera in a plan as Revit shows it while its 3D view is selected (picked on the plan, or Project Browser › Show Camera):
//! its eye, its view cone out to the far clip plane with the far clip line across it, and its target, each with a grip. A drag
//! moves the eye (the target staying), aims the target, sets the far clip, or widens or narrows the field of view from a side;
//! on release, one `camera`, so one undo step. What each grip does is the core's (`render::camera_grip`); this draws and drags.

use egui::{Painter, Pos2, Rect, Response, Stroke, StrokeKind, Ui, vec2};
use serde_json::Value;

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model, ViewCamera};
use bimcraft_core::render::{self, Cone, View};

use crate::BimApp;
use crate::canvas::GRIP_REACH;
use crate::style::pal;

/// The selected camera views: their id, camera and focal length.
fn selected_cameras(m: &Model, selection: &[Id]) -> Vec<(Id, ViewCamera, f64)> {
    selection
        .iter()
        .filter_map(|id| match m.elements.get(*id) {
            Some(Element::View3D(v)) => Some((*id, v.camera?, v.perspective.map_or(bimcraft_core::view3d::LENS, |p| p.lens))),
            _ => None,
        })
        .collect()
}

/// A cone's grips in the order `render::camera_grip` takes them: eye, target, far clip, left side, right side. A Free
/// camera has no target to drag.
fn grips(c: &Cone, targeted: bool) -> Vec<(usize, P2)> {
    let mut out = vec![(0, c.eye)];
    if targeted {
        out.push((1, c.target));
    }
    out.extend([(2, c.far), (3, c.sides[0]), (4, c.sides[1])]);
    out
}

/// The camera as grip `which` dragged to `to` leaves it, for the preview: its points and settings, and its focal length.
fn moved(cam: &ViewCamera, lens: f64, which: usize, to: P2) -> (ViewCamera, f64) {
    let mut c = *cam;
    let mut lens = lens;
    if let Some(Value::Object(p)) = render::camera_grip(cam, which, to) {
        let point = |v: &Value| -> Option<[f64; 3]> { serde_json::from_value(v.clone()).ok() };
        if let Some(e) = p.get("eye").and_then(point) {
            c.eye = e;
        }
        if let Some(t) = p.get("target").and_then(point) {
            c.target = t;
        }
        if let Some(f) = p.get("far").and_then(Value::as_f64) {
            (c.physical.clip_on, c.physical.far) = (true, f);
        }
        if let Some(fov) = p.get("fov").and_then(Value::as_f64) {
            lens = bimcraft_core::view3d::lens_for(fov, &c.physical);
        }
    }
    (c, lens)
}

fn at(rect: Rect, view: &View, q: P2) -> Pos2 {
    let (x, y) = view.px(q);
    rect.min + vec2(x, y)
}

impl BimApp {
    /// A drag from a grip of a selected camera: held while it is dragged, then one `camera` where it is let go, the point
    /// snapped as any grip's. Whether it took the release, which is then no click.
    pub(crate) fn camera_grip_drag(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, level: Id) -> bool {
        let cams = selected_cameras(&self.session.model, &self.selection);
        if cams.is_empty() {
            self.camera_grip = None;
            return false;
        }
        let cones: Vec<(Id, ViewCamera, f64, Cone)> =
            cams.into_iter().filter_map(|(id, c, l)| Some((id, c, l, render::cone_of(&c, l)?))).collect();
        for (_, c, _, cone) in &cones {
            for (_, g) in grips(cone, c.physical.targeted) {
                self.control_rect(ui, Rect::from_center_size(at(rect, view, g), vec2(GRIP_REACH, GRIP_REACH)));
            }
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |q: P2| at(rect, view, q).distance(o);
            self.camera_grip = cones
                .iter()
                .flat_map(|(id, c, _, cone)| grips(cone, c.physical.targeted).into_iter().map(move |(k, g)| (*id, k, g)))
                .filter(|(.., g)| near(*g) <= 8.0)
                .min_by(|a, b| near(a.2).total_cmp(&near(b.2)))
                .map(|(id, k, _)| (id, k));
        }
        let Some((id, which)) = self.camera_grip else { return false };
        if !response.drag_stopped() {
            return false;
        }
        self.camera_grip = None;
        if self.canvas.quick() {
            return true;
        }
        let Some(q) = ui.input(|i| i.pointer.latest_pos()) else { return true };
        let m = std::sync::Arc::clone(&self.session.model);
        let to = self.camera_grip_point(&m, level, view, rect, which, q);
        if let Some((_, cam, ..)) = cones.iter().find(|c| c.0 == id)
            && let Some(Value::Object(mut p)) = render::camera_grip(cam, which, to)
        {
            p.insert("view".into(), serde_json::json!(id));
            self.run("camera", Value::Object(p));
        }
        true
    }

    /// Where grip `which` lands for the pointer at `q`: the eye and the target snapped as a plan's points are, the far clip and
    /// the sides where the pointer is.
    fn camera_grip_point(&self, m: &Model, level: Id, view: &View, rect: Rect, which: usize, q: Pos2) -> P2 {
        let local = (q.x - rect.min.x, q.y - rect.min.y);
        match which {
            0 | 1 => self.snap(m, level, view, local, &[], false).at,
            _ => view.mm(local),
        }
    }

    /// The selected cameras' cones and grips over the plan; the held one as the drag leaves it.
    pub(crate) fn camera_grip_overlay(&self, ui: &Ui, m: &Model, view: &View, rect: Rect, level: Id) {
        let cams = selected_cameras(m, &self.selection);
        if cams.is_empty() {
            return;
        }
        let p = ui.painter_at(rect);
        let cursor = self.camera_grip.and_then(|_| ui.input(|i| i.pointer.latest_pos()));
        for (id, cam, lens) in cams {
            let (cam, lens) = match (self.camera_grip, cursor) {
                (Some((held, which)), Some(q)) if held == id => moved(&cam, lens, which, self.camera_grip_point(m, level, view, rect, which, q)),
                _ => (cam, lens),
            };
            let Some(cone) = render::cone_of(&cam, lens) else { continue };
            draw_cone(&p, rect, view, &cone, cam.physical.targeted, self.camera_grip.filter(|g| g.0 == id).map(|g| g.1));
        }
    }
}

/// A cone's lines and grips: the sides and the far line in the selection's colour, a dashed line to the target, a square
/// grip at each point (the held one filled), the target a round one, as Revit's target point is.
fn draw_cone(p: &Painter, rect: Rect, view: &View, c: &Cone, targeted: bool, held: Option<usize>) {
    let accent = pal().accent;
    let line = Stroke::new(1.5, accent);
    let s = |q: P2| at(rect, view, q);
    for [a, b] in render::cone_lines(c).into_iter().take(3) {
        p.line_segment([s(a), s(b)], line);
    }
    if targeted {
        p.extend(egui::Shape::dashed_line(&[s(c.eye), s(c.target)], Stroke::new(1.0, accent), 6.0, 4.0));
    }
    for (k, g) in grips(c, targeted) {
        let fill = if held == Some(k) { accent } else { pal().surface };
        if k == 1 {
            p.circle_filled(s(g), 5.0, fill);
            p.circle_stroke(s(g), 5.0, line);
        } else {
            let r = Rect::from_center_size(s(g), vec2(9.0, 9.0));
            p.rect_filled(r, 1.0, fill);
            p.rect_stroke(r, 1.0, line, StrokeKind::Middle);
        }
    }
}

impl BimApp {
    /// A selected camera in section or elevation `section`, as Revit shows it there: its eye and its target at their heights,
    /// the line between them, each a grip a drag raises or lowers (Eye Elevation, Target Elevation); one `camera` when let go.
    /// The shapes to draw over the view.
    pub(crate) fn camera_section_grips(&mut self, ui: &Ui, response: &Response, rect: Rect, view: &View, section: Id) -> Vec<egui::Shape> {
        let m = std::sync::Arc::clone(&self.session.model);
        let Some(sv) = m.section(section) else { return Vec::new() };
        let cams = selected_cameras(&m, &self.selection);
        if cams.is_empty() {
            self.camera_section_grip = None;
            return Vec::new();
        }
        let u = (sv.end - sv.start).unit();
        let along = |p: [f64; 3]| (P2(p[0], p[1]) - sv.start).dot(u);
        let at = |s: f64, z: f64| self::at(rect, view, P2(s, z));
        let points: Vec<(Id, usize, f64, f64)> =
            cams.iter().flat_map(|(id, c, _)| [(*id, 0, along(c.eye), c.eye[2]), (*id, 1, along(c.target), c.target[2])]).collect();
        for (.., s, z) in &points {
            self.control_rect(ui, Rect::from_center_size(at(*s, *z), vec2(GRIP_REACH, GRIP_REACH)));
        }
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(o) = ui.input(|i| i.pointer.press_origin())
        {
            let near = |s: f64, z: f64| at(s, z).distance(o);
            self.camera_section_grip = points
                .iter()
                .filter(|(.., s, z)| near(*s, *z) <= 8.0)
                .min_by(|a, b| near(a.2, a.3).total_cmp(&near(b.2, b.3)))
                .map(|(id, k, ..)| (*id, *k));
        }
        // Where the held one is now: at the pointer's height, to the millimetre.
        let now = ui.input(|i| i.pointer.latest_pos()).map(|q| view.mm((q.x - rect.min.x, q.y - rect.min.y)).1.round());
        let held_now = self.camera_section_grip;
        let height = |id: Id, k: usize, z: f64| match (held_now, now) {
            (Some(held), Some(to)) if held == (id, k) => to,
            _ => z,
        };
        if response.drag_stopped()
            && let (Some((id, k)), Some(to)) = (self.camera_section_grip.take(), now)
            && !self.canvas.quick()
        {
            let key = if k == 0 { "eye_elevation" } else { "target_elevation" };
            self.run("camera", serde_json::json!({"view": id, key: to}));
        }
        let accent = pal().accent;
        let mut shapes = Vec::new();
        for (id, cam, _) in &cams {
            let eye = at(along(cam.eye), height(*id, 0, cam.eye[2]));
            let target = at(along(cam.target), height(*id, 1, cam.target[2]));
            shapes.push(egui::Shape::line_segment([eye, target], Stroke::new(1.5, accent)));
            let r = Rect::from_center_size(eye, vec2(9.0, 9.0));
            shapes.push(egui::Shape::rect_filled(r, 1.0, pal().surface));
            shapes.push(egui::Shape::rect_stroke(r, 1.0, Stroke::new(1.5, accent), StrokeKind::Middle));
            shapes.push(egui::Shape::circle_filled(target, 5.0, pal().surface));
            shapes.push(egui::Shape::circle_stroke(target, 5.0, Stroke::new(1.5, accent)));
        }
        shapes
    }
}

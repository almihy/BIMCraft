//! Framing a shot in a camera view: flying as Enscape does (the right button held, the pointer looks about the eye, W and S go
//! forward and back, A and D sideways, Q and E down and up, Shift faster, the wheel the speed), Walk (Shift+W, Revit's
//! SteeringWheels shortcut: the same without holding the button, the left button looking), the eye's height kept while
//! walking, and Safe Frame (Shift+F, 3ds Max's): the image's frame, the picture fitted in it, with the thirds drawn in it.
//! The camera's own moves are the core's (`Camera::look`, `Camera::walk`).

use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};

use bimcraft_core::view3d::Camera;

/// How fast a flight starts, mm a second: a brisk walk.
pub(crate) const SPEED: f64 = 3000.0;
/// The slowest and fastest a flight goes, mm a second.
const SLOWEST: f64 = 100.0;
const FASTEST: f64 = 200_000.0;
/// Degrees the view turns for each point the pointer moves.
const TURN: f64 = 0.25;
/// Points clear round the safe frame inside the view.
const MARGIN: f32 = 24.0;

/// The speed a turn of the wheel by `wheel` points makes of `speed`: a notch (about 50 points) a quarter faster or slower.
pub(crate) fn faster(speed: f64, wheel: f64) -> f64 {
    let k = 1.25f64.powf(wheel / 50.0);
    if k.is_finite() { (speed * k).clamp(SLOWEST, FASTEST) } else { speed }
}

/// `cam` turned about its eye as the pointer moved by `delta` points: right turns right, up looks up.
pub(crate) fn look(cam: Camera, delta: Vec2) -> Camera {
    cam.look(-f64::from(delta.x) * TURN, -f64::from(delta.y) * TURN)
}

/// `cam` moved for `dt` seconds with the flight's keys held (`[W, S, A, D, Q, E]`), at `speed` mm a second (four times it
/// with `fast`), level when the eye's height is kept.
pub(crate) fn step(cam: Camera, keys: [bool; 6], fast: bool, dt: f64, speed: f64, level: bool) -> Camera {
    let v = speed * if fast { 4.0 } else { 1.0 } * dt;
    let axis = |plus: bool, minus: bool| (f64::from(u8::from(plus)) - f64::from(u8::from(minus))) * v;
    let (forward, right, up) = (axis(keys[0], keys[1]), axis(keys[3], keys[2]), axis(keys[5], keys[4]));
    cam.walk(forward, right, up, level)
}

/// Where the eye of a parallel camera `cam` stands, `distance` mm back from its target the way it looks.
pub(crate) fn eye_of_parallel(cam: &Camera, distance: f64) -> [f64; 3] {
    let (a, e) = (cam.azimuth.to_radians(), cam.elevation.clamp(-89.0, 89.0).to_radians());
    let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
    std::array::from_fn(|k| cam.target[k] + back[k] * distance)
}

/// The image's frame in a view filling `rect`: as large as fits with a margin round it, at `aspect` (width over height),
/// in the middle.
pub(crate) fn safe_frame(rect: Rect, aspect: f64) -> Rect {
    let room = rect.shrink(MARGIN.min(rect.width() / 4.0).min(rect.height() / 4.0));
    let aspect = if aspect.is_finite() && aspect > 0.0 { aspect as f32 } else { 16.0 / 9.0 };
    let (w, h) = if room.width() / room.height().max(1.0) > aspect {
        (room.height() * aspect, room.height())
    } else {
        (room.width(), room.width() / aspect)
    };
    Rect::from_center_size(rect.center(), vec2(w.max(1.0), h.max(1.0)))
}

/// The safe frame over the picture: what lies outside it dimmed, its edge, and the thirds in it when asked.
pub(crate) fn draw_frame(p: &Painter, rect: Rect, frame: Rect, thirds: bool, ink: Color32, paper: Color32) {
    let dim = Color32::from_rgba_unmultiplied(paper.r(), paper.g(), paper.b(), 170);
    for r in [
        Rect::from_min_max(rect.min, pos2(rect.max.x, frame.min.y)),
        Rect::from_min_max(pos2(rect.min.x, frame.max.y), rect.max),
        Rect::from_min_max(pos2(rect.min.x, frame.min.y), pos2(frame.min.x, frame.max.y)),
        Rect::from_min_max(pos2(frame.max.x, frame.min.y), pos2(rect.max.x, frame.max.y)),
    ] {
        if r.width() > 0.0 && r.height() > 0.0 {
            p.rect_filled(r, 0.0, dim);
        }
    }
    p.rect_stroke(frame, 0.0, Stroke::new(1.0, ink), egui::StrokeKind::Middle);
    if thirds {
        let faint = Stroke::new(1.0, Color32::from_rgba_unmultiplied(ink.r(), ink.g(), ink.b(), 110));
        for k in [1.0f32 / 3.0, 2.0 / 3.0] {
            let x = frame.min.x + frame.width() * k;
            let y = frame.min.y + frame.height() * k;
            p.line_segment([pos2(x, frame.min.y), pos2(x, frame.max.y)], faint);
            p.line_segment([pos2(frame.min.x, y), pos2(frame.max.x, y)], faint);
        }
    }
}

/// The thirds' crossings of `frame`, where a shot's subject is put.
#[cfg(test)]
pub(crate) fn third_points(frame: Rect) -> [Pos2; 4] {
    let at = |a: f32, b: f32| pos2(frame.min.x + frame.width() * a, frame.min.y + frame.height() * b);
    [at(1.0 / 3.0, 1.0 / 3.0), at(2.0 / 3.0, 1.0 / 3.0), at(1.0 / 3.0, 2.0 / 3.0), at(2.0 / 3.0, 2.0 / 3.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_safe_frame_fits_the_image_s_aspect_in_the_view() {
        let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 500.0));
        let f = safe_frame(rect, 16.0 / 9.0);
        assert!((f.width() / f.height() - 16.0 / 9.0).abs() < 1e-3);
        assert!(rect.contains_rect(f) && (f.center() - rect.center()).length() < 1e-3);
        let tall = safe_frame(rect, 0.7);
        assert!((tall.height() - (500.0 - 2.0 * MARGIN)).abs() < 1e-3);
        assert!((third_points(f)[0] - pos2(f.min.x + f.width() / 3.0, f.min.y + f.height() / 3.0)).length() < 1e-3);
    }

    #[test]
    fn the_wheel_changes_the_flight_s_speed_within_its_limits() {
        assert!(faster(SPEED, 50.0) > SPEED && faster(SPEED, -50.0) < SPEED);
        assert_eq!(faster(SLOWEST, -1e6), SLOWEST);
        assert_eq!(faster(SPEED, f64::NAN), SPEED);
    }
}

impl crate::BimApp {
    /// Walk (Shift+W): flying without holding the right button, in a camera view in perspective; again to stop.
    pub(crate) fn toggle_walk(&mut self) {
        let m = &self.session.model;
        let camera = self.active_3d().and_then(|id| match m.elements.get(id) {
            Some(bimcraft_core::model::Element::View3D(v)) => Some(v.camera.is_some() && v.lens().is_some()),
            _ => None,
        });
        if camera == Some(true) {
            self.orbit.walk = !self.orbit.walk;
        } else {
            self.status = crate::Status { text: crate::lang::t("Walk is for a camera view in perspective").into(), error: true };
        }
    }
}

impl crate::BimApp {
    /// A camera view's own buttons on its view control bar: Safe Frame (Shift+F) and its thirds, Two-Point Perspective
    /// (Auto Vertical Tilt Correction, SketchUp's toggle), Walk (Shift+W) and the eye's height kept while walking.
    pub(crate) fn camera_bar(&mut self, ui: &mut egui::Ui) {
        use crate::icons::Icon;
        use bimcraft_i18n::msg;
        let m = &self.session.model;
        let Some((id, cam, perspective)) = self.active_3d().and_then(|id| match m.elements.get(id) {
            Some(bimcraft_core::model::Element::View3D(v)) => Some((id, v.camera?, v.lens().is_some())),
            _ => None,
        }) else {
            return;
        };
        let tip = msg!("Safe Frame (Shift+F): the image the camera renders, its Output Size, framed in the view");
        if crate::small_button(ui, Icon::Crop, "Safe Frame", self.orbit.safe_frame, tip).clicked() {
            self.orbit.safe_frame = !self.orbit.safe_frame;
        }
        let tip = msg!("Rule of Thirds: lines across the safe frame at its thirds, where a shot's subject goes");
        if self.orbit.safe_frame && crate::small_button(ui, Icon::Grid, "Rule of Thirds", self.orbit.thirds, tip).clicked() {
            self.orbit.thirds = !self.orbit.thirds;
        }
        let tip = msg!("Two-Point Perspective: verticals stay vertical however the camera looks up or down (Auto Vertical Tilt Correction)");
        if crate::small_button(ui, Icon::Level, "Two-Point Perspective", cam.physical.auto_tilt, tip).clicked() {
            self.run("camera", serde_json::json!({"view": id, "auto_tilt": !cam.physical.auto_tilt}));
            self.orbit.camera = None;
            self.orbit.built = None;
        }
        if perspective {
            let tip = msg!(
                "Walk (Shift+W): W, A, S and D move, Q and E go down and up, the left button looks, Shift is faster, the wheel sets the speed; Esc stops. Holding the right button flies the same way at any time"
            );
            if crate::small_button(ui, Icon::Pan, "Walk", self.orbit.walk, tip).clicked() {
                self.toggle_walk();
            }
            let tip = msg!("Keep Eye Height: walking forward stays at the eye's height instead of climbing or falling as the camera looks");
            if crate::small_button(ui, Icon::Elevation, "Keep Eye Height", self.orbit.keep_height, tip).clicked() {
                self.orbit.keep_height = !self.orbit.keep_height;
            }
        }
    }
}

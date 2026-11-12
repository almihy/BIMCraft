//! A 3D view's camera in Properties, for the view the 3D tab shows or for a camera view picked on a plan. A default view
//! shows Projection (Axonometric or Perspective), its Focal Length and the eye's height on screen. A camera view shows Revit's
//! Camera group (Projection Mode, Eye Elevation, Target Elevation, Camera Position) and then 3ds Max's Physical Camera
//! rollouts: Basic, Physical Camera, Exposure, Perspective Control, Clipping, and Output Size (Render Setup's). Each field
//! runs `camera` when it is typed (Enter, or clicking away), as the rest of the palette does. How a camera sees is the core's
//! (`view3d::Camera::through`).

use bimcraft_i18n::msg;

use egui::Ui;
use egui::{WidgetInfo, WidgetType};
use serde_json::{Map, Value, json};

use bimcraft_core::model::{Element, FILM_PRESETS, Id, OUTPUT_PRESETS, ViewCamera};
use bimcraft_core::units::Units;
use bimcraft_core::view3d;

use crate::BimApp;
use crate::lang::t;

const TIP: &str = msg!("Perspective: see the 3D view as through a camera, its eye standing back as far as the lens and the zoom say");

/// What Projection lists: parallel, then through a camera.
const PROJECTIONS: [&str; 2] = [msg!("Axonometric"), msg!("Perspective")];
/// What a camera's Projection Mode lists, as Revit's.
const MODES: [&str; 2] = [msg!("Perspective"), msg!("Orthographic")];
/// Focus, as 3ds Max's: at the target's distance, or a distance of its own.
const FOCUS: [&str; 2] = [msg!("Use Target Distance"), msg!("Custom")];
/// Exposure, as 3ds Max's: by an exposure value, or by ISO, shutter and aperture.
const EXPOSURE: [&str; 2] = [msg!("Target (EV)"), msg!("Manual (ISO)")];

/// The rollouts' names, in order, as 3ds Max's.
const BASIC: &str = msg!("Basic");
const PHYSICAL: &str = msg!("Physical Camera");
const EXPOSURE_GROUP: &str = msg!("Exposure");
const PERSPECTIVE_CONTROL: &str = msg!("Perspective Control");
const CLIPPING: &str = msg!("Clipping");
const OUTPUT: &str = msg!("Output Size");

/// A number typed, with or without its unit.
fn number(s: &str, unit: &str) -> Option<f64> {
    s.trim().trim_end_matches(unit).trim().trim_start_matches("f/").trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A number shown without trailing zeros, to `places`.
fn shown(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

/// A shutter's duration as photographers write it: 1/125 s, or 2 s.
fn shutter_text(t: f64) -> String {
    if t < 1.0 { format!("1/{} s", shown(1.0 / t, 1)) } else { format!("{} s", shown(t, 2)) }
}

/// A shutter typed: `1/125`, `0.008` or `2 s`.
fn shutter_typed(s: &str) -> Option<f64> {
    let s = s.trim().trim_end_matches('s').trim();
    match s.split_once('/') {
        Some((a, b)) => Some(a.trim().parse::<f64>().ok()? / b.trim().parse::<f64>().ok()?),
        None => s.parse().ok(),
    }
    .filter(|v: &f64| v.is_finite() && *v > 0.0)
}

impl BimApp {
    /// A group's header, folded as the palette's are: whether it's open.
    fn rollout(&mut self, ui: &mut Ui, group: &'static str) -> bool {
        let open = !self.collapsed.contains(group);
        if crate::properties::group_header(ui, t(group), open) && !self.collapsed.remove(group) {
            self.collapsed.insert(group);
        }
        open
    }

    /// A checkbox row named `name`: the new state when it is clicked.
    fn tick(ui: &mut Ui, name: &str, now: bool) -> Option<bool> {
        let label = ui.label(t(name));
        let mut on = now;
        let r = ui.checkbox(&mut on, "");
        let named = t(name).to_string();
        r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, &named));
        let changed = r.labelled_by(label.id).changed();
        ui.end_row();
        changed.then_some(on)
    }

    /// A number row: what is typed there, read as `unit` (none: a plain number), once it is committed.
    fn number_row(&mut self, ui: &mut Ui, view: Id, name: &str, key: &str, text: String, unit: &str) -> Option<f64> {
        let label = ui.label(t(name));
        let typed = self.field(ui, view, key, text, label.id, "");
        ui.end_row();
        let typed = typed?;
        let v = number(&typed, unit);
        if v.is_none() {
            self.status = crate::Status { text: crate::lang::tf("`{}` is not a number", &[typed.trim()]), error: true };
        }
        v
    }

    /// A length row, in the project's units.
    fn length_row(&mut self, ui: &mut Ui, view: Id, name: &str, key: &str, mm: f64, units: Units) -> Option<f64> {
        let label = ui.label(t(name));
        let typed = self.field(ui, view, key, units.length(mm), label.id, "");
        ui.end_row();
        let typed = typed?;
        let v = units.parse(&typed);
        if v.is_none() {
            self.status = crate::Status { text: crate::lang::tf("`{}` is not a length", &[typed.trim()]), error: true };
        }
        v
    }

    /// The Camera properties of 3D view `view`: the view the 3D tab shows, or a camera view picked on a plan (None for the
    /// default `{3D}` before it exists).
    pub(crate) fn camera_properties(&mut self, ui: &mut Ui, view: Option<Id>) {
        let (lens, focal, camera, name, is_default, units) = {
            let model = &self.session.model;
            let (lens, focal, camera, name) = match view.and_then(|id| model.elements.get(id)) {
                // The default `{3D}` keeps its name: renaming it would leave the 3D tab without its own view.
                Some(Element::View3D(v)) => (
                    v.lens(),
                    v.perspective.map_or(view3d::LENS, |p| p.lens),
                    v.camera,
                    if v.name == "{3D}" { None } else { Some(v.name.clone()) },
                ),
                _ => (None, view3d::LENS, None, None),
            };
            let is_default = camera.is_none() && (view.is_none() || view == model.default_3d());
            (lens, focal, camera, name, is_default, model.units)
        };
        match (camera, view) {
            (Some(cam), Some(id)) => self.camera_view_properties(ui, id, name, cam, focal, lens.is_some(), units),
            _ => self.default_camera_properties(ui, view, name, lens, is_default, units),
        }
    }

    /// A view without a camera: its name, Projection, Focal Length and the eye's height on screen.
    fn default_camera_properties(
        &mut self,
        ui: &mut Ui,
        view: Option<Id>,
        name: Option<String>,
        lens: Option<f64>,
        is_default: bool,
        units: Units,
    ) {
        if !self.rollout(ui, msg!("Camera")) {
            return;
        }
        let widget = view.unwrap_or_default();
        let eye_elevation = self.orbit.camera.and_then(|c| c.eye_elevation());
        let (mut set, mut raise, mut renamed) = (None, None, None);
        crate::properties::grid(ui, "camera-properties", |ui| {
            if let Some(name) = name {
                let label = ui.label(t("View Name"));
                if let Some(typed) = self.field(ui, widget, "name", name, label.id, "") {
                    renamed = Some(typed);
                }
                ui.end_row();
            }
            if is_default {
                let label = ui.label(t("Projection")).on_hover_text(t(TIP));
                let options: Vec<String> = PROJECTIONS.iter().map(|p| t(p).to_string()).collect();
                let current = &options[usize::from(lens.is_some())];
                if let Some(picked) = crate::properties::combo(ui, (widget, "camera.projection"), label.id, current, &options) {
                    set = Some(json!({"on": picked == options[1]}));
                }
                ui.end_row();
            }
            if let Some(lens) = lens
                && let Some(mm) = self.number_row(ui, widget, "Focal Length", "camera.lens", format!("{lens} mm"), "mm")
            {
                set = Some(json!({"lens": mm}));
            }
            if is_default
                && let Some(mm) = self.length_row(ui, widget, "Eye Elevation", "camera.eye", eye_elevation.unwrap_or_default(), units)
            {
                raise = Some(mm);
            }
        });
        if let (Some(id), Some(new)) = (view, renamed) {
            self.run("modify", json!({"id": id, "name": new}));
        }
        if let Some(p) = set {
            self.run("perspective", p);
        }
        if let (Some(z), Some(c)) = (raise, self.orbit.camera) {
            self.orbit.camera = Some(c.with_eye_elevation(z));
        }
    }

    /// A camera view: Revit's Camera group, then 3ds Max's Physical Camera rollouts. Every field typed runs `camera`.
    #[allow(clippy::too_many_arguments)]
    fn camera_view_properties(&mut self, ui: &mut Ui, id: Id, name: Option<String>, cam: ViewCamera, focal: f64, on: bool, units: Units) {
        let ph = cam.physical;
        let mut set: Map<String, Value> = Map::new();
        let mut renamed = None;
        let mut put = |k: &str, v: Value| {
            set.insert(k.to_string(), v);
        };
        let choose = |ui: &mut Ui, label: &str, key: &str, options: &[&str], current: usize| -> Option<usize> {
            let l = ui.label(t(label));
            let names: Vec<String> = options.iter().map(|o| t(o).to_string()).collect();
            let picked = crate::properties::combo(ui, (id, key), l.id, &names[current.min(names.len() - 1)], &names);
            ui.end_row();
            picked.and_then(|p| names.iter().position(|n| *n == p))
        };
        if self.rollout(ui, msg!("Camera")) {
            crate::properties::grid(ui, "camera-revit", |ui| {
                if let Some(name) = name {
                    let label = ui.label(t("View Name"));
                    renamed = self.field(ui, id, "name", name, label.id, "");
                    ui.end_row();
                }
                if let Some(k) = choose(ui, "Projection Mode", "camera.mode", &MODES, usize::from(!on)) {
                    put("projection", json!(if k == 0 { "perspective" } else { "orthographic" }));
                }
                if let Some(z) = self.length_row(ui, id, "Eye Elevation", "camera.eye_z", cam.eye[2], units) {
                    put("eye_elevation", json!(z));
                }
                if let Some(z) = self.length_row(ui, id, "Target Elevation", "camera.target_z", cam.target[2], units) {
                    put("target_elevation", json!(z));
                }
                // Read-only, as Revit's: an explicit camera in perspective, an adjusting one parallel.
                ui.label(t("Camera Position"));
                ui.label(t(if on { "Explicit" } else { "Adjusting" }));
                ui.end_row();
            });
        }
        if self.rollout(ui, BASIC) {
            crate::properties::grid(ui, "camera-basic", |ui| {
                if let Some(v) = Self::tick(ui, "Targeted", ph.targeted) {
                    put("targeted", json!(v));
                }
                if let Some(d) = self.length_row(ui, id, "Target Distance", "camera.distance", cam.target_distance(), units) {
                    put("target_distance", json!(d));
                }
            });
        }
        if self.rollout(ui, PHYSICAL) {
            crate::properties::grid(ui, "camera-physical", |ui| {
                let presets: Vec<&str> = FILM_PRESETS.iter().map(|(n, _)| *n).chain(["Custom"]).collect();
                let at = presets.iter().position(|n| *n == ph.film_preset()).unwrap_or(presets.len() - 1);
                if let Some(k) = choose(ui, "Film/Sensor", "camera.film_preset", &presets, at)
                    && let Some(name) = presets.get(k).filter(|n| **n != "Custom")
                {
                    put("film_preset", json!(name));
                }
                if let Some(w) = self.number_row(ui, id, "Width", "camera.film", format!("{} mm", shown(ph.film, 2)), "mm") {
                    put("film", json!(w));
                }
                // The focal length and the field of view, each driving the other across the film.
                if let Some(mm) = self.number_row(ui, id, "Focal Length", "camera.lens", format!("{} mm", shown(focal, 2)), "mm") {
                    put("lens", json!(mm));
                }
                let fov = view3d::field_of_view(focal, &ph);
                if let Some(deg) = self.number_row(ui, id, "Field of View", "camera.fov", format!("{}°", shown(fov, 2)), "°") {
                    put("fov", json!(deg));
                }
                if let Some(z) = self.number_row(ui, id, "Zoom", "camera.zoom", format!("{}x", shown(ph.zoom, 3)), "x") {
                    put("zoom", json!(z));
                }
                if let Some(n) = self.number_row(ui, id, "Aperture", "camera.f_number", format!("f/{}", shown(ph.f_number, 1)), "") {
                    put("f_number", json!(n));
                }
                if let Some(k) = choose(ui, "Focus", "camera.focus", &FOCUS, usize::from(ph.focus.is_some())) {
                    put("focus", if k == 0 { json!("target") } else { json!(cam.focus_distance()) });
                }
                if ph.focus.is_some()
                    && let Some(d) = self.length_row(ui, id, "Focus Distance", "camera.focus_distance", cam.focus_distance(), units)
                {
                    put("focus", json!(d));
                }
                if let Some(v) = Self::tick(ui, "Enable Depth of Field", ph.depth_of_field) {
                    put("depth_of_field", json!(v));
                }
            });
        }
        if self.rollout(ui, EXPOSURE_GROUP) {
            crate::properties::grid(ui, "camera-exposure", |ui| {
                if let Some(k) = choose(ui, "Exposure Gain", "camera.exposure", &EXPOSURE, usize::from(ph.iso.is_some())) {
                    put("exposure", json!(if k == 0 { "target" } else { "manual" }));
                }
                match ph.iso {
                    None => {
                        if let Some(ev) = self.number_row(ui, id, "EV", "camera.ev", shown(ph.ev, 2), "") {
                            put("ev", json!(ev));
                        }
                    }
                    Some(iso) => {
                        if let Some(v) = self.number_row(ui, id, "ISO", "camera.iso", shown(iso, 0), "") {
                            put("iso", json!(v));
                        }
                        let label = ui.label(t("Shutter"));
                        if let Some(typed) = self.field(ui, id, "camera.shutter", shutter_text(ph.shutter), label.id, "") {
                            match shutter_typed(&typed) {
                                Some(s) => put("shutter", json!(s)),
                                None => {
                                    self.status =
                                        crate::Status { text: crate::lang::tf("`{}` is not a shutter speed", &[typed.trim()]), error: true }
                                }
                            }
                        }
                        ui.end_row();
                        ui.label(t("EV"));
                        ui.label(shown(cam.exposure_value(), 2));
                        ui.end_row();
                    }
                }
            });
        }
        if self.rollout(ui, PERSPECTIVE_CONTROL) {
            crate::properties::grid(ui, "camera-perspective", |ui| {
                for (k, (name, key)) in
                    [("Lens Shift Horizontal", "camera.shift_h"), ("Lens Shift Vertical", "camera.shift_v")].into_iter().enumerate()
                {
                    if let Some(v) = self.number_row(ui, id, name, key, format!("{} %", shown(ph.shift[k], 2)), "%") {
                        let mut both = ph.shift;
                        both[k] = v;
                        put("shift", json!(both));
                    }
                }
                // 3ds Max's Tilt Correction Vertical, a slope; Auto sets it (shown greyed, as 3ds Max's), and an orthographic
                // camera, whose verticals are vertical already, takes none.
                let label = ui.label(t("Tilt Correction Vertical"));
                let free = on && !ph.auto_tilt;
                let typed =
                    ui.add_enabled_ui(free, |ui| self.field(ui, id, "camera.tilt_v", shown(cam.vertical_tilt(), 3), label.id, "")).inner;
                ui.end_row();
                if let Some(typed) = typed {
                    match number(&typed, "") {
                        Some(v) => put("tilt", json!(v)),
                        None => self.status = crate::Status { text: crate::lang::tf("`{}` is not a number", &[typed.trim()]), error: true },
                    }
                }
                let label = ui.label(t("Auto Vertical Tilt Correction"));
                let mut auto = ph.auto_tilt;
                let r = ui.add_enabled(on, egui::Checkbox::without_text(&mut auto));
                let named = t("Auto Vertical Tilt Correction").to_string();
                r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, on, auto, &named));
                if r.labelled_by(label.id).changed() {
                    put("auto_tilt", json!(auto));
                }
                ui.end_row();
            });
        }
        if self.rollout(ui, CLIPPING) {
            crate::properties::grid(ui, "camera-clipping", |ui| {
                if let Some(v) = Self::tick(ui, "Clip Manually", ph.clip_on) {
                    put("clip", json!(v));
                }
                if ph.clip_on {
                    if let Some(d) = self.length_row(ui, id, "Near Clip", "camera.near", ph.near, units) {
                        put("near", json!(d));
                    }
                    if let Some(d) = self.length_row(ui, id, "Far Clip", "camera.far", ph.far, units) {
                        put("far", json!(d));
                    }
                }
            });
        }
        if self.rollout(ui, OUTPUT) {
            crate::properties::grid(ui, "camera-output", |ui| {
                let presets: Vec<&str> = OUTPUT_PRESETS.iter().map(|(n, ..)| *n).chain(["Custom"]).collect();
                let at = OUTPUT_PRESETS.iter().position(|(_, w, h)| [*w, *h] == ph.output).unwrap_or(presets.len() - 1);
                if let Some(k) = choose(ui, "Preset", "camera.output_preset", &presets, at)
                    && let Some(name) = presets.get(k).filter(|n| **n != "Custom")
                {
                    put("output_preset", json!(name));
                }
                if let Some(w) = self.number_row(ui, id, "Width", "camera.output_w", format!("{} px", ph.output[0]), "px") {
                    put("output", if ph.aspect_locked { json!([w.round(), null]) } else { json!([w.round(), ph.output[1]]) });
                }
                if let Some(h) = self.number_row(ui, id, "Height", "camera.output_h", format!("{} px", ph.output[1]), "px") {
                    put("output", if ph.aspect_locked { json!([null, h.round()]) } else { json!([ph.output[0], h.round()]) });
                }
                ui.label(t("Image Aspect"));
                ui.label(shown(ph.aspect(), 3));
                ui.end_row();
                if let Some(v) = Self::tick(ui, "Lock Image Aspect", ph.aspect_locked) {
                    put("aspect_locked", json!(v));
                }
            });
        }
        if let Some(new) = renamed {
            self.run("modify", json!({"id": id, "name": new}));
        }
        if !set.is_empty() {
            set.insert("view".into(), json!(id));
            let changed = self.run("camera", Value::Object(set)).is_some();
            // The view on screen still shows the camera as it was: let the next frame stand it from the model, so the
            // settled hand doesn't write the old eye and target back over what was just typed.
            if changed && self.active_3d() == Some(id) {
                self.orbit.camera = None;
                self.orbit.built = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shutter_reads_as_photographers_write_it() {
        assert_eq!(shutter_text(1.0 / 125.0), "1/125 s");
        assert_eq!(shutter_typed("1/250"), Some(1.0 / 250.0));
        assert_eq!(shutter_typed("2 s"), Some(2.0));
        assert_eq!(shutter_typed("fast"), None);
        assert_eq!(number("f/2.8", ""), Some(2.8));
        assert_eq!(number("50 mm", "mm"), Some(50.0));
    }
}

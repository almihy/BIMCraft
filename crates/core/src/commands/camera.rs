//! The `camera` command: a camera view placed as Revit's Camera tool places one, then everything it holds set or read, as 3ds
//! Max's Physical Camera holds it (target or free, film, focal length or field of view, zoom, aperture, focus, exposure, lens
//! shift and tilt correction, clipping planes, the image's size). The eye and target are Revit's: dragged on a plan, raised as
//! Eye Elevation and Target Elevation.

use serde_json::{Value, json};

use super::{R, absent, bad, num, only, point3, string, view_param, view_title};
use crate::engine::Session;
use crate::model::{Element, FILM_PRESETS, MAX_OUTPUT, OUTPUT_PRESETS, Perspective, Physical, TOL, View3D, ViewCamera};
use crate::view3d;

/// Every parameter it takes.
const KEYS: [&str; 30] = [
    "view",
    "name",
    "eye",
    "target",
    "eye_elevation",
    "target_elevation",
    "target_distance",
    "targeted",
    "projection",
    "lens",
    "fov",
    "film",
    "film_preset",
    "zoom",
    "f_number",
    "depth_of_field",
    "focus",
    "exposure",
    "ev",
    "iso",
    "shutter",
    "shift",
    "tilt",
    "auto_tilt",
    "clip",
    "near",
    "far",
    "output",
    "output_preset",
    "aspect_locked",
];

/// What its params doc says, for agents.
pub(super) const DOC: &str = "{view?: a 3D view's name or id, name?, eye?: [x, y, z], target?: [x, y, z], eye_elevation?: mm, \
     target_elevation?: mm, target_distance?: mm, targeted?: true | false, projection?: \"perspective\" | \"orthographic\", lens?: \
     mm, fov?: degrees, film?: mm, film_preset?: name, zoom?, f_number?, depth_of_field?: true | false, focus?: mm | \"target\", \
     exposure?: \"target\" | \"manual\", ev?, iso?, shutter?: s, shift?: [h, v] %, tilt?: [h, v], auto_tilt?: true | false, clip?: \
     true | false, near?: mm, far?: mm, output?: [w, h] px, output_preset?: name, aspect_locked?: true | false} a 3D camera: Revit's \
     Camera tool places it, 3ds Max's Physical Camera holds its settings. With `eye` (where it stands, mm) and `target` (what it \
     looks at) and no `view` it makes a new perspective view named `name` or by number (\"Camera 1\"…). With `view` it changes \
     that 3D view's camera (`{3D}` may be given one; a view without a camera needs both points): what is left out stays. \
     `eye_elevation` and `target_elevation` (Revit's) raise or lower each; `target_distance` moves the target along the way it \
     looks. `targeted` false is a Free camera (no target to drag; the target is only how far it focuses). `projection` \
     orthographic sees it parallel. The lens: `lens`, the focal length (mm, 10 to 600; 35 for a new camera), or `fov`, the \
     horizontal field of view in degrees, each setting the other across `film`, the sensor's width (mm; or `film_preset`: \
     \"35mm (Full Frame)\" 36, \"APS-C (Canon)\" 22.3, \"APS-C (Nikon, Sony, etc.)\" 23.7, \"APS-H (Canon)\" 27.9, \"Four \
     Thirds\" 17.3), and `zoom` (over 1 narrows it). `f_number` is the aperture, `depth_of_field` blurs away from `focus` (mm \
     from the eye, or \"target\" for the target's distance). `exposure` target uses `ev`; manual uses `iso`, `shutter` and \
     `f_number` (giving `iso` makes it manual). Perspective Control: `shift`, the lens shift right and up as % of the film's \
     width (-100 to 100); `tilt`, the tilt correction (0 to 1; vertical 1 keeps verticals vertical); `auto_tilt` keeps the \
     vertical at 1, a two-point perspective. Clipping: `clip` on cuts away what is nearer than `near` or further than `far` (mm \
     from the eye; Revit's Far Clip). `output` is the image it renders, [width, height] px (16 to 16384), or `output_preset` \
     (\"HD 720\", \"Full HD 1080\", \"4K UHD\", \"A4 landscape\", \"A3 landscape\", \"A4 portrait\", \"Square\"); with \
     `aspect_locked` on a single side given keeps the aspect. With `view` alone it reads the camera back: every setting, the \
     field of view, the target distance, the focus distance and the exposure value worked out";

/// A true or false under `key`, if given.
fn yes_no(c: &str, p: &Value, key: &str) -> Result<Option<bool>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(v) => Err(bad(c, format!("`{key}` must be true or false, got {v}"))),
        None => Ok(None),
    }
}

/// Two numbers under `key`, if given.
fn pair(c: &str, p: &Value, key: &str, what: &str) -> Result<Option<[f64; 2]>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(v) => match v.as_array().map(Vec::as_slice) {
            Some([a, b]) => match (a.as_f64(), b.as_f64()) {
                (Some(a), Some(b)) if a.is_finite() && b.is_finite() => Ok(Some([a, b])),
                _ => Err(bad(c, format!("`{key}` must be {what}, got {v}"))),
            },
            _ => Err(bad(c, format!("`{key}` must be {what}, got {v}"))),
        },
        None => Ok(None),
    }
}

/// Everything a camera view holds, as the command reads it back.
pub(crate) fn read(id: Option<crate::model::Id>, name: &str, cam: Option<&ViewCamera>, pers: Option<Perspective>) -> Value {
    let lens = pers.map_or(view3d::LENS, |p| p.lens);
    let on = pers.is_some_and(|p| p.on);
    let Some(cam) = cam else {
        return json!({"view": id, "name": name, "eye": null, "target": null, "lens": lens, "perspective": on});
    };
    let ph = &cam.physical;
    let preset = OUTPUT_PRESETS.iter().find(|(_, w, h)| [*w, *h] == ph.output).map(|(n, ..)| *n);
    json!({
        "view": id,
        "name": name,
        "eye": cam.eye,
        "target": cam.target,
        "eye_elevation": cam.eye[2],
        "target_elevation": cam.target[2],
        "target_distance": cam.target_distance(),
        "targeted": ph.targeted,
        "projection": if on { "perspective" } else { "orthographic" },
        "perspective": on,
        "lens": lens,
        "fov": view3d::field_of_view(lens, ph),
        "film": ph.film,
        "film_preset": ph.film_preset(),
        "zoom": ph.zoom,
        "f_number": ph.f_number,
        "depth_of_field": ph.depth_of_field,
        "focus": ph.focus.map_or(json!("target"), |f| json!(f)),
        "focus_distance": cam.focus_distance(),
        "exposure": if ph.iso.is_some() { "manual" } else { "target" },
        "ev": cam.exposure_value(),
        "iso": ph.iso,
        "shutter": ph.shutter,
        "shift": ph.shift,
        "tilt": [ph.tilt[0], ph.vertical_tilt()],
        "auto_tilt": ph.auto_tilt,
        "clip": ph.clip_on,
        "near": ph.near,
        "far": ph.far,
        "output": ph.output,
        "output_preset": preset,
        "aspect": ph.aspect(),
        "aspect_locked": ph.aspect_locked,
    })
}

pub(super) fn camera(s: &mut Session, p: &Value) -> R {
    let c = "camera";
    only(c, p, &KEYS)?;
    let m = &s.model;
    let view = p.get("view");
    let named_view = !absent(view);
    // The default 3D view asked for by name: it is made when it's first given a camera, as when its perspective is first
    // turned; nothing exists yet to look up.
    let default_3d = matches!(view, Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    let v = match view {
        Some(_) if named_view && !default_3d => {
            let id = view_param(c, m, p, false)?;
            if !matches!(m.elements.get(id), Some(Element::View3D(_))) {
                return Err(bad(c, format!("{} isn't a 3D view; a camera goes on a 3D view", view_title(m, id))));
            }
            Some(id)
        }
        Some(_) if named_view && default_3d => m.default_3d(),
        _ => None,
    };
    let held = v.and_then(|id| match m.elements.get(id) {
        Some(Element::View3D(v3)) => Some(v3.clone()),
        _ => None,
    });
    // With a view and nothing else, read it. Reading makes nothing: the default `{3D}` before anything made it reads as a
    // view with no camera yet.
    let given = |k: &str| !absent(p.get(k));
    if named_view && KEYS.iter().filter(|k| **k != "view").all(|k| !given(k)) {
        let name = held.as_ref().map_or_else(|| "{3D}".to_string(), |v3| v3.name.clone());
        return Ok(read(v, &name, held.as_ref().and_then(|v3| v3.camera.as_ref()), held.as_ref().and_then(|v3| v3.perspective)));
    }
    let point = |key: &str| -> Result<Option<[f64; 3]>, String> {
        match p.get(key) {
            v if absent(v) => Ok(None),
            Some(v) => point3(v).map(Some).ok_or_else(|| bad(c, format!("`{key}` must be a point [x, y, z] in mm, got {v}"))),
            None => Ok(None),
        }
    };
    // The points: those given, else as the camera has them; a view without a camera yet needs both.
    let kept = held.as_ref().and_then(|v3| v3.camera);
    let (Some(mut eye), Some(mut target)) = (point("eye")?.or(kept.map(|k| k.eye)), point("target")?.or(kept.map(|k| k.target))) else {
        return Err(bad(c, "`eye` and `target` [x, y, z] are both required to place a camera"));
    };
    if let Some(z) = num(c, p, "eye_elevation")? {
        eye[2] = z;
    }
    if let Some(z) = num(c, p, "target_elevation")? {
        target[2] = z;
    }
    let mut ph = kept.map_or_else(Physical::default, |k| k.physical);
    // Target Distance: the target moved along the way the camera looks.
    if let Some(d) = num(c, p, "target_distance")? {
        let along: [f64; 3] = std::array::from_fn(|k| target[k] - eye[k]);
        let now = along.iter().map(|x| x * x).sum::<f64>().sqrt();
        if !(d > TOL) {
            return Err(bad(c, format!("`target_distance` is how far the target is from the eye, above zero, not {d}")));
        }
        if now > TOL {
            target = std::array::from_fn(|k| eye[k] + along[k] / now * d);
        }
    }
    let across = (0..3).map(|k| eye[k] - target[k]).fold(0.0, |acc, d| acc + d * d).sqrt();
    if across <= TOL {
        return Err(bad(c, "the eye is on the target; give the camera two points a little apart"));
    }
    if let Some(t) = yes_no(c, p, "targeted")? {
        ph.targeted = t;
    }
    // The film, before the lens: a field of view is across it.
    if let Some(name) = string(c, p, "film_preset")? {
        match FILM_PRESETS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())) {
            Some((_, w)) => ph.film = *w,
            None if name.trim().eq_ignore_ascii_case("custom") => {}
            None => {
                let names: Vec<&str> = FILM_PRESETS.iter().map(|(n, _)| *n).collect();
                return Err(bad(c, format!("`film_preset` is one of {} (or Custom), not `{name}`", names.join(", "))));
            }
        }
    }
    if let Some(w) = num(c, p, "film")? {
        ph.film = w;
    }
    if let Some(z) = num(c, p, "zoom")? {
        ph.zoom = z;
    }
    if let Some(n) = num(c, p, "f_number")? {
        ph.f_number = n;
    }
    if let Some(d) = yes_no(c, p, "depth_of_field")? {
        ph.depth_of_field = d;
    }
    match p.get("focus") {
        v if absent(v) => {}
        Some(Value::String(t)) if t.eq_ignore_ascii_case("target") => ph.focus = None,
        Some(v) => match v.as_f64() {
            Some(f) => ph.focus = Some(f),
            None => return Err(bad(c, format!("`focus` is mm from the eye, or \"target\" for the target's distance, not {v}"))),
        },
        None => {}
    }
    if let Some(e) = num(c, p, "ev")? {
        ph.ev = e;
    }
    if let Some(t) = num(c, p, "shutter")? {
        ph.shutter = t;
    }
    if let Some(iso) = num(c, p, "iso")? {
        ph.iso = Some(iso);
    }
    match string(c, p, "exposure")?.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None => {}
        Some("target") => ph.iso = None,
        Some("manual") => ph.iso = Some(ph.iso.unwrap_or(100.0)),
        Some(other) => return Err(bad(c, format!("`exposure` is \"target\" (by `ev`) or \"manual\" (by `iso`, `shutter` and `f_number`), not `{other}`"))),
    }
    if let Some(sh) = pair(c, p, "shift", "[horizontal, vertical] % of the film's width")? {
        ph.shift = sh;
    }
    if let Some(t) = pair(c, p, "tilt", "[horizontal, vertical], each 0 to 1")? {
        ph.tilt = t;
    }
    if let Some(a) = yes_no(c, p, "auto_tilt")? {
        ph.auto_tilt = a;
    }
    if let Some(on) = yes_no(c, p, "clip")? {
        ph.clip_on = on;
    }
    if let Some(n) = num(c, p, "near")? {
        ph.near = n;
    }
    if let Some(f) = num(c, p, "far")? {
        ph.far = f;
    }
    if let Some(l) = yes_no(c, p, "aspect_locked")? {
        ph.aspect_locked = l;
    }
    if let Some(name) = string(c, p, "output_preset")? {
        match OUTPUT_PRESETS.iter().find(|(n, ..)| n.eq_ignore_ascii_case(name.trim())) {
            Some((_, w, h)) => ph.output = [*w, *h],
            None => {
                let names: Vec<&str> = OUTPUT_PRESETS.iter().map(|(n, ..)| *n).collect();
                return Err(bad(c, format!("`output_preset` is one of {}, not `{name}`", names.join(", "))));
            }
        }
    }
    match p.get("output") {
        v if absent(v) => {}
        Some(v) => {
            let side = |x: &Value| x.as_f64().filter(|n| n.fract() == 0.0 && (16.0..=f64::from(MAX_OUTPUT)).contains(n));
            let what = || bad(c, format!("`output` is [width, height] in px, each 16 to {MAX_OUTPUT} (one may be null with the aspect locked), not {v}"));
            let [w, h] = match v.as_array().map(Vec::as_slice) {
                Some([w, h]) => [w, h],
                _ => return Err(what()),
            };
            let aspect = ph.aspect();
            ph.output = match (side(w), side(h)) {
                (Some(w), Some(h)) => [w as u32, h as u32],
                (Some(w), None) if h.is_null() && ph.aspect_locked => [w as u32, (w / aspect).round().clamp(16.0, f64::from(MAX_OUTPUT)) as u32],
                (None, Some(h)) if w.is_null() && ph.aspect_locked => [(h * aspect).round().clamp(16.0, f64::from(MAX_OUTPUT)) as u32, h as u32],
                _ => return Err(what()),
            };
        }
        None => {}
    }
    // The lens: a focal length, or the field of view that makes one across the film.
    let was = held.as_ref().and_then(|v3| v3.perspective);
    let mut lens = num(c, p, "lens")?.or(was.map(|w| w.lens)).unwrap_or(view3d::LENS);
    if let Some(fov) = num(c, p, "fov")? {
        if !(fov > 0.0 && fov < 180.0) {
            return Err(bad(c, format!("`fov` is the horizontal field of view, above 0 and under 180 degrees, not {fov}")));
        }
        lens = (view3d::lens_for(fov, &ph) * 1000.0).round() / 1000.0;
    }
    if !(view3d::MIN_LENS..=view3d::MAX_LENS).contains(&lens) {
        let (lo, hi) = (view3d::MIN_LENS, view3d::MAX_LENS);
        return Err(bad(c, format!("a {lens} mm lens is no camera's; a camera's focal length is {lo} to {hi} mm")));
    }
    let on = match string(c, p, "projection")?.as_deref().map(str::to_ascii_lowercase).as_deref() {
        None => was.is_none_or(|w| w.on) || kept.is_none(),
        Some("perspective") => true,
        Some("orthographic") => false,
        Some(other) => return Err(bad(c, format!("`projection` is \"perspective\" or \"orthographic\", not `{other}`"))),
    };
    let set = ViewCamera { eye, target, physical: ph };
    let id = match v {
        Some(id) => id,
        // The default `{3D}` when asked for and not made yet keeps its own name; a new camera view is named by number.
        None if default_3d => s.model_mut().insert(Element::View3D(View3D { camera: Some(set), ..View3D::default_view() })),
        None => {
            let name = match string(c, p, "name")? {
                Some(n) => n,
                None => s.model.free_view_name(s.model.language.word("Camera"), true),
            };
            s.model_mut().insert(Element::View3D(View3D { name, camera: Some(set), ..View3D::default_view() }))
        }
    };
    if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(id) {
        v3.camera = Some(set);
        v3.perspective = Some(Perspective { lens, on });
    }
    if v.is_some()
        && let Some(n) = string(c, p, "name")?
        && let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(id)
    {
        v3.name = n;
    }
    let name = s.model.view_name(id).unwrap_or_default().to_string();
    let mut out = read(Some(id), &name, Some(&set), Some(Perspective { lens, on }));
    out["id"] = json!(id);
    Ok(out)
}

/// What its params doc says, for agents.
pub(super) const ORIENTATION_DOC: &str = "{view?: a 3D view's name or id ({3D} unless given), azimuth?: degrees, elevation?: degrees,      target?: [x, y, z], across?: mm, fitted?: true} where a 3D view stands, as Revit's views keep their orientation: seen from      `azimuth` degrees round from east (counter-clockwise) and `elevation` degrees up, the model point `target` in the middle of      its picture and `across` mm of the model taking up its width there (through its lens in perspective). The window writes it      back as the view is turned, panned or zoomed, so `render` of the view draws what the person sees. `fitted` true forgets where      it was left, so it is fitted over the model again. A camera view stands where its camera is (`camera`). With `view` alone (or      nothing) it reads back everything the view's picture depends on: its orientation, its camera (eye, target, lens and the rest,      or null), its section box, its visual style, and `revision`, a number that changes whenever any of them or its      Visibility/Graphics does, so a render made at one revision is out of date at another";

/// A short, stable number for `text`: FNV-1a, 64 bits.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

pub(super) fn view_orientation(s: &mut Session, p: &Value) -> R {
    let c = "view_orientation";
    only(c, p, &["view", "azimuth", "elevation", "target", "across", "fitted"])?;
    let m = &s.model;
    let default = absent(p.get("view"))
        || matches!(p.get("view"), Some(Value::String(n)) if n.eq_ignore_ascii_case("{3D}") || n.eq_ignore_ascii_case("3D"));
    let v = if default {
        m.default_3d()
    } else {
        let id = view_param(c, m, p, false)?;
        if !matches!(m.elements.get(id), Some(Element::View3D(_))) {
            return Err(bad(c, format!("{} isn't a 3D view", view_title(m, id))));
        }
        Some(id)
    };
    let azimuth = num(c, p, "azimuth")?;
    let elevation = num(c, p, "elevation")?;
    let across = num(c, p, "across")?;
    let target = match p.get("target") {
        t if absent(t) => None,
        Some(t) => Some(point3(t).ok_or_else(|| bad(c, format!("`target` must be a point [x, y, z] in mm, got {t}")))?),
        None => None,
    };
    let fitted = match p.get("fitted") {
        f if absent(f) => false,
        Some(Value::Bool(b)) => *b,
        Some(f) => return Err(bad(c, format!("`fitted` must be true or false, got {f}"))),
        None => false,
    };
    let setting = azimuth.is_some() || elevation.is_some() || across.is_some() || target.is_some() || fitted;
    if setting {
        if elevation.is_some_and(|e| !(-90.0..=90.0).contains(&e)) {
            return Err(bad(c, "`elevation` is degrees up from the horizon, -90 to 90"));
        }
        let held = v.and_then(|id| match m.elements.get(id) {
            Some(Element::View3D(v3)) => Some(v3.clone()),
            _ => None,
        });
        let was = held.as_ref().and_then(|v3| v3.framing);
        let framing = match (target.or(was.map(|f| f.target)), across.or(was.map(|f| f.across))) {
            _ if fitted => None,
            (Some(target), Some(across)) => Some(crate::model::Framing { target, across }),
            (None, None) => None,
            _ => return Err(bad(c, "a view not left anywhere yet needs both `target` and `across`")),
        };
        let id = match v {
            Some(id) => id,
            None => s.model_mut().insert(Element::View3D(View3D::default_view())),
        };
        if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(id) {
            if let Some(a) = azimuth {
                v3.azimuth = a;
            }
            if let Some(e) = elevation {
                v3.elevation = e;
            }
            v3.framing = framing;
        }
        return Ok(orientation_of(&s.model, Some(id)));
    }
    Ok(orientation_of(m, v))
}

/// Everything 3D view `v`'s picture depends on, and its revision.
pub(crate) fn orientation_of(m: &crate::model::Model, v: Option<crate::model::Id>) -> Value {
    let held = v.and_then(|id| match m.elements.get(id) {
        Some(Element::View3D(v3)) => Some(v3.clone()),
        _ => None,
    });
    let view = held.clone().unwrap_or_else(View3D::default_view);
    let look = v.map_or_else(|| crate::graphics::Look::of_3d(m), |id| crate::graphics::Look::of(m, id));
    let graphics = serde_json::to_string(&*view.graphics.resolved(m)).unwrap_or_default();
    let revision = fnv(&format!("{}|{graphics}", serde_json::to_string(&view).unwrap_or_default()));
    json!({
        "view": v,
        "name": view.name,
        "azimuth": view.azimuth,
        "elevation": view.elevation,
        "target": view.framing.map(|f| f.target),
        "across": view.framing.map(|f| f.across),
        "fitted": view.framing.is_none() && view.camera.is_none(),
        "perspective": view.lens().is_some(),
        "lens": view.perspective.map(|p| p.lens),
        "camera": view.camera.as_ref().map(|cam| read(v, &view.name, Some(cam), view.perspective)),
        "section_box": view.section_box.map(|b| json!({"min": b.min, "max": b.max, "on": b.on})),
        "style": look.style().name(),
        "revision": revision.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::engine::Session;
    use crate::model::{Element, ViewCamera};
    use crate::view3d::Camera;

    fn placed() -> Session {
        let mut s = Session::new();
        s.execute("camera", &json!({"eye": [0.0, -10000.0, 1600.0], "target": [0.0, 0.0, 1600.0]})).unwrap();
        s
    }

    fn read(s: &mut Session) -> serde_json::Value {
        s.execute("camera", &json!({"view": "Camera 1"})).unwrap()
    }

    fn cam_of(s: &Session) -> ViewCamera {
        s.model
            .elements
            .iter()
            .find_map(|(_, e)| match e {
                Element::View3D(v) if v.name == "Camera 1" => v.camera,
                _ => None,
            })
            .unwrap()
    }

    /// The lens as 3ds Max's Physical Camera has it: a focal length, or a field of view across the film, each setting the
    /// other; a smaller film or a zoom narrows what a lens sees.
    #[test]
    fn a_camera_s_focal_length_and_field_of_view_set_each_other_across_its_film() {
        let mut s = placed();
        let r = read(&mut s);
        assert_eq!(r["lens"].as_f64(), Some(35.0));
        let fov = r["fov"].as_f64().unwrap();
        assert!((fov - 2.0 * (18.0f64 / 35.0).atan().to_degrees()).abs() < 1e-9, "{fov}");
        // 90 degrees across 36 mm of film is an 18 mm lens.
        let r = s.execute("camera", &json!({"view": "Camera 1", "fov": 90})).unwrap();
        assert!((r["lens"].as_f64().unwrap() - 18.0).abs() < 1e-3, "{r}");
        assert!((r["fov"].as_f64().unwrap() - 90.0).abs() < 1e-2);
        // A Four Thirds sensor behind the same lens sees less, and a zoom less again.
        let r = s.execute("camera", &json!({"view": "Camera 1", "film_preset": "Four Thirds"})).unwrap();
        assert_eq!((r["film"].as_f64(), r["film_preset"].as_str()), (Some(17.3), Some("Four Thirds")));
        let narrow = r["fov"].as_f64().unwrap();
        assert!(narrow < 90.0);
        let r = s.execute("camera", &json!({"view": "Camera 1", "zoom": 2})).unwrap();
        assert!(r["fov"].as_f64().unwrap() < narrow);
        // A custom width reads as Custom.
        let r = s.execute("camera", &json!({"view": "Camera 1", "film": 30})).unwrap();
        assert_eq!(r["film_preset"].as_str(), Some("Custom"));
        // The picture sees as wide as the field of view says: a point on its edge lands on the picture's edge.
        let r = s.execute("camera", &json!({"view": "Camera 1", "film": 36, "zoom": 1, "fov": 90})).unwrap();
        assert_eq!(r["lens"].as_f64(), Some(18.0));
        let cam = Camera::through(&cam_of(&s), 18.0, true, 400, 300);
        let (x, ..) = cam.project([5000.0, -5000.0, 1600.0]);
        assert!((x - 400.0).abs() < 0.5, "{x}");
    }

    /// Revit's Eye Elevation and Target Elevation raise each point; Target Distance moves the target along the line of sight.
    #[test]
    fn eye_and_target_elevation_and_target_distance_move_the_camera_s_points() {
        let mut s = placed();
        let r = s.execute("camera", &json!({"view": "Camera 1", "eye_elevation": 1700, "target_elevation": 4000})).unwrap();
        assert_eq!((r["eye"].clone(), r["target"].clone()), (json!([0.0, -10000.0, 1700.0]), json!([0.0, 0.0, 4000.0])));
        let r = s
            .execute("camera", &json!({"view": "Camera 1", "eye_elevation": 1600, "target_elevation": 1600, "target_distance": 5000}))
            .unwrap();
        assert_eq!(r["target"], json!([0.0, -5000.0, 1600.0]));
        assert_eq!(r["target_distance"].as_f64(), Some(5000.0));
        // Focus follows the target until a distance of its own is given.
        assert_eq!((r["focus"].as_str(), r["focus_distance"].as_f64()), (Some("target"), Some(5000.0)));
        let r = s.execute("camera", &json!({"view": "Camera 1", "focus": 3000, "depth_of_field": true, "f_number": 2.8})).unwrap();
        assert_eq!(
            (r["focus_distance"].as_f64(), r["depth_of_field"].as_bool(), r["f_number"].as_f64()),
            (Some(3000.0), Some(true), Some(2.8))
        );
        let r = s.execute("camera", &json!({"view": "Camera 1", "focus": "target"})).unwrap();
        assert_eq!(r["focus_distance"].as_f64(), Some(5000.0));
        // A Free camera, and an orthographic one, read back so.
        let r = s.execute("camera", &json!({"view": "Camera 1", "targeted": false, "projection": "orthographic"})).unwrap();
        assert_eq!((r["targeted"].as_bool(), r["projection"].as_str()), (Some(false), Some("orthographic")));
    }

    /// Exposure as 3ds Max's: Target by an exposure value, or Manual by ISO, shutter and aperture, whose EV is worked out.
    #[test]
    fn exposure_is_an_ev_or_worked_out_from_iso_shutter_and_aperture() {
        let mut s = placed();
        let r = read(&mut s);
        assert_eq!((r["exposure"].as_str(), r["ev"].as_f64()), (Some("target"), Some(6.0)));
        let r = s.execute("camera", &json!({"view": "Camera 1", "ev": 12.5})).unwrap();
        assert_eq!(r["ev"].as_f64(), Some(12.5));
        // f/8 at 1/125 s and ISO 100: log2(64 × 125).
        let r = s.execute("camera", &json!({"view": "Camera 1", "iso": 100, "f_number": 8, "shutter": 0.008})).unwrap();
        assert_eq!(r["exposure"].as_str(), Some("manual"));
        assert!((r["ev"].as_f64().unwrap() - (64.0f64 / 0.008).log2()).abs() < 1e-9);
        let r = s.execute("camera", &json!({"view": "Camera 1", "exposure": "target"})).unwrap();
        assert_eq!((r["exposure"].as_str(), r["ev"].as_f64()), (Some("target"), Some(12.5)));
    }

    /// Output Size as Render Setup's: a size in px, a preset, and the aspect lock keeping the image's proportion when one side
    /// is given.
    #[test]
    fn output_size_takes_presets_and_keeps_its_aspect_when_locked() {
        let mut s = placed();
        assert_eq!(read(&mut s)["output"], json!([1920, 1080]));
        let r = s.execute("camera", &json!({"view": "Camera 1", "output_preset": "4K UHD"})).unwrap();
        assert_eq!((r["output"].clone(), r["output_preset"].as_str()), (json!([3840, 2160]), Some("4K UHD")));
        let r = s.execute("camera", &json!({"view": "Camera 1", "aspect_locked": true, "output": [1280, null]})).unwrap();
        assert_eq!(r["output"], json!([1280, 720]));
        let r = s.execute("camera", &json!({"view": "Camera 1", "output": [null, 1080]})).unwrap();
        assert_eq!(r["output"], json!([1920, 1080]));
        // Unlocked, both sides are needed; and the sides stay in range.
        s.execute("camera", &json!({"view": "Camera 1", "aspect_locked": false})).unwrap();
        for bad in [json!([1280, null]), json!([8, 8]), json!([100000, 1000]), json!("big")] {
            assert!(s.execute("camera", &json!({"view": "Camera 1", "output": bad})).is_err(), "{bad}");
        }
        // A render of the camera without a size is its output.
        s.execute("camera", &json!({"view": "Camera 1", "output": [320, 200]})).unwrap();
        let r = s.execute("render", &json!({"view": "Camera 1"})).unwrap();
        assert_eq!((r["width"].as_u64(), r["height"].as_u64()), (Some(320), Some(200)));
    }

    /// Perspective Control: with Auto Vertical Tilt Correction a camera looking up keeps verticals vertical and what it looks
    /// at in the middle; a lens shift moves the picture without turning the camera.
    #[test]
    fn auto_vertical_tilt_correction_keeps_verticals_vertical_and_lens_shift_moves_the_picture() {
        let mut s = placed();
        s.execute("camera", &json!({"view": "Camera 1", "target_elevation": 8000})).unwrap();
        let column = |cam: &Camera| {
            let (a, b) = (cam.project([3000.0, 0.0, 0.0]), cam.project([3000.0, 0.0, 9000.0]));
            (a.0 - b.0).abs()
        };
        let tilted = Camera::through(&cam_of(&s), 35.0, true, 400, 300);
        assert!(column(&tilted) > 5.0, "looking up, a column leans in: {}", column(&tilted));
        s.execute("camera", &json!({"view": "Camera 1", "auto_tilt": true})).unwrap();
        let cam = cam_of(&s);
        let upright = Camera::through(&cam, 35.0, true, 400, 300);
        assert!(column(&upright) < 1e-6, "with the tilt corrected it stands upright: {}", column(&upright));
        let (x, y, _) = upright.project(cam.target);
        assert!((x - 200.0).abs() < 1e-6 && (y - 150.0).abs() < 1e-6, "the target stays in the middle: {x} {y}");
        let eye = upright.eye().unwrap();
        assert!((0..3).all(|k| (eye[k] - cam.eye[k]).abs() < 1e-6), "the eye stays: {eye:?}");
        assert_eq!(read(&mut s)["tilt"], json!([0.0, 1.0]));
        // A lens shift of 10% up moves the picture 10% of its width up the film: what shows moves down.
        s.execute("camera", &json!({"view": "Camera 1", "shift": [0, 10]})).unwrap();
        let shifted = Camera::through(&cam_of(&s), 35.0, true, 400, 300);
        let (_, y2, _) = shifted.project(cam.target);
        assert!((y2 - (150.0 + 40.0)).abs() < 1e-6, "{y2}");
        assert!(s.execute("camera", &json!({"view": "Camera 1", "shift": [0, 400]})).is_err());
        assert!(s.execute("camera", &json!({"view": "Camera 1", "tilt": [0, 2]})).is_err());
    }

    /// Clipping as 3ds Max's Clip Manually, which is Revit's Far Clip too: what lies past the far plane is cut away from the
    /// picture.
    #[test]
    fn clipping_planes_cut_away_what_is_further() {
        let mut s = Session::new();
        s.execute("floor", &json!({"boundary": [[-3000.0, 3000.0], [3000.0, 3000.0], [3000.0, 9000.0], [-3000.0, 9000.0]]})).unwrap();
        s.execute("camera", &json!({"eye": [0.0, -5000.0, 1600.0], "target": [0.0, 6000.0, 0.0]})).unwrap();
        let picture = |s: &mut Session| {
            s.execute("render", &json!({"view": "Camera 1", "width": 200, "height": 150})).unwrap()["pngBase64"].clone()
        };
        let whole = picture(&mut s);
        s.execute("camera", &json!({"view": "Camera 1", "clip": true, "far": 20000})).unwrap();
        assert_eq!(picture(&mut s), whole, "the far plane past the floor cuts nothing");
        s.execute("camera", &json!({"view": "Camera 1", "far": 2000})).unwrap();
        assert_ne!(picture(&mut s), whole, "the far plane short of the floor cuts it away");
        let r = read(&mut s);
        assert_eq!((r["clip"].as_bool(), r["far"].as_f64()), (Some(true), Some(2000.0)));
        assert!(s.execute("camera", &json!({"view": "Camera 1", "near": 3000})).is_err(), "the near plane past the far one");
    }

    /// Strict params: a misspelled key is refused, and everything set is saved with the project.
    #[test]
    fn a_misspelled_setting_is_refused_and_the_settings_are_saved() {
        let mut s = placed();
        assert!(s.execute("camera", &json!({"view": "Camera 1", "focal": 50})).is_err());
        s.execute("camera", &json!({"view": "Camera 1", "f_number": 4, "auto_tilt": true, "output": [1000, 1000], "clip": true, "far": 9000}))
            .unwrap();
        let text = s.model.to_json().unwrap();
        let reopened = crate::model::Model::read(&text).unwrap().0;
        let again = reopened
            .elements
            .iter()
            .find_map(|(_, e)| match e {
                Element::View3D(v) if v.name == "Camera 1" => v.camera,
                _ => None,
            })
            .unwrap();
        assert_eq!(again, cam_of(&s));
        assert_eq!((again.physical.f_number, again.physical.auto_tilt, again.physical.far), (4.0, true, 9000.0));
    }
}

#[cfg(test)]
mod orientation_tests {
    use serde_json::json;

    use crate::engine::Session;

    fn house() -> Session {
        let mut s = Session::new();
        s.execute("floor", &json!({"boundary": [[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0], [0.0, 4000.0]]})).unwrap();
        s.execute("wall", &json!({"points": [[0.0, 0.0], [6000.0, 0.0], [6000.0, 4000.0]]})).unwrap();
        s
    }

    fn png(s: &mut Session, p: serde_json::Value) -> serde_json::Value {
        s.execute("render", &p).unwrap()["pngBase64"].clone()
    }

    /// Every 3D view keeps where it was left, as Revit's do: `render` of it draws that, not the default fitted view, and a
    /// view's own graphics and section box are in its picture.
    #[test]
    fn a_3d_view_renders_where_it_was_left_with_its_own_graphics_and_box() {
        let mut s = house();
        let fitted = png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160}));
        let r = s.execute("view_orientation", &json!({"view": "{3D}"})).unwrap();
        assert_eq!(r["fitted"].as_bool(), Some(true));
        let before = r["revision"].clone();
        let r = s
            .execute("view_orientation", &json!({"view": "{3D}", "azimuth": 30, "elevation": 20, "target": [3000.0, 2000.0, 0.0], "across": 9000}))
            .unwrap();
        assert_eq!((r["azimuth"].as_f64(), r["across"].as_f64(), r["fitted"].as_bool()), (Some(30.0), Some(9000.0), Some(false)));
        assert_ne!(r["revision"], before, "the revision follows the orientation");
        let left = png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160}));
        assert_ne!(left, fitted, "it draws where it was left");
        assert_eq!(png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160})), left, "and does so again");
        // Its own visual style and section box change the picture and the revision.
        let rev = s.execute("view_orientation", &json!({"view": "{3D}"})).unwrap()["revision"].clone();
        s.execute("visual_style", &json!({"view": "{3D}", "style": "wireframe"})).unwrap();
        let styled = s.execute("view_orientation", &json!({"view": "{3D}"})).unwrap();
        assert_eq!(styled["style"].as_str(), Some("Wireframe"));
        assert_ne!(styled["revision"], rev);
        assert_ne!(png(&mut s, json!({"view": "{3D}", "width": 240, "height": 160})), left);
        // A camera view's own box is its own: the default's leaves it alone.
        s.execute("camera", &json!({"eye": [-4000.0, -5000.0, 1600.0], "target": [3000.0, 2000.0, 1000.0]})).unwrap();
        let cam = png(&mut s, json!({"view": "Camera 1", "width": 240, "height": 160}));
        s.execute("section_box", &json!({"view": "{3D}", "min": [0.0, 0.0, 0.0], "max": [1000.0, 1000.0, 500.0]})).unwrap();
        assert_eq!(png(&mut s, json!({"view": "Camera 1", "width": 240, "height": 160})), cam, "the default's box is not the camera's");
        s.execute("section_box", &json!({"view": "Camera 1", "min": [0.0, 0.0, 0.0], "max": [1000.0, 1000.0, 500.0]})).unwrap();
        assert_ne!(png(&mut s, json!({"view": "Camera 1", "width": 240, "height": 160})), cam, "its own box cuts it");
        let read = s.execute("view_orientation", &json!({"view": "Camera 1"})).unwrap();
        assert!(read["camera"]["eye"].is_array() && read["section_box"]["on"].as_bool() == Some(true), "{read}");
        // Fitted again on asking, and refused nonsense.
        let r = s.execute("view_orientation", &json!({"view": "{3D}", "fitted": true})).unwrap();
        assert_eq!(r["fitted"].as_bool(), Some(true));
        assert!(s.execute("view_orientation", &json!({"view": "{3D}", "across": 5000})).is_err(), "a target is needed too");
        assert!(s.execute("view_orientation", &json!({"view": "{3D}", "elevation": 120})).is_err());
        assert!(s.execute("view_orientation", &json!({"view": "{3D}", "spin": 1})).is_err());
    }
}

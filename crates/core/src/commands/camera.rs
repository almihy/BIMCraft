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
     exposure?: \"target\" | \"manual\", ev?, iso?, shutter?: s, shift?: [h, v] %, tilt?: slope, auto_tilt?: true | false, clip?: \
     true | false, near?: mm, far?: mm, output?: [w, h] px, output_preset?: name, aspect_locked?: true | false} a 3D camera: Revit's \
     Camera tool places it, 3ds Max's Physical Camera holds its settings. With `eye` (where it stands, mm) and `target` (what it \
     looks at) and no `view` it makes a new perspective view named `name` or by number (\"Camera 1\"…). With `view` it changes \
     that 3D view's camera (`{3D}` may be given one; a view without a camera needs both points): what is left out stays. \
     `eye_elevation` and `target_elevation` (Revit's) raise or lower each; `target_distance` moves the target along the way it \
     looks. `targeted` false is a Free camera (no target to drag; the target is only how far it focuses). `projection` \
     orthographic sees it parallel. The lens: `lens`, the focal length (mm; 35 for a new camera), or `fov`, the horizontal field \
     of view in degrees, each setting the other across `film`, the sensor's width (mm; or `film_preset`: \
     \"35mm (Full Frame)\" 36, \"APS-C (Canon)\" 22.3, \"APS-C (Nikon, Sony, etc.)\" 23.7, \"APS-H (Canon)\" 27.9, \"Four \
     Thirds\" 17.3), and `zoom` (over 1 narrows it). Whatever the film, the lens is 10 to 600 mm as 35 mm film has it (`lens × zoom × 36 / film`), \
     a field of view of 3.4° to 121.9°: a small sensor goes as wide as a full frame, and a lens past that is refused. \
     `f_number` is the aperture, `depth_of_field` blurs away from `focus` (mm \
     from the eye, or \"target\" for the target's distance). `exposure` target uses `ev`; manual uses `iso`, `shutter` and \
     `f_number` (giving `iso` makes it manual). Perspective Control: `shift`, the lens shift right and up as % of the film's \
     width (-100 to 100); `tilt`, 3ds Max's Tilt Correction Vertical: the film plane tilted by that slope (0 for none; \
     the slope the camera looks up at, negative looking down, keeps verticals vertical), what it looks at staying in the \
     middle; `auto_tilt` sets it to that slope however the camera looks, a two-point perspective. In perspective only: an \
     orthographic camera's verticals are vertical already, and it neither tilts nor clips. Clipping: `clip` on cuts away what is nearer than `near` or further than `far` (mm \
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
        "tilt": cam.vertical_tilt(),
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
        if d.is_nan() || d <= TOL {
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
        Some(other) => {
            return Err(bad(
                c,
                format!("`exposure` is \"target\" (by `ev`) or \"manual\" (by `iso`, `shutter` and `f_number`), not `{other}`"),
            ));
        }
    }
    if let Some(sh) = pair(c, p, "shift", "[horizontal, vertical] % of the film's width")? {
        ph.shift = sh;
    }
    if let Some(t) = num(c, p, "tilt")? {
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
            let what = || {
                bad(
                    c,
                    format!("`output` is [width, height] in px, each 16 to {MAX_OUTPUT} (one may be null with the aspect locked), not {v}"),
                )
            };
            let [w, h] = match v.as_array().map(Vec::as_slice) {
                Some([w, h]) => [w, h],
                _ => return Err(what()),
            };
            let aspect = ph.aspect();
            ph.output = match (side(w), side(h)) {
                (Some(w), Some(h)) => [w as u32, h as u32],
                (Some(w), None) if h.is_null() && ph.aspect_locked => {
                    [w as u32, (w / aspect).round().clamp(16.0, f64::from(MAX_OUTPUT)) as u32]
                }
                (None, Some(h)) if w.is_null() && ph.aspect_locked => {
                    [(h * aspect).round().clamp(16.0, f64::from(MAX_OUTPUT)) as u32, h as u32]
                }
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
    // Held to its 35 mm equivalent, whatever the film: the one range the cone, the window and `render` all see through.
    view3d::lens_ok(lens, &ph).map_err(|e| bad(c, e))?;
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
pub(super) const ORIENTATION_DOC: &str = "{view?: a 3D view's name or id ({3D} unless given), azimuth?: degrees, elevation?: degrees,      target?: [x, y, z], across?: mm, fitted?: true, eye?: [x, y, z]} where a 3D view stands, as Revit's views keep their orientation: seen from      `azimuth` degrees round from east (counter-clockwise) and `elevation` degrees up, the model point `target` in the middle of      its picture and `across` mm of the model taking up its width there (through its lens in perspective). The window writes it      back as the view is turned, panned or zoomed, so `render` of the view draws what the person sees. `fitted` true forgets where      it was left, so it is fitted over the model again. A camera view stands where its camera is: \
     given `eye` and `target`, its camera stands there and looks there, the rest of it as it was (the window writes a flight \
     back so). Navigating is no undo step, as in Revit: the view is kept with the project, and undo leaves it where it was \
     turned; a camera's settings and its grips (`camera`) are undone as ever. With `view` alone (or      nothing) it reads back everything the view's picture depends on: its orientation, its camera (eye, target, lens and the rest,      or null), its section box, its visual style, and `revision`, a number that changes whenever any of them or its      Visibility/Graphics does, so a render made at one revision is out of date at another";

/// A short, stable number for `text`: FNV-1a, 64 bits.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

pub(super) fn view_orientation(s: &mut Session, p: &Value) -> R {
    let c = "view_orientation";
    only(c, p, &["view", "azimuth", "elevation", "target", "across", "fitted", "eye"])?;
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
    let eye = match p.get("eye") {
        e if absent(e) => None,
        Some(e) => Some(point3(e).ok_or_else(|| bad(c, format!("`eye` must be a point [x, y, z] in mm, got {e}")))?),
        None => None,
    };
    // A camera view: where its eye stands and what it looks at, nothing else of it.
    let camera = v.and_then(|id| match m.elements.get(id) {
        Some(Element::View3D(v3)) => v3.camera,
        _ => None,
    });
    if let Some(was) = camera {
        if azimuth.is_some() || elevation.is_some() || across.is_some() || fitted {
            return Err(bad(c, "a camera view stands where its camera is: give its `eye` and `target`"));
        }
        if eye.is_some() || target.is_some() {
            let (eye, target) = (eye.unwrap_or(was.eye), target.unwrap_or(was.target));
            if (0..3).map(|k| (eye[k] - target[k]).powi(2)).sum::<f64>().sqrt() <= TOL {
                return Err(bad(c, "the eye is on the target; give the camera two points a little apart"));
            }
            let id = v.unwrap_or_default();
            if let Some(Element::View3D(v3)) = s.model_mut().elements.get_mut(id)
                && let Some(cam) = &mut v3.camera
            {
                (cam.eye, cam.target) = (eye, target);
            }
            return Ok(orientation_of(&s.model, Some(id)));
        }
    } else if eye.is_some() {
        return Err(bad(c, "only a camera view has an eye; this one stands by `azimuth`, `elevation`, `target` and `across`"));
    }
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

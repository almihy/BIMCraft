//! The Massing & Site tab's commands (docs/areas/site.md): `site`, `site_edit`, `site_figures`, `site_kinds`,
//! `property_line`, `toposolid` and `building_pad`. What they make is shaped by `masterplan::keep`.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::{R, absent, bad, element_id, element_json, level_param, m2, num, points, string};
use crate::engine::Session;
use crate::geom::{self, P2};
use crate::masterplan::{self, BuildingPad, MassSite, PHASES, PropertyLine, Site};
use crate::model::{Element, FloorType, Id, MAX_POINTS, MIN_CONTOURS, Model, ShapePoint};
use crate::site::{Props, shapes};

/// Refuses a key `p` holds that the command doesn't take, saying which it does.
fn keys(c: &str, p: &Value, allowed: &[&str]) -> Result<(), String> {
    match p {
        Value::Object(o) => match o.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(bad(c, format!("unknown parameter `{k}`; it takes: {}", allowed.join(", ")))),
            None => Ok(()),
        },
        Value::Null => Ok(()),
        _ => Err(bad(c, "its parameters are an object {…}")),
    }
}

/// A kind by name, or what the kinds are.
fn kind_param(c: &str, p: &Value) -> Result<String, String> {
    let all = || shapes::KINDS.iter().map(|k| k.kind).collect::<Vec<_>>().join(", ");
    let kind = string(c, p, "kind")?.ok_or_else(|| bad(c, format!("`kind` is required: one of {}", all())))?;
    if shapes::kind(&kind).is_none() {
        return Err(bad(c, format!("`kind` is one of {}; not `{kind}`", all())));
    }
    Ok(kind)
}

fn phase_param(c: &str, p: &Value) -> Result<Option<String>, String> {
    let phase = string(c, p, "phase")?;
    if let Some(ph) = &phase
        && !PHASES.contains(&ph.as_str())
    {
        return Err(bad(c, format!("`phase` is one of {}; not `{ph}`", PHASES.join(", "))));
    }
    Ok(phase)
}

/// Text up to `cap` characters.
fn long_string(c: &str, p: &Value, key: &str, cap: usize) -> Result<Option<String>, String> {
    match p.get(key) {
        v if absent(v) => Ok(None),
        Some(Value::String(s)) if s.chars().count() <= cap => Ok(Some(s.clone())),
        Some(v) => Err(bad(c, format!("`{key}` must be text of at most {cap} characters, got {v}"))),
        None => Ok(None),
    }
}

/// `props` as given: an object (null values kept, for `site_edit` to take back), or none.
fn props_param(c: &str, p: &Value) -> Result<Map<String, Value>, String> {
    match p.get("props") {
        v if absent(v) => Ok(Map::new()),
        Some(Value::Object(o)) => Ok(o.clone()),
        Some(v) => Err(bad(c, format!("`props` is an object of what was drawn ({{\"path\": …, \"width\": …}}), got {v}"))),
        None => Ok(Map::new()),
    }
}

/// What a site element's command answers: its id, its area, what it was told and filled in, and the element.
fn answer(m: &Model, id: Id, filled: &[String]) -> R {
    let Some(Element::Site(st)) = m.elements.get(id) else { return Ok(json!({"id": id})) };
    let area: f64 = st.outline.iter().map(|p| geom::area(&p.outer).abs() - p.holes.iter().map(|h| geom::area(h).abs()).sum::<f64>()).sum();
    let filled: Map<String, Value> = filled.iter().filter_map(|k| Some((k.clone(), st.props.get(k)?.clone()))).collect();
    Ok(json!({"id": id, "area_m2": m2(area), "notes": st.notes, "filled": filled, "element": element_json(m, id)}))
}

/// A shape that couldn't be worked out refuses the command, saying why (the engine takes it back).
fn shaped(c: &str, m: &Model, id: Id) -> Result<(), String> {
    match m.elements.get(id) {
        Some(Element::Site(st)) if st.outline.is_empty() && !st.notes.is_empty() => Err(bad(c, st.notes.join(" "))),
        _ => Ok(()),
    }
}

pub(super) fn site(s: &mut Session, p: &Value) -> R {
    let c = "site";
    keys(c, p, &["kind", "use", "name", "level", "phase", "source", "ref", "props"])?;
    let kind = kind_param(c, p)?;
    let use_ = string(c, p, "use")?.unwrap_or_default();
    let level = level_param(c, &s.model, p, "level")?;
    let phase = phase_param(c, p)?.unwrap_or_else(|| "new".into());
    let props: Props = props_param(c, p)?.into_iter().filter(|(_, v)| !v.is_null()).collect();
    masterplan::check_props(&kind, &props).map_err(|e| bad(c, e))?;
    let given: Vec<String> = props.keys().cloned().collect();
    let mut st = Site {
        kind,
        use_,
        name: string(c, p, "name")?.unwrap_or_default(),
        level,
        phase,
        source: string(c, p, "source")?.unwrap_or_default(),
        reference: long_string(c, p, "ref", masterplan::MAX_PROP_TEXT)?.unwrap_or_default(),
        props,
        outline: Vec::new(),
        parts: Vec::new(),
        layout: None,
        notes: Vec::new(),
        params: BTreeMap::new(),
    };
    masterplan::check_site(c, &st)?;
    masterplan::put_phase(&s.model.phases, &mut st.params, &st.phase);
    let before = Arc::clone(&s.model);
    let id = s.model_mut().insert(Element::Site(Box::new(st)));
    masterplan::keep_model(s.model_mut(), &before);
    shaped(c, &s.model, id)?;
    let filled: Vec<String> = match s.model.elements.get(id) {
        Some(Element::Site(st)) => st.props.keys().filter(|k| !given.contains(k)).cloned().collect(),
        _ => Vec::new(),
    };
    answer(&s.model, id, &filled)
}

/// `props` given to `site_edit`: each set, or with null taken back to its default.
fn edit_props(props: &mut Props, given: Map<String, Value>) {
    for (k, v) in given {
        if v.is_null() {
            props.remove(&k);
        } else {
            props.insert(k, v);
        }
    }
}

pub(super) fn site_edit(s: &mut Session, p: &Value) -> R {
    let c = "site_edit";
    keys(c, p, &["id", "use", "name", "phase", "source", "ref", "props", "level"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let given = props_param(c, p)?;
    let phase = phase_param(c, p)?;
    let before = Arc::clone(&s.model);
    match s.model.elements.get(id).cloned() {
        Some(Element::Site(mut st)) => {
            if let Some(u) = string(c, p, "use")? {
                st.use_ = u;
            }
            if let Some(n) = string(c, p, "name")? {
                st.name = n;
            }
            if let Some(src) = string(c, p, "source")? {
                st.source = src;
            }
            if let Some(r) = long_string(c, p, "ref", masterplan::MAX_PROP_TEXT)? {
                st.reference = r;
            }
            if !absent(p.get("level")) {
                st.level = level_param(c, &s.model, p, "level")?;
            }
            if let Some(ph) = phase {
                st.phase = ph;
                masterplan::put_phase(&s.model.phases, &mut st.params, &st.phase);
            }
            // A prop taken back is filled in again from the figures: none of what was filled in before stays.
            let reset: Vec<String> = given.iter().filter(|(_, v)| v.is_null()).map(|(k, _)| k.clone()).collect();
            edit_props(&mut st.props, given);
            masterplan::check_site(c, &st)?;
            if let Some(Element::Site(slot)) = s.model_mut().elements.get_mut(id) {
                *slot = st;
            }
            masterplan::keep_model(s.model_mut(), &before);
            shaped(c, &s.model, id)?;
            answer(&s.model, id, &reset)
        }
        Some(Element::Mass(mut ms)) => {
            let mut site = ms.site.take().unwrap_or_default();
            if let Some(u) = string(c, p, "use")? {
                site.use_ = u;
            }
            if let Some(n) = string(c, p, "name")? {
                ms.name = n;
            }
            if let Some(src) = string(c, p, "source")? {
                site.source = src;
            }
            if let Some(r) = long_string(c, p, "ref", masterplan::MAX_PROP_TEXT)? {
                site.reference = r;
            }
            if let Some(ph) = phase {
                site.phase = ph;
                masterplan::put_phase(&s.model.phases, &mut ms.params, &site.phase);
            }
            edit_props(&mut site.props, given);
            masterplan::check_mass_site(c, &site)?;
            if let Some(h) = masterplan::mass_height(&site, &masterplan::figures(&s.model)) {
                (ms.height, ms.top_level) = (h, None);
            }
            ms.site = Some(site);
            if let Some(Element::Mass(slot)) = s.model_mut().elements.get_mut(id) {
                *slot = ms;
            }
            masterplan::keep_model(s.model_mut(), &before);
            let e = element_json(&s.model, id);
            Ok(json!({"id": id, "height": e["height"], "site": e["site"], "element": e}))
        }
        Some(e) => Err(bad(c, format!("{} {id} is no site element or mass", e.category()))),
        None => Err(bad(c, format!("no element {id}"))),
    }
}

/// A mass's masterplan figures given to `mass` as `site`.
pub(super) fn mass_site(c: &str, p: &Value) -> Result<Option<MassSite>, String> {
    let Some(v) = p.get("site").filter(|v| !v.is_null()) else { return Ok(None) };
    keys(c, v, &["use", "phase", "source", "ref", "props"]).map_err(|e| e.replace(&format!("{c}: "), &format!("{c}: `site`: ")))?;
    let site = MassSite {
        use_: string(c, v, "use")?.unwrap_or_default(),
        phase: phase_param(c, v)?.unwrap_or_else(|| "new".into()),
        source: string(c, v, "source")?.unwrap_or_default(),
        reference: long_string(c, v, "ref", masterplan::MAX_PROP_TEXT)?.unwrap_or_default(),
        props: props_param(c, v)?.into_iter().filter(|(_, x)| !x.is_null()).collect(),
    };
    masterplan::check_mass_site(c, &site)?;
    Ok(Some(site))
}

pub(super) fn site_figures(s: &mut Session, p: &Value) -> R {
    let c = "site_figures";
    keys(c, p, &["values"])?;
    if let Some(v) = p.get("values").filter(|v| !v.is_null()) {
        let o = v.as_object().ok_or_else(|| bad(c, format!("`values` is an object {{name: number}}, got {v}")))?;
        let mut figures = s.model.site_figures.clone();
        for (k, x) in o {
            let Some((_, _, what)) = shapes::FIGURES.iter().find(|f| f.0 == k) else {
                let names: Vec<&str> = shapes::FIGURES.iter().map(|f| f.0).collect();
                return Err(bad(c, format!("`{k}` isn't a figure; they are: {}", names.join(", "))));
            };
            if x.is_null() {
                figures.remove(k);
                continue;
            }
            let n = x
                .as_f64()
                .filter(|n| n.is_finite() && n.abs() <= crate::model::MAX_COORD)
                .ok_or_else(|| bad(c, format!("`{k}` is a number, got {x}")))?;
            let ok = match *what {
                "length" => n >= 0.0 || k == "water.level",
                "count" => (0.0..=1e6).contains(&n),
                "angle" => (0.0..=90.0).contains(&n),
                "percent" => (0.0..=100.0).contains(&n),
                "boolean" => n == 0.0 || n == 1.0,
                _ => (0.0..=1.0).contains(&n),
            };
            if !ok {
                return Err(bad(c, format!("`{k}` is a {what} that can't be {n}")));
            }
            figures.insert(k.clone(), n);
        }
        if figures != s.model.site_figures {
            s.model_mut().site_figures = figures;
        }
    }
    let f = masterplan::figures(&s.model);
    let all: Map<String, Value> = shapes::FIGURES
        .iter()
        .map(|(n, _, t)| {
            ((*n).to_string(), json!({"value": shapes::figure(&f, n), "type": t, "own": s.model.site_figures.contains_key(*n)}))
        })
        .collect();
    Ok(json!({"figures": all}))
}

pub(super) fn site_kinds(_: &mut Session, p: &Value) -> R {
    keys("site_kinds", p, &[])?;
    Ok(shapes::catalog())
}

/// A path's points, [x, y] or [x, y, bulge].
fn bulged(c: &str, p: &Value, key: &str) -> Result<Vec<[f64; 3]>, String> {
    let v = p.get(key).ok_or_else(|| bad(c, format!("`{key}` [[x, y] or [x, y, bulge], …] is required")))?;
    if v.as_array().is_some_and(|a| a.len() > MAX_POINTS) {
        return Err(bad(c, format!("`{key}` has more than {MAX_POINTS} points")));
    }
    let mut pts = shapes::path_points(v).map_err(|e| bad(c, format!("`{key}`: {e}")))?;
    if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a[0] == b[0] && a[1] == b[1]) {
        pts.pop();
    }
    Ok(pts.into_iter().map(|q| [q[0], q[1], (q[2] * 1e9).round() / 1e9]).collect())
}

pub(super) fn property_line(s: &mut Session, p: &Value) -> R {
    let c = "property_line";
    keys(c, p, &["points", "level", "name"])?;
    let points = bulged(c, p, "points")?;
    let level = level_param(c, &s.model, p, "level")?;
    let mut pl = PropertyLine { name: string(c, p, "name")?.unwrap_or_default(), level, points, params: BTreeMap::new() };
    // Counter-clockwise, as the plan's areas run: reversed, each arc's bulge on the piece now before it, the other way.
    let ring: Vec<P2> = pl.points.iter().map(|q| P2(q[0], q[1])).collect();
    if geom::area(&ring) < 0.0 {
        let n = pl.points.len();
        let mut r: Vec<[f64; 3]> = (0..n)
            .map(|i| {
                let q = pl.points[n - 1 - i];
                let b = pl.points[(2 * n - 2 - i) % n][2];
                [q[0], q[1], -b]
            })
            .collect();
        std::mem::swap(&mut pl.points, &mut r);
    }
    masterplan::check_property_line(c, &pl)?;
    let area = pl.area();
    let id = s.model_mut().insert(Element::PropertyLine(pl));
    Ok(json!({"id": id, "area_m2": m2(area)}))
}

/// The floor type `name`, made `thickness` thick when the project hasn't one of that name.
fn floor_type(s: &mut Session, name: &str, thickness: f64) {
    if !s.model.types.floor.contains_key(name) {
        s.model_mut().types.floor.insert(name.into(), FloorType { thickness, layers: Vec::new() });
    }
}

/// The type a command is given, or its own default, made when the project hasn't it.
fn type_param(c: &str, s: &mut Session, p: &Value, default: &str, thickness: f64) -> Result<String, String> {
    match string(c, p, "type")? {
        Some(t) if s.model.types.floor.contains_key(&t) => Ok(t),
        Some(t) => Err(bad(c, format!("no floor type `{t}`: a toposolid and a building pad are of the floor types; make one with `type`"))),
        None => {
            floor_type(s, default, thickness);
            Ok(default.into())
        }
    }
}

/// A ring counter-clockwise, its last point not the first again.
fn ccw(mut r: Vec<P2>) -> Vec<P2> {
    if r.len() > 1 && r.first() == r.last() {
        r.pop();
    }
    if geom::area(&r) < 0.0 {
        r.reverse();
    }
    r
}

pub(super) fn toposolid(s: &mut Session, p: &Value) -> R {
    let c = "toposolid";
    keys(c, p, &["boundary", "points", "contours", "type", "level", "offset", "name"])?;
    let boundary = ccw(points(c, p, "boundary")?);
    let level = level_param(c, &s.model, p, "level")?;
    let offset = num(c, p, "offset")?.unwrap_or(0.0);
    let contours = num(c, p, "contours")?.unwrap_or(1000.0);
    if !(contours == 0.0 || contours >= MIN_CONTOURS) {
        return Err(bad(c, format!("`contours` are at least {MIN_CONTOURS} mm apart, or 0 for none; not {contours}")));
    }
    let mut shape = crate::model::Shape::default();
    if let Some(v) = p.get("points").filter(|v| !v.is_null()) {
        let a = v.as_array().ok_or_else(|| bad(c, "`points` is a list of [x, y, z]: the ground's height there over the level, mm"))?;
        if a.len() > crate::model::MAX_SHAPE_POINTS {
            return Err(bad(c, format!("`points` holds at most {} points", crate::model::MAX_SHAPE_POINTS)));
        }
        for q in a {
            let xyz = q.as_array().and_then(|x| Some([x.first()?.as_f64()?, x.get(1)?.as_f64()?, x.get(2)?.as_f64()?]));
            let Some([x, y, z]) = xyz.filter(|x| x.len() == 3 && x.iter().all(|v| v.is_finite() && v.abs() <= crate::model::MAX_COORD))
            else {
                return Err(bad(c, format!("`points`: {q} is not a point [x, y, z] in mm")));
            };
            shape.points.push(ShapePoint { at: P2(x, y), z: z - offset, ..Default::default() });
        }
    }
    let type_name = type_param(c, s, p, "Toposolid", 1000.0)?;
    let f = crate::model::Floor {
        type_name,
        level,
        boundary,
        openings: Vec::new(),
        offset,
        slope: 0.0,
        arrow: None,
        structural: false,
        pattern_origin: P2(0.0, 0.0),
        pattern_angle: 0.0,
        sketch: Vec::new(),
        shape,
        contours,
        params: BTreeMap::new(),
    };
    let area = geom::area(&f.boundary).abs();
    let id = s.model_mut().insert(Element::Toposolid(f));
    Ok(json!({"id": id, "area_m2": m2(area)}))
}

pub(super) fn building_pad(s: &mut Session, p: &Value) -> R {
    let c = "building_pad";
    keys(c, p, &["boundary", "type", "level", "offset"])?;
    let boundary = ccw(points(c, p, "boundary")?);
    let level = level_param(c, &s.model, p, "level")?;
    let offset = num(c, p, "offset")?.unwrap_or(0.0);
    let type_name = type_param(c, s, p, "Building Pad", 300.0)?;
    let pad = BuildingPad { type_name, level, boundary, offset, params: BTreeMap::new() };
    let area = geom::area(&pad.boundary).abs();
    let id = s.model_mut().insert(Element::BuildingPad(pad));
    // Which toposolid it cuts, if one.
    let cuts: Vec<Id> = s
        .model
        .elements
        .iter()
        .filter_map(|(tid, e)| match e {
            Element::Toposolid(f) if s.model.pads_in(f).iter().any(|(pid, _)| *pid == id) => Some(*tid),
            _ => None,
        })
        .collect();
    Ok(json!({"id": id, "area_m2": m2(area), "cuts": cuts}))
}

//! A curtain wall worked on by hand, as Revit's tools: Curtain Grid (a grid line added), Add/Remove Segments (two panels
//! joined, or parted again) and Mullion (mullions put on a grid line, a segment or every line). What they change is what
//! the wall remembers and the lines it hosts (`curtain.rs`); the wall lays its panels and mullions out from that.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{R, absent, bad, element_id, flag, num, only, point, point_opt, string, type_param};
use crate::curtain::{self, Grid, GridLine, MAX_LINES, NEAR, Segment};
use crate::engine::Session;
use crate::model::{Element, Guid, Id, Model, Wall};
use crate::walls::Axis;

/// The curtain wall `wall` (or `host`) names: its id, its GUID, the wall and its grid.
fn host<'a>(c: &str, m: &'a Model, p: &Value) -> Result<(Id, Guid, &'a Wall, Grid), String> {
    let v = [p.get("wall"), p.get("host")]
        .into_iter()
        .find(|v| !absent(*v))
        .flatten()
        .ok_or_else(|| bad(c, "`wall`, a curtain wall's id, is required"))?;
    let id = element_id(c, m, v)?;
    let Some(w) = m.wall(id) else {
        let what = m.elements.get(id).map_or("element", Element::category);
        return Err(bad(c, format!("{what} {id} isn't a wall; `wall` is a curtain wall's id")));
    };
    if !curtain::is_curtain(m, w) {
        return Err(bad(
            c,
            format!(
                "wall {id} isn't a curtain wall: its type `{}` is a basic wall type. Draw one with `wall` and `type` \"Curtain Wall\", \
                 \"Exterior Glazing\" or \"Storefront\", or give this one such a type with `modify`",
                w.type_name
            ),
        ));
    }
    let g = curtain::grid(m, id).ok_or_else(|| bad(c, format!("wall {id} is too short or too low to have a grid")))?;
    Ok((id, m.guid(id), w, g))
}

/// To the micron.
fn fine(x: f64) -> f64 {
    (x * 1e3).round() / 1e3
}

/// The place `p` names on the wall: `at` [s, z] (mm along it from its start, mm over its base), or `point` [x, y] in plan
/// (the nearest place on the wall's line) with `z`, an elevation in the model (the wall's base when not given).
fn place(c: &str, m: &Model, w: &Wall, p: &Value) -> Result<(f64, f64), String> {
    if let Some(v) = p.get("at").filter(|v| !v.is_null()) {
        let q =
            point(v).ok_or_else(|| bad(c, format!("`at` is [s, z]: mm along the wall from its start, and mm over its base; got {v}")))?;
        return Ok((q.0, q.1));
    }
    let q = point_opt(c, p, "point")?.ok_or_else(|| {
        bad(c, "say where: `at` [s, z] (mm along the wall from its start, mm over its base), or `point` [x, y] in plan with `z`")
    })?;
    let z = num(c, p, "z")?.map_or(0.0, |z| z - m.own_base(w));
    Ok((fine(Axis::of(w).project(q).0), fine(z)))
}

/// Which way the line meant runs, when `vertical` says.
fn way(c: &str, p: &Value) -> Result<Option<bool>, String> {
    if absent(p.get("vertical")) { Ok(None) } else { flag(c, p, "vertical").map(Some) }
}

/// The wall's panels, by id.
fn panels(m: &Model, host: Guid) -> Vec<Id> {
    let mut ids: Vec<Id> =
        m.hosted_by(host).into_iter().filter(|id| matches!(m.elements.get(*id), Some(Element::CurtainPanel(_)))).collect();
    ids.sort_unstable();
    ids
}

/// The lines one entry asks for: `{vertical, at}`, or `point` (a standing line through it) and `z` (a lying line at that
/// elevation), either or both.
fn lines_asked(c: &str, m: &Model, w: &Wall, p: &Value) -> Result<Vec<(bool, f64)>, String> {
    if !p.is_object() {
        return Err(bad(c, format!("a grid line is {{vertical, at}}, {{point: [x, y]}} or {{z}}; got {p}")));
    }
    only(c, p, &["wall", "host", "vertical", "at", "point", "z", "lines"])?;
    if !absent(p.get("at")) {
        let at = num(c, p, "at")?.unwrap_or_default();
        let vertical = way(c, p)?.ok_or_else(|| {
            bad(c, "`vertical` is required with `at`: true for a line that stands (`at` mm along the wall from its start), false for one that lies (`at` mm over the wall's base)")
        })?;
        return Ok(vec![(vertical, at)]);
    }
    let mut out = Vec::new();
    if let Some(q) = point_opt(c, p, "point")? {
        out.push((true, fine(Axis::of(w).project(q).0)));
    }
    if let Some(z) = num(c, p, "z")? {
        out.push((false, fine(z - m.own_base(w))));
    }
    if out.is_empty() {
        return Err(bad(
            c,
            "say where the line goes: `vertical` and `at` (mm along the wall from its start for a standing line, mm over its base for a lying one), \
             `point` [x, y] in plan for a standing line through it, or `z` (an elevation) for a lying one",
        ));
    }
    Ok(out)
}

pub(super) fn curtain_grid(s: &mut Session, p: &Value) -> R {
    let c = "curtain_grid";
    only(c, p, &["wall", "host", "vertical", "at", "point", "z", "lines"])?;
    let m = &s.model;
    let (id, host, w, g) = host(c, m, p)?;
    let asked: Vec<(bool, f64)> = match p.get("lines") {
        v if absent(v) => lines_asked(c, m, w, p)?,
        Some(Value::Array(a)) if a.len() <= MAX_LINES => {
            let mut all = Vec::new();
            for one in a {
                all.extend(lines_asked(c, m, w, one)?);
            }
            all
        }
        Some(Value::Array(_)) => return Err(bad(c, format!("`lines` lists more than {MAX_LINES} lines"))),
        Some(v) => return Err(bad(c, format!("`lines` is a list of lines, each {{vertical, at}}, {{point}} or {{z}}; got {v}"))),
        None => Vec::new(),
    };
    if asked.is_empty() {
        return Err(bad(c, "`lines` is empty: give at least one line"));
    }
    let mut there: (Vec<f64>, Vec<f64>) = (g.u.iter().map(|l| l.at).collect(), g.v.iter().map(|l| l.at).collect());
    for &(vertical, at) in &asked {
        let (extent, lines, runs, size) =
            if vertical { (g.length, &mut there.0, "standing", "long") } else { (g.height, &mut there.1, "lying", "tall") };
        if !(at > NEAR && at < extent - NEAR) {
            return Err(bad(
                c,
                format!(
                    "a {runs} grid line at {at} mm is outside wall {id}, which is {} mm {size}: give `at` between {NEAR} and {} \
                     ({})",
                    fine(extent),
                    fine(extent - NEAR),
                    if vertical { "mm along the wall from its start" } else { "mm over the wall's base" },
                ),
            ));
        }
        if let Some(near) = lines.iter().find(|x| (**x - at).abs() <= NEAR) {
            return Err(bad(
                c,
                format!(
                    "wall {id} has a {runs} grid line at {} mm already, within {NEAR} mm of {at}: give an `at` further from it, or move \
                     that line (`modify` its `at`, when it is one drawn by hand)",
                    fine(*near)
                ),
            ));
        }
        lines.push(at);
    }
    let m = s.model_mut();
    let ids: Vec<Id> = asked.iter().map(|&(vertical, at)| m.insert(Element::CurtainGrid(GridLine { host, vertical, at }))).collect();
    curtain::lay(m, id);
    Ok(json!({"ids": ids, "wall": id, "panels": panels(m, host)}))
}

/// A segment as a message names it: where its middle is, which finds it again.
fn named(x: &Segment) -> Value {
    let [s, z] = x.middle();
    json!([fine(s), fine(z)])
}

pub(super) fn add_remove_segments(s: &mut Session, p: &Value) -> R {
    let c = "add_remove_segments";
    const KEYS: &[&str] = &["wall", "host", "at", "point", "z", "vertical", "segments", "remove"];
    only(c, p, KEYS)?;
    let m = &s.model;
    let (id, host, w, g) = host(c, m, p)?;
    let remove = absent(p.get("remove")) || flag(c, p, "remove")?;
    let one = |q: &Value| -> Result<Segment, String> {
        // A place alone, [s, z], is its `at`.
        let q = if q.is_array() { json!({"at": q}) } else { q.clone() };
        if !q.is_object() {
            return Err(bad(c, format!("a segment is named by a place: [s, z], {{at: [s, z]}} or {{point: [x, y], z}}; got {q}")));
        }
        only(c, &q, KEYS)?;
        let (along, up) = place(c, m, w, &q)?;
        let seg =
            g.segment_near(along, up, way(c, &q)?).ok_or_else(|| bad(c, format!("no grid line of wall {id} is near [{along}, {up}]")))?;
        if seg.border != 0 {
            return Err(bad(
                c,
                format!(
                    "the grid line nearest [{along}, {up}] is wall {id}'s border, whose segments stay: name a place on a line between the \
                     borders (`vertical` says which way the line runs when two are as near)"
                ),
            ));
        }
        Ok(seg)
    };
    let asked: Vec<Segment> = match p.get("segments") {
        v if absent(v) => vec![one(p)?],
        Some(Value::Array(a)) if a.len() <= MAX_LINES => a.iter().map(one).collect::<Result<_, _>>()?,
        Some(Value::Array(_)) => return Err(bad(c, format!("`segments` lists more than {MAX_LINES} segments"))),
        Some(v) => return Err(bad(c, format!("`segments` is a list of places, each [s, z]; got {v}"))),
        None => Vec::new(),
    };
    if asked.is_empty() {
        return Err(bad(c, "`segments` is empty: name at least one segment"));
    }
    let change = |merged: &mut BTreeSet<String>, x: &Segment| if remove { merged.insert(x.key.clone()) } else { merged.remove(&x.key) };
    let mut merged = w.curtain.merged.clone();
    for x in &asked {
        change(&mut merged, x);
    }
    let trouble = g.trouble(&merged);
    if !trouble.is_empty() {
        // What would make it sound: the segments inside the joined cells' boxes removed with it (or put back with it),
        // the boxes growing as they take in more.
        let mut all = merged.clone();
        let mut with: Vec<Segment> = asked.clone();
        let mut sound = false;
        for _ in 0..8 {
            let boxes = g.trouble(&all);
            if boxes.is_empty() {
                sound = true;
                break;
            }
            for x in boxes.iter().flat_map(|(lo, hi)| g.within(*lo, *hi)) {
                if all.contains(&x.key) != remove {
                    change(&mut all, &x);
                    with.push(x);
                }
            }
        }
        let (doing, done) = if remove { ("Removing", "remove") } else { ("Putting back", "put back") };
        let how = if sound || g.trouble(&all).is_empty() {
            format!("{done} these in one call, with `segments`: {}", Value::Array(with.iter().map(named).collect()))
        } else {
            format!("{done} the segments round it in one call (`segments`), so the cells joined make a rectangle")
        };
        return Err(bad(c, format!("{doing} only that would leave a panel that is not a rectangle, and a curtain panel is one: {how}")));
    }
    let m = s.model_mut();
    if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
        w.curtain.merged = merged;
    }
    curtain::lay(m, id);
    let places: Vec<Value> = asked.iter().map(named).collect();
    let mut reply = json!({"wall": id, "panels": panels(m, host)});
    reply[if remove { "removed" } else { "restored" }] = json!(places);
    Ok(reply)
}

pub(super) fn mullion(s: &mut Session, p: &Value) -> R {
    let c = "mullion";
    only(c, p, &["wall", "host", "type", "on", "at", "point", "z", "vertical"])?;
    // The starter mullion types, when the project has none.
    if s.model.types.mullion.is_empty() {
        curtain::ensure_mullions(s.model_mut());
    }
    let m = &s.model;
    let (id, host, w, g) = host(c, m, p)?;
    let type_name = type_param(c, p, m, "mullion")?;
    if !m.types.mullion.contains_key(&type_name) {
        let names: Vec<&str> = m.types.mullion.keys().map(String::as_str).collect();
        return Err(bad(
            c,
            format!("no mullion type `{type_name}`: the project has {}; make one with `type` (category `mullion`)", names.join(", ")),
        ));
    }
    let on = string(c, p, "on")?.unwrap_or_else(|| "grid_line".into()).trim().to_ascii_lowercase().replace(['-', ' '], "_");
    let merged = &w.curtain.merged;
    let there = |x: &Segment| x.border != 0 || !merged.contains(&x.key);
    let targets: Vec<Segment> = match on.as_str() {
        "all" | "all_grid_lines" => g.segments(),
        "grid_line" | "segment" | "grid_line_segment" => {
            let (along, up) = place(c, m, w, p)?;
            let seg = g
                .segment_near(along, up, way(c, p)?)
                .ok_or_else(|| bad(c, format!("no grid line of wall {id} is near [{along}, {up}]")))?;
            if on == "grid_line" {
                g.segments().into_iter().filter(|x| x.vertical == seg.vertical && x.line == seg.line).collect()
            } else if there(&seg) {
                vec![seg]
            } else {
                return Err(bad(
                    c,
                    format!(
                        "the segment at [{along}, {up}] was removed (the panels either side are one): put it back with \
                         `add_remove_segments` (`remove: false`) first"
                    ),
                ));
            }
        }
        _ => {
            return Err(bad(
                c,
                format!("`on` is grid_line (the whole line at the place), segment (the one segment there) or all; not `{on}`"),
            ));
        }
    };
    let targets: Vec<Segment> = targets.into_iter().filter(there).collect();
    let m = s.model_mut();
    let mut ids = Vec::new();
    for seg in &targets {
        ids.extend(curtain::put_mullion(m, host, seg, &type_name));
    }
    // What was deleted there is no longer left out.
    if let Some(Element::Wall(w)) = m.elements.get_mut(id)
        && targets.iter().any(|x| w.curtain.no_mullion.contains(&x.mullion_key()))
    {
        for x in &targets {
            w.curtain.no_mullion.remove(&x.mullion_key());
        }
    }
    curtain::lay(m, id);
    Ok(json!({"ids": ids, "skipped": targets.len() - ids.len(), "wall": id, "type": type_name}))
}

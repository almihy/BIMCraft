//! The ground's commands, after Revit's (docs/areas/site.md): `sub_division` (Sub-divide), `road` (our road tool, its
//! carriageway, pavements and median sub-divisions), `excavate` and `remove_excavation` (Excavate), `graded_region` (Graded
//! Region), `remove_point` (a toposolid's point deleted, Modify Sub Elements), `base_point` (the Survey Point and the
//! Project Base Point), `planting` and `site_component` (Planting and Site Component), `property_line_table` and
//! `property_line_edit` (Property Line's Edit Table), and `parking_component` (Parking Component), `car_park` and
//! `car_park_run` (the car park generator, `carpark.rs`). What they make is `ground.rs`'s and `carpark.rs`'s.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::{R, absent, bad, element_id, element_json, flag, ids_param, level_param, m2, num, point_req, points, string};
use crate::engine::Session;
use crate::geom::{self, P2};
use crate::ground::{self, BasePoint, GradedFrom, SubDivision};
use crate::masterplan::{self, Edge};
use crate::model::{Element, Floor, Guid, Id, MAX_COORD, MAX_POINTS, Model, TOL};
use crate::phasing;

pub(super) use crate::ground::{bearing, bearing_text, table_of};

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

/// A toposolid of the model by id or GUID, `key` in `p`.
fn toposolid_param(c: &str, m: &Model, p: &Value, key: &str) -> Result<Id, String> {
    let v = p.get(key).filter(|v| !v.is_null()).ok_or_else(|| bad(c, format!("`{key}`, a toposolid, is required")))?;
    let id = element_id(c, m, v)?;
    match m.elements.get(id) {
        Some(Element::Toposolid(_)) => Ok(id),
        Some(e) => Err(bad(c, format!("{} {id} is no toposolid", e.category()))),
        None => Err(bad(c, format!("no element {id}"))),
    }
}

/// The toposolid an outline goes on: `host` given, or the one holding its middle (of several, the highest), or the one it
/// overlaps most.
fn host_param(c: &str, m: &Model, p: &Value, ring: &[P2]) -> Result<Guid, String> {
    if !absent(p.get("host")) {
        let id = toposolid_param(c, m, p, "host")?;
        return Ok(m.guid(id));
    }
    let middle = geom::centroid(ring);
    let holding = m
        .elements
        .with_guids()
        .filter_map(|(_, g, e)| match e {
            Element::Toposolid(f) if geom::contains(&f.boundary, middle) => Some((g, m.elevation(f.level) + f.offset)),
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(g, _)| g);
    // Else the one it overlaps most.
    let own = vec![ring.iter().map(|q| [q.0, q.1]).collect::<Vec<[f64; 2]>>()];
    let overlapping = || {
        m.elements
            .with_guids()
            .filter_map(|(_, g, e)| match e {
                Element::Toposolid(f) => {
                    let theirs = vec![f.boundary.iter().map(|q| [q.0, q.1]).collect::<Vec<[f64; 2]>>()];
                    let a = crate::site::shapes::poly::area(&crate::site::shapes::poly::intersection(&own, &theirs));
                    (a > 1.0).then_some((g, a))
                }
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(g, _)| g)
    };
    holding.or_else(overlapping).ok_or_else(|| bad(c, "no toposolid lies under it: give `host`, the toposolid it is a part of, or make one with `toposolid`"))
}

/// Openings: rings, each clockwise.
fn openings_param(c: &str, p: &Value) -> Result<Vec<Vec<P2>>, String> {
    let Some(v) = p.get("openings").filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let a = v.as_array().ok_or_else(|| bad(c, "`openings` is a list of rings [[[x, y], …], …]"))?;
    if a.len() > MAX_POINTS {
        return Err(bad(c, format!("`openings` lists more than {MAX_POINTS}")));
    }
    a.iter()
        .map(|r| {
            let mut ring = ccw(points(c, &json!({"ring": r}), "ring")?);
            ring.reverse();
            Ok(ring)
        })
        .collect()
}

/// `type` given (a floor type the project has, or a surface it makes), or `default`, made when the project hasn't it.
fn surface_type(c: &str, s: &mut Session, p: &Value, default: &str) -> Result<String, String> {
    let name = string(c, p, "type")?.unwrap_or_else(|| default.to_string());
    if !s.model.types.floor.contains_key(&name) {
        if ground::surface(&name).is_none() && !s.model.materials.contains_key(&name) {
            let surfaces: Vec<&str> = ground::SURFACES.iter().map(|x| x.name).collect();
            return Err(bad(
                c,
                format!("no toposolid type `{name}`: name one of the project's floor types, a material, or a surface ({})", surfaces.join(", ")),
            ));
        }
        ground::ensure_type(s.model_mut(), &name);
    }
    Ok(name)
}

/// What a sub-division's command answers: its id, its area, its host, and the element.
fn subdivision_answer(m: &Model, id: Id) -> Value {
    let Some(Element::SubDivision(sd)) = m.elements.get(id) else { return json!({"id": id}) };
    json!({"id": id, "area_m2": m2(m.subdivision_area(sd)), "host": m.elements.id(sd.host), "element": element_json(m, id)})
}

pub(super) fn sub_division(s: &mut Session, p: &Value) -> R {
    let c = "sub_division";
    keys(c, p, &["boundary", "openings", "host", "type", "offset", "name", "role", "cad"])?;
    let boundary = match super::cad_pick::picked_outline(c, &s.model, p, &["boundary"])? {
        Some((_, outline, _)) => ccw(outline),
        None => ccw(points(c, p, "boundary")?),
    };
    if boundary.len() < 3 {
        return Err(bad(c, "`boundary` runs through 3 points or more"));
    }
    let host = host_param(c, &s.model, p, &boundary)?;
    let openings = openings_param(c, p)?;
    let type_name = surface_type(c, s, p, "Paving")?;
    let offset = match num(c, p, "offset")? {
        Some(o) => o,
        None => ground::surface(&type_name).map_or(0.0, |x| x.offset),
    };
    let sd = SubDivision {
        type_name,
        host,
        boundary,
        openings,
        offset,
        name: string(c, p, "name")?.unwrap_or_default(),
        role: string(c, p, "role")?.unwrap_or_default(),
        sketch: Vec::new(),
        road: None,
        params: BTreeMap::new(),
    };
    if s.model.subdivision_region(&sd).is_empty() {
        return Err(bad(c, "it lies outside its host toposolid: draw it on the toposolid"));
    }
    let id = s.model_mut().insert(Element::SubDivision(sd));
    Ok(subdivision_answer(&s.model, id))
}

/// The road keys `road` takes beside `path`: its cross-section, its turning heads and its junctions' kerb radius.
const ROAD_KEYS: [&str; 10] = ["width", "lanes", "lane_width", "kerb", "median", "oneway", "pavement", "heads", "markings", "corner_radius"];

/// The project's roads on `level` (made by `road`, or converted lanes), but `skip`: each its definition, its pieces and
/// its host.
fn roads_on(m: &Model, level: Guid, skip: Option<Guid>) -> Vec<(ground::RoadDef, Vec<Id>, Guid)> {
    let mut out: Vec<(ground::RoadDef, Vec<Id>, Guid)> = Vec::new();
    for (id, e) in m.elements.iter() {
        let Element::SubDivision(sd) = e else { continue };
        let Some(r) = &sd.road else { continue };
        if Some(r.of) == skip || m.subdivision_level(sd) != Some(level) {
            continue;
        }
        match out.iter_mut().find(|(o, _, _)| o.of == r.of) {
            Some((_, pieces, _)) => pieces.push(*id),
            None => out.push((r.clone(), vec![*id], sd.host)),
        }
    }
    out
}

/// A road's shapes among the others on its level, as the masterplan shaped its lanes: the corners where one road ends on
/// another rounded to the kerb's radius (owned by the road that ends there), two pieces end to end joined.
fn shaped_among(m: &Model, defs: &[(String, String, crate::site::Props)]) -> Vec<crate::site::shapes::Shaped> {
    let figures = masterplan::figures(m);
    let items: Vec<crate::site::shapes::Item> = defs
        .iter()
        .map(|(key, use_, props)| crate::site::shapes::Item { key, kind: "lane", use_, props, phase: "new", context: false })
        .collect();
    crate::site::shapes::normalise(&items, &figures)
}

/// Road `def` made again as `shaped` on `host`: its pieces go, the new ones take its first's id, GUID and parameters.
fn remake(s: &mut Session, def: &ground::RoadDef, pieces: &[Id], host: Guid, shaped: crate::site::shapes::Shaped) -> Result<Vec<Id>, String> {
    let level = match s.model.elements.id(host).and_then(|id| s.model.elements.get(id)) {
        Some(Element::Toposolid(f)) => f.level,
        _ => return Err(bad("road", "its host is no toposolid")),
    };
    let site = masterplan::Site {
        kind: "lane".into(),
        use_: def.use_.clone(),
        name: def.name.clone(),
        level,
        phase: "new".into(),
        source: String::new(),
        reference: String::new(),
        props: def.props.clone(),
        outline: shaped.outline,
        parts: shaped.parts,
        notes: Vec::new(),
        params: BTreeMap::new(),
    };
    let made = ground::subdivisions_from(s.model_mut(), &site, host);
    let m = s.model_mut();
    let reuse = pieces.iter().copied().find(|id| m.elements.get(*id).is_some() && m.guid(*id) == def.of).or(pieces.first().copied());
    let kept = reuse.and_then(|id| match m.elements.get(id) {
        Some(Element::SubDivision(sd)) => Some(sd.params.clone()),
        _ => None,
    });
    for id in pieces {
        m.elements.remove(*id);
    }
    let mut ids = Vec::new();
    for (k, mut e) in made.into_iter().enumerate() {
        if let Element::SubDivision(sd) = &mut e {
            sd.params = if k == 0 { kept.clone().unwrap_or_default() } else { BTreeMap::new() };
        }
        match (k, reuse) {
            (0, Some(id)) => {
                s.model_mut().elements.insert_as(id, def.of, e).map_err(|e| bad("road", e))?;
                ids.push(id);
            }
            _ => ids.push(s.model_mut().insert(e)),
        }
    }
    ground::link_road(s.model_mut(), &ids);
    Ok(ids)
}

pub(super) fn road(s: &mut Session, p: &Value) -> R {
    let c = "road";
    let mut allowed = vec!["path", "host", "level", "use", "name", "replace"];
    allowed.extend(ROAD_KEYS);
    keys(c, p, &allowed)?;
    // Made again: the road a piece of it is, its definition with what is given over it; its pieces go, the new ones take
    // its first's id.
    let mut was: Option<(ground::RoadDef, Vec<Id>, Guid)> = None;
    if !absent(p.get("replace")) {
        let piece = element_id(c, &s.model, p.get("replace").unwrap_or(&Value::Null))?;
        let Some(Element::SubDivision(SubDivision { road: Some(r), host, .. })) = s.model.elements.get(piece) else {
            return Err(bad(c, format!("{piece} is no piece of a road `road` made")));
        };
        let pieces: Vec<Id> = s
            .model
            .elements
            .iter()
            .filter(|(_, e)| matches!(e, Element::SubDivision(SubDivision { road: Some(o), .. }) if o.of == r.of))
            .map(|(id, _)| *id)
            .collect();
        was = Some((r.clone(), pieces, *host));
    }
    let old = was.as_ref().map(|(r, _, _)| r);
    let path = match p.get("path").filter(|v| v.is_array()) {
        Some(v) => v.clone(),
        None => old.and_then(|r| r.props.get("path").cloned()).ok_or_else(|| bad(c, "`path` [[x, y] or [x, y, bulge], …], its centre line, is required"))?,
    };
    let path = &path;
    let use_ = string(c, p, "use")?.or_else(|| old.map(|r| r.use_.clone()).filter(|u| !u.is_empty())).unwrap_or_else(|| "road".into());
    let lane = crate::site::shapes::kind("lane").ok_or_else(|| bad(c, "no lane kind"))?;
    if !lane.uses.contains(&use_.as_str()) {
        return Err(bad(c, format!("`use` is one of {}; not `{use_}`", lane.uses.join(", "))));
    }
    let mut props = crate::site::Props::new();
    props.insert("path".into(), path.clone());
    for k in ROAD_KEYS {
        // What is given, else what the road had (null takes it back to the default).
        match p.get(k) {
            Some(Value::Null) => {}
            Some(v) => {
                props.insert(k.into(), v.clone());
            }
            None => {
                if let Some(v) = old.and_then(|r| r.props.get(k)) {
                    props.insert(k.into(), v.clone());
                }
            }
        }
    }
    masterplan::check_props("lane", &props).map_err(|e| bad(c, e))?;
    let figures = masterplan::figures(&s.model);
    let item = crate::site::shapes::Item { key: "road", kind: "lane", use_: &use_, props: &props, phase: "new", context: false };
    let alone = crate::site::shapes::normalise(std::slice::from_ref(&item), &figures).pop().unwrap_or_default();
    let ring: Vec<P2> = alone.outline.first().map(|x| x.outer.clone()).unwrap_or_default();
    if ring.len() < 3 {
        return Err(bad(c, format!("its shape can't be worked out: {}", alone.notes.join(" "))));
    }
    // Its host: given, the one it had, the toposolid under it (or the one it overlaps most: it is clipped at that one's
    // edge, as Revit clips a sub-division); off every toposolid (a street of the context past the surveyed ground), a flat
    // toposolid made under it on its level, as Revit has no sub-division without one.
    let host = match &was {
        Some((_, _, h)) if absent(p.get("host")) => *h,
        _ => match host_param(c, &s.model, p, &ring) {
            Ok(h) => h,
            Err(_) if absent(p.get("host")) => {
                let level = if absent(p.get("level")) {
                    s.model.elements.values().find_map(|e| match e {
                        Element::Toposolid(f) => Some(f.level),
                        _ => None,
                    })
                } else {
                    None
                };
                let level = match level {
                    Some(l) => l,
                    None => level_param(c, &s.model, p, "level")?,
                };
                let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
                for q in &ring {
                    b = [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)];
                }
                ground::make_ground(s.model_mut(), level, b)
            }
            Err(e) => return Err(e),
        },
    };
    let level = match s.model.elements.id(host).and_then(|id| s.model.elements.get(id)) {
        Some(Element::Toposolid(f)) => f.level,
        _ => return Err(bad(c, "its host is no toposolid")),
    };
    // Its definition kept on its pieces, its defaults filled in.
    for (k, v) in &alone.filled {
        props.entry(k.clone()).or_insert_with(|| v.clone());
    }
    // Shaped among the roads on its level: where it ends on one its corners are rounded; where one ends on it, that one is
    // made again with its corners.
    let others = roads_on(&s.model, level, old.map(|r| r.of));
    let mut defs: Vec<(String, String, crate::site::Props)> =
        others.iter().map(|(r, _, _)| (r.of.to_string(), r.use_.clone(), r.props.clone())).collect();
    let before = if others.is_empty() { Vec::new() } else { shaped_among(&s.model, &defs) };
    defs.push(("road".into(), use_.clone(), props.clone()));
    let mut after = shaped_among(&s.model, &defs);
    let mut shaped = after.pop().unwrap_or_default();
    if shaped.outline.is_empty() {
        shaped = alone.clone();
    }
    let name = string(c, p, "name")?.or_else(|| old.map(|r| r.name.clone())).unwrap_or_default();
    let def = ground::RoadDef { of: old.map_or(Guid::default(), |r| r.of), use_, name, props };
    let pieces: Vec<Id> = was.as_ref().map(|(_, ps, _)| ps.clone()).unwrap_or_default();
    let ids = if was.is_some() {
        remake(s, &def, &pieces, host, shaped)?
    } else {
        let site = masterplan::Site {
            kind: "lane".into(),
            use_: def.use_.clone(),
            name: def.name.clone(),
            level,
            phase: "new".into(),
            source: String::new(),
            reference: String::new(),
            props: def.props.clone(),
            outline: shaped.outline,
            parts: shaped.parts,
            notes: Vec::new(),
            params: BTreeMap::new(),
        };
        let made = ground::subdivisions_from(s.model_mut(), &site, host);
        let mut ids = Vec::new();
        for mut e in made {
            if let Element::SubDivision(sd) = &mut e {
                sd.params.clear();
            }
            ids.push(s.model_mut().insert(e));
        }
        ground::link_road(s.model_mut(), &ids);
        ids
    };
    // The roads whose shape it changed (one that ends on it takes its corners), made again.
    let mut joined = Vec::new();
    for (k, (r, ps, h)) in others.iter().enumerate() {
        let (Some(b), Some(a)) = (before.get(k), after.get(k)) else { continue };
        if b.outline != a.outline || b.parts != a.parts {
            let again = remake(s, r, ps, *h, a.clone())?;
            joined.extend(again.first().copied());
        }
    }
    Ok(json!({"ids": ids, "host": s.model.elements.id(host), "joined": joined, "notes": alone.notes}))
}

pub(super) fn excavate(s: &mut Session, p: &Value) -> R {
    let c = "excavate";
    keys(c, p, &["toposolid", "by"])?;
    let topo = toposolid_param(c, &s.model, p, "toposolid")?;
    if absent(p.get("by")) {
        return Err(bad(c, "`by` [id, …], the floors, roofs or toposolids that cut it, is required"));
    }
    let by = ids_param(c, &s.model, &json!({"ids": p.get("by").cloned()}))?;
    let mut guids = Vec::new();
    for id in by {
        match s.model.elements.get(id) {
            _ if id == topo => return Err(bad(c, "a toposolid doesn't excavate itself")),
            Some(Element::Floor(_) | Element::Roof(_) | Element::Toposolid(_)) => guids.push(s.model.guid(id)),
            Some(e) => return Err(bad(c, format!("{} {id} doesn't excavate: a floor, a roof or a toposolid does", e.category()))),
            None => return Err(bad(c, format!("no element {id}"))),
        }
    }
    if let Some(Element::Toposolid(t)) = s.model_mut().elements.get_mut(topo) {
        for g in guids {
            if !t.excavated_by.contains(&g) {
                t.excavated_by.push(g);
            }
        }
    }
    Ok(excavation_answer(&s.model, topo))
}

fn excavation_answer(m: &Model, topo: Id) -> Value {
    let Some(Element::Toposolid(t)) = m.elements.get(topo) else { return json!({"id": topo}) };
    let by: Vec<Option<Id>> = t.excavated_by.iter().map(|g| m.elements.id(*g)).collect();
    let cut = m.toposolid_cut(t);
    json!({"id": topo, "excavated_by": by, "openings": cut.openings.len(), "top": super::top_range(m, &cut)})
}

pub(super) fn remove_excavation(s: &mut Session, p: &Value) -> R {
    let c = "remove_excavation";
    keys(c, p, &["toposolid", "by"])?;
    let topo = toposolid_param(c, &s.model, p, "toposolid")?;
    let only: Option<Vec<Guid>> = if absent(p.get("by")) {
        None
    } else {
        Some(ids_param(c, &s.model, &json!({"ids": p.get("by").cloned()}))?.iter().map(|id| s.model.guid(*id)).collect())
    };
    if let Some(Element::Toposolid(t)) = s.model_mut().elements.get_mut(topo) {
        match &only {
            Some(gs) => t.excavated_by.retain(|g| !gs.contains(g)),
            None => t.excavated_by.clear(),
        }
    }
    Ok(excavation_answer(&s.model, topo))
}

/// Toposolid `f` of model `from`, placed by `place` (a link's), as a toposolid of `m`: on the level of the same name, else
/// the nearest by elevation, its offset made up so it stands at the same height; its points, lines and openings placed;
/// its type made in `m` when `m` hasn't it.
fn copied_ground(m: &mut Model, from: &Model, f: &Floor, place: &dyn Fn(P2) -> P2) -> Floor {
    let (name, elevation) = from.level(f.level).map_or((String::new(), 0.0), |l| (l.name.clone(), l.elevation));
    let levels = m.levels();
    let level = levels
        .iter()
        .find(|(_, l)| l.name == name && (l.elevation - elevation).abs() <= TOL)
        .or_else(|| levels.iter().min_by(|a, b| (a.1.elevation - elevation).abs().total_cmp(&(b.1.elevation - elevation).abs())))
        .map(|(id, l)| (m.guid(*id), l.elevation));
    let (level, here) = level.unwrap_or_default();
    if !m.types.floor.contains_key(&f.type_name)
        && let Some(t) = from.types.floor.get(&f.type_name)
    {
        let mut t = t.clone();
        for l in &mut t.layers {
            if !m.materials.contains_key(&l.material) {
                l.material.clear();
            }
        }
        m.types.floor.insert(f.type_name.clone(), t);
    }
    let mut shape = f.shape.clone();
    for q in &mut shape.points {
        q.at = place(q.at);
        (q.id, q.guid) = (0, Guid::default());
    }
    for l in &mut shape.lines {
        (l.a, l.b) = (place(l.a), place(l.b));
        (l.id, l.guid) = (0, Guid::default());
    }
    Floor {
        type_name: if m.types.floor.contains_key(&f.type_name) { f.type_name.clone() } else { "Toposolid".into() },
        level,
        boundary: f.boundary.iter().map(|q| place(*q)).collect(),
        openings: f.openings.iter().map(|o| o.iter().map(|q| place(*q)).collect()).collect(),
        offset: f.offset + elevation - here,
        slope: 0.0,
        arrow: None,
        structural: false,
        pattern_origin: P2(0.0, 0.0),
        pattern_angle: 0.0,
        sketch: Vec::new(),
        shape,
        contours: f.contours,
        contours_hidden: false,
        excavated_by: Vec::new(),
        graded_from: None,
        params: BTreeMap::new(),
    }
}

pub(super) fn graded_region(s: &mut Session, p: &Value) -> R {
    let c = "graded_region";
    keys(c, p, &["toposolid", "link", "source", "replace", "phase"])?;
    let phases = s.model.phases.clone();
    let phase = match string(c, p, "phase")? {
        Some(ph) if phases.contains(&ph) => ph,
        Some(ph) => return Err(bad(c, format!("no phase `{ph}`; the phases are: {}", phases.join(", ")))),
        None => phases.last().cloned().unwrap_or_default(),
    };
    // The existing ground: in a project given as text, in a link, or in this project; the lowest toposolid there when
    // none is named.
    let lowest = |m: &Model| -> Option<Id> {
        m.elements
            .iter()
            .filter_map(|(id, e)| match e {
                Element::Toposolid(f) if f.graded_from.is_none() => Some((*id, m.elevation(f.level) + f.offset)),
                _ => None,
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    };
    let pick = |m: &Model, v: Option<&Value>| -> Result<Id, String> {
        match v.filter(|v| !v.is_null()) {
            Some(v) => {
                let g = serde_json::from_value::<Guid>(v.clone()).map_err(|_| bad(c, format!("`toposolid`: {v} is no id or GUID")))?;
                let id = m.elements.id(g).ok_or_else(|| bad(c, format!("no toposolid {v} there")))?;
                match m.elements.get(id) {
                    Some(Element::Toposolid(_)) => Ok(id),
                    _ => Err(bad(c, format!("{v} is no toposolid"))),
                }
            }
            None => lowest(m).ok_or_else(|| bad(c, "there is no toposolid there to grade: make the existing ground first")),
        }
    };
    let text_source = string(c, p, "source").or_else(|_| match p.get("source") {
        Some(Value::String(t)) if t.len() <= 256 << 20 => Ok(Some(t.clone())),
        _ => Err(bad(c, "`source` is a project's text")),
    })?;
    if !absent(p.get("source")) && !absent(p.get("link")) {
        return Err(bad(c, "give `link` (a linked model) or `source` (a project's text), not both"));
    }
    let replace = if absent(p.get("replace")) { None } else { Some(toposolid_param(c, &s.model, p, "replace")?) };
    let (new, from, demolish): (Floor, GradedFrom, Option<Id>) = if let Some(text) = text_source {
        let other = Model::read(&text).map(|(m, _)| m).map_err(|e| bad(c, format!("`source` isn't a project: {e}")))?;
        let tid = pick(&other, p.get("toposolid"))?;
        let Some(Element::Toposolid(f)) = other.elements.get(tid) else { return Err(bad(c, "no toposolid")) };
        let cut = other.toposolid_cut(f);
        let new = copied_ground(s.model_mut(), &other, &cut, &|q| q);
        (new, GradedFrom { toposolid: other.guid(tid), link: None, ground: None }, None)
    } else if !absent(p.get("link")) {
        let lid = element_id(c, &s.model, p.get("link").unwrap_or(&Value::Null))?;
        let Some(Element::Link(link)) = s.model.elements.get(lid).cloned() else { return Err(bad(c, format!("{lid} is no link"))) };
        let linked = s.model.loaded.get(&link.path).cloned().ok_or_else(|| bad(c, format!("link `{}` isn't loaded: load it first", link.name)))?;
        let tid = pick(&linked, p.get("toposolid"))?;
        let Some(Element::Toposolid(f)) = linked.elements.get(tid) else { return Err(bad(c, "no toposolid")) };
        let cut = linked.toposolid_cut(f);
        let (at, (sin, cos)) = (link.at, link.angle.to_radians().sin_cos());
        let place = move |q: P2| P2(at.0 + q.0 * cos - q.1 * sin, at.1 + q.0 * sin + q.1 * cos);
        let new = copied_ground(s.model_mut(), &linked, &cut, &place);
        (new, GradedFrom { toposolid: linked.guid(tid), link: Some(s.model.guid(lid)), ground: None }, None)
    } else {
        let tid = pick(&s.model, p.get("toposolid"))?;
        if Some(tid) == replace {
            return Err(bad(c, "a toposolid is graded from another: name the existing ground in `toposolid`"));
        }
        let Some(Element::Toposolid(f)) = s.model.elements.get(tid).cloned() else { return Err(bad(c, "no toposolid")) };
        let cut = s.model.toposolid_cut(&f);
        let here = s.model.clone();
        let new = copied_ground(s.model_mut(), &here, &cut, &|q| q);
        (new, GradedFrom { toposolid: s.model.guid(tid), link: None, ground: None }, Some(tid))
    };
    // The existing ground kept as it was taken, for cut and fill.
    let snapshot = Floor { graded_from: None, excavated_by: Vec::new(), params: BTreeMap::new(), ..new.clone() };
    let from = GradedFrom { ground: Some(Box::new(snapshot)), ..from };
    let id = match replace {
        // Taken again into a proposed ground that is there: its id, type, excavations and sub-divisions stay.
        Some(rid) => {
            if let Some(Element::Toposolid(t)) = s.model_mut().elements.get_mut(rid) {
                let level = new.level;
                *t = Floor {
                    type_name: t.type_name.clone(),
                    excavated_by: std::mem::take(&mut t.excavated_by),
                    params: std::mem::take(&mut t.params),
                    graded_from: Some(from),
                    level,
                    ..new
                };
            }
            rid
        }
        None => {
            let mut params = BTreeMap::new();
            params.insert(phasing::CREATED.to_string(), json!(phase));
            let id = s.model_mut().insert(Element::Toposolid(Floor { graded_from: Some(from), params, ..new }));
            // Revit's Graded Region demolishes the existing ground in the phase the new one is made in.
            if let Some(old) = demolish
                && phases.len() > 1
                && let Some(params) = s.model_mut().elements.get_mut(old).and_then(Element::params_mut)
                && !params.contains_key(phasing::DEMOLISHED)
            {
                params.insert(phasing::DEMOLISHED.to_string(), json!(phase));
            }
            id
        }
    };
    let e = element_json(&s.model, id);
    Ok(json!({"id": id, "cut_m3": e["cut_m3"], "fill_m3": e["fill_m3"], "net_m3": e["net_m3"], "element": e}))
}

pub(super) fn remove_point(s: &mut Session, p: &Value) -> R {
    let c = "remove_point";
    keys(c, p, &["id", "point", "at"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let mut f = s.model.shapeable(id).map_err(|e| bad(c, format!("{id}: {e}")))?;
    let before = f.shape.points.len();
    match (p.get("point").filter(|v| !v.is_null()), p.get("at").filter(|v| !v.is_null())) {
        (Some(v), _) => {
            let n = v.as_u64().ok_or_else(|| bad(c, format!("`point` is a point's id, got {v}")))?;
            f.shape.points.retain(|q| q.id != n);
        }
        (None, Some(_)) => {
            let at = point_req(c, p, "at")?;
            f.shape.points.retain(|q| q.at.dist(at) > TOL);
        }
        (None, None) => return Err(bad(c, "give `point`, the point's id, or `at` [x, y], where it is")),
    }
    if f.shape.points.len() == before {
        return Err(bad(c, "it has no such point"));
    }
    // A split line that ran to it goes too.
    let pts: Vec<P2> = f.shape.points.iter().map(|q| q.at).chain(f.boundary.iter().copied()).chain(f.openings.iter().flatten().copied()).collect();
    f.shape.lines.retain(|l| [l.a, l.b].iter().all(|e| pts.iter().any(|q| q.dist(*e) <= TOL)));
    let shape = f.shape.clone();
    if let Some(sh) = s.model_mut().elements.get_mut(id).and_then(Element::shape_mut) {
        *sh = shape;
    }
    Ok(json!({"id": id, "points": f.shape.points.len(), "top": super::top_range(&s.model, &f)}))
}

/// The two points as a command answers them: where each is, its shared coordinates, the angle to true north, the clip.
fn points_answer(m: &Model) -> Value {
    let one = |x: Option<(Id, &BasePoint)>| {
        x.map(|(id, b)| {
            let [east, north, height] = m.shared(b.at, b.elevation);
            let r = |v: f64| (v * 1000.0).round() / 1000.0;
            json!({"id": id, "at": b.at, "elevation": b.elevation, "east": r(east), "north": r(north), "height": r(height), "clipped": b.clipped})
        })
    };
    json!({"survey": one(m.survey_point()), "project": one(m.project_base_point()), "angle_to_true_north": m.angle_to_true_north()})
}

/// The survey point or project base point, made at the origin (clipped) when the project hasn't it.
fn base_point_id(m: &mut Model, survey: bool) -> Id {
    let found = if survey { m.survey_point() } else { m.project_base_point() }.map(|(id, _)| id);
    found.unwrap_or_else(|| {
        let b = BasePoint { clipped: true, ..BasePoint::default() };
        m.insert(if survey { Element::SurveyPoint(b) } else { Element::ProjectBasePoint(b) })
    })
}

fn base_mut(m: &mut Model, id: Id) -> Option<&mut BasePoint> {
    match m.elements.get_mut(id) {
        Some(Element::SurveyPoint(b) | Element::ProjectBasePoint(b)) => Some(b),
        _ => None,
    }
}

/// The survey point's shared coordinates set so project point `at`, `z` reads `want` (the system moved, the points
/// staying where they are in the project).
fn relocate_shared(m: &mut Model, at: P2, z: f64, want: [f64; 3]) {
    let now = m.shared(at, z);
    let sp = base_point_id(m, true);
    if let Some(b) = base_mut(m, sp) {
        b.east += want[0] - now[0];
        b.north += want[1] - now[1];
        b.height += want[2] - now[2];
    }
}

pub(super) fn base_point(s: &mut Session, p: &Value) -> R {
    let c = "base_point";
    keys(c, p, &["which", "at", "elevation", "east", "north", "height", "angle", "clipped"])?;
    let survey = match string(c, p, "which")?.as_deref() {
        None => {
            if ["at", "elevation", "east", "north", "height", "angle", "clipped"].iter().all(|k| absent(p.get(*k))) {
                return Ok(points_answer(&s.model));
            }
            return Err(bad(c, "`which` is `survey` (the Survey Point) or `project` (the Project Base Point)"));
        }
        Some("survey") => true,
        Some("project") => false,
        Some(w) => return Err(bad(c, format!("`which` is `survey` or `project`, not `{w}`"))),
    };
    let map = |c: &str, key: &str| -> Result<Option<f64>, String> {
        match p.get(key) {
            v if absent(v) => Ok(None),
            Some(v) => v
                .as_f64()
                .filter(|x| x.is_finite() && x.abs() <= crate::model::MAX_MAP)
                .map(Some)
                .ok_or_else(|| bad(c, format!("`{key}` is a shared coordinate in mm, got {v}"))),
            None => Ok(None),
        }
    };
    let (east, north, height) = (map(c, "east")?, map(c, "north")?, map(c, "height")?);
    let angle = num(c, p, "angle")?;
    if survey && angle.is_some() {
        return Err(bad(c, "the angle to true north is the project base point's: `which: project`"));
    }
    if !survey && !absent(p.get("clipped")) {
        return Err(bad(c, "the project base point has no clip (Revit's since 2020.2): only the survey point is clipped"));
    }
    let id = base_point_id(s.model_mut(), survey);
    if survey && !absent(p.get("clipped")) {
        let clipped = flag(c, p, "clipped")?;
        if let Some(b) = base_mut(s.model_mut(), id) {
            b.clipped = clipped;
        }
    }
    // Moved: as the Modify tools move it, by the clip's rule.
    if !absent(p.get("at")) || !absent(p.get("elevation")) {
        let Some(b) = base_mut(s.model_mut(), id).cloned() else { return Err(bad(c, "no point")) };
        let at = if absent(p.get("at")) { b.at } else { point_req(c, p, "at")? };
        let elevation = num(c, p, "elevation")?.unwrap_or(b.elevation);
        let moved_alone = survey && !b.clipped;
        let read = s.model.shared(at, elevation);
        if let Some(b) = base_mut(s.model_mut(), id) {
            if moved_alone {
                (b.east, b.north, b.height) = (read[0], read[1], read[2]);
            }
            (b.at, b.elevation) = (at, elevation);
        }
    }
    if let Some(a) = angle {
        if !(-360.0..=360.0).contains(&a) {
            return Err(bad(c, format!("`angle` is degrees, -360 to 360, not {a}")));
        }
        // Turned about the project base point: it reads the shared coordinates it did.
        let Some(b) = base_mut(s.model_mut(), id).cloned() else { return Err(bad(c, "no point")) };
        let kept = s.model.shared(b.at, b.elevation);
        if let Some(pb) = base_mut(s.model_mut(), id) {
            pb.angle = a;
        }
        relocate_shared(s.model_mut(), b.at, b.elevation, kept);
    }
    if east.is_some() || north.is_some() || height.is_some() {
        let Some(b) = base_mut(s.model_mut(), id).cloned() else { return Err(bad(c, "no point")) };
        let now = s.model.shared(b.at, b.elevation);
        let want = [east.unwrap_or(now[0]), north.unwrap_or(now[1]), height.unwrap_or(now[2])];
        if survey {
            if let Some(sp) = base_mut(s.model_mut(), id) {
                (sp.east, sp.north, sp.height) = (want[0], want[1], want[2]);
            }
        } else {
            // Revit's Specify Coordinates at a Point: the project relocated on the survey so the base point reads them.
            relocate_shared(s.model_mut(), b.at, b.elevation, want);
        }
    }
    Ok(points_answer(&s.model))
}

/// Writes the georeference's numbers into the survey point and project base point (made when missing): the internal
/// origin at (eastings, northings, height) of the map, its axes turned by `rotation`.
pub(super) fn points_from_numbers(m: &mut Model, g: &crate::model::Georeference) {
    let pb = base_point_id(m, false);
    if let Some(b) = base_mut(m, pb) {
        b.angle = g.rotation;
    }
    base_point_id(m, true);
    relocate_shared(m, P2(0.0, 0.0), 0.0, [g.eastings, g.northings, g.height]);
}

/// A family of the site families or the project's, of `category`: its name and type.
fn site_family(c: &str, s: &mut Session, p: &Value, default: &str, category: crate::family::Category) -> Result<(String, String), String> {
    let family = string(c, p, "family")?.unwrap_or_else(|| s.model.language.word(default).to_string());
    if !ground::load_family(s.model_mut(), &family) {
        let ours: Vec<String> = crate::family::site_library(s.model.language).into_iter().filter(|(_, f)| f.category == category).map(|(n, _)| n).collect();
        return Err(bad(c, format!("no family `{family}`: one of the project's, or of the site's ({})", ours.join(", "))));
    }
    let f = s.model.families.get(&family).ok_or_else(|| bad(c, "no family"))?;
    if f.category != category {
        let what = if category == crate::family::Category::Planting { "Planting" } else { "Site" };
        return Err(bad(c, format!("`{family}` is no family of the {what} category; place it with `component`")));
    }
    let type_name = match string(c, p, "type")? {
        Some(t) if f.types.contains_key(&t) => t,
        Some(t) => return Err(bad(c, format!("`{family}` has no type `{t}`; it has: {}", f.types.keys().cloned().collect::<Vec<_>>().join(", ")))),
        None => f.types.keys().next().cloned().unwrap_or_default(),
    };
    Ok((family, type_name))
}

/// Planting or a site component placed on the ground: at `at`, on the level of the toposolid under it (or `level`), its
/// offset the ground's height there.
fn place_on_ground(c: &str, s: &mut Session, p: &Value, default: &str, category: crate::family::Category) -> R {
    keys(c, p, &["family", "type", "at", "level", "angle", "params"])?;
    let at = point_req(c, p, "at")?;
    let (family, type_name) = site_family(c, s, p, default, category)?;
    let m = &s.model;
    let level = if absent(p.get("level")) {
        let under = m
            .elements
            .iter()
            .filter_map(|(_, e)| match e {
                Element::Toposolid(f) if geom::contains(&f.boundary, at) => Some((f.level, m.elevation(f.level) + f.offset)),
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l);
        match under {
            Some(l) => l,
            None => level_param(c, m, p, "level")?,
        }
    } else {
        level_param(c, m, p, "level")?
    };
    let offset = m.ground_at(level, at).map_or(0.0, |(_, z)| z - m.elevation(level));
    let params: BTreeMap<String, Value> = match p.get("params") {
        v if absent(v) => BTreeMap::new(),
        Some(Value::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(v) => return Err(bad(c, format!("`params` is an object {{parameter: value}}, got {v}"))),
        None => BTreeMap::new(),
    };
    let angle = num(c, p, "angle")?.unwrap_or(0.0);
    let comp = crate::model::Component {
        family,
        type_name,
        level,
        at,
        angle,
        offset: (offset * 100.0).round() / 100.0,
        mirrored: false,
        params,
    };
    let id = s.model_mut().insert(Element::Component(comp));
    Ok(json!({"id": id, "offset": (offset * 100.0).round() / 100.0, "element": element_json(&s.model, id)}))
}

pub(super) fn planting(s: &mut Session, p: &Value) -> R {
    place_on_ground("planting", s, p, crate::family::site::TREE, crate::family::Category::Planting)
}

pub(super) fn site_component(s: &mut Session, p: &Value) -> R {
    place_on_ground("site_component", s, p, crate::family::site::BENCH, crate::family::Category::SiteComponent)
}

// ====================================================================== the property line's table
/// A table's rows walked from `start`: the property line's points ([x, y, bulge]) and, where they don't close, the
/// closing segment's distance and bearing. Bearings are against true north (the project base point's angle).
fn walk_table(c: &str, m: &Model, rows: &[Value], start: P2) -> Result<(Vec<[f64; 3]>, Option<(f64, f64)>), String> {
    if !(2..=MAX_POINTS).contains(&rows.len()) {
        return Err(bad(c, format!("`table` has 2 to {MAX_POINTS} rows")));
    }
    let theta = m.angle_to_true_north().to_radians();
    let mut at = start;
    let mut out: Vec<[f64; 3]> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let o = row.as_object().ok_or_else(|| bad(c, format!("row {i} is {{distance, bearing, type?, radius?, side?}}")))?;
        if let Some(k) = o.keys().find(|k| !["distance", "bearing", "type", "radius", "side"].contains(&k.as_str())) {
            return Err(bad(c, format!("row {i}: unknown `{k}`; a row takes distance, bearing, type, radius, side")));
        }
        let d = o.get("distance").and_then(Value::as_f64).filter(|d| d.is_finite() && *d > 0.0 && *d <= MAX_COORD);
        let d = d.ok_or_else(|| bad(c, format!("row {i}: `distance` is a length in mm above 0")))?;
        let az = o.get("bearing").and_then(bearing).ok_or_else(|| {
            bad(c, format!("row {i}: `bearing` is a survey bearing (N 45°30'00\" E) or an azimuth in degrees, got {:?}", o.get("bearing")))
        })?;
        // The way in true north's frame, then in the project's: shared = R(θ)·project.
        let (tx, ty) = (az.to_radians().sin(), az.to_radians().cos());
        let dir = P2(tx * theta.cos() + ty * theta.sin(), -tx * theta.sin() + ty * theta.cos());
        let bulge = match o.get("type").and_then(Value::as_str).unwrap_or("line") {
            "line" => 0.0,
            "arc" => {
                let r = o.get("radius").and_then(Value::as_f64).filter(|r| r.is_finite() && *r > 0.0 && *r <= MAX_COORD);
                let r = r.ok_or_else(|| bad(c, format!("row {i}: an arc needs its `radius`, mm")))?;
                if d > 2.0 * r + 1e-6 {
                    return Err(bad(c, format!("row {i}: a chord of {d} mm is longer than an arc of radius {r} mm reaches")));
                }
                let sweep = 2.0 * (d / (2.0 * r)).min(1.0).asin();
                let b = (sweep / 4.0).tan();
                match o.get("side").and_then(Value::as_str).unwrap_or("L") {
                    "L" | "l" => b,
                    "R" | "r" => -b,
                    other => return Err(bad(c, format!("row {i}: `side` is L or R (the way the arc turns), not `{other}`"))),
                }
            }
            other => return Err(bad(c, format!("row {i}: `type` is line or arc, not `{other}`"))),
        };
        out.push([at.0, at.1, bulge]);
        at = at + dir * d;
    }
    let gap = at.dist(start);
    if gap <= 1.0 {
        return Ok((out, None));
    }
    // Revit's Add Line to Close: the last point joins the first.
    out.push([at.0, at.1, 0.0]);
    let back = start - at;
    let (sx, sy) = (back.0 * theta.cos() - back.1 * theta.sin(), back.0 * theta.sin() + back.1 * theta.cos());
    let az = sx.atan2(sy).to_degrees().rem_euclid(360.0);
    Ok((out, Some((gap, az))))
}

/// `edges` given: a list as long as the segments (each an edge or null), or {index: edge}; each `{setback?, kind?, street?,
/// street_width?}`. Merged into `now`.
pub fn edges_param(c: &str, p: &Value, segments: usize, now: &[Edge]) -> Result<Option<Vec<Edge>>, String> {
    let Some(v) = p.get("edges").filter(|v| !v.is_null()) else { return Ok(None) };
    let mut out: Vec<Edge> = (0..segments).map(|i| now.get(i).cloned().unwrap_or_default()).collect();
    let mut set = |i: usize, e: &Value| -> Result<(), String> {
        if i >= segments {
            return Err(bad(c, format!("edge {i}: the property line has {segments} segments")));
        }
        if e.is_null() {
            out[i] = Edge::default();
            return Ok(());
        }
        let edge: Edge = serde_json::from_value(e.clone())
            .map_err(|err| bad(c, format!("edge {i} is {{setback?, kind?: street|side|rear|corner, street?, street_width?}}: {err}")))?;
        out[i] = edge;
        Ok(())
    };
    match v {
        Value::Array(a) if a.len() <= segments => {
            for (i, e) in a.iter().enumerate() {
                set(i, e)?;
            }
        }
        Value::Array(a) => return Err(bad(c, format!("`edges` has {} entries; the property line has {segments} segments", a.len()))),
        Value::Object(o) => {
            for (k, e) in o {
                let i = k.parse::<usize>().map_err(|_| bad(c, format!("`edges`: `{k}` is no segment's number")))?;
                set(i, e)?;
            }
        }
        _ => return Err(bad(c, "`edges` is a list of edges, one a segment, or {index: edge}")),
    }
    // Trailing segments with nothing said are left out.
    while out.last().is_some_and(|e| *e == Edge::default()) {
        out.pop();
    }
    Ok(Some(out))
}

/// The points and edges of a property line turned to run counter-clockwise, each bulge and edge moved to the segment it
/// now starts.
pub fn counter_clockwise(points: &mut Vec<[f64; 3]>, edges: &mut Vec<Edge>) {
    let ring: Vec<P2> = points.iter().map(|q| P2(q[0], q[1])).collect();
    if geom::area(&ring) >= 0.0 {
        return;
    }
    let n = points.len();
    let old_edges: Vec<Edge> = (0..n).map(|i| edges.get(i).cloned().unwrap_or_default()).collect();
    *points = (0..n)
        .map(|i| {
            let q = points[n - 1 - i];
            let b = points[(2 * n - 2 - i) % n][2];
            [q[0], q[1], -b]
        })
        .collect();
    let mut e: Vec<Edge> = (0..n).map(|i| old_edges[(2 * n - 2 - i) % n].clone()).collect();
    while e.last().is_some_and(|x| *x == Edge::default()) {
        e.pop();
    }
    *edges = e;
}

/// `table` and `start`, if given: the points, and the closure.
pub fn table_param(c: &str, m: &Model, p: &Value) -> Result<Option<(Vec<[f64; 3]>, Option<(f64, f64)>)>, String> {
    let Some(v) = p.get("table").filter(|v| !v.is_null()) else { return Ok(None) };
    let rows = v.as_array().ok_or_else(|| bad(c, "`table` is a list of rows {distance, bearing, type?, radius?, side?}"))?;
    let start = if absent(p.get("start")) { P2(0.0, 0.0) } else { point_req(c, p, "start")? };
    walk_table(c, m, rows, start).map(Some)
}

pub(super) fn property_line_table(s: &mut Session, p: &Value) -> R {
    let c = "property_line_table";
    keys(c, p, &["id"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let Some(Element::PropertyLine(pl)) = s.model.elements.get(id) else { return Err(bad(c, format!("{id} is no property line"))) };
    let first = pl.points.first().map(|q| [q[0], q[1]]).unwrap_or_default();
    let edges: Vec<Value> = (0..pl.points.len()).map(|i| serde_json::to_value(pl.edge(i)).unwrap_or(Value::Null)).collect();
    Ok(json!({"id": id, "start": first, "table": table_of(&s.model, pl), "edges": edges, "angle_to_true_north": s.model.angle_to_true_north()}))
}

pub(super) fn property_line_edit(s: &mut Session, p: &Value) -> R {
    let c = "property_line_edit";
    keys(c, p, &["id", "points", "table", "start", "name", "edges", "level"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    let Some(Element::PropertyLine(mut pl)) = s.model.elements.get(id).cloned() else {
        return Err(bad(c, format!("{id} is no property line")));
    };
    let mut closure = None;
    if !absent(p.get("points")) && !absent(p.get("table")) {
        return Err(bad(c, "give `points` or `table`, not both"));
    }
    let mut fresh = None;
    if let Some((pts, cl)) = table_param(c, &s.model, p)? {
        fresh = Some(pts);
        closure = cl;
    } else if !absent(p.get("points")) {
        fresh = Some(super::site::bulged(c, p, "points")?);
    }
    if let Some(n) = string(c, p, "name")? {
        pl.name = n;
    }
    if !absent(p.get("level")) {
        pl.level = level_param(c, &s.model, p, "level")?;
    }
    match fresh {
        // New points: the edges given are of their segments as given, turned with them; else those it has stay with the
        // segments of the same number, counted counter-clockwise as it holds them.
        Some(mut pts) => {
            let ccw = geom::area(&pts.iter().map(|q| P2(q[0], q[1])).collect::<Vec<_>>()) >= 0.0;
            let kept = if ccw { pl.edges.clone() } else { Vec::new() };
            match edges_param(c, p, pts.len(), &kept)? {
                Some(mut edges) => {
                    counter_clockwise(&mut pts, &mut edges);
                    pl.edges = edges;
                }
                None => counter_clockwise(&mut pts, &mut Vec::new()),
            }
            pl.points = pts;
        }
        None => {
            if let Some(e) = edges_param(c, p, pl.points.len(), &pl.edges)? {
                pl.edges = e;
            }
        }
    }
    pl.edges.truncate(pl.points.len());
    masterplan::check_property_line(c, &pl)?;
    if let Some(Element::PropertyLine(slot)) = s.model_mut().elements.get_mut(id) {
        *slot = pl;
    }
    let e = element_json(&s.model, id);
    let mut out: Map<String, Value> = Map::new();
    out.insert("id".into(), json!(id));
    out.insert("area_m2".into(), e["area_m2"].clone());
    out.insert("buildable_area_m2".into(), e["buildable_area_m2"].clone());
    if let Some((d, az)) = closure {
        out.insert("closure".into(), json!({"distance": (d * 100.0).round() / 100.0, "bearing": bearing_text(az)}));
    }
    out.insert("element".into(), e);
    Ok(Value::Object(out))
}

// ====================================================================== parking
/// Revit's Parking Component: a parking space of the Parking category (the engine's Parking Space, loaded when the project
/// hasn't it, or the project's own) at `at`, of `type` (Standard, Accessible, EV, Motorcycle), turned `angle` (its x along
/// the aisle, its open side to its -y), numbered `mark`; on the surface under it (`host`: a toposolid, a sub-division or a
/// floor; else the toposolid under it, else a floor of its level), kept on the ground as the ground changes.
pub(super) fn parking_component(s: &mut Session, p: &Value) -> R {
    let c = "parking_component";
    keys(c, p, &["at", "type", "family", "host", "level", "angle", "mirrored", "mark", "params"])?;
    let at = point_req(c, p, "at")?;
    let family = string(c, p, "family")?.unwrap_or_else(|| s.model.language.word(crate::family::site::PARKING_SPACE).to_string());
    if !ground::load_family(s.model_mut(), &family) {
        return Err(bad(c, format!("no family `{family}`: one of the project's of the Parking category, or Parking Space")));
    }
    let m = &s.model;
    let f = m.families.get(&family).ok_or_else(|| bad(c, "no family"))?;
    if f.category != crate::family::Category::Parking {
        return Err(bad(c, format!("`{family}` is no family of the Parking category; place it with `component`")));
    }
    let type_name = match string(c, p, "type")? {
        Some(t) if f.types.contains_key(&t) => t,
        Some(t) => return Err(bad(c, format!("`{family}` has no type `{t}`; it has: {}", f.types.keys().cloned().collect::<Vec<_>>().join(", ")))),
        None => {
            let standard = m.language.word(crate::family::site::STANDARD).to_string();
            if f.types.contains_key(&standard) { standard } else { f.types.keys().next().cloned().unwrap_or_default() }
        }
    };
    // Its level: its host's, else the toposolid's under it, else the one given.
    let level = if !absent(p.get("host")) {
        let id = element_id(c, m, p.get("host").unwrap_or(&Value::Null))?;
        match m.elements.get(id) {
            Some(Element::Toposolid(t) | Element::Floor(t)) => t.level,
            Some(Element::SubDivision(sd)) => m.subdivision_level(sd).ok_or_else(|| bad(c, "its host's toposolid isn't in the project"))?,
            Some(e) => return Err(bad(c, format!("a parking space stands on a toposolid, a sub-division or a floor, not a {}", e.category()))),
            None => return Err(bad(c, format!("no element {id}"))),
        }
    } else if absent(p.get("level")) {
        let under = m
            .elements
            .iter()
            .filter_map(|(_, e)| match e {
                Element::Toposolid(f) if geom::contains(&f.boundary, at) => Some((f.level, m.elevation(f.level) + f.offset)),
                _ => None,
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(l, _)| l);
        match under {
            Some(l) => l,
            None => level_param(c, m, p, "level")?,
        }
    } else {
        level_param(c, m, p, "level")?
    };
    let (host, offset) = crate::carpark::surface_at(m, level, at);
    let mut params: BTreeMap<String, Value> = match p.get("params") {
        v if absent(v) => BTreeMap::new(),
        Some(Value::Object(o)) => o.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(v) => return Err(bad(c, format!("`params` is an object {{parameter: value}}, got {v}"))),
        None => BTreeMap::new(),
    };
    if let Some(mark) = string(c, p, "mark")? {
        params.insert(m.language.word(crate::family::site::MARK).to_string(), json!(mark));
    }
    let comp = crate::model::Component {
        family,
        type_name,
        level,
        at,
        angle: num(c, p, "angle")?.unwrap_or(0.0),
        offset,
        mirrored: flag(c, p, "mirrored")?,
        params,
    };
    let id = s.model_mut().insert(Element::Component(comp));
    Ok(json!({"id": id, "offset": offset, "host": host.and_then(|g| s.model.elements.id(g)), "element": element_json(&s.model, id)}))
}

/// The settings a car park takes (its props), each by name at the top of `car_park`'s params.
const CAR_PARK_OWN: [&str; 4] = ["level", "name", "phase", "props"];

/// The car park generator (docs/parking-ramps-plan.md): a car park's area (`control`) on `level`, its settings, laid out at
/// once as Parking Spaces and drive aisles among what stands on its level. Its result.
pub(super) fn car_park(s: &mut Session, p: &Value) -> R {
    let c = "car_park";
    let Some(o) = p.as_object() else { return Err(bad(c, "takes {control: [[x, y], …], level?, …settings}")) };
    let mut props: Map<String, Value> = match o.get("props") {
        Some(Value::Object(m)) => m.clone(),
        v if absent(v) => Map::new(),
        Some(v) => return Err(bad(c, format!("`props` is an object of settings, got {v}"))),
        None => Map::new(),
    };
    for (k, v) in o.iter().filter(|(k, _)| !CAR_PARK_OWN.contains(&k.as_str())) {
        props.insert(k.clone(), v.clone());
    }
    if !props.contains_key("control") {
        return Err(bad(c, "`control` [[x, y], …], the car park's area, is required"));
    }
    let mut site = json!({"kind": "car_park", "props": props});
    for k in ["level", "name", "phase"] {
        if let Some(v) = o.get(k) {
            site[k] = v.clone();
        }
    }
    let made = super::site_cmd(s, &site).map_err(|e| e.replacen("site", "car_park", 1))?;
    let id = made["id"].as_u64().ok_or_else(|| bad(c, "no car park made"))?;
    Ok(car_park_answer(&s.model, id))
}

/// What a car park came to: its result (bays, notes, proposed columns…), its bays' and aisles' ids.
fn car_park_answer(m: &Model, id: Id) -> Value {
    let me = m.guid(id).to_string();
    let (mut bays, mut aisles, mut kept) = (Vec::new(), Vec::new(), Vec::new());
    for (eid, g, e) in m.elements.with_guids() {
        let (params, bay) = match e {
            Element::Component(c) => (&c.params, true),
            Element::SubDivision(sd) => (&sd.params, false),
            _ => continue,
        };
        if params.get(crate::carpark::BY).and_then(Value::as_str) != Some(me.as_str()) {
            continue;
        }
        match (bay, m.pinned.contains(&g)) {
            (true, true) => kept.push(eid),
            (true, false) => bays.push(eid),
            (false, _) => aisles.push(eid),
        }
    }
    let result = match m.elements.get(id) {
        Some(Element::Site(st)) => st.props.get("result").cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    };
    json!({"id": id, "result": result, "bays": bays, "kept": kept, "aisles": aisles})
}

/// A car park laid out again now: what it came to.
pub(super) fn car_park_run(s: &mut Session, p: &Value) -> R {
    let c = "car_park_run";
    keys(c, p, &["id"])?;
    let id = element_id(c, &s.model, p.get("id").unwrap_or(&Value::Null))?;
    if !matches!(s.model.elements.get(id), Some(Element::Site(st)) if st.kind == "car_park") {
        return Err(bad(c, format!("{id} is no car park")));
    }
    crate::carpark::run_now(s.model_mut(), id);
    Ok(car_park_answer(&s.model, id))
}

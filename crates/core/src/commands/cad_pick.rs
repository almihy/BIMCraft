//! Building from an imported drawing's lines, as Revit's Pick Lines does (docs/areas/cad-import.md): `cad_outlines`, the
//! closed outlines an import's lines make, and the `cad` pick that `wall`, `floor`, `property_line`, `building_pad` and
//! `toposolid` take instead of points.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{R, absent, bad, cad_import, hundredth, layer_list, m2, point};
use crate::cad::outlines::{self, Outline, Pick, Segment};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Guid, Model};

/// The keys a `cad` pick takes.
const PICK_KEYS: [&str; 4] = ["id", "entities", "layers", "box"];

/// What a pick takes of an import: its level, and the entities.
pub(super) struct Picked {
    pub level: Guid,
    pub found: outlines::Found,
    pub labels: Vec<Vec<String>>,
    pub cad: String,
    pub id: crate::model::Id,
}

/// The pick `v` gives (`{id, entities?, layers?, box?}`), on the import `id` names: the entities it takes.
fn pick(c: &str, m: &Model, v: &Value) -> Result<(crate::model::Id, Guid, String, Vec<usize>), String> {
    let (id, ci) = cad_import(c, m, v)?;
    let cad = m.cads.get(&ci.cad).ok_or_else(|| bad(c, format!("the project holds no CAD drawing named `{}`", ci.cad)))?;
    let count = cad.dxf().entities.len();
    let entities = match v.get("entities") {
        e if absent(e) => None,
        Some(Value::Array(a)) if a.len() <= outlines::MAX_PICKED => Some(
            a.iter()
                .map(|n| match n.as_u64().and_then(|n| usize::try_from(n).ok()) {
                    Some(n) if n < count => Ok(n),
                    Some(n) => Err(bad(c, format!("the drawing has no entity {n}: it has {count}, from 0 to {}", count.saturating_sub(1)))),
                    None => Err(bad(c, format!("`entities` lists entities by their index in the drawing (0 and up), not {n}"))),
                })
                .collect::<Result<BTreeSet<usize>, String>>()?,
        ),
        Some(e) => {
            return Err(bad(c, format!("`entities` is a list of up to {} entity indices, not {e}", outlines::MAX_PICKED)));
        }
        None => None,
    };
    let layers = match v.get("layers") {
        l if absent(l) => None,
        _ => Some(layer_list(c, v, "layers", cad)?.into_iter().collect::<BTreeSet<String>>()),
    };
    let window = match v.get("box") {
        b if absent(b) => None,
        Some(Value::Array(a)) => match a.as_slice() {
            [p, q] => match (point(p), point(q)) {
                (Some(p), Some(q)) => Some([P2(p.0.min(q.0), p.1.min(q.1)), P2(p.0.max(q.0), p.1.max(q.1))]),
                _ => return Err(bad(c, "`box` is two corners [[x, y], [x, y]] in mm")),
            },
            _ => return Err(bad(c, "`box` is two corners [[x, y], [x, y]] in mm")),
        },
        Some(_) => return Err(bad(c, "`box` is two corners [[x, y], [x, y]] in mm")),
        None => None,
    };
    let picked = outlines::picked(cad, ci, &Pick { entities, layers, window });
    Ok((id, ci.level, ci.cad.clone(), picked))
}

/// The closed outlines the pick `v` makes.
fn found(c: &str, m: &Model, v: &Value) -> Result<Picked, String> {
    let (id, level, name, chosen) = pick(c, m, v)?;
    let (cad, ci) = match (m.cads.get(&name), m.elements.get(id)) {
        (Some(cad), Some(crate::model::Element::CadImport(ci))) => (cad, ci),
        _ => return Err(bad(c, format!("the project holds no CAD drawing named `{name}`"))),
    };
    let found = outlines::outlines(cad, ci, &chosen).map_err(|e| bad(c, e))?;
    let labels = outlines::labels(cad, ci, &found.outlines);
    Ok(Picked { level, found, labels, cad: name, id })
}

/// `cad`, the pick an element command takes: an object of [`PICK_KEYS`], if given.
fn pick_param<'p>(c: &str, p: &'p Value) -> Result<Option<&'p Value>, String> {
    match p.get("cad") {
        v if absent(v) => Ok(None),
        Some(v @ Value::Object(o)) => match o.keys().find(|k| !PICK_KEYS.contains(&k.as_str())) {
            Some(k) => Err(bad(c, format!("`cad` takes {}; not `{k}`", PICK_KEYS.join(", ")))),
            None => Ok(Some(v)),
        },
        Some(v) => Err(bad(c, format!("`cad` is a pick of a CAD import's lines {{id, entities?, layers?, box?}}, not {v}"))),
        None => Ok(None),
    }
}

/// Refuses `keys` given with `cad`.
fn not_with(c: &str, p: &Value, keys: &[&str]) -> Result<(), String> {
    match keys.iter().find(|k| !absent(p.get(**k))) {
        Some(k) => Err(bad(c, format!("give the outline as `cad` or as `{k}`, not both"))),
        None => Ok(()),
    }
}

fn round(p: P2) -> P2 {
    P2(hundredth(p.0), hundredth(p.1))
}

/// The one closed outline the `cad` pick of an element command makes (the outermost of those its lines make, with its
/// holes), and the import's level; none when `cad` isn't given. `instead` are the keys it takes the place of.
pub(super) fn picked_outline(c: &str, m: &Model, p: &Value, instead: &[&str]) -> Result<Option<(Guid, Vec<P2>, Vec<Vec<P2>>)>, String> {
    let Some(v) = pick_param(c, p)? else { return Ok(None) };
    not_with(c, p, instead)?;
    let picked = found(c, m, v)?;
    let top: Vec<&Outline> = picked.found.outlines.iter().filter(|o| o.depth == 0 && !o.face).collect();
    match top.as_slice() {
        [one] => Ok(Some((
            picked.level,
            one.ring.iter().copied().map(round).collect(),
            one.holes.iter().map(|h| h.iter().copied().map(round).collect()).collect(),
        ))),
        [] => Err(bad(
            c,
            format!(
                "the picked entities make no closed outline ({} open chains: their ends don't meet within {} mm); see what they make with `cad_outlines`",
                picked.found.open.len(),
                outlines::SNAP
            ),
        )),
        many => Err(bad(
            c,
            format!(
                "the picked entities make {} closed outlines side by side, not one; pick one's entities (`entities`, or a `box` round it), as `cad_outlines` lists them",
                many.len()
            ),
        )),
    }
}

/// The lines the `cad` pick of `wall` takes, one wall each, the import's level and what drew no line; none when `cad` isn't
/// given.
pub(super) fn picked_segments(c: &str, m: &Model, p: &Value) -> Result<Option<(Guid, Vec<Segment>, usize)>, String> {
    let Some(v) = pick_param(c, p)? else { return Ok(None) };
    not_with(c, p, &["points", "closed", "arc"])?;
    let (id, level, name, chosen) = pick(c, m, v)?;
    let (Some(cad), Some(crate::model::Element::CadImport(ci))) = (m.cads.get(&name), m.elements.get(id)) else {
        return Err(bad(c, format!("the project holds no CAD drawing named `{name}`")));
    };
    let (segs, skipped) = outlines::segments(cad, ci, &chosen);
    let segs: Vec<Segment> = segs.into_iter().filter(|s| s.start.dist(s.end) > outlines::SNAP).collect();
    if segs.is_empty() {
        return Err(bad(c, format!("the pick takes {} entities and no line or arc among them to draw a wall along", chosen.len())));
    }
    if segs.len() > crate::model::MAX_POINTS {
        return Err(bad(c, format!("the pick takes {} lines; up to {} walls at once", segs.len(), crate::model::MAX_POINTS)));
    }
    Ok(Some((level, segs, skipped)))
}

/// The closed outlines an import's lines make: `cad_outlines`.
pub(super) fn cad_outlines(s: &mut Session, p: &Value) -> R {
    let c = "cad_outlines";
    if let Value::Object(o) = p
        && let Some(k) = o.keys().find(|k| !PICK_KEYS.contains(&k.as_str()) && k.as_str() != "min_area")
    {
        return Err(bad(c, format!("unknown parameter `{k}`; it takes: id, entities, layers, box, min_area")));
    }
    let min_area = match p.get("min_area") {
        v if absent(v) => 0.0,
        Some(v) => v
            .as_f64()
            .filter(|a| a.is_finite() && *a >= 0.0)
            .ok_or_else(|| bad(c, format!("`min_area` is the least area an outline listed has, mm², 0 or more, not {v}")))?,
        None => 0.0,
    };
    let m = &s.model;
    let picked = found(c, m, p)?;
    let ents = m.cads.get(&picked.cad).map(|cad| &cad.dxf().entities);
    let layer_of = |n: &usize| ents.and_then(|e| e.get(*n)).map(|e| e.layer.clone());
    let ring = |r: &[P2]| json!(r.iter().map(|q| [hundredth(q.0), hundredth(q.1)]).collect::<Vec<_>>());
    let rows: Vec<Value> = picked
        .found
        .outlines
        .iter()
        .zip(&picked.labels)
        .filter(|(o, _)| o.area() >= min_area)
        .map(|(o, labels)| {
            let mut row = json!({
                "points": ring(&o.ring), "area": hundredth(o.area()), "area_m2": m2(o.area()), "from": o.from.name(),
                "entities": o.entities, "labels": labels,
            });
            let layers: BTreeSet<String> = o.entities.iter().filter_map(layer_of).collect();
            match layers.len() {
                1 => row["layer"] = json!(layers.iter().next()),
                _ => row["layers"] = json!(layers),
            }
            if !o.holes.is_empty() {
                row["holes"] = json!(o.holes.iter().map(|h| ring(h)).collect::<Vec<_>>());
            }
            if o.whole {
                row["whole"] = json!(true);
            }
            row
        })
        .collect();
    let open: Vec<Value> = picked.found.open.iter().map(|o| json!({"points": ring(&o.points), "entities": o.entities})).collect();
    Ok(json!({"id": picked.id, "cad": picked.cad, "outlines": rows, "open": open}))
}

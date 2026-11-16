//! The car park generator (docs/parking-ramps-plan.md): a car park (the masterplan's `car_park`, its area and settings on a
//! level) lays out Revit's Parking Components and, at ground level, its drive aisles as sub-divisions, among what stands
//! on its level as Revit's cut plane shows it: walls, columns, shafts, stairs, ramps, floor openings, components, masses.
//! The solver ([`crate::site::parking`]) keeps its layouts; this reads the model for it and writes ordinary elements.
//!
//! - Its own bays and aisles are linked to it: [`BY`] (its GUID) and [`PLACED`] (where and how it placed them) in their
//!   params. One the person moved, changed or pinned is kept as left, and pinned; one they deleted stays out (its middle
//!   added to the car park's `removed`); the rest are replaced each time it runs, keeping their ids where they come back.
//! - It uses the real columns and never makes its own: below ground with none in its area it proposes a grid in its result.
//! - Below ground its ways in are the real ramps touching its level (their foot or head on it), or the ways the person
//!   placed (`access`). It never invents a ramp: with none it lays out nothing and says so.
//! - It runs again after a command that changed it or anything on its level near it ([`keep`]), and by `car_park_run`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::geom::{self, P2};
use crate::model::{Changed, Component, Element, Guid, Id, Model, heading};
use crate::site::parking::{self, Among, LevelIn, Obstacle, solve::Solved, standards::Given};
use crate::site::{Poly, Props, shapes};

/// The params a generated element keeps of the car park that made it: its GUID, and where and how it placed it.
pub const BY: &str = "car_park";
pub const PLACED: &str = "placed";
/// What a car park's drive aisle is, as a sub-division's role and type.
pub const AISLE_ROLE: &str = "car_park/aisle";
pub const DRIVE_AISLE: &str = "Drive Aisle";
/// How near a change must be to a car park for it to run again, mm.
const NEAR: f64 = 10_000.0;
/// The height over its level a car needs clear: what stands in it is kept clear.
const HEADROOM: f64 = 2100.0;
/// A storey's height where a level has none above or below it.
const STOREY: f64 = 3500.0;

// ====================================================================== what it writes
/// A car park run: its result and its removed bays after it, what goes, what is put (replacing an id, or new),
/// what is pinned, and what the project needs first (the parking family, types of it, the drive aisle's type).
pub struct Laid {
    park: Id,
    result: Value,
    removed: Option<Value>,
    remove: Vec<Id>,
    put: Vec<(Option<Id>, Element)>,
    pin: Vec<Guid>,
    types: Vec<(String, BTreeMap<String, Value>)>,
}

/// What [`keep`] changes: a car park laid out again, or a car park gone (its own unpinned bays and aisles go with it, its
/// pinned ones stay, no longer linked).
pub enum Update {
    Laid(Box<Laid>),
    Gone { remove: Vec<Id>, unlink: Vec<Id> },
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// The firm's sizes for a car park, metres, read by the firm's names and the old ones (`shapes::FIGURES`): its two-way
/// and one-way aisles, its stalls, its accessible stall, its angle, its EV share, its ramps' slope, its floor to floor.
pub fn firm_sizes(m: &Model) -> (Given, Option<f64>) {
    let f = crate::masterplan::figures(m);
    let mm = |names: &[&str]| shapes::figure_of(&f, names).map(|v| v / 1000.0);
    let plain = |names: &[&str]| shapes::figure_of(&f, names);
    let given = Given {
        stall_width: mm(&["parking.stall.width", "parking.bay_width"]),
        stall_depth: mm(&["parking.stall.depth", "parking.bay_depth"]),
        aisle: mm(&["parking.aisle.width", "parking.aisle"]),
        accessible_width: mm(&["parking.accessible.width"]),
        angle: plain(&["parking.stall.angle"]).filter(|a| (30.0..=90.0).contains(a)),
        ev_share: plain(&["parking.ev.share"]).filter(|s| (0.0..=1.0).contains(s)),
        ramp_slope: plain(&["parking.ramp.slope"]).map(|s| s / 100.0).filter(|s| (0.02..=0.5).contains(s)),
        floor_to_floor: mm(&["parking.level_height"]),
        ..Given::default()
    };
    (given, mm(&["parking.aisle.one_way"]))
}

/// A car park's level's storey: the level below's elevation, its own, the level above's (a storey's height off where
/// there is none).
fn storey(m: &Model, level: Guid) -> (f64, f64, f64) {
    let e = m.elevation(level);
    let levels = m.levels();
    let below = levels.iter().rev().find(|(_, l)| l.elevation < e - 1.0).map_or(e - STOREY, |(_, l)| l.elevation);
    let above = levels.iter().find(|(_, l)| l.elevation > e + 1.0).map_or(e + STOREY, |(_, l)| l.elevation);
    (below, e, above)
}

/// Its GUID as its bays name it.
fn key(g: Guid) -> String {
    g.to_string()
}

fn by_of(params: &BTreeMap<String, Value>) -> Option<&str> {
    params.get(BY).and_then(Value::as_str)
}

/// What a bay's [`PLACED`] says: where, turned how, of which type, on which level, numbered what.
fn placed_of(c: &Component) -> String {
    let mark = c.params.get(crate::family::site::MARK).and_then(Value::as_str).unwrap_or("");
    format!("{:.0},{:.0},{:.3},{},{},{},{}", c.at.0, c.at.1, c.angle, c.mirrored, c.type_name, c.level, mark)
}

/// What an aisle's [`PLACED`] says: its outline.
fn placed_of_aisle(sd: &crate::ground::SubDivision) -> String {
    let pts: Vec<String> = sd.boundary.iter().chain(sd.openings.iter().flatten()).map(|q| format!("{:.0} {:.0}", q.0, q.1)).collect();
    format!("{}|{}", sd.type_name, pts.join(","))
}

/// A parking space's outline: `at` its middle, turned `angle`, `w` wide square to its sides, reaching `d` from its aisle,
/// its sides at `a` degrees to it (the family's, `family/site.rs`).
pub fn bay_ring(at: P2, angle: f64, mirrored: bool, w: f64, d: f64, a: f64) -> Vec<P2> {
    let s = a.to_radians().sin().max(0.1);
    let (h, k) = (w / s / 2.0, d * a.to_radians().cos() / s / 2.0);
    let flip = if mirrored { -1.0 } else { 1.0 };
    let (x, y) = (heading(angle), heading(angle).perp() * flip);
    [(-h - k, -d / 2.0), (h - k, -d / 2.0), (h + k, d / 2.0), (-h + k, d / 2.0)].iter().map(|(u, v)| at + x * *u + y * *v).collect()
}

/// A Parking component's Width, Depth and Angle, its type's (or the family's).
pub fn bay_size(m: &Model, c: &Component) -> Option<(f64, f64, f64)> {
    let f = m.families.get(&c.family)?;
    let w = |s: &str| m.language.word(s).to_string();
    let v = |name: &str| -> Option<f64> {
        f.types
            .get(&c.type_name)
            .and_then(|t| t.get(name))
            .and_then(Value::as_f64)
            .or_else(|| f.parameters.iter().find(|p| p.name == name).and_then(|p| p.value.as_f64()))
    };
    use crate::family::site as fam;
    Some((v(&w(fam::WIDTH))?, v(&w(fam::DEPTH))?, v(&w(fam::ANGLE)).unwrap_or(90.0)))
}

/// Whether component `c` is a parking space (of the Parking category).
pub fn is_bay(m: &Model, c: &Component) -> bool {
    m.families.get(&c.family).is_some_and(|f| f.category == crate::family::Category::Parking)
}

/// What a parking space stands on at `at` on `level`: the toposolid there (its surface), else a floor of the level holding
/// it (its top), else nothing (its level). The host and its height over the level.
pub fn surface_at(m: &Model, level: Guid, at: P2) -> (Option<Guid>, f64) {
    if let Some((id, z)) = m.ground_at(level, at) {
        return (Some(m.guid(id)), round2(z - m.elevation(level)));
    }
    let floor = m.elements.with_guids().find_map(|(_, g, e)| match e {
        Element::Floor(f) if f.level == level && geom::contains(&f.boundary, at) && !f.openings.iter().any(|o| geom::contains(o, at)) => {
            Some((g, f.offset))
        }
        _ => None,
    });
    match floor {
        Some((g, off)) => (Some(g), round2(off)),
        None => (None, 0.0),
    }
}

// ====================================================================== what it reads
/// The rings in plan of a stair's or a ramp's runs and landings whose heights reach between `lo` and `hi`.
fn flight_rings(m: &Model, e: &Element, lo: f64, hi: f64) -> Vec<Vec<P2>> {
    let Some(l) = crate::stairs::layout(m, e) else { return Vec::new() };
    let mut out: Vec<Vec<P2>> =
        l.runs.iter().filter(|r| r.z.0.min(r.z.1) < hi && r.z.0.max(r.z.1) > lo).map(|r| r.ring(l.width)).collect();
    out.extend(l.pads.iter().filter(|p| p.z > lo && p.z < hi).map(|p| p.ring.clone()));
    out
}

/// The ends of ramp `r` on the level at `z`: the line across its foot (its first run's start) when its base is there, its
/// head (its last run's end) when its top is; with the way a car comes off it onto the level.
fn ramp_ends(m: &Model, r: &crate::model::Ramp, z: f64) -> Vec<((P2, P2), P2)> {
    let (base, top) = m.span(r.level, 0.0, r.top_level, r.height);
    let l = crate::stairs::ramp(m, r);
    let mut out = Vec::new();
    if (base - z).abs() < 1.0
        && let Some(first) = l.runs.first()
    {
        let ring = first.ring(l.width);
        out.push(((ring[0], ring[3]), first.u * -1.0));
    }
    if (top - z).abs() < 1.0
        && let Some(last) = l.runs.last()
    {
        let ring = last.ring(l.width);
        out.push(((ring[1], ring[2]), last.u));
    }
    out
}

/// A way placed by hand below ground (`access`) as the foot of a ramp there: a drive aisle's width along the area's
/// nearest side, a car coming off it into the area.
fn foot_at(area: &[P2], at: P2, width: f64) -> Option<((P2, P2), P2)> {
    let n = area.len();
    let (mut best, mut far) = (None, f64::INFINITY);
    for i in 0..n {
        let (a, b) = (area[i], area[(i + 1) % n]);
        let d = b - a;
        let len = d.dot(d).sqrt();
        if len < 1.0 {
            continue;
        }
        let t = ((at - a).dot(d) / (len * len)).clamp(0.0, 1.0);
        let q = a + d * t;
        let dist = q.dist(at);
        if dist < far {
            far = dist;
            best = Some((q, d * (1.0 / len)));
        }
    }
    let (q, u) = best?;
    // Into the area: the side's left when it runs counter-clockwise round it.
    let inward = if geom::area(area) >= 0.0 { u.perp() } else { u.perp() * -1.0 };
    Some(((q - u * (width / 2.0), q + u * (width / 2.0)), inward))
}

/// A car park's area: its control ring, as its props say it (its outline may not be shaped yet), else its outline.
fn area_of(s: &crate::masterplan::Site) -> Vec<P2> {
    let control: Vec<P2> = s
        .props
        .get("control")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|q| Some(P2(q.get(0)?.as_f64()?, q.get(1)?.as_f64()?)))
        .collect();
    if control.len() >= 3 { control } else { s.outline.first().map(|p| p.outer.clone()).unwrap_or_default() }
}

/// What a car park reads of the model: what stands on its level, kept clear by kind or exactly, the ramps' ends there,
/// the street edges, and its own bays and aisles, kept or to replace.
struct Read {
    obstacles: Vec<Obstacle>,
    among: Among,
    streets: Vec<(P2, P2)>,
    /// Its own bays and aisles to replace: id, and what it placed (to keep the id where one comes back).
    replace: Vec<(Id, String)>,
    /// Its own bays the person kept (moved, changed or pinned): their ids and whether they are pinned already.
    kept: Vec<(Id, bool)>,
    /// Real columns within its area.
    columns: usize,
}

fn read(m: &Model, park: Id, s: &crate::masterplan::Site) -> Read {
    let level = s.level;
    let level_name = m.level(level).map(|l| l.name.clone()).unwrap_or_default();
    let (below, e, above) = storey(m, level);
    let me = key(m.guid(park));
    let area = area_of(s);
    let reach = {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for q in &area {
            b = [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)];
        }
        [b[0] - NEAR, b[1] - NEAR, b[2] + NEAR, b[3] + NEAR]
    };
    let near = |ring: &[P2]| ring.iter().any(|q| q.0 >= reach[0] && q.0 <= reach[2] && q.1 >= reach[1] && q.1 <= reach[3]);
    // What a car meets standing on the level: between its floor and its headroom.
    let stands = |lo: f64, hi: f64| lo < e + HEADROOM && hi > e + 10.0;
    let mut out = Read { obstacles: Vec::new(), among: Among::default(), streets: Vec::new(), replace: Vec::new(), kept: Vec::new(), columns: 0 };
    let keep = |ring: Vec<P2>, out: &mut Read| {
        if ring.len() >= 3 && near(&ring) {
            out.among.keep.push(ring);
        }
    };
    for (id, g, el) in m.elements.with_guids() {
        if id == park {
            continue;
        }
        match el {
            // The masterplan's own, on the level, by kind (as phase 1 had them).
            Element::Site(o) if o.level == level => {
                let kind = if o.kind == "lane" { format!("lane/{}", o.use_or_default()) } else { o.kind.clone() };
                out.obstacles.push(Obstacle { kind, level: level_name.clone(), outline: o.outline.clone() });
            }
            // Sub-divisions by their role: a road a way in, water kept clear, paving and lawns driven over; the drive
            // aisles of car parks are ground it may use.
            Element::SubDivision(sd) if !sd.role.is_empty() && m.subdivision_level(sd) == Some(level) => {
                if let Some(by) = by_of(&sd.params)
                    && by == me
                {
                    out.replace.push((id, placed_of_aisle(sd)));
                    continue;
                }
                out.obstacles.push(Obstacle {
                    kind: sd.role.clone(),
                    level: level_name.clone(),
                    outline: crate::ground::grouped(&m.subdivision_region(sd))
                        .into_iter()
                        .map(|(outer, holes)| Poly { outer, holes })
                        .collect(),
                });
            }
            Element::Mass(ms) if ms.level == level => out.obstacles.push(Obstacle {
                kind: "mass".into(),
                level: level_name.clone(),
                outline: vec![Poly { outer: ms.footprint(), holes: Vec::new() }],
            }),
            Element::Wall(w) if stands(m.base_z(w), m.top_z(w)) => keep(crate::walls::outline(m, id), &mut out),
            Element::Column(_) | Element::StructuralColumn(_) => {
                let Some(c) = el.column() else { continue };
                let (lo, hi) = m.span(c.level, c.base_offset, c.top_level, c.height);
                if stands(lo, hi) {
                    let ring = m.column_outline(el);
                    if area.len() >= 3 && geom::contains(&area, geom::centroid(&ring)) {
                        out.columns += 1;
                    }
                    keep(ring, &mut out);
                }
            }
            Element::Shaft(sh) => {
                let (lo, hi) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
                if lo < above - 10.0 && hi > below + 10.0 && (stands(lo, hi) || lo < e) {
                    keep(sh.boundary.clone(), &mut out);
                }
            }
            // A stair or a ramp in the storey, or under the level down to the one below (a hole in its floor): kept clear
            // where it is; a ramp's end on the level is a way in below ground.
            Element::Stair(_) | Element::Ramp(_) => {
                for ring in flight_rings(m, el, below + 10.0, above - 10.0) {
                    keep(ring, &mut out);
                }
                if let Element::Ramp(r) = el {
                    out.among.feet.extend(ramp_ends(m, r, e).into_iter().filter(|((a, b), _)| near(&[*a, *b])));
                }
            }
            Element::Floor(f) if f.level == level => {
                for o in &f.openings {
                    keep(o.clone(), &mut out);
                }
            }
            Element::Component(c) => {
                if is_bay(m, c) && by_of(&c.params) == Some(me.as_str()) {
                    let pinned = m.pinned.contains(&g);
                    let as_placed = c.params.get(PLACED).and_then(Value::as_str) == Some(placed_of(c).as_str());
                    if pinned || !as_placed {
                        out.kept.push((id, pinned));
                        if let Some((w, d, a)) = bay_size(m, c) {
                            keep(bay_ring(c.at, c.angle, c.mirrored, w, d, a), &mut out);
                        }
                    } else {
                        out.replace.push((id, placed_of(c)));
                    }
                    continue;
                }
                if c.level == level && is_bay(m, c) {
                    if let Some((w, d, a)) = bay_size(m, c) {
                        keep(bay_ring(c.at, c.angle, c.mirrored, w, d, a), &mut out);
                    }
                    continue;
                }
                for p in m.component_pieces(c) {
                    if stands(p.bottom, p.top) {
                        keep(p.outline, &mut out);
                    }
                }
            }
            // The plot's street edges, as the property lines say them.
            Element::PropertyLine(pl) => {
                let n = pl.points.len();
                for i in 0..n {
                    if pl.edge(i).kind == "street" {
                        let (a, b) = (pl.points[i], pl.points[(i + 1) % n]);
                        out.streets.push((P2(a[0], a[1]), P2(b[0], b[1])));
                    }
                }
            }
            _ => {}
        }
    }
    // Below ground, a way the person placed is a ramp's foot there.
    if e < -1e-3 {
        let width = s.props.get("aisle").and_then(Value::as_f64).unwrap_or(6000.0);
        for a in s.props.get("access").and_then(Value::as_array).into_iter().flatten() {
            if let Some(at) = a.get("at").and_then(Value::as_array).and_then(|q| Some(P2(q.first()?.as_f64()?, q.get(1)?.as_f64()?)))
                && let Some(foot) = foot_at(&area, at, width)
            {
                out.among.feet.push(foot);
            }
        }
    }
    (out.among.defaults, out.among.aisle_one_way) = firm_sizes(m);
    out
}

// ====================================================================== running it
/// The type of family `family` a bay of `base` (Standard, Accessible, EV) `w` by `d` at `a` degrees is: `base` when its
/// sizes are those, else one named for them (made with them when the family hasn't it).
fn type_for(f: &crate::family::Family, language: crate::language::Language, base: &str, w: f64, d: f64, a: f64) -> (String, Option<BTreeMap<String, Value>>) {
    use crate::family::site as fam;
    let word = |s: &str| language.word(s).to_string();
    let base = word(base);
    let v = |t: &str, name: &str| -> Option<f64> {
        f.types.get(t).and_then(|t| t.get(name)).and_then(Value::as_f64).or_else(|| f.parameters.iter().find(|p| p.name == name).and_then(|p| p.value.as_f64()))
    };
    let (wn, dn, an) = (word(fam::WIDTH), word(fam::DEPTH), word(fam::ANGLE));
    let same = |t: &str| {
        v(t, &wn).is_some_and(|x| (x - w).abs() < 1.0) && v(t, &dn).is_some_and(|x| (x - d).abs() < 1.0) && (v(t, &an).unwrap_or(90.0) - a).abs() < 0.01
    };
    if f.types.contains_key(&base) && same(&base) {
        return (base, None);
    }
    let name = if (a - 90.0).abs() < 0.01 { format!("{base} {w:.0} x {d:.0}") } else { format!("{base} {w:.0} x {d:.0} - {a:.0}°") };
    if f.types.contains_key(&name) {
        return (name, None);
    }
    let mut values = f.types.get(&base).cloned().unwrap_or_default();
    values.insert(wn, json!(w));
    values.insert(dn, json!(d));
    values.insert(an, json!(a));
    (name, Some(values))
}

/// A bay the solver laid (metres) as a parking space ([`bay_from`]).
fn bay_of(poly: &[[f64; 2]], mouth: [f64; 2], a: f64) -> Option<(P2, f64, bool, f64, f64)> {
    let p: Vec<P2> = poly.iter().map(|q| P2(q[0] * 1000.0, q[1] * 1000.0)).collect();
    bay_from(&p, P2(mouth[0] * 1000.0, mouth[1] * 1000.0), a)
}

/// A bay's outline (four corners, mm) and the middle of its open side as a parking space: its middle, its angle, whether
/// it is mirrored (its back leaning the other way), and its width square to its sides and its reach from its aisle.
pub fn bay_from(p: &[P2], mouth: P2, a: f64) -> Option<(P2, f64, bool, f64, f64)> {
    if p.len() != 4 {
        return None;
    }
    let centre = P2(p.iter().map(|q| q.0).sum::<f64>() / 4.0, p.iter().map(|q| q.1).sum::<f64>() / 4.0);
    // Its open side: the side whose middle is its mouth; its back, the side across from it.
    let mid = |i: usize| (p[i] + p[(i + 1) % 4]) * 0.5;
    let i = (0..4).min_by(|&i, &j| mid(i).dist(mouth).total_cmp(&mid(j).dist(mouth)))?;
    let (f0, f1) = (p[i], p[(i + 1) % 4]);
    let t = (f1 - f0).unit();
    let v = mid((i + 2) % 4) - mid(i);
    let x = if v.dot(t) < -1e-6 { t * -1.0 } else { t };
    let mirrored = x.perp().dot(v) < 0.0;
    let reach = v.dot(x.perp()).abs();
    let width = f0.dist(f1) * a.to_radians().sin();
    Some((centre, geom::degrees(x.1.atan2(x.0).to_degrees()), mirrored, width, reach))
}

/// Car park `park` laid out again: what it changes. None when it isn't a car park.
pub fn run(m: &Model, park: Id) -> Option<Laid> {
    let Some(Element::Site(s)) = m.elements.get(park) else { return None };
    if s.kind != "car_park" {
        return None;
    }
    let me = m.guid(park);
    let level = s.level;
    let level_name = m.level(level).map(|l| l.name.clone()).unwrap_or_default();
    let levels: Vec<LevelIn> = m.levels().iter().map(|(_, l)| LevelIn { name: l.name.clone(), elevation: l.elevation }).collect();
    let r = read(m, park, s);
    let props = &s.props;
    // The generator never makes its own columns: the real ones are kept clear.
    let mut own = props.clone();
    own.insert("column_every".into(), json!(0));
    let mut notes: Vec<String> = Vec::new();
    let input = parking::input_among(&own, &level_name, &levels, &r.obstacles, &r.streets, &r.among);
    let (solved, basement): (Option<Solved>, bool) = match input {
        Err(why) => {
            notes.push(why);
            (None, m.elevation(level) < -1e-3)
        }
        Ok((input, basement)) => {
            if basement && input.feet.is_empty() && input.basement.as_ref().is_none_or(Vec::is_empty) {
                // No ramp reaches it: nothing laid out, and said so.
                notes.push(format!("No ramp reaches {level_name}: add one, or say where (`access`)."));
                (None, true)
            } else {
                let solved = parking::solve::solve(&input);
                (Some(solved), basement)
            }
        }
    };
    let mut result = match &solved {
        Some(l) => parking::result_of(l, basement),
        None => {
            let mut p = Props::new();
            for (k, v) in [
                ("stalls", json!(0)),
                ("accessible", json!(0)),
                ("accessible_needed", json!(0)),
                ("ev", json!(0)),
                ("columns", json!(0)),
                ("arrangement", json!("fields")),
                ("directions", json!([])),
                ("basement", json!(basement)),
                ("area_per_stall", json!(0)),
                ("ways", json!([])),
                ("notes", json!([])),
            ] {
                p.insert(k.into(), v);
            }
            p
        }
    };
    // Its notes first, then the solver's; columns are the real ones in its area.
    let mut all = notes;
    all.extend(result.get("notes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
    all.retain(|n| !n.starts_with("Columns are set out"));
    result.insert("notes".into(), json!(all));
    result.insert("columns".into(), json!(r.columns));
    result.insert("kept".into(), json!(r.kept.len()));
    // Below ground with no column in its area: the grid it would set out, proposed, not placed.
    if solved.is_some()
        && basement
        && r.columns == 0
        && let Ok((mut input, _)) = parking::input_among(props, &level_name, &levels, &r.obstacles, &r.streets, &r.among)
    {
        if input.settings.column_every.is_none_or(|c| c <= 0.0) {
            input.settings.column_every = Some(3.0);
        }
        let grid = parking::solve::solve(&input);
        let centres: Vec<Value> = grid
            .columns
            .iter()
            .map(|c| {
                let n = c.len().max(1) as f64;
                json!([round2(c.iter().map(|q| q[0]).sum::<f64>() / n * 1000.0), round2(c.iter().map(|q| q[1]).sum::<f64>() / n * 1000.0)])
            })
            .collect();
        result.insert("proposed_columns".into(), json!(centres));
    }
    // Below ground the aisles are the floor: kept as the car park's annotation (its arrows).
    if let Some(l) = &solved
        && basement
    {
        let aisles: Vec<Value> = l
            .aisles
            .iter()
            .map(|a| {
                let ring: Vec<[f64; 2]> = a.poly.iter().map(|q| [round2(q[0] * 1000.0), round2(q[1] * 1000.0)]).collect();
                match a.flow {
                    Some(f) => json!({"ring": ring, "flow": [round2(f[0]), round2(f[1])]}),
                    None => json!({"ring": ring}),
                }
            })
            .collect();
        result.insert("aisles".into(), json!(aisles));
    }
    // The bays: Parking Spaces of the type each is.
    use crate::family::site as fam;
    let family_name = m.language.word(fam::PARKING_SPACE).to_string();
    let family = m.families.get(&family_name).cloned().or_else(|| crate::family::site_library(m.language).remove(&family_name));
    let mut put: Vec<(Option<Id>, Element)> = Vec::new();
    let mut types: Vec<(String, BTreeMap<String, Value>)> = Vec::new();
    let mut free: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    for (id, placed) in &r.replace {
        // Where and how, but not its number: a bay that comes back where it was keeps its id.
        let at: String = placed.rsplitn(2, ',').nth(1).unwrap_or(placed).to_string();
        free.entry(at).or_default().push(*id);
    }
    let mut reused: BTreeSet<Id> = BTreeSet::new();
    if let (Some(l), Some(f)) = (&solved, &family) {
        let angle = l.settings.angle;
        for (n, st) in l.stalls.iter().enumerate() {
            let Some((at, turn, mirrored, w, d)) = bay_of(&st.poly, st.mouth, angle) else { continue };
            let base = match st.kind {
                parking::StallKind::Accessible => fam::ACCESSIBLE,
                parking::StallKind::Ev => fam::EV,
                parking::StallKind::Standard => fam::STANDARD,
            };
            let (type_name, made) = type_for(f, m.language, base, round2(w).round(), round2(d).round(), angle);
            if let Some(values) = made
                && !types.iter().any(|(t, _)| *t == type_name)
            {
                types.push((type_name.clone(), values));
            }
            let at = P2(at.0.round(), at.1.round());
            let (_, offset) = surface_at(m, level, at);
            let mut params = BTreeMap::new();
            params.insert(m.language.word(fam::MARK).to_string(), json!((n + 1).to_string()));
            params.insert(BY.into(), json!(key(me)));
            let mut c = Component {
                family: family_name.clone(),
                type_name,
                level,
                at,
                angle: (turn * 1000.0).round() / 1000.0,
                offset,
                mirrored,
                params,
            };
            let placed = placed_of(&c);
            c.params.insert(PLACED.into(), json!(placed));
            let at_key = placed.rsplitn(2, ',').nth(1).unwrap_or(&placed).to_string();
            let id = free.get_mut(&at_key).and_then(Vec::pop);
            if let Some(id) = id {
                reused.insert(id);
            }
            put.push((id, Element::Component(c)));
        }
    }
    // At ground level, its drive aisles: sub-divisions of the toposolid under it.
    if let Some(l) = &solved
        && !basement
    {
        let rings: Vec<Vec<[f64; 2]>> =
            l.aisles.iter().map(|a| a.poly.iter().map(|q| [q[0] * 1000.0, q[1] * 1000.0]).collect()).collect();
        let joined = shapes::poly::union(&rings);
        for (outer, holes) in crate::ground::grouped(&joined.iter().map(|r| r.iter().map(|q| P2(q[0].round(), q[1].round())).collect()).collect::<Vec<_>>()) {
            if outer.len() < 3 || geom::area(&outer).abs() < 1e4 {
                continue;
            }
            let Some(host) = crate::ground::toposolid_under(m, level, geom::centroid(&outer)) else { continue };
            let mut sd = crate::ground::SubDivision {
                type_name: DRIVE_AISLE.into(),
                host,
                boundary: outer,
                openings: holes,
                offset: 0.0,
                name: String::new(),
                role: AISLE_ROLE.into(),
                sketch: Vec::new(),
                road: None,
                params: BTreeMap::new(),
            };
            sd.params.insert(BY.into(), json!(key(me)));
            let placed = placed_of_aisle(&sd);
            sd.params.insert(PLACED.into(), json!(placed));
            let id = r.replace.iter().find(|(id, p)| *p == placed && !reused.contains(id)).map(|(id, _)| *id);
            if let Some(id) = id {
                reused.insert(id);
            }
            put.push((id, Element::SubDivision(sd)));
        }
    }
    let remove: Vec<Id> = r.replace.iter().map(|(id, _)| *id).filter(|id| !reused.contains(id)).collect();
    let pin: Vec<Guid> = r.kept.iter().filter(|(_, pinned)| !pinned).map(|(id, _)| m.guid(*id)).collect();
    result.insert("generated".into(), json!(put.iter().filter(|(_, e)| matches!(e, Element::Component(_))).count()));
    Some(Laid { park, result: Value::Object(result), removed: s.props.get("removed").cloned(), remove, put, pin, types })
}

// ====================================================================== keeping it
/// A box round what an element stands on in plan, where one is quick to say.
fn plan_box(m: &Model, e: &Element) -> Option<[f64; 4]> {
    let pts: Vec<P2> = match e {
        Element::Wall(w) => vec![w.start, w.end],
        Element::Component(c) => vec![c.at],
        Element::Column(c) | Element::StructuralColumn(c) => vec![c.at],
        Element::Floor(f) | Element::Toposolid(f) => f.boundary.clone(),
        Element::Shaft(sh) => sh.boundary.clone(),
        Element::Mass(ms) => ms.boundary.clone(),
        Element::SubDivision(sd) => sd.boundary.clone(),
        Element::Site(s) => s.outline.iter().flat_map(|p| p.outer.clone()).collect(),
        Element::Stair(_) | Element::Ramp(_) => crate::stairs::layout(m, e).map(|l| l.runs.iter().flat_map(|r| r.ring(l.width)).collect())?,
        _ => return None,
    };
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for q in pts {
        b = [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)];
    }
    b[0].is_finite().then_some(b)
}

/// Whether a change to `e` may change what a car park on `level` within `reach` reads.
fn touches(m: &Model, e: &Element, level: Guid, reach: [f64; 4]) -> bool {
    let what = matches!(
        e,
        Element::Wall(_)
            | Element::Column(_)
            | Element::StructuralColumn(_)
            | Element::Shaft(_)
            | Element::Stair(_)
            | Element::Ramp(_)
            | Element::Floor(_)
            | Element::Component(_)
            | Element::Mass(_)
            | Element::Site(_)
            | Element::SubDivision(_)
            | Element::Toposolid(_)
            | Element::PropertyLine(_)
    );
    if !what {
        return false;
    }
    let (below, z, above) = storey(m, level);
    let on = match e {
        Element::SubDivision(sd) => m.subdivision_level(sd),
        _ => m.level_of(e),
    };
    // A stair, a ramp, a shaft or a column may reach the level from another: their heights say.
    let reaches = match e {
        Element::Stair(st) => {
            let (a, b) = m.span(st.level, 0.0, st.top_level, st.height);
            a < above && b > below
        }
        Element::Ramp(r) => {
            let (a, b) = m.span(r.level, 0.0, r.top_level, r.height);
            a < above && b > below
        }
        Element::Shaft(sh) => {
            let (a, b) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
            a < above && b > below
        }
        Element::Column(c) | Element::StructuralColumn(c) => {
            let (a, b) = m.span(c.level, c.base_offset, c.top_level, c.height);
            a < z + HEADROOM && b > z
        }
        Element::Wall(w) => m.base_z(w) < z + HEADROOM && m.top_z(w) > z,
        Element::PropertyLine(_) => true,
        _ => on == Some(level),
    };
    reaches && plan_box(m, e).is_none_or(|b| b[0] <= reach[2] && reach[0] <= b[2] && b[1] <= reach[3] && reach[1] <= b[3])
}

/// After a command: the car parks it changed, or near which it changed something on their level, laid out again; a bay of
/// one the person deleted kept out of it; a car park deleted takes its own unpinned bays and aisles with it.
pub fn keep(m: &Model, before: &Model) -> Vec<Update> {
    let all = m.site_figures != before.site_figures;
    let changed: Vec<Id> = match m.elements.changed_since(&before.elements) {
        Changed::Ids(ids) => ids.into_iter().collect(),
        Changed::All => m.elements.iter().map(|(id, _)| *id).chain(before.elements.iter().map(|(id, _)| *id)).collect(),
    };
    if changed.is_empty() && !all {
        return Vec::new();
    }
    let parks: Vec<(Id, Guid)> = m
        .elements
        .with_guids()
        .filter(|(_, _, e)| matches!(e, Element::Site(s) if s.kind == "car_park"))
        .map(|(id, g, _)| (id, g))
        .collect();
    let mut out = Vec::new();
    // Car parks gone: their own go with them; what the person kept stays, no longer linked.
    let gone: BTreeSet<String> = changed
        .iter()
        .filter(|id| m.elements.get(**id).is_none())
        .filter_map(|id| match before.elements.get(*id) {
            Some(Element::Site(s)) if s.kind == "car_park" => before.elements.guid(*id).map(key),
            _ => None,
        })
        .collect();
    if !gone.is_empty() {
        let (mut remove, mut unlink) = (Vec::new(), Vec::new());
        for (id, g, e) in m.elements.with_guids() {
            let params = match e {
                Element::Component(c) => &c.params,
                Element::SubDivision(sd) => &sd.params,
                _ => continue,
            };
            if by_of(params).is_some_and(|b| gone.contains(b)) {
                let as_placed = match e {
                    Element::Component(c) => params.get(PLACED).and_then(Value::as_str) == Some(placed_of(c).as_str()),
                    Element::SubDivision(sd) => params.get(PLACED).and_then(Value::as_str) == Some(placed_of_aisle(sd).as_str()),
                    _ => false,
                };
                if as_placed && !m.pinned.contains(&g) { remove.push(id) } else { unlink.push(id) }
            }
        }
        out.push(Update::Gone { remove, unlink });
    }
    if parks.is_empty() {
        return out;
    }
    // Bays of a car park the person deleted: kept out of it.
    let mut removed: BTreeMap<String, Vec<P2>> = BTreeMap::new();
    for id in &changed {
        if m.elements.get(*id).is_some() {
            continue;
        }
        if let Some(Element::Component(c)) = before.elements.get(*id)
            && let Some(by) = by_of(&c.params)
            && c.params.get(PLACED).is_some()
        {
            removed.entry(by.to_string()).or_default().push(c.at);
        }
    }
    let levels_changed =
        changed.iter().any(|id| [m, before].iter().any(|x| matches!(x.elements.get(*id), Some(Element::Level(_)))));
    for (park, g) in parks {
        let Some(Element::Site(s)) = m.elements.get(park) else { continue };
        let mine = key(g);
        let gone_bays = removed.get(&mine);
        let run_it = all
            || levels_changed
            || gone_bays.is_some()
            || changed.contains(&park)
            || {
                let area = area_of(s);
                let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
                for q in &area {
                    b = [b[0].min(q.0), b[1].min(q.1), b[2].max(q.0), b[3].max(q.1)];
                }
                let reach = [b[0] - NEAR, b[1] - NEAR, b[2] + NEAR, b[3] + NEAR];
                changed.iter().any(|id| {
                    [m.elements.get(*id), before.elements.get(*id)].into_iter().flatten().any(|e| touches(m, e, s.level, reach))
                })
            };
        if !run_it {
            continue;
        }
        // Its removed bays first, so it lays out without them.
        let laid = match gone_bays {
            Some(pts) => {
                let mut with = m.clone();
                if let Some(Element::Site(s)) = with.elements.get_mut(park) {
                    add_removed(&mut s.props, pts);
                }
                run(&with, park)
            }
            None => run(m, park),
        };
        if let Some(laid) = laid {
            out.push(Update::Laid(Box::new(laid)));
        }
    }
    out
}

/// `pts` added to a car park's `removed` (the bays the person took out).
fn add_removed(props: &mut Props, pts: &[P2]) {
    let mut list = props.get("removed").and_then(Value::as_array).cloned().unwrap_or_default();
    list.extend(pts.iter().map(|q| json!([q.0.round(), q.1.round()])));
    props.insert("removed".into(), Value::Array(list));
}

/// What [`keep`] and [`run`] worked out, made so.
pub fn apply(m: &mut Model, updates: Vec<Update>) {
    for u in updates {
        match u {
            Update::Gone { remove, unlink } => {
                for id in remove {
                    m.elements.remove(id);
                }
                for id in unlink {
                    let params = match m.elements.get_mut(id) {
                        Some(Element::Component(c)) => &mut c.params,
                        Some(Element::SubDivision(sd)) => &mut sd.params,
                        _ => continue,
                    };
                    params.remove(BY);
                    params.remove(PLACED);
                }
            }
            Update::Laid(l) => {
                let Laid { park, result, removed, remove, put, pin, types } = *l;
                if put.iter().any(|(_, e)| matches!(e, Element::Component(_))) {
                    let family = m.language.word(crate::family::site::PARKING_SPACE).to_string();
                    crate::ground::load_family(m, &family);
                    if let Some(f) = m.families.get_mut(&family) {
                        for (name, values) in types {
                            f.types.entry(name).or_insert(values);
                        }
                    }
                }
                if put.iter().any(|(_, e)| matches!(e, Element::SubDivision(_))) {
                    crate::ground::ensure_type(m, DRIVE_AISLE);
                }
                for id in remove {
                    m.elements.remove(id);
                }
                for (id, e) in put {
                    match id {
                        Some(id) => {
                            m.elements.insert(id, e);
                        }
                        None => {
                            m.insert(e);
                        }
                    }
                }
                m.pinned.extend(pin);
                if let Some(Element::Site(s)) = m.elements.get_mut(park) {
                    s.props.insert("result".into(), result);
                    if let Some(r) = removed {
                        s.props.insert("removed".into(), r);
                    }
                }
            }
        }
    }
}

/// Car park `park` laid out again now (`car_park_run`, and a car park just made). Its result.
pub fn run_now(m: &mut Model, park: Id) -> Option<Value> {
    let laid = run(m, park)?;
    apply(m, vec![Update::Laid(Box::new(laid))]);
    match m.elements.get(park) {
        Some(Element::Site(s)) => s.props.get("result").cloned(),
        _ => None,
    }
}

/// Every car park laid out again (a file from before, read).
pub fn run_all(m: &mut Model) {
    let parks: Vec<Id> = m.elements.iter().filter(|(_, e)| matches!(e, Element::Site(s) if s.kind == "car_park")).map(|(id, _)| *id).collect();
    for park in parks {
        run_now(m, park);
    }
}

// ====================================================================== a file from before
/// The masterplan kinds `site` no longer takes since ramps and parking are Revit's, and what makes each now.
pub const CONVERTED: &[(&str, &str)] = &[
    ("ramp", "ramp (its `path`)"),
    ("parking_stall", "parking_component"),
    ("parking_band", "car_park, or parking_component"),
];

/// The masterplan's ramps, parking stalls and parking bands of a file from before (format 26 or older) made what Revit
/// has (docs/parking-ramps-plan.md): each ramp an `Element::Ramp` along its path from its foot, each stall and each of a
/// band's bays a Parking Space, a band's aisle a drive aisle on the ground; then every car park laid out again, its bays
/// the person took out kept out. Whether the project's families changed (a family loaded, a type made).
pub fn convert(m: &mut Model) -> bool {
    let olds: Vec<(Id, Guid, crate::masterplan::Site)> = m
        .elements
        .with_guids()
        .filter_map(|(id, g, e)| match e {
            Element::Site(s) if matches!(s.kind.as_str(), "ramp" | "parking_stall" | "parking_band") => Some((id, g, (**s).clone())),
            _ => None,
        })
        .collect();
    let families = m.families.clone();
    for (id, guid, s) in olds {
        let made: Vec<Element> = match s.kind.as_str() {
            "ramp" => ramp_of(m, &s).into_iter().collect(),
            "parking_stall" => stall_of(m, &s).into_iter().collect(),
            _ => band_of(m, &s),
        };
        m.elements.remove(id);
        for (k, e) in made.into_iter().enumerate() {
            // The first takes the old element's id and GUID, so what pointed at it points at it still.
            if k == 0 {
                let _ = m.elements.insert_as(id, guid, e);
            } else {
                m.insert(e);
            }
        }
    }
    run_all(m);
    m.families != families
}

fn path_of(s: &crate::masterplan::Site) -> Vec<[f64; 3]> {
    s.props
        .get("path")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let q = q.as_array()?;
            Some([q.first()?.as_f64()?, q.get(1)?.as_f64()?, q.get(2).and_then(Value::as_f64).unwrap_or(0.0)])
        })
        .filter(|q| q.iter().all(|v| v.is_finite()))
        .collect()
}

/// A masterplan ramp (its path from its open end on the ground down to `to_level`) as a Ramp: its path reversed, from its
/// foot on `to_level` (by name; else the level below its own) up to its own level.
fn ramp_of(m: &Model, s: &crate::masterplan::Site) -> Option<Element> {
    let down = path_of(s);
    if down.len() < 2 {
        return None;
    }
    // Reversed: each piece's bulge on the point it now starts from, turning the other way.
    let n = down.len();
    let path: Vec<[f64; 3]> =
        (0..n).map(|i| {
            let q = down[n - 1 - i];
            let bulge = if i + 1 < n { -down[n - 2 - i][2] + 0.0 } else { 0.0 };
            [q[0], q[1], bulge]
        }).collect();
    let figures = crate::masterplan::figures(m);
    let num = |k: &str| s.props.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0);
    let width = num("width").unwrap_or_else(|| shapes::figure(&figures, "ramp.width"));
    let gradient = num("gradient").unwrap_or_else(|| shapes::figure(&figures, "ramp.gradient")).clamp(1.0, 50.0);
    let top = m.elevation(s.level);
    let named = s.props.get("to_level").and_then(Value::as_str).and_then(|name| {
        m.levels().into_iter().find(|(_, l)| l.name == name && l.elevation < top - 1.0).map(|(id, _)| m.guid(id))
    });
    let below = named.or_else(|| m.levels().into_iter().rev().find(|(_, l)| l.elevation < top - 1.0).map(|(id, _)| m.guid(id)));
    let length: f64 = crate::stairs::path_pieces(&path)
        .iter()
        .scan(P2(path[0][0], path[0][1]), |at, pts| {
            let mut len = 0.0;
            for q in pts {
                len += at.dist(*q);
                *at = *q;
            }
            Some(len)
        })
        .sum();
    let (level, top_level, height) = match below {
        Some(b) => (b, Some(s.level), 0.0),
        // No level under it: it climbs from its own level as far as its gradient takes it along its length.
        None => (s.level, None, round2((length * gradient / 100.0).max(100.0))),
    };
    let first = crate::stairs::path_pieces(&path).first().and_then(|q| q.first().copied()).unwrap_or(P2(path[1][0], path[1][1]));
    let start = P2(path[0][0], path[0][1]);
    Some(Element::Ramp(crate::model::Ramp {
        level,
        top_level,
        height,
        start,
        angle: geom::degrees((first.1 - start.1).atan2(first.0 - start.0).to_degrees()),
        width,
        slope: gradient,
        landings: Vec::new(),
        path,
        slopes: Vec::new(),
        params: s.params.clone(),
    }))
}

/// The engine's Parking Space put in the project (as Revit loads a family), and the type of `base` (Standard,
/// Accessible, EV) `w` by `d` at `a` degrees made in it where it hasn't one: its name and type.
fn bay_type(m: &mut Model, base: &str, w: f64, d: f64, a: f64) -> Option<(String, String)> {
    let family = m.language.word(crate::family::site::PARKING_SPACE).to_string();
    if !crate::ground::load_family(m, &family) {
        return None;
    }
    let language = m.language;
    let f = m.families.get_mut(&family)?;
    let (name, made) = type_for(f, language, base, w.round(), d.round(), a);
    if let Some(values) = made {
        f.types.insert(name.clone(), values);
    }
    Some((family, name))
}

fn bay_element(m: &mut Model, s: &crate::masterplan::Site, base: &str, at: P2, angle: f64, mirrored: bool, size: (f64, f64, f64), mark: Option<String>) -> Option<Element> {
    let (family, type_name) = bay_type(m, base, size.0, size.1, size.2)?;
    let (_, offset) = surface_at(m, s.level, at);
    let mut params = s.params.clone();
    if let Some(mark) = mark {
        params.insert(m.language.word(crate::family::site::MARK).to_string(), json!(mark));
    }
    Some(Element::Component(Component {
        family,
        type_name,
        level: s.level,
        at: P2(round2(at.0), round2(at.1)),
        angle: (angle * 1000.0).round() / 1000.0,
        offset,
        mirrored,
        params,
    }))
}

fn base_of(use_: &str) -> &'static str {
    use crate::family::site as fam;
    match use_ {
        "accessible" => fam::ACCESSIBLE,
        "ev" => fam::EV,
        _ => fam::STANDARD,
    }
}

/// A masterplan parking stall (its middle `at`, a car driving in along `angle`) as a Parking Space.
fn stall_of(m: &mut Model, s: &crate::masterplan::Site) -> Option<Element> {
    let at = s.props.get("at").and_then(Value::as_array).and_then(|q| Some(P2(q.first()?.as_f64()?, q.get(1)?.as_f64()?)))?;
    let num = |k: &str, d: f64| s.props.get(k).and_then(Value::as_f64).filter(|v| v.is_finite() && *v > 0.0).unwrap_or(d);
    let (w, d) = (num("width", 2500.0), num("depth", 5000.0));
    let drive = s.props.get("angle").and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(90.0);
    // Its y is the way a car drives in: its x a quarter turn clockwise of that.
    let use_ = s.use_or_default().to_string();
    bay_element(m, s, base_of(&use_), at, geom::degrees(drive - 90.0), false, (w, d, 90.0), None)
}

/// A masterplan parking band's bays as Parking Spaces (their mouths on its aisle) and its aisle as a drive aisle on the
/// ground.
fn band_of(m: &mut Model, s: &crate::masterplan::Site) -> Vec<Element> {
    let polys = |role: &str| -> Vec<Poly> { s.parts.iter().filter(|p| p.role == role).flat_map(|p| p.polys.clone()).collect() };
    let aisles = polys("aisle");
    let angle = s.props.get("angle").and_then(Value::as_f64).filter(|a| (1.0..=90.0).contains(a)).unwrap_or(90.0);
    let near_aisle = |q: P2| -> f64 {
        aisles
            .iter()
            .flat_map(|p| {
                let n = p.outer.len();
                (0..n).map(move |i| (p.outer[i], p.outer[(i + 1) % n]))
            })
            .map(|(a, b)| {
                let d = b - a;
                let t = ((q - a).dot(d) / d.dot(d).max(1e-9)).clamp(0.0, 1.0);
                (a + d * t).dist(q)
            })
            .fold(f64::INFINITY, f64::min)
    };
    let mut out = Vec::new();
    let mut number = 0;
    for (role, base) in [("stall", crate::family::site::STANDARD), ("accessible_stall", crate::family::site::ACCESSIBLE)] {
        for p in polys(role) {
            let ring: Vec<P2> = p.outer.clone();
            if ring.len() != 4 {
                continue;
            }
            let mouth = (0..4).map(|i| (ring[i] + ring[(i + 1) % 4]) * 0.5).min_by(|a, b| near_aisle(*a).total_cmp(&near_aisle(*b)));
            let Some((at, turn, mirrored, w, d)) = mouth.and_then(|mo| bay_from(&ring, mo, angle)) else { continue };
            number += 1;
            if let Some(e) = bay_element(m, s, base, at, turn, mirrored, (round2(w), round2(d), angle), Some(number.to_string())) {
                out.push(e);
            }
        }
    }
    // Its aisle: a drive aisle on the toposolid under it.
    for p in aisles {
        if p.outer.len() < 3 {
            continue;
        }
        let Some(host) = crate::ground::toposolid_under(m, s.level, geom::centroid(&p.outer)) else { continue };
        crate::ground::ensure_type(m, DRIVE_AISLE);
        let mut boundary = p.outer.clone();
        if geom::area(&boundary) < 0.0 {
            boundary.reverse();
        }
        out.push(Element::SubDivision(crate::ground::SubDivision {
            type_name: DRIVE_AISLE.into(),
            host,
            boundary,
            openings: p.holes.clone(),
            offset: 0.0,
            name: s.name.clone(),
            role: AISLE_ROLE.into(),
            sketch: Vec::new(),
            road: None,
            params: BTreeMap::new(),
        }));
    }
    out
}

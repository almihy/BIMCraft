//! A car park laid out over an area: its aisles, bays (stalls), accessible bays, columns and ramps, level by level. The
//! port of Arqiflow's web/lib/parking (carpark.ts, solver.ts, geom.ts, standards.ts), which lays out a car park the
//! person drags and the one the agent makes the same way.
//!
//! [`lay_out`] is the element's way in (millimetres, the car park's props); it reads what Arqiflow's `carParkInput`
//! reads, runs [`solve::solve`] (the solver, in metres as Arqiflow's, step for step, so the layouts match its fixtures
//! within 5 mm) and writes what `carParkChildren` and `carParkResult` write. The solver's own modules: [`geom`] (plane
//! geometry), [`standards`] (the sizes), `fields` (a field of modules, a plot cut in fields), `ring` (rows round every
//! side and a middle), [`solve`] (the ways in, the pick, the accessible bays).

// `!(a < b)` is the TypeScript's own test, and it is true for NaN where `a >= b` is not: the port keeps it.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

pub mod geom;
pub mod solve;
pub mod standards;

mod fields;
mod grid;
mod ring;

pub use fields::StallKind;

#[cfg(test)]
mod tests;

use serde_json::{Value, json};

use super::{Poly, Props};
use crate::geom::P2;
use geom::{Pt, Ring, js_round};
use solve::{Access, AccessKind, Input};
use standards::Given;

/// A level the car park may stand on: its name and elevation, mm.
#[derive(Clone, Debug)]
pub struct LevelIn {
    pub name: String,
    pub elevation: f64,
}

/// Something already on the site the layout keeps clear of (a building, a core, a ramp of another car park): its
/// outline and the level it stands on.
///
/// Its `kind` says what it is to the layout: a `ramp` is a way down (below ground the ramps on the level are the ways
/// in, and above ground the bays keep off them); a `lane` or `driveway` is a road the car park may be entered from
/// along a side it runs beside (`lane/footpath` and `lane/promenade`, a lane's use after a slash, are for people only:
/// kept clear but no way in); `landscape`, `paving`, `tree`, `hedge`, `parking_band`, `car_park`, `aisle`, `stall`
/// and `column` are ground a car park may be laid over (Arqiflow's masterplan kinds that are not obstacles); anything
/// else (a `mass`, a building, `water`, a `pitch`…) is kept clear, with a footway round it.
#[derive(Clone, Debug)]
pub struct Obstacle {
    pub kind: String,
    pub level: String,
    pub outline: Vec<Poly>,
}

/// One piece of a laid-out car park: an "aisle", "stall", "column", "ramp", … (carpark.ts `carParkChildren`), on its
/// level, with its own props (a stall's `accessible`, its angle…) and outline.
///
/// As Arqiflow writes them: an `aisle` with `part` "aisle", "cross", "drive" or "throat" (a way in from the street or
/// a ramp's foot) and a one-way aisle's `flow` (a unit vector); a `stall` with its `index`, its `mouth` (the middle of
/// its open side, mm) and `accessible: true` or `ev: true`; an `aisle` with `part` "access" (an accessible bay's
/// access aisle); and a `column`. Outlines counter-clockwise, to the millimetre.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    pub kind: String,
    pub level: String,
    #[serde(default)]
    pub props: Props,
    #[serde(default)]
    pub outline: Vec<Poly>,
}

/// The layout: its pieces and its figures (carpark.ts `carParkResult`: stalls, accessible stalls, levels, area per
/// stall…).
///
/// `result`: `stalls`, `accessible`, `accessible_needed`, `ev`, `columns` (counts), `arrangement` ("fields" or
/// "ring"), `directions` (degrees), `basement`, `area_per_stall` (m² of the plot a bay, as Arqiflow's figure),
/// `ways` (each `{at: [x, y] mm, kind: "in" | "out" | "both"}`, above ground) and `notes` (what the person should be
/// told).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    #[serde(default)]
    pub pieces: Vec<Piece>,
    #[serde(default)]
    pub result: Props,
}

/// The most points an area, an outline or a list of points may have.
pub const MAX_POINTS: usize = 1000;
/// The most obstacles read.
pub const MAX_OBSTACLES: usize = 2000;
/// The widest a car park may be, mm: past this it is not laid out (it would take too long), and the person is told.
pub const MAX_EXTENT: f64 = 1_500_000.0;
/// The farthest a point may lie from the origin, mm.
pub const MAX_COORD: f64 = 1e9;

/// The kinds a car park may be laid over: Arqiflow's masterplan kinds that are not obstacles.
const OVER: [&str; 9] = ["landscape", "paving", "tree", "hedge", "parking_band", "car_park", "aisle", "stall", "column"];
/// The kinds a car comes from on the ground.
const ROADS: [&str; 2] = ["lane", "driveway"];
/// A lane for people only is no way in for cars.
const FOOT: [&str; 2] = ["footpath", "promenade"];

/// What a car park is laid out among beyond what its props say (the generator's, `carpark.rs`): what stands on its level
/// kept clear exactly (walls, columns, shafts, stairs and ramps, openings, the bays the person keeps: no footway round
/// them, as a car parks beside a column), the ways off the real ramps onto its level below ground, and the firm's sizes
/// where its props say none. Lengths in mm.
#[derive(Clone, Debug, Default)]
pub struct Among {
    pub keep: Vec<Vec<P2>>,
    /// Each a ramp's end on the level: the line across it, and the way a car comes off it (unit).
    pub feet: Vec<((P2, P2), P2)>,
    /// The firm's sizes, metres: its `aisle` a two-way aisle's, `aisle_one_way` a one-way aisle's.
    pub defaults: Given,
    pub aisle_one_way: Option<f64>,
    /// The cores on the level (lifts, stairs: the middle of each), where people walk to from their car: the accessible
    /// bays go nearest them when the car park names no `target`.
    pub cores: Vec<P2>,
}

/// The car park whose area and choices are `props` (its `control` ring, mm, and the person's choices), laid out on
/// `level` among `levels`, keeping clear of `obstacles`, its way in towards the nearest of `streets` (street edges of the
/// plot, mm).
///
/// The props, as Arqiflow's car park holds them with lengths in mm: `control` ([[x, y], …]), `standard` ("saudi",
/// "us", "uk"), `angle` (degrees: 90, or 75, 60, 45, 30), `one_way`, `stall_width`, `stall_depth`, `aisle`,
/// `setback`, `end_island` (mm), `island_every` (bays), `ev_share` (0 to 1), `column_every` (bays), `column_size`,
/// `floor_to_floor` (mm), `ramp_slope` (a fraction, 0.15), `direction` (degrees, or null for the best), `access`
/// ([{at: [x, y], kind: "in" | "out" | "both"}]), `removed` ([[x, y], …]: a point in each bay taken out) and `target`
/// ([x, y]: where people walk to). A size left out is the standard's.
pub fn lay_out(props: &Props, level: &str, levels: &[LevelIn], obstacles: &[Obstacle], streets: &[(P2, P2)]) -> Layout {
    let (input, basement) = match input_of(props, level, levels, obstacles, streets) {
        Ok(x) => x,
        Err(why) => return Layout { pieces: Vec::new(), result: empty_result(why, false) },
    };
    let solved = solve::solve(&input);
    Layout { pieces: pieces_of(&solved, level), result: result_of(&solved, basement) }
}

/// [`input_of`], and `among` too: what it keeps clear exactly, the ramps' ends below ground, the firm's sizes.
pub fn input_among(
    props: &Props,
    level: &str,
    levels: &[LevelIn],
    obstacles: &[Obstacle],
    streets: &[(P2, P2)],
    among: &Among,
) -> Result<(Input, bool), String> {
    let (mut input, basement) = input_of(props, level, levels, obstacles, streets)?;
    let b = geom::bounds(&input.site);
    let ring = |r: &[P2]| -> Option<Ring> {
        let mut out: Ring = r.iter().filter(|p| finite_pt(p.0, p.1)).map(|p| [r3(p.0 / 1000.0), r3(p.1 / 1000.0)]).collect();
        if out.len() > 1 && out.first() == out.last() {
            out.pop();
        }
        let q = geom::bounds(&out);
        (out.len() >= 3 && out.len() <= MAX_POINTS && q.x0 <= b.x1 + 1.0 && b.x0 <= q.x1 + 1.0 && q.y0 <= b.y1 + 1.0 && b.y0 <= q.y1 + 1.0)
            .then_some(out)
    };
    input.keep_clear.extend(among.keep.iter().take(MAX_OBSTACLES).filter_map(|r| ring(r)));
    input.cores = among.cores.iter().filter(|p| finite_pt(p.0, p.1)).take(MAX_OBSTACLES).map(|p| [r3(p.0 / 1000.0), r3(p.1 / 1000.0)]).collect();
    if basement {
        let m = |p: P2| [r3(p.0 / 1000.0), r3(p.1 / 1000.0)];
        input.feet = among
            .feet
            .iter()
            .filter(|((a, c), o)| finite_pt(a.0, a.1) && finite_pt(c.0, c.1) && o.0.is_finite() && o.1.is_finite())
            .map(|((a, c), o)| ((m(*a), m(*c)), [o.0, o.1]))
            .collect();
    }
    // The person's sizes first, the firm's where they say none.
    let d = &among.defaults;
    let g = &mut input.settings;
    g.angle = g.angle.or(d.angle);
    g.one_way = g.one_way.or(d.one_way);
    g.stall_width = g.stall_width.or(d.stall_width);
    g.stall_depth = g.stall_depth.or(d.stall_depth);
    g.accessible_width = g.accessible_width.or(d.accessible_width);
    // A one-way aisle between angled bays is the firm's one-way width; a two-way, its aisle's. 90° bays need the two-way
    // width to turn into, one way or not.
    let angle = g.angle.unwrap_or(90.0);
    let one_way = g.one_way.unwrap_or(angle < 90.0);
    g.aisle = g.aisle.or(if one_way && angle < 89.9 { among.aisle_one_way } else { d.aisle });
    g.aisle_two_way = g.aisle_two_way.or(d.aisle);
    g.ev_share = g.ev_share.or(d.ev_share);
    g.ramp_slope = g.ramp_slope.or(d.ramp_slope);
    g.floor_to_floor = g.floor_to_floor.or(d.floor_to_floor);
    Ok((input, basement))
}

/// What the car park is laid out from (Arqiflow's `carParkInput`), in metres: the solver's input, and whether it is
/// below ground. An error says why it cannot be.
pub fn input_of(
    props: &Props,
    level: &str,
    levels: &[LevelIn],
    obstacles: &[Obstacle],
    streets: &[(P2, P2)],
) -> Result<(Input, bool), String> {
    let r3 = |v: f64| js_round(v) / 1000.0; // mm to metres, to the millimetre
    let control = points(props.get("control")).ok_or("The car park has no area: give its control ring, three points or more.")?;
    let mut area: Ring = control.iter().map(|p| [r3(p[0]), r3(p[1])]).collect();
    if area.len() > 1 && area.first() == area.last() {
        area.pop();
    }
    if area.len() < 3 {
        return Err("The car park has no area: give its control ring, three points or more.".into());
    }
    if geom::ring_area(&area) < 0.0 {
        area.reverse();
    }
    let site = area;
    let b = geom::bounds(&site);
    if (b.x1 - b.x0) * 1000.0 > MAX_EXTENT || (b.y1 - b.y0) * 1000.0 > MAX_EXTENT {
        return Err(format!("The car park is wider than {} m: lay it out in parts.", MAX_EXTENT / 1000.0));
    }
    let basement = levels.iter().find(|l| l.name == level).map(|l| l.elevation).filter(|e| e.is_finite()).unwrap_or(0.0) < -1e-3;
    let meets = |r: &Ring| {
        let q = geom::bounds(r);
        q.x0 <= b.x1 + 1.0 && b.x0 <= q.x1 + 1.0 && q.y0 <= b.y1 + 1.0 && b.y0 <= q.y1 + 1.0
    };
    let mut obst: Vec<Ring> = Vec::new();
    let mut ramps: Vec<Ring> = Vec::new();
    let mut roads: Vec<Ring> = Vec::new();
    for o in obstacles.iter().filter(|o| o.level == level).take(MAX_OBSTACLES) {
        let (kind, use_) = o.kind.split_once('/').unwrap_or((o.kind.as_str(), ""));
        if OVER.contains(&kind) {
            continue;
        }
        for poly in &o.outline {
            if poly.outer.len() > MAX_POINTS || poly.outer.iter().any(|p| !finite_pt(p.0, p.1)) {
                continue;
            }
            let mut r: Ring = poly.outer.iter().map(|p| [p.0 / 1000.0, p.1 / 1000.0]).collect();
            if r.len() > 1 && r.first() == r.last() {
                r.pop();
            }
            if r.len() < 3 || !meets(&r) {
                continue;
            }
            let rounded: Ring = r.iter().map(|p| [r3(p[0] * 1000.0), r3(p[1] * 1000.0)]).collect();
            if kind == "ramp" {
                ramps.push(rounded);
            } else {
                if !basement && ROADS.contains(&kind) && !FOOT.contains(&use_) {
                    roads.push(r);
                }
                obst.push(rounded);
            }
        }
    }
    let access_raw: Vec<(Pt, Option<AccessKind>)> = match props.get("access") {
        Some(Value::Array(list)) => list
            .iter()
            .take(64)
            .filter_map(|x| {
                let at = point(x.get("at"))?;
                Some(([at[0] / 1000.0, at[1] / 1000.0], x.get("kind").and_then(Value::as_str).and_then(AccessKind::parse)))
            })
            .collect(),
        _ => Vec::new(),
    };
    let street_lines: Vec<(Pt, Pt)> = streets
        .iter()
        .take(MAX_POINTS)
        .filter(|(a, c)| finite_pt(a.0, a.1) && finite_pt(c.0, c.1))
        .map(|(a, c)| ([a.0 / 1000.0, a.1 / 1000.0], [c.0 / 1000.0, c.1 / 1000.0]))
        .collect();
    // A side is a street side when it runs along a lane or the plot's street edge (within a metre and a half).
    let mut street_sides: Vec<usize> = Vec::new();
    if !basement {
        let n = site.len();
        for i in 0..n {
            let a = site[i];
            let c = site[(i + 1) % n];
            let on_road = |m: Pt| {
                roads.iter().any(|r| geom::point_in_ring(m, r) || (0..r.len()).any(|k| seg_dist(m, r[k], r[(k + 1) % r.len()]) < 1.5))
                    || street_lines.iter().any(|&(s0, s1)| seg_dist(m, s0, s1) < 1.5)
            };
            let placed = access_raw.iter().any(|(at, _)| seg_dist(*at, a, c) < 1.5);
            let mids = [0.25, 0.5, 0.75].iter().filter(|&&t| on_road([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t])).count();
            if placed || mids >= 2 {
                street_sides.push(i);
            }
        }
    }
    let access: Vec<Access> = if basement {
        Vec::new()
    } else {
        access_raw.iter().filter_map(|&(at, k)| Some(Access { at: [r3(at[0] * 1000.0), r3(at[1] * 1000.0)], kind: k? })).collect()
    };
    let removed: Vec<Pt> = match props.get("removed") {
        Some(Value::Array(list)) => list.iter().take(100_000).filter_map(|x| point(Some(x))).map(|p| [r3(p[0]), r3(p[1])]).collect(),
        _ => Vec::new(),
    };
    let target = point(props.get("target")).map(|p| [r3(p[0]), r3(p[1])]);
    let direction = num(props, "direction").filter(|d| d.abs() < 1e6);
    let input = Input {
        site,
        obstacles: obst,
        streets: street_sides,
        access,
        entries: Vec::new(),
        keep_clear: if basement { Vec::new() } else { ramps.clone() },
        removed,
        target,
        direction,
        quick: false,
        basement: if basement { Some(ramps) } else { None },
        feet: Vec::new(),
        cores: Vec::new(),
        settings: given_of(props),
    };
    Ok((input, basement))
}

/// The person's choices of the standard, from the car park's props: lengths in mm to metres; each out of a sane range
/// or not a number is left to the standard.
fn given_of(props: &Props) -> Given {
    let mm = |k: &str, lo: f64, hi: f64| num(props, k).map(|v| v / 1000.0).filter(|v| (lo..=hi).contains(v));
    let plain = |k: &str, lo: f64, hi: f64| num(props, k).filter(|v| (lo..=hi).contains(v));
    Given {
        standard: props.get("standard").and_then(Value::as_str).map(|s| s.chars().take(32).collect()),
        angle: plain("angle", 1.0, 180.0),
        one_way: props.get("one_way").and_then(Value::as_bool),
        stall_width: mm("stall_width", 1.5, 6.0),
        stall_depth: mm("stall_depth", 2.5, 12.0),
        aisle: mm("aisle", 2.5, 20.0),
        setback: mm("setback", 0.0, 50.0),
        end_island: mm("end_island", 0.0, 20.0),
        island_every: plain("island_every", 0.0, 500.0),
        ev_share: plain("ev_share", 0.0, 1.0),
        column_every: plain("column_every", 0.0, 50.0),
        column_size: mm("column_size", 0.1, 3.0),
        floor_to_floor: mm("floor_to_floor", 2.0, 12.0),
        ramp_slope: plain("ramp_slope", 0.02, 0.5),
        dead_end: mm("dead_end", 0.0, 500.0),
        ..Given::default()
    }
}

fn finite_pt(x: f64, y: f64) -> bool {
    x.is_finite() && y.is_finite() && x.abs() <= MAX_COORD && y.abs() <= MAX_COORD
}

fn num(props: &Props, k: &str) -> Option<f64> {
    props.get(k).and_then(Value::as_f64).filter(|v| v.is_finite())
}

fn point(v: Option<&Value>) -> Option<Pt> {
    let a = v?.as_array()?;
    let x = a.first()?.as_f64()?;
    let y = a.get(1)?.as_f64()?;
    finite_pt(x, y).then_some([x, y])
}

fn points(v: Option<&Value>) -> Option<Vec<Pt>> {
    let a = v?.as_array()?;
    if a.len() > MAX_POINTS {
        return None;
    }
    a.iter().map(|p| point(Some(p))).collect()
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / geom::or1(dx * dx + dy * dy)).clamp(0.0, 1.0);
    geom::hypot(p[0] - a[0] - dx * u, p[1] - a[1] - dy * u)
}

/// A metre figure to the millimetre, in mm.
fn to_mm(v: f64) -> f64 {
    js_round(v * 1000.0)
}

/// An outline in mm, to the millimetre, counter-clockwise.
fn outline(r: &[Pt]) -> Vec<Poly> {
    let mut c: Vec<P2> = r.iter().map(|p| P2(to_mm(p[0]), to_mm(p[1]))).collect();
    let pts: Ring = c.iter().map(|p| [p.0, p.1]).collect();
    if geom::ring_area(&pts) < 0.0 {
        c.reverse();
    }
    vec![Poly { outer: c, holes: Vec::new() }]
}

fn r3(v: f64) -> f64 {
    js_round(v * 1000.0) / 1000.0
}

/// The pieces a layout writes (Arqiflow's `carParkChildren`): its aisles and ways in, its bays, access aisles and
/// columns.
pub fn pieces_of(l: &solve::Solved, level: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut child = |kind: &str, ring: &[Pt], props: Value| {
        let props = match props {
            Value::Object(m) => m,
            _ => Props::new(),
        };
        out.push(Piece { kind: kind.to_string(), level: level.to_string(), props, outline: outline(ring) });
    };
    for a in &l.aisles {
        let mut p = json!({ "part": a.kind.name() });
        if let (Some(f), Some(m)) = (a.flow, p.as_object_mut()) {
            m.insert("flow".into(), json!([r3(f[0]), r3(f[1])]));
        }
        child("aisle", &a.poly, p);
    }
    for (i, s) in l.stalls.iter().enumerate() {
        let mut p = json!({ "index": i, "mouth": [to_mm(s.mouth[0]), to_mm(s.mouth[1])] });
        if let Some(m) = p.as_object_mut() {
            match s.kind {
                fields::StallKind::Accessible => {
                    m.insert("accessible".into(), Value::Bool(true));
                }
                fields::StallKind::Ev => {
                    m.insert("ev".into(), Value::Bool(true));
                }
                fields::StallKind::Standard => {}
            }
        }
        child("stall", &s.poly, p);
    }
    for r in &l.access_aisles {
        child("aisle", r, json!({ "part": "access" }));
    }
    for r in &l.columns {
        child("column", r, json!({}));
    }
    out
}

/// What a layout came to (Arqiflow's `carParkResult`), and each accessible bay's access aisle (`access_aisles`, rings in
/// mm). Below ground its ways in are the ramps' ways off (`ramp: true`).
pub fn result_of(l: &solve::Solved, basement: bool) -> Props {
    let ways: Vec<Value> = l
        .entries
        .iter()
        .map(|w| {
            let mut v = json!({ "at": [to_mm(w.at[0]), to_mm(w.at[1])], "kind": w.kind.name() });
            if basement && let Some(m) = v.as_object_mut() {
                m.insert("ramp".into(), Value::Bool(true));
            }
            v
        })
        .collect();
    let access: Vec<Value> = l.access_aisles.iter().map(|r| json!(r.iter().map(|p| [to_mm(p[0]), to_mm(p[1])]).collect::<Vec<_>>())).collect();
    let v = json!({
        "stalls": l.metrics.stalls,
        "accessible": l.metrics.accessible,
        "accessible_needed": l.metrics.accessible_needed,
        "ev": l.metrics.ev,
        "columns": l.columns.len(),
        "arrangement": if l.ring { "ring" } else { "fields" },
        "directions": l.directions,
        "basement": basement,
        "area_per_stall": l.metrics.area_per_stall,
        "ways": ways,
        "access_aisles": access,
        "notes": l.notes,
    });
    match v {
        Value::Object(m) => m,
        _ => Props::new(),
    }
}

fn empty_result(why: String, basement: bool) -> Props {
    let v = json!({
        "stalls": 0, "accessible": 0, "accessible_needed": 0, "ev": 0, "columns": 0, "arrangement": "fields",
        "directions": [], "basement": basement, "area_per_stall": 0, "ways": [], "access_aisles": [], "notes": [why],
    });
    match v {
        Value::Object(m) => m,
        _ => Props::new(),
    }
}

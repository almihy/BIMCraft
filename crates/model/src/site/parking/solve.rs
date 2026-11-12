//! The ways in and out, and the solve (Arqiflow's solver.ts, `solveParking`): every candidate layout of the plot,
//! each with its ways in, the most bays a car can reach from the street wins; then the accessible bays nearest the
//! building and the bays with a charger.

use std::collections::HashSet;

use super::fields::*;
use super::geom::*;
use super::ring::{attach, lay_ring};
use super::standards::{Given, Settings, accessible_needed, ramp_length, settings_of};

// ====================================================================================================== the API
/// A way in or out: in, out, or both ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessKind {
    In,
    Out,
    Both,
}

impl AccessKind {
    pub fn name(self) -> &'static str {
        match self {
            AccessKind::In => "in",
            AccessKind::Out => "out",
            AccessKind::Both => "both",
        }
    }
    pub fn parse(s: &str) -> Option<AccessKind> {
        match s {
            "in" => Some(AccessKind::In),
            "out" => Some(AccessKind::Out),
            "both" => Some(AccessKind::Both),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Access {
    pub at: Pt,
    pub kind: AccessKind,
}

/// What the solver lays a car park out from, in metres (Arqiflow's `ParkingInput`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Input {
    /// The plot.
    pub site: Ring,
    /// What stands on it: buildings, things kept clear.
    pub obstacles: Vec<Ring>,
    /// The plot's sides that are streets (by index: side i runs from site[i] to site[i + 1]).
    pub streets: Vec<usize>,
    /// The ways in and out the person placed. Found when none is given.
    pub access: Vec<Access>,
    /// Ways in both ways (older form of `access`).
    pub entries: Vec<Pt>,
    /// Areas kept free of parking.
    pub keep_clear: Vec<Ring>,
    /// Bays the person took out: any bay containing one of these points is planted instead.
    pub removed: Vec<Pt>,
    /// Where people walk to from their car: the accessible bays go nearest it.
    pub target: Option<Pt>,
    /// Hold the rows to this direction (degrees).
    pub direction: Option<f64>,
    /// A first look, while the person drags.
    pub quick: bool,
    /// Below ground: the ramps on the level (one is found when none is given).
    pub basement: Option<Vec<Ring>>,
    pub settings: Given,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stall {
    pub poly: Ring,
    pub kind: StallKind,
    /// The middle of the bay's open side, where the car drives in.
    pub mouth: Pt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Aisle {
    pub poly: Ring,
    pub kind: AisleKind,
    /// A one-way aisle's direction of travel.
    pub flow: Option<Pt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub at: Pt,
    pub poly: Ring,
    pub kind: AccessKind,
    pub into: Pt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ramp {
    pub poly: Ring,
    pub foot: (Pt, Pt),
    /// Placed by the solver.
    pub found: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metrics {
    pub stalls: usize,
    pub accessible: usize,
    pub accessible_needed: usize,
    pub ev: usize,
    pub plot_area: f64,
    pub usable_area: f64,
    pub paved_area: f64,
    /// Plot area per bay, m².
    pub area_per_stall: f64,
    pub fields: i64,
}

/// A car park laid out (Arqiflow's `ParkingLayout`), in metres.
#[derive(Clone, Debug)]
pub struct Solved {
    pub stalls: Vec<Stall>,
    pub access_aisles: Vec<Ring>,
    pub aisles: Vec<Aisle>,
    pub islands: Vec<(Ring, IslandKind)>,
    pub entries: Vec<Entry>,
    /// The ground the car park may use (its largest part), and every part of it.
    pub usable: Region,
    pub parts: Vec<Region>,
    pub columns: Vec<Ring>,
    pub ramps: Vec<Ramp>,
    pub direction: f64,
    pub directions: Vec<f64>,
    /// "fields" or "ring".
    pub ring: bool,
    pub metrics: Metrics,
    pub notes: Vec<String>,
    pub settings: Settings,
}

// ====================================================================================================== ways in and out
struct StreetSide {
    a: Pt,
    b: Pt,
    /// Square to the side, into the plot.
    n: Pt,
}

#[derive(Clone)]
struct Throat {
    at: Pt,
    poly: Ring,
    kind: AccessKind,
    into: Pt,
    len: f64,
    off: f64,
}

fn project(p: Pt, a: Pt, b: Pt) -> Pt {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    [a[0] + dx * u, a[1] + dy * u]
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    dist(p, project(p, a, b))
}

struct RayHit {
    t: f64,
    u: f64,
    len: f64,
}

fn ray_hit(o: Pt, d: Pt, a: Pt, b: Pt) -> Option<RayHit> {
    let ex = b[0] - a[0];
    let ey = b[1] - a[1];
    let den = d[0] * ey - d[1] * ex;
    if den.abs() < 1e-9 {
        return None;
    }
    let t = ((a[0] - o[0]) * ey - (a[1] - o[1]) * ex) / den;
    let u = ((a[0] - o[0]) * d[1] - (a[1] - o[1]) * d[0]) / den;
    if t < -0.05 || u < 0.0 || u > 1.0 {
        return None;
    }
    let len = hypot(ex, ey);
    if den.abs() / len < 0.7 {
        return None;
    }
    Some(RayHit { t: t.max(0.0), u, len })
}

/// The throats a car could come in or go out by: from a street side, straight to an aisle.
#[allow(clippy::too_many_arguments)]
fn throat_options(
    plan: &WPlan,
    streets: &[StreetSide],
    clear: &Region,
    s: &Settings,
    kind: AccessKind,
    from: Option<Pt>,
    relax: bool,
) -> Vec<Throat> {
    let from_corner = if relax { 0.0 } else { s.entry_from_corner };
    let reach = if relax { 250.0 } else { 40.0 };
    let width = if kind == AccessKind::Both { s.drive_aisle } else { s.one_way_entry };
    let mut options: Vec<Throat> = Vec::new();
    let co = &clear.outer;
    let cn = co.len();
    let free = |poly: &[Pt]| {
        poly.iter().all(|&p| point_in_ring(p, co) || (0..cn).any(|i| seg_dist(p, co[i], co[(i + 1) % cn]) < 0.05))
            && !clear.holes.iter().any(|h| shared_area(poly, h) > 0.1 || h.iter().any(|&p| point_in_ring(p, poly)))
    };
    if from.is_none() {
        for al in &plan.aisles {
            let c = ccw(&al.poly);
            let sides = short_sides(&c);
            if sides.len() < 2 {
                continue;
            }
            for (j, &(p, q)) in sides.iter().enumerate() {
                let w = dist(p, q);
                if w < width - 0.05 {
                    continue;
                }
                let end = [(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0];
                let other = sides[1 - j];
                let back = [(other.0[0] + other.1[0]) / 2.0, (other.0[1] + other.1[1]) / 2.0];
                let l = or1(dist(end, back));
                let dir = [(end[0] - back[0]) / l, (end[1] - back[1]) / l];
                for st in streets {
                    let Some(h) = ray_hit(end, dir, st.a, st.b) else { continue };
                    if h.t > reach {
                        continue;
                    }
                    if (h.u * h.len).min((1.0 - h.u) * h.len) < from_corner {
                        continue;
                    }
                    let at = [end[0] + dir[0] * h.t, end[1] + dir[1] * h.t];
                    let poly = band([end[0] - dir[0], end[1] - dir[1]], at, width);
                    if h.t > 0.05 && !free(&poly) {
                        continue;
                    }
                    options.push(Throat { at, poly, kind, into: [-dir[0], -dir[1]], len: h.t, off: (h.u - 0.5).abs() });
                }
            }
        }
    }
    let square = |options: &mut Vec<Throat>, p: Pt, st: &StreetSide| {
        let mut t: Option<f64> = None;
        for al in &plan.aisles {
            if let Some(tt) = ray_into([p[0] - st.n[0] * 0.01, p[1] - st.n[1] * 0.01], st.n, &al.poly)
                && tt < reach * 1.5
                && t.is_none_or(|t| tt < t)
            {
                t = Some(tt);
            }
        }
        let Some(t) = t else { return };
        let end = [p[0] + st.n[0] * (t + 1.5), p[1] + st.n[1] * (t + 1.5)];
        let poly = band(p, end, width);
        if !free(&poly) {
            return;
        }
        let l = dist(st.a, st.b);
        let u = ((p[0] - st.a[0]) * (st.b[0] - st.a[0]) + (p[1] - st.a[1]) * (st.b[1] - st.a[1])) / (l * l);
        options.push(Throat { at: p, poly, kind, into: st.n, len: t, off: (u - 0.5).abs() });
    };
    if let Some(from) = from {
        let mut st: Option<&StreetSide> = streets.first();
        for x in streets {
            if let Some(m) = st
                && seg_dist(from, x.a, x.b) < seg_dist(from, m.a, m.b)
            {
                st = Some(x);
            }
        }
        if let Some(st) = st {
            square(&mut options, project(from, st.a, st.b), st);
            if options.is_empty() && relax {
                let p = project(from, st.a, st.b);
                let mut nearest: Option<(Pt, f64)> = None;
                for al in &plan.aisles {
                    let q = nearest_on_ring(p, &al.poly);
                    let d = dist(p, q);
                    if d > 0.05 && d < 30.0 && nearest.is_none_or(|(_, nd)| d < nd) {
                        nearest = Some((q, d));
                    }
                }
                if let Some((q, d)) = nearest {
                    let dir = [(q[0] - p[0]) / d, (q[1] - p[1]) / d];
                    if dir[0] * st.n[0] + dir[1] * st.n[1] > 0.2 {
                        let poly = band(p, [q[0] + dir[0] * 1.5, q[1] + dir[1] * 1.5], width);
                        if free(&poly) {
                            options.push(Throat { at: p, poly, kind, into: dir, len: d, off: 0.0 });
                        }
                    }
                }
            }
        }
    } else {
        for st in streets {
            if dist(st.a, st.b) >= 2.0 * from_corner {
                square(&mut options, [(st.a[0] + st.b[0]) / 2.0, (st.a[1] + st.b[1]) / 2.0], st);
            }
        }
    }
    options
}

/// The ways in and out: those the person placed, else the best one found (two on a big car park).
fn throats_for(plan: &WPlan, streets: &[StreetSide], clear: &Region, s: &Settings, given: &[Access], relax: bool) -> Vec<Throat> {
    if !given.is_empty() {
        return given
            .iter()
            .filter_map(|g| throat_options(plan, streets, clear, s, g.kind, Some(g.at), relax).into_iter().next())
            .collect();
    }
    let options = throat_options(plan, streets, clear, s, AccessKind::Both, None, relax);
    let cost = |o: &Throat| plan.stalls.iter().filter(|x| hits(&o.poly, &x.poly)).count() as f64 + o.len / 10.0 + o.off * 4.0;
    let mut costed: Vec<(f64, Throat)> = options.into_iter().map(|o| (cost(&o), o)).collect();
    costed.sort_by(|m, n| m.0.partial_cmp(&n.0).unwrap_or(std::cmp::Ordering::Equal));
    let options: Vec<Throat> = costed.into_iter().map(|x| x.1).collect();
    let mut out: Vec<Throat> = options.iter().take(1).cloned().collect();
    if let Some(first) = out.first().cloned()
        && plan.stalls.len() as f64 >= s.second_access_over
        && let Some(far) = options.iter().find(|o| dist(o.at, first.at) >= 40.0)
    {
        out.push(far.clone());
    }
    out
}

/// Which aisles a car gets to from these throats.
fn reachable(plan: &WPlan, throats: &[&Throat]) -> HashSet<i64> {
    let mut seen: HashSet<i64> = HashSet::new();
    let mut todo: Vec<usize> = Vec::new();
    for t in throats {
        for (k, a) in plan.aisles.iter().enumerate() {
            if !seen.contains(&a.id) && touch_along(&t.poly, &a.poly, 2.0) {
                seen.insert(a.id);
                todo.push(k);
            }
        }
    }
    while let Some(cur) = todo.pop() {
        for (k, a) in plan.aisles.iter().enumerate() {
            if !seen.contains(&a.id) && touch_along(&plan.aisles[cur].poly, &a.poly, 2.0) {
                seen.insert(a.id);
                todo.push(k);
            }
        }
    }
    seen
}

/// The aisle id a stall opens onto, its drive aisle when it faces one.
fn opens_onto(st: &WStall) -> i64 {
    if st.aisle >= EXT_BASE { st.aisle - EXT_BASE } else { st.aisle }
}

/// A drive aisle runs only as far as what it serves: a stub past them is cut off.
fn trim_drives(plan: &mut WPlan, throats: &[Throat]) {
    for di in 0..plan.aisles.len() {
        let dr = &plan.aisles[di];
        if dr.kind != AisleKind::Drive || dr.poly.len() != 4 {
            continue;
        }
        let c = dr.poly.clone();
        let e1 = dist(c[0], c[1]);
        let e2 = dist(c[1], c[2]);
        let (a, b) = if e1 >= e2 { (c[0], c[1]) } else { (c[1], c[2]) };
        let l = dist(a, b);
        let u = [(b[0] - a[0]) / l, (b[1] - a[1]) / l];
        let along = |p: Pt| (p[0] - a[0]) * u[0] + (p[1] - a[1]) * u[1];
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        let mut take = |poly: &[Pt]| {
            for &p in poly {
                let t = along(p);
                lo = lo.min(t);
                hi = hi.max(t);
            }
        };
        for (oi, o) in plan.aisles.iter().enumerate() {
            if oi != di && touch_along(&o.poly, &c, 2.0) {
                take(&o.poly);
            }
        }
        for t in throats {
            if touch_along(&t.poly, &c, 2.0) {
                take(&t.poly);
            }
        }
        let id = plan.aisles[di].id;
        for st in &plan.stalls {
            if opens_onto(st) == id {
                take(&st.poly);
            }
        }
        lo = lo.max(0.0);
        hi = hi.min(l);
        if !(hi > lo) || (lo < 0.5 && hi > l - 0.5) {
            continue;
        }
        plan.aisles[di].poly = c
            .iter()
            .map(|&p| {
                let t = along(p);
                let tt = if t < l / 2.0 { lo } else { hi };
                [p[0] + u[0] * (tt - t), p[1] + u[1] * (tt - t)]
            })
            .collect();
    }
}

// ====================================================================================================== solve
struct Candidate {
    deg: f64,
    plan: WPlan,
    reach: HashSet<i64>,
    throats: Vec<Throat>,
    score: f64,
    relaxed: bool,
}

/// A ramp's foot, where cars come off it onto the level: of its two short sides, the one further into the ground
/// `area`; and the way a car comes off it.
fn ramp_foot(ramp: &[Pt], area: &[Pt]) -> ((Pt, Pt), Pt) {
    let r = ccw(ramp);
    let short = short_sides(&r);
    let mid = |x: &(Pt, Pt)| -> Pt { [(x.0[0] + x.1[0]) / 2.0, (x.0[1] + x.1[1]) / 2.0] };
    let mut foot = short.first().copied().unwrap_or(([0.0, 0.0], [0.0, 0.0]));
    for x in short.iter().skip(1) {
        if dist_to_ring(mid(x), area) > dist_to_ring(mid(&foot), area) {
            foot = *x;
        }
    }
    let n = r.len().max(1) as f64;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for p in &r {
        cx += p[0];
    }
    for p in &r {
        cy += p[1];
    }
    let c = [cx / n, cy / n];
    let m = mid(&foot);
    let l = or1(dist(c, m));
    (foot, [(m[0] - c[0]) / l, (m[1] - c[1]) / l])
}

/// Below ground with no ramp drawn: a straight two-way ramp against a wall, at a corner, where the most bays are
/// left (each place tried with a quick look).
fn find_ramp(input: &Input, s: &Settings) -> Option<Ring> {
    let l = ramp_length(s);
    let w = s.drive_aisle;
    let ground = offset_ring(&ccw(&input.site), s.setback);
    let n = ground.len();
    if n == 0 {
        return None;
    }
    let mut best: Option<(Ring, usize)> = None;
    let mut walls: Vec<usize> = (0..n).collect();
    walls.sort_by(|&i, &j| {
        dist(ground[j], ground[(j + 1) % n]).partial_cmp(&dist(ground[i], ground[(i + 1) % n])).unwrap_or(std::cmp::Ordering::Equal)
    });
    walls.truncate(3);
    for i in walls {
        let a = ground[i];
        let b = ground[(i + 1) % n];
        let len = dist(a, b);
        if len < l + 3.0 {
            continue;
        }
        let t = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let nn = [-t[1], t[0]];
        for start in [0.0, len - l] {
            let p0 = [a[0] + t[0] * start, a[1] + t[1] * start];
            let poly = vec![
                p0,
                [p0[0] + t[0] * l, p0[1] + t[1] * l],
                [p0[0] + t[0] * l + nn[0] * w, p0[1] + t[1] * l + nn[1] * w],
                [p0[0] + nn[0] * w, p0[1] + nn[1] * w],
            ];
            if !poly.iter().all(|&p| point_in_ring(p, &ground) || dist_to_ring(p, &ground) < 0.01) {
                continue;
            }
            if input.obstacles.iter().any(|o| shared_area(&hull(o), &poly) > 0.01) {
                continue;
            }
            let trial = Input { quick: true, basement: Some(vec![poly.clone()]), ..input.clone() };
            let bays = solve(&trial).metrics.stalls;
            if best.as_ref().is_none_or(|(_, b)| bays > *b) {
                best = Some((poly, bays));
            }
        }
    }
    best.map(|(p, _)| p)
}

fn centre(p: &[Pt]) -> Pt {
    let n = p.len().max(1) as f64;
    let mut x = 0.0;
    let mut y = 0.0;
    for q in p {
        x += q[0];
    }
    for q in p {
        y += q[1];
    }
    [x / n, y / n]
}

/// JavaScript's `${n}` of a number as the notes print it.
fn num(v: f64) -> String {
    if v == v.trunc() && v.abs() < 1e15 { format!("{}", v as i64) } else { format!("{v}") }
}

/// The car park laid out (Arqiflow's `solveParking`).
pub fn solve(input: &Input) -> Solved {
    if let Some(ramps) = &input.basement
        && ramps.is_empty()
    {
        let s0 = settings_of(&input.settings);
        if let Some(ramp) = find_ramp(input, &s0) {
            let mut l = solve(&Input { basement: Some(vec![ramp]), ..input.clone() });
            for r in &mut l.ramps {
                r.found = true;
            }
            l.notes.insert(
                    0,
                    format!(
                        "No ramp is drawn on this level: one is placed, {} m long and {} m wide ({}% for {} m floor to floor). Draw the ramp where it comes down.",
                        num(ramp_length(&s0)),
                        num(s0.drive_aisle),
                        num(js_round(s0.ramp_slope * 100.0)),
                        num(s0.floor_to_floor)
                    ),
                );
            return l;
        }
    }
    let mut ctx = Ctx::new();
    let mut s = settings_of(&input.settings);
    if input.basement.is_some() && input.settings.column_every.is_none() {
        s.column_every = 3.0;
    }
    let basement = input.basement.is_some();
    let ramp_rings: Vec<Ring> = input.basement.iter().flatten().map(|r| ccw(r)).collect();
    let site = ccw(&input.site);
    let raw: Ring = if input.site.len() > 1 && dist(input.site[0], input.site[input.site.len() - 1]) < 1e-6 {
        input.site[..input.site.len() - 1].to_vec()
    } else {
        input.site.clone()
    };
    let plot_area = ring_area(&site).abs();
    let mut holes: Vec<Ring> = input.obstacles.iter().map(|o| offset_ring(&ccw(o), -s.building_clearance)).collect();
    holes.extend(input.keep_clear.iter().map(|o| ccw(o)));
    holes.extend(ramp_rings.iter().cloned());
    let (usable, parts) = attach(&offset_ring(&site, s.setback), &holes, s.stall_depth);
    let clear = Region::new(site.clone(), input.obstacles.iter().map(|o| ccw(o)).collect());
    let origin = site.first().copied().unwrap_or([0.0, 0.0]);
    let mut notes: Vec<String> = Vec::new();
    let street_lines: Vec<(Pt, Pt)> = if basement || raw.is_empty() {
        Vec::new()
    } else {
        input.streets.iter().map(|&i| (raw[i % raw.len()], raw[(i + 1) % raw.len()])).collect()
    };
    let mut streets: Vec<StreetSide> = street_lines
        .iter()
        .map(|&(a, b)| {
            let l = or1(dist(a, b));
            let mut n = [-(b[1] - a[1]) / l, (b[0] - a[0]) / l];
            let mid = [(a[0] + b[0]) / 2.0 + n[0] * 0.5, (a[1] + b[1]) / 2.0 + n[1] * 0.5];
            if !point_in_ring(mid, &site) {
                n = [-n[0], -n[1]];
            }
            StreetSide { a, b, n }
        })
        .collect();
    let mut access: Vec<Access> = if basement {
        Vec::new()
    } else {
        input.access.iter().copied().chain(input.entries.iter().map(|&at| Access { at, kind: AccessKind::Both })).collect()
    };
    let ramps: Vec<((Pt, Pt), Pt)> = ramp_rings.iter().map(|r| ramp_foot(r, &site)).collect();
    for (foot, out) in &ramps {
        streets.push(StreetSide { a: foot.0, b: foot.1, n: *out });
        access.push(Access { at: [(foot.0[0] + foot.1[0]) / 2.0, (foot.0[1] + foot.1[1]) / 2.0], kind: AccessKind::Both });
    }
    let removed = &input.removed;
    let degs: Vec<f64> = match input.direction {
        Some(d) => vec![((d % 180.0) + 180.0) % 180.0],
        None => directions(&site, if input.quick { 4 } else { 6 }, 0.0),
    };
    let free_dirs = input.direction.is_none() && !input.quick && !basement;

    let pick = |ctx: &mut Ctx, part: &Region| -> Option<Candidate> {
        let mut plans: Vec<(f64, WPlan)> = Vec::new();
        for &deg in &degs {
            for plan in lay_plot(ctx, part, deg, &s, origin, &street_lines, free_dirs) {
                plans.push((deg, plan));
            }
        }
        if free_dirs {
            for plan in lay_oblique(ctx, part, &s, origin) {
                plans.push((plan.dirs.first().copied().unwrap_or(f64::NAN), plan));
            }
        }
        for plan in lay_ring(ctx, part, &s, origin, &degs, input.quick, basement) {
            plans.push((plan.dirs.first().copied().unwrap_or(f64::NAN), plan));
        }
        let mut best: Option<Candidate> = None;
        for (deg, mut plan) in plans {
            // The bays a person took out are planted.
            if !removed.is_empty() {
                let mut kept = Vec::with_capacity(plan.stalls.len());
                for st in std::mem::take(&mut plan.stalls) {
                    if removed.iter().any(|&p| point_in_ring(p, &st.poly)) {
                        plan.islands.push(WIsland { poly: st.poly, kind: IslandKind::Leftover });
                    } else {
                        kept.push(st);
                    }
                }
                plan.stalls = kept;
            }
            // A bay must stand on the ground: wholly in the part, clear of what stands on it.
            let mut kept = Vec::with_capacity(plan.stalls.len());
            for st in std::mem::take(&mut plan.stalls) {
                let c = centre(&st.poly);
                let ok = st.poly.iter().all(|q| {
                    let x = [q[0] + (c[0] - q[0]) * 0.01, q[1] + (c[1] - q[1]) * 0.01];
                    point_in_ring(x, &part.outer) && !part.holes.iter().any(|h| point_in_ring(x, h))
                }) && !part.holes.iter().any(|h| hits(h, &st.poly));
                if ok {
                    kept.push(st);
                } else {
                    plan.islands.push(WIsland { poly: st.poly, kind: IslandKind::Leftover });
                }
            }
            plan.stalls = kept;
            // A bay whose open side is not on paving cannot be driven into: it is planted.
            let mut kept = Vec::with_capacity(plan.stalls.len());
            for mut st in std::mem::take(&mut plan.stalls) {
                let n = st.poly.len().max(1) as f64;
                let mut c = [0.0, 0.0];
                for q in &st.poly {
                    c = [c[0] + q[0] / n, c[1] + q[1] / n];
                }
                let d = or1(dist(c, st.mouth));
                let m = [st.mouth[0] + ((st.mouth[0] - c[0]) / d) * 0.4, st.mouth[1] + ((st.mouth[1] - c[1]) / d) * 0.4];
                let own = opens_onto(&st);
                if plan.aisles.iter().find(|a| a.id == own).is_some_and(|a| point_in_ring(m, &a.poly)) {
                    kept.push(st);
                    continue;
                }
                if let Some(other) = plan.aisles.iter().find(|a| point_in_ring(m, &a.poly)) {
                    st.aisle = other.id;
                    kept.push(st);
                    continue;
                }
                plan.islands.push(WIsland { poly: st.poly, kind: IslandKind::Leftover });
            }
            plan.stalls = kept;
            let mut throats = if streets.is_empty() { Vec::new() } else { throats_for(&plan, &streets, &clear, &s, &access, false) };
            let mut relaxed = false;
            if !streets.is_empty() && (throats.is_empty() || throats.len() < access.len()) {
                let more = throats_for(&plan, &streets, &clear, &s, &access, true);
                if more.len() > throats.len() {
                    throats = more;
                    relaxed = true;
                }
            }
            let ins: Vec<&Throat> = throats.iter().filter(|t| t.kind != AccessKind::Out).collect();
            let reach: HashSet<i64> = if !ins.is_empty() { reachable(&plan, &ins) } else { plan.aisles.iter().map(|a| a.id).collect() };
            let count =
                plan.stalls.iter().filter(|st| reach.contains(&opens_onto(st)) && !throats.iter().any(|t| hits(&t.poly, &st.poly))).count()
                    as f64;
            let paved = paved_w(&plan);
            let score = count
                - paved / PAVED_PER_BAY
                - if !streets.is_empty() && ins.is_empty() { 10000.0 } else { 0.0 }
                - if relaxed { 0.5 } else { 0.0 };
            if best.as_ref().is_none_or(|b| score > b.score) {
                best = Some(Candidate { deg, plan, reach, throats, score, relaxed });
            }
        }
        best
    };
    let mut chosen: Vec<Candidate> = Vec::new();
    for part in &parts {
        if let Some(c) = pick(&mut ctx, part)
            && !c.plan.stalls.is_empty()
        {
            chosen.push(c);
        }
    }
    let n_chosen = chosen.len();
    let best: Option<Candidate> = if n_chosen > 1 {
        let deg = chosen[0].deg;
        let mut reach = HashSet::new();
        let mut throats = Vec::new();
        let mut score = 0.0;
        let mut relaxed = false;
        let mut plans = Vec::new();
        for c in chosen {
            reach.extend(c.reach);
            throats.extend(c.throats);
            score += c.score;
            relaxed = relaxed || c.relaxed;
            plans.push(c.plan);
        }
        notes.push(format!("A building cuts the plot in {n_chosen} parts: each is laid out on its own, with its own way in."));
        Some(Candidate { deg, plan: join(plans, vec![]), reach, throats, score, relaxed })
    } else {
        chosen.into_iter().next()
    };
    let Some(best) = best else {
        return Solved {
            stalls: Vec::new(),
            access_aisles: Vec::new(),
            aisles: Vec::new(),
            islands: Vec::new(),
            entries: Vec::new(),
            metrics: Metrics { plot_area, usable_area: region_area_js(&usable), ..Metrics::default() },
            usable,
            parts,
            columns: Vec::new(),
            ramps: Vec::new(),
            direction: degs.first().copied().unwrap_or(0.0),
            directions: Vec::new(),
            ring: false,
            settings: s,
            notes: vec!["No bay fits on the plot at these sizes.".to_string()],
        };
    };
    let Candidate { deg: best_deg, mut plan, reach, throats, relaxed, .. } = best;
    trim_drives(&mut plan, &throats);
    if basement && ramps.is_empty() {
        notes.push(
            "No ramp reaches this level and none fits along a wall: draw the ramp. The layout is shown as if every aisle were reached."
                .to_string(),
        );
    } else if basement && throats.is_empty() {
        notes.push("No aisle can be reached straight from the foot of a ramp: move the ramp so its foot faces an aisle. The layout is shown as if every aisle were reached.".to_string());
    } else if !streets.is_empty() && throats.is_empty() {
        notes.push("No aisle can be reached straight from a street side: place a way in on a street. The layout is shown as if every aisle were reached.".to_string());
    }
    if relaxed && !throats.is_empty() && basement {
        notes.push(
            "A ramp's foot does not face an aisle square: the way off it runs at an angle into the nearest aisle. Check the turn."
                .to_string(),
        );
    } else if relaxed && !throats.is_empty() {
        notes.push(format!(
            "A way in or out is nearer a street corner than {} m, or runs a long way to its aisle: check it with the traffic engineer.",
            num(s.entry_from_corner)
        ));
    }
    if streets.is_empty() && !basement {
        notes.push("No street side given: the ways in are not drawn and every aisle is taken as reached.".to_string());
    }
    if basement && s.column_every > 0.0 && s.angle < 89.9 {
        notes.push("Columns are set out for square bays only: with angled bays, place them by hand.".to_string());
    }
    if !basement && access.len() > throats.len() {
        notes.push(format!(
            "{} of the ways in or out placed could not be joined to an aisle: place them on a street side, in line with an aisle.",
            access.len() - throats.len()
        ));
    }
    if !throats.is_empty() && throats.iter().all(|t| t.kind == AccessKind::In) {
        notes.push("There is a way in but no way out: add an exit, or make the entrance both ways.".to_string());
    }
    if !throats.is_empty() && throats.iter().all(|t| t.kind == AccessKind::Out) {
        notes.push("There is a way out but no way in.".to_string());
    }
    let outs: Vec<&Throat> = throats.iter().filter(|t| t.kind != AccessKind::In).collect();
    if !outs.is_empty() && throats.iter().any(|t| t.kind == AccessKind::In) {
        let leave = reachable(&plan, &outs);
        if reach.iter().any(|id| !leave.contains(id)) {
            notes.push("Some aisles a car comes in by do not lead to a way out.".to_string());
        }
    }

    // The bays kept: reached, and off the throats.
    let mut kept: Vec<WStall> = Vec::new();
    let mut unreached = 0;
    for st in std::mem::take(&mut plan.stalls) {
        let r = reach.contains(&opens_onto(&st));
        if !r {
            unreached += 1;
        }
        if r && !throats.iter().any(|t| hits(&t.poly, &st.poly)) {
            kept.push(st);
        }
    }
    if unreached > 0 && !streets.is_empty() && !throats.is_empty() {
        notes.push(format!("{unreached} bays could not be driven to and were left out."));
    }

    // Accessible bays nearest the building's entrance: two to an access aisle, as many as the count needs.
    let need = accessible_needed(&s, kept.len());
    let target: Pt = input
        .target
        .or_else(|| throats.first().map(|t| t.at))
        .or_else(|| streets.first().map(|st| [(st.a[0] + st.b[0]) / 2.0, (st.a[1] + st.b[1]) / 2.0]))
        .unwrap_or(origin);
    let by_near = |kept: &[WStall]| -> Vec<usize> {
        let mut order: Vec<(f64, usize)> = kept.iter().enumerate().map(|(i, st)| (dist(centre(&st.poly), target), i)).collect();
        order.sort_by(|p, q| p.0.partial_cmp(&q.0).unwrap_or(std::cmp::Ordering::Equal));
        order.into_iter().map(|x| x.1).collect()
    };
    let mut marked = 0;
    if need > 0 {
        let key = |st: &WStall| format!("{}|{}", st.row, st.cell);
        let mut by_key: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (i, st) in kept.iter().enumerate() {
            by_key.insert(key(st), i);
        }
        let mut taken: HashSet<String> = HashSet::new();
        for i in by_near(&kept) {
            if marked >= need {
                break;
            }
            if taken.contains(&key(&kept[i])) {
                continue;
            }
            let row = kept[i].row.clone();
            let cell = kept[i].cell;
            let at = |dk: i64| by_key.get(&format!("{row}|{}", cell + dk)).copied();
            let free = |x: Option<usize>, taken: &HashSet<String>| x.is_some_and(|x| !taken.contains(&key(&kept[x])));
            let mut pick: Option<(Vec<usize>, usize)> = None;
            if need - marked >= 2 && free(at(1), &taken) && free(at(2), &taken) {
                pick = at(2).zip(at(1)).map(|(two, one)| (vec![i, two], one));
            } else if need - marked >= 2 && free(at(-1), &taken) && free(at(-2), &taken) {
                pick = at(-2).zip(at(-1)).map(|(two, one)| (vec![i, two], one));
            } else if free(at(1), &taken) {
                pick = at(1).map(|one| (vec![i], one));
            } else if free(at(-1), &taken) {
                pick = at(-1).map(|one| (vec![i], one));
            }
            let Some((pk, gap)) = pick else { continue };
            for &p in &pk {
                kept[p].kind = StallKind::Accessible;
                taken.insert(key(&kept[p]));
            }
            kept[gap].access = true;
            taken.insert(key(&kept[gap]));
            marked += pk.len();
        }
    }
    if need > marked {
        notes.push(format!("{need} accessible bays are needed; {marked} could be placed."));
    }
    // Bays with a charger: the share asked, next nearest the building.
    let ev_wanted = (kept.iter().filter(|st| !st.access).count() as f64 * s.ev_share.max(0.0)).ceil() as usize;
    let mut ev = 0;
    if ev_wanted > 0 {
        for i in by_near(&kept) {
            if ev >= ev_wanted {
                break;
            }
            if kept[i].access || kept[i].kind != StallKind::Standard {
                continue;
            }
            kept[i].kind = StallKind::Ev;
            ev += 1;
        }
    }

    let stalls: Vec<Stall> =
        kept.iter().filter(|st| !st.access).map(|st| Stall { poly: st.poly.clone(), kind: st.kind, mouth: st.mouth }).collect();
    let access_aisles: Vec<Ring> = kept.iter().filter(|st| st.access).map(|st| st.poly.clone()).collect();
    let used_aisles: Vec<&WAisle> = plan.aisles.iter().filter(|a| reach.contains(&a.id)).collect();
    let mut aisles: Vec<Aisle> = used_aisles.iter().map(|a| Aisle { poly: a.poly.clone(), kind: a.kind, flow: a.flow }).collect();
    aisles.extend(throats.iter().map(|t| Aisle { poly: t.poly.clone(), kind: AisleKind::Throat, flow: None }));
    let islands: Vec<(Ring, IslandKind)> =
        plan.islands.iter().filter(|i| !throats.iter().any(|t| hits(&t.poly, &i.poly))).map(|i| (i.poly.clone(), i.kind)).collect();
    let mut paved_area = 0.0;
    for p in used_aisles.iter().map(|a| &a.poly).chain(throats.iter().map(|t| &t.poly)).chain(kept.iter().map(|st| &st.poly)) {
        paved_area += ring_area(p).abs();
    }
    let count = stalls.len();
    let columns: Vec<Ring> = plan
        .columns
        .iter()
        .filter(|c| !throats.iter().any(|t| hits(&t.poly, c)) && !aisles.iter().any(|a| hits(&a.poly, c)))
        .cloned()
        .collect();
    let mut directions: Vec<f64> = Vec::new();
    for d in &plan.dirs {
        let v = js_round(d * 100.0) / 100.0;
        if !directions.contains(&v) {
            directions.push(v);
        }
    }
    Solved {
        stalls,
        access_aisles,
        aisles,
        islands,
        entries: throats.iter().map(|t| Entry { at: t.at, poly: t.poly.clone(), kind: t.kind, into: t.into }).collect(),
        metrics: Metrics {
            stalls: count,
            accessible: marked,
            accessible_needed: need,
            ev,
            plot_area: js_round(plot_area),
            usable_area: js_round(region_area_js(&usable)),
            paved_area: js_round(paved_area),
            area_per_stall: if count > 0 { js_round((plot_area / count as f64) * 10.0) / 10.0 } else { 0.0 },
            fields: plan.fields,
        },
        usable,
        parts,
        columns,
        ramps: ramp_rings.iter().zip(ramps.iter()).map(|(poly, r)| Ramp { poly: poly.clone(), foot: r.0, found: false }).collect(),
        direction: js_round(best_deg * 100.0) / 100.0,
        directions,
        ring: plan.ring,
        settings: s,
        notes,
    }
}

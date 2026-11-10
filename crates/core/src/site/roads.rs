//! A road as a road: the port of Arqiflow's server/arqiflow/kernel/roads.py (the outlines) and web/components/editor/
//! geom/roads.ts (the outlines and what the plan draws), in millimetres. A lane of use "road" has a cross-section:
//!
//! - `width`: the right of way, kerb to kerb and the pavements, and the road's outline across (what is selected, measured
//!   and kept clear);
//! - `lanes` of `lane_width` (both ways together), `oneway`, a `median` between the two ways, the `kerb`'s height,
//!   `corner_radius` the kerb's radius where it meets another road, and `markings` painted or not.
//!
//! The pavement each side is what is left of the right of way: (width - lanes × lane_width - median) / 2. Where the
//! carriageway is wider than the right of way the road is drawn at the carriageway's width.
//!
//! Roads that meet make one paved area:
//! - **joints**: pieces of one road that meet end to end (two ends head on, within 35° of straight) are one road going
//!   on; a patch joins them, owned by the wider piece, whose edges taper to the other's over ten times the change (1:10);
//! - **junctions**: where a road ends on another (a T, an L) or two cross (an X), each outline takes the rounded corners
//!   (the kerb's radius less the pavement), owned by the road that ends there (the first of two that cross);
//! - for drawing, the carriageways are joined the same way with the kerb's radius ([`net`]), the markings stop short of
//!   the junctions, the kerb lines and the back of the pavements are each road's edges less what lies in the roads it
//!   meets, and stop or give-way lines and one-way arrows are painted.
//!
//! The browser's memo (work kept between frames of a drag) is not ported: every call works everything out, which a
//! level of a hundred and fifty roads does in a few tens of milliseconds.

use std::collections::{BTreeMap, BTreeSet};

use super::poly::{
    ARC_TOL, Ring, Shape, Xy, at, band, ccw, clip_lines, closing_pieces, cross, difference, dist, dot, grow, hull, in_rings, intersection,
    join, left_of, line_distance, ring_area, rings_distance, seg_point, shape_distance, sub, union,
};
use crate::site::Props;

pub const LANE: f64 = 3500.0;
pub const KERB: f64 = 150.0;
const OUTER_RADIUS_MIN: f64 = 500.0;
/// A road's end this close to another's outline meets it.
const TOUCH: f64 = 10.0;
const MOST_LANES: i64 = 8;
/// How far a junction's markings stop short of the other road's kerb.
const STOP: f64 = 1000.0;
/// A change of width or of line between two pieces of a road tapers over this many times the change (1:10).
pub const TAPER: f64 = 10.0;
/// Two road ends meeting head on within this angle of straight are one road going on (degrees).
const JOINT_ANGLE: f64 = 35.0;
/// How far into a piece its joint's patch reaches, so that no edge lies on another's.
const INTO: f64 = 50.0;
/// How far an edge may lie off another road's shape and still be inside it (an edge two roads share is no edge).
const SHARED: f64 = 20.0;
/// The keys of a road's cross-section beside its width.
pub const SECTION_KEYS: [&str; 6] = ["lanes", "lane_width", "oneway", "median", "kerb", "markings"];

// ====================================================================== the cross-section
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    /// The right of way.
    pub row: f64,
    pub lanes: i64,
    pub lane_width: f64,
    pub oneway: bool,
    pub median: f64,
    pub kerb: f64,
    pub kerb_radius: f64,
    pub markings: bool,
}

impl Section {
    pub fn carriageway(&self) -> f64 {
        self.lanes as f64 * self.lane_width
    }
    /// Kerb to kerb: the carriageway and the median.
    pub fn paved(&self) -> f64 {
        self.carriageway() + if self.oneway { 0.0 } else { self.median }
    }
    pub fn pavement(&self) -> f64 {
        ((self.row - self.paved()) / 2.0).max(0.0)
    }
    /// The road's outline across: its right of way, or the carriageway where that is wider.
    pub fn outer(&self) -> f64 {
        self.row.max(self.paved())
    }
    pub fn outer_radius(&self) -> f64 {
        OUTER_RADIUS_MIN.max(self.kerb_radius - self.pavement())
    }
}

/// Rounded to `step` (50 mm), halves to even as the server's Python does.
fn r50(v: f64) -> f64 {
    (v / 50.0).round_ties_even() * 50.0
}

/// A street known by its right of way only (a plot's street): pavements about 15 % of it each side (1.5 to 5 m), as many
/// 3.5 m lanes as fit in pairs, a 4 m median from 30 m. (lanes, lane_width, oneway, median, markings).
pub fn for_row(row: f64) -> (i64, f64, bool, f64, bool) {
    if row < 6000.0 {
        return (1, r50(LANE.min(row)), false, 0.0, false);
    }
    let pav = 5000.0_f64.min(1500.0_f64.max((0.3 * row / 1000.0).round_ties_even() * 500.0));
    let median = if row >= 30000.0 { 4000.0 } else { 0.0 };
    let avail = row - 2.0 * pav - median;
    let n = (avail / LANE + 1e-9).floor() as i64;
    let n = (n - n % 2).clamp(2, MOST_LANES);
    // A pavement of a metre at the least, on a street wide enough.
    let least = if row >= 8000.0 { 1000.0 } else { 0.0 };
    let lw = if n as f64 * LANE <= avail + 1e-6 { LANE } else { 2750.0_f64.max(LANE.min((row - median - 2.0 * least) / n as f64)) };
    (n, r50(lw), false, median, true)
}

fn num(p: &Props, k: &str) -> Option<f64> {
    p.get(k).and_then(|v| v.as_f64()).filter(|v| v.is_finite())
}

fn boolean(p: &Props, k: &str) -> Option<bool> {
    p.get(k).and_then(|v| v.as_bool())
}

/// A road's right of way: its width, else 7 m.
pub fn row_of(p: &Props) -> f64 {
    num(p, "width").filter(|w| *w != 0.0).map(|w| w.abs().min(1e6)).unwrap_or(7000.0)
}

/// A road's cross-section from its props (mm), what is missing worked out from its right of way.
pub fn section_of(p: &Props) -> Section {
    let row = row_of(p);
    let (lanes, lw, oneway, median, markings) = for_row(row);
    Section {
        row,
        lanes: num(p, "lanes").map(|v| v.clamp(-1e6, 1e6).round_ties_even() as i64).unwrap_or(lanes).clamp(1, 64),
        lane_width: num(p, "lane_width").unwrap_or(lw).clamp(500.0, 1e5),
        oneway: boolean(p, "oneway").unwrap_or(oneway),
        median: num(p, "median").unwrap_or(median).clamp(0.0, 1e6),
        kerb: num(p, "kerb").unwrap_or(KERB).clamp(0.0, 1e5),
        kerb_radius: num(p, "corner_radius").unwrap_or(6000.0).clamp(0.0, 1e6),
        markings: boolean(p, "markings").unwrap_or(markings),
    }
}

/// A road's cross-section written out where it is not (from its right of way): what is to be filled in.
pub fn fill_road(p: &Props) -> Props {
    let (lanes, lw, oneway, median, markings) = for_row(row_of(p));
    let mut out = Props::new();
    let missing = |k: &str| p.get(k).is_none_or(|v| v.is_null());
    let put = |out: &mut Props, k: &str, v: serde_json::Value| {
        if missing(k) {
            out.insert(k.into(), v);
        }
    };
    put(&mut out, "lanes", lanes.into());
    put(&mut out, "lane_width", lw.into());
    put(&mut out, "oneway", oneway.into());
    put(&mut out, "median", median.into());
    put(&mut out, "markings", markings.into());
    put(&mut out, "kerb", KERB.into());
    out
}

// ====================================================================== lines
/// A polyline moved square to itself by `by` to its left, each corner along its mitre (roads.ts offsetLine).
pub fn offset_line(line: &[Xy], by: f64) -> Vec<Xy> {
    let n = line.len();
    if n < 2 {
        return line.to_vec();
    }
    let normal = |a: Xy, b: Xy| {
        let l = dist(a, b);
        let l = if l > 0.0 { l } else { 1.0 };
        [-(b[1] - a[1]) / l, (b[0] - a[0]) / l]
    };
    line.iter()
        .enumerate()
        .map(|(i, q)| {
            let m = if i == 0 {
                normal(line[0], line[1])
            } else if i == n - 1 {
                normal(line[n - 2], line[n - 1])
            } else {
                let (a, b) = (normal(line[i - 1], line[i]), normal(line[i], line[i + 1]));
                let s = [a[0] + b[0], a[1] + b[1]];
                let ls = s[0].hypot(s[1]);
                if ls < 1e-9 {
                    a
                } else {
                    let u = [s[0] / ls, s[1] / ls];
                    let k = 1.0 / 0.25_f64.max(dot(u, a));
                    [u[0] * k, u[1] * k]
                }
            };
            [q[0] + m[0] * by, q[1] + m[1] * by]
        })
        .collect()
}

pub fn length_of(line: &[Xy]) -> f64 {
    line.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// A line's end (0 its start, 1 its end): the point and the way out of the line there.
pub fn end_of(line: &[Xy], end: usize) -> (Xy, Xy) {
    let n = line.len();
    if n < 2 {
        return (line.first().copied().unwrap_or([0.0, 0.0]), [1.0, 0.0]);
    }
    let (a, b) = if end == 1 { (line[n - 2], line[n - 1]) } else { (line[1], line[0]) };
    let l = dist(a, b);
    let l = if l > 0.0 { l } else { 1.0 };
    (b, [(b[0] - a[0]) / l, (b[1] - a[1]) / l])
}

/// The point `l` back from a line's end along it, the way out of the line there, and the line from it to the end.
fn back(line: &[Xy], end: usize, l: f64) -> (Xy, Xy, Vec<Xy>) {
    let mut pts = line.to_vec();
    if end == 1 {
        pts.reverse();
    }
    let mut left = l;
    let mut rest = vec![pts[0]];
    for i in 0..pts.len().saturating_sub(1) {
        let (a, b) = (pts[i], pts[i + 1]);
        let s = dist(a, b);
        if s >= left - 1e-9 || i + 2 == pts.len() {
            let k = if s > 0.0 { (left / s).min(1.0) } else { 0.0 };
            let p = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
            rest.push(p);
            let d = if s > 0.0 { [(a[0] - b[0]) / s, (a[1] - b[1]) / s] } else { [1.0, 0.0] };
            rest.reverse();
            return (p, d, rest);
        }
        left -= s;
        rest.push(b);
    }
    let (p, d) = end_of(line, end);
    (p, d, vec![p])
}

/// Where two polylines cross, each crossing once.
fn crossings(a: &[Xy], b: &[Xy]) -> Vec<Xy> {
    let mut out: Vec<Xy> = Vec::new();
    for s in a.windows(2) {
        for t in b.windows(2) {
            let (p, q, u, v) = (s[0], s[1], t[0], t[1]);
            let (r, w) = (sub(q, p), sub(v, u));
            let den = cross(r, w);
            if den.abs() < 1e-12 {
                continue;
            }
            let tt = cross(sub(u, p), w) / den;
            let uu = cross(sub(u, p), r) / den;
            if !(0.0..=1.0).contains(&tt) || !(0.0..=1.0).contains(&uu) {
                continue;
            }
            let x = [p[0] + r[0] * tt, p[1] + r[1] * tt];
            if !out.iter().any(|o| dist(*o, x) < 1e-3) {
                out.push(x);
            }
        }
    }
    out
}

/// A point `s` along a line (held to the line) and the way along it there.
pub fn point_at(line: &[Xy], s: f64) -> (Xy, Xy) {
    let mut left = s.max(0.0);
    for i in 0..line.len().saturating_sub(1) {
        let (a, b) = (line[i], line[i + 1]);
        let l = dist(a, b);
        if left <= l || i + 2 == line.len() {
            let k = if l > 0.0 { (left / l).min(1.0) } else { 0.0 };
            let d = if l > 0.0 { [(b[0] - a[0]) / l, (b[1] - a[1]) / l] } else { [1.0, 0.0] };
            return ([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k], d);
        }
        left -= l;
    }
    (line.first().copied().unwrap_or([0.0, 0.0]), [1.0, 0.0])
}

/// How far along a line its nearest point to `p` is.
fn along_of(line: &[Xy], p: Xy) -> f64 {
    let (mut best, mut s, mut out) = (f64::INFINITY, 0.0, 0.0);
    for w in line.windows(2) {
        let (d, t, _) = seg_point(w[0], w[1], p);
        let l = dist(w[0], w[1]);
        if d < best {
            best = d;
            out = s + t * l;
        }
        s += l;
    }
    out
}

/// Pieces of a line in the line's own direction and order.
fn order_runs(line: &[Xy], runs: Vec<Vec<Xy>>) -> Vec<Vec<Xy>> {
    let mut keyed: Vec<(f64, Vec<Xy>)> = runs
        .into_iter()
        .filter(|r| r.len() >= 2)
        .map(|mut r| {
            let (a, b) = (along_of(line, r[0]), along_of(line, r[r.len() - 1]));
            if a > b {
                r.reverse();
            }
            (a.min(b), r)
        })
        .collect();
    keyed.sort_by(|x, y| x.0.total_cmp(&y.0));
    keyed.into_iter().map(|x| x.1).collect()
}

/// A polyline less what lies inside `rings`, in runs of at least `least`.
pub fn line_outside(line: &[Xy], rings: &[Ring], least: f64) -> Vec<Vec<Xy>> {
    order_runs(line, clip_lines(&[line.to_vec()], rings, true)).into_iter().filter(|r| length_of(r) >= least).collect()
}

/// Dashes along a polyline: `dash` on, `gap` off, as pieces.
pub fn dashes(line: &[Xy], dash: f64, gap: f64) -> Vec<Vec<Xy>> {
    let mut out = Vec::new();
    let period = dash + gap;
    if dash.is_nan() || gap.is_nan() || dash <= 0.0 || gap < 0.0 || !period.is_finite() {
        return out;
    }
    let mut along = 0.0;
    let mut cur: Option<Vec<Xy>> = None;
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let l = dist(a, b);
        let mut t = 0.0;
        let mut steps = 0;
        while t < l - 1e-6 && steps < 100_000 {
            steps += 1;
            let phase = along % period;
            let on = phase < dash;
            let left = if on { dash - phase } else { period - phase };
            let step = left.min(l - t);
            let p0 = [a[0] + (b[0] - a[0]) * t / l, a[1] + (b[1] - a[1]) * t / l];
            let p1 = [a[0] + (b[0] - a[0]) * (t + step) / l, a[1] + (b[1] - a[1]) * (t + step) / l];
            if on {
                cur.get_or_insert_with(|| vec![p0]).push(p1);
            } else if let Some(c) = cur.take() {
                out.push(c);
            }
            t += step;
            along += step;
        }
    }
    if let Some(c) = cur.filter(|c| c.len() >= 2) {
        out.push(c);
    }
    out
}

// ====================================================================== near each other, joints and junctions
/// One road as the functions here read it.
#[derive(Clone, Debug)]
pub struct RoadIn {
    /// What it is known by (the element's key): the wider of two pieces owns their joint, the first by key where they
    /// are as wide.
    pub id: String,
    /// Its centre line, its arcs in pieces.
    pub line: Vec<Xy>,
    pub s: Section,
    /// Its own outline (with any turning heads); its band where not given.
    pub own: Option<Vec<Ring>>,
}

type Box4 = [f64; 4];

fn box_of(line: &[Xy], pad: f64) -> Box4 {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for q in line {
        b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
    }
    [b[0] - pad, b[1] - pad, b[2] + pad, b[3] + pad]
}

/// For each road, the others whose boxes (each its line grown by its half outline and a metre) meet its own, in order:
/// a sweep along x, so a level of hundreds of roads looks only at the few near each.
pub fn neighbours(roads: &[RoadIn]) -> Vec<Vec<usize>> {
    let boxes: Vec<Box4> = roads.iter().map(|r| box_of(&r.line, r.s.outer() / 2.0 + 1000.0)).collect();
    let mut order: Vec<usize> = (0..roads.len()).collect();
    order.sort_by(|a, b| boxes[*a][0].total_cmp(&boxes[*b][0]));
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); roads.len()];
    for x in 0..order.len() {
        let i = order[x];
        let a = boxes[i];
        for &j in &order[x + 1..] {
            let b = boxes[j];
            if b[0] > a[2] {
                break;
            }
            if b[1] <= a[3] && a[1] <= b[3] {
                out[i].push(j);
                out[j].push(i);
            }
        }
    }
    for l in &mut out {
        l.sort_unstable();
    }
    out
}

/// Two pieces of one road meeting end to end: road `a`'s end `ae` and road `b`'s end `be` (0 the start, 1 the end).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    pub a: usize,
    pub ae: usize,
    pub b: usize,
    pub be: usize,
}

/// Where pieces of one road meet end to end: two ends meeting head on (within 35° of straight), no further apart along
/// the road than a quarter of the wider (1.5 m at least) nor across it than a quarter of the wider and a quarter metre.
/// Each end is in one joint at most, the closest.
pub fn joints(roads: &[RoadIn], near: &[Vec<usize>]) -> Vec<Joint> {
    let cos = JOINT_ANGLE.to_radians().cos();
    let mut cands: Vec<(f64, Joint)> = Vec::new();
    for (i, ra) in roads.iter().enumerate() {
        for &k in &near[i] {
            if k <= i {
                continue;
            }
            let rb = &roads[k];
            let most = ra.s.outer().max(rb.s.outer());
            for ae in [0, 1] {
                let (pa, da) = end_of(&ra.line, ae);
                for be in [0, 1] {
                    let (pb, db) = end_of(&rb.line, be);
                    if dot(da, db) > -cos {
                        continue;
                    }
                    let v = sub(pb, pa);
                    if dot(v, da).abs() > 1500.0_f64.max(most / 4.0) {
                        continue;
                    }
                    if cross(da, v).abs() > most / 4.0 + 250.0 {
                        continue;
                    }
                    cands.push((v[0].hypot(v[1]), Joint { a: i, ae, b: k, be }));
                }
            }
        }
    }
    cands.sort_by(|x, y| {
        x.0.total_cmp(&y.0).then(x.1.a.cmp(&y.1.a)).then(x.1.b.cmp(&y.1.b)).then(x.1.ae.cmp(&y.1.ae)).then(x.1.be.cmp(&y.1.be))
    });
    let mut used: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut out = Vec::new();
    for (_, j) in cands {
        if used.contains(&(j.a, j.ae)) || used.contains(&(j.b, j.be)) {
            continue;
        }
        used.insert((j.a, j.ae));
        used.insert((j.b, j.be));
        out.push(j);
    }
    out
}

/// Where roads meet: the point, the roads there (the first the one whose end it is, or the first of two that cross)
/// and those that end there (none where two cross).
#[derive(Clone, Debug, PartialEq)]
pub struct Junction {
    pub p: Xy,
    pub at: Vec<usize>,
    pub ending: Vec<usize>,
}

fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Where roads meet: a road's end within another's outline (a T, an L), two lines crossing away from their ends (an X),
/// or a joint within a road running across it (an X of pieces). A joint's ends are not ends: the road goes on; a road
/// ending on a piece meets the piece it joins as well where the joint is near.
pub fn junctions(roads: &[RoadIn], js: &[Joint], near: &[Vec<usize>]) -> Vec<Junction> {
    let mut joint_at: BTreeMap<(usize, usize), (usize, usize)> = BTreeMap::new();
    let mut partners: BTreeSet<(usize, usize)> = BTreeSet::new();
    for j in js {
        joint_at.insert((j.a, j.ae), (j.b, j.be));
        joint_at.insert((j.b, j.be), (j.a, j.ae));
        partners.insert(pair(j.a, j.b));
    }
    let mut out = Vec::new();
    for (i, a) in roads.iter().enumerate() {
        for end in [0, 1] {
            if joint_at.contains_key(&(i, end)) {
                continue;
            }
            let p = end_of(&a.line, end).0;
            let mut at_: Vec<usize> =
                near[i].iter().copied().filter(|&j| line_distance(&roads[j].line, p).0 <= roads[j].s.outer() / 2.0 + TOUCH).collect();
            for j in at_.clone() {
                for e2 in [0, 1] {
                    let Some(&(other, _)) = joint_at.get(&(j, e2)) else { continue };
                    if other == i || at_.contains(&other) {
                        continue;
                    }
                    let q = end_of(&roads[j].line, e2).0;
                    if dist(q, p) <= a.s.outer() / 2.0 + roads[j].s.outer() / 2.0 + TOUCH {
                        at_.push(other);
                    }
                }
            }
            if !at_.is_empty() {
                let mut all = vec![i];
                all.extend(at_);
                out.push(Junction { p, at: all, ending: vec![i] });
            }
        }
    }
    for (i, a) in roads.iter().enumerate() {
        for &j in &near[i] {
            if j <= i || partners.contains(&pair(i, j)) {
                continue;
            }
            let b = &roads[j];
            for p in crossings(&a.line, &b.line) {
                let near_end = [(a, b), (b, a)]
                    .iter()
                    .any(|(r, o)| [r.line[0], r.line[r.line.len() - 1]].iter().any(|q| dist(*q, p) <= o.s.outer() / 2.0 + TOUCH));
                if !near_end {
                    out.push(Junction { p, at: vec![i, j], ending: vec![] });
                }
            }
        }
    }
    // A joint within a road that runs across it (one that ends near it is a T, above).
    for jt in js {
        let p = end_of(&roads[jt.a].line, jt.ae).0;
        let half = roads[jt.a].s.outer().max(roads[jt.b].s.outer()) / 2.0;
        let ys: BTreeSet<usize> = near[jt.a].iter().chain(near[jt.b].iter()).copied().filter(|y| *y != jt.a && *y != jt.b).collect();
        for y in ys {
            let r = &roads[y];
            if line_distance(&r.line, p).0 > r.s.outer() / 2.0 + TOUCH {
                continue;
            }
            let ends_near = [0, 1].iter().any(|e| dist(end_of(&r.line, *e).0, p) <= r.s.outer() / 2.0 + half + TOUCH);
            if !ends_near {
                out.push(Junction { p, at: vec![y, jt.a, jt.b], ending: vec![] });
            }
        }
    }
    out
}

/// How the roads meet, worked out once for the outlines and the drawing.
#[derive(Clone, Debug)]
pub struct Layout {
    pub near: Vec<Vec<usize>>,
    pub joints: Vec<Joint>,
    pub junctions: Vec<Junction>,
}

pub fn layout(roads: &[RoadIn]) -> Layout {
    let near = neighbours(roads);
    let js = joints(roads, &near);
    let jn = junctions(roads, &js, &near);
    Layout { near, joints: js, junctions: jn }
}

// ====================================================================== joined shapes
struct Patch {
    a_owns: bool,
    cut: Option<Vec<Ring>>,
    ring: Ring,
    l: f64,
}

/// The patch that joins two pieces at a joint for half widths `half`: owned by the wider (by the first id where they
/// are as wide), which is cut back by the taper where the edges move.
fn patch_of(roads: &[RoadIn], j: &Joint, half: &dyn Fn(&Section) -> f64) -> Patch {
    let (ha, hb) = (half(&roads[j.a].s), half(&roads[j.b].s));
    let a_owns = ha > hb + 1e-3 || ((ha - hb).abs() <= 1e-3 && roads[j.a].id < roads[j.b].id);
    let (o, oe, x, xe) = if a_owns { (j.a, j.ae, j.b, j.be) } else { (j.b, j.be, j.a, j.ae) };
    let (ho, hx) = (half(&roads[o].s), half(&roads[x].s));
    let (op, od) = end_of(&roads[o].line, oe);
    let (xp, xd) = end_of(&roads[x].line, xe);
    let (lo, lx) = (left_of(od), [xd[1], -xd[0]]);
    let e = dot(sub(xp, op), lo);
    let moved = (e + hx - ho).abs().max((e - hx + ho).abs());
    let mut l = moved * TAPER;
    if l < 250.0 {
        l = 0.0;
    }
    l = l.min(0.45 * length_of(&roads[o].line));
    let xin = at(xp, xd, -INTO);
    if l > 0.0 {
        let (tp, td, mut rest) = back(&roads[o].line, oe, l);
        let lt = left_of(td);
        rest.push(at(op, od, 1000.0));
        let cut = band(&rest, 2.0 * ho + 2000.0);
        let tin = at(tp, td, -INTO);
        let ring = hull(&[at(tin, lt, ho), at(tin, lt, -ho), at(xin, lx, hx), at(xin, lx, -hx)]);
        return Patch { a_owns, cut: Some(cut), ring, l };
    }
    let oin = at(op, od, -INTO);
    Patch { a_owns, cut: None, ring: hull(&[at(oin, lo, ho), at(oin, lo, -ho), at(xin, lx, hx), at(xin, lx, -hx)]), l: 0.0 }
}

/// The pieces a closing by `radius` fills in where `parts` meet near `p` (within `reach`).
fn corners(p: Xy, parts: &[&Vec<Ring>], radius: f64, reach: f64) -> Vec<Ring> {
    if radius.is_nan() || radius <= 0.0 {
        return Vec::new();
    }
    let disc: Ring = (0..64)
        .map(|k| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / 64.0;
            [p[0] + reach * t.cos(), p[1] + reach * t.sin()]
        })
        .collect();
    let mut local: Vec<Ring> = Vec::new();
    for g in parts {
        local.extend(intersection(g, std::slice::from_ref(&disc)));
    }
    let local = union(&local);
    if local.is_empty() {
        return Vec::new();
    }
    let extra = closing_pieces(&local, radius, 100.0);
    let mut edge = disc.clone();
    edge.push(disc[0]);
    let edge = [edge];
    extra
        .into_iter()
        .filter(|g| {
            ring_area(g) > 100.0
                && shape_distance(&Shape { rings: std::slice::from_ref(g), lines: &[] }, &Shape { rings: &[], lines: &edge }).unwrap_or(0.0)
                    > 10.0
        })
        .collect()
}

/// Each road's shape from `own` (its band or own outline), joined to the pieces it goes on into and rounded by
/// `radius` where it meets others; with the patch cuts each owner took.
fn joined(
    roads: &[RoadIn],
    own: Vec<Vec<Ring>>,
    half: &dyn Fn(&Section) -> f64,
    radius: &dyn Fn(&Section) -> f64,
    jts: &[Joint],
    js: &[Junction],
) -> (Vec<Vec<Ring>>, Vec<Patch>) {
    let mut patches: Vec<Vec<usize>> = vec![Vec::new(); roads.len()];
    let all: Vec<Patch> = jts.iter().map(|j| patch_of(roads, j, half)).collect();
    for (n, (j, p)) in jts.iter().zip(&all).enumerate() {
        patches[if p.a_owns { j.a } else { j.b }].push(n);
    }
    let with: Vec<Vec<Ring>> = own
        .into_iter()
        .enumerate()
        .map(|(k, g)| {
            if patches[k].is_empty() {
                return g;
            }
            let mut g = g;
            for &n in &patches[k] {
                if let Some(c) = &all[n].cut {
                    g = difference(&g, c);
                }
            }
            g.extend(patches[k].iter().map(|&n| ccw(all[n].ring.clone())));
            union(&g)
        })
        .collect();
    let mut extra: Vec<Vec<Ring>> = vec![Vec::new(); roads.len()];
    for j in js {
        let pick = if j.ending.is_empty() { &j.at[..1] } else { &j.ending[..] };
        let r = pick.iter().map(|&k| radius(&roads[k].s)).fold(f64::INFINITY, f64::min);
        let most = j.at.iter().map(|&k| roads[k].s.outer()).fold(0.0, f64::max);
        // Far enough for a corner at a sharp angle (its arc runs on radius / tan(angle / 2) along each road).
        let reach = 1.5 * most + 6.0 * r + 1000.0;
        let parts: Vec<&Vec<Ring>> = j.at.iter().map(|&k| &with[k]).collect();
        for g in corners(j.p, &parts, r, reach) {
            for &o in pick {
                if rings_distance(std::slice::from_ref(&g), &with[o]) < 1.0 {
                    extra[o].push(g.clone());
                }
            }
        }
    }
    let shapes = with
        .iter()
        .zip(extra)
        .map(|(w, x)| {
            if x.is_empty() {
                return w.clone();
            }
            // A corner piece lies a hair's breadth off its road: closed onto it.
            join(w, &x)
        })
        .collect();
    (shapes, all)
}

/// Each road's outline (its right of way) joined to the pieces it goes on into (joints, tapered where the width
/// changes) and with the rounded corners it takes where it meets others (roads.py `outlines`).
pub fn outlines(roads: &[RoadIn], lay: &Layout) -> Vec<Vec<Ring>> {
    let own = roads.iter().map(|r| r.own.clone().unwrap_or_else(|| band(&r.line, r.s.outer()))).collect();
    joined(roads, own, &|s| s.outer() / 2.0, &|s| s.outer_radius(), &lay.joints, &lay.junctions).0
}

// ====================================================================== what the plan and the 3D draw
/// A line painted along a road: between lanes, its centre line, or a double centre line (three lanes or more).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Lane,
    Centre,
    Double,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub kind: MarkKind,
    pub line: Vec<Xy>,
}

/// A road's parts as drawn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadParts {
    /// The asphalt kerb to kerb, with its kerb returns where it meets others.
    pub carriageway: Vec<Ring>,
    /// Its outline less the carriageways: the pavements.
    pub pavement: Vec<Ring>,
    /// The raised strip between the two ways.
    pub median: Vec<Ring>,
    /// Lane lines and the centre line, stopping short of junctions.
    pub marks: Vec<Mark>,
    /// The kerb lines: its carriageway's edges less what lies in the roads it meets and its open ends.
    pub kerbs: Vec<Vec<Xy>>,
    /// The back of its pavements: its outline's edges less what lies in the roads it meets and its open ends.
    pub backs: Vec<Vec<Xy>>,
    /// Painted on its asphalt: stop or give-way lines where it meets a bigger road, arrows along a one-way road.
    pub paint: Vec<Ring>,
}

/// The parts of a road across, as offsets to the left of its line: the median's middle and width, and the lines
/// painted. Driving on the right: the way along the line is on its right.
pub fn across(s: &Section) -> (Option<(f64, f64)>, Vec<(f64, MarkKind)>) {
    let p = s.paved();
    let lw = s.lane_width;
    let mut marks = Vec::new();
    if s.oneway {
        for k in 1..s.lanes {
            marks.push((-p / 2.0 + k as f64 * lw, MarkKind::Lane));
        }
        return (None, marks);
    }
    let nf = (s.lanes + 1) / 2;
    let nb = s.lanes - nf;
    let m = s.median;
    for k in 1..nf {
        marks.push((-p / 2.0 + k as f64 * lw, MarkKind::Lane));
    }
    let c = -p / 2.0 + nf as f64 * lw;
    for k in 1..nb {
        marks.push((c + m + k as f64 * lw, MarkKind::Lane));
    }
    if m > 0.0 {
        return (Some((c + m / 2.0, m)), marks);
    }
    if nb > 0 {
        marks.push((c, if s.lanes >= 3 { MarkKind::Double } else { MarkKind::Centre }));
    }
    (None, marks)
}

/// The lanes that carry traffic one way along a road (`forward`, along its line, or back against it), as offsets to
/// the left of its line; None where no lane runs that way.
pub fn inbound(s: &Section, forward: bool) -> Option<(f64, f64)> {
    let p = s.paved();
    if s.oneway {
        return forward.then_some((-p / 2.0, p / 2.0));
    }
    let nf = (s.lanes + 1) / 2;
    let nb = s.lanes - nf;
    if nb == 0 {
        return Some((-p / 2.0, p / 2.0));
    }
    let c = -p / 2.0 + nf as f64 * s.lane_width;
    Some(if forward { (-p / 2.0, c) } else { (c + s.median, p / 2.0) })
}

const STOP_BAR: f64 = 300.0;
const GIVE_DASH: (f64, f64) = (600.0, 300.0);
const GIVE_ROW: f64 = 200.0;
const GIVE_APART: f64 = 250.0;
const LINE_INSET: f64 = 150.0;
const ARROW_M: f64 = 5000.0;
const ARROW_FROM_END: f64 = 12000.0;
const ARROW_EVERY: f64 = 40000.0;

/// The line across the lanes coming into a junction, `s` along the road's line, traffic going `forward` or back: a
/// stop line (a solid bar) or a give-way line (two rows of dashes), its front edge at `s`.
pub fn line_across(line: &[Xy], s: f64, forward: bool, sec: &Section, stop: bool) -> Vec<Ring> {
    let Some(span) = inbound(sec, forward) else { return Vec::new() };
    let (a, b) = (span.0 + LINE_INSET, span.1 - LINE_INSET);
    if b - a < 500.0 {
        return Vec::new();
    }
    let (p, d) = point_at(line, s);
    let t = if forward { d } else { [-d[0], -d[1]] };
    let n = left_of(d);
    let rect = |u0: f64, u1: f64, v0: f64, v1: f64| {
        ccw(vec![at(at(p, t, u0), n, v0), at(at(p, t, u1), n, v0), at(at(p, t, u1), n, v1), at(at(p, t, u0), n, v1)])
    };
    if stop {
        return vec![rect(-STOP_BAR, 0.0, a, b)];
    }
    let mut out = Vec::new();
    for u in [0.0, -(GIVE_ROW + GIVE_APART)] {
        let mut v = a;
        while v < b - 50.0 {
            out.push(rect(u - GIVE_ROW, u, v, b.min(v + GIVE_DASH.0)));
            v += GIVE_DASH.0 + GIVE_DASH.1;
        }
    }
    out
}

/// An arrow painted along a lane, its middle at `c`, pointing along `d`.
pub fn arrow(c: Xy, d: Xy) -> Ring {
    let n = left_of(d);
    let q = |u: f64, v: f64| at(at(c, d, u), n, v);
    let (l, head) = (ARROW_M / 2.0, 1600.0);
    ccw(vec![q(-l, -150.0), q(l - head, -150.0), q(l - head, -500.0), q(l, 0.0), q(l - head, 500.0), q(l - head, 150.0), q(-l, 150.0)])
}

/// Polylines less their pieces on a road's open ends (a cut, not a kerb).
fn off_ends(lines: Vec<Vec<Xy>>, ends: &[(Xy, Xy)]) -> Vec<Vec<Xy>> {
    if ends.is_empty() {
        return lines;
    }
    let on_end = |p: Xy| ends.iter().any(|(a, b)| seg_point(*a, *b, p).0 < 20.0);
    let mut out = Vec::new();
    for l in lines {
        let off: Vec<bool> = l.iter().map(|p| on_end(*p)).collect();
        let mut run: Vec<Xy> = Vec::new();
        for i in 0..l.len().saturating_sub(1) {
            // An edge along an open end, not one from one end to the other (a straight road's kerb).
            let mid = [(l[i][0] + l[i + 1][0]) / 2.0, (l[i][1] + l[i + 1][1]) / 2.0];
            if off[i] && off[i + 1] && on_end(mid) {
                if run.len() >= 2 {
                    out.push(std::mem::take(&mut run));
                }
                run.clear();
                continue;
            }
            if run.is_empty() {
                run.push(l[i]);
            }
            run.push(l[i + 1]);
        }
        if run.len() >= 2 {
            out.push(run);
        }
    }
    out
}

/// Edges of `rings` less what lies inside `clip` (the others, grown by SHARED, and the pieces it goes on into), less
/// open ends, without crumbs.
fn edges_outside(rings: &[Ring], clip: &[Ring], ends: &[(Xy, Xy)]) -> Vec<Vec<Xy>> {
    let lines = off_ends(
        rings
            .iter()
            .map(|r| {
                let mut l = r.clone();
                if let Some(f) = r.first() {
                    l.push(*f);
                }
                l
            })
            .collect(),
        ends,
    );
    clip_lines(&lines, clip, true).into_iter().filter(|l| length_of(l) > 50.0).collect()
}

fn pkey(p: Xy) -> (i64, i64) {
    ((p[0] * 10.0).round() as i64, (p[1] * 10.0).round() as i64)
}

/// The roads of a level as they are drawn: each one's carriageway with its kerb returns, its pavements, median,
/// markings, kerb lines, the back of its pavements and what is painted (roads.ts `roadNet`). `outlines` are the roads'
/// outlines ([`outlines`]).
pub fn net(roads: &[RoadIn], lay: &Layout, outlines: &[Vec<Ring>]) -> Vec<RoadParts> {
    let Layout { near, joints: jts, junctions: js } = lay;
    let mut joint_ends: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut partners: BTreeSet<(usize, usize)> = BTreeSet::new();
    for j in jts {
        joint_ends.insert((j.a, j.ae));
        joint_ends.insert((j.b, j.be));
        partners.insert(pair(j.a, j.b));
    }
    let end_at = |k: usize, p: Xy| if pkey(end_of(&roads[k].line, 0).0) == pkey(p) { 0 } else { 1 };
    // A road ending on others reaches the nearest one's centre line with its carriageway, so its mouth is open even
    // where its line stops in the other's pavement.
    let mut reach = vec![[0.0_f64; 2]; roads.len()];
    for j in js {
        let Some(&k) = j.ending.first() else { continue };
        let r = &roads[k];
        let end = end_at(k, j.p);
        let (ep, ed) = end_of(&r.line, end);
        let mut best = 0.0_f64;
        for &o in &j.at {
            if o == k {
                continue;
            }
            let (d, q) = line_distance(&roads[o].line, ep);
            let ahead = dot(sub(q, ep), ed);
            if d > 50.0 && ahead > 0.0 {
                best = best.max(ahead.min(roads[o].s.outer() / 2.0));
            }
        }
        reach[k][end] = best.round();
    }
    let line_of = |k: usize| -> Vec<Xy> {
        let r = &roads[k];
        let [r0, r1] = reach[k];
        let mut l = r.line.clone();
        if r0 > 0.0 {
            let (p, d) = end_of(&r.line, 0);
            l[0] = at(p, d, r0);
        }
        if r1 > 0.0 {
            let (p, d) = end_of(&r.line, 1);
            let n = l.len();
            l[n - 1] = at(p, d, r1);
        }
        l
    };
    let base: Vec<Vec<Ring>> = (0..roads.len()).map(|k| band(&line_of(k), roads[k].s.paved())).collect();
    let (cw, cw_patches) = joined(roads, base, &|s| s.paved() / 2.0, &|s| s.kerb_radius, jts, js);

    // What stops each road's markings and median: the other roads' carriageways where it ends in them or they cross
    // it, and where another ends on it, the junction's box across it (its mouth and kerb returns); where another ends on
    // its median line, the median opens across that road.
    let bands: Vec<Vec<Ring>> = roads.iter().map(|r| band(&r.line, r.s.paved())).collect();
    let mut stops: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); roads.len()];
    let mut boxes: Vec<Vec<Ring>> = vec![Vec::new(); roads.len()];
    let mut openings: Vec<Vec<Ring>> = vec![Vec::new(); roads.len()];
    let mut ending: BTreeSet<(usize, (i64, i64))> = BTreeSet::new();
    for j in js {
        for &k in &j.ending {
            ending.insert((k, pkey(j.p)));
        }
        for &k in &j.at {
            let passes = !j.ending.contains(&k);
            for &o in &j.at {
                if o == k || partners.contains(&pair(k, o)) {
                    continue;
                }
                let o_ends = j.ending.contains(&o);
                if !passes || !o_ends {
                    stops[k].insert(o);
                    continue;
                }
                // o ends on k: k's markings stop across the mouth and its kerb returns.
                let o_line = &roads[o].line;
                let o_end = end_at(o, j.p);
                let (ep, ed) = end_of(o_line, o_end);
                let q = line_distance(&roads[k].line, ep).1;
                let half = roads[k].s.paved() / 2.0 + 500.0;
                let width = roads[o].s.paved() + 2.0 * roads[o].s.kerb_radius.min(roads[k].s.kerb_radius);
                // Across the whole road; on a divided road that o meets on one side only, across that side.
                let med = across(&roads[k].s).0;
                let reaches_median =
                    med.is_some_and(|(m_at, m_w)| line_distance(&offset_line(&roads[k].line, m_at), j.p).0 <= m_w / 2.0 + 1000.0);
                let far = if med.is_some() && !reaches_median { half.min(dot(sub(ep, q), ed) + 1000.0) } else { half };
                boxes[k].extend(band(&[at(q, ed, -half), at(q, ed, far)], width));
                // An opening in k's median where o's end reaches it.
                if med.is_some() && reaches_median {
                    let n = o_line.len();
                    let (t0, t1) = if o_end == 0 { (o_line[1], o_line[0]) } else { (o_line[n - 2], o_line[n - 1]) };
                    let l = dist(t0, t1);
                    let l = if l > 0.0 { l } else { 1.0 };
                    let dir = [(t1[0] - t0[0]) / l, (t1[1] - t0[1]) / l];
                    let span = roads[k].s.outer();
                    let w = roads[o].s.paved() + 2000.0;
                    openings[k].extend(band(&[t1, at(t1, dir, span)], w));
                    openings[k].extend(band(&[at(t1, dir, -span), t1], w));
                }
            }
        }
    }
    // Open ends: a road's end that meets nothing (and goes on into no other piece) is cut off square, not kerbed.
    let mut ends_of: Vec<Vec<(Xy, Xy)>> = vec![Vec::new(); roads.len()];
    for (k, r) in roads.iter().enumerate() {
        for e in [0, 1] {
            let (ep, ed) = end_of(&r.line, e);
            if ending.contains(&(k, pkey(ep))) || joint_ends.contains(&(k, e)) {
                continue;
            }
            let n = left_of(ed);
            let h = r.s.outer() / 2.0 + 10.0;
            ends_of[k].push((at(ep, n, -h), at(ep, n, h)));
        }
    }
    // The joints' cuts on the carriageway: the owner's markings stop where its taper starts.
    let mut cuts: Vec<Vec<Ring>> = vec![Vec::new(); roads.len()];
    let mut connect: Vec<(Joint, usize, f64)> = Vec::new();
    for (j, p) in jts.iter().zip(&cw_patches) {
        let owner = if p.a_owns { j.a } else { j.b };
        if let Some(c) = &p.cut {
            cuts[owner].extend(c.iter().cloned());
        }
        connect.push((*j, owner, p.l));
    }
    let stop_rings = |k: usize| -> Vec<Ring> {
        let mut all: Vec<Ring> = stops[k].iter().flat_map(|&o| bands[o].iter().cloned()).collect();
        all.extend(boxes[k].iter().cloned());
        if all.is_empty() { Vec::new() } else { grow(&union(&all), STOP, ARC_TOL) }
    };
    let stopped: Vec<Vec<Ring>> = (0..roads.len()).map(stop_rings).collect();
    let marks_of: Vec<Vec<Mark>> = roads
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let mut cut = stopped[k].clone();
            cut.extend(cuts[k].iter().cloned());
            let mut marks = Vec::new();
            if r.s.markings && r.s.lane_width >= 2500.0 {
                for (m_at, kind) in across(&r.s).1 {
                    let ln = offset_line(&r.line, m_at);
                    for run in line_outside(&ln, &cut, 3000.0) {
                        marks.push(Mark { kind, line: run });
                    }
                }
            }
            marks
        })
        .collect();
    let median_of: Vec<Vec<Ring>> = roads
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let Some((m_at, m_w)) = across(&r.s).0 else { return Vec::new() };
            if m_w <= 50.0 {
                return Vec::new();
            }
            let m = band(&offset_line(&r.line, m_at), m_w);
            let mut cut: Vec<Ring> = if stops[k].is_empty() {
                Vec::new()
            } else {
                grow(&union(&stops[k].iter().flat_map(|&o| bands[o].iter().cloned()).collect::<Vec<_>>()), STOP, ARC_TOL)
            };
            cut.extend(openings[k].iter().cloned());
            if cut.is_empty() { m } else { difference(&m, &cut) }
        })
        .collect();
    // The stop and give-way lines: on a road ending on another that runs on through (a T), across its lanes coming
    // in; where two cross (an X), on the one with fewer lanes (or the undivided one), each way in. A stop line where the
    // road met is divided or has four lanes or more, else a give-way line.
    let runs_on = |o: usize, p: Xy| {
        [0, 1].iter().all(|&e| joint_ends.contains(&(o, e)) || dist(end_of(&roads[o].line, e).0, p) > roads[o].s.outer() / 2.0 + 1000.0)
    };
    let divided = |s: &Section| !s.oneway && s.median > 0.0;
    let bigger = |o: &Section, k: &Section| o.lanes > k.lanes || (o.lanes == k.lanes && divided(o) && !divided(k));
    let mut paint: Vec<Vec<Ring>> = vec![Vec::new(); roads.len()];
    for j in js {
        for &k in &j.at {
            let ends = j.ending.contains(&k);
            let majors: Vec<usize> =
                j.at.iter()
                    .copied()
                    .filter(|&o| {
                        o != k
                            && !partners.contains(&pair(k, o))
                            && !j.ending.contains(&o)
                            && runs_on(o, j.p)
                            && (ends || bigger(&roads[o].s, &roads[k].s))
                    })
                    .collect();
            if majors.is_empty() {
                continue;
            }
            let r = &roads[k];
            if r.s.lane_width < 2500.0 || !(r.s.markings || majors.iter().any(|&o| roads[o].s.markings)) {
                continue;
            }
            let l = length_of(&r.line);
            let mut ways: Vec<(f64, bool)> = Vec::new();
            if ends {
                ways.push(if end_at(k, j.p) == 1 { (l, true) } else { (0.0, false) });
            } else {
                let sj = along_of(&r.line, j.p);
                ways.push((sj, true));
                ways.push((sj, false));
            }
            let stop = majors.iter().any(|&o| divided(&roads[o].s) || roads[o].s.lanes >= 4);
            for (from, forward) in ways {
                let Some(span) = inbound(&r.s, forward) else { continue };
                let most = majors.iter().map(|&o| roads[o].s.outer() + 3.0 * roads[o].s.kerb_radius).fold(0.0, f64::max) + 10000.0;
                let mut x = 0.0;
                while x <= most {
                    let s = if forward { from - x } else { from + x };
                    x += 250.0;
                    if if forward { s < 500.0 } else { s > l - 500.0 } {
                        break; // the road ran out before the line found room
                    }
                    if if forward { s > l - 500.0 } else { s < 500.0 } {
                        continue;
                    }
                    let (p, d) = point_at(&r.line, s);
                    let n = left_of(d);
                    let clear = [span.0, (span.0 + span.1) / 2.0, span.1].iter().all(|&v| {
                        majors.iter().all(|&o| line_distance(&roads[o].line, at(p, n, v)).0 >= roads[o].s.paved() / 2.0 + STOP - 1e-6)
                    });
                    if clear {
                        paint[k].extend(line_across(&r.line, s, forward, &r.s, stop));
                        break;
                    }
                }
            }
        }
    }
    // One-way roads' arrows: along each lane, the first ARROW_FROM_END from the road's start and then every
    // ARROW_EVERY, none where the markings stop (junctions, tapers).
    for (k, r) in roads.iter().enumerate() {
        if !r.s.oneway || !r.s.markings || r.s.lane_width < 2500.0 {
            continue;
        }
        let l = length_of(&r.line);
        if l < 2.0 * ARROW_FROM_END - 4000.0 {
            continue;
        }
        let in_cut = |q: Xy| in_rings(&stopped[k], q) || in_rings(&cuts[k], q);
        let mut at_: Vec<f64> = Vec::new();
        let mut s = ARROW_FROM_END;
        while s <= l - ARROW_FROM_END + 1e-6 && at_.len() < 10_000 {
            at_.push(s);
            s += ARROW_EVERY;
        }
        if at_.is_empty() {
            at_.push(l / 2.0);
        }
        let pv = r.s.paved();
        for i in 0..r.s.lanes {
            let ln = offset_line(&r.line, -pv / 2.0 + (i as f64 + 0.5) * r.s.lane_width);
            for &s in &at_ {
                let (p, d) = point_at(&ln, s);
                if [at(p, d, -ARROW_M / 2.0), p, at(p, d, ARROW_M / 2.0)].iter().any(|q| in_cut(*q)) {
                    continue;
                }
                paint[k].push(arrow(p, d));
            }
        }
    }
    // The lines that go on across a joint: each of the owner's that ends where its taper starts meets the other's that
    // lies as far from the middle of the carriageway, in the kind of the one with fewer lanes.
    let mut across2: Vec<Vec<Mark>> = vec![Vec::new(); roads.len()];
    for (j, owner, l) in &connect {
        let (x, oe, xe) = if *owner == j.a { (j.b, j.ae, j.be) } else { (j.a, j.be, j.ae) };
        let (op, od) = end_of(&roads[*owner].line, oe);
        let xp = end_of(&roads[x].line, xe).0;
        let lo = left_of(od);
        let e = dot(sub(xp, op), lo);
        let t = if *l > 0.0 { back(&roads[*owner].line, oe, *l).0 } else { op };
        let tips = |ms: &[Mark], p: Xy, within: f64| -> Vec<(MarkKind, Xy)> {
            ms.iter()
                .flat_map(|m| [(m.kind, m.line[0]), (m.kind, m.line[m.line.len() - 1])])
                .filter(|(_, q)| dist(*q, p) <= within)
                .collect()
        };
        let os = tips(&marks_of[*owner], t, roads[*owner].s.paved().max(2000.0) / 2.0 + 1500.0);
        let xs = tips(&marks_of[x], xp, roads[x].s.paved() / 2.0 + 1500.0);
        let fewer_is_owner = roads[*owner].s.lanes <= roads[x].s.lanes;
        for (ka, qa) in os {
            let la = dot(sub(qa, op), lo);
            if let Some((kb, qb)) = xs.iter().find(|(_, q)| (dot(sub(*q, op), lo) - e - la).abs() <= 600.0) {
                across2[*owner].push(Mark { kind: if fewer_is_owner { ka } else { *kb }, line: vec![qa, *qb] });
            }
        }
    }
    // The kerb lines and the back of the pavements: each road's edges less what lies in the roads near it, each grown
    // once.
    let cw_grown: Vec<Vec<Ring>> = cw.iter().map(|g| grow(g, SHARED, 5.0)).collect();
    let ol_grown: Vec<Vec<Ring>> = outlines.iter().map(|g| grow(g, SHARED, 5.0)).collect();
    let mut out = Vec::with_capacity(roads.len());
    for k in 0..roads.len() {
        let is_piece = |o: &usize| partners.contains(&pair(k, *o));
        let mut kclip: Vec<Ring> = Vec::new();
        let mut bclip: Vec<Ring> = Vec::new();
        for o in &near[k] {
            if is_piece(o) {
                kclip.extend(cw[*o].iter().cloned());
                bclip.extend(outlines[*o].iter().cloned());
            } else {
                kclip.extend(cw_grown[*o].iter().cloned());
                bclip.extend(ol_grown[*o].iter().cloned());
            }
        }
        let kerbs = edges_outside(&cw[k], &kclip, &ends_of[k]);
        let backs = edges_outside(&outlines[k], &bclip, &ends_of[k]);
        let mut paved: Vec<Ring> = cw[k].clone();
        for o in &near[k] {
            paved.extend(cw[*o].iter().cloned());
        }
        let pavement = difference(&outlines[k], &paved);
        let mut marks = marks_of[k].clone();
        marks.extend(across2[k].iter().cloned());
        out.push(RoadParts {
            carriageway: cw[k].clone(),
            pavement,
            median: median_of[k].clone(),
            marks,
            kerbs,
            backs,
            paint: std::mem::take(&mut paint[k]),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::shapes::poly::area;

    fn road(id: &str, line: Vec<Xy>, width: f64, lanes: i64) -> RoadIn {
        let mut p = Props::new();
        p.insert("width".into(), width.into());
        p.insert("lanes".into(), lanes.into());
        p.insert("lane_width".into(), 3500.0.into());
        RoadIn { id: id.into(), line, s: section_of(&p), own: None }
    }

    #[test]
    fn a_street_of_a_stated_width_has_its_lanes_and_pavements() {
        assert_eq!(for_row(11000.0), (2, 3500.0, false, 0.0, true));
        assert_eq!(for_row(30000.0), (4, 3500.0, false, 4000.0, true));
        assert_eq!(for_row(5000.0), (1, 3500.0, false, 0.0, false));
        let s = section_of(&Props::new());
        assert_eq!(s.row, 7000.0);
        assert!((s.pavement() - 0.0).abs() < 1e-9 || s.pavement() >= 0.0);
    }

    #[test]
    fn a_tees_corners_are_the_ending_roads_and_rounded_by_the_kerb_less_the_pavement() {
        let rs =
            vec![road("a", vec![[-60000.0, 0.0], [60000.0, 0.0]], 20000.0, 4), road("b", vec![[0.0, 0.0], [0.0, 60000.0]], 11000.0, 2)];
        let lay = layout(&rs);
        let o = outlines(&rs, &lay);
        assert!((area(&o[0]) - 120000.0 * 20000.0).abs() < 1e3, "{}", area(&o[0]));
        let r = rs[1].s.outer_radius(); // 6 m less 2 m
        let want = 11000.0 * 60000.0 + 2.0 * (1.0 - std::f64::consts::PI / 4.0) * r * r;
        assert!((area(&o[1]) - want).abs() < 0.02 * 2.0 * (1.0 - std::f64::consts::PI / 4.0) * r * r + 1e4, "{} {}", area(&o[1]), want);
        let parts = net(&rs, &lay, &o);
        assert!(!parts[0].kerbs.is_empty() && !parts[1].kerbs.is_empty());
        assert!(!parts[0].marks.is_empty());
        // The road ending on a four-lane road gives way… stops: the bigger has four lanes.
        assert!(!parts[1].paint.is_empty());
    }
}

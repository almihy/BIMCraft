//! Sections of a solid by any plane: the plane's loops, each edge with the origin of the input face it came from, and the
//! edges of curved faces recovered as exact arcs.
//!
//! Why the triangles are cut here and not by Manifold's own `slice` (which gives polygons): Manifold's polygons carry no
//! ids, so an edge could not be traced to its input. Cutting the surface's triangles against the plane gives each edge its
//! triangle and so its origin; and every vertex is classified once (above the plane or not), so two triangles sharing an
//! edge always agree where it is cut, and the pieces join end to end into closed loops exactly, with no tolerance to guess.
//! The cut of an edge is computed once per edge, from its end with the lower index, so neighbours get the same bits.

use std::collections::HashMap;

use super::{Curve, Origin, Solid3, tol};
use crate::geom::P2;
use crate::geom::v3::{P3, cross, dot, sub};

/// A plane to cut by, with the axes its loops are drawn in: the loops are in `x` along `u` and `y` along `v`, measured from
/// `origin`. `u` and `v` are unit and square to each other, and the plane's normal `u × v` points at the viewer: **the
/// view looks along `-normal`, with `u` to the right and `v` up**. [`Frame::plan`] looks down on the plan; [`Frame::vertical`]
/// looks along a horizontal direction.
///
/// A plane through faces of the solid (a floor's top, a wall's face) cuts just behind itself, away from the viewer: the
/// solid behind a plane it touches is cut at its face, the solid in front of it is not.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    origin: P3,
    u: P3,
    v: P3,
}

impl Frame {
    /// The frame with `origin` and the axes `u` and `v` (made unit and square: `v` loses its part along `u`). None when either is
    /// zero or they are parallel.
    pub fn new(origin: P3, u: P3, v: P3) -> Option<Frame> {
        let unit = |a: P3| {
            let l = dot(a, a).sqrt();
            (l > 1e-12 && l.is_finite()).then(|| a.map(|c| c / l))
        };
        let u = unit(u)?;
        let k = dot(v, u);
        let v = unit([v[0] - k * u[0], v[1] - k * u[1], v[2] - k * u[2]])?;
        Some(Frame { origin, u, v })
    }

    /// The plan at height `z`, seen from above: `x` east, `y` north, the model's own coordinates.
    pub fn plan(z: f64) -> Frame {
        Frame { origin: [0.0, 0.0, z], u: [1.0, 0.0, 0.0], v: [0.0, 1.0, 0.0] }
    }

    /// The vertical plane through `through` along `direction` (in plan): `x` along `direction`, measured from `through`, `y` up
    /// from z = 0, looking along the plan direction turned 90 degrees clockwise from `direction` (for `direction` east, looking
    /// north: a south elevation). None for a zero direction.
    pub fn vertical(through: P2, direction: P2) -> Option<Frame> {
        Frame::new([through.0, through.1, 0.0], [direction.0, direction.1, 0.0], [0.0, 0.0, 1.0])
    }

    pub fn origin(&self) -> P3 {
        self.origin
    }

    pub fn u(&self) -> P3 {
        self.u
    }

    pub fn v(&self) -> P3 {
        self.v
    }

    /// `u × v`, towards the viewer.
    pub fn normal(&self) -> P3 {
        cross(self.u, self.v)
    }

    /// A model point as (x, y, height above the plane towards the viewer).
    pub fn to_local(&self, p: P3) -> [f64; 3] {
        let d = sub(p, self.origin);
        [dot(d, self.u), dot(d, self.v), dot(d, self.normal())]
    }

    /// The model point at `q` on the plane.
    pub fn to_world(&self, q: P2) -> P3 {
        [0, 1, 2].map(|i| self.origin[i] + q.0 * self.u[i] + q.1 * self.v[i])
    }

    /// Whether the plane is level (its normal vertical, either way): the cut of a vertical cylinder is then a circle.
    pub fn is_horizontal(&self) -> bool {
        let n = self.normal();
        n[0].abs() < 1e-12 && n[1].abs() < 1e-12
    }
}

/// One edge of a section loop, in the frame's `(x, y)`, with the input it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line {
        a: P2,
        b: P2,
        origin: Origin,
    },
    /// An arc of the circle about `centre`, exactly of `radius`, from `start` to `end` (both on the circle, to rounding),
    /// counter-clockwise when `ccw`. Never a whole circle: a full circle comes back as two arcs.
    Arc {
        centre: P2,
        radius: f64,
        start: P2,
        end: P2,
        ccw: bool,
        origin: Origin,
    },
}

impl Seg {
    pub fn origin(&self) -> Origin {
        match self {
            Seg::Line { origin, .. } | Seg::Arc { origin, .. } => *origin,
        }
    }

    pub fn start(&self) -> P2 {
        match self {
            Seg::Line { a, .. } => *a,
            Seg::Arc { start, .. } => *start,
        }
    }

    pub fn end(&self) -> P2 {
        match self {
            Seg::Line { b, .. } => *b,
            Seg::Arc { end, .. } => *end,
        }
    }

    /// An arc's signed sweep, radians: positive counter-clockwise, in (-2π, 0) or (0, 2π].
    pub fn sweep(&self) -> Option<f64> {
        match self {
            Seg::Line { .. } => None,
            Seg::Arc { centre, start, end, ccw, .. } => Some(sweep_of(*centre, *start, *end, *ccw)),
        }
    }
}

fn sweep_of(c: P2, a: P2, b: P2, ccw: bool) -> f64 {
    let tau = std::f64::consts::TAU;
    let (ta, tb) = ((a.1 - c.1).atan2(a.0 - c.0), (b.1 - c.1).atan2(b.0 - c.0));
    let s = (tb - ta).rem_euclid(tau);
    if ccw {
        if s == 0.0 { tau } else { s }
    } else if s == 0.0 {
        -tau
    } else {
        s - tau
    }
}

/// A closed loop of a section, its edges end to end (each starts where the one before ends, the first where the last ends).
/// The solid is on its left: an outer loop runs counter-clockwise, a hole clockwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub segs: Vec<Seg>,
}

impl Loop {
    /// The signed area it encloses, arcs exact: positive for an outer loop, negative for a hole.
    pub fn area(&self) -> f64 {
        let mut twice = 0.0;
        for s in &self.segs {
            twice += match s {
                Seg::Line { a, b, .. } => a.cross(*b),
                Seg::Arc { centre: c, radius: r, start, .. } => {
                    let sweep = s.sweep().unwrap_or(0.0);
                    let t0 = (start.1 - c.1).atan2(start.0 - c.0);
                    let t1 = t0 + sweep;
                    r * r * sweep + r * (c.0 * (t1.sin() - t0.sin()) - c.1 * (t1.cos() - t0.cos()))
                }
            };
        }
        twice / 2.0
    }

    /// Whether it bounds a hole (runs clockwise).
    pub fn is_hole(&self) -> bool {
        self.area() < 0.0
    }
}

/// A straight piece of a loop, before arcs are found.
#[derive(Clone, Copy)]
struct Line {
    a: P2,
    b: P2,
    origin: Origin,
}

impl Solid3 {
    /// The loops where the plane of `frame` cuts the solid, in the frame's axes. Each edge has the origin of the input face it
    /// lies on; collinear edges of one origin are one line; where an input's curve is tagged and the plane is level, the
    /// edges on it are exact arcs (see [`Curve`]). Nothing for an empty solid or a plane that misses it.
    pub fn slice(&self, frame: &Frame) -> Vec<Loop> {
        let mesh = self.surface();
        let local: Vec<[f64; 3]> = mesh.verts.iter().map(|p| frame.to_local(*p)).collect();
        // A vertex exactly on the plane counts as above it: the plane cuts a hair behind itself.
        let above: Vec<bool> = local.iter().map(|l| l[2] >= 0.0).collect();

        // Where each crossed edge is cut, once, from its lower-indexed end.
        let mut cut_of: HashMap<(u32, u32), u32> = HashMap::new();
        let mut points: Vec<P2> = Vec::new();
        let mut cut = |a: u32, b: u32| -> u32 {
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            *cut_of.entry((lo, hi)).or_insert_with(|| {
                let (p, q) = (local[lo as usize], local[hi as usize]);
                let t = p[2] / (p[2] - q[2]);
                points.push(P2(p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t));
                (points.len() - 1) as u32
            })
        };
        // Each crossed triangle gives one piece, from where its surface goes down through the plane to where it comes up (so
        // the solid is on the piece's left); `from` and `to` are the cut edges, which a neighbour shares the other way round.
        struct Piece {
            from: u32,
            to: u32,
            origin: Origin,
        }
        let mut pieces: Vec<Piece> = Vec::new();
        for (t, origin) in mesh.tris.iter().zip(&mesh.origins) {
            let ups = [0, 1, 2].map(|k| above[t[(k + 1) % 3] as usize]);
            let downs = [0, 1, 2].map(|k| above[t[k] as usize]);
            let (mut down, mut up) = (None, None);
            for k in 0..3 {
                if downs[k] == ups[k] {
                    continue;
                }
                let id = cut(t[k], t[(k + 1) % 3]);
                if ups[k] {
                    up = Some(id);
                } else {
                    down = Some(id);
                }
            }
            if let (Some(from), Some(to)) = (down, up) {
                pieces.push(Piece { from, to, origin: *origin });
            }
        }

        // Joined end to end.
        let mut start_of: HashMap<u32, usize> = HashMap::with_capacity(pieces.len());
        for (i, p) in pieces.iter().enumerate() {
            start_of.entry(p.from).or_insert(i);
        }
        let mut seen = vec![false; pieces.len()];
        let mut loops = Vec::new();
        for first in 0..pieces.len() {
            if seen[first] {
                continue;
            }
            let mut chain = vec![first];
            seen[first] = true;
            let closed = loop {
                let Some(&next) = start_of.get(&pieces[chain[chain.len() - 1]].to) else { break false };
                if next == first {
                    break true;
                }
                if seen[next] {
                    break false;
                }
                seen[next] = true;
                chain.push(next);
            };
            if !closed {
                continue;
            }
            let raw: Vec<Line> = chain
                .iter()
                .map(|&i| Line { a: points[pieces[i].from as usize], b: points[pieces[i].to as usize], origin: pieces[i].origin })
                .collect();
            if let Some(l) = finish(raw, frame) {
                loops.push(l);
            }
        }
        loops
    }
}

/// A loop's raw pieces made into its edges: no edge of no length, collinear ones of one origin joined, curved ones arcs.
fn finish(mut raw: Vec<Line>, frame: &Frame) -> Option<Loop> {
    raw.retain(|s| s.a.dist(s.b) > tol::ZERO_LENGTH);
    let n = raw.len();
    if n < 2 {
        return None;
    }
    // Ends meet exactly where a piece of no length was dropped between them.
    for i in 0..n {
        raw[i].b = raw[(i + 1) % n].a;
    }
    let lines = join_collinear(raw);
    let segs = if frame.is_horizontal() { find_arcs(&lines, frame) } else { lines.iter().map(line_seg).collect() };
    (segs.len() >= 2).then_some(Loop { segs })
}

fn line_seg(l: &Line) -> Seg {
    Seg::Line { a: l.a, b: l.b, origin: l.origin }
}

/// Consecutive pieces of one origin on one line made one.
fn join_collinear(raw: Vec<Line>) -> Vec<Line> {
    let n = raw.len();
    let joinable = |p: &Line, q: &Line| {
        let (d, e) = (p.b - p.a, q.b - p.a);
        p.origin == q.origin && e.length() > 0.0 && d.cross(e).abs() / e.length() <= tol::LINE && d.dot(q.b - p.b) > 0.0
    };
    // From a piece that does not join the one before it, so that no run is split by where the loop starts.
    let Some(s) = (0..n).find(|&i| !joinable(&raw[(i + n - 1) % n], &raw[i])) else { return raw };
    let mut out: Vec<Line> = Vec::with_capacity(n);
    for k in 0..n {
        let q = raw[(s + k) % n];
        match out.last_mut() {
            Some(p) if joinable(p, &q) => p.b = q.b,
            _ => out.push(q),
        }
    }
    out
}

/// The radius of the tagged cylinder a straight piece is a facet of (or a cut part of one), in the plane's `(x, y)`: the
/// piece lies on a chord of that circle, within it, and the chord is no more than [`tol::MAX_FACET_ANGLE`] across.
fn facet_radius(l: &Line, c: P2, curve: &Curve) -> Option<f64> {
    let len = l.a.dist(l.b);
    let far = l.a.dist(c).max(l.b.dist(c));
    let d_line = (l.b - l.a).cross(c - l.a).abs() / len;
    let least = (tol::MAX_FACET_ANGLE / 2.0).cos();
    let mut best: Option<f64> = None;
    for r in curve.radii {
        if r <= 0.0 || far > r + tol::CURVE || d_line > r + tol::CURVE || d_line < r * least - tol::CURVE {
            continue;
        }
        if best.is_none_or(|b| (far - r).abs() < (far - b).abs()) {
            best = Some(r);
        }
    }
    best
}

/// An arc being found: the pieces `from..=to` (of the rotated loop) on a circle.
struct Run {
    from: usize,
    to: usize,
    radius: f64,
    centre: P2,
    sweep: f64,
}

enum Item {
    Line(Line),
    Arc(Run),
}

/// The point on the circle (`c`, `r`) that `p`, an end of an arc found among facets, becomes: where `line`, the straight edge
/// that goes on from there, meets the circle, when that is near; else straight out from the centre.
fn snap(p: P2, c: P2, r: f64, line: Option<&Line>, cap: f64) -> P2 {
    if let Some(l) = line {
        let (d, f) = (l.b - l.a, l.a - c);
        let (a, b, k) = (d.dot(d), 2.0 * d.dot(f), f.dot(f) - r * r);
        let disc = b * b - 4.0 * a * k;
        if a > 0.0 && disc >= 0.0 {
            let root = disc.sqrt();
            let hit = [(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)].map(|t| l.a + d * t);
            let near = if hit[0].dist(p) <= hit[1].dist(p) { hit[0] } else { hit[1] };
            if near.dist(p) <= cap {
                return near;
            }
        }
    }
    let d = p.dist(c);
    if d < 1e-12 { p } else { c + (p - c) * (r / d) }
}

/// The loop's edges with the curved ones made arcs.
fn find_arcs(lines: &[Line], frame: &Frame) -> Vec<Seg> {
    let n = lines.len();
    let radius_of: Vec<Option<(f64, P2)>> = lines
        .iter()
        .map(|l| {
            let curve = l.origin.curve?;
            let c = frame.to_local([curve.centre.0, curve.centre.1, frame.origin()[2]]);
            let c = P2(c[0], c[1]);
            facet_radius(l, c, &curve).map(|r| (r, c))
        })
        .collect();
    if radius_of.iter().all(Option::is_none) {
        return lines.iter().map(line_seg).collect();
    }
    let turn = |l: &Line, c: P2| (l.a - c).cross(l.b - c).signum();
    let same_run = |i: usize, j: usize| match (radius_of[i], radius_of[j]) {
        (Some((ri, ci)), Some((rj, cj))) => {
            ri == rj && ci == cj && lines[i].origin == lines[j].origin && turn(&lines[i], ci) == turn(&lines[j], cj)
        }
        _ => false,
    };

    // A circle all round: two arcs, split at corner 0 and the corner half way.
    if (0..n).all(|i| same_run(i, (i + 1) % n)) {
        let (r, c) = radius_of[0].unwrap_or((0.0, P2::default()));
        let half = n / 2;
        let sweep_of_pieces = |range: std::ops::Range<usize>| -> f64 {
            range
                .map(|i| {
                    let (a, b) = (lines[i % n].a - c, lines[i % n].b - c);
                    a.cross(b).atan2(a.dot(b))
                })
                .sum()
        };
        let (p, q) = (snap(lines[0].a, c, r, None, 0.0), snap(lines[half].a, c, r, None, 0.0));
        let origin = lines[0].origin;
        let (s1, s2) = (sweep_of_pieces(0..half), sweep_of_pieces(half..n));
        return vec![
            Seg::Arc { centre: c, radius: r, start: p, end: q, ccw: s1 > 0.0, origin },
            Seg::Arc { centre: c, radius: r, start: q, end: p, ccw: s2 > 0.0, origin },
        ];
    }

    // Runs of facets on one circle, from a piece that does not go on from the one before it.
    let Some(s) = (0..n).find(|&i| !same_run(i, (i + n - 1) % n)) else { return lines.iter().map(line_seg).collect() };
    let mut items: Vec<Item> = Vec::new();
    let mut k = 0;
    while k < n {
        let i = (s + k) % n;
        let Some((radius, centre)) = radius_of[i] else {
            items.push(Item::Line(lines[i]));
            k += 1;
            continue;
        };
        let mut last = k;
        while last + 1 < n && same_run(i, (s + last + 1) % n) && same_run((s + last) % n, (s + last + 1) % n) {
            last += 1;
        }
        let sweep = (k..=last)
            .map(|j| {
                let l = &lines[(s + j) % n];
                let (a, b) = (l.a - centre, l.b - centre);
                a.cross(b).atan2(a.dot(b))
            })
            .sum();
        items.push(Item::Arc(Run { from: k, to: last, radius, centre, sweep }));
        k = last + 1;
    }

    // The arcs' ends on the circle, along the straight edge that goes on from them; those edges' ends follow.
    let m = items.len();
    let straight = |j: usize| match &items[j % m] {
        Item::Line(l) => Some(*l),
        Item::Arc(_) => None,
    };
    let mut ends: Vec<Option<(P2, P2)>> = vec![None; m];
    for (j, item) in items.iter().enumerate() {
        let Item::Arc(run) = item else { continue };
        let (first, last) = (&lines[(s + run.from) % n], &lines[(s + run.to) % n]);
        let sag = |l: &Line| (run.radius - (l.b - l.a).cross(run.centre - l.a).abs() / l.a.dist(l.b)).max(0.0);
        let start = snap(first.a, run.centre, run.radius, straight(j + m - 1).as_ref(), 2.0 * sag(first) + tol::CURVE);
        let end = snap(last.b, run.centre, run.radius, straight(j + 1).as_ref(), 2.0 * sag(last) + tol::CURVE);
        ends[j] = Some((start, end));
    }
    let mut out = Vec::with_capacity(m);
    for (j, item) in items.iter().enumerate() {
        match item {
            Item::Arc(run) => {
                let Some((start, end)) = ends[j] else { continue };
                let origin = lines[(s + run.from) % n].origin;
                out.push(Seg::Arc { centre: run.centre, radius: run.radius, start, end, ccw: run.sweep > 0.0, origin });
            }
            Item::Line(l) => {
                let a = ends[(j + m - 1) % m].map_or(l.a, |e| e.1);
                let b = ends[(j + 1) % m].map_or(l.b, |e| e.0);
                out.push(Seg::Line { a, b, origin: l.origin });
            }
        }
    }
    out
}

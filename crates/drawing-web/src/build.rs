//! `build`: what a clean plan draws, as the building elements a model is made of (Arqiflow's drawing reader hands
//! over clean drawings whose layers say what each item is: `A-WALL`, `A-DOOR`, `A-WIND`, `A-COLS`, `A-ROOM`...).
//!
//! - **Walls**: the two faces of a wall are parallel lines a wall's thickness apart. Faces are paired, the nearest
//!   first and each stretch of a face once; the pieces along one line are joined across the wall that meets them
//!   (a face runs on past the gap) and across an opening a door or a window stands in; ends are brought to the
//!   centre line of the wall they meet. Concentric arcs pair the same way.
//! - **Doors and windows**: what stands in a gap of a wall is hosted there, as wide as the gap; a door elsewhere
//!   goes to the nearest wall.
//! - **Glazing runs**: window lines no wall holds (a storefront between columns, a curved glass wall), as curtain
//!   walls along them.
//! - **Columns**: each as its centre, its size and its angle. **Rooms**: each label as a point with its name.
//!
//! Everything is answered in real millimetres: `k` (1 unless given) is how many real millimetres one of the
//! drawing's millimetres is, where the unit its file names is not the one it is drawn in. What could not be made
//! is answered too (`left`), with where it is.

use std::collections::HashMap;

use cadcraft_index::{Opened, Shape};
use serde_json::{Value, json};

/// The most lines and shapes read (a clean plan has a few thousand).
const MOST: usize = 400_000;
/// A wall's thickness, between.
const WALL_MIN: f64 = 40.0;
const WALL_MAX: f64 = 620.0;
/// A glazing run's two lines, between.
const GLASS_MIN: f64 = 10.0;
const GLASS_MAX: f64 = 350.0;
/// Lines within this many degrees are parallel.
const PARALLEL: f64 = 1.0;
/// Pieces whose centre lines and thicknesses are this near are of one wall.
const SAME: f64 = 15.0;
/// The widest opening a door or a window is taken to fill.
const OPENING_MAX: f64 = 6000.0;
/// The shortest glazing run kept.
const RUN_MIN: f64 = 800.0;
/// The most of each list of what was left.
const LEFT_LISTED: usize = 60;

#[derive(Clone, Copy, Debug)]
struct P(f64, f64);

impl P {
    fn dot(self, o: P) -> f64 {
        self.0 * o.0 + self.1 * o.1
    }
    fn sub(self, o: P) -> P {
        P(self.0 - o.0, self.1 - o.1)
    }
    fn len(self) -> f64 {
        self.0.hypot(self.1)
    }
}

#[derive(Clone, Copy, Debug)]
struct Seg {
    a: P,
    b: P,
}

/// An arc, counter-clockwise from `s` over `sweep` degrees.
#[derive(Clone, Copy, Debug)]
struct Arc {
    c: P,
    r: f64,
    s: f64,
    sweep: f64,
}

/// What one item of the drawing (a placement with all its block draws, or a loose line) draws.
#[derive(Default, Clone)]
struct Group {
    segs: Vec<Seg>,
    arcs: Vec<Arc>,
    circles: Vec<(P, f64)>,
    lo: Option<P>,
    hi: Option<P>,
}

impl Group {
    fn grow(&mut self, p: P) {
        self.lo = Some(self.lo.map_or(p, |l| P(l.0.min(p.0), l.1.min(p.1))));
        self.hi = Some(self.hi.map_or(p, |h| P(h.0.max(p.0), h.1.max(p.1))));
    }
    fn seg(&mut self, a: P, b: P) {
        self.grow(a);
        self.grow(b);
        if a.sub(b).len() > 1.0 {
            self.segs.push(Seg { a, b });
        }
    }
    fn arc(&mut self, a: Arc) {
        for q in arc_points(&a, 8) {
            self.grow(q);
        }
        self.arcs.push(a);
    }
    fn circle(&mut self, c: P, r: f64) {
        self.grow(P(c.0 - r, c.1 - r));
        self.grow(P(c.0 + r, c.1 + r));
        self.circles.push((c, r));
    }
    fn bounds(&self) -> Option<(P, P)> {
        Some((self.lo?, self.hi?))
    }
    fn centre(&self) -> Option<P> {
        let (l, h) = self.bounds()?;
        Some(P((l.0 + h.0) / 2.0, (l.1 + h.1) / 2.0))
    }
    fn join(&mut self, o: &Group) {
        self.segs.extend(o.segs.iter().copied());
        self.arcs.extend(o.arcs.iter().copied());
        self.circles.extend(o.circles.iter().copied());
        if let Some((l, h)) = o.bounds() {
            self.grow(l);
            self.grow(h);
        }
    }
    /// Its corners and the ends of what it draws: what its extent along any line is measured from.
    fn points(&self) -> Vec<P> {
        let mut out: Vec<P> = self.segs.iter().flat_map(|s| [s.a, s.b]).collect();
        for a in &self.arcs {
            out.extend(arc_points(a, 8));
        }
        for (c, r) in &self.circles {
            out.extend([P(c.0 - r, c.1), P(c.0 + r, c.1), P(c.0, c.1 - r), P(c.0, c.1 + r)]);
        }
        out
    }
}

fn arc_point(a: &Arc, deg: f64) -> P {
    let t = deg.to_radians();
    P(a.c.0 + a.r * t.cos(), a.c.1 + a.r * t.sin())
}

fn arc_points(a: &Arc, n: usize) -> Vec<P> {
    (0..=n).map(|i| arc_point(a, a.s + a.sweep * i as f64 / n as f64)).collect()
}

fn add(g: &mut Group, s: &Shape, k: f64) {
    let p = |q: &[f64; 2]| P(q[0] * k, q[1] * k);
    match s {
        Shape::Line(a, b) => g.seg(p(a), p(b)),
        Shape::Polyline { points, closed } => {
            for w in points.windows(2) {
                g.seg(p(&w[0]), p(&w[1]));
            }
            if *closed && points.len() > 2 {
                if let (Some(a), Some(b)) = (points.last(), points.first()) {
                    g.seg(p(a), p(b));
                }
            }
        }
        Shape::Circle { c, r } => g.circle(p(c), r * k),
        Shape::Arc { c, r, start, end } => {
            let sweep = (end - start).rem_euclid(360.0);
            g.arc(Arc { c: p(c), r: r * k, s: *start, sweep: if sweep < 1e-9 { 360.0 } else { sweep } });
        }
        Shape::Fill { rings, .. } => {
            for ring in rings {
                for w in ring.windows(2) {
                    g.seg(p(&w[0]), p(&w[1]));
                }
                if ring.len() > 2 {
                    if let (Some(a), Some(b)) = (ring.last(), ring.first()) {
                        g.seg(p(a), p(b));
                    }
                }
            }
        }
        Shape::Text { .. } => {}
    }
}

// ---------------------------------------------------------------------------------------------- pairing faces
/// A set of parallel lines in one frame: `u` along them, `n` across.
struct Cluster {
    u: P,
    n: P,
    /// Each line: how far across it lies, and from where to where along.
    faces: Vec<(f64, f64, f64)>,
}

fn clusters(segs: &[Seg]) -> Vec<Cluster> {
    let mut by: Vec<(f64, Seg)> = segs
        .iter()
        .map(|s| {
            let d = s.b.sub(s.a);
            let mut th = d.1.atan2(d.0).to_degrees().rem_euclid(180.0);
            if th > 180.0 - PARALLEL / 2.0 {
                th -= 180.0;
            }
            (th, *s)
        })
        .collect();
    by.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<Cluster> = Vec::new();
    let mut first = f64::NEG_INFINITY;
    for (th, s) in by {
        if th - first > PARALLEL {
            first = th;
            let t = th.to_radians();
            out.push(Cluster { u: P(t.cos(), t.sin()), n: P(-t.sin(), t.cos()), faces: Vec::new() });
        }
        if let Some(c) = out.last_mut() {
            let mid = P((s.a.0 + s.b.0) / 2.0, (s.a.1 + s.b.1) / 2.0);
            let (t0, t1) = (s.a.dot(c.u), s.b.dot(c.u));
            c.faces.push((mid.dot(c.n), t0.min(t1), t0.max(t1)));
        }
    }
    for c in &mut out {
        c.faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    out
}

/// What is left of `lo..hi` outside `used` (sorted or not).
fn free(lo: f64, hi: f64, used: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut cuts: Vec<(f64, f64)> = used.iter().copied().filter(|u| u.1 > lo && u.0 < hi).collect();
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut at = lo;
    for (a, b) in cuts {
        if a > at {
            out.push((at, a));
        }
        at = at.max(b);
    }
    if hi > at {
        out.push((at, hi));
    }
    out
}

/// A stretch of wall (or of glass) in a cluster's frame: its centre line `d` across, its thickness, from `lo` to `hi`.
#[derive(Clone, Copy, Debug)]
struct Piece {
    d: f64,
    g: f64,
    lo: f64,
    hi: f64,
}

/// The faces of a cluster paired, the nearest first, each stretch of a face once. (the pieces, each face's used stretches)
fn paired(c: &Cluster, min: f64, max: f64) -> (Vec<Piece>, Vec<Vec<(f64, f64)>>) {
    let f = &c.faces;
    let mut cands: Vec<(f64, usize, usize, f64, f64)> = Vec::new();
    for i in 0..f.len() {
        for j in i + 1..f.len() {
            let g = f[j].0 - f[i].0;
            if g > max {
                break;
            }
            if g < min {
                continue;
            }
            let (lo, hi) = (f[i].1.max(f[j].1), f[i].2.min(f[j].2));
            if hi - lo >= g.max(100.0) {
                cands.push((g, i, j, lo, hi));
            }
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used: Vec<Vec<(f64, f64)>> = vec![Vec::new(); f.len()];
    let mut out = Vec::new();
    for (g, i, j, lo, hi) in cands {
        let both: Vec<(f64, f64)> = used[i].iter().chain(used[j].iter()).copied().collect();
        for (a, b) in free(lo, hi, &both) {
            if b - a >= g.max(100.0) {
                out.push(Piece { d: (f[i].0 + f[j].0) / 2.0, g, lo: a, hi: b });
                used[i].push((a, b));
                used[j].push((a, b));
            }
        }
    }
    (out, used)
}

// ---------------------------------------------------------------------------------------------- walls
/// A door or a window as a wall's frame sees it: along from `lo` to `hi`, across from `near` to `far`.
struct InFrame {
    n: usize,
    lo: f64,
    hi: f64,
    near: f64,
    far: f64,
}

fn in_frame(c: &Cluster, groups: &[Group]) -> Vec<InFrame> {
    groups
        .iter()
        .enumerate()
        .filter_map(|(n, g)| {
            let pts = g.points();
            if pts.is_empty() {
                return None;
            }
            let (mut lo, mut hi, mut near, mut far) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
            for p in pts {
                let (t, d) = (p.dot(c.u), p.dot(c.n));
                lo = lo.min(t);
                hi = hi.max(t);
                near = near.min(d);
                far = far.max(d);
            }
            Some(InFrame { n, lo, hi, near, far })
        })
        .collect()
}

/// An opening of a wall: from where to where along it, and the doors or windows standing in it.
#[derive(Clone, Debug)]
struct Opening {
    lo: f64,
    hi: f64,
    door: bool,
    items: Vec<usize>,
}

struct Wall {
    a: P,
    b: P,
    g: f64,
    openings: Vec<(P, f64, bool, Vec<usize>)>,
}

/// The items of `all` standing in the gap `lo..hi` of a wall at `d`, `g` thick.
fn standing(all: &[InFrame], taken: &[bool], d: f64, g: f64, lo: f64, hi: f64) -> Vec<usize> {
    all.iter()
        .filter(|x| !taken[x.n])
        .filter(|x| x.near <= d + g / 2.0 + 50.0 && x.far >= d - g / 2.0 - 50.0)
        .filter(|x| {
            let over = x.hi.min(hi) - x.lo.max(lo);
            let own = (x.hi - x.lo).max(1.0);
            over >= 0.5 * own.min(hi - lo)
        })
        .map(|x| x.n)
        .collect()
}

struct Found {
    walls: Vec<Wall>,
    /// Stretches of face no other face pairs with.
    lone: Vec<Seg>,
    /// Gaps along a wall's line that nothing stands in.
    gaps: Vec<Seg>,
}

fn walls(segs: &[Seg], doors: &[Group], windows: &[Group], door_taken: &mut [bool], window_taken: &mut [bool]) -> Found {
    let mut found = Found { walls: Vec::new(), lone: Vec::new(), gaps: Vec::new() };
    for c in clusters(segs) {
        let at = |t: f64, d: f64| P(c.u.0 * t + c.n.0 * d, c.u.1 * t + c.n.1 * d);
        let (mut pieces, used) = paired(&c, WALL_MIN, WALL_MAX);
        for (f, u) in c.faces.iter().zip(&used) {
            for (a, b) in free(f.1, f.2, u) {
                if b - a >= 300.0 {
                    found.lone.push(Seg { a: at(a, f.0), b: at(b, f.0) });
                }
            }
        }
        let (ds, ws) = (in_frame(&c, doors), in_frame(&c, windows));
        // The pieces along one line, in order.
        pieces.sort_by(|a, b| a.d.total_cmp(&b.d));
        let mut lines: Vec<Vec<Piece>> = Vec::new();
        for p in pieces {
            match lines.iter_mut().rev().take(8).find(|l| l.first().is_some_and(|q| (q.d - p.d).abs() < SAME && (q.g - p.g).abs() < SAME)) {
                Some(l) => l.push(p),
                None => lines.push(vec![p]),
            }
        }
        for mut line in lines {
            line.sort_by(|a, b| a.lo.total_cmp(&b.lo));
            let Some(&first) = line.first() else { continue };
            let (d, g) = (first.d, first.g);
            let mut run = (first.lo, first.hi);
            let mut openings: Vec<Opening> = Vec::new();
            let close = |run: (f64, f64), openings: &mut Vec<Opening>, found: &mut Found| {
                found.walls.push(Wall {
                    a: at(run.0, d),
                    b: at(run.1, d),
                    g,
                    openings: openings.drain(..).map(|o| (at((o.lo + o.hi) / 2.0, d), o.hi - o.lo, o.door, o.items)).collect(),
                });
            };
            for q in line.iter().skip(1) {
                let gap = q.lo - run.1;
                if gap <= 30.0 {
                    run.1 = run.1.max(q.hi);
                    continue;
                }
                let mut joined = false;
                if gap <= OPENING_MAX {
                    let dn = standing(&ds, door_taken, d, g, run.1, q.lo);
                    let wn = if dn.is_empty() { standing(&ws, window_taken, d, g, run.1, q.lo) } else { Vec::new() };
                    if !dn.is_empty() || !wn.is_empty() {
                        for n in &dn {
                            door_taken[*n] = true;
                        }
                        for n in &wn {
                            window_taken[*n] = true;
                        }
                        openings.push(Opening { lo: run.1, hi: q.lo, door: !dn.is_empty(), items: if dn.is_empty() { wn } else { dn } });
                        joined = true;
                    }
                }
                // A wall meeting this one: one of its faces runs on past the gap.
                if !joined && gap <= WALL_MAX + 100.0 {
                    joined = c.faces.iter().any(|f| (f.0 - d).abs() <= g / 2.0 + SAME && f.1 <= run.1 + SAME && f.2 >= q.lo - SAME);
                }
                if joined {
                    run.1 = q.hi;
                } else {
                    if gap <= 3000.0 {
                        found.gaps.push(Seg { a: at(run.1, d), b: at(q.lo, d) });
                    }
                    close(run, &mut openings, &mut found);
                    run = (q.lo, q.hi);
                }
            }
            close(run, &mut openings, &mut found);
        }
    }
    found
}

/// Each wall's ends brought to the centre line of the wall they meet (a corner, or a wall met part way along).
fn meet(walls: &mut [Wall]) {
    let lines: Vec<(P, P, f64)> = walls.iter().map(|w| (w.a, w.b, w.g)).collect();
    for (n, w) in walls.iter_mut().enumerate() {
        let (a, b, _) = lines[n];
        let len = b.sub(a).len();
        if len < 1.0 {
            continue;
        }
        let u = P((b.0 - a.0) / len, (b.1 - a.1) / len);
        for end in [false, true] {
            let e = if end { b } else { a };
            let mut best: Option<(f64, P)> = None;
            for (m, &(oa, ob, og)) in lines.iter().enumerate() {
                if m == n {
                    continue;
                }
                let ol = ob.sub(oa).len();
                if ol < 1.0 {
                    continue;
                }
                let v = P((ob.0 - oa.0) / ol, (ob.1 - oa.1) / ol);
                let cross = u.0 * v.1 - u.1 * v.0;
                if cross.abs() < 0.2 {
                    continue;
                }
                // e + u·s = oa + v·t
                let w0 = oa.sub(e);
                let s = (w0.0 * v.1 - w0.1 * v.0) / cross;
                let t = (w0.0 * u.1 - w0.1 * u.0) / cross;
                let reach = og / 2.0 / cross.abs() + 60.0;
                if s.abs() > reach || t < -(lines[n].2 / 2.0 + 60.0) || t > ol + lines[n].2 / 2.0 + 60.0 {
                    continue;
                }
                if best.is_none_or(|(d, _)| s.abs() < d) {
                    best = Some((s.abs(), P(e.0 + u.0 * s, e.1 + u.1 * s)));
                }
            }
            if let Some((_, x)) = best {
                if end {
                    w.b = x;
                } else {
                    w.a = x;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------- arcs
/// A curved stretch: its centre, its radius on the centre line, its thickness, from `s` degrees over `sweep`.
struct Curved {
    arc: Arc,
    g: f64,
}

/// The stretch two arcs about one centre share, if any: (from, sweep), degrees.
fn shared(a: &Arc, b: &Arc) -> Option<(f64, f64)> {
    let mut best: Option<(f64, f64)> = None;
    for turn in [-360.0, 0.0, 360.0] {
        let (s, e) = (b.s + turn, b.s + turn + b.sweep);
        let (lo, hi) = (a.s.max(s), (a.s + a.sweep).min(e));
        if hi - lo > best.map_or(0.0, |x| x.1) {
            best = Some((lo, hi - lo));
        }
    }
    best
}

/// Arcs about one centre paired, the nearest first, each arc once. (the stretches, which arcs were used)
fn curved(arcs: &[Arc], min: f64, max: f64) -> (Vec<Curved>, Vec<bool>) {
    let mut cands: Vec<(f64, usize, usize, f64, f64)> = Vec::new();
    for i in 0..arcs.len() {
        for j in i + 1..arcs.len() {
            let (a, b) = (&arcs[i], &arcs[j]);
            let g = (a.r - b.r).abs();
            if g < min || g > max || a.c.sub(b.c).len() > 30.0 {
                continue;
            }
            if let Some((s, sweep)) = shared(a, b) {
                let r = (a.r + b.r) / 2.0;
                if r * sweep.to_radians() >= g.max(100.0) {
                    cands.push((g, i, j, s, sweep));
                }
            }
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut used = vec![false; arcs.len()];
    let mut out = Vec::new();
    for (g, i, j, s, sweep) in cands {
        if used[i] || used[j] {
            continue;
        }
        used[i] = true;
        used[j] = true;
        out.push(Curved { arc: Arc { c: arcs[i].c, r: (arcs[i].r + arcs[j].r) / 2.0, s, sweep }, g });
    }
    (out, used)
}

// ---------------------------------------------------------------------------------------------- groups
/// Groups whose boxes touch (within `reach`) joined into one.
fn touching(groups: Vec<Group>, reach: f64) -> Vec<Group> {
    let n = groups.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let boxes: Vec<Option<(P, P)>> = groups.iter().map(Group::bounds).collect();
    let mut order: Vec<usize> = (0..n).filter(|i| boxes[*i].is_some()).collect();
    order.sort_by(|a, b| boxes[*a].map_or(0.0, |x| x.0.0).total_cmp(&boxes[*b].map_or(0.0, |x| x.0.0)));
    for (k, &i) in order.iter().enumerate() {
        let Some((il, ih)) = boxes[i] else { continue };
        for &j in order.iter().skip(k + 1) {
            let Some((jl, jh)) = boxes[j] else { continue };
            if jl.0 > ih.0 + reach {
                break;
            }
            if jl.1 <= ih.1 + reach && jh.1 >= il.1 - reach {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut out: HashMap<usize, Group> = HashMap::new();
    let mut keys = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        let r = root(&mut parent, i);
        if !out.contains_key(&r) {
            keys.push(r);
        }
        out.entry(r).or_default().join(g);
    }
    keys.into_iter().filter_map(|k| out.remove(&k)).collect()
}

fn mm(x: f64) -> f64 {
    x.round()
}

fn pt(p: P) -> Value {
    json!([mm(p.0), mm(p.1)])
}

fn seg_json(s: &Seg) -> Value {
    json!({"a": pt(s.a), "b": pt(s.b), "length": mm(s.b.sub(s.a).len())})
}

fn listed(segs: &mut Vec<Seg>) -> Value {
    segs.sort_by(|a, b| b.b.sub(b.a).len().total_cmp(&a.b.sub(a.a).len()));
    let total: f64 = segs.iter().map(|s| s.b.sub(s.a).len()).sum();
    json!({"count": segs.len(), "length": mm(total), "longest": segs.iter().take(LEFT_LISTED).map(seg_json).collect::<Vec<_>>()})
}

/// A column: its centre, its width along `angle` degrees and its depth across; or round, `width` across.
fn column(g: &Group) -> Option<Value> {
    if g.segs.is_empty() && g.arcs.is_empty() {
        let (c, r) = g.circles.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))?;
        return Some(json!({"at": pt(c), "round": true, "width": mm(2.0 * r)}));
    }
    let longest = g.segs.iter().max_by(|a, b| a.b.sub(a.a).len().total_cmp(&b.b.sub(b.a).len()));
    let th = longest.map_or(0.0, |s| {
        let d = s.b.sub(s.a);
        d.1.atan2(d.0)
    });
    let (u, n) = (P(th.cos(), th.sin()), P(-th.sin(), th.cos()));
    let pts = g.points();
    let (mut lo, mut hi, mut near, mut far) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for p in &pts {
        lo = lo.min(p.dot(u));
        hi = hi.max(p.dot(u));
        near = near.min(p.dot(n));
        far = far.max(p.dot(n));
    }
    if pts.is_empty() {
        return None;
    }
    let (t, d) = ((lo + hi) / 2.0, (near + far) / 2.0);
    let c = P(u.0 * t + n.0 * d, u.1 * t + n.1 * d);
    let mut angle = th.to_degrees().rem_euclid(180.0);
    if (angle - 180.0).abs() < 0.05 {
        angle = 0.0;
    }
    Some(json!({"at": pt(c), "width": mm(hi - lo), "depth": mm(far - near), "angle": (angle * 10.0).round() / 10.0}))
}

/// A room's number among its label's words, if one reads as a number ("101", "G-12", "B1.04").
fn is_number(w: &str) -> bool {
    let digits = w.chars().filter(char::is_ascii_digit).count();
    digits > 0 && w.chars().count() <= 8 && w.chars().all(|c| c.is_ascii_alphanumeric() || "-./".contains(c)) && digits * 2 >= w.chars().count()
}

/// A level mark among the labels ("+5.00", "±0.00", "-3.20").
fn is_level(w: &str) -> bool {
    let w = w.trim();
    (w.starts_with('+') || w.starts_with('-') || w.starts_with('±') || w.ends_with('+'))
        && w.chars().all(|c| c.is_ascii_digit() || "+-±., ".contains(c))
}

fn is_area(w: &str) -> bool {
    let l = w.to_lowercase();
    l.contains("m2") || l.contains("m²") || l.contains("sqm") || l.contains("sq.m") || l.contains("م2") || l.contains("م²")
}

struct Label {
    at: P,
    h: f64,
    text: String,
}

fn rooms(labels: &[Label]) -> Vec<Value> {
    let n = labels.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let h = labels[i].h.max(labels[j].h).max(50.0);
            let d = labels[i].at.sub(labels[j].at);
            if d.0.abs() < 10.0 * h && d.1.abs() < 3.2 * h {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut sets: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut keys = Vec::new();
    for i in 0..n {
        let r = root(&mut parent, i);
        if !sets.contains_key(&r) {
            keys.push(r);
        }
        sets.entry(r).or_default().push(i);
    }
    let mut out = Vec::new();
    for k in keys {
        let Some(mut set) = sets.remove(&k) else { continue };
        set.sort_by(|a, b| labels[*b].at.1.total_cmp(&labels[*a].at.1));
        let (mut name, mut number): (Vec<String>, Option<String>) = (Vec::new(), None);
        let (mut x, mut y) = (0.0, 0.0);
        for i in &set {
            let l = &labels[*i];
            // A label is read from its start: its middle is about half its words along.
            x += l.at.0 + 0.3 * l.h * l.text.chars().count() as f64;
            y += l.at.1 + 0.5 * l.h;
            for line in l.text.lines() {
                let line = line.trim();
                if line.is_empty() || is_area(line) || is_level(line) {
                    continue;
                }
                if number.is_none() && is_number(line) {
                    number = Some(line.to_string());
                } else {
                    name.push(line.to_string());
                }
            }
        }
        if name.is_empty() && number.is_none() {
            continue;
        }
        let c = P(x / set.len() as f64, y / set.len() as f64);
        let tallest = set.iter().map(|i| labels[*i].h).fold(0.0, f64::max);
        let mut row = json!({"at": pt(c), "name": name.join(" "), "height": mm(tallest)});
        if let Some(num) = number {
            row["number"] = json!(num);
        }
        out.push(row);
    }
    out
}

// ---------------------------------------------------------------------------------------------- the answer
/// `build` (JSON params: k?): see the module.
pub(crate) fn build(o: &Opened, params: &str) -> Result<Value, String> {
    let p: Value = if params.trim().is_empty() { json!({}) } else { serde_json::from_str(params).map_err(|e| format!("the parameters are not JSON: {e}"))? };
    if let Some(key) = p.as_object().and_then(|m| m.keys().find(|key| key.as_str() != "k")) {
        return Err(format!("unknown parameter `{key}`; build takes: k"));
    }
    let k = match p.get("k") {
        None | Some(Value::Null) => 1.0,
        Some(v) => v.as_f64().filter(|x| x.is_finite() && *x > 0.0).ok_or_else(|| format!("`k` is a number over 0, not {v}"))?,
    };
    // What each item of the drawing is, by the layer it lies on itself: what a placement's block draws is the placement's.
    let layer_of: HashMap<String, String> =
        o.drawing.model.iter().map(|e| (format!("{:X}", e.handle.0), e.common.layer.to_uppercase())).collect();
    let flat = o.shapes(None, None, true, MOST)?;
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut labels: Vec<Label> = Vec::new();
    let mut other: HashMap<String, usize> = HashMap::new();
    let mut counted: HashMap<String, bool> = HashMap::new();
    for f in &flat {
        let layer = layer_of.get(&f.handle).map_or_else(|| f.layer.to_uppercase(), Clone::clone);
        if let Shape::Text { at, height, text, .. } = &f.shape {
            if layer == "A-ROOM" && !text.trim().is_empty() {
                labels.push(Label { at: P(at[0] * k, at[1] * k), h: height * k, text: text.clone() });
            }
            continue;
        }
        if !matches!(layer.as_str(), "A-WALL" | "A-DOOR" | "A-WIND" | "A-COLS") {
            if counted.insert(f.handle.clone(), true).is_none() {
                *other.entry(layer).or_default() += 1;
            }
            continue;
        }
        if !groups.contains_key(&f.handle) {
            order.push(f.handle.clone());
        }
        add(groups.entry(f.handle.clone()).or_default(), &f.shape, k);
    }
    let of = |layer: &str| -> Vec<Group> {
        order.iter().filter(|h| layer_of.get(*h).is_some_and(|l| l == layer)).filter_map(|h| groups.get(h)).cloned().collect()
    };
    let wall_groups = of("A-WALL");
    let wall_segs: Vec<Seg> = wall_groups.iter().flat_map(|g| g.segs.iter().copied()).filter(|s| s.b.sub(s.a).len() >= 20.0).collect();
    let wall_arcs: Vec<Arc> = wall_groups.iter().flat_map(|g| g.arcs.iter().copied()).collect();
    let doors = touching(of("A-DOOR"), 30.0);
    let windows = of("A-WIND");
    let columns = touching(of("A-COLS"), 10.0);

    let (mut door_taken, mut window_taken) = (vec![false; doors.len()], vec![false; windows.len()]);
    let mut found = walls(&wall_segs, &doors, &windows, &mut door_taken, &mut window_taken);
    let (wall_curves, arcs_used) = curved(&wall_arcs, WALL_MIN, WALL_MAX);

    // A door no gap holds goes to the nearest wall: one it stands along, or one that ends at its jamb (a door in a
    // corner, between a wall's end and the wall across), which is then run on past it.
    let mut doors_left: Vec<Value> = Vec::new();
    for (n, g) in doors.iter().enumerate() {
        if door_taken[n] {
            continue;
        }
        let Some(c) = g.centre() else { continue };
        let Some((lo, hi)) = g.bounds() else { continue };
        let size = (hi.0 - lo.0).max(hi.1 - lo.1);
        let mut best: Option<(f64, usize, f64, f64, f64)> = None;
        for (m, w) in found.walls.iter().enumerate() {
            let len = w.b.sub(w.a).len();
            if len < 1.0 {
                continue;
            }
            let u = P((w.b.0 - w.a.0) / len, (w.b.1 - w.a.1) / len);
            let t = c.sub(w.a).dot(u);
            if t < -(size + 300.0) || t > len + size + 300.0 {
                continue;
            }
            let foot = P(w.a.0 + u.0 * t, w.a.1 + u.1 * t);
            let off = c.sub(foot).len();
            // Past a wall's end counts against it: a wall it stands along is the nearer.
            let past = (-t).max(t - len).max(0.0);
            if off <= w.g / 2.0 + 0.75 * size && best.is_none_or(|b| off + past < b.0) {
                let along: Vec<f64> = g.points().iter().map(|q| q.sub(w.a).dot(u)).collect();
                let (t0, t1) = (along.iter().copied().fold(f64::INFINITY, f64::min), along.iter().copied().fold(f64::NEG_INFINITY, f64::max));
                best = Some((off + past, m, t0, t1, off));
            }
        }
        match best {
            Some((_, m, t0, t1, off)) => {
                let w = &mut found.walls[m];
                let len = w.b.sub(w.a).len();
                let u = P((w.b.0 - w.a.0) / len, (w.b.1 - w.a.1) / len);
                let a = w.a;
                if t0 < 0.0 {
                    w.a = P(a.0 + u.0 * t0, a.1 + u.1 * t0);
                }
                if t1 > len {
                    w.b = P(a.0 + u.0 * t1, a.1 + u.1 * t1);
                }
                let mid = (t0 + t1) / 2.0;
                let _ = off;
                w.openings.push((P(a.0 + u.0 * mid, a.1 + u.1 * mid), t1 - t0, true, vec![n]));
            }
            None => doors_left.push(json!({"at": pt(c), "size": mm(size)})),
        }
    }

    // Glazing no wall holds: runs of its own.
    let glass: Vec<&Group> = windows.iter().enumerate().filter(|(n, _)| !window_taken[*n]).map(|(_, g)| g).collect();
    let glass_segs: Vec<Seg> = glass.iter().flat_map(|g| g.segs.iter().copied()).filter(|s| s.b.sub(s.a).len() >= 100.0).collect();
    let glass_arcs: Vec<Arc> = glass.iter().flat_map(|g| g.arcs.iter().copied()).collect();
    let mut curtain: Vec<Value> = Vec::new();
    // Glazing along a wall, inside its thickness: (the wall, from where to where along it).
    let mut glass_on: Vec<(usize, f64, f64)> = Vec::new();
    for c in clusters(&glass_segs) {
        let at = |t: f64, d: f64| P(c.u.0 * t + c.n.0 * d, c.u.1 * t + c.n.1 * d);
        let (mut pieces, used) = paired(&c, GLASS_MIN, GLASS_MAX);
        for (f, u) in c.faces.iter().zip(&used) {
            for (a, b) in free(f.1, f.2, u) {
                pieces.push(Piece { d: f.0, g: 0.0, lo: a, hi: b });
            }
        }
        // Along one line (within a glazing run's own thickness), joined where they touch.
        pieces.sort_by(|a, b| a.d.total_cmp(&b.d));
        let mut lines: Vec<Vec<Piece>> = Vec::new();
        for p in pieces {
            match lines.iter_mut().rev().take(8).find(|l| l.first().is_some_and(|q| (q.d - p.d).abs() < 60.0)) {
                Some(l) => l.push(p),
                None => lines.push(vec![p]),
            }
        }
        for mut line in lines {
            line.sort_by(|a, b| a.lo.total_cmp(&b.lo));
            let Some(&first) = line.first() else { continue };
            let mut run = (first.lo, first.hi);
            let mut close = |run: (f64, f64), curtain: &mut Vec<Value>| {
                let (a, b) = (at(run.0, first.d), at(run.1, first.d));
                // Along a wall, inside its thickness: a window in it.
                if run.1 - run.0 >= 300.0 {
                    for (m, w) in found.walls.iter().enumerate() {
                        let len = w.b.sub(w.a).len();
                        if len < 1.0 {
                            continue;
                        }
                        let u = P((w.b.0 - w.a.0) / len, (w.b.1 - w.a.1) / len);
                        let n = P(-u.1, u.0);
                        let (ta, tb) = (a.sub(w.a).dot(u), b.sub(w.a).dot(u));
                        let (t0, t1) = (ta.min(tb), ta.max(tb));
                        if a.sub(w.a).dot(n).abs() <= w.g / 2.0 + 50.0 && b.sub(w.a).dot(n).abs() <= w.g / 2.0 + 50.0 && t0 >= -100.0 && t1 <= len + 100.0 {
                            glass_on.push((m, t0.max(0.0), t1.min(len)));
                            return;
                        }
                    }
                }
                if run.1 - run.0 >= RUN_MIN {
                    curtain.push(json!({"a": pt(a), "b": pt(b)}));
                }
            };
            for q in line.iter().skip(1) {
                if q.lo - run.1 <= 60.0 {
                    run.1 = run.1.max(q.hi);
                } else {
                    close(run, &mut curtain);
                    run = (q.lo, q.hi);
                }
            }
            close(run, &mut curtain);
        }
    }
    // The lines of one window are one window: what overlaps along a wall is joined, and a door's opening is left to it.
    glass_on.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut joined: Vec<(usize, f64, f64)> = Vec::new();
    for (m, t0, t1) in glass_on {
        match joined.last_mut() {
            Some(last) if last.0 == m && t0 <= last.2 + 100.0 => last.2 = last.2.max(t1),
            _ => joined.push((m, t0, t1)),
        }
    }
    for (m, t0, t1) in joined {
        let w = &mut found.walls[m];
        let len = w.b.sub(w.a).len();
        if len < 1.0 || t1 - t0 < 300.0 {
            continue;
        }
        let u = P((w.b.0 - w.a.0) / len, (w.b.1 - w.a.1) / len);
        let at = |t: f64| P(w.a.0 + u.0 * t, w.a.1 + u.1 * t);
        let mid = at((t0 + t1) / 2.0);
        if w.openings.iter().any(|o| o.0.sub(mid).len() < (o.1 + t1 - t0) / 2.0) {
            continue;
        }
        w.openings.push((mid, t1 - t0, false, Vec::new()));
    }
    let (glass_curves, glass_used) = curved(&glass_arcs, GLASS_MIN, GLASS_MAX);
    let mut curves: Vec<Arc> = glass_curves.iter().map(|c| c.arc).collect();
    // A lone arc is a run too, unless it lies along one already taken.
    for (a, u) in glass_arcs.iter().zip(&glass_used) {
        let along = curves.iter().any(|c| c.c.sub(a.c).len() < 60.0 && (c.r - a.r).abs() < GLASS_MAX && shared(c, a).is_some());
        if !*u && !along {
            curves.push(*a);
        }
    }
    for a in curves.iter().filter(|a| a.r * a.sweep.to_radians() >= RUN_MIN && a.sweep < 359.0) {
        curtain.push(json!({"a": pt(arc_point(a, a.s)), "b": pt(arc_point(a, a.s + a.sweep)), "arc": (a.sweep * 100.0).round() / 100.0}));
    }

    meet(&mut found.walls);
    let mut walls_out: Vec<Value> = Vec::new();
    let (mut doors_out, mut windows_out): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
    for w in &found.walls {
        let n = walls_out.len();
        walls_out.push(json!({"a": pt(w.a), "b": pt(w.b), "thickness": mm(w.g)}));
        for (at, width, door, items) in &w.openings {
            if *door {
                let mut row = json!({"at": pt(*at), "width": mm(*width), "wall": n});
                let _ = &row;
                // The side it swings to: where what draws it lies, off the wall.
                let mut all = Group::default();
                for i in items {
                    if let Some(g) = doors.get(*i) {
                        all.join(g);
                    }
                }
                if let Some(c) = all.centre() {
                    if c.sub(*at).len() > w.g / 2.0 {
                        row["toward"] = pt(c);
                    }
                }
                doors_out.push(row);
            } else {
                windows_out.push(json!({"at": pt(*at), "width": mm(*width), "wall": n}));
            }
        }
    }
    for c in &wall_curves {
        walls_out.push(json!({"a": pt(arc_point(&c.arc, c.arc.s)), "b": pt(arc_point(&c.arc, c.arc.s + c.arc.sweep)),
            "arc": (c.arc.sweep * 100.0).round() / 100.0, "thickness": mm(c.g)}));
    }
    let lone_arcs: usize = arcs_used.iter().filter(|u| !**u).count();

    let (mut cols, mut cols_left) = (Vec::new(), 0usize);
    for g in &columns {
        match column(g) {
            Some(c) => {
                let w = c["width"].as_f64().unwrap_or(0.0);
                let d = c["depth"].as_f64().unwrap_or(w);
                if w.min(d) >= 50.0 && w.max(d) <= 3000.0 { cols.push(c) } else { cols_left += 1 }
            }
            None => cols_left += 1,
        }
    }

    let mut thick: Vec<(i64, f64)> = Vec::new();
    for w in &found.walls {
        let t = mm(w.g) as i64;
        let len = w.b.sub(w.a).len();
        match thick.iter_mut().find(|x| x.0 == t) {
            Some(x) => x.1 += len,
            None => thick.push((t, len)),
        }
    }
    thick.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut other: Vec<(String, usize)> = other.into_iter().collect();
    other.sort();
    Ok(json!({
        "walls": walls_out,
        "doors": doors_out,
        "windows": windows_out,
        "curtain": curtain,
        "columns": cols,
        "rooms": rooms(&labels),
        "thicknesses": thick.iter().map(|(t, l)| json!({"thickness": t, "length": mm(*l)})).collect::<Vec<_>>(),
        "left": {
            "faces": listed(&mut found.lone),
            "gaps": listed(&mut found.gaps),
            "arcs": lone_arcs,
            "doors": doors_left,
            "columns": cols_left,
            "layers": other.into_iter().map(|(l, n)| json!({"layer": l, "items": n})).collect::<Vec<_>>(),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(a: (f64, f64), b: (f64, f64)) -> Seg {
        Seg { a: P(a.0, a.1), b: P(b.0, b.1) }
    }

    /// A box drawn with one line.
    fn boxed(lo: (f64, f64), hi: (f64, f64)) -> Group {
        let mut g = Group::default();
        g.seg(P(lo.0, lo.1), P(hi.0, lo.1));
        g.seg(P(hi.0, lo.1), P(hi.0, hi.1));
        g.seg(P(hi.0, hi.1), P(lo.0, hi.1));
        g.seg(P(lo.0, hi.1), P(lo.0, lo.1));
        g
    }

    #[test]
    fn two_faces_make_a_wall_and_a_door_in_its_gap_is_hosted_there() {
        // A wall 200 thick from 0 to 5000, open from 2000 to 2900 where a door stands.
        let faces = [s((0.0, 0.0), (2000.0, 0.0)), s((0.0, 200.0), (2000.0, 200.0)), s((2900.0, 0.0), (5000.0, 0.0)), s((2900.0, 200.0), (5000.0, 200.0))];
        let doors = [boxed((2000.0, 100.0), (2900.0, 1000.0))];
        let (mut dt, mut wt) = (vec![false], Vec::new());
        let f = walls(&faces, &doors, &[], &mut dt, &mut wt);
        assert_eq!(f.walls.len(), 1);
        let w = &f.walls[0];
        assert!((w.a.0 - 0.0).abs() < 1.0 && (w.b.0 - 5000.0).abs() < 1.0 && (w.a.1 - 100.0).abs() < 1.0 && (w.g - 200.0).abs() < 1.0);
        assert_eq!(w.openings.len(), 1);
        assert!((w.openings[0].0.0 - 2450.0).abs() < 1.0 && (w.openings[0].1 - 900.0).abs() < 1.0 && w.openings[0].2 && dt[0]);
    }

    #[test]
    fn a_gap_nothing_stands_in_parts_the_wall_and_is_said() {
        let faces = [s((0.0, 0.0), (2000.0, 0.0)), s((0.0, 200.0), (2000.0, 200.0)), s((2900.0, 0.0), (5000.0, 0.0)), s((2900.0, 200.0), (5000.0, 200.0))];
        let f = walls(&faces, &[], &[], &mut [], &mut []);
        assert_eq!((f.walls.len(), f.gaps.len()), (2, 1));
    }

    #[test]
    fn walls_at_a_corner_and_a_wall_met_part_way_end_on_centre_lines() {
        // An L: a wall along x (y 0..200) and one along y (x 0..200), drawn as their outer and inner faces; and a wall
        // meeting the first part way along, whose face is parted there.
        let faces = [
            s((0.0, 0.0), (6000.0, 0.0)),
            s((200.0, 200.0), (3000.0, 200.0)),
            s((3200.0, 200.0), (6000.0, 200.0)),
            s((0.0, 0.0), (0.0, 4000.0)),
            s((200.0, 200.0), (200.0, 4000.0)),
            s((3000.0, 200.0), (3000.0, 4000.0)),
            s((3200.0, 200.0), (3200.0, 4000.0)),
        ];
        let mut f = walls(&faces, &[], &[], &mut [], &mut []);
        meet(&mut f.walls);
        assert_eq!(f.walls.len(), 3, "{:?}", f.walls.iter().map(|w| (w.a, w.b)).collect::<Vec<_>>());
        let along_x = f.walls.iter().find(|w| (w.a.1 - w.b.1).abs() < 1.0).expect("the wall along x");
        assert!((along_x.a.0.min(along_x.b.0) - 100.0).abs() < 1.0 && (along_x.a.0.max(along_x.b.0) - 6000.0).abs() < 1.0);
        let met = f.walls.iter().find(|w| (w.a.0 - 3100.0).abs() < 1.0).expect("the wall met part way");
        assert!((met.a.1.min(met.b.1) - 100.0).abs() < 1.0);
    }

    #[test]
    fn a_narrow_space_between_two_walls_is_not_a_wall() {
        // Two walls 200 thick with 400 between them: the nearest faces pair, and each face once.
        let faces = [s((0.0, 0.0), (4000.0, 0.0)), s((0.0, 200.0), (4000.0, 200.0)), s((0.0, 600.0), (4000.0, 600.0)), s((0.0, 800.0), (4000.0, 800.0))];
        let f = walls(&faces, &[], &[], &mut [], &mut []);
        assert_eq!(f.walls.len(), 2);
        assert!(f.walls.iter().all(|w| (w.g - 200.0).abs() < 1.0));
    }

    #[test]
    fn arcs_about_one_centre_make_a_curved_wall() {
        let arcs = [Arc { c: P(0.0, 0.0), r: 5000.0, s: 0.0, sweep: 90.0 }, Arc { c: P(0.0, 0.0), r: 5200.0, s: 0.0, sweep: 90.0 }];
        let (c, used) = curved(&arcs, WALL_MIN, WALL_MAX);
        assert_eq!(c.len(), 1);
        assert!((c[0].arc.r - 5100.0).abs() < 1.0 && (c[0].arc.sweep - 90.0).abs() < 1e-6 && used.iter().all(|u| *u));
    }

    #[test]
    fn a_label_is_a_room_with_its_name_and_number() {
        let labels = [
            Label { at: P(1000.0, 1300.0), h: 200.0, text: "CLASSROOM".into() },
            Label { at: P(1000.0, 1000.0), h: 200.0, text: "101".into() },
            Label { at: P(1000.0, 700.0), h: 200.0, text: "48.5 m2".into() },
            Label { at: P(9000.0, 1000.0), h: 200.0, text: "STORE".into() },
        ];
        let r = rooms(&labels);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["name"], "CLASSROOM");
        assert_eq!(r[0]["number"], "101");
    }

    /// Not a test of the suite: `BUILD_DWG=path cargo test -p bimcraft-drawing-web a_real_drawing -- --ignored --nocapture`
    /// reads a clean drawing and writes what it builds beside it (`.build.json`).
    #[test]
    #[ignore]
    fn a_real_drawing() {
        let path = std::env::var("BUILD_DWG").expect("BUILD_DWG");
        let bytes = std::fs::read(&path).expect("read");
        let (pack, _) = cadcraft_index::index(&bytes, &path).expect("index");
        let o = Opened::unpack(&pack).expect("open");
        let k = std::env::var("BUILD_K").unwrap_or_default();
        let out = build(&o, &if k.is_empty() { String::new() } else { format!("{{\"k\": {k}}}") }).expect("build");
        std::fs::write(format!("{path}.build.json"), serde_json::to_string(&out).expect("json")).expect("write");
        for key in ["walls", "doors", "windows", "curtain", "columns", "rooms"] {
            println!("{key}: {}", out[key].as_array().map_or(0, Vec::len));
        }
        println!("thicknesses: {}", out["thicknesses"]);
        println!("left: faces {} gaps {} arcs {} doors {} columns {} layers {}", out["left"]["faces"]["count"], out["left"]["gaps"]["count"],
            out["left"]["arcs"], out["left"]["doors"].as_array().map_or(0, Vec::len), out["left"]["columns"], out["left"]["layers"]);
    }
}

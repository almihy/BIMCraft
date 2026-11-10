//! Plane geometry for the parking solver (Arqiflow's web/lib/parking/geom.ts), in metres as the solver works: points,
//! rings, a region (an outline with holes), and the question the solver asks most: along a strip of the plan, where
//! does the strip lie wholly inside the region?
//!
//! The arithmetic follows the TypeScript step for step (and JavaScript's own `Math.hypot`, `Math.round` and
//! `toFixed`), so the layouts come out the same as Arqiflow's.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

pub type Pt = [f64; 2];
pub type Ring = Vec<Pt>;
/// [from, to] along an axis.
pub type Span = [f64; 2];

pub const EPS: f64 = 1e-6;

/// A quick hash for the solver's own keys (numbers): the maps are asked millions of times a layout.
#[derive(Clone, Copy, Default)]
pub struct Fx(u64);

impl Fx {
    fn add(&mut self, i: u64) {
        self.0 = (self.0.rotate_left(5) ^ i).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(b as u64);
        }
    }
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
}

pub type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

/// An outline and the holes in it (buildings, things kept clear). It remembers the strips asked of it, as the
/// TypeScript does per object: a copy starts with none.
#[derive(Debug, Default)]
pub struct Region {
    pub outer: Ring,
    pub holes: Vec<Ring>,
    memo: RefCell<FxMap<(i64, i64), Vec<Span>>>,
    swapped: OnceCell<Box<Region>>,
}

impl Clone for Region {
    fn clone(&self) -> Self {
        Region::new(self.outer.clone(), self.holes.clone())
    }
}

impl Region {
    pub fn new(outer: Ring, holes: Vec<Ring>) -> Region {
        Region { outer, holes, memo: RefCell::new(FxMap::default()), swapped: OnceCell::new() }
    }
}

// ---------------------------------------------------------------------------------------------------- JavaScript's
/// `Math.hypot(a, b)` as V8 works it (scaled, with Kahan summation), so lengths agree to the last bit.
pub fn hypot(a: f64, b: f64) -> f64 {
    let (x, y) = (a.abs(), b.abs());
    if a.is_nan() || b.is_nan() {
        if x == f64::INFINITY || y == f64::INFINITY {
            return f64::INFINITY;
        }
        return f64::NAN;
    }
    let max = if x > y { x } else { y };
    if max == f64::INFINITY {
        return f64::INFINITY;
    }
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut comp = 0.0;
    for v in [x, y] {
        let n = v / max;
        let summand = n * n - comp;
        let pre = sum + summand;
        comp = (pre - sum) - summand;
        sum = pre;
    }
    sum.sqrt() * max
}

/// `Math.round`: halves go up.
pub fn js_round(x: f64) -> f64 {
    let c = x.ceil();
    if c - 0.5 > x {
        c - 1.0
    } else {
        c
    }
}

/// `x.toFixed(digits)`: the exact value rounded, halves away from zero.
pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    let x = if x == 0.0 { 0.0 } else { x };
    let scaled = x.abs() * 10f64.powi(digits as i32);
    let frac = scaled - scaled.floor();
    if (frac - 0.5).abs() < 1e-6 {
        // Maybe exactly half way: the exact decimal says.
        let exact = format!("{:.60}", x.abs());
        if let Some(dot) = exact.find('.') {
            let after = &exact[dot + 1..];
            let tie = after.as_bytes().get(digits) == Some(&b'5') && after.bytes().skip(digits + 1).all(|c| c == b'0');
            if tie {
                let up = (x.abs() * 10f64.powi(digits as i32)).floor() + 1.0;
                let s = format!("{:.*}", digits, up / 10f64.powi(digits as i32));
                return if x < 0.0 { format!("-{s}") } else { s };
            }
        }
    }
    let s = format!("{:.*}", digits, x);
    if s.starts_with('-') && s[1..].bytes().all(|c| c == b'0' || c == b'.') && x > -1.0 {
        // JavaScript keeps the sign of a small negative ("-0.00"), as Rust does: nothing to change.
        return s;
    }
    s
}

/// `x || 1`: a length of nothing (or not a number) taken as 1.
pub fn or1(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        1.0
    } else {
        x
    }
}

// ---------------------------------------------------------------------------------------------------- rings
pub fn ring_area(r: &[Pt]) -> f64 {
    let n = r.len();
    let mut a = 0.0;
    for i in 0..n {
        let [x1, y1] = r[i];
        let [x2, y2] = r[(i + 1) % n];
        a += x1 * y2 - x2 * y1;
    }
    a / 2.0
}

pub fn dist(a: Pt, b: Pt) -> f64 {
    hypot(b[0] - a[0], b[1] - a[1])
}

/// The ring without a repeated closing point, counter-clockwise.
pub fn ccw(r: &[Pt]) -> Ring {
    let mut out = r.to_vec();
    if out.len() > 1 && dist(out[0], out[out.len() - 1]) < EPS {
        out.pop();
    }
    if ring_area(&out) < 0.0 {
        out.reverse();
    }
    out
}

pub fn region_area(g: &Region) -> f64 {
    ring_area(&g.outer).abs() - g.holes.iter().map(|h| ring_area(h).abs()).sum::<f64>()
}

fn sum_holes(g: &Region) -> f64 {
    let mut s = 0.0;
    for h in &g.holes {
        s += ring_area(h).abs();
    }
    s
}

/// The region's area, summed the way the TypeScript sums it (left to right).
pub fn region_area_js(g: &Region) -> f64 {
    ring_area(&g.outer).abs() - sum_holes(g)
}

/// Every edge of the region, outer and holes.
fn for_edges(rings: &[&[Pt]], mut f: impl FnMut(Pt, Pt)) {
    for r in rings {
        let n = r.len();
        for i in 0..n {
            f(r[i], r[(i + 1) % n]);
        }
    }
}

pub fn point_in_ring(p: Pt, r: &[Pt]) -> bool {
    let n = r.len();
    if n == 0 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let [xi, yi] = r[i];
        let [xj, yj] = r[j];
        if (yi > p[1]) != (yj > p[1]) && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn point_in_region(p: Pt, g: &Region) -> bool {
    point_in_ring(p, &g.outer) && !g.holes.iter().any(|h| point_in_ring(p, h))
}

fn sort_f(v: &mut [f64]) {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}

/// Where the horizontal line at `y` is inside the rings: sorted spans of x.
fn scanline_rings(rings: &[&[Pt]], y: f64) -> Vec<Span> {
    let mut xs: Vec<f64> = Vec::new();
    for_edges(rings, |a, b| {
        if (a[1] > y) != (b[1] > y) {
            xs.push(a[0] + ((y - a[1]) * (b[0] - a[0])) / (b[1] - a[1]));
        }
    });
    sort_f(&mut xs);
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < xs.len() {
        if xs[i + 1] - xs[i] > EPS {
            out.push([xs[i], xs[i + 1]]);
        }
        i += 2;
    }
    out
}

/// Where the horizontal line at `y` is inside the region: sorted spans of x.
pub fn scanline(g: &Region, y: f64) -> Vec<Span> {
    let mut rings: Vec<&[Pt]> = vec![&g.outer];
    for h in &g.holes {
        rings.push(h);
    }
    scanline_rings(&rings, y)
}

/// Spans minus spans.
pub fn subtract(spans: Vec<Span>, cut: &[Span]) -> Vec<Span> {
    let mut out = spans;
    for &[c0, c1] in cut {
        let mut next = Vec::with_capacity(out.len() + 1);
        for &[s0, s1] in &out {
            if c1 <= s0 || c0 >= s1 {
                next.push([s0, s1]);
            } else {
                if c0 > s0 {
                    next.push([s0, c0]);
                }
                if c1 < s1 {
                    next.push([c1, s1]);
                }
            }
        }
        out = next;
    }
    out.retain(|[a, b]| b - a > EPS);
    out
}

/// Where the strip yLo ≤ y ≤ yHi lies wholly inside the region: the spans of x at which the whole cross-section of
/// the strip is inside. Kept per region, to the millimetre, as the TypeScript keeps them.
pub fn strip_spans(g: &Region, y_lo: f64, y_hi: f64) -> Vec<Span> {
    let key = (js_round(y_lo * 1000.0) as i64, js_round(y_hi * 1000.0) as i64);
    if let Some(s) = g.memo.borrow().get(&key) {
        return s.clone();
    }
    let spans = strip_spans_of(g, y_lo, y_hi);
    g.memo.borrow_mut().insert(key, spans.clone());
    spans
}

fn strip_spans_of(g: &Region, y_lo: f64, y_hi: f64) -> Vec<Span> {
    let mut out = strip_in_ring(&g.outer, y_lo, y_hi);
    for h in &g.holes {
        if out.is_empty() {
            break;
        }
        out = subtract(out, &strip_hits(h, y_lo, y_hi));
    }
    out
}

fn strip_in_ring(ring: &[Pt], y_lo: f64, y_hi: f64) -> Vec<Span> {
    let mid = scanline_rings(&[ring], (y_lo + y_hi) / 2.0);
    subtract(mid, &edge_shadows(ring, y_lo, y_hi))
}

fn strip_hits(ring: &[Pt], y_lo: f64, y_hi: f64) -> Vec<Span> {
    let mut spans = edge_shadows(ring, y_lo, y_hi);
    spans.extend(scanline_rings(&[ring], (y_lo + y_hi) / 2.0));
    spans.sort_by(|p, q| p[0].partial_cmp(&q[0]).unwrap_or(std::cmp::Ordering::Equal));
    let mut merged: Vec<Span> = Vec::new();
    for s in spans {
        if let Some(last) = merged.last_mut() {
            if s[0] <= last[1] {
                last[1] = last[1].max(s[1]);
                continue;
            }
        }
        merged.push(s);
    }
    merged
}

fn edge_shadows(ring: &[Pt], y_lo: f64, y_hi: f64) -> Vec<Span> {
    let mut bad = Vec::new();
    let n = ring.len();
    for i in 0..n {
        let a = ring[i];
        let b = ring[(i + 1) % n];
        let lo = a[1].min(b[1]);
        let hi = a[1].max(b[1]);
        if hi < y_lo + EPS || lo > y_hi - EPS {
            continue;
        }
        let flat = (b[1] - a[1]).abs() < EPS;
        let at = |y: f64| if flat { a[0] } else { a[0] + ((y - a[1]) * (b[0] - a[0])) / (b[1] - a[1]) };
        let (x0, x1);
        if flat {
            x0 = a[0].min(b[0]);
            x1 = a[0].max(b[0]);
        } else {
            let p = at(lo.max(y_lo));
            let q = at(hi.min(y_hi));
            x0 = p.min(q);
            x1 = p.max(q);
        }
        bad.push([x0 - EPS, x1 + EPS]);
    }
    bad
}

/// The same in the other direction: where the column xLo ≤ x ≤ xHi lies wholly inside, as spans of y.
pub fn column_spans(g: &Region, x_lo: f64, x_hi: f64) -> Vec<Span> {
    let t = g.swapped.get_or_init(|| Box::new(swap(g)));
    strip_spans(t, x_lo, x_hi)
}

fn swap(g: &Region) -> Region {
    let sw = |r: &Ring| r.iter().map(|p| [p[1], p[0]]).collect::<Ring>();
    Region::new(sw(&g.outer), g.holes.iter().map(sw).collect())
}

/// Whether the axis-aligned box lies wholly inside the region.
pub fn box_inside(g: &Region, x0: f64, y0: f64, x1: f64, y1: f64) -> bool {
    strip_spans(g, y0, y1).iter().any(|&[a, b]| a <= x0 + 1e-4 && b >= x1 - 1e-4)
}

// ---------------------------------------------------------------------------------------------------- frames
/// Coordinates along a direction: u along it, v square to it (to its left).
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub o: Pt,
    pub c: f64,
    pub s: f64,
}

pub fn frame_of(o: Pt, deg: f64) -> Frame {
    let t = (deg * std::f64::consts::PI) / 180.0;
    Frame { o, c: t.cos(), s: t.sin() }
}

pub fn to_local(f: &Frame, p: Pt) -> Pt {
    let dx = p[0] - f.o[0];
    let dy = p[1] - f.o[1];
    [dx * f.c + dy * f.s, -dx * f.s + dy * f.c]
}

pub fn to_world(f: &Frame, p: Pt) -> Pt {
    [f.o[0] + p[0] * f.c - p[1] * f.s, f.o[1] + p[0] * f.s + p[1] * f.c]
}

pub fn region_to_local(f: &Frame, g: &Region) -> Region {
    Region::new(
        g.outer.iter().map(|&p| to_local(f, p)).collect(),
        g.holes.iter().map(|h| h.iter().map(|&p| to_local(f, p)).collect()).collect(),
    )
}

pub fn region_to_world(f: &Frame, g: &Region) -> Region {
    Region::new(
        g.outer.iter().map(|&p| to_world(f, p)).collect(),
        g.holes.iter().map(|h| h.iter().map(|&p| to_world(f, p)).collect()).collect(),
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

pub fn bounds(r: &[Pt]) -> Bounds {
    let mut b = Bounds { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY };
    for &[x, y] in r {
        b.x0 = b.x0.min(x);
        b.y0 = b.y0.min(y);
        b.x1 = b.x1.max(x);
        b.y1 = b.y1.max(y);
    }
    b
}

// ---------------------------------------------------------------------------------------------------- offsets
/// A ring moved in (d > 0) or out (d < 0) by `d`, its corners mitred.
pub fn offset_ring(r: &[Pt], d: f64) -> Ring {
    let ring = clean(&ccw(r));
    let n = ring.len();
    if d.abs() < EPS || n == 0 {
        return ring;
    }
    struct Line {
        p: Pt,
        d: Pt,
    }
    let lines: Vec<Line> = (0..n)
        .map(|i| {
            let a = ring[i];
            let b = ring[(i + 1) % n];
            let l = or1(dist(a, b));
            let nx = -(b[1] - a[1]) / l;
            let ny = (b[0] - a[0]) / l;
            Line { p: [a[0] + nx * d, a[1] + ny * d], d: [(b[0] - a[0]) / l, (b[1] - a[1]) / l] }
        })
        .collect();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let l1 = &lines[(i + n - 1) % n];
        let l2 = &lines[i];
        let den = l1.d[0] * l2.d[1] - l1.d[1] * l2.d[0];
        if den.abs() < 1e-9 {
            out.push(l2.p);
            continue;
        }
        let t = ((l2.p[0] - l1.p[0]) * l2.d[1] - (l2.p[1] - l1.p[1]) * l2.d[0]) / den;
        out.push([l1.p[0] + l1.d[0] * t, l1.p[1] + l1.d[1] * t]);
    }
    out
}

/// A ring without repeated points or points on a straight run.
pub fn clean(r: &[Pt]) -> Ring {
    let n = r.len();
    if n == 0 {
        return Vec::new();
    }
    let first: Ring = (0..n).filter(|&i| dist(r[i], r[(i + n - 1) % n]) > 1e-6).map(|i| r[i]).collect();
    let m = first.len();
    let out: Ring = (0..m)
        .filter(|&i| {
            let a = first[(i + m - 1) % m];
            let b = first[(i + 1) % m];
            let p = first[i];
            ((p[0] - a[0]) * (b[1] - p[1]) - (p[1] - a[1]) * (b[0] - p[0])).abs() > 1e-9 * 1f64.max(dist(a, p) * dist(p, b))
        })
        .map(|i| first[i])
        .collect();
    if out.len() >= 3 {
        out
    } else {
        r.to_vec()
    }
}

fn clip_ring(r: &[Pt], side: &dyn Fn(Pt) -> f64) -> Ring {
    let n = r.len();
    let mut out = Vec::new();
    for i in 0..n {
        let a = r[i];
        let b = r[(i + 1) % n];
        let sa = side(a);
        let sb = side(b);
        if sa >= 0.0 {
            out.push(a);
        }
        if (sa >= 0.0) != (sb >= 0.0) {
            let t = sa / (sa - sb);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// The region cut by a half-plane: what lies where `side(p) >= 0` (Sutherland–Hodgman on each ring).
pub fn clip_half(g: &Region, side: &dyn Fn(Pt) -> f64) -> Option<Region> {
    let outer = clip_ring(&g.outer, side);
    if outer.len() < 3 || ring_area(&outer).abs() < 1.0 {
        return None;
    }
    let holes = g.holes.iter().map(|h| clip_ring(h, side)).filter(|h| h.len() >= 3 && ring_area(h).abs() > EPS).collect();
    Some(Region::new(outer, holes))
}

/// The corners of the region where it turns inward (an L's inner corner, a building's corner).
pub fn reflex_corners(outer: &[Pt], holes: &[Ring]) -> Vec<Pt> {
    let mut out = Vec::new();
    let o = ccw(outer);
    let n = o.len();
    let turn = |a: Pt, b: Pt, c: Pt| (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
    for i in 0..n {
        let a = o[(i + n - 1) % n];
        let c = o[(i + 1) % n];
        if turn(a, o[i], c) < -1e-6 {
            out.push(o[i]);
        }
    }
    for h in holes {
        out.extend(ccw(h));
    }
    out
}

// ---------------------------------------------------------------------------------------------------- convex pieces
/// What of `subject` lies inside the convex `clip` (Sutherland–Hodgman); both any orientation.
pub fn clip_convex(subject: &[Pt], clip: &[Pt]) -> Ring {
    let c = ccw(clip);
    let mut out = subject.to_vec();
    let n = c.len();
    let mut i = 0;
    while i < n && !out.is_empty() {
        let a = c[i];
        let b = c[(i + 1) % n];
        let side = |p: Pt| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let input = std::mem::take(&mut out);
        let m = input.len();
        for j in 0..m {
            let p = input[j];
            let q = input[(j + 1) % m];
            let sp = side(p);
            let sq = side(q);
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
        i += 1;
    }
    out
}

fn boxes_meet(a: &Bounds, b: &Bounds, tol: f64) -> bool {
    a.x0 <= b.x1 + tol && b.x0 <= a.x1 + tol && a.y0 <= b.y1 + tol && b.y0 <= a.y1 + tol
}

/// The area two convex pieces share.
pub fn shared_area(a: &[Pt], b: &[Pt]) -> f64 {
    if !boxes_meet(&bounds(a), &bounds(b), 0.0) {
        return 0.0;
    }
    let piece = clip_convex(a, b);
    if piece.len() >= 3 {
        ring_area(&piece).abs()
    } else {
        0.0
    }
}

/// Two convex pieces a car drives between: they overlap, or share an edge `least` long or more.
pub fn touch_along(a: &[Pt], b: &[Pt], least: f64) -> bool {
    if !boxes_meet(&bounds(a), &bounds(b), 0.1) {
        return false;
    }
    if shared_area(a, b) > 0.1 {
        return true;
    }
    let piece = clip_convex(a, &offset_ring(b, -0.06));
    let mut far: f64 = 0.0;
    for &p in &piece {
        for &q in &piece {
            far = far.max(dist(p, q));
        }
    }
    far >= least
}

/// A rectangle along a line: from `a` to `b`, `width` across.
pub fn band(a: Pt, b: Pt, width: f64) -> Ring {
    let l = or1(dist(a, b));
    let nx = (-(b[1] - a[1]) / l) * (width / 2.0);
    let ny = ((b[0] - a[0]) / l) * (width / 2.0);
    vec![[a[0] - nx, a[1] - ny], [b[0] - nx, b[1] - ny], [b[0] + nx, b[1] + ny], [a[0] + nx, a[1] + ny]]
}

/// Where a ray from `o` along unit `d` first enters a convex piece: its distance, or None.
pub fn ray_into(o: Pt, d: Pt, piece: &[Pt]) -> Option<f64> {
    let mut t0 = f64::NEG_INFINITY;
    let mut t1 = f64::INFINITY;
    let c = ccw(piece);
    let n = c.len();
    for i in 0..n {
        let a = c[i];
        let b = c[(i + 1) % n];
        let nx = b[1] - a[1];
        let ny = -(b[0] - a[0]);
        let num = nx * (a[0] - o[0]) + ny * (a[1] - o[1]);
        let den = nx * d[0] + ny * d[1];
        if den.abs() < 1e-12 {
            if num < 0.0 {
                return None;
            }
            continue;
        }
        let t = num / den;
        if den < 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
    }
    if t0 <= t1 && t1 >= 0.0 {
        Some(t0.max(0.0))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------------- untangling
/// How far a point is from the nearest side of a ring.
pub fn dist_to_ring(p: Pt, r: &[Pt]) -> f64 {
    let n = r.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let a = r[i];
        let b = r[(i + 1) % n];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let l2 = or1(dx * dx + dy * dy);
        let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0);
        best = best.min(hypot(p[0] - a[0] - dx * u, p[1] - a[1] - dy * u));
    }
    best
}

/// A ring that crosses itself split where it crosses into simple loops; the counter-clockwise loops `keep` accepts
/// a point inside of are kept.
pub fn untangle(r: &[Pt], keep: &dyn Fn(Pt) -> bool) -> Vec<Ring> {
    let n = r.len();
    if n < 3 {
        return Vec::new();
    }
    struct Hit {
        side: usize,
        t: f64,
        node: usize,
    }
    let mut hits: Vec<Hit> = Vec::new();
    let mut nodes: Vec<Pt> = Vec::new();
    for i in 0..n {
        let a = r[i];
        let b = r[(i + 1) % n];
        for j in i + 1..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            let c = r[j];
            let d = r[(j + 1) % n];
            let (ex, ey, fx, fy) = (b[0] - a[0], b[1] - a[1], d[0] - c[0], d[1] - c[1]);
            let den = ex * fy - ey * fx;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = ((c[0] - a[0]) * fy - (c[1] - a[1]) * fx) / den;
            let u = ((c[0] - a[0]) * ey - (c[1] - a[1]) * ex) / den;
            if t <= 1e-9 || t >= 1.0 - 1e-9 || u <= 1e-9 || u >= 1.0 - 1e-9 {
                continue;
            }
            let k = nodes.len();
            nodes.push([a[0] + ex * t, a[1] + ey * t]);
            hits.push(Hit { side: i, t, node: k });
            hits.push(Hit { side: j, t: u, node: k });
        }
    }
    let mut loops: Vec<Ring> = Vec::new();
    if nodes.is_empty() {
        loops.push(r.to_vec());
    } else {
        // (point, node or usize::MAX)
        let mut seq: Vec<(Pt, usize)> = Vec::new();
        for (i, &p) in r.iter().enumerate() {
            seq.push((p, usize::MAX));
            let mut mine: Vec<&Hit> = hits.iter().filter(|x| x.side == i).collect();
            mine.sort_by(|x, y| x.t.partial_cmp(&y.t).unwrap_or(std::cmp::Ordering::Equal));
            for h in mine {
                seq.push((nodes[h.node], h.node));
            }
        }
        let m = seq.len();
        let mut other: FxMap<usize, usize> = FxMap::default();
        let mut first: FxMap<usize, usize> = FxMap::default();
        for (k, e) in seq.iter().enumerate() {
            if e.1 == usize::MAX {
                continue;
            }
            match first.get(&e.1) {
                None => {
                    first.insert(e.1, k);
                }
                Some(&f) => {
                    other.insert(k, f);
                    other.insert(f, k);
                }
            }
        }
        let mut used = vec![false; m];
        for s in 0..m {
            if used[s] {
                continue;
            }
            let mut lp: Ring = Vec::new();
            let mut cur = s;
            let mut guard = 0;
            while guard <= m && !used[cur] {
                used[cur] = true;
                lp.push(seq[cur].0);
                let mut nx = (cur + 1) % m;
                if seq[nx].1 != usize::MAX {
                    match other.get(&nx) {
                        Some(&o) => nx = o,
                        None => break,
                    }
                }
                cur = nx;
                guard += 1;
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
    }
    loops
        .into_iter()
        .filter(|l| {
            if ring_area(l) < 1.0 {
                return false;
            }
            let ln = l.len();
            let mut k = 0;
            for i in 1..ln {
                if dist(l[i], l[(i + 1) % ln]) > dist(l[k], l[(k + 1) % ln]) {
                    k = i;
                }
            }
            let a = l[k];
            let b = l[(k + 1) % ln];
            let len = or1(dist(a, b));
            let p = [(a[0] + b[0]) / 2.0 - ((b[1] - a[1]) / len) * 0.05, (a[1] + b[1]) / 2.0 + ((b[0] - a[0]) / len) * 0.05];
            keep(p)
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------- clash index
/// Convex pieces filed by where they are, so asking whether a new piece clashes with any of them is quick.
pub struct PieceIndex {
    cells: FxMap<i64, Vec<usize>>,
    pieces: Vec<Ring>,
    boxes: Vec<Bounds>,
    size: f64,
}

impl PieceIndex {
    pub fn new() -> PieceIndex {
        PieceIndex { cells: FxMap::default(), pieces: Vec::new(), boxes: Vec::new(), size: 8.0 }
    }

    fn keys(&self, b: &Bounds) -> Vec<i64> {
        let s = self.size;
        let mut out = Vec::new();
        let (i0, i1) = ((b.x0 / s).floor() as i64, (b.x1 / s).floor() as i64);
        let (j0, j1) = ((b.y0 / s).floor() as i64, (b.y1 / s).floor() as i64);
        // A piece is a few metres; one far larger than the plot is not filed cell by cell.
        if (i1 - i0).saturating_mul(j1 - j0) > 1_000_000 {
            return out;
        }
        for i in i0..=i1 {
            for j in j0..=j1 {
                out.push(i.wrapping_mul(100003).wrapping_add(j));
            }
        }
        out
    }

    pub fn add(&mut self, p: &[Pt]) {
        let b = bounds(p);
        let id = self.pieces.len();
        self.pieces.push(p.to_vec());
        self.boxes.push(b);
        for k in self.keys(&b) {
            self.cells.entry(k).or_default().push(id);
        }
    }

    /// Whether `p` shares more than 0.005 m² with a piece filed.
    pub fn clash(&self, p: &[Pt]) -> bool {
        let b = bounds(p);
        let mut seen: HashSet<usize, BuildHasherDefault<Fx>> = HashSet::default();
        for k in self.keys(&b) {
            let Some(list) = self.cells.get(&k) else { continue };
            for &id in list {
                if !seen.insert(id) {
                    continue;
                }
                let q = &self.boxes[id];
                if q.x0 >= b.x1 || b.x0 >= q.x1 || q.y0 >= b.y1 || b.y0 >= q.y1 {
                    continue;
                }
                let piece = clip_convex(p, &self.pieces[id]);
                if piece.len() >= 3 && ring_area(&piece).abs() > 0.005 {
                    return true;
                }
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------------------------------- cutting out
fn key4(p: Pt) -> (i64, i64) {
    (js_round(p[0] * 1e4) as i64, js_round(p[1] * 1e4) as i64)
}

/// The ring less a piece that crosses its edge: the loops left, counter-clockwise; None when the two do not cross or
/// the cut cannot be traced.
pub fn cut_out(outer: &[Pt], piece: &[Pt]) -> Option<Vec<Ring>> {
    let a_r = ccw(outer);
    let b_r = ccw(piece);
    let (na, nb) = (a_r.len(), b_r.len());
    let mut extra_a: Vec<Vec<(f64, Pt)>> = vec![Vec::new(); na];
    let mut extra_b: Vec<Vec<(f64, Pt)>> = vec![Vec::new(); nb];
    let mut crossings = 0;
    for i in 0..na {
        let (a, b) = (a_r[i], a_r[(i + 1) % na]);
        for j in 0..nb {
            let (c, d) = (b_r[j], b_r[(j + 1) % nb]);
            let (ex, ey, fx, fy) = (b[0] - a[0], b[1] - a[1], d[0] - c[0], d[1] - c[1]);
            let den = ex * fy - ey * fx;
            if den.abs() < 1e-12 {
                continue;
            }
            let t = ((c[0] - a[0]) * fy - (c[1] - a[1]) * fx) / den;
            let u = ((c[0] - a[0]) * ey - (c[1] - a[1]) * ex) / den;
            if !(-1e-9..=1.0 + 1e-9).contains(&t) || !(-1e-9..=1.0 + 1e-9).contains(&u) {
                continue;
            }
            let p = [a[0] + ex * t, a[1] + ey * t];
            extra_a[i].push((t, p));
            extra_b[j].push((u, p));
            crossings += 1;
        }
    }
    if crossings == 0 {
        return None;
    }
    let split = |r: &[Pt], extra: &mut [Vec<(f64, Pt)>]| -> Ring {
        let mut out: Ring = Vec::new();
        for (i, &p) in r.iter().enumerate() {
            out.push(p);
            extra[i].sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
            for e in &extra[i] {
                out.push(e.1);
            }
        }
        let m = out.len();
        (0..m).filter(|&i| key4(out[i]) != key4(out[(i + 1) % m])).map(|i| out[i]).collect()
    };
    let sa = split(&a_r, &mut extra_a);
    let sb = split(&b_r, &mut extra_b);
    let strictly_in = |m: Pt, r: &[Pt]| point_in_ring(m, r) && dist_to_ring(m, r) > 1e-4;
    let mut frags: Vec<(Pt, Pt)> = Vec::new();
    let (ma, mb) = (sa.len(), sb.len());
    for i in 0..ma {
        let (p, q) = (sa[i], sa[(i + 1) % ma]);
        if !strictly_in([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0], &b_r) {
            frags.push((p, q));
        }
    }
    for i in 0..mb {
        let (p, q) = (sb[i], sb[(i + 1) % mb]);
        if strictly_in([(p[0] + q[0]) / 2.0, (p[1] + q[1]) / 2.0], &a_r) {
            frags.push((q, p));
        }
    }
    let mut from: FxMap<(i64, i64), Vec<usize>> = FxMap::default();
    for (k, f) in frags.iter().enumerate() {
        from.entry(key4(f.0)).or_default().push(k);
    }
    let mut used = vec![false; frags.len()];
    let mut loops: Vec<Ring> = Vec::new();
    for s in 0..frags.len() {
        if used[s] {
            continue;
        }
        let mut lp: Ring = Vec::new();
        let mut k = s;
        for _ in 0..=frags.len() {
            used[k] = true;
            lp.push(frags[k].0);
            let next = from.get(&key4(frags[k].1)).and_then(|l| l.iter().copied().find(|&x| !used[x]));
            match next {
                None => {
                    if key4(frags[k].1) != key4(frags[s].0) {
                        return None;
                    }
                    break;
                }
                Some(x) => k = x,
            }
        }
        if lp.len() >= 3 && ring_area(&lp).abs() > 1e-6 {
            loops.push(lp);
        }
    }
    Some(loops.into_iter().filter(|l| ring_area(l) > 0.0).collect())
}

/// The convex hull of points, counter-clockwise.
pub fn hull(points: &[Pt]) -> Ring {
    let mut p = points.to_vec();
    p.sort_by(|a, b| {
        let dx = a[0] - b[0];
        let d = if dx != 0.0 && !dx.is_nan() { dx } else { a[1] - b[1] };
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    if p.len() < 3 {
        return p;
    }
    let cross = |o: Pt, a: Pt, b: Pt| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lower: Ring = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0 {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Ring = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0 {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

/// The nearest point on a ring's sides.
pub fn nearest_on_ring(p: Pt, r: &[Pt]) -> Pt {
    let n = r.len();
    let Some(&first) = r.first() else { return p };
    let mut best = first;
    let mut bd = f64::INFINITY;
    for i in 0..n {
        let a = r[i];
        let b = r[(i + 1) % n];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let u = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / or1(dx * dx + dy * dy)).clamp(0.0, 1.0);
        let q = [a[0] + dx * u, a[1] + dy * u];
        let d = dist(p, q);
        if d < bd {
            bd = d;
            best = q;
        }
    }
    best
}

/// The ring without the corners that stand less than `tol` off the line between their neighbours.
pub fn simplify(r: &[Pt], tol: f64) -> Ring {
    let mut out = r.to_vec();
    let mut changed = true;
    while changed && out.len() > 3 {
        changed = false;
        let mut i: isize = 0;
        while (i as usize) < out.len() && out.len() > 3 {
            let n = out.len();
            let iu = i as usize;
            let a = out[(iu + n - 1) % n];
            let b = out[(iu + 1) % n];
            let p = out[iu];
            let dx = b[0] - a[0];
            let dy = b[1] - a[1];
            let l = hypot(dx, dy);
            let off = if l < 1e-9 { dist(p, a) } else { ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx).abs() / l };
            let turn = (p[0] - a[0]) * (b[1] - p[1]) - (p[1] - a[1]) * (b[0] - p[0]);
            if off < (if turn < 0.0 && ring_area(&out) > 0.0 { 0.02 } else { tol }) {
                out.remove(iu);
                changed = true;
                i -= 1;
            }
            i += 1;
        }
    }
    out
}

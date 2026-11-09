//! Plan polygons for the site's shapes, in millimetres: rings, their booleans, offsets and the lines they clip, over
//! `i_overlay` (pure Rust, so the core still builds for the browser). The twin of Arqiflow's web/components/editor/geom/
//! site.ts "Clipper2 in metres": rings are open (the first point not repeated), outer rings counter-clockwise and holes
//! clockwise, and a set of rings is filled by the non-zero rule, as Clipper2 and i_overlay hand them out.
//!
//! The strip a centre line gives ([`band`]) is built here, not by `i_overlay`'s stroke: each piece's rectangle and each
//! corner's mitre (squared off past a mitre limit of 2, as Clipper2's), unioned, which is what shapely's flat-capped
//! mitred buffer and Clipper2's butt-ended one both are within the fixtures' 5 mm.
//!
//! Every function takes hostile input calmly: points that are not finite or lie beyond [`MAX_COORD`] are dropped before
//! `i_overlay` sees them (it panics on bounds it cannot hold).

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::clip::FloatClip;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
use i_overlay::string::clip::ClipRule;

use super::Poly;
use crate::geom::P2;

/// A plan point, mm.
pub type Xy = [f64; 2];
/// A ring, open: its first point is not repeated.
pub type Ring = Vec<Xy>;

/// The farthest a point may lie from the origin, mm (the model's own limit).
pub const MAX_COORD: f64 = crate::model::MAX_COORD;
/// Arqiflow's mitre limit for a strip's corners (Clipper2's default): a sharper corner is squared off.
pub const MITRE_LIMIT: f64 = 2.0;
/// How far a rounded offset's pieces stay off the true arc, mm (Arqiflow's `grow`: 2 mm).
pub const ARC_TOL: f64 = 2.0;

// ====================================================================== small things
pub fn dist(a: Xy, b: Xy) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
pub fn sub(a: Xy, b: Xy) -> Xy {
    [a[0] - b[0], a[1] - b[1]]
}
pub fn dot(a: Xy, b: Xy) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
pub fn cross(a: Xy, b: Xy) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
/// The point `k` along `d` from `p`.
pub fn at(p: Xy, d: Xy, k: f64) -> Xy {
    [p[0] + d[0] * k, p[1] + d[1] * k]
}
/// A direction's left.
pub fn left_of(d: Xy) -> Xy {
    [-d[1], d[0]]
}
pub fn ok(p: Xy) -> bool {
    p[0].is_finite() && p[1].is_finite() && p[0].abs() <= MAX_COORD && p[1].abs() <= MAX_COORD
}
pub fn to_p2(p: Xy) -> P2 {
    P2(p[0], p[1])
}
pub fn of_p2(p: P2) -> Xy {
    [p.0, p.1]
}

/// A ring's signed area (positive counter-clockwise), mm².
pub fn ring_area(r: &[Xy]) -> f64 {
    let n = r.len();
    let mut s = 0.0;
    for i in 0..n {
        let (p, q) = (r[i], r[(i + 1) % n]);
        s += p[0] * q[1] - q[0] * p[1];
    }
    s / 2.0
}

/// Net area of rings (outer positive, holes negative), mm².
pub fn area(rings: &[Ring]) -> f64 {
    rings.iter().map(|r| ring_area(r)).sum()
}

/// A ring turned counter-clockwise.
pub fn ccw(r: Ring) -> Ring {
    if ring_area(&r) < 0.0 {
        let mut r = r;
        r.reverse();
        r
    } else {
        r
    }
}

/// A ring without its closing point.
pub fn open(r: &[Xy]) -> Ring {
    let mut out: Ring = r.to_vec();
    if out.len() > 1 && dist(out[0], out[out.len() - 1]) < 1e-6 {
        out.pop();
    }
    out
}

pub fn in_ring(r: &[Xy], p: Xy) -> bool {
    let mut inside = false;
    let n = r.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (r[i], r[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Inside a set of rings by the even-odd rule (holes are rings too).
pub fn in_rings(rings: &[Ring], p: Xy) -> bool {
    rings.iter().fold(false, |k, r| if in_ring(r, p) { !k } else { k })
}

/// A circle as a polygon of `sides`, its first corner due east of the centre.
pub fn circle(c: Xy, radius: f64, sides: usize) -> Ring {
    (0..sides)
        .map(|k| {
            let t = 2.0 * std::f64::consts::PI * k as f64 / sides as f64;
            [c[0] + radius * t.cos(), c[1] + radius * t.sin()]
        })
        .collect()
}

/// A rectangle centred on `c`, `length` along `angle` (degrees) and `width` across it, counter-clockwise.
pub fn oriented_rect(c: Xy, angle: f64, length: f64, width: f64) -> Ring {
    let a = angle.to_radians();
    let (d, n) = ([a.cos(), a.sin()], [-a.sin(), a.cos()]);
    let (hl, hw) = (length / 2.0, width / 2.0);
    vec![
        [c[0] - d[0] * hl - n[0] * hw, c[1] - d[1] * hl - n[1] * hw],
        [c[0] + d[0] * hl - n[0] * hw, c[1] + d[1] * hl - n[1] * hw],
        [c[0] + d[0] * hl + n[0] * hw, c[1] + d[1] * hl + n[1] * hw],
        [c[0] - d[0] * hl + n[0] * hw, c[1] - d[1] * hl + n[1] * hw],
    ]
}

/// The convex hull of points, counter-clockwise (Andrew's monotone chain).
pub fn hull(pts: &[Xy]) -> Ring {
    let mut p: Vec<Xy> = pts.iter().copied().filter(|q| ok(*q)).collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    if p.len() < 3 {
        return p;
    }
    let turn = |o: Xy, a: Xy, b: Xy| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    let mut lo: Vec<Xy> = Vec::new();
    for &q in &p {
        while lo.len() >= 2 && turn(lo[lo.len() - 2], lo[lo.len() - 1], q) <= 0.0 {
            lo.pop();
        }
        lo.push(q);
    }
    let mut hi: Vec<Xy> = Vec::new();
    for &q in p.iter().rev() {
        while hi.len() >= 2 && turn(hi[hi.len() - 2], hi[hi.len() - 1], q) <= 0.0 {
            hi.pop();
        }
        hi.push(q);
    }
    lo.pop();
    hi.pop();
    lo.extend(hi);
    lo
}

/// A closed outline smoothed by Chaikin's corner cutting, `passes` times (0 to 5): each side's points at a quarter and
/// three quarters.
pub fn chaikin(ring: &[Xy], passes: i64) -> Ring {
    let mut pts = open(ring);
    for _ in 0..passes.clamp(0, 5) {
        let n = pts.len();
        let mut nxt = Vec::with_capacity(2 * n);
        for i in 0..n {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            nxt.push([0.75 * a[0] + 0.25 * b[0], 0.75 * a[1] + 0.25 * b[1]]);
            nxt.push([0.25 * a[0] + 0.75 * b[0], 0.25 * a[1] + 0.75 * b[1]]);
        }
        pts = nxt;
    }
    pts
}

// ====================================================================== distances
/// The nearest point of the segment a–b to p: its distance, its place along it (0 to 1) and the point.
pub fn seg_point(a: Xy, b: Xy, p: Xy) -> (f64, f64, Xy) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let q = [a[0] + dx * t, a[1] + dy * t];
    (dist(q, p), t, q)
}

/// How far a point is from a polyline, and the nearest point on it.
pub fn line_distance(line: &[Xy], p: Xy) -> (f64, Xy) {
    let mut best = (f64::INFINITY, line.first().copied().unwrap_or(p));
    for w in line.windows(2) {
        let (d, _, q) = seg_point(w[0], w[1], p);
        if d < best.0 {
            best = (d, q);
        }
    }
    if line.len() == 1 {
        best = (dist(line[0], p), line[0]);
    }
    best
}

/// Where two segments cross, if they do.
pub fn segs_cross(a: Xy, b: Xy, c: Xy, d: Xy) -> Option<Xy> {
    let (r, s) = (sub(b, a), sub(d, c));
    let den = cross(r, s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = cross(sub(c, a), s) / den;
    let u = cross(sub(c, a), r) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| [a[0] + r[0] * t, a[1] + r[1] * t])
}

/// Something to measure to: filled rings (by the even-odd rule) or lines and points (a point is a line of one point).
#[derive(Clone, Debug, Default)]
pub struct Shape<'a> {
    pub rings: &'a [Ring],
    pub lines: &'a [Vec<Xy>],
}

fn segments(s: &Shape<'_>) -> Vec<(Xy, Xy)> {
    let mut out = Vec::new();
    for r in s.rings {
        for i in 0..r.len() {
            out.push((r[i], r[(i + 1) % r.len()]));
        }
    }
    for l in s.lines {
        if l.len() == 1 {
            out.push((l[0], l[0]));
        }
        for w in l.windows(2) {
            out.push((w[0], w[1]));
        }
    }
    out
}

fn bounds_of(segs: &[(Xy, Xy)]) -> [f64; 4] {
    let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
    for (p, q) in segs {
        for r in [p, q] {
            b = [b[0].min(r[0]), b[1].min(r[1]), b[2].max(r[0]), b[3].max(r[1])];
        }
    }
    b
}

/// Edge to edge in plan, 0 when they overlap (one inside the other's area, or their edges cross); None when either is
/// empty. As site.ts `shapeDistance`.
pub fn shape_distance(a: &Shape<'_>, b: &Shape<'_>) -> Option<f64> {
    let (sa, sb) = (segments(a), segments(b));
    if sa.is_empty() || sb.is_empty() {
        return None;
    }
    let first = |s: &Shape<'_>| s.rings.first().and_then(|r| r.first()).or_else(|| s.lines.first().and_then(|l| l.first())).copied();
    if let Some(p) = first(a) {
        if !b.rings.is_empty() && in_rings(b.rings, p) {
            return Some(0.0);
        }
    }
    if let Some(p) = first(b) {
        if !a.rings.is_empty() && in_rings(a.rings, p) {
            return Some(0.0);
        }
    }
    let mut best = f64::INFINITY;
    let bb = bounds_of(&sb);
    for &(p, q) in &sa {
        // A segment whose box lies further from b's box than the best so far cannot do better.
        let gx = (bb[0] - p[0].max(q[0])).max(p[0].min(q[0]) - bb[2]).max(0.0);
        let gy = (bb[1] - p[1].max(q[1])).max(p[1].min(q[1]) - bb[3]).max(0.0);
        if gx.hypot(gy) >= best {
            continue;
        }
        for &(u, v) in &sb {
            if segs_cross(p, q, u, v).is_some() {
                return Some(0.0);
            }
            for (pt, s0, s1) in [(p, u, v), (q, u, v), (u, p, q), (v, p, q)] {
                let d = seg_point(s0, s1, pt).0;
                if d < best {
                    best = d;
                }
            }
        }
    }
    Some(best)
}

/// Distance between two sets of filled rings.
pub fn rings_distance(a: &[Ring], b: &[Ring]) -> f64 {
    shape_distance(&Shape { rings: a, lines: &[] }, &Shape { rings: b, lines: &[] }).unwrap_or(f64::INFINITY)
}

/// Distance from filled rings to a point.
pub fn rings_point_distance(a: &[Ring], p: Xy) -> f64 {
    let pt = [vec![p]];
    shape_distance(&Shape { rings: a, lines: &[] }, &Shape { rings: &[], lines: &pt }).unwrap_or(f64::INFINITY)
}

// ====================================================================== i_overlay
/// Rings fit for `i_overlay`: at least `least` finite points each, within the model.
fn clean(rings: &[Ring], least: usize) -> Vec<Ring> {
    rings
        .iter()
        .filter(|r| r.len() >= least && r.iter().all(|p| ok(*p)))
        .cloned()
        .collect()
}

fn flatten(shapes: Vec<Vec<Vec<Xy>>>) -> Vec<Ring> {
    shapes.into_iter().flatten().map(despike).filter(|r| r.len() >= 3 && ring_area(r).abs() > 1e-6).collect()
}

/// A ring without repeated points and without spikes (out and back along the same line), which the integer snapping
/// of a boolean can leave where two edges almost lie on each other.
pub fn despike(r: Ring) -> Ring {
    let mut r = r;
    let mut changed = true;
    while changed && r.len() >= 3 {
        changed = false;
        let n = r.len();
        let mut out: Ring = Vec::with_capacity(n);
        for i in 0..n {
            let (p, q, s) = (r[(i + n - 1) % n], r[i], r[(i + 1) % n]);
            let repeated = dist(p, q) < 1e-6;
            // q is the tip of a spike when the edges into and out of it run back on each other.
            let (a, b) = (sub(q, p), sub(s, q));
            let (la, lb) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
            let spike = la > 0.0 && lb > 0.0 && dot(a, b) < 0.0 && cross(a, b).abs() <= 1e-9 * la * lb + 1e-6 * la.max(lb);
            if repeated || spike {
                changed = true;
                continue;
            }
            out.push(q);
        }
        r = out;
    }
    r
}

/// Union of rings (non-zero): outer rings counter-clockwise, holes clockwise.
pub fn union(rings: &[Ring]) -> Vec<Ring> {
    let s = clean(rings, 3);
    if s.is_empty() {
        return Vec::new();
    }
    let none: Vec<Ring> = Vec::new();
    flatten(s.overlay(&none, OverlayRule::Subject, FillRule::NonZero))
}

fn bool_op(rule: OverlayRule, a: &[Ring], b: &[Ring]) -> Vec<Ring> {
    let s = clean(a, 3);
    if s.is_empty() {
        return Vec::new();
    }
    let c = clean(b, 3);
    if c.is_empty() {
        return if rule == OverlayRule::Difference { union(&s) } else { Vec::new() };
    }
    flatten(s.overlay(&c, rule, FillRule::NonZero))
}

pub fn difference(a: &[Ring], b: &[Ring]) -> Vec<Ring> {
    bool_op(OverlayRule::Difference, a, b)
}

pub fn intersection(a: &[Ring], b: &[Ring]) -> Vec<Ring> {
    bool_op(OverlayRule::Intersect, a, b)
}

/// Rings grown (or shrunk, negative) by `d` with round corners, their arcs within `tol` of the true arc.
pub fn grow(rings: &[Ring], d: f64, tol: f64) -> Vec<Ring> {
    let s = clean(rings, 3);
    if s.is_empty() || !d.is_finite() || d.abs() > MAX_COORD {
        return Vec::new();
    }
    if d.abs() < 1e-9 {
        return union(&s);
    }
    // i_overlay takes the most a round corner's piece may turn: the angle whose arc stays within `tol`.
    let r = d.abs();
    let step = if r > tol { 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos() } else { std::f64::consts::FRAC_PI_2 };
    let style = OutlineStyle::new(d).line_join(LineJoin::Round(step));
    // Its input must be one non-overlapping set (outer rings counter-clockwise).
    let s = union(&s);
    if s.is_empty() {
        return Vec::new();
    }
    flatten(s.outline(&style))
}

/// What a closing by `radius` (grown by it, then shrunk by it, round) adds to `rings`: the pieces it fills in where
/// shapes meet, each at least `least` mm². A shape's arcs are polygons, and the closing rounds each of their corners by
/// a crumb under a millimetre thick; those crumbs, and the hairlines the snapping of the booleans leaves along the
/// edges, are left out: the pieces are what lies off `rings` by a fiftieth of a millimetre (no hairline is that thick),
/// and a piece counts only where some of it lies more than a millimetre off them. Union a piece with what it meets
/// through [`join`], which closes that fiftieth.
pub fn closing_pieces(rings: &[Ring], radius: f64, least: f64) -> Vec<Ring> {
    if !(radius > 0.0) || rings.is_empty() {
        return Vec::new();
    }
    let closed = grow(&grow(rings, radius, ARC_TOL), -radius, ARC_TOL);
    let cores: Vec<Ring> = difference(&closed, &grow(rings, 1.0, 0.5)).into_iter().filter(|g| ring_area(g) > 1.0).collect();
    if cores.is_empty() {
        return Vec::new();
    }
    difference(&closed, &grow(rings, 0.02, 0.01))
        .into_iter()
        .filter(|g| ring_area(g) > least && cores.iter().any(|c| c.iter().any(|q| in_ring(g, *q))))
        .collect()
}

/// The union of a shape and pieces that touch it (or lie within a fiftieth of a millimetre of it): one shape, the
/// hair's breadth between them closed.
pub fn join(shape: &[Ring], pieces: &[Ring]) -> Vec<Ring> {
    let mut all = shape.to_vec();
    all.extend(pieces.iter().cloned());
    let u = union(&all);
    let outers = |rs: &[Ring]| rs.iter().filter(|r| ring_area(r) > 0.0).count();
    if outers(&u) > outers(shape) { grow(&grow(&u, 0.1, ARC_TOL), -0.1, ARC_TOL) } else { u }
}

/// The strip `width` wide along a line: butt ends, mitred corners (squared off past the mitre limit, as Clipper2's),
/// as one union (shapely's flat-capped mitred buffer).
pub fn band(line: &[Xy], width: f64) -> Vec<Ring> {
    let mut pts: Vec<Xy> = Vec::with_capacity(line.len());
    for &p in line {
        if !ok(p) {
            return Vec::new();
        }
        if pts.last().is_none_or(|q| dist(*q, p) > 1e-6) {
            pts.push(p);
        }
    }
    if pts.len() < 2 || !(width > 0.0) || !width.is_finite() || width > MAX_COORD {
        return Vec::new();
    }
    let h = width / 2.0;
    let normal = |a: Xy, b: Xy| {
        let l = dist(a, b);
        [-(b[1] - a[1]) / l, (b[0] - a[0]) / l]
    };
    let mut parts: Vec<Ring> = Vec::with_capacity(2 * pts.len());
    for w in pts.windows(2) {
        let n = normal(w[0], w[1]);
        parts.push(vec![at(w[0], n, -h), at(w[1], n, -h), at(w[1], n, h), at(w[0], n, h)]);
    }
    for i in 1..pts.len() - 1 {
        let (n0, n1) = (normal(pts[i - 1], pts[i]), normal(pts[i], pts[i + 1]));
        let turn = cross(sub(pts[i], pts[i - 1]), sub(pts[i + 1], pts[i]));
        if turn.abs() < 1e-12 {
            continue;
        }
        // The outside of the corner: the right when the line turns left.
        let s = if turn > 0.0 { -1.0 } else { 1.0 };
        let (a, b) = (at(pts[i], n0, s * h), at(pts[i], n1, s * h));
        let cos_a = dot(n0, n1);
        // The wedge reaches from inside the corner (half a half width in along its bisector), so that none of its edges
        // lies on a piece's own end.
        let inward = [n0[0] + n1[0], n0[1] + n1[1]];
        let li = inward[0].hypot(inward[1]);
        let v = if li > 1e-12 { at(pts[i], [inward[0] / li, inward[1] / li], -s * h / 2.0) } else { pts[i] };
        let mut wedge = vec![v, a];
        if cos_a > 2.0 / (MITRE_LIMIT * MITRE_LIMIT) - 1.0 {
            // The mitre: where the two edges meet.
            let m = [(n0[0] + n1[0]) * s * h / (1.0 + cos_a), (n0[1] + n1[1]) * s * h / (1.0 + cos_a)];
            wedge.push([pts[i][0] + m[0], pts[i][1] + m[1]]);
        } else {
            // Squared off at the half width, square to the corner's bisector (Clipper2's DoSquare).
            let bis = [n0[0] + n1[0], n0[1] + n1[1]];
            let lb = bis[0].hypot(bis[1]);
            if lb > 1e-12 {
                let u = [bis[0] / lb * s, bis[1] / lb * s];
                let c = at(pts[i], u, h);
                let t = left_of(u);
                // The square edge through c along t meets each offset edge.
                for (o, d) in [(a, sub(pts[i], pts[i - 1])), (b, sub(pts[i + 1], pts[i]))] {
                    let den = cross(t, d);
                    if den.abs() > 1e-12 {
                        let k = cross(sub(o, c), d) / den;
                        wedge.push(at(c, t, k));
                    }
                }
            }
        }
        wedge.push(b);
        parts.push(ccw(wedge));
    }
    let parts: Vec<Ring> = parts.into_iter().map(ccw).collect();
    union(&parts)
}

/// Polylines less what lies inside `rings` (non-zero), or only what does: the pieces left, each two points or more.
pub fn clip_lines(lines: &[Vec<Xy>], rings: &[Ring], keep_outside: bool) -> Vec<Vec<Xy>> {
    let ls: Vec<Vec<Xy>> = lines
        .iter()
        .filter(|l| l.len() >= 2 && l.iter().all(|p| ok(*p)))
        .cloned()
        .collect();
    if ls.is_empty() {
        return Vec::new();
    }
    let rs = clean(rings, 3);
    if rs.is_empty() {
        return if keep_outside { ls } else { Vec::new() };
    }
    let rule = ClipRule { invert: keep_outside, boundary_included: false };
    ls.clip_by(&rs, FillRule::NonZero, rule).into_iter().filter(|l| l.len() >= 2).collect()
}

// ====================================================================== polygons
/// Rings grouped as polygons (each outer ring with the holes it holds), largest first: site.ts `geometryOf`.
pub fn polys_of(rings: &[Ring]) -> Vec<Poly> {
    let mut outers: Vec<&Ring> = rings.iter().filter(|r| ring_area(r) > 0.0).collect();
    outers.sort_by(|a, b| ring_area(a).total_cmp(&ring_area(b)));
    let mut polys: Vec<(Ring, Vec<Ring>)> = outers.into_iter().map(|o| (o.clone(), Vec::new())).collect();
    for h in rings.iter().filter(|r| ring_area(r) < 0.0) {
        let probe = if h.len() >= 2 { [(h[0][0] + h[1][0]) / 2.0, (h[0][1] + h[1][1]) / 2.0] } else { h[0] };
        if let Some(p) = polys.iter_mut().find(|(o, _)| in_ring(o, h[0]) || in_ring(o, probe)) {
            p.1.push(h.clone());
        }
    }
    polys.sort_by(|a, b| ring_area(&b.0).total_cmp(&ring_area(&a.0)));
    polys
        .into_iter()
        .map(|(o, hs)| Poly { outer: o.into_iter().map(to_p2).collect(), holes: hs.into_iter().map(|h| h.into_iter().map(to_p2).collect()).collect() })
        .collect()
}

/// A polygon's rings, outer counter-clockwise and holes clockwise.
pub fn rings_of(polys: &[Poly]) -> Vec<Ring> {
    let mut out = Vec::new();
    for p in polys {
        let o: Ring = p.outer.iter().map(|q| of_p2(*q)).collect();
        if o.len() >= 3 {
            out.push(ccw(o));
        }
        for h in &p.holes {
            let h: Ring = h.iter().map(|q| of_p2(*q)).collect();
            if h.len() >= 3 {
                let mut h = ccw(h);
                h.reverse();
                out.push(h);
            }
        }
    }
    out
}

/// Polygons with their points rounded to `step` mm (as the model keeps them).
pub fn rounded(polys: Vec<Poly>, step: f64) -> Vec<Poly> {
    let r = |p: P2| P2((p.0 / step).round() * step, (p.1 / step).round() * step);
    polys
        .into_iter()
        .map(|p| Poly { outer: p.outer.into_iter().map(r).collect(), holes: p.holes.into_iter().map(|h| h.into_iter().map(r).collect()).collect() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn a_straight_band_is_its_rectangle_with_butt_ends() {
        let b = band(&[[0.0, 0.0], [40000.0, 0.0]], 5500.0);
        assert_eq!(b.len(), 1);
        assert!(close(area(&b), 40000.0 * 5500.0, 1.0), "{}", area(&b));
    }

    #[test]
    fn a_right_angled_band_is_mitred() {
        // The hedge of the fixtures: 10 m and 10 m at 1.5 m, mitred: 2 × 10 × 1.5 = 30 m².
        let b = band(&[[0.0, 0.0], [10000.0, 0.0], [10000.0, 10000.0]], 1500.0);
        assert_eq!(b.len(), 1);
        assert!(close(area(&b), 30.0e6, 10.0), "{}", area(&b));
        // The outer corner's mitre point is there.
        assert!(b[0].iter().any(|p| dist(*p, [10750.0, -750.0]) < 0.5), "{:?}", b[0]);
    }

    #[test]
    fn a_hairpin_band_is_squared_off_not_spiked() {
        let b = band(&[[0.0, 0.0], [10000.0, 0.0], [0.0, 1000.0]], 1000.0);
        let far = b.iter().flatten().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max);
        assert!(far < 10000.0 + 600.0, "{far}");
    }

    #[test]
    fn booleans_and_offsets_keep_their_areas() {
        let a = vec![oriented_rect([0.0, 0.0], 0.0, 1000.0, 1000.0)];
        let b = vec![oriented_rect([500.0, 0.0], 0.0, 1000.0, 1000.0)];
        assert!(close(area(&union(&[a[0].clone(), b[0].clone()])), 1.5e6, 1.0));
        assert!(close(area(&difference(&a, &b)), 0.5e6, 1.0));
        assert!(close(area(&intersection(&a, &b)), 0.5e6, 1.0));
        let g = grow(&a, 100.0, 0.5);
        let want = 1.0e6 + 4.0 * 1000.0 * 100.0 + std::f64::consts::PI * 100.0 * 100.0;
        // The corners' arcs are chords within 0.5 mm of the true arc: a little short of a circle's area.
        assert!(close(area(&g), want, 300.0), "{} {}", area(&g), want);
        let s = grow(&a, -100.0, 0.5);
        assert!(close(area(&s), 800.0 * 800.0, 5.0), "{}", area(&s));
    }

    #[test]
    fn lines_are_clipped_outside_and_inside() {
        let r = vec![oriented_rect([0.0, 0.0], 0.0, 1000.0, 1000.0)];
        let l = vec![vec![[-2000.0, 0.0], [2000.0, 0.0]]];
        let out = clip_lines(&l, &r, true);
        assert_eq!(out.len(), 2, "{out:?}");
        let inside = clip_lines(&l, &r, false);
        assert_eq!(inside.len(), 1);
        let len: f64 = inside[0].windows(2).map(|w| dist(w[0], w[1])).sum();
        assert!(close(len, 1000.0, 0.5));
    }

    #[test]
    fn hostile_points_are_left_out_not_a_panic() {
        assert!(band(&[[0.0, 0.0], [f64::NAN, 1.0]], 100.0).is_empty());
        assert!(union(&[vec![[0.0, 0.0], [1e300, 0.0], [0.0, 1.0]]]).is_empty());
        assert!(grow(&[oriented_rect([0.0, 0.0], 0.0, 10.0, 10.0)], f64::INFINITY, 1.0).is_empty());
    }
}

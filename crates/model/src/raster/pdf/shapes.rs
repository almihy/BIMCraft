//! The geometry a page's vectors are read with, mm on the paper: the region what is drawn is clipped to (the page, a box, or
//! any shape a clipping path makes, by its fill rule), fills resolved into rings that fill by odd parity whatever rule the
//! page gave them, lines cut to a region, dashes laid along a line, and a stroke's outline as a fill. The booleans are
//! `i_overlay`'s (pure Rust, so the core still builds for the browser); every point is checked before it gets there.

use i_overlay::core::fill_rule::FillRule as Rule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::clip::FloatClip;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::float::stroke::offset::StrokeOffset;
use i_overlay::mesh::float::style::{LineCap, LineJoin, StrokeStyle};
use i_overlay::string::clip::ClipRule;

/// A point on the paper, mm.
pub(super) type Pt = [f64; 2];

/// How far a point may lie from the paper's corner for the booleans, mm: past any page (a page is at most 5 m a side; a user
/// unit may make it 75 000 times a point, which is still within).
const FAR: f64 = 1e7;

/// A region what is drawn shows inside: a box, or rings that fill it by odd parity (resolved, so they never overlap and a hole
/// is a ring inside another), with the box round them.
#[derive(Clone, Debug)]
pub(super) enum Region {
    Box([f64; 4]),
    Rings { rings: Vec<Vec<Pt>>, bounds: [f64; 4] },
}

impl Region {
    /// The box round it: x0, y0, x1, y1.
    pub(super) fn bounds(&self) -> [f64; 4] {
        match self {
            Region::Box(b) => *b,
            Region::Rings { bounds, .. } => *bounds,
        }
    }

    /// What shows of nothing: a box far off the page.
    pub(super) fn nothing() -> Region {
        Region::Box([-2e9, -2e9, -1e9, -1e9])
    }

    /// Whether point `p` is inside it.
    pub(super) fn holds(&self, p: Pt) -> bool {
        match self {
            Region::Box([x0, y0, x1, y1]) => p[0] >= *x0 && p[0] <= *x1 && p[1] >= *y0 && p[1] <= *y1,
            Region::Rings { rings, bounds } => inside_box(bounds, p) && rings.iter().filter(|r| in_ring(r, p)).count() % 2 == 1,
        }
    }

    /// This region within `clip` (a clipping path's rings, filled by `even_odd` or not): a box when both are boxes.
    pub(super) fn within(&self, clip: &[Vec<Pt>], even_odd: bool) -> Region {
        if let ([ring], Region::Box(b)) = (clip, self)
            && let Some(c) = as_box(ring)
        {
            let r = [b[0].max(c[0]), b[1].max(c[1]), b[2].min(c[2]), b[3].min(c[3])];
            return if r[0] <= r[2] && r[1] <= r[3] { Region::Box(r) } else { Region::nothing() };
        }
        let clip = resolve(clip, even_odd);
        if clip.is_empty() {
            return Region::nothing();
        }
        let mine = match self {
            Region::Box(b) => vec![box_ring(b)],
            Region::Rings { rings, .. } => rings.clone(),
        };
        let rings = overlay(&mine, &clip, OverlayRule::Intersect);
        match around(rings.iter().flatten()) {
            Some(bounds) => Region::Rings { rings, bounds },
            None => Region::nothing(),
        }
    }

    /// The pieces of polyline `pts` (closed back to its start when `closed`) inside it, each two points or more; a closed one
    /// wholly inside comes back whole, its start again at its end.
    pub(super) fn lines(&self, pts: &[Pt], closed: bool) -> Vec<Vec<Pt>> {
        match self {
            Region::Box(b) => clip_lines(pts, closed, b),
            Region::Rings { rings, bounds } => {
                // Cut to the box first: what lies past it can't show, and the booleans needn't see it.
                let mut out = Vec::new();
                for piece in clip_lines(pts, closed, bounds) {
                    if piece.iter().all(|p| self.holds(*p)) && !crosses(&piece, rings) {
                        out.push(piece);
                        continue;
                    }
                    let kept: Vec<Vec<Pt>> = vec![piece]
                        .clip_by(rings, Rule::EvenOdd, ClipRule { invert: false, boundary_included: true })
                        .into_iter()
                        .filter(|l| l.len() >= 2)
                        .collect();
                    out.extend(kept);
                }
                out
            }
        }
    }

    /// Rings `rings` (filling by odd parity) as they show inside it.
    pub(super) fn fill(&self, rings: Vec<Vec<Pt>>) -> Vec<Vec<Pt>> {
        match self {
            Region::Box(b) => rings.iter().map(|r| clip_ring(r, b)).filter(|r| r.len() >= 3).collect(),
            Region::Rings { rings: clip, bounds } => {
                let cut: Vec<Vec<Pt>> = rings.iter().map(|r| clip_ring(r, bounds)).filter(|r| r.len() >= 3).collect();
                if cut.is_empty() {
                    return cut;
                }
                if cut.iter().flatten().all(|p| self.holds(*p)) && !cut.iter().any(|r| crosses(r, clip)) {
                    return cut;
                }
                overlay_rule(&cut, clip, OverlayRule::Intersect, Rule::EvenOdd)
            }
        }
    }
}

fn inside_box(b: &[f64; 4], p: Pt) -> bool {
    p[0] >= b[0] && p[0] <= b[2] && p[1] >= b[1] && p[1] <= b[3]
}

/// The box round points, or none of none.
pub(super) fn around<'a>(pts: impl IntoIterator<Item = &'a Pt>) -> Option<[f64; 4]> {
    let mut b: Option<[f64; 4]> = None;
    for p in pts {
        b = Some(match b {
            None => [p[0], p[1], p[0], p[1]],
            Some([a, c, d, e]) => [a.min(p[0]), c.min(p[1]), d.max(p[0]), e.max(p[1])],
        });
    }
    b
}

fn box_ring(b: &[f64; 4]) -> Vec<Pt> {
    vec![[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]]
}

/// A ring's box, when the ring is one: four corners (a fifth back on the first), each side along an axis.
pub(super) fn as_box(r: &[Pt]) -> Option<[f64; 4]> {
    let r = match r {
        [a @ .., last] if r.len() == 5 && Some(last) == a.first() => a,
        _ => r,
    };
    let [a, b, c, d] = r else { return None };
    let along = |p: &Pt, q: &Pt| p[0] == q[0] || p[1] == q[1];
    let sides = along(a, b) && along(b, c) && along(c, d) && along(d, a) && (a[0] == b[0]) != (b[0] == c[0]);
    if !sides {
        return None;
    }
    around(r.iter())
}

/// Whether point `p` is inside ring `r` (crossings of a ray to its right).
pub(super) fn in_ring(r: &[Pt], p: Pt) -> bool {
    let mut inside = false;
    let n = r.len();
    for i in 0..n {
        let (a, b) = (r[i], r[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) {
            inside = !inside;
        }
    }
    inside
}

/// Whether polyline `pts` crosses an edge of `rings` anywhere (touching counts).
fn crosses(pts: &[Pt], rings: &[Vec<Pt>]) -> bool {
    let Some(b) = around(pts.iter()) else { return false };
    let meets = |s: &[f64; 4]| s[0] <= b[2] && b[0] <= s[2] && s[1] <= b[3] && b[1] <= s[3];
    rings.iter().any(|r| {
        let n = r.len();
        (0..n).any(|i| {
            let (c, d) = (r[i], r[(i + 1) % n]);
            meets(&[c[0].min(d[0]), c[1].min(d[1]), c[0].max(d[0]), c[1].max(d[1])])
                && pts.windows(2).any(|w| segments_meet(w[0], w[1], c, d))
        })
    })
}

fn segments_meet(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
    let cross = |o: Pt, p: Pt, q: Pt| (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0]);
    let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
    if ((d1 > 0.0) != (d2 > 0.0) || d1 == 0.0 || d2 == 0.0) && ((d3 > 0.0) != (d4 > 0.0) || d3 == 0.0 || d4 == 0.0) {
        let lo = |p: f64, q: f64| p.min(q);
        let hi = |p: f64, q: f64| p.max(q);
        return lo(a[0], b[0]) <= hi(c[0], d[0])
            && lo(c[0], d[0]) <= hi(a[0], b[0])
            && lo(a[1], b[1]) <= hi(c[1], d[1])
            && lo(c[1], d[1]) <= hi(a[1], b[1]);
    }
    false
}

/// Rings with their points checked for the booleans: those finite and near the paper, three points or more.
fn checked(rings: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    rings
        .iter()
        .filter(|r| r.len() >= 3 && r.iter().all(|p| p[0].is_finite() && p[1].is_finite() && p[0].abs() < FAR && p[1].abs() < FAR))
        .cloned()
        .collect()
}

fn flat(shapes: Vec<Vec<Vec<Pt>>>) -> Vec<Vec<Pt>> {
    shapes.into_iter().flatten().filter(|r| r.len() >= 3).collect()
}

fn overlay(a: &[Vec<Pt>], b: &[Vec<Pt>], op: OverlayRule) -> Vec<Vec<Pt>> {
    overlay_rule(a, b, op, Rule::EvenOdd)
}

fn overlay_rule(a: &[Vec<Pt>], b: &[Vec<Pt>], op: OverlayRule, rule: Rule) -> Vec<Vec<Pt>> {
    let (a, b) = (checked(a), checked(b));
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    flat(a.overlay(&b, op, rule))
}

/// Rings filled by `even_odd` or not (non-zero) as rings that fill the same by odd parity: those that overlap or wind round
/// a point more than once made one, so a renderer that fills by odd parity draws what the page does.
pub(super) fn resolve(rings: &[Vec<Pt>], even_odd: bool) -> Vec<Vec<Pt>> {
    let rings = checked(rings);
    if rings.is_empty() {
        return rings;
    }
    // One simple ring fills alike by either rule; so do rings apart that are each simple, by odd parity.
    if even_odd || simple_apart(&rings) {
        return rings;
    }
    let none: Vec<Vec<Pt>> = Vec::new();
    flat(rings.overlay(&none, OverlayRule::Subject, Rule::NonZero))
}

/// Whether rings are each simple (no side crossing another of it) and lie apart, none round another: then the non-zero rule
/// fills them as odd parity does.
fn simple_apart(rings: &[Vec<Pt>]) -> bool {
    const MOST: usize = 64;
    let boxes: Vec<[f64; 4]> = rings.iter().filter_map(|r| around(r.iter())).collect();
    for (i, a) in boxes.iter().enumerate() {
        if boxes[i + 1..].iter().any(|b| a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]) {
            return false;
        }
    }
    rings.iter().all(|r| {
        if r.len() > MOST {
            return false;
        }
        let n = r.len();
        // Each side against the sides that don't share a corner with it.
        (0..n).all(|i| (i + 2..n).filter(|j| (j + 1) % n != i).all(|j| !segments_meet(r[i], r[(i + 1) % n], r[j], r[(j + 1) % n])))
    })
}

/// The parts of polyline `pts` (closed back to its start when `closed`) inside box `r` (x0, y0, x1, y1), each an open run of two
/// or more points, Liang and Barsky's clipping of each segment. A polyline wholly inside comes back whole, its start again at
/// its end when it closes.
pub(super) fn clip_lines(pts: &[Pt], closed: bool, r: &[f64; 4]) -> Vec<Vec<Pt>> {
    let inside = |q: &Pt| inside_box(r, *q);
    if pts.iter().all(inside) {
        let mut all = pts.to_vec();
        if closed && let Some(&first) = pts.first() {
            all.push(first);
        }
        return vec![all];
    }
    let n = pts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    let mut runs: Vec<Vec<Pt>> = Vec::new();
    let mut run: Vec<Pt> = Vec::new();
    let end = |run: &mut Vec<Pt>, runs: &mut Vec<Vec<Pt>>| {
        let done = std::mem::take(run);
        if done.len() > 1 {
            runs.push(done);
        }
    };
    for i in 0..segments {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) else { break };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        let mut keep = true;
        for (p, q) in [(-dx, a[0] - r[0]), (dx, r[2] - a[0]), (-dy, a[1] - r[1]), (dy, r[3] - a[1])] {
            if p == 0.0 {
                keep &= q >= 0.0;
            } else if p < 0.0 {
                t0 = t0.max(q / p);
            } else {
                t1 = t1.min(q / p);
            }
        }
        if !keep || t0 > t1 {
            end(&mut run, &mut runs);
            continue;
        }
        let (s, e) = ([a[0] + dx * t0, a[1] + dy * t0], [a[0] + dx * t1, a[1] + dy * t1]);
        if run.last() != Some(&s) {
            end(&mut run, &mut runs);
            run.push(s);
        }
        run.push(e);
        if t1 < 1.0 {
            end(&mut run, &mut runs);
        }
    }
    end(&mut run, &mut runs);
    runs
}

/// Ring `pts` cut to box `r`, Sutherland and Hodgman's clipping by each side of the box in turn. Empty when none of it is
/// inside.
pub(super) fn clip_ring(pts: &[Pt], r: &[f64; 4]) -> Vec<Pt> {
    if pts.iter().all(|q| inside_box(r, *q)) {
        return pts.to_vec();
    }
    let mut ring = pts.to_vec();
    // Each side of the box: which coordinate, its line, and whether inside is past it (above or right of it).
    for (axis, at, past) in [(0, r[0], true), (0, r[2], false), (1, r[1], true), (1, r[3], false)] {
        let inside = |q: &Pt| if past { q[axis] >= at } else { q[axis] <= at };
        let cross = |a: Pt, b: Pt| {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        };
        let n = ring.len();
        let mut next = Vec::with_capacity(n + 4);
        for i in 0..n {
            let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else { break };
            match (inside(&a), inside(&b)) {
                (true, true) => next.push(b),
                (true, false) => next.push(cross(a, b)),
                (false, true) => {
                    next.push(cross(a, b));
                    next.push(b);
                }
                (false, false) => {}
            }
        }
        ring = next;
        if ring.is_empty() {
            break;
        }
    }
    ring.dedup();
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 { Vec::new() } else { ring }
}

/// Polyline `pts` (closed back to its start when `closed`) in the dashes of `dash` (on, off, on, …, lengths in its units),
/// begun at the start of the first: the pieces that are on, each two points or more.
pub(super) fn dashed(pts: &[Pt], closed: bool, dash: &[f64]) -> Vec<Vec<Pt>> {
    let total: f64 = dash.iter().sum();
    if dash.len() < 2 || !(total > 0.0 && total.is_finite()) {
        return vec![pts.to_vec()];
    }
    let mut all = pts.to_vec();
    if closed && let Some(&first) = pts.first() {
        all.push(first);
    }
    let (mut out, mut piece): (Vec<Vec<Pt>>, Vec<Pt>) = (Vec::new(), Vec::new());
    let (mut i, mut left) = (0usize, dash[0]);
    // A pattern of zero-length pieces never moves on: held to as many pieces as the line could take.
    let mut steps = 0usize;
    let most = 1_000_000usize;
    for w in all.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = (b[0] - a[0]).hypot(b[1] - a[1]);
        let mut at = 0.0;
        while at < len && steps < most {
            steps += 1;
            let on = i % 2 == 0;
            let step = left.min(len - at);
            let p = |t: f64| [a[0] + (b[0] - a[0]) * t / len, a[1] + (b[1] - a[1]) * t / len];
            if on {
                if piece.is_empty() {
                    piece.push(p(at));
                }
                piece.push(p(at + step));
            }
            at += step;
            left -= step;
            if left <= 1e-12 {
                if on && piece.len() >= 2 {
                    out.push(std::mem::take(&mut piece));
                }
                piece.clear();
                i = (i + 1) % dash.len();
                left = dash[i];
            }
        }
    }
    if piece.len() >= 2 {
        out.push(piece);
    }
    out
}

/// How a stroke's ends and corners are drawn, as the page says them.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ends {
    pub cap: u8,
    pub join: u8,
    pub miter_limit: f64,
}

/// The outline of a stroke `width` wide along each of `lines` (each its points and whether it closes), in their units, as
/// rings that fill by odd parity.
pub(super) fn outline(lines: &[(Vec<Pt>, bool)], width: f64, ends: Ends) -> Vec<Vec<Pt>> {
    if !(width > 0.0 && width.is_finite()) {
        return Vec::new();
    }
    let cap = || match ends.cap {
        1 => LineCap::Round(std::f64::consts::PI / 12.0),
        2 => LineCap::Square,
        _ => LineCap::Butt,
    };
    let join = match ends.join {
        1 => LineJoin::Round(std::f64::consts::PI / 12.0),
        2 => LineJoin::Bevel,
        // A mitre past the limit is bevelled: the angle the limit allows.
        _ => LineJoin::Miter(2.0 * (1.0 / ends.miter_limit.max(1.0)).clamp(-1.0, 1.0).asin()),
    };
    let mut rings: Vec<Vec<Pt>> = Vec::new();
    for (pts, closed) in lines {
        let pts: Vec<Pt> =
            pts.iter().copied().filter(|p| p[0].is_finite() && p[1].is_finite() && p[0].abs() < FAR && p[1].abs() < FAR).collect();
        if pts.len() < 2 {
            continue;
        }
        let style = StrokeStyle::new(width).start_cap(cap()).end_cap(cap()).line_join(join.clone());
        rings.extend(flat(pts.stroke(style, *closed)));
    }
    if lines.len() > 1 {
        let none: Vec<Vec<Pt>> = Vec::new();
        return flat(checked(&rings).overlay(&none, OverlayRule::Subject, Rule::NonZero));
    }
    rings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(x: f64, y: f64, s: f64) -> Vec<Pt> {
        vec![[x, y], [x + s, y], [x + s, y + s], [x, y + s]]
    }

    #[test]
    fn a_triangle_clip_cuts_a_line_and_a_fill_to_it() {
        let page = Region::Box([0.0, 0.0, 200.0, 200.0]);
        let tri = page.within(&[vec![[20.0, 20.0], [180.0, 20.0], [20.0, 180.0]]], false);
        assert!(matches!(tri, Region::Rings { .. }));
        let cut = tri.lines(&[[0.0, 150.0], [200.0, 150.0]], false);
        assert_eq!(cut.len(), 1, "{cut:?}");
        let xs: Vec<f64> = cut[0].iter().map(|p| p[0]).collect();
        let (lo, hi) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        assert!((lo - 20.0).abs() < 1e-3 && (hi - 50.0).abs() < 1e-3, "{cut:?}");
        let fill = tri.fill(vec![square(0.0, 0.0, 200.0)]);
        let area: f64 =
            fill.iter().map(|r| crate::geom::area(&r.iter().map(|p| crate::geom::P2(p[0], p[1])).collect::<Vec<_>>()).abs()).sum();
        assert!((area - 160.0 * 160.0 / 2.0).abs() < 1.0, "{area}");
    }

    #[test]
    fn non_zero_rings_resolve_to_odd_parity() {
        // Two squares wound alike that overlap: one ring, filled through.
        let r = resolve(&[square(0.0, 0.0, 10.0), square(5.0, 5.0, 10.0)], false);
        assert_eq!(r.len(), 1, "{r:?}");
        // A square with a square inside wound alike: non-zero fills it through.
        let r = resolve(&[square(0.0, 0.0, 10.0), square(3.0, 3.0, 4.0)], false);
        assert_eq!(r.len(), 1, "{r:?}");
        // Wound against it: a hole.
        let mut hole = square(3.0, 3.0, 4.0);
        hole.reverse();
        assert_eq!(resolve(&[square(0.0, 0.0, 10.0), hole], false).len(), 2);
        // By odd parity, kept as they are.
        assert_eq!(resolve(&[square(0.0, 0.0, 10.0), square(3.0, 3.0, 4.0)], true).len(), 2);
    }

    #[test]
    fn dashes_are_laid_along_a_line_and_a_stroke_outlined() {
        let d = dashed(&[[0.0, 0.0], [40.0, 0.0]], false, &[12.0, 3.0, 2.0, 3.0]);
        let lens: Vec<f64> = d.iter().map(|p| p.last().unwrap()[0] - p[0][0]).collect();
        assert_eq!(lens.len(), 4, "{d:?}");
        assert!((lens[0] - 12.0).abs() < 1e-9 && (lens[1] - 2.0).abs() < 1e-9 && (lens[2] - 12.0).abs() < 1e-9, "{lens:?}");
        let o = outline(&[(vec![[0.0, 0.0], [10.0, 0.0]], false)], 2.0, Ends { cap: 0, join: 0, miter_limit: 10.0 });
        let b = around(o.iter().flatten()).unwrap();
        assert!((b[1] + 1.0).abs() < 1e-6 && (b[3] - 1.0).abs() < 1e-6 && b[0].abs() < 1e-6 && (b[2] - 10.0).abs() < 1e-6, "{b:?}");
    }
}

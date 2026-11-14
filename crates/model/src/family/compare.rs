//! How near two drawings of one family are, by their geometry: BIMCraft's rebuild of a type against Revit's own result for
//! it ([`super::check`]). Shapes are compared surface to surface both ways: points spread evenly over one's triangles, each
//! measured to the nearest point of the other's, so a part mirrored, turned upside down, moved or missing is as far off as
//! it really is, whatever the totals say. A point on a face inside the drawing's own solid (where forms overlap or touch) is
//! no part of its outer surface and isn't measured ([`Solid`]). Plan and elevation lines are compared the same way, point by
//! point along them.

use std::collections::HashMap;

use crate::geom::P2;
use crate::geom::v3::{P3, add, cross, dot, mul, sub};

/// Points spread over a shape's surface, or along a drawing's lines, at most.
const SAMPLES: usize = 6000;
/// Triangle by point tests, at most, that deciding whether faces lie inside their own solid may cost in one comparison: past
/// it the rest are measured as they are, never skipped.
const WINDING_WORK: usize = 40_000_000;
/// How far to either side of a face a point is tested for lying inside its own solid, mm.
const SIDE: f64 = 0.05;
/// The winding number from which a point counts as inside a solid.
const INSIDE: f64 = 0.75;
/// Cells a grid spreads a shape over along its longest side.
const CELLS: f64 = 48.0;
/// Points off by more than the tolerance measured out to the search's reach, at most: the rest are only counted.
const FAR: usize = 128;

/// How far one drawing is from another: the farthest of its points from the other (capped at the search's reach), the
/// share of it (by area, or length) farther than the tolerance, and where its farthest point is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub worst: f64,
    pub share: f64,
    pub at: P3,
}

impl Gap {
    const NONE: Gap = Gap { worst: 0.0, share: 0.0, at: [0.0; 3] };
}

/// The point of triangle `t` nearest `p` (Ericson, Real-Time Collision Detection, 5.1.5).
fn nearest_on(p: P3, [a, b, c]: [P3; 3]) -> P3 {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return add(a, mul(ab, v));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return add(a, mul(ac, w));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add(b, mul(sub(c, b), w));
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-30 {
        return a;
    }
    let (v, w) = (vb / denom, vc / denom);
    add(a, add(mul(ab, v), mul(ac, w)))
}

fn length(v: P3) -> f64 {
    dot(v, v).sqrt()
}

fn area(t: &[P3; 3]) -> f64 {
    length(cross(sub(t[1], t[0]), sub(t[2], t[0]))) / 2.0
}

/// A grid of cells over a box, each listing what lies in it: what a nearest point is looked for in.
struct Grid {
    lo: P3,
    cell: f64,
    cells: HashMap<[i64; 3], Vec<usize>>,
    /// The least and the most cell keys anything is in.
    span: [[i64; 3]; 2],
    /// What reaches too many cells to be listed in each: looked at for every point instead.
    big: Vec<usize>,
    /// How many places the cells list items in all.
    entries: usize,
}

/// An item that touches more cells than this is kept apart (`Grid::big`), not listed in each.
const MOST_CELLS: f64 = 512.0;
/// The most places the cells list items in all; past that, items are kept apart.
const MOST_ENTRIES: usize = 20_000_000;

impl Grid {
    fn new(lo: P3, hi: P3, near: f64) -> Grid {
        let side = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
        Grid {
            lo,
            cell: (side / CELLS).max(near * 2.0).max(1.0),
            cells: HashMap::new(),
            span: [[i64::MAX; 3], [i64::MIN; 3]],
            big: Vec::new(),
            entries: 0,
        }
    }

    fn key(&self, p: P3) -> [i64; 3] {
        std::array::from_fn(|k| ((p[k] - self.lo[k]) / self.cell).floor() as i64)
    }

    /// Item `i` in every cell its box (`lo` to `hi`) touches.
    fn put(&mut self, i: usize, lo: P3, hi: P3) {
        let (a, b) = (self.key(lo), self.key(hi));
        // A long or diagonal triangle across the whole box would be in tens of thousands of cells; one of a hundred thousand
        // such would be in more than memory holds.
        let cells = (0..3).map(|k| (b[k] - a[k] + 1) as f64).product::<f64>();
        if cells > MOST_CELLS || self.entries as f64 + cells > MOST_ENTRIES as f64 {
            self.big.push(i);
            return;
        }
        self.entries += cells as usize;
        for k in 0..3 {
            self.span[0][k] = self.span[0][k].min(a[k]);
            self.span[1][k] = self.span[1][k].max(b[k]);
        }
        for x in a[0]..=b[0] {
            for y in a[1]..=b[1] {
                for z in a[2]..=b[2] {
                    self.cells.entry([x, y, z]).or_default().push(i);
                }
            }
        }
    }

    /// The items in the cells within `reach` of `p`, each cell once (an item may come more than once): every cell's where
    /// that reaches more cells than it holds.
    fn around(&self, p: P3, reach: f64) -> Box<dyn Iterator<Item = usize> + '_> {
        let (a, b) = (self.key(sub(p, [reach; 3])), self.key(add(p, [reach; 3])));
        let a: [i64; 3] = std::array::from_fn(|k| a[k].max(self.span[0][k]));
        let b: [i64; 3] = std::array::from_fn(|k| b[k].min(self.span[1][k]));
        let big = self.big.iter().copied();
        if (0..3).any(|k| a[k] > b[k]) {
            return Box::new(big);
        }
        let count = (0..3).map(|k| (b[k] - a[k] + 1) as f64).product::<f64>();
        if count > self.cells.len() as f64 {
            return Box::new(
                self.cells
                    .iter()
                    .filter(move |(k, _)| (0..3).all(|i| k[i] >= a[i] && k[i] <= b[i]))
                    .flat_map(|(_, v)| v.iter().copied())
                    .chain(big),
            );
        }
        Box::new(
            (a[0]..=b[0])
                .flat_map(move |x| (a[1]..=b[1]).flat_map(move |y| (a[2]..=b[2]).map(move |z| [x, y, z])))
                .filter_map(|k| self.cells.get(&k))
                .flatten()
                .copied()
                .chain(big),
        )
    }
}

fn bounds(points: impl Iterator<Item = P3>) -> Option<(P3, P3)> {
    points.fold(None, |b, p| {
        Some(match b {
            None => (p, p),
            Some((lo, hi)) => (std::array::from_fn(|k| f64::min(lo[k], p[k])), std::array::from_fn(|k| f64::max(hi[k], p[k]))),
        })
    })
}

/// A shape's surface, its triangles in a grid: what the nearest point to a point is found on.
pub struct Surface<'a> {
    tris: &'a [[P3; 3]],
    grid: Grid,
}

impl<'a> Surface<'a> {
    /// `tris` in a grid fit for finding what is within `near` of a point, and farther out.
    pub fn new(tris: &'a [[P3; 3]], near: f64) -> Option<Surface<'a>> {
        let (lo, hi) = bounds(tris.iter().flatten().copied())?;
        let mut grid = Grid::new(lo, hi, near);
        for (i, t) in tris.iter().enumerate() {
            let (a, b) = bounds(t.iter().copied())?;
            grid.put(i, a, b);
        }
        Some(Surface { tris, grid })
    }

    /// How far `p` is from it, looked for within `reach`: `reach` where nothing is nearer. Looked for near first, as most
    /// points are.
    fn distance(&self, p: P3, reach: f64) -> f64 {
        let mut r = (self.grid.cell / 2.0).min(reach);
        loop {
            let mut best = r;
            for i in self.grid.around(p, r) {
                best = best.min(length(sub(p, nearest_on(p, self.tris[i]))));
            }
            if best < r || r >= reach {
                return best;
            }
            r = (r * 4.0).min(reach);
        }
    }
}

/// A shape's solid, its triangles taken as closed surfaces however many lumps they make (forms that overlap, or touch with
/// faces face to face): what says whether a point lies inside it, by the generalised winding number.
pub struct Solid<'a> {
    tris: &'a [[P3; 3]],
    bounds: Option<(P3, P3)>,
}

impl<'a> Solid<'a> {
    pub fn new(tris: &'a [[P3; 3]]) -> Solid<'a> {
        Solid { tris, bounds: bounds(tris.iter().flatten().copied()) }
    }

    /// How many times `p` is inside it: the sum of the solid angles its triangles subtend at `p` over 4π (Van Oosterom and
    /// Strackee), 1 inside a lump, 2 inside two that overlap, 0 outside. Never defined on its surface.
    pub fn winding(&self, p: P3) -> f64 {
        let mut sum = 0.0;
        for [a, b, c] in self.tris {
            let (a, b, c) = (sub(*a, p), sub(*b, p), sub(*c, p));
            let (la, lb, lc) = (length(a), length(b), length(c));
            let denom = la * lb * lc + dot(a, b) * lc + dot(a, c) * lb + dot(b, c) * la;
            sum += 2.0 * dot(a, cross(b, c)).atan2(denom);
        }
        sum / (4.0 * std::f64::consts::PI)
    }

    /// Whether a point on a face of this solid, whose unit normal is `n`, lies inside it on both sides of the face: the face is
    /// between parts, not on the solid's outer surface. `work` is what is left to spend on triangles tested; a point with no
    /// work left to decide it is not inside.
    fn encloses(&self, p: P3, n: P3, work: &mut usize) -> bool {
        let Some((lo, hi)) = self.bounds else { return false };
        for side in [SIDE, -SIDE] {
            let q = add(p, mul(n, side));
            if (0..3).any(|k| q[k] < lo[k] || q[k] > hi[k]) || *work < self.tris.len() {
                return false;
            }
            *work -= self.tris.len();
            if self.winding(q) < INSIDE {
                return false;
            }
        }
        true
    }
}

/// Points spread over `tris`, about `SAMPLES` of them by area (a small triangle gets its corners and its middle at least),
/// each with the area it stands for and its triangle's unit normal (a triangle of no area has no points).
fn spread(tris: &[[P3; 3]]) -> Vec<(P3, f64, P3)> {
    let total: f64 = tris.iter().map(area).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let step = (total / SAMPLES as f64).sqrt();
    let mut out = Vec::new();
    for t in tris {
        let a = area(t);
        if a <= 1e-12 {
            continue;
        }
        let normal = mul(cross(sub(t[1], t[0]), sub(t[2], t[0])), 1.0 / (2.0 * a));
        let longest = (0..3).map(|k| length(sub(t[(k + 1) % 3], t[k]))).fold(0.0, f64::max);
        let n = ((longest / step).ceil() as usize).clamp(1, 64);
        // A lattice of n steps along two of its sides, and its middle: each point stands for an even share of its area.
        let mut pts = vec![mul(add(add(t[0], t[1]), t[2]), 1.0 / 3.0)];
        if n > 1 || a > step * step {
            for i in 0..=n {
                for j in 0..=(n - i) {
                    let (u, v) = (i as f64 / n as f64, j as f64 / n as f64);
                    pts.push(add(t[0], add(mul(sub(t[1], t[0]), u), mul(sub(t[2], t[0]), v))));
                }
            }
        }
        let share = a / pts.len() as f64;
        out.extend(pts.into_iter().map(|p| (p, share, normal)));
    }
    // Too many: every k-th kept, its weight taken up.
    if out.len() > SAMPLES * 4 {
        let k = out.len().div_ceil(SAMPLES * 4);
        out = out.chunks(k).map(|c| (c[0].0, c.iter().map(|q| q.1).sum(), c[0].2)).collect();
    }
    out
}

/// How far `a`'s surface is from `b`'s: each of `a`'s points measured to `b`, those farther than `near` counted, the farthest
/// looked for within `reach`. With `own` (`a`'s solid), a point farther than `near` that lies inside it on both sides of its
/// face is left out, weight and all: `a`'s joined forms are one solid, and a face between them is no surface of it.
pub fn surface_gap(a: &[[P3; 3]], b: &Surface, near: f64, reach: f64, own: Option<&Solid>) -> Gap {
    let points = spread(a);
    let total: f64 = points.iter().map(|p| p.1).sum();
    if total <= 0.0 {
        return Gap::NONE;
    }
    let mut gap = Gap::NONE;
    let (mut off, mut far) = (0.0, 0);
    let mut work = WINDING_WORK;
    let mut inside = 0.0;
    for (p, w, n) in points {
        let d = b.distance(p, near * 1.000_001);
        if d <= near {
            continue;
        }
        if let Some(s) = own
            && s.encloses(p, n, &mut work)
        {
            inside += w;
            continue;
        }
        off += w;
        far += 1;
        let d = if far <= FAR { b.distance(p, reach) } else { d };
        if d > gap.worst {
            gap.worst = d;
            gap.at = p;
        }
    }
    // What stays is a share of the surface that stays.
    gap.share = if total > inside { off / (total - inside) } else { 0.0 };
    gap
}

/// How far under the cut plane a corner may be and still lie on it, mm: Revit counts a face lying on the cut plane as not cut,
/// and a coordinate a millionth under it is on it (a trim head's underside worked out as 1199.99999999 for a cut at 1200).
const ON_PLANE: f64 = 1e-6;

/// What the plane at height `z` cuts of a solid's triangles, as segments: for each triangle with corners on both sides of `z`
/// (one at `z`, or within a millionth of a millimetre under it, is above it), the segment between its two edges' crossings, as
/// a plan draws what its cut plane cuts. A segment shorter than a millionth of a millimetre is left out.
pub fn section(tris: &[[P3; 3]], z: f64) -> Vec<[P2; 2]> {
    let mut out = Vec::new();
    for t in tris {
        let above = t.map(|c| c[2] >= z - ON_PLANE);
        if above.iter().all(|a| *a) || above.iter().all(|a| !*a) {
            continue;
        }
        let mut at = Vec::with_capacity(2);
        for k in 0..3 {
            let (i, j) = (k, (k + 1) % 3);
            if above[i] != above[j] {
                let (a, b) = (t[i], t[j]);
                let u = (z - a[2]) / (b[2] - a[2]);
                at.push(P2(a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u));
            }
        }
        if let [a, b] = at.as_slice()
            && ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() >= 1e-6
        {
            out.push([*a, *b]);
        }
    }
    out
}

/// [`section`] joined end to end into runs of points, each with whether it closes on itself: what a plan draws of a solid its cut
/// plane crosses, a jamb's section as a closed ring and a rebated frame's as one ring with the rebate in it. Points the same to
/// a hundredth of a millimetre are one; a point on the straight line between its neighbours is left out.
pub fn section_runs(tris: &[[P3; 3]], z: f64) -> Vec<(Vec<P2>, bool)> {
    let segments = section(tris, z);
    let key = |p: P2| ((p.0 * 100.0).round() as i64, (p.1 * 100.0).round() as i64);
    let mut at: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    for (i, [a, b]) in segments.iter().enumerate() {
        at.entry(key(*a)).or_default().push(i);
        at.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; segments.len()];
    // The run's end continued by any segment not used yet that starts there.
    let extend = |run: &mut Vec<P2>, used: &mut Vec<bool>| {
        while let Some(end) = run.last().copied() {
            let Some(next) = at.get(&key(end)).and_then(|l| l.iter().copied().find(|j| !used[*j])) else { break };
            used[next] = true;
            let [a, b] = segments[next];
            run.push(if key(a) == key(end) { b } else { a });
        }
    };
    let mut out = Vec::new();
    for i in 0..segments.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut run = vec![segments[i][0], segments[i][1]];
        extend(&mut run, &mut used);
        run.reverse();
        extend(&mut run, &mut used);
        let closed = run.len() > 3 && key(run[0]) == key(run[run.len() - 1]);
        if closed {
            run.pop();
        }
        // Without the points that lie on the line between their neighbours (where a face's two triangles met).
        let n = run.len();
        let straight = |a: P2, b: P2, c: P2| ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() < 1e-6 * (1.0 + a.dist(c));
        let keep: Vec<P2> = (0..n)
            .filter(|k| {
                let prev = if *k == 0 { closed.then(|| run[n - 1]) } else { Some(run[k - 1]) };
                let next = if k + 1 == n { closed.then(|| run[0]) } else { Some(run[k + 1]) };
                !matches!((prev, next), (Some(a), Some(c)) if straight(a, run[*k], c))
            })
            .map(|k| run[k])
            .collect();
        out.push((keep, closed));
    }
    out
}

/// A drawing's lines, each a run of points, as segments in a grid: what the nearest point to a point is found on.
pub struct Lines {
    segs: Vec<[P2; 2]>,
    grid: Grid,
}

fn flat(p: P2) -> P3 {
    [p.0, p.1, 0.0]
}

pub fn near_on_segment(p: P2, [a, b]: [P2; 2]) -> P2 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = dx * dx + dy * dy;
    if l <= 0.0 {
        return a;
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l).clamp(0.0, 1.0);
    P2(a.0 + dx * t, a.1 + dy * t)
}

impl Lines {
    /// `runs`' segments (a lone point is one of no length) in a grid fit for what is within `near`.
    pub fn new(runs: &[Vec<P2>], near: f64) -> Option<Lines> {
        let mut segs = Vec::new();
        for r in runs {
            match r.as_slice() {
                [] => {}
                [p] => segs.push([*p, *p]),
                _ => segs.extend(r.windows(2).map(|w| [w[0], w[1]])),
            }
        }
        let (lo, hi) = bounds(segs.iter().flatten().map(|p| flat(*p)))?;
        let mut grid = Grid::new(lo, hi, near);
        for (i, [a, b]) in segs.iter().enumerate() {
            let (l, h) = bounds([flat(*a), flat(*b)].into_iter())?;
            grid.put(i, l, h);
        }
        Some(Lines { segs, grid })
    }

    fn distance(&self, p: P2, reach: f64) -> f64 {
        let mut r = (self.grid.cell / 2.0).min(reach);
        loop {
            let mut best = r;
            for i in self.grid.around(flat(p), r) {
                let q = near_on_segment(p, self.segs[i]);
                best = best.min(((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt());
            }
            if best < r || r >= reach {
                return best;
            }
            r = (r * 4.0).min(reach);
        }
    }
}

/// How far `a`'s lines are from `b`'s: points along each of `a`'s (and its lone points) measured to `b`, those farther than
/// `near` counted by the length they stand for.
pub fn line_gap(a: &[Vec<P2>], b: &Lines, near: f64, reach: f64) -> Gap {
    let total: f64 = a.iter().flat_map(|r| r.windows(2)).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
    let step = (total / SAMPLES as f64).max(near / 2.0).max(0.5);
    let mut points: Vec<(P2, f64)> = Vec::new();
    for r in a {
        if let [p] = r.as_slice() {
            points.push((*p, step));
        }
        for w in r.windows(2) {
            let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            let n = ((l / step).ceil() as usize).clamp(1, 4096);
            for k in 0..=n {
                let t = k as f64 / n as f64;
                points.push((P2(w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t), l / (n + 1) as f64));
            }
        }
    }
    let total: f64 = points.iter().map(|p| p.1).sum();
    if total <= 0.0 {
        return Gap::NONE;
    }
    let mut gap = Gap::NONE;
    let (mut off, mut far) = (0.0, 0);
    for (p, w) in points {
        let d = b.distance(p, near * 1.000_001);
        if d <= near {
            continue;
        }
        off += w;
        far += 1;
        let d = if far <= FAR { b.distance(p, reach) } else { d };
        if d > gap.worst {
            gap.worst = d;
            gap.at = flat(p);
        }
    }
    gap.share = off / total;
    gap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cuboid(lo: P3, hi: P3) -> Vec<[P3; 3]> {
        let p = |i: usize| -> P3 {
            [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]
        };
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        quads.iter().flat_map(|[a, b, c, d]| [[p(*a), p(*b), p(*c)], [p(*a), p(*c), p(*d)]]).collect()
    }

    #[test]
    fn triangles_across_the_whole_box_are_kept_apart_not_listed_in_every_cell() {
        // Each reaches nearly every cell of the grid (48 by 48 by 48): listed in each, these would be millions of entries.
        let tris: Vec<[P3; 3]> = (0..200)
            .map(|k| {
                let d = k as f64 * 4.0;
                [[0.0, 0.0, 0.0], [1000.0, d, 1000.0], [d, 1000.0, 1000.0]]
            })
            .collect();
        let surface = Surface::new(&tris, 5.0).unwrap();
        assert!(surface.grid.entries < 1_000_000, "{} entries", surface.grid.entries);
        assert_eq!(surface.grid.big.len(), 200);
        for p in [[500.0, 500.0, 500.0], [10.0, 900.0, 20.0], [2000.0, 0.0, 0.0]] {
            let brute = tris.iter().map(|t| length(sub(p, nearest_on(p, *t)))).fold(f64::INFINITY, f64::min);
            assert!((surface.distance(p, 5000.0) - brute.min(5000.0)).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn a_shape_is_near_itself_and_as_far_from_a_moved_copy_as_it_was_moved() {
        let a = cuboid([0.0; 3], [1000.0, 500.0, 750.0]);
        let s = Surface::new(&a, 2.0).unwrap();
        let g = surface_gap(&a, &s, 2.0, 200.0, None);
        assert!(g.worst < 1e-6 && g.share == 0.0, "{g:?}");
        let b = cuboid([30.0, 0.0, 0.0], [1030.0, 500.0, 750.0]);
        let g = surface_gap(&b, &s, 2.0, 200.0, None);
        assert!((g.worst - 30.0).abs() < 1e-6 && g.share > 0.0, "{g:?}");
    }

    #[test]
    fn a_mirrored_l_is_far_from_the_l_though_its_volume_and_box_are_the_same() {
        // An L of two boxes, its leg on +x; mirrored, on -x.
        let mut l = cuboid([-500.0, 0.0, 0.0], [500.0, 100.0, 100.0]);
        l.extend(cuboid([400.0, 100.0, 0.0], [500.0, 600.0, 100.0]));
        let mut m = cuboid([-500.0, 0.0, 0.0], [500.0, 100.0, 100.0]);
        m.extend(cuboid([-500.0, 100.0, 0.0], [-400.0, 600.0, 100.0]));
        let s = Surface::new(&l, 2.0).unwrap();
        let g = surface_gap(&m, &s, 2.0, 2000.0, None);
        // The leg's far end is 500 from the bar, the nearest of the L.
        assert!(g.worst > 400.0 && g.share > 0.2, "{g:?}");
    }

    #[test]
    fn lines_are_near_their_own_and_far_from_lines_moved() {
        let a = vec![vec![P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 500.0)]];
        let l = Lines::new(&a, 1.0).unwrap();
        assert!(line_gap(&a, &l, 1.0, 100.0).worst < 1e-9);
        let b = vec![vec![P2(0.0, 5.0), P2(1000.0, 5.0), P2(1000.0, 500.0)]];
        let g = line_gap(&b, &l, 1.0, 100.0);
        assert!((g.worst - 5.0).abs() < 1e-9 && g.share > 0.5, "{g:?}");
    }

    /// `a`'s outer surface measured to `b`, with `a`'s own solid taken into account.
    fn outer_gap(a: &[[P3; 3]], b: &[[P3; 3]]) -> Gap {
        let s = Surface::new(b, 2.0).unwrap();
        surface_gap(a, &s, 2.0, 500.0, Some(&Solid::new(a)))
    }

    #[test]
    fn faces_inside_the_solid_are_not_compared() {
        // Two cuboids overlapping, against the one cuboid they make together.
        let mut ours = cuboid([0.0, 0.0, 0.0], [600.0, 100.0, 100.0]);
        ours.extend(cuboid([400.0, 0.0, 0.0], [1000.0, 100.0, 100.0]));
        let one = cuboid([0.0, 0.0, 0.0], [1000.0, 100.0, 100.0]);
        let (a, b) = (outer_gap(&ours, &one), outer_gap(&one, &ours));
        assert!(a.worst <= 2.0 && b.worst <= 2.0, "{a:?} {b:?}");
        // Without its solid, the faces inside it are off by tens of millimetres.
        let s = Surface::new(&one, 2.0).unwrap();
        assert!(surface_gap(&ours, &s, 2.0, 500.0, None).worst > 10.0);
        // Two lumps sharing a face, against one cuboid.
        let mut lumps = cuboid([0.0, 0.0, 0.0], [500.0, 100.0, 100.0]);
        lumps.extend(cuboid([500.0, 0.0, 0.0], [1000.0, 100.0, 100.0]));
        let (a, b) = (outer_gap(&lumps, &one), outer_gap(&one, &lumps));
        assert!(a.worst <= 2.0 && b.worst <= 2.0, "{a:?} {b:?}");
        let s = Surface::new(&one, 2.0).unwrap();
        assert!(surface_gap(&lumps, &s, 2.0, 500.0, None).worst > 10.0);
    }

    #[test]
    fn a_missing_part_is_still_far() {
        let ours = cuboid([0.0, 0.0, 0.0], [1000.0, 100.0, 100.0]);
        let mut revit = ours.clone();
        revit.extend(cuboid([1200.0, 0.0, 0.0], [1300.0, 100.0, 100.0]));
        let (a, b) = (outer_gap(&ours, &revit), outer_gap(&revit, &ours));
        assert!(a.worst <= 2.0, "{a:?}");
        assert!(b.worst >= 200.0 - 1e-6 && b.share > 0.0, "{b:?}");
    }

    #[test]
    fn a_hollow_inside_a_solid_is_still_its_surface() {
        // A cuboid with a closed cavity: the cavity's faces face into the hollow, so they are not inside the solid.
        let mut ours = cuboid([0.0, 0.0, 0.0], [1000.0, 1000.0, 1000.0]);
        let cavity = cuboid([400.0, 400.0, 400.0], [600.0, 600.0, 600.0]);
        ours.extend(cavity.iter().map(|[a, b, c]| [*a, *c, *b]));
        let outer = cuboid([0.0, 0.0, 0.0], [1000.0, 1000.0, 1000.0]);
        let g = outer_gap(&ours, &outer);
        assert!(g.worst > 100.0 && g.share > 0.0, "{g:?}");
    }

    #[test]
    fn the_winding_number_counts_the_lumps_a_point_is_in() {
        let mut t = cuboid([0.0; 3], [600.0, 100.0, 100.0]);
        t.extend(cuboid([400.0, 0.0, 0.0], [1000.0, 100.0, 100.0]));
        let s = Solid::new(&t);
        assert!((s.winding([100.0, 50.0, 50.0]) - 1.0).abs() < 1e-9);
        assert!((s.winding([500.0, 50.0, 50.0]) - 2.0).abs() < 1e-9);
        assert!(s.winding([1500.0, 50.0, 50.0]).abs() < 1e-9);
    }

    #[test]
    fn a_section_joins_into_closed_rings_with_only_its_corners() {
        // A box cut across: one ring of its four corners, the two triangles of each face met at no point of their own.
        let one = section_runs(&cuboid([0.0, 0.0, 0.0], [50.0, 200.0, 100.0]), 50.0);
        assert_eq!(one.len(), 1, "{one:?}");
        assert!(one[0].1 && one[0].0.len() == 4, "{one:?}");
        // Two boxes side by side, the second shorter (a jamb and its stop): their sections are rings of their own, the second's
        // corner at the jamb's face among the points.
        let mut tris = cuboid([0.0, 0.0, 0.0], [50.0, 200.0, 100.0]);
        tris.extend(cuboid([50.0, 0.0, 0.0], [70.0, 40.0, 100.0]));
        let runs = section_runs(&tris, 50.0);
        assert!(runs.iter().all(|r| r.1), "{runs:?}");
        assert!(runs.iter().flat_map(|r| &r.0).any(|p| (p.0 - 70.0).abs() < 1e-6 && (p.1 - 40.0).abs() < 1e-6), "{runs:?}");
        // Nothing cut, nothing drawn.
        assert!(section_runs(&one_box_above(), 50.0).is_empty());
    }

    fn one_box_above() -> Vec<[P3; 3]> {
        cuboid([0.0, 0.0, 60.0], [10.0, 10.0, 70.0])
    }

    #[test]
    fn a_section_is_what_the_plane_cuts() {
        // A 100 x 200 x 300 box cut at 150 draws its four sides: a rectangle round it.
        let b = cuboid([0.0; 3], [100.0, 200.0, 300.0]);
        let cut = section(&b, 150.0);
        let len: f64 = cut.iter().map(|[a, b]| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()).sum();
        assert!((len - 600.0).abs() < 1e-6, "{len}");
        assert!(cut.iter().flatten().all(|p| p.0 == 0.0 || p.0 == 100.0 || p.1 == 0.0 || p.1 == 200.0), "{cut:?}");
        // Above the box, below it, or at its bottom face (a corner at exactly z is above it): nothing.
        assert!(section(&b, 400.0).is_empty() && section(&b, -1.0).is_empty() && section(&b, 0.0).is_empty());
    }

    #[test]
    fn a_head_whose_underside_is_on_the_cut_plane_is_not_cut() {
        // A U-shaped trim: two jambs 50 thick and a head between them whose underside works out as 1199.9999999 for a cut at 1200
        // (Height 1200). Revit counts a face on the cut plane as not cut: the jambs' sections are drawn, no line along the head.
        let mut u = cuboid([-400.0, 100.0, 0.0], [-350.0, 120.0, 1250.0]);
        u.extend(cuboid([350.0, 100.0, 0.0], [400.0, 120.0, 1250.0]));
        u.extend(cuboid([-350.0, 100.0, 1_199.999_999_9], [350.0, 120.0, 1250.0]));
        let runs = section_runs(&u, 1200.0);
        assert_eq!(runs.len(), 2, "two jamb rings and nothing along the head: {runs:?}");
        assert!(runs.iter().all(|r| r.1), "{runs:?}");
        assert!(section(&u, 1200.0).iter().flatten().all(|p| p.0.abs() >= 350.0 - 1e-9), "nothing between the jambs");
        // A bottom on the plane, or a millionth under it, is not cut; one a thousandth under it still is.
        assert!(section(&cuboid([0.0, 0.0, 1200.0], [10.0, 10.0, 1300.0]), 1200.0).is_empty(), "a bottom on the plane");
        assert!(section(&cuboid([0.0, 0.0, 1_199.999_999_9], [10.0, 10.0, 1300.0]), 1200.0).is_empty(), "a bottom a hair under it");
        assert!(!section(&cuboid([0.0, 0.0, 1199.999], [10.0, 10.0, 1300.0]), 1200.0).is_empty(), "a bottom a thousandth under it");
    }
}

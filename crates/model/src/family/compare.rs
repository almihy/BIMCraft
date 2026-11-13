//! How near two drawings of one family are, by their geometry: BIMCraft's rebuild of a type against Revit's own result for
//! it ([`super::check`]). Shapes are compared surface to surface both ways: points spread evenly over one's triangles, each
//! measured to the nearest point of the other's, so a part mirrored, turned upside down, moved or missing is as far off as
//! it really is, whatever the totals say. Plan and elevation lines are compared the same way, point by point along them.

use std::collections::HashMap;

use crate::geom::P2;
use crate::geom::v3::{P3, add, cross, dot, mul, sub};

/// Points spread over a shape's surface, or along a drawing's lines, at most.
const SAMPLES: usize = 6000;
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
}

impl Grid {
    fn new(lo: P3, hi: P3, near: f64) -> Grid {
        let side = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max);
        Grid { lo, cell: (side / CELLS).max(near * 2.0).max(1.0), cells: HashMap::new(), span: [[i64::MAX; 3], [i64::MIN; 3]] }
    }

    fn key(&self, p: P3) -> [i64; 3] {
        std::array::from_fn(|k| ((p[k] - self.lo[k]) / self.cell).floor() as i64)
    }

    /// Item `i` in every cell its box (`lo` to `hi`) touches.
    fn put(&mut self, i: usize, lo: P3, hi: P3) {
        let (a, b) = (self.key(lo), self.key(hi));
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
        if (0..3).any(|k| a[k] > b[k]) {
            return Box::new(std::iter::empty());
        }
        let count = (0..3).map(|k| (b[k] - a[k] + 1) as f64).product::<f64>();
        if count > self.cells.len() as f64 {
            return Box::new(
                self.cells.iter().filter(move |(k, _)| (0..3).all(|i| k[i] >= a[i] && k[i] <= b[i])).flat_map(|(_, v)| v.iter().copied()),
            );
        }
        Box::new(
            (a[0]..=b[0])
                .flat_map(move |x| (a[1]..=b[1]).flat_map(move |y| (a[2]..=b[2]).map(move |z| [x, y, z])))
                .filter_map(|k| self.cells.get(&k))
                .flatten()
                .copied(),
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

/// Points spread over `tris`, about `SAMPLES` of them by area (a small triangle gets its corners and its middle at least),
/// each with the area it stands for.
fn spread(tris: &[[P3; 3]]) -> Vec<(P3, f64)> {
    let total: f64 = tris.iter().map(area).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let step = (total / SAMPLES as f64).sqrt();
    let mut out = Vec::new();
    for t in tris {
        let a = area(t);
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
        out.extend(pts.into_iter().map(|p| (p, share)));
    }
    // Too many: every k-th kept, its weight taken up.
    if out.len() > SAMPLES * 4 {
        let k = out.len().div_ceil(SAMPLES * 4);
        out = out.chunks(k).map(|c| (c[0].0, c.iter().map(|q| q.1).sum())).collect();
    }
    out
}

/// How far `a`'s surface is from `b`'s: each of `a`'s points measured to `b`, those farther than `near` counted, the farthest
/// looked for within `reach`.
pub fn surface_gap(a: &[[P3; 3]], b: &Surface, near: f64, reach: f64) -> Gap {
    let points = spread(a);
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
            gap.at = p;
        }
    }
    gap.share = off / total;
    gap
}

/// A drawing's lines, each a run of points, as segments in a grid: what the nearest point to a point is found on.
pub struct Lines {
    segs: Vec<[P2; 2]>,
    grid: Grid,
}

fn flat(p: P2) -> P3 {
    [p.0, p.1, 0.0]
}

fn near_on_segment(p: P2, [a, b]: [P2; 2]) -> P2 {
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
    fn a_shape_is_near_itself_and_as_far_from_a_moved_copy_as_it_was_moved() {
        let a = cuboid([0.0; 3], [1000.0, 500.0, 750.0]);
        let s = Surface::new(&a, 2.0).unwrap();
        let g = surface_gap(&a, &s, 2.0, 200.0);
        assert!(g.worst < 1e-6 && g.share == 0.0, "{g:?}");
        let b = cuboid([30.0, 0.0, 0.0], [1030.0, 500.0, 750.0]);
        let g = surface_gap(&b, &s, 2.0, 200.0);
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
        let g = surface_gap(&m, &s, 2.0, 2000.0);
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
}

//! A hatch drawn with its pattern ([`crate::dxf::Tile`], a PDF's tiling pattern): its cell laid again at each of its steps
//! over the part of it in view, each laid cell cut to the hatch's rings (by odd parity, so an island is a hole). A laid cell
//! wholly inside is drawn as it is; only those across an edge are cut. Where the cell would be too small to see, or laying it
//! would take more than the frame may draw, the hatch is its tint instead (the caller fills it), as Revit draws a pattern too
//! dense to show.

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;

use crate::dxf::Tile;
use crate::geom::P2;
use crate::prim::{Rgb, Rule};

use super::{Drawing, Pen, Prim};

/// The smallest a cell's step may be on a screen, px, before the hatch is drawn as its tint; and on paper or in a picture.
pub(super) const SCREEN_PX: f64 = 3.0;
pub(super) const PAPER_PX: f64 = 1.0;
/// The most points of laid cells a frame draws of one drawing's hatches on a screen: past them, a hatch is its tint.
pub(super) const SCREEN_POINTS: usize = 2_000_000;
/// The same on paper and in a picture, which draw every hatch a page kept ([`crate::raster`]'s budget bounds them).
pub(super) const PAPER_POINTS: usize = 32_000_000;
/// The cells a side of the grid a hatch's edges are found through.
const GRID: usize = 32;

/// How a hatch's laid cells are drawn: where a point of the drawing lands in pixels, how a pattern's colour is drawn (the
/// import's colours, halftone), and pixels to a unit of the drawing.
pub(super) struct Look<'a> {
    pub px: &'a dyn Fn(P2) -> (f32, f32),
    pub color: &'a dyn Fn([u8; 3]) -> Rgb,
    pub k: f64,
}

/// The edges of a hatch's rings, found by the cells of a grid over them they cross: for cutting a laid cell to the rings and
/// telling whether a point is inside.
struct Edges {
    lo: P2,
    cell: P2,
    edges: Vec<(P2, P2)>,
    /// Each cell's edges, by their place in `edges`, row by row from the bottom.
    cells: Vec<Vec<u32>>,
    /// Each row's edges, for the ray from a point to the right.
    rows: Vec<Vec<u32>>,
}

impl Edges {
    fn new(rings: &[Vec<P2>]) -> Option<Edges> {
        let pts = rings.iter().flatten();
        let [lo, hi] = crate::dxf::union(pts.map(|p| [*p, *p]))?;
        let cell = P2(((hi.0 - lo.0) / GRID as f64).max(1e-9), ((hi.1 - lo.1) / GRID as f64).max(1e-9));
        let mut edges = Vec::new();
        for r in rings {
            for (i, a) in r.iter().enumerate() {
                let Some(b) = r.get((i + 1) % r.len()) else { continue };
                if a != b {
                    edges.push((*a, *b));
                }
            }
        }
        let mut e = Edges { lo, cell, edges, cells: vec![Vec::new(); GRID * GRID], rows: vec![Vec::new(); GRID] };
        for (n, (a, b)) in e.edges.iter().enumerate() {
            let (x0, y0, x1, y1) = e.span([a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)]);
            for y in y0..=y1 {
                if let Some(row) = e.rows.get_mut(y) {
                    row.push(n as u32);
                }
                for x in x0..=x1 {
                    if let Some(c) = e.cells.get_mut(y * GRID + x) {
                        c.push(n as u32);
                    }
                }
            }
        }
        Some(e)
    }

    /// The grid's cells box `b` (x0, y0, x1, y1) meets: first and last column, first and last row.
    fn span(&self, b: [f64; 4]) -> (usize, usize, usize, usize) {
        let at = |v: f64, o: f64, c: f64| (((v - o) / c).floor().max(0.0) as usize).min(GRID - 1);
        (
            at(b[0], self.lo.0, self.cell.0),
            at(b[1], self.lo.1, self.cell.1),
            at(b[2], self.lo.0, self.cell.0),
            at(b[3], self.lo.1, self.cell.1),
        )
    }

    /// The edges that may cross box `b`, each once.
    fn near(&self, b: [f64; 4]) -> Vec<u32> {
        let (x0, y0, x1, y1) = self.span(b);
        let mut out: Vec<u32> =
            (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| y * GRID + x)).filter_map(|c| self.cells.get(c)).flatten().copied().collect();
        out.sort_unstable();
        out.dedup();
        out.retain(|n| {
            self.edges
                .get(*n as usize)
                .is_some_and(|(a, z)| a.0.min(z.0) <= b[2] && a.0.max(z.0) >= b[0] && a.1.min(z.1) <= b[3] && a.1.max(z.1) >= b[1])
        });
        out
    }

    /// Whether point `p` is inside the rings, by odd parity: the edges its ray to the right crosses.
    fn inside(&self, p: P2) -> bool {
        let (_, y, _, _) = self.span([p.0, p.1, p.0, p.1]);
        let mut inside = false;
        for n in self.rows.get(y).into_iter().flatten() {
            let Some((a, b)) = self.edges.get(*n as usize) else { continue };
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) {
                inside = !inside;
            }
        }
        inside
    }

    /// The pieces of the run of points `pts` (closed back to its start when `closed`) inside the rings, cut where edges `near`
    /// cross it.
    fn cut(&self, pts: &[P2], closed: bool, near: &[u32]) -> Vec<Vec<P2>> {
        let back = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
        let segments = pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(back);
        let mut out: Vec<Vec<P2>> = Vec::new();
        let mut run: Vec<P2> = Vec::new();
        for (a, b) in segments {
            // Where along it (0 to 1) the edges cross it, its ends among them.
            let mut ts = vec![0.0, 1.0];
            let d = b - a;
            for n in near {
                let Some((c, e)) = self.edges.get(*n as usize) else { continue };
                let f = *e - *c;
                let den = d.0 * f.1 - d.1 * f.0;
                if den.abs() < 1e-18 {
                    continue;
                }
                let w = *c - a;
                let (t, u) = ((w.0 * f.1 - w.1 * f.0) / den, (w.0 * d.1 - w.1 * d.0) / den);
                if t > 0.0 && t < 1.0 && (0.0..=1.0).contains(&u) {
                    ts.push(t);
                }
            }
            ts.sort_by(f64::total_cmp);
            for w in ts.windows(2) {
                let (Some(&t0), Some(&t1)) = (w.first(), w.get(1)) else { continue };
                if t1 - t0 <= 1e-12 {
                    continue;
                }
                let (p, q) = (a + d * t0, a + d * t1);
                if self.inside(a + d * ((t0 + t1) / 2.0)) {
                    if run.last().is_none_or(|l| l.dist(p) > 1e-9) {
                        if run.len() >= 2 {
                            out.push(std::mem::take(&mut run));
                        }
                        run.clear();
                        run.push(p);
                    }
                    run.push(q);
                } else if run.len() >= 2 {
                    out.push(std::mem::take(&mut run));
                } else {
                    run.clear();
                }
            }
        }
        if run.len() >= 2 {
            out.push(run);
        }
        out
    }
}

/// Draws hatch `rings` with pattern `t`: the cells laid over the part of it in box `view` (low x, low y, high x, high y,
/// drawing units), as `look` says, with alpha `alpha` times each piece's own. True when it is drawn so; false when its cell is
/// under `least` pixels a step or its cells would take more than `budget` points (what's left of the frame's, which this
/// takes from), for the caller to fill it with its tint.
pub(super) fn draw(d: &mut Drawing, t: &Tile, rings: &[Vec<P2>], view: [f64; 4], look: &Look<'_>, least: f64, budget: &mut usize) -> bool {
    // The cell's steps on screen.
    let o = (look.px)(P2(0.0, 0.0));
    let on_screen = |s: P2| {
        let q = (look.px)(s);
        f64::from(q.0 - o.0).hypot(f64::from(q.1 - o.1))
    };
    if !(on_screen(t.steps[0]) >= least && on_screen(t.steps[1]) >= least) {
        return false;
    }
    let (Some(edges), Some(cb)) = (Edges::new(rings), t.bounds()) else { return true };
    // What of it shows: its box within the view's.
    let b = [
        edges.lo.0.max(view[0]),
        edges.lo.1.max(view[1]),
        (edges.lo.0 + edges.cell.0 * GRID as f64).min(view[2]),
        (edges.lo.1 + edges.cell.1 * GRID as f64).min(view[3]),
    ];
    if !(b[0] <= b[2] && b[1] <= b[3]) {
        return true;
    }
    // The steps that lay a cell meeting it: the box's corners less the cell's, in steps.
    let [s0, s1] = t.steps;
    let det = s0.0 * s1.1 - s0.1 * s1.0;
    if !det.is_normal() {
        return false;
    }
    let steps_of = |p: P2| ((p.0 * s1.1 - p.1 * s1.0) / det, (s0.0 * p.1 - s0.1 * p.0) / det);
    let (mut i0, mut i1, mut j0, mut j1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for q in [P2(b[0], b[1]), P2(b[2], b[1]), P2(b[2], b[3]), P2(b[0], b[3])] {
        for c in [P2(cb[0], cb[1]), P2(cb[2], cb[1]), P2(cb[2], cb[3]), P2(cb[0], cb[3])] {
            let (i, j) = steps_of(q - c);
            (i0, i1, j0, j1) = (i0.min(i), i1.max(i), j0.min(j), j1.max(j));
        }
    }
    let (i0, i1, j0, j1) = (i0.floor(), i1.ceil(), j0.floor(), j1.ceil());
    let laid = (i1 - i0 + 1.0) * (j1 - j0 + 1.0);
    let cost = laid * t.points() as f64;
    if !(laid.is_finite() && cost <= *budget as f64) {
        return false;
    }
    *budget -= cost as usize;
    let dash = |l: &crate::dxf::TileLine| {
        l.dash
            .as_ref()
            .map(|d| d.iter().map(|x| (x * look.k) as f32).collect::<Vec<f32>>())
            .filter(|d| d.iter().sum::<f32>() >= 3.0 && d.iter().skip(1).step_by(2).any(|gap| *gap >= 0.5))
    };
    let pen = |l: &crate::dxf::TileLine| l.weight.filter(|w| *w > 0.0).map_or(Pen::FINE, Pen::nearest);
    let px = |pts: &[P2], by: P2| super::thinned(pts.iter().map(|p| (look.px)(*p + by)));
    let fill = |d: &mut Drawing, rings: Vec<Vec<(f32, f32)>>, f: &crate::dxf::TileFill| {
        let rings: Vec<Vec<(f32, f32)>> = rings.into_iter().filter(|r| r.len() >= 3).collect();
        if !rings.is_empty() {
            d.push(Prim::Fill { rings, color: (look.color)(f.color), rule: Rule::EvenOdd, alpha: f.alpha });
        }
    };
    for j in j0 as i64..=j1 as i64 {
        for i in i0 as i64..=i1 as i64 {
            let by = s0 * i as f64 + s1 * j as f64;
            let at = [cb[0] + by.0, cb[1] + by.1, cb[2] + by.0, cb[3] + by.1];
            if at[0] > b[2] || at[2] < b[0] || at[1] > b[3] || at[3] < b[1] {
                continue;
            }
            let near = edges.near(at);
            if near.is_empty() {
                // Wholly inside or wholly out.
                if !edges.inside(P2((at[0] + at[2]) / 2.0, (at[1] + at[3]) / 2.0)) {
                    continue;
                }
                for f in &t.fills {
                    fill(d, f.rings.iter().map(|r| r.iter().map(|p| (look.px)(*p + by)).collect()).collect(), f);
                }
                for l in &t.lines {
                    d.stroke_seen(px(&l.pts, by), pen(l), (look.color)(l.color), dash(l), l.closed, l.alpha);
                }
                continue;
            }
            // Across an edge: each fill and line cut to the rings.
            for f in &t.fills {
                let moved: Vec<Vec<[f64; 2]>> = f.rings.iter().map(|r| r.iter().map(|p| [p.0 + by.0, p.1 + by.1]).collect()).collect();
                let clip: Vec<Vec<[f64; 2]>> = rings.iter().map(|r| r.iter().map(|p| [p.0, p.1]).collect()).collect();
                let cut = moved.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);
                let px_rings = cut.into_iter().flatten().map(|r| r.iter().map(|p| (look.px)(P2(p[0], p[1]))).collect()).collect();
                fill(d, px_rings, f);
            }
            for l in &t.lines {
                let moved: Vec<P2> = l.pts.iter().map(|p| *p + by).collect();
                for piece in edges.cut(&moved, l.closed, &near) {
                    d.stroke_seen(px(&piece, P2(0.0, 0.0)), pen(l), (look.color)(l.color), dash(l), false, l.alpha);
                }
            }
        }
    }
    true
}

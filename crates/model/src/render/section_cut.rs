//! What a section cuts of the model, from the element solids (`view3d::element_solids`, step 4 of the Manifold plan).
//!
//! Each layer of an element is one closed solid; the section's vertical plane is cut through it by `bimcraft-geom::solid`
//! ([`Solid3::slice`]), which gives closed loops (outer ones counter-clockwise, holes clockwise, the solid on their left) of
//! lines, and of arcs where a curved face meets a level plane. What a section draws is rings, filled, outlined and patterned
//! one at a time, so a loop becomes one or more simple rings here: a loop with no hole stays as it is; a hole is cut out by
//! splitting the region along the vertical lines through every one of its corners (into trapezoids that abut, which the
//! drawing joins as it joins a wall's abutting pieces: no seam in the fill, no line along the join).
//!
//! A piece of an element that is not a solid (an open mesh: a pipe's tube, a family's form with holes in its skin) is cut as
//! it was before there were solids: [`crate::view3d::Solid::cuts`].
//!
//! Everything is in a section's own plane: `x` along its line from where the cut begins, `y` up, in mm.

use crate::geom::{self, P2};
use crate::solid::{Frame, Loop, Seg};
use crate::view3d::{ElementSolids, Vis};

/// The rings of an element's layers: each layer (as `Origin::layer` counts them) and its rings.
pub(crate) type LayerRings = Vec<(u16, Vec<Vec<P2>>)>;

/// A region of a cut: its outline (counter-clockwise) and the holes in it (clockwise), the solid on the left of every edge. What a
/// vector drawing (an outline, a hatch over the whole region) wants; the raster poché is filled from [`ElementCut::layers`]'s
/// simple rings, which cut a hole out as trapezoids.
///
/// A region that runs past a cropped section's end is cut to the line's range as trapezoids with no holes (as the rings are).
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Vec<P2>,
    pub holes: Vec<Vec<P2>>,
}

/// The regions of an element's layers, by layer.
pub(crate) type LayerRegions = Vec<(u16, Vec<Region>)>;

/// What a section cuts of one element.
#[derive(Default)]
pub(crate) struct ElementCut {
    /// The rings of each closed layer solid it is cut through, by layer: simple, counter-clockwise, the holes cut out.
    pub layers: LayerRings,
    /// The same cut as regions, each an outline and its holes, by layer.
    pub regions: LayerRegions,
    /// The rings of its open pieces (as they were cut before the solids), in no layer.
    pub open: Vec<Vec<P2>>,
}

impl ElementCut {
    /// Every ring of it, layers' first.
    pub fn rings(&self) -> impl Iterator<Item = &Vec<P2>> {
        self.layers.iter().flat_map(|(_, rings)| rings).chain(&self.open)
    }

    pub fn is_empty(&self) -> bool {
        self.layers.iter().all(|(_, r)| r.is_empty()) && self.open.is_empty()
    }
}

/// The cut `es` is made into by the vertical plane `frame`, `len` mm of it from where it begins (`p`, along unit `u`: the
/// frame's own origin and direction), its `x` moved on by `shift`. Layers the plane does not reach are not sliced. Only what the
/// view `drawn` says draws a piece is cut (a form of a family drawn at Fine only is not cut in a Coarse section, one that is drawn
/// in no front view is not cut in a section that looks at the family's front).
pub(crate) fn cut_element(
    es: &ElementSolids,
    frame: &Frame,
    (p, u): (P2, P2),
    len: f64,
    shift: f64,
    drawn: &dyn Fn(Vis) -> bool,
) -> ElementCut {
    let mut out = ElementCut::default();
    for (layer, solid) in &es.drawn_layers(drawn) {
        if !reaches(solid.bounds().map(|b| (b.min, b.max)), frame) {
            // Its twin, which tests hold it to: what is passed over is cut by none of the plane.
            if crate::selfcheck::on(crate::selfcheck::Check::SectionLayers) {
                assert!(solid.slice(frame).is_empty(), "a layer solid the plane passes by is cut by it");
            }
            continue;
        }
        let regions = regions_of(&solid.slice(frame), len);
        if !regions.is_empty() {
            out.layers.push((*layer, rings_of_regions(&regions, len)));
            out.regions.push((*layer, regions));
        }
    }
    for open in &es.open {
        out.open.extend(open.piece.cuts(p, u, len));
    }
    if shift != 0.0 {
        let rings = out.layers.iter_mut().flat_map(|(_, r)| r.iter_mut()).chain(out.open.iter_mut());
        let loops =
            out.regions.iter_mut().flat_map(|(_, rs)| rs.iter_mut()).flat_map(|r| std::iter::once(&mut r.outer).chain(r.holes.iter_mut()));
        for ring in rings.chain(loops) {
            for q in ring.iter_mut() {
                q.0 += shift;
            }
        }
    }
    out
}

/// Whether the box `(min, max)` may meet the plane: not wholly in front of it nor wholly behind (a face on the plane is cut
/// from behind, as [`Solid3::slice`] says, so only a box all in front, to the last bit, is passed over).
fn reaches(bounds: Option<([f64; 3], [f64; 3])>, frame: &Frame) -> bool {
    let Some((lo, hi)) = bounds else { return false };
    let mut side = (f64::INFINITY, f64::NEG_INFINITY);
    for x in [lo[0], hi[0]] {
        for y in [lo[1], hi[1]] {
            for z in [lo[2], hi[2]] {
                let s = frame.to_local([x, y, z])[2];
                side = (side.0.min(s), side.1.max(s));
            }
        }
    }
    // A hair of room for what rounding the box makes of a corner.
    side.0 <= 1e-6 && side.1 >= -1e-6
}

/// The simple rings the regions come to: a region with no hole as it is, one with a hole cut into trapezoids that abut.
pub(crate) fn rings_of_regions(regions: &[Region], len: f64) -> Vec<Vec<P2>> {
    let mut rings = Vec::new();
    for r in regions {
        if r.holes.is_empty() {
            rings.push(r.outer.clone());
        } else {
            let group: Vec<&[P2]> = std::iter::once(r.outer.as_slice()).chain(r.holes.iter().map(Vec::as_slice)).collect();
            rings.extend(slabs(&group, 0.0, len));
        }
    }
    rings
}

/// The regions a plane's loops make, within `0..=len` along the line: each outer loop with the holes in it (a hole goes with the
/// smallest outer loop that holds it). One that runs past the range is cut to it as trapezoids, each a region of its own.
pub(crate) fn regions_of(loops: &[Loop], len: f64) -> Vec<Region> {
    let polys: Vec<Vec<P2>> = loops.iter().map(polyline).filter(|p| p.len() >= 3).collect();
    let area: Vec<f64> = polys.iter().map(|p| geom::area(p)).collect();
    let outers: Vec<usize> = (0..polys.len()).filter(|&i| area[i] > 0.0).collect();
    // Each hole goes with the smallest outer ring that holds it.
    let mut holes: Vec<Vec<usize>> = vec![Vec::new(); polys.len()];
    for h in (0..polys.len()).filter(|&i| area[i] < 0.0) {
        let probe = inside_of(&polys[h]);
        let owner = outers.iter().copied().filter(|&o| geom::contains(&polys[o], probe)).min_by(|&a, &b| area[a].total_cmp(&area[b]));
        if let Some(o) = owner {
            holes[o].push(h);
        }
    }
    let mut regions = Vec::new();
    for &o in &outers {
        let (lo, hi) = polys[o].iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
        let inside = lo >= -1e-9 && hi <= len + 1e-9;
        if inside {
            regions.push(Region { outer: polys[o].clone(), holes: holes[o].iter().map(|&h| polys[h].clone()).collect() });
        } else {
            let group: Vec<&[P2]> = std::iter::once(polys[o].as_slice()).chain(holes[o].iter().map(|&h| polys[h].as_slice())).collect();
            regions.extend(slabs(&group, 0.0, len).into_iter().map(|outer| Region { outer, holes: Vec::new() }));
        }
    }
    regions
}

/// A point on the inside of a loop's first edge: the middle of that edge (on the hole's boundary, so the test that asks which
/// outer ring holds it is for one that holds the whole hole).
fn inside_of(ring: &[P2]) -> P2 {
    match (ring.first(), ring.get(1)) {
        (Some(a), Some(b)) => (*a + *b) * 0.5,
        _ => P2::default(),
    }
}

/// The region inside `rings` (even-odd: an outline and the holes in it) between `x = lo` and `x = hi`, as trapezoids that
/// abut along vertical lines: one pair of edges to a strip between two neighbouring corners' `x`.
pub(crate) fn slabs(rings: &[&[P2]], lo: f64, hi: f64) -> Vec<Vec<P2>> {
    const SAME: f64 = 1e-9;
    let mut xs: Vec<f64> = rings.iter().flat_map(|r| r.iter().map(|q| q.0)).filter(|x| *x > lo && *x < hi).collect();
    xs.extend([lo, hi]);
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() <= SAME);
    let edges: Vec<(P2, P2)> = rings.iter().flat_map(|r| geom::ring_edges(r)).filter(|(a, b)| (b.0 - a.0).abs() > SAME).collect();
    let mut out = Vec::new();
    for w in xs.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b - a <= SAME {
            continue;
        }
        let mid = (a + b) / 2.0;
        // The edges across the whole strip (every corner's `x` is in `xs`, so an edge is across it or misses it), each as its
        // height at the strip's two sides, from the lowest at its middle.
        let mut across: Vec<(f64, f64, f64)> = edges
            .iter()
            .filter(|(p, q)| p.0.min(q.0) <= a + SAME && p.0.max(q.0) >= b - SAME)
            .map(|(p, q)| {
                let at = |x: f64| p.1 + (q.1 - p.1) * ((x - p.0) / (q.0 - p.0));
                (at(a), at(b), at(mid))
            })
            .collect();
        across.sort_by(|x, y| x.2.total_cmp(&y.2));
        for &[below, above] in across.as_chunks::<2>().0 {
            let mut ring = vec![P2(a, below.0), P2(b, below.1), P2(b, above.1), P2(a, above.0)];
            ring.dedup_by(|p, q| p.dist(*q) <= SAME);
            if ring.len() > 1 && ring.first().zip(ring.last()).is_some_and(|(p, q)| p.dist(*q) <= SAME) {
                ring.pop();
            }
            if ring.len() >= 3 && geom::area(&ring) > 1e-9 {
                out.push(ring);
            }
        }
    }
    out
}

/// A loop as the corners of a polygon: its lines' starts, its arcs flattened to within [`ARC_SAG`] of the circle.
pub(crate) fn polyline(l: &Loop) -> Vec<P2> {
    let mut pts: Vec<P2> = Vec::new();
    for s in &l.segs {
        match s {
            Seg::Line { a, .. } => pts.push(*a),
            Seg::Arc { centre, radius, start, .. } => {
                pts.push(*start);
                let sweep = s.sweep().unwrap_or(0.0);
                let step = if *radius > ARC_SAG { 2.0 * (1.0 - ARC_SAG / radius).acos() } else { std::f64::consts::FRAC_PI_2 };
                let n = ((sweep.abs() / step.max(1e-4)).ceil() as usize).clamp(1, 4096);
                let t0 = (start.1 - centre.1).atan2(start.0 - centre.0);
                for i in 1..n {
                    let t = t0 + sweep * i as f64 / n as f64;
                    pts.push(P2(centre.0 + radius * t.cos(), centre.1 + radius * t.sin()));
                }
            }
        }
    }
    pts.dedup_by(|a, b| a.dist(*b) <= 1e-9);
    if pts.len() > 1 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-9) {
        pts.pop();
    }
    pts
}

/// How far a flattened arc may lie inside its circle, mm.
const ARC_SAG: f64 = 0.01;

/// The thinnest width of a ring that a pattern needs room in: twice its area over its perimeter (a thin strip's own width, a
/// sloping slab's thickness at right angles to it).
pub(crate) fn width_of(ring: &[P2]) -> f64 {
    let perimeter: f64 = geom::ring_edges(ring).map(|(a, b)| a.dist(b)).sum();
    if perimeter > 0.0 { 2.0 * geom::area(ring).abs() / perimeter } else { 0.0 }
}

/// The way a layer's courses run along a ring: its longest edge that is not upright, turned to run to the right; level when
/// it has none.
pub(crate) fn course_of(ring: &[P2]) -> P2 {
    geom::ring_edges(ring)
        .filter(|(a, b)| (b.0 - a.0).abs() > 1e-6 * a.dist(*b).max(1.0))
        .max_by(|x, y| x.0.dist(x.1).total_cmp(&y.0.dist(y.1)))
        .map(|(a, b)| if b.0 >= a.0 { (b - a).unit() } else { (a - b).unit() })
        .unwrap_or(P2(1.0, 0.0))
}

/// The edges between one layer's rings and the next's of the same element, as runs of points: each ring's edge where a ring of a
/// layer with a higher number lies against it, on the outer side (so each boundary once).
pub(crate) fn interfaces(layers: &[(u16, Vec<Vec<P2>>)]) -> Vec<Vec<P2>> {
    // How far past an edge, outward, a point is looked for in another ring: mm.
    const PAST: f64 = 1.0;
    let all: Vec<(u16, &[P2])> = layers.iter().flat_map(|(k, rings)| rings.iter().map(move |r| (*k, r.as_slice()))).collect();
    let mut out = Vec::new();
    for (k, ring) in &all {
        let others: Vec<&[P2]> = all.iter().filter(|(k2, r)| k2 > k && near(ring, r, PAST)).map(|(_, r)| *r).collect();
        if others.is_empty() {
            continue;
        }
        // Outward is to the right of a counter-clockwise ring.
        let sign = if geom::area(ring) > 0.0 { -1.0 } else { 1.0 };
        for (a, b) in geom::ring_edges(ring) {
            let len = a.dist(b);
            if len <= 1e-9 {
                continue;
            }
            let mut ts = vec![0.0, 1.0];
            for (c, e) in others.iter().flat_map(|r| geom::ring_edges(r)) {
                ts.extend(geom::crossing(a, b, c, e));
                for q in [c, e] {
                    if geom::dist_to_segment(q, a, b) < 1e-6 * len.max(1.0) {
                        ts.push(geom::param_on_segment(q, a, b));
                    }
                }
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y) * len < 1e-6);
            let out_of = ((b - a).perp() * sign).unit() * PAST;
            let mut run: Option<Vec<P2>> = None;
            for w in ts.windows(2) {
                let (p, q) = (a + (b - a) * w[0], a + (b - a) * w[1]);
                let against = others.iter().any(|r| geom::contains(r, (p + q) * 0.5 + out_of));
                match (against, run.as_mut()) {
                    (true, Some(r)) => r.push(q),
                    (true, None) => run = Some(vec![p, q]),
                    (false, _) => out.extend(run.take()),
                }
            }
            out.extend(run);
        }
    }
    out
}

/// Whether the boxes of two rings are within `past` of each other.
fn near(a: &[P2], b: &[P2], past: f64) -> bool {
    let bx = |r: &[P2]| {
        r.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        })
    };
    let ((a0, a1), (b0, b1)) = (bx(a), bx(b));
    a0.0 <= b1.0 + past && b0.0 <= a1.0 + past && a0.1 <= b1.1 + past && b0.1 <= a1.1 + past
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, x1: f64, y0: f64, y1: f64) -> Vec<P2> {
        vec![P2(x0, y0), P2(x1, y0), P2(x1, y1), P2(x0, y1)]
    }

    fn total(rings: &[Vec<P2>]) -> f64 {
        rings.iter().map(|r| geom::area(r)).sum()
    }

    #[test]
    fn a_ring_with_a_hole_is_cut_into_pieces_round_it() {
        let outer = rect(0.0, 100.0, 0.0, 60.0);
        let hole: Vec<P2> = rect(30.0, 70.0, 20.0, 40.0).into_iter().rev().collect();
        let pieces = slabs(&[&outer, &hole], 0.0, 100.0);
        assert_eq!(pieces.len(), 4, "{pieces:?}");
        assert!((total(&pieces) - (6000.0 - 800.0)).abs() < 1e-9);
        assert!(pieces.iter().all(|p| geom::area(p) > 0.0 && p.len() == 4));
        assert!(!pieces.iter().any(|p| geom::contains(p, P2(50.0, 30.0))), "the hole is open");
    }

    #[test]
    fn a_ring_is_cut_to_the_range() {
        // An L past the range on its right, and a slope: the part from 10 to 50.
        let l = vec![P2(0.0, 0.0), P2(100.0, 0.0), P2(100.0, 20.0), P2(40.0, 20.0), P2(40.0, 60.0), P2(0.0, 60.0)];
        let pieces = slabs(&[&l], 10.0, 50.0);
        let want = (40.0 - 10.0) * 60.0 + (50.0 - 40.0) * 20.0;
        assert!((total(&pieces) - want).abs() < 1e-9, "{pieces:?}");
        assert!(pieces.iter().flatten().all(|q| q.0 >= 10.0 - 1e-9 && q.0 <= 50.0 + 1e-9));
        // A ring wholly inside the range is as it was: one piece.
        assert_eq!(slabs(&[&rect(1.0, 5.0, 2.0, 9.0)], 0.0, 10.0).len(), 1);
        // And a sloped top: a trapezoid keeps its slope.
        let t = vec![P2(0.0, 0.0), P2(10.0, 0.0), P2(10.0, 10.0), P2(0.0, 5.0)];
        let cut = slabs(&[&t], 0.0, 5.0);
        assert_eq!(cut.len(), 1);
        // 0..5 wide, 0 to 5 at the left and 0 to 7.5 at the right: 5 x (5 + 7.5) / 2.
        assert!((geom::area(&cut[0]) - 31.25).abs() < 1e-9, "{cut:?}");
    }

    #[test]
    fn an_arc_is_flattened_within_a_hundredth_of_a_millimetre() {
        let (c, r) = (P2(10.0, 20.0), 1900.0);
        let at = |t: f64| P2(c.0 + r * t.cos(), c.1 + r * t.sin());
        let (start, end) = (at(0.0), at(std::f64::consts::FRAC_PI_2));
        let arc = Seg::Arc { centre: c, radius: r, start, end, ccw: true, origin: crate::solid::Origin::new(1, 0) };
        let back = Seg::Line { a: end, b: start, origin: crate::solid::Origin::new(1, 0) };
        let poly = polyline(&Loop { segs: vec![arc, back] });
        assert!(poly.len() > 20, "{}", poly.len());
        for (a, b) in geom::ring_edges(&poly).take(poly.len() - 1) {
            let mid = (a + b) * 0.5;
            assert!((mid.dist(c) - r).abs() <= 0.0101, "{}", mid.dist(c) - r);
        }
        assert!(poly.iter().take(poly.len() - 1).all(|q| (q.dist(c) - r).abs() < 1e-9));
    }

    #[test]
    fn the_width_and_the_courses_of_a_layer() {
        // A 20 thick wall layer 3000 tall: its width is its thickness (a hair less), its courses level.
        let wall = rect(900.0, 920.0, 0.0, 3000.0);
        assert!((width_of(&wall) - 19.867).abs() < 0.01, "{}", width_of(&wall));
        assert_eq!(course_of(&wall), P2(1.0, 0.0));
        // A sloped slab layer: courses along its slope.
        let slab = vec![P2(0.0, 0.0), P2(1000.0, 500.0), P2(1000.0, 600.0), P2(0.0, 100.0)];
        let c = course_of(&slab);
        assert!((c.0 - 0.894427).abs() < 1e-5 && (c.1 - 0.447214).abs() < 1e-5, "{c:?}");
    }

    #[test]
    fn the_line_between_two_layers_is_drawn_once() {
        let layers = vec![
            (0, vec![rect(0.0, 20.0, 0.0, 100.0)]),
            (1, vec![rect(20.0, 180.0, 0.0, 100.0)]),
            (2, vec![rect(180.0, 200.0, 0.0, 100.0)]),
        ];
        let lines = interfaces(&layers);
        assert_eq!(lines.len(), 2, "{lines:?}");
        for (line, x) in lines.iter().zip([20.0, 180.0]) {
            assert!(line.iter().all(|q| (q.0 - x).abs() < 1e-9), "{line:?}");
            let (lo, hi) = line.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q.1), b.max(q.1)));
            assert!((lo - 0.0).abs() < 1e-9 && (hi - 100.0).abs() < 1e-9);
        }
        // Layers that do not touch have none; nor does one layer's own join between its pieces.
        assert!(interfaces(&[(0, vec![rect(0.0, 10.0, 0.0, 10.0)]), (1, vec![rect(50.0, 60.0, 0.0, 10.0)])]).is_empty());
        assert!(interfaces(&[(0, vec![rect(0.0, 10.0, 0.0, 10.0), rect(10.0, 20.0, 0.0, 10.0)])]).is_empty());
    }
}

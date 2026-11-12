//! The straight skeleton of a polygon, holes and all, each edge moving in at its own speed: the
//! lines its corners leave as the edges move in and meet (a roof's hips, valleys and ridges), and
//! the face each edge sweeps (its slope). An edge that doesn't move stays upright, a gable.
//!
//! The wavefront moves in time: a corner moves so it stays on both its edges as they move. An
//! edge event is an edge shrinking away between its two corners; a split event, a reflex corner
//! running into an edge across the polygon and cutting the wavefront in two, or, the edge a
//! hole's, joining the two into one.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::geom::{self, P2};

/// How close two points along an edge are to be the same: mm.
const EPS: f64 = 1e-6;

struct Edge {
    a: P2,
    d: P2,
    /// Inward: the polygon is counter-clockwise, so its inside is on its edges' left.
    n: P2,
    speed: f64,
}

#[derive(Clone)]
struct Corner {
    p: P2,
    t: f64,
    v: P2,
    /// The edge before it and the edge after it, going round.
    left: usize,
    right: usize,
    prev: usize,
    next: usize,
    node: usize,
    alive: bool,
    /// Between parallel edges: no point stays on both as they move, so one of them goes.
    stuck: bool,
}

impl Corner {
    fn at(&self, t: f64) -> P2 {
        self.p + self.v * (t - self.t)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// The edge between two corners shrinks away.
    Shrink(usize, usize),
    /// A reflex corner runs into an edge.
    Split(usize, usize),
}

#[derive(Clone, Copy, PartialEq)]
struct Event {
    t: f64,
    kind: Kind,
}

impl Eq for Event {}

impl Ord for Event {
    // Earliest first, and at the same time, edges shrinking before splits.
    fn cmp(&self, other: &Event) -> Ordering {
        let rank = |k: &Kind| matches!(k, Kind::Split(..)) as u8;
        other.t.total_cmp(&self.t).then(rank(&other.kind).cmp(&rank(&self.kind)))
    }
}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Event) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Wavefront {
    edges: Vec<Edge>,
    corners: Vec<Corner>,
    /// Where corners start and meet: a point, and when.
    nodes: Vec<(P2, f64)>,
    /// What corners leave: from node to node, between the faces of two edges.
    arcs: Vec<(usize, usize, usize, usize)>,
    queue: BinaryHeap<Event>,
    /// How near points are to be one: a ten-millionth of the outline's size.
    eps: f64,
    /// Edges met at a point that don't pair up: the wavefront is tangled.
    broken: bool,
}

impl Wavefront {
    /// A corner between edges `left` and `right`, at `p` at time `t`, starting from `node`.
    fn corner(&mut self, p: P2, t: f64, left: usize, right: usize, node: usize) -> usize {
        let (l, r) = (&self.edges[left], &self.edges[right]);
        let det = l.n.cross(r.n);
        // It stays on both edges: v · n = speed, for each.
        let (v, stuck) = if det.abs() > 1e-9 {
            (P2((l.speed * r.n.1 - l.n.1 * r.speed) / det, (l.n.0 * r.speed - l.speed * r.n.0) / det), false)
        } else if l.n.dot(r.n) > 0.0 && (l.speed - r.speed).abs() < 1e-12 {
            (l.n * l.speed, false)
        } else {
            (P2(0.0, 0.0), true)
        };
        self.corners.push(Corner { p, t, v, left, right, prev: usize::MAX, next: usize::MAX, node, alive: true, stuck });
        self.corners.len() - 1
    }

    /// When the edge between corners `a` and `b` shrinks away, if it does from `now` on: now, if
    /// they're together and staying so.
    fn shrink(&mut self, a: usize, b: usize, now: f64) {
        let (ca, cb) = (&self.corners[a], &self.corners[b]);
        let d = self.edges[ca.right].d;
        // Their distance along the edge: c + t · k.
        let c = (cb.p - cb.v * cb.t - (ca.p - ca.v * ca.t)).dot(d);
        let k = (cb.v - ca.v).dot(d);
        let together = (c + k * now).abs() <= self.eps && k < 1e-9;
        let t = if together { now } else { (-c / k).max(now) };
        if (together || k < -1e-12) && t.is_finite() {
            self.queue.push(Event { t, kind: Kind::Shrink(a, b) });
        }
    }

    /// When a reflex corner `c` runs into each edge but its own, from `now` on.
    fn splits(&mut self, c: usize, now: f64) {
        let corner = &self.corners[c];
        let (dl, dr) = (self.edges[corner.left].d, self.edges[corner.right].d);
        if corner.stuck || dl.cross(dr) >= -1e-12 {
            return;
        }
        let p0 = corner.p - corner.v * corner.t;
        let found: Vec<Event> = self
            .edges
            .iter()
            .enumerate()
            .filter(|(f, _)| *f != corner.left && *f != corner.right)
            .filter_map(|(f, e)| {
                let k = corner.v.dot(e.n) - e.speed;
                let t = -(p0 - e.a).dot(e.n) / k;
                (k < -1e-12 && t.is_finite() && t >= now - EPS).then(|| Event { t: t.max(now), kind: Kind::Split(c, f) })
            })
            .collect();
        self.queue.extend(found);
    }

    /// Link corner `c` between `prev` and `next`.
    fn link(&mut self, prev: usize, c: usize, next: usize) {
        self.corners[prev].next = c;
        self.corners[c].prev = prev;
        self.corners[c].next = next;
        self.corners[next].prev = c;
    }

    /// A corner just made: closing a wavefront of two corners into a ridge, else its events. One
    /// between parallel edges, on the same line now, can't move: the slower of two going the same
    /// way, or the shorter of two meeting head on, goes at once, leaving a level line.
    fn settle(&mut self, c: usize, now: f64) {
        let (prev, next) = (self.corners[c].prev, self.corners[c].next);
        if next == c {
            self.corners[c].alive = false;
        } else if next == prev {
            let (a, b) = (&self.corners[c], &self.corners[next]);
            self.arcs.push((a.node, b.node, a.left, a.right));
            self.corners[c].alive = false;
            self.corners[next].alive = false;
        } else if self.corners[c].stuck {
            let k = &self.corners[c];
            let (l, r) = (&self.edges[k.left], &self.edges[k.right]);
            let (back, on) = (self.corners[prev].at(now), self.corners[next].at(now));
            let behind = if l.n.dot(r.n) > 0.0 { l.speed < r.speed } else { k.p.dist(back) <= k.p.dist(on) };
            if behind { self.merge(prev, c, back, now) } else { self.merge(c, next, on, now) }
        } else {
            self.shrink(prev, c, now);
            self.shrink(c, next, now);
            self.splits(c, now);
        }
    }

    /// Corners `a` and `b`, next to each other, meet at `p` at time `t`: the edge between them is
    /// gone, and a corner between the edges either side of it goes on from there.
    fn merge(&mut self, a: usize, b: usize, p: P2, t: f64) {
        let (prev, next) = (self.corners[a].prev, self.corners[b].next);
        // Three corners left: they meet in a point.
        if prev == next {
            let peak = self.node(p, t);
            for c in [a, b, next] {
                self.arc(c, peak);
            }
            return;
        }
        let n = self.node(p, t);
        let (left, right) = (self.corners[a].left, self.corners[b].right);
        self.arc(a, n);
        self.arc(b, n);
        let c = self.corner(p, t, left, right, n);
        self.link(prev, c, next);
        self.settle(c, t);
    }

    fn node(&mut self, p: P2, t: f64) -> usize {
        self.nodes.push((p, t));
        self.nodes.len() - 1
    }

    /// The arc corner `c` leaves as it reaches `node`.
    fn arc(&mut self, c: usize, node: usize) {
        let k = &self.corners[c];
        self.arcs.push((k.node, node, k.left, k.right));
        self.corners[c].alive = false;
    }

    fn step(&mut self, e: Event) {
        match e.kind {
            Kind::Shrink(a, b) => {
                let (ca, cb) = (&self.corners[a], &self.corners[b]);
                if ca.alive && cb.alive && ca.next == b {
                    let p = ca.at(e.t);
                    self.meet(p, e.t, &[a, b]);
                }
            }
            Kind::Split(v, f) => {
                if !self.corners[v].alive {
                    return;
                }
                let p = self.corners[v].at(e.t);
                // Only if it runs into a part of edge `f` still there.
                let hit = self
                    .corners
                    .iter()
                    .filter(|x| x.alive && x.right == f)
                    .any(|x| geom::dist_to_segment(p, x.at(e.t), self.corners[x.next].at(e.t)) < self.eps);
                if hit {
                    self.meet(p, e.t, &[v]);
                }
            }
        }
    }

    /// Corners `must`, and any others there, meet at `p` at time `t`, and maybe the middle of an
    /// edge they run into. The edges that come in to there and go on from there pair up into new
    /// corners: each going on with the one coming in next to it counter-clockwise, the wavefront
    /// between them; two along the same line close up what's left between them first.
    fn meet(&mut self, p: P2, t: f64, must: &[usize]) {
        let here: Vec<usize> = (0..self.corners.len())
            .filter(|&i| self.corners[i].alive && (must.contains(&i) || self.corners[i].at(t).dist(p) < self.eps))
            .collect();
        // Each: its direction from `p`, the edge, the corner at its other end, and whether it comes in.
        let mut rays: Vec<(f64, usize, usize, bool)> = Vec::new();
        let angle = |d: P2| d.1.atan2(d.0);
        for &i in &here {
            let c = &self.corners[i];
            if !here.contains(&c.prev) {
                rays.push((angle(self.edges[c.left].d * -1.0), c.left, c.prev, true));
            }
            if !here.contains(&c.next) {
                rays.push((angle(self.edges[c.right].d), c.right, c.next, false));
            }
        }
        for (x, c) in self.corners.iter().enumerate() {
            if c.alive
                && !here.contains(&x)
                && !here.contains(&c.next)
                && geom::dist_to_segment(p, c.at(t), self.corners[c.next].at(t)) < self.eps
            {
                let d = self.edges[c.right].d;
                rays.extend([(angle(d * -1.0), c.right, x, true), (angle(d), c.right, c.next, false)]);
            }
        }
        let node = self.node(p, t);
        for &i in &here {
            self.arc(i, node);
        }
        let (outs, ins): (Vec<_>, Vec<_>) = rays.into_iter().partition(|r| !r.3);
        let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
        for (i, o) in outs.iter().enumerate() {
            for (j, c) in ins.iter().enumerate() {
                let turn = (c.0 - o.0).rem_euclid(std::f64::consts::TAU);
                pairs.push((if turn > std::f64::consts::TAU - 1e-9 { 0.0 } else { turn }, i, j));
            }
        }
        pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut used_out, mut used_in) = (vec![false; outs.len()], vec![false; ins.len()]);
        let mut made = Vec::new();
        for (_, i, j) in pairs {
            if used_out[i] || used_in[j] {
                continue;
            }
            (used_out[i], used_in[j]) = (true, true);
            let c = self.corner(p, t, ins[j].1, outs[i].1, node);
            self.link(ins[j].2, c, outs[i].2);
            made.push(c);
        }
        if outs.len() != ins.len() {
            self.broken = true;
            return;
        }
        for c in made {
            if self.corners[c].alive {
                self.settle(c, t);
            }
        }
    }
}

/// A loop round a face an edge sweeps: its corners, each with the time the wavefront reaches it.
pub type Face = Vec<(P2, f64)>;

/// The faces the edges of `rings` sweep moving in at `speeds`: an outline (counter-clockwise,
/// simple) and the holes in it (each clockwise, inside it and apart), their edges numbered ring
/// after ring, edge i from point i to the next. For each edge that moves, the loops round its
/// face: its outline (counter-clockwise, from its start along it, then back through the
/// skeleton), then any holes in it, clockwise; none for one that doesn't move.
pub fn faces<R: AsRef<[P2]>>(rings: &[R], speeds: &[f64]) -> Result<Vec<Vec<Face>>, String> {
    let rings: Vec<&[P2]> = rings.iter().map(AsRef::as_ref).collect();
    let outline = rings.first().copied().unwrap_or_default();
    // Every point, and the one after it round its ring.
    let (mut points, mut next) = (Vec::new(), Vec::new());
    for r in &rings {
        next.extend((0..r.len()).map(|i| points.len() + (i + 1) % r.len()));
        points.extend_from_slice(r);
    }
    let n = points.len();
    if outline.len() < 3 || speeds.len() != n || geom::area(outline) <= 0.0 {
        return Err("it needs a counter-clockwise outline of three points or more".into());
    }
    if rings.iter().skip(1).any(|h| h.len() < 3 || geom::area(h) >= 0.0) {
        return Err("a hole in it goes clockwise round three points or more".into());
    }
    if speeds.iter().all(|s| *s <= 0.0) {
        return Err("none of its edges slopes".into());
    }
    let mut prev = vec![0; n];
    for (i, &j) in next.iter().enumerate() {
        prev[j] = i;
    }
    let edges: Vec<Edge> = (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[next[i]]);
            let d = (b - a).unit();
            Edge { a, d, n: d.perp(), speed: speeds[i] }
        })
        .collect();
    if edges.iter().any(|e| !e.d.is_finite() || e.d.length() < 0.5) {
        return Err("two of its points are the same".into());
    }
    let size = outline.iter().fold(0.0f64, |m, p| m.max(p.dist(outline[0])));
    let mut w = Wavefront {
        edges,
        corners: Vec::new(),
        nodes: points.iter().map(|p| (*p, 0.0)).collect(),
        arcs: Vec::new(),
        queue: BinaryHeap::new(),
        eps: size * 1e-7,
        broken: false,
    };
    for (i, p) in points.iter().enumerate() {
        w.corner(*p, 0.0, prev[i], i, i);
    }
    for i in 0..n {
        w.link(prev[i], i, next[i]);
    }
    for i in 0..n {
        if w.corners.get(i).is_some_and(|c| c.alive) {
            w.settle(i, 0.0);
        }
    }
    let mut budget = 100 * n * n + 1000;
    while let Some(e) = w.queue.pop() {
        budget = budget.checked_sub(1).ok_or("its skeleton doesn't settle")?;
        w.step(e);
    }
    let left: Vec<&Corner> = w.corners.iter().filter(|c| c.alive).collect();
    if !w.broken && !left.is_empty() && left.iter().all(|c| w.edges[c.left].speed <= 0.0 && w.edges[c.right].speed <= 0.0) {
        return Err("its upright edges fence in part of it that no slope reaches; let one of them slope".into());
    }
    if w.broken || !left.is_empty() {
        return Err("BIMCraft can't find where its slopes meet".into());
    }
    // Each edge's face: along it, then from its end back to its start through the arcs it borders;
    // then any loops of them left, holes in it (a slope closing round a courtyard and the slopes up
    // from it). An arc with the face on both sides, where two pieces of its edge met again, is none.
    (0..n)
        .map(|i| {
            if speeds[i] <= 0.0 {
                return Ok(Vec::new());
            }
            let mut arcs: Vec<(usize, usize)> = w.arcs.iter().filter(|a| (a.2 == i) != (a.3 == i)).map(|a| (a.0, a.1)).collect();
            // From the last node of `path` through the arcs round to its first.
            let walk = |arcs: &mut Vec<(usize, usize)>, mut path: Vec<usize>| -> Result<Face, String> {
                let (start, mut at) = (path[0], path[path.len() - 1]);
                while at != start {
                    let k = arcs.iter().position(|(a, b)| *a == at || *b == at).ok_or("its slopes don't close")?;
                    let (a, b) = arcs.swap_remove(k);
                    at = if a == at { b } else { a };
                    if at != start {
                        path.push(at);
                    }
                }
                let mut face: Vec<(P2, f64)> = path.into_iter().filter_map(|k| w.nodes.get(k).copied()).collect();
                face.dedup_by(|b, a| a.0.dist(b.0) < EPS);
                if face.len() > 2 && face.first().zip(face.last()).is_some_and(|(a, b)| a.0.dist(b.0) < EPS) {
                    face.pop();
                }
                Ok(face)
            };
            let mut loops = pinched(walk(&mut arcs, vec![i, next[i]])?);
            while let Some((a, b)) = arcs.pop() {
                loops.extend(pinched(walk(&mut arcs, vec![a, b])?));
            }
            // Its outline, then the holes in it, clockwise.
            let mut holes = loops.split_off(1);
            holes.retain(|h| h.len() >= 3);
            for h in &mut holes {
                if geom::area(&h.iter().map(|(p, _)| *p).collect::<Vec<_>>()) > 0.0 {
                    h.reverse();
                }
            }
            loops.extend(holes);
            Ok(loops)
        })
        .collect()
}

/// `ring` cut wherever it comes back to a point it passed: what's left, then each loop it went
/// round in between, a hole touching it there (a slope closing round a courtyard meets itself at
/// a corner of it that doesn't move).
fn pinched(mut ring: Face) -> Vec<Face> {
    let same = |a: &(P2, f64), b: &(P2, f64)| a.0.dist(b.0) < EPS;
    let mut loops = Vec::new();
    loop {
        // Back at its start, what follows is a loop round to it; elsewhere, what lies between.
        if let Some(j) = (2..ring.len()).find(|&j| same(&ring[j], &ring[0])) {
            loops.push(ring.split_off(j));
        } else if let Some((i, j)) = (2..ring.len()).find_map(|j| (1..j).find(|&i| same(&ring[i], &ring[j])).map(|i| (i, j))) {
            loops.push(ring.drain(i..j).collect());
        } else {
            break;
        }
    }
    loops.insert(0, ring);
    loops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(face: &[(P2, f64)]) -> f64 {
        geom::area(&face.iter().map(|(p, _)| *p).collect::<Vec<_>>())
    }

    fn highest(faces: &[Vec<Face>]) -> f64 {
        faces.iter().flatten().flatten().map(|(_, t)| *t).fold(0.0, f64::max)
    }

    #[test]
    fn a_rectangle_hips_its_ends_and_ridges_its_middle() {
        let ring = [P2(0.0, 0.0), P2(10.0, 0.0), P2(10.0, 4.0), P2(0.0, 4.0)];
        let f = faces(&[ring], &[1.0; 4]).unwrap();
        // Its long sides' faces are trapezoids up to the ridge, 2 in; its ends', triangles.
        assert_eq!(f.iter().flatten().map(Vec::len).collect::<Vec<_>>(), [4, 3, 4, 3]);
        assert!((highest(&f) - 2.0).abs() < 1e-9);
        let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
        assert!((total - 40.0).abs() < 1e-9, "the faces cover it: {total}");
        // Its ends left upright, gables: the ridge runs out to them.
        let g = faces(&[ring], &[1.0, 0.0, 1.0, 0.0]).unwrap();
        assert!(g[1].is_empty() && g[3].is_empty());
        assert!((g[0].iter().map(|x| area(x)).sum::<f64>() - 20.0).abs() < 1e-9, "{:?}", g[0]);
        // One side sloping, the rest upright: a shed, up to the far side.
        let shed = faces(&[ring], &[1.0, 0.0, 0.0, 0.0]).unwrap();
        assert!((shed[0].iter().map(|x| area(x)).sum::<f64>() - 40.0).abs() < 1e-9 && (highest(&shed) - 4.0).abs() < 1e-9, "{shed:?}");
        assert!(faces(&[ring], &[0.0; 4]).is_err());
    }

    #[test]
    fn an_l_meets_in_a_valley_at_its_inner_corner() {
        let ring = [P2(0.0, 0.0), P2(10.0, 0.0), P2(10.0, 4.0), P2(4.0, 4.0), P2(4.0, 10.0), P2(0.0, 10.0)];
        let f = faces(&[ring], &[1.0; 6]).unwrap();
        let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
        assert!((total - 64.0).abs() < 1e-9, "{total}");
        assert!((highest(&f) - 2.0).abs() < 1e-9);
        // The inner corner's edges each sweep up to the valley from it to (2, 2).
        let valley = |x: &Vec<(P2, f64)>| x.iter().any(|(p, t)| p.dist(P2(2.0, 2.0)) < 1e-9 && (t - 2.0).abs() < 1e-9);
        assert!(f[2].iter().any(valley) && f[3].iter().any(valley), "{f:?}");
    }

    #[test]
    fn a_convex_polygon_rises_to_a_point_or_a_ridge() {
        // A triangle: three slopes up to its incentre.
        let tri = [P2(0.0, 0.0), P2(6.0, 0.0), P2(3.0, 5.196152422706632)];
        let f = faces(&[tri], &[1.0; 3]).unwrap();
        assert!((highest(&f) - 3.0f64.sqrt()).abs() < 1e-9);
        // A skewed quadrilateral and a pentagon close up whole.
        for ring in [
            vec![P2(0.0, 0.0), P2(9.0, 1.0), P2(8.0, 6.0), P2(1.0, 4.0)],
            vec![P2(0.0, 0.0), P2(6.0, 0.0), P2(8.0, 4.0), P2(3.0, 7.0), P2(-2.0, 4.0)],
        ] {
            let f = faces(&[&ring], &vec![1.0; ring.len()]).unwrap();
            let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
            assert!((total - geom::area(&ring)).abs() < 1e-6, "{ring:?}: {total}");
        }
    }

    /// Whether each face is flat: every corner of it as far in from its edge as its edge has moved.
    fn flat(rings: &[Vec<P2>], speeds: &[f64], f: &[Vec<Face>]) -> bool {
        let edges: Vec<(P2, P2)> = rings.iter().flat_map(|r| geom::ring_edges(r)).collect();
        f.iter().zip(edges.iter().zip(speeds)).all(|(face, ((a, b), s))| {
            let n = (*b - *a).unit().perp();
            face.iter().flatten().all(|(p, t)| ((*p - *a).dot(n) - t * s).abs() < 1e-3)
        })
    }

    #[test]
    fn a_courtyard_slopes_up_from_its_eaves_to_a_ring_of_ridges() {
        // A 12 square round a 4 square courtyard, clockwise: its edges move out into the roof.
        let rings = [
            vec![P2(0.0, 0.0), P2(12.0, 0.0), P2(12.0, 12.0), P2(0.0, 12.0)],
            vec![P2(4.0, 4.0), P2(4.0, 8.0), P2(8.0, 8.0), P2(8.0, 4.0)],
        ];
        let f = faces(&rings, &[1.0; 8]).unwrap();
        // Every slope a trapezoid from its eaves up to the ridges, 2 in from both.
        assert_eq!(f.iter().flatten().map(Vec::len).collect::<Vec<_>>(), [4; 8], "{f:?}");
        let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
        assert!((total - 128.0).abs() < 1e-9, "{total}");
        assert!(flat(&rings, &[1.0; 8], &f));
        // The ridges: a ring 2 out from the courtyard, 2 in from the outline.
        for (p, t) in f.iter().flatten().flatten() {
            let ring = (p.0 - 6.0).abs().max((p.1 - 6.0).abs());
            assert!(*t == 0.0 || ((t - 2.0).abs() < 1e-9 && (ring - 4.0).abs() < 1e-9), "{p:?} at {t}");
        }
        // Its north edge upright, a gable: the outline's north slope runs up to it, 4 in.
        let speeds = [1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0];
        let g = faces(&rings, &speeds).unwrap();
        assert!(g[5].is_empty());
        let total: f64 = g.iter().flatten().map(|x| area(x)).sum();
        assert!((total - 128.0).abs() < 1e-9 && flat(&rings, &speeds, &g), "{total}: {g:?}");
        assert!((highest(&g) - 4.0).abs() < 1e-9);
        let gable = |x: &Vec<(P2, f64)>| {
            [P2(4.0, 8.0), P2(8.0, 8.0)].iter().all(|q| x.iter().any(|(p, t)| p.dist(*q) < 1e-9 && (t - 4.0).abs() < 1e-9))
        };
        assert!(g[2].iter().any(gable), "{:?}", g[2]);
        // Off the middle, each wing's ridge halfway across it: the widest, 5 across, highest.
        let off = [
            vec![P2(0.0, 0.0), P2(12.0, 0.0), P2(12.0, 10.0), P2(0.0, 10.0)],
            vec![P2(3.0, 3.0), P2(3.0, 7.0), P2(7.0, 7.0), P2(7.0, 3.0)],
        ];
        let f = faces(&off, &[1.0; 8]).unwrap();
        let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
        assert!((total - 104.0).abs() < 1e-9 && flat(&off, &[1.0; 8], &f), "{total}: {f:?}");
        assert!((highest(&f) - 2.5).abs() < 1e-9);
        // Holes go clockwise.
        assert!(faces(&[off[0].clone(), off[0].clone()], &[1.0; 8]).is_err());
    }

    #[test]
    fn a_slope_closes_round_a_courtyard_whose_far_side_is_upright() {
        // A courtyard 200 in from the south eaves, its north side a gable: the south slope rises
        // past it on both sides and on over it, its face a loop round the courtyard and its slopes.
        let rings = [
            vec![
                P2(90.0, 0.0),
                P2(90.0, 10.0),
                P2(70.0, 10.0),
                P2(70.0, 30.0),
                P2(-30.0, 30.0),
                P2(-30.0, 10.0),
                P2(-80.0, 10.0),
                P2(-80.0, 0.0),
            ],
            vec![P2(-5.0, 2.0), P2(-5.0, 8.0), P2(5.0, 8.0), P2(5.0, 2.0)],
        ];
        let speeds = [1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 1.0, 1.0];
        // Turned, too: events that met exactly now nearly do.
        let (c, s) = 0.3468f64.sin_cos();
        let turned: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|p| P2(p.0 * c - p.1 * s, p.0 * s + p.1 * c)).collect()).collect();
        for rings in [rings.to_vec(), turned] {
            let f = faces(&rings, &speeds).unwrap();
            let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
            assert!((total - 3640.0).abs() < 1e-6 && flat(&rings, &speeds, &f), "{total}: {f:?}");
            let island = 60.0 + [8, 10, 11].iter().flat_map(|i| &f[*i]).map(|x| area(x)).sum::<f64>();
            assert!(f[7].len() == 2 && (area(&f[7][1]) + island).abs() < 1e-6, "{:?}", f[7]);
        }
    }

    #[test]
    fn a_slope_closing_round_a_courtyard_can_touch_itself_where_two_gables_meet() {
        // Two courtyards near the south eaves, the east one's west and north sides gables: their
        // corner stays put, and the south slope, closing round the courtyard, meets itself there.
        let rings = [
            vec![P2(-30.0, 0.0), P2(80.0, 0.0), P2(80.0, 70.0), P2(-30.0, 70.0)],
            vec![P2(-9.0, 2.0), P2(-9.0, 8.0), P2(-4.0, 8.0), P2(-4.0, 2.0)],
            vec![P2(4.0, 2.0), P2(4.0, 8.0), P2(9.0, 8.0), P2(9.0, 2.0)],
        ];
        let mut speeds = [1.0; 12];
        (speeds[8], speeds[9]) = (0.0, 0.0);
        let (c, s) = 0.3468f64.sin_cos();
        let turned: Vec<Vec<P2>> = rings.iter().map(|r| r.iter().map(|p| P2(p.0 * c - p.1 * s, p.0 * s + p.1 * c)).collect()).collect();
        for rings in [rings.to_vec(), turned] {
            let f = faces(&rings, &speeds).unwrap();
            let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
            assert!((total - 7640.0).abs() < 1e-6 && flat(&rings, &speeds, &f), "{total}: {:?}", f[0]);
            assert!(f[0].len() == 2 && area(&f[0][1]) < 0.0, "{:?}", f[0]);
        }
    }

    /// Footprints a house might have: random stars (many reflex corners, splits everywhere) and
    /// random stepped outlines (square corners, events at the same time), with some edges upright,
    /// half of them round a courtyard, some stepped ones round two.
    #[test]
    fn random_footprints_close_up_whole() {
        let mut seed = 12345u64;
        let mut rand = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as f64 / (1u64 << 31) as f64
        };
        for round in 0..2000 {
            let n = 5 + (rand() * 12.0) as usize;
            let court = round % 4 >= 2;
            let rings: Vec<Vec<P2>> = if round % 2 == 0 {
                let star = (0..n)
                    .map(|i| {
                        let a = (i as f64 + rand() * 0.8) / n as f64 * std::f64::consts::TAU;
                        let r = 3000.0 + rand() * 7000.0;
                        P2(r * a.cos(), r * a.sin())
                    })
                    .collect();
                // A courtyard round its middle, clockwise, within the 1277 every edge keeps clear of it.
                let k = 3 + (rand() * 6.0) as usize;
                let mut hole: Vec<P2> = (0..k)
                    .rev()
                    .map(|i| {
                        let a = (i as f64 + rand() * 0.8) / k as f64 * std::f64::consts::TAU;
                        let r = 300.0 + rand() * 900.0;
                        P2(r * a.cos(), r * a.sin())
                    })
                    .collect();
                // A triangle's corners may span more than half a turn, and go round the other way.
                if geom::area(&hole) > 0.0 {
                    hole.reverse();
                }
                if court { vec![star, hole] } else { vec![star] }
            } else {
                // Up a staircase of random steps on the right, back down the left.
                let steps = 1 + (rand() * 4.0) as usize;
                let mut right = vec![];
                let mut left = vec![];
                let mut y = 0.0;
                for _ in 0..steps {
                    let (xr, xl, h) = (
                        2000.0 + (rand() * 8.0).floor() * 1000.0,
                        -2000.0 - (rand() * 8.0).floor() * 1000.0,
                        1000.0 + (rand() * 5.0).floor() * 1000.0,
                    );
                    right.extend([P2(xr, y), P2(xr, y + h)]);
                    left.splice(0..0, [P2(xl, y + h), P2(xl, y)]);
                    y += h;
                }
                let first = right.get(1).map_or(1000.0, |p| p.1);
                let mut ring: Vec<P2> = right;
                ring.extend(left);
                ring.dedup_by(|b, a| a.dist(*b) < 1e-9);
                // A courtyard in its first step, clockwise; or two, side by side.
                let (w, y0, g) = (
                    500.0 + (rand() * 3.0).floor() * 500.0,
                    200.0 + (rand() * 3.0).floor() * 100.0,
                    200.0 + (rand() * 3.0).floor() * 100.0,
                );
                let hole = |a: f64, b: f64| vec![P2(a, y0), P2(a, first - y0), P2(b, first - y0), P2(b, y0)];
                let holes = if round % 8 == 7 { vec![hole(-w - g, -g), hole(g, w + g)] } else { vec![hole(-w, w)] };
                // Turned any way, so edges meant parallel are only nearly so.
                let (c, s) = (rand() * 7.0).sin_cos();
                let turn = |r: Vec<P2>| r.into_iter().map(|p| P2(p.0 * c - p.1 * s, p.0 * s + p.1 * c)).collect();
                let mut rings = vec![turn(ring)];
                if court {
                    rings.extend(holes.into_iter().map(turn));
                }
                rings
            };
            let count = rings.iter().map(Vec::len).sum();
            let mut speeds: Vec<f64> = (0..count).map(|_| if rand() < 0.15 { 0.0 } else { 1.0 }).collect();
            if let Some(s) = speeds.first_mut() {
                *s = 1.0;
            }
            let whole: f64 = rings.iter().map(|r| geom::area(r)).sum();
            for speeds in [vec![1.0; count], speeds] {
                let f = match faces(&rings, &speeds) {
                    Ok(f) => f,
                    Err(e) if e.contains("fence") && speeds.contains(&0.0) => continue,
                    Err(e) => panic!("{e}: {rings:?} {speeds:?}"),
                };
                let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
                assert!((total - whole).abs() < 1e-3 * whole, "{rings:?} {speeds:?}: {total} vs {whole}");
                assert!(flat(&rings, &speeds, &f), "{rings:?} {speeds:?}: {f:?}");
            }
        }
    }

    #[test]
    fn a_t_and_a_u_close_up_whole() {
        let t = [P2(0.0, 0.0), P2(4.0, 0.0), P2(4.0, 6.0), P2(9.0, 6.0), P2(9.0, 10.0), P2(-5.0, 10.0), P2(-5.0, 6.0), P2(0.0, 6.0)];
        let u = [P2(0.0, 0.0), P2(12.0, 0.0), P2(12.0, 9.0), P2(8.0, 9.0), P2(8.0, 4.0), P2(4.0, 4.0), P2(4.0, 9.0), P2(0.0, 9.0)];
        for ring in [&t[..], &u[..]] {
            let f = faces(&[ring], &vec![1.0; ring.len()]).unwrap();
            let total: f64 = f.iter().flatten().map(|x| area(x)).sum();
            assert!((total - geom::area(ring)).abs() < 1e-6, "{ring:?}: {total}");
            assert!(f.iter().flatten().all(|x| area(x) > 0.0), "{f:?}");
        }
    }
}

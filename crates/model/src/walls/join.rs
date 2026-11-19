//! How the layers of two joined walls clean up, as Revit's layer join cleanup: which wall's layer holds each cell of the patch
//! where the two walls overlap. A cell is the crossing of one layer of each wall. A layer's rank is whether it is in its
//! wall's core (the core first), then its priority (1 to 5: its function's, Structure 1 to Finish 2 5, unless Edit Assembly's Priority
//! column gives it another). Layers are settled best rank
//! first: a layer runs from its own wall into the patch, cell after cell, through the other wall's layers of a worse rank;
//! it stops before a cell a better layer has taken; and where it reaches a layer of its own rank that reaches it too, the two
//! join there and both stop, the cell going to the wall that runs through the join. Nothing here knows where the walls are:
//! [`solve`] takes the order each wall's layers cross the other's in, and says how far each gets.

use std::collections::BTreeSet;

use crate::geom::{self, P2};
use crate::model::{Function, Id, Model, TOL, WallType};

/// A layer's standing where walls join: in the core before outside it, then by its function's priority (1 the best).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    pub shell: bool,
    pub priority: u8,
}

impl Rank {
    /// The rank of each layer of type `t`, exterior first; a type without layers is one structure layer, its core. A membrane
    /// has no priority of its own: the worst there is, and no thickness to hold a cell with.
    pub fn of(t: &WallType) -> Vec<Rank> {
        if t.layers.is_empty() {
            return vec![Rank { shell: false, priority: 1 }];
        }
        let core = t.core_layers();
        t.layers
            .iter()
            .enumerate()
            .map(|(k, l)| Rank { shell: !core.contains(&k), priority: l.join_priority().unwrap_or(Function::ALL.len() as u8) })
            .collect()
    }
}

/// One of the two walls at a join: its layers' ranks, exterior first, and each way it comes into the patch (one for a wall
/// that ends there, two for a wall that runs on through it, from either side): the other wall's layers, by index, in the
/// order its own layers cross them.
#[derive(Clone, Debug)]
pub struct Side {
    pub ranks: Vec<Rank>,
    pub arrives: Vec<Vec<usize>>,
}

/// How far each layer of a wall gets into the patch: for each layer, for each way its wall comes in, how many of the other
/// wall's layers it crosses and holds, counted along that way.
pub type Reach = Vec<Vec<usize>>;

/// A layer of one of the two walls coming in one way.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Runner {
    first: bool,
    layer: usize,
    way: usize,
}

/// How far each layer of `a` and of `b` gets. Where two layers of one rank meet, the cell is `a`'s with `a_through`, else
/// `b`'s.
pub fn solve(a: &Side, b: &Side, a_through: bool) -> (Reach, Reach) {
    let side = |first: bool| if first { a } else { b };
    // The cell a runner is in after `k` steps, as (layer of a, layer of b).
    let cell = |r: Runner, k: usize| -> Option<(usize, usize)> {
        let other = *side(r.first).arrives.get(r.way)?.get(k)?;
        Some(if r.first { (r.layer, other) } else { (other, r.layer) })
    };
    let rank = |first: bool, layer: usize| side(first).ranks.get(layer).copied();
    let mut held: [Reach; 2] = [a, b].map(|s| vec![vec![0; s.arrives.len()]; s.ranks.len()]);
    let at = |first: bool| usize::from(!first);
    let mut claimed: BTreeSet<(usize, usize)> = BTreeSet::new();
    let ranks: BTreeSet<Rank> = a.ranks.iter().chain(&b.ranks).copied().collect();
    for r in ranks {
        let runners: Vec<Runner> = [true, false]
            .into_iter()
            .flat_map(|first| {
                let s = side(first);
                (0..s.ranks.len())
                    .filter(move |k| s.ranks.get(*k) == Some(&r))
                    .flat_map(move |layer| (0..s.arrives.len()).map(move |way| Runner { first, layer, way }))
            })
            .collect();
        // How far each could go for the better layers alone.
        let free: Vec<usize> =
            runners.iter().map(|x| (0..).take_while(|k| cell(*x, *k).is_some_and(|c| !claimed.contains(&c))).count()).collect();
        // How many cells each gets to (a cell it meets its like in and loses, too), and how many of them it holds. Settled
        // by going round until nothing changes: one layer stopping short may let another run on.
        let (mut gets, mut holds) = (free.clone(), free.clone());
        for _ in 0..=runners.len() + 1 {
            let (mut next_gets, mut next_holds) = (free.clone(), free.clone());
            for (n, x) in runners.iter().enumerate() {
                for k in 0..free.get(n).copied().unwrap_or(0) {
                    let Some(c) = cell(*x, k) else { break };
                    let other = if x.first { c.1 } else { c.0 };
                    // Its like in the other wall, getting to this cell some way it comes in.
                    let met = rank(!x.first, other) == Some(r)
                        && runners
                            .iter()
                            .zip(&gets)
                            .any(|(y, got)| y.first != x.first && y.layer == other && (0..*got).any(|j| cell(*y, j) == Some(c)));
                    if met {
                        let wins = x.first == a_through;
                        if let (Some(g), Some(h)) = (next_gets.get_mut(n), next_holds.get_mut(n)) {
                            *g = k + 1;
                            *h = k + usize::from(wins);
                        }
                        break;
                    }
                }
            }
            if next_gets == gets && next_holds == holds {
                break;
            }
            (gets, holds) = (next_gets, next_holds);
        }
        for (x, h) in runners.iter().zip(&holds) {
            claimed.extend((0..*h).filter_map(|k| cell(*x, k)));
            if let Some(slot) = held.get_mut(at(x.first)).and_then(|w| w.get_mut(x.layer)).and_then(|l| l.get_mut(x.way)) {
                *slot = *h;
            }
        }
    }
    let [ra, rb] = held;
    (ra, rb)
}

/// A boundary of a wall, as two walls' boundaries cross: a straight wall's is a line (a point of it and the way it runs), a
/// curved wall's a circle (its centre and radius).
#[derive(Clone, Copy)]
enum Edge {
    Line(P2, P2),
    Circle(P2, f64),
}

/// Where two boundaries cross: of the two places a circle is crossed, the one nearer `near`.
fn meet(a: Edge, b: Edge, near: P2) -> Option<P2> {
    let nearer = |x: P2, y: P2| if x.dist(near) <= y.dist(near) { x } else { y };
    match (a, b) {
        (Edge::Line(p, u), Edge::Line(q, v)) => geom::line_intersection(p, p + u, q, q + v),
        (Edge::Line(p, u), Edge::Circle(c, r)) | (Edge::Circle(c, r), Edge::Line(p, u)) => {
            let (u, f) = (u.unit(), p - c);
            let half = f.dot(u);
            let under = half * half - (f.dot(f) - r * r);
            (under >= 0.0).then(|| nearer(p + u * (-half - under.sqrt()), p + u * (-half + under.sqrt())))
        }
        (Edge::Circle(c0, r0), Edge::Circle(c1, r1)) => {
            let d = c0.dist(c1);
            if d <= 1e-9 || d > r0 + r1 || d < (r0 - r1).abs() {
                return None;
            }
            let along = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
            let (u, off) = ((c1 - c0).unit(), (r0 * r0 - along * along).max(0.0).sqrt());
            Some(nearer(c0 + u * along + u.perp() * off, c0 + u * along - u.perp() * off))
        }
    }
}

/// A wall as the join's geometry reads it: its location line, straight or an arc, how far to its left each of its layers'
/// boundaries lies (its exterior face first), and each layer's rank and material.
struct Strips<'a> {
    axis: super::Axis,
    start: P2,
    end: P2,
    /// The way a straight one runs (an arc's chord: not for an arc's geometry).
    dir: P2,
    offs: Vec<f64>,
    ranks: Vec<Rank>,
    materials: Vec<&'a str>,
    /// How far to its left its core's faces lie: the exterior one, the interior one.
    core: (f64, f64),
}

impl Strips<'_> {
    fn of(m: &Model, id: Id) -> Option<Strips<'_>> {
        let w = m.wall(id)?;
        let t = m.types.wall.get(&w.type_name)?;
        let offs = super::boundaries(m, w);
        if w.start.dist(w.end) <= TOL || offs.len() < 2 {
            return None;
        }
        let materials = if t.layers.is_empty() { vec![""] } else { t.layers.iter().map(|l| l.material.as_str()).collect() };
        let ranks = Rank::of(t);
        let cored = if t.layers.is_empty() { 0..1 } else { t.core_layers() };
        let face = |k: usize| offs.get(k).copied().unwrap_or(0.0);
        let core = (face(cored.start), face(cored.end));
        (ranks.len() + 1 == offs.len()).then(|| Strips {
            axis: super::Axis::of(w),
            start: w.start,
            end: w.end,
            dir: (w.end - w.start).unit(),
            offs,
            ranks,
            materials,
            core,
        })
    }

    /// The line along it, nearest its location line, that runs in its core: how far to its left. Its location line, when
    /// that runs in its core.
    fn spine(&self) -> f64 {
        0.0_f64.clamp(self.core.1.min(self.core.0), self.core.0.max(self.core.1))
    }

    /// The line `off` mm to its left.
    fn line(&self, off: f64) -> Edge {
        match self.axis.arc() {
            Some((c, ..)) => Edge::Circle(c, c.dist(self.axis.at(0.0, off))),
            None => Edge::Line(self.start + self.dir.perp() * off, self.dir),
        }
    }

    fn layers(&self) -> usize {
        self.ranks.len()
    }

    fn straight(&self) -> bool {
        self.axis.arc().is_none()
    }

    /// Its boundary `k`.
    fn edge(&self, k: usize) -> Edge {
        let off = self.offs.get(k).copied().unwrap_or(0.0);
        match self.axis.arc() {
            Some((c, ..)) => Edge::Circle(c, c.dist(self.axis.at(0.0, off))),
            None => Edge::Line(self.start + self.dir.perp() * off, self.dir),
        }
    }

    /// Its end (`at_end`, else its start), and the way it runs on out of itself there.
    fn out(&self, at_end: bool) -> (P2, P2) {
        if at_end { (self.end, self.axis.tangent(self.axis.length())) } else { (self.start, self.axis.tangent(0.0) * -1.0) }
    }

    /// Where its boundary `k` crosses boundary `j` of `other`, near `near`.
    fn cross(&self, k: usize, other: &Strips, j: usize, near: P2) -> Option<P2> {
        meet(self.edge(k), other.edge(j), near)
    }

    fn thick(&self) -> f64 {
        self.offs.first().zip(self.offs.last()).map_or(0.0, |(a, b)| (a - b).abs())
    }

    /// The half of it to the left (`left`), or to the right, of the line `at` mm to the left of its location line: its
    /// layers there, the one the line runs through cut along it. With it, the material across the line from that half, and
    /// which of the wall's layers the half's first is.
    fn half(&self, left: bool, at: f64) -> (Strips<'_>, Option<&str>, usize) {
        let layers = self.layers();
        // The boundaries strictly to the left of the line, and the first strictly to its right.
        let above = self.offs.iter().filter(|o| **o > at + 1e-6).count().min(layers);
        let below = self.offs.iter().position(|o| *o < at - 1e-6).unwrap_or(self.offs.len()).max(1);
        let (offs, first, count, beyond) = if left {
            let mut offs: Vec<f64> = self.offs.iter().take(above).copied().collect();
            offs.push(at);
            (offs, 0, above, self.materials.get(below - 1).copied().filter(|_| below < self.offs.len()))
        } else {
            let mut offs = vec![at];
            offs.extend(self.offs.iter().skip(below).copied());
            (offs, below - 1, (layers + 1).saturating_sub(below), above.checked_sub(1).and_then(|k| self.materials.get(k).copied()))
        };
        let half = Strips {
            axis: self.axis,
            start: self.start,
            end: self.end,
            dir: self.dir,
            offs,
            ranks: self.ranks.iter().skip(first).take(count).copied().collect(),
            materials: self.materials.iter().skip(first).take(count).copied().collect(),
            core: self.core,
        };
        (half, beyond, first)
    }

    /// Its boundaries, by index, in the order something going `way` from `from` crosses them near `near`: first to last, or
    /// back.
    fn crossed(&self, from: P2, way: P2, near: P2) -> Option<Vec<usize>> {
        let along = |k: usize| meet(Edge::Line(from, way), self.edge(k), near).map(|q| (q - from).dot(way));
        let (first, last) = (along(0)?, along(self.layers())?);
        let all: Vec<usize> = (0..=self.layers()).collect();
        Some(if first <= last { all } else { all.into_iter().rev().collect() })
    }
}

/// The layers between boundaries crossed in `order`: layer `min(a, b)` between each two.
fn between(order: &[usize]) -> Vec<usize> {
    order.windows(2).filter_map(|w| Some(*w.first()?.min(w.get(1)?))).collect()
}

/// How a straight wall's end is cut where it joins another straight wall: each layer's end, the point on its exterior
/// boundary and the one on its interior boundary, and the lines the plan draws there: where a layer ends on, or runs beside,
/// a layer of the other wall of another material.
#[derive(Default)]
pub(super) struct Cut {
    pub(super) layers: Vec<[P2; 2]>,
    /// Where a layer's end bends between those two points, for the layers whose does: where three walls or more end at
    /// a point, the layer the wall's location line runs through ends against the wall on either side of it.
    pub(super) bends: Vec<Vec<P2>>,
    pub(super) seams: Vec<[P2; 2]>,
    /// The line the wall stops on against the other, where the join is not cleaned up: drawn at every level of detail.
    pub(super) butt: Option<[P2; 2]>,
}

/// The cut of wall `id`'s end (`at_end`, else its start) against wall `other`, which ends there too at its end or its start
/// (`other_end`), or which it ends on the side of (none). `through`: at a corner, this wall runs through; with `square`,
/// it is squared instead (Revit's Square off): its end stands at 90 degrees to itself where the other wall's far face meets
/// its own nearer face, where a Butt would run it on to a point along that far face. None when either is no straight wall
/// of a type, or they run too nearly the same way for their layers to cross near the join. Without `clean` (Revit's Don't
/// Clean Join) the layers are left as they come: the wall running through whole to the other's far face, the other whole
/// against its near face, and a line drawn between the two.
#[allow(clippy::too_many_arguments)]
pub(super) fn end_cut(
    m: &Model,
    id: Id,
    at_end: bool,
    other: Id,
    other_end: Option<bool>,
    through: bool,
    square: bool,
    clean: bool,
) -> Option<Cut> {
    let (me, o) = (Strips::of(m, id)?, Strips::of(m, other)?);
    let far = 6.0 * me.thick().max(o.thick());
    cut_of(&me, at_end, &o, other_end, through, square, clean, None, far)
}

/// [`end_cut`], of walls as the join's geometry reads them. `beyond`: the material past the other's last boundary, where
/// something stands there (the rest of a wall cut in half along its location line); `far`: how far from the join a layer
/// may end.
#[allow(clippy::too_many_arguments)]
fn cut_of(
    me: &Strips,
    at_end: bool,
    o: &Strips,
    other_end: Option<bool>,
    through: bool,
    square: bool,
    clean: bool,
    beyond: Option<&str>,
    far: f64,
) -> Option<Cut> {
    // The way this wall goes into the join, from its end there, and the way the other runs there.
    let (node, way) = me.out(at_end);
    let beside = other_end.map_or_else(|| o.axis.tangent(o.axis.project(node).0), |end| o.out(end).1);
    if way.cross(beside).abs() < 0.2 {
        return None;
    }
    let lines = o.crossed(node, way, node)?;
    let mine = Side { ranks: me.ranks.clone(), arrives: vec![between(&lines)] };
    let theirs = match other_end {
        Some(end) => {
            let (from, back) = o.out(end);
            Side { ranks: o.ranks.clone(), arrives: vec![between(&me.crossed(from, back, from)?)] }
        }
        // It runs on through: its layers come to this wall's from either side.
        None => Side { ranks: o.ranks.clone(), arrives: vec![(0..me.layers()).collect(), (0..me.layers()).rev().collect()] },
    };
    let (reach, _) = solve(&mine, &theirs, through && other_end.is_some());
    let whole = if through && other_end.is_some() { o.layers() } else { 0 };
    let held = |k: usize| if clean { reach.get(k).and_then(|r| r.first()).copied().unwrap_or(0) } else { whole };
    // Squared: no layer runs past where the other's far face meets this wall's nearer face.
    let along = |q: P2| (q - node).dot(way);
    let stop = match (square && through && other_end.is_some(), lines.last()) {
        (true, Some(last)) => Some(along(me.cross(0, o, *last, node)?).min(along(me.cross(me.layers(), o, *last, node)?))),
        _ => None,
    };
    let squared = |q: P2| match stop {
        Some(t) if along(q) > t => q - way * (along(q) - t),
        _ => q,
    };
    let mut cut = Cut::default();
    for k in 0..me.layers() {
        let line = *lines.get(held(k))?;
        let (a, b) = (squared(me.cross(k, o, line, node)?), squared(me.cross(k + 1, o, line, node)?));
        if a.dist(node) > far || b.dist(node) > far {
            return None;
        }
        cut.layers.push([a, b]);
        // The layer of the other wall past its end, when it stops short of the far face.
        let past = match mine.arrives.first().and_then(|order| order.get(held(k))) {
            Some(j) => o.materials.get(*j).copied(),
            None => beyond,
        };
        if clean && past.is_some_and(|x| Some(&x) != me.materials.get(k)) {
            cut.seams.push([a, b]);
        }
    }
    // Not cleaned up, the wall that stops draws the line the two ends butt on, face to face.
    if !clean && whole == 0 {
        cut.butt = cut.layers.first().zip(cut.layers.last()).map(|(first, last)| [first[0], last[1]]);
    }
    // Beside a layer that runs on past its neighbour: the other wall's layers along its side there. At a corner each such
    // side is the end of a layer of the other wall, which draws it; a wall that runs on draws no ends of its own there.
    for k in (1..me.layers()).filter(|_| other_end.is_none()) {
        let (outer, inner) = (held(k - 1), held(k));
        let longer = if outer > inner { k - 1 } else { k };
        for step in outer.min(inner)..outer.max(inner) {
            let beside = mine.arrives.first().and_then(|order| order.get(step)).copied();
            if beside.is_some_and(|j| o.materials.get(j) != me.materials.get(longer)) {
                cut.seams.push([me.cross(k, o, *lines.get(step)?, node)?, me.cross(k, o, *lines.get(step + 1)?, node)?]);
            }
        }
    }
    Some(cut)
}

/// A piece of a wall's layer near a join, convex: which wall's it is (by its place in the join's order) and which layer's.
#[derive(Clone)]
struct Piece {
    wall: usize,
    layer: usize,
    rank: Rank,
    material: String,
    ring: Vec<P2>,
}

/// What walls ending at a point make of one another, taken in order, as Revit's join of three or four walls is: the
/// first two join as the corner they would be alone, the first running through; each wall after them butts what is
/// there, its layers running in by rank (through layers of a worse rank, to the first of its own rank or a better).
pub(super) struct Ordered {
    /// Each wall's end, in the order given.
    pub(super) cuts: Vec<Cut>,
    /// What the walls give up to those after them: the wall (by its place in the order), its layer, and the piece of it
    /// a later wall's layer runs through, convex.
    pub(super) taken: Vec<(usize, usize, Vec<P2>)>,
}

/// Where the line through `from` going `way` is inside the convex `ring`: how far along it, in and out.
fn inside(ring: &[P2], from: P2, way: P2) -> Option<(f64, f64)> {
    let ccw = geom::area(ring) >= 0.0;
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (a, b) in geom::ring_edges(ring) {
        let edge = if ccw { b - a } else { a - b };
        // Inside is to the left of each edge of a counter-clockwise ring.
        let (side, turn) = (edge.cross(from - if ccw { a } else { b }), edge.cross(way));
        if turn.abs() < 1e-12 {
            if side < 0.0 {
                return None;
            }
        } else if turn > 0.0 {
            lo = lo.max(-side / turn);
        } else {
            hi = hi.min(-side / turn);
        }
    }
    (hi - lo > 1e-9).then_some((lo, hi))
}

/// The join of the wall ends `ends` (each a wall, and whether it is its end that is there, else its start), in their
/// order. None unless they are three or more straight walls of types, the first two crossing near the point. Without
/// `clean` (Revit's Don't Clean Join) no layer runs into another wall's: each wall stands whole against what is there, and
/// a line is drawn where it stops.
pub(super) fn ordered(m: &Model, ends: &[(Id, bool)], clean: bool) -> Option<Ordered> {
    let strips: Vec<Strips> = ends.iter().map(|(id, _)| Strips::of(m, *id).filter(Strips::straight)).collect::<Option<_>>()?;
    let (first, second) = (strips.first()?, strips.get(1)?);
    let (e0, e1) = (ends.first()?.1, ends.get(1)?.1);
    if strips.len() < 3 {
        return None;
    }
    let far = 6.0 * strips.iter().map(Strips::thick).fold(0.0, f64::max);
    let mut cuts = vec![
        cut_of(first, e0, second, Some(e1), true, false, clean, None, far)?,
        cut_of(second, e1, first, Some(e0), false, false, clean, None, far)?,
    ];
    // The two walls' layers near the point, each from well back along its wall to where the corner ends it.
    let mut pieces: Vec<Piece> = Vec::new();
    for (w, cut) in cuts.iter().enumerate() {
        let (strip, (_, at_end)) = (strips.get(w)?, ends.get(w)?);
        let (node, way) = strip.out(*at_end);
        let back = |q: P2| q - way * ((q - node).dot(way) + far);
        for (k, [a, b]) in cut.layers.iter().enumerate() {
            pieces.push(Piece {
                wall: w,
                layer: k,
                rank: *strip.ranks.get(k)?,
                material: strip.materials.get(k)?.to_string(),
                ring: vec![back(*a), *a, *b, back(*b)],
            });
        }
    }
    let mut taken = Vec::new();
    for (w, strip) in strips.iter().enumerate().skip(2) {
        let (cut, mine, gone) = butting(strip, ends.get(w)?.1, w, &pieces, far, clean)?;
        // What it ran through is no longer the earlier walls': to the walls after it, too.
        let mut left: Vec<Piece> = Vec::new();
        for (n, piece) in pieces.iter().enumerate() {
            let holes: Vec<&Vec<P2>> = gone.iter().filter(|(of, _)| *of == n).map(|(_, hole)| hole).collect();
            if holes.is_empty() {
                left.push(piece.clone());
                continue;
            }
            let rings = holes.iter().fold(vec![piece.ring.clone()], |rings, hole| super::minus(&rings, hole));
            left.extend(rings.into_iter().map(|ring| Piece { ring, ..piece.clone() }));
            taken.extend(holes.into_iter().map(|hole| (piece.wall, piece.layer, hole.clone())));
        }
        pieces = left;
        pieces.extend(mine);
        cuts.push(cut);
    }
    Some(Ordered { cuts, taken })
}

/// The end of a wall that butts what `pieces` hold already: its cut, its own layers' pieces near the point, and what of
/// `pieces` (by their places) its layers run through. A layer runs on from its wall, each line of it along the wall on its
/// own, through what is empty and through layers of a worse rank, and stops at the first layer of its own rank or a better
/// one; past the last thing it runs through with nothing to stop it, it ends there, and with nothing in its way at all,
/// square at the point.
#[allow(clippy::type_complexity)]
fn butting(
    me: &Strips,
    at_end: bool,
    place: usize,
    pieces: &[Piece],
    far: f64,
    clean: bool,
) -> Option<(Cut, Vec<Piece>, Vec<(usize, Vec<P2>)>)> {
    let (node, way) = me.out(at_end);
    let across = way.perp();
    let offset = |q: P2| (q - node).dot(across);
    let at = |u: f64, t: f64| node + across * u + way * t;
    // Where the pieces' corners lie across the wall: between two of them next each other, every piece's edges run
    // straight across.
    let mut corners: Vec<f64> = pieces.iter().flat_map(|p| p.ring.iter().map(|q| offset(*q))).collect();
    corners.sort_by(f64::total_cmp);
    let (mut cut, mut mine, mut gone) = (Cut::default(), Vec::new(), Vec::new());
    // Each layer's first and last stretch: how far along it ends, and the material that stops it.
    let mut edges: Vec<[(f64, Option<String>); 2]> = Vec::new();
    for k in 0..me.layers() {
        let (Edge::Line(p0, _), Edge::Line(p1, _)) = (me.edge(k), me.edge(k + 1)) else { return None };
        let (u0, u1) = (offset(p0), offset(p1));
        let (lo, hi) = (u0.min(u1), u0.max(u1));
        let mut marks: Vec<f64> = corners.iter().copied().filter(|c| *c > lo + 1e-6 && *c < hi - 1e-6).collect();
        marks.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        if u0 > u1 {
            marks.reverse();
        }
        let mut stops = vec![u0];
        stops.extend(marks);
        stops.push(u1);
        let (rank, material) = (*me.ranks.get(k)?, *me.materials.get(k)?);
        // The layer's end, across it from its exterior side: the points of it, and each stretch's reach and stopper.
        let mut end: Vec<P2> = Vec::new();
        let mut reaches: Vec<(f64, Option<String>)> = Vec::new();
        for pair in stops.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            if (to - from).abs() <= 1e-6 {
                continue;
            }
            let inset = (to - from).signum() * 1e-7;
            let (near, middle, last) = (from + inset, (from + to) / 2.0, to - inset);
            // What lies along the middle of the stretch, nearest first.
            let mut hits: Vec<(usize, f64, f64)> = pieces
                .iter()
                .enumerate()
                .filter_map(|(n, p)| inside(&p.ring, at(middle, 0.0), way).map(|(t0, t1)| (n, t0, t1)))
                .filter(|(_, _, t1)| *t1 > -far + 1e-6)
                .collect();
            hits.sort_by(|a, b| a.1.total_cmp(&b.1));
            let mut passed: Vec<usize> = Vec::new();
            let mut stopper: Option<usize> = None;
            for (n, ..) in &hits {
                if pieces.get(*n).is_some_and(|p| !clean || p.rank <= rank) {
                    stopper = Some(*n);
                    break;
                }
                passed.push(*n);
            }
            // How far each side of the stretch runs: into the stopper, else out of the last thing passed, else to the point.
            let reach = |u: f64| -> f64 {
                let through = |n: usize| pieces.get(n).and_then(|p| inside(&p.ring, at(u, 0.0), way));
                let t = match stopper {
                    Some(n) => through(n).map(|(t0, _)| t0),
                    None => passed.iter().filter_map(|n| through(*n).map(|(_, t1)| t1)).max_by(f64::total_cmp),
                };
                t.unwrap_or(0.0).clamp(-far, far)
            };
            let (t0, t1) = (reach(near), reach(last));
            let (q0, q1) = (at(from, t0), at(to, t1));
            let stopped_by = stopper.and_then(|n| pieces.get(n)).map(|p| p.material.clone());
            if stopped_by.as_deref().is_some_and(|other| !clean || other != material) {
                cut.seams.push([q0, q1]);
            }
            // Beside the stretch before it, where one runs on past the other: the shorter one's stopper is beside the longer.
            if let (Some(before), Some((t_before, stopper_before))) = (end.last().copied(), reaches.last()) {
                let beside = if *t_before < t0 { stopper_before.as_deref() } else { stopped_by.as_deref() };
                if before.dist(q0) > 1e-6 && beside.is_some_and(|other| other != material) {
                    cut.seams.push([before, q0]);
                }
            }
            for n in passed {
                let Some(piece) = pieces.get(n) else { continue };
                let within =
                    geom::clip(&geom::clip(&piece.ring, |q| offset(q) - lo.max(from.min(to))), |q| hi.min(from.max(to)) - offset(q));
                if within.len() >= 3 && geom::area(&within).abs() > 1.0 {
                    gone.push((n, within));
                }
            }
            mine.push(Piece {
                wall: place,
                layer: k,
                rank,
                material: material.to_string(),
                ring: vec![at(from, -far), q0, q1, at(to, -far)],
            });
            end.extend([q0, q1]);
            reaches.extend([(t0, stopped_by.clone()), (t1, stopped_by)]);
        }
        end.dedup_by(|a, b| a.dist(*b) < 1e-6);
        let (a, b) = (*end.first()?, *end.last()?);
        cut.layers.push([a, b]);
        cut.bends.push(end.iter().copied().skip(1).take(end.len().saturating_sub(2)).collect());
        edges.push([reaches.first()?.clone(), reaches.last()?.clone()]);
    }
    // Between two of its own layers, where one runs on past the other: the shorter one's stopper is beside the longer.
    for k in 1..cut.layers.len() {
        let (Some(before), Some(after), Some(e0), Some(e1)) = (cut.layers.get(k - 1), cut.layers.get(k), edges.get(k - 1), edges.get(k))
        else {
            continue;
        };
        let ((t_before, stop_before), (t_after, stop_after)) = (&e0[1], &e1[0]);
        let (beside, longer) = if t_before < t_after { (stop_before, k) } else { (stop_after, k - 1) };
        if before[1].dist(after[0]) > 1e-6 && beside.as_deref().is_some_and(|other| Some(&other) != me.materials.get(longer)) {
            cut.seams.push([before[1], after[0]]);
        }
    }
    Some((cut, mine, gone))
}

/// The cut of wall `id`'s end (`at_end`, else its start) where three walls or more end at a point, none of them running
/// on: against the wall next round the point counter-clockwise, `next`, and the one before, `prev` (each a wall and which
/// of its ends is there). Each wall is read as two halves, either side of a line along it: between two neighbours'
/// lines, the half of each that faces the other joins it as two walls do at a corner, their layers cleaned up by rank,
/// the wall made first running through. The layer a wall's line runs through ends against both neighbours: its end
/// bends. Towards a neighbour too nearly in line for their layers to cross near the point, it ends square.
///
/// The line is the wall's location line; with `cored` (three walls), the line nearest that which runs in its core
/// ([`Strips::spine`]), so the three cores come together whatever lines the walls stand on. Those three lines need not
/// cross at one point: the triangle they leave between them, which no two walls' halves cover, is the core's of the
/// wall made first.
pub(super) fn wedge_cut(m: &Model, id: Id, at_end: bool, next: (Id, bool), prev: (Id, bool), cored: bool) -> Option<Cut> {
    let (me, n, p) = (Strips::of(m, id)?, Strips::of(m, next.0)?, Strips::of(m, prev.0)?);
    let far = 6.0 * me.thick().max(n.thick()).max(p.thick());
    let spine = |x: &Strips| if cored { x.spine() } else { 0.0 };
    let (node, _) = me.out(at_end);
    // Leaving the point from its start, a wall's left is its counter-clockwise side; from its end, its right is.
    let towards = |mine: bool, other: &Strips, (other_id, other_end): (Id, bool), others_left: bool| -> Option<(Cut, usize)> {
        let (half, _, first) = me.half(mine, spine(&me));
        let (theirs, beyond, _) = other.half(others_left, spine(other));
        // A neighbour too nearly in line to cross: the half ends square at the point, as two walls in line do.
        let cut = cut_of(&half, at_end, &theirs, Some(other_end), id < other_id, false, true, beyond, far).unwrap_or_else(|| {
            let s = if at_end { half.axis.length() } else { 0.0 };
            let at = |k: usize| half.axis.at(s, half.offs.get(k).copied().unwrap_or(0.0));
            Cut { layers: (0..half.layers()).map(|k| [at(k), at(k + 1)]).collect(), ..Cut::default() }
        });
        Some((cut, first))
    };
    let (to_next, to_prev) = (towards(!at_end, &n, next, next.1)?, towards(at_end, &p, prev, !prev.1)?);
    // The half on its left has its exterior layers, the first of them.
    let ((left, _), (right, first)) = if at_end { (to_prev, to_next) } else { (to_next, to_prev) };
    let mut cut = Cut { seams: left.seams.iter().chain(&right.seams).copied().collect(), ..Cut::default() };
    for (k, ends) in left.layers.iter().enumerate() {
        match right.layers.first().filter(|_| k == first) {
            // The layer the location line runs through: its end against one neighbour, along the line, and against the other.
            Some(rest) => {
                cut.layers.push([ends[0], rest[1]]);
                let mut bend = vec![ends[1], rest[0]];
                bend.dedup_by(|a, b| a.dist(*b) < 1e-6);
                cut.bends.push(bend);
            }
            None => {
                cut.layers.push(*ends);
                cut.bends.push(Vec::new());
            }
        }
    }
    let shared = usize::from(first < left.layers.len());
    for ends in right.layers.iter().skip(shared) {
        cut.layers.push(*ends);
        cut.bends.push(Vec::new());
    }
    // The triangle between the three walls' lines, for the wall made first: its halves run to where its line crosses
    // each neighbour's, and its core takes in the triangle from there round by where the neighbours' two lines cross.
    let on_line = left.layers.last().map(|l| l[1]).zip(right.layers.first().map(|l| l[0]));
    if cored
        && id < next.0
        && id < prev.0
        && next.0 != prev.0
        && let Some((m_left, m_right)) = on_line
    {
        let mine = me.line(spine(&me));
        // The half on the left ends towards the neighbour on that side: the next round the point from a wall's start.
        let (to_left, to_right) = if at_end { (&p, &n) } else { (&n, &p) };
        let corners = (
            meet(mine, to_left.line(spine(to_left)), node),
            meet(mine, to_right.line(spine(to_right)), node),
            meet(n.line(spine(&n)), p.line(spine(&p)), node),
        );
        if let (Some(a), Some(b), Some(third)) = corners
            && a.dist(b) > 1e-6
            && third.dist(node) <= far
        {
            // Its core's half on a side has to reach the corner there: it does, running through, unless the neighbour's
            // layers are better than its core.
            let (reaches_left, reaches_right) = (a.dist(m_left) < 1e-6, b.dist(m_right) < 1e-6);
            let last = left.layers.len() - 1;
            if shared == 1 {
                // Its line runs through a layer: that layer's end goes round the triangle.
                if let Some(bend) = cut.bends.get_mut(last).filter(|_| reaches_left && reaches_right) {
                    *bend = vec![a, third, b];
                }
            } else if (spine(&me) - me.core.0).abs() < 1e-6 {
                // Its line is its core's exterior face: the core is the first layer on the right.
                if let (Some(ends), Some(bend), true) = (cut.layers.get_mut(last + 1), cut.bends.get_mut(last + 1), reaches_right) {
                    ends[0] = a;
                    *bend = vec![third, b];
                }
            } else if let (Some(ends), Some(bend), true) = (cut.layers.get_mut(last), cut.bends.get_mut(last), reaches_left) {
                // Its line is its core's interior face: the core is the last layer on the left.
                ends[1] = b;
                *bend = vec![a, third];
            }
        }
    }
    (cut.layers.len() == me.layers()).then_some(cut)
}

/// A stretch of a wall's layer another wall's layer holds, where that wall ends on its side: between two lines across it,
/// each a point and the way it runs, the first nearer the wall's start.
#[derive(Clone, Copy, Debug)]
pub(super) struct Gap {
    pub(super) from: (P2, P2),
    pub(super) to: (P2, P2),
}

/// Where each layer of the straight wall `id` is broken by the straight wall `other`, whose end (`other_end`, else its
/// start) lies on its side: a gap for each layer the other's layers pass through, none for those that run on whole.
pub(super) fn gaps(m: &Model, id: Id, other: Id, other_end: bool) -> Option<Vec<Option<Gap>>> {
    let (me, o) = (Strips::of(m, id)?, Strips::of(m, other)?);
    let (from, way) = o.out(other_end);
    // The way this wall runs where the other ends on it: a curved one's, there.
    let along = me.axis.tangent(me.axis.project(from).0);
    if along.cross(way).abs() < 0.2 {
        return None;
    }
    // The other's boundaries in the order this wall crosses them, start to end; this wall's in the order the other does.
    let lines = o.crossed(from - along * (4.0 * o.thick() + me.thick()), along, from)?;
    let cells = between(&lines);
    let mine = Side { ranks: me.ranks.clone(), arrives: vec![cells.clone(), cells.iter().rev().copied().collect()] };
    let theirs = Side { ranks: o.ranks.clone(), arrives: vec![between(&me.crossed(from, way, from)?)] };
    let (reach, _) = solve(&mine, &theirs, true);
    // The other's boundary across layer `k` of this wall: through where it crosses the layer's two sides, a curved one too.
    let line = |n: usize, k: usize| {
        let j = *lines.get(n)?;
        let (a, b) = (o.cross(j, &me, k, from)?, o.cross(j, &me, k + 1, from)?);
        Some(if a.dist(b) > TOL { (a, b - a) } else { (a, way) })
    };
    Some(
        (0..me.layers())
            .map(|k| {
                let (before, after) = (reach.get(k)?.first().copied()?, reach.get(k)?.get(1).copied()?);
                (before + after < o.layers()).then_some(())?;
                Some(Gap { from: line(before, k)?, to: line(o.layers() - after, k)? })
            })
            .collect(),
    )
}

#[cfg(test)]
#[path = "join_tests.rs"]
mod tests;

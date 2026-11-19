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
        (ranks.len() + 1 == offs.len()).then(|| Strips {
            axis: super::Axis::of(w),
            start: w.start,
            end: w.end,
            dir: (w.end - w.start).unit(),
            offs,
            ranks,
            materials,
        })
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
    let far = 6.0 * me.thick().max(o.thick());
    // Squared: no layer runs past where the other's far face meets this wall's nearer face.
    let along = |q: P2| (q - node).dot(way);
    let stop = match (square && through && other_end.is_some(), lines.last()) {
        (true, Some(last)) => Some(along(me.cross(0, &o, *last, node)?).min(along(me.cross(me.layers(), &o, *last, node)?))),
        _ => None,
    };
    let squared = |q: P2| match stop {
        Some(t) if along(q) > t => q - way * (along(q) - t),
        _ => q,
    };
    let mut cut = Cut::default();
    for k in 0..me.layers() {
        let line = *lines.get(held(k))?;
        let (a, b) = (squared(me.cross(k, &o, line, node)?), squared(me.cross(k + 1, &o, line, node)?));
        if a.dist(node) > far || b.dist(node) > far {
            return None;
        }
        cut.layers.push([a, b]);
        // The layer of the other wall past its end, when it stops short of the far face.
        let past = mine.arrives.first().and_then(|order| order.get(held(k))).copied();
        if clean && past.is_some_and(|j| o.materials.get(j) != me.materials.get(k)) {
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
                cut.seams.push([me.cross(k, &o, *lines.get(step)?, node)?, me.cross(k, &o, *lines.get(step + 1)?, node)?]);
            }
        }
    }
    Some(cut)
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

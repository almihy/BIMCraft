//! Wall geometry. A wall's location line runs from its start to its end, straight or an arc; its
//! faces and its layers' boundaries lie across it where its type and its location line put them,
//! the exterior side to its left. Where walls meet, end to end at any angle or one ending on
//! another, their faces meet in corners: each end's cap. Caps and faces make the outline plans
//! fill, 3D and sections cut, and IFC sweeps. A wall's top may follow a roof or a floor above it,
//! its base one below: the lowest of their planes, which is a pitched roof's surface.

use std::f64::consts::{PI, TAU};

#[doc(hidden)]
pub mod index;
#[doc(hidden)]
pub mod join;
mod wrap;
#[doc(hidden)]
pub use index::Index;
pub(crate) use index::reach;

use crate::geom::{self, P2};
use crate::model::{Element, Guid, Id, Location, Model, TOL, Wall, WallType};

/// A wall's location line, from its start to its end: straight, or an arc.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub start: P2,
    pub end: P2,
    /// An arc's centre, its radius, the angle its start lies at from the centre, and how far it
    /// sweeps from there (radians, counter-clockwise positive).
    arc: Option<(P2, f64, f64, f64)>,
}

impl Axis {
    pub fn of(w: &Wall) -> Axis {
        Axis::new(w.start, w.end, w.arc)
    }

    /// From `start` to `end`, turning `arc` degrees.
    pub fn new(start: P2, end: P2, arc: f64) -> Axis {
        let (sweep, d) = (arc.to_radians(), start.dist(end));
        if !(sweep.is_finite() && sweep.abs() > 1e-9 && d > 0.0) {
            return Axis { start, end, arc: None };
        }
        let half = sweep.abs() / 2.0;
        // The centre: off the chord's middle, to its left turning left, past half a circle to its right.
        let c = (start + end) * 0.5 + (end - start).unit().perp() * (sweep.signum() * d / (2.0 * half.tan()));
        Axis { start, end, arc: Some((c, d / (2.0 * half.sin()), (start.1 - c.1).atan2(start.0 - c.0), sweep)) }
    }

    pub fn centre(&self) -> Option<P2> {
        self.arc.map(|a| a.0)
    }

    pub fn radius(&self) -> Option<f64> {
        self.arc.map(|a| a.1)
    }

    /// An arc's centre, radius, the angle its start lies at from the centre and how far it sweeps (radians,
    /// counter-clockwise positive); none when it's straight.
    pub fn arc(&self) -> Option<(P2, f64, f64, f64)> {
        self.arc
    }

    pub fn length(&self) -> f64 {
        match self.arc {
            Some((_, r, _, sweep)) => r * sweep.abs(),
            None => self.start.dist(self.end),
        }
    }

    /// The angle from the centre `s` mm along an arc.
    fn angle(r: f64, a0: f64, sweep: f64, s: f64) -> f64 {
        a0 + sweep.signum() * s / r
    }

    /// The way it runs `s` mm along it.
    pub fn tangent(&self, s: f64) -> P2 {
        match self.arc {
            Some((_, r, a0, sweep)) => {
                let a = Axis::angle(r, a0, sweep, s);
                P2(-a.sin(), a.cos()) * sweep.signum()
            }
            None => (self.end - self.start).unit(),
        }
    }

    /// The point `s` mm along it, `off` mm to its left.
    pub fn at(&self, s: f64, off: f64) -> P2 {
        match self.arc {
            // Turning left, its left is toward the centre.
            Some((c, r, a0, sweep)) => {
                let a = Axis::angle(r, a0, sweep, s);
                c + P2(a.cos(), a.sin()) * (r - sweep.signum() * off)
            }
            None => {
                let u = (self.end - self.start).unit();
                self.start + u * s + u.perp() * off
            }
        }
    }

    /// How far along it the point nearest `p` is (from 0 to its length), and how far to its left
    /// `p` lies.
    pub fn project(&self, p: P2) -> (f64, f64) {
        match self.arc {
            Some((c, r, a0, sweep)) => {
                // Turned from its start the way it turns, counted from its middle so a point past
                // either end goes to the nearer one.
                let mid = sweep.abs() / 2.0;
                let t = (sweep.signum() * ((p.1 - c.1).atan2(p.0 - c.0) - a0) - mid + PI).rem_euclid(TAU) - PI + mid;
                (t.clamp(0.0, sweep.abs()) * r, sweep.signum() * (r - p.dist(c)))
            }
            None => {
                let u = (self.end - self.start).unit();
                ((p - self.start).dot(u).clamp(0.0, self.length()), (p - self.start).dot(u.perp()))
            }
        }
    }

    /// How far `p` is from the line `off` mm to its left.
    pub fn distance(&self, p: P2, off: f64) -> f64 {
        p.dist(self.at(self.project(p).0, off))
    }

    /// Where it's cut into pieces from `s0` to `s1`: the two, and along an arc, enough between that
    /// no chord strays a millimetre from it or turns more than 5°.
    pub fn stations(&self, s0: f64, s1: f64) -> Vec<f64> {
        let n = match self.arc {
            Some((_, r, _, _)) => {
                let step = (2.0 * (1.0 - 1.0 / r.max(2.0)).acos()).min(5f64.to_radians());
                ((s1 - s0).abs() / r / step).ceil().clamp(1.0, 720.0) as usize
            }
            None => 1,
        };
        (0..=n).map(|i| s0 + (s1 - s0) * i as f64 / n as f64).collect()
    }

    /// Points from `s0` to `s1` along the line `off` mm to its left: see [`Axis::stations`].
    pub fn points(&self, s0: f64, s1: f64, off: f64) -> Vec<P2> {
        self.stations(s0, s1).into_iter().map(|s| self.at(s, off)).collect()
    }
}

/// How far an arc from `a` through `p` to `b` turns, degrees counter-clockwise: 0 when `p` lies on
/// the line from `a` to `b`, more the farther it bows out to the right.
pub fn sweep_through(a: P2, p: P2, b: P2) -> f64 {
    let (u, v) = (a - p, b - p);
    let side = (b - a).cross(p - a);
    if side.abs() <= 1e-9 * (b - a).length() * (p - a).length() {
        return 0.0;
    }
    // Twice the angle the chord subtends beyond the one at `p`, the way the arc turns.
    let sweep = 2.0 * (PI - u.cross(v).atan2(u.dot(v)).abs());
    if side < 0.0 { sweep.to_degrees() } else { -sweep.to_degrees() }
}

/// How far from the exterior face of a wall of type `t` its location line `loc` lies: its core between the type's Core
/// Boundary rows ([`WallType::core_span`]).
pub fn depth(t: &WallType, loc: Location) -> f64 {
    let (c0, c1) = t.core_span();
    match loc {
        Location::Centre => t.thickness / 2.0,
        Location::CoreCentre => (c0 + c1) / 2.0,
        Location::ExteriorFinish => 0.0,
        Location::InteriorFinish => t.thickness,
        Location::ExteriorCore => c0,
        Location::InteriorCore => c1,
    }
}

/// Where a wall's faces and its layers' boundaries lie: mm to the left of its location line, from
/// its exterior face to its interior one.
pub fn boundaries(m: &Model, w: &Wall) -> Vec<f64> {
    let Some(t) = m.types.wall.get(&w.type_name) else { return vec![0.0, 0.0] };
    let ext = depth(t, w.location);
    let mut out = vec![ext];
    let mut at = ext;
    for l in &t.layers {
        at -= l.thickness;
        out.push(at);
    }
    if t.layers.is_empty() {
        out.push(ext - t.thickness);
    }
    out
}

/// A wall's exterior and interior faces: how far to the left of its location line they lie.
pub fn faces(m: &Model, w: &Wall) -> (f64, f64) {
    let t = m.thickness(w);
    let ext = m.types.wall.get(&w.type_name).map_or(t / 2.0, |ty| depth(ty, w.location));
    (ext, ext - t)
}

/// How far to the left of its location line a wall's core's exterior and interior boundaries lie, when it has a core of its
/// own: a structure layer, the core running from the first to the last, that finishes, insulation or membranes lie outside of
/// (a core that is all of the wall's thickness is its faces, and has none of its own). Revit's Center of Core and Faces of Core.
pub fn core_faces(m: &Model, w: &Wall) -> Option<(f64, f64)> {
    let ty = m.types.wall.get(&w.type_name)?;
    let t = m.thickness(w);
    let (c0, c1) = ty.core_span();
    if c0 <= 1e-6 && c1 >= t - 1e-6 {
        return None;
    }
    let ext = depth(ty, w.location);
    Some((ext - c0, ext - c1))
}

/// How far to the left of its location line a wall's centre line lies.
pub fn centre(m: &Model, w: &Wall) -> f64 {
    let (ext, int) = faces(m, w);
    (ext + int) / 2.0
}

/// Whether two walls overlap in height, so they join.
pub fn stacked(m: &Model, a: &Wall, b: &Wall) -> bool {
    m.base_z(a) < m.top_z(b) && m.base_z(b) < m.top_z(a)
}

/// A wall where it leaves a node: the way it goes, and where its faces' lines and its centre line
/// are there (each running that way): the face on the counter-clockwise side of that way, the one
/// on the clockwise side, and which wall's end it is (`id`, `at_end`). A run's arm at its end is one too ([`crate::runs`]),
/// naming no wall: its `id` is 0, below every element's.
#[derive(Default)]
pub(crate) struct Arm {
    pub(crate) id: Id,
    pub(crate) at_end: bool,
    pub(crate) mine: bool,
    pub(crate) way: P2,
    pub(crate) ccw: P2,
    pub(crate) cw: P2,
    pub(crate) centre: P2,
    pub(crate) thick: f64,
}

/// Wall `w` leaving the node at its start, or with `at_end`, at its end.
pub(crate) fn arm(m: &Model, id: Id, w: &Wall, at_end: bool, mine: bool) -> Arm {
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let s = if at_end { axis.length() } else { 0.0 };
    let t = axis.tangent(s);
    // Leaving from its end, it goes back: its interior face is on the counter-clockwise side.
    let (way, ccw, cw) = if at_end { (t * -1.0, int, ext) } else { (t, ext, int) };
    Arm { id, at_end, mine, way, ccw: axis.at(s, ccw), cw: axis.at(s, cw), centre: axis.at(s, (ext + int) / 2.0), thick: ext - int }
}

/// The walls at wall `id`'s start (or `at_end`, its end), round the node counter-clockwise, its own
/// among them: those ending there, and a wall it ends on, both ways from where it does. Whether it
/// ends on one, too.
fn arms(m: &Model, id: Id, at_end: bool, index: Option<&Index>) -> (Vec<Arm>, bool) {
    let Some(w) = m.wall(id) else { return (Vec::new(), false) };
    let node = if at_end { w.end } else { w.start };
    let mut arms = vec![arm(m, id, w, at_end, true)];
    let mut through = false;
    // An end that refuses to join meets nothing: its own square end is its whole cap.
    // A curtain wall joins nothing, and nothing joins it: its panels and mullions end where its line does.
    if !w.joins(at_end) || crate::curtain::is_curtain(m, w) {
        return (arms, false);
    }
    // An end that refuses to join is no candidate for anyone either, nor a wall that refuses altogether for one ending on
    // its side: neither is cut to the other.
    let joins = |o: &Wall, end: Option<bool>| end.map_or(o.join, |e| o.joins(e)) && !crate::curtain::is_curtain(m, o) && stacked(m, w, o);
    let candidates: Box<dyn Iterator<Item = (Id, &Wall)> + '_> = match index {
        Some(index) => Box::new(index.neighbours_at(id, node).into_iter().filter_map(|id| Some((id, m.wall(id)?)))),
        // The walls whose location lines come within the thickest wall's thickness of the node (those that end at it,
        // and those it ends on: the node within their thickness), of heights it overlaps (only those join it).
        None => {
            let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
            let at = ([node.0, node.1, node.0, node.1], m.base_z(w), m.top_z(w));
            let near = m.lookup().walls_near_at(&[at], thickest + 1.0);
            Box::new(near.into_iter().filter_map(|id| Some((id, m.wall(id)?))))
        }
    };
    // Where they meet is asked first: whether they overlap in height takes both walls' heights, under any roof or floor
    // they're attached to.
    for (other, o) in candidates {
        if other == id {
            continue;
        }
        if o.start.dist(node) <= TOL {
            if joins(o, Some(false)) {
                arms.push(arm(m, other, o, false, false));
            }
        } else if o.end.dist(node) <= TOL {
            if joins(o, Some(true)) {
                arms.push(arm(m, other, o, true, false));
            }
        } else {
            // A wall it ends on: the node within its thickness, between its ends.
            let ax = Axis::of(o);
            let (s, across) = ax.project(node);
            let (ext, int) = faces(m, o);
            if s > TOL
                && s < ax.length() - TOL
                && (int - TOL..=ext + TOL).contains(&across)
                && ax.distance(node, across) <= TOL
                && joins(o, None)
            {
                let (t, (e, i), c) = (ax.tangent(s), (ax.at(s, ext), ax.at(s, int)), ax.at(s, (ext + int) / 2.0));
                arms.push(Arm { id: other, at_end: false, mine: false, way: t, ccw: e, cw: i, centre: c, thick: ext - int });
                arms.push(Arm { id: other, at_end: false, mine: false, way: t * -1.0, ccw: i, cw: e, centre: c, thick: ext - int });
                through = true;
            }
        }
    }
    arms.sort_by(|a, b| a.way.1.atan2(a.way.0).total_cmp(&b.way.1.atan2(b.way.0)));
    (arms, through)
}

/// The corner points that close wall `id`'s start, or with `at_end`, its end: from its face on the
/// clockwise side of the way it leaves the node round to the other. Walls ending where it ends meet
/// it at any angle, their faces running on until they cross (no farther from the node than the
/// thickest is thick, either way: past that, the end is square); a wall it ends on stops it at its
/// face. At a square compound Butt the shared cap follows the structure around the corner.
/// Where three or more walls meet, the node itself fills between them.
pub fn cap(m: &Model, id: Id, at_end: bool) -> Vec<P2> {
    cap_in(m, id, at_end, None)
}

#[doc(hidden)]
pub fn cap_in(m: &Model, id: Id, at_end: bool, index: Option<&Index>) -> Vec<P2> {
    end_geometry(m, id, at_end, index).outline
}

#[derive(Default)]
struct WallEnd {
    outline: Vec<P2>,
    /// Its layers' ends, where it joins another straight wall and their layers clean up ([`join::end_cut`]).
    cut: Option<join::Cut>,
    /// It joins nothing: the end its layers wrap at, where its type wraps at ends.
    free: bool,
}

impl WallEnd {
    fn plain(outline: Vec<P2>) -> Self {
        Self { outline, cut: None, free: false }
    }

    /// The end `cut` makes: its cap steps from layer to layer, from its face on the clockwise side of the way it leaves the
    /// node (its interior face at its start, its exterior one at its end) round to the other.
    fn stepped(cut: join::Cut, at_end: bool) -> Self {
        // Each layer's end, from its exterior side to its interior one: bent, where it ends against two walls.
        let ends = cut.layers.iter().enumerate().map(|(k, [a, b])| {
            let mut end = vec![*a];
            end.extend(cut.bends.get(k).into_iter().flatten().copied());
            end.push(*b);
            end
        });
        let mut outline: Vec<P2> = if at_end { ends.flatten().collect() } else { ends.rev().flat_map(|e| e.into_iter().rev()).collect() };
        outline.dedup_by(|a, b| a.dist(*b) < 1e-6);
        Self { outline, cut: Some(cut), free: false }
    }
}

fn end_geometry(m: &Model, id: Id, at_end: bool, index: Option<&Index>) -> WallEnd {
    let Some(w) = m.wall(id) else { return WallEnd::default() };
    let (arms, through) = arms(m, id, at_end, index);
    let n = arms.len();
    let Some(i) = arms.iter().position(|a| a.id == id) else { return WallEnd::default() };
    let (prev, me, next) = (&arms[(i + n - 1) % n], &arms[i], &arms[(i + 1) % n]);
    if n < 2 {
        return WallEnd { free: true, ..WallEnd::plain(vec![me.cw, me.ccw]) };
    }
    // Two straight walls: a Butt, Revit's default, their layers cleaned up by rank. At a corner the wall made first (the
    // lower id) runs through, so the same walls drawn the same way join the same way; which one does is the person's to
    // change once a join keeps its order (the Wall Joins tool's Previous and Next). A wall ending on another's side stops
    // against it, the other running on.
    if n == 2 && !through {
        // What the Wall Joins tool set for this corner, if anything: its kind, and which wall is first.
        let kept = m.wall_join((m.guid(id), at_end), (m.guid(next.id), next.at_end));
        let first = kept.and_then(|j| j.first).map_or(id < next.id, |g| g == m.guid(id));
        match kept.map_or(crate::model::JoinKind::Butt, |j| j.kind) {
            // A Mitre is the faces' crossings, below.
            crate::model::JoinKind::Mitre => {}
            kind => {
                let square = kind == crate::model::JoinKind::SquareOff;
                let clean = kept.is_none_or(|j| j.display != crate::model::JoinDisplay::DontClean);
                if let Some(cut) = join::end_cut(m, id, at_end, next.id, Some(next.at_end), first, square, clean) {
                    return WallEnd::stepped(cut, at_end);
                }
            }
        }
    }
    // Walls ending on the side of one that runs on (one, or one from each side: a crossing): each joins it as a T.
    if n >= 3
        && through
        && let Some(on) = running_on(&arms).filter(|on| *on != id)
        && let Some(cut) = join::end_cut(m, id, at_end, on, None, false, false, tee_cleans(m, id, at_end, on))
    {
        return WallEnd::stepped(cut, at_end);
    }
    // Ends at a point, two of them one wall carried on in line (a wall split in two, or drawn in two lengths; where two
    // such pairs cross, the pair with the wall made first): the others join the pair as they would the one wall, and the
    // two stand square against each other there.
    if n >= 3
        && !through
        && let Some((a, b)) = carried_on(m, &arms)
    {
        if i == a || i == b {
            return WallEnd::plain(vec![me.cw, me.ccw]);
        }
        if let Some(cut) = arms.get(a).and_then(|on| join::end_cut(m, id, at_end, on.id, None, false, false, true)) {
            return WallEnd::stepped(cut, at_end);
        }
    }
    // Three or four ends at a point, none in line with another, as Revit joins them: in order, the first two as the
    // corner they would be alone, each one after butting what is there.
    let order = node_order(m, &arms, through);
    let (kind, clean) = order.as_deref().map_or((crate::model::JoinKind::Butt, true), |order| order_setting(m, order));
    let mitred = kind == crate::model::JoinKind::Mitre;
    if let Some(order) = order.as_deref().filter(|_| !mitred)
        && let Some(place) = order.iter().position(|(other, end)| *other == id && *end == at_end)
        && let Some(cut) =
            join::ordered(m, order, clean).and_then(|mut joined| (place < joined.cuts.len()).then(|| joined.cuts.swap_remove(place)))
    {
        return WallEnd::stepped(cut, at_end);
    }
    // Else each joins the one on either side of it, half a wall to
    // half a wall, their layers cleaned up by rank as at a corner.
    if n >= 3
        && !through
        && !mitred
        && prev.id != id
        && next.id != id
        && let Some(cut) = join::wedge_cut(m, id, at_end, (next.id, next.at_end), (prev.id, prev.at_end), n == 3)
    {
        return WallEnd::stepped(cut, at_end);
    }
    let node = if at_end { w.end } else { w.start };
    let limit = arms.iter().map(|a| a.thick).fold(0.0, f64::max);
    // Where a face of this wall crosses a neighbour's facing face, near enough to the corner either way; else its own
    // end. A crossing far along the wall is no corner at all: two walls meeting at a shallow angle cross metres in.
    // The corner is where the two centre lines cross (when that is near the node): walls standing on a face meet
    // there, a thickness or more from their location lines' node, their far faces running on past it.
    let meet = |face: P2, other: P2, near: &Arm| {
        let corner = geom::line_intersection(me.centre, me.centre + me.way, near.centre, near.centre + near.way)
            .filter(|c| c.dist(node) <= limit)
            .unwrap_or(node);
        geom::line_intersection(face, face + me.way, other, other + near.way)
            .filter(|c| (-limit..=limit).contains(&(*c - corner).dot(me.way)))
            .unwrap_or(face)
    };
    let (cw, ccw) = (meet(me.cw, prev.ccw, prev), meet(me.ccw, next.cw, next));
    WallEnd::plain(if through { vec![cw, ccw] } else { vec![cw, node, ccw] })
}

/// The order the walls ending at a node join in, where they are three or four straight walls' ends and none runs on in
/// line with another: the order they were made in (their ids). Each is a wall and which of its ends is there.
fn node_order(m: &Model, arms: &[Arm], through: bool) -> Option<Vec<(Id, bool)>> {
    if through || !(3..=4).contains(&arms.len()) || carried_on(m, arms).is_some() {
        return None;
    }
    let mut order: Vec<(Id, bool)> = arms.iter().map(|a| (a.id, a.at_end)).collect();
    order.sort_unstable();
    let apart = order.windows(2).all(|w| w[0].0 != w[1].0);
    let straight = order.iter().all(|(id, _)| m.wall(*id).is_some_and(|w| w.arc == 0.0));
    if !(apart && straight) {
        return None;
    }
    // The order the Wall Joins tool left, kept wall by wall (each two ends' join says which of them is first): a wall
    // comes before as many of the others as it is first of.
    if !m.wall_joins.is_empty() {
        let before = |x: (Id, bool), y: (Id, bool)| match m.wall_join((m.guid(x.0), x.1), (m.guid(y.0), y.1)).and_then(|j| j.first) {
            Some(first) => first == m.guid(x.0),
            None => x.0 < y.0,
        };
        let firsts: Vec<usize> = order.iter().map(|x| order.iter().filter(|y| *y != x && before(*x, **y)).count()).collect();
        let mut ranked: Vec<(usize, (Id, bool))> = firsts.into_iter().zip(order.iter().copied()).collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        order = ranked.into_iter().map(|(_, end)| end).collect();
    }
    Some(order)
}

/// What the Wall Joins tool set for the join of the wall ends `order`, three or four at a point: its kind (a Butt, or a
/// Mitre: each wall's faces run to where they cross its neighbours', the walls meeting at the point), and whether their
/// layers clean up. Kept on each two ends' join, all alike.
pub fn order_setting(m: &Model, order: &[(Id, bool)]) -> (crate::model::JoinKind, bool) {
    let kept = order
        .iter()
        .enumerate()
        .find_map(|(i, a)| order.iter().skip(i + 1).find_map(|b| m.wall_join((m.guid(a.0), a.1), (m.guid(b.0), b.1))));
    match kept {
        Some(j) => (j.kind, j.display != crate::model::JoinDisplay::DontClean),
        None => (crate::model::JoinKind::Butt, true),
    }
}

/// The order the three or four walls ending where wall `id`'s end (`at_end`, else its start) is join in, where none runs
/// on in line with another: each a wall and which of its ends is there. The first runs through, the second butts it, and
/// each one after butts what is there. None for any other join.
pub fn join_order(m: &Model, id: Id, at_end: bool) -> Option<Vec<(Id, bool)>> {
    let (arms, through) = arms(m, id, at_end, None);
    node_order(m, &arms, through)
}

/// The wall that runs on through a node others end at: the one with two arms there, when it is the only one.
fn running_on(arms: &[Arm]) -> Option<Id> {
    let mut twice = arms.iter().map(|a| a.id).filter(|id| arms.iter().filter(|a| a.id == *id).count() == 2);
    let on = twice.next()?;
    twice.all(|id| id == on).then_some(on)
}

/// Of the ends at a node, the two that are one wall carried on in line, by their places among `arms`: where two such pairs
/// cross, the pair that has the wall made first (the lowest id).
fn carried_on(m: &Model, arms: &[Arm]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize)> = None;
    for (a, x) in arms.iter().enumerate() {
        for (b, y) in arms.iter().enumerate().skip(a + 1) {
            let first = |p: (usize, usize)| arms.get(p.0).map(|q| q.id).min(arms.get(p.1).map(|q| q.id));
            if in_line(m, x, y) && best.is_none_or(|kept| first((a, b)) < first(kept)) {
                best = Some((a, b));
            }
        }
    }
    best
}

/// Whether two walls leaving a node are one wall carried on: straight, running opposite ways. They may be of two types (a
/// wall that changes where a partition meets it): each stands square against the other, their faces where their own
/// types put them.
fn in_line(m: &Model, a: &Arm, b: &Arm) -> bool {
    let (Some(x), Some(y)) = (m.wall(a.id), m.wall(b.id)) else { return false };
    a.id != b.id && a.way.dot(b.way) < -1.0 + 1e-9 && x.arc == 0.0 && y.arc == 0.0
}

/// The walls wall `id`'s end (`at_end`, else its start) is joined to: those ending there with it, and one it ends on.
pub fn joined_at(m: &Model, id: Id, at_end: bool) -> Vec<Id> {
    let mut ids: Vec<Id> = arms(m, id, at_end, None).0.iter().map(|a| a.id).filter(|other| *other != id).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The wall end that wall `id`'s end (`at_end`, else its start) joins, when it joins one other wall end and nothing else:
/// the join the Wall Joins tool changes.
pub fn end_join(m: &Model, id: Id, at_end: bool) -> Option<(Id, bool)> {
    let (arms, through) = arms(m, id, at_end, None);
    let other = arms.iter().find(|a| a.id != id)?;
    (arms.len() == 2 && !through).then_some((other.id, other.at_end))
}

/// Whether the layers of wall `ender`'s end and of wall `through`, on whose side it ends, clean up: not where the Wall
/// Joins tool set that join to Don't Clean Join.
fn tee_cleans(m: &Model, ender: Id, at_end: bool, through: Id) -> bool {
    m.tee_join((m.guid(ender), at_end), m.guid(through)).is_none_or(|j| j.display != crate::model::JoinDisplay::DontClean)
}

/// The wall on whose side wall `id`'s end (`at_end`, else its start) ends, the two of them alone there: a T join, which
/// the Wall Joins tool shows and sets how plans draw.
pub fn ends_on(m: &Model, id: Id, at_end: bool) -> Option<Id> {
    let (arms, through) = arms(m, id, at_end, None);
    if through { running_on(&arms).filter(|on| *on != id) } else { None }
}

/// The wall ends that end on the side of wall `id`: each a wall and which of its ends (its end, else its start).
pub fn ending_on(m: &Model, id: Id) -> Vec<(Id, bool)> {
    tees(m, id, None).into_iter().filter(|(other, at_end)| ends_on(m, *other, *at_end) == Some(id)).collect()
}

/// The walls, straight or curved, that end on the side of the straight wall `id`, each with which of its ends does (its end, else its
/// start): those whose join there is theirs and this wall's alone.
fn tees(m: &Model, id: Id, index: Option<&Index>) -> Vec<(Id, bool)> {
    let Some(w) = m.wall(id).filter(|w| w.join) else { return Vec::new() };
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let thickest = m.types.wall.values().map(|t| t.thickness).filter(|t| t.is_finite()).fold(0.0, f64::max);
    let reach = [w.start.0.min(w.end.0), w.start.1.min(w.end.1), w.start.0.max(w.end.0), w.start.1.max(w.end.1)];
    let near = m.lookup().walls_near_at(&[(reach, m.base_z(w), m.top_z(w))], thickest + 1.0);
    let mut out = Vec::new();
    for other in near.into_iter().filter(|o| *o != id) {
        let Some(o) = m.wall(other) else { continue };
        for at_end in [false, true].into_iter().filter(|e| o.joins(*e)) {
            let node = if at_end { o.end } else { o.start };
            let (s, across) = axis.project(node);
            if !(int - TOL..=ext + TOL).contains(&across) || axis.distance(node, across) > TOL {
                continue;
            }
            let (arms, through) = arms(m, other, at_end, index);
            let on_its_side = s > TOL && s < axis.length() - TOL;
            // On its side; or at its end, where it carries on as another wall in line with it.
            let ends_on = if on_its_side {
                through && running_on(&arms) == Some(id)
            } else {
                let pair = carried_on(m, &arms).and_then(|(a, b)| Some((arms.get(a)?.id, arms.get(b)?.id)));
                !through && pair.is_some_and(|(a, b)| (a == id || b == id) && a != other && b != other)
            };
            if ends_on {
                out.push((other, at_end));
            }
        }
    }
    out
}

/// Wall `id`'s centre line, an arc as chords: each end it joins others at run on (or back) to the
/// nearest of their centre lines, so rooms close at the centre lines whatever line walls stand on.
pub fn centre_line(m: &Model, id: Id) -> Vec<P2> {
    centre_line_in(m, id, None)
}

/// [`centre_line`], its joins found through `index` when there is one.
#[doc(hidden)]
pub fn centre_line_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let mut line = axis.points(0.0, axis.length(), centre(m, w));
    for at_end in [false, true] {
        let (arms, _) = arms(m, id, at_end, index);
        let Some(me) = arms.iter().find(|a| a.id == id) else { continue };
        let reach = 2.0 * arms.iter().map(|a| a.thick).fold(1.0, f64::max);
        let crossing = arms
            .iter()
            .filter(|a| a.id != id)
            .filter_map(|a| geom::line_intersection(me.centre, me.centre + me.way, a.centre, a.centre + a.way))
            .min_by(|a, b| a.dist(me.centre).total_cmp(&b.dist(me.centre)));
        let end = if at_end { line.last_mut() } else { line.first_mut() };
        if let (Some(end), Some(c)) = (end, crossing.filter(|c| c.dist(me.centre) <= reach)) {
            *end = c;
        }
    }
    line
}

/// Wall `id`'s outline in plan: its start cap, its exterior face, its end cap and its interior face.
pub fn outline(m: &Model, id: Id) -> Vec<P2> {
    outline_in(m, id, None)
}

#[doc(hidden)]
pub fn outline_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<P2> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let len = axis.length();
    let between = |pts: Vec<P2>| {
        let n = pts.len();
        pts.into_iter().skip(1).take(n.saturating_sub(2))
    };
    let mut ring = cap_in(m, id, false, index);
    ring.extend(between(axis.points(0.0, len, ext)));
    ring.extend(cap_in(m, id, true, index));
    ring.extend(between(axis.points(len, 0.0, int)));
    ring.dedup_by(|a, b| a.dist(*b) < 1e-6);
    ring
}

/// Where the line `off` mm to the left of wall `w`'s location line meets its cap at its start (or
/// its end): along the line's tangent there, onto the cap's corners.
fn on_cap(axis: &Axis, cap: &[P2], at_end: bool, off: f64) -> P2 {
    let s = if at_end { axis.length() } else { 0.0 };
    let (p, t) = (axis.at(s, off), axis.tangent(s));
    cap.windows(2)
        .filter_map(|seg| match seg {
            [a, b] => geom::line_intersection(p, p + t, *a, *b).filter(|q| geom::dist_to_segment(*q, *a, *b) <= 1e-6),
            _ => None,
        })
        .min_by(|a, b| a.dist(p).total_cmp(&b.dist(p)))
        .unwrap_or(p)
}

/// The lines of wall `id`'s faces and of the boundaries between its layers, exterior first, each
/// from its start cap to its end cap: between two layers that end apart, to where the shorter ends.
pub fn layer_lines(m: &Model, id: Id) -> Vec<Vec<P2>> {
    layer_lines_in(m, id, None)
}

#[doc(hidden)]
pub fn layer_lines_in(m: &Model, id: Id, index: Option<&Index>) -> Vec<Vec<P2>> {
    layer_geometry(m, id, index).lines
}

/// A wall's layers in plan: each layer's pieces (one, or more where a wall ending on its side passes through the layer),
/// the lines of its faces and between its layers, and the lines the plan draws between layers: where two of its own lie
/// side by side, and where one ends on or runs beside a layer of another wall of another material.
#[derive(Default)]
#[doc(hidden)]
pub struct LayerGeometry {
    pub lines: Vec<Vec<P2>>,
    bands: Vec<Vec<Vec<P2>>>,
    seams: Vec<Vec<P2>>,
    butts: Vec<Vec<P2>>,
}

impl LayerGeometry {
    /// Layer `k`'s pieces, each a ring.
    #[doc(hidden)]
    pub fn bands(&self, k: usize) -> &[Vec<P2>] {
        self.bands.get(k).map_or(&[], Vec::as_slice)
    }

    #[doc(hidden)]
    pub fn interfaces(&self) -> impl Iterator<Item = &[P2]> {
        self.seams.iter().map(Vec::as_slice)
    }

    /// The lines its ends butt on where a join is set not to clean up (Revit's Don't Clean Join): drawn at any detail, of a
    /// wall with layers or without.
    #[doc(hidden)]
    pub fn butts(&self) -> impl Iterator<Item = &[P2]> {
        self.butts.iter().map(Vec::as_slice)
    }
}

#[doc(hidden)]
pub fn layer_geometry(m: &Model, id: Id, index: Option<&Index>) -> LayerGeometry {
    let Some(w) = m.wall(id) else { return LayerGeometry::default() };
    let axis = Axis::of(w);
    let len = axis.length();
    let offs = boundaries(m, w);
    let layers = offs.len().saturating_sub(1);
    let ends = [end_geometry(m, id, false, index), end_geometry(m, id, true, index)];
    // Where boundary `b` of layer `k` ends, at the wall's start (0) or its end (1).
    let end_of = |b: usize, k: usize, at: usize| -> P2 {
        let off = offs.get(b).copied().unwrap_or(0.0);
        let cut = ends.get(at).and_then(|e| e.cut.as_ref()).and_then(|c| c.layers.get(k)).and_then(|l| l.get(b - k)).copied();
        cut.unwrap_or_else(|| ends.get(at).map_or(axis.at(0.0, off), |e| on_cap(&axis, &e.outline, at == 1, off)))
    };
    // Boundary `b` as layer `k` has it: along the wall, from the layer's start to its end.
    // How far along a curved wall `q` lies, measured round its centre from its start, or from its end (`from_end`): less
    // than nothing before its start, more than its length past its end.
    let turned = |q: P2, from_end: bool| -> f64 {
        let Some((c, r, a0, sweep)) = axis.arc() else { return axis.project(q).0 };
        let base = if from_end { a0 + sweep } else { a0 };
        let d = (sweep.signum() * ((q.1 - c.1).atan2(q.0 - c.0) - base) + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        r * d + if from_end { len } else { 0.0 }
    };
    let side = |b: usize, k: usize| -> Vec<P2> {
        // A curved wall's runs from where its end is cut to where its other is: no station of it past either.
        let (from, to) = if w.arc == 0.0 {
            (0.0, len)
        } else {
            // Past its own ends too, as far as a join carries it on: a thickness or two.
            let past = 4.0 * (offs.first().copied().unwrap_or(0.0) - offs.last().copied().unwrap_or(0.0)).abs();
            (turned(end_of(b, k, 0), false).clamp(-past, len), turned(end_of(b, k, 1), true).clamp(0.0, len + past))
        };
        if to <= from {
            return vec![end_of(b, k, 0), end_of(b, k, 1)];
        }
        let off = offs.get(b).copied().unwrap_or(0.0);
        let mut line = if w.arc == 0.0 {
            axis.points(from, to, off)
        } else {
            // The same places along the curve whichever layer's side this is, and where the layer on its other side is cut:
            // two layers' sides along one boundary are one line, with no sliver between them.
            let other = if b == k { k.checked_sub(1) } else { (k + 1 < layers).then_some(k + 1) };
            let mut at: Vec<f64> = axis.stations(0.0, len).into_iter().filter(|s| *s > from + 1e-6 && *s < to - 1e-6).collect();
            // Past its ends it curves on: every 25 mm from the end, the same for every layer.
            let steps = |far: f64| (1..).map(|n| f64::from(n) * 25.0).take_while(move |d| *d < far);
            at.extend(steps(-from).map(|d| -d).chain(steps(to - len).map(|d| len + d)).filter(|s| *s > from + 1e-6 && *s < to - 1e-6));
            if let Some(o) = other {
                at.extend([0, 1].map(|i| turned(end_of(b, o, i), i == 1)).into_iter().filter(|s| *s > from + 1e-6 && *s < to - 1e-6));
            }
            at.sort_by(f64::total_cmp);
            let mut line = vec![axis.at(from, off)];
            line.extend(at.into_iter().map(|s| axis.at(s, off)));
            line.push(axis.at(to, off));
            line
        };
        if let Some(first) = line.first_mut() {
            *first = end_of(b, k, 0);
        }
        if let Some(last) = line.last_mut() {
            *last = end_of(b, k, 1);
        }
        line
    };
    // How far along the wall a point is: of a straight wall, past its ends too.
    let u = (w.end - w.start).unit();
    let along = |q: P2| if w.arc == 0.0 { (q - w.start).dot(u) } else { axis.project(q).0 };
    // Each layer's gaps, where walls ending on this one's side pass through it.
    let broken: Vec<Vec<Option<join::Gap>>> = tees(m, id, index)
        .into_iter()
        .filter(|(other, at_end)| tee_cleans(m, *other, *at_end, id))
        .filter_map(|(other, at_end)| join::gaps(m, id, other, at_end))
        .collect();
    let gaps_of = |k: usize| -> Vec<join::Gap> { broken.iter().filter_map(|g| g.get(k).copied().flatten()).collect() };
    // A line along the wall (two points of a straight wall's, the points of a curved one's) less the gaps: the stretches
    // left, in order.
    let left = |line: &[P2], gaps: &[join::Gap]| -> Vec<Vec<P2>> {
        let mut far = vec![0.0];
        for s in line.windows(2) {
            far.push(far.last().copied().unwrap_or(0.0) + s[0].dist(s[1]));
        }
        let total = far.last().copied().unwrap_or(0.0);
        if line.len() < 2 || total <= 0.0 {
            return Vec::new();
        }
        let last = line.len() - 2;
        // How far along it a line across it crosses it: where it does nearest that line's own point, the first and the
        // last stretch carried on past the ends.
        let at = |cross: (P2, P2)| -> Option<f64> {
            line.windows(2)
                .enumerate()
                .filter_map(|(i, s)| {
                    let (a, d) = (s[0], s[1] - s[0]);
                    let q = geom::line_intersection(a, s[1], cross.0, cross.0 + cross.1)?;
                    let t = (q - a).dot(d) / d.dot(d).max(1e-12);
                    ((t >= -1e-9 || i == 0) && (t <= 1.0 + 1e-9 || i == last))
                        .then(|| (q.dist(cross.0), far.get(i).copied().unwrap_or(0.0) + t * d.length()))
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, s)| s)
        };
        // The stretch from `s0` to `s1` along it.
        let point = |s: f64| -> P2 {
            let i = far.iter().rposition(|f| *f <= s).unwrap_or(0).min(last);
            let (a, b) = (line.get(i).copied().unwrap_or_default(), line.get(i + 1).copied().unwrap_or_default());
            let span = (far.get(i + 1).copied().unwrap_or(0.0) - far.get(i).copied().unwrap_or(0.0)).max(1e-12);
            a + (b - a) * ((s - far.get(i).copied().unwrap_or(0.0)) / span)
        };
        let stretch = |s0: f64, s1: f64| -> Vec<P2> {
            let mut out = vec![point(s0)];
            out.extend(line.iter().zip(&far).filter(|(_, f)| **f > s0 + 1e-9 && **f < s1 - 1e-9).map(|(q, _)| *q));
            out.push(point(s1));
            out
        };
        let mut cuts: Vec<(f64, f64)> =
            gaps.iter().filter_map(|g| Some((at(g.from)?, at(g.to)?))).map(|(x, y)| (x.min(y), x.max(y))).collect();
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let (mut out, mut from) = (Vec::new(), 0.0);
        for (lo, hi) in cuts {
            if lo > from {
                out.push(stretch(from, lo.min(total)));
            }
            from = from.max(hi);
        }
        if from < total {
            out.push(stretch(from, total));
        }
        out
    };
    // Where layer `k`'s end bends, at the wall's start (0) or its end (1), from its exterior side to its interior one.
    let bends = |k: usize, at: usize| -> &[P2] {
        ends.get(at).and_then(|e| e.cut.as_ref()).and_then(|c| c.bends.get(k)).map_or(&[], Vec::as_slice)
    };
    // The pieces of a layer between its two sides, each from the wall's start to its end, less `gaps`: each a ring. With
    // `whole`, the sides run to the layer's ends, which bend where its end does.
    let rings_of = |outer: &[P2], inner: &[P2], gaps: &[join::Gap], k: usize, whole: [bool; 2]| -> Vec<Vec<P2>> {
        let sides: Vec<(Vec<P2>, Vec<P2>)> = if gaps.is_empty() {
            vec![(outer.to_vec(), inner.to_vec())]
        } else {
            left(outer, gaps).into_iter().zip(left(inner, gaps)).collect()
        };
        let last = sides.len().saturating_sub(1);
        sides
            .into_iter()
            .enumerate()
            .map(|(i, (a, b))| {
                let mut ring = a;
                if i == last && whole[1] {
                    ring.extend(bends(k, 1));
                }
                ring.extend(b.into_iter().rev());
                if i == 0 && whole[0] {
                    ring.extend(bends(k, 0).iter().rev());
                }
                ring
            })
            .collect()
    };
    let mut bands = Vec::with_capacity(layers);
    for k in 0..layers {
        bands.push(rings_of(&side(k, k), &side(k + 1, k), &gaps_of(k), k, [true, true]));
    }
    // Each boundary's line: a face's is its layer's; between two layers, to where the shorter of them ends.
    let mut lines = Vec::with_capacity(offs.len());
    let mut seams: Vec<Vec<P2>> = Vec::new();
    for b in 0..offs.len() {
        let (before, after) = (b.checked_sub(1), (b < layers).then_some(b));
        let line = match (before, after) {
            (Some(x), Some(y)) => {
                let (p, q) = (side(b, x), side(b, y));
                let mut line = p.clone();
                if let (Some(first), Some(a), Some(c)) = (line.first_mut(), p.first(), q.first()) {
                    *first = if along(*a) >= along(*c) { *a } else { *c };
                }
                if let (Some(last), Some(a), Some(c)) = (line.last_mut(), p.last(), q.last()) {
                    *last = if along(*a) <= along(*c) { *a } else { *c };
                }
                // The line between the two, where both are there.
                let gaps: Vec<join::Gap> = gaps_of(x).into_iter().chain(gaps_of(y)).collect();
                if gaps.is_empty() {
                    seams.push(line.clone());
                } else {
                    seams.extend(left(&line, &gaps));
                }
                line
            }
            (None, Some(k)) | (Some(k), None) => side(b, k),
            (None, None) => Vec::new(),
        };
        lines.push(line);
    }
    seams.extend(ends.iter().filter_map(|e| e.cut.as_ref()).flat_map(|c| c.seams.iter().map(|s| s.to_vec())));
    // A line of no length (a layer as thin as nothing, two layers ending together) and a piece of no area are nothing to draw.
    seams.retain(|l| l.windows(2).any(|w| matches!(w, [a, b] if a.dist(*b) > 1e-6)));
    for rings in &mut bands {
        rings.retain(|r| geom::area(r).abs() > 1e-6);
    }
    let butts = ends.iter().filter_map(|e| e.cut.as_ref()).filter_map(|c| c.butt).map(|b| b.to_vec()).collect();
    // Where its type wraps its layers, at its free ends and at the jambs of its doors and windows: the layers' pieces and
    // the lines between them are the wrapped ones ([`wrap::shape`]), its joined ends staying as their joins cut them.
    let wraps = m.types.wall.get(&w.type_name).filter(|t| {
        use crate::model::Wrapping;
        w.arc == 0.0
            && t.layers.len() == layers
            && t.layers.iter().any(|l| l.wraps)
            && (t.wrap_ends != Wrapping::None || t.wrap_inserts != Wrapping::None)
    });
    if let (Some(t), Some(&face)) = (wraps, offs.first()) {
        use crate::model::Wrapping;
        let guid = m.guid(id);
        let openings: Vec<(f64, f64)> = m
            .lookup()
            .referrers(&guid)
            .filter_map(|k| m.elements.get(k))
            .filter_map(|e| match e {
                Element::Door(o) | Element::Window(o) if o.host == guid => {
                    let (width, _) = m.opening_type(e)?.cut();
                    Some((o.offset - width / 2.0, o.offset + width / 2.0))
                }
                _ => None,
            })
            .collect();
        let at = |i: usize| if ends.get(i).is_some_and(|e| e.free) { t.wrap_ends } else { Wrapping::None };
        let bodies = wrap::bodies(len, &openings, [at(0), at(1)], t.wrap_inserts);
        let (pieces, lines_between) = wrap::shape(t, &bodies);
        let deep: Vec<f64> = offs.iter().map(|o| face - o).collect();
        // A point `s` along the wall and `d` deep; at an end a join has cut, on that cut where layer `k` ends.
        let point = |s: f64, d: f64, k: usize| -> P2 {
            let joined = |i: usize| ends.get(i).is_some_and(|e| !e.free);
            let end = if s <= 1e-6 && joined(0) {
                Some(0)
            } else if s >= len - 1e-6 && joined(1) {
                Some(1)
            } else {
                None
            };
            match (end, deep.get(k), deep.get(k + 1)) {
                (Some(i), Some(&d0), Some(&d1)) if d1 > d0 => {
                    let (a, b) = (end_of(k, k, i), end_of(k + 1, k, i));
                    a + (b - a) * ((d - d0) / (d1 - d0))
                }
                _ => w.start + u * s + u.perp() * (face - d),
            }
        };
        bands = pieces
            .iter()
            .enumerate()
            .map(|(k, list)| {
                let gaps = gaps_of(k);
                list.iter()
                    .flat_map(|&[s0, s1, d0, d1]| {
                        let (a0, a1, b0, b1) = (point(s0, d0, k), point(s1, d0, k), point(s0, d1, k), point(s1, d1, k));
                        // A piece as deep as its layer, at an end of the wall: the layer's end there, bent where it is.
                        let deep = |d: f64, b: usize| deep.get(b).is_some_and(|x| (x - d).abs() < 1e-6);
                        let full = deep(d0, k) && deep(d1, k + 1);
                        rings_of(&[a0, a1], &[b0, b1], &gaps, k, [full && s0 <= 1e-6, full && s1 >= len - 1e-6])
                    })
                    .collect()
            })
            .collect();
        // The lines between layers: a layer's boundary by the layer under it, so a joined end's is where that one is cut
        // (the shorter of the two is found as before, from `lines`); a return's, where it stands.
        let boundary = |d: f64| deep.iter().position(|x| (x - d).abs() < 1e-6).filter(|b| *b > 0 && *b < layers);
        let mut wrapped: Vec<Vec<P2>> = Vec::new();
        for [[s0, d0], [s1, d1]] in lines_between {
            let along_wall = (d0 - d1).abs() < 1e-9;
            let line = match boundary(d0).filter(|_| along_wall) {
                Some(b) => {
                    let whole = lines.get(b).filter(|l| l.len() == 2);
                    let end = |s: f64, i: usize| {
                        let cut = (i == 0 && s <= 1e-6 || i == 1 && s >= len - 1e-6) && ends.get(i).is_some_and(|e| !e.free);
                        whole.filter(|_| cut).and_then(|l| l.get(i).copied()).unwrap_or_else(|| point(s, d0, b))
                    };
                    let (a, c) = (end(s0, 0), end(s1, 1));
                    let both: Vec<join::Gap> = gaps_of(b - 1).into_iter().chain(gaps_of(b)).collect();
                    if both.is_empty() { vec![vec![a, c]] } else { left(&[a, c], &both) }
                }
                None => vec![vec![w.start + u * s0 + u.perp() * (face - d0), w.start + u * s1 + u.perp() * (face - d1)]],
            };
            wrapped.extend(line);
        }
        wrapped.extend(ends.iter().filter_map(|e| e.cut.as_ref()).flat_map(|c| c.seams.iter().map(|s| s.to_vec())));
        seams = wrapped;
        seams.retain(|l| l.windows(2).any(|w| matches!(w, [a, b] if a.dist(*b) > 1e-6)));
        for rings in &mut bands {
            rings.retain(|r| geom::area(r).abs() > 1e-6);
        }
    }
    // Where it joins walls made after it at a point of three or four ends, their layers run through some of its own by
    // rank ([`join::ordered`]): it gives those up, and the lines between its layers there with them.
    for at_end in [false, true] {
        let (arms, through) = arms(m, id, at_end, index);
        let Some(order) = node_order(m, &arms, through) else { continue };
        let Some(place) = order.iter().position(|(other, end)| *other == id && *end == at_end) else { continue };
        let (kind, clean) = order_setting(m, &order);
        if kind == crate::model::JoinKind::Mitre {
            continue;
        }
        let Some(joined) = join::ordered(m, &order, clean) else { continue };
        let holes: Vec<(usize, &Vec<P2>)> =
            joined.taken.iter().filter(|(wall, ..)| *wall == place).map(|(_, k, hole)| (*k, hole)).collect();
        for (k, hole) in &holes {
            if let Some(rings) = bands.get_mut(*k) {
                *rings = minus(rings, hole);
            }
        }
        // A line of its own that runs along the side of what it gave up is gone there.
        let sides: Vec<(P2, P2)> = holes.iter().flat_map(|(_, hole)| geom::ring_edges(hole)).collect();
        seams = seams
            .into_iter()
            .flat_map(|line| {
                let segments: Vec<[P2; 2]> = line.windows(2).filter_map(|w| Some([*w.first()?, *w.get(1)?])).collect();
                segments.into_iter().flat_map(|s| uncovered(s, &sides)).map(|s| s.to_vec()).collect::<Vec<_>>()
            })
            .collect();
    }
    LayerGeometry { lines, bands, seams, butts }
}

/// What is left of the line `s` where none of `sides` lies along it.
fn uncovered(s: [P2; 2], sides: &[(P2, P2)]) -> Vec<[P2; 2]> {
    let (a, d) = (s[0], s[1] - s[0]);
    let len = d.length();
    if len <= 1e-9 {
        return Vec::new();
    }
    let u = d * (1.0 / len);
    let on = |q: P2| (q - a).cross(u).abs() < 1e-4;
    let mut covers: Vec<(f64, f64)> = sides
        .iter()
        .filter(|(p, q)| on(*p) && on(*q))
        .map(|(p, q)| ((*p - a).dot(u), (*q - a).dot(u)))
        .map(|(x, y)| (x.min(y), x.max(y)))
        .collect();
    covers.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (mut out, mut from) = (Vec::new(), 0.0);
    for (lo, hi) in covers {
        if lo > from + 1e-6 {
            out.push([a + u * from, a + u * lo.min(len)]);
        }
        from = from.max(hi);
    }
    if from < len - 1e-6 {
        out.push([a + u * from, s[1]]);
    }
    out
}

/// A plane over the plan: its height at `q` is `z + g.0 · q.0 + g.1 · q.1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub z: f64,
    pub g: P2,
}

impl Plane {
    pub fn flat(z: f64) -> Plane {
        Plane { z, g: P2(0.0, 0.0) }
    }

    pub fn at(&self, q: P2) -> f64 {
        self.z + self.g.dot(q)
    }

    /// `ring` lifted onto it: points `[x, y, z]`.
    pub fn lift(&self, ring: &[P2]) -> Vec<[f64; 3]> {
        ring.iter().map(|q| [q.0, q.1, self.at(*q)]).collect()
    }

    /// The plane a flat polygon in space lies in, by Newell's normal; none for an upright one.
    pub fn of(face: &[[f64; 3]]) -> Option<Plane> {
        let n = face.iter().zip(face.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| {
            [n[0] + (a[1] - b[1]) * (a[2] + b[2]), n[1] + (a[2] - b[2]) * (a[0] + b[0]), n[2] + (a[0] - b[0]) * (a[1] + b[1])]
        });
        let o = face.first()?;
        (n[2].abs() > 1e-9).then(|| {
            let g = P2(-n[0] / n[2], -n[1] / n[2]);
            Plane { z: o[2] - g.0 * o[0] - g.1 * o[1], g }
        })
    }
}

/// What a wall's base stands on, or its top keeps under: a level plane at `own`, or, over the roof
/// or floor it's attached to, that one's plane there (each of a roof's slopes over its own part of
/// the plan, in convex pieces), and outside it still `own`.
#[derive(Clone)]
struct Bound {
    regions: Vec<(Vec<P2>, Plane)>,
    own: f64,
}

impl Bound {
    /// The roof or floor `id` from below (with `top`, from above), or `own` without one.
    fn of(m: &Model, id: Option<Guid>, top: bool, own: f64) -> Bound {
        let regions = match id.and_then(|id| m.elements.get(id)) {
            // From below its underside (a shaped one's level under a variable layer), from above its top.
            Some(Element::Roof(r)) => m
                .roof_prisms(r)
                .into_iter()
                .flat_map(|(ring, bottom, up)| {
                    let plane = if top { up } else { bottom };
                    convex_pieces(&ring).into_iter().map(move |c| (c, plane))
                })
                .collect(),
            // A shaped one's underside under a variable layer: level.
            Some(Element::Floor(f)) if !top && m.floor_under(f).is_some() => {
                let under = m.floor_under(f).map_or(Plane::flat(0.0), |(p, _)| p);
                convex_pieces(&f.boundary).into_iter().map(|c| (c, under)).collect()
            }
            // A shaped one's triangles, each its own plane.
            Some(Element::Floor(f)) if !f.shape.is_empty() => {
                let depth = if top { 0.0 } else { m.floor_depth(f) };
                m.shape_triangles(f).into_iter().map(|(t, p)| (t, Plane { z: p.z - depth, ..p })).collect()
            }
            Some(Element::Floor(f)) => {
                let face = m.floor_top(f);
                let plane = Plane { z: face.z - if top { 0.0 } else { m.floor_depth(f) }, ..face };
                convex_pieces(&f.boundary).into_iter().map(|c| (c, plane)).collect()
            }
            _ => Vec::new(),
        };
        Bound { regions, own }
    }

    /// The plane over `q`: of the part over it, or within a millimetre of it; of two, the lower.
    fn over(&self, q: P2) -> Option<Plane> {
        let near = |p: &Vec<P2>| geom::contains(p, q) || geom::ring_edges(p).any(|(a, b)| geom::dist_to_segment(q, a, b) <= TOL);
        self.regions.iter().filter(|(r, _)| near(r)).map(|(_, p)| *p).min_by(|a, b| a.at(q).total_cmp(&b.at(q)))
    }

    /// Its height over `q`.
    fn at(&self, q: P2) -> f64 {
        self.over(q).map_or(self.own, |p| p.at(q))
    }

    /// `ring` cut where a different plane holds, each part with its plane.
    fn parts(&self, ring: Vec<P2>) -> Vec<(Vec<P2>, Plane)> {
        let mut out = Vec::new();
        let mut rest = vec![ring];
        for (piece, plane) in &self.regions {
            let mut outside = Vec::new();
            for r in rest {
                let (inn, out_) = split(r, piece);
                out.extend(inn.map(|i| (i, *plane)));
                outside.extend(out_);
            }
            rest = outside;
        }
        out.extend(rest.into_iter().map(|r| (r, Plane::flat(self.own))));
        out
    }
}

/// A polygon as convex pieces, each counter-clockwise: itself when it's convex, else its triangles.
#[doc(hidden)]
pub fn convex_pieces(ring: &[P2]) -> Vec<Vec<P2>> {
    if ring.len() < 3 {
        return Vec::new();
    }
    let ccw: Vec<P2> = if geom::area(ring) < 0.0 { ring.iter().rev().copied().collect() } else { ring.to_vec() };
    let n = ccw.len();
    let convex = (0..n).all(|i| (ccw[(i + 1) % n] - ccw[i]).cross(ccw[(i + 2) % n] - ccw[(i + 1) % n]) >= -1e-6);
    if convex {
        return vec![ccw];
    }
    geom::triangulate(&ccw).into_iter().map(|t| t.iter().filter_map(|i| ccw.get(*i).copied()).collect()).collect()
}

#[cfg(debug_assertions)]
thread_local! {
    /// The polygons cut by [`split`] on this thread: the tests of what is drawn assert on it, not on time, which varies with the
    /// load of the machine.
    #[doc(hidden)]
    pub static SPLITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `ring` cut by the convex counter-clockwise `piece`: the part inside it (if any), and the parts
/// outside, apart. A repeated point of the piece's is no edge to cut by.
#[doc(hidden)]
pub fn split(ring: Vec<P2>, piece: &[P2]) -> (Option<Vec<P2>>, Vec<Vec<P2>>) {
    #[cfg(debug_assertions)]
    SPLITS.with(|n| n.set(n.get() + 1));
    let big = |r: &Vec<P2>| r.len() >= 3 && geom::area(r).abs() > 1.0;
    let mut inside = ring;
    let mut outside = Vec::new();
    for (a, b) in geom::ring_edges(piece).filter(|(a, b)| a.dist(*b) > 1e-6) {
        let left = |q: P2| (b - a).cross(q - a);
        let out = geom::clip(&inside, |q| -left(q));
        if big(&out) {
            outside.push(out);
        }
        inside = geom::clip(&inside, left);
    }
    (Some(inside).filter(big), outside)
}

/// What's left of `rings` outside `hole`: convex pieces, counter-clockwise.
#[doc(hidden)]
pub fn minus(rings: &[Vec<P2>], hole: &[P2]) -> Vec<Vec<P2>> {
    // The box round a ring: a piece clear of the hole's piece's box is left whole, without cutting it (a big concave ring and
    // a big concave hole make hundreds of pieces each, and cutting each by each is cubic).
    let extent = |ring: &[P2]| {
        ring.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        })
    };
    let holes: Vec<_> = convex_pieces(hole).into_iter().map(|p| (extent(&p), p)).collect();
    rings
        .iter()
        .flat_map(|r| {
            holes.iter().fold(convex_pieces(r), |parts, ((lo, hi), piece)| {
                parts
                    .into_iter()
                    .flat_map(|p| {
                        let (a, b) = extent(&p);
                        if b.0 < lo.0 || a.0 > hi.0 || b.1 < lo.1 || a.1 > hi.1 {
                            Some(p).filter(|p| p.len() >= 3 && geom::area(p).abs() > 1.0).into_iter().collect::<Vec<_>>()
                        } else {
                            split(p, piece).1
                        }
                    })
                    .collect()
            })
        })
        .collect()
}

/// The underside of roof or floor `id` over `q`, if it's over `q`.
pub fn underside(m: &Model, id: Id, q: P2) -> Option<f64> {
    Bound::of(m, Some(m.guid(id)), false, f64::NAN).over(q).map(|p| p.at(q))
}

/// A wall's base and top: what it's attached to, or a level plane.
fn bounds(m: &Model, w: &Wall) -> (Bound, Bound) {
    (Bound::of(m, w.attach_base, true, m.own_base(w)), Bound::of(m, w.attach_top, false, m.own_top(w)))
}

/// A wall's top over the point `s` mm along its centre line.
pub fn top_at(m: &Model, w: &Wall, s: f64) -> f64 {
    bounds(m, w).1.at(Axis::of(w).at(s, centre(m, w)))
}

/// A wall's lowest base and highest top along its faces and its centre line, where what it's
/// attached to bends (sampled: a ridge along it may fall between samples).
pub fn heights(m: &Model, w: &Wall) -> (f64, f64) {
    if w.attach_base.is_none() && w.attach_top.is_none() {
        return (m.own_base(w), m.own_top(w));
    }
    let (base, top) = bounds(m, w);
    let (axis, (ext, int)) = (Axis::of(w), faces(m, w));
    let len = axis.length();
    let pts: Vec<P2> =
        [ext, (ext + int) / 2.0, int].iter().flat_map(|off| (0..=32).map(move |i| axis.at(len * f64::from(i) / 32.0, *off))).collect();
    let lo = pts.iter().map(|q| base.at(*q)).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|q| top.at(*q)).fold(f64::NEG_INFINITY, f64::max);
    (lo, hi)
}

/// The sides of the notches wall `id`'s doors and windows cut where they rise past its top, as Revit lets them: a line across
/// the wall from face to face at each jamb whose head reaches the top there, which a plan seeing the wall from above draws.
pub fn notches(m: &Model, id: Id) -> Vec<[P2; 2]> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    // A curtain wall has no solid to notch: a door in it fills its cell, to the wall's top where the cell reaches it.
    if crate::curtain::is_curtain(m, w) {
        return Vec::new();
    }
    let guid = m.guid(id);
    let (axis, (ext, int), base) = (Axis::of(w), faces(m, w), m.own_base(w));
    let mut out = Vec::new();
    for e in m.lookup().referrers(&guid).filter_map(|k| m.elements.get(k)) {
        let (Element::Door(o) | Element::Window(o)) = e else { continue };
        let Some(t) = m.opening_type(e).filter(|_| o.host == guid) else { continue };
        let (width, height) = t.cut();
        let head = base + o.sill + height;
        for s in [o.offset - width / 2.0, o.offset + width / 2.0] {
            if head >= top_at(m, w, s) - 1e-6 {
                out.push([axis.at(s, int), axis.at(s, ext)]);
            }
        }
    }
    out
}

/// `ring` cut by where the flat plane `flat` is above (`higher`) or below the planes of `bound` over it, each part with the
/// plane that holds there: the flat one or `bound`'s. An opening's edge held to the wall's own base or top.
fn within(ring: Vec<P2>, bound: &Bound, flat: Plane, higher: bool) -> Vec<(Vec<P2>, Plane)> {
    let mut out = Vec::new();
    for (piece, other) in bound.parts(ring) {
        let d = |q: P2| if higher { flat.at(q) - other.at(q) } else { other.at(q) - flat.at(q) };
        for (part, plane) in [(geom::clip(&piece, d), flat), (geom::clip(&piece, |q| -d(q)), other)] {
            if part.len() >= 3 && geom::area(&part).abs() > 1e-6 {
                out.push((part, plane));
            }
        }
    }
    out
}

/// A part of a wall's solid: a ring in plan, from a bottom plane up to a top one; of a curved wall,
/// with its centre and its centre line's radius.
#[derive(Debug, PartialEq)]
pub struct Prism {
    pub ring: Vec<P2>,
    pub bottom: Plane,
    pub top: Plane,
    pub curve: Option<(P2, f64)>,
}

/// What cuts wall `id` besides its doors and windows: its wall openings (Revit's Wall Opening) and the curtain walls
/// embedded in it (Automatically Embed), each as the stretch along its location line (mm from its start) and the
/// heights it cuts between.
pub fn other_cuts(m: &Model, id: Id) -> Vec<((f64, f64), (f64, f64))> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let guid = m.guid(id);
    let mut out: Vec<((f64, f64), (f64, f64))> = m
        .lookup()
        .referrers(&guid)
        .filter_map(|k| match m.elements.get(k) {
            Some(Element::WallOpening(o)) if o.host == guid => Some(o.cut(m, w)),
            _ => None,
        })
        .collect();
    out.extend(crate::curtain::embedded_cuts(m, id, None));
    out
}

/// `gaps` (an opening's lower and upper edge, each) as the stretches they cover together: sorted, and joined where they touch or
/// overlap, so that one inside another (a window in a door's span) leaves the door's and nothing between.
fn merged(mut gaps: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (g0, g1) in gaps {
        match out.last_mut() {
            Some(last) if g0 <= last.1 => last.1 = last.1.max(g1),
            _ => out.push((g0, g1)),
        }
    }
    out
}

/// Wall `id` as prisms: its outline cut across where its doors and windows start and end (with
/// `openings`) and along an arc into short straight pieces, each piece from its base up to its
/// top less the openings over it, cut where what it's attached to changes plane.
pub fn prisms(m: &Model, id: Id, openings: bool) -> Vec<Prism> {
    prisms_in(m, id, openings, None)
}

#[doc(hidden)]
pub fn prisms_in(m: &Model, id: Id, openings: bool, index: Option<&Index>) -> Vec<Prism> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    // A curtain wall has no solid of its own: its panels and mullions are elements, each with its own.
    if crate::curtain::is_curtain(m, w) {
        return Vec::new();
    }
    let guid = m.guid(id);
    let axis = Axis::of(w);
    let (ext, int) = faces(m, w);
    let len = axis.length();
    let z0 = m.own_base(w);
    // The openings: along the wall, and up it.
    let candidates: Box<dyn Iterator<Item = &Element> + '_> = if !openings {
        Box::new(std::iter::empty())
    } else {
        match index {
            Some(index) => Box::new(index.openings(guid).iter().filter_map(|id| m.elements.get(*id))),
            None => Box::new(m.lookup().referrers(&guid).filter_map(|id| m.elements.get(id)).collect::<Vec<_>>().into_iter()),
        }
    };
    let holes: Vec<((f64, f64), (f64, f64))> = candidates
        .filter_map(|e| match e {
            Element::Door(o) | Element::Window(o) if o.host == guid => {
                let (width, height) = m.opening_type(e)?.cut();
                Some(((o.offset - width / 2.0, o.offset + width / 2.0), (z0 + o.sill, z0 + o.sill + height)))
            }
            // A wall opening: from its base offset over the wall's level, as tall as it is.
            Element::WallOpening(o) if o.host == guid => Some(o.cut(m, w)),
            _ => None,
        })
        .chain(crate::curtain::embedded_cuts(m, id, index))
        .collect();
    let mut cuts = axis.stations(0.0, len);
    cuts.extend(holes.iter().flat_map(|((s0, s1), _)| [*s0, *s1]).filter(|s| *s > 0.0 && *s < len));
    cuts.sort_by(f64::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    let (start, end) = (cap_in(m, id, false, index), cap_in(m, id, true, index));
    let (base, top_bound) = bounds(m, w);
    let curve = axis.centre().map(|c| (c, axis.at(0.0, (ext + int) / 2.0).dist(c)));
    let mut out = Vec::new();
    for pair in cuts.windows(2) {
        let &[a, b] = pair else { continue };
        let mut ring = if a <= 0.0 { start.clone() } else { vec![axis.at(a, int), axis.at(a, ext)] };
        ring.extend(if b >= len { end.clone() } else { vec![axis.at(b, ext), axis.at(b, int)] });
        // Up from its base to its top, past the openings over its middle.
        let mid = (a + b) / 2.0;
        let gaps = merged(holes.iter().filter(|((s0, s1), _)| *s0 <= mid && mid <= *s1).map(|(_, z)| *z).collect());
        let flat = |z: f64| Bound { regions: Vec::new(), own: z };
        let mut floors: Vec<Bound> = vec![base.clone()];
        let mut ceilings: Vec<Bound> = Vec::new();
        for (g0, g1) in gaps {
            ceilings.push(flat(g0));
            floors.push(flat(g1));
        }
        ceilings.push(top_bound.clone());
        for (k, (under, over)) in floors.iter().zip(&ceilings).enumerate() {
            for (part, bottom) in under.parts(ring.clone()) {
                // An opening's lower edge is no lower than the wall's base (one wholly under it cuts nothing), its upper edge no
                // higher than the wall's top (one wholly over it, the same): the wall stays between its own planes.
                let bottoms = if k > 0 { within(part, &base, bottom, true) } else { vec![(part, bottom)] };
                for (part, bottom) in bottoms {
                    for (part, top) in over.parts(part) {
                        let tops = if k + 1 < ceilings.len() { within(part, &top_bound, top, false) } else { vec![(part, top)] };
                        for (part, top) in tops {
                            // Only where it has some height.
                            let part = geom::clip(&part, |q| top.at(q) - bottom.at(q));
                            if part.len() >= 3 && geom::area(&part).abs() > 1.0 {
                                out.push(Prism { ring: part, bottom, top, curve });
                            }
                        }
                    }
                }
            }
        }
    }
    // Where a wall ending on this one's side runs its layers into it, that part is the other's: this wall keeps only what
    // is under the other's base or over its top there.
    for (hole, lo, hi) in tongues(m, id, index) {
        let pieces: Vec<Vec<P2>> = convex_pieces(&hole);
        let mut cut = Vec::with_capacity(out.len());
        for p in out {
            for ring in minus(std::slice::from_ref(&p.ring), &hole) {
                cut.push(Prism { ring, bottom: p.bottom, top: p.top, curve: p.curve });
            }
            for piece in &pieces {
                let Some(inside) = split(p.ring.clone(), piece).0 else { continue };
                for (bottom, top) in [(p.bottom, Plane::flat(lo)), (Plane::flat(hi), p.top)] {
                    // Only where it has some height: under the other wall where its base is over this one's, over it
                    // where its top is under this one's.
                    let part = geom::clip(&inside, |q| top.at(q) - bottom.at(q) - TOL);
                    if part.len() >= 3 && geom::area(&part).abs() > 1.0 {
                        cut.push(Prism { ring: part, bottom, top, curve: p.curve });
                    }
                }
            }
        }
        out = cut;
    }
    out
}

/// Where walls ending on the side of the straight wall `id` run into its body: each one's part of the plan there (its end's
/// cap, closed back along its own faces), with the heights it stands between.
fn tongues(m: &Model, id: Id, index: Option<&Index>) -> Vec<(Vec<P2>, f64, f64)> {
    let Some(w) = m.wall(id) else { return Vec::new() };
    let back = 2.0 * m.thickness(w) + 1.0;
    // And the walls that butt it where three or four end at a point, each one later in the join's order than it: what of
    // its plan their ends stand in is theirs.
    let mut later: Vec<(Id, bool)> = Vec::new();
    for at_end in [false, true] {
        let (arms, through) = arms(m, id, at_end, index);
        let Some(order) = node_order(m, &arms, through) else { continue };
        if order_setting(m, &order).0 == crate::model::JoinKind::Mitre {
            continue;
        }
        let place = order.iter().position(|(other, end)| *other == id && *end == at_end).unwrap_or(order.len());
        later.extend(order.into_iter().skip(place + 1).filter(|_| place >= 1 || true).skip(usize::from(place == 0)));
    }
    tees(m, id, index)
        .into_iter()
        .filter(|(other, at_end)| tee_cleans(m, *other, *at_end, id))
        .chain(later)
        .filter_map(|(other, at_end)| {
            let o = m.wall(other)?;
            let end = end_geometry(m, other, at_end, index);
            // Only an end whose layers were run into this wall: a plain one stops at its face and takes nothing.
            end.cut.as_ref()?;
            let a = arm(m, other, o, at_end, false);
            let mut ring = vec![a.cw + a.way * back];
            ring.extend(end.outline.iter().copied());
            ring.push(a.ccw + a.way * back);
            ring.dedup_by(|p, q| p.dist(*q) < 1e-6);
            let (lo, hi) = heights(m, o);
            (ring.len() >= 3 && geom::area(&ring).abs() > 1.0).then_some((ring, lo, hi))
        })
        .collect()
}

#[cfg(test)]
#[path = "walls_tests.rs"]
mod tests;

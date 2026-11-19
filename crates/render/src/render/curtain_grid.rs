//! A curtain wall's grid for the hand that works on it, in any view: where each grid line and the wall's outline are in the
//! model ([`marks`], [`outline`]), the place on a wall a point of a section is ([`in_section`]) or a point in the model is
//! ([`place_of`]), the line the Curtain Grid tool would put at a place ([`aim`]: which way it runs, what it snaps to and the
//! lines either side of it, as Revit's tool previews it), and the grid line under the cursor ([`line_near`]). A front end
//! turns its cursor into a place, draws what these give through its own projection, and runs the commands.

use crate::curtain::{self, Grid, Segment};
use crate::geom::P2;
use crate::graphics::Look;
use crate::model::{Element, Id, Model, Section, TOL};
use crate::view3d::P3;
use crate::walls::Axis;

/// How many pieces a lying line of a curved wall is drawn in.
const CURVE: usize = 24;

/// A place on a curtain wall: the wall, mm along it from its start and mm over its base.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub wall: Id,
    pub s: f64,
    pub z: f64,
}

/// A grid line of a curtain wall in the model: the hand-made line it is (none for one the wall's type places, and for a
/// border), which way it runs, where it is (mm along the wall, or over its base), which border it is (0: none) and the
/// points it runs through, from its foot to its head or from the wall's start to its end.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub line: Option<Id>,
    pub vertical: bool,
    pub at: f64,
    pub border: u8,
    pub pts: Vec<P3>,
}

/// What the Curtain Grid tool snaps a line to, as Revit names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Snap {
    /// Half way between the lines either side.
    Midpoint,
    /// A third of the way from one of them.
    Third,
    /// A level's elevation, for a lying line.
    Level(Id),
}

/// The line the Curtain Grid tool would put at a place: standing or lying, where (mm along the wall, or over its base),
/// what it snapped to, and where the lines either side of it are, which its temporary dimensions measure to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    pub vertical: bool,
    pub at: f64,
    pub snap: Option<Snap>,
    pub before: f64,
    pub after: f64,
}

/// The point of curtain wall `wall` that is `s` mm along it and `z` mm over its base.
pub fn point(m: &Model, wall: Id, s: f64, z: f64) -> Option<P3> {
    let w = m.wall(wall)?;
    let p = Axis::of(w).at(s, 0.0);
    Some([p.0, p.1, m.own_base(w) + z])
}

/// The place on curtain wall `wall` nearest model point `p`.
pub fn place_of(m: &Model, wall: Id, p: P3) -> Option<Place> {
    let w = m.wall(wall).filter(|w| curtain::is_curtain(m, w))?;
    let (s, _) = Axis::of(w).project(P2(p[0], p[1]));
    Some(Place { wall, s, z: p[2] - m.own_base(w) })
}

/// The curtain wall an element is, or is a panel, a mullion or a grid line of.
pub fn wall_of(m: &Model, id: Id) -> Option<Id> {
    match m.elements.get(id)? {
        Element::Wall(w) => curtain::is_curtain(m, w).then_some(id),
        e => curtain::host_of(e).and_then(|h| m.elements.id(h)),
    }
}

/// The points a line of the grid runs through: a standing one at `at` from `from` to `to` over the base, a lying one at
/// `at` over the base from `from` to `to` along the wall (in pieces round a curved wall).
fn run(axis: &Axis, base: f64, vertical: bool, at: f64, (from, to): (f64, f64)) -> Vec<P3> {
    if vertical {
        let p = axis.at(at, 0.0);
        return vec![[p.0, p.1, base + from], [p.0, p.1, base + to]];
    }
    let n = if axis.radius().is_some() { CURVE } else { 1 };
    (0..=n)
        .map(|k| {
            let p = axis.at(from + (to - from) * k as f64 / n as f64, 0.0);
            [p.0, p.1, base + at]
        })
        .collect()
}

/// Every line of curtain wall `wall`'s grid, its borders too: the standing ones, then the lying ones. Empty for a wall that
/// is not a curtain wall.
pub fn marks(m: &Model, wall: Id) -> Vec<Mark> {
    let (Some(w), Some(g)) = (m.wall(wall), curtain::grid(m, wall)) else { return Vec::new() };
    let (axis, base) = (Axis::of(w), m.own_base(w));
    let mut out = Vec::new();
    for (vertical, lines, extent) in [(true, &g.u, g.height), (false, &g.v, g.length)] {
        for l in lines {
            out.push(Mark {
                line: l.manual.and_then(|guid| m.elements.id(guid)),
                vertical,
                at: l.at,
                border: l.border,
                pts: run(&axis, base, vertical, l.at, (0.0, extent)),
            });
        }
    }
    out
}

/// Curtain wall `wall`'s extents in the model, a closed ring: along its base, up its end, back along its top.
pub fn outline(m: &Model, wall: Id) -> Vec<P3> {
    let (Some(w), Some(g)) = (m.wall(wall), curtain::grid(m, wall)) else { return Vec::new() };
    let (axis, base) = (Axis::of(w), m.own_base(w));
    let mut ring = run(&axis, base, false, 0.0, (0.0, g.length));
    ring.extend(run(&axis, base, false, g.height, (g.length, 0.0)));
    ring
}

/// The points a line of curtain wall `wall` runs through, standing or lying at `at`: across the whole wall, or only from
/// and to `span` (mm over the base for a standing line, along the wall for a lying one).
pub fn line(m: &Model, wall: Id, vertical: bool, at: f64, span: Option<(f64, f64)>) -> Vec<P3> {
    let (Some(w), Some(g)) = (m.wall(wall), curtain::grid(m, wall)) else { return Vec::new() };
    let whole = (0.0, if vertical { g.height } else { g.length });
    run(&Axis::of(w), m.own_base(w), vertical, at, span.unwrap_or(whole))
}

/// The points segment `seg` of curtain wall `wall`'s grid runs through.
pub fn segment(m: &Model, wall: Id, seg: &Segment) -> Vec<P3> {
    line(m, wall, seg.vertical, seg.at, Some((seg.span[0], seg.span[1])))
}

/// A model point in the plane of section or elevation `v`: mm along its line from its start, and its height.
pub fn to_section(v: &Section, p: P3) -> P2 {
    let u = (v.end - v.start).unit();
    P2((P2(p[0], p[1]) - v.start).dot(u), p[2])
}

/// The place on a curtain wall that point `q` of section or elevation `v` shows (mm along its line, and up): of the walls
/// `look` shows that the view looks at, not end on, the one nearest its line that reaches `q`, within `reach` mm of its
/// edges. A wall is read straight from end to end, as the view shows a straight one.
pub fn in_section(m: &Model, v: &Section, look: &Look, q: P2, reach: f64) -> Option<Place> {
    let u = (v.end - v.start).unit();
    let looking = v.looking();
    let along = |p: P2| (p - v.start).dot(u);
    let past = |p: P2| (p - v.start).dot(looking);
    let mut best: Option<(f64, Place)> = None;
    for wall in m.lookup().walls_between(f64::NEG_INFINITY, f64::INFINITY) {
        let Some(w) = m.wall(wall).filter(|w| curtain::is_curtain(m, w) && look.shows(wall)) else { continue };
        let axis = Axis::of(w);
        let len = axis.length();
        let (a, b) = (axis.at(0.0, 0.0), axis.at(len, 0.0));
        let (pa, pb, xa, xb) = (past(a), past(b), along(a), along(b));
        if pa.max(pb) < -TOL || v.depth.is_some_and(|far| pa.min(pb) > far) || (xa - xb).abs() < 1.0 {
            continue;
        }
        let (z0, z1) = (m.own_base(w), m.own_top(w));
        if q.0 < xa.min(xb) - reach || q.0 > xa.max(xb) + reach || q.1 < z0 - reach || q.1 > z1 + reach {
            continue;
        }
        let t = ((q.0 - xa) / (xb - xa)).clamp(0.0, 1.0);
        let near = pa + (pb - pa) * t;
        if best.as_ref().is_none_or(|(d, _)| near < *d) {
            best = Some((near, Place { wall, s: t * len, z: (q.1 - z0).clamp(0.0, z1 - z0) }));
        }
    }
    best.map(|(_, p)| p)
}

/// The lines either side of `at` among `lines`, the borders at the least.
fn beside(lines: &[curtain::Line], at: f64, extent: f64) -> (f64, f64) {
    let before = lines.iter().map(|l| l.at).filter(|x| *x < at - curtain::NEAR).fold(0.0, f64::max);
    let after = lines.iter().map(|l| l.at).filter(|x| *x > at + curtain::NEAR).fold(extent, f64::min);
    (before, after)
}

/// The line the Curtain Grid tool would put at place (`s`, `z`) of curtain wall `wall`, whose grid is `g`, as Revit's: it
/// runs square to the edge the cursor is nearest (along the base, the top or a lying line it is a standing line; beside an
/// end or a standing line, a lying one; `way` says which when the view shows one way only), and within `reach` mm it snaps
/// to the middle and the thirds between the lines either side and, lying, to a level. None where no line can go (on a line
/// that is there, or outside the wall).
pub fn aim(m: &Model, wall: Id, g: &Grid, s: f64, z: f64, way: Option<bool>, reach: f64) -> Option<Aim> {
    if !(s.is_finite() && z.is_finite() && reach.is_finite()) {
        return None;
    }
    let nearest = |lines: &[curtain::Line], x: f64| lines.iter().map(|l| (l.at - x).abs()).fold(f64::INFINITY, f64::min);
    // Nearer a lying edge than a standing one: a standing line.
    let vertical = way.unwrap_or_else(|| nearest(&g.v, z) <= nearest(&g.u, s));
    let (lines, extent, x) = if vertical { (&g.u, g.length, s) } else { (&g.v, g.height, z) };
    let (before, after) = beside(lines, x, extent);
    let mut snaps: Vec<(f64, Snap)> = vec![
        ((before + after) / 2.0, Snap::Midpoint),
        (before + (after - before) / 3.0, Snap::Third),
        (after - (after - before) / 3.0, Snap::Third),
    ];
    if !vertical && let Some(w) = m.wall(wall) {
        let base = m.own_base(w);
        snaps.extend(m.levels().into_iter().map(|(id, l)| (l.elevation - base, Snap::Level(id))));
    }
    let snapped = snaps
        .into_iter()
        .filter(|(at, _)| (at - x).abs() <= reach && *at > before + curtain::NEAR && *at < after - curtain::NEAR)
        .min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs()));
    let (at, snap) = match snapped {
        Some((at, snap)) => (at, Some(snap)),
        None => (x.round(), None),
    };
    (at > before + curtain::NEAR && at < after - curtain::NEAR).then_some(Aim { vertical, at, snap, before, after })
}

/// How far pixel `at` is from the line through `pts` as `project` shows them, px.
pub fn distance(pts: &[P3], project: &dyn Fn(P3) -> (f64, f64), at: (f64, f64)) -> f64 {
    let on: Vec<(f64, f64)> = pts.iter().map(|p| project(*p)).collect();
    on.windows(2)
        .map(|w| {
            let [a, b] = w else { return f64::INFINITY };
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = dx * dx + dy * dy;
            let t = if len > 0.0 { (((at.0 - a.0) * dx + (at.1 - a.1) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
            (a.0 + dx * t - at.0).hypot(a.1 + dy * t - at.1)
        })
        .fold(f64::INFINITY, f64::min)
}

/// The grid line of `marks` nearest pixel `at` as `project` shows them, within `reach` px: a hand-made one before one of
/// the wall's type, and no border.
pub fn line_near<'a>(marks: &'a [Mark], project: &dyn Fn(P3) -> (f64, f64), at: (f64, f64), reach: f64) -> Option<&'a Mark> {
    marks
        .iter()
        .filter(|k| k.border == 0)
        .map(|k| (distance(&k.pts, project, at), k))
        .filter(|(d, _)| *d <= reach)
        .min_by(|a, b| b.1.line.is_some().cmp(&a.1.line.is_some()).then(a.0.total_cmp(&b.0)))
        .map(|(_, k)| k)
}

/// Where hand-made grid line `line` would be, dragged from where it is by (`dx`, `dy`) px in a view that shows model points
/// through `project`: mm along its wall or over its base, kept between the lines either side of it. None for what is not a
/// hand-made grid line, or one the view shows end on.
pub fn dragged(m: &Model, line: Id, project: &dyn Fn(P3) -> (f64, f64), (dx, dy): (f64, f64)) -> Option<f64> {
    let Some(Element::CurtainGrid(l)) = m.elements.get(line) else { return None };
    let wall = m.elements.id(l.host)?;
    let g = curtain::grid(m, wall)?;
    let (extent, other) = if l.vertical { (g.length, g.height) } else { (g.height, g.length) };
    // A step along the way it moves, as the view shows it at the line's middle.
    let step = (extent / 100.0).max(1.0);
    let here = |at: f64| if l.vertical { point(m, wall, at, other / 2.0) } else { point(m, wall, other / 2.0, at) };
    let (a, b) = (project(here(l.at)?), project(here(l.at + step)?));
    let (ux, uy) = (b.0 - a.0, b.1 - a.1);
    let len = ux * ux + uy * uy;
    if len < 1e-6 {
        return None;
    }
    let moved = (dx * ux + dy * uy) / len * step;
    let lines = if l.vertical { &g.u } else { &g.v };
    let (before, after) = beside(lines, l.at, extent);
    Some((l.at + moved).round().clamp(before + 2.0 * curtain::NEAR, after - 2.0 * curtain::NEAR))
}

//! Elements moved, turned and mirrored as the Modify tools take them: what `move`, `copy`, `rotate`, `mirror` and `array`
//! do to each element, which a detail group and a linked model place their members by too.

use std::collections::BTreeSet;

use crate::geom::{P2, degrees, micron};
use crate::model::*;

/// How a command takes elements: through `map`, a translation, a turn or a mirror. A translation
/// slides a door or window on its own along its wall by `slide`; a mirror flips the way doors and
/// windows face, as a mirror image does.
pub struct Motion<'a> {
    pub map: &'a dyn Fn(P2) -> P2,
    pub slide: Option<P2>,
    pub mirror: bool,
}

/// The element taken by `mo`. Levels have no place in plan; a door or window on its own goes
/// only along its wall, by a translation, else only with its wall.
pub fn transformed(c: &str, m: &Model, id: Id, mo: &Motion, with: &BTreeSet<Id>) -> Result<Element, String> {
    let e = m.elements.get(id).cloned().ok_or_else(|| bad(c, format!("no element {id}")))?;
    transform(c, m, e, id, mo, with)
}

/// Element `e`, element `id` of `m` or a detail group's member, taken by `mo`, as [`transformed`].
pub fn transform(c: &str, m: &Model, mut e: Element, id: Id, mo: &Motion, with: &BTreeSet<Id>) -> Result<Element, String> {
    let f = mo.map;
    let within = |g: Guid| m.elements.id(g).is_some_and(|i| with.contains(&i));
    let category = e.category();
    // Its sketch goes with it, line for line.
    if let Some(lines) = e.sketch_mut() {
        lines.iter_mut().for_each(|l| *l = l.map(f));
    }
    match &mut e {
        Element::Level(_) => return Err(bad(c, format!("level {id} has no place in plan; change its elevation, or add a level"))),
        Element::Sheet(_) => return Err(bad(c, format!("sheet {id} has no place in plan; move the views on it"))),
        Element::Schedule(_) | Element::PlanView(_) => {
            return Err(bad(c, format!("{category} {id} has no place in plan; move its viewport on its sheet")));
        }
        // A camera view has a place in plan: its mark. The camera goes as a whole (the plan point of its eye and its
        // target, each keeping its height), so moving, copying, turning or mirroring it keeps it looking the same way.
        Element::View3D(v) => match v.camera {
            Some(cam) => {
                let lift = |p: [f64; 3]| {
                    let q = f(P2(p[0], p[1]));
                    [q.0, q.1, p[2]]
                };
                v.camera = Some(crate::model::ViewCamera { eye: lift(cam.eye), target: lift(cam.target), ..cam });
            }
            None => return Err(bad(c, format!("view3d {id} has no place in plan; move its viewport on its sheet"))),
        },
        Element::CadImport(ci) => {
            if mo.mirror {
                return Err(bad(
                    c,
                    format!("CAD import {id} can't be mirrored; mirror it in the program that made the drawing, or turn it"),
                ));
            }
            (ci.at, ci.angle) = turned(f, ci.at, ci.angle);
        }
        // A mirror image is the picture flipped.
        Element::Image(im) => {
            (im.at, im.angle) = turned(f, im.at, im.angle);
            im.mirrored ^= mo.mirror;
        }
        Element::PointCloud(pc) => {
            if mo.mirror {
                return Err(bad(c, format!("point cloud {id} can't be mirrored: a scan is what was measured; move it or turn it")));
            }
            (pc.at, pc.angle) = turned(f, pc.at, pc.angle);
        }
        Element::Text(n) => {
            let (at, angle) = turned(f, n.at, n.angle);
            // A mirrored note still reads the right way round.
            (n.at, n.angle) = (at, if mo.mirror { n.angle } else { angle });
            n.leader = n.leader.map(f);
        }
        Element::Spot(sp) => sp.at = f(sp.at),
        Element::DetailLine(l) => l.curve = l.curve.map(|p| micron(f(p))),
        // Its sketch went with it, above; its pattern goes too.
        Element::FilledRegion(r) => (r.pattern_origin, r.pattern_angle) = turned(f, r.pattern_origin, r.pattern_angle),
        // Its members go with it; a mirror image is its type's, flipped.
        Element::DetailGroup(g) => {
            (g.at, g.angle) = turned(f, g.at, g.angle);
            g.mirrored ^= mo.mirror;
        }
        Element::RepeatingDetail(r) => {
            (r.start, r.end) = (f(r.start), f(r.end));
            r.mirrored ^= mo.mirror;
        }
        // A link is moved and turned as a whole; its file stays as it is.
        Element::Link(l) => (l.at, l.angle) = turned(f, l.at, l.angle),
        Element::Viewport(v) => v.at = f(v.at),
        Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col) => (col.at, col.angle) = turned(f, col.at, col.angle),
        // A run keeps its heights at its ends; its section is the same either way round.
        crate::model::run_arm!(r) => (r.start, r.end) = (f(r.start), f(r.end)),
        // A mirror image is the family's, flipped.
        Element::Component(cp) => {
            (cp.at, cp.angle) = turned(f, cp.at, cp.angle);
            cp.mirrored ^= mo.mirror;
        }
        // A mirror image turns the other way at its landings.
        Element::Stair(Stair { start, angle, landings, .. }) | Element::Ramp(Ramp { start, angle, landings, .. }) => {
            (*start, *angle) = turned(f, *start, *angle);
            if mo.mirror {
                landings.iter_mut().for_each(|l| l.turn = -l.turn + 0.0);
            }
        }
        Element::Railing(rl) => match rl.host {
            // Its stair's or ramp's, mirrored, its sides swap.
            Some(h) if within(h) => {
                if mo.mirror {
                    rl.side = rl.side.map(other_side);
                }
            }
            Some(_) => return Err(bad(c, format!("railing {id} goes only with its stair or ramp; select that too"))),
            None => rl.path.iter_mut().for_each(|q| *q = f(*q)),
        },
        // Its edges keep their numbers, its gables with them; a mirror image sweeps the other way.
        Element::Roof(r) => {
            r.boundary.iter_mut().chain(r.courtyards.iter_mut().flatten()).chain(r.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            // A flat one's shape goes with it, each point at its height.
            r.shape.points.iter_mut().for_each(|x| x.at = f(x.at));
            r.shape.lines.iter_mut().for_each(|l| (l.a, l.b) = (f(l.a), f(l.b)));
            if let Some(x) = &mut r.extrusion {
                x.line = x.line.map(&f);
                if mo.mirror {
                    (x.from, x.to) = (-x.to, -x.from);
                }
            }
        }
        // A mirror image runs back the other way, so its exterior side is the mirror image of the old one.
        Element::Wall(w) => (w.start, w.end) = if mo.mirror { (f(w.end), f(w.start)) } else { (f(w.start), f(w.end)) },
        Element::Grid(g) => (g.start, g.end) = (f(g.start), f(g.end)),
        Element::RoomSeparator(rs) => (rs.start, rs.end) = (f(rs.start), f(rs.end)),
        Element::Rebar(rb) => {
            for q in &mut rb.path {
                let moved = f(P2(q[0], q[1]));
                (q[0], q[1]) = (moved.0, moved.1);
            }
        }
        Element::Floor(fl) | Element::Toposolid(fl) => {
            fl.boundary.iter_mut().chain(fl.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            // A mirror turns the outline clockwise; floors keep theirs counter-clockwise.
            if mo.mirror {
                fl.boundary.reverse();
            }
            fl.arrow = fl.arrow.map(|a| a.map(f));
            (fl.pattern_origin, fl.pattern_angle) = turned(f, fl.pattern_origin, fl.pattern_angle);
            // Its shape's points and lines go with it, each at its height.
            fl.shape.points.iter_mut().for_each(|x| x.at = f(x.at));
            fl.shape.lines.iter_mut().for_each(|l| (l.a, l.b) = (f(l.a), f(l.b)));
        }
        Element::Ceiling(cl) => {
            cl.boundary.iter_mut().chain(cl.openings.iter_mut().flatten()).for_each(|q| *q = f(*q));
            if mo.mirror {
                cl.boundary.reverse();
            }
        }
        Element::Shaft(Shaft { boundary, .. }) => {
            boundary.iter_mut().for_each(|q| *q = f(*q));
            if mo.mirror {
                boundary.reverse();
            }
        }
        // Its top outline goes the same way, twin for twin; its masterplan figures' points and directions too.
        Element::Mass(Mass { boundary, top, site, .. }) => {
            for ring in std::iter::once(boundary).chain(top.as_mut()) {
                ring.iter_mut().for_each(|q| *q = f(*q));
                if mo.mirror {
                    ring.reverse();
                }
            }
            if let Some(site) = site {
                crate::masterplan::transform_props("mass", &mut site.props, f, mo.mirror);
            }
        }
        // Its truth goes (paths, control points, where it stands and which way it faces), and its shape with it until
        // `masterplan::keep` works it out again after the command.
        Element::Site(st) => {
            let kind = st.kind.clone();
            crate::masterplan::transform_props(&kind, &mut st.props, f, mo.mirror);
            let poly = |p: &mut crate::site::Poly| p.outer.iter_mut().chain(p.holes.iter_mut().flatten()).for_each(|q| *q = f(*q));
            st.outline.iter_mut().for_each(poly);
            for part in &mut st.parts {
                part.polys.iter_mut().for_each(poly);
                part.lines.iter_mut().flatten().for_each(|q| *q = f(*q));
            }
            for piece in st.layout.iter_mut().flat_map(|l| l.pieces.iter_mut()) {
                piece.outline.iter_mut().for_each(poly);
            }
        }
        // A mirror image of an arc bulges the other way.
        Element::PropertyLine(pl) => {
            for q in &mut pl.points {
                let p = f(P2(q[0], q[1]));
                (q[0], q[1]) = (p.0, p.1);
                if mo.mirror {
                    q[2] = -q[2];
                }
            }
        }
        Element::BuildingPad(bp) => {
            bp.boundary.iter_mut().for_each(|q| *q = f(*q));
            if mo.mirror {
                bp.boundary.reverse();
            }
        }
        // A frozen room's boundary goes with it.
        Element::Room(r) => {
            r.point = f(r.point);
            if let Some(k) = &mut r.kept {
                k.map(f);
            }
        }
        Element::Door(o) | Element::Window(o) if !within(o.host) => match (mo.slide, m.wall(o.host)) {
            (Some(by), Some(w)) => o.offset += by.dot((w.end - w.start).unit()),
            _ => return Err(bad(c, format!("{category} {id} turns and mirrors only with its wall; select the wall too"))),
        },
        Element::Door(_) | Element::Window(_) => {}
        Element::Dimension(d) => d.at = f(d.at),
        // In plan, its points and edges; their heights stay.
        Element::Dimension3D(d) => {
            let flat = |q: &mut [f64; 3]| {
                let P2(x, y) = f(P2(q[0], q[1]));
                (q[0], q[1]) = (x, y);
            };
            flat(&mut d.at);
            for r in &mut d.refs {
                flat(&mut r.at);
                if let Some(e) = &mut r.edge {
                    e.iter_mut().for_each(flat);
                }
            }
        }
        // A tag's place hangs on its door or window's: see `relocate` and `duplicate`.
        Element::Tag(_) => {}
        Element::Section(v) | Element::Elevation(v) => {
            (v.start, v.end) = (f(v.start), f(v.end));
            // A mirror image of it looks the mirrored way round: to the left of its line again.
            if mo.mirror {
                std::mem::swap(&mut v.start, &mut v.end);
            }
        }
    }
    Ok(e)
}

/// Where `f` takes a point and the way `angle` degrees points from it.
pub fn turned(f: &dyn Fn(P2) -> P2, at: P2, angle: f64) -> (P2, f64) {
    let (a, b) = (f(at), f(at + heading(angle) * 1000.0));
    (a, degrees((b.1 - a.1).atan2(b.0 - a.0).to_degrees()))
}

pub fn other_side(s: Side) -> Side {
    match s {
        Side::Left => Side::Right,
        Side::Right => Side::Left,
    }
}

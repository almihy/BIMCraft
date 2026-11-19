//! A curtain wall in a plan, a section and an elevation (`curtain.rs` in the model). The wall has no solid, so a plan draws
//! no poché, no layers and no cut outline of its own for it: what shows are its panels and its mullions, each an element.
//!
//! - **Cut by the plan's cut plane:** a panel is its thickness, an outline closed at its ends (glass is not filled; a panel
//!   of another material is, as what is cut is); a mullion is its section, a rectangle or a circle, filled. The same at
//!   Coarse, Medium and Fine: Revit has no two-line coarse form for a curtain wall, and only a material's cut pattern is
//!   Fine's.
//! - **Below the cut plane, in the view range:** their outlines, as a wall seen below is drawn. Over it: nothing, and in a
//!   reflected ceiling plan what is over its cut and under the range's top is seen the same way.
//! - **The wall itself** owns what is under all of it, drawn in no ink: its extents filled and outlined dashed, so a click
//!   anywhere on it takes the wall first (the last drawn of what the cursor is inside), Tab goes on to the mullion or the
//!   panel, and a front end that highlights an element's primitives shows the whole wall dashed, as Revit does.
//! - **A curved wall's** panels are flat between its grid lines (`curtain::panel_face`), so they draw straight.
//!
//! In a section or an elevation the picture is the solids', and the cut is theirs too; here are the wall's dashed extents
//! and its hand-made grid lines, there to be picked.

use super::{BY_CATEGORY, Detail, Drawing, FAINT, INK, Layer, POCHE, Part, Prim, cut_layer, override_patterns, pattern_style};
use crate::curtain::{self, GridLine, Mullion, Panel};
use crate::geom::P2;
use crate::graphics::Look;
use crate::model::{Element, Id, Model, PlanRange, Section, TOL, Wall};
use crate::prim::PAPER;
use crate::view3d::Image;
use crate::walls::Axis;

/// The sides of a circular mullion's section as a plan draws it.
const ROUND: usize = 24;
/// How many places along each edge of a wall's extents a section's picture is asked what it shows, and how far in, px.
const SAMPLES: usize = 24;
const INSET: f32 = 3.0;
/// The dashes of a curtain wall's extents and of its grid lines, px.
const DASH: [f32; 2] = [8.0, 5.0];

/// What a panel or a mullion takes in plan: its outline, the heights it reaches from and to, the way it runs, how wide it is
/// across that, the material it is made of, and whether it is see-through (glass, which a cut does not fill).
pub(crate) struct Footprint {
    pub ring: Vec<P2>,
    pub bottom: f64,
    pub top: f64,
    along: P2,
    width: f64,
    material: String,
    clear: bool,
}

/// Whether a material lets what is behind it show: glass, or no material at all, which a panel draws as glass.
fn clear(m: &Model, material: &str) -> bool {
    m.materials.get(material).is_none_or(|x| x.transparency > 0)
}

/// A panel's footprint: its thickness about the plane through its middle, between its two grid lines. As its solid is.
fn panel(m: &Model, p: &Panel) -> Option<Footprint> {
    let ([a, b, _, d], out, thickness) = curtain::panel_face(m, p)?;
    let (o, e, half) = (P2(a[0], a[1]), P2(b[0], b[1]), out * (thickness / 2.0));
    let material = m.types.curtain_panel.get(&p.type_name).map_or_else(String::new, |t| t.material.clone());
    Some(Footprint {
        ring: vec![o - half, e - half, e + half, o + half],
        bottom: a[2],
        top: d[2],
        along: (e - o).unit(),
        width: thickness,
        clear: clear(m, &material),
        material,
    })
}

/// A mullion's footprint: a standing one its section (a rectangle about its grid line, or a circle), a lying one its length
/// by its depth. As its solid is.
fn mullion(m: &Model, x: &Mullion) -> Option<Footprint> {
    let (a, b, along, out, t) = curtain::mullion_line(m, x)?;
    let (o, e) = (P2(a[0], a[1]), P2(b[0], b[1]));
    let (s1, s2) = t.sides();
    let half = out * (t.thickness / 2.0);
    let (ring, bottom, top, along) = match along {
        Some(_) if t.round => {
            let r = t.thickness / 2.0;
            let ring = (0..ROUND)
                .map(|k| {
                    let (s, c) = (k as f64 / ROUND as f64 * std::f64::consts::TAU).sin_cos();
                    o + P2(c * r, s * r)
                })
                .collect();
            (ring, a[2], b[2], out)
        }
        Some(u) => (vec![o - u * s1 - half, o + u * s2 - half, o + u * s2 + half, o - u * s1 + half], a[2], b[2], out),
        None => {
            let (under, over) = if t.round { (t.thickness / 2.0, t.thickness / 2.0) } else { (s1, s2) };
            (vec![o - half, e - half, e + half, o + half], a[2] - under, a[2] + over, (e - o).unit())
        }
    };
    Some(Footprint { ring, bottom, top, along, width: t.thickness.min(t.width()), material: t.material.clone(), clear: false })
}

/// What element `e` takes in plan when it is a curtain wall's panel or mullion.
pub(crate) fn footprint(m: &Model, e: &Element) -> Option<Footprint> {
    match e {
        Element::CurtainPanel(p) => panel(m, p),
        Element::Mullion(x) => mullion(m, x),
        _ => None,
    }
}

/// A curtain wall's extents in plan: its line, as deep each way as its deepest panel or mullion reaches.
pub(crate) fn extents(m: &Model, w: &Wall) -> Vec<P2> {
    let (axis, half) = (Axis::of(w), m.thickness(w) / 2.0);
    let len = axis.length();
    axis.points(0.0, len, half).into_iter().chain(axis.points(len, 0.0, -half)).collect()
}

/// Whether a plan of `range` cuts what reaches from `bottom` to `top`.
fn cut_by(range: &PlanRange, bottom: f64, top: f64) -> bool {
    bottom <= range.cut && range.cut < top
}

/// Whether a plan of `range` sees what reaches from `bottom` to `top` and is not cut by it: looking down, what tops out
/// under its cut plane and over its view depth; looking `up` (a reflected ceiling plan), what starts over its cut and under
/// its top.
fn seen_by(range: &PlanRange, up: bool, bottom: f64, top: f64) -> bool {
    if up { range.cut < bottom && bottom <= range.top } else { range.depth - TOL < top && top <= range.cut }
}

/// The panels and mullions among `elements` that `look` shows, with what each takes in plan.
fn shown<'a>(m: &'a Model, elements: &'a [(Id, &'a Element)], look: &'a Look) -> impl Iterator<Item = (Id, Footprint)> + 'a {
    elements.iter().filter(|(id, _)| look.shows(*id)).filter_map(|(id, e)| Some((*id, footprint(m, e)?)))
}

/// The corners of the panels and mullions a floor plan of `range` cuts or sees: what it is fitted to.
pub(super) fn shows(m: &Model, elements: &[(Id, &Element)], range: &PlanRange, look: &Look) -> Vec<P2> {
    shown(m, elements, look)
        .filter(|(_, f)| cut_by(range, f.bottom, f.top) || seen_by(range, false, f.bottom, f.top))
        .flat_map(|(_, f)| f.ring)
        .collect()
}

/// The panels and mullions a plan sees and does not cut: their outlines, in projection; past the range's bottom, faint.
pub(super) fn seen(d: &mut Drawing, m: &Model, elements: &[(Id, &Element)], range: &PlanRange, look: &Look, up: bool) {
    for (id, f) in shown(m, elements, look) {
        if cut_by(range, f.bottom, f.top) || !seen_by(range, up, f.bottom, f.top) {
            continue;
        }
        d.own(Some(id));
        override_patterns(d, m, std::slice::from_ref(&f.ring), false, (P2(0.0, 0.0), 0.0));
        let ink = if up || f.top > range.bottom - TOL { INK } else { FAINT };
        d.line(&f.ring, BY_CATEGORY, ink, true);
    }
}

/// The panels and mullions a plan cuts, and then each curtain wall's extents for the cursor (see the module's notes). The
/// drawing's part is the cut's on the way in and on the way out.
pub(super) fn cut(d: &mut Drawing, m: &Model, elements: &[(Id, &Element)], range: &PlanRange, look: &Look, up: bool) {
    // Panels first, then the mullions over their ends.
    for mullions in [false, true] {
        for (id, f) in shown(m, elements, look) {
            let is_mullion = matches!(m.elements.get(id), Some(Element::Mullion(_)));
            if is_mullion != mullions || !cut_by(range, f.bottom, f.top) {
                continue;
            }
            d.own(Some(id));
            let rings = std::slice::from_ref(&f.ring);
            if pattern_style(d, true).is_some() {
                d.fill(rings, PAPER);
                override_patterns(d, m, rings, true, (P2(0.0, 0.0), 0.0));
            } else if !f.clear {
                d.fill(rings, POCHE);
                if d.detail == Detail::Fine {
                    let layer = Layer { material: f.material.clone(), thickness: f.width, ..Layer::default() };
                    cut_layer(d, m, &layer, &f.ring, f.along, f.width * d.view.scale);
                }
            }
            d.line(&f.ring, BY_CATEGORY, INK, true);
        }
    }
    // Each wall whose panels or mullions the plan shows: its extents, in no ink, over them.
    d.part(Part::Projection);
    for (id, e) in elements.iter().filter(|(id, _)| look.shows(*id)) {
        let Element::Wall(w) = e else { continue };
        if !curtain::is_curtain(m, w) {
            continue;
        }
        let (bottom, top) = (m.base_z(w), m.top_z(w));
        if !(cut_by(range, bottom, top) || seen_by(range, up, bottom, top)) {
            continue;
        }
        d.own(Some(*id));
        unseen(d, &extents(m, w), true);
    }
    d.part(Part::Cut);
}

/// `ring` (mm) as what is there for the cursor and for a highlight, in no ink: filled when `filled`, and outlined dashed.
fn unseen(d: &mut Drawing, ring: &[P2], filled: bool) {
    let pts: Vec<(f32, f32)> = ring.iter().map(|p| d.view.px(*p)).collect();
    if pts.len() < 2 {
        return;
    }
    if filled {
        d.push(Prim::Fill { rings: vec![pts.clone()], color: INK, rule: Default::default(), alpha: 0 });
    }
    d.push(Prim::Line { pts, width: 1.0, color: INK, dash: Some(DASH.to_vec()), closed: ring.len() > 2, alpha: 0 });
}

/// In a section or an elevation `v` (its plane: x along its line from its start, y up, mm), from `first` to `last` along
/// its line: each curtain wall `look` shows that it looks at, its extents dashed in no ink (an outline: the panels and
/// mullions inside it are the picture's to pick), and each of its hand-made grid lines across it, a little past its edges.
pub(super) fn in_section(d: &mut Drawing, m: &Model, v: &Section, look: &Look, (first, last): (f64, f64)) {
    let u = (v.end - v.start).unit();
    let (looking, lookup) = (v.looking(), m.lookup());
    let along = |q: P2| (q - v.start).dot(u);
    let past = |q: P2| (q - v.start).dot(looking);
    for wall in lookup.walls_between(f64::NEG_INFINITY, f64::INFINITY) {
        let Some(w) = m.wall(wall).filter(|w| curtain::is_curtain(m, w) && look.shows(wall)) else { continue };
        let axis = Axis::of(w);
        let (a, b) = (axis.at(0.0, 0.0), axis.at(axis.length(), 0.0));
        // Beyond the line (or on it), within its depth, and not seen end on.
        let (pa, pb) = (past(a), past(b));
        let (xa, xb) = (along(a), along(b));
        if pa.max(pb) < -TOL || v.depth.is_some_and(|far| pa.min(pb) > far) || (xa - xb).abs() < 1.0 {
            continue;
        }
        if xa.max(xb) < first || xa.min(xb) > last {
            continue;
        }
        let (z0, z1) = (m.own_base(w), m.own_top(w));
        d.own(Some(wall));
        unseen(d, &[P2(xa, z0), P2(xb, z0), P2(xb, z1), P2(xa, z1)], false);
        // Its hand-made grid lines: a little past its edges, as Revit's reach past the wall, and across it where no mullion
        // lies on them (a mullion is the picture's to pick; the drawing's line over it would always be taken first).
        let reach = ((xa - xb).abs().max(z1 - z0) / 50.0).max(100.0);
        let (host, grid) = (m.guid(wall), curtain::grid(m, wall));
        for id in m.hosted_by(host) {
            let Some(Element::CurtainGrid(GridLine { vertical, at, .. })) = m.elements.get(id) else { continue };
            if !look.shows(id) {
                continue;
            }
            // Where it is in the section's plane, `t` mm along it from the wall's start or base.
            let place = |t: f64| if *vertical { P2(along(axis.at(*at, 0.0)), z0 + t) } else { P2(along(axis.at(t, 0.0)), z0 + at) };
            let extent = if *vertical { z1 - z0 } else { axis.length() };
            let mut pieces = vec![(-reach, 0.0), (extent, extent + reach)];
            pieces.extend(free_pieces(m, host, grid.as_ref(), m.guid(id), *vertical).unwrap_or_else(|| vec![(0.0, extent)]));
            d.own(Some(id));
            for (from, to) in pieces {
                unseen(d, &[place(from), place(to)], false);
            }
        }
    }
    d.own(None);
}

/// The pieces of the hand-made grid line `line` of the wall whose GUID is `host` that carry no mullion: each from and to, mm
/// along the line. None when the grid has no such line (it lies on another, or outside the wall).
fn free_pieces(
    m: &Model,
    host: crate::model::Guid,
    grid: Option<&curtain::Grid>,
    line: crate::model::Guid,
    vertical: bool,
) -> Option<Vec<(f64, f64)>> {
    let g = grid?;
    let (lines, across) = if vertical { (&g.u, &g.v) } else { (&g.v, &g.u) };
    let l = lines.iter().find(|l| l.manual == Some(line))?;
    let mut out: Vec<(f64, f64)> = Vec::new();
    for pair in across.windows(2) {
        let [from, to] = pair else { continue };
        let key = format!("m:{}", curtain::segment_key(&l.key, &from.key));
        let carries =
            m.elements.id(curtain::child_guid(m.uid, host, &key)).is_some_and(|c| matches!(m.elements.get(c), Some(Element::Mullion(_))));
        if carries {
            continue;
        }
        match out.last_mut() {
            Some(last) if (last.1 - from.at).abs() < 1e-6 => last.1 = to.at,
            _ => out.push((from.at, to.at)),
        }
    }
    Some(out)
}

/// A section's curtain walls that `picture` (its own, over the same view) shows nothing of, behind something else: their
/// extents and grid lines are no longer theirs to pick. The drawing has no depth, the picture has.
pub(super) fn behind(d: &mut Drawing, m: &Model, picture: &Image) {
    let view = d.view;
    let (rx, ry) = (f64::from(picture.width) / f64::from(view.width.max(1)), f64::from(picture.height) / f64::from(view.height.max(1)));
    let wall_of = |id: Id| m.elements.get(id).and_then(curtain::host_of).and_then(|h| m.elements.id(h));
    // Each wall's outline: whether the picture shows one of its panels or mullions just inside it.
    let mut shows: std::collections::BTreeMap<Id, bool> = std::collections::BTreeMap::new();
    for (prim, owner) in d.prims.iter().zip(&d.owners) {
        let (Prim::Line { pts, alpha: 0, closed: true, .. }, Some(wall)) = (prim, *owner) else { continue };
        if !matches!(m.elements.get(wall), Some(Element::Wall(_))) || pts.is_empty() {
            continue;
        }
        let n = pts.len() as f32;
        let mid = pts.iter().fold((0.0, 0.0), |s, p| (s.0 + p.0 / n, s.1 + p.1 / n));
        let seen = pts.iter().zip(pts.iter().cycle().skip(1)).any(|(a, b)| {
            (0..=SAMPLES).any(|k| {
                let t = k as f32 / SAMPLES as f32;
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                // A few pixels in from the edge, toward the middle.
                let (dx, dy) = (mid.0 - x, mid.1 - y);
                let far = dx.hypot(dy).max(1.0);
                let (x, y) = (x + dx / far * INSET, y + dy / far * INSET);
                picture.pick(f64::from(x) * rx, f64::from(y) * ry).and_then(wall_of) == Some(wall)
            })
        });
        *shows.entry(wall).or_insert(false) |= seen;
    }
    for (prim, owner) in d.prims.iter().zip(d.owners.iter_mut()) {
        let Some(id) = *owner else { continue };
        if !matches!(prim, Prim::Line { alpha: 0, .. }) {
            continue;
        }
        let wall = if matches!(m.elements.get(id), Some(Element::Wall(_))) { Some(id) } else { wall_of(id) };
        if wall.is_some_and(|w| shows.get(&w) == Some(&false)) {
            *owner = None;
        }
    }
}

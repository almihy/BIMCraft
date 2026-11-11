//! What walls, floors, ceilings and roofs measure: the one place schedules (Area, Volume, Perimeter), element JSON and IFC's
//! `Qto_*BaseQuantities` read their numbers from, so a schedule and an exported file never disagree. Lengths are in mm, areas in
//! mm², volumes in mm³; each reader converts.
//!
//! The rules, as Revit's: a wall's area is one side of it, less the doors and windows in it; its volume is its solid's, joined
//! to the walls it meets (a corner counted once), cut by them and by what it is attached to. A floor's area is its top's (a
//! sloped one's stretched by its slope), less its openings and the shafts through it, its volume the solid's; a ceiling's, its
//! outline less its openings and the shafts, through its thickness; a roof's area is its surface (its plan stretched by its
//! slope) less its openings and the shafts, its volume the solid's.

use crate::geom::{self, P2};
use crate::model::{Ceiling, Floor, Id, Model, Roof};
use crate::walls::{self, Axis, Plane, Prism};

/// What a wall measures.
#[derive(Clone, Debug, PartialEq)]
pub struct WallQ {
    /// Along its location line.
    pub length: f64,
    pub width: f64,
    /// Its height; none where it is attached to a roof or a floor, which makes it no one height.
    pub height: Option<f64>,
    /// One side, with and without the openings: the area of the middle of its thickness, as it stands (cut where it is joined
    /// to other walls, up to what it is attached to).
    pub gross_side: f64,
    pub net_side: f64,
    pub gross_volume: f64,
    pub net_volume: f64,
}

/// Wall `id`, measured from its solid (`walls::prisms`): where each door and window really is along it and up it, held to the
/// wall (one past its end, or under its base, cuts only what is in it; two that overlap cut what they cover once), the top
/// where it is attached, its arc, and its corners once (the mitre shares them between the walls that meet). Its volume is the
/// solid's, without the openings (gross) and with them (net); its side is the area of the middle of its thickness (its
/// volume through its thickness: a straight, free wall's length by its height, a curved one's along its own middle and not
/// along its location line), the openings' taken off it. None for an id that is no wall.
pub fn wall(m: &Model, id: Id) -> Option<WallQ> {
    let wl = m.wall(id)?;
    let (length, width) = (Axis::of(wl).length(), m.thickness(wl));
    let volume = |openings: bool| walls::prisms(m, id, openings).iter().map(volume_of).sum::<f64>();
    let gross_volume = volume(false);
    let net_volume = volume(true).min(gross_volume);
    let side = |v: f64| if width > 0.0 { v / width } else { 0.0 };
    let height = (wl.attach_top.is_none() && wl.attach_base.is_none()).then(|| m.top_z(wl) - m.base_z(wl));
    Some(WallQ { length, width, height, gross_side: side(gross_volume), net_side: side(net_volume), gross_volume, net_volume })
}

/// What a wall's prism holds: its plan through how far its top lies over its bottom at its middle (both are planes, so exact),
/// the plan of a curved wall's piece true to its arc, not to the chords the ring is drawn with.
fn volume_of(p: &Prism) -> f64 {
    let c = geom::centroid(&p.ring);
    plan_of(p) * (p.top.at(c) - p.bottom.at(c)).max(0.0)
}

/// A prism's plan area: its ring's, and for each edge that runs between two points of one circle about a curved wall's centre,
/// the segment between the chord and the arc, added where the wall is on the centre's side of the edge and taken off where it is
/// beyond.
fn plan_of(p: &Prism) -> f64 {
    let signed = geom::area(&p.ring);
    let Some((centre, _)) = p.curve else { return signed.abs() };
    let toward = signed.signum();
    let on_arc = geom::ring_edges(&p.ring).filter_map(|(a, b)| {
        let (ra, rb, chord) = (a.dist(centre), b.dist(centre), a.dist(b));
        if chord < 1e-6 || ra < 1e-6 || (ra - rb).abs() > 1e-4 {
            return None;
        }
        let turn = 2.0 * (chord / (2.0 * ra)).clamp(-1.0, 1.0).asin();
        let segment = ra * ra / 2.0 * (turn - turn.sin());
        Some(if (b - a).cross(centre - a) * toward > 0.0 { segment } else { -segment })
    });
    signed.abs() + on_arc.sum::<f64>()
}

/// What a floor measures.
#[derive(Clone, Debug, PartialEq)]
pub struct SlabQ {
    /// How thick it is: its type's, plumb.
    pub depth: f64,
    /// Round its outline.
    pub perimeter: f64,
    /// Its top's area (a sloped one's stretched by its slope, as Revit's), over its outline, and that less its openings and the
    /// shafts through it.
    pub gross_area: f64,
    pub net_area: f64,
    /// What it holds; under a variable layer, less where its top falls. A sloped or shaped one's is its plan through its
    /// depth plumb, as its underside follows its top.
    pub gross_volume: f64,
    pub net_volume: f64,
    /// Whether it is level, with one area to give.
    pub level: bool,
}

/// Floor `f`, measured.
pub fn floor(m: &Model, f: &Floor) -> SlabQ {
    let depth = m.floor_depth(f);
    let level = m.floor_top(f).g == P2(0.0, 0.0) && f.shape.is_empty();
    let outline = geom::area(&f.boundary).abs();
    // Its top over the whole outline, openings closed: its shape's triangles, else its one plane.
    let top = m.shape_triangles(f);
    let gross_area = if f.shape.is_empty() || top.is_empty() {
        outline * stretch(&m.floor_top(f))
    } else {
        top.iter().map(|(ring, plane)| geom::area(ring).abs() * stretch(plane)).sum()
    };
    let net_area: f64 = pieces(m, m.floor_prisms(f)).map(|(ring, _, top)| geom::area(&ring).abs() * stretch(&top)).sum();
    let gross_volume =
        if level { outline * depth } else { m.shape_solid_over(f, &[]).map_or(outline * depth, |s| crate::model::volume(&s)) };
    SlabQ { depth, perimeter: geom::perimeter(&f.boundary), gross_area, net_area, gross_volume, net_volume: m.floor_volume(f), level }
}

/// What a ceiling measures.
#[derive(Clone, Debug, PartialEq)]
pub struct CoveringQ {
    pub width: f64,
    pub perimeter: f64,
    pub gross_area: f64,
    pub net_area: f64,
    pub volume: f64,
}

/// Ceiling `c`, measured.
pub fn ceiling(m: &Model, c: &Ceiling) -> CoveringQ {
    let net_area: f64 = m.ceiling_slabs(c).iter().map(|(face, rise)| plan_area(&m.less_shafts(face, *rise).0)).sum();
    let width = m.types.ceiling.get(&c.type_name).map_or(0.0, |t| t.thickness);
    CoveringQ {
        width,
        perimeter: geom::perimeter(&c.boundary),
        gross_area: geom::area(&c.boundary).abs(),
        net_area,
        volume: net_area * width,
    }
}

/// What a roof measures.
#[derive(Clone, Debug, PartialEq)]
pub struct RoofQ {
    pub thickness: f64,
    pub perimeter: f64,
    /// What it covers in plan: its outline less its courtyards, its openings and the shafts through it.
    pub plan_area: f64,
    /// Its top surface (each slope's plan area stretched by the slope) before its openings and the shafts through it: its
    /// courtyards are not part of it.
    pub gross_surface_area: f64,
    /// The same less its openings and the shafts through it, as Revit's roof Area.
    pub surface_area: f64,
    /// What it holds: its solid's, less its openings and the shafts through it. A shaped flat roof's is its plan through its
    /// thickness plumb, not its surface through it.
    pub volume: f64,
}

/// Roof `r`, measured.
pub fn roof(m: &Model, r: &Roof) -> RoofQ {
    let outline = m.roof_outline(r);
    // A face's loops are its outline and then its holes, wound the other way: their areas, signed, add up.
    let gross_surface_area: f64 = m
        .roof_faces(r)
        .unwrap_or_default()
        .iter()
        .map(|(loops, plane)| loops.iter().map(|l| geom::area(l)).sum::<f64>().abs() * stretch(plane))
        .sum();
    let (mut plan_area, mut surface_area, mut volume) = (0.0, 0.0, 0.0);
    for (ring, bottom, top) in pieces(m, m.roof_prisms(r)) {
        let plan = geom::area(&ring).abs();
        let c = geom::centroid(&ring);
        plan_area += plan;
        surface_area += plan * stretch(&top);
        volume += plan * (top.at(c) - bottom.at(c)).max(0.0);
    }
    let thickness = m.types.roof.get(&r.type_name).map_or(0.0, |t| t.thickness);
    RoofQ { thickness, perimeter: geom::perimeter(&outline), plan_area, gross_surface_area, surface_area, volume }
}

/// A slab's prisms (each a ring in plan from its underside's plane up to its top's) with the shafts through it taken out: what is
/// left of each, as the rings and the planes they lie between.
fn pieces(m: &Model, prisms: Vec<(Vec<P2>, Plane, Plane)>) -> impl Iterator<Item = (Vec<P2>, Plane, Plane)> + '_ {
    prisms.into_iter().flat_map(move |(ring, bottom, top)| {
        let rise = ring.iter().map(|q| top.at(*q) - bottom.at(*q)).fold(0.0, f64::max);
        m.less_shafts(&bottom.lift(&ring), rise).0.into_iter().map(move |piece| (piece, bottom, top))
    })
}

/// How much a plane's surface exceeds its plan: 1 over `cos` of its slope.
fn stretch(plane: &Plane) -> f64 {
    (1.0 + plane.g.dot(plane.g)).sqrt()
}

/// The area of `rings` in plan.
fn plan_area(rings: &[Vec<P2>]) -> f64 {
    rings.iter().map(|r| geom::area(r).abs()).sum()
}

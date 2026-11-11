//! What walls, floors, ceilings and roofs measure: the one place schedules (Area, Volume, Perimeter), element JSON and IFC's
//! `Qto_*BaseQuantities` read their numbers from, so a schedule and an exported file never disagree. Lengths are in mm, areas in
//! mm², volumes in mm³; each reader converts.
//!
//! The rules, as Revit's: a wall's area is one side of it, less the doors and windows in it; its volume is that area through its
//! thickness (so, as its geometry, less its openings). A floor's, a ceiling's area is its outline less its openings and the shafts
//! through it, its volume that through its thickness; a roof's area is its surface (its plan area stretched by its slope), its
//! volume that through its thickness. Joins between walls take nothing off.

use std::collections::BTreeMap;

use crate::geom::{self, P2};
use crate::model::{Ceiling, Element, Floor, Id, Model, Roof, Wall};
use crate::walls::{self, Axis};

/// A door's or a window's opening in a wall: its width, its height and its sill height over the wall's base, mm.
pub type Hole = (f64, f64, f64);

/// What a wall measures.
#[derive(Clone, Debug, PartialEq)]
pub struct WallQ {
    /// Along its location line.
    pub length: f64,
    pub width: f64,
    /// Its height; none where it is attached to a roof or a floor, which makes it no one height.
    pub height: Option<f64>,
    /// One side, with and without the openings.
    pub gross_side: f64,
    pub net_side: f64,
    pub gross_volume: f64,
    pub net_volume: f64,
}

/// The doors and windows in each wall, by the wall's id.
pub fn all_holes(m: &Model) -> BTreeMap<Id, Vec<Hole>> {
    let mut holes: BTreeMap<Id, Vec<Hole>> = BTreeMap::new();
    for e in m.elements.values() {
        if let Element::Door(o) | Element::Window(o) = e
            && let (Some(t), Some(host)) = (m.opening_type(e), m.elements.id(o.host))
        {
            holes.entry(host).or_default().push((t.width, t.height, o.sill));
        }
    }
    holes
}

/// The doors and windows in wall `id`.
pub fn holes_in(m: &Model, id: Id) -> Vec<Hole> {
    let guid = m.guid(id);
    m.lookup()
        .referrers(&guid)
        .filter_map(|r| {
            let e = m.elements.get(r)?;
            match e {
                Element::Door(o) | Element::Window(o) if o.host == guid => {
                    let t = m.opening_type(e)?;
                    Some((t.width, t.height, o.sill))
                }
                _ => None,
            }
        })
        .collect()
}

/// Wall `id`, `wl`, measured with its `holes`. Attached to a roof or a floor its height varies along it: its volume is its
/// pieces' (their plan through how far their top lies over their base), its side that through its thickness, and each opening is
/// cut as far as the wall's mean height allows.
pub fn wall(m: &Model, id: Id, wl: &Wall, holes: &[Hole]) -> WallQ {
    let (length, width) = (Axis::of(wl).length(), m.thickness(wl));
    let attached = wl.attach_top.is_some() || wl.attach_base.is_some();
    let (height, gross_side, gross_volume) = if attached {
        let volume: f64 = walls::prisms(m, id, false)
            .iter()
            .map(|p| {
                let c = geom::centroid(&p.ring);
                geom::area(&p.ring).abs() * (p.top.at(c) - p.bottom.at(c)).max(0.0)
            })
            .sum();
        let side = if width > 0.0 { volume / width } else { 0.0 };
        (None, side, volume)
    } else {
        let height = m.top_z(wl) - m.base_z(wl);
        (Some(height), length * height, length * height * width)
    };
    let reach = height.unwrap_or(if length > 0.0 { gross_side / length } else { 0.0 });
    let cut: f64 = holes.iter().map(|&(wd, h, sill)| wd.min(length) * h.min(reach - sill).max(0.0)).sum();
    let net_side = (gross_side - cut).max(0.0);
    WallQ { length, width, height, gross_side, net_side, gross_volume, net_volume: net_side * width }
}

/// What a floor measures.
#[derive(Clone, Debug, PartialEq)]
pub struct SlabQ {
    /// How thick it is: its type's, plumb.
    pub depth: f64,
    /// Round its outline.
    pub perimeter: f64,
    /// Its outline's area, and that less its openings and the shafts through it.
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
    let gross_area = geom::area(&f.boundary).abs();
    let net_area: f64 = m.floor_slabs(f).iter().map(|(face, rise)| plan_area(&m.less_shafts(face, *rise).0)).sum();
    let gross_volume =
        if level { gross_area * depth } else { m.shape_solid_over(f, &[]).map_or(gross_area * depth, |s| crate::model::volume(&s)) };
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
    /// What it covers in plan, its courtyards left out.
    pub plan_area: f64,
    /// Its top surface: each slope's plan area stretched by the slope.
    pub surface_area: f64,
    pub volume: f64,
}

/// Roof `r`, measured.
pub fn roof(m: &Model, r: &Roof) -> RoofQ {
    let outline = m.roof_outline(r);
    let courtyards: f64 = r.courtyards.iter().map(|c| geom::area(c).abs()).sum();
    let plan_area = geom::area(&outline).abs() - courtyards;
    // A face's loops are its outline and then its holes, wound the other way: their areas, signed, add up.
    let surface_area: f64 = m
        .roof_faces(r)
        .unwrap_or_default()
        .iter()
        .map(|(loops, plane)| loops.iter().map(|l| geom::area(l)).sum::<f64>().abs() * (1.0 + plane.g.dot(plane.g)).sqrt())
        .sum();
    let thickness = m.types.roof.get(&r.type_name).map_or(0.0, |t| t.thickness);
    RoofQ { thickness, perimeter: geom::perimeter(&outline), plan_area, surface_area, volume: surface_area * thickness }
}

/// The area of `rings` in plan.
fn plan_area(rings: &[Vec<P2>]) -> f64 {
    rings.iter().map(|r| geom::area(r).abs()).sum()
}

//! One closed solid per element, per material layer.
//!
//! The scene's solids ([`Solid`]) are an element's pieces: a wall is a row of abutting prisms cut at its doors' and windows' jambs
//! and at its arc's chords, a holed floor is its triangles each extruded. [`element_solids`] makes of them what the element is:
//! the pieces of each layer of its type (a wall's finish, its core) unioned by Manifold ([`Solid3`], `bimcraft-geom::solid`) into
//! one closed solid, tagged with the element and the layer, and for a curved wall's or a round column's faces with the cylinders
//! they lie on, so a section cut through them shows true arcs and a quantity is the circle's.
//!
//! A piece that is not a solid (a triangle mesh with holes, a prism with no thickness) is left out of the union and recorded, with
//! why, in [`ElementSolids::open`], for whatever draws or measures the element to fall back on.
//!
//! The pieces are today's, as the 3D view draws them ([`solids_in_with`], [`layered_indexed`]); nothing about what that view, glTF
//! and IFC render changes. The design and the numbers are in `docs/manifold-elements.md`.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use super::{Solid, Vis, layered_indexed, solids_in_with};
use crate::component_geometry::Components;
use crate::geom::P2;
use crate::model::{Element, Id, Model};
use crate::prim::Rgb;
use crate::solid::{Curve, Level, Matrix, Origin, Solid3, SolidError, tol};
use crate::walls::{self, Axis, Index, Plane};

/// A piece of an element that is not in its solid, and why.
#[derive(Clone)]
pub struct OpenPiece {
    /// The layer it belongs to.
    pub layer: u16,
    /// Why it is not a solid: the mesh's open edges, a prism with no thickness.
    pub why: SolidError,
    /// The piece itself, as the scene holds it, for what falls back on it.
    pub piece: Solid,
}

impl std::fmt::Debug for OpenPiece {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenPiece").field("layer", &self.layer).field("why", &self.why).finish()
    }
}

/// An element as closed solids.
#[derive(Clone, Debug, Default)]
pub struct ElementSolids {
    /// One solid for each layer that has pieces, by the layer's place in its type (a component's, a door's: by material, in
    /// the order the materials first appear), every one closed. Each carries [`Origin`] `{ element, layer, curve }`.
    pub layers: Vec<(u16, Solid3)>,
    /// What is left out of them: the pieces that are not solids.
    pub open: Vec<OpenPiece>,
    /// Where some of its pieces are drawn only in some views (a family's forms' Visibility Settings: detail levels, front and back,
    /// left and right): its solids again, one for each layer and each way of being shown, for a section to cut only what it
    /// draws ([`ElementSolids::drawn_layers`]). Empty when every piece is drawn everywhere, which is every element but a family's.
    pub shown: Vec<ShownSolid>,
}

/// The pieces of one layer that a view draws alike, as one solid.
#[derive(Clone, Debug)]
pub struct ShownSolid {
    pub layer: u16,
    /// Which views draw it ([`Vis::drawn`]).
    pub vis: Vis,
    pub solid: Solid3,
}

impl ElementSolids {
    /// What the element holds, mm³: the layers' volumes, a curved face taken as the cylinder it lies on
    /// ([`Solid3::volume_on_curves`]), and what each open piece holds the way the prisms were summed before the solids
    /// ([`crate::quantities::piece_volume`]): the pieces the solid kernel refuses are in the quantity all the same.
    pub fn volume(&self) -> f64 {
        self.layers.iter().map(|(_, s)| s.volume_on_curves()).sum::<f64>()
            + self.open.iter().map(|o| crate::quantities::piece_volume(&o.piece)).sum::<f64>()
    }

    /// The solid of layer `layer`.
    pub fn layer(&self, layer: u16) -> Option<&Solid3> {
        self.layers.iter().find(|(k, _)| *k == layer).map(|(_, s)| s)
    }

    /// Whether the element is all solid: no piece left open.
    pub fn is_whole(&self) -> bool {
        self.open.is_empty()
    }

    /// What a view draws of each layer, for a section to cut: the layers' own solids, or where the pieces are drawn in some
    /// views only, the pieces `drawn` says the view draws, united (a layer with none left is left out). Pieces that overlap
    /// across ways of being shown are cut as the one solid they make.
    pub fn drawn_layers(&self, drawn: &dyn Fn(Vis) -> bool) -> Vec<(u16, Solid3)> {
        if self.shown.is_empty() {
            return self.layers.clone();
        }
        let mut out: Vec<(u16, Vec<&Solid3>)> = Vec::new();
        for s in self.shown.iter().filter(|s| drawn(s.vis)) {
            match out.iter_mut().find(|(k, _)| *k == s.layer) {
                Some((_, v)) => v.push(&s.solid),
                None => out.push((s.layer, vec![&s.solid])),
            }
        }
        out.into_iter().map(|(k, v)| (k, if let [one] = v.as_slice() { (*one).clone() } else { Solid3::union_all(v) })).collect()
    }
}

/// Element `id` as one closed solid per layer, its pieces as the 3D view makes them (the wall index and families' geometry
/// found as one element needs them). Nothing for an id that is none.
pub fn element_solids(m: &Model, id: Id) -> ElementSolids {
    element_solids_in(m, id, None, &mut Components::new(m))
}

/// [`element_solids`], with the wall `index` and the families' geometry `components` a caller making many elements shares.
pub fn element_solids_in(m: &Model, id: Id, index: Option<&Index>, components: &mut Components) -> ElementSolids {
    build(m, id, index, components, true)
}

/// Wall `id`'s volume with its doors and windows cut out (`openings`) or without them, mm³: its layers' solids, a curved wall's
/// faces taken as arcs. None for an id that is no wall.
pub fn wall_volume(m: &Model, id: Id, openings: bool) -> Option<f64> {
    m.wall(id)?;
    Some(build(m, id, None, &mut Components::new(m), openings).volume())
}

/// The solids and open pieces of `id`.
fn build(m: &Model, id: Id, index: Option<&Index>, components: &mut Components, openings: bool) -> ElementSolids {
    let Some(e) = m.elements.get(id) else { return ElementSolids::default() };
    let pieces: Vec<Solid> = match e {
        // A wall without its openings is the one piece the scene never draws: its prisms alone.
        Element::Wall(_) if !openings => walls::prisms_in(m, id, false, index)
            .into_iter()
            .map(|p| Solid::Prism { owner: id, color: super::WALL, tex: 0, ring: p.ring, bottom: p.bottom, top: p.top, curve: p.curve })
            .collect(),
        _ => solids_in_with(m, &[(id, e)], index, components),
    };
    let mut by_layer: BTreeMap<u16, Vec<(Vis, Solid3)>> = BTreeMap::new();
    let mut open = Vec::new();
    // Where an element's type has no layers (a component, a door), the pieces of one material are one layer.
    let mut paints: Vec<(Rgb, u32)> = Vec::new();
    for piece in pieces {
        for (k, part) in layered_indexed(m, piece) {
            let layer = match k {
                Some(k) => k as u16,
                None => {
                    let paint = paint_of(&part);
                    let at = paints.iter().position(|p| *p == paint).unwrap_or_else(|| {
                        paints.push(paint);
                        paints.len() - 1
                    });
                    at as u16
                }
            };
            let mut origin = Origin::new(id, layer);
            origin.curve = curve_of(m, e, &part, k);
            match solid_of(&part, origin) {
                Ok(Some(solid)) => by_layer.entry(layer).or_default().push((vis_of(&part), solid)),
                // No volume: a flat sliver of a wall where an opening reaches the base or the top, left in the scene's pieces.
                Ok(None) => {}
                Err(why) => open.push(OpenPiece { layer, why, piece: part }),
            }
        }
    }
    // Where a piece is drawn in some views only, the layers again by the way they are shown.
    let apart = by_layer.values().flatten().any(|(v, _)| *v != Vis::ALWAYS);
    let mut shown = Vec::new();
    let mut layers = Vec::new();
    for (layer, pieces) in by_layer {
        let whole = if let [(_, only)] = pieces.as_slice() { only.clone() } else { Solid3::union_all(pieces.iter().map(|(_, s)| s)) };
        if whole.is_empty() {
            continue;
        }
        if apart {
            let mut ways: Vec<(Vis, Vec<&Solid3>)> = Vec::new();
            for (vis, solid) in &pieces {
                match ways.iter_mut().find(|(v, _)| v == vis) {
                    Some((_, v)) => v.push(solid),
                    None => ways.push((*vis, vec![solid])),
                }
            }
            for (vis, v) in &ways {
                // A layer drawn one way throughout is its own solid again.
                let solid = if ways.len() == 1 { whole.clone() } else { Solid3::union_all(v.iter().copied()) };
                if !solid.is_empty() {
                    shown.push(ShownSolid { layer, vis: *vis, solid });
                }
            }
        }
        layers.push((layer, whole));
    }
    ElementSolids { layers, open, shown }
}

/// Which views draw `s`: its form's Visibility Settings, every view for what says nothing.
fn vis_of(s: &Solid) -> Vis {
    match s {
        Solid::Mesh { vis, .. } => *vis,
        Solid::Box { .. } | Solid::Slab { .. } | Solid::Prism { .. } => Vis::ALWAYS,
    }
}

fn paint_of(s: &Solid) -> (Rgb, u32) {
    match s {
        Solid::Box { color, tex, .. }
        | Solid::Slab { color, tex, .. }
        | Solid::Prism { color, tex, .. }
        | Solid::Mesh { color, tex, .. } => (*color, *tex),
    }
}

fn level_of(p: &Plane) -> Level {
    Level { z: p.z, g: p.g }
}

/// One piece as a solid of `origin`: a box, a slab (its face, level or sloped, and what lies up to `rise` over it), a prism or a
/// mesh (which must be closed). None for a piece that holds nothing: a prism or slab no thick anywhere (a wall's piece over an
/// opening that reaches its base is one, between the two planes that meet).
fn solid_of(s: &Solid, origin: Origin) -> Result<Option<Solid3>, SolidError> {
    let thin = match s {
        Solid::Slab { rise, .. } => *rise <= tol::THICKNESS,
        Solid::Prism { ring, bottom, top, .. } => ring.iter().all(|q| top.at(*q) - bottom.at(*q) <= tol::THICKNESS),
        Solid::Box { .. } | Solid::Mesh { .. } => false,
    };
    if thin {
        return Ok(None);
    }
    solid_in(s, origin).map(Some)
}

fn solid_in(s: &Solid, origin: Origin) -> Result<Solid3, SolidError> {
    match s {
        Solid::Box { frame, a, c, z, .. } => {
            let (o, u, n) = (frame.o, frame.u, frame.n);
            // The box's own axes (along, across, up) put in the plan.
            let to_plan = Matrix([[u.0, n.0, 0.0, o.0], [u.1, n.1, 0.0, o.1], [0.0, 0.0, 1.0, 0.0]]);
            Solid3::cuboid_transformed([a.0, c.0, z.0], [a.1, c.1, z.1], &to_plan, origin)
        }
        Solid::Slab { face, rise, .. } => {
            let ring: Vec<P2> = face.iter().map(|p| P2(p[0], p[1])).collect();
            let level = match face.first() {
                Some(first) if face.iter().all(|p| (p[2] - first[2]).abs() < 1e-9) => Level::flat(first[2]),
                _ => level_of(&Plane::of(face).ok_or_else(|| SolidError::Degenerate("its face is upright".into()))?),
            };
            Solid3::prism(&ring, level, Level { z: level.z + rise, g: level.g }, origin)
        }
        Solid::Prism { ring, bottom, top, .. } => Solid3::prism(ring, level_of(bottom), level_of(top), origin),
        Solid::Mesh { tris, .. } => Solid3::from_triangles(tris, origin),
    }
}

/// The cylinders `part`'s curved faces lie on: a curved wall layer's two faces, a round column's.
fn curve_of(m: &Model, e: &Element, part: &Solid, layer: Option<usize>) -> Option<Curve> {
    match (e, part) {
        (Element::Wall(w), Solid::Prism { curve: Some(_), .. }) => {
            let (centre, r, _, sweep) = Axis::of(w).arc()?;
            let b = walls::boundaries(m, w);
            let k = layer.unwrap_or(0);
            let (r0, r1) = (r - sweep.signum() * b.get(k)?, r - sweep.signum() * b.get(k + 1)?);
            Some(Curve::band(centre, r0.min(r1), r0.max(r1)))
        }
        (Element::Column(col) | Element::StructuralColumn(col) | Element::Footing(col), Solid::Slab { .. }) => {
            let t = m.column_type(e).filter(|t| t.round)?;
            Some(Curve::cylinder(col.at, t.width / 2.0))
        }
        _ => None,
    }
}

/// The element solids of a scene, made when first asked for and kept until the scene's edit touches the element (the scene
/// makes the triangles of just those again, `Scene::update`; the cache drops just those).
#[derive(Default)]
pub struct SolidCache {
    entries: Mutex<HashMap<Id, Arc<ElementSolids>>>,
    /// The walls' index of the model the scene is of, made when first needed: a wall's joins are found through it, not by
    /// asking the model's lookup for each end.
    index: OnceLock<Index>,
}

impl SolidCache {
    /// Element `id`'s solids: those kept, or made now from `m` (which must be the model the scene is of) and kept.
    pub fn get(&self, m: &Model, id: Id) -> Arc<ElementSolids> {
        if let Some(kept) = self.entries.lock().ok().and_then(|e| e.get(&id).cloned()) {
            return kept;
        }
        let made = Arc::new(element_solids_in(m, id, Some(self.index.get_or_init(|| Index::new(m))), &mut Components::new(m)));
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(id, Arc::clone(&made));
        }
        made
    }

    /// How many elements it holds.
    pub fn len(&self) -> usize {
        self.entries.lock().map_or(0, |e| e.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every element it holds, with its solids.
    pub(crate) fn entries(&self) -> Vec<(Id, Arc<ElementSolids>)> {
        self.entries.lock().map(|e| e.iter().map(|(id, s)| (*id, Arc::clone(s))).collect()).unwrap_or_default()
    }

    /// Keeps `solids` as element `id`'s (made from the same pieces, in the same library, as it would be now).
    pub(crate) fn adopt(&self, id: Id, solids: Arc<ElementSolids>) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(id, solids);
        }
    }

    /// What it holds of the elements `keep` says are unchanged.
    #[doc(hidden)]
    pub fn kept(&self, keep: impl Fn(Id) -> bool) -> SolidCache {
        let entries = self.entries.lock().map(|e| e.iter().filter(|(id, _)| keep(**id)).map(|(id, s)| (*id, Arc::clone(s))).collect());
        // The index is of the model the scene was of: the new scene makes its own.
        SolidCache { entries: Mutex::new(entries.unwrap_or_default()), index: OnceLock::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom;

    /// A prism over `ring` from 0 to `rise`.
    fn prism(ring: &[P2], rise: f64) -> Solid {
        Solid::Prism {
            owner: 1,
            color: super::super::WALL,
            tex: 0,
            ring: ring.to_vec(),
            bottom: Plane::flat(0.0),
            top: Plane::flat(rise),
            curve: None,
        }
    }

    #[test]
    fn a_piece_the_kernel_refuses_is_still_in_the_volume() {
        // A ring that crosses itself, which the kernel refuses, beside a plain block it takes.
        let ring = [P2(0.0, 0.0), P2(2000.0, 0.0), P2(0.0, 1000.0), P2(2000.0, 1000.0), P2(1000.0, 2500.0)];
        let (bad, block) = (prism(&ring, 100.0), prism(&[P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 500.0), P2(0.0, 500.0)], 100.0));
        let (origin, other) = (Origin::new(1, 0), Origin::new(1, 1));
        let why = solid_of(&bad, other).unwrap_err();
        let solid = solid_of(&block, origin).unwrap().unwrap();
        let es = ElementSolids { layers: vec![(0, solid)], open: vec![OpenPiece { layer: 1, why, piece: bad }], shown: Vec::new() };
        assert!(!es.is_whole());
        let (kept, refused) = (1000.0 * 500.0 * 100.0, geom::area(&ring).abs() * 100.0);
        assert!(refused > 0.0);
        let v = es.volume();
        assert!((v - (kept + refused)).abs() < 1e-6 * v, "{v} is not the block's {kept} and the refused ring's {refused}");
        // The same for each kind of piece that can be left open.
        let slab = Solid::Slab {
            owner: 1,
            color: super::super::WALL,
            tex: 0,
            face: vec![[0.0, 0.0, 0.0], [1000.0, 0.0, 0.0], [1000.0, 1000.0, 0.0], [0.0, 1000.0, 0.0]],
            rise: 50.0,
        };
        assert!((crate::quantities::piece_volume(&slab) - 5e7).abs() < 1e-6);
    }
}

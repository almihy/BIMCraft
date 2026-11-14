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

use super::{Solid, layered_indexed, solids_in_with};
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
}

impl ElementSolids {
    /// What the element holds, mm³: the layers' volumes, a curved face taken as the cylinder it lies on
    /// ([`Solid3::volume_on_curves`]). The open pieces are not in it.
    pub fn volume(&self) -> f64 {
        self.layers.iter().map(|(_, s)| s.volume_on_curves()).sum()
    }

    /// The solid of layer `layer`.
    pub fn layer(&self, layer: u16) -> Option<&Solid3> {
        self.layers.iter().find(|(k, _)| *k == layer).map(|(_, s)| s)
    }

    /// Whether the element is all solid: no piece left open.
    pub fn is_whole(&self) -> bool {
        self.open.is_empty()
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
            .map(|p| Solid::Prism {
                owner: id,
                color: super::WALL,
                tex: 0,
                ring: p.ring,
                bottom: p.bottom,
                top: p.top,
                curve: p.curve,
            })
            .collect(),
        _ => solids_in_with(m, &[(id, e)], index, components),
    };
    let mut by_layer: BTreeMap<u16, Vec<Solid3>> = BTreeMap::new();
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
                Ok(Some(solid)) => by_layer.entry(layer).or_default().push(solid),
                // No volume: a flat sliver of a wall where an opening reaches the base or the top, left in the scene's pieces.
                Ok(None) => {}
                Err(why) => open.push(OpenPiece { layer, why, piece: part }),
            }
        }
    }
    let layers = by_layer
        .into_iter()
        .filter_map(|(layer, mut solids)| {
            let solid = if solids.len() == 1 { solids.pop()? } else { Solid3::union_all(&solids) };
            (!solid.is_empty()).then_some((layer, solid))
        })
        .collect();
    ElementSolids { layers, open }
}

fn paint_of(s: &Solid) -> (Rgb, u32) {
    match s {
        Solid::Box { color, tex, .. } | Solid::Slab { color, tex, .. } | Solid::Prism { color, tex, .. } | Solid::Mesh { color, tex, .. } => {
            (*color, *tex)
        }
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

    /// What it holds of the elements `keep` says are unchanged.
    pub(super) fn kept(&self, keep: impl Fn(Id) -> bool) -> SolidCache {
        let entries = self.entries.lock().map(|e| e.iter().filter(|(id, _)| keep(**id)).map(|(id, s)| (*id, Arc::clone(s))).collect());
        // The index is of the model the scene was of: the new scene makes its own.
        SolidCache { entries: Mutex::new(entries.unwrap_or_default()), index: OnceLock::new() }
    }
}

//! Closed 3D solids and the booleans between them, on Manifold (`manifold-csg`, bindings to Manifold 3.5.5): the one doorway to
//! it. **This module and its children are the only code in the engine that names `manifold_csg`.** Nothing outside may import a
//! Manifold type; everything is [`Solid3`], [`Origin`], [`Frame`], [`Loop`] and the plain arrays below. The design, the frames,
//! the tolerances and what Manifold cannot do are in `docs/manifold-solid.md`.
//!
//! A [`Solid3`] is a closed solid. It is built from a plan ring between two planes ([`Solid3::prism`]), a box
//! ([`Solid3::cuboid`], [`Solid3::cuboid_transformed`]), a round column ([`Solid3::cylinder`]) or a triangle mesh
//! ([`Solid3::from_triangles`], refused with the reason when it is not closed). Every constructor takes an [`Origin`]: the element
//! the solid is of, its layer, and where its curved faces lie. The origin survives any chain of booleans: every output triangle
//! ([`Solid3::triangles`]) and every edge of a section ([`Solid3::slice`]) says which input it came from, and where that
//! input was curved the section's edges come back as exact arcs.
//!
//! Millimetres throughout, x east, y north, z up, as the rest of the engine.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use manifold_csg::{BoundingBox, CsgError, Manifold, MeshGL64, OpType};

use crate::geom::v3::P3;
use crate::geom::{self, P2};

mod curved;
mod mesh;
mod slice;
#[cfg(test)]
mod tests;

pub use slice::{Frame, Loop, Seg};

/// Every tolerance of this module, in millimetres (angles in radians), in one place.
pub mod tol {
    /// Triangle-mesh vertices closer than this are one vertex ([`Solid3::from_triangles`](super::Solid3::from_triangles)).
    pub const WELD: f64 = 1e-3;
    /// A prism thinner than this at a corner of its ring is no thickness there: its top and bottom share the corner.
    pub const THICKNESS: f64 = 1e-6;
    /// A ring corner within this of the line through its neighbours is not a corner (dropped from a prism's ring), as is a
    /// corner closer than this to the one before it.
    pub const RING: f64 = 1e-7;
    /// A section's point within this of the line through its neighbours, of the same origin, is no corner: the two edges
    /// are one line.
    pub const LINE: f64 = 1e-6;
    /// A section's edge shorter than this is dropped (a plane through a vertex leaves one of no length).
    pub const ZERO_LENGTH: f64 = 1e-9;
    /// How far a curved face's vertex may be outside its tagged cylinder and still be on it (the faces are inscribed in the
    /// cylinder: their vertices are on it, to rounding).
    pub const CURVE: f64 = 1e-4;
    /// The most one facet of a curved face may turn, as the angle it subtends at the axis: a face turning more than this
    /// is a corner, not a curve.
    pub const MAX_FACET_ANGLE: f64 = std::f64::consts::FRAC_PI_6;
}

/// What a piece of a solid is of, so any triangle of it, and any edge of its section, can be traced back: the element, its
/// layer and where its curved faces lie. Carried through every boolean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Origin {
    /// The element it belongs to: the model's element id (geom knows no model; the caller fills it).
    pub element: u64,
    /// Which of the element's layers (a wall's core, its finish), counted from the first.
    pub layer: u16,
    /// Where the input's curved faces lie, when it has any a section should show as arcs.
    pub curve: Option<Curve>,
}

impl Origin {
    /// A triangle whose input is not known: never made by this module; what a caller gets from a Manifold that lost it.
    pub const NONE: Origin = Origin { element: u64::MAX, layer: u16::MAX, curve: None };

    pub fn new(element: u64, layer: u16) -> Origin {
        Origin { element, layer, curve: None }
    }

    /// With its curved faces on `curve`.
    pub fn with_curve(self, curve: Curve) -> Origin {
        Origin { curve: Some(curve), ..self }
    }
}

/// "This input's curved faces lie on vertical cylinders about `centre` (in plan) of these `radii`": a curved wall's inside and
/// outside, a round column's one. The faces are facets whose vertices are on the cylinders; a section by a horizontal plane
/// then shows them as arcs of exactly these radii about exactly this centre (the facets' ends cut by something else are put
/// on the circle). A section by any other plane shows lines: a vertical plane cuts a cylinder in lines, a slanted one in an
/// ellipse, which is not recovered.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub centre: P2,
    /// The cylinders' radii; the same twice for one.
    pub radii: [f64; 2],
}

impl Curve {
    /// One cylinder: a round column's, or a curve a wall has one face of.
    pub fn cylinder(centre: P2, radius: f64) -> Curve {
        Curve { centre, radii: [radius, radius] }
    }

    /// A curved wall's two faces.
    pub fn band(centre: P2, inner: f64, outer: f64) -> Curve {
        Curve { centre, radii: [inner, outer] }
    }
}

/// A plane over the plan: its height at `q` is `z + g · q`. Sloped when `g` is not zero (the engine's `Plane` in walls.rs, which
/// the model converts from).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub z: f64,
    pub g: P2,
}

impl Level {
    pub fn flat(z: f64) -> Level {
        Level { z, g: P2(0.0, 0.0) }
    }

    pub fn at(&self, q: P2) -> f64 {
        self.z + self.g.dot(q)
    }
}

/// An affine map of space: row `i` gives output coordinate `i` as `m[i][0] x + m[i][1] y + m[i][2] z + m[i][3]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [[f64; 4]; 3]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]);

    pub fn translation(t: P3) -> Matrix {
        Matrix([[1.0, 0.0, 0.0, t[0]], [0.0, 1.0, 0.0, t[1]], [0.0, 0.0, 1.0, t[2]]])
    }

    /// Turned counter-clockwise about the vertical axis through the origin.
    pub fn rotation_z(angle: f64) -> Matrix {
        let (s, c) = angle.sin_cos();
        Matrix([[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]])
    }

    pub fn scale(s: P3) -> Matrix {
        Matrix([[s[0], 0.0, 0.0, 0.0], [0.0, s[1], 0.0, 0.0], [0.0, 0.0, s[2], 0.0]])
    }

    /// `self` first, then `next`.
    pub fn then(&self, next: &Matrix) -> Matrix {
        let (a, b) = (&self.0, &next.0);
        let mut m = [[0.0; 4]; 3];
        for (i, row) in m.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = b[i][0] * a[0][j] + b[i][1] * a[1][j] + b[i][2] * a[2][j] + if j == 3 { b[i][3] } else { 0.0 };
            }
        }
        Matrix(m)
    }

    pub fn apply(&self, p: P3) -> P3 {
        let m = &self.0;
        [0, 1, 2].map(|i| m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3])
    }

    /// Manifold's layout: four columns of three, column-major.
    fn manifold(&self) -> [f64; 12] {
        let m = &self.0;
        [m[0][0], m[1][0], m[2][0], m[0][1], m[1][1], m[2][1], m[0][2], m[1][2], m[2][2], m[0][3], m[1][3], m[2][3]]
    }

    /// What a vertical cylinder `curve` becomes under this map, when it stays one: the vertical axis stays vertical and the
    /// plan is only turned, mirrored, scaled alike and moved.
    fn map_curve(&self, curve: &Curve) -> Option<Curve> {
        let m = &self.0;
        let (a, b, c, d) = (m[0][0], m[0][1], m[1][0], m[1][1]);
        let scale2 = a * a + c * c;
        let near = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (1.0 + x.abs().max(y.abs()));
        let vertical = m[2][0] == 0.0 && m[2][1] == 0.0 && m[0][2] == 0.0 && m[1][2] == 0.0;
        let similar = near(scale2, b * b + d * d) && (a * b + c * d).abs() <= 1e-9 * (1.0 + scale2);
        (vertical && similar && scale2 > 0.0).then(|| {
            let k = scale2.sqrt();
            let q = curve.centre;
            Curve { centre: P2(a * q.0 + b * q.1 + m[0][3], c * q.0 + d * q.1 + m[1][3]), radii: curve.radii.map(|r| r * k) }
        })
    }
}

/// Why a solid could not be made.
#[derive(Clone, Debug, PartialEq)]
pub enum SolidError {
    /// The input has no volume or is not valid: too few corners, a ring that crosses itself, a top below its bottom, a
    /// coordinate that is not a number.
    Degenerate(String),
    /// A triangle mesh that is not a closed solid.
    NotClosed {
        /// Edges used by one triangle only (the mesh has a hole there).
        open_edges: usize,
        /// Edges used by more than two triangles, or twice in the same direction (two triangles turned the same way).
        non_manifold_edges: usize,
        /// Triangles read, after welding and dropping those with a repeated corner.
        triangles: usize,
    },
    /// Manifold refused it.
    Manifold(String),
}

impl std::fmt::Display for SolidError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SolidError::Degenerate(why) => write!(f, "not a solid: {why}"),
            SolidError::NotClosed { open_edges, non_manifold_edges, triangles } => {
                write!(f, "the mesh is not a closed solid ({triangles} triangles): ")?;
                let mut why = Vec::new();
                if *open_edges > 0 {
                    why.push(format!("{open_edges} open edge(s), used by one triangle only"));
                }
                if *non_manifold_edges > 0 {
                    why.push(format!("{non_manifold_edges} edge(s) used by more than two triangles or by two turned the same way"));
                }
                write!(f, "{}", why.join("; "))
            }
            SolidError::Manifold(why) => write!(f, "refused by the solid kernel: {why}"),
        }
    }
}

impl std::error::Error for SolidError {}

impl From<CsgError> for SolidError {
    fn from(e: CsgError) -> SolidError {
        SolidError::Manifold(e.to_string())
    }
}

/// An axis-aligned box in space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds3 {
    pub min: P3,
    pub max: P3,
}

/// One triangle of a solid's surface, counter-clockwise seen from outside.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tri {
    pub p: [P3; 3],
    /// Which input it comes from.
    pub origin: Origin,
    /// The unit normal, out of the solid (zero for a triangle of no area).
    pub normal: P3,
}

/// A solid's surface as indexed triangles: what a mesh format (glTF, IFC) wants.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub verts: Vec<P3>,
    /// Counter-clockwise from outside.
    pub tris: Vec<[u32; 3]>,
    /// Where each triangle came from, one per triangle.
    pub origins: Vec<Origin>,
}

/// A closed solid (or nothing), with where each of its parts came from.
#[derive(Clone)]
pub struct Solid3 {
    m: Manifold,
    /// Manifold's original id of each input -> its origin. Every boolean merges its operands' tables.
    origins: Arc<HashMap<u32, Origin>>,
    /// Its surface as indexed triangles, taken from Manifold (a full copy) the first time something asks, then kept: a plan, a
    /// section and an elevation slice the same solid again and again. Shared by the clones of this solid, which are the same
    /// manifold; every operation makes a new solid, with a new cell.
    surface: Arc<OnceLock<Mesh>>,
}

impl std::fmt::Debug for Solid3 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Solid3").field("triangles", &self.m.num_tri()).field("volume", &self.m.volume()).finish()
    }
}

fn finite3(p: P3) -> bool {
    p.iter().all(|c| c.is_finite() && c.abs() <= geom::MAX_COORD)
}

impl Solid3 {
    /// Nothing: the identity of union.
    pub fn empty() -> Solid3 {
        Solid3 { m: Manifold::empty(), origins: Arc::new(HashMap::new()), surface: Arc::default() }
    }

    /// A kernel solid made for `origin`: it gets its own original id, which the table maps to `origin`.
    fn from_manifold(m: Manifold, origin: Origin) -> Solid3 {
        let m = if m.original_id() < 0 { m.as_original() } else { m };
        let mut table = HashMap::new();
        table.insert(m.original_id() as u32, origin);
        Solid3 { m, origins: Arc::new(table), surface: Arc::default() }
    }

    /// A solid from a mesh of its own (indices into `verts`, counter-clockwise from outside).
    pub(crate) fn from_indexed(verts: &[P3], tris: &[[u32; 3]], origin: Origin) -> Result<Solid3, SolidError> {
        let props: Vec<f64> = verts.iter().flatten().copied().collect();
        let idx: Vec<u64> = tris.iter().flatten().map(|&i| u64::from(i)).collect();
        let g = MeshGL64::new(&props, 3, &idx)?;
        let m = Manifold::from_meshgl64(&g)?;
        if m.is_empty() {
            return Err(SolidError::Degenerate("it encloses no volume".into()));
        }
        Ok(Solid3::from_manifold(m, origin))
    }

    /// The box from `a` to `b` (either corner first), sides along the axes.
    pub fn cuboid(a: P3, b: P3, origin: Origin) -> Result<Solid3, SolidError> {
        if !finite3(a) || !finite3(b) {
            return Err(SolidError::Degenerate("a corner is not a number or is beyond 1000 km".into()));
        }
        let (lo, hi) = ([0, 1, 2].map(|i| a[i].min(b[i])), [0, 1, 2].map(|i| a[i].max(b[i])));
        let size = [0, 1, 2].map(|i| hi[i] - lo[i]);
        if size.iter().any(|s| *s <= 0.0) {
            return Err(SolidError::Degenerate("the box has no thickness along an axis".into()));
        }
        let m = Manifold::cube(size[0], size[1], size[2], false).translate(lo[0], lo[1], lo[2]);
        Ok(Solid3::from_manifold(m, origin))
    }

    /// The box from `a` to `b`, then moved by `matrix` (a turned box, a mirrored one).
    pub fn cuboid_transformed(a: P3, b: P3, matrix: &Matrix, origin: Origin) -> Result<Solid3, SolidError> {
        Ok(Solid3::cuboid(a, b, origin)?.transform(matrix))
    }

    /// The ring of `ring` (plan, either winding) from the plane `bottom` up to the plane `top`: a wall's part, a slab, a roof.
    /// Either plane may slope; `top` may meet `bottom` at corners (a wedge) but not cross it. The ring must not cross itself.
    pub fn prism(ring: &[P2], bottom: Level, top: Level, origin: Origin) -> Result<Solid3, SolidError> {
        if ring.iter().any(|q| !q.is_finite() || q.0.abs() > geom::MAX_COORD || q.1.abs() > geom::MAX_COORD) {
            return Err(SolidError::Degenerate("a corner of the ring is not a number or is beyond 1000 km".into()));
        }
        let level_ok = |l: &Level| l.z.is_finite() && l.g.is_finite() && l.z.abs() <= geom::MAX_COORD;
        if !level_ok(&bottom) || !level_ok(&top) {
            return Err(SolidError::Degenerate("a plane is not a number".into()));
        }
        let ring = clean_ring(ring);
        if ring.len() < 3 {
            return Err(SolidError::Degenerate("the ring has fewer than three corners".into()));
        }
        let area = geom::area(&ring);
        let ring: Vec<P2> = if area < 0.0 { ring.into_iter().rev().collect() } else { ring };
        let tris = geom::triangulate(&ring);
        let covered: f64 = tris.iter().map(|[a, b, c]| geom::area(&[ring[*a], ring[*b], ring[*c]])).sum();
        if area.abs() < 1e-9 || (covered - area.abs()).abs() > 1e-6 * area.abs().max(1.0) {
            return Err(SolidError::Degenerate("the ring has no area or crosses itself".into()));
        }
        let n = ring.len();
        let mut thin = 0;
        // Bottom corners 0..n, top corners after them; a corner where the planes meet has one vertex.
        let mut verts: Vec<P3> = Vec::with_capacity(2 * n);
        let mut top_of: Vec<u32> = Vec::with_capacity(n);
        for q in &ring {
            verts.push([q.0, q.1, bottom.at(*q)]);
        }
        for (i, q) in ring.iter().enumerate() {
            let (zb, zt) = (bottom.at(*q), top.at(*q));
            if zt < zb - tol::THICKNESS {
                return Err(SolidError::Degenerate(format!("the top is below the bottom at ({:.3}, {:.3})", q.0, q.1)));
            }
            if zt - zb <= tol::THICKNESS {
                thin += 1;
                top_of.push(i as u32);
            } else {
                top_of.push(verts.len() as u32);
                verts.push([q.0, q.1, zt]);
            }
        }
        if thin == n {
            return Err(SolidError::Degenerate("the prism has no thickness".into()));
        }
        let mut idx: Vec<[u32; 3]> = Vec::new();
        for [a, b, c] in &tris {
            idx.push([top_of[*a], top_of[*b], top_of[*c]]);
            idx.push([*a as u32, *c as u32, *b as u32]);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            let (b0, b1, t0, t1) = (i as u32, j as u32, top_of[i], top_of[j]);
            idx.push([b0, b1, t1]);
            idx.push([b0, t1, t0]);
        }
        // Where the planes meet, the quad has fewer corners: its triangles with a repeated vertex are no triangles.
        idx.retain(|[a, b, c]| a != b && b != c && a != c);
        Solid3::from_indexed(&verts, &idx, origin)
    }

    /// A round column: `segments` facets inscribed in the circle (a vertex at angle 0), from `z0` to `z1`, tagged with that
    /// cylinder unless `origin` already carries a curve.
    pub fn cylinder(centre: P2, radius: f64, z0: f64, z1: f64, segments: usize, origin: Origin) -> Result<Solid3, SolidError> {
        if segments < 3 || radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return Err(SolidError::Degenerate("a cylinder needs a radius and at least three facets".into()));
        }
        let ring: Vec<P2> = (0..segments)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / segments as f64;
                P2(centre.0 + radius * a.cos(), centre.1 + radius * a.sin())
            })
            .collect();
        let origin = if origin.curve.is_some() { origin } else { origin.with_curve(Curve::cylinder(centre, radius)) };
        Solid3::prism(&ring, Level::flat(z0.min(z1)), Level::flat(z0.max(z1)), origin)
    }

    /// A closed triangle mesh (a family's form, a file's): corners within [`tol::WELD`] are welded, triangles with a
    /// repeated corner dropped, and one wound inside out turned over. A mesh that is not a closed solid, whose every edge
    /// is shared by exactly two triangles turned opposite ways, is refused with how many open and how many non-manifold
    /// edges it has. It never panics.
    pub fn from_triangles(tris: &[[P3; 3]], origin: Origin) -> Result<Solid3, SolidError> {
        let (verts, idx) = mesh::weld_closed(tris)?;
        Solid3::from_indexed(&verts, &idx, origin)
    }

    // ----- booleans ---------------------------------------------------------------------------------------------

    fn joined(&self, other: &Solid3, m: Manifold) -> Solid3 {
        Solid3 { m, origins: merge_tables([&self.origins, &other.origins]), surface: Arc::default() }
    }

    /// Both: what is in either.
    pub fn union(&self, other: &Solid3) -> Solid3 {
        self.joined(other, self.m.union(&other.m))
    }

    /// What is in `self` and not in `other`.
    pub fn difference(&self, other: &Solid3) -> Solid3 {
        self.joined(other, self.m.difference(&other.m))
    }

    /// What is in both.
    pub fn intersection(&self, other: &Solid3) -> Solid3 {
        self.joined(other, self.m.intersection(&other.m))
    }

    fn batch<'a>(items: impl IntoIterator<Item = &'a Solid3>, op: OpType) -> Solid3 {
        let items: Vec<&Solid3> = items.into_iter().collect();
        let ms: Vec<Manifold> = items.iter().map(|s| s.m.clone()).collect();
        Solid3 { m: Manifold::batch_boolean(&ms, op), origins: merge_tables(items.iter().map(|s| &s.origins)), surface: Arc::default() }
    }

    /// Everything in any of them, in one operation (far faster than one pair at a time). Nothing for none.
    pub fn union_all<'a>(items: impl IntoIterator<Item = &'a Solid3>) -> Solid3 {
        Solid3::batch(items, OpType::Add)
    }

    /// What is in all of them, in one operation. Nothing for none.
    pub fn intersection_all<'a>(items: impl IntoIterator<Item = &'a Solid3>) -> Solid3 {
        Solid3::batch(items, OpType::Intersect)
    }

    /// `self` less every one of `cutters`, in one operation.
    pub fn difference_all<'a>(&'a self, cutters: impl IntoIterator<Item = &'a Solid3>) -> Solid3 {
        Solid3::batch(std::iter::once(self).chain(cutters), OpType::Subtract)
    }

    // ----- moving -----------------------------------------------------------------------------------------------

    pub fn translate(&self, by: P3) -> Solid3 {
        self.transform(&Matrix::translation(by))
    }

    /// Moved by `matrix`. A curve tag follows when the map keeps vertical cylinders cylinders (a turn about the vertical, a
    /// move, a mirror, a uniform scale of the plan); under any other map the tag is dropped and its faces come back as
    /// lines.
    pub fn transform(&self, matrix: &Matrix) -> Solid3 {
        let table = self.origins.iter().map(|(id, o)| (*id, Origin { curve: o.curve.and_then(|c| matrix.map_curve(&c)), ..*o })).collect();
        Solid3 { m: self.m.transform(&matrix.manifold()), origins: Arc::new(table), surface: Arc::default() }
    }

    // ----- questions --------------------------------------------------------------------------------------------

    pub fn is_empty(&self) -> bool {
        self.m.is_empty()
    }

    /// Enclosed volume, mm³.
    pub fn volume(&self) -> f64 {
        self.m.volume()
    }

    /// Surface area, mm².
    pub fn surface_area(&self) -> f64 {
        self.m.surface_area()
    }

    pub fn triangle_count(&self) -> usize {
        self.m.num_tri()
    }

    /// The box around it; none for nothing.
    pub fn bounds(&self) -> Option<Bounds3> {
        let b: BoundingBox = self.m.bounding_box()?;
        Some(Bounds3 { min: b.min(), max: b.max() })
    }

    /// Whether its surface is a closed, consistently turned skin (every edge shared by two triangles, once each way) and
    /// the kernel finds it sound. Every solid this module makes is; this is for tests and for what comes from outside.
    pub fn is_closed(&self) -> bool {
        if !self.m.status().is_ok() {
            return false;
        }
        let mesh = self.surface();
        mesh.tris.is_empty() || mesh::edge_faults(&mesh.tris) == (0, 0)
    }

    /// The origin of an input by Manifold's id, for what this solid holds.
    fn origin_of(&self, id: u32) -> Origin {
        self.origins.get(&id).copied().unwrap_or(Origin::NONE)
    }

    /// The surface as indexed triangles, each with its origin: a copy of [`Solid3::surface`].
    pub fn mesh(&self) -> Mesh {
        self.surface().clone()
    }

    /// The surface as indexed triangles, each with its origin, made once for this solid (and its clones) and kept: what slicing,
    /// measuring and drawing a solid again and again read, without a copy of Manifold's mesh each time.
    pub fn surface(&self) -> &Mesh {
        self.surface.get_or_init(|| self.made_mesh())
    }

    fn made_mesh(&self) -> Mesh {
        let g = self.m.to_meshgl64();
        let np = g.num_prop().max(3);
        let props = g.vert_properties();
        let verts: Vec<P3> = props.chunks(np).map(|v| [v[0], v[1], v[2]]).collect();
        let tris: Vec<[u32; 3]> = g.tri_verts().chunks(3).map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).collect();
        let (ids, starts) = (g.run_original_id(), g.run_index());
        let mut origins = vec![Origin::NONE; tris.len()];
        if ids.is_empty() {
            // One run, no table of them: the solid's own id, if it still has one.
            let only = self.origin_of(self.m.original_id() as u32);
            origins.iter_mut().for_each(|o| *o = only);
        }
        for (r, id) in ids.iter().enumerate() {
            let from = starts.get(r).map_or(0, |s| (*s / 3) as usize);
            let to = starts.get(r + 1).map_or(tris.len(), |s| (*s / 3) as usize).min(tris.len());
            let origin = self.origin_of(*id);
            for o in origins.iter_mut().take(to).skip(from) {
                *o = origin;
            }
        }
        Mesh { verts, tris, origins }
    }

    /// The surface's triangles, each with its origin and its outward unit normal.
    pub fn triangles(&self) -> Vec<Tri> {
        let mesh = self.surface();
        mesh.tris
            .iter()
            .zip(&mesh.origins)
            .map(|(t, origin)| {
                let p = t.map(|i| mesh.verts[i as usize]);
                Tri { p, origin: *origin, normal: normal_of(&p) }
            })
            .collect()
    }
}

fn normal_of(p: &[P3; 3]) -> P3 {
    use crate::geom::v3::{cross, sub};
    let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 0.0 { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0; 3] }
}

/// The union of tables; the first's when there is one alone.
fn merge_tables<'a>(tables: impl IntoIterator<Item = &'a Arc<HashMap<u32, Origin>>>) -> Arc<HashMap<u32, Origin>> {
    let mut it = tables.into_iter();
    let Some(first) = it.next() else { return Arc::new(HashMap::new()) };
    let mut out: Option<HashMap<u32, Origin>> = None;
    for t in it {
        if Arc::ptr_eq(first, t) {
            continue;
        }
        let m = out.get_or_insert_with(|| (**first).clone());
        for (k, v) in t.iter() {
            m.entry(*k).or_insert(*v);
        }
    }
    out.map_or_else(|| first.clone(), Arc::new)
}

/// A ring without repeated corners (the closing one included), without corners on the line between their neighbours, and
/// without spikes: a corner where the ring goes out along a line and straight back along it, which holds no area and which the
/// kernel takes for a non-manifold edge (a wall's layer cut to its band where a butt joint's step lies along the band's edge
/// leaves one).
fn clean_ring(ring: &[P2]) -> Vec<P2> {
    let mut out: Vec<P2> = ring.to_vec();
    loop {
        // Repeated corners, the closing one included.
        out.dedup_by(|b, a| a.dist(*b) <= tol::RING);
        while out.len() > 1 && out.first().zip(out.last()).is_some_and(|(a, b)| a.dist(*b) <= tol::RING) {
            out.pop();
        }
        let n = out.len();
        if n < 3 {
            return out;
        }
        // A corner on the line between its neighbours goes, and so does a spike's tip, until none does.
        let loose = (0..n).find(|&k| {
            let (a, b, c) = (out[(k + n - 1) % n], out[k], out[(k + 1) % n]);
            let ac = c - a;
            let flat = ac.length() > tol::RING && (b - a).cross(ac).abs() / ac.length() <= tol::RING && (b - a).dot(c - b) > 0.0;
            // The way on, `c`, lies on the line it came by (`a` to `b`) and back along it.
            let spike =
                (b - a).length() > tol::RING && (b - a).cross(c - a).abs() / (b - a).length() <= tol::RING && (b - a).dot(c - b) < 0.0;
            flat || spike
        });
        match loose {
            Some(k) => {
                out.remove(k);
            }
            None => return out,
        }
    }
}

// ----- the trial, on the doorway ------------------------------------------------------------------------------------

/// What [`solid_trial`] makes: a box minus a cylinder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolidTrial {
    /// The solid's volume.
    pub volume: f64,
    /// Its triangles.
    pub triangles: usize,
}

/// A 100 x 100 x 100 box, centred on the origin, minus a cylinder through it (radius 25, 120 long, 64 sides): the volume
/// and the triangle count of what is left. Volume 1,000,000 - 100 x (64-gon of radius 25 = 1,960.34) = 803,966. Made through
/// the same doorway as everything else (docs/manifold-trial.md: this is how the engine's wasm proves Manifold runs in it).
pub fn solid_trial() -> SolidTrial {
    let (Ok(block), Ok(hole)) = (
        Solid3::cuboid([-50.0; 3], [50.0; 3], Origin::new(1, 0)),
        Solid3::cylinder(P2(0.0, 0.0), 25.0, -60.0, 60.0, 64, Origin::new(2, 0)),
    ) else {
        return SolidTrial { volume: f64::NAN, triangles: 0 };
    };
    let cut = block.difference(&hole);
    SolidTrial { volume: cut.volume(), triangles: cut.triangle_count() }
}

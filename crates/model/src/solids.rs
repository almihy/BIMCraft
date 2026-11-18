//! What each element is in 3D: its solids (walls with their doors and windows cut out, floor slabs and ceilings with the
//! shafts through them cut out, door and window panels, columns, stairs' steps, roofs' slopes) and the triangles they are
//! drawn and measured with, each with its look ([`Vis`]). The model's own queries ask them (an extent, the tops over a
//! point, a wall's volume); the 3D view, sections and elevations (`bimcraft-render`'s `view3d`) draw them.
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use crate::geom::{self, P2};
use crate::graphics::Look;
use crate::model::{Element, Id, Layer, Model};
use crate::paint;
use crate::prim::Rgb;
use crate::stairs;
use crate::walls::{self, Axis, Plane};

/// Shared CPU rendering workers, bounded so background work leaves input and I/O capacity.
#[doc(hidden)]
pub fn render_pool() -> Option<&'static rayon::ThreadPool> {
    static POOL: OnceLock<Option<rayon::ThreadPool>> = OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(1, usize::from).saturating_sub(1).clamp(1, 8);
        rayon::ThreadPoolBuilder::new().num_threads(n).thread_name(|n| format!("bimcraft-render-{n}")).build().ok()
    })
    .as_ref()
}

use crate::geom::v3::{P3, add, cross, dot, extremes, mul, sub, unit};

const WALL: Rgb = Rgb(236, 236, 232);
const SLAB: Rgb = Rgb(214, 214, 210);
const DOOR: Rgb = Rgb(198, 168, 128);
const GLASS: Rgb = Rgb(166, 200, 228);
const ROOF: Rgb = Rgb(176, 118, 96);
const CEILING: Rgb = Rgb(244, 242, 236);
const MASS: Rgb = Rgb(158, 190, 224);
/// The ground: a toposolid's earth.
const TERRAIN: Rgb = Rgb(150, 168, 118);
const RAIL: Rgb = Rgb(120, 122, 128);
/// A beam, a footing, a structural column made of nothing the library has.
#[doc(hidden)]
pub const STRUCTURE: Rgb = Rgb(182, 184, 190);
/// Steel, darker than the concrete round it.
const REBAR: Rgb = Rgb(92, 96, 104);
/// A component's form made of nothing the library has.
const COMPONENT: Rgb = Rgb(214, 204, 188);

/// A triangle of an element's surface; on a curved one, which (its element's, and which of its
/// curved surfaces: a wall's outside or inside, a component's by its first triangle's place in the
/// scene), so the facets read as one.
#[derive(Clone, Copy, Debug, PartialEq)]
#[doc(hidden)]
pub struct Tri {
    #[doc(hidden)]
    pub p: [P3; 3],
    #[doc(hidden)]
    pub owner: Id,
    #[doc(hidden)]
    pub color: Rgb,
    /// The skin of its material ([`skin`]): in the low 24 bits the texture over its colour, one more than where the material
    /// is in the library, 0 for none; in the high 8, how see-through the material is, percent.
    #[doc(hidden)]
    pub tex: u32,
    curve: Option<(Id, usize)>,
    /// Which views draw it, where its family's form says ([`Vis`]); every view, else.
    #[doc(hidden)]
    pub vis: Vis,
}

/// Which views draw a family's form, carried by its solid and its triangles: Revit's Visibility Settings (the detail levels,
/// and whether a view along the family's y, its front or back, or along its x, its left or right, draws it) with the way the
/// family's x axis runs in plan. [`Vis::ALWAYS`] for everything else, and for a form that says nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[doc(hidden)]
pub struct Vis {
    /// None for always; else bit 0 Coarse, 1 Medium, 2 Fine, 3 front, 4 side, and 7 set.
    bits: u8,
    /// The family's x axis in plan: its cosine and sine.
    x: [f32; 2],
}

impl Vis {
    pub const ALWAYS: Vis = Vis { bits: 0, x: [1.0, 0.0] };

    /// What `show` says, of a family whose x axis runs along `x` in plan.
    pub fn of(show: Option<crate::family::Show>, x: P2) -> Vis {
        let Some(s) = show else { return Vis::ALWAYS };
        let bit = |on: bool, k: u8| if on { 1u8 << k } else { 0 };
        let bits = bit(s.coarse, 0) | bit(s.medium, 1) | bit(s.fine, 2) | bit(s.front, 3) | bit(s.side, 4) | 0x80;
        let x = x.unit();
        Vis { bits, x: [x.0 as f32, x.1 as f32] }
    }

    /// Whether a view at detail level `detail` draws it, looking along `looking` in plan where it is a section or an
    /// elevation: along the family's x within 45° its side, else its front.
    pub fn drawn(self, detail: crate::graphics::Detail, looking: Option<P2>) -> bool {
        use crate::graphics::Detail;
        if self.bits == 0 {
            return true;
        }
        let level = match detail {
            Detail::Coarse => 0,
            Detail::Medium => 1,
            Detail::Fine => 2,
        };
        let way = match looking {
            None => true,
            Some(l) => {
                let l = l.unit();
                let side = (l.0 * f64::from(self.x[0]) + l.1 * f64::from(self.x[1])).abs() > std::f64::consts::FRAC_1_SQRT_2;
                self.bits & (1 << if side { 4 } else { 3 }) != 0
            }
        };
        self.bits & (1 << level) != 0 && way
    }
}

/// The bits of a skin that hold the texture.
const TEXTURE: u32 = 0x00ff_ffff;

impl Tri {
    /// The texture over its colour, 0 for none.
    #[doc(hidden)]
    pub fn texture(&self) -> u32 {
        self.tex & TEXTURE
    }

    /// How see-through its material makes it, percent.
    #[doc(hidden)]
    pub fn clearness(&self) -> u8 {
        (self.tex >> 24).min(100) as u8
    }

    /// How see-through it is in `look`: as much as its material, or the view makes it, whichever is more. A view's own
    /// override counts only where it changes the picture.
    pub(crate) fn alpha(&self, look: &Look) -> u8 {
        let view = if look.is_plain() { 0 } else { look.transparency(self.owner) };
        self.clearness().max(view)
    }
}

/// A wall's plan frame: its start, along it, and square to it.
#[derive(Clone, Copy, PartialEq)]
#[doc(hidden)]
pub struct Frame {
    o: P2,
    u: P2,
    n: P2,
}

impl Frame {
    /// A box from `a.0` to `a.1` along, `c.0` to `c.1` across and `z.0` to `z.1` up.
    fn cuboid(&self, out: &mut Vec<Tri>, owner: Id, (color, tex): (Rgb, u32), a: (f64, f64), c: (f64, f64), z: (f64, f64)) {
        let at = |a: f64, c: f64, z: f64| {
            let p = self.o + self.u * a + self.n * c;
            [p.0, p.1, z]
        };
        let v = [
            at(a.0, c.0, z.0),
            at(a.1, c.0, z.0),
            at(a.1, c.1, z.0),
            at(a.0, c.1, z.0),
            at(a.0, c.0, z.1),
            at(a.1, c.0, z.1),
            at(a.1, c.1, z.1),
            at(a.0, c.1, z.1),
        ];
        for [i, j, k, l] in [[0, 1, 2, 3], [4, 5, 6, 7], [0, 1, 5, 4], [1, 2, 6, 5], [2, 3, 7, 6], [3, 0, 4, 7]] {
            out.push(Tri { p: [v[i], v[j], v[k]], owner, color, tex, curve: None, vis: Vis::ALWAYS });
            out.push(Tri { p: [v[i], v[k], v[l]], owner, color, tex, curve: None, vis: Vis::ALWAYS });
        }
    }
}

/// A solid of the model: a box (a door's leaf, a window's glass, a stair's step); a slab: a flat
/// face, level or sloped, and what lies up to `rise` mm above it (a floor, a column, a roof's
/// slope); a prism: a ring in plan from one plane up to another (a wall's part; of a curved wall,
/// with its centre and its centre line's radius, its faces being one surface each); or triangles
/// (a component's form, which a section cuts as Revit's category of the family does ([`crate::graphics::cuttable`]) and the
/// element solids make of it; a round run's tube, convex, which they cut by its `cut`): a round
/// one's sides, past `caps`, are one curved surface; any other's facets that turn little where they meet are ([`facets`]).
#[derive(Clone, PartialEq)]
#[doc(hidden)]
pub enum Solid {
    Box { owner: Id, color: Rgb, tex: u32, frame: Frame, a: (f64, f64), c: (f64, f64), z: (f64, f64) },
    Slab { owner: Id, color: Rgb, tex: u32, face: Vec<P3>, rise: f64 },
    Prism { owner: Id, color: Rgb, tex: u32, ring: Vec<P2>, bottom: Plane, top: Plane, curve: Option<(P2, f64)> },
    Mesh { owner: Id, color: Rgb, tex: u32, tris: Vec<[P3; 3]>, caps: usize, round: bool, cut: bool, vis: Vis },
}

/// Retained geometry with per-snapshot appearance. Copying prepared work shares
/// triangle/outline storage; applying paint happens only when making a scene.
#[derive(Clone)]
#[doc(hidden)]
pub enum Prepared {
    Owned(Solid),
    Shared { geometry: Arc<Solid>, paint: Option<(Rgb, u32)> },
}

impl From<Solid> for Prepared {
    fn from(solid: Solid) -> Self {
        Self::Owned(solid)
    }
}

impl std::ops::Deref for Prepared {
    type Target = Solid;
    fn deref(&self) -> &Solid {
        match self {
            Self::Owned(solid) => solid,
            Self::Shared { geometry, .. } => geometry,
        }
    }
}

impl Prepared {
    #[doc(hidden)]
    pub fn painted(&self) -> Solid {
        let mut solid = (**self).clone();
        if let Self::Shared { paint: Some((color, tex)), .. } = self {
            match &mut solid {
                Solid::Box { color: c, tex: t, .. }
                | Solid::Slab { color: c, tex: t, .. }
                | Solid::Prism { color: c, tex: t, .. }
                | Solid::Mesh { color: c, tex: t, .. } => (*c, *t) = (*color, *tex),
            }
        }
        solid
    }
}

/// Preserve the uncached renderer's original contiguous storage. Cached preparation
/// may mix owned and shared geometry without converting the full oracle's buffer.
#[doc(hidden)]
pub enum PreparedSolids {
    Owned(Vec<Solid>),
    Cached(Vec<Prepared>),
}

impl PreparedSolids {
    fn slices(&self, range: std::ops::Range<usize>) -> (&[Solid], &[Prepared]) {
        match self {
            Self::Owned(solids) => (solids.get(range).unwrap_or_default(), &[]),
            Self::Cached(solids) => (&[], solids.get(range).unwrap_or_default()),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Owned(solids) => solids.len(),
            Self::Cached(solids) => solids.len(),
        }
    }

    #[doc(hidden)]
    pub fn iter(&self) -> impl Iterator<Item = &Solid> {
        self.in_range(0..self.len())
    }

    pub(crate) fn in_range(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = &Solid> {
        let (owned, cached) = self.slices(range);
        owned.iter().chain(cached.iter().map(|s| &**s))
    }

    #[doc(hidden)]

    pub fn painted(&self, range: std::ops::Range<usize>) -> impl Iterator<Item = Solid> {
        let (owned, cached) = self.slices(range);
        owned.iter().cloned().chain(cached.iter().map(Prepared::painted))
    }
}

mod element_solids;
pub use element_solids::{ElementSolids, OpenPiece, ShownSolid, SolidCache, element_solids, element_solids_in, wall_volume};
#[doc(hidden)]
pub type PreparedMade = (Vec<Prepared>, Vec<(Id, Vec<P3>)>, Vec<Option<[P3; 2]>>);

/// A level slab: `ring` from `z.0` up to `z.1`.
fn slab(owner: Id, color: Rgb, ring: &[P2], z: (f64, f64)) -> Solid {
    Solid::Slab { owner, color, tex: 0, face: ring.iter().map(|p| [p.0, p.1, z.0]).collect(), rise: z.1 - z.0 }
}

fn plan(face: &[P3]) -> Vec<P2> {
    face.iter().map(|p| P2(p[0], p[1])).collect()
}

impl Solid {
    #[doc(hidden)]
    pub fn owner(&self) -> Id {
        match self {
            Solid::Box { owner, .. } | Solid::Slab { owner, .. } | Solid::Prism { owner, .. } | Solid::Mesh { owner, .. } => *owner,
        }
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn color(&self) -> Rgb {
        match self {
            Solid::Box { color, .. } | Solid::Slab { color, .. } | Solid::Prism { color, .. } | Solid::Mesh { color, .. } => *color,
        }
    }

    /// From its bottom to its top.
    #[doc(hidden)]
    pub fn heights(&self) -> (f64, f64) {
        match self {
            Solid::Box { z, .. } => *z,
            Solid::Slab { face, rise, .. } => {
                let (lo, hi) = face.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
                (lo, hi + rise)
            }
            Solid::Prism { ring, bottom, top, .. } => {
                ring.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(bottom.at(*q)), hi.max(top.at(*q))))
            }
            Solid::Mesh { tris, .. } => tris.iter().flatten().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2]))),
        }
    }

    /// Its bottom and top over plan point `q`.
    pub(crate) fn z_at(&self, q: P2) -> (f64, f64) {
        match self {
            Solid::Box { z, .. } => *z,
            Solid::Slab { face, rise, .. } => {
                // The face's plane, by Newell's normal, over `q`.
                let n = face.iter().zip(face.iter().cycle().skip(1)).fold([0.0; 3], |n, (a, b)| add(n, cross(*a, *b)));
                let o = face.first().copied().unwrap_or_default();
                let z = if n[2].abs() > 1e-9 { o[2] - (n[0] * (q.0 - o[0]) + n[1] * (q.1 - o[1])) / n[2] } else { o[2] };
                (z, z + rise)
            }
            Solid::Prism { bottom, top, .. } => (bottom.at(q), top.at(q)),
            // Its triangles' heights over `q`, lowest and highest; off it, all of it.
            Solid::Mesh { tris, .. } => {
                let zs = tris.iter().filter_map(|[a, b, c]| {
                    let (pa, pb, pc) = (P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1]));
                    let det = (pb - pa).cross(pc - pa);
                    if det.abs() < 1e-9 {
                        return None;
                    }
                    let (u, v) = ((q - pa).cross(pc - pa) / det, (pb - pa).cross(q - pa) / det);
                    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| a[2] + u * (b[2] - a[2]) + v * (c[2] - a[2]))
                });
                let (lo, hi) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
                if lo <= hi { (lo, hi) } else { self.heights() }
            }
        }
    }

    /// Whether plan point `q` lies within its footprint.
    pub(crate) fn over(&self, q: P2) -> bool {
        match self {
            Solid::Box { frame, a, c, .. } => {
                let (s, t) = ((q - frame.o).dot(frame.u), (q - frame.o).dot(frame.n));
                (a.0..=a.1).contains(&s) && (c.0..=c.1).contains(&t)
            }
            Solid::Slab { face, .. } => geom::contains(&plan(face), q),
            Solid::Prism { ring, .. } => geom::contains(ring, q),
            Solid::Mesh { tris, .. } => tris.iter().any(|[a, b, c]| geom::contains(&[P2(a[0], a[1]), P2(b[0], b[1]), P2(c[0], c[1])], q)),
        }
    }

    /// Its corners.
    pub(crate) fn corners(&self) -> Vec<P3> {
        let mut out = Vec::new();
        match self {
            Solid::Box { frame, a, c, z, .. } => {
                for (s, t, h) in [a.0, a.1].into_iter().flat_map(|s| [(s, c.0), (s, c.1)]).flat_map(|(s, t)| [(s, t, z.0), (s, t, z.1)]) {
                    let p = frame.o + frame.u * s + frame.n * t;
                    out.push([p.0, p.1, h]);
                }
            }
            Solid::Slab { face, rise, .. } => out.extend(face.iter().flat_map(|p| [*p, [p[0], p[1], p[2] + rise]])),
            Solid::Prism { ring, bottom, top, .. } => {
                out.extend(ring.iter().flat_map(|q| [[q.0, q.1, bottom.at(*q)], [q.0, q.1, top.at(*q)]]))
            }
            Solid::Mesh { tris, .. } => out.extend(tris.iter().flatten()),
        }
        out
    }

    /// The stretches of the line from `p` along unit `u` that lie over it, as distances from `p`.
    pub(crate) fn across(&self, p: P2, u: P2) -> Vec<(f64, f64)> {
        match self {
            Solid::Box { frame, a, c, .. } => {
                // Where the line lies between the box's sides, a pair of them at a time.
                let span = |axis: P2, (lo, hi): (f64, f64)| {
                    let (k0, k1) = ((p - frame.o).dot(axis), u.dot(axis));
                    if k1.abs() < 1e-12 {
                        return (lo..=hi).contains(&k0).then_some((f64::NEG_INFINITY, f64::INFINITY));
                    }
                    let (s0, s1) = ((lo - k0) / k1, (hi - k0) / k1);
                    Some((s0.min(s1), s0.max(s1)))
                };
                match (span(frame.u, *a), span(frame.n, *c)) {
                    (Some(x), Some(y)) if x.0.max(y.0) < x.1.min(y.1) => vec![(x.0.max(y.0), x.1.min(y.1))],
                    _ => Vec::new(),
                }
            }
            Solid::Slab { face, .. } => crossings(&plan(face), p, u),
            Solid::Prism { ring, .. } => crossings(ring, p, u),
            // Shown beyond a section's line, as Revit shows furniture, and never cut.
            Solid::Mesh { .. } => Vec::new(),
        }
    }

    /// The rings a section along unit `u` from `p`, `len` long, cuts it into, as (along, up) in mm: where it lies over the
    /// line, level or along a sloped face; a convex mesh that `cut` allows is cut by the plane through the line, where its
    /// edges cross it (a tube across is its ring, an ellipse if it is oblique).
    #[doc(hidden)]
    pub fn cuts(&self, p: P2, u: P2, len: f64) -> Vec<Vec<P2>> {
        if let Solid::Mesh { tris, cut: true, .. } = self {
            let side = |q: P3| (P2(q[0], q[1]) - p).dot(u.perp());
            let at = |q: P3| P2((P2(q[0], q[1]) - p).dot(u), q[2]);
            let mut pts = Vec::new();
            for [a, b, c] in tris {
                for (x, y) in [(a, b), (b, c), (c, a)] {
                    let (dx, dy) = (side(*x), side(*y));
                    if dx.abs() < 1e-9 {
                        pts.push(at(*x));
                    } else if dx * dy < 0.0 {
                        pts.push(at(*x) + (at(*y) - at(*x)) * (dx / (dx - dy)));
                    }
                }
            }
            let ring = geom::clip(&geom::clip(&geom::hull(pts), |q| q.0), |q| len - q.0);
            return if ring.len() >= 3 && geom::area(&ring) > 1e-6 { vec![ring] } else { Vec::new() };
        }
        self.across(p, u)
            .into_iter()
            .filter_map(|(a, b)| {
                let (a, b) = (a.max(0.0), b.min(len));
                // Level, or along a sloped face.
                let ((a0, a1), (b0, b1)) = (self.z_at(p + u * a), self.z_at(p + u * b));
                (b - a > 1e-6).then(|| vec![P2(a, a0), P2(b, b0), P2(b, b1), P2(a, a1)])
            })
            .collect()
    }

    #[doc(hidden)]
    pub fn tris(&self, out: &mut Vec<Tri>) {
        match self {
            Solid::Box { owner, color, tex, frame, a, c, z } => frame.cuboid(out, *owner, (*color, *tex), *a, *c, *z),
            Solid::Slab { owner, color, tex, face, rise } => prism(out, *owner, (*color, *tex), face, *rise),
            Solid::Prism { owner, color, tex, ring, bottom, top, curve } => {
                let (owner, color, tex) = (*owner, *color, *tex);
                // Counter-clockwise, so every face is turned out: a wall's ring runs the other way round.
                let turned: Vec<P2>;
                let ring = if geom::area(ring) < 0.0 {
                    turned = ring.iter().rev().copied().collect();
                    &turned
                } else {
                    ring
                };
                let at = |q: P2, p: &Plane| [q.0, q.1, p.at(q)];
                for [a, b, c] in geom::triangulate(ring) {
                    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
                    out.push(Tri { p: [at(pt(a), top), at(pt(b), top), at(pt(c), top)], owner, color, tex, curve: None, vis: Vis::ALWAYS });
                    out.push(Tri {
                        p: [at(pt(a), bottom), at(pt(c), bottom), at(pt(b), bottom)],
                        owner,
                        color,
                        tex,
                        curve: None,
                        vis: Vis::ALWAYS,
                    });
                }
                for (a, b) in geom::ring_edges(ring) {
                    let (a0, b0, a1, b1) = (at(a, bottom), at(b, bottom), at(a, top), at(b, top));
                    // Along a curved wall's face (not across it, where it's cut), which face.
                    let curve = curve
                        .filter(|(c, _)| (b - a).unit().cross((a - *c).unit()).abs() > 0.5)
                        .map(|(c, r)| (owner, usize::from(((a + b) * 0.5).dist(c) > r)));
                    out.push(Tri { p: [a0, b0, b1], owner, color, tex, curve, vis: Vis::ALWAYS });
                    out.push(Tri { p: [a0, b1, a1], owner, color, tex, curve, vis: Vis::ALWAYS });
                }
            }
            // Each surface keyed by its first triangle's place in `out`, which no other form's shares.
            // ponytail: worked out again for each component of a family; keep it per form if many big meshes slow a scene.
            Solid::Mesh { owner, color, tex, tris, caps, round, vis, .. } => {
                let first = out.len();
                let surfaces = if *round { (0..tris.len()).map(|i| (i >= *caps).then_some(*caps)).collect() } else { facets(tris) };
                for (p, s) in tris.iter().zip(surfaces) {
                    out.push(Tri { p: *p, owner: *owner, color: *color, tex: *tex, curve: s.map(|s| (*owner, first + s)), vis: *vis });
                }
            }
        }
    }
}

/// How far, in degrees, two triangles may turn at the edge they share and still be one surface:
/// the facets a curve is cut into turn less, a crease like a seat's rim more.
const SMOOTH: f64 = 20.0;

/// The curved surfaces a mesh's triangles make: those that share an edge and turn less than
/// [`SMOOTH`] degrees there are one. Each triangle's surface is its first triangle's index; none
/// for a flat face (`planes` finds those by their plane) or a triangle alone.
#[doc(hidden)]
pub fn facets(tris: &[[P3; 3]]) -> Vec<Option<usize>> {
    /// The first triangle of `i`'s surface, as `up` links them, shortening the way as it goes.
    fn first(up: &mut [usize], mut i: usize) -> usize {
        while let Some(&p) = up.get(i)
            && p != i
        {
            let grand = up.get(p).copied().unwrap_or(p);
            if let Some(slot) = up.get_mut(i) {
                *slot = grand;
            }
            i = grand;
        }
        i
    }
    let limit = SMOOTH.to_radians().cos();
    let normals: Vec<P3> = tris.iter().map(|[a, b, c]| unit(cross(sub(*b, *a), sub(*c, *a)))).collect();
    let normal = |i: usize| normals.get(i).copied().unwrap_or_default();
    let mut up: Vec<usize> = (0..tris.len()).collect();
    // The first triangle on each edge, which its corners name to a micron (as a file rounds them).
    // ponytail: a corner on a rounding boundary may miss its twin, and a third triangle on an edge meets
    // only the first; probe the neighbouring cells and pair them all if facet lines show.
    let mut edges: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
    let mut bent = Vec::new();
    for (i, t) in tris.iter().enumerate() {
        let corner = |k: usize| t.get(k % 3).map_or([0; 3], |p| p.map(|x| (x * 1e3).round() as i64));
        for k in 0..3 {
            let (a, b) = (corner(k), corner(k + 1));
            let j = *edges.entry((a.min(b), a.max(b))).or_insert(i);
            // A triangle with no area has no normal, so it turns 90° from every other.
            let turn = dot(normal(i), normal(j));
            if j != i && a != b && turn > limit {
                let (ri, rj) = (first(&mut up, i), first(&mut up, j));
                if let Some(slot) = up.get_mut(ri.max(rj)) {
                    *slot = ri.min(rj);
                }
                // Triangles in one plane (as `planes` takes it) are a flat face, not a curve.
                if turn < 1.0 - 1e-9 {
                    bent.push(i);
                }
            }
        }
    }
    let curved: HashSet<usize> = bent.into_iter().map(|i| first(&mut up, i)).collect();
    (0..tris.len()).map(|i| Some(first(&mut up, i)).filter(|s| curved.contains(s))).collect()
}

/// The corner normals of each triangle of a curved surface, for it to shade smoothly across its
/// facets: the unit normals of that surface's triangles that meet at a corner, averaged by the angle
/// each makes there (so a quad cut into two triangles weighs the same at every corner). A crease keeps
/// its step: only a surface's own triangles are averaged. None for the rest, flat faces among them.
#[doc(hidden)]
pub fn corner_normals(tris: &[Tri]) -> Vec<Option<Arc<[P3; 3]>>> {
    let face = |t: &Tri| unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
    // A corner of a surface, to a micron: its triangles' own corners are a file's roundings of it.
    let at = |key: (Id, usize), p: P3| (key, p.map(|x| (x * 1e3).round() as i64));
    let mut sums: HashMap<((Id, usize), [i64; 3]), P3> = HashMap::new();
    for t in tris {
        let Some(key) = t.curve else { continue };
        let n = face(t);
        if dot(n, n) > 0.5 {
            for k in 0..3 {
                let (p, q, r) = (t.p[k], t.p[(k + 1) % 3], t.p[(k + 2) % 3]);
                let angle = dot(unit(sub(q, p)), unit(sub(r, p))).clamp(-1.0, 1.0).acos();
                let sum = sums.entry(at(key, p)).or_default();
                *sum = add(*sum, mul(n, angle));
            }
        }
    }
    tris.iter()
        .map(|t| {
            let (key, n) = (t.curve?, face(t));
            // Never turned from its own face; where nothing averages, its face's.
            let corner = |p: P3| {
                let m = sums.get(&at(key, p)).map_or([0.0; 3], |s| unit(*s));
                if dot(m, n) > 0.0 { m } else { n }
            };
            (dot(n, n) > 0.5).then(|| Arc::new(t.p.map(corner)))
        })
        .collect()
}

/// The stretches of the line from `p` along unit `u` that lie over `ring`, as distances from `p`:
/// where it crosses the ring's edges, paired off, in and out.
#[doc(hidden)]
pub fn crossings(ring: &[P2], p: P2, u: P2) -> Vec<(f64, f64)> {
    // ponytail: a line through a corner it only touches unpairs the rest; nudge it if slabs show gaps.
    let mut cross: Vec<f64> = geom::ring_edges(ring)
        .filter_map(|(a, b)| {
            let (e, den) = (b - a, u.cross(b - a));
            (den.abs() > 1e-12 && (0.0..1.0).contains(&((a - p).cross(u) / den))).then(|| (a - p).cross(e) / den)
        })
        .collect();
    cross.sort_by(f64::total_cmp);
    cross.as_chunks::<2>().0.iter().map(|[s0, s1]| (*s0, *s1)).collect()
}

/// The slabs `face`, and what lies `rise` over it, make: less the shafts through it, each piece of
/// it on the face's plane.
fn slabs(out: &mut Vec<Solid>, m: &Model, owner: Id, color: Rgb, face: Vec<P3>, rise: f64) {
    let (pieces, shafts) = m.less_shafts(&face, rise);
    match (shafts.is_empty(), Plane::of(&face)) {
        (false, Some(plane)) => out.extend(pieces.into_iter().map(|ring| Solid::Slab {
            owner,
            color,
            tex: 0,
            face: ring.iter().map(|q| [q.0, q.1, plane.at(*q)]).collect(),
            rise,
        })),
        _ => out.push(Solid::Slab { owner, color, tex: 0, face, rise }),
    }
}

/// The model's solids: walls cut around their openings, door leaves and glass, floor slabs and
/// ceilings, columns, stairs' steps and roofs' slopes.
#[doc(hidden)]
pub fn solids(m: &Model) -> Vec<Solid> {
    solids_cancellable(m, &|| false).unwrap_or_default()
}

#[doc(hidden)]
pub fn solids_cancellable(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Vec<Solid>> {
    if cancelled() {
        return None;
    }
    let elements: Vec<_> = m.elements.iter().map(|(&id, e)| (id, e)).collect();
    let index = walls::Index::new(m);
    if cancelled() {
        return None;
    }
    if let Some(pool) = (elements.len() >= 1000).then(render_pool).flatten() {
        use rayon::prelude::*;
        // Collect indexed chunks before flattening: triangle order, depth ties and
        // picking are identical to the serial path. Each job owns its family cache.
        let parts = pool.install(|| {
            elements.par_chunks(128).map(|chunk| (!cancelled()).then(|| solids_in(m, chunk, Some(&index)))).collect::<Vec<_>>()
        });
        return Some(parts.into_iter().collect::<Option<Vec<_>>>()?.into_iter().flatten().collect());
    }
    if cancelled() {
        return None;
    }
    Some(solids_in(m, &elements, Some(&index)))
}

/// What [`solids_and_extremes`] makes: the solids, each element's farthest points and its box.
#[doc(hidden)]
pub type Made = (Vec<Solid>, Vec<(Id, Vec<P3>)>, Vec<Option<[P3; 2]>>);

/// [`solids`] but the components', and each element's [`extremes`] (a component's from
/// [`crate::component_geometry::Components::extremes`], placed): the points that may be farthest any way. What a section's
/// frame and cut ask of the model, without the components' triangles (a section never cuts one).
#[doc(hidden)]
pub fn solids_and_extremes(m: &Model) -> Made {
    solids_and_extremes_cancellable(m, &|| false).unwrap_or_default()
}

#[doc(hidden)]
pub fn solids_and_extremes_cancellable(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Made> {
    if cancelled() {
        return None;
    }
    // Without it, each element's levels and hosts are found by going through every element.
    m.lookup();
    let elements: Vec<_> = m.elements.iter().map(|(&id, e)| (id, e)).collect();
    let index = walls::Index::new(m);
    if cancelled() {
        return None;
    }
    let part = |chunk: &[(Id, &Element)]| -> Option<Made> {
        let mut components = crate::component_geometry::Components::new(m);
        let (mut solids, mut placed) = (Vec::new(), Vec::new());
        for &(id, e) in chunk {
            if cancelled() {
                return None;
            }
            match e {
                Element::Component(c) => placed.push((id, components.extremes(c))),
                _ => {
                    let own = solids_in_with(m, &[(id, e)], Some(&index), &mut components);
                    if !own.is_empty() {
                        placed.push((id, extremes(own.iter().flat_map(Solid::corners))));
                    }
                    solids.extend(own);
                }
            }
        }
        let boxes: Vec<Option<[P3; 2]>> = placed.iter().map(|(_, p): &(Id, Vec<P3>)| bounds(p.iter().copied())).collect();
        Some((solids, placed, boxes))
    };
    let parts = match (elements.len() >= 1000).then(render_pool).flatten() {
        Some(pool) => {
            use rayon::prelude::*;
            pool.install(|| elements.par_chunks(256).map(part).collect::<Vec<_>>())
        }
        None => vec![part(&elements)],
    };
    let (mut solids, mut placed, mut boxes) = (Vec::new(), Vec::new(), Vec::new());
    for part in parts {
        if cancelled() {
            return None;
        }
        let (s, p, b) = part?;
        solids.extend(s);
        placed.extend(p);
        boxes.extend(b);
    }
    Some((solids, placed, boxes))
}

/// As the full preparation oracle, but supported components supply conservative
/// boxes and empty point slots, marked in the list beside `placed`. Their exact
/// extrema can be resolved by the work.
#[doc(hidden)]
pub fn solids_and_extremes_deferred(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<(Made, Vec<bool>)> {
    if cancelled() {
        return None;
    }
    m.lookup();
    let elements: Vec<_> = m.elements.iter().map(|(&id, e)| (id, e)).collect();
    let index = walls::Index::new(m);
    if cancelled() {
        return None;
    }
    let plain = crate::component_geometry::plain_envelopes(
        m,
        elements.iter().filter_map(|(_, e)| match e {
            Element::Component(c) => Some(c),
            _ => None,
        }),
        cancelled,
    )?;
    let part = |chunk: &[(Id, &Element)]| -> Option<(Made, Vec<bool>)> {
        let mut components = crate::component_geometry::Components::new(m);
        let (mut solids, mut placed, mut boxes, mut deferred) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for &(id, e) in chunk {
            if cancelled() {
                return None;
            }
            if let Element::Component(c) = e {
                if let Some(envelope) = components.envelope(c, &plain) {
                    placed.push((id, Vec::new()));
                    boxes.push(Some(envelope));
                    deferred.push(true);
                } else {
                    let points = components.extremes(c);
                    boxes.push(bounds(points.iter().copied()));
                    placed.push((id, points));
                    deferred.push(false);
                }
            } else if let Some(reach) = lazy_run_box(m, id, e) {
                placed.push((id, Vec::new()));
                boxes.push(Some(reach));
                deferred.push(true);
            } else {
                let own = solids_in_with(m, &[(id, e)], Some(&index), &mut components);
                if !own.is_empty() {
                    let points = extremes(own.iter().flat_map(Solid::corners));
                    boxes.push(bounds(points.iter().copied()));
                    placed.push((id, points));
                    deferred.push(false);
                }
                solids.extend(own);
            }
        }
        Some(((solids, placed, boxes), deferred))
    };
    let parts = match (elements.len() >= 1000).then(render_pool).flatten() {
        Some(pool) => {
            use rayon::prelude::*;
            pool.install(|| elements.par_chunks(256).map(part).collect::<Option<Vec<_>>>())?
        }
        None => vec![part(&elements)?],
    };
    let (mut solids, mut placed, mut boxes, mut deferred) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for ((s, p, b), d) in parts {
        if cancelled() {
            return None;
        }
        solids.extend(s);
        placed.extend(p);
        boxes.extend(b);
        deferred.extend(d);
    }
    Some(((solids, placed, boxes), deferred))
}

/// The box of a round run, whose tube is the most a work would hold and whose place alone says where it is: most are hidden,
/// and made only for a view that needs them. None for what isn't one, or has no finite box: made now.
fn lazy_run_box(m: &Model, id: Id, e: &Element) -> Option<[P3; 2]> {
    e.run()?;
    if !m.run_geom(e)?.round {
        return None;
    }
    match crate::bounds::primitive(m, id) {
        crate::bounds::Bounds::Box(reach) => Some(reach),
        _ => None,
    }
}

/// The solids of an element whose preparation was deferred: a round run reads no wall, and a component no wall index.
#[doc(hidden)]
pub fn lazy_solids(m: &Model, id: Id, e: &Element, components: &mut crate::component_geometry::Components) -> Vec<Solid> {
    solids_in_with(m, &[(id, e)], None, components)
}

/// The points that may be farthest of an element whose preparation was deferred, as preparing it would have found them.
#[doc(hidden)]
pub fn lazy_points(m: &Model, id: Id, e: &Element, components: &mut crate::component_geometry::Components) -> Vec<P3> {
    match e {
        Element::Component(c) => components.extremes(c),
        _ => extremes(lazy_solids(m, id, e, components).iter().flat_map(Solid::corners)),
    }
}

#[doc(hidden)]
pub fn solids_in(m: &Model, elements: &[(Id, &Element)], index: Option<&walls::Index>) -> Vec<Solid> {
    solids_in_with(m, elements, index, &mut crate::component_geometry::Components::new(m))
}

/// [`solids_in`], its families' geometry from `components`.
#[doc(hidden)]
pub fn solids_in_with(
    m: &Model,
    elements: &[(Id, &Element)],
    index: Option<&walls::Index>,
    components: &mut crate::component_geometry::Components,
) -> Vec<Solid> {
    let mut out = Vec::new();
    for &(id, e) in elements {
        match e {
            Element::Wall(_) => out.extend(walls::prisms_in(m, id, true, index).into_iter().map(|p| Solid::Prism {
                owner: id,
                color: WALL,
                tex: 0,
                ring: p.ring,
                bottom: p.bottom,
                top: p.top,
                curve: p.curve,
            })),
            // Under a variable layer, a shaped floor's underside is level: each piece a prism from it up to its top.
            Element::Floor(f) if m.floor_under(f).is_some() => {
                for (ring, bottom, top) in m.floor_prisms(f) {
                    let rise = ring.iter().map(|q| top.at(*q) - bottom.at(*q)).fold(0.0, f64::max);
                    for piece in m.less_shafts(&bottom.lift(&ring), rise).0 {
                        out.push(Solid::Prism { owner: id, color: SLAB, tex: 0, ring: piece, bottom, top, curve: None });
                    }
                }
            }
            Element::Floor(f) => {
                for (face, rise) in m.floor_slabs(f) {
                    slabs(&mut out, m, id, SLAB, face, rise);
                }
            }
            // The ground, as its building pads cut it: their holes through it, each pad a slab in its own.
            Element::Toposolid(t) => {
                let f = m.toposolid_cut(t);
                for (face, rise) in m.floor_slabs(&f) {
                    slabs(&mut out, m, id, TERRAIN, face, rise);
                }
            }
            // A sub-division on its host's surface, in its surface's colour.
            Element::SubDivision(sd) => {
                let c = m.subdivision_colours(sd).2;
                for (ring, bottom, top) in m.subdivision_pieces(sd) {
                    out.push(Solid::Prism { owner: id, color: Rgb(c[0], c[1], c[2]), tex: 0, ring, bottom, top, curve: None });
                }
            }
            // The site's: each area a thin slab, a tree its trunk and crown, a car park its levels' pieces (masterplan.rs).
            Element::Site(st) => {
                for (ring, lo, hi, color) in crate::masterplan::solids(m, st) {
                    if hi > lo && ring.len() >= 3 {
                        out.push(slab(id, color, &ring, (lo, hi)));
                    }
                }
            }
            Element::Ceiling(c) => {
                for (face, rise) in m.ceiling_slabs(c) {
                    slabs(&mut out, m, id, CEILING, face, rise);
                }
            }
            Element::Column(col) => {
                out.push(slab(id, WALL, &m.column_outline(e), m.span(col.level, col.base_offset, col.top_level, col.height)))
            }
            Element::StructuralColumn(col) | Element::Footing(col) => {
                out.push(slab(id, STRUCTURE, &m.column_outline(e), m.span(col.level, col.base_offset, col.top_level, col.height)))
            }
            // A rectangular run is a prism of its outline — mitred where it joins, as the plan fills it — which a
            // section cuts; a round one a tube, its ends cut by the same planes, shown beyond its line.
            crate::model::run_arm!(_) => {
                if let (Some(t), Some(g)) = (m.run_type(e), m.run_geom(e)) {
                    let color = m.materials.get(&t.material).map_or(STRUCTURE, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                    let tex = skin(m, &t.material);
                    if g.round {
                        let (tris, caps) = g.tube();
                        let cuts: Vec<_> = [false, true]
                            .into_iter()
                            .filter_map(|at_end| crate::runs::end_cuts(m, id, at_end, &g, index))
                            .flatten()
                            .collect();
                        let (tris, caps) = if cuts.is_empty() { (tris, caps) } else { crate::runs::clip_tube(tris, caps, &cuts) };
                        out.push(Solid::Mesh { owner: id, color, tex, tris, caps, round: true, cut: true, vis: Vis::ALWAYS });
                    } else {
                        let (_, bottom, top) = g.prism();
                        let ring = crate::runs::outline_in(m, id, index);
                        out.push(Solid::Prism { owner: id, color, tex, ring, bottom, top, curve: None });
                    }
                }
            }
            // A bar: a tube along each straight piece of its path, cut by a section as a round run is.
            Element::Rebar(rb) => {
                let base = m.elevation(rb.level);
                let path: Vec<P3> = rb.path.iter().map(|q| [q[0], q[1], q[2] + base]).collect();
                for (tris, caps) in crate::rebar::tubes(&path, rb.diameter) {
                    out.push(Solid::Mesh { owner: id, color: REBAR, tex: 0, tris, caps, round: true, cut: true, vis: Vis::ALWAYS });
                }
            }
            // Lofted, a skin of triangles from the footprint to the top outline.
            Element::Mass(ms) if ms.lofted() => {
                let tris = crate::massing::tris(&ms.footprint(), &ms.top_ring(), m.span(ms.level, ms.base_offset, ms.top_level, ms.height));
                out.push(Solid::Mesh { owner: id, color: MASS, tex: 0, tris, caps: 0, round: false, cut: false, vis: Vis::ALWAYS });
            }
            Element::Mass(ms) => out.push(slab(id, MASS, &ms.footprint(), m.span(ms.level, ms.base_offset, ms.top_level, ms.height))),
            // Each form in its material's colour; a curved one's facets one surface, as a curved wall's are. A section cuts it
            // only when Revit cuts its category (`graphics::cuttable`), from the element solids, not by `cut`.
            Element::Component(c) => {
                let x = P2(c.angle.to_radians().cos(), c.angle.to_radians().sin());
                for piece in components.pieces(c) {
                    let color = m.materials.get(&piece.material).map_or(COMPONENT, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                    out.push(Solid::Mesh {
                        owner: id,
                        color,
                        tex: skin(m, &piece.material),
                        tris: piece.tris,
                        caps: piece.caps,
                        round: piece.round,
                        cut: false,
                        vis: Vis::of(piece.show, x),
                    });
                }
            }
            // Each tread a block from the floor up to it, each landing a slab up to its top; the last
            // riser of a run climbs onto a landing, or the floor above.
            Element::Stair(st) => {
                let (_, each, _) = m.flight(st);
                let (base, l) = (m.elevation(st.level), stairs::stair(m, st));
                for run in &l.runs {
                    let frame = Frame { o: run.start, u: run.u, n: run.u.perp() };
                    for k in 1..run.risers {
                        let k = f64::from(k);
                        let (a, c) = (((k - 1.0) * st.tread, k * st.tread), (-st.width / 2.0, st.width / 2.0));
                        out.push(Solid::Box { owner: id, color: SLAB, tex: 0, frame, a, c, z: (base, run.z.0 + k * each) });
                    }
                }
                out.extend(l.pads.iter().map(|p| slab(id, SLAB, &p.ring, (base, p.z))));
            }
            // Each run solid from its level up to its slope, each landing up to its top.
            Element::Ramp(r) => {
                let (base, l) = (m.elevation(r.level), stairs::ramp(m, r));
                for run in &l.runs {
                    let g = run.u * ((run.z.1 - run.z.0) / run.length.max(1e-9));
                    let top = Plane { z: run.z.0 - g.dot(run.start), g };
                    out.push(Solid::Prism {
                        owner: id,
                        color: SLAB,
                        tex: 0,
                        ring: run.ring(r.width),
                        bottom: Plane::flat(base),
                        top,
                        curve: None,
                    });
                }
                out.extend(l.pads.iter().map(|p| slab(id, SLAB, &p.ring, (base, p.z))));
            }
            Element::Railing(rl) => out.extend(stairs::railing_parts(m, rl).into_iter().map(|(ring, bottom, top)| Solid::Prism {
                owner: id,
                color: RAIL,
                tex: 0,
                ring,
                bottom,
                top,
                curve: None,
            })),
            // Under a variable layer, a shaped roof's underside is level: each piece a prism from it up to its top.
            Element::Roof(r) if m.roof_under(r).is_some() => {
                for (ring, bottom, top) in m.roof_prisms(r) {
                    let rise = ring.iter().map(|q| top.at(*q) - bottom.at(*q)).fold(0.0, f64::max);
                    for piece in m.less_shafts(&bottom.lift(&ring), rise).0 {
                        out.push(Solid::Prism { owner: id, color: ROOF, tex: 0, ring: piece, bottom, top, curve: None });
                    }
                }
            }
            Element::Roof(r) => {
                for (face, rise) in m.roof_slabs(r) {
                    slabs(&mut out, m, id, ROOF, face, rise);
                }
            }
            // Of a family: each form in its material's colour, shown beyond a section's line and cut by it from the element
            // solids (a door's and a window's are cut, as Revit cuts them). Else a door's leaf, as thick as its type says (`OpeningType::leaf`), or a window's glass, a 50 mm
            // panel: in the middle of the wall, shut, square to it.
            Element::Door(o) | Element::Window(o) => {
                if o.family.is_some() {
                    let fallback = if matches!(e, Element::Door(_)) { DOOR } else { GLASS };
                    let x = m.opening_frame(o).map_or(P2(1.0, 0.0), |(_, u, _)| u);
                    for piece in m.opening_pieces(e) {
                        let color = m.materials.get(&piece.material).map_or(fallback, |x| Rgb(x.color[0], x.color[1], x.color[2]));
                        out.push(Solid::Mesh {
                            owner: id,
                            color,
                            tex: skin(m, &piece.material),
                            tris: piece.tris,
                            caps: piece.caps,
                            round: piece.round,
                            cut: false,
                            vis: Vis::of(piece.show, x),
                        });
                    }
                } else if let (Some(t), Some(w)) = (m.opening_type(e), m.wall(o.host)) {
                    let color = if matches!(e, Element::Door(_)) { DOOR } else { GLASS };
                    let tex = if matches!(e, Element::Door(_)) { 0 } else { skin(m, m.language.word("Glass")) & !TEXTURE };
                    let z = m.own_base(w) + o.sill;
                    let axis = Axis::of(w);
                    let u = axis.tangent(o.offset);
                    let frame = Frame { o: axis.at(o.offset, walls::centre(m, w)), u, n: u.perp() };
                    let a = (-t.width / 2.0, t.width / 2.0);
                    let half = if matches!(e, Element::Door(_)) { t.leaf() / 2.0 } else { 25.0 };
                    out.push(Solid::Box { owner: id, color, tex, frame, a, c: (-half, half), z: (z, z + t.height) });
                }
            }
            _ => {}
        }
    }
    out
}

/// A slab: `face`, its copy `rise` above, and the sides between them.
fn prism(out: &mut Vec<Tri>, owner: Id, (color, tex): (Rgb, u32), face: &[P3], rise: f64) {
    // Counter-clockwise seen from above, so every face is turned out.
    let turned: Vec<P3>;
    let face = if geom::area(&plan(face)) < 0.0 {
        turned = face.iter().rev().copied().collect();
        &turned
    } else {
        face
    };
    let up = |p: P3| [p[0], p[1], p[2] + rise];
    let pt = |i: usize| face.get(i).copied().unwrap_or_default();
    for [a, b, c] in geom::triangulate(&plan(face)) {
        out.push(Tri { p: [up(pt(a)), up(pt(b)), up(pt(c))], owner, color, tex, curve: None, vis: Vis::ALWAYS });
        out.push(Tri { p: [pt(a), pt(c), pt(b)], owner, color, tex, curve: None, vis: Vis::ALWAYS });
    }
    for (a, b) in face.iter().zip(face.iter().cycle().skip(1)) {
        out.push(Tri { p: [*a, *b, up(*b)], owner, color, tex, curve: None, vis: Vis::ALWAYS });
        out.push(Tri { p: [*a, up(*b), up(*a)], owner, color, tex, curve: None, vis: Vis::ALWAYS });
    }
}

/// Each triangle's plane, as an index the triangles lying in it (within half a millimetre) share;
/// the facets of one curved surface (a curved wall's face, a mesh's curve) share one too.
#[doc(hidden)]
pub fn planes(tris: &[Tri]) -> Vec<usize> {
    let mut table = PlaneTable::default();
    tris.iter().map(|t| table.assign(t)).collect()
}

/// The planes found so far, and the curved surfaces: what numbers a triangle's plane, kept with a scene so the triangles an
/// edit makes are numbered among those it keeps.
#[derive(Clone, Default)]
#[doc(hidden)]
pub struct PlaneTable {
    /// Planes by their normal, in cells wider than the tolerance, and a millimetre's slab of distance: each plane's normal,
    /// distance and number.
    found: HashMap<[i64; 4], Vec<(P3, f64, usize)>>,
    curves: HashMap<(Id, usize), usize>,
    /// The next number to give.
    #[doc(hidden)]
    pub next: usize,
}

impl PlaneTable {
    /// The number of `t`'s plane: that of a plane found before within the tolerance, else a new one.
    #[doc(hidden)]
    pub fn assign(&mut self, t: &Tri) -> usize {
        if let Some(c) = t.curve {
            let next = &mut self.next;
            return *self.curves.entry(c).or_insert_with(|| {
                *next += 1;
                *next - 1
            });
        }
        let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
        self.next += 1;
        let made = self.next - 1;
        // No area, no plane to share: its own.
        if dot(n, n) < 0.5 {
            return made;
        }
        let d = dot(n, t.p[0]);
        // A plane and its flip are one: each faces where its first ratio that isn't zero is positive.
        let (n, d) = if n.iter().find(|x| x.abs() > 1e-9).is_some_and(|x| *x < 0.0) { (mul(n, -1.0), -d) } else { (n, d) };
        // A match is looked for in the nearer neighbouring cell on each axis and the slabs either side, so the tolerance
        // holds across a cell's edge.
        let cell = n.map(|x| x * 1e4);
        let q = cell.map(|x| x.round() as i64);
        let near = |k: usize| [q[k], q[k] + if cell[k] < q[k] as f64 { -1 } else { 1 }];
        let slab = d.floor() as i64;
        for a in near(0) {
            for b in near(1) {
                for c in near(2) {
                    for s in slab - 1..=slab + 1 {
                        let same = self
                            .found
                            .get(&[a, b, c, s])
                            .and_then(|v| v.iter().find(|(m, e, _)| dot(*m, n) > 1.0 - 1e-9 && (e - d).abs() < 0.5));
                        if let Some(&(_, _, id)) = same {
                            self.next -= 1;
                            return id;
                        }
                    }
                }
            }
        }
        self.found.entry([q[0], q[1], q[2], slab]).or_default().push((n, d, made));
        made
    }
}

/// Each element's curved surfaces numbered in the order its triangles meet them, so what names a curve depends on the element
/// alone, never on where its triangles are in the scene: `tris` is one element's.
#[doc(hidden)]
pub fn local_curves(tris: &mut [Tri]) {
    let mut seen: HashMap<usize, usize> = HashMap::new();
    for t in tris {
        if let Some((owner, k)) = t.curve {
            let n = seen.len();
            t.curve = Some((owner, *seen.entry(k).or_insert(n)));
        }
    }
}

/// What a library material gives what is made of it: in the low 24 bits the texture id, one more than where it is in the
/// library's order (0 for none), and in the high 8 how see-through it is, percent.
#[doc(hidden)]
pub fn skin(m: &Model, name: &str) -> u32 {
    let Some((i, x)) = m.materials.iter().enumerate().find_map(|(i, (k, x))| (k == name).then_some((i, x))) else { return 0 };
    let texture = if x.texture.is_some() { i as u32 + 1 } else { 0 };
    texture | u32::from(x.transparency.min(100)) << 24
}

/// A layer's skin, or its element's where the layer has none.
fn over(layer: u32, outer: u32) -> u32 {
    if layer == 0 { outer } else { layer }
}

/// The model's solids split into their material layers, used by the layer oracle tests.
#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn shaded(m: &Model) -> Vec<Solid> {
    solids(m).into_iter().flat_map(|s| layered(m, s)).collect()
}

#[doc(hidden)]
pub fn layered(m: &Model, s: Solid) -> Vec<Solid> {
    layered_indexed(m, s).into_iter().map(|(_, part)| part).collect()
}

/// [`layered`], each part with the layer of the element's type it is of, counted from the first as the type lists its layers
/// (none where the element is not split into layers): what a solid per layer (`element_solids`) is made of.
pub(crate) fn layered_indexed(m: &Model, s: Solid) -> Vec<(Option<usize>, Solid)> {
    let layers = match m.elements.get(s.owner()) {
        Some(Element::Wall(w)) => m.type_layers("wall", &w.type_name),
        Some(Element::Floor(f) | Element::Toposolid(f)) => m.type_layers("floor", &f.type_name),
        Some(Element::Roof(r)) => m.type_layers("roof", &r.type_name),
        Some(Element::Ceiling(c)) => m.type_layers("ceiling", &c.type_name),
        _ => &[],
    };
    let total: f64 = layers.iter().map(|l| l.thickness).sum();
    if total <= 0.0 {
        return vec![(None, s)];
    }
    let colour = |l: &Layer, or: Rgb| m.materials.get(&l.material).map_or(or, |x| Rgb(x.color[0], x.color[1], x.color[2]));
    match s {
        Solid::Slab { owner, color, tex, face, rise } => {
            let mut out = Vec::new();
            let mut above = 0.0;
            for (k, l) in layers.iter().enumerate() {
                let r = rise * l.thickness / total;
                if r > 1e-9 {
                    let lift = rise - above - r;
                    let face = face.iter().map(|p| [p[0], p[1], p[2] + lift]).collect();
                    out.push((
                        Some(k),
                        Solid::Slab { owner, color: colour(l, color), tex: over(skin(m, &l.material), tex), face, rise: r },
                    ));
                }
                above += r;
            }
            out
        }
        // A shaped floor's or roof's over a level underside: from the top down, the layers above its variable one following
        // its top, those below it level over its underside, and the variable one between.
        Solid::Prism { owner, color, tex, ring, bottom, top, curve: None } if let Some(v) = m.variable_under(owner) => {
            let depth = top.z - bottom.z;
            let b = crate::model::Variable::bounds(Some(v), layers, depth);
            // Where layer `k` starts, from the top down: over the variable layer, under the top; under it, over the bottom.
            let at = |k: usize| {
                let from_top = b.get(k).copied().unwrap_or(depth);
                if k <= v.index { Plane { z: top.z - from_top, ..top } } else { Plane { z: bottom.z + (depth - from_top), ..bottom } }
            };
            layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.thickness > 1e-9 || l.variable)
                .map(|(k, l)| {
                    let part = Solid::Prism {
                        owner,
                        color: colour(l, color),
                        tex: over(skin(m, &l.material), tex),
                        ring: ring.clone(),
                        bottom: at(k + 1),
                        top: at(k),
                        curve: None,
                    };
                    (Some(k), part)
                })
                .collect()
        }
        // Across the wall from its exterior face in, each layer between two of its boundaries; the
        // outermost and innermost reach whatever lies past them.
        Solid::Prism { owner, color, tex, ring, bottom, top, curve } => {
            let Some(w) = m.wall(owner) else { return vec![(None, Solid::Prism { owner, color, tex, ring, bottom, top, curve })] };
            let (axis, b) = (Axis::of(w), walls::boundaries(m, w));
            let off = |q: P2| axis.project(q).1;
            let n = layers.len();
            layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.thickness > 1e-9)
                .filter_map(|(k, l)| {
                    let mut part = ring.clone();
                    if let Some(hi) = b.get(k).filter(|_| k > 0) {
                        part = geom::clip(&part, |q| hi - off(q));
                    }
                    if let Some(lo) = b.get(k + 1).filter(|_| k + 1 < n) {
                        part = geom::clip(&part, |q| off(q) - lo);
                    }
                    (part.len() >= 3 && geom::area(&part).abs() > 1e-6).then(|| {
                        let part = Solid::Prism {
                            owner,
                            color: colour(l, color),
                            tex: over(skin(m, &l.material), tex),
                            ring: part,
                            bottom,
                            top,
                            curve,
                        };
                        (Some(k), part)
                    })
                })
                .collect()
        }
        other => vec![(None, other)],
    }
}

/// Paint laid on the faces of elements: each triangle on a painted face takes the material it's painted, its colour and its
/// texture, in place of what its type is made of.
#[doc(hidden)]
pub fn painted(m: &Model, tris: &mut [Tri], faces: &[&'static str]) {
    let owners: BTreeSet<Id> = tris.iter().map(|t| t.owner).collect();
    let paints: HashMap<Id, &serde_json::Map<String, serde_json::Value>> =
        owners.into_iter().filter_map(|id| Some((id, m.elements.get(id)?.params()?.get(paint::KEY)?.as_object()?))).collect();
    if paints.is_empty() {
        return;
    }
    for (t, face) in tris.iter_mut().zip(faces) {
        let Some(name) = paints.get(&t.owner).and_then(|p| p.get(*face)).and_then(|v| v.as_str()) else { continue };
        if let Some(x) = m.materials.get(name) {
            (t.color, t.tex) = (Rgb(x.color[0], x.color[1], x.color[2]), skin(m, name));
        }
    }
}

/// Each element's top over plan point `p`: the highest top of its solids standing over it.
#[doc(hidden)]
pub fn tops_at(m: &Model, p: P2) -> BTreeMap<Id, f64> {
    let mut tops = BTreeMap::new();
    for s in solids(m).iter().filter(|s| s.over(p)) {
        let z = s.z_at(p).1;
        let top = tops.entry(s.owner()).or_insert(z);
        *top = f64::max(*top, z);
    }
    tops
}

/// The box `points` span; none for none.
#[doc(hidden)]
pub fn bounds(points: impl Iterator<Item = P3>) -> Option<[P3; 2]> {
    points.fold(None, |b, p| {
        Some(match b {
            None => [p, p],
            Some([lo, hi]) => [std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i]))],
        })
    })
}

#[cfg(test)]
#[path = "solids_tests.rs"]
mod tests;

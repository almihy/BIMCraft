//! The 3D view: the model as solids (walls with their doors and windows cut out, floor slabs and
//! ceilings with the shafts through them cut out, door and window panels, columns, stairs' steps, roofs' slopes) in an axonometric projection, as Revit's default 3D view shows it. A
//! depth buffer keeps the nearest surface at each pixel; faces are shaded by how they turn to
//! the light, and an edge is drawn wherever the plane a pixel shows changes, so coplanar faces
//! (joined walls' tops, a face's triangles) read as one. The picture keeps which element each
//! pixel shows, for picking. Sections and elevations use the same pictures, clipped at their
//! line and in hidden line, and cut the same solids.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, OnceLock};

use crate::geom::{self, P2};
use crate::graphics::{Look, VisualStyle};
use crate::model::{Element, Id, Model, TOL};
use crate::paint;
use crate::prim::{PAPER, Rgb};
use crate::render::clip;
use crate::texture::Sampler;
use crate::walls::{self, Axis, Plane};

pub mod gpu;

pub use crate::camera::{
    AZIMUTH, DisplayOptions, ELEVATION, FILM, LENS, MAX_LENS, MAX_SILHOUETTE, MIN_LENS, field_of_view, fov_limits, fov_of_35mm, full_frame,
    lens_for, lens_ok,
};
pub use crate::geom::v3::{P3, add, cross, dot, extremes, mul, sub, unit};

use crate::solids::*;

/// The largest picture, in px a side.
pub const MAX_SIDE: u32 = 4096;
/// Where faces meet.
pub const EDGE: Rgb = Rgb(48, 48, 48);
/// What a level's plane is outlined in.
const DATUM: Rgb = Rgb(60, 100, 150);
/// Room around a fitted model, px.
const MARGIN: f64 = 30.0;

/// How far apart in depth two neighbouring pixels on different planes must be for a silhouette between them: four pixels'
/// worth, more than the slope of any face but one seen nearly edge on.
const OUTLINE_GAP: f64 = 4.0;
/// Where ambient shadows look round a pixel for what rises toward the eye about it: the pixels across from each other at
/// these offsets, on four lines through it at two reaches (the card's `AMBIENT` is the same).
const AMBIENT: [(i32, i32); 8] = [(12, 0), (0, 12), (8, 8), (8, -8), (6, 0), (0, 6), (4, 4), (4, -4)];
/// A neighbour further off in depth than this many pixels' worth a pixel is on something else (in front, or behind), not
/// the corner the pixel is in.
const AMBIENT_REACH: f64 = 4.0;
/// How dark the deepest ambient shadow is: the share of the light it takes.
const AMBIENT_DARK: f64 = 0.6;

mod fragments;
#[doc(hidden)]
pub mod update;
pub use fragments::{FragmentStats, SectionCache};

/// The model as triangles, each with its plane, and its corner normals where it's a curve's: what
/// every picture of it is drawn from.
pub struct Scene {
    #[doc(hidden)]
    pub tris: Vec<Tri>,
    #[doc(hidden)]
    pub planes: Vec<usize>,
    #[doc(hidden)]
    pub smooth: Vec<Option<Arc<[P3; 3]>>>,
    /// Each triangle's face, as paint names them: `exterior`, `interior`, `top`, `bottom`, `all`, or none.
    #[doc(hidden)]
    pub faces: Arc<Vec<&'static str>>,
    /// Each texture the scene uses, by id. A textured triangle lies across its plane as [`basis`] says.
    #[doc(hidden)]
    pub skins: Vec<Option<Sampler>>,
    /// The edges a wireframe draws, found when one is first asked for.
    edges: OnceLock<Vec<(usize, [P3; 2])>>,
    /// The masses, which a section's picture leaves out: it outlines where its line cuts them.
    #[doc(hidden)]
    pub masses: BTreeSet<Id>,
    /// Each level's plane: its name, and the corners of its extent at its elevation.
    #[doc(hidden)]
    pub levels: Vec<(Id, String, [P3; 4])>,
    /// The scans point clouds place, drawn as dots over the surfaces.
    #[doc(hidden)]
    pub scans: Vec<Scan>,
    /// The components whose triangles are a shape of [`Scene::shapes`] placed, exactly: where in `tris` the copy starts, and
    /// how it is placed. A card draws each shape once and places it for each copy.
    #[doc(hidden)]
    pub placements: Vec<(usize, crate::component_geometry::ComponentInstance)>,
    /// Each component shape's triangles in its family's own axes, in the order a copy's are in `tris`.
    #[doc(hidden)]
    pub shapes: Vec<Vec<[P3; 3]>>,
    /// What each shape is the shape of, so a copy an edit makes finds its own.
    shape_keys: HashMap<ShapeKey, usize>,
    /// Each element's run of triangles and the box round them, in the elements' order: what an edit replaces.
    #[doc(hidden)]
    pub spans: Vec<Span>,
    /// What numbered the planes, so an edit numbers its triangles' among them.
    table: PlaneTable,
    /// Each element as closed solids, made when first asked for ([`Scene::element_solids`]).
    solids: SolidCache,
}

/// An element's triangles in a scene: where they are in its `tris`, and the box round them.
#[derive(Clone, Debug, PartialEq)]
#[doc(hidden)]
pub struct Span {
    #[doc(hidden)]
    pub id: Id,
    #[doc(hidden)]
    pub at: std::ops::Range<usize>,
    bounds: [P3; 2],
}

/// What a shape of a scene is the shape of: a family's type, mirrored or not, with the values its family evaluates; or the
/// tube the round runs of a material are.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ShapeKey {
    Family(crate::component_geometry::Key),
    Tube(String),
}

/// Each element's run of `tris`, in their order: an element's triangles are made together.
fn spans_of(tris: &[Tri]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(first) = tris.get(i) {
        let end = i + tris[i..].iter().take_while(|t| t.owner == first.owner).count();
        let bounds = bounds(tris[i..end].iter().flat_map(|t| t.p)).unwrap_or_default();
        out.push(Span { id: first.owner, at: i..end, bounds });
        i = end;
    }
    out
}

/// A triangle of the scene as a file takes it ([`Scene::facets`]).
#[doc(hidden)]
pub struct Facet {
    pub owner: Id,
    pub corners: [P3; 3],
    /// Unit, at the corners: the curve's where the triangle is on a curved surface, else its own, turned out as its corners wind.
    /// All zero for a triangle with no area.
    pub normals: [P3; 3],
    /// As the view shades it.
    pub color: Rgb,
    /// How see-through, percent: as much as its material, or the view makes it.
    pub clearness: u8,
}

/// A scan as a point cloud places it: its points and colours (shared with the model's), and where they go.
#[doc(hidden)]
pub struct Scan {
    owner: Id,
    points: Arc<Vec<[f32; 3]>>,
    colours: Option<Arc<Vec<[u8; 3]>>>,
    /// Its middle in the model, and the cosine and sine of its turn.
    at: P3,
    turn: (f64, f64),
    /// The corners of the box round its points in the model, for fitting the view to it.
    #[doc(hidden)]
    pub corners: [P3; 8],
}

impl Scan {
    fn place(&self, p: [f32; 3]) -> P3 {
        let (x, y, (cos, sin)) = (f64::from(p[0]), f64::from(p[1]), self.turn);
        [self.at[0] + x * cos - y * sin, self.at[1] + x * sin + y * cos, self.at[2] + f64::from(p[2])]
    }
}

impl Scan {
    /// Point `i`'s colour: its own, or the grey of one with none; the view's, unless it gives none.
    fn colour(&self, i: usize, plain: bool, look: &Look) -> Rgb {
        let grey = Rgb(crate::cloud::GREY, crate::cloud::GREY, crate::cloud::GREY);
        let base = self.colours.as_ref().and_then(|c| c.get(i)).map_or(grey, |c| Rgb(c[0], c[1], c[2]));
        if plain { base } else { look.surface(self.owner, base) }
    }
}

/// The first pixel of the dot a point seen at `(x, y, z)` draws, [`DOT`] pixels each way from it, in a picture of `(w, h)`; none
/// where none of it is in the picture.
fn dot_at((x, y, z): (f64, f64, f64), (w, h): (usize, usize)) -> Option<(i64, i64)> {
    let half = DOT as f64 / 2.0;
    let (x0, y0) = ((x - half).round(), (y - half).round());
    (x0 > -(DOT as f64) && y0 > -(DOT as f64) && x0 < w as f64 && y0 < h as f64 && z.is_finite()).then_some((x0 as i64, y0 as i64))
}

/// Whether all that changed from `before` to `after` is the section box of the default 3D view: then its scene and its mesh are
/// as they were, a box being asked of them when they are drawn and picked.
pub fn differs_only_in_section_box(before: &Model, after: &Model) -> bool {
    let crate::ids::Changed::Ids(changed) = after.changed_since(before) else { return false };
    let views = [before.default_3d(), after.default_3d()];
    changed.iter().all(|id| views.contains(&Some(*id)))
        && Look::of_3d(before).with_section_box(None) == Look::of_3d(after).with_section_box(None)
}

/// Whether 3D view `view` (none: the default `{3D}`) draws `after` from the scene and the mesh it made of `before`: what
/// changed is only what neither reads (3D views and the dimensions in them: a camera view's eye and target written back as
/// the hand lets it be, a camera view made, a 3D dimension placed), and that view looks as it did (its own Visibility/
/// Graphics and visual style, a camera view's being its own) but for its section box, which a picture asks of them as it is
/// drawn.
pub fn scene_kept(before: &Model, after: &Model, view: Option<Id>) -> bool {
    let crate::ids::Changed::Ids(changed) = after.changed_since(before) else { return false };
    let unseen = |m: &Model, id: Id| m.elements.get(id).is_none_or(|e| matches!(e, Element::View3D(_) | Element::Dimension3D(_)));
    let look = |m: &Model| match view.filter(|id| matches!(m.elements.get(*id), Some(Element::View3D(_)))) {
        Some(id) => Look::of(m, id),
        None => Look::of_3d(m),
    };
    changed.iter().all(|id| unseen(before, *id) && unseen(after, *id))
        && look(before).with_section_box(None) == look(after).with_section_box(None)
}

/// The most points a picture draws, as Revit's density does: past it, every so many of each scan's.
pub const DRAWN: usize = 2_000_000;

/// How wide a point is drawn, px.
pub(crate) const DOT: usize = 2;

/// Which of `total` points a picture draws: every one of so many.
pub(crate) fn every(total: usize) -> usize {
    total.div_ceil(DRAWN).max(1)
}

/// How an element's triangles name their faces.
enum Rule {
    /// A wall's: its exterior is to the left of its start to its end.
    Wall(Axis),
    /// A floor's, a roof's or a ceiling's: facing up or down.
    Slab,
    /// Any other: all of it is one face.
    Whole,
}

impl Rule {
    /// The face a triangle is on, or none where it's on no face that can be painted (a wall's top or end).
    fn face(&self, p: &[P3; 3]) -> &'static str {
        let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        match self {
            Rule::Wall(axis) => {
                let c = P2((p[0][0] + p[1][0] + p[2][0]) / 3.0, (p[0][1] + p[1][1] + p[2][1]) / 3.0);
                let left = axis.tangent(axis.project(c).0).perp();
                match n[0] * left.0 + n[1] * left.1 {
                    d if d > 0.5 => "exterior",
                    d if d < -0.5 => "interior",
                    _ => "",
                }
            }
            Rule::Slab if n[2] > 0.5 => "top",
            Rule::Slab if n[2] < -0.5 => "bottom",
            Rule::Slab => "",
            Rule::Whole => "all",
        }
    }
}

/// Two unit vectors across a triangle's plane, the way a texture's across and up lie on it: across is level (to the
/// right of someone looking at the face from outside, east where it lies flat), up goes up it (north where it lies flat).
fn basis(t: &Tri) -> [P3; 2] {
    let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
    let level = cross([0.0, 0.0, 1.0], n);
    let across = if dot(level, level) < 1e-4 { [1.0, 0.0, 0.0] } else { unit(level) };
    [across, unit(cross(n, across))]
}

/// Whether `p` is inside `look`'s section box, or there is none.
fn kept(look: &Look, p: P3) -> bool {
    look.section_box().is_none_or(|(lo, hi)| (0..3).all(|k| p[k] >= lo[k] - TOL && p[k] <= hi[k] + TOL))
}

/// A face of the model under a pixel ([`Scene::face_at`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub at: P3,
    pub normal: P3,
    pub owner: Id,
    pub tris: Vec<[P3; 3]>,
}

/// Where a level's name goes in a 3D view: a vertical plane, unseen, past the right end of its
/// plane's far edges and in line with the one that ends there, lettered with the name. It is where an
/// elevation marker (an annotation family, later) will stand.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelHead {
    pub level: Id,
    pub name: String,
    /// Along its foot, then up its far side: on the level's elevation, and its lettering's height above.
    pub corners: [P3; 4],
}

/// How tall a level's name is lettered in 3D, px; and the room round it.
const HEAD_TEXT: f32 = 11.0;
const HEAD_PAD: f32 = 3.0;

impl Scene {
    pub fn new(m: &Model) -> Scene {
        Self::new_cancellable(m, &|| false).unwrap_or_else(|| Scene {
            tris: Vec::new(),
            planes: Vec::new(),
            smooth: Vec::new(),
            faces: Arc::default(),
            skins: Vec::new(),
            masses: BTreeSet::new(),
            levels: Vec::new(),
            scans: Vec::new(),
            edges: OnceLock::new(),
            placements: Vec::new(),
            shapes: Vec::new(),
            shape_keys: HashMap::new(),
            spans: Vec::new(),
            table: PlaneTable::default(),
            solids: SolidCache::default(),
        })
    }

    /// Build a view snapshot, releasing superseded work between its expensive stages.
    pub fn new_cancellable(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Scene> {
        if cancelled() {
            return None;
        }
        let mut tris = Vec::new();
        for solid in solids_cancellable(m, cancelled)?.into_iter().flat_map(|s| layered(m, s)) {
            if cancelled() {
                return None;
            }
            solid.tris(&mut tris);
        }
        if cancelled() {
            return None;
        }
        let spans = spans_of(&tris);
        for span in &spans {
            if let Some(own) = tris.get_mut(span.at.clone()) {
                local_curves(own);
            }
        }
        // Each paintable element's rule for naming the faces of its triangles; paint gives those it's laid on their material.
        let rules = rules_of(m, spans.iter().map(|s| s.id));
        let faces: Vec<&'static str> = tris.iter().map(|t| rules.get(&t.owner).map_or("", |r| r.face(&t.p))).collect();
        painted(m, &mut tris, &faces);
        if cancelled() {
            return None;
        }
        // The textures the scene uses, ready to look up, and what each textured triangle lies across.
        let mut skins: Vec<Option<Sampler>> = vec![None; m.materials.len() + 1];
        skins_for(m, &mut skins, &tris);
        if cancelled() {
            return None;
        }
        let mut table = PlaneTable::default();
        let planes = tris.iter().map(|t| table.assign(t)).collect();
        if cancelled() {
            return None;
        }
        let smooth = corner_normals(&tris);
        if cancelled() {
            return None;
        }
        let (masses, levels, scans) = whole_model(m, &spans);
        let (shapes, shape_keys, placements) = placements(m, &tris, cancelled)?;
        Some(Scene {
            tris,
            planes,
            smooth,
            faces: Arc::new(faces),
            skins,
            masses,
            levels,
            scans,
            edges: OnceLock::new(),
            placements,
            shapes,
            shape_keys,
            spans,
            table,
            solids: SolidCache::default(),
        })
    }

    /// The scene a section's or an elevation's picture through `cam` and `clip` is drawn from, of what `look` shows: only
    /// what can show in it, each element whole. Its picture is hidden line, so the scene holds the triangles, their faces and
    /// their planes, not the textures and smooth normals a shaded view needs. What it leaves out:
    /// - what lies outside the picture, before the line (cut away) or past the far clip;
    /// - what lies strictly behind what hides it: walls, floors, roofs, ceilings, doors, windows and columns are drawn into
    ///   a depth buffer first (as the picture draws them, clipped as it is), kept as tiles of their farthest depth, and an
    ///   element whose box is nearer than none of the tiles it covers can win no pixel of the picture.
    ///
    /// Every pixel of its picture is what the whole scene's is: what is left out was behind, and the rest are drawn in the
    /// order they are in the whole scene.
    #[doc(hidden)]
    pub fn section(m: &Model, work: Work<'_>, cam: &Camera, clip: &Clip, look: &Look) -> Scene {
        Self::section_cancellable(m, work, cam, clip, look, &|| false).unwrap_or_else(|| Self::bare(Vec::new()))
    }

    pub(crate) fn section_cancellable(
        m: &Model,
        (solids, ranges, placed, boxes): Work<'_>,
        cam: &Camera,
        clip: &Clip,
        look: &Look,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<Scene> {
        if cancelled() {
            return None;
        }
        // Each element's box, round the points that may be its farthest, and where its solids are; a component's are made
        // only if it shows.
        let listed: Vec<_> = placed.iter().zip(boxes).collect();
        let boxes: Vec<Boxed3> = in_order(&listed, |((id, _), b)| {
            let e = m.elements.get(*id);
            (look.shows(*id) && !matches!(e, Some(Element::Mass(_)))).then(|| {
                let at = match e {
                    Some(Element::Component(_)) => 0..0,
                    _ => ranges.get(id).cloned().unwrap_or(0..0),
                };
                (*id, **b, at)
            })
        })
        .into_iter()
        .flatten()
        .collect();
        let own = |id: Id, at: &std::ops::Range<usize>| solids.painted(at.clone()).filter(move |s| s.owner() == id);
        // The occluders drawn, as the picture draws them.
        let (w, h) = (cam.width.clamp(1, MAX_SIDE) as usize, cam.height.clamp(1, MAX_SIDE) as usize);
        let hides = |id: Id| {
            matches!(
                m.elements.get(id),
                Some(
                    Element::Wall(_)
                        | Element::Floor(_)
                        | Element::Roof(_)
                        | Element::Ceiling(_)
                        | Element::Door(_)
                        | Element::Window(_)
                        | Element::Column(_)
                        | Element::StructuralColumn(_)
                )
            )
        };
        // Each box on the screen, once.
        let project = cam.projector();
        let screens: Vec<Option<[f64; 6]>> = in_order(&boxes, |(_, b, _)| b.as_ref().and_then(|b| Tiles::screen(&project, b)));
        // The occluders nearest first, drawn in batches each twice the last; before each, those already behind what is
        // drawn are left out: behind part of the depths, they are behind all of them, and the nearest at every pixel is
        // what it would be with them.
        let mut tiles = Tiles::of(vec![f32::NEG_INFINITY; w * h], w, h);
        let nearest = |i: &usize| screens.get(*i).copied().flatten().map_or(f64::INFINITY, |s| s[5]);
        let mut pending: Vec<usize> =
            in_order(&boxes, |(id, ..)| hides(*id)).into_iter().enumerate().filter_map(|(i, hiding)| hiding.then_some(i)).collect();
        pending.sort_by(|a, b| nearest(b).total_cmp(&nearest(a)).then(a.cmp(b)));
        let (mut batch, mut hit, mut lit) = (256, vec![u32::MAX; w * h], vec![0.0; w * h]);
        while !pending.is_empty() {
            if cancelled() {
                return None;
            }
            let rest = pending.split_off(batch.min(pending.len()));
            let groups: Vec<&[usize]> = pending.chunks(TRIS_AT_ONCE).collect();
            let made = in_order(&groups, |group| {
                let mut tris = Vec::new();
                for &i in *group {
                    if cancelled() {
                        return None;
                    }
                    let Some((id, _, at)) = boxes.get(i) else { continue };
                    if tiles.may_show_on(clip, screens.get(i).and_then(Option::as_ref)) {
                        for solid in own(*id, at).flat_map(|s| layered(m, s)) {
                            solid.tris(&mut tris);
                        }
                    }
                }
                // A family's form the view doesn't draw (its detail level, the way it looks) hides nothing.
                tris.retain(|t| look.draws_vis(t.vis));
                Some(tris)
            });
            let mut occluders = Vec::with_capacity(made.iter().flatten().map(Vec::len).sum());
            for tris in made {
                occluders.extend(tris?);
            }
            Scene::bare(occluders).raster_cancellable(
                (cam, Some(clip), None),
                (w, h),
                |_| true,
                None,
                &|_| None,
                (&mut tiles.depth, &mut hit, &mut lit),
                cancelled,
            );
            tiles.refresh();
            pending = rest.into_iter().filter(|i| tiles.may_show_on(clip, screens.get(*i).and_then(Option::as_ref))).collect();
            batch *= 2;
        }
        // What can show, in the order of the elements.
        let screened: Vec<_> = boxes.iter().zip(&screens).collect();
        let kept: Vec<&Boxed3> =
            in_order(&screened, |(b, s)| tiles.may_show_on(clip, s.as_ref()).then_some(*b)).into_iter().flatten().collect();
        let groups: Vec<&[&Boxed3]> = kept.chunks(TRIS_AT_ONCE).collect();
        let made = in_order(&groups, |group| {
            let mut components = crate::component_geometry::Components::new(m);
            let mut tris = Vec::new();
            for (id, _, at) in group.iter().copied() {
                if cancelled() {
                    return None;
                }
                let made: Vec<Solid> = match m.elements.get(*id) {
                    Some(e @ Element::Component(_)) => lazy_solids(m, *id, e, &mut components),
                    // No solids in the list: a round run left until now, or one that has none, which makes none here.
                    Some(e) if at.is_empty() && e.run().is_some() => lazy_solids(m, *id, e, &mut components),
                    _ => own(*id, at).collect(),
                };
                for solid in made.into_iter().flat_map(|s| layered(m, s)) {
                    solid.tris(&mut tris);
                }
            }
            Some(tris)
        });
        let mut tris = Vec::with_capacity(made.iter().flatten().map(Vec::len).sum());
        for part in made {
            tris.extend(part?);
        }
        let rules: HashMap<Id, Rule> = rules_of(m, tris.iter().map(|t| t.owner));
        let faces: Vec<&'static str> = in_order(&tris, |t| rules.get(&t.owner).map_or("", |r| r.face(&t.p)));
        if cancelled() {
            return None;
        }
        let planes = planes(&tris);
        if cancelled() {
            return None;
        }
        Some(Scene { planes, faces: Arc::new(faces), scans: scans_of(m), ..Scene::bare(tris) })
    }

    /// A scene of `tris` alone: no planes, faces, textures, smooth normals, levels or scans.
    fn bare(tris: Vec<Tri>) -> Scene {
        Scene {
            tris,
            planes: Vec::new(),
            smooth: Vec::new(),
            faces: Arc::new(Vec::new()),
            skins: Vec::new(),
            masses: BTreeSet::new(),
            levels: Vec::new(),
            scans: Vec::new(),
            edges: OnceLock::new(),
            placements: Vec::new(),
            shapes: Vec::new(),
            shape_keys: HashMap::new(),
            spans: Vec::new(),
            table: PlaneTable::default(),
            solids: SolidCache::default(),
        }
    }
}

/// The samplers of the textures among `tris` that `skins` hasn't yet.
fn skins_for(m: &Model, skins: &mut [Option<Sampler>], tris: &[Tri]) {
    let used: BTreeSet<u32> = tris.iter().map(Tri::texture).filter(|t| *t > 0).collect();
    for id in used {
        let found = m.materials.values().nth(id as usize - 1);
        if let (Some(x), Some(slot @ None)) = (found, skins.get_mut(id as usize)) {
            *slot = x.texture.as_ref().and_then(|t| Sampler::new(t, x.color, &m.images));
        }
    }
}

/// What a scene takes of the whole model, not of each element's triangles: the masses, each level's plane, and the scans. A
/// level with no extent of its own spans the model's ([`Model::model_extent`]), found from the boxes round each element's
/// triangles, `spans`, which hold the corners of its solids.
#[allow(clippy::type_complexity)]
fn whole_model(m: &Model, spans: &[Span]) -> (BTreeSet<Id>, Vec<(Id, String, [P3; 4])>, Vec<Scan>) {
    let masses = m.elements.iter().filter(|(_, e)| matches!(e, Element::Mass(_))).map(|(id, _)| *id).collect();
    let default = std::cell::OnceCell::new();
    let extent = || m.model_extent_of(spans.iter().flat_map(|s| [P2(s.bounds[0][0], s.bounds[0][1]), P2(s.bounds[1][0], s.bounds[1][1])]));
    let levels = m
        .levels()
        .into_iter()
        .map(|(id, l)| {
            let [lo, hi] = l.extent.unwrap_or_else(|| *default.get_or_init(extent));
            let z = l.elevation;
            (id, l.name.clone(), [[lo.0, lo.1, z], [hi.0, lo.1, z], [hi.0, hi.1, z], [lo.0, hi.1, z]])
        })
        .collect();
    (masses, levels, scans_of(m))
}

/// The components among `tris` that are a shape placed, to the last bit: each shape's triangles in its own axes, and for each
/// copy where its run of triangles starts and how it is placed. A copy whose triangles aren't exactly its shape's placed (or
/// aren't together) is left out, and is drawn from its own triangles as any element is.
#[allow(clippy::type_complexity)]
fn placements(
    m: &Model,
    tris: &[Tri],
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<(Vec<Vec<[P3; 3]>>, HashMap<ShapeKey, usize>, Vec<(usize, crate::component_geometry::ComponentInstance)>)> {
    let shared = crate::component_geometry::SharedComponents::new(m, cancelled)?;
    let mut shapes: Vec<Vec<[P3; 3]>> =
        shared.shapes.iter().map(|pieces| pieces.iter().flat_map(|p| p.tris.iter().copied()).collect()).collect();
    let mut keys: HashMap<ShapeKey, usize> = shared.keys.into_iter().map(|(k, n)| (ShapeKey::Family(k), n)).collect();
    let mut copies: HashMap<Id, crate::component_geometry::ComponentInstance> = shared.instances.iter().map(|i| (i.owner, *i)).collect();
    // Each round run a copy of one tube, a tube for each material, so a copy takes its shape's colour and texture.
    let tube = crate::runs::unit_tube().0;
    let mut tubes: HashMap<&str, usize> = HashMap::new();
    for (n, (id, e)) in m.elements.iter().enumerate() {
        if n % 4096 == 0 && cancelled() {
            return None;
        }
        let (Some(t), Some(g)) = (m.run_type(e), m.run_geom(e)) else { continue };
        if !g.round {
            continue;
        }
        let shape = *tubes.entry(t.material.as_str()).or_insert_with(|| {
            shapes.push(tube.clone());
            keys.insert(ShapeKey::Tube(t.material.clone()), shapes.len() - 1);
            shapes.len() - 1
        });
        let place = g.tube_place(*id, shape);
        if place.solid() {
            copies.insert(*id, place);
        }
    }
    let mut out = Vec::new();
    let mut i = 0;
    let mut runs = 0usize;
    while i < tris.len() {
        runs += 1;
        if runs.is_multiple_of(4096) && cancelled() {
            return None;
        }
        let owner = tris[i].owner;
        let end = i + tris[i..].iter().take_while(|t| t.owner == owner).count();
        if let Some(copy) = copies.get(&owner)
            && let Some(local) = shapes.get(copy.shape)
            && !local.is_empty()
            && local.len() == end - i
            && local.iter().zip(&tris[i..end]).all(|(l, t)| l.map(|q| copy.place(q)) == t.p)
        {
            out.push((i, *copy));
        }
        i = end;
    }
    Some((shapes, keys, out))
}

/// How many elements a thread makes the triangles of at a time.
const TRIS_AT_ONCE: usize = 256;

/// `f` of each item, in their order, on the render threads when there are enough to pay for it: the same list either way.
fn in_order<T: Sync, U: Send>(items: &[T], f: impl Fn(&T) -> U + Sync) -> Vec<U> {
    match (items.len() >= 1000).then(render_pool).flatten() {
        Some(pool) => {
            use rayon::prelude::*;
            pool.install(|| items.par_iter().map(&f).collect())
        }
        None => items.iter().map(f).collect(),
    }
}

/// What a section's scene is made of, as `render::SectionWork` holds it: the model's solids but the components', where each
/// element's are among them, and each element's farthest points ([`extremes`]).
pub(crate) type Work<'a> = (&'a PreparedSolids, &'a HashMap<Id, std::ops::Range<usize>>, &'a [(Id, Vec<P3>)], &'a [Option<[P3; 2]>]);

/// An element, the box round it (none where it isn't known) and where its solids are in the list (a component's are made
/// only when it shows).
type Boxed3 = (Id, Option<[P3; 2]>, std::ops::Range<usize>);

/// A picture's depth buffer as tiles of [`Tiles::SIDE`] px, each keeping its farthest depth: what an element must be nearer
/// than somewhere to show.
struct Tiles {
    far: Vec<f32>,
    depth: Vec<f32>,
    across: usize,
    w: usize,
    h: usize,
}

impl Tiles {
    const SIDE: usize = 8;
    /// The most pixels an element is tested over one by one.
    const PIXELS: usize = 1024;

    fn of(depth: Vec<f32>, w: usize, h: usize) -> Tiles {
        let across = w.div_ceil(Self::SIDE);
        let mut far = vec![f32::INFINITY; across * h.div_ceil(Self::SIDE)];
        for (k, d) in depth.iter().enumerate() {
            let t = (k / w) / Self::SIDE * across + (k % w) / Self::SIDE;
            if let Some(f) = far.get_mut(t) {
                *f = f.min(*d);
            }
        }
        Tiles { far, depth, across, w, h }
    }

    /// Box `b` through `project`: across, down and toward the camera, each lowest and highest; none where a corner isn't
    /// of finite numbers.
    fn screen(project: &Projector, b: &[P3; 2]) -> Option<[f64; 6]> {
        let corners = (0..8).map(|k| [b[k & 1][0], b[(k >> 1) & 1][1], b[(k >> 2) & 1][2]]).map(|p| project.at(p));
        let mut s = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
        for (x, y, z) in corners {
            if !(x.is_finite() && y.is_finite() && z.is_finite()) {
                return None;
            }
            s = [s[0].min(x), s[1].max(x), s[2].min(y), s[3].max(y), s[4].min(z), s[5].max(z)];
        }
        Some(s)
    }

    /// The farthest depth of each tile again, after more was drawn into the depths.
    fn refresh(&mut self) {
        self.far.iter_mut().for_each(|f| *f = f32::INFINITY);
        for (k, d) in self.depth.iter().enumerate() {
            let t = (k / self.w) / Self::SIDE * self.across + (k % self.w) / Self::SIDE;
            if let Some(f) = self.far.get_mut(t) {
                *f = f.min(*d);
            }
        }
    }

    /// Whether what lies in a box, `s` on the screen ([`Tiles::screen`]; none, and it may), may win a pixel of a picture
    /// through `clip`: it lies in the picture, past the line and before the far clip, and somewhere nearer than the
    /// farthest depth of a tile it covers. Depths are toward the camera, a pixel kept where a surface is nearer than what it
    /// has; a margin covers the rounding of a triangle's depth from its corners' and of the depths kept as `f32`.
    fn may_show_on(&self, clip: &Clip, s: Option<&[f64; 6]>) -> bool {
        let Some(&[x0, x1, y0, y1, z0, z1]) = s else { return true };
        let slack = |z: f64| 1e-3 + 1e-6 * z.abs();
        // Outside the picture, or its crop; cut away before the line; past the far clip.
        let (w, h) = (self.w as f64, self.h as f64);
        if x1 < -1.0
            || y1 < -1.0
            || x0 > w + 1.0
            || y0 > h + 1.0
            || x1 < clip.x.0 - 1.0
            || x0 > clip.x.1 + 1.0
            || y1 < clip.y.0 - 1.0
            || y0 > clip.y.1 + 1.0
        {
            return false;
        }
        if z0 > clip.near + slack(z0) || clip.far.is_some_and(|f| z1 < clip.near - f - slack(z1)) {
            return false;
        }
        // The pixels whose middles it may cover (a pixel is drawn where its middle is in a triangle, to a hair): none, and it shows
        // nowhere.
        let first = |v: f64| (v - 0.5 - 1e-3).ceil().max(0.0);
        let last = |v: f64, n: usize| (v - 0.5 + 1e-3).floor().min(n as f64 - 1.0);
        let (px0, px1, py0, py1) = (first(x0), last(x1, self.w), first(y0), last(y1, self.h));
        if px0 > px1 || py0 > py1 {
            return false;
        }
        let (px0, px1, py0, py1) = (px0 as usize, px1 as usize, py0 as usize, py1 as usize);
        let nearer = |f: f32| z1 + slack(z1) >= f64::from(f);
        // By tile first: nearer than none of its tiles' farthest depths, it is behind everywhere.
        let (tx0, tx1, ty0, ty1) = (px0 / Self::SIDE, px1 / Self::SIDE, py0 / Self::SIDE, py1 / Self::SIDE);
        if !(ty0..=ty1).any(|ty| (tx0..=tx1).any(|tx| self.far.get(ty * self.across + tx).is_none_or(|f| nearer(*f)))) {
            return false;
        }
        // Small, then pixel by pixel: a tile it touches may hold what it is behind and, beside it, what it isn't.
        if (px1 - px0 + 1) * (py1 - py0 + 1) > Self::PIXELS {
            return true;
        }
        (py0..=py1).any(|y| (px0..=px1).any(|x| self.depth.get(y * self.w + x).is_none_or(|d| nearer(*d))))
    }
}

/// Each paintable element's rule for naming the faces of its triangles, of `owners`.
fn rules_of(m: &Model, owners: impl Iterator<Item = Id>) -> HashMap<Id, Rule> {
    let mut rules = HashMap::new();
    for id in owners {
        if rules.contains_key(&id) {
            continue;
        }
        let rule = match m.elements.get(id) {
            Some(Element::Wall(w)) => Some(Rule::Wall(Axis::of(w))),
            Some(e) if matches!(e.category(), "floor" | "roof" | "ceiling") => Some(Rule::Slab),
            Some(e) if paint::faces(e.category()).contains(&"all") => Some(Rule::Whole),
            _ => None,
        };
        if let Some(r) = rule {
            rules.insert(id, r);
        }
    }
    rules
}

/// The scans the model's point clouds place.
fn scans_of(m: &Model) -> Vec<Scan> {
    m.rare("pointcloud")
        .iter()
        .filter_map(|id| {
            let Some(Element::PointCloud(pc)) = m.elements.get(*id) else { return None };
            let cloud = m.clouds.get(&pc.cloud)?;
            let (points, colours) = (cloud.shared_points(), cloud.shared_colours());
            let (lo, hi) = m.cloud_box(*id)?;
            let corners = std::array::from_fn(|k| std::array::from_fn(|axis| if k >> axis & 1 == 0 { lo[axis] } else { hi[axis] }));
            let (sin, cos) = pc.angle.to_radians().sin_cos();
            let at = [pc.at.0, pc.at.1, m.elevation(pc.level) + pc.offset];
            Some(Scan { owner: *id, points, colours, at, turn: (cos, sin), corners })
        })
        .collect()
}

impl Scene {
    /// How many triangles `look` shows: the number [`facets`](Self::facets) yields.
    #[doc(hidden)]
    pub fn shown(&self, look: &Look) -> usize {
        let plain = look.is_plain();
        self.tris.iter().filter(|t| (plain || look.shows(t.owner)) && look.draws_vis(t.vis)).count()
    }

    /// The triangles `look` shows, in the scene's order and as a picture shades them, for a file to hold: what the card's mesh
    /// is made of, less the textures and the edges.
    #[doc(hidden)]
    pub fn facets<'a>(&'a self, look: &'a Look) -> impl Iterator<Item = Facet> + 'a {
        let plain = look.is_plain();
        self.tris.iter().enumerate().filter(move |(_, t)| (plain || look.shows(t.owner)) && look.draws_vis(t.vis)).map(move |(i, t)| {
            let face = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            let curve = self.smooth.get(i).and_then(Option::as_deref);
            // A curve's corners shade across their facets; one that comes out to nothing takes the triangle's own.
            let normals = std::array::from_fn(|k| {
                let n = curve.map_or(face, |n| unit(n[k]));
                if dot(n, n) > 0.5 { n } else { face }
            });
            Facet { owner: t.owner, corners: t.p, normals, color: look.surface(t.owner, t.color), clearness: t.alpha(look).min(100) }
        })
    }

    /// A view of the whole model from `azimuth` and `elevation` (degrees), fitted into the picture.
    pub fn fit(&self, azimuth: f64, elevation: f64, width: u32, height: u32) -> Camera {
        self.fit_in(azimuth, elevation, width, height, &Look::default())
    }

    /// A view of what `look` shows of the model, fitted into the picture.
    pub fn fit_in(&self, azimuth: f64, elevation: f64, width: u32, height: u32, look: &Look) -> Camera {
        let cam = Camera { azimuth, elevation, scale: 1.0, target: [0.0; 3], width, height, lens: None, shift: [0.0; 2], clip: None };
        self.fit_from(cam, look)
    }

    /// What `look` shows of the model, fitted into `cam`'s picture from where it looks and through its lens: in perspective,
    /// with the eye as far back as lets all of it in.
    pub fn fit_from(&self, cam: Camera, look: &Look) -> Camera {
        let fitted = self.fit_parallel(Camera { lens: None, ..cam }, look);
        let Some(lens) = cam.lens else { return fitted };
        let cam = Camera { lens: Some(lens), ..fitted };
        let Some(f) = cam.focal() else { return cam };
        // All of it within a ball round the target, and the ball within the narrower side of the picture.
        let solids = self.tris.iter().filter(|t| look.draws(t)).flat_map(|t| t.p);
        let levels = self.levels.iter().filter(|(id, ..)| look.shows(*id)).flat_map(|(.., c)| *c);
        let scans = self.scans.iter().filter(|s| look.shows(s.owner)).flat_map(|s| s.corners);
        let reach = solids.chain(levels).chain(scans).map(|p| dot(sub(p, cam.target), sub(p, cam.target)).sqrt()).fold(0.0, f64::max);
        let half = (f64::from(cam.width.min(cam.height)) / 2.0 - MARGIN).max(1.0);
        let d = reach.max(1000.0) / (half / f).atan().sin().max(1e-9);
        Camera { scale: (f / d).clamp(1e-6, 50.0), ..cam }
    }

    fn fit_parallel(&self, cam: Camera, look: &Look) -> Camera {
        let (width, height) = (cam.width, cam.height);
        let mut cam = Camera { elevation: cam.elevation.clamp(-89.0, 89.0), scale: 1.0, target: [0.0; 3], ..cam };
        let room = |px: u32| (f64::from(px) - 2.0 * MARGIN).max(1.0);
        let solids = self.tris.iter().filter(|t| look.draws(t)).flat_map(|t| t.p);
        let levels = self.levels.iter().filter(|(id, ..)| look.shows(*id)).flat_map(|(.., c)| *c);
        let scans = self.scans.iter().filter(|s| look.shows(s.owner)).flat_map(|s| s.corners);
        let mut pts: Vec<P3> = solids.chain(levels).chain(scans).collect();
        let Some((target, w, h)) = extent(&cam, &pts) else {
            cam.scale = room(width).min(room(height)) / 10_000.0;
            return cam;
        };
        cam.target = target;
        cam.scale = (room(width) / w.max(1000.0)).min(room(height) / h.max(1000.0)).clamp(1e-6, 50.0);
        // The levels' names take room at the scale that gives: fitted again with their heads.
        pts.extend(self.level_heads(&cam).into_iter().filter(|h| look.shows(h.level)).flat_map(|h| h.corners));
        if let Some((target, w, h)) = extent(&cam, &pts) {
            cam.target = target;
            cam.scale = (room(width) / w.max(1000.0)).min(room(height) / h.max(1000.0)).clamp(1e-6, 50.0);
        }
        cam
    }

    /// Each level's head as `cam` sees it: see [`LevelHead`].
    /// The elements a selection box between pixels `a` and `b` of the picture through `cam` picks, of those `look` shows: from
    /// the left (`crossing` false) those wholly inside it, from the right those it touches too. As Revit's, it goes by where
    /// the model is, not what's in front: what lies behind something else is boxed as well.
    pub fn in_box(&self, cam: &Camera, a: (f64, f64), b: (f64, f64), crossing: bool, look: &Look) -> Vec<Id> {
        let (lo, hi) = (P2(a.0.min(b.0), a.1.min(b.1)), P2(a.0.max(b.0), a.1.max(b.1)));
        let inside = |q: P2| q.0 >= lo.0 && q.0 <= hi.0 && q.1 >= lo.1 && q.1 <= hi.1;
        let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
        // Whether triangle `t`, on screen, overlaps the box: a corner of either in the other, or an edge of each across.
        let touches = |t: &[P2; 3]| {
            t.iter().any(|q| inside(*q))
                || corners.iter().any(|c| geom::contains(t, *c))
                || geom::ring_edges(t).any(|(p, q)| {
                    geom::ring_edges(&corners).any(|(c, d)| {
                        geom::line_intersection(p, q, c, d).is_some_and(|x| {
                            let on = |a: P2, b: P2| geom::dist_to_segment(x, a, b) <= 1e-9 * a.dist(b).max(1.0);
                            on(p, q) && on(c, d)
                        })
                    })
                })
        };
        // Each element: whether all of it is inside, and whether any of it is.
        let mut seen: BTreeMap<Id, (bool, bool)> = BTreeMap::new();
        for t in self.tris.iter().filter(|t| look.draws(t) && !crate::link::is_linked(t.owner)) {
            let on = t.p.map(|p| {
                let (x, y, _) = cam.project(p);
                P2(x, y)
            });
            if on.iter().any(|q| !(q.0.is_finite() && q.1.is_finite())) {
                continue;
            }
            let e = seen.entry(t.owner).or_insert((true, false));
            *e = (e.0 && on.iter().all(|q| inside(*q)), e.1 || (crossing && touches(&on)) || on.iter().any(|q| inside(*q)));
        }
        seen.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(id, _)| id).collect()
    }

    /// The triangle of what `look` shows nearest the camera at pixel `at`, and how near (as `project` says): none where
    /// nothing is, or only what the section box cuts away.
    fn hit(&self, cam: &Camera, at: (f64, f64), look: &Look) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (i, t) in self.tris.iter().enumerate().filter(|(_, t)| look.draws(t) && !crate::link::is_linked(t.owner)) {
            let [a, b, c] = t.p.map(|p| cam.project(p));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            if area.is_nan() || area.abs() <= 1e-12 {
                continue;
            }
            // Its share of each corner at the pixel: all three at least none, it's inside.
            let wa = ((b.0 - at.0) * (c.1 - at.1) - (b.1 - at.1) * (c.0 - at.0)) / area;
            let wb = ((c.0 - at.0) * (a.1 - at.1) - (c.1 - at.1) * (a.0 - at.0)) / area;
            let wc = 1.0 - wa - wb;
            if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                continue;
            }
            let z = wa * a.2 + wb * b.2 + wc * c.2;
            if z.is_finite() && best.is_none_or(|(d, _)| z > d) && kept(look, cam.at_depth(at, z)) {
                best = Some((z, i));
            }
        }
        best
    }

    /// The face of the model at pixel `at` through `cam`, of what `look` shows: where on it, which way it faces (toward the
    /// camera), whose it is, and its triangles: those of the element lying in its plane. As a work plane is set on a surface.
    pub fn face_at(&self, cam: &Camera, at: (f64, f64), look: &Look) -> Option<Face> {
        let (z, i) = self.hit(cam, at, look)?;
        let t = self.tris.get(i)?;
        let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
        let (back, ..) = cam.axes();
        let normal = if dot(n, back) < 0.0 { mul(n, -1.0) } else { n };
        let plane = self.planes.get(i).copied();
        let tris = self.tris.iter().zip(&self.planes).filter(|(o, k)| o.owner == t.owner && Some(**k) == plane).map(|(o, _)| o.p).collect();
        Some(Face { at: cam.at_depth(at, z), normal, owner: t.owner, tris })
    }

    /// The corner of what `look` shows nearest pixel `at`, within `reach` pixels, that the camera sees (nothing nearer hides
    /// it): what a picked point snaps to, as a dimension's ends snap to corners in plan.
    pub fn corner_near(&self, cam: &Camera, at: (f64, f64), reach: f64, look: &Look) -> Option<P3> {
        let mut near: Vec<(f64, P3)> = self
            .tris
            .iter()
            .filter(|t| look.draws(t) && !crate::link::is_linked(t.owner))
            .flat_map(|t| t.p)
            .filter_map(|p| {
                let (x, y, _) = cam.project(p);
                let d = ((x - at.0).powi(2) + (y - at.1).powi(2)).sqrt();
                (d <= reach).then_some((d, p))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.dedup_by(|a, b| sub(a.1, b.1).iter().all(|v| v.abs() < TOL));
        // Seen where what's in front at its pixel is it, or no nearer than it: to a millimetre, and a little more for
        // the pixel's slant across a face seen edge on.
        near.into_iter().take(16).map(|(_, p)| p).find(|p| {
            let (x, y, z) = cam.project(*p);
            kept(look, *p) && self.hit(cam, (x, y), look).is_none_or(|(d, _)| d <= z + 1.0 + 2.0 / cam.scale.max(1e-9))
        })
    }

    /// The edge of what `look` shows that passes nearest pixel `at`, within `reach` pixels, where the camera sees it: its
    /// ends, and its point nearest the pixel. What a dimension in a 3D view picks to measure square to.
    pub fn edge_near(&self, cam: &Camera, at: (f64, f64), reach: f64, look: &Look) -> Option<([P3; 2], P3)> {
        let mut near: Vec<(f64, [P3; 2], P3)> = self
            .edges()
            .iter()
            .filter(|(i, _)| self.tris.get(*i).is_some_and(|t| look.draws(t) && !crate::link::is_linked(t.owner)))
            .filter_map(|&(_, [a, b])| {
                let [a, b] = cam.in_front(a, b)?;
                let ((ax, ay, _), (bx, by, _)) = (cam.project(a), cam.project(b));
                let (dx, dy) = (bx - ax, by - ay);
                let l = dx * dx + dy * dy;
                let t = if l > 0.0 { (((at.0 - ax) * dx + (at.1 - ay) * dy) / l).clamp(0.0, 1.0) } else { 0.0 };
                let d = (ax + dx * t - at.0).hypot(ay + dy * t - at.1);
                (d.is_finite() && d <= reach).then(|| (d, [a, b], add(a, mul(sub(b, a), t))))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        // Seen where its point is: nothing nearer the camera there, as a corner is seen.
        near.into_iter().take(16).map(|(_, e, p)| (e, p)).find(|(_, p)| {
            let (x, y, z) = cam.project(*p);
            kept(look, *p) && self.hit(cam, (x, y), look).is_none_or(|(d, _)| d <= z + 1.0 + 2.0 / cam.scale.max(1e-9))
        })
    }

    pub fn level_heads(&self, cam: &Camera) -> Vec<LevelHead> {
        heads(&self.levels, cam)
    }

    /// The picture `cam` takes. Elements in `tints` show halfway to their tint, and their edges in it.
    pub fn render(&self, cam: &Camera, tints: &[(Id, Rgb)]) -> Image {
        self.render_in(cam, tints, &Look::default())
    }

    /// The picture `cam` takes of what `look` shows, in the colours it gives.
    pub fn render_in(&self, cam: &Camera, tints: &[(Id, Rgb)], look: &Look) -> Image {
        self.picture(cam, None, tints, look)
    }

    /// A section's or an elevation's picture: what `clip` keeps, in hidden line (faces the paper's
    /// colour, edges dark), for the cut to be drawn over.
    pub fn cut(&self, cam: &Camera, clip: &Clip, tints: &[(Id, Rgb)]) -> Image {
        self.cut_in(cam, clip, tints, &Look::default())
    }

    /// A section's or an elevation's picture of what `look` shows.
    pub fn cut_in(&self, cam: &Camera, clip: &Clip, tints: &[(Id, Rgb)], look: &Look) -> Image {
        self.picture(cam, Some(clip), tints, look)
    }

    /// The triangles `keep` takes, drawn into `into`'s depths and hits: the nearest at each pixel of a
    /// `size` picture through `cam`; with `front`, only where they are nearer than its depths. Where
    /// `lights` says how lit a triangle's corners are (a curve's), its pixels take the blend of the three.
    #[doc(hidden)]
    pub fn raster(
        &self,
        (cam, clip, cut): (&Camera, Option<&Clip>, Option<&Boxed>),
        (w, h): (usize, usize),
        keep: impl Fn(&Tri) -> bool,
        front: Option<&[f32]>,
        lights: &dyn Fn(usize) -> Option<[f64; 3]>,
        (depth, hit, lit): (&mut [f32], &mut [u32], &mut [f32]),
    ) {
        self.raster_cancellable((cam, clip, cut), (w, h), keep, front, lights, (depth, hit, lit), &|| false);
    }

    #[allow(clippy::too_many_arguments)]
    fn raster_cancellable(
        &self,
        (cam, clip, cut): (&Camera, Option<&Clip>, Option<&Boxed>),
        (w, h): (usize, usize),
        keep: impl Fn(&Tri) -> bool,
        front: Option<&[f32]>,
        lights: &dyn Fn(usize) -> Option<[f64; 3]>,
        (depth, hit, lit): (&mut [f32], &mut [u32], &mut [f32]),
        cancelled: &(dyn Fn() -> bool + Sync),
    ) {
        // Project once, then bin in stable triangle order. Each job owns disjoint rows:
        // no locks, atomic depth writes, duplicate framebuffers, or depth-tie races.
        const ROWS: usize = 32;
        struct Projected {
            id: u32,
            p: [(f64, f64, f64); 3],
            area: f64,
            x: (usize, usize),
            y: (usize, usize),
            corners: Option<[f64; 3]>,
            /// Its face turned away from the camera inside a section box: drawn as the cut face ([`capped`]).
            cap: bool,
        }
        let project = cam.projector();
        let mut tris = Vec::new();
        let mut bins = vec![Vec::new(); h.div_ceil(ROWS)];
        for (i, t) in self.tris.iter().enumerate() {
            if i % 256 == 0 && cancelled() {
                return;
            }
            if !keep(t) || cut.is_some_and(|k| k.misses(t)) {
                continue;
            }
            // In perspective, what is in front of the eye: a triangle, or the one or two its near part makes.
            let (parts, count) = project.in_front(&t.p);
            let lit = lights(i);
            let cap = capped(cam, cut, front, t);
            for part in parts.iter().take(count) {
                let [a, b, c] = weighed(&t.p, part).map(|p| project.at(p));
                let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                if area.is_nan() || area.abs() <= 1e-12 {
                    continue;
                }
                let (x0, x1) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
                let (y0, y1) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
                if !(x1 >= 0.0 && x0 < w as f64 && y1 >= 0.0 && y0 < h as f64) {
                    continue;
                }
                let x = (x0.max(0.0) as usize, x1.min(w as f64 - 1.0) as usize);
                let y = (y0.max(0.0) as usize, y1.min(h as f64 - 1.0) as usize);
                let corners = lit.map(|l| part.map(|w| w[0] * l[0] + w[1] * l[1] + w[2] * l[2]));
                let n = tris.len();
                tris.push(Projected { id: i as u32, p: [a, b, c], area, x, y, corners, cap });
                for bin in bins.iter_mut().take(y.1 / ROWS + 1).skip(y.0 / ROWS) {
                    bin.push(n);
                }
            }
        }
        let stripe = |band: usize, (depth, hit, lit): (&mut [f32], &mut [u32], &mut [f32])| {
            let first = band * ROWS;
            let end = (first + ROWS).min(h);
            for &i in &bins[band] {
                if cancelled() {
                    return;
                }
                let t = &tris[i];
                let [a, b, c] = t.p;
                for py in t.y.0.max(first)..=t.y.1.min(end - 1) {
                    for px in t.x.0..=t.x.1 {
                        let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                        let wa = ((b.0 - x) * (c.1 - y) - (b.1 - y) * (c.0 - x)) / t.area;
                        let wb = ((c.0 - x) * (a.1 - y) - (c.1 - y) * (a.0 - x)) / t.area;
                        let wc = 1.0 - wa - wb;
                        if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                            continue;
                        }
                        let z = wa * a.2 + wb * b.2 + wc * c.2;
                        if clip.is_some_and(|k| {
                            z > k.near || k.far.is_some_and(|f| z < k.near - f) || x < k.x.0 || x > k.x.1 || y < k.y.0 || y > k.y.1
                        }) || project.clipped(z)
                            || cut.is_some_and(|k| !k.keeps(x, y, z))
                        {
                            continue;
                        }
                        let z = if t.cap { z - CAP_BEHIND } else { z } as f32;
                        let global = py * w + px;
                        if front.is_some_and(|f| f.get(global).is_some_and(|d| z <= *d)) {
                            continue;
                        }
                        let k = (py - first) * w + px;
                        if let (Some(d), Some(s)) = (depth.get_mut(k), hit.get_mut(k))
                            && z > *d
                        {
                            (*d, *s) = (z, t.id);
                            if let (Some([ka, kb, kc]), Some(slot)) = (t.corners, lit.get_mut(k)) {
                                *slot = (wa * ka + wb * kb + wc * kc) as f32;
                            }
                        }
                    }
                }
            }
        };
        // A persistent bounded pool; keep CPU capacity for input, autosave and the OS.
        // If threads cannot be created (including restricted targets), render sequentially.
        let pool = (w * h >= 256 * 256 && tris.len() >= 128).then(render_pool).flatten();
        let size = w * ROWS;
        if let Some(pool) = pool {
            use rayon::prelude::*;
            pool.install(|| {
                depth
                    .par_chunks_mut(size)
                    .zip(hit.par_chunks_mut(size))
                    .zip(lit.par_chunks_mut(size))
                    .enumerate()
                    .for_each(|(i, ((d, h), l))| stripe(i, (d, h, l)))
            });
        } else {
            for (i, ((d, h), l)) in depth.chunks_mut(size).zip(hit.chunks_mut(size)).zip(lit.chunks_mut(size)).enumerate() {
                stripe(i, (d, h, l));
            }
        }
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn raster_reference(
        &self,
        (cam, clip, cut): (&Camera, Option<&Clip>, Option<&Boxed>),
        (w, h): (usize, usize),
        keep: impl Fn(&Tri) -> bool,
        front: Option<&[f32]>,
        lights: &dyn Fn(usize) -> Option<[f64; 3]>,
        (depth, hit, lit): (&mut [f32], &mut [u32], &mut [f32]),
    ) {
        for (i, t) in self.tris.iter().enumerate() {
            if !keep(t) || cut.is_some_and(|k| k.misses(t)) {
                continue;
            }
            let [a, b, c] = t.p.map(|p| cam.project(p));
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            // Edge-on (or not a number): nothing to fill.
            if area.is_nan() || area.abs() <= 1e-12 {
                continue;
            }
            let (x0, x1) = (a.0.min(b.0).min(c.0), a.0.max(b.0).max(c.0));
            let (y0, y1) = (a.1.min(b.1).min(c.1), a.1.max(b.1).max(c.1));
            if !(x1 >= 0.0 && x0 < w as f64 && y1 >= 0.0 && y0 < h as f64) {
                continue;
            }
            let corners = lights(i);
            let cap = capped(cam, cut, front, t);
            for py in y0.max(0.0) as usize..=y1.min(h as f64 - 1.0) as usize {
                for px in x0.max(0.0) as usize..=x1.min(w as f64 - 1.0) as usize {
                    let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
                    // Barycentric weights; a hair's tolerance so neighbours leave no gap between them.
                    let wa = ((b.0 - x) * (c.1 - y) - (b.1 - y) * (c.0 - x)) / area;
                    let wb = ((c.0 - x) * (a.1 - y) - (c.1 - y) * (a.0 - x)) / area;
                    let wc = 1.0 - wa - wb;
                    if wa < -1e-9 || wb < -1e-9 || wc < -1e-9 {
                        continue;
                    }
                    let z = wa * a.2 + wb * b.2 + wc * c.2;
                    if clip.is_some_and(|k| {
                        z > k.near || k.far.is_some_and(|f| z < k.near - f) || x < k.x.0 || x > k.x.1 || y < k.y.0 || y > k.y.1
                    }) || cam.projector().clipped(z)
                        || cut.is_some_and(|k| !k.keeps(x, y, z))
                    {
                        continue;
                    }
                    let z = if cap { z - CAP_BEHIND } else { z } as f32;
                    let k = py * w + px;
                    if front.is_some_and(|f| f.get(k).is_some_and(|d| z <= *d)) {
                        continue;
                    }
                    if let (Some(d), Some(s)) = (depth.get_mut(k), hit.get_mut(k))
                        && z > *d
                    {
                        (*d, *s) = (z, i as u32);
                        if let (Some([ka, kb, kc]), Some(slot)) = (corners, lit.get_mut(k)) {
                            *slot = (wa * ka + wb * kb + wc * kc) as f32;
                        }
                    }
                }
            }
        }
    }

    /// The edges of the model's surfaces: where two meet at an angle (not in one plane, nor one curve) and where one ends,
    /// each as the triangle it is an edge of and its ends, in the order of their triangles.
    #[doc(hidden)]
    pub fn edges(&self) -> &[(usize, [P3; 2])] {
        self.edges_cancellable(&|| false).unwrap_or_default()
    }

    /// [`edges`](Self::edges), abandoned for a superseded view: none then, and nothing kept, so the next to ask finds them.
    #[doc(hidden)]
    pub fn edges_cancellable(&self, cancelled: &dyn Fn() -> bool) -> Option<&[(usize, [P3; 2])]> {
        if let Some(edges) = self.edges.get() {
            return Some(edges);
        }
        if cancelled() {
            return None;
        }
        // Each triangle's sides by their corners to a micron, as `facets` names them, sorted: a side's triangles lie together, in
        // the triangles' order. A list and not a map, which on a large model is most of a wireframe's wait and many times the room.
        let corner = |t: &Tri, k: usize| t.p.get(k % 3).copied().unwrap_or_default();
        let q = |p: P3| p.map(|x| (x * 1e3).round() as i64);
        let mut sides: Vec<([i64; 3], [i64; 3], usize)> = Vec::with_capacity(self.tris.len().saturating_mul(3));
        for (i, t) in self.tris.iter().enumerate() {
            if i % 8192 == 0 && cancelled() {
                return None;
            }
            for k in 0..3 {
                let (qa, qb) = (q(corner(t, k)), q(corner(t, k + 1)));
                if qa != qb {
                    sides.push((qa.min(qb), qa.max(qb), i * 3 + k));
                }
            }
        }
        if cancelled() {
            return None;
        }
        let pool = (sides.len() >= 1 << 16).then(render_pool).flatten();
        match pool {
            Some(pool) => {
                use rayon::prelude::*;
                pool.install(|| sides.par_sort_unstable());
            }
            None => sides.sort_unstable(),
        }
        if cancelled() {
            return None;
        }
        let plane = |side: usize| self.planes.get(side / 3).copied();
        let mut edges: Vec<(usize, [P3; 2])> = sides
            .chunk_by(|a, b| (a.0, a.1) == (b.0, b.1))
            .filter(|on| !matches!(on, [x, y] if plane(x.2) == plane(y.2)))
            .filter_map(|on| {
                let (i, k) = (on.first()?.2 / 3, on.first()?.2 % 3);
                let t = self.tris.get(i)?;
                Some((i, [corner(t, k), corner(t, k + 1)]))
            })
            .collect();
        drop(sides);
        if cancelled() {
            return None;
        }
        let order =
            |x: &(usize, [P3; 2]), y: &(usize, [P3; 2])| x.0.cmp(&y.0).then(x.1.partial_cmp(&y.1).unwrap_or(std::cmp::Ordering::Equal));
        match pool {
            Some(pool) => {
                use rayon::prelude::*;
                pool.install(|| edges.par_sort_by(order));
            }
            None => edges.sort_by(order),
        }
        Some(self.edges.get_or_init(|| edges))
    }

    /// The picture `cam` takes in wireframe: paper, and every edge of what `look` shows, in the colour it gives them (a
    /// tinted element's in its tint, over the rest). A pixel within two of a line picks the element the line is of.
    fn wireframe(&self, cam: &Camera, tints: &[(Id, Rgb)], look: &Look) -> Image {
        let (w, h) = (cam.width.clamp(1, MAX_SIDE) as usize, cam.height.clamp(1, MAX_SIDE) as usize);
        let tints: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
        let mut rgba = vec![255u8; w * h * 4];
        let mut hit = vec![u32::MAX; w * h];
        for tinted in [false, true] {
            for &(i, [a, b]) in self.edges() {
                let Some(owner) = self.tris.get(i).filter(|t| look.draws(t)).map(|t| t.owner).filter(|o| tints.contains_key(o) == tinted)
                else {
                    continue;
                };
                let Rgb(r, g, bl) = tints.get(&owner).copied().unwrap_or_else(|| look.edge(owner, EDGE));
                let Some((p, q)) = wire_line(cam, look.section_box(), [a, b], (w, h)) else { continue };
                for (x, y) in wire_pixels(p, q) {
                    for (dx, dy) in (-2..=2).flat_map(|dx| (-2..=2).map(move |dy| (dx, dy))) {
                        let (px, py) = (x + dx, y + dy);
                        if (0..w as i64).contains(&px) && (0..h as i64).contains(&py) {
                            let at = py as usize * w + px as usize;
                            if let Some(slot) = hit.get_mut(at) {
                                *slot = i as u32;
                            }
                            if (dx, dy) == (0, 0)
                                && let Some(px) = rgba.get_mut(4 * at..4 * at + 3)
                            {
                                px.copy_from_slice(&[r, g, bl]);
                            }
                        }
                    }
                }
            }
        }
        Image {
            width: w as u32,
            height: h as u32,
            rgba,
            hit: Arc::new(hit),
            depth: Arc::new(Vec::new()),
            owners: Arc::new(self.tris.iter().map(|t| t.owner).collect()),
            faces: Arc::clone(&self.faces),
            section_paint: None,
        }
    }

    /// The points of the scans `keep` takes that `clip` keeps, drawn into `depth` and `hit` as dots [`DOT`] wide where they are
    /// nearer than what is there, no more than [`DRAWN`] of them; each pixel's colour, which is none where no scan is kept.
    fn dots(
        &self,
        (cam, clip, cut): (&Camera, Option<&Clip>, Option<&Boxed>),
        (w, h): (usize, usize),
        keep: impl Fn(&Scan) -> bool,
        plain: bool,
        look: &Look,
        (depth, hit): (&mut [f32], &mut [u32]),
    ) -> Vec<Rgb> {
        let kept: Vec<(usize, &Scan)> = self.scans.iter().enumerate().filter(|(_, s)| keep(s)).collect();
        if kept.is_empty() {
            return Vec::new();
        }
        let mut dots = vec![PAPER; w * h];
        let step = every(kept.iter().map(|(_, s)| s.points.len()).sum());
        for (n, scan) in kept {
            let slot = (self.tris.len() + n) as u32;
            for (i, p) in scan.points.iter().enumerate().step_by(step) {
                let at = scan.place(*p);
                if cut.is_some_and(|k| !k.holds(at)) {
                    continue;
                }
                let (x, y, z) = cam.project(at);
                if clip.is_some_and(|k| {
                    z > k.near || k.far.is_some_and(|f| z < k.near - f) || x < k.x.0 || x > k.x.1 || y < k.y.0 || y > k.y.1
                }) {
                    continue;
                }
                let Some((x0, y0)) = dot_at((x, y, z), (w, h)) else { continue };
                let colour = scan.colour(i, plain, look);
                let z = z as f32;
                for py in y0.max(0)..(y0 + DOT as i64).min(h as i64) {
                    for px in x0.max(0)..(x0 + DOT as i64).min(w as i64) {
                        let k = py as usize * w + px as usize;
                        if let (Some(d), Some(s), Some(c)) = (depth.get_mut(k), hit.get_mut(k), dots.get_mut(k))
                            && z > *d
                        {
                            (*d, *s, *c) = (z, slot, colour);
                        }
                    }
                }
            }
        }
        dots
    }

    fn picture(&self, cam: &Camera, clip: Option<&Clip>, tints: &[(Id, Rgb)], look: &Look) -> Image {
        self.picture_cancellable(cam, clip, tints, look, &|| false)
    }

    pub(crate) fn picture_cancellable(
        &self,
        cam: &Camera,
        clip: Option<&Clip>,
        tints: &[(Id, Rgb)],
        look: &Look,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Image {
        if cancelled() {
            return Image::empty();
        }
        // A section's picture is hidden line, whatever the view says.
        let style = if clip.is_some() { VisualStyle::HiddenLine } else { look.style() };
        if style == VisualStyle::Wireframe {
            return self.wireframe(cam, tints, look);
        }
        let flat = style != VisualStyle::Shaded;
        let plain = look.is_plain();
        let (w, h) = (cam.width.clamp(1, MAX_SIDE) as usize, cam.height.clamp(1, MAX_SIDE) as usize);
        let (back, right, up) = cam.axes();
        // The side of a face the camera sees: in perspective, the side the eye is on.
        let eye = cam.eye();
        let toward = |p: P3| eye.map_or(back, |e| sub(e, p));
        // Lit from over the viewer's left shoulder; a face turned away from the light keeps 62%.
        let light = unit(add(add(mul(right, -0.35), mul(up, 0.55)), mul(back, 0.75)));
        let lit = |n: P3| 0.62 + 0.38 * dot(n, light).max(0.0);
        let paint = |color: Rgb, k: f64| {
            let c = |v: u8| (f64::from(v) * k).round() as u8;
            Rgb(c(color.0), c(color.1), c(color.2))
        };
        // The colour a surface is shaded in: the view's, unless it gives none.
        let colour = |t: &Tri| if plain { t.color } else { look.surface(t.owner, t.color) };
        // A curve's corners are lit by their own normals, and its pixels take the blend of the three;
        // the faces of a section are paper.
        let smooth: &[Option<Arc<[P3; 3]>>] = if flat { &[] } else { &self.smooth };
        // How lit each corner of a curve is: its normals turned to the viewer as its face is.
        let corner_light = |i: usize| {
            let (t, n) = (self.tris.get(i)?, smooth.get(i)?.as_deref()?);
            let face = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
            Some(n.map(|n| lit(if dot(face, toward(t.p[0])) < 0.0 { mul(n, -1.0) } else { n })))
        };
        // What the view sees through is drawn over what lies behind it, after the opaque surfaces: the
        // nearest of them at each pixel, where nothing opaque is nearer. (Not in a section's hidden line.)
        let see_through = |t: &Tri| !flat && t.alpha(look) > 0;
        let shown =
            |t: &Tri| ((plain || look.shows(t.owner)) && look.draws_vis(t.vis)) && !(clip.is_some() && self.masses.contains(&t.owner));
        let mut depth = vec![f32::NEG_INFINITY; w * h];
        let mut hit = vec![u32::MAX; w * h];
        let mut curve_lit = vec![0.0; w * h];
        let opaque = |t: &Tri| shown(t) && !see_through(t);
        // A 3D view's section box keeps what lies inside it; a section has its own clip.
        let cut = look.section_box().filter(|_| clip.is_none()).map(|b| Boxed::new(cam, b));
        let cut = cut.as_ref();
        self.raster_cancellable((cam, clip, cut), (w, h), opaque, None, &corner_light, (&mut depth, &mut hit, &mut curve_lit), cancelled);
        if cancelled() {
            return Image::empty();
        }
        // The view's edges, silhouettes and ambient shadows, from the opaque surfaces' depths before a scan's dots are in them;
        // a section's picture draws as ever. Hidden Line draws its edges whatever the view says.
        let display = if clip.is_some() { DisplayOptions::default() } else { look.display() };
        let shadows = display.ambient_shadows.then(|| ambient(&depth, (w, h), cam));
        // Under a section box, the faces turned away from the camera are the cut faces (`capped`): lit as if facing it, in one
        // flat colour, and one plane each element.
        let caps: Vec<bool> = self.tris.iter().map(|t| !see_through(t) && capped(cam, cut, None, t)).collect();
        let plane_of = |t: usize| match (caps.get(t), self.tris.get(t)) {
            (Some(true), Some(tri)) => Some(CAP_PLANE | tri.owner as usize),
            _ => self.planes.get(t).copied(),
        };
        let outlined = (display.silhouette > 0).then(|| {
            let plane = |k: usize| hit.get(k).and_then(|t| plane_of(*t as usize));
            outlines(&depth, &plane, (w, h), cam, display.silhouette.min(MAX_SILHOUETTE))
        });
        let edges = display.edges || flat;
        // The scans' points, each a dot in its colour where nothing opaque is nearer; in a section's picture, those between its
        // line and its far clip. A pixel a point shows holds its scan's slot, after the triangles', and the point's colour.
        let first = self.tris.len();
        let dots = self.dots((cam, clip, cut), (w, h), |s| plain || look.shows(s.owner), plain, look, (&mut depth, &mut hit));
        // Only a picture with something in it to see through needs the second pass, and its buffers.
        let through_any = self.tris.iter().any(|t| shown(t) && see_through(t));
        let (mut seen, mut seen_lit, mut near) = (Vec::new(), Vec::new(), Vec::new());
        if through_any {
            near = vec![f32::NEG_INFINITY; w * h];
            (seen, seen_lit) = (vec![u32::MAX; w * h], vec![0.0; w * h]);
            let clear = |t: &Tri| shown(t) && see_through(t);
            self.raster((cam, clip, cut), (w, h), clear, Some(&depth), &corner_light, (&mut near, &mut seen, &mut seen_lit));
        }
        // How lit each triangle is, as its face turns to the light; and its flat colour, which a section's is the paper's.
        let light: Vec<f64> = self
            .tris
            .iter()
            .map(|t| {
                let n = unit(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])));
                lit(if dot(n, toward(t.p[0])) < 0.0 { mul(n, -1.0) } else { n })
            })
            .collect();
        let shade: Vec<Rgb> = self.tris.iter().zip(&light).map(|(t, l)| if flat { PAPER } else { paint(colour(t), *l) }).collect();
        // A textured triangle's colour at pixel `k`, where its surface lies `z` toward the camera: the texture at that point of
        // its plane, for a pixel's width of it. None for any other, or where the view colours it its own way, or in a section.
        let texturing = !flat && self.skins.iter().any(Option::is_some);
        let foot = 1.0 / cam.scale;
        let textured = |t: usize, k: usize, z: f32| -> Option<Rgb> {
            if !texturing {
                return None;
            }
            let tri = self.tris.get(t)?;
            let sampler = self.skins.get(tri.texture() as usize)?.as_ref()?;
            if !plain && colour(tri) != tri.color {
                return None;
            }
            let [across, up] = basis(tri);
            let (x, y) = ((k % w) as f64 + 0.5, (k / w) as f64 + 0.5);
            let at = cam.point_at((x, y), f64::from(z));
            Some(sampler.at(dot(at, across), dot(at, up), foot))
        };
        // What triangle `t` shows at pixel `k`, whose surface is `depths` there: a curve's, the blend of its corners' lighting; a
        // flat face's, its own shade; a texture's colour lit as the face is.
        let cap_light = lit(back);
        let shaded = |t: usize, k: usize, curve: &[f32], depths: &[f32]| {
            let tri = self.tris.get(t)?;
            if caps.get(t) == Some(&true) {
                return Some(if flat { PAPER } else { paint(colour(tri), cap_light) });
            }
            let corners = curve.get(k).filter(|_| smooth.get(t).is_some_and(Option::is_some));
            let skin = textured(t, k, depths.get(k).copied().unwrap_or_default());
            Some(match (skin, corners) {
                (Some(c), Some(l)) => paint(c, f64::from(*l)),
                (Some(c), None) => paint(c, *light.get(t)?),
                (None, Some(l)) => paint(colour(tri), f64::from(*l)),
                (None, None) => *shade.get(t)?,
            })
        };
        let tints: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
        let tri = |k: usize| hit.get(k).map(|&t| t as usize).filter(|t| *t < self.tris.len());
        let through = |k: usize| seen.get(k).map(|&t| t as usize).filter(|t| *t < self.tris.len());
        let plane = |k: usize| tri(k).and_then(plane_of);
        // The scan a point at pixel `k` is of, and its colour.
        let dot = |k: usize| {
            let scan = self.scans.get((*hit.get(k)? as usize).checked_sub(first)?)?;
            Some((scan.owner, *dots.get(k)?))
        };
        let pixel = |k: usize, tints: &BTreeMap<Id, Rgb>| {
            let (px, py) = (k % w, k / w);
            let here = plane(k);
            // A surface's edge is where it meets another surface or nothing; a point is not one.
            let differs = |n: usize| dot(n).is_none() && plane(n) != here;
            let outline = outlined.as_ref().is_some_and(|o| o.get(k) == Some(&true));
            let edge = dot(k).is_none() && (outline || (edges && ((px + 1 < w && differs(k + 1)) || (py + 1 < h && differs(k + w)))));
            let owner_at = |k: usize| tri(k).and_then(|t| self.tris.get(t)).map(|t| t.owner);
            let tint = owner_at(k).and_then(|o| tints.get(&o));
            // An edge is drawn in the colour the view gives the edges of what it bounds.
            let edge_of = || [k, k + 1, k + w].into_iter().find_map(owner_at).map_or(EDGE, |o| look.edge(o, EDGE));
            let c = match (edge, dot(k), tri(k).and_then(|t| shaded(t, k, &curve_lit, &depth))) {
                (_, Some((owner, c)), _) => tints.get(&owner).map_or(c, |t| mix(c, *t)),
                (true, ..) => tint.copied().unwrap_or_else(edge_of),
                (false, _, Some(s)) => {
                    let s = shadows.as_ref().and_then(|v| v.get(k)).map_or(s, |l| paint(s, f64::from(*l)));
                    tint.map_or(s, |c| mix(s, *c))
                }
                (false, _, None) => PAPER,
            };
            // Over it, the nearest surface seen through, as much of it as the view lets show (and a
            // tinted one always a little, or selecting it would show nothing).
            let c = match through(k)
                .and_then(|t| Some((shaded(t, k, &seen_lit, &near)?, self.tris.get(t)?.owner, self.tris.get(t)?.alpha(look))))
            {
                Some((s, owner, alpha)) => {
                    let tint = tints.get(&owner);
                    let weight = 1.0 - f64::from(alpha) / 100.0;
                    blend(c, tint.map_or(s, |c| mix(s, *c)), if tint.is_some() { weight.max(0.35) } else { weight })
                }
                None => c,
            };
            [c.0, c.1, c.2, 255]
        };
        let mut rgba = vec![0; w * h * 4];
        let fill = |offset: usize, bytes: &mut [u8]| {
            if cancelled() {
                return;
            }
            for (i, out) in bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                out.copy_from_slice(&pixel(offset + i, &tints));
            }
        };
        if let Some(pool) = (w * h >= 256 * 256).then(render_pool).flatten() {
            use rayon::prelude::*;
            const PIXELS: usize = 8192;
            pool.install(|| rgba.par_chunks_mut(4 * PIXELS).enumerate().for_each(|(i, out)| fill(i * PIXELS, out)));
        } else {
            fill(0, &mut rgba);
        }

        // Sections are opaque hidden-line pictures, including glass. Retain only their
        // untinted paint and edge mask; picking already retains ownership. No scene is held.
        let section_paint = clip.map(|_| {
            let base = if tints.is_empty() {
                rgba.clone()
            } else {
                let empty = BTreeMap::new();
                (0..w * h).flat_map(|k| pixel(k, &empty)).collect()
            };
            let edges = (0..w * h)
                .map(|k| {
                    let (px, py) = (k % w, k / w);
                    let differs = |n| dot(n).is_none() && plane(n) != plane(k);
                    dot(k).is_none() && ((px + 1 < w && differs(k + 1)) || (py + 1 < h && differs(k + w)))
                })
                .collect();
            SectionPaint { base, edges }
        });

        // What a pixel shows, for picking: a surface seen through is the one in front.
        if through_any {
            hit = hit.iter().zip(&seen).map(|(h, s)| if *s == u32::MAX { *h } else { *s }).collect();
        }
        let mut owners: Vec<Id> = self.tris.iter().map(|t| t.owner).chain(self.scans.iter().map(|s| s.owner)).collect();
        // Over it, each level's plane outlined where nothing nearer hides it (not in a section's picture,
        // which draws its own), picked as the level.
        // Unselected, the two edges nearer the camera are left out, as in Revit; selected, all four.
        // Its head goes past the far edges' right end: its name lettered, all of it picking the level.
        if clip.is_none() {
            let color = |id: Id| (plain || look.shows(id)).then(|| tints.get(&id).map_or((look.edge(id, DATUM), false), |c| (*c, true)));
            // A section box leaves out a level whose plane is above or below it, and cuts the rest to its sides.
            let levels = cut.map_or_else(|| self.levels.clone(), |k| levels_in(&self.levels, (k.lo, k.hi)));
            for marks in level_marks(&levels, cam, &color) {
                let (c, slot) = (marks.color, owners.len() as u32);
                owners.push(marks.id);
                let mut paint = |k: usize| {
                    if let (Some(px), Some(t)) = (rgba.get_mut(4 * k..4 * k + 3), hit.get_mut(k)) {
                        px.copy_from_slice(&[c.0, c.1, c.2]);
                        *t = slot;
                    }
                };
                for ([a, b], wide) in &marks.lines {
                    outline((cam.project(*a), cam.project(*b)), (w, h), *wide, &|z| cam.slack(1.5, z), &depth, &mut paint);
                }
                // The rest of the head picks the level too, unpainted: where all of it is in front of the eye.
                let ring: Vec<P2> = marks.head.iter().map(|p| cam.project(*p)).map(|(x, y, _)| P2(x, y)).collect();
                if !ring.iter().all(|q| q.0.is_finite() && q.1.is_finite()) {
                    continue;
                }
                let z = marks.head.iter().map(|p| cam.project(*p).2).sum::<f64>() / 4.0;
                let slack = cam.slack(1.5, z);
                let (lo, hi) = ring.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
                    (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
                });
                let (x0, x1) = (lo.0.max(0.0).floor() as usize, hi.0.min(w as f64 - 1.0).max(0.0) as usize);
                let (y0, y1) = (lo.1.max(0.0).floor() as usize, hi.1.min(h as f64 - 1.0).max(0.0) as usize);
                for py in y0..=y1.min(y0.saturating_add(4 * MAX_SIDE as usize)) {
                    for px in x0..=x1.min(x0.saturating_add(4 * MAX_SIDE as usize)) {
                        let k = py * w + px;
                        if geom::contains(&ring, P2(px as f64 + 0.5, py as f64 + 0.5))
                            && depth.get(k).is_some_and(|d| f64::from(*d) <= z + slack)
                            && let Some(t) = hit.get_mut(k)
                        {
                            *t = slot;
                        }
                    }
                }
            }
        }
        Image {
            width: w as u32,
            height: h as u32,
            rgba,
            hit: Arc::new(hit),
            depth: Arc::new(depth),
            owners: Arc::new(owners),
            faces: Arc::clone(&self.faces),
            section_paint: section_paint.map(Arc::new),
        }
    }
}

/// Each of `levels`' heads as `cam` sees it: see [`LevelHead`].
fn heads(levels: &[(Id, String, [P3; 4])], cam: &Camera) -> Vec<LevelHead> {
    let px = 1.0 / cam.scale.max(1e-9);
    levels
        .iter()
        .map(|(id, name, c)| {
            let near = nearest(cam, c);
            let (far, sides) = (c[(near + 2) % 4], [c[(near + 1) % 4], c[(near + 3) % 4]]);
            // The far edges' outer ends: the one further right.
            let end = if cam.project(sides[0]).0 >= cam.project(sides[1]).0 { sides[0] } else { sides[1] };
            let d = unit(sub(end, far));
            let (w, h) = (f64::from(crate::font::width(name, HEAD_TEXT) + 2.0 * HEAD_PAD) * px, f64::from(HEAD_TEXT + 2.0 * HEAD_PAD) * px);
            let foot = add(end, mul(d, 2.0 * f64::from(HEAD_PAD) * px));
            let up = [0.0, 0.0, h];
            let corners = [foot, add(foot, mul(d, w)), add(add(foot, mul(d, w)), up), add(foot, up)];
            LevelHead { level: *id, name: name.clone(), corners }
        })
        .collect()
}

/// What a 3D picture draws over the surfaces for one level: its plane's edges and its head's lettering, as segments in `color`
/// and how many pixels wide (1 or 2), and its head's corners, all of which picks the level.
pub struct LevelMarks {
    pub id: Id,
    pub color: Rgb,
    pub lines: Vec<([P3; 2], u8)>,
    pub head: [P3; 4],
}

/// The marks of each of `levels` that `color` gives a colour (and whether it is selected: all four edges, not the far two).
/// Unselected, the two edges nearer the camera are left out, as in Revit. The head goes past the far edges' right end:
/// its name lettered.
fn level_marks(levels: &[(Id, String, [P3; 4])], cam: &Camera, color: &dyn Fn(Id) -> Option<(Rgb, bool)>) -> Vec<LevelMarks> {
    let mut marks = Vec::new();
    for ((id, _, corners), head) in levels.iter().zip(heads(levels, cam)) {
        let Some((color, selected)) = color(*id) else { continue };
        let near = nearest(cam, corners);
        let mut lines = Vec::new();
        for (i, (a, b)) in corners.iter().zip(corners.iter().cycle().skip(1)).enumerate() {
            if (selected || (i != near && (i + 1) % 4 != near))
                && let Some(seen) = cam.in_front(*a, *b)
            {
                lines.push((seen, 2));
            }
        }
        let [foot, along, ..] = head.corners;
        let (d, px) = (unit(sub(along, foot)), 1.0 / cam.scale.max(1e-9));
        let height = f64::from(HEAD_TEXT + 2.0 * HEAD_PAD);
        let to3 = |(x, y): (f32, f32)| add(add(foot, mul(d, f64::from(x) * px)), [0.0, 0.0, (height - f64::from(y)) * px]);
        for stroke in crate::font::strokes(&head.name, HEAD_PAD, HEAD_PAD + HEAD_TEXT, HEAD_TEXT) {
            for seg in stroke.windows(2) {
                if let [a, b] = seg
                    && let Some(seen) = cam.in_front(to3(*a), to3(*b))
                {
                    lines.push((seen, 1));
                }
            }
        }
        marks.push(LevelMarks { id: *id, color, lines, head: head.corners });
    }
    marks
}

/// Which of a level plane's `corners` is nearest the camera: the two edges from it are its near ones.
fn nearest(cam: &Camera, corners: &[P3; 4]) -> usize {
    (0..4).max_by(|a, b| cam.project(corners[*a]).2.total_cmp(&cam.project(corners[*b]).2)).unwrap_or(0)
}

/// The pixels of a `w` x `h` picture a line from `a` to `b` (each a pixel and its nearness)
/// covers, `wide` (1 or 2) px wide, where nothing in `depth` lies more than `slack` gives (for its nearness) nearer: each to `paint`.
fn outline(
    (a, b): ((f64, f64, f64), (f64, f64, f64)),
    (w, h): (usize, usize),
    wide: u8,
    slack: &dyn Fn(f64) -> f64,
    depth: &[f32],
    mut paint: impl FnMut(usize),
) {
    let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil();
    if !steps.is_finite() || steps > 4.0 * MAX_SIDE as f64 {
        return;
    }
    let steps = steps.max(1.0) as usize;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let (x, y, z) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t);
        let offsets: &[(f64, f64)] = if wide > 1 { &[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] } else { &[(0.5, 0.5)] };
        for &(dx, dy) in offsets {
            let (px, py) = ((x - 0.5 + dx).floor(), (y - 0.5 + dy).floor());
            if px < 0.0 || py < 0.0 || px >= w as f64 || py >= h as f64 {
                continue;
            }
            let k = py as usize * w + px as usize;
            if depth.get(k).is_some_and(|d| f64::from(*d) <= z + slack(z)) {
                paint(k);
            }
        }
    }
}

/// What a section's or an elevation's picture keeps: what lies beyond its plane, depths (toward
/// the camera) below `near`, and no more than `far` past it; between pixel columns `x.0` and `x.1`,
/// and rows `y.0` and `y.1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    pub near: f64,
    pub far: Option<f64>,
    pub x: (f64, f64),
    pub y: (f64, f64),
}

/// Whether triangle `t` is drawn as a cut face: under a section box, among the opaque surfaces (no `front`), its face turned
/// away from the camera. A solid the box cuts shows the inside of its far faces through the cut: drawn in one flat colour, it
/// shows solid, as Revit shows a cut. Where it is as near as a face turned toward the camera (a cut wall's foot on the floor's
/// top) it lies behind it ([`CAP_BEHIND`]), so the two don't fight.
fn capped(cam: &Camera, cut: Option<&Boxed>, front: Option<&[f32]>, t: &Tri) -> bool {
    cut.is_some() && front.is_none() && dot(cross(sub(t.p[1], t.p[0]), sub(t.p[2], t.p[0])), cam.toward(t.p[0])) < 0.0
}

/// How far behind its own depth a cut face is drawn, mm.
pub const CAP_BEHIND: f64 = 0.01;

/// The plane a cut face's pixels hold, for its element `owner`: one for the whole cut, so no edge is drawn across it, past any
/// surface's own.
pub const CAP_PLANE: usize = 1 << (usize::BITS - 1);

/// A 3D view's section box as `cam` sees it: what of a pixel's depth, or of the model, it keeps. In a parallel view, the
/// model point a pixel shows at a depth is the same sum for every pixel; in perspective, that sum gives the point on the plane
/// through the target, and the point is along the line from the eye through it.
#[doc(hidden)]
pub struct Boxed {
    lo: P3,
    hi: P3,
    /// The model point at pixel (0, 0) at depth 0, and how far it moves for a pixel across, a pixel down and a mm nearer.
    o: P3,
    dx: P3,
    dy: P3,
    dz: P3,
    /// In perspective, the eye and how far it is from the target.
    eye: Option<(P3, f64)>,
}

impl Boxed {
    fn new(cam: &Camera, (lo, hi): (P3, P3)) -> Boxed {
        let (back, right, up) = cam.axes();
        let k = 1.0 / cam.scale.max(1e-12);
        let (w, h) = cam.middle();
        let o = add(cam.target, sub(mul(up, h * k), mul(right, w * k)));
        Boxed { lo, hi, o, dx: mul(right, k), dy: mul(up, -k), dz: back, eye: cam.eye().zip(cam.eye_distance()) }
    }

    /// Whether model point `p` lies in the box.
    fn holds(&self, p: P3) -> bool {
        (0..3).all(|i| p[i] >= self.lo[i] && p[i] <= self.hi[i])
    }

    /// Whether what pixel `(x, y)` shows at depth `z` lies in the box.
    fn keeps(&self, x: f64, y: f64, z: f64) -> bool {
        match self.eye {
            None => self.holds(std::array::from_fn(|i| self.o[i] + self.dx[i] * x + self.dy[i] * y + self.dz[i] * z)),
            Some((eye, d)) => {
                let k = d / (z + d);
                self.holds(std::array::from_fn(|i| eye[i] + (self.o[i] + self.dx[i] * x + self.dy[i] * y - eye[i]) * k))
            }
        }
    }

    /// Whether triangle `t` lies wholly outside one of the box's planes, so none of it is kept.
    fn misses(&self, t: &Tri) -> bool {
        self.misses_corners(&t.p)
    }

    /// Whether the triangle with these corners lies wholly outside one of the box's planes.
    fn misses_corners(&self, corners: &[P3; 3]) -> bool {
        (0..3).any(|i| corners.iter().all(|p| p[i] < self.lo[i]) || corners.iter().all(|p| p[i] > self.hi[i]))
    }
}

/// What a section box from `lo` to `hi` keeps of `levels`: those whose plane lies within its height, cut to its sides.
fn levels_in(levels: &[(Id, String, [P3; 4])], (lo, hi): (P3, P3)) -> Vec<(Id, String, [P3; 4])> {
    levels
        .iter()
        .filter(|(.., c)| (lo[2]..=hi[2]).contains(&c[0][2]))
        .map(|(id, name, c)| (*id, name.clone(), c.map(|p| [p[0].clamp(lo[0], hi[0]), p[1].clamp(lo[1], hi[1]), p[2]])))
        .collect()
}

/// The middle of each face of section box `(lo, hi)`, where its grips are: west, east, south, north, bottom and top.
pub fn box_grips((lo, hi): (P3, P3)) -> [P3; 6] {
    let mid: P3 = std::array::from_fn(|i| (lo[i] + hi[i]) / 2.0);
    std::array::from_fn(|k| {
        let mut p = mid;
        p[k / 2] = if k.is_multiple_of(2) { lo[k / 2] } else { hi[k / 2] };
        p
    })
}

/// The twelve edges of section box `(lo, hi)`.
pub fn box_edges((lo, hi): (P3, P3)) -> Vec<[P3; 2]> {
    let corner = |n: usize| -> P3 { std::array::from_fn(|i| if n >> i & 1 == 0 { lo[i] } else { hi[i] }) };
    (0..8).flat_map(|n| (0..3).filter(move |i| n >> i & 1 == 0).map(move |i| [corner(n), corner(n | 1 << i)])).collect()
}

/// Section box `b` with face `k` (as [`box_grips`] orders them) moved `by` mm along its axis, to 10 mm, stopping
/// [`MIN_SECTION_BOX`](crate::model::MIN_SECTION_BOX) short of the face across from it.
pub fn move_box_face((mut lo, mut hi): (P3, P3), k: usize, by: f64) -> (P3, P3) {
    let (i, gap) = (k / 2 % 3, crate::model::MIN_SECTION_BOX);
    let round = |v: f64| (v / 10.0).round() * 10.0;
    if by.is_finite() {
        if k.is_multiple_of(2) {
            lo[i] = round(lo[i] + by).min(hi[i] - gap);
        } else {
            hi[i] = round(hi[i] + by).max(lo[i] + gap);
        }
    }
    (lo, hi)
}

/// What a wireframe draws of the edge from `a` to `b` in a picture of `(w, h)` pixels: its ends in the picture, what lies
/// outside the section box, behind the eye and off the picture cut away.
fn wire_line(cam: &Camera, section_box: Option<(P3, P3)>, [a, b]: [P3; 2], (w, h): (usize, usize)) -> Option<(P2, P2)> {
    let [a, b] = section_box.map_or(Some([a, b]), |bx| within(a, b, bx))?;
    let [a, b] = cam.in_front(a, b)?;
    let (pa, pb) = (cam.project(a), cam.project(b));
    if ![pa.0, pa.1, pb.0, pb.1].iter().all(|x| x.is_finite()) {
        return None;
    }
    clip(P2(pa.0, pa.1), P2(pb.0, pb.1), P2(0.0, 0.0), P2(w as f64, h as f64))
}

/// The pixels a wireframe's line from `p` to `q` steps through, a step no longer than a pixel. The card's `fs_wire` takes the
/// same steps.
fn wire_pixels(p: P2, q: P2) -> impl Iterator<Item = (i64, i64)> {
    let steps = (p.dist(q).ceil() as usize).max(1);
    (0..=steps).map(move |k| {
        let t = k as f64 / steps as f64;
        ((p.0 + (q.0 - p.0) * t).floor() as i64, (p.1 + (q.1 - p.1) * t).floor() as i64)
    })
}

/// What of the line from `a` to `b` lies within the box from `lo` to `hi`, if any (Liang and Barsky's clipping).
fn within(a: P3, b: P3, (lo, hi): (P3, P3)) -> Option<[P3; 2]> {
    let d = sub(b, a);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for i in 0..3 {
        for (p, q) in [(-d[i], a[i] - lo[i]), (d[i], hi[i] - a[i])] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
    }
    (t0 <= t1).then(|| [add(a, mul(d, t0)), add(a, mul(d, t1))])
}

/// Where `pts` lie as `cam` looks at them: the point in their middle, and how far they spread
/// across the picture and up it, mm.
fn extent(cam: &Camera, pts: &[P3]) -> Option<(P3, f64, f64)> {
    let first = *pts.first()?;
    let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), p| {
        ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
    });
    let middle = mul(add(lo, hi), 0.5);
    let (_, right, up) = cam.axes();
    let (mut x, mut y) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
    for p in pts {
        let d = sub(*p, middle);
        let (px, py) = (dot(d, right), dot(d, up));
        (x, y) = ((x.0.min(px), x.1.max(px)), (y.0.min(py), y.1.max(py)));
    }
    Some((add(middle, add(mul(right, (x.0 + x.1) / 2.0), mul(up, (y.0 + y.1) / 2.0))), x.1 - x.0, y.1 - y.0))
}

/// The whole model from `azimuth` and `elevation` at `scale` px per mm: the picture as large as
/// that makes it (up to [`MAX_SIDE`] a side), `room` px clear around it. A 3D view on a sheet.
pub fn camera(m: &Model, azimuth: f64, elevation: f64, scale: f64, room: f64) -> Camera {
    camera_in(m, azimuth, elevation, scale, room, &Look::default())
}

/// [`camera`] for what `look` shows of the model.
pub fn camera_in(m: &Model, azimuth: f64, elevation: f64, scale: f64, room: f64, look: &Look) -> Camera {
    let mut cam = Camera {
        azimuth,
        elevation: elevation.clamp(-89.0, 89.0),
        scale,
        target: [0.0; 3],
        width: 1,
        height: 1,
        lens: None,
        shift: [0.0; 2],
        clip: None,
    };
    let pts: Vec<P3> = solids(m).iter().filter(|s| look.shows(s.owner())).flat_map(Solid::corners).collect();
    if let Some((target, w, h)) = extent(&cam, &pts) {
        let side = |d: f64| (d * scale + 2.0 * room).ceil().clamp(1.0, f64::from(MAX_SIDE)) as u32;
        (cam.target, cam.width, cam.height) = (target, side(w), side(h));
    }
    cam
}

/// The camera view `v` is drawn through, as large as [`camera_in`] fits it at `scale` and `room` px clear: from the view's
/// own eye and target through its lens when it has a camera (Revit's Camera tool), else fitted over the model as the default
/// 3D view is. A camera view on a sheet keeps the picture the fit gives, so it fills its frame the same way.
pub fn camera_of(m: &Model, v: &crate::model::View3D, scale: f64, room: f64, look: &Look) -> Camera {
    let fitted = camera_in(m, v.azimuth, v.elevation, scale, room, look);
    match (v.camera, v.lens()) {
        (Some(cam), Some(lens)) => Camera::through(&cam, lens, true, fitted.width, fitted.height),
        _ => fitted,
    }
}

/// The camera 3D view `v` is seen through, as the person left it, in a picture `width` by `height` px: its own camera's (its
/// physical camera, the film the picture's width), else where it was left ([`Framing`](crate::model::Framing)) from its
/// azimuth and elevation through its lens. None for a view still fitted over the model.
pub fn as_left(v: &crate::model::View3D, width: u32, height: u32) -> Option<Camera> {
    if let Some(cam) = &v.camera {
        let focal = v.perspective.map_or(LENS, |p| p.lens);
        return Some(Camera::through(cam, focal, v.lens().is_some(), width, height));
    }
    let f = v.framing?;
    let scale = (f64::from(width.max(1)) / f.across.max(1e-6)).clamp(1e-6, 50.0);
    let elevation = v.elevation.clamp(-89.0, 89.0);
    Some(Camera { azimuth: v.azimuth, elevation, scale, target: f.target, width, height, lens: v.lens(), shift: [0.0; 2], clip: None })
}

/// Each pixel's value from `at`, over all `w * h` of them: on the render pool for a big picture.
fn per_pixel<T: Copy + Send + Sync>((w, h): (usize, usize), empty: T, at: &(dyn Fn(usize) -> T + Sync)) -> Vec<T> {
    let mut out = vec![empty; w * h];
    let fill = |offset: usize, out: &mut [T]| out.iter_mut().enumerate().for_each(|(i, o)| *o = at(offset + i));
    if let Some(pool) = (w * h >= 256 * 256).then(render_pool).flatten() {
        use rayon::prelude::*;
        const PIXELS: usize = 8192;
        pool.install(|| out.par_chunks_mut(PIXELS).enumerate().for_each(|(i, o)| fill(i * PIXELS, o)));
    } else {
        fill(0, &mut out);
    }
    out
}

/// How lit each pixel of a picture is under ambient shadows (1 for not at all), from its `depth`: nearness as `cam` projects
/// it, nothing negative infinity. A pixel the surfaces round it rise toward the eye from, on both sides, is in a corner, and as
/// dark as they rise, in pixels' worth of depth a pixel across. Over a plane the depth runs straight, so the pixels across from
/// each other are as near on average as the one between them: a flat face takes no shadow, nor does an edge that stands out.
fn ambient(depth: &[f32], (w, h): (usize, usize), cam: &Camera) -> Vec<f32> {
    let at = |x: i64, y: i64| -> Option<f64> {
        let inside = x >= 0 && y >= 0 && x < w as i64 && y < h as i64;
        inside.then(|| depth.get(y as usize * w + x as usize)).flatten().map(|d| f64::from(*d)).filter(|d| d.is_finite())
    };
    per_pixel((w, h), 1.0, &|k| {
        let (x, y) = ((k % w) as i64, (k / w) as i64);
        let Some(z) = at(x, y) else { return 1.0 };
        let px = cam.slack(1.0, z);
        let (mut sum, mut n) = (0.0, 0.0);
        for (dx, dy) in AMBIENT.map(|(dx, dy)| (i64::from(dx), i64::from(dy))) {
            let (Some(a), Some(b)) = (at(x + dx, y + dy), at(x - dx, y - dy)) else { continue };
            let unit = px * ((dx * dx + dy * dy) as f64).sqrt();
            if !unit.is_finite() || unit <= 0.0 || (a - z).abs() > AMBIENT_REACH * unit || (b - z).abs() > AMBIENT_REACH * unit {
                continue;
            }
            sum += ((a + b) / 2.0 - z) / unit;
            n += 1.0;
        }
        if n == 0.0 { 1.0 } else { (1.0 - AMBIENT_DARK * (sum / n).clamp(0.0, 1.0)) as f32 }
    })
}

/// Where a picture draws a silhouette `width` px wide, from its `depth` (as [`ambient`] reads it) and the `plane` each pixel
/// shows: wherever two neighbouring pixels are one on a surface and one on nothing, or on two surfaces [`OUTLINE_GAP`] pixels'
/// worth apart in depth, each pixel whose square `width + 1` across (from `width / 2` before it) holds both is outlined. So a
/// straight outline is `width` px wide, about the line between what stands in front and what is behind.
fn outlines(depth: &[f32], plane: &dyn Fn(usize) -> Option<usize>, (w, h): (usize, usize), cam: &Camera, width: u32) -> Vec<bool> {
    let near = |k: usize| depth.get(k).copied().filter(|d| d.is_finite());
    let gap = |k: usize, n: usize| match (near(k), near(n)) {
        (Some(a), Some(b)) => plane(k) != plane(n) && f64::from((a - b).abs()) > cam.slack(OUTLINE_GAP, f64::from(a.max(b))),
        (a, b) => a.is_some() != b.is_some(),
    };
    let across: Vec<bool> = (0..w * h).map(|k| k % w + 1 < w && gap(k, k + 1)).collect();
    let down: Vec<bool> = (0..w * h).map(|k| k / w + 1 < h && gap(k, k + w)).collect();
    // Whether `mask` holds anywhere `lo..=hi` from each pixel, along a row or down a column.
    let spread = |mask: &[bool], (lo, hi): (i64, i64), along: bool| -> Vec<bool> {
        (0..w * h)
            .map(|k| {
                let (x, y) = ((k % w) as i64, (k / w) as i64);
                (lo..=hi).any(|d| {
                    let (qx, qy) = if along { (x + d, y) } else { (x, y + d) };
                    qx >= 0 && qy >= 0 && qx < w as i64 && qy < h as i64 && mask.get(qy as usize * w + qx as usize) == Some(&true)
                })
            })
            .collect()
    };
    let (a, b) = (i64::from(width / 2), i64::from(width - width / 2));
    let across = spread(&spread(&across, (-a, b - 1), true), (-a, b), false);
    let down = spread(&spread(&down, (-a, b), true), (-a, b - 1), false);
    across.iter().zip(&down).map(|(x, y)| *x || *y).collect()
}

/// `a`, and `b` over it: `b` as much as `weight` (0 to 1) says.
fn blend(a: Rgb, b: Rgb, weight: f64) -> Rgb {
    let m = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * weight.clamp(0.0, 1.0)).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Halfway from `a` to `b`.
fn mix(a: Rgb, b: Rgb) -> Rgb {
    let m = |x: u8, y: u8| ((u16::from(x) + u16::from(y)) / 2) as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// How far in front of the eye a perspective picture begins, mm: what is nearer is cut away, on the processor and the card.
pub const NEAR: f64 = 10.0;

/// Where the 3D view looks from, and how much of the model its picture holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// Degrees counter-clockwise from east, around the model, of where it looks from.
    pub azimuth: f64,
    /// Degrees above the horizon it looks down from, within ±89.
    pub elevation: f64,
    /// Pixels per mm: at the target, in perspective.
    pub scale: f64,
    /// The model point in the middle of the picture.
    pub target: P3,
    pub width: u32,
    pub height: u32,
    /// In perspective, the focal length of the lens it looks through: mm on a 35 mm camera, whose film ([`FILM`]) is the
    /// picture's width. None for a parallel (axonometric) view. The eye stands back from the target as far as makes `scale`
    /// true there, so orbiting turns it about the target, panning carries both, and zooming walks it in.
    pub lens: Option<f64>,
    /// Where the target shows from the middle of the picture, px (right and down): a lens shift, as 3ds Max's Perspective
    /// Control makes one, which moves the picture over the film without turning the camera. Zero for most views.
    pub shift: [f64; 2],
    /// In perspective, the nearest and furthest the picture shows, mm in front of the eye (a camera's clipping planes):
    /// what is nearer than the first or further than the second is cut away. None for all of it.
    pub clip: Option<[f64; 2]>,
}

/// A camera's projection, its axes worked out once for the many points of a picture.
#[derive(Clone, Copy)]
pub(crate) struct Projector {
    back: P3,
    right: P3,
    up: P3,
    target: P3,
    middle: (f64, f64),
    scale: f64,
    /// In perspective: the focal length in px, and how far the eye is from the target, mm.
    eye: Option<(f64, f64)>,
    /// In perspective with clipping planes: the nearness ([`Projector::at`]'s third) of the near plane and of the far one.
    pub(crate) clip: Option<(f64, f64)>,
}

impl Projector {
    /// Whether nearness `z` lies outside the camera's clipping planes, nearer than the near one or further than the far one.
    pub(crate) fn clipped(&self, z: f64) -> bool {
        self.clip.is_some_and(|(near, far)| z > near || z < far)
    }

    /// The pixel `p` shows at, and how near the camera it is (larger is nearer): mm toward the camera from the target in a
    /// parallel view; in perspective, `d² / a - d` for a point `a` in front of an eye `d` from the target, which is as near as
    /// that is at the target and, as `1 / a` is, runs straight across the picture of a plane. Behind the eye, not a number.
    fn at(&self, p: P3) -> (f64, f64, f64) {
        let d = sub(p, self.target);
        let (x, y, z) = (dot(d, self.right), dot(d, self.up), dot(d, self.back));
        match self.eye {
            None => (self.middle.0 + x * self.scale, self.middle.1 - y * self.scale, z),
            Some((f, e)) => {
                let ahead = e - z;
                if ahead.is_nan() || ahead <= 0.0 {
                    return (f64::NAN, f64::NAN, f64::NEG_INFINITY);
                }
                (self.middle.0 + x * f / ahead, self.middle.1 - y * f / ahead, e * e / ahead - e)
            }
        }
    }

    /// How far in front of the eye `p` is, mm; infinitely, in a parallel view.
    fn ahead(&self, p: P3) -> f64 {
        self.eye.map_or(f64::INFINITY, |(_, e)| e - dot(sub(p, self.target), self.back))
    }

    /// The parts of triangle `p` at least [`NEAR`] in front of the eye, each as the weights of its corners on `p`'s: the
    /// triangle itself where it all is, none where none of it is.
    fn in_front(&self, p: &[P3; 3]) -> ([[P3; 3]; 2], usize) {
        const ONE: [P3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let ahead = p.map(|q| self.ahead(q) - NEAR);
        if ahead.iter().all(|a| *a >= 0.0) {
            return ([ONE, ONE], 1);
        }
        // Sutherland and Hodgman's clipping, to one plane: three corners in, four at most out.
        let mut ring: [P3; 4] = [[0.0; 3]; 4];
        let mut n = 0;
        for k in 0..3 {
            let (j, a, b) = ((k + 1) % 3, ahead[k], ahead[(k + 1) % 3]);
            if a >= 0.0 {
                ring[n.min(3)] = ONE[k];
                n += 1;
            }
            if (a >= 0.0) != (b >= 0.0) && (a - b).abs() > 0.0 {
                let t = a / (a - b);
                ring[n.min(3)] = std::array::from_fn(|i| ONE[k][i] * (1.0 - t) + ONE[j][i] * t);
                n += 1;
            }
        }
        match n {
            3 => ([[ring[0], ring[1], ring[2]], ONE], 1),
            4 => ([[ring[0], ring[1], ring[2]], [ring[0], ring[2], ring[3]]], 2),
            _ => ([ONE, ONE], 0),
        }
    }
}

/// The points weights `w` give on triangle `p`'s corners.
pub(crate) fn weighed(p: &[P3; 3], w: &[P3; 3]) -> [P3; 3] {
    w.map(|w| std::array::from_fn(|i| w[0] * p[0][i] + w[1] * p[1][i] + w[2] * p[2][i]))
}

impl Camera {
    /// Toward the camera, and the picture's right and up, in the model.
    #[doc(hidden)]
    pub fn axes(&self) -> (P3, P3, P3) {
        let (a, e) = (self.azimuth.to_radians(), self.elevation.clamp(-89.0, 89.0).to_radians());
        let back = [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()];
        let right = [-a.sin(), a.cos(), 0.0];
        (back, right, cross(back, right))
    }

    pub(crate) fn projector(&self) -> Projector {
        let (back, right, up) = self.axes();
        let eye = self.focal().zip(self.eye_distance());
        let middle = self.middle();
        let clip = self.clip.zip(eye).map(|([near, far], (_, e))| {
            let z = |a: f64| e * e / a.max(NEAR) - e;
            (z(near), z(far.max(near)))
        });
        Projector { back, right, up, target: self.target, middle, scale: self.scale, eye, clip }
    }

    /// The pixel the target shows at: the picture's middle, moved by the lens's shift.
    pub fn middle(&self) -> (f64, f64) {
        (f64::from(self.width) / 2.0 + self.shift[0], f64::from(self.height) / 2.0 + self.shift[1])
    }

    /// In perspective, the lens's focal length in px.
    fn focal(&self) -> Option<f64> {
        let lens = self.lens.filter(|l| l.is_finite() && *l > 0.0)?;
        Some(f64::from(self.width.max(1)) * lens / FILM)
    }

    /// In perspective, how far the eye is from the target, mm.
    pub fn eye_distance(&self) -> Option<f64> {
        Some(self.focal()? / self.scale.max(1e-12))
    }

    /// In perspective, where the eye is.
    pub fn eye(&self) -> Option<P3> {
        let (back, ..) = self.axes();
        Some(add(self.target, mul(back, self.eye_distance()?)))
    }

    /// In perspective, how high the eye is, mm.
    pub fn eye_elevation(&self) -> Option<f64> {
        Some(self.eye()?[2])
    }

    /// The eye raised or lowered to height `z` (in perspective; a parallel view is as it was), looking at the same target from
    /// as far across the plan: it looks down, or up, the more for it.
    pub fn with_eye_elevation(self, z: f64) -> Camera {
        let (Some(eye), Some(f)) = (self.eye(), self.focal()) else { return self };
        if !z.is_finite() {
            return self;
        }
        let across = (eye[0] - self.target[0]).hypot(eye[1] - self.target[1]).max(1e-6);
        let elevation = (z - self.target[2]).atan2(across).to_degrees().clamp(-89.0, 89.0);
        let d = across / elevation.to_radians().cos();
        Camera { elevation, scale: (f / d).clamp(1e-6, 50.0), ..self }
    }

    /// The picture made `width` by `height` px, through `lens` (none for a parallel view). In perspective the eye stays where
    /// it stood, so a wider picture or a longer lens takes in less round the target; a parallel view keeps its scale, and
    /// one turned to perspective keeps it at the target.
    pub fn sized(self, width: u32, height: u32, lens: Option<f64>) -> Camera {
        let next = Camera { width, height, lens, ..self };
        match (self.eye_distance(), next.focal()) {
            (Some(d), Some(f)) => Camera { scale: (f / d).clamp(1e-6, 50.0), ..next },
            _ => next,
        }
    }

    /// The camera that stands at `eye`, looking at `target`, through `lens`, into a picture `width` by `height` px: the
    /// exact view of [`ViewCamera`](crate::model::ViewCamera) (its eye and target), so `eye()` is `eye` and `target` is
    /// `target` — the render path for a camera view. Its azimuth and elevation are the direction from the eye to the
    /// target, as [`axes`](Self::axes) reads them.
    pub fn at(eye: P3, target: P3, lens: f64, width: u32, height: u32) -> Camera {
        // Never clamped to a camera's range here: a camera's lens is checked where it is set (`lens_ok`), and the window sees
        // through a longer one when its safe frame is narrower than the view.
        let lens = if lens.is_finite() && lens > 0.0 { lens } else { LENS };
        let back = sub(eye, target);
        let d = dot(back, back).sqrt().max(1e-9);
        let back = mul(back, 1.0 / d);
        // The angle of `back`, as axes() rebuilds it: up within ±89°, and round from east.
        let elevation = back[2].clamp(-1.0, 1.0).asin().to_degrees().clamp(-89.0, 89.0);
        let azimuth = back[1].atan2(back[0]).to_degrees();
        let f = f64::from(width.max(1)) * lens / FILM;
        let scale = (f / d).clamp(1e-6, 50.0);
        Camera { azimuth, elevation, scale, target, width, height, lens: Some(lens), shift: [0.0; 2], clip: None }
    }

    /// The camera a camera view `cam` is seen through, into a picture `width` by `height` px whose width is the film: in
    /// `perspective` through `lens` (the focal length, mm), else parallel along the way it looks (an orthographic camera, as
    /// large at the target as the lens would show it). Its film and zoom make the lens the 35 mm one it is (`lens × zoom ×
    /// 36 / film`); its perspective control and clipping are [`physical`](Self::physical)'s.
    pub fn through(cam: &crate::model::ViewCamera, lens: f64, perspective: bool, width: u32, height: u32) -> Camera {
        Camera::bare(cam, lens, perspective, width, height).physical(&cam.physical, cam.target, f64::from(width))
    }

    /// [`through`](Self::through) before its perspective control and clipping: the camera the hand moves, which
    /// [`physical`](Self::physical) turns into the one the picture is drawn through.
    pub fn bare(cam: &crate::model::ViewCamera, lens: f64, perspective: bool, width: u32, height: u32) -> Camera {
        let seen = Camera::at(cam.eye, cam.target, full_frame(lens, &cam.physical), width, height);
        if perspective { seen } else { Camera { lens: None, ..seen } }
    }

    /// This camera (standing as [`at`](Self::at) stands it, `target` in the middle) seen through `p`'s perspective control
    /// and clipping, as 3ds Max's Physical Camera. In perspective its film plane is tilted by its vertical tilt correction, a
    /// slope `t` ([`Physical::tilt`](crate::model::Physical::tilt); with Auto, the slope this camera looks up at), as
    /// `IPhysicalCamera::GetTiltCorrection` applies it: a ray's target on the focus plane moved back by its height × `t`, its
    /// height divided by √(1 + t²). That plane, seen from the eye, is a camera turned down by atan `t` about its horizontal
    /// axis, its focal length cos(atan `t`) as long, and `target` (the film's middle) kept in the middle by a lens shift; a
    /// slope of the camera's own keeps verticals vertical. Then the picture moves by its Lens Shift (% of the film's width,
    /// `film` px of the picture), and its clipping planes cut while Clip Manually is on. An orthographic camera takes neither
    /// tilt nor clipping: its verticals stand vertical already. The eye stays where it stands.
    pub fn physical(self, p: &crate::model::Physical, target: P3, film: f64) -> Camera {
        let mut cam = self;
        let slope = if p.auto_tilt { -self.elevation.clamp(-89.0, 89.0).to_radians().tan() } else { p.tilt };
        if let (Some(eye), Some(d), Some(lens)) = (self.eye(), self.eye_distance(), self.lens)
            && slope.is_finite()
            && slope != 0.0
        {
            let turn = slope.atan();
            let k = turn.cos();
            let elevation = (self.elevation + turn.to_degrees()).clamp(-89.0, 89.0);
            // The eye stays: the film turns about it, its middle a target as far along the turned axis, its focal length
            // shorter by the turn (the scale with it, so the eye stands as far back).
            let turned = Camera { elevation, lens: Some(lens * k), scale: self.scale * k, ..self };
            let (back, ..) = turned.axes();
            cam = Camera { target: sub(eye, mul(back, d)), ..turned };
            // Where `target` falls now, moved back to the middle.
            let (x, y, _) = cam.project(target);
            let (mx, my) = cam.middle();
            if x.is_finite() && y.is_finite() {
                cam.shift = [cam.shift[0] + mx - x, cam.shift[1] + my - y];
            }
        }
        // Lens Shift: right and up as a share of the film's width, which is the picture's; the picture moves over the film,
        // so what it shows moves the other way.
        cam.shift = [cam.shift[0] - p.shift[0] / 100.0 * film, cam.shift[1] + p.shift[1] / 100.0 * film];
        cam.clip = (p.clip_on && cam.lens.is_some()).then_some([p.near, p.far]);
        cam
    }

    /// The pixel `p` shows at, and how near the camera it is (larger is nearer): mm toward the camera from the target in a
    /// parallel view, and as near as that at the target in perspective (where it is `d² / a - d` for a point `a` in front of
    /// an eye `d` from the target, which runs straight across the picture of a plane). Behind the eye, not a number.
    pub fn project(&self, p: P3) -> (f64, f64, f64) {
        self.projector().at(p)
    }

    /// The model point that pixel `(x, y)` shows at nearness `z`, as [`project`](Self::project) gives it.
    pub fn point_at(&self, (x, y): (f64, f64), z: f64) -> P3 {
        let (back, ..) = self.axes();
        let p = self.unproject((x, y));
        match (self.eye(), self.eye_distance()) {
            (Some(eye), Some(d)) => add(eye, mul(sub(p, eye), d / (z + d))),
            _ => add(p, mul(back, z)),
        }
    }

    /// Toward the camera from `p`: from anywhere the same way in a parallel view, toward the eye in perspective.
    pub fn toward(&self, p: P3) -> P3 {
        match self.eye() {
            Some(eye) => sub(eye, p),
            None => self.axes().0,
        }
    }

    /// What of the line from `a` to `b` is at least [`NEAR`] in front of the eye: all of it in a parallel view.
    pub fn in_front(&self, a: P3, b: P3) -> Option<[P3; 2]> {
        let k = self.projector();
        let (near, far) = match (self.clip, k.eye) {
            (Some([near, far]), Some(_)) => (near.max(NEAR), far.max(near)),
            _ => (NEAR, f64::INFINITY),
        };
        // Cut to each plane in turn: in front of the near one, then short of the far one.
        let cut = |[a, b]: [P3; 2], da: f64, db: f64| -> Option<[P3; 2]> {
            match (da >= 0.0, db >= 0.0) {
                (true, true) => Some([a, b]),
                (false, false) => None,
                _ => {
                    let cut = add(a, mul(sub(b, a), da / (da - db)));
                    Some(if da >= 0.0 { [a, cut] } else { [cut, b] })
                }
            }
        };
        let [a, b] = cut([a, b], k.ahead(a) - near, k.ahead(b) - near)?;
        if far.is_finite() { cut([a, b], far - k.ahead(a), far - k.ahead(b)) } else { Some([a, b]) }
    }

    /// How much nearer, in nearness as [`project`](Self::project) gives it, a surface may be than a line at nearness `z` and
    /// leave it showing: `px` pixels' worth, which in perspective is less the further off it is.
    pub fn slack(&self, px: f64, z: f64) -> f64 {
        let base = px / self.scale.max(1e-9);
        match self.eye_distance() {
            Some(d) => base * (1.0 + z / d).max(0.0),
            None => base,
        }
    }

    /// The point at pixel `(x, y)` on the horizontal plane at height `z`; none where the view is
    /// level with it, or in perspective where the plane is behind the eye there.
    pub fn on_plane(&self, at: (f64, f64), z: f64) -> Option<P3> {
        let (back, ..) = self.axes();
        let p = self.unproject(at);
        let (from, along) = match self.eye() {
            Some(eye) => (eye, unit(sub(p, eye))),
            None => (p, back),
        };
        let t = (z - from[2]) / along[2];
        (along[2].abs() > 1e-9 && t.is_finite() && (self.lens.is_none() || t > 0.0)).then(|| add(from, mul(along, t)))
    }

    /// The point at pixel `(x, y)` on `plane`, level or sloped; none where the view runs along it, or in perspective where it
    /// is behind the eye there.
    pub fn on_slope(&self, at: (f64, f64), plane: Plane) -> Option<P3> {
        let (back, ..) = self.axes();
        let p = self.unproject(at);
        let (from, along) = match self.eye() {
            Some(eye) => (eye, unit(sub(p, eye))),
            None => (p, back),
        };
        // Along the ray, the height left over the plane falls by `rate` a mm.
        let rate = along[2] - plane.g.0 * along[0] - plane.g.1 * along[1];
        let t = (plane.at(P2(from[0], from[1])) - from[2]) / rate;
        (rate.abs() > 1e-9 && t.is_finite() && (self.lens.is_none() || t > 0.0)).then(|| add(from, mul(along, t)))
    }

    /// The model point at pixel `at`, `z` toward the camera from the plane through the target (as `project` says it).
    pub fn at_depth(&self, at: (f64, f64), z: f64) -> P3 {
        let (back, ..) = self.axes();
        add(self.unproject(at), mul(back, z))
    }

    /// The model point at pixel `(x, y)` on the plane through the target, square to the view.
    pub fn unproject(&self, (x, y): (f64, f64)) -> P3 {
        let (_, right, up) = self.axes();
        let (mx, my) = self.middle();
        let (dx, dy) = ((x - mx) / self.scale, (y - my) / self.scale);
        add(self.target, sub(mul(right, dx), mul(up, dy)))
    }

    /// Turned `around` degrees about the target and raised `up` degrees. The azimuth stays within
    /// (-180, 180].
    pub fn orbit(self, around: f64, up: f64) -> Camera {
        let a = (self.azimuth + around).rem_euclid(360.0);
        Camera { azimuth: if a > 180.0 { a - 360.0 } else { a }, elevation: (self.elevation + up).clamp(-89.0, 89.0), ..self }
    }

    /// Turned `around` degrees (counter-clockwise, seen from above) and `up` degrees about the eye, which stays where it
    /// stands: Revit's Look and a mouse look while flying. A parallel view turns about its target.
    pub fn look(self, around: f64, up: f64) -> Camera {
        let Some((eye, d)) = self.eye().zip(self.eye_distance()) else { return self.orbit(around, -up) };
        let turned = self.orbit(around, -up);
        let (back, ..) = turned.axes();
        let target = sub(eye, mul(back, d));
        if target.iter().all(|x| x.is_finite()) { Camera { target, ..turned } } else { self }
    }

    /// Moved `forward` mm the way it looks, `right` mm to its right and `up` mm straight up, turned as it was: Revit's Walk and
    /// Up/Down, and flying with W, A, S, D, Q and E. With `level` the way forward is level (walking keeps the eye's height),
    /// else it climbs or falls as the camera looks (flying).
    pub fn walk(self, forward: f64, right: f64, up: f64, level: bool) -> Camera {
        let (back, side, _) = self.axes();
        let ahead = if level {
            let flat = [-back[0], -back[1], 0.0];
            let n = dot(flat, flat).sqrt();
            if n > 1e-9 { mul(flat, 1.0 / n) } else { [0.0; 3] }
        } else {
            mul(back, -1.0)
        };
        let by = add(add(mul(ahead, forward), mul(side, right)), [0.0, 0.0, up]);
        let target = add(self.target, by);
        if target.iter().all(|x| x.is_finite() && x.abs() <= crate::model::MAX_COORD) { Camera { target, ..self } } else { self }
    }

    /// Moved so the model point under pixel `from` comes under pixel `to`: in perspective, the point on the plane through
    /// the target.
    pub fn pan(self, from: (f64, f64), to: (f64, f64)) -> Camera {
        Camera { target: add(self.target, sub(self.unproject(from), self.unproject(to))), ..self }
    }

    /// Zoomed by `factor` about pixel `at`, which keeps its model point (in perspective, on the plane through the target,
    /// which the eye walks toward).
    pub fn zoom(self, factor: f64, at: (f64, f64)) -> Camera {
        let p = self.unproject(at);
        let zoomed = Camera { scale: (self.scale * factor).clamp(1e-6, 50.0), ..self };
        zoomed.pan(zoomed.project_xy(p), at)
    }

    /// Turned as [`orbit`](Self::orbit) turns it, but about `pivot` rather than the target: the whole camera turns round
    /// `pivot` as its axes turn, so `pivot` stays on the pixel it showed at, in perspective too.
    pub fn orbit_about(self, around: f64, up: f64, pivot: P3) -> Camera {
        let turned = self.orbit(around, up);
        let ((b0, r0, u0), (b1, r1, u1)) = (self.axes(), turned.axes());
        let v = sub(self.target, pivot);
        let target = add(pivot, add(add(mul(b1, dot(v, b0)), mul(r1, dot(v, r0))), mul(u1, dot(v, u0))));
        if target.iter().all(|x| x.is_finite()) { Camera { target, ..turned } } else { turned }
    }

    /// Zoomed by `factor` toward `pivot`, which stays on the pixel it showed at: everything closes in on it, the target and
    /// (in perspective) the eye, as [`zoom`](Self::zoom) closes in on the point on the plane through the target.
    pub fn zoom_about(self, factor: f64, pivot: P3) -> Camera {
        let scale = (self.scale * factor).clamp(1e-6, 50.0);
        let k = scale / self.scale;
        let target = add(pivot, mul(sub(self.target, pivot), 1.0 / k));
        if k.is_finite() && k > 0.0 && target.iter().all(|x| x.is_finite()) { Camera { scale, target, ..self } } else { self }
    }

    /// How far, mm, a drag on the picture from pixel `from` to pixel `to` moves model point `at` along the model's axis `axis`
    /// (0 for x, 1 for y, 2 for z): the drag's part along that axis as the picture shows it there. None where the axis points
    /// at the camera.
    pub fn along(&self, axis: usize, at: P3, from: (f64, f64), to: (f64, f64)) -> Option<f64> {
        let unit: P3 = std::array::from_fn(|i| if i == axis { 1.0 } else { 0.0 });
        // In perspective, a mm along the axis shows shorter the further off it is: the point on the axis through `at` nearest
        // the line from the eye through each pixel, so what is dragged stays under the pointer.
        if let Some(eye) = self.eye() {
            let on_axis = |q: (f64, f64)| {
                let ray = sub(self.unproject(q), eye);
                let (b, c, w) = (dot(unit, ray), dot(ray, ray), sub(at, eye));
                let square = c - b * b;
                (axis < 3 && square > 1e-9 * c).then(|| (b * dot(ray, w) - c * dot(unit, w)) / square)
            };
            return Some(on_axis(to)? - on_axis(from)?).filter(|d| d.is_finite());
        }
        let (o, e) = (self.project_xy(at), self.project_xy(add(at, unit)));
        let (dx, dy) = (e.0 - o.0, e.1 - o.1);
        let shown = dx * dx + dy * dy;
        // Less than a twentieth of a pixel for 100 m: square to the picture.
        (axis < 3 && shown > (0.05 / 1e5f64).powi(2)).then(|| ((to.0 - from.0) * dx + (to.1 - from.1) * dy) / shown)
    }

    fn project_xy(&self, p: P3) -> (f64, f64) {
        let (x, y, _) = self.project(p);
        (x, y)
    }
}

/// `cam` with its eye in the middle of `m`'s plan, 1.6 m up, a metre from its target and looking level through a 20 mm lens:
/// for tests of what a camera inside a building sees.
#[doc(hidden)]
pub fn inside(m: &Model, cam: Camera) -> Camera {
    let (lo, hi) = m.model_box().unwrap_or(([0.0; 3], [0.0; 3]));
    let cam = Camera { elevation: 0.0, lens: Some(20.0), ..cam };
    let (back, ..) = cam.axes();
    let middle = [(lo[0] + hi[0]) / 2.0 - back[0] * 1000.0, (lo[1] + hi[1]) / 2.0 - back[1] * 1000.0, 1600.0];
    let f = cam.focal().unwrap_or(1.0);
    Camera { target: middle, scale: f / 1000.0, ..cam }
}

/// A picture of the 3D view: its pixels, and which element each one shows.
#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// Row by row: red, green, blue and alpha.
    pub rgba: Vec<u8>,
    /// Each pixel's triangle (`u32::MAX` for none), how near the camera the opaque surface there is (none in a wireframe),
    /// and each triangle's element.
    #[doc(hidden)]
    pub hit: Arc<Vec<u32>>,
    depth: Arc<Vec<f32>>,
    owners: Arc<Vec<Id>>,
    faces: Arc<Vec<&'static str>>,
    section_paint: Option<Arc<SectionPaint>>,
}

struct SectionPaint {
    base: Vec<u8>,
    edges: Vec<bool>,
}

impl Image {
    fn empty() -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
            hit: Arc::new(vec![u32::MAX]),
            depth: Arc::new(Vec::new()),
            owners: Arc::new(Vec::new()),
            faces: Arc::new(Vec::new()),
            section_paint: None,
        }
    }

    /// Repaint a section's hover/selection without rebuilding geometry or visibility.
    /// Returns false for other images, whose shaded transparency needs a different compositor.
    /// Picking, face information and coverage remain those of the original image.
    pub fn retint_section(&mut self, tints: &[(Id, Rgb)]) -> bool {
        let Some(paint) = &self.section_paint else { return false };
        self.rgba.clone_from(&paint.base);
        let tints: BTreeMap<Id, Rgb> = tints.iter().copied().collect();
        for (k, px) in self.rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let tint = self.hit.get(k).and_then(|t| self.owners.get(*t as usize)).and_then(|o| tints.get(o));
            if let Some(tint) = tint {
                let c = if paint.edges.get(k).copied().unwrap_or(false) { *tint } else { mix(Rgb(px[0], px[1], px[2]), *tint) };
                px[..3].copy_from_slice(&[c.0, c.1, c.2]);
            }
        }
        true
    }

    fn index(&self, x: f64, y: f64) -> Option<usize> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        (x >= 0.0 && y >= 0.0 && x < w && y < h).then(|| y as usize * self.width as usize + x as usize)
    }

    /// Whether it is `other` to the pixel: its colours, and what each pixel picks, its face, and whether it's covered.
    #[doc(hidden)]
    pub fn same_as(&self, other: &Image) -> Result<(), String> {
        if (self.width, self.height) != (other.width, other.height) {
            return Err(format!("{}x{} against {}x{}", self.width, self.height, other.width, other.height));
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let k = 4 * (y as usize * self.width as usize + x as usize);
                let (a, b) = (self.rgba.get(k..k + 4), other.rgba.get(k..k + 4));
                let mine = (a, self.pick(fx, fy), self.face(fx, fy), self.covered(fx, fy));
                let theirs = (b, other.pick(fx, fy), other.face(fx, fy), other.covered(fx, fy));
                if mine != theirs {
                    return Err(format!("pixel ({x}, {y}): {mine:?} against {theirs:?}"));
                }
            }
        }
        Ok(())
    }

    /// Whether a pixel is covered by model geometry, including linked elements and meshes
    /// without named faces. Canvas backgrounds may show through only where this is false.
    pub fn covered(&self, x: f64, y: f64) -> bool {
        self.index(x, y).and_then(|i| self.hit.get(i)).is_some_and(|t| *t != u32::MAX)
    }

    /// The point of the model's surface the pixel at `(x, y)` shows, through `cam`, the camera it was taken through: the
    /// nearest opaque surface there. None where nothing is, and in a wireframe, which has no surfaces.
    pub fn point(&self, cam: &Camera, x: f64, y: f64) -> Option<P3> {
        let z = *self.depth.get(self.index(x, y)?)?;
        (z.is_finite()).then(|| cam.at_depth((x.floor() + 0.5, y.floor() + 0.5), f64::from(z)))
    }

    /// The element the pixel at `(x, y)` shows.
    pub fn pick(&self, x: f64, y: f64) -> Option<Id> {
        let t = *self.hit.get(self.index(x, y)?)?;
        self.owners.get(t as usize).copied().filter(|id| !crate::link::is_linked(*id))
    }

    /// The face of the element the pixel at `(x, y)` shows, as paint names it: `exterior` or `interior` for a wall's, `top` or
    /// `bottom` for a floor's, a roof's or a ceiling's, `all` for another's; none where it's on no face that can be painted.
    pub fn face(&self, x: f64, y: f64) -> Option<&'static str> {
        let t = *self.hit.get(self.index(x, y)?)?;
        self.faces.get(t as usize).copied().filter(|f| !f.is_empty())
    }

    /// The elements the picture shows in the box from pixel `a` to `b`: those it shows only
    /// inside it, or with `crossing`, any of.
    pub fn in_box(&self, a: (f64, f64), b: (f64, f64), crossing: bool) -> Vec<Id> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        // Each element: whether all of it is inside, and whether any of it is.
        let mut seen: BTreeMap<Id, (bool, bool)> = BTreeMap::new();
        for (k, t) in self.hit.iter().enumerate() {
            let Some(&id) = self.owners.get(*t as usize).filter(|id| !crate::link::is_linked(**id)) else { continue };
            let (x, y) = ((k % self.width as usize) as f64 + 0.5, (k / self.width as usize) as f64 + 0.5);
            let inside = (lo.0..=hi.0).contains(&x) && (lo.1..=hi.1).contains(&y);
            let e = seen.entry(id).or_insert((true, false));
            *e = (e.0 && inside, e.1 || inside);
        }
        seen.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(id, _)| id).collect()
    }

    /// Every element the picture shows.
    pub fn shows(&self) -> BTreeSet<Id> {
        self.hit.iter().filter_map(|t| self.owners.get(*t as usize).copied()).filter(|id| !crate::link::is_linked(*id)).collect()
    }

    /// The colour of the pixel at `(x, y)`; the paper's, off the picture.
    pub fn color(&self, x: f64, y: f64) -> Rgb {
        match self.index(x, y).and_then(|k| self.rgba.get(4 * k..4 * k + 3)) {
            Some([r, g, b]) => Rgb(*r, *g, *b),
            _ => PAPER,
        }
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        let size = tiny_skia::IntSize::from_wh(self.width, self.height).ok_or("the picture's size is out of range")?;
        let pm = tiny_skia::Pixmap::from_vec(self.rgba.clone(), size).ok_or("the picture's size is out of range")?;
        pm.encode_png().map_err(|e| e.to_string())
    }
}

/// A shaped floor's handles in the 3D view: its points and corners, in plan and in the model; and its edges, as plan pairs.
pub type ShapeHandles = (Vec<(P2, P3)>, Vec<(P2, P2)>);

/// A floor's shape as the 3D view edits it: each of its shape's points and its outline's and openings' corners, in plan and
/// where its top is there; and the edges between them (split lines, the outline's and the openings'), as pairs of those.
pub fn shape_handles(m: &Model, f: &crate::model::Floor) -> ShapeHandles {
    let faces = m.floor_faces(f);
    let top = |q: P2| {
        let near = |r: &[P2]| geom::contains(r, q) || geom::ring_edges(r).any(|(a, b)| geom::dist_to_segment(q, a, b) <= crate::model::TOL);
        faces.iter().find(|(r, _)| near(r)).map_or_else(|| m.floor_top(f).at(q), |(_, plane)| plane.at(q))
    };
    let rings = || std::iter::once(&f.boundary).chain(&f.openings);
    let mut handles: Vec<(P2, P3)> = Vec::new();
    for q in f.shape.points.iter().map(|p| p.at).chain(rings().flatten().copied()) {
        if !handles.iter().any(|(h, _)| h.dist(q) <= crate::model::TOL) {
            handles.push((q, [q.0, q.1, top(q)]));
        }
    }
    let edges = f.shape.lines.iter().map(|l| (l.a, l.b)).chain(rings().flat_map(|r| geom::ring_edges(r))).collect();
    (handles, edges)
}

/// Where a shape snap lands in plan, the edge it's on (in plan, or in the model) to light up, and which it was.
pub type PlanSnap = (P2, Option<(P2, P2)>, ShapeSnap);
pub type ModelSnap = (P2, Option<[P3; 2]>, ShapeSnap);

/// What a shape snap found, by priority: a point or corner, an edge's middle, or the nearest point along an edge. (Its
/// edges meet only at its points, being one triangulation's, so an intersection is always an endpoint.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeSnap {
    Endpoint,
    Midpoint,
    Nearest,
}

/// Where Add Point or Add Split Line snaps near plan point `q` on floor `f`'s top in a plan, within `reach` mm: the nearest
/// point or corner; else the middle of an edge (its outline's, an opening's, a split line, or a crease between its planes);
/// else the nearest point on the nearest edge; with the edge to light up, and which it was. None away from them all.
pub fn shape_snap_plan(m: &Model, f: &crate::model::Floor, q: P2, reach: f64) -> Option<PlanSnap> {
    let (handles, mut edges) = shape_handles(m, f);
    if let Some((c, _)) = handles.iter().map(|(c, _)| (*c, c.dist(q))).filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1)) {
        return Some((c, None, ShapeSnap::Endpoint));
    }
    edges.extend(m.shape_creases(f).into_iter().map(|[a, b]| (P2(a[0], a[1]), P2(b[0], b[1]))));
    let mid = |(a, b): (P2, P2)| (a + b) * 0.5;
    if let Some(&e) = edges.iter().filter(|e| mid(**e).dist(q) <= reach).min_by(|x, y| mid(**x).dist(q).total_cmp(&mid(**y).dist(q))) {
        return Some((mid(e), Some(e), ShapeSnap::Midpoint));
    }
    let (a, b) = edges
        .into_iter()
        .map(|(a, b)| (a, b, geom::dist_to_segment(q, a, b)))
        .filter(|(.., d)| *d <= reach)
        .min_by(|x, y| x.2.total_cmp(&y.2))
        .map(|(a, b, _)| (a, b))?;
    let t = geom::param_on_segment(q, a, b).clamp(0.0, 1.0);
    Some((a + (b - a) * t, Some((a, b)), ShapeSnap::Nearest))
}

/// Where Add Point or Add Split Line snaps near pixel `at` on floor `f`'s top in the 3D view through `cam`, within `reach`
/// pixels, as in a plan (`shape_snap_plan`): the nearest point or corner; else an edge's middle; else the point under the
/// cursor on the nearest edge; with that edge in the model to light up, and which it was. None away from them all.
pub fn shape_snap_3d(m: &Model, f: &crate::model::Floor, cam: &Camera, at: (f64, f64), reach: f64) -> Option<ModelSnap> {
    let (handles, edges) = shape_handles(m, f);
    let q = P2(at.0, at.1);
    let shown = |p: P3| {
        let (x, y, z) = cam.project(p);
        (x.is_finite() && y.is_finite() && z.is_finite()).then_some(P2(x, y))
    };
    let corner =
        handles.iter().filter_map(|(c, p)| Some((*c, shown(*p)?.dist(q)))).filter(|(_, d)| *d <= reach).min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((c, _)) = corner {
        return Some((c, None, ShapeSnap::Endpoint));
    }
    // The edges in the model: the creases between its planes (its outline's and openings' edges among them), and its split
    // lines, where its top is.
    let height = |p: P2| m.floor_top(f).at(p) + m.shape_height(f, p);
    let lift = |p: P2| [p.0, p.1, height(p)];
    let mut lines: Vec<[P3; 2]> = m.shape_creases(f);
    lines.extend(edges.iter().map(|&(a, b)| [lift(a), lift(b)]));
    let lerp = |[a, b]: [P3; 2], t: f64| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    let mid_off = |e: [P3; 2]| shown(lerp(e, 0.5)).map_or(f64::INFINITY, |s| s.dist(q));
    if let Some(&e) = lines.iter().filter(|e| mid_off(**e) <= reach).min_by(|x, y| mid_off(**x).total_cmp(&mid_off(**y))) {
        let p = lerp(e, 0.5);
        return Some((P2(p[0], p[1]), Some(e), ShapeSnap::Midpoint));
    }
    let off = |e: [P3; 2], t: f64| shown(lerp(e, t)).map_or(f64::INFINITY, |s| s.dist(q));
    // Along an edge, how far its picture lies from the cursor falls then rises, even in perspective: narrowed down to it.
    let nearest = |e: [P3; 2]| {
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if off(e, a) < off(e, b) { hi = b } else { lo = a }
        }
        let t = (lo + hi) / 2.0;
        (t, off(e, t))
    };
    let (e, t, _) = lines
        .into_iter()
        .map(|e| {
            let (t, d) = nearest(e);
            (e, t, d)
        })
        .filter(|(.., d)| *d <= reach)
        .min_by(|a, b| a.2.total_cmp(&b.2))?;
    let p = lerp(e, t);
    Some((P2(p[0], p[1]), Some(e), ShapeSnap::Nearest))
}

/// Where pixel `at` meets floor `f`'s top in the 3D view through `cam`, in plan: on the nearest of its planes there.
pub fn on_floor_top(m: &Model, f: &crate::model::Floor, cam: &Camera, at: (f64, f64)) -> Option<P2> {
    let mut best: Option<(f64, P2)> = None;
    for (ring, plane) in m.floor_faces(f) {
        let Some(p) = cam.on_slope(at, plane) else { continue };
        let q = P2(p[0], p[1]);
        let inside = geom::contains(&ring, q) || geom::ring_edges(&ring).any(|(a, b)| geom::dist_to_segment(q, a, b) <= crate::model::TOL);
        let near = cam.project(p).2;
        if inside && near.is_finite() && best.is_none_or(|(n, _)| near > n) {
            best = Some((near, q));
        }
    }
    best.map(|(_, q)| q)
}

/// What of floor `f`'s shape a click at plan point `q` picks in a plan, within `reach` mm, as `Floor::shape_pick` does, its
/// folding lines among its edges (`Model::shape_folds`): the nearest point or corner; else both ends of the nearest edge.
pub fn shape_pick_plan(m: &Model, f: &crate::model::Floor, q: P2, reach: f64) -> Vec<P2> {
    let picked = f.shape_pick(q, reach);
    if picked.len() == 1 {
        return picked;
    }
    let off = |(a, b): &(P2, P2)| geom::dist_to_segment(q, *a, *b);
    let fold = m.shape_folds(f).into_iter().filter(|e| off(e) <= reach).min_by(|x, y| off(x).total_cmp(&off(y)));
    let edge = match *picked.as_slice() {
        [a, b] => Some((a, b)),
        _ => None,
    };
    match (edge, fold) {
        (Some(e), Some(g)) if off(&g) < off(&e) => vec![g.0, g.1],
        (None, Some(g)) => vec![g.0, g.1],
        _ => picked,
    }
}

/// What of floor `f`'s shape a click at pixel `at` picks in the 3D view through `cam`, as `Floor::shape_pick` does in plan,
/// within `reach` pixels: the nearest point or corner; else both ends of the nearest edge; else nothing.
pub fn shape_pick_3d(m: &Model, f: &crate::model::Floor, cam: &Camera, at: (f64, f64), reach: f64) -> Vec<P2> {
    let (handles, mut edges) = shape_handles(m, f);
    edges.extend(m.shape_folds(f));
    let q = P2(at.0, at.1);
    let shown = |p: P3| {
        let (x, y, z) = cam.project(p);
        (x.is_finite() && y.is_finite() && z.is_finite()).then_some(P2(x, y))
    };
    let near = handles.iter().filter_map(|(plan, p)| Some((*plan, shown(*p)?.dist(q)))).filter(|(_, d)| *d <= reach);
    if let Some((plan, _)) = near.min_by(|a, b| a.1.total_cmp(&b.1)) {
        return vec![plan];
    }
    let lift = |plan: P2| handles.iter().find(|(h, _)| h.dist(plan) <= crate::model::TOL).map(|(_, p)| *p);
    let off = |(a, b): &(P2, P2)| Some(geom::dist_to_segment(q, shown(lift(*a)?)?, shown(lift(*b)?)?));
    edges
        .iter()
        .filter_map(|e| Some((*e, off(e)?)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map_or_else(Vec::new, |((a, b), _)| vec![a, b])
}

/// A surface the line of sight through a pixel meets: the element it is of, the point it meets it at, and the surface's unit
/// normal, turned toward the camera. What the 3D view orbits about and zooms toward, and what its tools draw on (`workplane`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub id: Id,
    pub at: P3,
    pub normal: P3,
}

/// Where the line of sight through pixel `at` meets the plane of triangle `p`, element `id`'s: in perspective, in front of the eye.
pub(crate) fn surface_of(cam: &Camera, at: (f64, f64), id: Id, p: &[P3; 3]) -> Option<Surface> {
    let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
    let on = cam.unproject(at);
    let eye = cam.eye();
    let (from, along) = match eye {
        Some(eye) => (eye, unit(sub(on, eye))),
        None => (on, mul(cam.axes().0, -1.0)),
    };
    let facing = dot(along, n);
    let t = dot(sub(p[0], from), n) / facing;
    let normal = if facing > 0.0 { mul(n, -1.0) } else { n };
    (facing.abs() > 1e-9 && t.is_finite() && (eye.is_none() || t > 0.0)).then(|| Surface { id, at: add(from, mul(along, t)), normal })
}

/// Where the line of sight through pixel `at` meets level `id`'s plane, of `levels` (each a level, its name and its plane's corners).
pub(crate) fn on_level(levels: &[(Id, String, [P3; 4])], cam: &Camera, at: (f64, f64), id: Id) -> Option<Surface> {
    let (.., c) = levels.iter().find(|(l, ..)| *l == id)?;
    surface_of(cam, at, id, &[c[0], c[1], c[2]])
}

impl Scene {
    /// The surface pixel `at` of `image`, a picture of this scene through `cam`, shows: a triangle's, or a level's plane where it
    /// shows the level's mark; none where it shows nothing, or a scan's point.
    pub fn surface_in(&self, image: &Image, cam: &Camera, at: (f64, f64)) -> Option<Surface> {
        let t = *image.hit.get(image.index(at.0, at.1)?)? as usize;
        match self.tris.get(t) {
            Some(tri) => surface_of(cam, at, tri.owner, &tri.p),
            None => on_level(&self.levels, cam, at, *image.owners.get(t)?),
        }
    }
}

/// A level plane a tool draws on: the element whose face it is (a level, for its own plane or the ground), the level its
/// points go on and how far over it, mm, and its height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CursorPlane {
    pub on: Id,
    pub level: Id,
    pub offset: f64,
    pub z: f64,
}

/// How nearly straight up a face's normal turns for the face to be level: within about half a degree.
const LEVEL_UP: f64 = 0.99995;

/// The plane a tool draws on at pixel `at` of a 3D view through `cam`, where the line of sight meets `under`: a level face turned up at
/// its height, on its element's level (a level's plane, that level), else on the highest level at or below it (the lowest,
/// if none is). Nothing under the cursor, the lowest level's plane. None for a face that isn't level or is turned down, or
/// with no level to stand on.
pub fn cursor_plane(m: &Model, cam: &Camera, at: (f64, f64), under: Option<Surface>) -> Option<CursorPlane> {
    let levels = m.levels();
    let ground = || {
        let (id, l) = levels.first()?;
        Some(Surface { id: *id, at: cam.on_plane(at, l.elevation)?, normal: [0.0, 0.0, 1.0] })
    };
    let s = under.or_else(ground)?;
    let z = s.at[2];
    if !(s.normal[2] >= LEVEL_UP && z.is_finite()) {
        return None;
    }
    let own = m.level(s.id).map(|_| s.id).or_else(|| m.elements.get(s.id).and_then(|e| m.level_of(e)).and_then(|g| m.elements.id(g)));
    let below = levels.iter().rev().find(|(_, l)| l.elevation <= z + crate::model::TOL).or(levels.first()).map(|(id, _)| *id);
    let level = own.or(below)?;
    // To the micron, as the commands keep points: no float noise from the line of sight.
    let offset = ((z - m.elevation(level)) * 1e3).round() / 1e3 + 0.0;
    Some(CursorPlane { on: s.id, level, offset, z: m.elevation(level) + offset })
}

#[cfg(test)]
#[path = "view3d_tests.rs"]
mod tests;

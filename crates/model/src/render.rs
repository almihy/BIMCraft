//! Plan views: what a level's plan shows, as a display list of filled rings and polylines in
//! pixels, drawn to SVG or PNG. Drafting conventions: walls and columns the cut plane passes
//! through get a heavy outline and grey poché, and below it a thin one (faint past the view
//! range's bottom); doors a leaf and swing, windows glass lines, stairs their risers up to the cut
//! line and a walk line, roofs their ridge and hips (dashed overhead, below the view range's top),
//! rooms a tag, grids a dashed line with a bubble at each end, dimensions witness lines and ticks
//! (an arc for an angle, an arrow for a radius), door and window tags their mark in a box, text
//! notes their lines, spot elevations a triangle and their height, sections their line and head,
//! elevations a marker, sloped floors their slope arrow, shafts their outline and a cross; all of
//! it inside the plan's crop region. A level's reflected ceiling plan cuts its walls higher, over
//! the doors, and looks up at its ceilings: their outlines and tiles. Sections and elevations draw the
//! same way along their line and up: the cut in poché, levels and grids, notes and spot
//! elevations, over the 3D view's hidden-line picture of what lies beyond.

use bimcraft_i18n::msg;

use std::f64::consts::PI;
use std::fmt::Write;

use tiny_skia::{Color, FillRule, IntSize, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};

use crate::geom::{self, P2};
use crate::graphics::{Detail, Look, scale_of};
use crate::model::Guid;
use crate::model::{
    CEILING_CUT, Ceiling, DetailLine, DimKind, Dimension, Element, FilledRegion, Floor, Function, Id, Key, Layer, Model, Note, Opening,
    OpeningWay, Operation, Pattern, PlanRange, Railing, Ramp, Roof, Section, Shaft, Stair, TOL, Tag, View3D, Wall,
};
use crate::pen::{Pen, Weights};
pub use crate::prim::{LIGHT_POCHE, OPAQUE, PAPER, POCHE, Part, Prim, Rgb, Rule, View};
use crate::styles::{BY_CATEGORY, ObjectStyle};
use crate::units::Units;
use crate::view3d::{self, Camera, Clip, Image, Scene};
pub use crate::view3d::{FragmentStats, SectionCache};
use crate::walls::{self, Axis};
use crate::{font, massing, sheet, stairs};

mod dimensions;
use dimensions::dimension;
mod hatch;
#[doc(hidden)]
pub mod regions;
mod site;

#[doc(hidden)]
pub const INK: Rgb = Rgb(25, 25, 25);
const FAINT: Rgb = Rgb(120, 120, 120);
const GRID: Rgb = Rgb(60, 100, 150);
/// A mass's outline, dashed.
const MASS: Rgb = Rgb(88, 112, 168);
/// BIMCraft's own colours for a light ground, each with the one a dark canvas draws it in, as light as reads on it (at least
/// 4.5 : 1 against its ground): a grid's blue, a mass's.
pub const ON_DARK: [(Rgb, Rgb); 2] = [(GRID, Rgb(120, 165, 220)), (MASS, Rgb(140, 160, 225))];
/// Room for grid bubbles and the view title around the drawing, px.
#[doc(hidden)]
pub const MARGIN: f64 = 70.0;
const BUBBLE: f32 = 11.0;

impl Drawing {
    /// Whether primitive `i`'s corners are mitred ([`Part::mitred`]).
    pub(crate) fn mitred(&self, i: usize) -> bool {
        self.parts.get(i).is_none_or(|p| p.mitred())
    }
}

/// What of it lies inside the box from pixel `lo` to `hi`: a fill's rings clipped, a line in
/// the pieces inside, a picture's whole pixels inside.
fn cropped(prim: Prim, lo: (f32, f32), hi: (f32, f32)) -> Vec<Prim> {
    let mm = |p: (f32, f32)| P2(f64::from(p.0), f64::from(p.1));
    let px = |p: P2| (p.0 as f32, p.1 as f32);
    match prim {
        Prim::Fill { rings, color, rule, alpha } => {
            let rings: Vec<Vec<(f32, f32)>> = rings.iter().map(|r| clip_ring(r, lo, hi)).filter(|r| r.len() >= 3).collect();
            if rings.is_empty() { Vec::new() } else { vec![Prim::Fill { rings, color, rule, alpha }] }
        }
        Prim::Line { pts, width, color, dash, closed, alpha } => {
            let inside = |p: &(f32, f32)| (lo.0..=hi.0).contains(&p.0) && (lo.1..=hi.1).contains(&p.1);
            if pts.iter().all(inside) {
                return vec![Prim::Line { pts, width, color, dash, closed, alpha }];
            }
            let close = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
            let mut runs: Vec<Vec<(f32, f32)>> = Vec::new();
            for (a, b) in pts.iter().copied().zip(pts.iter().copied().skip(1)).chain(close) {
                let Some((p, q)) = clip(mm(a), mm(b), mm(lo), mm(hi)) else { continue };
                let (p, q) = (px(p), px(q));
                match runs.last_mut() {
                    Some(run) if run.last() == Some(&p) => run.push(q),
                    _ => runs.push(vec![p, q]),
                }
            }
            runs.into_iter().map(|pts| Prim::Line { pts, width, color, dash: dash.clone(), closed: false, alpha }).collect()
        }
        Prim::Image { at, width, height, rgba, ink } => {
            let side = |lo: f32, hi: f32, from: f32, n: u32| {
                let first = (lo - from).ceil().clamp(0.0, n as f32) as u32;
                (first, ((hi - from).floor().clamp(0.0, n as f32) as u32).max(first))
            };
            let ((x0, x1), (y0, y1)) = (side(lo.0, hi.0, at.0, width), side(lo.1, hi.1, at.1, height));
            if x1 == x0 || y1 == y0 {
                return Vec::new();
            }
            let row = (x1 - x0) as usize * 4;
            let mut kept = Vec::with_capacity(row * (y1 - y0) as usize);
            for y in y0..y1 {
                let from = (y as usize * width as usize + x0 as usize) * 4;
                kept.extend_from_slice(rgba.get(from..from + row).unwrap_or_default());
            }
            vec![Prim::Image { at: (at.0 + x0 as f32, at.1 + y0 as f32), width: x1 - x0, height: y1 - y0, rgba: kept, ink }]
        }
    }
}

/// A primitive with its owner, part, pen and halftone, taken out of a drawing to be pushed again.
type Kept = (Prim, Option<Id>, Part, Option<Pen>, bool);

pub struct Drawing {
    pub view: View,
    pub prims: Vec<Prim>,
    /// The element each primitive draws, for picking and highlighting.
    pub owners: Vec<Option<Id>>,
    /// What each primitive draws of its element, for overrides.
    pub parts: Vec<Part>,
    /// The pen each stroke or pattern dot is drawn in, for [`Drawing::weigh`] to size; none for other fills, pictures, lettering and
    /// what is sized already (a view's lines on a sheet).
    pub pens: Vec<Option<Pen>>,
    /// Whether each primitive is halftone, its colour halfway to the paper (baked in, as paper and pictures draw it): a screen
    /// with a dark ground draws it halfway to that ground instead, as Revit fades halftone toward the background. Read through
    /// [`Drawing::half`]: it may be shorter than `prims`, those past it not halftone.
    pub halftone: Vec<bool>,
    /// Whether the primitives pushed next are halftone.
    half: bool,
    regions: regions::Layers,
    /// What it could not draw as asked, in English (a fill pattern too dense for the view): for a front end to say.
    pub notes: Vec<String>,
    /// What the pens draw in this view, mm on paper: the project's line weights at its scale.
    pub(crate) weights: Weights,
    /// The element being drawn now, and the part of it.
    owner: Option<Id>,
    pub(crate) part: Part,
    /// How big annotation is drawn, times the pixels it is drawn in: see [`annotation`].
    #[doc(hidden)]
    pub k: f32,
    /// How many times [`sheet::DENSITY`] pixels a mm of the paper the view stands for is, so the pens' size: `k`, not
    /// clamped ([`paper_of`]).
    paper: f32,
    /// How much of what the view cuts it draws.
    detail: Detail,
    /// Whether it is for a screen, where a dense drawing zoomed out may be drawn simplified ([`crate::cad::lod`]); a picture
    /// or paper draws every entity.
    pub(crate) screen: bool,
    pattern_styles: std::collections::BTreeMap<Id, [crate::graphics::Style; 2]>,
}

impl Drawing {
    /// Nothing yet, through `view`.
    pub fn new(view: View) -> Drawing {
        Drawing {
            view,
            prims: Vec::new(),
            owners: Vec::new(),
            parts: Vec::new(),
            pens: Vec::new(),
            halftone: Vec::new(),
            half: false,
            regions: regions::Layers::default(),
            notes: Vec::new(),
            weights: Weights::default(),
            owner: None,
            part: Part::Projection,
            k: 1.0,
            paper: 1.0,
            detail: Detail::Fine,
            screen: false,
            pattern_styles: std::collections::BTreeMap::new(),
        }
    }
    /// Finish the model layer before view-specific regions and annotation are drawn.
    pub(crate) fn model_under_regions(&mut self, section: Option<Id>) {
        self.linked_under();
        self.regions = regions::Layers::new(self.prims.len(), section);
    }
    /// The picture the section drawn under the regions is over, which they keep the ink of: given by whoever has it, else drawn
    /// when a region first wants it.
    pub(crate) fn picture(&mut self, m: &Model, section: Id, image: &Image) {
        if regions::wanted(m, section) {
            self.regions.picture(image, self.view);
        }
    }

    /// What the primitives pushed next draw.
    pub(crate) fn own(&mut self, owner: Option<Id>) {
        self.owner = owner;
    }
    /// The part of their element the primitives pushed next draw.
    pub(crate) fn part(&mut self, part: Part) {
        self.part = part;
    }
    pub(crate) fn push(&mut self, p: Prim) {
        self.push_pen(p, None);
    }
    fn push_pen(&mut self, p: Prim, pen: Option<Pen>) {
        self.prims.push(p);
        self.owners.push(self.owner);
        self.parts.push(self.part);
        self.pens.push(pen);
        if self.half {
            self.set_half(self.prims.len() - 1);
        }
    }
    /// Whether the primitives pushed next are halftone ([`Drawing::halftone`]).
    pub(crate) fn halftone_next(&mut self, half: bool) {
        self.half = half;
    }
    /// Whether primitive `i` is halftone ([`Drawing::halftone`]).
    pub fn half(&self, i: usize) -> bool {
        self.halftone.get(i).copied().unwrap_or(false)
    }
    /// Primitive `i` halftone ([`Drawing::halftone`]).
    pub(crate) fn set_half(&mut self, i: usize) {
        if self.halftone.len() <= i {
            self.halftone.resize(i + 1, false);
        }
        self.halftone[i] = true;
    }
    /// Each primitive's halftone ([`Drawing::half`]), taken out to be pushed again.
    fn take_halves(&mut self) -> Vec<bool> {
        let halves = (0..self.prims.len()).map(|i| self.half(i)).collect();
        self.halftone.clear();
        halves
    }
    /// A line through pixels `pts` in `pen`, `dash`ed if it says.
    pub(crate) fn stroke(&mut self, pts: Vec<(f32, f32)>, pen: Pen, color: Rgb, dash: Option<Vec<f32>>, closed: bool) {
        let width = self.width(pen);
        self.push_pen(Prim::line(pts, width, color, dash, closed), Some(pen));
    }
    /// [`Drawing::stroke`], `alpha` opaque.
    pub(crate) fn stroke_seen(&mut self, pts: Vec<(f32, f32)>, pen: Pen, color: Rgb, dash: Option<Vec<f32>>, closed: bool, alpha: u8) {
        let width = self.width(pen);
        self.push_pen(Prim::Line { pts, width, color, dash, closed, alpha }, Some(pen));
    }
    /// One shared path for detail lines and region boundaries, in paper units.
    fn styled(&mut self, m: &Model, st: &crate::lines::LineStyle, mut pts: Vec<(f32, f32)>, closed: bool) {
        let k = sheet::DENSITY as f32 * self.paper;
        let pattern = st.pattern.as_ref().and_then(|n| m.line_patterns.get(n));
        let color = Rgb(st.color[0], st.color[1], st.color[2]);
        if let Some(p) = pattern.filter(|p| !p.is_dash()) {
            if closed && let Some(first) = pts.first().copied() {
                pts.push(first);
            }
            let part = std::mem::replace(&mut self.part, Part::LinePattern);
            for graphic in p.geometry(&pts, k, (self.weights.mm(st.pen, true) * sheet::DENSITY) as f32 * self.paper, color) {
                self.push_pen(graphic.prim, graphic.weighted.then_some(st.pen));
            }
            self.part = part;
        } else {
            self.stroke(pts, st.pen, color, pattern.and_then(|p| p.dash(k)), closed);
        }
    }

    /// Each line's width as its pen draws it in this view, as annotation where `annotation` says its owner is, else as
    /// the model: the last step of drawing a view, once its overrides have set their pens.
    pub(crate) fn weigh(&mut self, annotation: impl Fn(Option<Id>) -> bool) {
        let (weights, k) = (self.weights, self.paper);
        for ((prim, owner), pen) in self.prims.iter_mut().zip(&self.owners).zip(&self.pens) {
            let Some(pen) = pen else { continue };
            let width = (weights.mm(*pen, annotation(*owner)) * sheet::DENSITY) as f32 * k;
            match prim {
                Prim::Line { width: w, .. } => *w = width,
                // Only dot fills carry a pen. Other fills and font outlines keep their geometry.
                Prim::Fill { rings, .. } => {
                    let bounds = rings.iter().flatten().fold(((f32::MAX, f32::MAX), (f32::MIN, f32::MIN)), |(lo, hi), &(x, y)| {
                        ((lo.0.min(x), lo.1.min(y)), (hi.0.max(x), hi.1.max(y)))
                    });
                    let ((l, t), (r, b)) = bounds;
                    let diameter = r - l;
                    if diameter > 0.0 && diameter.is_finite() {
                        let k = if width > 0.0 { width } else { 1.0 } / diameter;
                        let (cx, cy) = ((l + r) * 0.5, (t + b) * 0.5);
                        for (x, y) in rings.iter_mut().flatten() {
                            *x = cx + (*x - cx) * k;
                            *y = cy + (*y - cy) * k;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    /// What a link builds, first, so what the project draws lies over it: each kept in the order it was drawn in.
    pub(crate) fn linked_under(&mut self) {
        let under = |o: &Option<Id>| o.is_some_and(crate::link::is_linked);
        if !self.owners.iter().any(under) {
            return;
        }
        let halves = self.take_halves();
        let prims = std::mem::take(&mut self.prims);
        let mut all: Vec<Kept> = prims
            .into_iter()
            .zip(std::mem::take(&mut self.owners))
            .zip(std::mem::take(&mut self.parts))
            .zip(std::mem::take(&mut self.pens))
            .zip(halves)
            .map(|((((p, o), part), pen), half)| (p, o, part, pen, half))
            .collect();
        all.sort_by_key(|(_, o, ..)| !under(o));
        for (p, o, part, pen, half) in all {
            keep(self, p, o, part, pen, half);
        }
    }
    /// Each line marked [`BY_CATEGORY`] in the pen its owner's object style gives, cut or in projection as its part is: before
    /// a view's overrides set theirs.
    pub(crate) fn by_category(&mut self, style: impl Fn(Option<Id>) -> ObjectStyle) {
        for ((pen, owner), part) in self.pens.iter_mut().zip(&self.owners).zip(&self.parts) {
            if *pen == Some(BY_CATEGORY) {
                let st = style(*owner);
                *pen = Some(if *part == Part::Cut { st.cut } else { st.projection });
            }
        }
    }
    /// `rings` filled, each turned the same way round, so where they overlap or meet they fill as one.
    pub(crate) fn fill(&mut self, rings: &[Vec<P2>], color: Rgb) {
        if rings.is_empty() {
            return;
        }
        let rings = rings
            .iter()
            .map(|r| {
                let mut r: Vec<P2> = if geom::area(r) < 0.0 { r.iter().rev().copied().collect() } else { r.clone() };
                r.dedup();
                r.iter().map(|p| self.view.px(*p)).collect()
            })
            .collect();
        self.push(Prim::fill(rings, color));
    }
    /// `pts` joined in `pen`, as wide as it is on the paper the view stands for.
    pub(crate) fn line(&mut self, pts: &[P2], pen: Pen, color: Rgb, closed: bool) {
        let pts = pts.iter().map(|p| self.view.px(*p)).collect();
        self.stroke(pts, pen, color, None, closed);
    }
    /// [`Drawing::line`], dashed.
    pub(crate) fn dashed(&mut self, pts: &[P2], pen: Pen, color: Rgb, closed: bool) {
        let pts = pts.iter().map(|p| self.view.px(*p)).collect();
        self.stroke(pts, pen, color, Some(vec![10.0, 5.0]), closed);
    }
    /// How many pixels wide `pen` draws here as the model: 0 for a hairline, which every painter draws as thin as it
    /// can. [`Drawing::weigh`] sizes annotation's at the end.
    #[doc(hidden)]
    pub fn width(&self, pen: Pen) -> f32 {
        (self.weights.mm(pen, false) * sheet::DENSITY) as f32 * self.paper
    }
    /// `px` pixels of annotation, as big as the view's scale makes them.
    fn px(&self, px: f32) -> f32 {
        px * self.k
    }

    /// The primitives pushed since `from`, grown `k` times about pixel `about`: annotation is laid out one pixel to the
    /// pixel and made as big as its view's scale says.
    fn grow(&mut self, from: usize, about: (f32, f32)) {
        let k = self.k;
        let at = |p: &mut (f32, f32)| *p = (about.0 + (p.0 - about.0) * k, about.1 + (p.1 - about.1) * k);
        for prim in self.prims.iter_mut().skip(from).filter(|_| k != 1.0) {
            match prim {
                Prim::Fill { rings, .. } => rings.iter_mut().flatten().for_each(at),
                Prim::Line { pts, .. } => pts.iter_mut().for_each(at),
                Prim::Image { .. } => {}
            }
        }
    }

    /// `text` centred on `at` px, capitals `height` px tall.
    pub(crate) fn text(&mut self, text: &str, at: (f32, f32), height: f32, color: Rgb) {
        self.text_turned(text, at, height, 0.0, color);
    }

    /// What's drawn cropped to the box from pixel `lo` to `hi`, but for the primitives of the
    /// elements `keep` takes.
    pub(crate) fn crop(&mut self, lo: (f32, f32), hi: (f32, f32), keep: impl Fn(Option<Id>) -> bool) {
        let halves = self.take_halves();
        let (prims, owners, parts) = (std::mem::take(&mut self.prims), std::mem::take(&mut self.owners), std::mem::take(&mut self.parts));
        let pens = std::mem::take(&mut self.pens);
        for ((((prim, owner), part), pen), half) in prims.into_iter().zip(owners).zip(parts).zip(pens).zip(halves) {
            for piece in if keep(owner) { vec![prim] } else { cropped(prim, lo, hi) } {
                self::keep(self, piece, owner, part, pen, half);
            }
        }
    }

    /// The fills and lines pushed since `from` made `alpha` opaque.
    fn see_through(&mut self, from: usize, alpha: u8) {
        if alpha == OPAQUE {
            return;
        }
        for p in self.prims.iter_mut().skip(from) {
            match p {
                Prim::Fill { alpha: a, .. } | Prim::Line { alpha: a, .. } => *a = alpha,
                Prim::Image { .. } => {}
            }
        }
    }

    /// `text` centred on `(x, y)` px, capitals `height` px tall, turned `angle` radians clockwise
    /// on screen (y runs down).
    pub(crate) fn text_turned(&mut self, text: &str, at: (f32, f32), height: f32, angle: f32, color: Rgb) {
        self.text_shaped(text, at, height, angle, (1.0, false), color);
    }

    /// [`Drawing::text_turned`], its letters `wide` times as wide as the lettering draws them and, `mirrored`, read backwards,
    /// each letter mirrored, as a drawing's text may be (DXF's width factor and backward flag).
    pub(crate) fn text_shaped(
        &mut self,
        text: &str,
        (x, y): (f32, f32),
        height: f32,
        angle: f32,
        (wide, mirrored): (f32, bool),
        color: Rgb,
    ) {
        let (s, c) = angle.sin_cos();
        let w = font::width(text, height);
        let across = if mirrored { -wide } else { wide };
        let part = std::mem::replace(&mut self.part, Part::Text);
        for pts in font::strokes(text, -w / 2.0, height / 2.0, height) {
            let pts = pts.into_iter().map(|(a, b)| (a * across, b)).map(|(a, b)| (x + a * c - b * s, y + a * s + b * c)).collect();
            self.push(Prim::line(pts, (height / 10.0).max(1.0), color, None, false));
        }
        self.part = part;
    }

    /// The element drawn nearest the pixel `at`, within `tol` px. Inside a fill beats lying on a
    /// line, so a grid line along a wall doesn't hide the wall; of equally near ones the last
    /// drawn, which shows on top, wins.
    pub fn pick(&self, at: (f32, f32), tol: f32) -> Option<Id> {
        self.pick_all(at, tol).first().copied()
    }

    /// Every element drawn within `tol` px of `at`, in the order [`Drawing::pick`] prefers them:
    /// what Tab cycles through.
    pub fn pick_all(&self, at: (f32, f32), tol: f32) -> Vec<Id> {
        let p = P2(f64::from(at.0), f64::from(at.1));
        let ring = |pts: &[(f32, f32)]| -> Vec<P2> { pts.iter().map(|&(x, y)| P2(f64::from(x), f64::from(y))).collect() };
        let edges = |pts: &[P2], closed: bool| {
            let open = pts.windows(2).filter_map(|w| match w {
                [a, b] => Some(geom::dist_to_segment(p, *a, *b)),
                _ => None,
            });
            let close = match (closed, pts.first(), pts.last()) {
                (true, Some(a), Some(b)) => Some(geom::dist_to_segment(p, *b, *a)),
                _ => None,
            };
            open.chain(close).fold(f64::INFINITY, f64::min)
        };
        // Each element's nearest primitive: its distance, and how late it's drawn.
        let mut near: std::collections::BTreeMap<Id, (f64, usize)> = std::collections::BTreeMap::new();
        for (order, (prim, owner)) in self.prims.iter().zip(&self.owners).enumerate() {
            // What a link builds is drawn and snapped to, never picked.
            let Some(id) = owner.filter(|id| !crate::link::is_linked(*id)) else { continue };
            let picture = self.parts.get(order) == Some(&Part::Picture);
            let d = match prim {
                // An image underlay gives way to whatever else is near, as Revit's does.
                Prim::Image { at, width, height, rgba, .. } if picture => {
                    drawn_near(p, (*at, *width, *height), rgba, tol).max(f64::from(tol))
                }
                Prim::Fill { rings, .. } => {
                    let rings: Vec<Vec<P2>> = rings.iter().map(|r| ring(r)).collect();
                    if rings.iter().filter(|r| geom::contains(r, p)).count() % 2 == 1 {
                        -1.0
                    } else {
                        rings.iter().map(|r| edges(r, true)).fold(f64::INFINITY, f64::min)
                    }
                }
                Prim::Line { pts, width, closed, .. } => (edges(&ring(pts), *closed) - f64::from(*width) / 2.0).max(0.0),
                Prim::Image { at, width, height, rgba, .. } => drawn_near(p, (*at, *width, *height), rgba, tol),
            };
            if d <= f64::from(tol) && near.get(&id).is_none_or(|&(b, _)| d <= b) {
                near.insert(id, (d, order));
            }
        }
        let mut found: Vec<(Id, (f64, usize))> = near.into_iter().collect();
        found.sort_by(|a, b| a.1.0.total_cmp(&b.1.0).then(b.1.1.cmp(&a.1.1)));
        found.into_iter().map(|(id, _)| id).collect()
    }

    /// The elements drawn wholly inside the box between pixels `a` and `b`: Revit's window
    /// selection. With `crossing`, the ones it touches too.
    pub fn in_box(&self, a: (f32, f32), b: (f32, f32), crossing: bool) -> Vec<Id> {
        let (lo, hi) = ((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)));
        let inside = |q: &(f32, f32)| (lo.0..=hi.0).contains(&q.0) && (lo.1..=hi.1).contains(&q.1);
        // Each element: whether all of it is inside, and whether any of it is.
        let mut seen: std::collections::BTreeMap<Id, (bool, bool)> = std::collections::BTreeMap::new();
        for (prim, owner) in self.prims.iter().zip(&self.owners) {
            let Some(id) = owner.filter(|id| !crate::link::is_linked(*id)) else { continue };
            let corners;
            let (rings, closed): (Vec<&[(f32, f32)]>, bool) = match prim {
                Prim::Fill { rings, .. } => (rings.iter().map(Vec::as_slice).collect(), true),
                Prim::Image { at, width, height, .. } => {
                    corners = Prim::corners(*at, *width, *height);
                    (vec![corners.as_slice()], true)
                }
                Prim::Line { pts, closed, .. } => (vec![pts.as_slice()], *closed),
            };
            let all = rings.iter().all(|r| r.iter().all(inside));
            let any = rings.iter().any(|r| {
                let last = closed.then(|| (r.last(), r.first()));
                r.windows(2)
                    .filter_map(|w| Some((w.first()?, w.get(1)?)))
                    .chain(last.into_iter().filter_map(|(p, q)| Some((p?, q?))))
                    .any(|(p, q)| clips(*p, *q, lo, hi))
                    || r.iter().any(inside)
            });
            let e = seen.entry(id).or_insert((true, false));
            *e = (e.0 && all, e.1 || any);
        }
        seen.into_iter().filter(|(_, (all, any))| if crossing { *any } else { *all }).map(|(id, _)| id).collect()
    }
}

/// How far pixel point `p` is from the nearest pixel a picture (its corner, width and height) draws, of those within `tol`: -1
/// on one, and none where all within it are clear. A picture picks where it isn't clear: a scan's where its points are.
fn drawn_near(p: P2, (at, width, height): ((f32, f32), u32, u32), rgba: &[u8], tol: f32) -> f64 {
    let (x, y) = (p.0 - f64::from(at.0), p.1 - f64::from(at.1));
    let r = f64::from(tol).ceil() + 1.0;
    let span = |v: f64, n: u32| ((v - r).floor().max(0.0) as u32, (v + r).ceil().clamp(0.0, f64::from(n)) as u32);
    let ((x0, x1), (y0, y1)) = (span(x, width), span(y, height));
    let mut best = f64::INFINITY;
    for py in y0..y1 {
        for px in x0..x1 {
            let alpha = (py as usize * width as usize + px as usize) * 4 + 3;
            if rgba.get(alpha).is_none_or(|a| *a == 0) {
                continue;
            }
            let (dx, dy) = ((x - f64::from(px) - 0.5).abs() - 0.5, (y - f64::from(py) - 0.5).abs() - 0.5);
            if dx <= 0.0 && dy <= 0.0 {
                return -1.0;
            }
            best = best.min(dx.max(0.0).hypot(dy.max(0.0)));
        }
    }
    best
}

/// The outlines of the walls and columns a plan cutting at height `z` cuts, but for those `look` hides.
fn cut_outlines(m: &Model, elements: &[(Id, &Element)], z: f64, look: &Look, index: Option<&walls::Index>) -> Vec<(Id, Vec<P2>)> {
    elements
        .iter()
        .filter(|(id, _)| look.shows(*id))
        .filter_map(|&(id, e)| match e {
            Element::Wall(w) if m.spans(w, z) => Some((id, walls::outline_in(m, id, index))),
            _ => stands(m, id, e, index).filter(|(_, bottom, top)| *bottom <= z && z < *top).map(|(outline, ..)| (id, outline)),
        })
        .collect()
}

/// What a column (a structural one, a footing) or a run (a beam, a strip footing) `id` of `e` takes in plan, and the
/// heights it reaches: its outline, its bottom and its top. A run's outline is its joined one, through `index`, as a
/// wall's is in a plan.
#[doc(hidden)]
pub fn stands(m: &Model, id: Id, e: &Element, index: Option<&walls::Index>) -> Option<(Vec<P2>, f64, f64)> {
    if let Some(col) = e.column() {
        let (bottom, top) = m.span(col.level, col.base_offset, col.top_level, col.height);
        return Some((m.column_outline(e), bottom, top));
    }
    let g = m.run_geom(e)?;
    let (bottom, top) = g.z_span();
    Some((crate::runs::outline_in(m, id, index), bottom, top))
}

/// The walls and columns a plan sees below its cut plane, down to its view depth: their outlines,
/// and their tops; but for those `look` hides.
fn seen_below(m: &Model, elements: &[(Id, &Element)], r: &PlanRange, look: &Look, index: Option<&walls::Index>) -> Vec<(Id, Vec<P2>, f64)> {
    elements
        .iter()
        .filter(|(id, _)| look.shows(*id))
        .filter_map(|&(id, e)| {
            // A footing or a run hangs under the level it is drawn on, its top flush with the view's bottom: that is seen.
            let flush = if matches!(e, Element::Footing(_) | crate::model::run_arm!(_)) { TOL } else { 0.0 };
            let (outline, top) = match e {
                Element::Wall(w) => {
                    let top = m.top_z(w);
                    // Joining a wall scans its neighbours. Reject other storeys before
                    // building their outlines; a plan only needs walls in its depth range.
                    if !(r.depth < top && top <= r.cut) {
                        return None;
                    }
                    (walls::outline_in(m, id, index), top)
                }
                _ if e.run().is_some() => {
                    let top = m.run_geom(e)?.z_span().1;
                    // Joining a run scans its neighbours too: reject what the plan doesn't see first.
                    if !(r.depth - flush < top && top <= r.cut) {
                        return None;
                    }
                    let (outline, _, top) = stands(m, id, e, index)?;
                    (outline, top)
                }
                _ => {
                    let (outline, _, top) = stands(m, id, e, index)?;
                    (outline, top)
                }
            };
            (r.depth - flush < top && top <= r.cut).then_some((id, outline, top))
        })
        .collect()
}

/// What a floor plan's passes look at, in order of id: every element but those of the kinds below that can't show in it,
/// by what each pass asks of them (a superset of what the passes draw, which is all their filters need): walls, columns
/// and runs that don't reach from its view depth up to its cut plane (runs: its top), doors and windows in walls it doesn't
/// cut, components it doesn't see, rooms, room separators and bars of other levels, ceilings, and other views' annotation.
/// A ceiling plan (`up`) looks at every element.
fn plan_elements<'a>(
    m: &'a Model,
    key: Guid,
    level: Guid,
    r: &PlanRange,
    up: bool,
    components: &mut crate::component_geometry::Components<'a>,
) -> Vec<(Id, &'a Element)> {
    if up {
        return m.elements.iter().map(|(id, e)| (*id, e)).collect();
    }
    let found = plan_candidates(m, key, level, r, components);
    let candidates = found.iter().filter_map(|id| Some((*id, m.elements.get(*id)?)));
    let shown: Vec<(Id, &Element)> = plan_filter(m, key, level, r, components, candidates);
    // Its twin, which tests hold it to: every element of the model, each asked the same.
    if crate::selfcheck::on(crate::selfcheck::Check::PlanElements) {
        let every = plan_filter(m, key, level, r, components, m.elements.iter().map(|(id, e)| (*id, e)));
        assert!(
            every.iter().map(|(id, _)| id).eq(shown.iter().map(|(id, _)| id)),
            "a plan's elements found from the lookup aren't every element that shows in it"
        );
    }
    shown
}

/// The elements that may show in plan `key` of `level`, range `r`, by id: every one that does, and others. From the
/// lookup: the walls whose heights reach the range, the doors and windows in them, the components and runs whose own
/// heights reach it once their group's geometry is added, the annotation in the plan (and in its parent's), the level's
/// rooms, the columns, and the rest of the kinds a plan finds among all of them.
fn plan_candidates<'a>(
    m: &'a Model,
    key: Guid,
    level: Guid,
    r: &PlanRange,
    components: &mut crate::component_geometry::Components<'a>,
) -> std::collections::BTreeSet<Id> {
    let lookup = m.lookup();
    // Every height a plan asks of an element, a hair over and under.
    let lo = r.depth.min(r.cut).min(r.top) - TOL - 1.0;
    let hi = r.cut.max(r.top).max(r.depth) + 1.0;
    let mut out: std::collections::BTreeSet<Id> = lookup.other().collect();
    for wall in lookup.walls_between(lo, hi) {
        out.insert(wall);
        let g = m.guid(wall);
        out.extend(lookup.referrers(&g).filter(|id| matches!(m.elements.get(*id), Some(Element::Door(_) | Element::Window(_)))));
    }
    for (group, one) in lookup.groups() {
        let found = match (group, m.elements.get(one)) {
            (crate::derived::Group::Component { .. }, Some(Element::Component(c))) => {
                let (under, over) = components.local_heights(c);
                if under > over {
                    // No geometry: none of them shows.
                    continue;
                }
                lookup.group_between(group, lo - over - 1.0, hi - under + 1.0)
            }
            (crate::derived::Group::Run { .. }, Some(e)) => match m.run_type(e) {
                Some(t) => {
                    let grow = 2.0 * t.width.abs().max(t.depth.abs()) + 1.0;
                    lookup.group_between(group, lo - grow, hi + grow)
                }
                None => lookup.group_between(group, f64::NAN, f64::NAN),
            },
            _ => lookup.group_between(group, f64::NAN, f64::NAN),
        };
        out.extend(found);
    }
    out.extend(lookup.referrers(&key));
    if let Some(Element::PlanView(crate::model::PlanView { parent: Some(p), .. })) = m.elements.get(key) {
        out.extend(lookup.referrers(p));
    }
    out.extend(lookup.referrers(&level));
    out.extend(tags_of_level(m, &lookup, level));
    out.extend(lookup.rare("column"));
    out.extend(lookup.rare("structuralcolumn"));
    out
}

/// The tags that may be in plan `key` of `level` with no view of their own, by id: those of what is of the level
/// (`Model::level_of`), what points at it, the doors and windows in its walls, the railings on its stairs and ramps, and
/// the annotation in its plan views.
fn tags_of_level(m: &Model, lookup: &crate::derived::Lookup, level: Guid) -> std::collections::BTreeSet<Id> {
    let mut hosts: Vec<Id> = lookup.referrers(&level).collect();
    let mut more = Vec::new();
    for &id in &hosts {
        if matches!(m.elements.get(id), Some(Element::Wall(_) | Element::Stair(_) | Element::Ramp(_) | Element::PlanView(_))) {
            more.extend(lookup.referrers(&m.guid(id)));
        }
    }
    hosts.extend(more);
    hosts
        .into_iter()
        .flat_map(|h| lookup.referrers(&m.guid(h)).collect::<Vec<_>>())
        .filter(|id| matches!(m.elements.get(*id), Some(Element::Tag(t)) if t.view.is_none()))
        .collect()
}

/// Of `candidates`, those that may show in plan `key` of `level`, range `r`.
fn plan_filter<'a>(
    m: &'a Model,
    key: Guid,
    level: Guid,
    r: &PlanRange,
    components: &mut crate::component_geometry::Components<'a>,
    candidates: impl Iterator<Item = (Id, &'a Element)>,
) -> Vec<(Id, &'a Element)> {
    // What reaches from the view depth (and a hair under it: a footing's or a run's top flush with it) to the range's top.
    let reaches = |bottom: f64, top: f64| bottom <= r.top && top > r.depth - TOL;
    candidates
        .filter(|(id, e)| match e {
            Element::Wall(w) => m.base_z(w) <= r.cut && m.top_z(w) > r.depth - TOL,
            Element::Door(o) | Element::Window(o) => m.wall(o.host).is_some_and(|w| m.spans(w, r.cut)),
            Element::Component(c) => {
                let (bottom, top) = components.heights_of(c);
                // Its plan symbols are annotation of its level, whatever height its forms reach.
                bottom < r.cut && top > r.depth || c.level == level && m.families.get(&c.family).is_some_and(|f| !f.symbols.is_empty())
            }
            Element::Room(x) => x.level == level,
            Element::RoomSeparator(x) => x.level == level,
            Element::Site(x) => x.level == level,
            Element::PropertyLine(x) => x.level == level,
            Element::BuildingPad(x) => x.level == level,
            Element::Rebar(_) => m.level_of(e) == Some(level),
            Element::Ceiling(_) => false,
            // A 3D view's own dimension shows in no plan.
            Element::Dimension3D(_) => false,
            Element::Text(_)
            | Element::DetailLine(_)
            | Element::FilledRegion(_)
            | Element::Dimension(_)
            | Element::Tag(_)
            | Element::Spot(_)
            | Element::DetailGroup(_)
            | Element::RepeatingDetail(_) => m.annotation_in(e, key),
            _ if e.column().is_some() => stands(m, *id, e, None).is_none_or(|(_, bottom, top)| reaches(bottom, top)),
            _ if e.run().is_some() => m.run_geom(e).is_none_or(|g| {
                let (bottom, top) = g.z_span();
                reaches(bottom, top)
            }),
            _ => true,
        })
        .collect()
}

/// Cut outlines filled together: each colour, whether it is halftone, and the outlines it fills.
type Unions = Vec<((Rgb, bool), Vec<Vec<P2>>)>;

/// The cut outlines `look` fills in the same colour, together, so no seam shows where two fills
/// meet: each colour, whether it is halftone, and the outlines it fills.
fn cut_unions(look: &Look, outlines: impl Iterator<Item = (Id, Vec<P2>)>) -> Unions {
    let mut unions: Unions = Vec::new();
    for (id, o) in outlines {
        let color = (look.cut_fill(id, POCHE), look.half(id));
        match unions.iter_mut().find(|(c, _)| *c == color) {
            Some((_, rings)) => rings.push(o),
            None => unions.push((color, vec![o])),
        }
    }
    unions
}

/// The object style of `owner`'s category; with no owner, what a project's categories start with.
fn style_of(m: &Model, owner: Option<Id>) -> ObjectStyle {
    owner.and_then(|id| m.elements.get(id)).map_or_else(|| m.object_style(""), |e| m.object_style_of(e))
}

/// Whether what `owner` is draws in the annotation's pens: a dimension, a tag, text, a spot elevation, a grid, a level, a
/// room's tag, the mark of a section or an elevation. The rest is the model's.
fn annotates(m: &Model, owner: Option<Id>) -> bool {
    matches!(
        owner.and_then(|id| m.elements.get(id)),
        Some(
            Element::Dimension(_)
                | Element::Tag(_)
                | Element::Text(_)
                | Element::Spot(_)
                | Element::Grid(_)
                | Element::Level(_)
                | Element::Room(_)
                | Element::Section(_)
                | Element::Elevation(_)
        )
    )
}

/// What of each cut outline is the edge of what the view cuts, as runs of points: its edges, but for the stretches
/// another outline lies against on their outer side, where two walls meet at a join or one runs into another; where two
/// runs meet, each carried on `reach` mm round the corner ([`turn_corners`]).
/// Boxes `(lo, hi)` by cells of a grid, to find those within `past` of one another without asking each of every other.
struct BoxGrid<'a> {
    boxes: &'a [(P2, P2)],
    past: f64,
    side: f64,
    cells: std::collections::HashMap<(i64, i64), Vec<usize>>,
    /// The boxes too big (or not finite) for cells: near anything.
    big: Vec<usize>,
}

impl<'a> BoxGrid<'a> {
    /// The most cells one box goes in.
    const MAX_CELLS: i64 = 256;

    fn new(boxes: &'a [(P2, P2)], past: f64) -> BoxGrid<'a> {
        // Cells about as big as a box is, by the median of their larger sides.
        let mut sides: Vec<f64> =
            boxes.iter().map(|(lo, hi)| (hi.0 - lo.0).max(hi.1 - lo.1)).filter(|s| s.is_finite() && *s >= 0.0).collect();
        sides.sort_by(f64::total_cmp);
        let side = sides.get(sides.len() / 2).copied().unwrap_or(1.0).max(1.0) + 2.0 * past;
        let mut grid = BoxGrid { boxes, past, side, cells: std::collections::HashMap::new(), big: Vec::new() };
        for (i, b) in boxes.iter().enumerate() {
            match grid.cells_of(b) {
                Some([x0, y0, x1, y1]) => {
                    for x in x0..=x1 {
                        for y in y0..=y1 {
                            grid.cells.entry((x, y)).or_default().push(i);
                        }
                    }
                }
                None => grid.big.push(i),
            }
        }
        grid
    }

    /// The cells box `b`, grown by `past`, reaches; none when it is too big for them.
    fn cells_of(&self, (lo, hi): &(P2, P2)) -> Option<[i64; 4]> {
        let cell = |v: f64| (v / self.side).floor();
        let c = [cell(lo.0 - self.past), cell(lo.1 - self.past), cell(hi.0 + self.past), cell(hi.1 + self.past)];
        if !c.iter().all(|v| v.is_finite() && v.abs() < 1e15) {
            return None;
        }
        let [x0, y0, x1, y1] = c.map(|v| v as i64);
        ((x1 - x0 + 1).saturating_mul(y1 - y0 + 1) <= Self::MAX_CELLS).then_some([x0, y0, x1, y1])
    }

    /// The boxes but `i` within `past` of box `i`, and perhaps others near, in order: those in a cell it reaches, and the
    /// big ones (with box `i` big, every one).
    fn near(&self, i: usize) -> Vec<usize> {
        let Some(b) = self.boxes.get(i) else { return Vec::new() };
        let mut found: Vec<usize> = match self.cells_of(b) {
            Some([x0, y0, x1, y1]) => {
                let mut v: Vec<usize> = self.big.clone();
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        v.extend(self.cells.get(&(x, y)).into_iter().flatten().copied());
                    }
                }
                v
            }
            None => (0..self.boxes.len()).collect(),
        };
        found.sort_unstable();
        found.dedup();
        let ((a, b), past) = (*b, self.past);
        found
            .into_iter()
            .filter(|&j| {
                j != i
                    && self
                        .boxes
                        .get(j)
                        .is_some_and(|(c, d)| a.0 <= d.0 + past && c.0 <= b.0 + past && a.1 <= d.1 + past && c.1 <= b.1 + past)
            })
            .collect()
    }
}

fn cut_edges(outlines: &[(Id, Vec<P2>)], reach: f64) -> Vec<(Id, Vec<Vec<P2>>)> {
    // How far past an edge, outward, a point is looked for in another outline: mm.
    const PAST: f64 = 1.0;
    let boxes: Vec<(P2, P2)> = outlines
        .iter()
        .map(|(_, o)| {
            o.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), p| {
                (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
            })
        })
        .collect();
    let near = |i: usize, j: usize| {
        let ((a, b), (c, d)) = (boxes[i], boxes[j]);
        i != j && a.0 <= d.0 + PAST && c.0 <= b.0 + PAST && a.1 <= d.1 + PAST && c.1 <= b.1 + PAST
    };
    let grid = BoxGrid::new(&boxes, PAST);
    let mut out = Vec::with_capacity(outlines.len());
    for (i, (id, o)) in outlines.iter().enumerate() {
        let found = grid.near(i);
        // Its twin, which tests hold it to: every outline asked.
        if crate::selfcheck::on(crate::selfcheck::Check::NearOutlines) {
            assert_eq!(found, (0..outlines.len()).filter(|&j| near(i, j)).collect::<Vec<_>>(), "the outlines near outline {i}");
        }
        let others: Vec<&[P2]> = found.into_iter().filter(|&j| near(i, j)).map(|j| outlines[j].1.as_slice()).collect();
        if others.is_empty() {
            let mut ring = o.clone();
            ring.extend(o.first().copied());
            out.push((*id, vec![ring]));
            continue;
        }
        // Outward is to the right of a counter-clockwise ring.
        let ccw = geom::area(o) > 0.0;
        let mut runs: Vec<Vec<P2>> = Vec::new();
        for (a, b) in geom::ring_edges(o) {
            let len = a.dist(b);
            if len <= 0.0 {
                continue;
            }
            // Cut where another outline's edge crosses it, or starts or ends on it.
            let mut ts = vec![0.0, 1.0];
            for (c, e) in others.iter().flat_map(|r| geom::ring_edges(r)) {
                ts.extend(geom::crossing(a, b, c, e));
                for p in [c, e] {
                    if geom::dist_to_segment(p, a, b) < 1e-6 * len.max(1.0) {
                        ts.push(geom::param_on_segment(p, a, b));
                    }
                }
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|x, y| (*x - *y) * len < 1e-6);
            let out_of = ((b - a).perp() * if ccw { -1.0 } else { 1.0 }).unit() * PAST;
            for w in ts.windows(2) {
                let (p, q) = (a + (b - a) * w[0], a + (b - a) * w[1]);
                let probe = (p + q) * 0.5 + out_of;
                if others.iter().any(|r| geom::contains(r, probe)) {
                    continue;
                }
                match runs.last_mut() {
                    Some(run) if run.last().is_some_and(|l| l.dist(p) < 1e-9) => run.push(q),
                    _ => runs.push(vec![p, q]),
                }
            }
        }
        // A run that ends where the first starts goes on round.
        if runs.len() > 1
            && let (Some(first), Some(last)) = (runs.first().and_then(|r| r.first().copied()), runs.last().and_then(|r| r.last().copied()))
            && first.dist(last) < 1e-9
        {
            let mut last = runs.pop().unwrap_or_default();
            last.extend(runs[0].iter().skip(1));
            runs[0] = last;
        }
        out.push((*id, runs));
    }
    turn_corners(&mut out, reach);
    out
}

/// A run of [`cut_edges`] in its category's pen: one that comes back to where it starts (an outline nothing lies against)
/// closed, so none of its corners is a line's round end.
fn ink_cut(d: &mut Drawing, run: &[P2]) {
    match run.split_last() {
        Some((last, ring)) if ring.len() > 2 && ring[0].dist(*last) < 1e-9 => d.line(ring, BY_CATEGORY, INK, true),
        _ => d.line(run, BY_CATEGORY, INK, false),
    }
}

/// Each run that ends where another ends (two walls' outlines meeting at a corner) carried on `reach` mm (or the other's
/// first edge, if shorter) along the other: so the corner is a join inside a line, mitred square, not two ends meeting with
/// a notch between them. What's carried on draws over the other's line, in its place.
fn turn_corners(runs: &mut [(Id, Vec<Vec<P2>>)], reach: f64) {
    // Every end: its point, and the way its run goes from it.
    let ends: Vec<(P2, P2)> = runs
        .iter()
        .flat_map(|(_, rs)| rs)
        .filter(|r| r.len() >= 2 && r.first().zip(r.last()).is_some_and(|(a, b)| a.dist(*b) > 1e-9))
        .flat_map(|r| {
            let n = r.len();
            [(r[0], r[1]), (r[n - 1], r[n - 2])]
        })
        .collect();
    // The ends by cell of a micron (a match is within a millionth of a millimetre, in its cell or the next); the ends
    // that aren't finite numbers are asked every time.
    const CELL: f64 = 1e-3;
    let key = |p: P2| ((p.0 / CELL).floor(), (p.1 / CELL).floor());
    let fits = |(x, y): (f64, f64)| x.is_finite() && y.is_finite() && x.abs() < 1e15 && y.abs() < 1e15;
    let mut cells: std::collections::HashMap<(i64, i64), Vec<usize>> = std::collections::HashMap::new();
    let mut loose = Vec::new();
    for (i, (p, _)) in ends.iter().enumerate() {
        match key(*p) {
            k if fits(k) => cells.entry((k.0 as i64, k.1 as i64)).or_default().push(i),
            _ => loose.push(i),
        }
    }
    let on = |at: P2, from: P2| {
        let matches = |i: &usize| ends.get(*i).is_some_and(|(p, q)| p.dist(at) < 1e-6 && q.dist(from) > 1e-9);
        // The first that matches, as a search of every end in order would find.
        let first = match key(at) {
            k if fits(k) => {
                let (x, y) = (k.0 as i64, k.1 as i64);
                let near = (x - 1..=x + 1).flat_map(|x| (y - 1..=y + 1).map(move |y| (x, y)));
                near.filter_map(|c| cells.get(&c)).flatten().chain(&loose).copied().filter(matches).min()
            }
            _ => (0..ends.len()).find(matches),
        };
        if crate::selfcheck::on(crate::selfcheck::Check::RunEnd) {
            assert_eq!(first, (0..ends.len()).find(|i| matches(i)), "the end a run carries on along");
        }
        first.and_then(|i| ends.get(i)).map(|(p, q)| {
            let len = p.dist(*q);
            *p + (*q - *p) * (reach.min(len) / len)
        })
    };
    for r in runs.iter_mut().flat_map(|(_, rs)| rs.iter_mut()) {
        let n = r.len();
        if n < 2 || r[0].dist(r[n - 1]) <= 1e-9 {
            continue;
        }
        let (head, tail) = (on(r[0], r[1]), on(r[n - 1], r[n - 2]));
        if let Some(q) = tail {
            r.push(q);
        }
        if let Some(q) = head {
            r.insert(0, q);
        }
    }
}

/// The plan of `level` fitted into `width` x `height` px, as an image of it is made: with its name lettered at its bottom left.
pub fn plan(m: &Model, level: impl Key, width: u32, height: u32) -> Drawing {
    let view = fit(&plan_extent(m, level), width, height, scale_of(m, level));
    draw_plan(m, level, view, false, annotation(m, level, view), scale_of(m, level), Out::Picture)
}

/// What a plan of `level` takes on paper: all it shows, or cropped, its crop region and the grids'
/// ends at its edges.
pub(crate) fn plan_extent(m: &Model, level: impl Key) -> Vec<P2> {
    match m.shown_crop(level) {
        None => plan_shows(m, level),
        Some([lo, hi]) => {
            let lookup = m.lookup();
            let grids = lookup.grids().filter_map(|id| match m.elements.get(id) {
                Some(Element::Grid(g)) => clip(g.start, g.end, lo, hi),
                _ => None,
            });
            [lo, hi].into_iter().chain(grids.flat_map(|(a, b)| [a, b])).collect()
        }
    }
}

/// Everything a plan of `level` shows, uncropped: its walls, floors, rooms, dimensions, tags,
/// notes and spot elevations, the grids, the marks of the sections and elevations, and its CAD imports.
#[doc(hidden)]
pub fn plan_shows(m: &Model, plan: impl Key) -> Vec<P2> {
    let key = m.guid(plan);
    let shows = plan_shows_among(m, key, true);
    // Its twin, which tests hold it to, as [`draw_plan`]'s.
    if crate::selfcheck::on(crate::selfcheck::Check::PlanShows) {
        assert!(
            shows == regions::uncounted(|| plan_shows_among(m, key, false)),
            "a plan shows other things among the elements that may show in it"
        );
    }
    shows
}

fn plan_shows_among(m: &Model, key: Guid, among: bool) -> Vec<P2> {
    // The plan's own guid says what it is: its graphics, range and annotation; its level is what it shows of the model.
    let level = m.plan_level(key).map_or(key, |l| m.guid(l));
    let r = m.shown_range(key);
    let look = Look::of(m, key);
    let index = match m.derived.made(&m.elements) {
        Some(_) if among => None,
        _ => Some(walls::Index::new(m)),
    };
    let index = index.as_ref();
    let mut components = crate::component_geometry::Components::new(m);
    let elements: Vec<(Id, &Element)> =
        if among { plan_elements(m, key, level, &r, false, &mut components) } else { m.elements.iter().map(|(id, e)| (*id, e)).collect() };
    let mut extent: Vec<P2> = cut_outlines(m, &elements, r.cut, &look, index).into_iter().flat_map(|(_, o)| o).collect();
    extent.extend(seen_below(m, &elements, &r, &look, index).into_iter().flat_map(|(_, o, _)| o));
    // What it shows besides what is cut and seen: of its level, in it, or of the kinds a plan looks for among all.
    let rest: Vec<(Id, &Element)> = if among {
        let lookup = m.lookup();
        let mut ids: std::collections::BTreeSet<Id> = lookup.other().collect();
        ids.extend(lookup.referrers(&level));
        ids.extend(tags_of_level(m, &lookup, level));
        ids.extend(lookup.referrers(&key));
        if let Some(Element::PlanView(crate::model::PlanView { parent: Some(p), .. })) = m.elements.get(key) {
            ids.extend(lookup.referrers(p));
        }
        ids.into_iter().filter_map(|id| Some((id, m.elements.get(id)?))).collect()
    } else {
        m.elements.iter().map(|(id, e)| (*id, e)).collect()
    };
    for (id, e) in rest.iter().filter(|(id, _)| look.shows(*id)) {
        match e {
            Element::CadImport(ci) if ci.level == level && ci.view.is_none_or(|v| v == key) => {
                extent.extend(m.cad_box(*id).into_iter().flatten())
            }
            Element::Floor(Floor { level: l, boundary, .. })
            | Element::Toposolid(Floor { level: l, boundary, .. })
            | Element::Ceiling(Ceiling { level: l, boundary, .. })
            | Element::Shaft(Shaft { level: l, boundary, .. })
                if *l == level =>
            {
                extent.extend(boundary)
            }
            Element::Site(st) if st.level == level => extent.extend(st.outline.iter().flat_map(|p| p.outer.iter().copied())),
            Element::PropertyLine(pl) if pl.level == level => extent.extend(pl.ring()),
            Element::BuildingPad(bp) if bp.level == level => extent.extend(bp.boundary.iter().copied()),
            Element::Room(r) if r.level == level => extent.push(r.point),
            Element::Mass(ms) if m.mass_cut_by(ms, r.cut) => extent.extend(m.mass_outline(ms, r.cut)),
            Element::Component(c) if c.level == level => {
                extent.extend(components.outlines(c));
                for mark in m.component_marks(c) {
                    match mark {
                        crate::family::Mark::Line(pts) => extent.extend(pts),
                        crate::family::Mark::Dot { centre, .. } => extent.push(centre),
                        crate::family::Mark::Label { at, .. } => extent.push(at),
                    }
                }
            }
            Element::Grid(g) => extent.extend([g.start, g.end]),
            Element::RoomSeparator(rs) if rs.level == level => extent.extend([rs.start, rs.end]),
            Element::Dimension(dm) if m.annotation_in(e, key) => extent.push(dm.at),
            Element::Section(v) | Element::Elevation(v) => extent.extend([v.start, v.end]),
            Element::Tag(t) if m.annotation_in(e, key) => extent.extend(m.anchor(t.host).map(|a| a + t.offset)),
            Element::Stair(Stair { level: l, top_level: t, .. }) | Element::Ramp(Ramp { level: l, top_level: t, .. })
                if *l == level || *t == Some(level) =>
            {
                if let Some(l) = stairs::layout(m, e) {
                    extent.extend(l.runs.iter().flat_map(|r| r.ring(l.width)).chain(l.pads.into_iter().flat_map(|p| p.ring)));
                }
            }
            Element::Railing(rl) if m.level_of(e) == Some(level) => extent.extend(stairs::railing(m, rl).into_iter().map(|(p, _)| p)),
            crate::model::run_arm!(_) if m.level_of(e) == Some(level) => extent.extend(crate::runs::outline_in(m, *id, index)),
            Element::Roof(r) if r.level == level || m.level_above(level) == Some(r.level) => extent.extend(m.roof_outline(r)),
            Element::Text(n) if m.annotation_in(e, key) => extent.extend([Some(n.at), n.leader].into_iter().flatten()),
            Element::DetailLine(l) if m.annotation_in(e, key) => extent.extend(l.curve.points()),
            Element::FilledRegion(r) if m.annotation_in(e, key) => extent.extend(r.sketch.iter().flat_map(|l| l.points())),
            Element::DetailGroup(g) if m.annotation_in(e, key) => extent.push(g.at),
            Element::RepeatingDetail(r) if m.annotation_in(e, key) => extent.extend([r.start, r.end]),
            Element::Spot(sp) if m.annotation_in(e, key) => extent.push(sp.at),
            _ => {}
        }
    }
    extent
}

/// An element that isn't in the model yet, drawn as a plan would draw it, its primitives owned
/// by `id`: what a tool previews. Annotation, doors, windows, columns, components, stairs, ramps, railings, roofs and masses only.
pub fn sketch(m: &Model, view: View, id: Id, e: &Element) -> Drawing {
    let mut d = Drawing::new(view);
    d.owner = Some(id);
    // Annotation as big as the view it will be in draws it.
    let on = m.annotation_plan(e);
    d.k = on.map_or(1.0, |v| annotation(m, v, view));
    // Its pens as the view it will be in draws them: annotation's, or the plan of its level's.
    let n = on.or_else(|| m.level_of(e)).map_or_else(|| m.units.default_scale(), |v| scale_of(m, v));
    (d.weights, d.paper) = (m.line_weights().at(n), paper_of(n, view.scale));
    match e {
        Element::Dimension(dm) => dimension(&mut d, m, dm),
        Element::Tag(t) => tag(&mut d, m, t, &crate::rooms::Rooms::new(m)),
        Element::Text(n) => note(&mut d, m, n),
        Element::DetailLine(l) => detail_line(&mut d, m, l),
        Element::FilledRegion(r) => filled_region(&mut d, m, r),
        Element::DetailGroup(g) => detail_group(&mut d, m, g),
        Element::RepeatingDetail(r) => r.groups().iter().for_each(|g| detail_group(&mut d, m, g)),
        Element::Spot(sp) if sp.slope => slope_mark(&mut d, sp.at, m.spot_slope(sp)),
        Element::Spot(sp) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
        Element::Section(v) => section_mark(&mut d, v),
        Element::Elevation(v) => elevation_mark(&mut d, v),
        Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) => d.line(&m.column_outline(e), Pen::HEAVY, INK, true),
        crate::model::run_arm!(_) => {
            if let Some(g) = m.run_geom(e) {
                // What is already in the model joins as the plan draws it; an element a tool previews before
                // creating it has no neighbours yet and takes its plain rectangle.
                let ring = if m.elements.get(id) == Some(e) { crate::runs::outline(m, id) } else { g.footprint() };
                d.line(&ring, Pen::HEAVY, INK, true)
            }
        }
        Element::Component(c) => {
            component(&mut d, m, &m.component_pieces(c));
            if d.detail != Detail::Coarse {
                marks(&mut d, &m.component_marks(c));
            }
        }
        // Drawn at its sill, where the cut plane would pass through it whatever the plan's cut is.
        Element::Door(o) | Element::Window(o) => {
            if let Some(w) = m.wall(o.host) {
                opening(&mut d, m, e, o, w, m.own_base(w) + o.sill, true);
            }
        }
        Element::Stair(st) => stair(&mut d, m, st, st.level),
        Element::Ramp(r) => ramp(&mut d, m, r, r.level),
        Element::Railing(rl) => d.line(&stairs::railing(m, rl).into_iter().map(|(p, _)| p).collect::<Vec<_>>(), Pen::MEDIUM, INK, false),
        Element::RoomSeparator(rs) => d.line(&[rs.start, rs.end], Pen::PROJECTION, INK, false),
        Element::Roof(r) => roof(&mut d, m, r),
        Element::Mass(ms) => d.dashed(&ms.boundary, Pen::MEDIUM, MASS, true),
        // A site tool's preview: the element as its plan draws it.
        Element::Site(st) => site::site(&mut d, id, st),
        Element::PropertyLine(pl) => site::property_line(&mut d, id, pl),
        Element::BuildingPad(bp) => site::building_pad(&mut d, id, bp),
        _ => {}
    }
    let annotation = on.is_some() || matches!(e, Element::Section(_) | Element::Elevation(_));
    let style = m.object_style_of(e);
    d.by_category(|_| style);
    d.weigh(|_| annotation);
    d
}

/// A reinforcing bar seen from above: its line, with each point once after another, thin; a bar that stands, a dot as thick as it
/// is. `seen` keeps what was drawn, so what lies on it is not drawn again.
fn bar_in_plan(d: &mut Drawing, rb: &crate::model::Rebar, dashed: bool, seen: &mut std::collections::HashSet<Vec<(i64, i64)>>) {
    let mut pts: Vec<P2> = Vec::new();
    for p in &rb.path {
        let q = P2(p[0], p[1]);
        if pts.last().is_none_or(|last| last.dist(q) > TOL) {
            pts.push(q);
        }
    }
    let px: Vec<(f32, f32)> = pts.iter().map(|p| d.view.px(*p)).collect();
    if !seen.insert(px.iter().map(|(x, y)| ((x * 10.0).round() as i64, (y * 10.0).round() as i64)).collect()) {
        return;
    }
    match pts.as_slice() {
        [] => {}
        [at] => {
            // At least a pixel across, so a thin bar seen end on is there.
            let r = (rb.diameter / 2.0).max(1.0 / d.view.scale);
            let ring: Vec<P2> = (0..12).map(|i| *at + crate::model::heading(f64::from(i) * 30.0) * r).collect();
            d.fill(&[ring], INK);
        }
        _ => d.stroke(px, BY_CATEGORY, INK, dashed.then(|| vec![10.0, 5.0]), false),
    }
}

/// How far past the lowest and highest heights a section's grids run, mm.
const GRID_PAST: f64 = 600.0;

/// From the lowest level or solid `look` shows to the highest.
/// The walls whose layers section or elevation `v` may cut between `first` and `last` along its line, by id: those near
/// the line in the lookup's tiles, by as much as a wall is thick, twice (its joins carry its layers past its ends).
fn section_walls<'a>(
    m: &'a Model,
    lookup: &crate::derived::Lookup,
    v: &Section,
    first: f64,
    last: f64,
) -> Vec<(Id, &'a crate::model::Wall)> {
    let u = (v.end - v.start).unit();
    let (a, b) = (v.start + u * first, v.start + u * last);
    let grow = 2.0 * m.types.wall.values().map(|t| t.thickness.abs()).fold(0.0, f64::max) + 10.0;
    let near = lookup.walls_near([a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)], grow);
    let walls: Vec<(Id, &crate::model::Wall)> = near.into_iter().filter_map(|id| Some((id, m.wall(id)?))).collect();
    // Its twin, which tests hold it to: no other wall's layer lines cross the line there.
    if crate::selfcheck::on(crate::selfcheck::Check::SectionWalls) {
        for (&id, e) in &m.elements {
            if let Element::Wall(_) = e
                && !walls.iter().any(|(w, _)| *w == id)
            {
                for line in walls::layer_lines(m, id) {
                    for seg in line.windows(2) {
                        let &[p, q] = seg else { continue };
                        let crosses = geom::line_intersection(v.start, v.end, p, q)
                            .filter(|x| geom::dist_to_segment(*x, p, q) < 1e-6)
                            .is_some_and(|x| (first - 1e-6..=last + 1e-6).contains(&(x - v.start).dot(u)));
                        assert!(!crosses, "wall {id}'s layers cross the section's line, and it isn't among the walls near it");
                    }
                }
            }
        }
    }
    walls
}

/// Bounds of external data used by cached level/model extents. Names and floating
/// point bits suffice: no scan points, drawing entities or old models are retained.
struct ExtentSources {
    clouds: Vec<(String, [[u64; 3]; 2])>,
    cads: Vec<(String, Option<[[u64; 2]; 2]>)>,
}

impl ExtentSources {
    fn cloud(cloud: &crate::cloud::Cloud) -> [[u64; 3]; 2] {
        let (lo, hi) = cloud.bounds();
        [lo.map(f64::to_bits), hi.map(f64::to_bits)]
    }

    fn cad(cad: &crate::cad::Cad) -> Option<[[u64; 2]; 2]> {
        cad.bulk().map(|bounds| bounds.map(|p| [p.0.to_bits(), p.1.to_bits()]))
    }

    fn of(m: &Model) -> Self {
        Self {
            clouds: m.clouds.iter().map(|(name, cloud)| (name.clone(), Self::cloud(cloud))).collect(),
            cads: m.cads.iter().map(|(name, cad)| (name.clone(), Self::cad(cad))).collect(),
        }
    }

    fn is_of(&self, m: &Model) -> bool {
        self.clouds.len() == m.clouds.len()
            && self.cads.len() == m.cads.len()
            && self.clouds.iter().zip(&m.clouds).all(|((name, bounds), (current, cloud))| name == current && *bounds == Self::cloud(cloud))
            && self.cads.iter().zip(&m.cads).all(|((name, bounds), (current, cad))| name == current && *bounds == Self::cad(cad))
    }
}

/// Geometry inputs, without holding a model snapshot or old library data.
/// Weak allocation identities also detach on in-place library edits.
struct GeometryKey {
    elements: (u64, u64),
    types: crate::shared::Identity<crate::model::Types>,
    materials: crate::shared::Identity<std::collections::BTreeMap<String, crate::model::Material>>,
    families: crate::shared::Identity<std::collections::BTreeMap<String, crate::family::Family>>,
    language: crate::language::Language,
    extents: ExtentSources,
}

impl GeometryKey {
    fn of(m: &Model) -> Self {
        Self {
            elements: m.elements.state(),
            types: m.types.identity(),
            materials: m.materials.identity(),
            families: m.families.identity(),
            language: m.language,
            extents: ExtentSources::of(m),
        }
    }
    fn is_of(&self, m: &Model) -> bool {
        self.elements == m.elements.state()
            && self.types.is_of(&m.types)
            && self.materials.is_of(&m.materials)
            && self.families.is_of(&m.families)
            && self.language == m.language
            && self.extents.is_of(m)
    }
}

/// Structural counts for opt-in component-extrema preparation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparationStats {
    pub deferred_components: usize,
    /// Round runs whose tubes are made only for a view that needs them.
    pub deferred_runs: usize,
    pub resolved_components: usize,
}

/// A snapshot's section/elevation geometry, component extrema or deferred envelopes,
/// and exact model extent. `new` prepares the full oracle; `new_deferred` can resolve
/// supported component extrema lazily when fitting or datum queries need them.
pub struct SectionWork {
    /// Elements and geometry-definition identities; stale work is rebuilt even after a library-only edit.
    of: GeometryKey,
    solids: view3d::PreparedSolids,
    ranges: std::collections::HashMap<Id, std::ops::Range<usize>>,
    #[doc(hidden)]
    pub placed: Vec<(Id, Vec<[f64; 3]>)>,
    /// Each element's box (as `placed` lists them, by id).
    #[doc(hidden)]
    pub boxes: Vec<Option<[[f64; 3]; 2]>>,
    extent: std::sync::OnceLock<[P2; 2]>,
    /// Beside `placed`, by place, not by id: a framing query asks of every element, and a hash each would cost more than
    /// the components' points it saves. Empty when nothing is deferred.
    deferred: Vec<Option<std::sync::OnceLock<Vec<[f64; 3]>>>>,
    deferred_components: usize,
    deferred_runs: usize,
    resolved: std::sync::atomic::AtomicUsize,
}

impl SectionWork {
    pub fn new(m: &Model) -> Self {
        let (solids, placed, boxes) = view3d::solids_and_extremes(m);
        let solids = view3d::PreparedSolids::Owned(solids);
        // Each element's solids lie together, in the order of the elements.
        let mut ranges: std::collections::HashMap<Id, std::ops::Range<usize>> = std::collections::HashMap::new();
        for (i, s) in solids.iter().enumerate() {
            ranges.entry(s.owner()).and_modify(|r| r.end = i + 1).or_insert(i..i + 1);
        }
        SectionWork {
            of: GeometryKey::of(m),
            solids,
            ranges,
            placed,
            boxes,
            extent: std::sync::OnceLock::new(),
            deferred: Default::default(),
            deferred_components: 0,
            deferred_runs: 0,
            resolved: Default::default(),
        }
    }

    /// Cooperative geometry preparation. No partial work is returned.
    pub fn new_cancellable(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Self> {
        let (solids, placed, boxes) = view3d::solids_and_extremes_cancellable(m, cancelled)?;
        Self::assembled_cancellable(m, view3d::PreparedSolids::Owned(solids), placed, boxes, cancelled)
    }

    /// Opt-in envelope preparation. Exact extrema are resolved only when a fitting
    /// or datum query needs them; `new` remains the full-computation oracle.
    pub fn new_deferred(m: &Model) -> Self {
        Self::new_deferred_cancellable(m, &|| false).unwrap_or_else(|| Self::new(m))
    }

    /// Cooperative [`Self::new_deferred`]. The lazy queries are cancellable too
    /// ([`section_fit_cancellable`], [`SectionOverlay::new_cancellable`]): a cancelled one
    /// leaves each component's extrema whole or unresolved, and the model's extent unset.
    pub fn new_deferred_cancellable(m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Self> {
        let ((solids, placed, boxes), deferred) = view3d::solids_and_extremes_deferred(m, cancelled)?;
        let mut work = Self::assembled_cancellable(m, view3d::PreparedSolids::Owned(solids), placed, boxes, cancelled)?;
        for ((id, _), d) in work.placed.iter().zip(&deferred) {
            match m.elements.get(*id) {
                Some(Element::Component(_)) if *d => work.deferred_components += 1,
                _ if *d => work.deferred_runs += 1,
                _ => {}
            }
        }
        if work.deferred_components + work.deferred_runs > 0 {
            work.deferred = deferred.into_iter().map(|d| d.then(std::sync::OnceLock::new)).collect();
        }
        (!cancelled()).then_some(work)
    }

    pub fn preparation_stats(&self) -> PreparationStats {
        PreparationStats {
            deferred_components: self.deferred_components,
            deferred_runs: self.deferred_runs,
            resolved_components: self.resolved.load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    pub(crate) fn from_prepared_cancellable(
        m: &Model,
        (solids, placed, boxes): view3d::PreparedMade,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<Self> {
        Self::assembled_cancellable(m, view3d::PreparedSolids::Cached(solids), placed, boxes, cancelled)
    }

    fn assembled_cancellable(
        m: &Model,
        solids: view3d::PreparedSolids,
        placed: Vec<(Id, Vec<[f64; 3]>)>,
        boxes: Vec<Option<[[f64; 3]; 2]>>,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<Self> {
        if cancelled() {
            return None;
        }
        let mut ranges: std::collections::HashMap<Id, std::ops::Range<usize>> = std::collections::HashMap::new();
        for (i, s) in solids.iter().enumerate() {
            if i % 256 == 0 && cancelled() {
                return None;
            }
            ranges.entry(s.owner()).and_modify(|r| r.end = i + 1).or_insert(i..i + 1);
        }
        Some(SectionWork {
            of: GeometryKey::of(m),
            solids,
            ranges,
            placed,
            boxes,
            extent: std::sync::OnceLock::new(),
            deferred: Default::default(),
            deferred_components: 0,
            deferred_runs: 0,
            resolved: Default::default(),
        })
    }

    /// The box of element `id`: `placed` lists elements as the model does, by id. None tells a caller nothing of where it is.
    #[doc(hidden)]
    pub fn box_of(&self, id: Id) -> Option<&[[f64; 3]; 2]> {
        let at = self.placed.binary_search_by_key(&id, |(id, _)| *id).ok()?;
        self.boxes.get(at)?.as_ref()
    }

    /// The cell of what `placed` holds at `at`, if its points are deferred.
    fn cell(&self, at: usize) -> Option<&std::sync::OnceLock<Vec<[f64; 3]>>> {
        self.deferred.get(at)?.as_ref()
    }

    fn known_points<'s>(&'s self, at: usize, points: &'s Vec<[f64; 3]>) -> Option<&'s Vec<[f64; 3]>> {
        match self.cell(at) {
            Some(cell) => cell.get(),
            None => Some(points),
        }
    }

    fn points<'s, 'm>(
        &'s self,
        m: &'m Model,
        (at, id): (usize, Id),
        points: &'s Vec<[f64; 3]>,
        components: &mut crate::component_geometry::Components<'m>,
    ) -> &'s Vec<[f64; 3]> {
        let Some(cell) = self.cell(at) else { return points };
        cell.get_or_init(|| {
            let points = m.elements.get(id).map_or_else(Vec::new, |e| view3d::lazy_points(m, id, e, components));
            self.resolved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            points
        })
    }

    fn deferred_heights(&self, m: &Model, look: &Look, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<(f64, f64)> {
        let mut out = m.levels().iter().map(|(_, level)| level.elevation).fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        for (i, ((id, _), bounds)) in self.placed.iter().zip(&self.boxes).enumerate() {
            if i % 256 == 0 && cancelled() {
                return None;
            }
            if self.cell(i).is_none()
                && look.shows(*id)
                && let Some([lo, hi]) = bounds
            {
                out = (out.0.min(lo[2]), out.1.max(hi[2]));
            }
        }
        let mut components = crate::component_geometry::Components::new(m);
        for (at, ((id, points), bounds)) in self.placed.iter().zip(&self.boxes).enumerate() {
            if self.cell(at).is_some() && bounds.is_none_or(|[lo, hi]| lo[2] < out.0 || hi[2] > out.1) && look.shows(*id) {
                if cancelled() {
                    return None;
                }
                for p in self.points(m, (at, *id), points, &mut components) {
                    out = (out.0.min(p[2]), out.1.max(p[2]));
                }
            }
        }
        Some(if out.0 <= out.1 { out } else { (0.0, 3000.0) })
    }

    /// Whether every geometry source still belongs to the same immutable definitions.
    fn is_of(&self, m: &Model) -> bool {
        self.of.is_of(m)
    }

    /// The solids `look` shows, but the components'.
    fn shown<'a>(&'a self, look: &'a Look) -> impl Iterator<Item = &'a view3d::Solid> + 'a {
        self.solids.iter().filter(|s| look.shows(s.owner()))
    }

    /// The lowest and highest of each element `look` shows: its box's, which are its points'.
    fn heights<'a>(&'a self, look: &'a Look) -> impl Iterator<Item = (f64, f64)> + 'a {
        self.placed.iter().zip(&self.boxes).filter(|((id, _), _)| look.shows(*id)).filter_map(|(_, b)| b.map(|[lo, hi]| (lo[2], hi[2])))
    }

    /// The least and the most of `along` (a measure linear in plan, as far along a line) over the points of every element
    /// `look` shows: of the elements whose boxes reach past what another surely reaches, their points. Of a box, `along` is
    /// the most at one of its corners, which no point inside it passes, its sums rounded as they are.
    fn along_range(&self, m: &Model, look: &Look, along: impl Fn(P2) -> f64, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<(f64, f64)> {
        #[allow(clippy::type_complexity)]
        let mut shown: Vec<(usize, Id, &Vec<[f64; 3]>, (f64, f64))> = Vec::new();
        for (i, ((id, p), b)) in self.placed.iter().zip(&self.boxes).enumerate() {
            if i % 256 == 0 && cancelled() {
                return None;
            }
            if !look.shows(*id) {
                continue;
            }
            let reach = b.map_or((f64::NEG_INFINITY, f64::INFINITY), |[lo, hi]| {
                let xs = [P2(lo[0], lo[1]), P2(hi[0], lo[1]), P2(hi[0], hi[1]), P2(lo[0], hi[1])].map(&along);
                let (a, z) = xs.iter().fold((f64::MAX, f64::MIN), |(a, z), x| (a.min(*x), z.max(*x)));
                // Not finite numbers: asked of its points.
                if a.is_finite() && z.is_finite() { (a, z) } else { (f64::NEG_INFINITY, f64::INFINITY) }
            });
            shown.push((i, *id, p, reach));
        }
        // What some point surely reaches: each element's first point.
        let first = |at, p| self.known_points(at, p).and_then(|p| p.first()).map(|q| along(P2(q[0], q[1]))).filter(|x| !x.is_nan());
        let (least, most) =
            shown.iter().filter_map(|(at, _, p, _)| first(*at, p)).fold((f64::MAX, f64::MIN), |(a, z), x| (a.min(x), z.max(x)));
        let mut out = (f64::MAX, f64::MIN);
        let mut components = crate::component_geometry::Components::new(m);
        for (at, id, p, (a, z)) in &shown {
            if *a <= least || *z >= most {
                if cancelled() {
                    return None;
                }
                for q in self.points(m, (*at, *id), p, &mut components) {
                    let x = along(P2(q[0], q[1]));
                    out = (out.0.min(x), out.1.max(x));
                }
            }
        }
        Some(out)
    }

    /// [`Model::model_extent`] of `m`, the model it was made of.
    /// Kept only when it is whole: a query cancelled part of the way sets nothing.
    fn extent(&self, m: &Model, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<[P2; 2]> {
        if let Some(extent) = self.extent.get() {
            return Some(*extent);
        }
        // Each box's lowest and highest corners in plan: the least and most of its points either way.
        let extent = if self.deferred.is_empty() {
            m.model_extent_of(self.boxes.iter().flatten().flat_map(|[lo, hi]| [P2(lo[0], lo[1]), P2(hi[0], hi[1])]))
        } else {
            let look = Look::default();
            let (x0, x1) = self.along_range(m, &look, |p| p.0, cancelled)?;
            let (y0, y1) = self.along_range(m, &look, |p| p.1, cancelled)?;
            m.model_extent_of([P2(x0, y0), P2(x1, y1)].into_iter().filter(|_| x0 <= x1 && y0 <= y1))
        };
        // Its twin, which tests hold it to: of every solid's corners.
        if crate::selfcheck::on(crate::selfcheck::Check::ModelExtent) {
            assert_eq!(extent, m.model_extent(), "the model's extent from the components' farthest points");
        }
        Some(*self.extent.get_or_init(|| extent))
    }
}

fn heights(m: &Model, w: &SectionWork, look: &Look, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<(f64, f64)> {
    let of = |solids: &mut dyn Iterator<Item = (f64, f64)>| {
        let zs = m.levels().into_iter().map(|(_, l)| l.elevation).chain(solids.flat_map(|(a, b)| [a, b]));
        let (lo, hi) = zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
        if lo <= hi { (lo, hi) } else { (0.0, 3000.0) }
    };
    let found = if w.deferred.is_empty() { of(&mut w.heights(look)) } else { w.deferred_heights(m, look, cancelled)? };
    // Its twin, which tests hold it to: of every solid the look shows.
    if crate::selfcheck::on(crate::selfcheck::Check::ViewHeights) {
        assert_eq!(found, of(&mut view3d::solids(m).iter().filter(|s| look.shows(s.owner())).map(|s| s.heights())), "a view's heights");
    }
    Some(found)
}

/// The crop region of view `id` as it shows: a cropped plan's corners; a cropped section's or
/// elevation's line, along it, and its crop's heights. None for a view that isn't cropped, or
/// another element.
pub fn crop_region(m: &Model, id: impl Key) -> Option<[P2; 2]> {
    match m.elements.get(id)? {
        Element::Level(l) => l.crop,
        Element::PlanView(v) => v.crop,
        Element::Section(v) | Element::Elevation(v) => v.crop.map(|[bottom, top]| [P2(0.0, bottom), P2(v.start.dist(v.end), top)]),
        _ => None,
    }
}

/// The heights section or elevation `id` would crop to: all of the model's, as far as its grids
/// reach past them.
#[doc(hidden)]
pub fn section_heights(m: &Model, id: impl Key) -> [f64; 2] {
    let (lo, hi) = heights(m, &SectionWork::new(m), &Look::of(m, id), &|| false).unwrap_or((0.0, 3000.0));
    [lo - GRID_PAST, hi + GRID_PAST]
}

/// How far along its line, from its start, section or elevation `v` shows: cropped, from its start
/// to its end; else as far as anything it shows reaches (what lies beyond its line, its levels and
/// its grids), and its line.
fn span(m: &Model, w: &SectionWork, v: &Section, look: &Look, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<(f64, f64)> {
    let len = v.start.dist(v.end);
    if v.crop.is_some() {
        return Some((0.0, len));
    }
    let u = (v.end - v.start).unit();
    let along = |q: P2| (q - v.start).dot(u);
    let (least, most) = w.along_range(m, look, along, cancelled)?;
    // Nothing shown, nothing found: no reach of its own.
    let solids = [least, most].into_iter().filter(|_| least <= most);
    // Its twin, which tests hold it to: every corner of every solid the look shows, as far along the line.
    if crate::selfcheck::on(crate::selfcheck::Check::ViewReach) {
        let fold = |xs: &mut dyn Iterator<Item = f64>| xs.fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let every = view3d::solids(m);
        let all = fold(&mut every.iter().filter(|s| look.shows(s.owner())).flat_map(|s| s.corners()).map(|p| along(P2(p[0], p[1]))));
        assert_eq!((least, most), all, "how far along its line a view's solids reach");
    }
    let levels = level_spans(m, w, v, look, cancelled)?.into_iter().flat_map(|(_, a, b)| [a, b]);
    let lookup = m.lookup();
    let grids = lookup.grids().filter(|id| look.shows(*id)).filter_map(|id| match m.elements.get(id) {
        Some(Element::Grid(g)) => geom::line_intersection(v.start, v.end, g.start, g.end).map(along),
        _ => None,
    });
    Some(solids.chain(levels).chain(grids).fold((0.0, len), |(lo, hi), x| (lo.min(x), hi.max(x))))
}

/// Where level `level` ends in section or elevation `view`, along its line from its start: its near
/// end and its far one, as far as the view shows. None where it doesn't show it.
pub fn level_ends(m: &Model, level: impl Key, view: impl Key) -> Option<(f64, f64)> {
    let v = m.section(view)?;
    let look = Look::of(m, view);
    let w = SectionWork::new(m);
    let (first, last) = span(m, &w, v, &look, &|| false)?;
    let level = m.elements.id(level)?;
    let (_, a, b) = level_spans(m, &w, v, &look, &|| false)?.into_iter().find(|(id, ..)| *id == level)?;
    let (a, b) = (a.max(first), b.min(last));
    (a < b).then_some((a, b))
}

fn section_level_ends(
    m: &Model,
    work: &SectionWork,
    id: Id,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<std::collections::BTreeMap<Id, (f64, f64)>> {
    let Some(v) = m.section(id) else { return Some(Default::default()) };
    let look = Look::of(m, id);
    let (first, last) = span(m, work, v, &look, cancelled)?;
    let spans = level_spans(m, work, v, &look, cancelled)?;
    Some(
        spans
            .into_iter()
            .filter_map(|(level, a, b)| {
                let (a, b) = (a.max(first), b.min(last));
                (a < b).then_some((level, (a, b)))
            })
            .collect(),
    )
}

/// The levels section or elevation `v` sees, each from how far along its line to how far: where
/// its extent lies, past its line (within its depth).
fn level_spans(m: &Model, w: &SectionWork, v: &Section, look: &Look, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Vec<(Id, f64, f64)>> {
    let u = (v.end - v.start).unit();
    let past = |q: P2| (q - v.start).dot(v.looking());
    let mut spans = Vec::new();
    for (id, l) in m.levels().into_iter().filter(|(id, _)| look.shows(*id)) {
        let [lo, hi] = match l.extent {
            Some(extent) => extent,
            None => w.extent(m, cancelled)?,
        };
        let corners = [lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)];
        let (near, far) = corners.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(past(*q)), b.max(past(*q))));
        if far < -TOL || v.depth.is_some_and(|d| near > d) {
            continue;
        }
        let (a, b) = corners.iter().fold((f64::MAX, f64::MIN), |(a, b), q| (a.min((*q - v.start).dot(u)), b.max((*q - v.start).dot(u))));
        spans.push((id, a, b));
    }
    Some(spans)
}

/// The heights a section or elevation shows: its crop's, else all of the model's.
fn shown_heights(m: &Model, w: &SectionWork, v: &Section, look: &Look, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<(f64, f64)> {
    match v.crop {
        Some([bottom, top]) => Some((bottom, top)),
        None => heights(m, w, look, cancelled),
    }
}

/// The view section or elevation `id` fits into `width` x `height` px: along its line from its
/// start, and up, with room for its levels' names and its grids' bubbles, which are as big as its
/// scale makes them.
pub fn section_fit(m: &Model, id: impl Key, width: u32, height: u32) -> View {
    section_fit_with(m, &SectionWork::new(m), id, width, height)
}

/// Fit a section using work retained with the same immutable model snapshot.
pub fn section_fit_with(m: &Model, work: &SectionWork, id: impl Key, width: u32, height: u32) -> View {
    section_fit_cancellable(m, work, id, width, height, &|| false).unwrap_or_else(|| fit(&[], width, height, m.units.default_scale()))
}

/// [`section_fit_with`] on a worker: None only when cancelled, between components' extrema. What isn't a section still has
/// its empty frame.
pub fn section_fit_cancellable(
    m: &Model,
    work: &SectionWork,
    id: impl Key,
    width: u32,
    height: u32,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<View> {
    if cancelled() {
        return None;
    }
    if !work.is_of(m) {
        let fresh = SectionWork::new_cancellable(m, cancelled)?;
        return section_fit_cancellable(m, &fresh, id, width, height, cancelled);
    }
    if m.section(id).is_none() {
        return Some(fit(&[], width, height, m.units.default_scale()));
    }
    let [lo, hi] = section_bare(m, work, id, cancelled)?;
    let (n, names) = (scale_of(m, id), level_names(m));
    Some(fit_by(|scale| [lo, P2(hi.0 + f64::from(annotation_of(n, scale) * names) / scale, hi.1)], width, height, n))
}

/// What section or elevation `id` shows, along its line and up, at `scale` px per mm, its annotation `k` times the size it is
/// laid out at: from its start to its end, past its lowest and highest heights, with room past the end for the widest
/// level name.
pub(crate) fn section_extent(m: &Model, w: &SectionWork, id: impl Key, scale: f64, k: f32) -> Option<[P2; 2]> {
    let [lo, hi] = section_bare(m, w, id, &|| false)?;
    Some([lo, P2(hi.0 + f64::from(k * level_names(m)) / scale, hi.1)])
}

/// Section or elevation `id` along its line, past its lowest and highest heights: all it shows but the names of its levels.
/// Cropped, from its start to its end; else as far as anything it shows reaches.
fn section_bare(m: &Model, w: &SectionWork, id: impl Key, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<[P2; 2]> {
    let v = m.section(id)?;
    let look = Look::of(m, id);
    let ((lo, hi), (first, last)) = (shown_heights(m, w, v, &look, cancelled)?, span(m, w, v, &look, cancelled)?);
    Some([P2(first, lo - GRID_PAST), P2(last, hi + GRID_PAST)])
}

/// The room, px, the widest level name or height takes past the end of a section's line, drawn at the size it is laid out at.
fn level_names(m: &Model) -> f32 {
    m.levels().iter().map(|(_, l)| font::width(&l.name, 10.0).max(font::width(&m.units.length(l.elevation), 10.0))).fold(0.0, f32::max)
        + 12.0
}

/// The camera, and what it keeps, that show what section or elevation `id` sees through `view`.
pub fn section_camera(m: &Model, id: impl Key, view: View) -> Option<(Camera, Clip)> {
    let v = m.section(id)?;
    let (u, back) = ((v.end - v.start).unit(), v.looking() * -1.0);
    let at = v.start + u * view.cx;
    let cam = Camera {
        azimuth: back.1.atan2(back.0).to_degrees(),
        elevation: 0.0,
        scale: view.scale,
        target: [at.0, at.1, view.cy],
        width: view.width,
        height: view.height,
        lens: None,
        shift: [0.0; 2],
        clip: None,
    };
    let column = |s: f64| f64::from(view.width) / 2.0 + (s - view.cx) * view.scale;
    let row = |z: f64| f64::from(view.height) / 2.0 - (z - view.cy) * view.scale;
    let rows = v.crop.map_or((f64::NEG_INFINITY, f64::INFINITY), |[bottom, top]| (row(top), row(bottom)));
    let columns = match v.crop {
        Some(_) => (column(0.0), column(v.start.dist(v.end))),
        None => (f64::NEG_INFINITY, f64::INFINITY),
    };
    Some((cam, Clip { near: 0.0, far: v.depth, x: columns, y: rows }))
}

/// What section or elevation `id` sees past its line through `view`, in hidden line: the
/// picture its cut, levels and grids ([`section`]) go over.
pub fn section_image(m: &Model, scene: &Scene, id: impl Key, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    let (cam, clip) = section_camera(m, id, view)?;
    Some(scene.cut_in(&cam, &clip, tints, &Look::of(m, id)))
}

/// What section or elevation `id` sees past its line through `view`, in hidden line, as [`section_image`] draws it from
/// the whole scene: drawn from a scene of only what can show in it ([`Scene::section`]), the same to the pixel.
pub fn section_picture(m: &Model, id: impl Key, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    section_picture_in(m, &SectionWork::new(m), id, view, tints)
}

/// [`section_picture`] and the drawing over it, [`section_over`], through `view`, the picture `pixels` (its view at the
/// screen's pixels): the whole model's work done once for both.
pub fn section_picture_and_over(m: &Model, id: Id, view: View, pixels: View, tints: &[(Id, Rgb)]) -> (Option<Image>, Drawing) {
    section_picture_and_over_with(m, &SectionWork::new(m), id, view, pixels, tints)
}

/// [`section_picture_and_over`] of `work`, which a front end keeps with model `m` (made again here if it is of another).
pub fn section_picture_and_over_with(
    m: &Model,
    work: &SectionWork,
    id: Id,
    view: View,
    pixels: View,
    tints: &[(Id, Rgb)],
) -> (Option<Image>, Drawing) {
    let fresh;
    let w = if work.is_of(m) {
        work
    } else {
        fresh = SectionWork::new(m);
        &fresh
    };
    let picture = section_picture_in(m, w, id, pixels, tints);
    let drawing = section_in(m, w, id, view, annotation(m, id, view), scale_of(m, id), picture.as_ref());
    (picture, drawing)
}

/// A section picture using work retained with the supplied model snapshot.
pub fn section_picture_with(m: &Model, work: &SectionWork, id: Id, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    if work.is_of(m) { section_picture_in(m, work, id, view, tints) } else { section_picture(m, id, view, tints) }
}

/// Render on a worker, checking cancellation between geometry/raster chunks.
/// `work` must belong to this exact immutable model snapshot.
pub fn section_picture_cancellable(
    m: &Model,
    work: &SectionWork,
    id: Id,
    view: View,
    tints: &[(Id, Rgb)],
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Image> {
    if cancelled() {
        return None;
    }
    if !work.is_of(m) {
        let fresh = SectionWork::new_cancellable(m, cancelled)?;
        return section_picture_cancellable(m, &fresh, id, view, tints, cancelled);
    }
    let (cam, clip) = section_camera(m, id, view)?;
    let look = Look::of(m, id);
    let scene = Scene::section_cancellable(m, (&work.solids, &work.ranges, &work.placed, &work.boxes), &cam, &clip, &look, cancelled)?;
    let image = scene.picture_cancellable(&cam, Some(&clip), tints, &look, cancelled);
    (!cancelled()).then_some(image)
}

/// [`section_picture`], of `w`'s solids and points.
pub(crate) fn section_picture_in(m: &Model, w: &SectionWork, id: impl Key, view: View, tints: &[(Id, Rgb)]) -> Option<Image> {
    let (cam, clip) = section_camera(m, id, view)?;
    let look = Look::of(m, id);
    let picture = Scene::section(m, (&w.solids, &w.ranges, &w.placed, &w.boxes), &cam, &clip, &look).cut_in(&cam, &clip, tints, &look);
    // Its twin, which tests hold it to: the picture of the whole scene.
    if crate::selfcheck::on(crate::selfcheck::Check::SectionPicture)
        && let Err(e) = picture.same_as(&Scene::new(m).cut_in(&cam, &clip, tints, &look))
    {
        #[allow(clippy::panic)]
        {
            panic!("a section's picture from what can show in it isn't the whole scene's: {e}");
        }
    }
    Some(picture)
}

/// What section or elevation `id` draws over its picture through `view`: where its line cuts
/// the solids, in poché; its levels, named past its end; the grids it crosses; its notes and spot
/// elevations; and its name. Cropped, only what lies between its crop's heights.
pub fn section(m: &Model, id: impl Key, view: View) -> Drawing {
    section_in(m, &SectionWork::new(m), id, view, annotation(m, id, view), scale_of(m, id), None)
}

/// [`section`], over `picture`, which [`section_image`] drew (at `view`'s size or a multiple of it): what its regions keep of
/// the picture's ink comes from there, instead of from a picture drawn again, which costs as much as the picture does.
pub fn section_over(m: &Model, id: impl Key, view: View, picture: &Image) -> Drawing {
    section_in(m, &SectionWork::new(m), id, view, annotation(m, id, view), scale_of(m, id), Some(picture))
}

/// Section or elevation `id` as `section` draws it, its annotation `k` times the size it is laid out at: a sheet's, at
/// paper size. Its pens are the line weights' at the scale `k` and `view` make, a viewport's.
pub fn section_at(m: &Model, id: impl Key, view: View, k: f32) -> Drawing {
    section_in(m, &SectionWork::new(m), id, view, k, f64::from(k) * sheet::DENSITY / view.scale, None)
}

/// [`section_at`], over `picture` as [`section_over`] is.
pub fn section_at_over(m: &Model, id: impl Key, view: View, k: f32, picture: &Image) -> Drawing {
    section_at_over_in(m, &SectionWork::new(m), id, view, k, picture)
}

/// [`section_at_over`], of `w`'s solids and points.
pub(crate) fn section_at_over_in(m: &Model, w: &SectionWork, id: impl Key, view: View, k: f32, picture: &Image) -> Drawing {
    section_in(m, w, id, view, k, f64::from(k) * sheet::DENSITY / view.scale, Some(picture))
}

/// The model cut and datums for one immutable snapshot and framing. Repainting
/// view annotations/region ink reads this base without constructing or cutting solids.
/// Keep it with the exact model snapshot supplied to `new`.
pub struct SectionOverlay {
    id: Id,
    base: Drawing,
    levels: std::collections::BTreeMap<Id, (f64, f64)>,
}

impl SectionOverlay {
    pub fn new(m: &Model, work: &SectionWork, id: Id, view: View) -> Self {
        if !work.is_of(m) {
            return Self::new(m, &SectionWork::new(m), id, view);
        }
        Self {
            id,
            base: section_base(m, work, id, view, annotation(m, id, view), scale_of(m, id)),
            levels: section_level_ends(m, work, id, &|| false).unwrap_or_default(),
        }
    }

    /// Cached datum endpoints for lightweight grips; never rebuild geometry on hover/selection.
    pub fn levels(&self) -> &std::collections::BTreeMap<Id, (f64, f64)> {
        &self.levels
    }

    pub fn new_cancellable(m: &Model, work: &SectionWork, id: Id, view: View, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Self> {
        if !work.is_of(m) {
            let fresh = SectionWork::new_cancellable(m, cancelled)?;
            return Self::new_cancellable(m, &fresh, id, view, cancelled);
        }
        let base = section_base_cancellable(m, work, id, view, annotation(m, id, view), scale_of(m, id), cancelled)?;
        if cancelled() {
            return None;
        }
        let levels = section_level_ends(m, work, id, cancelled)?;
        (!cancelled()).then_some(Self { id, base, levels })
    }

    pub fn draw(&self, m: &Model, picture: Option<&Image>) -> Drawing {
        self.draw_cancellable(m, picture, &|| false).unwrap_or_else(|| Drawing::new(self.base.view))
    }

    pub fn draw_cancellable(&self, m: &Model, picture: Option<&Image>, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Drawing> {
        if cancelled() {
            return None;
        }
        let b = &self.base;
        let mut d = Drawing {
            view: b.view,
            prims: b.prims.clone(),
            owners: b.owners.clone(),
            parts: b.parts.clone(),
            pens: b.pens.clone(),
            halftone: b.halftone.clone(),
            half: false,
            regions: regions::Layers::new(b.regions.end(), Some(self.id)),
            notes: b.notes.clone(),
            weights: b.weights,
            owner: None,
            part: b.part,
            k: b.k,
            paper: b.paper,
            detail: b.detail,
            screen: b.screen,
            pattern_styles: b.pattern_styles.clone(),
        };
        if let Some(image) = picture {
            behind_model(&mut d, m, image);
            d.picture(m, self.id, image);
        }
        section_finish_cancellable(m, self.id, d, cancelled)
    }
}

/// [`section_at`], its pens the line weights' at 1 : `n`.
fn section_in(m: &Model, work: &SectionWork, id: impl Key, view: View, k: f32, n: f64, picture: Option<&Image>) -> Drawing {
    let mut d = section_base(m, work, id, view, k, n);
    let Some(id) = m.elements.id(id) else { return d };
    if let Some(image) = picture {
        behind_model(&mut d, m, image);
        d.picture(m, id, image);
    }
    section_finish(m, id, d)
}

fn section_base(m: &Model, work: &SectionWork, id: impl Key, view: View, k: f32, n: f64) -> Drawing {
    section_base_cancellable(m, work, id, view, k, n, &|| false).unwrap_or_else(|| Drawing::new(view))
}

fn section_base_cancellable(
    m: &Model,
    work: &SectionWork,
    id: impl Key,
    view: View,
    k: f32,
    n: f64,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<Drawing> {
    if cancelled() {
        return None;
    }
    let mut d = Drawing::new(view);
    (d.k, d.paper, d.detail, d.weights) = (k, paper_of(n, view.scale), detail_of(m, id), m.line_weights().at(n));
    let Some(v) = m.section(id) else { return Some(d) };
    let look = Look::of(m, id);
    d.pattern_styles = look.pattern_styles();
    // Its images on the background layer, under all of it; where its picture shows the model they're cleared.
    images(&mut d, m, m.guid(id), crate::raster::DrawLayer::Background, Some(&look));
    let u = (v.end - v.start).unit();
    let (first, last) = span(m, work, v, &look, cancelled)?;
    // Each element's cut pieces, filled as one so where they meet (a roof's slopes at its ridge)
    // leaves no seam.
    let mut cuts: std::collections::BTreeMap<Id, Vec<Vec<P2>>> = std::collections::BTreeMap::new();
    let solids: Vec<&view3d::Solid> = work.shown(&look).collect();
    d.part(Part::Cut);
    // A bar lies inside the concrete it reinforces, so its cuts are dots over that poché, not pieces of it.
    let mut dots: Vec<(Id, Vec<P2>)> = Vec::new();
    // A mass is outlined where the line cuts it, below, not filled.
    // What lies all on one side of the line's upright plane is cut by none of it, whatever it is made of.
    let side = |q: P2| (q - v.start).dot(u.perp());
    let one_side = |id: Id| {
        work.box_of(id).is_some_and(|[lo, hi]| {
            let s = [P2(lo[0], lo[1]), P2(hi[0], lo[1]), P2(hi[0], hi[1]), P2(lo[0], hi[1])].map(side);
            s.iter().all(|x| *x > 1e-3) || s.iter().all(|x| *x < -1e-3)
        })
    };
    // Its twin, which tests hold it to: what is left out cuts nothing.
    if crate::selfcheck::on(crate::selfcheck::Check::OneSide) {
        for s in solids.iter().filter(|s| one_side(s.owner())) {
            if cancelled() {
                return None;
            }
            assert!(s.cuts(v.start + u * first, u, last - first).is_empty(), "{} is cut by a line it lies all on one side of", s.owner());
        }
    }
    for s in solids.iter().filter(|s| !matches!(m.elements.get(s.owner()), Some(Element::Mass(_))) && !one_side(s.owner())) {
        if cancelled() {
            return None;
        }
        let bar = matches!(m.elements.get(s.owner()), Some(Element::Rebar(_)));
        // Cut from where the view begins, then back along the line from its start.
        for ring in s.cuts(v.start + u * first, u, last - first) {
            let ring: Vec<P2> = ring.into_iter().map(|q| P2(q.0 + first, q.1)).collect();
            if bar { dots.push((s.owner(), ring)) } else { cuts.entry(s.owner()).or_default().push(ring) }
        }
    }
    // The round runs not made yet: only those whose boxes the line's plane crosses.
    let mut lazy = crate::component_geometry::Components::new(m);
    for (at, (id, _)) in work.placed.iter().enumerate() {
        if work.cell(at).is_none() || !look.shows(*id) {
            continue;
        }
        let Some(e) = m.elements.get(*id).filter(|e| e.run().is_some()) else { continue };
        // Its twin, which tests hold it to: what is left out cuts nothing, so it is made and cut all the same.
        let twin = crate::selfcheck::on(crate::selfcheck::Check::OneSideLazy);
        if !twin && one_side(*id) {
            continue;
        }
        if cancelled() {
            return None;
        }
        for s in view3d::lazy_solids(m, *id, e, &mut lazy) {
            let rings = s.cuts(v.start + u * first, u, last - first);
            if twin {
                assert!(!one_side(*id) || rings.is_empty(), "{id} is cut by a line it lies all on one side of");
            }
            for ring in rings {
                cuts.entry(*id).or_default().push(ring.into_iter().map(|q| P2(q.0 + first, q.1)).collect());
            }
        }
    }
    // Poché, then the edge of what is cut over it: joined walls show their union, as in plan, filled
    // as one first so no seam shows where pieces meet.
    d.owner = None;
    let mut unions: Unions = Vec::new();
    for (owner, rings) in &cuts {
        if cancelled() {
            return None;
        }
        let color = (look.cut_fill(*owner, POCHE), look.half(*owner));
        match unions.iter_mut().find(|(c, _)| *c == color) {
            Some((_, all)) => all.extend(rings.iter().cloned()),
            None => unions.push((color, rings.clone())),
        }
    }
    for ((color, half), rings) in unions {
        if cancelled() {
            return None;
        }
        d.halftone_next(half);
        d.fill(&rings, color);
    }
    d.halftone_next(false);
    for (owner, rings) in &cuts {
        if cancelled() {
            return None;
        }
        d.owner = Some(*owner);
        if pattern_style(&d, true).is_some() {
            d.fill(rings, PAPER);
            override_patterns(&mut d, m, rings, true, (P2(0.0, 0.0), 0.0));
        } else {
            d.fill(rings, POCHE);
        }
    }
    let pieces: Vec<(Id, Vec<P2>)> = cuts.iter().flat_map(|(owner, rings)| rings.iter().map(|r| (*owner, r.clone()))).collect();
    for (owner, runs) in cut_edges(&pieces, 1.0 / d.view.scale) {
        if cancelled() {
            return None;
        }
        d.owner = Some(owner);
        for run in runs {
            ink_cut(&mut d, &run);
        }
    }
    for (owner, ring) in dots {
        if cancelled() {
            return None;
        }
        d.owner = Some(owner);
        d.fill(std::slice::from_ref(&ring), INK);
        ink_cut(&mut d, &ring);
    }
    // A compound wall's layers where the line cuts it: each its material's cut pattern, between where
    // the line crosses its boundaries; a thin line up each boundary between two, as high as the wall
    // stands there, around its openings.
    let lookup = m.lookup();
    for (wall, w) in section_walls(m, &lookup, v, first, last) {
        if cancelled() {
            return None;
        }
        let layers = m.type_layers("wall", &w.type_name);
        if layers.is_empty() || !look.shows(wall) || d.detail == Detail::Coarse {
            continue;
        }
        d.owner = Some(wall);
        let geometry = walls::layer_geometry(m, wall, None);
        let intervals = if geometry.joined() {
            // At a butt the core and its neighbouring finish can end at different stations.
            // A section can also run lengthwise into the return and cross only its cap.
            (0..layers.len()).map(|k| view3d::crossings(&geometry.band(k), v.start, u)).collect::<Vec<_>>()
        } else {
            let crossings: Vec<Vec<f64>> = geometry
                .lines
                .iter()
                .map(|line| {
                    let mut xs: Vec<f64> = line
                        .windows(2)
                        .filter_map(|seg| {
                            let &[p, q] = seg else { return None };
                            let x = geom::line_intersection(v.start, v.end, p, q).filter(|x| geom::dist_to_segment(*x, p, q) < 1e-6)?;
                            Some((x - v.start).dot(u))
                        })
                        .collect();
                    xs.sort_by(f64::total_cmp);
                    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
                    xs
                })
                .collect();
            crossings
                .windows(2)
                .map(|pair| {
                    let (a, b) = (&pair[0], &pair[1]);
                    if a.len() == b.len() { a.iter().copied().zip(b.iter().copied()).collect() } else { Vec::new() }
                })
                .collect()
        };
        for (layer, spans) in layers.iter().zip(intervals) {
            for (s0, s1) in spans {
                let (s0, s1) = (s0.min(s1).max(first), s0.max(s1).min(last));
                for r in cuts.get(&wall).into_iter().flatten() {
                    let part = geom::clip(&geom::clip(r, |q| q.0 - s0), |q| s1 - q.0);
                    if part.len() >= 3 {
                        let room = (s1 - s0) * d.view.scale;
                        cut_layer(&mut d, m, layer, &part, P2(1.0, 0.0), room);
                    }
                }
            }
        }
        for line in geometry.interfaces() {
            for seg in line.windows(2) {
                let &[p, q] = seg else { continue };
                let Some(x) = geom::line_intersection(v.start, v.end, p, q).filter(|x| geom::dist_to_segment(*x, p, q) < 1e-6) else {
                    continue;
                };
                let along = (x - v.start).dot(u);
                if !(first..=last).contains(&along) {
                    continue;
                }
                let own = work.solids.in_range(work.ranges.get(&wall).cloned().unwrap_or(0..0));
                // A butt interface can lie on the body edge; test either side of that edge.
                let n = (q - p).unit().perp() * 1e-6;
                for s in own.filter(|s| s.owner() == wall && (s.over(x) || geometry.joined() && (s.over(x + n) || s.over(x - n)))) {
                    let (z0, z1) = s.z_at(x);
                    d.part(Part::Pattern);
                    d.line(&[P2(along, z0), P2(along, z1)], Pen::PROJECTION, INK, false);
                    d.part(Part::Cut);
                }
            }
        }
    }
    // Each layer of a compound floor, roof or ceiling it cuts: its material's cut pattern, a thin
    // line between each two, parallel to its faces.
    let coarse = d.detail == Detail::Coarse;
    for (&owner, rings) in cuts.iter().filter(|_| !coarse) {
        if cancelled() {
            return None;
        }
        let layers = match m.elements.get(owner) {
            Some(Element::Floor(f) | Element::Toposolid(f)) => m.type_layers("floor", &f.type_name),
            Some(Element::Roof(r)) => m.type_layers("roof", &r.type_name),
            Some(Element::Ceiling(c)) => m.type_layers("ceiling", &c.type_name),
            _ => continue,
        };
        let total: f64 = layers.iter().map(|l| l.thickness).sum();
        if total <= 0.0 {
            continue;
        }
        // A shaped floor's or roof's over a level underside: the layers above its variable one from the top, those below it
        // from the bottom, the variable one between; any other's in proportion to their thicknesses.
        let variable = m.variable_under(owner);
        d.owner = Some(owner);
        for r in rings {
            let &[a0, b0, b1, a1] = r.as_slice() else { continue };
            // Where each layer starts down from the top at each end.
            let down = |top: P2, bottom: P2| {
                let depth = top.1 - bottom.1;
                crate::model::Variable::bounds(variable, layers, depth)
                    .into_iter()
                    .map(|x| if depth.abs() > 1e-9 { top + (bottom - top) * (x / depth) } else { top })
                    .collect::<Vec<P2>>()
            };
            let (at_a, at_b) = (down(a1, a0), down(b1, b0));
            for (k, layer) in layers.iter().enumerate() {
                let (Some(&p), Some(&q), Some(&p2), Some(&q2)) = (at_a.get(k), at_b.get(k), at_a.get(k + 1), at_b.get(k + 1)) else {
                    continue;
                };
                let room = (p.1 - p2.1).abs().min((q.1 - q2.1).abs()) * d.view.scale;
                cut_layer(&mut d, m, layer, &[p2, q2, q, p], (q - p).unit(), room);
                if k > 0 {
                    d.part(Part::Pattern);
                    d.line(&[p, q], Pen::PROJECTION, INK, false);
                    d.part(Part::Cut);
                }
            }
        }
    }
    for (mid, e) in lookup.rare("mass").filter(|id| look.shows(*id)).filter_map(|id| Some((id, m.elements.get(id)?))) {
        if cancelled() {
            return None;
        }
        let Element::Mass(ms) = e else { continue };
        let (z0, z1) = m.span(ms.level, ms.base_offset, ms.top_level, ms.height);
        d.owner = Some(mid);
        // Lofted, where the line cuts its skin.
        if ms.lofted() {
            for line in
                massing::section_lines(&massing::tris(&ms.footprint(), &ms.top_ring(), (z0, z1)), v.start + u * first, u, last - first)
            {
                let line: Vec<P2> = line.into_iter().map(|q| P2(q.0 + first, q.1)).collect();
                d.dashed(&line, BY_CATEGORY, MASS, false);
            }
            continue;
        }
        for (s0, s1) in view3d::crossings(&ms.boundary, v.start, u) {
            let (a, b) = (s0.max(first), s1.min(last));
            if b - a > 1e-6 {
                d.dashed(&[P2(a, z0), P2(b, z0), P2(b, z1), P2(a, z1)], BY_CATEGORY, MASS, true);
            }
        }
    }
    d.owner = None;
    d.part(Part::Projection);
    let (lo, hi) = shown_heights(m, work, v, &look, cancelled)?;
    if v.crop.is_some() {
        let (a, b) = (d.view.px(P2(first, hi)), d.view.px(P2(last, lo)));
        d.crop(a, b, |_| false);
    }
    d.model_under_regions(m.elements.id(id));
    // Each level over as much of its extent as the view sees, named past its end.
    for (level, from, to) in level_spans(m, work, v, &look, cancelled)? {
        if cancelled() {
            return None;
        }
        let (from, to) = (from.max(first), to.min(last));
        let Some(l) = m.level(level).filter(|l| from < to && ((lo..=hi).contains(&l.elevation) || v.crop.is_none())) else { continue };
        d.owner = Some(level);
        let (a, b) = (d.view.px(P2(from, l.elevation)), d.view.px(P2(to, l.elevation)));
        d.stroke(vec![a, b], Pen::PROJECTION, INK, Some(vec![12.0, 4.0]), false);
        for (text, dy) in [(l.name.clone(), -8.0), (m.units.length(l.elevation), 8.0)] {
            let h = d.px(10.0);
            d.text(&text, (b.0 + d.px(8.0) + font::width(&text, h) / 2.0, b.1 + d.px(dy)), h, INK);
        }
    }
    // Grids running into what it sees: across its line, reaching past it (within its depth).
    let past = |q: P2| (q - v.start).dot(v.looking());
    for (grid_id, e) in lookup.grids().filter(|id| look.shows(*id)).filter_map(|id| Some((id, m.elements.get(id)?))) {
        if cancelled() {
            return None;
        }
        if let Element::Grid(g) = e
            && let Some(x) = geom::line_intersection(v.start, v.end, g.start, g.end)
            && past(g.start).max(past(g.end)) >= -TOL
            && v.depth.is_none_or(|far| past(g.start).min(past(g.end)) <= far)
            && (first..=last).contains(&(x - v.start).dot(u))
        {
            let along = (x - v.start).dot(u);
            d.owner = Some(grid_id);
            grid(&mut d, P2(along, lo - GRID_PAST), P2(along, hi + GRID_PAST), &g.name);
        }
    }
    if cancelled() {
        return None;
    }
    Some(d)
}

fn section_finish(m: &Model, id: Id, d: Drawing) -> Drawing {
    let view = d.view;
    section_finish_cancellable(m, id, d, &|| false).unwrap_or_else(|| Drawing::new(view))
}

fn section_finish_cancellable(m: &Model, id: Id, mut d: Drawing, cancelled: &(dyn Fn() -> bool + Sync)) -> Option<Drawing> {
    if cancelled() {
        return None;
    }
    let Some(v) = m.section(id) else { return Some(d) };
    let (lookup, look, view) = (m.lookup(), Look::of(m, id), d.view);
    // Its images on the foreground layer, over the model.
    images(&mut d, m, m.guid(id), crate::raster::DrawLayer::Foreground, Some(&look));
    // What is drawn in it, from what points at it: its regions, notes, details and spot elevations.
    let own: Vec<(Id, &Element)> =
        lookup.referrers(&m.guid(id)).filter(|id| look.shows(*id)).filter_map(|id| Some((id, m.elements.get(id)?))).collect();
    // Filled regions over the model, under the annotation.
    for &(nid, e) in &own {
        if cancelled() {
            return None;
        }
        if let Element::FilledRegion(r) = e
            && m.same(r.view, id)
        {
            d.owner = Some(nid);
            filled_region(&mut d, m, r);
        }
    }
    for &(nid, e) in &own {
        if cancelled() {
            return None;
        }
        d.owner = Some(nid);
        match e {
            Element::Text(n) if m.same(n.view, id) => note(&mut d, m, n),
            Element::DetailLine(l) if m.same(l.view, id) => detail_line(&mut d, m, l),
            Element::DetailGroup(g) if m.same(g.view, id) => detail_group(&mut d, m, g),
            Element::RepeatingDetail(r) if m.same(r.view, id) => r.groups().iter().for_each(|g| detail_group(&mut d, m, g)),
            // A spot slope reads a plan's floor or roof; a section has none to show.
            Element::Spot(sp) if m.same(sp.view, id) && !sp.slope => {
                let z = m.spot_value(sp).unwrap_or(sp.at.1);
                spot_mark(&mut d, view.px(P2(sp.at.0, z)), Some(z), m.units);
            }
            _ => {}
        }
    }
    d.owner = None;
    d.by_category(|o| style_of(m, o));
    look.dress(&mut d);
    d.weigh(|o| annotates(m, o));
    let at = (24.0 + font::width(&v.name, 16.0) / 2.0, view.height as f32 - 26.0);
    d.text_turned(&v.name, at, 16.0, 0.0, INK);
    (!cancelled()).then_some(d)
}

/// Section or elevation `id` through `view`, as a PNG: its picture, and its drawing over it.
pub fn section_png(m: &Model, id: impl Key, view: View) -> Result<Vec<u8>, String> {
    section_png_noted(m, id, view).map(|(png, _)| png)
}

/// [`section_png`], and what its drawing could not draw as asked ([`Drawing::notes`]).
pub fn section_png_noted(m: &Model, id: impl Key, view: View) -> Result<(Vec<u8>, Vec<String>), String> {
    let w = SectionWork::new(m);
    let image = section_picture_in(m, &w, id, view, &[]).ok_or_else(|| format!("{id} is not a section or an elevation"))?;
    let size = tiny_skia::IntSize::from_wh(image.width, image.height).ok_or("the image size is out of range")?;
    let drawing = section_in(m, &w, id, view, annotation(m, id, view), scale_of(m, id), Some(&image));
    let mut pm = Pixmap::from_vec(image.rgba, size).ok_or("the image size is out of range")?;
    paint(&mut pm, &drawing);
    Ok((pm.encode_png().map_err(|e| e.to_string())?, drawing.notes))
}

/// The plan of `level` as `view` shows it: what a canvas that pans and zooms draws.
pub fn draw(m: &Model, level: impl Key, view: View) -> Drawing {
    draw_plan(m, level, view, false, annotation(m, level, view), scale_of(m, level), Out::Screen)
}

/// The reflected ceiling plan of `level` fitted into `width` x `height` px.
pub fn ceiling_plan(m: &Model, level: impl Key, width: u32, height: u32) -> Drawing {
    let view = fit(&plan_shows(m, level), width, height, scale_of(m, level));
    draw_plan(m, level, view, true, annotation(m, level, view), scale_of(m, level), Out::Picture)
}

/// The reflected ceiling plan of `level` as `view` shows it: cut [`CEILING_CUT`] above the level
/// (over the doors) and looking up, at the ceilings up to the level above, as a floor plan lies
/// (not mirrored). Its grids and its sections' marks; no annotation of the floor plan's.
pub fn draw_ceiling(m: &Model, level: impl Key, view: View) -> Drawing {
    draw_plan(m, level, view, true, annotation(m, level, view), scale_of(m, level), Out::Screen)
}

/// How big the annotation of view `id` is drawn through `view`, times the size it is laid out at. A view with a scale
/// holds it as paper at that scale would: its lettering and marks a fixed size on paper, which `scale` mm of the model per
/// mm of paper and `view` showing `view.scale` pixels to the mm make of it ([`sheet::DENSITY`] pixels to a mm of paper); on
/// a sheet, where the viewport sets the scale, it is 1. A view with no scale of its own is at the units' default
/// ([`crate::units::Units::default_scale`]). A shortcut: clamped to a tenth and ten times, so a hostile scale or zoom
/// can't draw lettering a screen tall; widen it if a plan at 1 : 10000 on screen wants more. The pens aren't clamped
/// ([`paper_of`], [`Drawing::width`]): a line is as wide as on the paper the view stands for, however far in or out.
pub fn annotation(m: &Model, id: impl Key, view: View) -> f32 {
    annotation_of(scale_of(m, id), view.scale)
}

/// [`annotation`] for a view at scale 1 : `n` shown at `scale` pixels to the mm.
fn annotation_of(n: f64, scale: f64) -> f32 {
    (n * scale / sheet::DENSITY).clamp(0.1, 10.0) as f32
}

/// [`annotation_of`] unclamped, what sizes the pens: but for a hostile scale or zoom, 0 (a hairline) when it isn't a
/// number and no more than a million.
fn paper_of(n: f64, scale: f64) -> f32 {
    let k = n * scale / sheet::DENSITY;
    if k.is_finite() && k > 0.0 { k.min(1e6) as f32 } else { 0.0 }
}

/// The level of detail view `id` draws at: its own, or its template's where that controls it.
fn detail_of(m: &Model, id: impl Key) -> Detail {
    crate::graphics::graphics_of(m, id).map_or_else(Detail::default, |g| g.resolved(m).detail)
}

/// Plan `level` as `draw` draws it, its annotation `k` times the size it is laid out at: a sheet's, at paper size.
pub fn draw_at(m: &Model, level: impl Key, view: View, k: f32) -> Drawing {
    draw_plan(m, level, view, false, k, f64::from(k) * sheet::DENSITY / view.scale, Out::Paper)
}

/// The points of the scans a plan sees, from its cut plane down to its view depth, as Revit draws a point cloud in plan: from
/// above, each a dot [`view3d::DOT`] pixels wide in its colour (or the view's), the highest at each pixel on top, no more than
/// [`view3d::DRAWN`] of them among the scans. Each scan is one picture, no bigger than its dots and clear where it has none, so
/// it picks where its points are.
fn scans(d: &mut Drawing, m: &Model, range: &PlanRange, look: &Look) {
    let placed: Vec<(Id, &crate::model::PointCloud, &crate::cloud::Cloud)> = m
        .rare("pointcloud")
        .into_iter()
        .filter_map(|id| Some((id, m.elements.get(id)?)))
        .filter_map(|(id, e)| match e {
            Element::PointCloud(pc) if look.shows(id) => Some((id, pc, m.clouds.get(&pc.cloud)?)),
            _ => None,
        })
        .collect();
    let step = view3d::every(placed.iter().map(|(_, _, c)| c.len()).sum());
    let (w, h) = (i64::from(d.view.width), i64::from(d.view.height));
    let (dot, half) = (view3d::DOT as i64, view3d::DOT as f64 / 2.0);
    let grey = Rgb(crate::cloud::GREY, crate::cloud::GREY, crate::cloud::GREY);
    for (id, pc, cloud) in placed {
        let place = m.cloud_place(pc);
        let colours = cloud.colours();
        // Each point it sees: its dot's corner, px, its height and its colour.
        let mut seen: Vec<(i64, i64, f64, Rgb)> = Vec::new();
        for (i, p) in cloud.points().iter().enumerate().step_by(step) {
            let [x, y, z] = place(*p);
            if !(range.depth - TOL..=range.cut).contains(&z) {
                continue;
            }
            let (px, py) = d.view.px(P2(x, y));
            let (x0, y0) = ((f64::from(px) - half).round(), (f64::from(py) - half).round());
            if !(x0 > -(dot as f64) && y0 > -(dot as f64) && x0 < w as f64 && y0 < h as f64) {
                continue;
            }
            let base = colours.and_then(|c| c.get(i)).map_or(grey, |c| Rgb(c[0], c[1], c[2]));
            seen.push((x0 as i64, y0 as i64, z, look.surface(id, base)));
        }
        let Some((lo, hi)) = seen.iter().fold(None, |b: Option<((i64, i64), (i64, i64))>, &(x, y, ..)| {
            let ((x0, y0), (x1, y1)) = b.unwrap_or(((x, y), (x, y)));
            Some(((x0.min(x), y0.min(y)), (x1.max(x), y1.max(y))))
        }) else {
            continue;
        };
        let (lo, hi) = ((lo.0.max(0), lo.1.max(0)), ((hi.0 + dot).min(w), (hi.1 + dot).min(h)));
        let (pw, ph) = ((hi.0 - lo.0).max(0) as usize, (hi.1 - lo.1).max(0) as usize);
        let mut top = vec![f64::NEG_INFINITY; pw * ph];
        let mut rgba = vec![0; pw * ph * 4];
        for (x0, y0, z, c) in seen {
            for py in y0.max(lo.1)..(y0 + dot).min(hi.1) {
                for px in x0.max(lo.0)..(x0 + dot).min(hi.0) {
                    let k = (py - lo.1) as usize * pw + (px - lo.0) as usize;
                    if let (Some(t), Some(out)) = (top.get_mut(k), rgba.get_mut(4 * k..4 * k + 4))
                        && z > *t
                    {
                        *t = z;
                        out.copy_from_slice(&[c.0, c.1, c.2, 255]);
                    }
                }
            }
        }
        d.own(Some(id));
        d.push(Prim::Image { at: (lo.0 as f32, lo.1 as f32), width: pw as u32, height: ph as u32, rgba, ink: look.filled(id) });
    }
    d.own(None);
}

/// The CAD drawings a plan shows, under all the rest but its scans, as Revit draws an import in the background: those placed on
/// its level, in each of its plans or in this one only, of the layers the import and the view show, as the view's Imported
/// Categories draw them.
fn cad_imports(d: &mut Drawing, m: &Model, key: crate::ids::Guid, level: crate::ids::Guid, look: &Look) {
    for (id, e) in m.rare("cadimport").into_iter().filter_map(|id| Some((id, m.elements.get(id)?))) {
        let Element::CadImport(ci) = e else { continue };
        if ci.level != level || ci.view.is_some_and(|v| v != key) || !look.shows(id) {
            continue;
        }
        let Some(cad) = m.cads.get(&ci.cad) else { continue };
        d.own(Some(id));
        cad_drawing(d, cad, ci, look.hidden_layers(&ci.cad), look.import_look(&ci.cad));
    }
    d.own(None);
}

/// Pixels of a run of a drawing, those nearer than [`ONE_PIXEL`] to the last kept left out (the last always kept): zoomed out,
/// a dense drawing's many points under a pixel are the run as it shows, in a fraction of the points.
fn thinned(pts: impl Iterator<Item = (f32, f32)>) -> Vec<(f32, f32)> {
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut last = None;
    for q in pts {
        last = Some(q);
        if out.last().is_none_or(|p| (q.0 - p.0).abs() >= ONE_PIXEL || (q.1 - p.1).abs() >= ONE_PIXEL) {
            out.push(q);
        }
    }
    if let (Some(q), Some(p)) = (last, out.last())
        && q != *p
    {
        out.push(q);
    }
    out
}

/// The colour an import's fill or hatch is drawn in, of its own colour `own`: lighter when its layer's settings in the view's
/// Imported Categories (`over`) are halftone. As in Revit, an import's Patterns can't be overridden, and its Lines' colour is
/// its lines' and text's only.
fn import_fill(over: Option<&crate::graphics::Override>, own: Rgb) -> Rgb {
    if over.is_some_and(|o| o.halftone) { halftone(own) } else { own }
}

/// A colour halfway to the paper, as halftone draws it.
fn halftone(c: Rgb) -> Rgb {
    Rgb(c.0 + (255 - c.0) / 2, c.1 + (255 - c.1) / 2, c.2 + (255 - c.2) / 2)
}

/// How near two points of a drawing's run may come on screen before the second is left out, px.
const ONE_PIXEL: f32 = 0.75;
/// The most points of a fill's ring drawn as they are, never thinned.
const FEW: usize = 16;

/// A CAD drawing placed by `ci` (in the view's terms), of the layers it and `in_view` show, what lies in the view, as `look` (the
/// view's Imported Categories) draws it: each entity in its colour (the view's ink where it has none, or the import draws in
/// black), its pen and its linetype, and its lines and text in what its layer's override (else its import's) gives instead,
/// lighter in halftone; a solid hatch filled (by odd parity: its islands are holes), a patterned one drawn with its pattern
/// ([`hatch`]) or, with none, as a pale fill of its colour; text lettered at its height and width, turned, aligned and
/// mirrored as the drawing has it. Only the entities in the view are looked at, and a run's points under a pixel apart are
/// drawn as one. On a screen, a dense drawing zoomed out is drawn from its level of detail in the page's order: pictures of its
/// small entities' cells with its big fills drawn into them ([`cad_cells`]), and its other big entities one by one between
/// them. An import's, and a PDF's lines where its image shows them.
fn cad_drawing(
    d: &mut Drawing,
    cad: &crate::cad::Cad,
    ci: &crate::model::CadImport,
    in_view: Option<&std::collections::BTreeSet<String>>,
    look: Option<&crate::graphics::ImportLook>,
) {
    use crate::cad::lod::Step;
    if look.is_some_and(|l| l.all.hidden) {
        return;
    }
    // The view in the drawing's units, to leave out what lies outside it.
    let (w, h) = (d.view.width as f32, d.view.height as f32);
    let view = [d.view.mm((0.0, 0.0)), d.view.mm((w, 0.0)), d.view.mm((w, h)), d.view.mm((0.0, h))];
    let Some([lo, hi]) = crate::dxf::union(view.map(|q| {
        let p = crate::model::cad_unplace(ci, q);
        [p, p]
    })) else {
        return;
    };
    // What the frame may draw of its hatches' patterns, and the smallest a pattern's cell may be on it before a hatch is its
    // tint.
    let (budget, least) = if d.screen { (hatch::SCREEN_POINTS, hatch::SCREEN_PX) } else { (hatch::PAPER_POINTS, hatch::PAPER_PX) };
    let mut ents = CadEntities { cad, ci, in_view, look, lo, hi, overrides: std::collections::HashMap::new(), budget, least };
    // Pixels to a unit of the drawing along whichever way it is the most: a cell of the band drawn is no more than a pixel.
    let most = ci.scale.abs().max((ci.scale * ci.stretch).abs()) * d.view.scale;
    if let Some((lod, band)) = cad.lod_band(most).filter(|_| d.screen)
        && let Some(picture) = cad_picture(ci, (in_view, look), lod, band)
        && cad_fills_fit(d, ci, lod, band, [lo, hi])
    {
        let meets = |i: usize| lod.entity_box(i).is_some_and(|b| b[0] <= hi.0 && b[2] >= lo.0 && b[1] <= hi.1 && b[3] >= lo.1);
        let cells_half = look.is_some_and(|l| {
            let shown = |name: &&String| !(ci.hidden.contains(*name) || in_view.is_some_and(|v| v.contains(*name)));
            lod.layers.iter().filter(shown).all(|name| l.of_layer(name).halftone)
        });
        let mut pictures = 0;
        for step in &band.steps {
            match step {
                Step::Picture(runs) => {
                    // A picture of cells is halftone where every layer it may draw is: on a dark screen, one of a drawing
                    // halftone in part keeps its halftone layers' cells halfway to the paper.
                    d.halftone_next(cells_half);
                    cad_cells(d, ci, (&picture, pictures, runs), band.cell, [lo, hi], &|i| ents.fill(i));
                    d.halftone_next(false);
                    pictures += 1;
                }
                Step::Whole(list) => {
                    for i in list.iter().map(|i| *i as usize).filter(|i| meets(*i)) {
                        ents.draw(d, i);
                    }
                }
            }
        }
        return;
    }
    for i in cad.entities_in(lo, hi) {
        ents.draw(d, i);
    }
}

/// A CAD drawing's entities being drawn ([`cad_drawing`]): the drawing, its placement, the layers the view hides and how it
/// draws them, the view's box in the drawing's units, each layer's override worked out once, and what is left of the frame's
/// budget for its hatches' patterns and the smallest a cell of one may be.
struct CadEntities<'a> {
    cad: &'a crate::cad::Cad,
    ci: &'a crate::model::CadImport,
    in_view: Option<&'a std::collections::BTreeSet<String>>,
    look: Option<&'a crate::graphics::ImportLook>,
    lo: P2,
    hi: P2,
    overrides: std::collections::HashMap<String, crate::graphics::Override>,
    budget: usize,
    least: f64,
}

impl CadEntities<'_> {
    /// Whether entity `e` is shown: its layer isn't hidden by the import or the view.
    fn shows(&self, e: &crate::dxf::Entity) -> bool {
        self.cad.shows(e, &self.ci.hidden) && !self.in_view.is_some_and(|l| l.contains(&e.layer))
    }

    /// The override entity `e`'s layer is drawn with, if the view sets how its import draws.
    fn over(&mut self, e: &crate::dxf::Entity) -> Option<crate::graphics::Override> {
        let look = self.look?;
        if let Some(o) = self.overrides.get(&e.layer) {
            return Some(o.clone());
        }
        let o = look.of_layer(&e.layer);
        self.overrides.insert(e.layer.clone(), o.clone());
        Some(o)
    }

    /// Entity `i` if it is a fill shown: its rings, its colour as drawn (its own, halftone or not) and its opacity.
    fn fill(&self, i: usize) -> Option<CadFill<'_>> {
        let e = self.cad.dxf().entities.get(i)?;
        let crate::dxf::Shape::Fill { rings, .. } = &e.shape else { return None };
        if !self.shows(e) {
            return None;
        }
        let half = self.look.is_some_and(|l| l.of_layer(&e.layer).halftone);
        let own = self.cad.color(e, self.ci.colors).unwrap_or(INK);
        Some((rings, if half { halftone(own) } else { own }, e.alpha))
    }

    /// Draws entity `i`, if it is shown: all of it halftone, or none ([`Drawing::halftone`]).
    fn draw(&mut self, d: &mut Drawing, i: usize) {
        let Some(ent) = self.cad.dxf().entities.get(i) else { return };
        let half = self.over(ent).is_some_and(|o| o.halftone);
        d.halftone_next(half);
        self.draw_one(d, i);
        d.halftone_next(false);
    }

    /// [`CadEntities::draw`], halftone or not as it sets.
    fn draw_one(&mut self, d: &mut Drawing, i: usize) {
        use crate::dxf::{Horizontal, Shape, Vertical};
        let (cad, ci) = (self.cad, self.ci);
        let Some(ent) = cad.dxf().entities.get(i) else { return };
        if !self.shows(ent) {
            return;
        }
        let over = self.over(ent);
        let lines = over.as_ref().map(|o| &o.projection);
        let mut color = lines.and_then(|s| s.color).map_or_else(|| cad.color(ent, ci.colors).unwrap_or(INK), |[r, g, b]| Rgb(r, g, b));
        if over.as_ref().is_some_and(|o| o.halftone) {
            color = halftone(color);
        }
        let place = crate::model::cad_place(ci);
        let frame = d.view;
        // Pixels to each of the drawing's units.
        let k = ci.scale * frame.scale;
        let px_of = |p: &P2| frame.px(place(*p));
        match &ent.shape {
            Shape::Text { at, height, angle, text, align, wide: factor, mirrored, .. } => {
                let px = (height * k) as f32;
                if px < 1.0 {
                    return;
                }
                // Its width as drawn, and the way its letters run: backwards when it is mirrored.
                let wide = f64::from(font::width(text, px)) / k * factor;
                let run = if *mirrored { -1.0 } else { 1.0 };
                let (u, v) =
                    (P2(angle.to_radians().cos(), angle.to_radians().sin()), P2(-angle.to_radians().sin(), angle.to_radians().cos()));
                let left = match align.h {
                    Horizontal::Left => *at,
                    Horizontal::Center => *at - u * (run * wide / 2.0),
                    Horizontal::Right => *at - u * (run * wide),
                };
                let base = match align.v {
                    Vertical::Baseline | Vertical::Bottom => left,
                    Vertical::Middle => left - v * (height / 2.0),
                    Vertical::Top => left - v * *height,
                };
                // Its angle on screen: the drawing's turned by the import's, and mirrored when the import is.
                let turned = if ci.stretch < 0.0 { -angle } else { *angle };
                let middle = frame.px(place(base + u * (run * wide / 2.0) + v * (height / 2.0)));
                let from = d.prims.len();
                d.text_shaped(text, middle, px, -((turned + ci.angle).to_radians()) as f32, (*factor as f32, *mirrored), color);
                d.see_through(from, ent.alpha);
            }
            Shape::Point(_) => {}
            // A hatch's boundaries fill by odd parity, as DXF's do: an island in it is a hole.
            Shape::Fill { rings, solid, tile, shade } => {
                // In its own colour, halftone or not.
                let own = import_fill(over.as_ref(), cad.color(ent, ci.colors).unwrap_or(INK));
                // Drawn with its pattern, its cell laid again cut to it; where the cell is too small to see, its tint.
                if let Some(t) = tile {
                    let px = |p: P2| frame.px(place(p));
                    let color = |c: [u8; 3]| {
                        let c = match ci.colors {
                            crate::cad::Colors::Preserve => Rgb(c[0], c[1], c[2]),
                            crate::cad::Colors::Invert => Rgb(255 - c[0], 255 - c[1], 255 - c[2]),
                            crate::cad::Colors::Black => INK,
                        };
                        import_fill(over.as_ref(), c)
                    };
                    let look = hatch::Look { px: &px, color: &color, k };
                    let view = [self.lo.0, self.lo.1, self.hi.0, self.hi.1];
                    if hatch::draw(d, t, rings, view, &look, self.least, &mut self.budget) {
                        return;
                    }
                }
                let fill = if *solid || tile.is_some() { own } else { Rgb(pale(own.0), pale(own.1), pale(own.2)) };
                // A ring of a few points is drawn as it is: thinned, a band of a gradient narrower than a pixel would be a dot.
                let px: Vec<Vec<(f32, f32)>> = rings
                    .iter()
                    .map(|r| if r.len() <= FEW { r.iter().map(px_of).collect() } else { thinned(r.iter().map(px_of)) })
                    .collect();
                if px.iter().all(|r| r.len() < 3) {
                    // Under a pixel: a dot of its colour, so a dense hatch still reads as one.
                    if let Some(&q) = px.iter().flatten().next() {
                        d.stroke_seen(vec![q, (q.0 + ONE_PIXEL, q.1)], Pen::FINE, fill, None, false, ent.alpha);
                    }
                    return;
                }
                let rings = px.into_iter().filter(|r| r.len() >= 3).collect();
                // A gradient's band keeps its colours on a dark canvas, as a picture does.
                let part = if *shade { std::mem::replace(&mut d.part, Part::Shade) } else { d.part };
                d.push(Prim::Fill { rings, color: fill, rule: Rule::EvenOdd, alpha: ent.alpha });
                d.part = part;
            }
            shape => {
                let pen = lines.and_then(|s| s.weight).unwrap_or_else(|| cad.pen(ent));
                // Its dashes in pixels (the override's pattern, else its linetype's); a pattern whose cycle is under a few
                // pixels draws solid, as it would look.
                let dash = match lines.and_then(|s| s.dash) {
                    Some(k) => k.pattern(),
                    None => cad
                        .dash(ent)
                        .map(|d| d.into_iter().map(|x| (x * k) as f32).collect::<Vec<f32>>())
                        .filter(|d| d.iter().sum::<f32>() >= 3.0 && d.iter().skip(1).step_by(2).any(|gap| *gap >= 0.5)),
                };
                for (pts, closed) in crate::cad::paths(shape) {
                    let mut px = thinned(pts.iter().map(px_of));
                    if px.len() == 1
                        && let Some(&q) = px.first()
                    {
                        px.push((q.0 + ONE_PIXEL, q.1));
                    }
                    let closed = closed && px.len() > 2;
                    d.stroke_seen(px, pen, color, dash.clone(), closed, ent.alpha);
                }
            }
        }
    }
}

// The images placed in view `key` (a plan, with a dependent view's parent's, a section, an elevation or a sheet) on `layer`
/// that `look` shows (all on a sheet, which has none), each drawn over its corners in the view's terms.
pub(crate) fn images(d: &mut Drawing, m: &Model, key: Guid, layer: crate::raster::DrawLayer, look: Option<&Look>) {
    // Back to front, as Arrange orders them.
    let mut ids: Vec<(i32, Id)> = m
        .rare("image")
        .into_iter()
        .filter_map(|id| match m.elements.get(id) {
            Some(Element::Image(im)) => Some((im.order, id)),
            _ => None,
        })
        .collect();
    ids.sort_unstable();
    for (_, id) in ids {
        let Some(e @ Element::Image(im)) = m.elements.get(id) else { continue };
        if im.layer != layer || !(im.view == key || m.annotation_in(e, key)) || look.is_some_and(|l| !l.shows(id)) {
            continue;
        }
        let Some(t) = m.image_types.get(&im.type_name) else { continue };
        let corners = im.corners().map(|q| {
            let (x, y) = d.view.px(q);
            (f64::from(x), f64::from(y))
        });
        d.own(Some(id));
        let part = std::mem::replace(&mut d.part, Part::Picture);
        // Its picture over the part of it the picture covers (all of it, but for a page drawn from its lines): none, nothing.
        if let Some([u0, v0, u1, v1]) = t.covers() {
            let [tl, tr, _, bl] = corners;
            let at = |u: f64, v: f64| (tl.0 + (tr.0 - tl.0) * u + (bl.0 - tl.0) * v, tl.1 + (tr.1 - tl.1) * u + (bl.1 - tl.1) * v);
            crate::raster::draw(d, t, [at(u0, v0), at(u1, v0), at(u1, v1), at(u0, v1)]);
        }
        // A page drawn from its lines: they go over its picture, of the layers it and the view show.
        if t.drawn()
            && look.is_none_or(|l| l.draws_pdf_lines(&im.type_name))
            && let Some((cad, ci)) = im.lines_placed(t, im.view)
        {
            cad_drawing(
                d,
                &cad,
                &ci,
                look.and_then(|l| l.hidden_pdf_layers(&im.type_name)),
                look.and_then(|l| l.pdf_import_look(&im.type_name)),
            );
        }
        d.part = part;
    }
    d.own(None);
}

/// Clears the pixels of the background images in `d` that lie where `picture` (a section's, over the same view) shows the
/// model: behind it, as Revit draws an image on the background layer.
fn behind_model(d: &mut Drawing, m: &Model, picture: &Image) {
    let view = d.view;
    let (rx, ry) = (f64::from(picture.width) / f64::from(view.width.max(1)), f64::from(picture.height) / f64::from(view.height.max(1)));
    for ((prim, owner), part) in d.prims.iter_mut().zip(&d.owners).zip(&d.parts) {
        let back = owner.and_then(|id| match m.elements.get(id) {
            Some(Element::Image(im)) => Some(im.layer == crate::raster::DrawLayer::Background),
            _ => None,
        });
        if *part != Part::Picture || back != Some(true) {
            continue;
        }
        if let Prim::Image { at, width, rgba, .. } = prim {
            let (w, ax, ay) = (*width as usize, f64::from(at.0), f64::from(at.1));
            for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x, y) = (ax + (i % w) as f64 + 0.5, ay + (i / w) as f64 + 0.5);
                if picture.covered(x * rx, y * ry) {
                    px[3] = 0;
                }
            }
        }
    }
}

/// The picture of `band` (of a drawing placed by `ci`, its level of detail `lod`) for the view's look of it: its layers'
/// marks in their colours as the view draws them (the import's colours, its ink inverted or black as the import says; a line's
/// in its layer's override's colour; halftone, a fill's as a line's), none for a layer it hides. Made once for that look
/// ([`crate::cad::lod::Lod::picture`]); none when it would hold too much.
fn cad_picture(
    ci: &crate::model::CadImport,
    (in_view, look): (Option<&std::collections::BTreeSet<String>>, Option<&crate::graphics::ImportLook>),
    lod: &crate::cad::lod::Lod,
    band: &crate::cad::lod::Band,
) -> Option<std::sync::Arc<crate::cad::lod::Picture>> {
    use crate::cad::Colors;
    use std::hash::{Hash, Hasher};
    // Each layer as the view draws it: hidden, or its override's colour and halftone.
    let seen: Vec<Option<(Option<Rgb>, bool)>> = lod
        .layers
        .iter()
        .map(|name| {
            if ci.hidden.contains(name) || in_view.is_some_and(|l| l.contains(name)) {
                return None;
            }
            let over = look.map(|l| l.of_layer(name));
            let color = over.as_ref().and_then(|o| o.projection.color).map(|[r, g, b]| Rgb(r, g, b));
            Some((color, over.as_ref().is_some_and(|o| o.halftone)))
        })
        .collect();
    let mut key = std::collections::hash_map::DefaultHasher::new();
    ci.colors.name().hash(&mut key);
    for s in &seen {
        s.map(|(c, half)| (c.map(|c| (c.0, c.1, c.2)), half)).hash(&mut key);
    }
    let color = |l: usize, own: Option<Rgb>, fill: bool| -> Option<Rgb> {
        let (over, half) = (*seen.get(l)?)?;
        let own = match (ci.colors, own) {
            (Colors::Preserve, Some(c)) => c,
            (Colors::Invert, Some(c)) => Rgb(255 - c.0, 255 - c.1, 255 - c.2),
            _ => INK,
        };
        // As drawn whole: a layer's Lines' colour is its lines' and text's, never its fills'.
        let c = if fill { own } else { over.unwrap_or(own) };
        Some(if half { halftone(c) } else { c })
    };
    lod.picture(band, key.finish(), &color)
}

/// The most pixels a frame's big fills drawn into the pictures of a drawing's level of detail may cover among them: past it,
/// the drawing is drawn entity by entity.
const MOST_FILLED: f64 = 32e6;

/// Whether the big fills `band`'s pictures draw (of a drawing placed by `ci`) cover few enough of the frame's pixels, the
/// part of each in the box from `lo` to `hi` (the view, in the drawing's units), to draw them into the pictures.
fn cad_fills_fit(
    d: &Drawing,
    ci: &crate::model::CadImport,
    lod: &crate::cad::lod::Lod,
    band: &crate::cad::lod::Band,
    [lo, hi]: [P2; 2],
) -> bool {
    let k = (ci.scale.abs() * d.view.scale) * ((ci.scale * ci.stretch).abs() * d.view.scale);
    let mut covered = 0.0;
    for step in &band.steps {
        let crate::cad::lod::Step::Picture(runs) = step else { continue };
        for run in runs {
            let crate::cad::lod::Run::Fill(i) = run else { continue };
            let Some(b) = lod.entity_box(*i as usize) else { continue };
            let (w, h) = ((b[2].min(hi.0) - b[0].max(lo.0)).max(0.0), (b[3].min(hi.1) - b[1].max(lo.1)).max(0.0));
            covered += w * h * k;
        }
    }
    covered <= MOST_FILLED
}

/// A big fill a picture of cells draws: its rings, its colour as drawn and its opacity.
type CadFill<'c> = (&'c [Vec<P2>], Rgb, u8);

/// Picture `p` of a drawing's level of detail (placed by `ci`), its runs `runs`, the part of it in the box from `lo` to `hi`
/// in the drawing's units, drawn as one image ([`Part::Cells`]) in the page's order: each run of cells the pixels their ink
/// makes, a big fill (`fill` gives its rings, colour and opacity; none, it is hidden) filled where it covers. A cell's ink
/// (its lines as long as they are and a pixel wide, as the finest pen draws on a screen, their round ends; its fills as much
/// of it as they cover) is spread over the four pixels round its middle: a pixel is as opaque as its fills cover it, and its
/// lines over them as opaque as their ink, laid at random, covers it.
fn cad_cells<'c>(
    d: &mut Drawing,
    ci: &crate::model::CadImport,
    (picture, p, runs): (&crate::cad::lod::Picture, usize, &[crate::cad::lod::Run]),
    cell: f64,
    [lo, hi]: [P2; 2],
    fill: &dyn Fn(usize) -> Option<CadFill<'c>>,
) {
    let place = crate::model::cad_place(ci);
    let frame = d.view;
    // The pixels the picture may reach: its grid in the view, a pixel round.
    let [glo, ghi] = picture.bounds();
    let (from, to) = (P2(lo.0.max(glo.0), lo.1.max(glo.1)), P2(hi.0.min(ghi.0), hi.1.min(ghi.1)));
    if !(from.0 <= to.0 && from.1 <= to.1) {
        return;
    }
    let corners = [from, P2(to.0, from.1), to, P2(from.0, to.1)].map(|q| frame.px(place(q)));
    let (w, h) = (frame.width as f32, frame.height as f32);
    let (xs, ys) = (corners.map(|c| c.0), corners.map(|c| c.1));
    let x0 = (xs.iter().copied().fold(f32::INFINITY, f32::min) - 1.0).floor().clamp(0.0, w);
    let y0 = (ys.iter().copied().fold(f32::INFINITY, f32::min) - 1.0).floor().clamp(0.0, h);
    let x1 = (xs.iter().copied().fold(f32::NEG_INFINITY, f32::max) + 1.0).ceil().clamp(0.0, w);
    let y1 = (ys.iter().copied().fold(f32::NEG_INFINITY, f32::max) + 1.0).ceil().clamp(0.0, h);
    let (pw, ph) = ((x1 - x0) as usize, (y1 - y0) as usize);
    if pw == 0 || ph == 0 {
        return;
    }
    // Pixels a cell is across (along a line, near enough for a stretched drawing): a cell's line ink is a pixel wide line
    // through it that long; its fill ink, the share of it covered, is that many pixels squared.
    let (kx, ky) = (ci.scale.abs() * frame.scale, (ci.scale * ci.stretch).abs() * frame.scale);
    let across = ((kx * ky).sqrt() * cell) as f32;
    // The picture, premultiplied; each run's lines' ink and fills' as it is summed, px², their colours times them; the pixels
    // a run touched; and the box round all it touched.
    let mut out = vec![[0.0_f32; 4]; pw * ph];
    let mut ink = vec![[0.0_f32; 8]; pw * ph];
    let mut touched: Vec<usize> = Vec::new();
    let mut seen = [usize::MAX, usize::MAX, 0, 0];
    let mark = |i: usize, seen: &mut [usize; 4]| {
        let (x, y) = (i % pw, i / pw);
        *seen = [seen[0].min(x), seen[1].min(y), seen[2].max(x + 1), seen[3].max(y + 1)];
    };
    for (r, run) in runs.iter().enumerate() {
        if let crate::cad::lod::Run::Fill(i) = run {
            let Some((rings, rgb, alpha)) = fill(*i as usize) else { continue };
            let px: Vec<Vec<(f32, f32)>> = rings
                .iter()
                .map(|ring| {
                    ring.iter()
                        .map(|q| {
                            let (x, y) = frame.px(place(*q));
                            (x - x0, y - y0)
                        })
                        .collect()
                })
                .collect();
            let c = [f32::from(rgb.0), f32::from(rgb.1), f32::from(rgb.2)];
            fill_into(&mut out, (pw, ph), &px, c, f32::from(alpha) / 255.0, &mut |i| mark(i, &mut seen));
            continue;
        }
        for (c, cell) in picture.run_in(p, r, lo, hi) {
            let all = crate::cad::lod::ink_of(cell.ink) as f32 * if cell.fill { across * across } else { across };
            let rgb = [f32::from(cell.rgb.0), f32::from(cell.rgb.1), f32::from(cell.rgb.2)];
            // Its middle's pixel, and the four round it by how near.
            let (px, py) = frame.px(place(c));
            let (fx, fy) = (px - x0 - 0.5, py - y0 - 0.5);
            let (ix, iy) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - ix, fy - iy);
            let o = if cell.fill { 4 } else { 0 };
            for (dx, dy, share) in [(0, 0, (1.0 - tx) * (1.0 - ty)), (1, 0, tx * (1.0 - ty)), (0, 1, (1.0 - tx) * ty), (1, 1, tx * ty)] {
                let (x, y) = (ix as i64 + dx, iy as i64 + dy);
                if x < 0 || y < 0 || x >= pw as i64 || y >= ph as i64 {
                    continue;
                }
                let at = y as usize * pw + x as usize;
                if let Some(q) = ink.get_mut(at) {
                    if q.iter().all(|v| *v == 0.0) {
                        touched.push(at);
                    }
                    let a = all * share;
                    for (k, v) in [rgb[0] * a, rgb[1] * a, rgb[2] * a, a].into_iter().enumerate() {
                        if let Some(s) = q.get_mut(o + k) {
                            *s += v;
                        }
                    }
                }
            }
        }
        // The run over what's under it: each pixel its fills, as opaque as they cover it, and its lines over them, as opaque as
        // strokes laid one over another at random cover it (1 - e^-ink).
        for at in touched.drain(..) {
            let (Some(q), Some(o)) = (ink.get_mut(at), out.get_mut(at)) else { continue };
            let (l, f) = (q[3], q[7]);
            let al = if l > 0.0 && l.is_finite() { 1.0 - (-l).exp() } else { 0.0 };
            let af = if f > 0.0 && f.is_finite() { f.min(1.0) } else { 0.0 };
            let src_a = al + af * (1.0 - al);
            if src_a > 0.0 {
                let ch = |k: usize| {
                    let line = if l > 0.0 { q[k] / l * al } else { 0.0 };
                    let under = if f > 0.0 { q[4 + k] / f * af * (1.0 - al) } else { 0.0 };
                    line + under
                };
                let src = [ch(0), ch(1), ch(2), src_a];
                for (k, v) in src.into_iter().enumerate() {
                    o[k] = v + o[k] * (1.0 - src_a);
                }
                mark(at, &mut seen);
            }
            *q = [0.0; 8];
        }
    }
    // What it touched, as one image.
    let [sx0, sy0, sx1, sy1] = seen;
    if sx0 >= sx1 || sy0 >= sy1 {
        return;
    }
    let (iw, ih) = (sx1 - sx0, sy1 - sy0);
    let mut rgba = vec![0_u8; iw * ih * 4];
    for y in 0..ih {
        for x in 0..iw {
            let (Some(o), Some(px)) = (out.get((sy0 + y) * pw + sx0 + x), rgba.get_mut((y * iw + x) * 4..(y * iw + x) * 4 + 4)) else {
                continue;
            };
            if o[3] > 0.0 {
                let c = |v: f32| (v / o[3]).round().clamp(0.0, 255.0) as u8;
                px.copy_from_slice(&[c(o[0]), c(o[1]), c(o[2]), (o[3] * 255.0).round().clamp(0.0, 255.0) as u8]);
            }
        }
    }
    let part = std::mem::replace(&mut d.part, Part::Cells);
    d.push(Prim::Image { at: (x0 + sx0 as f32, y0 + sy0 as f32), width: iw as u32, height: ih as u32, rgba, ink: false });
    d.part = part;
}

/// Fills rings `rings` (pixels of a picture `pw` by `ph`, by odd parity) into `out` (premultiplied) in colour `c` at opacity
/// `alpha`, each pixel by how much of it they cover (four rows of samples a pixel, each across exactly), telling `seen` each
/// pixel it covers.
fn fill_into(
    out: &mut [[f32; 4]],
    (pw, ph): (usize, usize),
    rings: &[Vec<(f32, f32)>],
    c: [f32; 3],
    alpha: f32,
    seen: &mut dyn FnMut(usize),
) {
    // Each edge that isn't level: its top, its bottom, and its x at the top and its slope, top down.
    let mut edges: Vec<(f32, f32, f32, f32)> = Vec::new();
    for r in rings.iter().filter(|r| r.len() >= 3) {
        for (i, a) in r.iter().enumerate() {
            let Some(b) = r.get((i + 1) % r.len()) else { continue };
            if !(a.0.is_finite() && a.1.is_finite() && b.0.is_finite() && b.1.is_finite()) || a.1 == b.1 {
                continue;
            }
            let (t, z) = if a.1 < b.1 { (a, b) } else { (b, a) };
            edges.push((t.1, z.1, t.0, (z.0 - t.0) / (z.1 - t.1)));
        }
    }
    if edges.is_empty() {
        return;
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let top = edges.iter().map(|e| e.0).fold(f32::INFINITY, f32::min).floor().max(0.0) as usize;
    let bottom = (edges.iter().map(|e| e.1).fold(f32::NEG_INFINITY, f32::max).ceil().max(0.0) as usize).min(ph);
    let mut cover = vec![0.0_f32; pw];
    let (mut active, mut next): (Vec<usize>, usize) = (Vec::new(), 0);
    let mut xs: Vec<f32> = Vec::new();
    const SUB: usize = 4;
    for y in top..bottom {
        let (mut lo, mut hi) = (pw, 0);
        for s in 0..SUB {
            let sy = y as f32 + (s as f32 + 0.5) / SUB as f32;
            while let Some(e) = edges.get(next).filter(|e| e.0 <= sy) {
                let _ = e;
                active.push(next);
                next += 1;
            }
            active.retain(|i| edges.get(*i).is_some_and(|e| e.1 > sy));
            xs.clear();
            xs.extend(active.iter().filter_map(|i| edges.get(*i)).filter(|e| e.0 <= sy).map(|e| e.2 + (sy - e.0) * e.3));
            xs.sort_by(f32::total_cmp);
            for pair in xs.as_chunks::<2>().0 {
                let (a, b) = (pair[0].clamp(0.0, pw as f32), pair[1].clamp(0.0, pw as f32));
                if b <= a {
                    continue;
                }
                let (ia, ib) = (a.floor() as usize, (b.floor() as usize).min(pw));
                let share = 1.0 / SUB as f32;
                lo = lo.min(ia);
                hi = hi.max((ib + 1).min(pw));
                if ia == ib {
                    if let Some(v) = cover.get_mut(ia) {
                        *v += (b - a) * share;
                    }
                    continue;
                }
                if let Some(v) = cover.get_mut(ia) {
                    *v += (ia as f32 + 1.0 - a) * share;
                }
                for v in cover.get_mut(ia + 1..ib).unwrap_or_default() {
                    *v += share;
                }
                if let Some(v) = cover.get_mut(ib) {
                    *v += (b - ib as f32) * share;
                }
            }
        }
        for x in lo..hi {
            let Some(v) = cover.get_mut(x) else { continue };
            let a = v.min(1.0) * alpha;
            *v = 0.0;
            if a <= 0.0 {
                continue;
            }
            let at = y * pw + x;
            if let Some(o) = out.get_mut(at) {
                *o = [c[0] * a + o[0] * (1.0 - a), c[1] * a + o[1] * (1.0 - a), c[2] * a + o[2] * (1.0 - a), a + o[3] * (1.0 - a)];
                seen(at);
            }
        }
    }
}

// A channel of a colour most of the way to the paper: a patterned hatch's fill.
fn pale(v: u8) -> u8 {
    v + (255 - v) / 4 * 3
}

/// A floor plan of `plan`, or `up`, its reflected ceiling plan; `titled`, with its name lettered at its bottom left (an image of
/// it has it; a window letters it itself over the canvas, and shows it in the view's tab, and a sheet has it under its viewport). Its annotation `k` times the size it
/// is laid out at, its pens the line weights' at 1 : `n`.
/// What a plan is drawn for: a picture of it (titled with its name), a screen (where a dense drawing zoomed out is drawn
/// simplified), or paper (a sheet's viewport).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Out {
    Picture,
    Screen,
    Paper,
}

fn draw_plan(m: &Model, plan: impl Key, view: View, up: bool, k: f32, n: f64, out: Out) -> Drawing {
    let key = m.guid(plan);
    let d = draw_plan_among(m, key, view, up, k, n, out, true);
    // Its twin, which tests hold it to: every element looked at, the walls joined through an index of every wall.
    if crate::selfcheck::on(crate::selfcheck::Check::DrawPlan) {
        let every = regions::uncounted(|| draw_plan_among(m, key, view, up, k, n, out, false));
        let same = d.prims.len() == every.prims.len()
            && d.owners == every.owners
            && d.parts == every.parts
            && d.prims.iter().zip(&every.prims).all(|(a, b)| a.points() == b.points());
        assert!(same, "a plan drawn among the elements that may show in it isn't the plan drawn among every element");
    }
    d
}

/// [`draw_plan`] of plan `key`: with `among`, its passes look only at the elements that may show in it
/// ([`plan_elements`]), its walls joined through the model's lookup when it is made; else at every element, through an
/// index of every wall.
#[allow(clippy::too_many_arguments)]
fn draw_plan_among(m: &Model, key: Guid, view: View, up: bool, k: f32, n: f64, out: Out, among: bool) -> Drawing {
    let titled = out == Out::Picture;
    // The plan's own guid says what it is: its graphics, range, crop and annotation; its level is what it shows of the model.
    let level = m.plan_level(key).map_or(key, |l| m.guid(l));
    let index = match m.derived.made(&m.elements) {
        Some(_) if among => None,
        _ => Some(walls::Index::new(m)),
    };
    let index = index.as_ref();
    let mut d = Drawing::new(view);
    (d.k, d.paper, d.detail, d.weights) = (k, paper_of(n, view.scale), detail_of(m, key), m.line_weights().at(n));
    d.screen = out == Out::Screen;
    let look = Look::of(m, key);
    let shown = |id: Id| look.shows(id);
    d.pattern_styles = look.pattern_styles();
    // A ceiling plan looks up from its own cut, whatever box its floor plan follows.
    let mut range = if up { m.plan_range(key) } else { m.shown_range(key) };
    if up {
        range.cut = m.elevation(level) + CEILING_CUT;
        range.top = range.top.max(range.cut);
    }
    let mut components = crate::component_geometry::Components::new(m);
    let elements: Vec<(Id, &Element)> =
        if among { plan_elements(m, key, level, &range, up, &mut components) } else { m.elements.iter().map(|(id, e)| (*id, e)).collect() };
    let all = || elements.iter().map(|(id, e)| (id, *e));
    // The scans it sees, under all the rest; then its CAD drawings.
    if !up {
        scans(&mut d, m, &range, &look);
        cad_imports(&mut d, m, key, level, &look);
    }
    // Its images on the background layer, behind the model.
    images(&mut d, m, key, crate::raster::DrawLayer::Background, Some(&look));
    // Floors whose tops it sees, from the cut plane down to the view depth: their outlines and their
    // openings', the holes shafts cut in them, and a sloped one's slope arrow. The ground first (toposolids, as their
    // building pads cut them), then the site's elements on it (`render/site.rs`), then the building's floors.
    for pass in 0..2 {
        if pass == 1 && !up {
            site::draw(&mut d, m, level, &elements, &shown);
        }
        for (&id, e) in all().filter(|_| !up).filter(|(id, _)| shown(**id)) {
            let cut;
            let f = match (pass, e) {
                (0, Element::Toposolid(t)) => {
                    cut = m.toposolid_cut(t);
                    &cut
                }
                (1, Element::Floor(f)) => f,
                _ => continue,
            };
            let top = m.floor_top(f);
            let (lo, hi) =
                f.boundary.iter().map(|q| top.at(*q)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
            if hi < range.depth || lo > range.cut {
                continue;
            }
            d.owner = Some(id);
            // Surface overrides also work on an unpatterned material and at coarse detail.
            let material = m.type_layers("floor", &f.type_name).first().and_then(|l| m.materials.get(&l.material));
            // In plan a shaped top is its outline: its patterns go over it whole, not plane by plane, or each plane's edge would
            // show as a seam through a fill (a terrain's thousands of them).
            let flat = (!f.shape.is_empty()).then(|| crate::model::Floor { shape: Default::default(), ..f.clone() });
            for (face, rise) in m.floor_slabs(flat.as_ref().unwrap_or(f)) {
                for piece in m.less_shafts(&face, rise).0 {
                    let replaced = override_patterns(&mut d, m, std::slice::from_ref(&piece), false, (f.pattern_origin, f.pattern_angle));
                    if !replaced
                        && d.detail == Detail::Fine
                        && let Some(x) = material
                    {
                        match x.surface_pattern.as_ref().and_then(|n| m.fill_patterns.get(n)) {
                            Some(p) => fill_pattern(
                                &mut d,
                                p,
                                std::slice::from_ref(&piece),
                                std::slice::from_ref(&piece),
                                x.surface_ink(FAINT),
                                (f.pattern_origin, f.pattern_angle),
                            ),
                            None => surface(&mut d, &piece, x.surface, x.surface_ink(FAINT)),
                        }
                    }
                }
            }
            for ring in std::iter::once(&f.boundary).chain(&f.openings) {
                d.line(ring, BY_CATEGORY, FAINT, true);
            }
            // A shaped one's creases: the edges of each of its planes, as thin as its outline; or, drawn as a toposolid is, its
            // contours.
            if f.contours > 0.0 {
                contours(&mut d, m, f);
            } else if !f.shape.is_empty() {
                for (loops, _) in m.shape_planes(f).unwrap_or_default() {
                    for ring in &loops {
                        d.line(ring, BY_CATEGORY, FAINT, true);
                    }
                }
            }
            holes(&mut d, m, &m.floor_slabs(f));
            if let Some([tail, head]) = f.arrow.filter(|_| f.slope != 0.0) {
                slope_arrow(&mut d, tail, head, f.slope);
            }
        }
    }
    // Components it sees, in projection, as Revit draws furniture: whole while any of one is under
    // the cut plane and over the view depth; and the symbols of those on its level.
    for (&id, e) in all().filter(|(id, _)| shown(**id)) {
        let Element::Component(c) = e else { continue };
        let pieces = components.pieces(c);
        d.owner = Some(id);
        if !up && !pieces.is_empty() {
            let (bottom, top) = components.heights(c);
            if bottom < range.cut && top > range.depth {
                component(&mut d, m, &pieces);
            }
        }
        // Its symbols are annotation of its level: in that level's plan and ceiling plan, whatever height its forms reach.
        if c.level == level && d.detail != Detail::Coarse {
            marks(&mut d, &m.component_marks(c));
        }
    }
    // Walls and columns below the cut plane, as seen from it; past the bottom, faint.
    for (id, o, top) in seen_below(m, &elements, &range, &look, index).into_iter().filter(|_| !up) {
        d.owner = Some(id);
        override_patterns(&mut d, m, std::slice::from_ref(&o), false, (P2(0.0, 0.0), 0.0));
        let ink = if top > range.bottom - TOL { INK } else { FAINT };
        d.line(&o, BY_CATEGORY, ink, true);
        // A door or window rising past a wall's top notches it: the notch's sides, seen from above.
        if matches!(m.elements.get(id), Some(Element::Wall(_))) {
            for side in walls::notches(m, id) {
                d.line(&side, BY_CATEGORY, ink, false);
            }
        }
    }
    // A run's centre line, faint and dashed, where it is seen but not cut: Revit's mark of a beam, and where to pick it.
    for (&id, e) in all().filter(|_| !up).filter(|(id, _)| shown(**id)) {
        let Some(g) = m.run_geom(e).filter(|g| !g.vertical()) else { continue };
        let (lo, hi) = g.z_span();
        let seen = range.depth - TOL < hi && hi <= range.cut;
        let over = range.cut < lo && lo <= range.top;
        if seen || over {
            d.owner = Some(id);
            let pts = [P2(g.a[0], g.a[1]), P2(g.b[0], g.b[1])].iter().map(|p| d.view.px(*p)).collect();
            d.stroke(pts, BY_CATEGORY, FAINT, Some(vec![14.0, 5.0]), false);
        }
    }
    // Looking up from its cut, a reflected ceiling plan sees the beams and columns over it, up to the ceilings: their outlines,
    // in projection. One over a ceiling it lies above is hidden by that ceiling.
    if up {
        let ceilings: Vec<(f64, &Ceiling)> = all()
            .filter(|(id, _)| shown(**id))
            .filter_map(|(_, e)| match e {
                Element::Ceiling(c) => Some((m.elevation(c.level) + c.offset, c)),
                _ => None,
            })
            .filter(|(z, _)| range.cut < *z && *z <= range.top)
            .collect();
        for (&id, e) in all().filter(|(id, _)| shown(**id)) {
            if !matches!(e, Element::Beam(_)) && e.column().is_none() {
                continue;
            }
            let Some((outline, bottom, _)) = stands(m, id, e, index) else { continue };
            if !(range.cut < bottom && bottom <= range.top) || outline.is_empty() {
                continue;
            }
            let mid = outline.iter().fold(P2(0.0, 0.0), |s, p| s + *p) * (1.0 / outline.len() as f64);
            let hidden = ceilings
                .iter()
                .any(|(z, c)| *z <= bottom + TOL && geom::contains(&c.boundary, mid) && !c.openings.iter().any(|o| geom::contains(o, mid)));
            if !hidden {
                d.owner = Some(id);
                d.line(&outline, BY_CATEGORY, INK, true);
            }
        }
    }
    d.part(Part::Cut);
    let outlines = cut_outlines(m, &elements, range.cut, &look, index);
    // Poché first, then the outlines over it, but for where walls meet at their joins: only the edge
    // of their union is inked (`cut_edges`). The union is filled first, as one for each colour the
    // view fills them in, so no seam shows where two fills meet.
    // What a link builds is filled apart, for [`Drawing::linked_under`] to put under the project's.
    let (under, own): (Vec<_>, Vec<_>) = outlines.iter().partition(|(id, _)| crate::link::is_linked(*id));
    for (group, owner) in [(under, Some(crate::link::UNION)), (own, None)] {
        d.owner = owner;
        for ((color, half), rings) in cut_unions(&look, group.into_iter().map(|(id, o)| (*id, o.clone()))) {
            d.halftone_next(half);
            d.fill(&rings, color);
        }
        d.halftone_next(false);
    }
    // One fill per wall or column: in one even-odd fill, overlapping walls would cancel out.
    for (id, o) in &outlines {
        d.owner = Some(*id);
        let rings = std::slice::from_ref(o);
        if pattern_style(&d, true).is_some() {
            d.fill(rings, PAPER);
            override_patterns(&mut d, m, rings, true, (P2(0.0, 0.0), 0.0));
        } else {
            d.fill(rings, POCHE);
        }
    }
    for (id, runs) in cut_edges(&outlines, 1.0 / d.view.scale) {
        d.owner = Some(id);
        for run in runs {
            ink_cut(&mut d, &run);
        }
    }
    // A compound wall's layers: each its material's cut pattern where it's wide enough to hold it,
    // else lighter where it isn't the wall's structure; a thin line between each two. A column the plan cuts stands in the wall's
    // way: its poche is solid, so the layers stop at its outline.
    let columns: Vec<&[P2]> =
        outlines.iter().filter(|(id, _)| m.elements.get(*id).is_some_and(|e| e.column().is_some())).map(|(_, o)| o.as_slice()).collect();
    for (&id, e) in all() {
        let Element::Wall(w) = e else { continue };
        let layers = m.type_layers("wall", &w.type_name);
        if layers.is_empty() || !m.spans(w, range.cut) || !shown(id) || d.detail == Detail::Coarse {
            continue;
        }
        d.owner = Some(id);
        let geometry = walls::layer_geometry(m, id, index);
        let along = (w.end - w.start).unit();
        for (k, layer) in layers.iter().enumerate() {
            let ring = geometry.band(k);
            let room = layer.thickness * d.view.scale;
            for piece in outside_columns(vec![ring], &columns) {
                cut_layer(&mut d, m, layer, &piece, along, room);
            }
        }
        // The lines between the layers are detail, as the hatching is: a cut's weight is its outline's.
        d.part(Part::Pattern);
        for line in geometry.interfaces() {
            for part in lines_outside_columns(line, &columns) {
                d.line(&part, Pen::PROJECTION, INK, false);
            }
        }
        d.part(Part::Cut);
    }
    for (&id, e) in all() {
        // The opening is the wall's: with neither the wall nor the opening shown, there is nothing to leave a gap in.
        if let Element::Door(o) | Element::Window(o) = e
            && let Some(w) = m.wall(o.host)
            && m.spans(w, range.cut)
            && (shown(id) || m.elements.id(o.host).is_some_and(shown))
        {
            d.owner = Some(id);
            opening(&mut d, m, e, o, w, range.cut, shown(id));
        }
    }
    d.part(Part::Projection);
    // A ceiling plan isn't cropped: the crop region is the floor plan's.
    let crop = m.shown_crop(key).filter(|_| !up);
    // Cropped, annotation shows whole while its anchor is inside the crop, and not at all outside it.
    let inside = |p: P2| crop.is_none_or(|[lo, hi]| (lo.0..=hi.0).contains(&p.0) && (lo.1..=hi.1).contains(&p.1));
    // Every room of the level is found among the same lines, which take each wall's joins and heights to find: once.
    let rooms = crate::rooms::Rooms::new(m);
    // Rooms > Reference: off unless the view shows it.
    let reference =
        !crate::graphics::graphics_of(m, key).map_or_else(Default::default, |g| g.resolved(m).into_owned()).hides("roomreference");
    // Bars seen from above that lie where another was drawn (a column's stirrups, up its height) are drawn once.
    let mut bars_seen: std::collections::HashSet<Vec<(i64, i64)>> = std::collections::HashSet::new();
    for (&id, e) in all().filter(|(id, _)| shown(**id)) {
        d.owner = Some(id);
        match e {
            // Cropped, a grid stops at the crop's edge, with its bubbles there.
            Element::Grid(g) => {
                if let Some((a, b)) = crop.map_or(Some((g.start, g.end)), |[lo, hi]| clip(g.start, g.end, lo, hi)) {
                    grid(&mut d, a, b, &g.name);
                }
            }
            Element::Room(r) if r.level == level && inside(r.point) && !up => {
                // A frozen room has its boundary: the level's lines are found only for those that don't.
                let (region, redundant) = match &r.kept {
                    Some(k) => (k.at(m.room_area).cloned(), false),
                    // Two rooms can't share a space: the one placed later is left unbounded, Revit's redundant room.
                    None => match rooms.loop_at(r.level, m.room_area, r.point) {
                        Some(g) if rooms.before(r, &g).is_some() => (None, true),
                        g => (g, false),
                    },
                };
                // Its colour, a fill of its region under its name.
                if let (Some([red, green, blue]), Some(g)) = (r.colour, &region) {
                    let rings: Vec<Vec<P2>> = std::iter::once(g.outer.clone()).chain(g.holes.iter().cloned()).collect();
                    d.fill(&rings, Rgb(red, green, blue));
                }
                let area = region.as_ref().map_or_else(
                    || m.language.word(if redundant { msg!("REDUNDANT") } else { msg!("NOT ENCLOSED") }).to_string(),
                    |g| {
                        let (area, unit) = m.units.area(g.area());
                        format!("{area:.2} {unit}")
                    },
                );
                // Its reference, as Revit's: a cross from the point it was placed at, out along the diagonals to its boundary
                // (the square an unbounded room covers), the room's own lines, so a click on it picks the room.
                if reference {
                    let square;
                    let (outer, holes): (&[P2], &[Vec<P2>]) = match &region {
                        Some(g) => (&g.outer, &g.holes),
                        None => {
                            square = crate::model::unbounded_square(r.point);
                            (&square, &[])
                        }
                    };
                    let edges = || geom::ring_edges(outer).chain(holes.iter().flat_map(|h| geom::ring_edges(h)));
                    for dir in [P2(1.0, 1.0), P2(-1.0, 1.0), P2(-1.0, -1.0), P2(1.0, -1.0)] {
                        if let Some(end) = geom::ray_hit(r.point, dir, edges()) {
                            d.stroke(vec![d.view.px(r.point), d.view.px(end)], BY_CATEGORY, INK, None, false);
                        }
                    }
                }
                let (x, y) = d.view.px(r.point);
                let from = d.prims.len();
                d.text(&r.name, (x, y - 16.0), 13.0, INK);
                d.text(&r.number, (x, y), 10.0, INK);
                d.text(&area, (x, y + 14.0), 10.0, INK);
                d.grow(from, (x, y));
            }
            // A shaft the plane cuts: its outline, and a cross.
            Element::Shaft(sh) => {
                let (bottom, top) = m.span(sh.level, sh.base_offset, sh.top_level, sh.height);
                if bottom <= range.cut && range.cut < top {
                    shaft(&mut d, &sh.boundary);
                }
            }
            // Over the cut plane, under the view range's top: its outline, and its tiles.
            Element::Ceiling(c) if up => {
                let z = m.elevation(c.level) + c.offset;
                if range.cut < z && z <= range.top {
                    ceiling(&mut d, m, c);
                }
            }
            _ if up => {}
            // A mass the plane passes through: its outline, dashed.
            Element::Mass(ms) if m.mass_cut_by(ms, range.cut) => d.dashed(&m.mass_outline(ms, range.cut), BY_CATEGORY, MASS, true),
            Element::Stair(st) => stair(&mut d, m, st, level),
            Element::Ramp(r) => ramp(&mut d, m, r, level),
            Element::Railing(rl) if m.level_of(e) == Some(level) => railing(&mut d, m, rl, range.cut),
            // A room separation line: a thin line, on its own level's plan only.
            Element::RoomSeparator(rs) if rs.level == level => {
                let pts = vec![d.view.px(rs.start), d.view.px(rs.end)];
                d.stroke(pts, BY_CATEGORY, INK, None, false);
            }
            Element::Roof(r) if r.level == level => roof(&mut d, m, r),
            // Overhead, from below: its edges, round its courtyards too, dashed, while its eaves are under
            // the view range's top.
            Element::Roof(r) => {
                let eave = m
                    .roof_faces(r)
                    .into_iter()
                    .flatten()
                    .flat_map(|(loops, p)| loops.into_iter().flatten().map(move |q| p.at(q)))
                    .fold(f64::INFINITY, f64::min);
                if range.cut < eave && eave <= range.top {
                    for ring in std::iter::once(&m.roof_outline(r)).chain(&r.courtyards) {
                        let pts = ring.iter().map(|p| d.view.px(*p)).collect();
                        d.stroke(pts, BY_CATEGORY, INK, Some(vec![10.0, 5.0]), true);
                    }
                }
            }
            // A reinforcing bar of this level: solid where the cut plane is over its lowest point, dashed over the plane and under
            // the view range's top.
            Element::Rebar(rb) if m.level_of(e) == Some(level) => {
                let base = m.elevation(level);
                let lo = rb.path.iter().map(|p| base + p[2]).fold(f64::INFINITY, f64::min);
                if lo <= range.cut {
                    bar_in_plan(&mut d, rb, false, &mut bars_seen);
                } else if lo <= range.top {
                    bar_in_plan(&mut d, rb, true, &mut bars_seen);
                }
            }
            // Overhead too: a run over the cut plane and under the view range's top, dashed.
            crate::model::run_arm!(_) => {
                if m.run_geom(e).is_some_and(|g| range.cut < g.z_span().0 && g.z_span().0 <= range.top) {
                    let pts = crate::runs::outline_in(m, id, index).iter().map(|p| d.view.px(*p)).collect();
                    d.stroke(pts, BY_CATEGORY, INK, Some(vec![10.0, 5.0]), true);
                }
            }
            _ => {}
        }
    }
    // Its images on the foreground layer, in front of the model.
    images(&mut d, m, key, crate::raster::DrawLayer::Foreground, Some(&look));
    d.model_under_regions(None);
    // Regions replace model backgrounds; non-masking ones retain the visible model foreground.
    for (&id, e) in all().filter(|_| !up).filter(|(id, _)| shown(**id)) {
        if let Element::FilledRegion(r) = e
            && m.annotation_in(e, key)
        {
            d.owner = Some(id);
            filled_region(&mut d, m, r);
        }
    }
    // Annotation last, over the model; sections and elevations show on every plan. A floor plan's
    // annotation is its own.
    for (&id, e) in all().filter(|(id, _)| shown(**id)) {
        d.owner = Some(id);
        match e {
            Element::Dimension(_)
            | Element::Tag(_)
            | Element::Text(_)
            | Element::Spot(_)
            | Element::DetailLine(_)
            | Element::DetailGroup(_)
            | Element::RepeatingDetail(_)
                if up => {}
            Element::Dimension(dm) if m.annotation_in(e, key) && inside(dm.at) => dimension(&mut d, m, dm),
            Element::Tag(t)
                if m.annotation_in(e, key)
                    && m.elements.id(t.host).is_some_and(shown)
                    && m.anchor(t.host).is_some_and(|a| inside(a + t.offset)) =>
            {
                tag(&mut d, m, t, &rooms)
            }
            Element::Text(n) if m.annotation_in(e, key) && inside(n.at) => note(&mut d, m, n),
            Element::DetailLine(l) if m.annotation_in(e, key) => detail_line(&mut d, m, l),
            Element::DetailGroup(g) if m.annotation_in(e, key) => detail_group(&mut d, m, g),
            Element::RepeatingDetail(r) if m.annotation_in(e, key) => r.groups().iter().for_each(|g| detail_group(&mut d, m, g)),
            Element::Spot(sp) if sp.slope && m.annotation_in(e, key) && inside(sp.at) => slope_mark(&mut d, sp.at, m.spot_slope(sp)),
            Element::Spot(sp) if m.annotation_in(e, key) && inside(sp.at) => spot_mark(&mut d, view.px(sp.at), m.spot_value(sp), m.units),
            // A section's line is cropped as a line is.
            Element::Section(v) => section_mark(&mut d, v),
            Element::Elevation(v) if inside(v.start + (v.end - v.start) * 0.5) => elevation_mark(&mut d, v),
            // A camera isn't drawn on a plan, as Revit's: only while its view is selected (`camera_marks`).
            _ => {}
        }
    }
    // Grids stop at the crop's edge on their own, and annotation is whole or not there: the rest is cut.
    if let Some([lo, hi]) = crop {
        let (a, b) = (d.view.px(lo), d.view.px(hi));
        d.crop((a.0.min(b.0), a.1.min(b.1)), (a.0.max(b.0), a.1.max(b.1)), |o| {
            o.and_then(|id| m.elements.get(id)).is_some_and(|e| {
                matches!(
                    e,
                    Element::Grid(_)
                        | Element::Room(_)
                        | Element::Dimension(_)
                        | Element::Tag(_)
                        | Element::Text(_)
                        | Element::Spot(_)
                        | Element::Elevation(_)
                )
            })
        });
    }
    d.owner = None;
    d.by_category(|o| style_of(m, o));
    look.dress(&mut d);
    if let Some(g) = crate::graphics::graphics_of(m, key) {
        linework(&mut d, m, &g.linework);
    }
    d.weigh(|o| annotates(m, o));
    if titled {
        let mut title = m.view_name(key).map_or_else(String::new, str::to_string);
        if up {
            title = format!("{title} ({})", m.language.word("Ceiling Plan"));
        }
        let at = (24.0 + font::width(&title, 16.0) / 2.0, view.height as f32 - 26.0);
        d.text_turned(&title, at, 16.0, 0.0, INK);
    }
    d
}

/// A family's plan marks, in the model's axes: its lines and lettering in the category's pen, its dots solid ink.
fn marks(d: &mut Drawing, marks: &[crate::family::Mark]) {
    use crate::family::Mark;
    for mark in marks {
        match mark {
            Mark::Line(pts) => d.line(pts, BY_CATEGORY, INK, false),
            Mark::Label { at, height, text, align } => {
                for line in crate::family::lettering(text, *at, *height, *align) {
                    d.line(&line, BY_CATEGORY, INK, false);
                }
            }
            Mark::Dot { centre, radius } => {
                const SIDES: usize = 24;
                let ring = (0..SIDES)
                    .map(|k| {
                        let (s, c) = (std::f64::consts::TAU * k as f64 / SIDES as f64).sin_cos();
                        P2(centre.0 + radius * c, centre.1 + radius * s)
                    })
                    .collect::<Vec<_>>();
                d.fill(&[ring], INK);
            }
        }
    }
}

/// A component seen from above: each of its forms' outlines, but where a higher form covers it (a
/// table's top over its legs).
fn component(d: &mut Drawing, m: &Model, pieces: &[crate::family::Piece]) {
    if pattern_style(d, false).is_some() {
        for p in pieces {
            let visible =
                pieces.iter().filter(|q| q.top > p.top + 1e-6).fold(vec![p.outline.clone()], |rings, q| walls::minus(&rings, &q.outline));
            override_patterns(d, m, &visible, false, (P2(0.0, 0.0), 0.0));
        }
    }
    for (i, p) in pieces.iter().enumerate() {
        for (a, b) in geom::ring_edges(&p.outline) {
            let len = a.dist(b);
            if len < 1e-9 {
                continue;
            }
            let u = (b - a) * (1.0 / len);
            let mut hidden: Vec<(f64, f64)> = pieces
                .iter()
                .enumerate()
                .filter(|(j, q)| *j != i && q.top > p.top + 1e-6)
                .flat_map(|(_, q)| view3d::crossings(&q.outline, a, u))
                .map(|(s0, s1)| (s0.max(0.0), s1.min(len)))
                .filter(|(s0, s1)| s1 > s0)
                .collect();
            hidden.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut at = 0.0;
            for (s0, s1) in hidden {
                if s0 > at {
                    d.line(&[a + u * at, a + u * s0], BY_CATEGORY, INK, false);
                }
                at = f64::max(at, s1);
            }
            if at < len {
                d.line(&[a + u * at, b], BY_CATEGORY, INK, false);
            }
        }
    }
}

/// The outlines of the holes shafts cut in a floor's or a ceiling's slabs.
fn holes(d: &mut Drawing, m: &Model, slabs: &[(Vec<[f64; 3]>, f64)]) {
    let cut: std::collections::BTreeSet<Id> = slabs.iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).1).collect();
    for id in cut {
        if let Some(Element::Shaft(sh)) = m.elements.get(id) {
            d.line(&sh.boundary, Pen::PROJECTION, FAINT, true);
        }
    }
}

/// A slope arrow from `tail` up to `head`, and its slope beside its middle.
fn slope_arrow(d: &mut Drawing, tail: P2, head: P2, slope: f64) {
    d.line(&[tail, head], Pen::PROJECTION, INK, false);
    let (a, b) = (d.view.px(tail), d.view.px(head));
    let (x, y) = arrow(d, a, b);
    let text = format!("{}°", figure(slope));
    // Beside it, to its left on screen.
    let clear = font::width(&text, d.px(10.0)) / 2.0 * y.abs() + d.px(6.0) * x.abs() + d.px(4.0);
    d.text(&text, ((a.0 + b.0) / 2.0 + y * clear, (a.1 + b.1) / 2.0 - x * clear), d.px(10.0), INK);
}

/// A shaft cut in plan: its outline, and the diagonals of its box across it.
fn shaft(d: &mut Drawing, ring: &[P2]) {
    d.line(ring, BY_CATEGORY, INK, true);
    let Some((lo, hi)) = bounds(ring) else { return };
    for (from, to) in [(lo, hi), (P2(lo.0, hi.1), P2(hi.0, lo.1))] {
        let u = (to - from).unit();
        for (s0, s1) in view3d::crossings(ring, from - u, u) {
            d.line(&[from - u + u * s0, from - u + u * s1], Pen::PROJECTION, INK, false);
        }
    }
}

/// How far apart a cut pattern's lines are, px: a drafting pattern, the same on screen and on
/// paper whatever the scale.
const GAP: f64 = 6.0;
/// How far apart a surface pattern's lines are, mm: a model pattern, its tiles and courses their
/// size in the building (a tile 200 mm square, a brick 250 by 100).
const MODULE: f64 = 100.0;
/// How wide a layer is to be, px, to show its pattern: narrower, it's filled instead.
const PATTERN_ROOM: f64 = 10.0;

/// Whether the boxes round two sets of points overlap: the quick test before the exact one.
fn boxes_meet(a: &[P2], b: &[P2]) -> bool {
    let bounds = |pts: &[P2]| {
        pts.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        })
    };
    let ((alo, ahi), (blo, bhi)) = (bounds(a), bounds(b));
    alo.0 <= bhi.0 && blo.0 <= ahi.0 && alo.1 <= bhi.1 && blo.1 <= ahi.1
}

/// `rings` less the outlines of the columns the plan cuts (convex, a rectangle or a round one's polygon): what of a wall's layer
/// is not inside one.
fn outside_columns(rings: Vec<Vec<P2>>, columns: &[&[P2]]) -> Vec<Vec<P2>> {
    columns.iter().fold(rings, |rings, hole| {
        rings.into_iter().flat_map(|r| if boxes_meet(&r, hole) { geom::subtract_convex(&r, hole) } else { vec![r] }).collect()
    })
}

/// The parts of `line` outside the columns' outlines.
fn lines_outside_columns(line: &[P2], columns: &[&[P2]]) -> Vec<Vec<P2>> {
    columns.iter().fold(vec![line.to_vec()], |lines, hole| {
        lines.into_iter().flat_map(|l| if boxes_meet(&l, hole) { geom::line_outside_convex(&l, hole) } else { vec![l] }).collect()
    })
}

/// A layer where it's cut, along `ring`: where it's `room` px wide or more, its material's cut
/// pattern over the paper, its courses along unit `u`; else, unless it's its element's structure
/// (which stays poché), lighter.
fn cut_layer(d: &mut Drawing, m: &Model, layer: &Layer, ring: &[P2], u: P2, room: f64) {
    let over = pattern_style(d, true);
    if over.as_ref().is_some_and(|s| s.foreground.is_some()) {
        return;
    }
    let material = m.materials.get(&layer.material);
    let pattern = material.map_or(Pattern::None, |x| x.cut);
    let fill = material.and_then(|x| x.cut_pattern.as_ref()).and_then(|n| m.fill_patterns.get(n));
    if let Some(p) = fill.filter(|_| room >= PATTERN_ROOM && d.detail == Detail::Fine) {
        // A fill pattern of the project's, its courses along the layer.
        if over.is_none() {
            d.fill(&[ring.to_vec()], PAPER);
        }
        fill_pattern(d, p, &[ring.to_vec()], &[ring.to_vec()], INK, (P2(0.0, 0.0), u.1.atan2(u.0).to_degrees()));
    } else if pattern != Pattern::None && room >= PATTERN_ROOM && d.detail == Detail::Fine {
        if over.is_none() {
            d.fill(&[ring.to_vec()], PAPER);
        }
        let gap = GAP / d.view.scale;
        hatch(d, ring, pattern, u, INK, gap);
    } else if layer.function != Function::Structure && over.is_none() {
        d.fill(&[ring.to_vec()], LIGHT_POCHE);
    }
}

/// A surface pattern over `ring`, where its lines are far enough apart to read; a solid one at any scale.
fn surface(d: &mut Drawing, ring: &[P2], pattern: Pattern, color: Rgb) {
    if pattern == Pattern::Solid || MODULE * d.view.scale >= GAP / 2.0 {
        hatch(d, ring, pattern, P2(1.0, 0.0), color, MODULE);
    }
}

/// The most contour lines a floor's plan draws: past it, its contours are too close to read at any scale.
const MOST_CONTOURS: i64 = 2000;

/// A floor's contours in plan, as Revit draws a toposolid's: where its top stands at each whole number of `contours` over
/// zero, thin, every fifth heavier.
fn contours(d: &mut Drawing, m: &Model, f: &crate::model::Floor) {
    let step = f.contours;
    let faces = m.floor_faces(f);
    let zs = faces.iter().flat_map(|(ring, plane)| ring.iter().map(move |q| plane.at(*q)));
    let (lo, hi) = zs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
    if !(lo.is_finite() && hi.is_finite()) || ((hi - lo) / step) as i64 > MOST_CONTOURS {
        return;
    }
    for (ring, plane) in &faces {
        let (a, b) = ring.iter().map(|q| plane.at(*q)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| (lo.min(z), hi.max(z)));
        for k in (a / step).ceil() as i64..=(b / step).floor() as i64 {
            let at = k as f64 * step;
            // A convex face crosses a level line at two points of its edges, each edge counted from below to above.
            let cross: Vec<P2> = ring
                .iter()
                .zip(ring.iter().cycle().skip(1))
                .filter_map(|(p, q)| {
                    let (zp, zq) = (plane.at(*p), plane.at(*q));
                    ((zp < at) != (zq < at)).then(|| *p + (*q - *p) * ((at - zp) / (zq - zp)))
                })
                .collect();
            if let [p, q] = cross.as_slice() {
                let pen = if k.rem_euclid(5) == 0 { Pen::PROJECTION } else { Pen::FINE };
                d.line(&[*p, *q], pen, FAINT, false);
            }
        }
    }
}

/// `pattern` over `ring` (model mm), as far as the view shows it: its lines `gap` mm apart, thin,
/// its courses along unit `u`; an insulation's zigzag along the ring's length, side to side.
fn hatch(d: &mut Drawing, ring: &[P2], pattern: Pattern, u: P2, color: Rgb, gap: f64) {
    let part = std::mem::replace(&mut d.part, Part::Pattern);
    hatching(d, ring, pattern, u, color, gap);
    d.part = part;
}

fn hatching(d: &mut Drawing, ring: &[P2], pattern: Pattern, u: P2, color: Rgb, gap: f64) {
    let (w, h) = (d.view.width as f32, d.view.height as f32);
    let (lo, hi) = (d.view.mm((0.0, h)), d.view.mm((w, 0.0)));
    let ring = [(P2(1.0, 0.0), lo.0), (P2(-1.0, 0.0), -hi.0), (P2(0.0, 1.0), lo.1), (P2(0.0, -1.0), -hi.1)]
        .into_iter()
        .fold(ring.to_vec(), |r, (n, k)| geom::clip(&r, |q| q.dot(n) - k));
    if ring.len() < 3 || pattern == Pattern::None {
        return;
    }
    if pattern == Pattern::Solid {
        d.fill(&[ring], color);
        return;
    }
    let span = |dir: P2| ring.iter().map(|q| q.dot(dir)).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
    let n = u.perp();
    let mut segment = |d: &mut Drawing, from: P2, dir: P2, s0: f64, s1: f64| {
        for (a, b) in view3d::crossings(&ring, from, dir) {
            let (a, b) = (a.max(s0), b.min(s1));
            if b > a {
                d.line(&[from + dir * a, from + dir * b], Pen::FINE, color, false);
            }
        }
    };
    // Lines along `dir`, `step` apart across it, `shift` along it from line to line.
    let lines = |d: &mut Drawing, dir: P2, step: f64, seg: &mut dyn FnMut(&mut Drawing, P2, P2, f64, f64)| {
        let ((a0, _), (c0, c1)) = (span(dir), span(dir.perp()));
        if (c1 - c0) / step > 2000.0 {
            return;
        }
        let mut k = (c0 / step).ceil();
        while k * step <= c1 {
            seg(d, dir.perp() * (k * step) + dir * (a0 - 1.0), dir, f64::NEG_INFINITY, f64::INFINITY);
            k += 1.0;
        }
    };
    let turned = |deg: f64| {
        let (sin, cos) = deg.to_radians().sin_cos();
        P2(u.0 * cos - u.1 * sin, u.0 * sin + u.1 * cos)
    };
    match pattern {
        Pattern::None | Pattern::Solid => {}
        Pattern::Diagonal => lines(d, turned(45.0), gap, &mut segment),
        Pattern::Crosshatch => {
            lines(d, turned(45.0), gap * 1.4, &mut segment);
            lines(d, turned(-45.0), gap * 1.4, &mut segment);
        }
        Pattern::Horizontal => lines(d, u, gap, &mut segment),
        Pattern::Vertical => lines(d, n, gap, &mut segment),
        Pattern::Grid => {
            lines(d, u, gap * 2.0, &mut segment);
            lines(d, n, gap * 2.0, &mut segment);
        }
        // Courses along `u`, their joints staggered half a brick from course to course.
        Pattern::Brick => {
            lines(d, u, gap, &mut segment);
            let ((a0, a1), (c0, c1)) = (span(u), span(n));
            if (a1 - a0) / gap * (c1 - c0) / gap > 40_000.0 {
                return;
            }
            let mut k = (c0 / gap).floor();
            while k * gap <= c1 {
                let stagger = if (k as i64).rem_euclid(2) == 0 { 0.0 } else { gap * 1.25 };
                let mut j = ((a0 - stagger) / (gap * 2.5)).floor();
                while j * gap * 2.5 + stagger <= a1 {
                    let from = u * (j * gap * 2.5 + stagger) + n * (k * gap);
                    segment(d, from, n, 0.0, gap);
                    j += 1.0;
                }
                k += 1.0;
            }
        }
        // Stipple: a dot every so often, each nudged its own way.
        Pattern::Concrete => {
            let ((a0, a1), (c0, c1)) = (span(u), span(n));
            let step = gap * 1.3;
            if (a1 - a0) / step * (c1 - c0) / step > 40_000.0 {
                return;
            }
            let dot = 0.8 / d.view.scale;
            let mut k = (c0 / step).floor();
            while k * step <= c1 {
                let mut j = (a0 / step).floor();
                while j * step <= a1 {
                    let h = crate::model::mix((j as i64 as u64).wrapping_mul(0x9E37) ^ (k as i64 as u64));
                    let (x, y) = ((h & 0xffff) as f64 / 65535.0, ((h >> 16) & 0xffff) as f64 / 65535.0);
                    let at = u * ((j + x) * step) + n * ((k + y) * step);
                    if geom::contains(&ring, at) {
                        d.line(&[at, at + u * dot], Pen::STIPPLE, color, false);
                    }
                    j += 1.0;
                }
                k += 1.0;
            }
        }
        // A batt's zigzag along the ring's longer way, touching its sides.
        Pattern::Insulation => {
            let (along, across) = if span(u).1 - span(u).0 >= span(n).1 - span(n).0 { (u, n) } else { (n, u) };
            let ((a0, a1), (c0, c1)) = (span(along), span(across));
            let wave = (c1 - c0).max(gap);
            if (a1 - a0) / wave > 5000.0 {
                return;
            }
            let mut i = 0.0;
            while a0 + i * wave / 2.0 <= a1 {
                let side = |i: f64| if (i as i64).rem_euclid(2) == 0 { c0 } else { c1 };
                let (p, q) =
                    (along * (a0 + i * wave / 2.0) + across * side(i), along * (a0 + (i + 1.0) * wave / 2.0) + across * side(i + 1.0));
                let len = p.dist(q);
                if len > 0.0 {
                    segment(d, p, (q - p).unit(), 0.0, len);
                }
                i += 1.0;
            }
        }
    }
}

/// A ceiling in its reflected ceiling plan: its outline and its openings', the holes shafts cut in
/// it, and with a grid, its tiles: lines through its middle, a tile apart, stopping at the holes.
fn ceiling(d: &mut Drawing, m: &Model, c: &Ceiling) {
    let slabs = m.ceiling_slabs(c);
    // Its bottom layer's surface pattern, or the view's foreground and background.
    let material = m.type_layers("ceiling", &c.type_name).last().and_then(|l| m.materials.get(&l.material));
    for (face, rise) in &slabs {
        for piece in m.less_shafts(face, *rise).0 {
            let replaced = override_patterns(d, m, std::slice::from_ref(&piece), false, (P2(0.0, 0.0), 0.0));
            if !replaced && let Some(x) = material {
                match x.surface_pattern.as_ref().and_then(|n| m.fill_patterns.get(n)) {
                    Some(p) => fill_pattern(
                        d,
                        p,
                        std::slice::from_ref(&piece),
                        std::slice::from_ref(&piece),
                        x.surface_ink(INK),
                        (P2(0.0, 0.0), 0.0),
                    ),
                    None => surface(d, &piece, x.surface, x.surface_ink(INK)),
                }
            }
        }
    }
    for ring in std::iter::once(&c.boundary).chain(&c.openings) {
        d.line(ring, BY_CATEGORY, INK, true);
    }
    holes(d, m, &slabs);
    if pattern_style(d, false).is_some_and(|s| s.foreground.is_some()) {
        return;
    }
    let (Some(t), Some((lo, hi))) = (m.types.ceiling.get(&c.type_name), bounds(&c.boundary)) else { return };
    // ponytail: tiles square to the axes; a ceiling's own grid angle when rooms turn.
    if t.grid <= 0.0 || (hi.0 - lo.0).max(hi.1 - lo.1) / t.grid > 2000.0 {
        return;
    }
    let pieces: Vec<Vec<P2>> = slabs.iter().flat_map(|(face, rise)| m.less_shafts(face, *rise).0).collect();
    let mid = (lo + hi) * 0.5;
    for (u, across, (a, b)) in
        [(P2(0.0, 1.0), P2(1.0, 0.0), (lo.0 - mid.0, hi.0 - mid.0)), (P2(1.0, 0.0), P2(0.0, 1.0), (lo.1 - mid.1, hi.1 - mid.1))]
    {
        let n = ((a / t.grid).ceil() as i64, (b / t.grid).floor() as i64);
        for k in n.0..=n.1 {
            // A micron off the grid: a line along an edge between two pieces (where a hole cuts the
            // ceiling up) pairs with neither and would vanish.
            let from = mid + across * (k as f64 * t.grid + 1e-3) - u * (hi - lo).length();
            for piece in &pieces {
                for (s0, s1) in view3d::crossings(piece, from, u) {
                    d.line(&[from + u * s0, from + u * s1], Pen::PROJECTION, INK, false);
                }
            }
        }
    }
}

/// The corners of the box around `ring`.
fn bounds(ring: &[P2]) -> Option<(P2, P2)> {
    let first = *ring.first()?;
    Some(ring.iter().fold((first, first), |(lo, hi), p| (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))))
}

/// The part of the segment from `p` to `q` inside the box from `lo` to `hi` (Liang-Barsky); its
/// ends as they were where they're inside.
pub(crate) fn clip(p: P2, q: P2, lo: P2, hi: P2) -> Option<(P2, P2)> {
    let d = q - p;
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (pk, qk) in [(-d.0, p.0 - lo.0), (d.0, hi.0 - p.0), (-d.1, p.1 - lo.1), (d.1, hi.1 - p.1)] {
        if pk == 0.0 {
            if qk < 0.0 {
                return None;
            }
        } else if pk < 0.0 {
            t0 = t0.max(qk / pk);
        } else {
            t1 = t1.min(qk / pk);
        }
    }
    (t0 <= t1).then(|| (if t0 > 0.0 { p + d * t0 } else { p }, if t1 < 1.0 { p + d * t1 } else { q }))
}

/// Whether the segment from `p` to `q` passes through the box from `lo` to `hi`.
fn clips(p: (f32, f32), q: (f32, f32), lo: (f32, f32), hi: (f32, f32)) -> bool {
    let mm = |p: (f32, f32)| P2(f64::from(p.0), f64::from(p.1));
    clip(mm(p), mm(q), mm(lo), mm(hi)).is_some()
}

/// The part of a ring inside the box from `lo` to `hi`, side by side (Sutherland-Hodgman): exact
/// for the convex rings plans fill.
fn clip_ring(ring: &[(f32, f32)], lo: (f32, f32), hi: (f32, f32)) -> Vec<(f32, f32)> {
    let mut pts = ring.to_vec();
    // Each side of the box: which coordinate, where, and whether inside lies above it.
    for (x, bound, above) in [(true, lo.0, true), (true, hi.0, false), (false, lo.1, true), (false, hi.1, false)] {
        let at = |p: (f32, f32)| if x { p.0 } else { p.1 };
        let inside = |p: (f32, f32)| if above { at(p) >= bound } else { at(p) <= bound };
        let cross = |a: (f32, f32), b: (f32, f32)| {
            let t = (bound - at(a)) / (at(b) - at(a));
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        };
        let n = pts.len();
        let mut kept = Vec::with_capacity(n + 4);
        for i in 0..n {
            let (a, b) = (pts[(i + n - 1) % n], pts[i]);
            match (inside(a), inside(b)) {
                (true, true) => kept.push(b),
                (true, false) => kept.push(cross(a, b)),
                (false, true) => kept.extend([cross(a, b), b]),
                (false, false) => {}
            }
        }
        pts = kept;
    }
    pts
}

/// A view fitting `pts` into the image, leaving the margin free: see [`fit_by`].
fn fit(pts: &[P2], width: u32, height: u32, n: f64) -> View {
    let lo = pts.iter().fold(P2(f64::MAX, f64::MAX), |a, p| P2(a.0.min(p.0), a.1.min(p.1)));
    let hi = pts.iter().fold(P2(f64::MIN, f64::MIN), |a, p| P2(a.0.max(p.0), a.1.max(p.1)));
    let (lo, hi) = if pts.is_empty() { (P2(-5000.0, -5000.0), P2(5000.0, 5000.0)) } else { (lo, hi) };
    fit_by(|_| [lo, hi], width, height, n)
}

/// A view fitting what `extent` says is shown at a scale (px per mm) into the image, leaving the margin free: `MARGIN` px
/// round it, and as many times that as the annotation of a view at 1 : `n` is bigger than on screen ([`annotation_of`]), which
/// would otherwise outgrow it. The annotation's size and so the margin follow the scale the view comes out at, so that is
/// found by halving: the largest scale at which the view and its margin fit.
fn fit_by(extent: impl Fn(f64) -> [P2; 2], width: u32, height: u32, n: f64) -> View {
    let fits = |scale: f64| {
        let [lo, hi] = extent(scale);
        let room = |px: u32| (f64::from(px) - 2.0 * MARGIN * f64::from(annotation_of(n, scale)).max(1.0)).max(1.0);
        (hi.0 - lo.0).max(1000.0) * scale <= room(width) && (hi.1 - lo.1).max(1000.0) * scale <= room(height)
    };
    // A view is at least a metre across, so no scale is past a pixel to the mm of the image's smaller side.
    let (mut fits_at, mut too_big) = (0.0, f64::from(width.min(height).max(1)) / 1000.0);
    for _ in 0..48 {
        let mid = (fits_at + too_big) / 2.0;
        *(if fits(mid) { &mut fits_at } else { &mut too_big }) = mid;
    }
    let [lo, hi] = extent(fits_at);
    View { scale: fits_at, cx: (lo.0 + hi.0) / 2.0, cy: (lo.1 + hi.1) / 2.0, width, height }
}

/// A door or window the cut plane at height `cut` passes through: a gap in the wall, jambs, and
/// a leaf with its swing, or glass.
fn opening(d: &mut Drawing, m: &Model, e: &Element, o: &Opening, w: &Wall, cut: f64, shown: bool) {
    let Some(t) = m.opening_type(e) else { return };
    let bottom = m.own_base(w) + o.sill;
    if !(bottom <= cut && cut < bottom + t.height) {
        return;
    }
    let (axis, (ext, int)) = (Axis::of(w), walls::faces(m, w));
    let (s0, s1) = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
    // The gap is the wall's: where an opening reaches past the wall's end (a warning, not a refusal), only the part in it is cut.
    let (g0, g1) = (s0.max(0.0), s1.min(axis.length()));
    if g1 - g0 <= 0.0 {
        return;
    }
    // Clear a little past the faces too, where the outlines' outer halves are; along an arc, an arc.
    let past = 3.0 / d.view.scale;
    let gap: Vec<P2> = axis.points(g0, g1, int - past).into_iter().chain(axis.points(g1, g0, ext + past)).collect();
    // The opening is the wall's: a hidden door or window leaves it, outlined, owned by no one.
    let owner = d.owner;
    if !shown {
        d.own(None);
    }
    d.fill(&[gap], PAPER);
    for s in [g0, g1] {
        d.line(&[axis.at(s, int), axis.at(s, ext)], Pen::HEAVY, INK, false);
    }
    d.own(owner);
    // At coarse, a window of the project's own type is its gap; a door of the project's own type still shows its leaf and
    // swing, as Revit's Single-Flush draws its panel and Plan Swing at every detail level. A family's forms and symbols are
    // drawn at the levels their Visibility Settings say, and those that say nothing at every level but Coarse.
    if !shown || (d.detail == Detail::Coarse && o.family.is_none() && !matches!(e, Element::Door(_))) {
        return;
    }
    // Of a family: the forms the cut plane passes through that a plan at its detail level draws (as Revit's Plan/RCP when
    // cut: a door's leaf, shut in the opening, says no and is drawn open by its symbols), outlined, and the lines of the
    // symbols drawn at that level (a door's leaf and swing).
    if o.family.is_some() {
        let detail = d.detail;
        let drawn = |p: &crate::family::Piece| p.show.map_or(detail != Detail::Coarse, |s| s.plan && s.plan_cut && s.at(detail));
        for piece in m.opening_pieces(e).iter().filter(|p| p.bottom <= cut && cut < p.top && drawn(p)) {
            d.line(&piece.outline, BY_CATEGORY, INK, true);
        }
        d.part(Part::Projection);
        marks(d, &m.opening_marks(e, detail));
        d.part(Part::Cut);
        return;
    }
    if matches!(e, Element::Door(_)) {
        if let Some(way) = m.opening_way(e) {
            door_leaves(d, &axis, &way, t.operation.unwrap_or(Operation::Swing), t.leaf());
        }
    } else {
        for across in [int, (ext + int) / 2.0, ext] {
            d.line(&axis.points(s0, s1, across), Pen::PROJECTION, INK, false);
        }
    }
}

/// A door's leaves in plan, as Revit's door families draw them (`way`: [`Model::opening_way`]; `thick`: the leaf's
/// thickness): each leaf a panel, outlined in the door's cut pen, and a swinging leaf open square to its wall on the face it
/// swings to, with the arc its free edge sweeps, seen, in the projection pen (Plan Swing). One leaf swings from its hinged
/// jamb; a double door's two from both jambs, meeting in the middle; a sliding one's leaf lies on the side it slides on,
/// past a fixed one on the other side; a double sliding door's two meet in the middle of the wall; a folding one's two
/// panels fold toward the hinged jamb, half open; a fixed one is a panel in the middle of the wall, shut.
fn door_leaves(d: &mut Drawing, axis: &Axis, way: &OpeningWay, op: Operation, thick: f64) {
    let (hinge, free) = (way.hinge, way.free);
    let mid = (hinge + free) / 2.0;
    let toward = (free - hinge).signum();
    let centre = (way.face + way.back) / 2.0;
    // A panel lying along the wall, from `a` to `b` along it and from `off0` to `off1` across.
    let along = |d: &mut Drawing, a: f64, b: f64, off0: f64, off1: f64| {
        let ring: Vec<P2> = axis.points(a, b, off0).into_iter().chain(axis.points(b, a, off1)).collect();
        d.line(&ring, BY_CATEGORY, INK, true);
    };
    // A panel from `a` to `b`, `thick` to its left.
    let panel = |d: &mut Drawing, a: P2, b: P2, side: f64| {
        let n = (b - a).unit().perp() * (thick * side);
        d.line(&[a, b, b + n, a + n], BY_CATEGORY, INK, true);
    };
    // A leaf hinged at `at` along the wall, its free edge at `to`: open square to the wall, inside the opening, and the arc.
    let swing = |d: &mut Drawing, at: f64, to: f64| {
        let (pivot, shut) = (axis.at(at, way.face), axis.at(to, way.face));
        let (u, out) = (axis.tangent(at) * (to - at).signum(), axis.tangent(at).perp() * way.to);
        let open = pivot + out * pivot.dist(shut);
        d.line(&[pivot, open, open + u * thick, pivot + u * thick], BY_CATEGORY, INK, true);
        // What the leaf sweeps is seen, not cut.
        d.part(Part::Projection);
        d.line(&arc(pivot, open, shut, 24), BY_CATEGORY, INK, false);
        d.part(Part::Cut);
    };
    match op {
        Operation::Swing => swing(d, hinge, free),
        Operation::DoubleSwing => {
            swing(d, hinge, mid);
            swing(d, free, mid);
        }
        Operation::Sliding => {
            // The sliding leaf on the face it opens to, over the free half and past the middle by a leaf; the fixed
            // one behind it over the hinged half.
            along(d, mid - toward * thick, free, centre, centre + way.to * thick);
            along(d, hinge, mid + toward * thick, centre - way.to * thick, centre);
        }
        Operation::DoubleSliding => {
            along(d, hinge, mid, centre - thick / 2.0, centre + thick / 2.0);
            along(d, mid, free, centre - thick / 2.0, centre + thick / 2.0);
        }
        Operation::Folding => {
            // Two panels, each half the opening, folded 45° out from the hinged jamb on the face it opens to.
            let (pivot, u) = (axis.at(hinge, way.face), axis.tangent(hinge) * toward);
            let out = axis.tangent(hinge).perp() * way.to;
            let half = (free - hinge).abs() / 2.0;
            let k = std::f64::consts::FRAC_1_SQRT_2;
            let knee = pivot + (u + out) * (k * half);
            let end = knee + (u - out) * (k * half);
            // Each panel's thickness inside the fold, toward the wall.
            let side = |a: P2, b: P2| if (b - a).perp().dot(out) > 0.0 { -1.0 } else { 1.0 };
            panel(d, pivot, knee, side(pivot, knee));
            panel(d, knee, end, side(knee, end));
        }
        Operation::Fixed => along(d, hinge, free, centre - thick / 2.0, centre + thick / 2.0),
    }
}

/// A stair in the plan of `level`. From its own level: its risers up to the cut plane, a cut line
/// across the run there, the rest (and the landings over the cut plane) dashed, and its walk line
/// marked UP. From the level it climbs to: all of it, and the walk line marked DN.
fn stair(d: &mut Drawing, m: &Model, st: &Stair, level: impl Key) {
    let level = m.guid(level);
    let up = st.level == level;
    if !up && st.top_level != Some(level) {
        return;
    }
    let (_, each, _) = m.flight(st);
    let l = stairs::stair(m, st);
    let half = st.width / 2.0;
    // The riser the cut plane passes through, counted from the first; from above, none.
    let rise = m.shown_range(level).cut - m.elevation(st.level);
    let cut = if up && each > 0.0 { (rise / each).floor() } else { f64::INFINITY };
    // Risers below the run drawn.
    let mut below = 0.0;
    for run in &l.runs {
        let at = |along: f64, across: f64| run.start + run.u * along + run.u.perp() * across;
        for k in 0..run.risers {
            let k = f64::from(k);
            segment(d, at(k * st.tread, -half), at(k * st.tread, half), below + k > cut);
        }
        let line = ((cut - below + 0.5) * st.tread).clamp(0.0, run.length);
        for side in [-half, half] {
            if line > 0.0 {
                segment(d, at(0.0, side), at(line, side), false);
            }
            if line < run.length {
                segment(d, at(line, side), at(run.length, side), true);
            }
        }
        if cut >= below && line < run.length {
            d.line(&[at(line - 0.4 * st.tread, -half), at(line + 0.4 * st.tread, half)], Pen::PROJECTION, INK, false);
        }
        below += f64::from(run.risers);
    }
    for pad in &l.pads {
        let over = up && pad.z - m.elevation(st.level) > rise;
        for (a, b) in geom::ring_edges(&pad.ring) {
            segment(d, a, b, over);
        }
    }
    walk(d, m, &l.walk, up);
}

/// A ramp in the plan of `level`: from its own level, its runs' and landings' outlines, dashed
/// where they're over the cut plane, and its walk line marked UP; from the level it climbs to, all
/// of it, marked DN.
fn ramp(d: &mut Drawing, m: &Model, r: &Ramp, level: impl Key) {
    let level = m.guid(level);
    let up = r.level == level;
    if !up && r.top_level != Some(level) {
        return;
    }
    let l = stairs::ramp(m, r);
    let cut = if up { m.plan_range(level).cut } else { f64::INFINITY };
    let rings = l.runs.iter().map(|run| (run.ring(r.width), run.z.0)).chain(l.pads.iter().map(|p| (p.ring.clone(), p.z)));
    for (ring, z) in rings {
        for (a, b) in geom::ring_edges(&ring) {
            segment(d, a, b, z > cut);
        }
    }
    walk(d, m, &l.walk, up);
}

/// A railing in a plan cutting at height `cut`: its line, solid where what it runs along is under
/// the cut plane, dashed over it.
fn railing(d: &mut Drawing, m: &Model, rl: &Railing, cut: f64) {
    for w in stairs::railing(m, rl).windows(2) {
        let &[(a, za), (b, zb)] = w else { continue };
        let parts = if (za > cut) != (zb > cut) {
            let mid = a + (b - a) * ((cut - za) / (zb - za));
            vec![(a, mid, za > cut), (mid, b, zb > cut)]
        } else {
            vec![(a, b, za > cut)]
        };
        for (p, q, over) in parts {
            let pts = vec![d.view.px(p), d.view.px(q)];
            d.stroke(pts, BY_CATEGORY, INK, over.then(|| vec![6.0, 4.0]), false);
        }
    }
}

/// A line from `a` to `b`, one pixel wide; dashed, `over`.
fn segment(d: &mut Drawing, a: P2, b: P2, over: bool) {
    let pts = vec![d.view.px(a), d.view.px(b)];
    d.stroke(pts, BY_CATEGORY, INK, over.then(|| vec![6.0, 4.0]), false);
}

/// A stair's or a ramp's walk line, its arrow at the top, marked UP at its foot; from above, the
/// other way, marked DN.
fn walk(d: &mut Drawing, m: &Model, line: &[(P2, f64)], up: bool) {
    let mut pts: Vec<P2> = line.iter().map(|(p, _)| *p).collect();
    pts.dedup_by(|a, b| a.dist(*b) < 1e-6);
    if !up {
        pts.reverse();
    }
    let (Some(&first), Some(&second), [.., before, last]) = (pts.first(), pts.get(1), pts.as_slice()) else { return };
    let label = m.language.word(if up { msg!("UP") } else { msg!("DN") });
    d.line(&pts, Pen::PROJECTION, INK, false);
    arrow(d, d.view.px(*before), d.view.px(*last));
    let (a, b) = (d.view.px(first), d.view.px(second));
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    // Before the walk line's start, clear of it however long the word is.
    let back = font::width(label, d.px(9.0)) / 2.0 * x.abs() + d.px(5.0) * y.abs() + d.px(8.0);
    d.text(label, (a.0 - x * back, a.1 - y * back), d.px(9.0), INK);
}

/// An arrowhead at pixel `b`, pointing from `a`; the way it points, on screen.
fn arrow(d: &mut Drawing, a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (x, y) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let (long, wide) = (d.px(10.0), d.px(4.0));
    let head = vec![b, (b.0 - x * long - y * wide, b.1 - y * long + x * wide), (b.0 - x * long + y * wide, b.1 - y * long - x * wide)];
    d.push(Prim::fill(vec![head], INK));
    (x, y)
}

/// A roof in the plan of its level: its outline, and where its slopes meet, its ridges, hips and
/// valleys.
// ponytail: openings aren't drawn; a dormer's roof covers its own. Draw them for skylights.
fn roof(d: &mut Drawing, m: &Model, r: &Roof) {
    let faces = m.roof_faces(r).map_or_else(|_| vec![m.roof_outline(r)], |f| f.into_iter().flat_map(|(loops, _)| loops).collect());
    // A shaped one's creases thin, as a shaped floor's are, its outline and courtyards as any roof's.
    let shaped = !r.shape.is_empty();
    for f in faces {
        override_patterns(d, m, std::slice::from_ref(&f), false, (P2(0.0, 0.0), 0.0));
        d.line(&f, BY_CATEGORY, if shaped { FAINT } else { INK }, true);
    }
    if shaped {
        for ring in std::iter::once(&r.boundary).chain(&r.courtyards) {
            d.line(ring, BY_CATEGORY, INK, true);
        }
    }
}

/// The short arc around `center` from `from` to `to`, as `n` segments.
fn arc(center: P2, from: P2, to: P2, n: u32) -> Vec<P2> {
    let (a, b) = (from - center, to - center);
    let start = a.1.atan2(a.0);
    let mut sweep = b.1.atan2(b.0) - start;
    if sweep > PI {
        sweep -= 2.0 * PI;
    } else if sweep < -PI {
        sweep += 2.0 * PI;
    }
    (0..=n)
        .map(|i| {
            let t = start + sweep * f64::from(i) / f64::from(n);
            center + P2(t.cos(), t.sin()) * a.length()
        })
        .collect()
}

/// A number as a dimension shows it: to the hundredth, without trailing zeros.
fn figure(x: f64) -> String {
    format!("{}", (x * 100.0).round() / 100.0 + 0.0)
}

/// Fill pattern `p` over `rings` (model mm; `pieces`, the same with their holes cut out, for a solid one) in `color`, its
/// lines thin, started at `placed`'s origin and turned its angle. Lines closer than a pixel are too dense to read: left out,
/// and so are all of them when there would be more than a drawing can hold, which `d.notes` says.
fn fill_pattern(d: &mut Drawing, p: &crate::fills::FillPattern, rings: &[Vec<P2>], pieces: &[Vec<P2>], color: Rgb, placed: (P2, f64)) {
    let part = std::mem::replace(&mut d.part, Part::Pattern);
    if p.is_solid() {
        d.fill(pieces, color);
        d.part = part;
        return;
    }
    let (w, h) = (d.view.width as f32, d.view.height as f32);
    let window = (d.view.mm((0.0, h)), d.view.mm((w, 0.0)));
    // A drafting pattern's mm are the view's paper's.
    let k = match p.target {
        crate::fills::Target::Drafting => (sheet::DENSITY as f32 * d.paper) as f64 / d.view.scale,
        crate::fills::Target::Model => 1.0,
    };
    let closest = p.lines.iter().map(|l| l.spacing).fold(f64::INFINITY, f64::min) * k * d.view.scale;
    if closest >= 1.0 {
        match crate::fills::strokes_placed(p, rings, k, window, 50_000, placed) {
            Some(strokes) => {
                for [a, b] in strokes {
                    d.line(&[a, b], Pen::FINE, color, false);
                }
            }
            None => {
                let who = d.owner.map_or_else(|| "a fill pattern".to_string(), |id| format!("the fill pattern of element {id}"));
                let note =
                    format!("{who} has more lines than a view can hold at this scale, so none is drawn: zoom in, or space its lines wider");
                if d.notes.len() < 32 && !d.notes.contains(&note) {
                    d.notes.push(note);
                }
            }
        }
    }
    d.part = part;
}

fn pattern_style(d: &Drawing, cut: bool) -> Option<crate::graphics::Style> {
    let s = d.pattern_styles.get(&d.owner?)?.get(usize::from(cut))?;
    (s.foreground.is_some() || s.background.is_some()).then(|| s.clone())
}

/// The overridden layers, clipped to the same pieces as the original surface. Whether its foreground replaces the material's.
fn override_patterns(d: &mut Drawing, m: &Model, pieces: &[Vec<P2>], cut: bool, placed: (P2, f64)) -> bool {
    let Some(s) = pattern_style(d, cut) else { return false };
    for layer in [&s.background, &s.foreground].into_iter().flatten() {
        let Some(p) = layer.pattern.as_ref().and_then(|n| m.fill_patterns.get(n)) else { continue };
        let [r, g, b] = layer.color;
        fill_pattern(d, p, pieces, pieces, Rgb(r, g, b), placed);
    }
    s.foreground.is_some()
}

/// A filled region: over the paper where its type masks, its background pattern, then its foreground, each in its colour
/// (a solid one filled, a drafting one's lengths mm on the view's paper, a model one's mm), then its boundary's lines in
/// their style, unless it has none. A sketch that doesn't close draws nothing.
pub(crate) fn filled_region(d: &mut Drawing, m: &Model, r: &FilledRegion) {
    let Some(t) = m.region_types.get(&r.type_name) else { return };
    let Ok(profile) = crate::sketch::solve(&r.sketch) else { return };
    let part = std::mem::replace(&mut d.part, Part::Pattern);
    // A view's override of the type's foreground or background replaces that layer's pattern and colour.
    let over = pattern_style(d, false);
    let layers: Vec<(&crate::fills::FillPattern, Rgb)> = [
        (&t.background, t.background_color, over.as_ref().and_then(|s| s.background.as_ref())),
        (&t.foreground, t.foreground_color, over.as_ref().and_then(|s| s.foreground.as_ref())),
    ]
    .into_iter()
    .filter_map(|(name, color, layer)| {
        let (name, [cr, cg, cb]) = layer.map_or((name, color), |p| (&p.pattern, p.color));
        Some((name.as_ref().and_then(|n| m.fill_patterns.get(n))?, Rgb(cr, cg, cb)))
    })
    .collect();
    for region in &profile.regions {
        let rings: Vec<Vec<P2>> = std::iter::once(region.outer.clone()).chain(region.holes.iter().cloned()).collect();
        // Its outline less its holes, as convex pieces: a solid fill and a mask need them, and they cost more than a
        // pattern's lines do, which run through the rings as they are.
        let pieces = if t.masking || layers.iter().any(|(p, _)| p.is_solid()) {
            region.holes.iter().fold(vec![region.outer.clone()], |ps, hole| walls::minus(&ps, hole))
        } else {
            Vec::new()
        };
        if t.masking {
            d.fill(&pieces, PAPER);
            regions::mask(d, &pieces);
        }
        let before_patterns = d.prims.len();
        for (p, color) in &layers {
            fill_pattern(d, p, &rings, &pieces, *color, (r.pattern_origin, r.pattern_angle));
        }
        if !t.masking && d.prims.len() > before_patterns {
            regions::foreground(d, m, &rings);
        }
    }
    d.part = part;
    // Each boundary line in its own style, else the region's; an invisible one, or one of a region with invisible lines
    // and no style of its own, not at all.
    for l in r.sketch.iter().filter(|l| !l.invisible) {
        let Some(st) = l.style.as_ref().or(r.line_style.as_ref()).and_then(|n| m.line_styles.get(n)) else { continue };
        let pts = l.points().into_iter().map(|p| d.view.px(p)).collect();
        d.styled(m, st, pts, false);
    }
}

/// A detail line, in its style's pen, colour and pattern: its dashes as long on the paper the view stands for as the
/// pattern says, solid where they'd be too small to see.
pub(crate) fn detail_line(d: &mut Drawing, m: &Model, l: &DetailLine) {
    let Some(st) = m.line_styles.get(&l.style) else { return };
    // A closed one is stroked as a ring, so its dashes and its corner run on round its first point.
    let closed = l.curve.is_closed() && l.insulation.is_none();
    let mut pts: Vec<(f32, f32)> = l.drawn().into_iter().map(|p| d.view.px(p)).collect();
    if closed && pts.len() > 2 {
        pts.pop();
    }
    let [r, g, b] = st.color;
    d.styled(m, st, pts, closed);
    // Its centre marks: a cross 3 mm across on paper, solid.
    if l.centre_mark {
        let arm = d.px((1.5 * sheet::DENSITY) as f32);
        for c in l.curve.centres() {
            let (x, y) = d.view.px(c);
            d.stroke(vec![(x - arm, y), (x + arm, y)], st.pen, Rgb(r, g, b), None, false);
            d.stroke(vec![(x, y - arm), (x, y + arm)], st.pen, Rgb(r, g, b), None, false);
        }
    }
}

/// The edge element `id` draws in plan `key` nearest `at`, within `reach` mm: a straight piece of one of its lines, its
/// ends in plan mm. What Linework picks.
pub fn edge_at(m: &Model, key: Id, id: Id, at: P2, reach: f64) -> Option<(P2, P2)> {
    line_at(m, key, id, at, reach, false)
}

/// Every edge element `id` draws in plan `key` within `reach` mm of `at` (as [`edge_at`] finds one) and a little past it,
/// nearest first: an edge at the rim of the reach is still seen. What says which Linework is on an edge that is drawn.
pub fn edges_at(m: &Model, key: Id, id: Id, at: P2, reach: f64) -> Vec<(P2, P2)> {
    lines_at(m, key, id, at, reach, false).into_iter().map(|(_, a, b)| (a, b)).collect()
}

/// The line of element `id`'s fill pattern in plan `key` nearest `at`, within `reach` mm, its ends in plan mm: what Align
/// moves a region's or a floor's model pattern by.
pub fn pattern_line_at(m: &Model, key: Id, id: Id, at: P2, reach: f64) -> Option<(P2, P2)> {
    line_at(m, key, id, at, reach, true)
}

fn line_at(m: &Model, key: Id, id: Id, at: P2, reach: f64, pattern: bool) -> Option<(P2, P2)> {
    lines_at(m, key, id, at, reach, pattern).into_iter().find(|(dist, ..)| *dist <= 10.0).map(|(_, a, b)| (a, b))
}

/// The straight pieces of element `id`'s lines in plan `key` (its fill pattern's, when `pattern`) within `reach` mm of `at`
/// and half a pixel more, nearest first, each with its distance in pixels, 10 of which are `reach`.
fn lines_at(m: &Model, key: Id, id: Id, at: P2, reach: f64, pattern: bool) -> Vec<(f64, P2, P2)> {
    // A window round the point, 10 px to `reach`, drawn without the view's own linework (an edge drawn unseen is still one).
    let scale = 10.0 / reach.max(1.0);
    let view = View { scale, cx: at.0, cy: at.1, width: 1000, height: 1000 };
    if !pattern {
        let centerlines: Option<Vec<Vec<P2>>> = match m.elements.get(id).filter(|e| m.annotation_in(e, key)) {
            Some(Element::DetailLine(l)) => Some(vec![l.curve.points()]),
            Some(Element::FilledRegion(r)) => Some(r.sketch.iter().map(|l| l.points()).collect()),
            _ => None,
        };
        if let Some(lines) = centerlines {
            let mut out: Vec<_> = lines
                .iter()
                .flat_map(|pts| pts.windows(2))
                .map(|w| (geom::dist_to_segment(at, w[0], w[1]) * scale, w[0], w[1]))
                .filter(|(d, ..)| *d <= 10.5)
                .collect();
            out.sort_by(|a, b| a.0.total_cmp(&b.0));
            return out;
        }
    }
    let bare;
    let m = match crate::graphics::graphics_of(m, key).filter(|g| !g.linework.is_empty()) {
        Some(_) => {
            let mut c = m.clone();
            if let Some(g) = c.elements.get_mut(c.plan_source(key)).and_then(crate::graphics::graphics_mut) {
                g.linework.clear();
            }
            bare = c;
            &bare
        }
        None => m,
    };
    let d = draw(m, key, view);
    let q = view.px(at);
    let near = |a: (f32, f32), b: (f32, f32)| geom::dist_to_segment(P2(f64::from(q.0), f64::from(q.1)), pt(a), pt(b));
    let mut out: Vec<_> = (d.prims.iter().zip(&d.owners).zip(&d.parts))
        .filter(|((_, o), part)| {
            **o == Some(id) && if pattern { **part == Part::Pattern } else { !matches!(part, Part::Text | Part::Pattern) }
        })
        .flat_map(|((p, _), _)| match p {
            Prim::Line { pts, closed, .. } => segments(pts, *closed),
            _ => Vec::new(),
        })
        .map(|(a, b)| (near(a, b), view.mm(a), view.mm(b)))
        .filter(|(dist, ..)| *dist <= 10.5)
        .collect();
    out.sort_by(|x, y| x.0.total_cmp(&y.0));
    out
}

fn pt((x, y): (f32, f32)) -> P2 {
    P2(f64::from(x), f64::from(y))
}

/// A line's straight pieces, round to its first point again where it's closed.
fn segments(pts: &[(f32, f32)], closed: bool) -> Vec<((f32, f32), (f32, f32))> {
    let mut out: Vec<_> = pts.windows(2).map(|w| (w[0], w[1])).collect();
    if closed && pts.len() > 2 {
        out.extend(pts.last().copied().zip(pts.first().copied()));
    }
    out
}

/// The view's Linework: each edge it lists drawn in its line style, or not at all, where its element draws a line along it
/// (within a pixel of it, as far as the two overlap); the rest of the element's lines as they were. One pass over the
/// drawing: an element's lines are split along its edges one after another, in the order the view lists them.
fn linework(d: &mut Drawing, m: &Model, items: &[crate::graphics::Linework]) {
    let px_per_mm = sheet::DENSITY as f32 * d.paper;
    let mut edges: std::collections::HashMap<Id, Vec<Edge>> = std::collections::HashMap::new();
    for l in items {
        let Some(id) = m.elements.id(l.element) else { continue };
        let style = l.style.as_ref().and_then(|n| m.line_styles.get(n)).map(|st| {
            let dash = st.pattern.as_ref().and_then(|p| m.line_patterns.get(p)).and_then(|p| p.dash(px_per_mm));
            EdgeStyle {
                pen: st.pen,
                color: Rgb(st.color[0], st.color[1], st.color[2]),
                dash,
                pattern: st.pattern.as_ref().and_then(|n| m.line_patterns.get(n)).filter(|p| !p.is_dash()).cloned(),
            }
        });
        if l.style.is_some() && style.is_none() {
            continue;
        }
        let (a, b) = (pt(d.view.px(l.from)), pt(d.view.px(l.to)));
        let len = a.dist(b);
        if len < 1e-3 {
            continue;
        }
        edges.entry(id).or_default().push(Edge { a, u: (b - a) * (1.0 / len), len, style, k: px_per_mm });
    }
    if edges.is_empty() {
        return;
    }
    let halves = d.take_halves();
    let prims = std::mem::take(&mut d.prims);
    let (owners, parts, pens) = (std::mem::take(&mut d.owners), std::mem::take(&mut d.parts), std::mem::take(&mut d.pens));
    for ((((prim, owner), part), pen), half) in prims.into_iter().zip(owners).zip(parts).zip(pens).zip(halves) {
        let mine = owner.and_then(|id| edges.get(&id)).filter(|_| !matches!(part, Part::Text | Part::Pattern));
        let (Some(mine), Prim::Line { .. }) = (mine, &prim) else {
            keep(d, prim, owner, part, pen, half);
            continue;
        };
        let mut pieces = vec![(prim, pen)];
        for edge in mine {
            pieces = pieces.into_iter().flat_map(|(prim, pen)| along(prim, pen, edge)).collect();
        }
        for (prim, pen) in pieces {
            let pattern = mine
                .iter()
                .rev()
                .find(|edge| {
                    let Prim::Line { pts, .. } = &prim else { return false };
                    pts.iter().all(|&p| {
                        let p = pt(p) - edge.a;
                        p.cross(edge.u).abs() <= 1.0 && p.dot(edge.u) >= -0.01 && p.dot(edge.u) <= edge.len + 0.01
                    })
                })
                .and_then(|edge| {
                    let style = edge.style.as_ref()?;
                    Some((style.pattern.as_ref()?, style.color, edge.k))
                });
            if let (Some((pattern, color, k)), Prim::Line { pts, .. }, Some(pen)) = (pattern, &prim, pen) {
                for graphic in pattern.geometry(pts, k, d.width(pen), color) {
                    keep(d, graphic.prim, owner, Part::LinePattern, graphic.weighted.then_some(pen), half);
                }
            } else {
                keep(d, prim, owner, part, pen, half);
            }
        }
    }
}

/// An edge of Linework in pixels: its start `a`, the way it runs `u` for `len`, and the style it takes (none, unseen).
struct Edge {
    a: P2,
    u: P2,
    len: f64,
    style: Option<EdgeStyle>,
    k: f32,
}

struct EdgeStyle {
    pen: Pen,
    color: Rgb,
    dash: Option<Vec<f32>>,
    pattern: Option<crate::lines::LinePattern>,
}

/// The line `prim`, split where `edge` begins and ends along it: the pieces on the edge in its style (gone, when it is
/// unseen), the rest as they were. A line off the edge comes back whole.
fn along(prim: Prim, pen: Option<Pen>, edge: &Edge) -> Vec<(Prim, Option<Pen>)> {
    let Prim::Line { pts, width, color, dash, closed, alpha } = &prim else { return vec![(prim, pen)] };
    let Edge { a, u, len, style, .. } = edge;
    // Each piece of the line, marked when it lies on the edge.
    let mut runs: Vec<(bool, Vec<(f32, f32)>)> = Vec::new();
    let mut put = |on: bool, p: P2, q: P2| {
        let (p, q) = ((p.0 as f32, p.1 as f32), (q.0 as f32, q.1 as f32));
        match runs.last_mut() {
            Some((was, run)) if *was == on && run.last() == Some(&p) => run.push(q),
            _ => runs.push((on, vec![p, q])),
        }
    };
    for (p, q) in segments(pts, *closed) {
        let (p, q) = (pt(p), pt(q));
        let across = |r: P2| (r - *a).cross(*u).abs();
        let (sp, sq) = ((p - *a).dot(*u), (q - *a).dot(*u));
        let (lo, hi) = (sp.min(sq).max(0.0), sp.max(sq).min(*len));
        if across(p) > 1.0 || across(q) > 1.0 || hi - lo < 0.5 {
            put(false, p, q);
            continue;
        }
        // Along from p: before the edge, on it, after it.
        let at = |s: f64| p + (q - p) * ((s - sp) / (sq - sp));
        let (s0, s1) = if sp <= sq { (lo, hi) } else { (hi, lo) };
        let (x0, x1) = (at(s0), at(s1));
        if p.dist(x0) > 0.01 {
            put(false, p, x0);
        }
        put(true, x0, x1);
        if x1.dist(q) > 0.01 {
            put(false, x1, q);
        }
    }
    if !runs.iter().any(|(on, _)| *on) {
        return vec![(prim, pen)];
    }
    runs.into_iter()
        .filter_map(|(on, run)| match (on, style) {
            (false, _) => {
                Some((Prim::Line { pts: run, width: *width, color: *color, dash: dash.clone(), closed: false, alpha: *alpha }, pen))
            }
            (true, Some(EdgeStyle { pen: p, color: c, dash: k, .. })) => {
                Some((Prim::Line { pts: run, width: *width, color: *c, dash: k.clone(), closed: false, alpha: *alpha }, Some(*p)))
            }
            (true, None) => None,
        })
        .collect()
}

/// `prim` back in the drawing, with its owner, part, pen and halftone.
fn keep(d: &mut Drawing, prim: Prim, owner: Option<Id>, part: Part, pen: Option<Pen>, half: bool) {
    d.prims.push(prim);
    d.owners.push(owner);
    d.parts.push(part);
    d.pens.push(pen);
    if half {
        d.set_half(d.prims.len() - 1);
    }
}

/// A detail group's members where it puts them: its regions under its lines and notes.
pub(crate) fn detail_group(d: &mut Drawing, m: &Model, g: &crate::groups::DetailGroup) {
    let members = crate::groups::placed(m, g);
    for e in &members {
        if let Element::FilledRegion(r) = e {
            filled_region(d, m, r);
        }
    }
    for e in &members {
        match e {
            Element::DetailLine(l) => detail_line(d, m, l),
            Element::Text(n) => note(d, m, n),
            _ => {}
        }
    }
}

/// A text note's type, laid out: what its drawing and a front end's editor read.
pub fn note_layout(m: &Model, n: &Note) -> crate::text::Layout {
    let t = m.text_types.get(&n.type_name).cloned().unwrap_or_default();
    crate::text::layout(&t, &n.text, &crate::text::styles(&t, &n.text, n.override_format, &n.runs))
}

/// Where a note's layout lands in a drawing: the pixel of its top left, the pixels a mm of paper is, and how far it is
/// turned, radians clockwise on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteFrame {
    pub at: (f32, f32),
    pub k: f32,
    pub angle: f32,
}

impl NoteFrame {
    /// Point `(x, y)` of the layout (mm, y down) as a pixel of the drawing.
    pub fn px(&self, (x, y): (f64, f64)) -> (f32, f32) {
        let (sin, cos) = self.angle.sin_cos();
        let (x, y) = (x as f32 * self.k, y as f32 * self.k);
        (self.at.0 + x * cos - y * sin, self.at.1 + x * sin + y * cos)
    }

    /// Pixel `p` of the drawing as a point of the layout.
    pub fn mm(&self, p: (f32, f32)) -> (f64, f64) {
        let (sin, cos) = self.angle.sin_cos();
        let (dx, dy) = (p.0 - self.at.0, p.1 - self.at.1);
        let k = self.k.max(f32::EPSILON);
        (f64::from((dx * cos + dy * sin) / k), f64::from((-dx * sin + dy * cos) / k))
    }
}

impl Drawing {
    /// Where note `n` lands in this drawing.
    pub fn note_frame(&self, n: &Note) -> NoteFrame {
        // Counter-clockwise in the view, clockwise on screen.
        NoteFrame { at: self.view.px(n.at), k: self.px(sheet::DENSITY as f32), angle: -(n.angle.to_radians() as f32) }
    }
}

/// A text note: its lines down from its top left corner in its type's lettering, turned as it is, its border when its
/// type has one, and its leader with an arrow at its end.
pub(crate) fn note(d: &mut Drawing, m: &Model, n: &Note) {
    let t = m.text_types.get(&n.type_name).cloned().unwrap_or_default();
    let layout = note_layout(m, n);
    let frame = d.note_frame(n);
    let color = Rgb(t.color[0], t.color[1], t.color[2]);
    lettering(d, &layout, frame, color);
    let off = if t.border { t.border_offset } else { 0.4 * t.size };
    if t.border {
        let [l, top, r, b] = layout.frame(t.border_offset);
        let ring = [(l, top), (r, top), (r, b), (l, b)].into_iter().map(|p| frame.px(p)).collect();
        d.stroke(ring, t.border_pen, color, None, true);
    }
    if let Some(to) = n.leader {
        // From the side nearer where it points.
        let to = d.view.px(to);
        let right = frame.mm(to).0 > layout.width / 2.0;
        let from = frame.px((if right { layout.width + off } else { -off }, t.size / 2.0));
        d.stroke(vec![from, to], Pen::PROJECTION, INK, None, false);
        arrow(d, from, to);
    }
}

/// `layout` lettered where `frame` puts it, in `color`: a note's text, or a title block's.
pub(crate) fn lettering(d: &mut Drawing, layout: &crate::text::Layout, frame: NoteFrame, color: Rgb) {
    let f = |(x, y): (f32, f32)| frame.px((f64::from(x), f64::from(y)));
    let k = frame.k;
    let part = std::mem::replace(&mut d.part, Part::Text);
    let drawn = layout.drawn();
    for letter in &drawn.fill {
        d.push(Prim::fill(letter.iter().map(|r| r.iter().copied().map(f).collect()).collect(), color));
    }
    for (pts, width) in drawn.strokes {
        d.push(Prim::line(pts.into_iter().map(f).collect(), (width * k).max(1.0), color, None, false));
    }
    for (a, b) in drawn.underlines {
        d.push(Prim::Line {
            pts: vec![frame.px(a), frame.px(b)],
            width: (drawn.underline_width * k).max(1.0),
            color,
            dash: None,
            closed: false,
            alpha: crate::prim::OPAQUE,
        });
    }
    d.part = part;
}

/// Where lettering with its top left at paper point `top_left`, turned `angle` degrees counter-clockwise, lands in `d`.
pub(crate) fn frame_at(d: &Drawing, top_left: P2, angle: f64) -> NoteFrame {
    NoteFrame { at: d.view.px(top_left), k: d.px(sheet::DENSITY as f32), angle: -(angle.to_radians() as f32) }
}

/// A spot elevation's mark at pixel `at`: a triangle standing on the point, and the height it
/// reads over a line beside it, in `units`.
fn spot_mark(d: &mut Drawing, (x, y): (f32, f32), z: Option<f64>, units: Units) {
    let from = d.prims.len();
    d.push(Prim::fill(vec![vec![(x, y), (x + 5.0, y - 8.0), (x - 5.0, y - 8.0)]], INK));
    let text = z.map_or_else(|| "?".into(), |z| height(units, z));
    let w = font::width(&text, 10.0);
    d.stroke(vec![(x - 5.0, y - 8.0), (x + 9.0 + w, y - 8.0)], Pen::PROJECTION, INK, None, false);
    d.text(&text, (x + 7.0 + w / 2.0, y - 16.0), 10.0, INK);
    d.grow(from, (x, y));
}

/// A spot slope: from its point an arrow 24 px long pointing downhill, and the fall over it, as `1.5%`; on the level, `0%`
/// alone; `?` while its host has no top there.
fn slope_mark(d: &mut Drawing, at: P2, read: Option<(f64, Option<P2>)>) {
    let from = d.prims.len();
    let (x, y) = d.view.px(at);
    let text = read.map_or_else(|| "?".into(), |(f, _)| format!("{}%", figure(f)));
    let w = font::width(&text, 10.0);
    match read.and_then(|(_, way)| way) {
        Some(way) => {
            // Downhill on paper: the way in plan, as the view turns it, 24 px long.
            let (fx, fy) = d.view.px(at + way * 1000.0);
            let (dx, dy) = (fx - x, fy - y);
            let n = dx.hypot(dy).max(1e-6);
            let (ux, uy) = (dx / n, dy / n);
            let tip = (x + ux * 24.0, y + uy * 24.0);
            d.stroke(vec![(x, y), tip], Pen::PROJECTION, INK, None, false);
            let back = (tip.0 - ux * 6.0, tip.1 - uy * 6.0);
            let side = (-uy * 3.0, ux * 3.0);
            d.push(Prim::Fill {
                rings: vec![vec![tip, (back.0 + side.0, back.1 + side.1), (back.0 - side.0, back.1 - side.1)]],
                color: INK,
                rule: crate::prim::Rule::NonZero,
                alpha: crate::prim::OPAQUE,
            });
            // Its fall beside the arrow's middle, upright.
            d.text(&text, (x + ux * 12.0 + w / 2.0 + 4.0 * uy.abs(), y + uy * 12.0 - 8.0), 10.0, INK);
        }
        None => d.text(&text, (x + w / 2.0 + 4.0, y - 8.0), 10.0, INK),
    }
    d.grow(from, (x, y));
}

/// A height as a spot elevation reads it, signed: ±0, +3000, -600, +9'-10 1/8".
fn height(units: Units, z: f64) -> String {
    let shown = units.length(z);
    match shown.strip_prefix('-') {
        Some(_) => shown,
        // What rounds to nothing reads as nothing, either side.
        None if units.length(0.0) == shown => format!("±{shown}"),
        None => format!("+{shown}"),
    }
}

/// A tag: a door's mark in a box, a window's in a hexagon, over a paper fill; any other element's label as plain
/// text over a paper fill; with a leader, a line to the point it names.
fn tag(d: &mut Drawing, m: &Model, t: &Tag, rooms: &crate::rooms::Rooms) {
    let Some(anchor) = m.anchor(t.host) else { return };
    let head = anchor + t.offset;
    if t.leader {
        d.line(&[anchor, head], Pen::PROJECTION, INK, false);
    }
    let text = m.tag_text_with(t, rooms);
    let (x, y) = d.view.px(head);
    let from = d.prims.len();
    let (w, h) = ((font::width(&text, 10.0) / 2.0 + 5.0).max(8.0), 8.0);
    let ring = if matches!(m.elements.get(t.host), Some(Element::Window(_))) {
        vec![(x - w - 4.0, y), (x - w, y - h), (x + w, y - h), (x + w + 4.0, y), (x + w, y + h), (x - w, y + h)]
    } else {
        vec![(x - w, y - h), (x + w, y - h), (x + w, y + h), (x - w, y + h)]
    };
    d.push(Prim::fill(vec![ring.clone()], PAPER));
    // Openings' marks sit in a box or a hexagon; anything else's label is plain text.
    if matches!(m.elements.get(t.host), Some(Element::Door(_) | Element::Window(_))) {
        d.stroke(ring, Pen::PROJECTION, INK, None, true);
    }
    d.text(&text, (x, y), 10.0, INK);
    d.grow(from, (x, y));
}

/// The way a section or elevation looks, on screen (y down).
fn looking_px(v: &Section) -> (f32, f32) {
    let l = v.looking();
    (l.0 as f32, -l.1 as f32)
}

/// A section's line in plan, dashed, with its head at its start (a flag pointing the way it
/// looks, its name behind it) and a tick at its end.
fn section_mark(d: &mut Drawing, v: &Section) {
    let (a, b) = (d.view.px(v.start), d.view.px(v.end));
    d.stroke(vec![a, b], Pen::MEDIUM, INK, Some(vec![18.0, 5.0]), false);
    let (lx, ly) = looking_px(v);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let from = d.prims.len();
    d.push(Prim::fill(vec![vec![a, (a.0 + ux * 12.0, a.1 + uy * 12.0), (a.0 + lx * 12.0, a.1 + ly * 12.0)]], INK));
    let back = font::width(&v.name, 10.0) / 2.0 + 8.0;
    d.text(&v.name, (a.0 - ux * back, a.1 - uy * back), 10.0, INK);
    d.grow(from, a);
    let from = d.prims.len();
    d.stroke(vec![b, (b.0 + lx * 9.0, b.1 + ly * 9.0)], Pen::MEDIUM, INK, None, false);
    d.grow(from, b);
}

/// An elevation's marker in plan, at its line's middle: a circle with a flag pointing the way it
/// looks, and its name behind it.
fn elevation_mark(d: &mut Drawing, v: &Section) {
    const R: f32 = 9.0;
    let c = d.view.px(v.start + (v.end - v.start) * 0.5);
    let from = d.prims.len();
    let ring: Vec<(f32, f32)> = (0..32)
        .map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + R * t.cos() as f32, c.1 + R * t.sin() as f32)
        })
        .collect();
    d.push(Prim::fill(vec![ring.clone()], PAPER));
    d.stroke(ring, Pen::BUBBLE, INK, None, true);
    let (lx, ly) = looking_px(v);
    let side = (-ly * R * 0.8, lx * R * 0.8);
    let flag = vec![(c.0 + side.0, c.1 + side.1), (c.0 + lx * (R + 7.0), c.1 + ly * (R + 7.0)), (c.0 - side.0, c.1 - side.1)];
    d.push(Prim::fill(vec![flag], INK));
    let back = R + 6.0 + font::width(&v.name, 10.0) / 2.0 * lx.abs() + 6.0 * ly.abs();
    d.text(&v.name, (c.0 - lx * back, c.1 - ly * back), 10.0, INK);
    d.grow(from, c);
}

/// A grid line, dashed, with a bubble holding its name beyond each end.
fn grid(d: &mut Drawing, start: P2, end: P2, name: &str) {
    let (a, b) = (d.view.px(start), d.view.px(end));
    d.stroke(vec![a, b], Pen::PROJECTION, GRID, Some(vec![14.0, 4.0]), false);
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let dir = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    for ((x, y), k) in [(a, -BUBBLE), (b, BUBBLE)] {
        let from = d.prims.len();
        let c = (x + dir.0 * k, y + dir.1 * k);
        let ring = (0..32).map(|i| {
            let t = f64::from(i) * PI / 16.0;
            (c.0 + BUBBLE * t.cos() as f32, c.1 + BUBBLE * t.sin() as f32)
        });
        d.stroke(ring.collect(), Pen::BUBBLE, GRID, None, true);
        d.text(name, c, 10.0, GRID);
        d.grow(from, (x, y));
    }
}

/// The cameras of the 3D views `shown` (those selected: Revit shows a camera on a plan only then, or with Show Camera) that
/// stand on plan `plan`'s level, as `view` draws it: each one's symbol at its eye, its line to what it looks at and its name,
/// the view as each one's owner, for a front end to draw over the plan and pick.
pub fn camera_marks(m: &Model, plan: impl Key, view: View, shown: &[Id]) -> Drawing {
    let level = m.plan_level(plan).map_or_else(|| m.elements.id(plan), Some);
    let n = scale_of(m, plan);
    let mut d = Drawing::new(view);
    (d.k, d.paper, d.weights) = (annotation(m, plan, view), paper_of(n, view.scale), m.line_weights().at(n));
    for id in shown {
        if let Some(Element::View3D(v)) = m.elements.get(*id)
            && v.camera.is_some_and(|c| m.camera_level(c.eye[2]) == level)
        {
            d.owner = Some(*id);
            camera_mark(&mut d, v);
        }
    }
    d.owner = None;
    d.weigh(|_| true);
    d
}

/// A camera's symbol in plan, at its eye: a little body pointing the way it looks, a line to what it looks at, and its name
/// behind it. Drawn as a section's or an elevation's mark is (the symbol grows with the view's annotation).
fn camera_mark(d: &mut Drawing, v: &View3D) {
    let Some(cam) = v.camera else { return };
    let a = d.view.px(P2(cam.eye[0], cam.eye[1]));
    let b = d.view.px(P2(cam.target[0], cam.target[1]));
    // The line to what it looks at, dashed, at the model's size.
    d.stroke(vec![a, b], Pen::PROJECTION, INK, Some(vec![12.0, 6.0]), false);
    // The symbol, drawn at its own size then grown as the view's annotation is.
    let len = (b.0 - a.0).hypot(b.1 - a.1).max(1e-3);
    let (ux, uy) = ((b.0 - a.0) / len, (b.1 - a.1) / len);
    let (px, py) = (-uy, ux);
    let from = d.prims.len();
    // The body: a small box square to the line, centred a little behind the eye.
    let (back, half, depth) = (-7.0f32, 9.0f32, 8.0f32);
    let c = (a.0 + ux * back, a.1 + uy * back);
    let body = vec![
        (c.0 - ux * depth + px * half, c.1 - uy * depth + py * half),
        (c.0 + ux * depth + px * half, c.1 + uy * depth + py * half),
        (c.0 + ux * depth - px * half, c.1 + uy * depth - py * half),
        (c.0 - ux * depth - px * half, c.1 - uy * depth - py * half),
    ];
    d.push(Prim::fill(vec![body.clone()], PAPER));
    d.stroke(body, Pen::BUBBLE, INK, None, true);
    // The lens: a small dark circle at the front, toward the target.
    let lc = (c.0 + ux * (depth + 3.0), c.1 + uy * (depth + 3.0));
    let ring: Vec<(f32, f32)> = (0..16)
        .map(|i| (lc.0 + 3.5 * (f64::from(i) * PI / 8.0).cos() as f32, lc.1 + 3.5 * (f64::from(i) * PI / 8.0).sin() as f32))
        .collect();
    d.push(Prim::fill(vec![ring], INK));
    // Its name behind the eye, against the way it looks.
    let name_at = (a.0 - ux * (14.0 + font::width(&v.name, 10.0) / 2.0), a.1 - uy * (14.0 + font::width(&v.name, 10.0) / 2.0));
    d.text(&v.name, name_at, 10.0, INK);
    d.grow(from, a);
}

/// A camera seen in plan as Revit shows it while its view is selected (Show Camera): its eye, its target, and its view cone,
/// the field of view's two sides out to the far clip plane and the far clip line across them. Each point is a grip: the eye
/// moves the camera (the target stays), the target aims it, the far line's middle sets the far clip, a side's middle the
/// field of view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cone {
    pub eye: P2,
    pub target: P2,
    /// The far clip line's ends, left and right as the camera looks, and its middle.
    pub far_left: P2,
    pub far_right: P2,
    pub far: P2,
    /// Each side's middle, left then right: the field of view's grips.
    pub sides: [P2; 2],
    /// How far ahead of the eye the far line is in plan, mm.
    pub reach: f64,
    /// Whether the far line clips (Clip Manually on); else it only shows the cone going on past the target.
    pub clipped: bool,
}

/// The plan's unit way from the eye toward the target, and how steeply it looks (the cosine of its pitch): none for a camera
/// looking straight up or down.
fn plan_way(cam: &crate::model::ViewCamera) -> Option<(P2, f64)> {
    let d = P2(cam.target[0] - cam.eye[0], cam.target[1] - cam.eye[1]);
    let across = d.0.hypot(d.1);
    let whole = cam.target_distance();
    (across > TOL && whole > TOL).then(|| (P2(d.0 / across, d.1 / across), across / whole))
}

/// The cone of 3D view `v`'s camera in plan: out to its far clip plane while Clip Manually is on, else a quarter past its
/// target (so the far line's grip stands clear of the target's). None for a view without a camera, or
/// one looking straight down.
pub fn camera_cone(v: &View3D) -> Option<Cone> {
    let cam = v.camera?;
    let lens = v.perspective.map_or(crate::camera::LENS, |p| p.lens);
    cone_of(&cam, lens, v.lens().is_some())
}

/// [`camera_cone`] for camera `cam` through focal length `lens`, in `perspective` or orthographic. Its far clip (in
/// perspective only, which alone clips) is a plane square to the way the picture looks: with the tilt corrected, the way
/// turned by it (upright for a two-point perspective), so the cone reaches as far on the plan as the plane cuts.
pub fn cone_of(cam: &crate::model::ViewCamera, lens: f64, perspective: bool) -> Option<Cone> {
    let (way, cos) = plan_way(cam)?;
    let eye = P2(cam.eye[0], cam.eye[1]);
    let target = P2(cam.target[0], cam.target[1]);
    let clipped = cam.physical.clip_on && perspective;
    let pitch = cos.clamp(-1.0, 1.0).acos().copysign(cam.target[2] - cam.eye[2]) - cam.vertical_tilt().atan();
    let reach = if clipped { cam.physical.far * pitch.clamp(-1.55, 1.55).cos() } else { eye.dist(target) * 1.25 };
    let half = (crate::camera::field_of_view(lens, &cam.physical) / 2.0).to_radians();
    let side = |k: f64| {
        let (c, s) = (half.cos(), half.sin() * k);
        let dir = P2(way.0 * c - way.1 * s, way.0 * s + way.1 * c);
        eye + dir * (reach / half.cos().max(1e-6))
    };
    let (far_left, far_right) = (side(1.0), side(-1.0));
    let far = eye + way * reach;
    let mid = |a: P2| eye + (a - eye) * 0.5;
    Some(Cone { eye, target, far_left, far_right, far, sides: [mid(far_left), mid(far_right)], reach, clipped })
}

/// Where a camera's grip, dragged to plan point `to`, puts it, as the `camera` command's params: the eye (`which` 0, its
/// height kept, the target staying), the target (1), the far clip (2: Clip Manually on, as far along the way it looks as `to`
/// is), a side (3 or 4: the field of view twice the angle `to` makes with the way it looks). None where it would put nothing.
pub fn camera_grip(cam: &crate::model::ViewCamera, which: usize, to: P2) -> Option<serde_json::Value> {
    let (way, cos) = plan_way(cam)?;
    let eye = P2(cam.eye[0], cam.eye[1]);
    match which {
        0 => Some(serde_json::json!({"eye": [to.0, to.1, cam.eye[2]]})),
        1 => (to.dist(eye) > TOL).then(|| serde_json::json!({"target": [to.0, to.1, cam.target[2]]})),
        2 => {
            let along = (to - eye).dot(way);
            (along > TOL).then(|| serde_json::json!({"clip": true, "far": (along / cos.max(1e-6)).max(cam.physical.near + 1.0)}))
        }
        3 | 4 => {
            let d = to - eye;
            let along = d.dot(way);
            let off = (way.0 * d.1 - way.1 * d.0).abs();
            // Within what a camera's lens sees ([`fov_limits`](crate::camera::fov_limits)), as far as the drag goes.
            let (narrow, wide) = crate::camera::fov_limits();
            (along > TOL).then(|| serde_json::json!({"fov": (2.0 * off.atan2(along).to_degrees()).clamp(narrow + 1e-3, wide - 1e-3)}))
        }
        _ => None,
    }
}

/// The camera's cone over a plan drawn through `v`: its sides and far line, the line to its target, and a grip at each
/// point, as [`Cone`] lists them; for a front end to draw in its own colours.
pub fn cone_lines(c: &Cone) -> [[P2; 2]; 4] {
    [[c.eye, c.far_left], [c.eye, c.far_right], [c.far_left, c.far_right], [c.eye, c.target]]
}

/// The eye of the nearest camera whose mark (body, lens or name) the cursor at pixel `at` is over, so that a click on
/// the mark lands on the camera's eye, never on a corner of its symbol. None off a camera's mark. The body, lens and
/// name sizes are those `render::camera_mark` draws with.
pub fn camera_at(m: &Model, level: Id, v: &View, at: (f32, f32), shown: &[Id]) -> Option<(Id, P2)> {
    let (x, y) = at;
    let plan = m.plan_level(level).unwrap_or(level);
    shown
        .iter()
        .filter_map(|&id| {
            let Some(Element::View3D(view)) = m.elements.get(id) else { return None };
            let c = view.camera?;
            if m.camera_level(c.eye[2]) != Some(plan) {
                return None;
            }
            let eye = P2(c.eye[0], c.eye[1]);
            let target = P2(c.target[0], c.target[1]);
            let a = v.px(eye);
            let b = v.px(target);
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let len = (dx * dx + dy * dy).sqrt();
            let (ux, uy) = if len > 1e-6 { (dx / len, dy / len) } else { (1.0, 0.0) };
            let (px, py) = (-uy, ux);
            let (back, half, depth) = (-7.0, 9.0, 8.0);
            let c = (a.0 + ux * back, a.1 + uy * back);
            let body = [
                (c.0 - ux * depth + px * half, c.1 - uy * depth + py * half),
                (c.0 + ux * depth + px * half, c.1 + uy * depth + py * half),
                (c.0 + ux * depth - px * half, c.1 + uy * depth - py * half),
                (c.0 - ux * depth - px * half, c.1 - uy * depth - py * half),
            ];
            let lens = (c.0 + ux * (depth + 3.0), c.1 + uy * (depth + 3.0));
            let w = font::width(&view.name, 10.0);
            let name = (a.0 - ux * (14.0 + w / 2.0), a.1 - uy * (14.0 + w / 2.0));
            let on_body = inside_polygon((x, y), &body);
            let on_lens = (x - lens.0).powi(2) + (y - lens.1).powi(2) <= 5.5f32.powi(2);
            let on_name = (x - name.0).abs() <= w / 2.0 + 2.0 && (y - name.1).abs() <= 7.0;
            (on_body || on_lens || on_name).then_some(((x - a.0).powi(2) + (y - a.1).powi(2), id, eye))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id, eye)| (id, eye))
}

/// Whether `p` lies inside `polygon`, by the even-odd rule. A camera's mark is a turned box, so its corners are not
/// axis-aligned.
fn inside_polygon(p: (f32, f32), polygon: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = polygon.len().saturating_sub(1);
    for i in 0..polygon.len() {
        let (xi, yi) = polygon[i];
        let (xj, yj) = polygon[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

/// A dash's lengths, a space between each: as SVG's `stroke-dasharray` and PDF's `d` take them.
pub(crate) fn spaced(dash: &[f32]) -> String {
    dash.iter().map(f32::to_string).collect::<Vec<_>>().join(" ")
}

/// SVG path data for a polyline or ring.
fn path_data(pts: &[(f32, f32)], closed: bool) -> String {
    let mut s = String::new();
    for (i, (x, y)) in pts.iter().enumerate() {
        let _ = write!(s, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { " L" });
    }
    if closed {
        s.push_str(" Z");
    }
    s
}

pub fn svg(d: &Drawing) -> String {
    let (w, h) = (d.view.width, d.view.height);
    let mut s = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><rect width="{w}" height="{h}" fill="{}"/>"#,
        hex(PAPER)
    );
    for (i, p) in d.prims.iter().enumerate() {
        let _ = match p {
            Prim::Fill { rings, color, rule, alpha } => {
                let data: Vec<String> = rings.iter().map(|r| path_data(r, true)).collect();
                let rule = if *rule == Rule::EvenOdd { r#" fill-rule="evenodd""# } else { "" };
                write!(s, r#"<path d="{}" fill="{}"{rule}{}/>"#, data.join(" "), hex(*color), opacity("fill", *alpha))
            }
            Prim::Line { pts, width, color, dash, closed, alpha } => {
                let dash = dash.as_ref().map_or_else(String::new, |d| format!(r#" stroke-dasharray="{}""#, spaced(d)));
                // A hairline (pen 0) is one pixel of whatever shows the SVG, at any zoom.
                let width = if *width > 0.0 { format!(r#"{width}""#) } else { r#"1" vector-effect="non-scaling-stroke""#.to_string() };
                write!(
                    s,
                    r#"<path d="{}" fill="none" stroke="{}" stroke-width="{width} stroke-linecap="round" stroke-linejoin="{}"{dash}{}/>"#,
                    path_data(pts, *closed),
                    hex(*color),
                    if d.mitred(i) { "miter" } else { "round" },
                    opacity("stroke", *alpha)
                )
            }
            Prim::Image { at, width, height, rgba, .. } => match picture(*width, *height, rgba).and_then(|pm| pm.encode_png().ok()) {
                Some(png) => write!(
                    s,
                    r#"<image x="{}" y="{}" width="{width}" height="{height}" href="data:image/png;base64,{}"/>"#,
                    at.0,
                    at.1,
                    crate::files::base64(&png)
                ),
                None => Ok(()),
            },
        };
    }
    s.push_str("</svg>\n");
    s
}

/// An SVG paint's opacity attribute (`fill` or `stroke`), when it isn't opaque.
fn opacity(paint: &str, alpha: u8) -> String {
    if alpha == OPAQUE { String::new() } else { format!(r#" {paint}-opacity="{:.3}""#, f32::from(alpha) / 255.0) }
}

fn path(rings: &[&[(f32, f32)]], closed: bool) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for ring in rings {
        for (i, &(x, y)) in ring.iter().enumerate() {
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        if closed {
            pb.close();
        }
    }
    pb.finish()
}

pub fn png(d: &Drawing) -> Result<Vec<u8>, String> {
    let mut pm = Pixmap::new(d.view.width, d.view.height).ok_or("the image size is out of range")?;
    pm.fill(Color::from_rgba8(PAPER.0, PAPER.1, PAPER.2, 255));
    paint(&mut pm, d);
    pm.encode_png().map_err(|e| e.to_string())
}

/// A picture's pixels (each its colour and how opaque it is, as a front end takes them) as a pixmap, whose colours are
/// times how opaque they are.
fn picture(width: u32, height: u32, rgba: &[u8]) -> Option<Pixmap> {
    let mut held = rgba.to_vec();
    for px in held.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        if a < 255 {
            for c in &mut px[..3] {
                *c = (u16::from(*c) * a / 255) as u8;
            }
        }
    }
    Pixmap::from_vec(held, IntSize::from_wh(width, height)?)
}

/// The display list drawn over what `pm` holds.
fn paint(pm: &mut Pixmap, d: &Drawing) {
    for (i, p) in d.prims.iter().enumerate() {
        let mut paint = Paint { anti_alias: true, ..Paint::default() };
        match p {
            Prim::Fill { rings, color, rule, alpha } => {
                paint.set_color_rgba8(color.0, color.1, color.2, *alpha);
                let rings: Vec<&[(f32, f32)]> = rings.iter().map(Vec::as_slice).collect();
                let rule = if *rule == Rule::EvenOdd { FillRule::EvenOdd } else { FillRule::Winding };
                if let Some(path) = path(&rings, true) {
                    pm.fill_path(&path, &paint, rule, Transform::identity(), None);
                }
            }
            Prim::Line { pts, width, color, dash, closed, alpha } => {
                paint.set_color_rgba8(color.0, color.1, color.2, *alpha);
                let stroke = Stroke {
                    width: *width,
                    line_cap: LineCap::Round,
                    line_join: if d.mitred(i) { LineJoin::Miter } else { LineJoin::Round },
                    dash: dash.clone().and_then(|d| StrokeDash::new(d, 0.0)),
                    ..Stroke::default()
                };
                if let Some(path) = path(&[pts], *closed) {
                    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                }
            }
            Prim::Image { at, width, height, rgba, .. } => {
                if let Some(img) = picture(*width, *height, rgba) {
                    pm.draw_pixmap(
                        at.0.round() as i32,
                        at.1.round() as i32,
                        img.as_ref(),
                        &PixmapPaint::default(),
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;

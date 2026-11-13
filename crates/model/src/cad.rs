//! CAD drawings brought into a project: Revit's Import CAD and Link CAD. A project holds each drawing once, as a [`Cad`]: the
//! DXF it was read from, kept whole (deflated, in base64) so a file holds what the user brought in and reads it again as this
//! build does, and the drawing [`crate::dxf`] reads of it. A `CadImport` element places one on a level, in every plan of it or
//! in one view only. A linked one remembers its file, and `reload_cad` (and opening the project) reads it again.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::dxf::{self, Color, Dxf, Entity, Shape};

pub mod outlines;
pub mod packed;
use crate::geom::P2;
use crate::pen::Pen;
use crate::prim::Rgb;

/// What reads a DWG drawing for a session: the DXF the file at a path becomes, and notes on what the read left out, or why it
/// can't be read. The core reads no DWG of its own; the apps hand their `Session` one (`bimcraft-dwg`, which reads it in a
/// child process), and a session without one says to save the drawing as DXF.
pub type DwgReader = Arc<dyn Fn(&std::path::Path) -> Result<(Vec<u8>, Vec<String>), String> + Send + Sync>;

/// Drawings one project may hold.
pub const MAX_CADS: usize = 64;
/// What a project's drawings may hold among them, bytes of DXF.
pub const MAX_HELD: usize = 512 << 20;
/// Entities a drawing needs before any of it counts as far from the rest: a sparse one is all bulk.
const BULK_FROM: usize = 20;
/// How many times the middle of a drawing's span an entity may lie beyond it before it's far from the rest.
const FAR: f64 = 20.0;

/// How an import's colours are drawn: as the drawing has them, inverted (light on dark becomes dark on light), or all in the
/// view's ink.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colors {
    #[default]
    Preserve,
    Invert,
    Black,
}

impl Colors {
    pub fn from_name(name: &str) -> Option<Colors> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "preserve" => Colors::Preserve,
            "invert" => Colors::Invert,
            "black" | "black and white" | "black_and_white" => Colors::Black,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Colors::Preserve => "preserve",
            Colors::Invert => "invert",
            Colors::Black => "black",
        }
    }
}

/// A drawing as a project holds it. Cheap to clone: the drawing is shared.
#[derive(Clone)]
pub struct Cad {
    /// The file's name, for people to know it by.
    source: String,
    /// Where it was read from, when it's linked: `reload_cad`, and opening the project, read it again from there.
    path: Option<String>,
    bytes: Arc<Vec<u8>>,
    /// Whether `bytes` are a page's lines packed ([`packed`]), not a DXF.
    lines: bool,
    dxf: Arc<Dxf>,
    /// The box round its bulk, drawing units: all of it but what lies far from the rest.
    bulk: Option<[P2; 2]>,
    /// How many of its entities lie far from the rest.
    far: usize,
    /// What a project file holds of it, made the first time it's saved.
    packed: Arc<OnceLock<CadFile>>,
    /// Its entities' boxes and a grid of them, made the first time what lies in a box is asked.
    index: Arc<OnceLock<Index>>,
}

/// A drawing's entities' boxes (x0, y0, x1, y1, drawing units), and a grid over them: each cell the entities whose boxes meet
/// it, so what lies in a view or under the cursor is found among a few, not looked for among a dense sheet's hundreds of
/// thousands. One that spans many cells is in none, and always looked at.
struct Index {
    boxes: Vec<[f64; 4]>,
    lo: P2,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    wide: Vec<u32>,
}

/// The most cells one entity is put in: past it, it is a wide one.
const MOST_CELLS: usize = 64;

impl Index {
    fn new(d: &Dxf) -> Index {
        let boxes: Vec<[f64; 4]> = d
            .entities
            .iter()
            .map(|e| {
                let [lo, hi] = e.shape.bounds();
                [lo.0, lo.1, hi.0, hi.1]
            })
            .collect();
        let finite = |b: &&[f64; 4]| b.iter().all(|v| v.is_finite());
        let all = boxes.iter().filter(finite).fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |a, b| {
            [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]
        });
        let (w, h) = ((all[2] - all[0]).max(1e-9), (all[3] - all[1]).max(1e-9));
        let want = (boxes.len() / 8).clamp(1, 1 << 20) as f64;
        let cell = ((w * h) / want).sqrt().max(w.max(h) / 4096.0).max(1e-9);
        let (nx, ny) = (((w / cell).ceil() as usize).clamp(1, 4096), ((h / cell).ceil() as usize).clamp(1, 4096));
        let lo = if all[0].is_finite() { P2(all[0], all[1]) } else { P2(0.0, 0.0) };
        let mut index = Index { boxes: Vec::new(), lo, cell, nx, ny, cells: vec![Vec::new(); nx * ny], wide: Vec::new() };
        for (i, b) in boxes.iter().enumerate() {
            let Some([x0, y0, x1, y1]) = index.span(b) else {
                index.wide.push(i as u32);
                continue;
            };
            if (x1 - x0 + 1) * (y1 - y0 + 1) > MOST_CELLS {
                index.wide.push(i as u32);
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(c) = index.cells.get_mut(y * nx + x) {
                        c.push(i as u32);
                    }
                }
            }
        }
        index.boxes = boxes;
        index
    }

    /// The cells box `b` covers, first and last along each axis; none for a box that isn't a number's.
    fn span(&self, b: &[f64; 4]) -> Option<[usize; 4]> {
        if !b.iter().all(|v| v.is_finite()) {
            return None;
        }
        let at = |v: f64, o: f64, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        Some([at(b[0], self.lo.0, self.nx), at(b[1], self.lo.1, self.ny), at(b[2], self.lo.0, self.nx), at(b[3], self.lo.1, self.ny)])
    }

    /// The entities whose boxes meet `b`, in the drawing's order.
    fn within(&self, b: [f64; 4]) -> Vec<usize> {
        let meets = |i: usize| self.boxes.get(i).is_some_and(|e| e[0] <= b[2] && e[2] >= b[0] && e[1] <= b[3] && e[3] >= b[1]);
        let span = self.span(&b);
        let cells = span.map_or(usize::MAX, |[x0, y0, x1, y1]| (x1 - x0 + 1) * (y1 - y0 + 1));
        // Most of it: each entity looked at once is fewer steps than the cells' lists.
        if cells * 2 >= self.nx * self.ny {
            return (0..self.boxes.len()).filter(|i| meets(*i)).collect();
        }
        let Some([x0, y0, x1, y1]) = span else { return Vec::new() };
        let mut found: Vec<u32> = self.wide.clone();
        for y in y0..=y1 {
            for x in x0..=x1 {
                found.extend(self.cells.get(y * self.nx + x).into_iter().flatten());
            }
        }
        found.sort_unstable();
        found.dedup();
        found.into_iter().map(|i| i as usize).filter(|i| meets(*i)).collect()
    }
}

impl std::fmt::Debug for Cad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cad({:?}, {} entities)", self.source, self.dxf.entities.len())
    }
}

impl PartialEq for Cad {
    fn eq(&self, other: &Cad) -> bool {
        self.source == other.source
            && self.path == other.path
            && self.lines == other.lines
            && (Arc::ptr_eq(&self.bytes, &other.bytes) || self.bytes == other.bytes)
    }
}

impl Cad {
    /// The drawing a DXF file's `bytes` hold, known by `source`, linked to `path` if it's given; or why it can't be held.
    pub fn new(source: &str, path: Option<String>, bytes: Vec<u8>) -> Result<Cad, String> {
        let dxf = dxf::read(&bytes)?;
        if dxf.entities.is_empty() {
            return Err("it draws nothing in model space".into());
        }
        let (bulk, far) = bulk(&dxf);
        Ok(Cad {
            source: source.chars().take(crate::model::MAX_TEXT).collect(),
            path: path.map(|p| p.chars().take(4096).collect()),
            bytes: Arc::new(bytes),
            lines: false,
            dxf: Arc::new(dxf),
            bulk,
            far,
            packed: Arc::default(),
            index: Arc::default(),
        })
    }

    /// A page's lines a PDF's [`packed`] `bytes` hold, known by `source`; or why they can't be held.
    pub fn of_lines(source: &str, bytes: Vec<u8>) -> Result<Cad, String> {
        let dxf = packed::unpack(&bytes)?;
        Ok(Cad::drawn(source, dxf, bytes))
    }

    /// Drawing `dxf`, read from a page (never empty: a page that draws nothing has no lines), held packed.
    pub fn of_drawing(source: &str, dxf: Dxf) -> Cad {
        let bytes = packed::pack(&dxf);
        Cad::drawn(source, dxf, bytes)
    }

    fn drawn(source: &str, dxf: Dxf, bytes: Vec<u8>) -> Cad {
        let (bulk, far) = bulk(&dxf);
        Cad {
            source: source.chars().take(crate::model::MAX_TEXT).collect(),
            path: None,
            bytes: Arc::new(bytes),
            lines: true,
            dxf: Arc::new(dxf),
            bulk,
            far,
            packed: Arc::default(),
            index: Arc::default(),
        }
    }

    /// The entities whose boxes meet the box from `lo` to `hi` (drawing units), by their place in its entities, in its order:
    /// what a view or the cursor's reach finds of it.
    pub fn entities_in(&self, lo: P2, hi: P2) -> Vec<usize> {
        self.index.get_or_init(|| Index::new(&self.dxf)).within([lo.0, lo.1, hi.0, hi.1])
    }

    /// Entity `i`'s box, drawing units, its low corner and its high.
    pub fn entity_box(&self, i: usize) -> Option<[P2; 2]> {
        let b = self.index.get_or_init(|| Index::new(&self.dxf)).boxes.get(i).copied()?;
        Some([P2(b[0], b[1]), P2(b[2], b[3])])
    }

    /// The bytes it holds: its DXF, or its packed lines.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The file it's linked to, if it is.
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The same drawing, linked to `path` or not.
    pub fn linked(mut self, path: Option<String>) -> Cad {
        self.path = path;
        self.packed = Arc::default();
        self
    }

    pub fn dxf(&self) -> &Dxf {
        &self.dxf
    }

    /// Bytes of DXF it holds.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether `bytes` are what it was read from.
    pub fn same_bytes(&self, bytes: &[u8]) -> bool {
        self.bytes.as_slice() == bytes
    }

    /// The box round its bulk, drawing units: what centring it and fitting a view to it go by.
    pub fn bulk(&self) -> Option<[P2; 2]> {
        self.bulk
    }

    /// How many of its entities lie far from the rest: the stray lines that make a drawing's extents huge.
    pub fn far(&self) -> usize {
        self.far
    }

    fn file(&self) -> &CadFile {
        self.packed.get_or_init(|| {
            let held = crate::files::base64(&crate::cloud::deflate(&self.bytes));
            let (dxf, lines) = if self.lines { (String::new(), Some(held)) } else { (held, None) };
            CadFile { source: self.source.clone(), path: self.path.clone(), dxf, lines }
        })
    }

    /// The colour an entity is drawn in when `colors` says how, or none for the view's ink: a colour by layer is its layer's,
    /// by block outside a block the ink's, index 7 the ink.
    pub fn color(&self, e: &Entity, colors: Colors) -> Option<Rgb> {
        let own = match e.color {
            Color::ByLayer => self.dxf.layer(&e.layer).map_or(Color::Index(7), |l| l.color),
            c => c,
        };
        let rgb = own.rgb()?;
        match colors {
            Colors::Preserve => Some(rgb),
            Colors::Invert => Some(Rgb(255 - rgb.0, 255 - rgb.1, 255 - rgb.2)),
            Colors::Black => None,
        }
    }

    /// The pen an entity is drawn with: its weight's, else its layer's, else the finest.
    pub fn pen(&self, e: &Entity) -> Pen {
        e.weight.or_else(|| self.dxf.layer(&e.layer).and_then(|l| l.weight)).filter(|w| *w > 0.0).map_or(Pen::FINE, Pen::nearest)
    }

    /// An entity's dashes, drawing units, when its linetype (its own, else its layer's) has gaps: on, off, on, …, all of them
    /// (a dash-dot's dash, gap, dot and gap), times `ltscale`. A dot is drawn as a short dash, a quarter of the shortest gap;
    /// pieces of one kind side by side are one, and a pattern that starts with a gap or ends with a dash starts with a dash of
    /// nothing or ends with a gap of nothing, so the list is on and off in turn from its start.
    pub fn dash(&self, e: &Entity) -> Option<Vec<f64>> {
        let name = e.linetype.as_deref().or_else(|| self.dxf.layer(&e.layer).map(|l| l.linetype.as_str()))?;
        let lt = self.dxf.linetypes.iter().find(|l| l.name.eq_ignore_ascii_case(name))?;
        let gap = lt.dashes.iter().copied().filter(|d| *d < 0.0).map(f64::abs).fold(f64::INFINITY, f64::min);
        if !gap.is_finite() {
            return None;
        }
        let mut out: Vec<(bool, f64)> = Vec::new();
        for d in lt.dashes.iter().take(64) {
            let (on, len) = if *d < 0.0 {
                (false, -d)
            } else if *d == 0.0 {
                (true, gap / 4.0)
            } else {
                (true, *d)
            };
            match out.last_mut() {
                Some(last) if last.0 == on => last.1 += len,
                _ => out.push((on, len)),
            }
        }
        if out.first().is_some_and(|p| !p.0) {
            out.insert(0, (true, 0.0));
        }
        if out.last().is_some_and(|p| p.0) {
            out.push((false, 0.0));
        }
        let k = self.dxf.ltscale;
        Some(out.into_iter().map(|(_, len)| len * k).collect())
    }

    /// Whether an entity is drawn when the layers `hidden` are.
    pub fn shows(&self, e: &Entity, hidden: &BTreeSet<String>) -> bool {
        !hidden.contains(&e.layer)
    }
}

/// What a shape draws as lines, drawing units: each run of points, and whether it closes. A circle or an arc is points on it; a
/// point and a text draw none.
pub fn paths(shape: &Shape) -> Vec<(Vec<P2>, bool)> {
    match shape {
        Shape::Line(a, b) => vec![(vec![*a, *b], false)],
        Shape::Polyline { pts, closed } => vec![(pts.clone(), *closed)],
        Shape::Circle { c, r } => {
            let mut pts = dxf::arc_points(*c, *r, 0.0, 360.0);
            pts.pop();
            vec![(pts, true)]
        }
        Shape::Arc { c, r, start, end } => vec![(dxf::arc_points(*c, *r, *start, end - start), false)],
        Shape::Fill { rings, .. } => rings.iter().map(|r| (r.clone(), true)).collect(),
        Shape::Point(_) | Shape::Text { .. } => Vec::new(),
    }
}

/// `shape` through a similarity: `f` takes its points, which turns them `turn` degrees and scales them `k` times; circles stay
/// circles, an arc's start comes into [0, 360) with its sweep kept, a text's angle turns.
fn similar(shape: &Shape, f: impl Fn(P2) -> P2, turn: f64, k: f64) -> Shape {
    let ring = |r: &Vec<P2>| r.iter().map(|p| f(*p)).collect::<Vec<P2>>();
    match shape {
        Shape::Line(a, b) => Shape::Line(f(*a), f(*b)),
        Shape::Polyline { pts, closed } => Shape::Polyline { pts: ring(pts), closed: *closed },
        Shape::Circle { c, r } => Shape::Circle { c: f(*c), r: r * k },
        Shape::Arc { c, r, start, end } => {
            let s = (start + turn).rem_euclid(360.0);
            Shape::Arc { c: f(*c), r: r * k, start: s, end: s + (end - start) }
        }
        Shape::Point(p) => Shape::Point(f(*p)),
        Shape::Text { at, height, angle, text, align, tag, wide, mirrored } => Shape::Text {
            at: f(*at),
            height: height * k,
            angle: (angle + turn).rem_euclid(360.0),
            text: text.clone(),
            align: *align,
            tag: tag.clone(),
            wide: *wide,
            mirrored: *mirrored,
        },
        Shape::Fill { rings, solid } => Shape::Fill { rings: rings.iter().map(ring).collect(), solid: *solid },
    }
}

/// A shape of a placed drawing as it lies in the model, mm: through [`crate::model::cad_place`], turned by the import's angle.
/// A stretched one (an image's lines) is no similarity: its circles and arcs come out as the polylines they become.
pub fn placed(shape: &Shape, ci: &crate::model::CadImport) -> Shape {
    if ci.stretch == 1.0 {
        return similar(shape, crate::model::cad_place(ci), ci.angle, ci.scale);
    }
    let f = crate::model::cad_place(ci);
    match shape {
        Shape::Circle { .. } | Shape::Arc { .. } => {
            let mut runs = paths(shape);
            let (pts, closed) = runs.pop().unwrap_or_default();
            Shape::Polyline { pts: pts.into_iter().map(f).collect(), closed }
        }
        Shape::Text { at, height, angle, text, align, tag, wide, mirrored } => {
            // Its baseline turned as the page is, its height as its up stretches, its width as its baseline does.
            let (sin, cos) = angle.to_radians().sin_cos();
            let (o, along, up) = (f(*at), f(*at + P2(cos, sin)), f(*at + P2(-sin, cos)));
            let turn = (along.1 - o.1).atan2(along.0 - o.0).to_degrees().rem_euclid(360.0);
            let (h, w) = (up.dist(o), along.dist(o));
            let wide = if h > 0.0 { wide * w / h } else { *wide };
            Shape::Text {
                at: o,
                height: height * h,
                angle: turn,
                text: text.clone(),
                align: *align,
                tag: tag.clone(),
                wide,
                mirrored: *mirrored,
            }
        }
        other => similar(other, f, ci.angle, ci.scale),
    }
}

/// The box round `shapes` in a frame turned `angle` degrees: its four corners (its low corner first, counter-clockwise in that
/// frame), back in the shapes' own coordinates. A block placed is boxed in its own turned frame this way.
pub fn frame_corners(shapes: impl IntoIterator<Item = Shape>, angle: f64) -> Option<[P2; 4]> {
    let (sin, cos) = angle.to_radians().sin_cos();
    let unturn = move |p: P2| P2(p.0 * cos + p.1 * sin, -p.0 * sin + p.1 * cos);
    let turn = |p: P2| P2(p.0 * cos - p.1 * sin, p.0 * sin + p.1 * cos);
    let [lo, hi] = dxf::union(shapes.into_iter().map(|s| similar(&s, unturn, -angle, 1.0).bounds()))?;
    Some([lo, P2(hi.0, lo.1), hi, P2(lo.0, hi.1)].map(turn))
}

/// What kind of entity a shape came from, as Query names it.
pub fn kind(shape: &Shape) -> &'static str {
    match shape {
        Shape::Line(..) => "line",
        Shape::Polyline { .. } => "polyline",
        Shape::Circle { .. } => "circle",
        Shape::Arc { .. } => "arc",
        Shape::Point(_) => "point",
        Shape::Text { .. } => "text",
        Shape::Fill { .. } => "hatch",
    }
}

/// The entity of a placed drawing nearest `at` in plan within `reach` mm, of those it shows, and how far it is, mm.
pub fn nearest<'c>(cad: &'c Cad, ci: &crate::model::CadImport, at: P2, reach: f64) -> Option<(&'c Entity, f64)> {
    let q = crate::model::cad_unplace(ci, at);
    // Mm of plan a unit of the drawing is, along its x and its y: a stretched image's lines are more one way than the other.
    let (kx, ky) = (ci.scale.abs(), (ci.scale * ci.stretch).abs());
    if !(kx > 0.0 && ky > 0.0) {
        return None;
    }
    let (rx, ry) = (reach / kx, reach / ky);
    let to_plan = |p: P2| P2(p.0 * kx, p.1 * ky);
    let entities = &cad.dxf().entities;
    cad.entities_in(P2(q.0 - rx, q.1 - ry), P2(q.0 + rx, q.1 + ry))
        .into_iter()
        .filter_map(|i| entities.get(i))
        .filter(|e| cad.shows(e, &ci.hidden))
        .map(|e| (e, distance_scaled(&e.shape, q, &to_plan)))
        .filter(|(_, d)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// How far point `p` is from `shape` (both in the drawing's units) once `scale` takes them to plan mm, along each axis.
fn distance_scaled(shape: &Shape, p: P2, scale: &impl Fn(P2) -> P2) -> f64 {
    let at = scale(p);
    match shape {
        Shape::Point(q) | Shape::Text { at: q, .. } => scale(*q).dist(at),
        Shape::Fill { rings, .. } if rings.iter().filter(|r| crate::geom::contains(r, p)).count() % 2 == 1 => 0.0,
        _ => paths(shape)
            .iter()
            .flat_map(|(pts, closed)| {
                let close = (*closed).then(|| pts.last().copied().zip(pts.first().copied())).flatten();
                pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(close).collect::<Vec<_>>()
            })
            .map(|(a, b)| crate::geom::dist_to_segment(at, scale(a), scale(b)))
            .fold(f64::INFINITY, f64::min),
    }
}

/// The box round a drawing's bulk, and how many of its entities lie far from it: past [`FAR`] times the span of the middle
/// of its entities (the 2nd to the 98th of each hundred, along each axis) beyond it.
fn bulk(d: &Dxf) -> (Option<[P2; 2]>, usize) {
    let boxes: Vec<[P2; 2]> = d.entities.iter().map(|e| e.shape.bounds()).collect();
    if boxes.len() < BULK_FROM {
        return (dxf::union(boxes), 0);
    }
    let mid = |b: &[P2; 2]| (b[0] + b[1]) * 0.5;
    let mut xs: Vec<f64> = boxes.iter().map(|b| mid(b).0).collect();
    let mut ys: Vec<f64> = boxes.iter().map(|b| mid(b).1).collect();
    xs.sort_by(f64::total_cmp);
    ys.sort_by(f64::total_cmp);
    let at = |v: &[f64], q: usize| v.get((v.len() - 1) * q / 100).copied().unwrap_or(0.0);
    let (lo, hi) = (P2(at(&xs, 2), at(&ys, 2)), P2(at(&xs, 98), at(&ys, 98)));
    let reach = ((hi.0 - lo.0).max(hi.1 - lo.1) * FAR).max(1.0);
    let (lo, hi) = (lo - P2(reach, reach), hi + P2(reach, reach));
    let near = |b: &[P2; 2]| b[0].0 <= hi.0 && b[1].0 >= lo.0 && b[0].1 <= hi.1 && b[1].1 >= lo.1;
    let far = boxes.iter().filter(|b| !near(b)).count();
    (dxf::union(boxes.into_iter().filter(near)), far)
}

/// How a drawing goes in a project file: its DXF deflated (zlib) and in base64; a PDF page's lines are its packed lines
/// (`lines`) instead.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CadFile {
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    dxf: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lines: Option<String>,
}

impl Serialize for Cad {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.file().serialize(s)
    }
}

impl Cad {
    /// A drawing as a file holds it, unpacking no more than `most` bytes.
    fn from_file(f: CadFile, most: usize) -> Result<Cad, String> {
        let cad = match &f.lines {
            Some(lines) => {
                let bytes = crate::cloud::inflate_of("a page's lines", lines, most.min(dxf::MAX_BYTES))?;
                Cad::of_lines(&f.source, bytes).map_err(|e| format!("the lines of `{}`: {e}", f.source))?
            }
            None => {
                let bytes = crate::cloud::inflate_of("a CAD drawing", &f.dxf, most.min(dxf::MAX_BYTES))?;
                Cad::new(&f.source, f.path.clone(), bytes).map_err(|e| format!("CAD drawing `{}`: {e}", f.source))?
            }
        };
        let _ = cad.packed.set(f);
        Ok(cad)
    }
}

impl<'de> Deserialize<'de> for Cad {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Cad, D::Error> {
        Cad::from_file(CadFile::deserialize(d)?, dxf::MAX_BYTES).map_err(serde::de::Error::custom)
    }
}

/// A project file's drawings (`Model::cads`), read no further than a project may hold: the entry past [`MAX_CADS`] is refused
/// before it's unpacked, and each is unpacked to no more than what the others leave of [`MAX_HELD`].
pub(crate) fn deserialize_cads<'de, D: serde::Deserializer<'de>>(d: D) -> Result<std::collections::BTreeMap<String, Cad>, D::Error> {
    use serde::de::{Error, MapAccess, Visitor};
    struct Cads;
    impl<'de> Visitor<'de> for Cads {
        type Value = std::collections::BTreeMap<String, Cad>;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a map of CAD drawings")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let (mut cads, mut held) = (Self::Value::new(), 0_usize);
            while let Some(name) = a.next_key::<String>()? {
                if cads.len() >= MAX_CADS {
                    return Err(A::Error::custom(format!("the file holds more than {MAX_CADS} CAD drawings, all a project may hold")));
                }
                let left = MAX_HELD.saturating_sub(held);
                let cad = Cad::from_file(a.next_value()?, left).map_err(A::Error::custom)?;
                held += cad.len();
                cads.insert(name, cad);
            }
            Ok(cads)
        }
    }
    d.deserialize_map(Cads)
}

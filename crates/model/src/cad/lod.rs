//! A dense drawing zoomed out: its level of detail. Drawn whole, a sheet of hundreds of thousands of entities a few hundred
//! pixels wide is hundreds of thousands of lines, each under a pixel. Instead its small entities are summed into a grid of
//! cells, layer by layer: in each cell, how much ink each layer lays there (its lines' length, their round ends, its fills'
//! area) and that ink's colour (the one most of it is in). A view that shows a cell under a pixel draws the cells' ink as
//! pixels, and only the entities too big for a cell's band as themselves.
//!
//! A drawing has a few bands, each a grid twice as fine as the last ([`BANDS`]); each is made the first time a view needs it,
//! and kept. What a view shows of a band, its layers' ink mixed as the view draws them (its layers hidden, their colours and
//! halftone), is a [`Picture`], made from the band's marks when the view's look of the drawing is new; the last [`PICTURES`]
//! are kept for the next frames.
//!
//! What it holds is bounded: a band no more than [`MOST_MARKS`] marks (4 bytes each), all of a drawing's bands no more than
//! [`MOST_HELD`] (so 64 MB), and a picture no more than [`MOST_CELLS`] cells (6 bytes each). A band past what it may hold is
//! dropped (the finest first, as they hold the most), and a view at its zoom draws every entity as before. What's drawn is
//! only a picture: picking and snapping go by the entities themselves.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crate::dxf::{Dxf, Entity, Horizontal, Shape, Vertical};
use crate::geom::P2;
use crate::prim::Rgb;

/// Entities a drawing needs before it is drawn simplified zoomed out: a sparser one is drawn whole quickly.
pub const LOD_FROM: usize = 10_000;
/// Each band's cells across the long side of the drawing's bulk, coarsest first.
pub const BANDS: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];
/// An entity no more than this many of a band's cells across goes in its cells; a bigger one is drawn itself.
const SMALL_CELLS: f64 = 16.0;
/// The most marks one band holds among its layers (4 bytes each), and the most cells a picture of it holds (6 bytes each):
/// past either the band is dropped. A dense A0 sheet's band of cells a millimetre across holds a few million marks.
pub const MOST_MARKS: usize = 8 << 20;
pub const MOST_CELLS: usize = 4 << 20;
/// The most marks all of a drawing's bands hold.
pub const MOST_HELD: usize = 16 << 20;
/// The colours a drawing's cells name: past these, a colour is drawn in the nearest of them.
const MOST_COLORS: usize = 255;
/// How much of a text's box its letters ink.
const TEXT_INK: f64 = 0.35;
/// How many pixels a cell is when its band is drawn, near enough: from half to one, three quarters on the whole. A line's ink
/// is drawn a pixel wide, as long as it is: a cell's ink is a line through it a cell long (so many pixels times its cells
/// across); a run's round ends and a fill are as big in pixels whatever the cells are, and are held as that line would be at
/// this size.
const CELL_PX: f64 = 0.75;
/// The ink of a run's two round ends: a dot a pixel across.
const ENDS: f64 = std::f64::consts::FRAC_PI_4 / CELL_PX;
/// Pictures a drawing keeps, for the views that show it differently (a plan and its ceiling plan, two windows).
pub const PICTURES: usize = 2;

/// A drawing's level of detail: its layers by number, each entity's layer and colour, the box its bands cover, and the
/// bands, each made when first asked.
pub struct Lod {
    pub layers: Vec<String>,
    /// The entities of each layer, by their place in the drawing.
    of_layer: Vec<Vec<u32>>,
    /// The colours its entities are drawn in (none, the view's ink), and each entity's among them.
    pub colors: Vec<Option<Rgb>>,
    color_of: Vec<u8>,
    /// Each entity's box, to a float's precision: the cells it may lay ink in, and whether it is in a view when it is drawn
    /// itself.
    boxes: Vec<[f32; 4]>,
    lo: P2,
    size: P2,
    bands: [OnceLock<Option<Band>>; BANDS.len()],
    /// The marks its bands hold among them.
    held: AtomicUsize,
    /// The most marks a band of it holds, and all of them: [`MOST_MARKS`] and [`MOST_HELD`].
    pub(crate) most: (usize, usize),
    /// The pictures made of it last.
    pictures: Mutex<Vec<Kept>>,
}

impl std::fmt::Debug for Lod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lod({} layers, {} colours)", self.layers.len(), self.colors.len())
    }
}

/// A picture made of a drawing: of which band, for which look, and the picture (none when it would hold too much).
type Kept = (usize, u64, Option<Arc<Picture>>);

/// A grid over a drawing's bulk, `cell` drawing units square: each layer's marks, and the entities too big for it.
pub struct Band {
    /// Which of [`BANDS`] it is.
    at: usize,
    pub cell: f64,
    lo: P2,
    nx: usize,
    ny: usize,
    layers: Vec<Marks>,
    /// The entities drawn themselves at this band's zoom, in the drawing's order: too big for its cells, or lying outside
    /// its grid.
    pub big: Vec<u32>,
}

impl std::fmt::Debug for Band {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Band({} by {} cells of {}, {} marks)", self.nx, self.ny, self.cell, self.len())
    }
}

/// One layer's marks in a band: the rows it inks (from the bottom) and where each's marks start, and its marks, each a cell
/// along its row (two bytes), the ink there ([`ink_code`]) and its colour (by number in [`Lod::colors`]).
#[derive(Default)]
struct Marks {
    rows: Vec<(u32, u32)>,
    marks: Vec<[u8; 4]>,
}

/// What a view shows of a band: each inked cell's ink, all its layers' the view shows, mixed in their colours as it draws
/// them. Row by row from the bottom.
pub struct Picture {
    cell: f64,
    lo: P2,
    nx: usize,
    ny: usize,
    /// Where each row's cells start, and one past the last row's end.
    rows: Vec<u32>,
    cells: Vec<Inked>,
}

/// A cell of a picture: its place along its row, its colour, and its ink ([`ink_code`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inked {
    pub x: u16,
    pub rgb: Rgb,
    pub ink: u8,
}

/// Ink as a byte, by its logarithm: 16 steps a doubling, from 2⁻⁸ to 2⁸ of a cell (a cell a pixel, a line a pixel wide
/// through it is 1), 0 for none.
pub fn ink_code(v: f64) -> u8 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    (v.log2() * 16.0 + 128.0).round().clamp(1.0, 255.0) as u8
}

/// The ink a byte of [`ink_code`] stands for.
pub fn ink_of(code: u8) -> f64 {
    if code == 0 { 0.0 } else { ((f64::from(code) - 128.0) / 16.0).exp2() }
}

/// The cells of a grid of `nx` by `ny` cells `cell` units square from `lo`, that meet the box from `lo` to `hi`: the rows,
/// and the first and last along them.
fn span(grid: (P2, f64, usize, usize), lo: P2, hi: P2) -> Option<(std::ops::Range<usize>, usize, usize)> {
    let (o, cell, nx, ny) = grid;
    let meets = lo.0 <= o.0 + nx as f64 * cell && hi.0 >= o.0 && lo.1 <= o.1 + ny as f64 * cell && hi.1 >= o.1;
    if !meets {
        return None;
    }
    let at = |v: f64, from: f64, n: usize| {
        let c = ((v - from) / cell).floor();
        if c.is_finite() { c.clamp(0.0, (n - 1) as f64) as usize } else { 0 }
    };
    Some((at(lo.1, o.1, ny)..at(hi.1, o.1, ny) + 1, at(lo.0, o.0, nx), at(hi.0, o.0, nx)))
}

impl Picture {
    /// The inked cells that meet the box from `lo` to `hi` (drawing units), each with its middle.
    pub fn cells_in(&self, lo: P2, hi: P2) -> impl Iterator<Item = (P2, &Inked)> {
        let (ys, x0, x1) = span((self.lo, self.cell, self.nx, self.ny), lo, hi).unwrap_or((0..0, 0, 0));
        ys.flat_map(move |y| {
            let (from, to) = (self.rows.get(y).copied().unwrap_or(0) as usize, self.rows.get(y + 1).copied().unwrap_or(0) as usize);
            let row = self.cells.get(from..to).unwrap_or_default();
            let first = row.partition_point(|c| usize::from(c.x) < x0);
            row.get(first..)
                .unwrap_or_default()
                .iter()
                .take_while(move |c| usize::from(c.x) <= x1)
                .map(move |c| (self.lo + P2((f64::from(c.x) + 0.5) * self.cell, (y as f64 + 0.5) * self.cell), c))
        })
    }

    /// The box its grid covers, drawing units.
    pub fn bounds(&self) -> [P2; 2] {
        [self.lo, self.lo + P2(self.nx as f64 * self.cell, self.ny as f64 * self.cell)]
    }

    /// How many inked cells it holds.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

impl Lod {
    /// The level of detail of drawing `d`, over its `bulk`, its entities in the colours `color` gives (none, the view's ink):
    /// none for a drawing too sparse to need one, of no bulk, or of more layers than a mark can name.
    pub fn new(d: &Dxf, bulk: Option<[P2; 2]>, color: &dyn Fn(&Entity) -> Option<Rgb>) -> Option<Lod> {
        if d.entities.len() < LOD_FROM {
            return None;
        }
        let [lo, hi] = bulk?;
        let size = hi - lo;
        if !(size.0.is_finite() && size.1.is_finite() && size.0.max(size.1) > 0.0) {
            return None;
        }
        let mut layers: Vec<String> = Vec::new();
        let mut of_layer: Vec<Vec<u32>> = Vec::new();
        let mut known: HashMap<&str, usize> = HashMap::new();
        let mut colors: Vec<Option<Rgb>> = Vec::new();
        let mut color_at: HashMap<Option<(u8, u8, u8)>, u8> = HashMap::new();
        let mut color_of = Vec::with_capacity(d.entities.len());
        let mut boxes = Vec::with_capacity(d.entities.len());
        // A colour by layer is its layer's: looked up once a layer.
        let mut by_layer: Vec<Option<Option<Rgb>>> = Vec::new();
        for (i, e) in d.entities.iter().enumerate() {
            boxes.push(bounds(&e.shape));
            let n = match known.get(e.layer.as_str()) {
                Some(n) => *n,
                None => {
                    if layers.len() > usize::from(u16::MAX) {
                        return None;
                    }
                    layers.push(e.layer.clone());
                    of_layer.push(Vec::new());
                    known.insert(e.layer.as_str(), layers.len() - 1);
                    layers.len() - 1
                }
            };
            if let Some(l) = of_layer.get_mut(n) {
                l.push(i as u32);
            }
            // A patterned hatch's fill is pale.
            let mut c = match (e.color, by_layer.get(n)) {
                (crate::dxf::Color::ByLayer, Some(Some(c))) => *c,
                (crate::dxf::Color::ByLayer, _) => {
                    let c = color(e);
                    by_layer.resize(by_layer.len().max(n + 1), None);
                    if let Some(at) = by_layer.get_mut(n) {
                        *at = Some(c);
                    }
                    c
                }
                _ => color(e),
            };
            if let Shape::Fill { solid: false, .. } = e.shape {
                let Rgb(r, g, b) = c.unwrap_or(crate::render::INK);
                c = Some(Rgb(pale(r), pale(g), pale(b)));
            }
            let held = c.map(|c| (c.0, c.1, c.2));
            let at = match color_at.get(&held) {
                Some(at) => *at,
                None if colors.len() < MOST_COLORS => {
                    colors.push(c);
                    let at = (colors.len() - 1) as u8;
                    color_at.insert(held, at);
                    at
                }
                None => {
                    let near = nearest(&colors, c);
                    color_at.insert(held, near);
                    near
                }
            };
            color_of.push(at);
        }
        Some(Lod {
            layers,
            of_layer,
            colors,
            color_of,
            boxes,
            lo,
            size,
            bands: Default::default(),
            held: AtomicUsize::new(0),
            most: (MOST_MARKS, MOST_HELD),
            pictures: Mutex::default(),
        })
    }

    /// The coarsest band whose cells are no more than a pixel when a drawing unit is `px` pixels, made now if it hasn't been:
    /// none when even the finest's are bigger, or that band holds too much to keep.
    pub fn band(&self, d: &Dxf, px: f64) -> Option<&Band> {
        let long = self.size.0.max(self.size.1);
        let i = BANDS.iter().position(|n| long / f64::from(*n) * px <= 1.0)?;
        let n = *BANDS.get(i)?;
        self.bands.get(i)?.get_or_init(|| self.make(d, i, long / f64::from(n))).as_ref()
    }

    /// The picture of `band` for a view's `look` of the drawing (any number that is the same for the same look, and differs
    /// for another): its layers' marks in the colour `color` gives each layer's (the colour a mark names among
    /// [`Lod::colors`], which it may override, halftone, or draw in the view's ink), none for a layer the view hides. The last
    /// [`PICTURES`] are kept, so the next frames of the views find theirs made; none when it would hold more than
    /// [`MOST_CELLS`] cells (and that too is kept).
    pub fn picture(&self, band: &Band, look: u64, color: &dyn Fn(usize, Option<Rgb>) -> Option<Rgb>) -> Option<Arc<Picture>> {
        if let Ok(kept) = self.pictures.lock()
            && let Some((.., p)) = kept.iter().find(|(b, l, _)| *b == band.at && *l == look)
        {
            return p.clone();
        }
        let picture = band.paint(&self.colors, color).map(Arc::new);
        if let Ok(mut kept) = self.pictures.lock() {
            kept.retain(|(b, l, _)| (*b, *l) != (band.at, look));
            kept.insert(0, (band.at, look, picture.clone()));
            kept.truncate(PICTURES);
        }
        picture
    }

    /// Entity `i`'s box, drawing units (low x, low y, high x, high y), to a float's precision.
    pub fn entity_box(&self, i: usize) -> Option<[f64; 4]> {
        self.boxes.get(i).map(|b| b.map(f64::from))
    }

    /// Band `at`, of cells `cell` units square, layer by layer; none when it would hold more marks than it may, or than the
    /// drawing's other bands leave. What the small entities would lay (a cell for each a run passes through, each a fill or a
    /// text covers) is counted first, so a band past it is let go at once.
    fn make(&self, d: &Dxf, at: usize, cell: f64) -> Option<Band> {
        let nx = ((self.size.0 / cell).ceil() as usize).clamp(1, 1 << 16);
        let ny = ((self.size.1 / cell).ceil() as usize).clamp(1, 1 << 16);
        let (gx, gy) = (self.lo.0 + nx as f64 * cell, self.lo.1 + ny as f64 * cell);
        let small = |b: &[f64; 4]| {
            b[0] >= self.lo.0 && b[1] >= self.lo.1 && b[2] < gx && b[3] < gy && (b[2] - b[0]).max(b[3] - b[1]) <= SMALL_CELLS * cell
        };
        let most = self.most.0.min(self.most.1.saturating_sub(self.held.load(Ordering::Relaxed)));
        let laid: f64 = d
            .entities
            .iter()
            .zip(&self.boxes)
            .filter(|(_, b)| small(&b.map(f64::from)))
            .map(|(e, b)| match &e.shape {
                Shape::Fill { .. } | Shape::Text { .. } => {
                    ((f64::from(b[2] - b[0]) / cell).ceil() + 1.0) * ((f64::from(b[3] - b[1]) / cell).ceil() + 1.0)
                }
                shape => length(shape) / cell + 1.0,
            })
            .sum();
        if laid > most as f64 {
            return None;
        }
        let (mut big, mut layers, mut held) = (Vec::new(), Vec::with_capacity(self.of_layer.len()), 0_usize);
        for entities in &self.of_layer {
            let mut sums = Sums { lo: self.lo, cell, nx, ny, laid: Vec::new(), merged: 0 };
            for &i in entities {
                let (Some(e), Some(b)) = (d.entities.get(i as usize), self.entity_box(i as usize)) else { continue };
                let ink = Ink { color: self.color_of.get(i as usize).copied().unwrap_or(0), alpha: f64::from(e.alpha) / 255.0 };
                let fits = small(&b)
                    && match &e.shape {
                        Shape::Point(_) => true,
                        Shape::Text { at, height, angle, text, align, wide, .. } => {
                            let w = f64::from(crate::font::width(text, 1.0)) * height * wide;
                            sums.text(*at, (w, *height), *angle, (align.h, align.v), ink, (gx, gy))
                        }
                        Shape::Fill { rings, .. } => {
                            sums.spread(b, fill_area(rings) / (cell * cell) * CELL_PX, ink);
                            true
                        }
                        Shape::Line(a, b) => {
                            sums.run(&[*a, *b], false, ink);
                            true
                        }
                        Shape::Polyline { pts, closed } => {
                            sums.run(pts, *closed, ink);
                            true
                        }
                        shape => {
                            for (pts, closed) in super::paths(shape) {
                                sums.run(&pts, closed, ink);
                            }
                            true
                        }
                    };
                if !fits {
                    big.push(i);
                }
                if held + sums.merged > most {
                    return None;
                }
            }
            sums.merge();
            if held + sums.laid.len() > most {
                return None;
            }
            held += sums.laid.len();
            layers.push(sums.marks());
        }
        big.sort_unstable();
        self.held.fetch_add(held, Ordering::Relaxed);
        Some(Band { at, cell, lo: self.lo, nx, ny, layers, big })
    }
}

impl Band {
    /// How many marks it holds among its layers.
    pub fn len(&self) -> usize {
        self.layers.iter().map(|l| l.marks.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// [`Lod::picture`], made: row by row, each shown layer's marks in the row summed into the row's cells.
    fn paint(&self, colors: &[Option<Rgb>], color: &dyn Fn(usize, Option<Rgb>) -> Option<Rgb>) -> Option<Picture> {
        // Each layer's colours, worked out once, and where it is in its rows.
        let shown: Vec<(usize, Vec<Rgb>)> = (0..self.layers.len())
            .filter_map(|l| {
                let all: Option<Vec<Rgb>> = colors.iter().map(|c| color(l, *c)).collect();
                Some((l, all?))
            })
            .collect();
        let mut at: Vec<usize> = vec![0; shown.len()];
        let mut row = vec![[0.0_f64; 4]; self.nx];
        let (mut rows, mut cells) = (Vec::with_capacity(self.ny + 1), Vec::new());
        for y in 0..self.ny {
            rows.push(cells.len() as u32);
            let mut any = false;
            for ((l, palette), next) in shown.iter().zip(at.iter_mut()) {
                let Some(marks) = self.layers.get(*l) else { continue };
                let Some(&(ry, start)) = marks.rows.get(*next) else { continue };
                if ry as usize != y {
                    continue;
                }
                let end = marks.rows.get(*next + 1).map_or(marks.marks.len(), |r| r.1 as usize);
                *next += 1;
                for m in marks.marks.get(start as usize..end).unwrap_or_default() {
                    let x = usize::from(u16::from_le_bytes([m[0], m[1]]));
                    let (ink, c) = (ink_of(m[2]), palette.get(usize::from(m[3])).copied().unwrap_or(crate::render::INK));
                    if let Some(p) = row.get_mut(x) {
                        *p = [p[0] + ink, p[1] + f64::from(c.0) * ink, p[2] + f64::from(c.1) * ink, p[3] + f64::from(c.2) * ink];
                        any = true;
                    }
                }
            }
            if !any {
                continue;
            }
            for (x, p) in row.iter_mut().enumerate() {
                if p[0] > 0.0 {
                    let c = |v: f64| (v / p[0]).round().clamp(0.0, 255.0) as u8;
                    cells.push(Inked { x: x as u16, rgb: Rgb(c(p[1]), c(p[2]), c(p[3])), ink: ink_code(p[0]) });
                    *p = [0.0; 4];
                }
            }
            if cells.len() > MOST_CELLS {
                return None;
            }
        }
        rows.push(cells.len() as u32);
        Some(Picture { cell: self.cell, lo: self.lo, nx: self.nx, ny: self.ny, rows, cells })
    }
}

/// The box round `shape` ([`Shape::bounds`]), to a float's precision; a line's, a polyline's and a fill's found without
/// copying their points.
fn bounds(shape: &Shape) -> [f32; 4] {
    let of = |pts: &mut dyn Iterator<Item = &P2>| {
        pts.fold([f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY], |b, p| {
            [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)]
        })
    };
    let b = match shape {
        Shape::Line(a, b) => of(&mut [a, b].into_iter()),
        Shape::Polyline { pts, .. } => of(&mut pts.iter()),
        Shape::Fill { rings, .. } => of(&mut rings.iter().flatten()),
        other => {
            let [lo, hi] = other.bounds();
            [lo.0, lo.1, hi.0, hi.1]
        }
    };
    b.map(|v| v as f32)
}

/// How long the lines `shape` draws are, drawing units: a circle's and an arc's along them.
fn length(shape: &Shape) -> f64 {
    let run = |pts: &[P2], closed: bool| {
        let back = if closed { pts.last().zip(pts.first()).map_or(0.0, |(a, b)| a.dist(*b)) } else { 0.0 };
        pts.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).sum::<f64>() + back
    };
    let d = match shape {
        Shape::Line(a, b) => a.dist(*b),
        Shape::Polyline { pts, closed } => run(pts, *closed),
        Shape::Circle { r, .. } => std::f64::consts::TAU * r,
        Shape::Arc { r, start, end, .. } => r * (end - start).abs().to_radians(),
        Shape::Fill { rings, .. } => rings.iter().map(|r| run(r, true)).sum(),
        Shape::Point(_) | Shape::Text { .. } => 0.0,
    };
    if d.is_finite() { d } else { 0.0 }
}

/// The colour among `colors` nearest `c` (the view's ink as [`crate::render::INK`]).
fn nearest(colors: &[Option<Rgb>], c: Option<Rgb>) -> u8 {
    let rgb = |c: Option<Rgb>| c.unwrap_or(crate::render::INK);
    let d = |a: Rgb, b: Rgb| {
        let (r, g, b2) = (i32::from(a.0) - i32::from(b.0), i32::from(a.1) - i32::from(b.1), i32::from(a.2) - i32::from(b.2));
        r * r + g * g + b2 * b2
    };
    colors.iter().enumerate().min_by_key(|(_, k)| d(rgb(**k), rgb(c))).map_or(0, |(i, _)| i as u8)
}

/// A channel of a colour most of the way to the paper, as a patterned hatch is filled.
fn pale(v: u8) -> u8 {
    v + (255 - v) / 4 * 3
}

/// What a layer lays in a cell: its ink, and the colour most of it is in (a weighted vote: the colour leading and by how much).
#[derive(Default, Clone, Copy)]
struct Laid {
    cell: u32,
    ink: f32,
    lead: f32,
    color: u8,
}

/// Ink laid in the same cell as what's laid before it: the two as one.
fn and(a: Laid, b: Laid) -> Laid {
    let (color, lead) = if a.color == b.color {
        (a.color, a.lead + b.lead)
    } else if a.lead >= b.lead {
        (a.color, a.lead - b.lead)
    } else {
        (b.color, b.lead - a.lead)
    };
    Laid { cell: a.cell, ink: a.ink + b.ink, lead, color }
}

/// How many pieces of ink a layer lays before they are merged cell by cell, so what it holds while it is summed stays
/// bounded.
const MERGE_AT: usize = 1 << 20;

/// What an entity is drawn in: its colour, by number, and how opaque it is.
#[derive(Clone, Copy)]
struct Ink {
    color: u8,
    alpha: f64,
}

/// One layer of a band being summed: its grid, and the ink it has laid so far, piece by piece (in order of its cells, one
/// each, up to `merged`).
struct Sums {
    lo: P2,
    cell: f64,
    nx: usize,
    ny: usize,
    laid: Vec<Laid>,
    merged: usize,
}

impl Sums {
    /// The cell point `p` lies in.
    fn cell_of(&self, p: P2) -> usize {
        let at = |v: f64, o: f64, n: usize| (((v - o) / self.cell).floor().max(0.0) as usize).min(n - 1);
        at(p.1, self.lo.1, self.ny) * self.nx + at(p.0, self.lo.0, self.nx)
    }

    /// `amount` of ink (cells squared) added to cell `c`, in what `ink` draws.
    fn add(&mut self, c: usize, amount: f64, ink: Ink) {
        let amount = (amount * ink.alpha) as f32;
        if amount.is_nan() || amount <= 0.0 {
            return;
        }
        self.laid.push(Laid { cell: c as u32, ink: amount, lead: amount, color: ink.color });
        if self.laid.len() >= self.merged + MERGE_AT {
            self.merge();
        }
    }

    /// What's laid, merged: one for each cell, in their order.
    fn merge(&mut self) {
        self.laid.sort_unstable_by_key(|l| l.cell);
        let mut kept: usize = 0;
        for i in 0..self.laid.len() {
            let Some(&l) = self.laid.get(i) else { break };
            match kept.checked_sub(1).and_then(|k| self.laid.get_mut(k)) {
                Some(last) if last.cell == l.cell => *last = and(*last, l),
                _ => {
                    if let Some(at) = self.laid.get_mut(kept) {
                        *at = l;
                    }
                    kept += 1;
                }
            }
        }
        self.laid.truncate(kept);
        self.merged = kept;
    }

    /// A run of points (closed back to its first when `closed`): its length in each cell it runs through, a line a cell
    /// across that is a cell long being 1; and its round ends where it starts. A run of no length is a dot, as it is drawn.
    fn run(&mut self, pts: &[P2], closed: bool, ink: Ink) {
        let back = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
        let segments = pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(back);
        let mut any = false;
        // What it has laid in the cell it is in, added when it leaves it.
        let mut held = (usize::MAX, 0.0);
        for (a, b) in segments {
            any |= self.segment(a, b, &mut held, ink);
        }
        if held.1 > 0.0 {
            self.add(held.0, held.1, ink);
        }
        if let Some(p) = pts.first() {
            self.add(self.cell_of(*p), ENDS + if any { 0.0 } else { 0.75 }, ink);
        }
    }

    /// The line from `a` to `b`: its length in each cell it crosses, cell by cell along it (as a ray is walked through a grid),
    /// added to what's `held` for the cell it is in while it stays in it. Whether it has any length.
    fn segment(&mut self, a: P2, b: P2, held: &mut (usize, f64), ink: Ink) -> bool {
        let len = a.dist(b) / self.cell;
        if !(len > 0.0 && len.is_finite()) {
            return false;
        }
        let (fx, fy) = ((a.0 - self.lo.0) / self.cell, (a.1 - self.lo.1) / self.cell);
        let (dx, dy) = ((b.0 - a.0) / self.cell, (b.1 - a.1) / self.cell);
        let last = |n: usize| (n - 1) as i64;
        let (mut x, mut y) = ((fx.floor() as i64).clamp(0, last(self.nx)), (fy.floor() as i64).clamp(0, last(self.ny)));
        // Where along it (0 to 1) it next crosses a column's side and a row's, and how far apart those crossings are.
        let next = |f: f64, d: f64, at: i64| {
            if d > 0.0 {
                ((at + 1) as f64 - f) / d
            } else if d < 0.0 {
                (f - at as f64) / -d
            } else {
                f64::INFINITY
            }
        };
        let (mut tx, mut ty) = (next(fx, dx, x), next(fy, dy, y));
        let (sx, sy) = (1.0 / dx.abs(), 1.0 / dy.abs());
        let mut t = 0.0;
        // No more steps than the cells between its ends.
        let steps = (dx.abs() + dy.abs()).ceil().min(1e6) as usize + 2;
        for _ in 0..steps {
            let to = tx.min(ty).min(1.0);
            let c = y as usize * self.nx + x as usize;
            if c != held.0 {
                if held.1 > 0.0 {
                    self.add(held.0, held.1, ink);
                }
                *held = (c, 0.0);
            }
            held.1 += (to - t).max(0.0) * len;
            if to >= 1.0 {
                break;
            }
            t = to;
            if tx <= ty {
                x = (x + if dx > 0.0 { 1 } else { -1 }).clamp(0, last(self.nx));
                tx += sx;
            } else {
                y = (y + if dy > 0.0 { 1 } else { -1 }).clamp(0, last(self.ny));
                ty += sy;
            }
        }
        true
    }

    /// `ink` spread over the cells box `b` covers, by how much of each it covers.
    fn spread(&mut self, b: [f64; 4], amount: f64, ink: Ink) {
        let (w, h) = ((b[2] - b[0]).max(0.0), (b[3] - b[1]).max(0.0));
        let [first, last] = [self.cell_of(P2(b[0], b[1])), self.cell_of(P2(b[2], b[3]))];
        let (x0, y0, x1, y1) = (first % self.nx, first / self.nx, last % self.nx, last / self.nx);
        if w * h <= 0.0 || (x0 == x1 && y0 == y1) {
            self.add(self.cell_of(P2((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)), amount, ink);
            return;
        }
        let (cell, lo) = (self.cell, self.lo);
        let along = move |i: usize, o: f64, from: f64, to: f64| {
            let (c0, c1) = (o + i as f64 * cell, o + (i + 1) as f64 * cell);
            (c1.min(to) - c0.max(from)).max(0.0)
        };
        for y in y0..=y1 {
            let dy = along(y, lo.1, b[1], b[3]) / h;
            for x in x0..=x1 {
                let dx = along(x, lo.0, b[0], b[2]) / w;
                self.add(y * self.nx + x, amount * dx * dy, ink);
            }
        }
    }

    /// A text `size` (width, height) at `at`, turned `angle` degrees and aligned so: its letters' ink spread over its box, if
    /// that box is small and inside the grid (up to `top`, its far corner).
    fn text(&mut self, at: P2, (w, h): (f64, f64), angle: f64, (ah, av): (Horizontal, Vertical), ink: Ink, top: (f64, f64)) -> bool {
        if !(w.is_finite() && h.is_finite() && w >= 0.0 && h > 0.0) || w.max(h) > SMALL_CELLS * self.cell {
            return false;
        }
        let (u, v) = (P2(angle.to_radians().cos(), angle.to_radians().sin()), P2(-angle.to_radians().sin(), angle.to_radians().cos()));
        let left = match ah {
            Horizontal::Left => at,
            Horizontal::Center => at - u * (w / 2.0),
            Horizontal::Right => at - u * w,
        };
        let base = match av {
            Vertical::Baseline | Vertical::Bottom => left,
            Vertical::Middle => left - v * (h / 2.0),
            Vertical::Top => left - v * h,
        };
        let corners = [base, base + u * w, base + u * w + v * h, base + v * h];
        let Some([lo, hi]) = crate::dxf::union(corners.map(|p| [p, p])) else { return false };
        if lo.0 < self.lo.0 || lo.1 < self.lo.1 || hi.0 >= top.0 || hi.1 >= top.1 {
            return false;
        }
        self.spread([lo.0, lo.1, hi.0, hi.1], w * h * TEXT_INK / (self.cell * self.cell) * CELL_PX, ink);
        true
    }

    /// Its cells' sums, merged, as a layer's marks, row by row.
    fn marks(self) -> Marks {
        let mut out = Marks { rows: Vec::new(), marks: Vec::with_capacity(self.laid.len()) };
        for a in self.laid {
            let (y, x) = (a.cell as usize / self.nx, a.cell as usize % self.nx);
            if out.rows.last().is_none_or(|r| r.0 as usize != y) {
                out.rows.push((y as u32, out.marks.len() as u32));
            }
            let [x0, x1] = (x as u16).to_le_bytes();
            out.marks.push([x0, x1, ink_code(f64::from(a.ink)), a.color]);
        }
        out
    }
}

/// The area a hatch's rings fill by odd parity, near enough: its biggest ring's, less those inside it, plus those outside.
fn fill_area(rings: &[Vec<P2>]) -> f64 {
    let areas: Vec<f64> = rings.iter().map(|r| crate::geom::area(r).abs()).collect();
    let Some((big, outer)) = areas.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)) else { return 0.0 };
    let Some(ring) = rings.get(big) else { return 0.0 };
    let mut total = outer;
    for (i, (r, a)) in rings.iter().zip(&areas).enumerate() {
        if i == big {
            continue;
        }
        if r.first().is_some_and(|p| crate::geom::contains(ring, *p)) {
            total -= a;
        } else {
            total += a;
        }
    }
    total.max(0.0)
}

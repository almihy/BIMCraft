//! A dense drawing zoomed out: its level of detail. Drawn whole, a sheet of hundreds of thousands of entities a few hundred
//! pixels wide is hundreds of thousands of lines, each under a pixel. Instead its small entities are summed into a grid of
//! cells, layer by layer: in each cell, how much ink each layer lays there (its lines' length, their round ends, its fills'
//! area) and that ink's colour (the one most of it is in). A view that shows a cell under a pixel draws the cells' ink as
//! pixels, and only the entities too big for a cell's band as themselves.
//!
//! It keeps the page's order, so what is drawn over a fill stays over it: a band is a few pictures and, between them, the big
//! entities drawn themselves ([`Step`]). A picture is runs of cells, each the small entities between two big fills, and those
//! big fills, which are drawn into the picture at their place; a big line, text or patterned hatch is drawn itself after the
//! picture that holds what lies under it, and what is drawn over it goes in a later picture. Which picture each goes in is
//! found on a coarse grid of the drawing ([`LEVELS`] a side): an entity goes in the first picture after every big one drawn
//! itself before it that it may meet; past [`MOST_PICTURES`], in the last.
//!
//! A drawing has a few bands, each a grid twice as fine as the last ([`BANDS`]); each is made the first time a view needs it,
//! and kept. What a view shows of a band, its layers' ink mixed as the view draws them (its layers hidden, their colours and
//! halftone), is a [`Picture`], made from the band's marks when the view's look of the drawing is new; the last [`PICTURES`]
//! are kept for the next frames.
//!
//! What it holds is bounded: a band no more than [`MOST_MARKS`] marks (4 bytes each), all of a drawing's bands no more than
//! [`MOST_HELD`] (so 64 MB), and a picture no more than [`MOST_CELLS`] cells (8 bytes each). A band past what it may hold is
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
/// The most marks one band holds among its layers (4 bytes each), and the most cells a picture of it holds (8 bytes each):
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
/// across); a run's round ends are as big in pixels whatever the cells are, and are held as that line would be at this size.
const CELL_PX: f64 = 0.75;
/// The ink of a run's two round ends: a dot a pixel across.
const ENDS: f64 = std::f64::consts::FRAC_PI_4 / CELL_PX;
/// Pictures a drawing keeps, for the views that show it differently (a plan and its ceiling plan, two windows).
pub const PICTURES: usize = 2;
/// The most pictures a band is drawn in, with big entities drawn themselves between them.
pub const MOST_PICTURES: usize = 8;
/// The cells a side of the grid that finds which picture an entity goes in.
const LEVELS: usize = 64;

/// A drawing's level of detail: its layers by number, each entity's layer and colour, the box its bands cover, and the
/// bands, each made when first asked.
pub struct Lod {
    pub layers: Vec<String>,
    /// Each entity's layer, by number in `layers`.
    layer_of: Vec<u16>,
    /// The colours its entities are drawn in (none, the view's ink), each a fill's or a line's, and each entity's among them.
    pub colors: Vec<(Option<Rgb>, bool)>,
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

/// A colour as a drawing's cells hold it: its channels (none, the view's ink) and whether it is a fill's.
type Held = (Option<(u8, u8, u8)>, bool);

/// A row of a layer's marks being painted: which row, its marks and its layer's colours.
type RowOf<'a> = (u32, &'a [[u8; 4]], &'a [Rgb]);

/// A picture made of a drawing: of which band, for which look, and the picture (none when it would hold too much).
type Kept = (usize, u64, Option<Arc<Picture>>);

/// A grid over a drawing's bulk, `cell` drawing units square: what it draws, in the drawing's order.
pub struct Band {
    /// Which of [`BANDS`] it is.
    at: usize,
    pub cell: f64,
    lo: P2,
    nx: usize,
    ny: usize,
    pub steps: Vec<Step>,
}

impl std::fmt::Debug for Band {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Band({} by {} cells of {}, {} marks, {} steps)", self.nx, self.ny, self.cell, self.len(), self.steps.len())
    }
}

/// What a band draws, in order: a picture of its cells, or entities drawn themselves.
#[derive(Debug)]
pub enum Step {
    /// A picture: its runs, in order.
    Picture(Vec<Run>),
    /// Entities drawn themselves (a line, a text or a patterned hatch too big for the cells, or lying outside its grid), by
    /// their place in the drawing, in its order.
    Whole(Vec<u32>),
}

/// A run of a picture: the small entities between two big fills, summed into each layer's marks; or a big fill, drawn into
/// the picture at its place.
pub enum Run {
    Cells(Vec<(u16, Marks)>),
    Fill(u32),
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Run::Cells(layers) => write!(f, "Cells({} layers)", layers.len()),
            Run::Fill(i) => write!(f, "Fill({i})"),
        }
    }
}

/// One layer's marks in a run: the rows it inks (from the bottom) and where each's marks start, and its marks, each a cell
/// along its row (two bytes), the ink there ([`ink_code`]) and its colour (by number in [`Lod::colors`]).
#[derive(Default)]
pub struct Marks {
    rows: Vec<(u32, u32)>,
    marks: Vec<[u8; 4]>,
}

/// What a view shows of a band: for each of its pictures' runs of cells, each inked cell's ink, all its layers' the view
/// shows, mixed in their colours as it draws them, a fill's apart from a line's. Row by row from the bottom.
pub struct Picture {
    cell: f64,
    lo: P2,
    nx: usize,
    ny: usize,
    /// For each [`Step::Picture`] of the band in order, for each of its runs, its cells (none for a fill's).
    pub runs: Vec<Vec<Cells>>,
}

/// A run's cells in a picture: its rows (from the bottom) and where each's cells start, and the cells.
#[derive(Default)]
pub struct Cells {
    rows: Vec<(u32, u32)>,
    cells: Vec<Inked>,
}

/// A cell of a picture: its place along its row, its colour, its ink ([`ink_code`]) and whether that is a fill's (its share of
/// the cell it covers) or a line's (a line a cell long through it being 1). A cell inked by both has the fill's first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inked {
    pub x: u16,
    pub rgb: Rgb,
    pub ink: u8,
    pub fill: bool,
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
    /// Run `run` of picture `picture`'s inked cells that meet the box from `lo` to `hi` (drawing units), each with its middle.
    pub fn run_in(&self, picture: usize, run: usize, lo: P2, hi: P2) -> impl Iterator<Item = (P2, &Inked)> {
        let cells = self.runs.get(picture).and_then(|p| p.get(run));
        let (ys, x0, x1) = match cells {
            Some(_) => span((self.lo, self.cell, self.nx, self.ny), lo, hi).unwrap_or((0..0, 0, 0)),
            None => (0..0, 0, 0),
        };
        let rows: &[(u32, u32)] = cells.map_or(&[], |c| &c.rows);
        let first = rows.partition_point(|r| (r.0 as usize) < ys.start);
        let all: &[Inked] = cells.map_or(&[], |c| &c.cells);
        rows.get(first..).unwrap_or_default().iter().enumerate().take_while(move |(_, r)| (r.0 as usize) < ys.end).flat_map(
            move |(k, r)| {
                let end = rows.get(first + k + 1).map_or(all.len(), |n| n.1 as usize);
                let row = all.get(r.1 as usize..end).unwrap_or_default();
                let from = row.partition_point(|c| usize::from(c.x) < x0);
                let y = r.0 as f64;
                row.get(from..)
                    .unwrap_or_default()
                    .iter()
                    .take_while(move |c| usize::from(c.x) <= x1)
                    .map(move |c| (self.lo + P2((f64::from(c.x) + 0.5) * self.cell, (y + 0.5) * self.cell), c))
            },
        )
    }

    /// Every run's inked cells that meet the box from `lo` to `hi`, each with its middle.
    pub fn cells_in(&self, lo: P2, hi: P2) -> impl Iterator<Item = (P2, &Inked)> {
        self.runs.iter().enumerate().flat_map(move |(p, runs)| (0..runs.len()).flat_map(move |r| self.run_in(p, r, lo, hi)))
    }

    /// The box its grid covers, drawing units.
    pub fn bounds(&self) -> [P2; 2] {
        [self.lo, self.lo + P2(self.nx as f64 * self.cell, self.ny as f64 * self.cell)]
    }

    /// How many inked cells it holds.
    pub fn len(&self) -> usize {
        self.runs.iter().flatten().map(|c| c.cells.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
        let mut known: HashMap<&str, u16> = HashMap::new();
        let mut layer_of = Vec::with_capacity(d.entities.len());
        let mut colors: Vec<(Option<Rgb>, bool)> = Vec::new();
        let mut color_at: HashMap<Held, u8> = HashMap::new();
        let mut color_of = Vec::with_capacity(d.entities.len());
        let mut boxes = Vec::with_capacity(d.entities.len());
        // A colour by layer is its layer's: looked up once a layer.
        let mut by_layer: Vec<Option<Option<Rgb>>> = Vec::new();
        for e in &d.entities {
            boxes.push(bounds(&e.shape));
            let n = match known.get(e.layer.as_str()) {
                Some(n) => *n,
                None => {
                    if layers.len() > usize::from(u16::MAX) {
                        return None;
                    }
                    layers.push(e.layer.clone());
                    let n = (layers.len() - 1) as u16;
                    known.insert(e.layer.as_str(), n);
                    n
                }
            };
            layer_of.push(n);
            let mut c = match (e.color, by_layer.get(usize::from(n))) {
                (crate::dxf::Color::ByLayer, Some(Some(c))) => *c,
                (crate::dxf::Color::ByLayer, _) => {
                    let c = color(e);
                    by_layer.resize(by_layer.len().max(usize::from(n) + 1), None);
                    if let Some(at) = by_layer.get_mut(usize::from(n)) {
                        *at = Some(c);
                    }
                    c
                }
                _ => color(e),
            };
            // A patterned hatch with no pattern held is filled pale; one drawn with its pattern is its tint, its colour.
            if let Shape::Fill { solid: false, tile: None, .. } = e.shape {
                let Rgb(r, g, b) = c.unwrap_or(crate::render::INK);
                c = Some(Rgb(pale(r), pale(g), pale(b)));
            }
            let fill = matches!(e.shape, Shape::Fill { .. });
            let held = (c.map(|c| (c.0, c.1, c.2)), fill);
            let at = match color_at.get(&held) {
                Some(at) => *at,
                None if colors.len() < MOST_COLORS => {
                    colors.push((c, fill));
                    let at = (colors.len() - 1) as u8;
                    color_at.insert(held, at);
                    at
                }
                None => {
                    let near = nearest(&colors, c, fill);
                    color_at.insert(held, near);
                    near
                }
            };
            color_of.push(at);
        }
        Some(Lod {
            layers,
            layer_of,
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
    /// for another): its layers' marks in the colour `color` gives each (of a layer, the colour a mark names among
    /// [`Lod::colors`], and whether that is a fill's: it may override a line's, halftone it, or draw it in the view's ink),
    /// none for a layer the view hides. The last [`PICTURES`] are kept, so the next frames of the views find theirs made; none
    /// when it would hold more than [`MOST_CELLS`] cells (and that too is kept).
    pub fn picture(&self, band: &Band, look: u64, color: &dyn Fn(usize, Option<Rgb>, bool) -> Option<Rgb>) -> Option<Arc<Picture>> {
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

    /// Entity `i`'s layer, by number in [`Lod::layers`].
    pub fn layer_of(&self, i: usize) -> Option<usize> {
        self.layer_of.get(i).map(|l| usize::from(*l))
    }

    /// Band `at`, of cells `cell` units square, in the drawing's order; none when it would hold more marks than it may, or
    /// than the drawing's other bands leave. What the small entities would lay (a cell for each a run passes through, each a
    /// fill or a text covers) is counted first, so a band past it is let go at once.
    fn make(&self, d: &Dxf, at: usize, cell: f64) -> Option<Band> {
        let nx = ((self.size.0 / cell).ceil() as usize).clamp(1, 1 << 16);
        let ny = ((self.size.1 / cell).ceil() as usize).clamp(1, 1 << 16);
        let (gx, gy) = (self.lo.0 + nx as f64 * cell, self.lo.1 + ny as f64 * cell);
        let inside = |b: &[f64; 4]| b[0] >= self.lo.0 && b[1] >= self.lo.1 && b[2] < gx && b[3] < gy;
        let small = |b: &[f64; 4]| inside(b) && (b[2] - b[0]).max(b[3] - b[1]) <= SMALL_CELLS * cell;
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
        let mut order = Order::new(self.lo, self.size);
        let mut pictures: Vec<Making> = Vec::new();
        // Big entities drawn themselves: those before picture k at k, the last after them all.
        let mut wholes: Vec<Vec<u32>> = Vec::new();
        let mut held = 0_usize;
        for (i, e) in d.entities.iter().enumerate() {
            let (Some(b), Some(&layer)) = (self.entity_box(i), self.layer_of.get(i)) else { continue };
            let ink = Ink { color: self.color_of.get(i).copied().unwrap_or(0), alpha: f64::from(e.alpha) / 255.0 };
            // A text's box is its letters', turned and aligned; one of numbers that aren't is drawn itself.
            let b = match &e.shape {
                Shape::Text { at, height, angle, text, align, wide, .. } => {
                    let w = f64::from(crate::font::width(text, 1.0)) * height * wide;
                    text_box(*at, (w, *height), *angle, (align.h, align.v)).unwrap_or([f64::NEG_INFINITY, 0.0, f64::INFINITY, 0.0])
                }
                _ => b,
            };
            // In its cells, as a big fill in a picture, or drawn itself.
            let what = if small(&b) {
                What::Cells
            } else if inside(&b) && matches!(&e.shape, Shape::Fill { tile: None, .. }) {
                What::Fill
            } else {
                What::Whole
            };
            if what == What::Whole {
                let s = order.whole(&e.shape, b);
                wholes.resize_with(wholes.len().max(s + 1), Vec::new);
                if let Some(w) = wholes.get_mut(s) {
                    w.push(i as u32);
                }
                continue;
            }
            let k = order.picture(b);
            pictures.resize_with(pictures.len().max(k + 1), || Making::new(self.lo, cell, nx, ny));
            let Some(p) = pictures.get_mut(k) else { continue };
            if what == What::Fill {
                held += p.close();
                p.runs.push(Run::Fill(i as u32));
                continue;
            }
            let Some(sums) = p.sums(layer) else { continue };
            match &e.shape {
                Shape::Point(_) => {}
                Shape::Text { height, text, wide, .. } => {
                    let w = f64::from(crate::font::width(text, 1.0)) * height * wide;
                    sums.spread(b, w * height * TEXT_INK / (cell * cell) * CELL_PX, ink);
                }
                Shape::Fill { rings, .. } => sums.spread(b, fill_area(rings) / (cell * cell), ink),
                Shape::Line(a, z) => sums.run(&[*a, *z], false, ink),
                Shape::Polyline { pts, closed } => sums.run(pts, *closed, ink),
                shape => {
                    for (pts, closed) in super::paths(shape) {
                        sums.run(&pts, closed, ink);
                    }
                }
            }
            // What's laid and not yet merged kept within bounds across every layer open; and what's merged within what a band
            // may hold.
            if i % 256 == 0 && pictures.iter().map(Making::unmerged).sum::<usize>() > MERGE_AT {
                pictures.iter_mut().for_each(Making::merge);
            }
            if held + pictures.iter().map(Making::merged).sum::<usize>() > most {
                return None;
            }
        }
        let mut steps = Vec::new();
        let n = pictures.len().max(wholes.len().saturating_sub(1));
        let mut pictures = pictures.into_iter();
        let mut wholes = wholes.into_iter();
        for _ in 0..=n {
            if let Some(w) = wholes.next().filter(|w| !w.is_empty()) {
                steps.push(Step::Whole(w));
            }
            if let Some(mut p) = pictures.next() {
                held += p.close();
                if !p.runs.is_empty() {
                    steps.push(Step::Picture(p.runs));
                }
            }
        }
        if held > most {
            return None;
        }
        self.held.fetch_add(held, Ordering::Relaxed);
        Some(Band { at, cell, lo: self.lo, nx, ny, steps })
    }
}

/// What an entity is in a band: summed into cells (a line, a fill or a text small enough), a big fill drawn into a picture,
/// or drawn itself.
#[derive(PartialEq)]
enum What {
    Cells,
    Fill,
    Whole,
}

/// A picture of a band being made: its runs so far, and each layer's sums of the run open.
struct Making {
    lo: P2,
    cell: f64,
    nx: usize,
    ny: usize,
    runs: Vec<Run>,
    open: Vec<(u16, Sums)>,
}

impl Making {
    fn new(lo: P2, cell: f64, nx: usize, ny: usize) -> Making {
        Making { lo, cell, nx, ny, runs: Vec::new(), open: Vec::new() }
    }

    /// The sums of layer `layer` in the run open.
    fn sums(&mut self, layer: u16) -> Option<&mut Sums> {
        if !self.open.iter().any(|(l, _)| *l == layer) {
            self.open.push((layer, Sums { lo: self.lo, cell: self.cell, nx: self.nx, ny: self.ny, laid: Vec::new(), merged: 0 }));
        }
        self.open.iter_mut().rev().find(|(l, _)| *l == layer).map(|(_, s)| s)
    }

    /// How many pieces of ink its open run has merged.
    fn merged(&self) -> usize {
        self.open.iter().map(|(_, s)| s.merged).sum()
    }

    /// How many pieces of ink its open run has laid since they were merged.
    fn unmerged(&self) -> usize {
        self.open.iter().map(|(_, s)| s.laid.len().saturating_sub(s.merged)).sum()
    }

    /// Its open run's pieces merged, cell by cell.
    fn merge(&mut self) {
        for (_, s) in &mut self.open {
            if s.laid.len() > s.merged {
                s.merge();
            }
        }
    }

    /// Its open run closed: each layer's sums merged into marks. How many marks it holds.
    fn close(&mut self) -> usize {
        if self.open.is_empty() {
            return 0;
        }
        let mut layers: Vec<(u16, Marks)> = std::mem::take(&mut self.open)
            .into_iter()
            .map(|(l, mut s)| {
                s.merge();
                (l, s.marks())
            })
            .collect();
        layers.sort_by_key(|(l, _)| *l);
        let n = layers.iter().map(|(_, m)| m.marks.len()).sum();
        self.runs.push(Run::Cells(layers));
        n
    }
}

/// Which picture an entity of a band goes in, and after which picture an entity drawn itself is: found on a coarse grid of the
/// drawing, each of its cells holding the last picture anything in it went in and the last place anything drawn itself was.
struct Order {
    lo: P2,
    cell: P2,
    /// The last picture something in each cell went in, and after which picture the last drawn itself there is (0, before
    /// the first), each one more than that (0 for none).
    pictures: Vec<u8>,
    wholes: Vec<u8>,
}

impl Order {
    fn new(lo: P2, size: P2) -> Order {
        let cell = P2((size.0 / LEVELS as f64).max(1e-9), (size.1 / LEVELS as f64).max(1e-9));
        Order { lo, cell, pictures: vec![0; LEVELS * LEVELS], wholes: vec![0; LEVELS * LEVELS] }
    }

    /// The grid's cells box `b` meets, as rows and columns.
    fn cells(&self, b: [f64; 4]) -> (std::ops::RangeInclusive<usize>, std::ops::RangeInclusive<usize>) {
        let at = |v: f64, o: f64, c: f64| {
            let k = ((v - o) / c).floor();
            if k.is_finite() { k.clamp(0.0, (LEVELS - 1) as f64) as usize } else { 0 }
        };
        let (x0, x1) = (at(b[0], self.lo.0, self.cell.0), at(b[2], self.lo.0, self.cell.0));
        let (y0, y1) = (at(b[1], self.lo.1, self.cell.1), at(b[3], self.lo.1, self.cell.1));
        (y0..=y1, x0..=x1)
    }

    /// The picture an entity in box `b` goes in: the first after every entity drawn itself before it that it may meet.
    fn picture(&mut self, b: [f64; 4]) -> usize {
        let (ys, xs) = self.cells(b);
        let mut k = 0;
        for y in ys.clone() {
            for x in xs.clone() {
                k = k.max(self.wholes.get(y * LEVELS + x).copied().unwrap_or(0));
            }
        }
        let k = usize::from(k).min(MOST_PICTURES - 1);
        for y in ys {
            for x in xs.clone() {
                if let Some(p) = self.pictures.get_mut(y * LEVELS + x) {
                    *p = (*p).max(k as u8 + 1);
                }
            }
        }
        k
    }

    /// Where an entity drawn itself goes, `shape` in box `b`: after the last picture anything it may meet went in, and after
    /// those drawn themselves before it; 0 before the first picture, `k + 1` after picture `k`. A line is found along its
    /// runs, so a frame round a sheet lies over only its edges.
    fn whole(&mut self, shape: &Shape, b: [f64; 4]) -> usize {
        let mut cells: Vec<usize> = Vec::new();
        match shape {
            Shape::Line(..) | Shape::Polyline { .. } | Shape::Circle { .. } | Shape::Arc { .. } => {
                for (pts, closed) in super::paths(shape) {
                    let back = closed.then(|| pts.last().copied().zip(pts.first().copied())).flatten();
                    for (a, z) in pts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).chain(back) {
                        // Along it, half a cell a step.
                        let steps = ((z.0 - a.0).abs() / self.cell.0 + (z.1 - a.1).abs() / self.cell.1).mul_add(2.0, 1.0);
                        let steps = if steps.is_finite() { (steps as usize).min(4 * LEVELS) } else { 1 };
                        for s in 0..=steps {
                            let p = a + (z - a) * (s as f64 / steps.max(1) as f64);
                            let (ys, xs) = self.cells([p.0, p.1, p.0, p.1]);
                            cells.push(ys.start() * LEVELS + xs.start());
                        }
                    }
                }
            }
            _ => {
                let (ys, xs) = self.cells(b);
                for y in ys {
                    for x in xs.clone() {
                        cells.push(y * LEVELS + x);
                    }
                }
            }
        }
        cells.sort_unstable();
        cells.dedup();
        let s = cells
            .iter()
            .map(|c| self.pictures.get(*c).copied().unwrap_or(0).max(self.wholes.get(*c).copied().unwrap_or(0)))
            .max()
            .unwrap_or(0)
            .min(MOST_PICTURES as u8);
        for c in cells {
            if let Some(w) = self.wholes.get_mut(c) {
                *w = (*w).max(s);
            }
        }
        usize::from(s)
    }
}

impl Band {
    /// How many marks it holds among its pictures' runs.
    pub fn len(&self) -> usize {
        self.steps
            .iter()
            .filter_map(|s| match s {
                Step::Picture(runs) => Some(runs),
                Step::Whole(_) => None,
            })
            .flatten()
            .map(|r| match r {
                Run::Cells(layers) => layers.iter().map(|(_, m)| m.marks.len()).sum(),
                Run::Fill(_) => 0,
            })
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entities it draws themselves: those drawn whole, and the big fills its pictures draw.
    pub fn whole(&self) -> impl Iterator<Item = u32> + '_ {
        self.steps.iter().flat_map(|s| match s {
            Step::Whole(w) => w.clone(),
            Step::Picture(runs) => runs
                .iter()
                .filter_map(|r| match r {
                    Run::Fill(i) => Some(*i),
                    Run::Cells(_) => None,
                })
                .collect(),
        })
    }

    /// [`Lod::picture`], made: each run of cells row by row, each shown layer's marks in the row summed into the row's cells,
    /// a fill's apart from a line's.
    fn paint(&self, colors: &[(Option<Rgb>, bool)], color: &dyn Fn(usize, Option<Rgb>, bool) -> Option<Rgb>) -> Option<Picture> {
        // Each layer's colours, worked out once; none for a layer hidden.
        let mut palettes: HashMap<u16, Option<Vec<Rgb>>> = HashMap::new();
        // Each cell of a row: a line's ink and its colour times it, and a fill's.
        let mut row = vec![[0.0_f64; 8]; self.nx];
        let mut touched: Vec<usize> = Vec::new();
        let mut total = 0_usize;
        let mut runs_out = Vec::new();
        for step in &self.steps {
            let Step::Picture(runs) = step else { continue };
            let mut pictures = Vec::with_capacity(runs.len());
            for run in runs {
                let Run::Cells(layers) = run else {
                    pictures.push(Cells::default());
                    continue;
                };
                // Every shown layer's rows, by row.
                for (l, _) in layers {
                    palettes
                        .entry(*l)
                        .or_insert_with(|| colors.iter().map(|(c, fill)| color(usize::from(*l), *c, *fill)).collect::<Option<Vec<Rgb>>>());
                }
                let mut rows: Vec<RowOf<'_>> = Vec::new();
                for (l, marks) in layers {
                    let Some(palette) = palettes.get(l).and_then(|p| p.as_deref()) else { continue };
                    for (k, (y, start)) in marks.rows.iter().enumerate() {
                        let end = marks.rows.get(k + 1).map_or(marks.marks.len(), |r| r.1 as usize);
                        rows.push((*y, marks.marks.get(*start as usize..end).unwrap_or_default(), palette));
                    }
                }
                rows.sort_by_key(|r| r.0);
                let mut out = Cells::default();
                let mut at = 0;
                while let Some(&(y, ..)) = rows.get(at) {
                    out.rows.push((y, out.cells.len() as u32));
                    while let Some(&(_, marks, palette)) = rows.get(at).filter(|r| r.0 == y) {
                        at += 1;
                        for m in marks {
                            let x = usize::from(u16::from_le_bytes([m[0], m[1]]));
                            let k = usize::from(m[3]);
                            let (ink, c) = (ink_of(m[2]), palette.get(k).copied().unwrap_or(crate::render::INK));
                            let fill = colors.get(k).is_some_and(|c| c.1);
                            if let Some(p) = row.get_mut(x) {
                                if p.iter().all(|v| *v == 0.0) {
                                    touched.push(x);
                                }
                                let o = if fill { 4 } else { 0 };
                                for (i, v) in
                                    [ink, f64::from(c.0) * ink, f64::from(c.1) * ink, f64::from(c.2) * ink].into_iter().enumerate()
                                {
                                    if let Some(s) = p.get_mut(o + i) {
                                        *s += v;
                                    }
                                }
                            }
                        }
                    }
                    touched.sort_unstable();
                    for x in touched.drain(..) {
                        let Some(p) = row.get_mut(x) else { continue };
                        for (o, fill) in [(4, true), (0, false)] {
                            let ink = p.get(o).copied().unwrap_or(0.0);
                            if ink > 0.0 {
                                let ch = |i: usize| (p.get(o + i).copied().unwrap_or(0.0) / ink).round().clamp(0.0, 255.0) as u8;
                                out.cells.push(Inked { x: x as u16, rgb: Rgb(ch(1), ch(2), ch(3)), ink: ink_code(ink), fill });
                            }
                        }
                        *p = [0.0; 8];
                    }
                    if out.rows.last().is_some_and(|r| r.1 as usize == out.cells.len()) {
                        out.rows.pop();
                    }
                }
                total += out.cells.len();
                if total > MOST_CELLS {
                    return None;
                }
                pictures.push(out);
            }
            runs_out.push(pictures);
        }
        Some(Picture { cell: self.cell, lo: self.lo, nx: self.nx, ny: self.ny, runs: runs_out })
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

/// The colour among `colors` of `fill`'s kind nearest `c` (the view's ink as [`crate::render::INK`]).
fn nearest(colors: &[(Option<Rgb>, bool)], c: Option<Rgb>, fill: bool) -> u8 {
    let rgb = |c: Option<Rgb>| c.unwrap_or(crate::render::INK);
    let d = |a: Rgb, b: Rgb| {
        let (r, g, b2) = (i32::from(a.0) - i32::from(b.0), i32::from(a.1) - i32::from(b.1), i32::from(a.2) - i32::from(b.2));
        r * r + g * g + b2 * b2
    };
    colors.iter().enumerate().filter(|(_, k)| k.1 == fill).min_by_key(|(_, k)| d(rgb(k.0), rgb(c))).map_or(0, |(i, _)| i as u8)
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

/// One layer of a run being summed: its grid, and the ink it has laid so far, piece by piece (in order of its cells, one
/// each, up to `merged`). A line's ink is a line a cell long through a cell being 1; a fill's, the share of the cell it covers.
/// A fill's and a line's are told apart by their colours ([`Lod::colors`]).
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

    /// `amount` of ink added to cell `c`, in what `ink` draws.
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

    /// What's laid, merged: one for each cell and colour's kind, in their order.
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

    /// `amount` of ink spread over the cells box `b` covers, by how much of each it covers.
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

/// The box of a text `size` (width, height) at `at`, turned `angle` degrees and aligned so: low x, low y, high x, high y; none
/// for a text of no height or of numbers that aren't.
fn text_box(at: P2, (w, h): (f64, f64), angle: f64, (ah, av): (Horizontal, Vertical)) -> Option<[f64; 4]> {
    if !(w.is_finite() && h.is_finite() && w >= 0.0 && h > 0.0) {
        return None;
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
    let [lo, hi] = crate::dxf::union(corners.map(|p| [p, p]))?;
    [lo.0, lo.1, hi.0, hi.1].iter().all(|x| x.is_finite()).then_some([lo.0, lo.1, hi.0, hi.1])
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

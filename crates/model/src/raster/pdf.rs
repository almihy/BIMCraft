//! A PDF's pages read for Import PDF, through hayro (pure Rust, Apache-2.0 OR MIT, so it builds for the browser too): each
//! page's size, its pixels at a resolution, and what it draws as vectors (its stroked and filled paths, and the text it
//! shows) in millimetres on the paper, from its bottom left corner, y up, as the pixels have it. The file itself is not kept:
//! a project holds the pixels and the lines.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    self as interpret, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, ImageDrawProps, InterpreterCache, InterpreterSettings,
    Paint, SoftMask, TransformExt,
};
use hayro::hayro_syntax::object::{Array, Dict, Name, Number, String as PdfString};
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::kurbo::{Affine, BezPath, CubicBez, ParamCurve, PathEl, Point, QuadBez, Rect};

/// Millimetres in a point, PDF's unit.
pub const MM_PER_PT: f64 = 25.4 / 72.0;
/// The most pages a file may have to be read.
pub const MAX_PAGES: usize = 10_000;
/// The most path segments and texts kept of one page: past them the rest is left out, with a note.
pub const MAX_SEGMENTS: usize = 400_000;
/// The most points one path may become: a path that would be more is left out.
pub const MAX_PATH_POINTS: usize = 100_000;
/// The most points the curves of one page are worked into, kept or not: past it, the rest of the page's paths are left
/// out, so a hostile file's curves can't hold up the read.
const MAX_WORK: usize = 4 * MAX_SEGMENTS;
/// How far a curve's chords may stray from it, mm on the paper.
const FLATNESS: f64 = 0.05;
/// The most chords one piece of a curve is cut into.
const MAX_CHORDS: usize = 4096;
/// How many times, at most, a curve far bigger than the box it is clipped to is halved to find the parts of it in the box.
const MAX_HALVINGS: u32 = 48;

/// A PDF file opened: its pages.
pub struct Document {
    pdf: Pdf,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} pages)", self.pages())
    }
}

/// What a page draws as vectors, mm on the paper: lines (open or closed runs of points), the outlines of what it fills,
/// and its texts; each with its colour and, for a stroke, its width. Read with [`Document::vectors`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vectors {
    pub strokes: Vec<Run>,
    pub fills: Vec<Run>,
    pub texts: Vec<Text>,
    /// The paths left out past [`MAX_SEGMENTS`], [`MAX_PATH_POINTS`] or the work a page may take.
    pub left_out: usize,
    /// The character maps (CMaps) its text asked for that BIMCraft doesn't hold, by name: text in them (Chinese, Japanese
    /// or Korean, mostly) is missing from the lines and may show wrongly in the picture.
    pub cmaps: Vec<String>,
}

/// A run of points a page draws, mm, and whether it closes; its colour, and a stroke's width, mm.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub pts: Vec<[f64; 2]>,
    pub closed: bool,
    pub color: [u8; 3],
    pub width: f64,
}

/// A text a page shows: what it says, where its first letter stands, how tall its letters are and the turn of its baseline,
/// degrees counter-clockwise, all on the paper in mm.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub at: [f64; 2],
    pub height: f64,
    pub angle: f64,
    pub color: [u8; 3],
}

impl Document {
    /// The file's bytes opened, or why they can't be.
    pub fn open(bytes: Vec<u8>) -> Result<Document, String> {
        if !bytes.starts_with(b"%PDF") && !bytes.get(..1024).unwrap_or(&bytes).windows(4).any(|w| w == b"%PDF") {
            return Err("it isn't a PDF file".into());
        }
        let pdf = Pdf::new(bytes).map_err(|e| match e {
            LoadPdfError::Decryption(_) => {
                "it is password-protected: PDF files with a password can't be imported; remove the password first".to_string()
            }
            LoadPdfError::Invalid => "it isn't a PDF BIMCraft can read".to_string(),
        })?;
        let n = pdf.pages().len();
        if n == 0 {
            return Err("it has no pages".into());
        }
        if n > MAX_PAGES {
            return Err(format!("it has {n} pages; a file to import has up to {MAX_PAGES}"));
        }
        Ok(Document { pdf })
    }

    /// How many pages it has.
    pub fn pages(&self) -> usize {
        self.pdf.pages().len()
    }

    fn page(&self, page: usize) -> Result<&Page<'_>, String> {
        let n = self.pages();
        page.checked_sub(1).and_then(|i| self.pdf.pages().get(i)).ok_or_else(|| format!("it has no page {page}: its pages are 1 to {n}"))
    }

    /// The size of page `page` (from 1), mm, as it's shown: turned as the page says, its user unit taken in.
    pub fn size(&self, page: usize) -> Result<[f64; 2], String> {
        let p = self.page(page)?;
        let (w, h) = p.render_dimensions();
        let k = user_unit(p) * MM_PER_PT;
        Ok([f64::from(w) * k, f64::from(h) * k])
    }

    /// Page `page`'s pixels at `dpi` dots an inch: its width, height and RGBA, on white.
    pub fn render(&self, page: usize, dpi: f64) -> Result<(u32, u32, Vec<u8>), String> {
        self.render_into(page, dpi, |w, h, rgba| (w, h, rgba.to_vec()))
    }

    /// Page `page`'s pixels at `dpi` dots an inch, handed to `f` (its width, height and RGBA, on white) while they are drawn,
    /// so they needn't be copied: what `f` makes of them.
    pub fn render_into<T>(&self, page: usize, dpi: f64, f: impl FnOnce(u32, u32, &[u8]) -> T) -> Result<T, String> {
        let p = self.page(page)?;
        let k = (dpi / 72.0 * user_unit(p)) as f32;
        let (w, h) = p.render_dimensions();
        let (pw, ph) = ((w * k).floor(), (h * k).floor());
        if !(pw >= 1.0 && ph >= 1.0 && pw <= f32::from(u16::MAX) && ph <= f32::from(u16::MAX)) {
            return Err(format!("its page {page} would be {pw} by {ph} px at {dpi} dpi"));
        }
        let cache = hayro::RenderCache::new();
        let white = hayro::vello_cpu::color::palette::css::WHITE;
        let settings = hayro::PixmapSettings { x_scale: k, y_scale: k, bg_color: white };
        let pixmap = hayro::render(p, &cache, &InterpreterSettings::default(), &hayro::RenderSettings::default(), &settings);
        let (w, h) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        // On white, every pixel is opaque: premultiplied is as it is.
        Ok(f(w, h, pixmap.data_as_u8_slice()))
    }

    /// What page `page` draws as vectors, mm on the paper from its bottom left corner, y up: only what lies on the page
    /// (its crop box, as it shows) and inside the clipping paths it sets, cut where it leaves them.
    pub fn vectors(&self, page: usize) -> Result<Vectors, String> {
        let p = self.page(page)?;
        let [w, h] = self.size(page)?;
        // To mm, y down from the top left as the pixels are; then flipped, so y runs up from the bottom.
        let to_mm =
            Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, h]) * Affine::scale(user_unit(p) * MM_PER_PT) * p.initial_transform(true).to_kurbo();
        let page_box = Rect::new(0.0, 0.0, w, h);
        let cache = InterpreterCache::new();
        // The character maps the text asks for, none of which are held (hayro's own are left out of the build).
        let asked: Arc<Mutex<BTreeSet<String>>> = Arc::default();
        let heard = Arc::clone(&asked);
        let settings = InterpreterSettings {
            cmap_resolver: Arc::new(move |name: hayro::hayro_interpret::hayro_cmap::CMapName<'_>| -> Option<&'static [u8]> {
                if let Ok(mut a) = heard.lock()
                    && a.len() < 64
                {
                    a.insert(String::from_utf8_lossy(name.to_bytes()).chars().take(64).collect());
                }
                None
            }),
            ..InterpreterSettings::default()
        };
        let mut ctx = Context::new(to_mm, page_box, &cache, p.xref(), settings);
        let mut device = Collect { out: Vectors::default(), held: 0, worked: 0, clips: vec![page_box] };
        interpret::interpret_page(p, &mut ctx, &mut device);
        let mut out = device.out;
        out.cmaps = asked.lock().map(|a| a.iter().cloned().collect()).unwrap_or_default();
        Ok(out)
    }

    /// What page `page` says one millimetre of it stands for, mm, when it says (a viewport's measure, PDF's `/VP`, whose
    /// rectilinear `/X` number format gives the real length of a point): the scale it was drawn at, 100 at 1 : 100.
    pub fn scale(&self, page: usize) -> Option<f64> {
        let p = self.page(page).ok()?;
        let viewports: Array<'_> = p.raw().get(b"VP")?;
        viewports.iter::<Dict<'_>>().find_map(|vp| {
            let measure: Dict<'_> = vp.get(b"Measure")?;
            let subtype: Name<'_> = measure.get(b"Subtype")?;
            if subtype.as_ref() != b"RL" {
                return None;
            }
            let x: Array<'_> = measure.get(b"X")?;
            let first: Dict<'_> = x.iter::<Dict<'_>>().next()?;
            let c = first.get::<Number>(b"C")?.as_f64();
            let unit: PdfString<'_> = first.get(b"U")?;
            let mm = unit_mm(&String::from_utf8_lossy(unit.as_bytes()))?;
            // A point of the page is `c` of the unit: mm of the building to a mm of paper.
            let k = c * mm / (MM_PER_PT * user_unit(p));
            (k.is_finite() && k > 0.0 && k <= 1e7).then_some(k)
        })
    }
}

/// Millimetres in a length unit as a PDF's measure names it.
fn unit_mm(unit: &str) -> Option<f64> {
    Some(match unit.trim().to_ascii_lowercase().trim_end_matches('.') {
        "mm" | "millimeter" | "millimeters" | "millimetre" | "millimetres" => 1.0,
        "cm" | "centimeter" | "centimeters" | "centimetre" | "centimetres" => 10.0,
        "m" | "meter" | "meters" | "metre" | "metres" => 1000.0,
        "km" => 1e6,
        "in" | "inch" | "inches" | "\"" => 25.4,
        "ft" | "foot" | "feet" | "'" | "ft-in" => 304.8,
        "yd" | "yard" | "yards" => 914.4,
        "pt" | "point" | "points" => MM_PER_PT,
        _ => return None,
    })
}

/// A page's user unit: how many points one of its units is (PDF 1.6's `/UserUnit`), 1 unless it says.
fn user_unit(p: &Page<'_>) -> f64 {
    p.raw().get::<Number>(b"UserUnit").map(|n| n.as_f64()).filter(|u| u.is_finite() && *u > 0.0 && *u <= 75_000.0).unwrap_or(1.0)
}

/// The device a page is interpreted on to read its vectors.
struct Collect {
    out: Vectors,
    /// Points kept so far.
    held: usize,
    /// Points the page's curves were worked into so far, kept or not.
    worked: usize,
    /// The boxes what's drawn is clipped to, mm: the page's, then each clipping path's within the one before.
    clips: Vec<Rect>,
}

/// Whether boxes `a` and `b` overlap or touch.
fn meets(a: Rect, b: Rect) -> bool {
    a.x0 <= b.x1 && b.x0 <= a.x1 && a.y0 <= b.y1 && b.y0 <= a.y1
}

/// The box round points `pts`.
fn around(pts: &[Point]) -> Rect {
    pts.iter().fold(Rect::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY), |r, q| {
        Rect::new(r.x0.min(q.x), r.y0.min(q.y), r.x1.max(q.x), r.y1.max(q.y))
    })
}

/// A curve (its control points: a quadratic's three or a cubic's four) in chords, their ends pushed on `out` after its
/// start. Only the parts of it near box `clip` are worked into chords; the rest become straight lines, which the clip leaves
/// out as it would the curve (a curve lies inside the box round its control points). `work` counts the chords: false once
/// it passes [`MAX_WORK`].
fn chords(ctrl: &[Point], clip: Rect, depth: u32, work: &mut usize, out: &mut Vec<Point>) -> bool {
    let Some(&end) = ctrl.last() else { return true };
    let b = around(ctrl);
    let near = Rect::new(clip.x0 - 1.0, clip.y0 - 1.0, clip.x1 + 1.0, clip.y1 + 1.0);
    if !meets(b, near) {
        out.push(end);
        *work += 1;
        return *work <= MAX_WORK;
    }
    let side = (clip.x1 - clip.x0).max(clip.y1 - clip.y0).max(1.0);
    if (b.x1 - b.x0).max(b.y1 - b.y0) > 4.0 * side && depth < MAX_HALVINGS {
        let halves: [Vec<Point>; 2] = match *ctrl {
            [a, c, d] => {
                let (l, r) = QuadBez::new(a, c, d).subdivide();
                [vec![l.p0, l.p1, l.p2], vec![r.p0, r.p1, r.p2]]
            }
            [a, c, d, e] => {
                let (l, r) = CubicBez::new(a, c, d, e).subdivide();
                [vec![l.p0, l.p1, l.p2, l.p3], vec![r.p0, r.p1, r.p2, r.p3]]
            }
            _ => {
                out.push(end);
                return true;
            }
        };
        return halves.iter().all(|h| chords(h, clip, depth + 1, work, out));
    }
    // Wang's formula: how many chords keep within FLATNESS of the curve.
    let second = |a: Point, c: Point, d: Point| (a.to_vec2() - 2.0 * c.to_vec2() + d.to_vec2()).hypot();
    let (m, k) = match *ctrl {
        [a, c, d] => (second(a, c, d), 0.25),
        [a, c, d, e] => (second(a, c, d).max(second(c, d, e)), 0.75),
        _ => (0.0, 0.0),
    };
    let n = (k * m / FLATNESS).sqrt().ceil();
    let n = if n.is_finite() { (n as usize).clamp(1, MAX_CHORDS) } else { 1 };
    *work += n;
    if *work > MAX_WORK {
        return false;
    }
    for i in 1..=n {
        let t = i as f64 / n as f64;
        out.push(match *ctrl {
            [a, c, d] => QuadBez::new(a, c, d).eval(t),
            [a, c, d, e] => CubicBez::new(a, c, d, e).eval(t),
            _ => end,
        });
    }
    true
}

/// The parts of polyline `pts` (closed back to its start when `closed`) inside box `r`, each an open run of two or more
/// points, Liang and Barsky's clipping of each segment: a stroke as it shows inside a clip. A polyline wholly inside comes
/// back whole, its start again at its end when it closes.
fn clip_lines(pts: &[[f64; 2]], closed: bool, r: Rect) -> Vec<Vec<[f64; 2]>> {
    let inside = |q: &[f64; 2]| q[0] >= r.x0 && q[0] <= r.x1 && q[1] >= r.y0 && q[1] <= r.y1;
    if pts.iter().all(inside) {
        let mut all = pts.to_vec();
        if closed && let Some(&first) = pts.first() {
            all.push(first);
        }
        return vec![all];
    }
    let n = pts.len();
    let segments = if closed { n } else { n.saturating_sub(1) };
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut run: Vec<[f64; 2]> = Vec::new();
    let end = |run: &mut Vec<[f64; 2]>, runs: &mut Vec<Vec<[f64; 2]>>| {
        let done = std::mem::take(run);
        if done.len() > 1 {
            runs.push(done);
        }
    };
    for i in 0..segments {
        let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) else { break };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
        let mut keep = true;
        for (p, q) in [(-dx, a[0] - r.x0), (dx, r.x1 - a[0]), (-dy, a[1] - r.y0), (dy, r.y1 - a[1])] {
            if p == 0.0 {
                keep &= q >= 0.0;
            } else if p < 0.0 {
                t0 = t0.max(q / p);
            } else {
                t1 = t1.min(q / p);
            }
        }
        if !keep || t0 > t1 {
            end(&mut run, &mut runs);
            continue;
        }
        let (s, e) = ([a[0] + dx * t0, a[1] + dy * t0], [a[0] + dx * t1, a[1] + dy * t1]);
        if run.last() != Some(&s) {
            end(&mut run, &mut runs);
            run.push(s);
        }
        run.push(e);
        if t1 < 1.0 {
            end(&mut run, &mut runs);
        }
    }
    end(&mut run, &mut runs);
    runs
}

/// Ring `pts` cut to box `r`, Sutherland and Hodgman's clipping by each side of the box in turn: a fill as it shows inside
/// a clip. Empty when none of it is inside.
fn clip_ring(pts: &[[f64; 2]], r: Rect) -> Vec<[f64; 2]> {
    let mut ring = pts.to_vec();
    // Each side of the box: which coordinate, its line, and whether inside is past it (above or right of it).
    for (axis, at, past) in [(0, r.x0, true), (0, r.x1, false), (1, r.y0, true), (1, r.y1, false)] {
        let inside = |q: &[f64; 2]| if past { q[axis] >= at } else { q[axis] <= at };
        let cross = |a: [f64; 2], b: [f64; 2]| {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        };
        let n = ring.len();
        let mut next = Vec::with_capacity(n + 4);
        for i in 0..n {
            let (Some(&a), Some(&b)) = (ring.get(i), ring.get((i + 1) % n)) else { break };
            match (inside(&a), inside(&b)) {
                (true, true) => next.push(b),
                (true, false) => next.push(cross(a, b)),
                (false, true) => {
                    next.push(cross(a, b));
                    next.push(b);
                }
                (false, false) => {}
            }
        }
        ring = next;
        if ring.is_empty() {
            break;
        }
    }
    ring.dedup();
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 { Vec::new() } else { ring }
}

/// Runs of points: each one's points and whether it closes.
type Runs = Vec<(Vec<[f64; 2]>, bool)>;

impl Collect {
    /// The box what's drawn now is clipped to.
    pub fn clip(&self) -> Rect {
        self.clips.last().copied().unwrap_or(Rect::ZERO)
    }

    /// Runs of points a path draws, through `t`: its curves in chords, and of them only what lies near the clip. None when
    /// it is left out, past the points a page or a path may have.
    fn runs(&mut self, path: &BezPath, t: Affine) -> Option<Runs> {
        if self.held >= MAX_SEGMENTS || self.worked > MAX_WORK {
            return None;
        }
        let clip = self.clip();
        let placed = t * path.clone();
        let b = placed.control_box();
        if !(b.x0.is_finite() && b.y0.is_finite() && b.x1.is_finite() && b.y1.is_finite()) || !meets(b, clip) {
            return Some(Vec::new());
        }
        // Its subpaths as points, and whether each closes.
        let mut subpaths: Vec<(Vec<Point>, bool)> = Vec::new();
        let mut run: Vec<Point> = Vec::new();
        let mut points = 0;
        let end = |run: &mut Vec<Point>, closed: bool, subpaths: &mut Vec<(Vec<Point>, bool)>| {
            let done = std::mem::take(run);
            if done.len() > 1 {
                subpaths.push((done, closed));
            }
        };
        for el in placed.elements() {
            let before = run.len();
            let ok = match *el {
                PathEl::MoveTo(q) => {
                    end(&mut run, false, &mut subpaths);
                    run.push(q);
                    true
                }
                PathEl::LineTo(q) => {
                    run.push(q);
                    true
                }
                PathEl::QuadTo(c, q) => {
                    let from = run.last().copied().unwrap_or(q);
                    chords(&[from, c, q], clip, 0, &mut self.worked, &mut run)
                }
                PathEl::CurveTo(c, d, q) => {
                    let from = run.last().copied().unwrap_or(q);
                    chords(&[from, c, d, q], clip, 0, &mut self.worked, &mut run)
                }
                PathEl::ClosePath => {
                    let start = run.first().copied();
                    end(&mut run, true, &mut subpaths);
                    // A path goes on from where it closed.
                    run.extend(start);
                    true
                }
            };
            points += run.len().saturating_sub(before);
            if !ok || points > MAX_PATH_POINTS {
                return None;
            }
        }
        end(&mut run, false, &mut subpaths);
        Some(
            subpaths
                .into_iter()
                .map(|(pts, closed)| {
                    let mut pts: Vec<[f64; 2]> = pts.into_iter().map(pt).collect();
                    pts.dedup();
                    if closed && pts.len() > 2 && pts.first() == pts.last() {
                        pts.pop();
                    }
                    (pts, closed)
                })
                .filter(|(pts, _)| pts.len() > 1 && pts.iter().flatten().all(|v| v.is_finite()))
                .collect(),
        )
    }

    /// Keeps the runs a path draws, cut to the clip: a stroke's lines (at `width`), else the outlines of a fill.
    fn keep(&mut self, runs: Option<Runs>, color: [u8; 3], width: Option<f64>) {
        let Some(runs) = runs else {
            self.out.left_out += 1;
            return;
        };
        let clip = self.clip();
        let mut cut: Runs = Vec::new();
        for (pts, closed) in runs {
            if width.is_some() {
                let pieces = clip_lines(&pts, closed, clip);
                // A closed run wholly inside stays closed; one the clip cuts is open where it was cut.
                if closed && pieces.len() == 1 && pieces.first().is_some_and(|p| p.len() == pts.len() + 1) {
                    cut.push((pts, true));
                } else {
                    cut.extend(pieces.into_iter().map(|p| (p, false)));
                }
            } else {
                let ring = clip_ring(&pts, clip);
                if !ring.is_empty() {
                    cut.push((ring, true));
                }
            }
        }
        for (pts, closed) in cut {
            if self.held + pts.len() > MAX_SEGMENTS {
                self.out.left_out += 1;
                continue;
            }
            self.held += pts.len();
            let run = Run { pts, closed, color, width: width.unwrap_or(0.0) };
            if width.is_some() { self.out.strokes.push(run) } else { self.out.fills.push(run) }
        }
    }
}

fn pt(q: Point) -> [f64; 2] {
    [q.x, q.y]
}

/// A paint's colour, or black for a pattern.
fn rgb(paint: &Paint<'_>) -> [u8; 3] {
    match paint {
        Paint::Color(c) => {
            let [r, g, b, _] = c.to_rgba().to_rgba8();
            [r, g, b]
        }
        Paint::Pattern(_) => [0, 0, 0],
    }
}

/// How much `t` stretches a length, on the whole: the square root of its area's scale.
fn stretch(t: Affine) -> f64 {
    t.determinant().abs().sqrt()
}

impl<'a> Device<'a> for Collect {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let color = rgb(&props.paint);
        if matches!(mode, DrawMode::Invisible) {
            return;
        }
        let runs = self.runs(path, props.transform);
        match mode {
            DrawMode::Stroke(s) => {
                let width = f64::from(s.line_width) * stretch(props.transform);
                self.keep(runs, color, Some(width));
            }
            DrawMode::Fill(_) => self.keep(runs, color, None),
            DrawMode::FillAndStroke(_, s) => {
                let width = f64::from(s.line_width) * stretch(props.transform);
                self.keep(runs, color, Some(width));
            }
            DrawMode::Invisible => {}
        }
    }

    fn push_clip_path(&mut self, clip: &ClipPath) {
        // The box round the clipping path, within the clip before: what is drawn is cut to it.
        let b = clip.path.control_box();
        let now = self.clip();
        let inner = Rect::new(b.x0.max(now.x0), b.y0.max(now.y0), b.x1.min(now.x1), b.y1.min(now.y1));
        let shows = inner.x0 <= inner.x1 && inner.y0 <= inner.y1 && inner.x0.is_finite() && inner.y1.is_finite();
        // Through a clip of nothing, nothing shows: a box far off the page.
        self.clips.push(if shows { inner } else { Rect::new(-2e9, -2e9, -1e9, -1e9) });
    }

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}

    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, props: DrawProps<'a>, mode: &DrawMode) {
        if matches!(mode, DrawMode::Invisible) || self.held >= MAX_SEGMENTS {
            return;
        }
        let glyphs = run.glyphs();
        let mut text = String::new();
        for g in glyphs {
            match g.as_unicode() {
                Some(hayro::hayro_interpret::hayro_cmap::BfString::Char(c)) => text.push(c),
                Some(hayro::hayro_interpret::hayro_cmap::BfString::String(s)) => text.push_str(&s),
                None => {}
            }
        }
        let text: String = text.trim().chars().take(crate::model::MAX_TEXT).collect();
        let Some(first) = glyphs.first() else { return };
        if text.is_empty() {
            return;
        }
        // A glyph's space is an em a side: its height is the em's, placed.
        let t = props.transform * first.transform();
        let origin = t * Point::ORIGIN;
        let up = t * Point::new(0.0, 1.0) - origin;
        let along = t * Point::new(1.0, 0.0) - origin;
        // Capitals stand about seven tenths of an em.
        let height = up.hypot() * 0.7;
        let angle = along.y.atan2(along.x).to_degrees().rem_euclid(360.0);
        let clip = self.clip();
        if !(height.is_finite() && height > 0.0 && origin.x.is_finite() && origin.y.is_finite())
            || !(origin.x >= clip.x0 && origin.x <= clip.x1 && origin.y >= clip.y0 && origin.y <= clip.y1)
        {
            return;
        }
        self.held += 1;
        self.out.texts.push(Text { text, at: pt(origin), height, angle, color: rgb(&props.paint) });
    }

    fn draw_image(&mut self, _: interpret::Image<'a, '_>, _: ImageDrawProps<'a>) {}

    fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn pop_transparency_group(&mut self) {}
}

impl Vectors {
    /// Whether it draws anything as vectors.
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.fills.is_empty() && self.texts.is_empty()
    }

    /// Itself as an ASCII DXF in mm, which [`crate::cad::Cad`] reads: its strokes on layer `Lines`, the outlines of its
    /// fills on `Fills` and its texts on `Text`, each in its colour and a stroke at its width.
    pub fn dxf(&self) -> Vec<u8> {
        let mut s = String::with_capacity(64 * (self.strokes.len() + self.fills.len() + self.texts.len()) + 256);
        s.push_str("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n0\nTABLE\n2\nLAYER\n");
        for name in ["Lines", "Fills", "Text"] {
            let _ = write!(s, "0\nLAYER\n2\n{name}\n70\n0\n62\n7\n6\nCONTINUOUS\n");
        }
        s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        let color = |c: [u8; 3]| (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
        for (layer, runs) in [("Lines", &self.strokes), ("Fills", &self.fills)] {
            for r in runs.iter() {
                let _ = write!(s, "0\nLWPOLYLINE\n8\n{layer}\n420\n{}\n90\n{}\n70\n{}\n", color(r.color), r.pts.len(), u8::from(r.closed));
                if r.width > 0.0 {
                    // DXF's line weights are hundredths of a mm, the thinnest 0.05 mm.
                    let _ = write!(s, "370\n{}\n", ((r.width * 100.0).round() as i64).clamp(5, 211));
                }
                for [x, y] in &r.pts {
                    let _ = write!(s, "10\n{x:.4}\n20\n{y:.4}\n");
                }
            }
        }
        for t in &self.texts {
            let text: String = t.text.chars().filter(|c| !c.is_control()).collect();
            let _ = write!(
                s,
                "0\nTEXT\n8\nText\n420\n{}\n10\n{:.4}\n20\n{:.4}\n40\n{:.4}\n50\n{:.4}\n1\n{text}\n",
                color(t.color),
                t.at[0],
                t.at[1],
                t.height,
                t.angle
            );
        }
        s.push_str("0\nENDSEC\n0\nEOF\n");
        s.into_bytes()
    }
}

/// A page of a PDF [`sample_pdf`] makes, for tests: its content stream, its media box `size` points wide and high, and,
/// when given, its crop box (`[x0, y0, x1, y1]` points), its turn (`/Rotate`, degrees clockwise), its user unit and a
/// viewport's measure saying a point is `measure` (a length and its unit, `(0.0352778, "m")` at 1 : 100).
#[doc(hidden)]
#[derive(Clone, Debug, Default)]
pub struct SamplePage {
    pub content: String,
    pub size: [f64; 2],
    pub crop: Option<[f64; 4]>,
    pub rotate: i32,
    pub user_unit: Option<f64>,
    pub measure: Option<(f64, String)>,
}

/// A PDF of `pages`, what tests import. Every page has the fonts `/F1` (Helvetica) and `/F2` (a Chinese font, set in the
/// character map `UniGB-UCS2-H`, which isn't embedded) and the form `/X1`, whose content is `form` (a form may draw itself:
/// `/X1 Do`).
#[doc(hidden)]
pub fn sample_pdf(pages: &[SamplePage], form: &str) -> Vec<u8> {
    // 1 the catalog, 2 the pages, 3 to 6 the fonts, 7 the form; then each page and its content.
    let first = 8;
    let kids: Vec<String> = (0..pages.len()).map(|i| format!("{} 0 R", first + 2 * i)).collect();
    let resources = "/Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << /X1 7 0 R >> >>";
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), pages.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /UniGB-UCS2-H /DescendantFonts [5 0 R] >>".to_string(),
        "<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 2 >> /FontDescriptor 6 0 R >>".to_string(),
        "<< /Type /FontDescriptor /FontName /STSong-Light /Flags 4 /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 880 /Descent -120 /CapHeight 700 /StemV 80 >>".to_string(),
        format!("<< /Type /XObject /Subtype /Form /BBox [-1e9 -1e9 1e9 1e9] {resources} /Length {} >>\nstream\n{form}\nendstream", form.len()),
    ];
    for (i, pg) in pages.iter().enumerate() {
        let [w, h] = pg.size;
        let mut extra = String::new();
        if let Some([x0, y0, x1, y1]) = pg.crop {
            let _ = write!(extra, " /CropBox [{x0} {y0} {x1} {y1}]");
        }
        if pg.rotate != 0 {
            let _ = write!(extra, " /Rotate {}", pg.rotate);
        }
        if let Some(u) = pg.user_unit {
            let _ = write!(extra, " /UserUnit {u}");
        }
        if let Some((c, u)) = &pg.measure {
            let _ = write!(
                extra,
                " /VP [<< /Type /Viewport /BBox [0 0 {w} {h}] /Measure << /Type /Measure /Subtype /RL /R (scale) /X [<< /Type /NumberFormat /U ({u}) /C {c} >>] /D [<< /Type /NumberFormat /U ({u}) /C 1 >>] /A [<< /Type /NumberFormat /U ({u}2) /C 1 >>] >> >>]"
            );
        }
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents {} 0 R {resources}{extra} >>",
            first + 2 * i + 1
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{}\nendstream", pg.content.len(), pg.content));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    out
}

/// A PDF of one page `size` points wide and high whose content stream is `content`, with a viewport's measure saying a point
/// is `measure` (a length and its unit, `(0.0352778, "m")` at 1 : 100) when it's given: what tests import.
#[doc(hidden)]
pub fn sample(content: &str, size: [f64; 2], measure: Option<(f64, &str)>) -> Vec<u8> {
    let page = SamplePage { content: content.into(), size, measure: measure.map(|(c, u)| (c, u.to_string())), ..SamplePage::default() };
    sample_pdf(&[page], "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An A4 page with a square 100 points a side (35.28 mm) whose lower left corner is 72 points (25.4 mm) in.
    fn square() -> Vec<u8> {
        sample("0 0 1 RG 2 w 72 72 m 172 72 l 172 172 l 72 172 l h S BT /F1 12 Tf 80 180 Td (WC 1) Tj ET", [595.0, 842.0], None)
    }

    #[test]
    fn a_page_reads_its_size_pixels_and_lines_in_mm() {
        let d = Document::open(square()).unwrap();
        assert_eq!(d.pages(), 1);
        let [w, h] = d.size(1).unwrap();
        assert!((w - 209.9).abs() < 0.1 && (h - 297.0).abs() < 0.1, "{w} {h}");
        let (pw, ph, rgba) = d.render(1, 72.0).unwrap();
        assert_eq!((pw, ph), (595, 842));
        // The square's bottom side, 72 points up from the bottom, is blue; the middle of the square is white.
        let px = |x: u32, y: u32| &rgba[((y * pw + x) * 4) as usize..((y * pw + x) * 4 + 3) as usize];
        assert_eq!(px(120, 842 - 72), [0, 0, 255]);
        assert_eq!(px(120, 842 - 120), [255, 255, 255]);
        let v = d.vectors(1).unwrap();
        assert_eq!(v.strokes.len(), 1, "{v:?}");
        let s = &v.strokes[0];
        assert!(s.closed && s.pts.len() == 4 && s.color == [0, 0, 255], "{s:?}");
        let near = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01;
        assert!(near(s.pts[0], [25.4, 25.4]) && near(s.pts[2], [172.0 * MM_PER_PT, 172.0 * MM_PER_PT]), "{s:?}");
        assert!((s.width - 2.0 * MM_PER_PT).abs() < 1e-6);
        assert_eq!(v.texts.len(), 1, "{v:?}");
        assert_eq!(v.texts[0].text, "WC 1");
        assert!(near(v.texts[0].at, [80.0 * MM_PER_PT, 180.0 * MM_PER_PT]), "{:?}", v.texts[0]);
    }

    #[test]
    fn its_lines_read_as_a_drawing_in_mm() {
        let v = Document::open(square()).unwrap().vectors(1).unwrap();
        let cad = crate::cad::Cad::new("square.pdf", None, v.dxf()).unwrap();
        let dxf = cad.dxf();
        assert_eq!(dxf.unit, "mm");
        assert_eq!(dxf.entities.len(), 2);
        assert_eq!(dxf.entities[0].layer, "Lines");
        assert!(matches!(&dxf.entities[0].shape, crate::dxf::Shape::Polyline { pts, closed: true } if pts.len() == 4));
        assert!(matches!(&dxf.entities[1].shape, crate::dxf::Shape::Text { text, .. } if text == "WC 1"));
    }

    #[test]
    fn a_viewports_measure_gives_the_scale_the_page_was_drawn_at() {
        // A point is 0.0352778 m: 0.3528 mm of paper is 35.28 mm of building, 1 : 100.
        let d = Document::open(sample("0 0 m 10 0 l S", [595.0, 842.0], Some((0.035_277_8, "m")))).unwrap();
        let k = d.scale(1).unwrap();
        assert!((k - 100.0).abs() < 0.01, "{k}");
        assert_eq!(Document::open(square()).unwrap().scale(1), None);
    }

    fn page(content: &str, size: [f64; 2]) -> SamplePage {
        SamplePage { content: content.into(), size, ..SamplePage::default() }
    }

    /// Every point of every run of `v` inside `w` by `h` mm, and the box round them all.
    fn extent(v: &Vectors, w: f64, h: f64) -> [f64; 4] {
        let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for q in v.strokes.iter().chain(&v.fills).flat_map(|r| &r.pts) {
            assert!(q[0] >= -1e-6 && q[0] <= w + 1e-6 && q[1] >= -1e-6 && q[1] <= h + 1e-6, "{q:?} is off the {w} by {h} page");
            b = [b[0].min(q[0]), b[1].min(q[1]), b[2].max(q[0]), b[3].max(q[1])];
        }
        b
    }

    /// The pixel at (`x`, `y`) of a page rendered at 72 dpi, from its top left: whether it is dark.
    fn dark(d: &Document, x: u32, y: u32) -> bool {
        let (w, _, rgba) = d.render(1, 72.0).unwrap();
        let i = ((y * w + x) * 4) as usize;
        rgba[i..i + 3].iter().all(|c| *c < 100)
    }

    #[test]
    fn a_turned_page_reads_turned_and_its_lines_and_pixels_agree() {
        // 200 by 100 points turned a quarter clockwise: 100 wide and 200 high as it shows; its line along the middle stands.
        let pdf = sample_pdf(&[SamplePage { rotate: 90, ..page("0 0 0 RG 4 w 20 50 m 180 50 l S", [200.0, 100.0]) }], "");
        let d = Document::open(pdf).unwrap();
        let [w, h] = d.size(1).unwrap();
        assert!((w - 100.0 * MM_PER_PT).abs() < 1e-6 && (h - 200.0 * MM_PER_PT).abs() < 1e-6, "{w} {h}");
        let v = d.vectors(1).unwrap();
        let [x0, y0, x1, y1] = extent(&v, w, h);
        assert!((x0 - 50.0 * MM_PER_PT).abs() < 0.01 && (x1 - x0).abs() < 0.01, "upright: {v:?}");
        assert!((y0 - 20.0 * MM_PER_PT).abs() < 0.01 && (y1 - 180.0 * MM_PER_PT).abs() < 0.01, "{v:?}");
        assert!(dark(&d, 50, 100) && !dark(&d, 20, 100), "the pixels have it where the lines do");
    }

    #[test]
    fn a_crop_box_is_the_page_and_what_lies_off_it_is_left_out() {
        // A page 200 points a side cropped to its middle 100: the line across it is cut at the crop, the one below it gone.
        let content = "0 0 0 RG 4 w 0 100 m 200 100 l S 10 10 m 40 10 l S";
        let d =
            Document::open(sample_pdf(&[SamplePage { crop: Some([50.0, 50.0, 150.0, 150.0]), ..page(content, [200.0; 2]) }], "")).unwrap();
        let [w, h] = d.size(1).unwrap();
        assert!((w - 100.0 * MM_PER_PT).abs() < 1e-6 && (h - w).abs() < 1e-6, "{w} {h}");
        let v = d.vectors(1).unwrap();
        assert_eq!(v.strokes.len(), 1, "{v:?}");
        let [x0, y0, x1, y1] = extent(&v, w, h);
        assert!(x0.abs() < 1e-6 && (x1 - w).abs() < 1e-6 && (y0 - 50.0 * MM_PER_PT).abs() < 1e-6 && (y1 - y0).abs() < 1e-6, "{v:?}");
        assert!(dark(&d, 50, 50) && !dark(&d, 50, 80));
    }

    #[test]
    fn a_user_unit_makes_the_page_and_its_lines_bigger() {
        let d =
            Document::open(sample_pdf(&[SamplePage { user_unit: Some(2.0), ..page("4 w 0 50 m 100 50 l S", [100.0; 2]) }], "")).unwrap();
        let [w, h] = d.size(1).unwrap();
        assert!((w - 200.0 * MM_PER_PT).abs() < 1e-6 && (h - w).abs() < 1e-6, "{w} {h}");
        let v = d.vectors(1).unwrap();
        let [_, y0, x1, _] = extent(&v, w, h);
        assert!((y0 - 100.0 * MM_PER_PT).abs() < 1e-6 && (x1 - w).abs() < 1e-6, "{v:?}");
    }

    #[test]
    fn a_clipping_path_cuts_what_is_drawn_inside_it() {
        // Clipped to the square from (20, 20) to (80, 80) points: a line across it and a fill under it are cut to it; the
        // line drawn after the clip is let go goes across the page.
        let content = "q 20 20 60 60 re W n 4 w 0 50 m 200 50 l S 0 0 200 100 re f Q 4 w 0 150 m 200 150 l S";
        let d = Document::open(sample_pdf(&[page(content, [200.0; 2])], "")).unwrap();
        let v = d.vectors(1).unwrap();
        let k = MM_PER_PT;
        assert_eq!(v.strokes.len(), 2, "{v:?}");
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let cut = &v.strokes[0].pts;
        assert!(near(cut[0][0], 20.0 * k) && near(cut[1][0], 80.0 * k), "cut at the clip: {cut:?}");
        let whole = &v.strokes[1].pts;
        assert!(near(whole[0][0], 0.0) && near(whole[1][0], 200.0 * k), "{whole:?}");
        assert_eq!(v.fills.len(), 1, "{v:?}");
        let ring = &v.fills[0].pts;
        assert!(
            ring.iter().all(|q| q[0] >= 20.0 * k - 1e-6 && q[0] <= 80.0 * k + 1e-6 && q[1] >= 20.0 * k - 1e-6 && q[1] <= 80.0 * k + 1e-6)
        );
        let xs: Vec<f64> = ring.iter().map(|q| q[0]).collect();
        assert!(xs.iter().any(|x| near(*x, 20.0 * k)) && xs.iter().any(|x| near(*x, 80.0 * k)), "{ring:?}");
    }

    #[test]
    fn hostile_curves_far_bigger_than_the_page_are_read_quickly_and_only_on_the_page() {
        // Two hundred curves each a million million times the page, through its corner: only the parts of them on it are worked
        // into lines.
        let curve = "0 0 m 1 1 2 -1 3 0 c S ";
        let content = format!("1000000000000 0 0 1000000000000 0 0 cm 0 w {}", curve.repeat(200));
        let d = Document::open(sample_pdf(&[page(&content, [200.0; 2])], "")).unwrap();
        let started = std::time::Instant::now();
        let v = d.vectors(1).unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "{:?}", started.elapsed());
        let [w, h] = d.size(1).unwrap();
        extent(&v, w, h);
        assert_eq!(v.strokes.len(), 200, "each curve's start, on the page: {} strokes, {} left out", v.strokes.len(), v.left_out);
    }

    #[test]
    fn hostile_files_never_panic() {
        // A form that draws itself, over and over; a content stream of nonsense; a file cut short; huge and odd numbers.
        let selfish = sample_pdf(&[page("/X1 Do", [200.0; 2])], "0 0 m 10 10 l S /X1 Do");
        let d = Document::open(selfish).unwrap();
        assert!(d.vectors(1).is_ok() && d.render(1, 72.0).is_ok());
        let junk: String = (0..4000_u32)
            .map(|i| char::from(b"q Q cm m l c re W n S f BT ET Tj 9e9 -1 [ ] ( ) < > /X1 Do"[(i * 7919 % 51) as usize]))
            .collect();
        let d = Document::open(sample_pdf(&[page(&junk, [200.0; 2])], "")).unwrap();
        assert!(d.vectors(1).is_ok() && d.render(1, 72.0).is_ok());
        let odd = "99999999999999999999 0 0 -0.000000000000001 1e400 nan cm 0 0 m 1 1 l S 1 0 0 0 0 0 cm 0 0 m 5 5 l S";
        let d = Document::open(sample_pdf(&[page(odd, [200.0; 2])], "")).unwrap();
        assert!(d.vectors(1).is_ok() && d.render(1, 72.0).is_ok());
        let whole = sample_pdf(&[page("0 0 m 10 10 l S", [200.0; 2])], "");
        for cut in [10, whole.len() / 3, whole.len() / 2, whole.len() - 20] {
            if let Ok(d) = Document::open(whole[..cut].to_vec()) {
                let _ = (d.vectors(1), d.render(1, 72.0), d.scale(1));
            }
        }
    }

    #[test]
    fn text_in_a_character_map_bimcraft_doesnt_hold_is_named_in_the_notes() {
        let content = "BT /F2 12 Tf 50 50 Td <4E2D> Tj ET BT /F1 12 Tf 50 80 Td (WC) Tj ET";
        let pdf = sample_pdf(&[page(content, [200.0; 2])], "");
        let v = Document::open(pdf.clone()).unwrap().vectors(1).unwrap();
        assert_eq!(v.cmaps, vec!["UniGB-UCS2-H".to_string()], "{v:?}");
        assert!(v.texts.iter().any(|t| t.text == "WC"), "{v:?}");
        let read = crate::raster::read_pdf(pdf, 1, 72).unwrap();
        assert!(read.notes.iter().any(|n| n.contains("UniGB-UCS2-H") && n.contains("Chinese, Japanese or Korean")), "{:?}", read.notes);
        let plain = crate::raster::read_pdf(sample_pdf(&[page("BT /F1 12 Tf 50 80 Td (WC) Tj ET", [200.0; 2])], ""), 1, 72).unwrap();
        assert!(plain.notes.is_empty(), "{:?}", plain.notes);
    }

    #[test]
    fn what_isnt_a_pdf_or_has_no_such_page_says_so() {
        assert!(Document::open(b"hello".to_vec()).unwrap_err().contains("isn't a PDF"));
        let d = Document::open(square()).unwrap();
        assert!(d.size(2).unwrap_err().contains("no page 2"));
        assert!(d.size(0).is_err());
        assert!(Document::open(b"%PDF-1.4 garbage".to_vec()).is_err());
    }
}

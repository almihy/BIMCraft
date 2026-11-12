//! A PDF's pages read for Import PDF, through hayro (pure Rust, Apache-2.0 OR MIT, so it builds for the browser too): each
//! page's size, its pixels at a resolution, and what it draws as vectors (its stroked and filled paths, and the text it
//! shows) in millimetres on the paper, from its bottom left corner, y up, as the pixels have it, each on the layer (the
//! optional content group) it is drawn on. The file itself is not kept: a project holds the pixels and the lines.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use hayro::hayro_interpret::font::{Glyph, GlyphRun};
use hayro::hayro_interpret::{
    self as interpret, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageData, ImageDrawProps, InterpreterCache,
    InterpreterSettings, LumaData, Paint, SoftMask, StrokeProps, TransformExt,
};
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Number, ObjectIdentifier, String as PdfString};
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
/// The most layers (optional content groups) a file's are read of.
pub const MAX_LAYERS: usize = 4096;
/// The most pixels of a page's pictures painted, among them all: past it, the rest are left out of its picture.
const MAX_PAINTED: u64 = 4 * 36_000_000;
/// What the lines drawn on no layer of a file that has layers are on.
pub const NO_LAYER: &str = "No layer";

/// A PDF file opened: its pages, and its layers (its optional content groups, as Acrobat's Layers list them).
pub struct Document {
    pdf: Pdf,
    layers: Vec<(ObjectIdentifier, Layer)>,
}

/// A layer of a PDF, an optional content group: its name and whether the file has it off (hidden as it opens).
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub off: bool,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Document({} pages)", self.pages())
    }
}

/// What a page draws as vectors, mm on the paper: lines (open or closed runs of points), the outlines of what it fills,
/// and its texts; each with its colour, its layer and, for a stroke, its width and dashes. Read with [`Document::vectors`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vectors {
    pub strokes: Vec<Run>,
    pub fills: Vec<Run>,
    pub texts: Vec<Text>,
    /// The file's layers, which a run's or a text's `layer` counts in: every optional content group it has, whether this
    /// page draws on it or not.
    pub layers: Vec<Layer>,
    /// The paths left out past [`MAX_SEGMENTS`], [`MAX_PATH_POINTS`] or the work a page may take.
    pub left_out: usize,
    /// The character maps (CMaps) its text asked for that BIMCraft doesn't hold, by name: text in them (Chinese, Japanese
    /// or Korean, mostly) is missing from the lines and may show wrongly in the picture.
    pub cmaps: Vec<String>,
    /// The letters drawn in a way its lines can't hold (a Type 3 font's, with no Unicode for them): they show only in the
    /// picture.
    pub unlettered: usize,
    /// Whether layers the file has off couldn't be read (an encrypted file is read only as it opens): what they draw is
    /// missing from the lines.
    pub off_unread: bool,
}

/// A run of points a page draws, mm, and whether it closes; its colour, and a stroke's width, mm, and dashes (a dash and
/// the gap after it, mm, when it is dashed). `seq` is its place in the order the page draws (a fill and its stroke share
/// one, the fill first; the rings of one fill share one); `layer` the file's layer it is on, by its place in
/// [`Vectors::layers`]; `pattern` a fill painted with a pattern or a shading, not a colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub pts: Vec<[f64; 2]>,
    pub closed: bool,
    pub color: [u8; 3],
    pub width: f64,
    pub dash: Option<[f64; 2]>,
    pub seq: usize,
    pub layer: Option<usize>,
    pub pattern: bool,
}

/// A text a page shows: what it says, where its first letter stands, how tall its letters are and the turn of its baseline,
/// degrees counter-clockwise, all on the paper in mm; its place in the order the page draws and its layer, as a run's.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub at: [f64; 2],
    pub height: f64,
    pub angle: f64,
    pub color: [u8; 3],
    pub seq: usize,
    pub layer: Option<usize>,
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
        let layers = read_layers(&pdf);
        Ok(Document { pdf, layers })
    }

    /// How many pages it has.
    pub fn pages(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Its layers (optional content groups), in the order the file lists them.
    pub fn layers(&self) -> Vec<Layer> {
        self.layers.iter().map(|(_, l)| l.clone()).collect()
    }

    fn page(&self, page: usize) -> Result<&Page<'_>, String> {
        page_of(&self.pdf, page)
    }

    /// The file with every layer on: itself with one more revision appended (a catalog with no optional content, as PDF's
    /// incremental updates add one), so a layer the file has off is read too. None when it has no layers, or is encrypted
    /// (the revision would need its key), or the revision can't be read.
    fn all_on(&self) -> Option<Pdf> {
        if self.layers.is_empty() {
            return None;
        }
        let data: &[u8] = self.pdf.data().as_ref();
        if data.windows(8).any(|w| w == b"/Encrypt") {
            return None;
        }
        let xref = self.pdf.xref();
        let root = xref.root_id();
        let catalog = xref.get::<Dict<'_>>(root)?;
        let pages = catalog.get_ref(b"Pages")?;
        // Where its last cross-reference section starts: the one the revision goes on from.
        let at = data.windows(9).rposition(|w| w == b"startxref")?;
        let prev: usize = String::from_utf8_lossy(data.get(at + 9..(at + 40).min(data.len()))?).split_whitespace().next()?.parse().ok()?;
        let (n, g) = (root.obj_number, root.gen_number);
        let mut out = data.to_vec();
        out.push(b'\n');
        let obj = out.len();
        let _ = write!(Bytes(&mut out), "{n} {g} obj\n<< /Type /Catalog /Pages {} {} R >>\nendobj\n", pages.obj_number, pages.gen_number);
        let table = out.len();
        let size = n.saturating_add(1).max(u32::try_from(self.pdf.len()).unwrap_or(u32::MAX).saturating_add(1));
        let _ = write!(
            Bytes(&mut out),
            "xref\n0 1\n0000000000 65535 f \n{n} 1\n{obj:010} {:05} n \ntrailer\n<< /Size {size} /Root {n} {g} R /Prev {prev} >>\nstartxref\n{table}\n%%EOF\n",
            g.clamp(0, 65_535)
        );
        let pdf = Pdf::new(out).ok()?;
        (pdf.pages().len() == self.pages()).then_some(pdf)
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
        // Read with every layer on, so what the file has off is in the lines too, on its layer.
        let all_on = self.all_on();
        let p = match &all_on {
            Some(pdf) => page_of(pdf, page)?,
            None => p,
        };
        let mut ctx = Context::new(to_mm, page_box, &cache, p.xref(), settings);
        let ids: HashMap<ObjectIdentifier, usize> = self.layers.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
        let names: HashMap<String, usize> = self.layers.iter().enumerate().rev().map(|(i, (_, l))| (l.name.clone(), i)).collect();
        let mut device = Collect {
            out: Vectors { layers: self.layers(), ..Vectors::default() },
            held: 0,
            worked: 0,
            clips: vec![page_box],
            ids,
            names,
            oc: Vec::new(),
            seq: 0,
        };
        interpret::interpret_page(p, &mut ctx, &mut device);
        let mut out = device.out;
        out.cmaps = asked.lock().map(|a| a.iter().cloned().collect()).unwrap_or_default();
        out.off_unread = all_on.is_none() && self.layers.iter().any(|(_, l)| l.off);
        Ok(out)
    }

    /// Page `page`'s pictures (the raster images it draws) at `dpi` dots an inch, on white, handed to `f` as
    /// [`Document::render_into`] does: the part of the page its lines can't draw. With how many pictures it drew.
    pub fn pictures_into<T>(&self, page: usize, dpi: f64, f: impl FnOnce(u32, u32, &[u8]) -> T) -> Result<(T, usize), String> {
        let p = self.page(page)?;
        let k = (dpi / 72.0 * user_unit(p)) as f32;
        let (w, h) = p.render_dimensions();
        let (pw, ph) = ((w * k).floor(), (h * k).floor());
        if !(pw >= 1.0 && ph >= 1.0 && pw <= f32::from(u16::MAX) && ph <= f32::from(u16::MAX)) {
            return Err(format!("its page {page} would be {pw} by {ph} px at {dpi} dpi"));
        }
        let (pw, ph) = (pw as u32, ph as u32);
        let to_px = Affine::scale(f64::from(k)) * p.initial_transform(true).to_kurbo();
        let canvas = Rect::new(0.0, 0.0, f64::from(pw), f64::from(ph));
        let cache = InterpreterCache::new();
        let mut ctx = Context::new(to_px, canvas, &cache, p.xref(), InterpreterSettings::default());
        let mut device =
            Pictures { w: pw, h: ph, rgba: vec![255; pw as usize * ph as usize * 4], clips: vec![canvas], drawn: 0, painted: 0 };
        interpret::interpret_page(p, &mut ctx, &mut device);
        Ok((f(pw, ph, &device.rgba), device.drawn))
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

/// Page `page` (from 1) of `pdf`, or why there is none.
fn page_of(pdf: &Pdf, page: usize) -> Result<&Page<'_>, String> {
    let n = pdf.pages().len();
    page.checked_sub(1).and_then(|i| pdf.pages().get(i)).ok_or_else(|| format!("it has no page {page}: its pages are 1 to {n}"))
}

/// A byte buffer written to as text.
struct Bytes<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for Bytes<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// A PDF text string as text: UTF-16 (big-endian) after its byte order mark, UTF-8 after its own, else PDFDocEncoding,
/// read as Latin-1 (they differ only in a few marks).
fn text_string(b: &[u8]) -> String {
    if let Some(rest) = b.strip_prefix(&[0xFE, 0xFF]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else if let Some(rest) = b.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else {
        b.iter().map(|c| char::from(*c)).collect()
    }
}

/// A file's layers, its optional content groups as its catalog lists them (`/OCProperties /OCGs`), each with its name and
/// whether its default configuration (`/D`: its base state, its `/ON` and `/OFF`) has it off.
fn read_layers(pdf: &Pdf) -> Vec<(ObjectIdentifier, Layer)> {
    let xref = pdf.xref();
    let Some(catalog) = xref.get::<Dict<'_>>(xref.root_id()) else { return Vec::new() };
    let Some(props) = catalog.get::<Dict<'_>>(b"OCProperties") else { return Vec::new() };
    let Some(list) = props.get::<Array<'_>>(b"OCGs") else { return Vec::new() };
    let config = props.get::<Dict<'_>>(b"D");
    let refs = |key: &[u8]| -> Vec<ObjectIdentifier> {
        config
            .as_ref()
            .and_then(|d| d.get::<Array<'_>>(key))
            .map(|a| a.raw_iter().take(MAX_LAYERS).filter_map(|i| i.as_obj_ref()).map(ObjectIdentifier::from).collect())
            .unwrap_or_default()
    };
    let base_off = config.as_ref().and_then(|d| d.get::<Name<'_>>(b"BaseState")).is_some_and(|n| n.as_ref() == b"OFF");
    let (on, off) = (refs(b"ON"), refs(b"OFF"));
    let mut out: Vec<(ObjectIdentifier, Layer)> = Vec::new();
    for item in list.raw_iter().take(MAX_LAYERS) {
        let Some(r) = item.as_obj_ref() else { continue };
        let id = ObjectIdentifier::from(r);
        if out.iter().any(|(i, _)| *i == id) {
            continue;
        }
        let name = xref.get::<Dict<'_>>(id).and_then(|d| d.get::<PdfString<'_>>(b"Name")).map(|n| text_string(n.as_bytes()));
        let off = if base_off { !on.contains(&id) } else { off.contains(&id) };
        out.push((id, Layer { name: name.unwrap_or_default(), off }));
    }
    out
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
    /// The file's layers by their object, and by their name (a group read where its object isn't known), to their place in
    /// [`Vectors::layers`].
    ids: HashMap<ObjectIdentifier, usize>,
    names: HashMap<String, usize>,
    /// The layer of each marked-content sequence open (`BDC` to `EMC`), innermost last: one of optional content (`/OC`) is
    /// on its group (an OCMD's first group), any other on the layer of the one round it.
    oc: Vec<Option<usize>>,
    /// What the page has drawn so far, the order its runs and texts keep.
    seq: usize,
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
    fn clip(&self) -> Rect {
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

    /// The layer what is drawn now is on.
    fn layer(&self) -> Option<usize> {
        self.oc.last().copied().flatten()
    }

    /// The layer an optional content dictionary stands for: a group's own (by its object, else its name), an OCMD's first
    /// group (of `/OCGs`, else of its visibility expression `/VE`).
    fn layer_of(&self, d: &Dict<'_>) -> Option<usize> {
        if d.get::<Name<'_>>(b"Type").is_some_and(|n| n.as_ref() == b"OCMD") {
            if let Some(r) = d.get_ref(b"OCGs") {
                return self.ids.get(&ObjectIdentifier::from(r)).copied();
            }
            if let Some(a) = d.get::<Array<'_>>(b"OCGs") {
                return a.raw_iter().take(MAX_LAYERS).filter_map(|i| i.as_obj_ref()).find_map(|r| self.ids.get(&r.into()).copied());
            }
            return d.get::<Array<'_>>(b"VE").and_then(|a| self.first_in(&a, 0));
        }
        d.obj_id()
            .and_then(|id| self.ids.get(&id).copied())
            .or_else(|| d.get::<PdfString<'_>>(b"Name").and_then(|n| self.names.get(&text_string(n.as_bytes())).copied()))
    }

    /// The first group a visibility expression names, depth first.
    fn first_in(&self, a: &Array<'_>, depth: u32) -> Option<usize> {
        if depth > 16 {
            return None;
        }
        a.raw_iter().take(MAX_LAYERS).find_map(|item| match item {
            MaybeRef::Ref(r) => self.ids.get(&r.into()).copied(),
            MaybeRef::NotRef(o) => o.into_array().and_then(|inner| self.first_in(&inner, depth + 1)),
        })
    }

    /// Keeps the runs a path draws, cut to the clip: a stroke's lines (at `width`, dashed by `dash`), else the outlines
    /// of a fill (painted with a pattern when `pattern`), in the page's order at `seq`.
    fn keep(&mut self, runs: Option<Runs>, color: [u8; 3], width: Option<f64>, dash: Option<[f64; 2]>, pattern: bool, seq: usize) {
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
            let layer = self.layer();
            let run = Run { pts, closed, color, width: width.unwrap_or(0.0), dash, seq, layer, pattern };
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

/// A stroke's dash and the gap after it, mm (its dash array's first two, through `t`): none when it is solid.
fn dash_of(s: &StrokeProps, t: Affine) -> Option<[f64; 2]> {
    let k = stretch(t);
    let a = f64::from(*s.dash_array.first()?) * k;
    let b = f64::from(s.dash_array.get(1).copied().unwrap_or(*s.dash_array.first()?)) * k;
    (a.is_finite() && b.is_finite() && a >= 0.0 && b > 0.0 && a + b > 1e-3).then_some([a, b])
}

impl<'a> Device<'a> for Collect {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let color = rgb(&props.paint);
        let pattern = matches!(props.paint, Paint::Pattern(_));
        if matches!(mode, DrawMode::Invisible) {
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        match mode {
            DrawMode::Stroke(s) => {
                let runs = self.runs(path, props.transform);
                let width = f64::from(s.line_width) * stretch(props.transform);
                self.keep(runs, color, Some(width), dash_of(s, props.transform), false, seq);
            }
            DrawMode::Fill(_) => {
                let runs = self.runs(path, props.transform);
                self.keep(runs, color, None, None, pattern, seq);
            }
            DrawMode::FillAndStroke(_, s) => {
                // Filled, then its outline stroked over it: the stroke's paint is hayro's for both, so the fill is in it too.
                let runs = self.runs(path, props.transform);
                self.keep(runs.clone(), color, None, None, pattern, seq);
                let width = f64::from(s.line_width) * stretch(props.transform);
                self.keep(runs, color, Some(width), dash_of(s, props.transform), false, seq);
            }
            DrawMode::Invisible => {}
        }
    }

    fn begin_marked_content(&mut self, tag: &[u8], properties: Option<&Dict<'_>>) {
        let around = self.layer();
        let here = if tag == b"OC" { properties.and_then(|d| self.layer_of(d)).or(around) } else { around };
        if self.oc.len() < 4096 {
            self.oc.push(here);
        }
    }

    fn end_marked_content(&mut self) {
        self.oc.pop();
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
        self.seq += 1;
        let seq = self.seq;
        let glyphs = run.glyphs();
        let mut text = String::new();
        let color = rgb(&props.paint);
        for g in glyphs {
            match g.as_unicode() {
                Some(hayro::hayro_interpret::hayro_cmap::BfString::Char(c)) => text.push(c),
                Some(hayro::hayro_interpret::hayro_cmap::BfString::String(s)) => text.push_str(&s),
                // A letter with no Unicode is kept as its outline, filled: it draws as the page does, though it reads as no
                // text. A Type 3 font's letters are drawings of their own, which only the picture holds.
                None => match &**g {
                    Glyph::Outline(o) => {
                        let runs = self.runs(&o.outline(), props.transform * g.transform());
                        self.keep(runs, color, None, None, false, seq);
                    }
                    Glyph::Type3(_) => self.out.unlettered += 1,
                },
            }
        }
        let text: String = text.trim().chars().take(crate::model::MAX_TEXT).collect();
        let Some(first) = glyphs.first() else { return };
        if text.is_empty() {
            return;
        }
        // A glyph's space is a thousand units an em (hayro's outlines are in them): its height is the em's, placed.
        let t = props.transform * first.transform();
        let origin = t * Point::ORIGIN;
        let up = t * Point::new(0.0, 1000.0) - origin;
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
        let layer = self.layer();
        self.out.texts.push(Text { text, at: pt(origin), height, angle, color, seq, layer });
    }

    fn draw_image(&mut self, _: interpret::Image<'a, '_>, _: ImageDrawProps<'a>) {}

    fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn pop_transparency_group(&mut self) {}
}

/// The device a page's pictures are painted on: its raster images and stencil masks, each pixel the image's nearest, inside
/// the box round each clipping path; what it fills and strokes, and its text, are left to its lines.
struct Pictures {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
    /// The boxes what's painted is clipped to, px: the page's, then each clipping path's within the one before.
    clips: Vec<Rect>,
    /// The pictures painted.
    drawn: usize,
    /// The pixels painted so far, held to [`MAX_PAINTED`].
    painted: u64,
}

impl Pictures {
    /// Paints an image of `dw` by `dh` pixels placed by `t` (its pixels to the page's): each of the page's pixels it covers
    /// takes `color(x, y)` of its nearest, an RGB and an opacity.
    fn paint(&mut self, t: Affine, dw: u32, dh: u32, color: impl Fn(u32, u32) -> ([u8; 3], u8)) {
        if dw == 0 || dh == 0 || t.determinant().abs() < 1e-12 || !t.determinant().is_finite() {
            return;
        }
        let clip = self.clips.last().copied().unwrap_or(Rect::ZERO);
        let b = t.transform_rect_bbox(Rect::new(0.0, 0.0, f64::from(dw), f64::from(dh)));
        let (x0, y0) = (b.x0.max(clip.x0).max(0.0).floor(), b.y0.max(clip.y0).max(0.0).floor());
        let (x1, y1) = (b.x1.min(clip.x1).min(f64::from(self.w)).ceil(), b.y1.min(clip.y1).min(f64::from(self.h)).ceil());
        if !(x0 < x1 && y0 < y1) {
            return;
        }
        let area = (x1 - x0) * (y1 - y0);
        if self.painted as f64 + area > MAX_PAINTED as f64 {
            return;
        }
        self.painted += area as u64;
        self.drawn += 1;
        let back = t.inverse();
        for y in y0 as u32..y1 as u32 {
            for x in x0 as u32..x1 as u32 {
                let q = back * Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                if !(q.x >= 0.0 && q.y >= 0.0 && q.x < f64::from(dw) && q.y < f64::from(dh)) {
                    continue;
                }
                let (c, a) = color(q.x as u32, q.y as u32);
                let i = (y as usize * self.w as usize + x as usize) * 4;
                if let Some(px) = self.rgba.get_mut(i..i + 3) {
                    for (p, v) in px.iter_mut().zip(c) {
                        *p = ((u32::from(v) * u32::from(a) + u32::from(*p) * (255 - u32::from(a))) / 255) as u8;
                    }
                }
            }
        }
    }
}

/// The opacity of pixel (`x`, `y`) of an image of `w` by `h` pixels, from its mask, which may be of another size.
fn alpha_at(alpha: Option<&LumaData>, w: u32, h: u32, x: u32, y: u32) -> u8 {
    let Some(a) = alpha else { return 255 };
    let (ax, ay) = (u64::from(x) * u64::from(a.width) / u64::from(w.max(1)), u64::from(y) * u64::from(a.height) / u64::from(h.max(1)));
    a.data.get((ay * u64::from(a.width) + ax) as usize).copied().unwrap_or(255)
}

impl<'a> Device<'a> for Pictures {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}

    fn push_clip_path(&mut self, clip: &ClipPath) {
        let b = clip.path.control_box();
        let now = self.clips.last().copied().unwrap_or(Rect::ZERO);
        let inner = Rect::new(b.x0.max(now.x0), b.y0.max(now.y0), b.x1.min(now.x1), b.y1.min(now.y1));
        let shows = inner.x0 <= inner.x1 && inner.y0 <= inner.y1 && inner.x0.is_finite() && inner.y1.is_finite();
        self.clips.push(if shows { inner } else { Rect::ZERO });
    }

    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}

    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}

    fn draw_image(&mut self, image: Image<'a, '_>, props: ImageDrawProps<'a>) {
        // About the size it shows at, so a huge picture is decoded no finer than it is painted.
        let t = props.transform;
        let along = |v: Point| (t * v - t * Point::ORIGIN).hypot();
        let (iw, ih) = (f64::from(image.width()), f64::from(image.height()));
        let target = (
            (along(Point::new(1.0, 0.0)) * iw).ceil().clamp(1.0, 65_535.0) as u32,
            (along(Point::new(0.0, 1.0)) * ih).ceil().clamp(1.0, 65_535.0) as u32,
        );
        match image {
            Image::Raster(r) => r.with_rgba(
                |data, alpha| {
                    let (sx, sy) = data.scale_factors();
                    let placed = t * Affine::scale_non_uniform(f64::from(sx), f64::from(sy));
                    let (w, h) = (data.width(), data.height());
                    match &data {
                        ImageData::Rgb(d) => self.paint(placed, w, h, |x, y| {
                            let i = (y as usize * w as usize + x as usize) * 3;
                            let c = d.data.get(i..i + 3).map_or([255; 3], |p| [p[0], p[1], p[2]]);
                            (c, alpha_at(alpha.as_ref(), w, h, x, y))
                        }),
                        ImageData::Luma(d) => self.paint(placed, w, h, |x, y| {
                            let v = d.data.get(y as usize * w as usize + x as usize).copied().unwrap_or(255);
                            ([v; 3], alpha_at(alpha.as_ref(), w, h, x, y))
                        }),
                    }
                },
                Some(target),
            ),
            Image::Stencil(st) => st.with_stencil(
                |mask, paint| {
                    let color = rgb(paint);
                    let (sx, sy) = mask.scale_factors;
                    let placed = t * Affine::scale_non_uniform(f64::from(sx), f64::from(sy));
                    let w = mask.width;
                    self.paint(placed, w, mask.height, |x, y| {
                        (color, mask.data.get(y as usize * w as usize + x as usize).copied().unwrap_or(0))
                    });
                },
                Some(target),
            ),
        }
    }

    fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    fn pop_transparency_group(&mut self) {}
}

/// The colours a page's lines are named by when it has no layers of its own: the plain names of a few, the nearest taken.
const COLOUR_NAMES: [(&str, [u8; 3]); 18] = [
    ("Black", [0, 0, 0]),
    ("Dark grey", [64, 64, 64]),
    ("Grey", [128, 128, 128]),
    ("Light grey", [192, 192, 192]),
    ("White", [255, 255, 255]),
    ("Red", [230, 0, 0]),
    ("Dark red", [128, 0, 0]),
    ("Orange", [255, 140, 0]),
    ("Brown", [140, 80, 20]),
    ("Yellow", [255, 230, 0]),
    ("Green", [0, 190, 0]),
    ("Dark green", [0, 100, 0]),
    ("Cyan", [0, 220, 220]),
    ("Blue", [0, 70, 255]),
    ("Dark blue", [0, 0, 128]),
    ("Purple", [128, 0, 160]),
    ("Magenta", [230, 0, 230]),
    ("Pink", [255, 150, 190]),
];

/// The plain name of the colour nearest `c`.
pub fn colour_name(c: [u8; 3]) -> &'static str {
    let d = |n: [u8; 3]| (0..3).map(|i| (i32::from(c[i]) - i32::from(n[i])).pow(2)).sum::<i32>();
    COLOUR_NAMES.iter().min_by_key(|(_, n)| d(*n)).map_or("Black", |(name, _)| name)
}

/// A layer name as a drawing holds it: no characters that end a line of DXF, trimmed, not empty, not too long.
fn layer_name(name: &str, i: usize) -> String {
    let n: String = name.chars().filter(|c| !c.is_control()).collect::<String>().trim().chars().take(crate::model::MAX_TEXT).collect();
    if n.is_empty() { format!("Layer {}", i + 1) } else { n }
}

/// Which layer of a drawing an item goes on: from the file's layer it is on, and its plain name of colour and weight.
trait LayerOf: Fn(Option<usize>, &str) -> String {}
impl<F: Fn(Option<usize>, &str) -> String> LayerOf for F {}

/// What a page draws, item by item, in the order it draws it: a fill (its rings), a stroke or a text.
enum Item<'v> {
    Fill(Vec<&'v Run>),
    Stroke(&'v Run),
    Text(&'v Text),
}

impl Vectors {
    /// Whether it draws anything as vectors.
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.fills.is_empty() && self.texts.is_empty()
    }

    /// Whether any of it is on a layer of the file's.
    pub fn layered(&self) -> bool {
        self.strokes.iter().chain(&self.fills).any(|r| r.layer.is_some()) || self.texts.iter().any(|t| t.layer.is_some())
    }

    /// The layer each run and text goes on in its drawing: the file's layer it is on (those it draws on no layer on
    /// [`NO_LAYER`]), or, for a page of a file with no layers, a layer of its own colour and weight ("Black 0.35", "Red
    /// hairline", "Grey fill", "Black text"). With the names, in order, and whether each is off.
    fn layer_names(&self) -> (Vec<(String, bool)>, impl LayerOf + '_) {
        let layered = self.layered();
        let mut names: Vec<String> = Vec::new();
        let mut own: BTreeMap<usize, String> = BTreeMap::new();
        if layered {
            // The file's names, made unique as a drawing's must be.
            let mut taken: BTreeSet<String> = BTreeSet::new();
            for (i, l) in self.layers.iter().enumerate() {
                let base = layer_name(&l.name, i);
                let mut n = base.clone();
                let mut k = 2;
                while taken.contains(&n) || n == NO_LAYER {
                    n = format!("{base} ({k})");
                    k += 1;
                }
                taken.insert(n.clone());
                own.insert(i, n);
            }
        }
        let name = move |layer: Option<usize>, plain: &str| -> String {
            if layered { layer.and_then(|i| own.get(&i).cloned()).unwrap_or_else(|| NO_LAYER.to_string()) } else { plain.to_string() }
        };
        let items = self.items();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut layers: BTreeMap<String, Option<usize>> = BTreeMap::new();
        for it in &items {
            let (layer, plain) = item_layer(it);
            let n = name(layer, &plain);
            if seen.insert(n.clone()) {
                names.push(n.clone());
                layers.insert(n, layer);
            }
        }
        if layered {
            // In the file's order, those on no layer last.
            let order = |n: &String| layers.get(n).copied().flatten().unwrap_or(usize::MAX);
            names.sort_by_key(order);
        } else {
            names.sort();
        }
        let off: Vec<(String, bool)> = names
            .into_iter()
            .map(|n| {
                let off = layered && layers.get(&n).copied().flatten().and_then(|i| self.layers.get(i)).is_some_and(|l| l.off);
                (n, off)
            })
            .collect();
        (off, name)
    }

    /// What it draws, in the order the page draws it: each fill once with all its rings, before the stroke of the same path.
    fn items(&self) -> Vec<Item<'_>> {
        let mut fills: BTreeMap<usize, Vec<&Run>> = BTreeMap::new();
        for f in &self.fills {
            fills.entry(f.seq).or_default().push(f);
        }
        let mut items: Vec<(usize, u8, Item<'_>)> = fills.into_iter().map(|(seq, rings)| (seq, 0, Item::Fill(rings))).collect();
        items.extend(self.strokes.iter().map(|r| (r.seq, 1, Item::Stroke(r))));
        items.extend(self.texts.iter().map(|t| (t.seq, 2, Item::Text(t))));
        items.sort_by_key(|(seq, kind, _)| (*seq, *kind));
        items.into_iter().map(|(_, _, it)| it).collect()
    }

    /// The layers its drawing has ([`Vectors::dxf`]), in order, and whether the file has each off.
    pub fn drawing_layers(&self) -> Vec<(String, bool)> {
        self.layer_names().0
    }

    /// Itself as an ASCII DXF in mm, which [`crate::cad::Cad`] reads, in the order the page draws: strokes as polylines at
    /// their width and dashes, fills as hatches (solid in their colour; one painted with a pattern as a pattern's pale fill),
    /// texts as texts, each in its colour on its layer ([`Vectors::drawing_layers`]); a layer the file has off is off.
    pub fn dxf(&self) -> Vec<u8> {
        let (layers, name) = self.layer_names();
        let mut s = String::with_capacity(64 * (self.strokes.len() + self.fills.len() + self.texts.len()) + 256);
        // Each dash pattern a linetype, named by its dash and gap in hundredths of a mm.
        let dashes: BTreeSet<(i64, i64)> = self.strokes.iter().filter_map(|r| r.dash).map(|[a, b]| hundredths(a, b)).collect();
        s.push_str("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n9\n$LTSCALE\n40\n1.0\n0\nENDSEC\n0\nSECTION\n2\nTABLES\n");
        s.push_str("0\nTABLE\n2\nLTYPE\n");
        for (a, b) in &dashes {
            let (da, db) = (*a as f64 / 100.0, *b as f64 / 100.0);
            let _ =
                write!(s, "0\nLTYPE\n2\nDASH_{a}_{b}\n70\n0\n3\nDashed\n72\n65\n73\n2\n40\n{:.2}\n49\n{da:.2}\n49\n{:.2}\n", da + db, -db);
        }
        s.push_str("0\nENDTAB\n0\nTABLE\n2\nLAYER\n");
        for (n, off) in &layers {
            let _ = write!(s, "0\nLAYER\n2\n{n}\n70\n0\n62\n{}\n6\nCONTINUOUS\n", if *off { -7 } else { 7 });
        }
        s.push_str("0\nENDTAB\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        let color = |c: [u8; 3]| (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]);
        for it in self.items() {
            let (layer, plain) = item_layer(&it);
            let layer = name(layer, &plain);
            match it {
                Item::Stroke(r) => {
                    let _ =
                        write!(s, "0\nLWPOLYLINE\n8\n{layer}\n420\n{}\n90\n{}\n70\n{}\n", color(r.color), r.pts.len(), u8::from(r.closed));
                    if let Some([a, b]) = r.dash {
                        let (a, b) = hundredths(a, b);
                        let _ = write!(s, "6\nDASH_{a}_{b}\n");
                    }
                    if r.width > 0.0 {
                        // DXF's line weights are hundredths of a mm, the thinnest 0.05 mm.
                        let _ = write!(s, "370\n{}\n", ((r.width * 100.0).round() as i64).clamp(5, 211));
                    }
                    for [x, y] in &r.pts {
                        let _ = write!(s, "10\n{x:.4}\n20\n{y:.4}\n");
                    }
                }
                Item::Fill(rings) => {
                    let Some(first) = rings.first() else { continue };
                    let solid = !first.pattern;
                    let c = if solid { first.color } else { [128, 128, 128] };
                    let _ = write!(
                        s,
                        "0\nHATCH\n8\n{layer}\n420\n{}\n2\n{}\n70\n{}\n91\n{}\n",
                        color(c),
                        if solid { "SOLID" } else { "ANSI31" },
                        u8::from(solid),
                        rings.len()
                    );
                    for r in &rings {
                        let _ = write!(s, "92\n2\n72\n0\n73\n1\n93\n{}\n", r.pts.len());
                        for [x, y] in &r.pts {
                            let _ = write!(s, "10\n{x:.4}\n20\n{y:.4}\n");
                        }
                    }
                }
                Item::Text(t) => {
                    let text: String = t.text.chars().filter(|c| !c.is_control()).collect();
                    let _ = write!(
                        s,
                        "0\nTEXT\n8\n{layer}\n420\n{}\n10\n{:.4}\n20\n{:.4}\n40\n{:.4}\n50\n{:.4}\n1\n{text}\n",
                        color(t.color),
                        t.at[0],
                        t.at[1],
                        t.height,
                        t.angle
                    );
                }
            }
        }
        s.push_str("0\nENDSEC\n0\nEOF\n");
        s.into_bytes()
    }
}

/// A dash and a gap, mm, in hundredths of a mm, the gap at least one.
fn hundredths(a: f64, b: f64) -> (i64, i64) {
    ((a * 100.0).round().clamp(0.0, 1e6) as i64, (b * 100.0).round().clamp(1.0, 1e6) as i64)
}

/// An item's layer of the file's, and the layer of its colour and weight it goes on when the file has none.
fn item_layer(it: &Item<'_>) -> (Option<usize>, String) {
    match it {
        Item::Stroke(r) => {
            let w = if r.width < 0.005 { "hairline".to_string() } else { format!("{:.2}", r.width) };
            (r.layer, format!("{} {w}", colour_name(r.color)))
        }
        Item::Fill(rings) => {
            let first = rings.first();
            let plain = match first {
                Some(r) if r.pattern => "Pattern fill".to_string(),
                Some(r) => format!("{} fill", colour_name(r.color)),
                None => "Fill".to_string(),
            };
            (first.and_then(|r| r.layer), plain)
        }
        Item::Text(t) => (t.layer, format!("{} text", colour_name(t.color))),
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

/// A PDF of one page `size` points wide and high whose content stream is `content`, with layers: each of `layers` an optional
/// content group (its name, and whether the file has it off), named `/L0`, `/L1`… in the page's properties for `BDC`, and
/// `/M0` an OCMD of the first two (any on), what tests import. The page has the font `/F1` (Helvetica).
#[doc(hidden)]
pub fn sample_layered(content: &str, size: [f64; 2], layers: &[(&str, bool)]) -> Vec<u8> {
    // 1 the catalog, 2 the pages, 3 the page, 4 its content, 5 the font, 6 the OCMD, then each layer.
    let first = 7;
    let refs: Vec<String> = (0..layers.len()).map(|i| format!("{} 0 R", first + i)).collect();
    let off: Vec<String> = layers.iter().zip(&refs).filter(|((_, o), _)| *o).map(|(_, r)| r.clone()).collect();
    let props: Vec<String> = refs.iter().enumerate().map(|(i, r)| format!("/L{i} {r}")).collect();
    let pair: Vec<String> = refs.iter().take(2).cloned().collect();
    let [w, h] = size;
    let mut objects = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [{}] /D << /Order [{}] /OFF [{}] >> >> >>",
            refs.join(" "),
            refs.join(" "),
            off.join(" ")
        ),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> /Properties << {} /M0 6 0 R >> >> >>",
            props.join(" ")
        ),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        format!("<< /Type /OCMD /OCGs [{}] /P /AnyOn >>", pair.join(" ")),
    ];
    for (name, _) in layers {
        objects.push(format!("<< /Type /OCG /Name ({name}) >>"));
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
        // 12 points of letter: an em of 4.23 mm, its capitals seven tenths of it.
        assert!((v.texts[0].height - 12.0 * MM_PER_PT * 0.7).abs() < 1e-6, "{:?}", v.texts[0]);
        assert!(near(v.texts[0].at, [80.0 * MM_PER_PT, 180.0 * MM_PER_PT]), "{:?}", v.texts[0]);
    }

    #[test]
    fn its_lines_read_as_a_drawing_in_mm() {
        let v = Document::open(square()).unwrap().vectors(1).unwrap();
        let cad = crate::cad::Cad::new("square.pdf", None, v.dxf()).unwrap();
        let dxf = cad.dxf();
        assert_eq!(dxf.unit, "mm");
        assert_eq!(dxf.entities.len(), 2);
        assert_eq!(dxf.entities[0].layer, "Blue 0.71", "a page with no layers has its lines by colour and weight");
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

    /// A page on three layers, Walls, Furniture (off in the file) and Notes: a wall on Walls, a chair on Furniture inside a
    /// group on Walls (the inner one counts), a line on the OCMD (its first group, Walls), a note on Notes, and a border on no
    /// layer.
    fn layered() -> Vec<u8> {
        let content = "/OC /L0 BDC 0 0 0 RG 2 w 10 10 m 190 10 l S /OC /L1 BDC 1 0 0 RG 1 w 50 50 m 60 60 l S EMC EMC \
                       /OC /M0 BDC 0 0 1 RG 0.5 w 10 20 m 100 20 l S EMC \
                       /OC /L2 BDC BT /F1 12 Tf 20 100 Td (Kitchen) Tj ET EMC \
                       0 0 0 RG 0 w 0 0 200 200 re S";
        sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Furniture", true), ("Notes", false)])
    }

    #[test]
    fn a_pages_lines_and_words_keep_the_layer_they_are_drawn_on() {
        let d = Document::open(layered()).unwrap();
        let names: Vec<(String, bool)> = d.layers().into_iter().map(|l| (l.name, l.off)).collect();
        let want = |n: &str, off: bool| (n.to_string(), off);
        assert_eq!(names, vec![want("Walls", false), want("Furniture", true), want("Notes", false)]);
        let v = d.vectors(1).unwrap();
        // Furniture is off in the file, and still read: its chair is in the lines, on its layer.
        let on = |c: [u8; 3]| v.strokes.iter().find(|r| r.color == c).map(|r| r.layer);
        assert_eq!(on([0, 0, 0]), Some(Some(0)), "the wall: {v:?}");
        assert_eq!(on([255, 0, 0]), Some(Some(1)), "the chair, in a group in Walls, is on Furniture");
        assert_eq!(on([0, 0, 255]), Some(Some(0)), "the OCMD's line is on its first group");
        assert_eq!(v.texts.len(), 1);
        assert_eq!((v.texts[0].text.as_str(), v.texts[0].layer), ("Kitchen", Some(2)));
        assert!(v.strokes.iter().any(|r| r.layer.is_none() && r.pts.len() == 4), "the border, on no layer");
        assert!(!v.off_unread);
        // Its drawing has the file's layers, in its order, those on none last; Furniture is off.
        let layers = v.drawing_layers();
        assert_eq!(layers, vec![want("Walls", false), want("Furniture", true), want("Notes", false), want(NO_LAYER, false)]);
        let cad = crate::cad::Cad::new("plan.pdf", None, v.dxf()).unwrap();
        let dxf = cad.dxf();
        let furniture = dxf.layer("Furniture").unwrap();
        assert!(furniture.off, "{furniture:?}");
        let kitchen = dxf.entities.iter().find(|e| matches!(&e.shape, crate::dxf::Shape::Text { text, .. } if text == "Kitchen")).unwrap();
        assert_eq!(kitchen.layer, "Notes");
        // The rendered page shows it as the file opens: the chair, on a layer the file has off, isn't drawn.
        assert!(!dark(&d, 55, 200 - 55), "the chair is hidden in the picture");
        assert!(dark(&d, 100, 200 - 10), "the wall shows");
    }

    #[test]
    fn a_page_with_no_layers_has_its_lines_grouped_by_colour_and_weight() {
        let content = "0 0 0 RG 1 w 10 10 m 100 10 l S 1 0 0 RG 0.5 w 10 20 m 100 20 l S 0 0 0 RG 1 w 10 30 m 100 30 l S \
                       0 0 1 rg 20 40 30 30 re f 0 0 0 rg BT /F1 12 Tf 20 100 Td (WC) Tj ET 0 w 10 150 m 50 150 l S";
        let v = Document::open(sample(content, [200.0, 200.0], None)).unwrap().vectors(1).unwrap();
        assert!(!v.layered());
        let names: Vec<String> = v.drawing_layers().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Black 0.35", "Black hairline", "Black text", "Blue fill", "Red 0.18"], "{names:?}");
        let cad = crate::cad::Cad::new("p.pdf", None, v.dxf()).unwrap();
        let on = |l: &str| cad.dxf().entities.iter().filter(|e| e.layer == l).count();
        assert_eq!((on("Black 0.35"), on("Red 0.18"), on("Blue fill"), on("Black text")), (2, 1, 1, 1));
        // The fill is a solid hatch, in its colour.
        let fill = cad.dxf().entities.iter().find(|e| e.layer == "Blue fill").unwrap();
        assert!(matches!(&fill.shape, crate::dxf::Shape::Fill { solid: true, rings } if rings.len() == 1), "{fill:?}");
        assert_eq!(cad.color(fill, crate::cad::Colors::Preserve), Some(crate::prim::Rgb(0, 0, 255)));
    }

    #[test]
    fn a_dashed_line_keeps_its_dashes_and_a_fill_its_holes_and_its_place_in_the_order() {
        // A white mask over a line, then a dashed line over it, then a square with a hole (even-odd).
        let content = "0 0 0 RG 1 w 10 50 m 190 50 l S 1 1 1 rg 40 40 20 20 re f [6 3] 0 d 10 60 m 190 60 l S [] 0 d \
                       0 0 0 rg 100 100 m 150 100 l 150 150 l 100 150 l h 110 110 m 140 110 l 140 140 l 110 140 l h f*";
        let v = Document::open(sample(content, [200.0, 200.0], None)).unwrap().vectors(1).unwrap();
        let k = MM_PER_PT;
        let dashed = v.strokes.iter().find(|r| r.dash.is_some()).unwrap();
        let [a, b] = dashed.dash.unwrap();
        assert!((a - 6.0 * k).abs() < 1e-6 && (b - 3.0 * k).abs() < 1e-6, "{dashed:?}");
        let cad = crate::cad::Cad::new("p.pdf", None, v.dxf()).unwrap();
        let ents = &cad.dxf().entities;
        // In the order the page draws: the line, the mask over it, the dashed line, the square.
        let kinds: Vec<&str> = ents.iter().map(|e| crate::cad::kind(&e.shape)).collect();
        assert_eq!(kinds, ["polyline", "hatch", "polyline", "hatch"], "{ents:?}");
        let [dash, gap] = cad.dash(&ents[2]).unwrap();
        assert!((dash - 6.0 * k).abs() < 0.01 && (gap - 3.0 * k).abs() < 0.01, "{dash} {gap}");
        assert!(matches!(&ents[3].shape, crate::dxf::Shape::Fill { rings, .. } if rings.len() == 2), "one fill, its hole a ring");
    }

    #[test]
    fn a_pages_pictures_are_painted_without_its_lines() {
        // A grey picture 50 points a side at (100, 100), and a black line across the page.
        let content = "0 0 0 RG 4 w 0 20 m 200 20 l S q 50 0 0 50 100 100 cm BI /W 2 /H 2 /CS /G /BPC 8 /F /AHx ID 80808080> EI Q";
        let d = Document::open(sample(content, [200.0, 200.0], None)).unwrap();
        let ((w, h, rgba), n) = d.pictures_into(1, 72.0, |w, h, rgba| (w, h, rgba.to_vec())).unwrap();
        assert_eq!((w, h, n), (200, 200, 1));
        let px = |x: u32, y: u32| rgba[((y * w + x) * 4) as usize];
        assert_eq!(px(125, 200 - 125), 128, "the picture");
        assert_eq!(px(50, 200 - 20), 255, "no line: the lines draw it");
        assert_eq!(px(50, 50), 255);
        assert!(dark(&d, 50, 200 - 20), "the whole page has the line");
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

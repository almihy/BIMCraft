//! A PDF's pages read for Import PDF, through hayro (pure Rust, Apache-2.0 OR MIT, so it builds for the browser too): each
//! page's size, its pixels at a resolution, and what it draws as vectors (its stroked and filled paths, and the text it
//! shows) in millimetres on the paper, from its bottom left corner, y up, as the pixels have it. The file itself is not kept:
//! a project holds the pixels and the lines.

use std::fmt::Write as _;

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    self as interpret, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, ImageDrawProps, InterpreterCache, InterpreterSettings,
    Paint, SoftMask, TransformExt,
};
use hayro::hayro_syntax::object::{Array, Dict, Name, Number, String as PdfString};
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::{LoadPdfError, Pdf};
use hayro::kurbo::{Affine, BezPath, PathEl, Point, Rect};

/// Millimetres in a point, PDF's unit.
pub const MM_PER_PT: f64 = 25.4 / 72.0;
/// The most pages a file may have to be read.
pub const MAX_PAGES: usize = 10_000;
/// The most path segments and texts kept of one page: past them the rest is left out, with a note.
pub const MAX_SEGMENTS: usize = 400_000;
/// How far a curve's chords may stray from it, mm on the paper.
const FLATNESS: f64 = 0.05;

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
    /// What was left out past [`MAX_SEGMENTS`].
    pub left_out: usize,
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
        page.checked_sub(1)
            .and_then(|i| self.pdf.pages().get(i))
            .ok_or_else(|| format!("it has no page {page}: its pages are 1 to {n}"))
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
        Ok((w, h, pixmap.data_as_u8_slice().to_vec()))
    }

    /// What page `page` draws as vectors, mm on the paper from its bottom left corner, y up.
    pub fn vectors(&self, page: usize) -> Result<Vectors, String> {
        let p = self.page(page)?;
        let [_, h] = self.size(page)?;
        // To mm, y down from the top left as the pixels are; then flipped, so y runs up from the bottom.
        let to_mm = Affine::new([1.0, 0.0, 0.0, -1.0, 0.0, h]) * Affine::scale(user_unit(p) * MM_PER_PT) * p.initial_transform(true).to_kurbo();
        let cache = InterpreterCache::new();
        let mut ctx = Context::new(to_mm, Rect::new(-1e9, -1e9, 1e9, 1e9), &cache, p.xref(), InterpreterSettings::default());
        let mut device = Collect { out: Vectors::default(), held: 0 };
        interpret::interpret_page(p, &mut ctx, &mut device);
        Ok(device.out)
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
}

impl Collect {
    /// Runs of points a path draws, through `t`: its curves in chords.
    fn runs(&mut self, path: &BezPath, t: Affine) -> Vec<(Vec<[f64; 2]>, bool)> {
        let placed = t * path.clone();
        let mut runs: Vec<(Vec<[f64; 2]>, bool)> = Vec::new();
        let mut run: Vec<[f64; 2]> = Vec::new();
        let mut closed = false;
        let end = |run: &mut Vec<[f64; 2]>, closed: &mut bool, runs: &mut Vec<(Vec<[f64; 2]>, bool)>| {
            run.dedup();
            if *closed && run.len() > 2 && run.first() == run.last() {
                run.pop();
            }
            if run.len() > 1 && run.iter().flatten().all(|v| v.is_finite()) {
                runs.push((std::mem::take(run), *closed));
            }
            run.clear();
            *closed = false;
        };
        let mut flattened: Vec<PathEl> = Vec::new();
        hayro::kurbo::flatten(placed.elements().iter().copied(), FLATNESS, |el| flattened.push(el));
        for el in flattened {
            match el {
                PathEl::MoveTo(q) => {
                    end(&mut run, &mut closed, &mut runs);
                    run.push(pt(q));
                }
                PathEl::LineTo(q) | PathEl::QuadTo(_, q) | PathEl::CurveTo(_, _, q) => run.push(pt(q)),
                PathEl::ClosePath => {
                    closed = true;
                    let start = run.first().copied();
                    end(&mut run, &mut closed, &mut runs);
                    // A path goes on from where it closed.
                    run.extend(start);
                }
            }
        }
        end(&mut run, &mut closed, &mut runs);
        runs
    }

    fn keep(&mut self, runs: Vec<(Vec<[f64; 2]>, bool)>, color: [u8; 3], width: Option<f64>) {
        for (pts, closed) in runs {
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

    fn push_clip_path(&mut self, _: &ClipPath) {}

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
        if !(height.is_finite() && height > 0.0 && origin.x.is_finite() && origin.y.is_finite()) {
            return;
        }
        self.held += 1;
        self.out.texts.push(Text { text, at: pt(origin), height, angle, color: rgb(&props.paint) });
    }

    fn draw_image(&mut self, _: interpret::Image<'a, '_>, _: ImageDrawProps<'a>) {}

    fn pop_clip(&mut self) {}

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

/// A PDF of one page `size` points wide and high whose content stream is `content`, with a viewport's measure saying a point
/// is `measure` (a length and its unit, `(0.0352778, "m")` at 1 : 100) when it's given: what tests import.
#[doc(hidden)]
pub fn sample(content: &str, size: [f64; 2], measure: Option<(f64, &str)>) -> Vec<u8> {
    let vp = measure.map_or_else(String::new, |(c, u)| {
        format!(
            " /VP [<< /Type /Viewport /BBox [0 0 {} {}] /Measure << /Type /Measure /Subtype /RL /R (scale) /X [<< /Type /NumberFormat /U ({u}) /C {c} >>] /D [<< /Type /NumberFormat /U ({u}) /C 1 >>] /A [<< /Type /NumberFormat /U ({u}2) /C 1 >>] >> >>]",
            size[0], size[1]
        )
    });
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >>{vp} >>",
            size[0], size[1]
        ),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
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

    #[test]
    fn what_isnt_a_pdf_or_has_no_such_page_says_so() {
        assert!(Document::open(b"hello".to_vec()).unwrap_err().contains("isn't a PDF"));
        let d = Document::open(square()).unwrap();
        assert!(d.size(2).unwrap_err().contains("no page 2"));
        assert!(d.size(0).is_err());
        assert!(Document::open(b"%PDF-1.4 garbage".to_vec()).is_err());
    }
}

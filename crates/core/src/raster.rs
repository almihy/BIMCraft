//! Raster images: Revit's Insert > Image and Import PDF. A project holds each picture once, as an [`ImageType`] (Revit's image
//! type, listed by Manage Images): its pixels, kept as a PNG, and, for a page of a PDF, the lines and texts it draws as
//! vectors, held as a drawing ([`crate::cad::Cad`]) so they snap and an agent reads them as an imported DXF's. The PDF itself
//! is not kept. An [`ImageInstance`] element places one in one view (a plan, a ceiling plan, a section, an elevation or a
//! sheet, in the view's own terms), its middle on `at`, `width` by `height`, turned, behind the model or in front of it.

pub mod pdf;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::cad::Cad;
use crate::geom::P2;
use crate::model::{Guid, MAX_TEXT, is_zero};
use crate::render::{Drawing, Prim};

/// Images one project may hold.
pub const MAX_TYPES: usize = 256;
/// The most bytes of PNG a project's images may hold among them: in base64, a third more, they and the rest of the project
/// stay within the longest text a browser holds (Chrome's is 2^29 - 24 characters), which a project saved there is.
pub const MAX_HELD: usize = 256 << 20;
/// The largest file Import PDF or Insert > Image reads, bytes.
pub const MAX_FILE: usize = 256 << 20;
/// The most pixels one image may have: a page at a resolution that would have more is read at the finest that fits.
pub const MAX_PIXELS: u64 = 36_000_000;
/// Import PDF's resolutions, dots an inch, as Revit offers them, and the one it starts on.
pub const RESOLUTIONS: [u32; 4] = [72, 150, 300, 600];
pub const DEFAULT_DPI: u32 = 150;
/// The resolution a picture (not a PDF) is taken at, as Revit takes every raster image: 72 dpi.
pub const PICTURE_DPI: u32 = 72;
/// The most a stated scale or a placed image's scale may be, 1 : this.
pub const MAX_SCALE: f64 = 1e6;
/// The smallest and largest an image may be placed, mm in its view's terms.
pub const MIN_SIDE: f64 = 0.01;
/// The most pixels a view draws of one image.
const MAX_DRAWN: u64 = 64_000_000;

/// Where an image draws in its view, Revit's Draw Layer: behind the model (Background, as it comes in), or in front of it
/// (Foreground). Annotation draws over both.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DrawLayer {
    #[default]
    Background,
    Foreground,
}

impl DrawLayer {
    pub fn from_name(name: &str) -> Option<DrawLayer> {
        match name.trim().to_ascii_lowercase().as_str() {
            "background" | "back" | "behind" => Some(DrawLayer::Background),
            "foreground" | "front" => Some(DrawLayer::Foreground),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DrawLayer::Background => "background",
            DrawLayer::Foreground => "foreground",
        }
    }
}

fn is_background(l: &DrawLayer) -> bool {
    *l == DrawLayer::Background
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

/// An image placed, Revit's image instance: the project's image `type_name`, in view `view` (in its own terms: plan mm, a
/// section's along and up, a sheet's paper mm), its middle on `at`, `width` by `height`, turned `angle` degrees
/// counter-clockwise about its middle and, `mirrored`, flipped top for bottom in its own frame (so turned and flipped, the mirror
/// image of it across any line). `lock` keeps its proportions as it's sized
/// (Revit's Lock Proportions); `snaps` lets what's drawn over it snap to a PDF's lines (Enable Snaps); `layer` is its Draw Layer,
/// and `order` its place in the draw order of its view's images, back to front (Revit's Arrange), within its layer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageInstance {
    #[serde(rename = "type")]
    pub type_name: String,
    pub view: Guid,
    pub at: P2,
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub angle: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mirrored: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub lock: bool,
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub snaps: bool,
    #[serde(default, skip_serializing_if = "is_background")]
    pub layer: DrawLayer,
    #[serde(default, skip_serializing_if = "is_first")]
    pub order: i32,
}

fn is_first(o: &i32) -> bool {
    *o == 0
}

impl ImageInstance {
    /// Its corners in its view's terms: the picture's top left, top right, bottom right and bottom left, as it shows.
    pub fn corners(&self) -> [P2; 4] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (x, y) = (P2(cos, sin), P2(-sin, cos));
        let (hw, hh) = (self.width / 2.0, self.height / 2.0);
        // A mirror image is flipped top for bottom in its own frame, as a detail group's is: across any line, its turn
        // (`commands::turned`) and the flip make the picture's mirror image.
        let side = if self.mirrored { -1.0 } else { 1.0 };
        let at = |u: f64, v: f64| self.at + x * (u * hw) + y * (v * hh * side);
        [at(-1.0, 1.0), at(1.0, 1.0), at(1.0, -1.0), at(-1.0, -1.0)]
    }

    /// The box round it, in its view's terms.
    pub fn bounds(&self) -> [P2; 2] {
        let c = self.corners();
        let (lo, hi) = c.iter().fold((P2(f64::INFINITY, f64::INFINITY), P2(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        });
        [lo, hi]
    }

    /// Its Horizontal and Vertical Scale: its size over its image's own.
    pub fn scales(&self, t: &ImageType) -> [f64; 2] {
        let [w, h] = t.size();
        [self.width / w, self.height / h]
    }

    /// Its image's lines as a drawing placed where it shows them: a CAD import of the image's drawing (its own mm, from its
    /// paper's bottom left), on `level`, so they snap, read and pick as an import's do. None for an image of no lines.
    pub fn lines_placed(&self, t: &ImageType, level: Guid) -> Option<(Cad, crate::model::CadImport)> {
        let cad = t.lines()?.clone();
        let [w, h] = t.size();
        let k = self.width / w;
        let stretch = (self.height / h) / k;
        // A mirror image is the page flipped top for bottom.
        let (angle, stretch) = (self.angle, if self.mirrored { -stretch } else { stretch });
        let ci = crate::model::CadImport {
            cad: String::new(),
            level,
            at: self.at,
            origin: P2(w / 2.0, h / 2.0),
            scale: k,
            stretch,
            angle,
            view: Some(self.view),
            hidden: Default::default(),
            colors: Default::default(),
        };
        Some((cad, ci))
    }
}

/// An image a project holds, Revit's image type: the file it came from (`source`, and the page of a PDF), the resolution its
/// pixels are at, its size at that resolution in mm (a PDF's page size, a picture's at 72 dpi), its pixels as a PNG, and a
/// PDF's vectors as a drawing in mm of the page, from its bottom left. Cheap to clone: what it holds is shared.
#[derive(Clone)]
pub struct ImageType {
    source: String,
    page: Option<u32>,
    dpi: u32,
    size: [f64; 2],
    /// What the page says a mm of it stands for (its viewport's measure), when it says.
    scale: Option<f64>,
    png: Arc<Vec<u8>>,
    px: (u32, u32),
    pixels: Arc<OnceLock<Option<Arc<Pixels>>>>,
    lines: Option<Cad>,
}

impl std::fmt::Debug for ImageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageType({:?}, page {:?}, {} x {} px)", self.source, self.page, self.px.0, self.px.1)
    }
}

impl PartialEq for ImageType {
    fn eq(&self, o: &ImageType) -> bool {
        self.source == o.source
            && self.page == o.page
            && self.dpi == o.dpi
            && self.size == o.size
            && self.scale == o.scale
            && (Arc::ptr_eq(&self.png, &o.png) || self.png == o.png)
            && self.lines == o.lines
    }
}

/// The decoded pixels and their levels, each half the one before, made when first drawn (or when the image comes in).
struct Pixels {
    levels: Vec<Level>,
}

impl Pixels {
    /// `base` and each level under it, down to a pixel.
    fn new(base: Level, keep_lines: bool) -> Pixels {
        let mut levels = vec![base];
        while let Some(last) = levels.last()
            && (last.w > 1 || last.h > 1)
        {
            let next = last.halved(keep_lines);
            levels.push(next);
        }
        Pixels { levels }
    }
}

/// Pixels `w` by `h`, row by row, each `ch` bytes: grey (1), RGB (3) or RGBA (4), as little as they need: a drawing's
/// page, grey, holds a quarter of what RGBA would.
struct Level {
    w: u32,
    h: u32,
    ch: usize,
    px: Vec<u8>,
}

impl Level {
    /// RGBA pixels as the fewest channels that hold them: grey when every pixel is opaque and grey, RGB when opaque.
    fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Level {
        let px = rgba.as_chunks::<4>().0;
        let opaque = px.iter().all(|p| p[3] == 255);
        let grey = opaque && px.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
        let (ch, px) = if grey {
            (1, px.iter().map(|p| p[0]).collect())
        } else if opaque {
            (3, px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect())
        } else {
            (4, rgba.to_vec())
        };
        Level { w, h, ch, px }
    }

    /// Its pixel at (`x`, `y`), RGBA; white past its sides.
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y.min(self.h.saturating_sub(1)) as usize * self.w as usize + x.min(self.w.saturating_sub(1)) as usize) * self.ch;
        match (self.ch, self.px.get(i..i + self.ch)) {
            (1, Some(&[g])) => [g, g, g, 255],
            (3, Some(&[r, g, b])) => [r, g, b, 255],
            (4, Some(&[r, g, b, a])) => [r, g, b, a],
            _ => [255; 4],
        }
    }

    /// All of it as RGBA.
    fn rgba(&self) -> Vec<u8> {
        (0..self.h).flat_map(|y| (0..self.w).map(move |x| (x, y))).flat_map(|(x, y)| self.at(x, y)).collect()
    }

    /// Half its size: each pixel the darkest of the four it covers when `keep_lines` (a drawing's lines stay as it zooms out),
    /// else their mean.
    fn halved(&self, keep_lines: bool) -> Level {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let ch = self.ch;
        let mut px = Vec::with_capacity(w as usize * h as usize * ch);
        let at = |x: u32, y: u32| {
            let i = (y.min(self.h - 1) as usize * self.w as usize + x.min(self.w - 1) as usize) * ch;
            self.px.get(i..i + ch).unwrap_or(&[])
        };
        for y in 0..h {
            for x in 0..w {
                let four = [at(2 * x, 2 * y), at(2 * x + 1, 2 * y), at(2 * x, 2 * y + 1), at(2 * x + 1, 2 * y + 1)];
                if keep_lines {
                    let dark = four.iter().min_by_key(|p| p.iter().take(3).map(|c| u32::from(*c)).sum::<u32>()).copied().unwrap_or(&[]);
                    px.extend((0..ch).map(|c| dark.get(c).copied().unwrap_or(255)));
                } else {
                    for c in 0..ch {
                        px.push((four.iter().map(|p| u32::from(p.get(c).copied().unwrap_or(255))).sum::<u32>() / 4) as u8);
                    }
                }
            }
        }
        Level { w, h, ch, px }
    }
}

/// How an image goes in a project file.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageFile {
    source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    page: Option<u32>,
    dpi: u32,
    size: [f64; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scale: Option<f64>,
    png: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lines: Option<Cad>,
}

impl Serialize for ImageType {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ImageFile {
            source: self.source.clone(),
            page: self.page,
            dpi: self.dpi,
            size: self.size,
            scale: self.scale,
            png: crate::commands::base64(&self.png),
            lines: self.lines.clone(),
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for ImageType {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<ImageType, D::Error> {
        let f = ImageFile::deserialize(d)?;
        let bad = |what: String| -> D::Error { <D::Error as serde::de::Error>::custom(format!("image `{}`: {what}", f.source)) };
        if !f.size.iter().all(|s| s.is_finite() && *s >= MIN_SIDE && *s <= crate::model::MAX_COORD) {
            return Err(bad(format!(
                "its size, {:?} mm, isn't one an image can have: each side from {MIN_SIDE} to {} mm",
                f.size,
                crate::model::MAX_COORD
            )));
        }
        let finest = RESOLUTIONS.iter().copied().max().unwrap_or(600);
        if !(1..=finest).contains(&f.dpi) {
            return Err(bad(format!("its resolution, {} dpi, isn't one an image can have: from 1 to {finest} dpi", f.dpi)));
        }
        if let Some(k) = f.scale
            && !(k.is_finite() && k > 0.0 && k <= MAX_SCALE)
        {
            return Err(bad(format!("the scale its page states, 1 : {k}, isn't one: from above 0 to {MAX_SCALE}")));
        }
        let png = crate::texture::unbase64(&f.png).ok_or_else(|| serde::de::Error::custom("an image's pixels aren't base64"))?;
        let px = png_size(&png).map_err(|e| serde::de::Error::custom(format!("image `{}`: {e}", f.source)))?;
        Ok(ImageType {
            source: f.source,
            page: f.page,
            dpi: f.dpi,
            size: f.size,
            scale: f.scale,
            png: Arc::new(png),
            px,
            pixels: Arc::default(),
            lines: f.lines,
        })
    }
}

/// A PNG's width and height, from its header, held to [`MAX_PIXELS`].
fn png_size(png: &[u8]) -> Result<(u32, u32), String> {
    if !png.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Err("its pixels aren't a PNG".into());
    }
    let word = |i: usize| png.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
    let (w, h) = (word(16).ok_or("its PNG is cut short")?, word(20).ok_or("its PNG is cut short")?);
    too_big(w, h)?;
    Ok((w, h))
}

fn too_big(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(format!("it's {w} by {h} px; an image has up to {MAX_PIXELS} pixels"));
    }
    Ok(())
}

/// Pixels as a PNG, in their own channels, row by row (no copy of them is made).
fn encode(l: &Level) -> Result<Vec<u8>, String> {
    use std::io::Write as _;
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, l.w, l.h);
    e.set_color(match l.ch {
        1 => png::ColorType::Grayscale,
        3 => png::ColorType::Rgb,
        _ => png::ColorType::Rgba,
    });
    e.set_depth(png::BitDepth::Eight);
    e.set_compression(png::Compression::Fast);
    e.set_filter(png::Filter::Adaptive);
    let mut w = e.write_header().map_err(|e| e.to_string())?;
    {
        let mut rows = w.stream_writer().map_err(|e| e.to_string())?;
        let row = l.w as usize * l.ch;
        for r in l.px.chunks(row.max(1)) {
            rows.write_all(r).map_err(|e| e.to_string())?;
        }
        rows.finish().map_err(|e| e.to_string())?;
    }
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// A PNG's pixels, in its channels (grey, RGB or RGBA: other kinds as RGBA).
fn unpng(png: &[u8]) -> Option<Level> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?;
    let (w, h) = (img.width(), img.height());
    Some(match img {
        image::DynamicImage::ImageLuma8(g) => Level { w, h, ch: 1, px: g.into_raw() },
        image::DynamicImage::ImageRgb8(c) => Level { w, h, ch: 3, px: c.into_raw() },
        other => Level { w, h, ch: 4, px: other.into_rgba8().into_raw() },
    })
}

/// The pixels of a PNG, JPEG or BMP file, held to [`MAX_PIXELS`] before they're decoded.
fn decode(bytes: &[u8]) -> Result<Level, String> {
    if bytes.len() > MAX_FILE {
        return Err(format!("it's {} bytes; up to {MAX_FILE}", bytes.len()));
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("it can't be read: {e}"))?;
    match reader.format() {
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Bmp) => {}
        _ => return Err("it isn't a PNG, a JPEG or a BMP".into()),
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(65_535);
    limits.max_image_height = Some(65_535);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let (w, h) = reader.into_dimensions().map_err(|e| format!("it can't be read: {e}"))?;
    too_big(w, h)?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("it can't be read: {e}"))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| format!("it can't be read: {e}"))?;
    let (w, h) = (img.width(), img.height());
    Ok(match img {
        image::DynamicImage::ImageLuma8(g) => Level { w, h, ch: 1, px: g.into_raw() },
        image::DynamicImage::ImageRgb8(c) => Level { w, h, ch: 3, px: c.into_raw() },
        other => {
            let rgba = other.into_rgba8();
            Level::from_rgba(w, h, rgba.as_raw())
        }
    })
}

/// The size a picture comes in at, mm (72 dpi, as Revit takes one), read from its header: where a front end outlines it
/// before it's placed.
pub fn picture_size(bytes: &[u8]) -> Result<[f64; 2], String> {
    let reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("it can't be read: {e}"))?;
    match reader.format() {
        Some(image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::Bmp) => {}
        _ => return Err("it isn't a PNG, a JPEG or a BMP".into()),
    }
    let (w, h) = reader.into_dimensions().map_err(|e| format!("it can't be read: {e}"))?;
    too_big(w, h)?;
    let mm = 25.4 / f64::from(PICTURE_DPI);
    Ok([f64::from(w) * mm, f64::from(h) * mm])
}

impl ImageType {
    /// A picture, Insert > Image: a PNG, JPEG or BMP file's `bytes`, known by `source`, taken at 72 dpi as Revit takes one.
    pub fn picture(source: &str, bytes: &[u8]) -> Result<ImageType, String> {
        let level = decode(bytes)?;
        let png = encode(&level)?;
        let (w, h) = (level.w, level.h);
        let mm = 25.4 / f64::from(PICTURE_DPI);
        Ok(ImageType {
            source: source.chars().take(MAX_TEXT).collect(),
            page: None,
            dpi: PICTURE_DPI,
            size: [f64::from(w) * mm, f64::from(h) * mm],
            scale: None,
            png: Arc::new(png),
            px: (w, h),
            pixels: Arc::new(OnceLock::from(Some(Arc::new(Pixels::new(level, false))))),
            lines: None,
        })
    }

    /// A page read from a PDF ([`read_pdf`], here or apart), known by `source`; and notes on what was left out of it.
    pub fn from_pdf(source: &str, read: &PdfRead) -> (ImageType, Vec<String>) {
        let mut notes = read.notes.clone();
        let lines = read.dxf.as_ref().and_then(|dxf| match Cad::new(source, None, dxf.clone()) {
            Ok(cad) => Some(cad),
            Err(e) => {
                notes.push(format!(
                    "page {}'s lines couldn't be kept ({e}): it shows as a picture only, with nothing to snap to",
                    read.page
                ));
                None
            }
        });
        let pixels = read.level.clone().map_or_else(Arc::default, |l| Arc::new(OnceLock::from(Some(l))));
        let t = ImageType {
            source: source.chars().take(MAX_TEXT).collect(),
            page: u32::try_from(read.page).ok(),
            dpi: read.dpi,
            size: read.size,
            scale: read.scale,
            png: Arc::clone(&read.png),
            px: read.px,
            pixels,
            lines,
        };
        (t, notes)
    }

    /// The file it came from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The page of the PDF it is, from 1; none for a picture.
    pub fn page(&self) -> Option<u32> {
        self.page
    }

    /// The resolution its pixels are at, dots an inch.
    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    /// Its width and height, px.
    pub fn pixels(&self) -> (u32, u32) {
        self.px
    }

    /// Its width and height at its resolution, mm: a PDF's page, a picture's at 72 dpi.
    pub fn size(&self) -> [f64; 2] {
        self.size
    }

    /// What its page says a mm of it stands for, mm (100 at 1 : 100), when it says.
    pub fn stated_scale(&self) -> Option<f64> {
        self.scale
    }

    /// A PDF's lines and texts, as a drawing in mm of its page from its bottom left.
    pub fn lines(&self) -> Option<&Cad> {
        self.lines.as_ref()
    }

    /// Bytes of PNG it holds.
    pub fn len(&self) -> usize {
        self.png.len()
    }

    pub fn is_empty(&self) -> bool {
        self.png.is_empty()
    }

    /// Its pixels and their levels, decoded the first time they're asked for; none when its PNG can't be read.
    fn levels(&self) -> Option<Arc<Pixels>> {
        self.pixels.get_or_init(|| Some(Arc::new(Pixels::new(unpng(&self.png)?, self.page.is_some())))).clone()
    }

    /// A small picture of it, no side over `side` px: its width, height and RGBA (Manage Images, the box that places it).
    pub fn thumbnail(&self, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        let p = self.levels()?;
        let l = p.levels.iter().find(|l| l.w <= side && l.h <= side).or(p.levels.last())?;
        Some((l.w, l.h, l.rgba()))
    }
}

/// A page of a PDF read for Import PDF ([`read_pdf`]): which page of how many, the resolution it came in at, its size on
/// paper, mm, the scale it states, its pixels as a PNG (and, read here, decoded), its vectors as a DXF and notes on what was
/// left out. It holds nothing of the file it came from, so it may be read apart (in a worker of the browser of its own) and
/// handed over as bytes ([`PdfRead::to_bytes`]).
#[derive(Clone)]
pub struct PdfRead {
    pub pages: usize,
    pub page: usize,
    pub dpi: u32,
    pub size: [f64; 2],
    pub scale: Option<f64>,
    png: Arc<Vec<u8>>,
    px: (u32, u32),
    dxf: Option<Vec<u8>>,
    pub notes: Vec<String>,
    /// Its pixels, when it was read here: the image needn't decode its PNG again.
    level: Option<Arc<Pixels>>,
}

impl std::fmt::Debug for PdfRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PdfRead(page {} of {}, {} x {} px)", self.page, self.pages, self.px.0, self.px.1)
    }
}

/// What [`PdfRead::to_bytes`] writes ahead of its PNG and its DXF.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadHead {
    pages: usize,
    page: usize,
    dpi: u32,
    /// The size's and the scale's numbers by their bits, so they come over exactly.
    size: [u64; 2],
    scale: Option<u64>,
    png: usize,
    dxf: Option<usize>,
    notes: Vec<String>,
}

impl PdfRead {
    /// Itself as bytes, to hand over from where it was read: the length of a JSON head (4 bytes, little-endian), the head,
    /// the PNG and the DXF.
    pub fn to_bytes(&self) -> Vec<u8> {
        let head = ReadHead {
            pages: self.pages,
            page: self.page,
            dpi: self.dpi,
            size: self.size.map(f64::to_bits),
            scale: self.scale.map(f64::to_bits),
            png: self.png.len(),
            dxf: self.dxf.as_ref().map(Vec::len),
            notes: self.notes.clone(),
        };
        let head = serde_json::to_vec(&head).unwrap_or_default();
        let mut out = Vec::with_capacity(4 + head.len() + self.png.len() + self.dxf.as_ref().map_or(0, Vec::len));
        out.extend((head.len() as u32).to_le_bytes());
        out.extend(head);
        out.extend(self.png.iter());
        out.extend(self.dxf.iter().flatten());
        out
    }

    /// What [`PdfRead::to_bytes`] wrote, checked as a file is: what came from elsewhere is hostile.
    pub fn from_bytes(bytes: &[u8]) -> Result<PdfRead, String> {
        let at = |from: usize, len: usize| {
            from.checked_add(len).and_then(|to| bytes.get(from..to)).ok_or_else(|| "the page read isn't whole".to_string())
        };
        let n = u32::from_le_bytes(<[u8; 4]>::try_from(at(0, 4)?).map_err(|e| e.to_string())?) as usize;
        let head: ReadHead = serde_json::from_slice(at(4, n)?).map_err(|e| e.to_string())?;
        let png = at(4 + n, head.png)?.to_vec();
        let dxf = head.dxf.map(|d| at(4 + n + head.png, d).map(<[u8]>::to_vec)).transpose()?;
        let px = png_size(&png)?;
        let (size, scale) = (head.size.map(f64::from_bits), head.scale.map(f64::from_bits));
        let finest = RESOLUTIONS.iter().copied().max().unwrap_or(600);
        let sized = size.iter().all(|s| s.is_finite() && *s >= MIN_SIDE && *s <= crate::model::MAX_COORD);
        let scaled = scale.is_none_or(|k| k.is_finite() && k > 0.0 && k <= MAX_SCALE);
        if !(sized && scaled && (1..=finest).contains(&head.dpi) && (1..=head.pages).contains(&head.page) && head.pages <= pdf::MAX_PAGES) {
            return Err("the page read says what no page is".into());
        }
        let notes = head.notes.into_iter().take(16).map(|n| n.chars().take(MAX_TEXT).collect()).collect();
        Ok(PdfRead { pages: head.pages, page: head.page, dpi: head.dpi, size, scale, png: Arc::new(png), px, dxf, notes, level: None })
    }
}

/// Page `page` (from 1) of a PDF file's `bytes` read for Import PDF: rasterised at `dpi` (or the finest under it that keeps
/// it within [`MAX_PIXELS`], said in its notes), with what it draws as vectors. This is where a hostile file is read: in the
/// browser, only ever in a worker of its own ([`run_pdf_job`]).
pub fn read_pdf(bytes: Vec<u8>, page: usize, dpi: u32) -> Result<PdfRead, String> {
    if bytes.len() > MAX_FILE {
        return Err(format!("it's {} bytes; up to {MAX_FILE}", bytes.len()));
    }
    let doc = pdf::Document::open(bytes)?;
    let size = doc.size(page)?;
    let mut notes = Vec::new();
    let [w_in, h_in] = size.map(|s| s / 25.4);
    let fits = (MAX_PIXELS as f64 / (w_in * h_in).max(1e-9)).sqrt().floor();
    let mut dots = f64::from(dpi);
    if dots * w_in > 65_535.0 || dots * h_in > 65_535.0 || (dots * w_in) * (dots * h_in) > MAX_PIXELS as f64 {
        dots = fits.min((65_535.0 / w_in.max(h_in)).floor()).max(1.0);
        notes.push(format!(
            "page {page} is {:.0} × {:.0} mm: at {dpi} dpi it would be more than {MAX_PIXELS} pixels, so it came in at {dots} dpi",
            size[0], size[1]
        ));
    }
    // The page's pixels in the channels they need, taken straight from what hayro drew, which is then let go.
    let level = doc.render_into(page, dots, Level::from_rgba)?;
    let (w, h) = (level.w, level.h);
    too_big(w, h)?;
    let png = encode(&level)?;
    let vectors = doc.vectors(page)?;
    if vectors.left_out > 0 {
        notes.push(format!(
            "page {page} draws more than its lines can hold ({} points, or curves past the work a page may take): {} of its paths were left out of the lines it snaps to",
            pdf::MAX_SEGMENTS,
            vectors.left_out
        ));
    }
    if !vectors.cmaps.is_empty() {
        notes.push(format!(
            "page {page}'s text is set in character maps BIMCraft doesn't hold ({}): its Chinese, Japanese or Korean text is missing from its lines and may show wrongly",
            vectors.cmaps.join(", ")
        ));
    }
    Ok(PdfRead {
        pages: doc.pages(),
        page,
        dpi: dots as u32,
        size,
        scale: doc.scale(page),
        png: Arc::new(png),
        px: (w, h),
        dxf: (!vectors.is_empty()).then(|| vectors.dxf()),
        notes,
        level: Some(Arc::new(Pixels::new(level, true))),
    })
}

/// A PDF file's pages, as Import PDF's box and `pdf_pages` list them: how many it has, and each one's size, mm, and the
/// scale it states.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfOutline {
    pub pages: usize,
    pub list: Vec<PdfPage>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PdfPage {
    pub size: [f64; 2],
    pub scale: Option<f64>,
}

/// A PDF file's pages ([`PdfOutline`]), from its `bytes`.
pub fn pdf_outline(bytes: Vec<u8>) -> Result<PdfOutline, String> {
    if bytes.len() > MAX_FILE {
        return Err(format!("it's {} bytes; up to {MAX_FILE}", bytes.len()));
    }
    let doc = pdf::Document::open(bytes)?;
    let list = (1..=doc.pages()).map(|n| PdfPage { size: doc.size(n).unwrap_or([0.0, 0.0]), scale: doc.scale(n) }).collect();
    Ok(PdfOutline { pages: doc.pages(), list })
}

/// What is asked of a PDF file where it is read apart from the editor: its pages, a page's thumbnail no side over `side`
/// px, or a page read for import at a resolution.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "job", rename_all = "lowercase", deny_unknown_fields)]
pub enum PdfJob {
    Outline,
    Thumbnail { page: usize, side: u32 },
    Page { page: usize, dpi: u32 },
}

/// Does `job` on a PDF file's `bytes`, giving its answer as bytes: a [`PdfOutline`] as JSON, a thumbnail as its width and
/// height (4 bytes each, little-endian) and its RGBA, or a [`PdfRead`] as [`PdfRead::to_bytes`]. A panic in the reader
/// is an error where panics unwind (a thread of the window's); in the browser, where they don't, this runs in a worker of
/// its own, which an abort ends without the editor.
pub fn run_pdf_job(job: &PdfJob, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let run = || -> Result<Vec<u8>, String> {
        match *job {
            PdfJob::Outline => serde_json::to_vec(&pdf_outline(bytes)?).map_err(|e| e.to_string()),
            PdfJob::Thumbnail { page, side } => {
                if bytes.len() > MAX_FILE {
                    return Err(format!("it's {} bytes; up to {MAX_FILE}", bytes.len()));
                }
                let doc = pdf::Document::open(bytes)?;
                let [w, h] = doc.size(page)?;
                let dpi = (f64::from(side.clamp(8, 1024)) / (w.max(h) / 25.4).max(1e-3)).clamp(1.0, 72.0);
                let (w, h, rgba) = doc.render(page, dpi)?;
                let mut out = Vec::with_capacity(8 + rgba.len());
                out.extend(w.to_le_bytes());
                out.extend(h.to_le_bytes());
                out.extend(rgba);
                Ok(out)
            }
            PdfJob::Page { page, dpi } => Ok(read_pdf(bytes, page, dpi)?.to_bytes()),
        }
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| Err("it isn't a PDF BIMCraft can read".into()))
}

/// A thumbnail [`run_pdf_job`] gave: its width, height and RGBA, checked.
pub fn thumbnail_from_bytes(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let word = |i: usize| bytes.get(i..i + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
    let (Some(w), Some(h)) = (word(0), word(4)) else { return Err("the thumbnail isn't whole".into()) };
    let rgba = bytes.get(8..).unwrap_or_default();
    if w == 0 || h == 0 || w > 4096 || h > 4096 || rgba.len() != w as usize * h as usize * 4 {
        return Err("the thumbnail isn't whole".into());
    }
    Ok((w, h, rgba.to_vec()))
}

/// The PDF pages a session takes from elsewhere instead of reading the file (`import_pdf`, `pdf_pages`): those the window
/// read apart from the engine, by the file's path, and whether it may read one itself. A session in the browser's editor
/// may not ([`PdfReads::elsewhere`]): a hostile file read there would end the editor, so it is read in a worker of its own.
#[derive(Clone, Debug, Default)]
pub struct PdfReads {
    /// Whether every PDF must be read elsewhere and given ([`crate::engine::Session::give_pdf_page`]) before a command
    /// takes it.
    pub elsewhere: bool,
    pub(crate) outlines: BTreeMap<String, PdfOutline>,
    pub(crate) pages: BTreeMap<(String, usize, u32), Arc<PdfRead>>,
}

impl PdfReads {
    /// Lets go of what was given of the file at `key`, or of every file when none.
    pub fn forget(&mut self, key: Option<&str>) {
        match key {
            Some(k) => {
                self.outlines.remove(k);
                self.pages.retain(|(p, _, _), _| p != k);
            }
            None => {
                self.outlines.clear();
                self.pages.clear();
            }
        }
    }
}

/// Draws image `t` on `d` over the pixels `corners` (the picture's top left, top right, bottom right and bottom left, px of
/// `d`), as far as they lie in it: one picture, each of its pixels the image's nearest at the level that is about its size.
pub(crate) fn draw(d: &mut Drawing, t: &ImageType, corners: [(f64, f64); 4]) {
    let Some(p) = t.levels() else { return };
    let Some(base) = p.levels.first() else { return };
    let (vw, vh) = (f64::from(d.view.width), f64::from(d.view.height));
    let xs = corners.map(|c| c.0);
    let ys = corners.map(|c| c.1);
    let lo = (
        xs.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0),
        ys.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0),
    );
    let hi = (
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(vw),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(vh),
    );
    if !(lo.0 < hi.0 && lo.1 < hi.1) {
        return;
    }
    let (pw, ph) = ((hi.0 - lo.0) as u32, (hi.1 - lo.1) as u32);
    if u64::from(pw) * u64::from(ph) > MAX_DRAWN {
        return;
    }
    // Picture (u across, v down, 0 to 1) to pixel: the top left plus u along the top and v down the left side.
    let [tl, tr, _, bl] = corners;
    let (ex, ey) = ((tr.0 - tl.0, tr.1 - tl.1), (bl.0 - tl.0, bl.1 - tl.1));
    let det = ex.0 * ey.1 - ex.1 * ey.0;
    if det.abs() < 1e-9 {
        return;
    }
    // Pixels of the base level a pixel of the view spans: which level is drawn.
    let across = (ex.0.hypot(ex.1) / f64::from(base.w)).min(ey.0.hypot(ey.1) / f64::from(base.h));
    let step = if across > 0.0 { (1.0 / across).log2().floor().max(0.0) as usize } else { 0 };
    let Some(l) = p.levels.get(step.min(p.levels.len() - 1)) else { return };
    let mut rgba = vec![0_u8; pw as usize * ph as usize * 4];
    for y in 0..ph {
        let qy = lo.1 + f64::from(y) + 0.5 - tl.1;
        for x in 0..pw {
            let qx = lo.0 + f64::from(x) + 0.5 - tl.0;
            let u = (qx * ey.1 - qy * ey.0) / det;
            let v = (ex.0 * qy - ex.1 * qx) / det;
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let (sx, sy) = ((u * f64::from(l.w)) as u32, (v * f64::from(l.h)) as u32);
            let to = (y as usize * pw as usize + x as usize) * 4;
            if let Some(dst) = rgba.get_mut(to..to + 4) {
                dst.copy_from_slice(&l.at(sx, sy));
            }
        }
    }
    d.push(Prim::Image { at: (lo.0 as f32, lo.1 as f32), width: pw, height: ph, rgba, ink: false });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let rgba: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).flat_map(|(x, y)| f(x, y)).collect();
        encode(&Level::from_rgba(w, h, &rgba)).unwrap()
    }

    /// A one-page PDF 100 points a side with a black line across it.
    fn page() -> Vec<u8> {
        pdf::sample("0 0 0 RG 2 w 10 50 m 90 50 l S", [100.0, 100.0], Some((0.035_277_8, "m")))
    }

    #[test]
    fn a_page_whose_lines_cant_be_kept_says_so() {
        let mut read = read_pdf(page(), 1, 72).unwrap();
        assert!(read.dxf.is_some());
        read.dxf = Some(b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n".to_vec());
        let (t, notes) = ImageType::from_pdf("p.pdf", &read);
        assert!(t.lines().is_none());
        assert!(notes.iter().any(|n| n.contains("lines couldn't be kept")), "{notes:?}");
    }

    #[test]
    fn a_drawings_grey_page_holds_a_byte_a_pixel() {
        let read = read_pdf(page(), 1, 72).unwrap();
        let p = read.level.clone().unwrap();
        assert_eq!((p.levels[0].ch, p.levels[0].px.len()), (1, 100 * 100), "grey");
        // Its PNG is grey too, and decoded again it is the same.
        let (t, _) = ImageType::from_pdf("p.pdf", &PdfRead { level: None, ..read });
        let again = t.levels().unwrap();
        assert_eq!((again.levels[0].ch, &again.levels[0].px), (1, &p.levels[0].px));
        assert_eq!(again.levels[0].at(50, 50), [0, 0, 0, 255], "the line, in the middle");
        assert_eq!(again.levels[0].at(50, 20), [255, 255, 255, 255]);
    }

    #[test]
    fn a_page_read_apart_comes_over_whole_and_broken_bytes_are_refused() {
        let read = read_pdf(page(), 1, 150).unwrap();
        let bytes = read.to_bytes();
        let back = PdfRead::from_bytes(&bytes).unwrap();
        assert_eq!((back.pages, back.page, back.dpi, back.size, back.scale), (1, 1, 150, read.size, read.scale));
        let (a, _) = ImageType::from_pdf("p.pdf", &read);
        let (b, _) = ImageType::from_pdf("p.pdf", &back);
        assert_eq!(a, b);
        for cut in [0, 3, 10, bytes.len() / 2, bytes.len() - 1] {
            assert!(PdfRead::from_bytes(&bytes[..cut]).is_err(), "cut at {cut}");
        }
        let mut lying = bytes.clone();
        let n = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let head = String::from_utf8(bytes[4..4 + n].to_vec()).unwrap().replace("\"dpi\":150", "\"dpi\":0");
        lying.splice(4..4 + n, head.bytes());
        lying.splice(0..4, (head.len() as u32).to_le_bytes());
        assert!(PdfRead::from_bytes(&lying).is_err());
    }

    #[test]
    fn the_jobs_done_apart_give_the_pages_a_thumbnail_and_a_page() {
        let outline: PdfOutline = serde_json::from_slice(&run_pdf_job(&PdfJob::Outline, page()).unwrap()).unwrap();
        assert_eq!(outline.pages, 1);
        assert!((outline.list[0].scale.unwrap() - 100.0).abs() < 0.01);
        let (w, h, rgba) = thumbnail_from_bytes(&run_pdf_job(&PdfJob::Thumbnail { page: 1, side: 50 }, page()).unwrap()).unwrap();
        assert!(w <= 50 && h <= 50 && rgba.len() == (w * h * 4) as usize);
        let read = PdfRead::from_bytes(&run_pdf_job(&PdfJob::Page { page: 1, dpi: 72 }, page()).unwrap()).unwrap();
        assert_eq!(read.px, (100, 100));
        assert!(run_pdf_job(&PdfJob::Page { page: 2, dpi: 72 }, page()).unwrap_err().contains("no page 2"));
        assert!(run_pdf_job(&PdfJob::Outline, b"junk".to_vec()).is_err());
        let job: PdfJob = serde_json::from_str(r#"{"job": "page", "page": 1, "dpi": 300}"#).unwrap();
        assert_eq!(job, PdfJob::Page { page: 1, dpi: 300 });
    }

    #[test]
    fn what_a_project_holds_of_images_stays_within_what_a_browser_saves() {
        // Its PNG in base64, and room for the rest of the project, within Chrome's longest string.
        const LONGEST: usize = (1 << 29) - 24;
        const { assert!(MAX_HELD / 3 * 4 + (64 << 20) < LONGEST) };
    }

    #[test]
    fn a_picture_comes_in_at_72_dpi_and_reads_back_from_its_file() {
        let bytes = png(144, 72, |x, _| if x < 72 { [255, 0, 0, 255] } else { [0, 0, 255, 255] });
        let t = ImageType::picture("plan.png", &bytes).unwrap();
        assert_eq!(t.pixels(), (144, 72));
        let [w, h] = t.size();
        assert!((w - 50.8).abs() < 1e-9 && (h - 25.4).abs() < 1e-9);
        let back: ImageType = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        let (tw, th, px) = back.thumbnail(64).unwrap();
        assert!(tw <= 64 && th <= 64);
        assert_eq!(&px[0..3], &[255, 0, 0]);
    }

    #[test]
    fn what_isnt_a_picture_says_so() {
        assert!(ImageType::picture("a.txt", b"hello").unwrap_err().contains("isn't a PNG, a JPEG or a BMP"));
    }

    #[test]
    fn a_bmp_comes_in() {
        let mut bmp = Vec::new();
        image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30])).write_to(&mut Cursor::new(&mut bmp), image::ImageFormat::Bmp).unwrap();
        let t = ImageType::picture("a.bmp", &bmp).unwrap();
        assert_eq!(t.pixels(), (3, 2));
    }

    #[test]
    fn an_image_draws_its_pixels_where_its_corners_say() {
        let t =
            ImageType::picture("q.png", &png(2, 2, |x, y| [if x == 0 { 0 } else { 255 }, if y == 0 { 0 } else { 255 }, 0, 255])).unwrap();
        let mut d = Drawing::new(crate::render::View { scale: 1.0, cx: 50.0, cy: 50.0, width: 100, height: 100 });
        draw(&mut d, &t, [(10.0, 10.0), (30.0, 10.0), (30.0, 30.0), (10.0, 30.0)]);
        let Some(Prim::Image { at, width, height, rgba, .. }) = d.prims.first() else { panic!("no picture") };
        assert_eq!((*at, *width, *height), ((10.0, 10.0), 20, 20));
        let px = |x: usize, y: usize| &rgba[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4];
        assert_eq!(px(2, 2), [0, 0, 0, 255]);
        assert_eq!(px(17, 2), [255, 0, 0, 255]);
        assert_eq!(px(2, 17), [0, 255, 0, 255]);
    }
}

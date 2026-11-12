//! Raster images: Revit's Insert > Image and Import PDF. A project holds each picture once, as an [`ImageType`] (Revit's image
//! type, listed by Manage Images): its pixels, kept as a PNG, and, for a page of a PDF, the lines and texts it draws as
//! vectors, held as a drawing ([`crate::cad::Cad`]) so they snap and an agent reads them as an imported DXF's. The PDF itself
//! is not kept. An [`ImageInstance`] element places one in one view (a plan, a ceiling plan, a section, an elevation or a
//! sheet, in the view's own terms), its middle on `at`, `width` by `height`, turned, behind the model or in front of it.

pub mod pdf;

use std::io::Cursor;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::cad::Cad;
use crate::geom::P2;
use crate::model::{Guid, MAX_TEXT, is_zero};
use crate::render::{Drawing, Prim};

/// Images one project may hold.
pub const MAX_TYPES: usize = 256;
/// The most bytes of PNG a project's images may hold among them.
pub const MAX_HELD: usize = 1 << 30;
/// The largest file Import PDF or Insert > Image reads, bytes.
pub const MAX_FILE: usize = 256 << 20;
/// The most pixels one image may have: a page at a resolution that would have more is read at the finest that fits.
pub const MAX_PIXELS: u64 = 36_000_000;
/// Import PDF's resolutions, dots an inch, as Revit offers them, and the one it starts on.
pub const RESOLUTIONS: [u32; 4] = [72, 150, 300, 600];
pub const DEFAULT_DPI: u32 = 150;
/// The resolution a picture (not a PDF) is taken at, as Revit takes every raster image: 72 dpi.
pub const PICTURE_DPI: u32 = 72;
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
/// counter-clockwise about its middle and, `mirrored`, flipped left for right. `lock` keeps its proportions as it's sized
/// (Revit's Lock Proportions); `snaps` lets what's drawn over it snap to a PDF's lines (Enable Snaps); `layer` is its Draw Layer.
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
}

impl ImageInstance {
    /// Its corners in its view's terms: the picture's top left, top right, bottom right and bottom left, as it shows.
    pub fn corners(&self) -> [P2; 4] {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let (x, y) = (P2(cos, sin), P2(-sin, cos));
        let (hw, hh) = (self.width / 2.0, self.height / 2.0);
        let side = if self.mirrored { -1.0 } else { 1.0 };
        let at = |u: f64, v: f64| self.at + x * (u * hw * side) + y * (v * hh);
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
        // A mirror image is the page flipped top for bottom, turned half round.
        let (angle, stretch) = if self.mirrored { ((self.angle + 180.0).rem_euclid(360.0), -stretch) } else { (self.angle, stretch) };
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

/// The decoded pixels, RGBA, and their levels, each half the one before, made when first drawn.
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

struct Level {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

impl Level {
    /// Half its size: each pixel the darkest of the four it covers when `keep_lines` (a drawing's lines stay as it zooms out),
    /// else their mean.
    fn halved(&self, keep_lines: bool) -> Level {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
        let at = |x: u32, y: u32| {
            let i = (y.min(self.h - 1) as usize * self.w as usize + x.min(self.w - 1) as usize) * 4;
            self.rgba.get(i..i + 4).map_or([255; 4], |p| [p[0], p[1], p[2], p[3]])
        };
        for y in 0..h {
            for x in 0..w {
                let four = [at(2 * x, 2 * y), at(2 * x + 1, 2 * y), at(2 * x, 2 * y + 1), at(2 * x + 1, 2 * y + 1)];
                if keep_lines {
                    let dark = four.iter().min_by_key(|p| u32::from(p[0]) + u32::from(p[1]) + u32::from(p[2])).copied().unwrap_or([255; 4]);
                    rgba.extend(dark);
                } else {
                    for c in 0..4 {
                        rgba.push((four.iter().map(|p| u32::from(p[c])).sum::<u32>() / 4) as u8);
                    }
                }
            }
        }
        Level { w, h, rgba }
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

/// RGBA pixels as a PNG: grey, or colour without alpha, when that is all they are.
fn encode(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder as _;
    let px = rgba.as_chunks::<4>().0;
    let opaque = px.iter().all(|p| p[3] == 255);
    let grey = opaque && px.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        &mut out,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    let r = if grey {
        let g: Vec<u8> = px.iter().map(|p| p[0]).collect();
        encoder.write_image(&g, w, h, image::ExtendedColorType::L8)
    } else if opaque {
        let rgb: Vec<u8> = px.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        encoder.write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)
    } else {
        encoder.write_image(rgba, w, h, image::ExtendedColorType::Rgba8)
    };
    r.map_err(|e| e.to_string())?;
    Ok(out)
}

/// The pixels of a PNG, JPEG or BMP file, RGBA, held to [`MAX_PIXELS`] before they're decoded.
fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    if bytes.len() > MAX_FILE {
        return Err(format!("it's {} bytes; up to {MAX_FILE}", bytes.len()));
    }
    let mut reader =
        image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| format!("it can't be read: {e}"))?;
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
    let rgba = img.into_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
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
        let (w, h, rgba) = decode(bytes)?;
        let png = encode(w, h, &rgba)?;
        let mm = 25.4 / f64::from(PICTURE_DPI);
        Ok(ImageType {
            source: source.chars().take(MAX_TEXT).collect(),
            page: None,
            dpi: PICTURE_DPI,
            size: [f64::from(w) * mm, f64::from(h) * mm],
            scale: None,
            png: Arc::new(png),
            px: (w, h),
            pixels: Arc::new(OnceLock::from(Some(Arc::new(Pixels::new(Level { w, h, rgba }, false))))),
            lines: None,
        })
    }

    /// Page `page` (from 1) of a PDF file's `bytes`, known by `source`, rasterised at `dpi` (or the finest under it that
    /// keeps it within [`MAX_PIXELS`], said in the notes), with what it draws as vectors. Returns it and notes on what was
    /// left out.
    pub fn pdf(source: &str, bytes: Vec<u8>, page: usize, dpi: u32) -> Result<(ImageType, Vec<String>), String> {
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
        let (w, h, rgba) = doc.render(page, dots)?;
        too_big(w, h)?;
        let png = encode(w, h, &rgba)?;
        let vectors = doc.vectors(page)?;
        if vectors.left_out > 0 {
            notes.push(format!(
                "page {page} draws more than {} points of lines: {} of its paths were left out of the lines it snaps to",
                pdf::MAX_SEGMENTS,
                vectors.left_out
            ));
        }
        let lines = if vectors.is_empty() { None } else { Cad::new(source, None, vectors.dxf()).ok() };
        Ok((
            ImageType {
                source: source.chars().take(MAX_TEXT).collect(),
                page: u32::try_from(page).ok(),
                dpi: dots as u32,
                size,
                scale: doc.scale(page),
                png: Arc::new(png),
                px: (w, h),
                pixels: Arc::new(OnceLock::from(Some(Arc::new(Pixels::new(Level { w, h, rgba }, true))))),
                lines,
            },
            notes,
        ))
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
        self.pixels
            .get_or_init(|| {
                let img = image::load_from_memory_with_format(&self.png, image::ImageFormat::Png).ok()?.into_rgba8();
                let (w, h) = (img.width(), img.height());
                Some(Arc::new(Pixels::new(Level { w, h, rgba: img.into_raw() }, self.page.is_some())))
            })
            .clone()
    }

    /// A small picture of it, no side over `side` px: its width, height and RGBA (Manage Images, the box that places it).
    pub fn thumbnail(&self, side: u32) -> Option<(u32, u32, Vec<u8>)> {
        let p = self.levels()?;
        let l = p.levels.iter().find(|l| l.w <= side && l.h <= side).or(p.levels.last())?;
        Some((l.w, l.h, l.rgba.clone()))
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
    let lo = (xs.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0), ys.iter().copied().fold(f64::INFINITY, f64::min).floor().max(0.0));
    let hi = (xs.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(vw), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max).ceil().min(vh));
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
            let (sx, sy) = ((u * f64::from(l.w)) as usize, (v * f64::from(l.h)) as usize);
            let from = (sy * l.w as usize + sx) * 4;
            let to = (y as usize * pw as usize + x as usize) * 4;
            if let (Some(src), Some(dst)) = (l.rgba.get(from..from + 4), rgba.get_mut(to..to + 4)) {
                dst.copy_from_slice(src);
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
        encode(w, h, &rgba).unwrap()
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
        image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30]))
            .write_to(&mut Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .unwrap();
        let t = ImageType::picture("a.bmp", &bmp).unwrap();
        assert_eq!(t.pixels(), (3, 2));
    }

    #[test]
    fn an_image_draws_its_pixels_where_its_corners_say() {
        let t = ImageType::picture("q.png", &png(2, 2, |x, y| [if x == 0 { 0 } else { 255 }, if y == 0 { 0 } else { 255 }, 0, 255])).unwrap();
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

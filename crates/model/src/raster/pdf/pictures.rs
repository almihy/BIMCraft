//! The device a page's pictures are painted on: its raster images and stencil masks, each pixel the image's nearest, on
//! nothing, as the page shows them: inside each clipping path whatever its shape (a box at once, any other shape by a mask of
//! it, its edges smooth), as see-through as the transparency groups round them (the fill's opacity, `ca`, among them) and their
//! soft masks make them. What it fills and strokes, and its text, are left to its lines.

use std::sync::Arc;

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::{
    BlendMode, ClipPath, Device, DrawMode, DrawProps, FillRule, Image, ImageData, ImageDrawProps, LumaData, MaskType, Paint, SoftMask,
};
use hayro::kurbo::{Affine, BezPath, PathEl, Point, Rect};

use super::{MAX_PAINTED, paint};

/// The most pixels a mask (a clipping path's, or a soft mask's) covers: past it, a clip is its box and a soft mask lets all
/// through.
const MOST_MASKED: u64 = 64_000_000;

/// A clipping path, px: the box round it within the one before, and, when it is no box, how much of each pixel of that box it
/// lets through (0 to 255), the masks before it taken in.
#[derive(Clone)]
struct Clip {
    b: Rect,
    mask: Option<Arc<Mask>>,
}

/// How much of each pixel of a box (x0, y0 and its size, px) shows, 0 to 255.
struct Mask {
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    data: Vec<u8>,
}

impl Mask {
    fn at(&self, x: u32, y: u32) -> u8 {
        if x < self.x0 || y < self.y0 || x >= self.x0 + self.w || y >= self.y0 + self.h {
            return 0;
        }
        self.data.get(((y - self.y0) * self.w + (x - self.x0)) as usize).copied().unwrap_or(0)
    }
}

/// A transparency group open: how opaque it is, the groups round it taken in, and its soft mask.
struct Group<'a> {
    opacity: f32,
    mask: Option<SoftMask<'a>>,
}

pub(super) struct Pictures<'a> {
    pub(super) w: u32,
    pub(super) h: u32,
    pub(super) rgba: Vec<u8>,
    clips: Vec<Clip>,
    groups: Vec<Group<'a>>,
    /// The pictures painted.
    pub(super) drawn: usize,
    /// The pixels painted so far, held to [`MAX_PAINTED`].
    painted: u64,
    /// The pixels masked so far, held to [`MOST_MASKED`].
    masked: u64,
    /// The box of pixels painted, x0 y0 x1 y1 (the last two past it).
    pub(super) covered: Option<[u32; 4]>,
}

/// A path as tiny-skia's, moved by `-x0`, `-y0`.
fn skia_path(path: &BezPath, x0: f64, y0: f64) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    let p = |q: Point| ((q.x - x0) as f32, (q.y - y0) as f32);
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(a) => {
                let (x, y) = p(a);
                pb.move_to(x, y);
            }
            PathEl::LineTo(a) => {
                let (x, y) = p(a);
                pb.line_to(x, y);
            }
            PathEl::QuadTo(a, b) => {
                let ((ax, ay), (bx, by)) = (p(a), p(b));
                pb.quad_to(ax, ay, bx, by);
            }
            PathEl::CurveTo(a, b, c) => {
                let ((ax, ay), (bx, by), (cx, cy)) = (p(a), p(b), p(c));
                pb.cubic_to(ax, ay, bx, by, cx, cy);
            }
            PathEl::ClosePath => pb.close(),
        }
    }
    pb.finish()
}

/// Whether `path` is a box with its sides along the axes, as most clips are.
fn is_box(path: &BezPath) -> bool {
    let mut pts: Vec<Point> = Vec::new();
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(a) if pts.is_empty() => pts.push(a),
            PathEl::LineTo(a) => pts.push(a),
            PathEl::ClosePath => {}
            _ => return false,
        }
    }
    if pts.len() == 5 && pts.first() == pts.last() {
        pts.pop();
    }
    pts.len() == 4
        && (0..4).all(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % 4]);
            (a.x - b.x).abs() < 1e-6 || (a.y - b.y).abs() < 1e-6
        })
}

impl<'a> Pictures<'a> {
    pub(super) fn new(w: u32, h: u32) -> Pictures<'a> {
        let canvas = Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
        Pictures {
            w,
            h,
            rgba: vec![0; w as usize * h as usize * 4],
            clips: vec![Clip { b: canvas, mask: None }],
            groups: vec![Group { opacity: 1.0, mask: None }],
            drawn: 0,
            painted: 0,
            masked: 0,
            covered: None,
        }
    }

    /// The soft masks of the groups open, each made over the pixels from (`x0`, `y0`), `w` by `h`: how much of each pixel each
    /// lets through.
    fn soft_masks(&mut self, x0: u32, y0: u32, w: u32, h: u32) -> Vec<Mask> {
        let masks: Vec<SoftMask<'a>> = self.groups.iter().filter_map(|g| g.mask.clone()).collect();
        let mut out = Vec::new();
        for m in masks {
            let area = u64::from(w) * u64::from(h);
            if self.masked + area > MOST_MASKED {
                break;
            }
            self.masked += area;
            if let Some(mask) = soft_mask(&m, x0, y0, w, h) {
                out.push(mask);
            }
        }
        out
    }

    /// Paints an image of `dw` by `dh` pixels placed by `t` (its pixels to the page's): each of the page's pixels it covers
    /// takes `color(x, y)` of its nearest, an RGB and an opacity, as much of it as the clips, the groups and their soft masks
    /// let through.
    fn paint(&mut self, t: Affine, dw: u32, dh: u32, color: impl Fn(u32, u32) -> ([u8; 3], u8)) {
        if dw == 0 || dh == 0 || t.determinant().abs() < 1e-12 || !t.determinant().is_finite() {
            return;
        }
        let clip = self.clips.last().cloned().unwrap_or(Clip { b: Rect::ZERO, mask: None });
        let b = t.transform_rect_bbox(Rect::new(0.0, 0.0, f64::from(dw), f64::from(dh)));
        let (x0, y0) = (b.x0.max(clip.b.x0).max(0.0).floor(), b.y0.max(clip.b.y0).max(0.0).floor());
        let (x1, y1) = (b.x1.min(clip.b.x1).min(f64::from(self.w)).ceil(), b.y1.min(clip.b.y1).min(f64::from(self.h)).ceil());
        if !(x0 < x1 && y0 < y1) {
            return;
        }
        let area = (x1 - x0) * (y1 - y0);
        if self.painted as f64 + area > MAX_PAINTED as f64 {
            return;
        }
        let opacity = self.groups.last().map_or(1.0, |g| g.opacity);
        if opacity <= 0.0 {
            return;
        }
        self.painted += area as u64;
        self.drawn += 1;
        let (bx0, by0, bx1, by1) = (x0 as u32, y0 as u32, x1 as u32, y1 as u32);
        self.covered = Some(match self.covered {
            None => [bx0, by0, bx1, by1],
            Some([a, b, c, d]) => [a.min(bx0), b.min(by0), c.max(bx1), d.max(by1)],
        });
        let soft = self.soft_masks(bx0, by0, bx1 - bx0, by1 - by0);
        let back = t.inverse();
        for y in by0..by1 {
            for x in bx0..bx1 {
                let q = back * Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                if !(q.x >= 0.0 && q.y >= 0.0 && q.x < f64::from(dw) && q.y < f64::from(dh)) {
                    continue;
                }
                let (c, a) = color(q.x as u32, q.y as u32);
                // How much shows: its own opacity, the groups', the clip's and each soft mask's.
                let mut shown = f32::from(a) * opacity;
                if let Some(m) = &clip.mask {
                    shown *= f32::from(m.at(x, y)) / 255.0;
                }
                for m in &soft {
                    shown *= f32::from(m.at(x, y)) / 255.0;
                }
                let a = shown.round().clamp(0.0, 255.0) as u32;
                if a == 0 {
                    continue;
                }
                let i = (y as usize * self.w as usize + x as usize) * 4;
                if let Some(px) = self.rgba.get_mut(i..i + 4) {
                    // Over what is there, straight alpha.
                    let under = u32::from(px[3]);
                    let out = a * 255 + under * (255 - a);
                    for k in 0..3 {
                        let v = u32::from(c[k]) * a * 255 + u32::from(px[k]) * under * (255 - a);
                        px[k] = v.checked_div(out).unwrap_or(0) as u8;
                    }
                    px[3] = (out / 255) as u8;
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

/// Soft mask `m` over the pixels from (`x0`, `y0`), `w` by `h`: its group painted (its fills and strokes, a gradient in its
/// mean colour) over its backdrop, and of each pixel its luminosity (a luminosity mask) or its opacity (an alpha mask).
fn soft_mask(m: &SoftMask<'_>, x0: u32, y0: u32, w: u32, h: u32) -> Option<Mask> {
    let mut pix = tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let luminosity = matches!(m.mask_type(), MaskType::Luminosity);
    if luminosity {
        let [r, g, b, _] = m.background_color().to_rgba().to_rgba8();
        pix.fill(tiny_skia::Color::from_rgba8(r, g, b, 255));
    }
    let mut device = MaskPainter { pix: &mut pix, x0: f64::from(x0), y0: f64::from(y0), groups: vec![1.0] };
    m.interpret(&mut device);
    let data = pix
        .pixels()
        .iter()
        .map(|p| {
            let c = p.demultiply();
            if luminosity {
                let l = 0.30 * f32::from(c.red()) + 0.59 * f32::from(c.green()) + 0.11 * f32::from(c.blue());
                l.round().clamp(0.0, 255.0) as u8
            } else {
                c.alpha()
            }
        })
        .collect();
    Some(Mask { x0, y0, w, h, data })
}

/// The device a soft mask's group is painted on, over a part of the page, px: its fills and strokes in their colours (a
/// pattern's mean), as see-through as its groups make them.
struct MaskPainter<'p> {
    pix: &'p mut tiny_skia::Pixmap,
    x0: f64,
    y0: f64,
    groups: Vec<f32>,
}

impl<'a> Device<'a> for MaskPainter<'_> {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let placed = props.transform * path.clone();
        let b = placed.control_box();
        let Some((c, a)) = paint::rgba(&props.paint, [b.x0, b.y0, b.x1, b.y1]) else { return };
        let a = (f32::from(a) * self.groups.last().copied().unwrap_or(1.0)).round().clamp(0.0, 255.0) as u8;
        let Some(p) = skia_path(&placed, self.x0, self.y0) else { return };
        let mut sp = tiny_skia::Paint::default();
        sp.set_color_rgba8(c[0], c[1], c[2], a);
        sp.anti_alias = true;
        let id = tiny_skia::Transform::identity();
        let fill = |pix: &mut tiny_skia::Pixmap, rule: &FillRule| {
            let rule = if *rule == FillRule::EvenOdd { tiny_skia::FillRule::EvenOdd } else { tiny_skia::FillRule::Winding };
            pix.fill_path(&p, &sp, rule, id, None);
        };
        let stroke = |pix: &mut tiny_skia::Pixmap, s: &hayro::hayro_interpret::StrokeProps| {
            let k = props.transform.determinant().abs().sqrt();
            let st = tiny_skia::Stroke { width: (f64::from(s.line_width) * k).max(1.0) as f32, ..tiny_skia::Stroke::default() };
            pix.stroke_path(&p, &sp, &st, id, None);
        };
        match mode {
            DrawMode::Fill(rule) => fill(self.pix, rule),
            DrawMode::Stroke(s) => stroke(self.pix, s),
            DrawMode::FillAndStroke(rule, s) => {
                fill(self.pix, rule);
                stroke(self.pix, s);
            }
            DrawMode::Invisible => {}
        }
    }

    fn push_clip_path(&mut self, _: &ClipPath) {}

    fn push_transparency_group(&mut self, opacity: f32, _: Option<SoftMask<'a>>, _: BlendMode) {
        let o = self.groups.last().copied().unwrap_or(1.0) * opacity.clamp(0.0, 1.0);
        if self.groups.len() < 4096 {
            self.groups.push(o);
        }
    }

    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}

    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}

    fn pop_clip(&mut self) {}

    fn pop_transparency_group(&mut self) {
        if self.groups.len() > 1 {
            self.groups.pop();
        }
    }
}

impl<'a> Device<'a> for Pictures<'a> {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}

    fn push_clip_path(&mut self, clip: &ClipPath) {
        let b = clip.path.control_box();
        let now = self.clips.last().cloned().unwrap_or(Clip { b: Rect::ZERO, mask: None });
        let inner = Rect::new(b.x0.max(now.b.x0), b.y0.max(now.b.y0), b.x1.min(now.b.x1), b.y1.min(now.b.y1));
        let shows = inner.x0 <= inner.x1 && inner.y0 <= inner.y1 && inner.x0.is_finite() && inner.y1.is_finite();
        if !shows {
            self.clips.push(Clip { b: Rect::ZERO, mask: None });
            return;
        }
        // A box clips by its box; any other shape by a mask of it over its box, within the mask before.
        let (x0, y0) = (inner.x0.floor().max(0.0), inner.y0.floor().max(0.0));
        let (x1, y1) = (inner.x1.ceil().min(f64::from(self.w)), inner.y1.ceil().min(f64::from(self.h)));
        let (w, h) = ((x1 - x0).max(0.0) as u32, (y1 - y0).max(0.0) as u32);
        let area = u64::from(w) * u64::from(h);
        let mask = if is_box(&clip.path) || w == 0 || h == 0 || self.masked + area > MOST_MASKED {
            now.mask.clone()
        } else {
            self.masked += area;
            let made = (|| {
                let p = skia_path(&clip.path, x0, y0)?;
                let mut m = tiny_skia::Mask::new(w, h)?;
                let rule = if clip.fill == FillRule::EvenOdd { tiny_skia::FillRule::EvenOdd } else { tiny_skia::FillRule::Winding };
                m.fill_path(&p, rule, true, tiny_skia::Transform::identity());
                let (ox, oy) = (x0 as u32, y0 as u32);
                let mut data = m.data().to_vec();
                if let Some(before) = &now.mask {
                    for (i, v) in data.iter_mut().enumerate() {
                        let (x, y) = (ox + i as u32 % w, oy + i as u32 / w);
                        *v = ((u32::from(*v) * u32::from(before.at(x, y))) / 255) as u8;
                    }
                }
                Some(Arc::new(Mask { x0: ox, y0: oy, w, h, data }))
            })();
            made.or_else(|| now.mask.clone())
        };
        self.clips.push(Clip { b: inner, mask });
    }

    fn push_transparency_group(&mut self, opacity: f32, mask: Option<SoftMask<'a>>, _: BlendMode) {
        let o = self.groups.last().map_or(1.0, |g| g.opacity) * opacity.clamp(0.0, 1.0);
        if self.groups.len() < 4096 {
            self.groups.push(Group { opacity: o, mask });
        }
    }

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
                |mask, paint: &Paint<'_>| {
                    let (color, a) = paint::rgba(paint, [0.0; 4]).unwrap_or(([0, 0, 0], 255));
                    let (sx, sy) = mask.scale_factors;
                    let placed = t * Affine::scale_non_uniform(f64::from(sx), f64::from(sy));
                    let w = mask.width;
                    self.paint(placed, w, mask.height, |x, y| {
                        let m = mask.data.get(y as usize * w as usize + x as usize).copied().unwrap_or(0);
                        (color, ((u32::from(m) * u32::from(a)) / 255) as u8)
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

    fn pop_transparency_group(&mut self) {
        if self.groups.len() > 1 {
            self.groups.pop();
        }
    }
}

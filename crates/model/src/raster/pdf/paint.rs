//! What a page paints with, as a drawing can fill: a colour and how opaque it is. A colour is itself; a gradient (a shading)
//! is the mean of its colours over what it fills, sampled; a tiling pattern (a hatch, a brick, insulation) is the mean of the
//! colours its cell paints, as opaque as the share of the cell they cover. A drawing has no gradient or pattern fill of its
//! own, and a mean colour is what the page shows at a glance; the lines of a pattern are not the drawing's lines.

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::pattern::{Pattern, ShadingPattern, TilingPattern};
use hayro::hayro_interpret::shading::ShadingType;
use hayro::hayro_interpret::{BlendMode, ClipPath, Device, DrawMode, DrawProps, Image, ImageDrawProps, Paint, SoftMask};
use hayro::kurbo::{Affine, BezPath, ParamCurveArclen, Point, Shape};

/// A colour and its opacity, 0 to 255.
pub(super) type Rgba = ([u8; 3], u8);

/// How many samples a side a gradient is read at over what it fills.
const SAMPLES: usize = 8;
/// How deep patterns painted with patterns are read.
const DEPTH: u32 = 3;

/// What `paint` fills with: its colour and opacity, or a gradient's or a pattern's mean over box `over` (mm, x0 y0 x1 y1, what
/// it fills); none for a shading that paints nothing.
pub(super) fn rgba(paint: &Paint<'_>, over: [f64; 4]) -> Option<Rgba> {
    rgba_at(paint, over, 0)
}

fn rgba_at(paint: &Paint<'_>, over: [f64; 4], depth: u32) -> Option<Rgba> {
    match paint {
        Paint::Color(c) => {
            let [r, g, b, a] = c.to_rgba().to_rgba8();
            Some(([r, g, b], a))
        }
        Paint::Pattern(p) => match &**p {
            Pattern::Shading(s) => shading(s, over),
            Pattern::Tiling(t) => tiling(t, depth),
        },
    }
}

/// The mean of colours, each with a weight, and the opacity to give it.
fn mean(colors: impl IntoIterator<Item = ([f32; 4], f64)>, opacity: f64) -> Option<Rgba> {
    let (mut sum, mut weight) = ([0.0_f64; 4], 0.0);
    for (c, w) in colors {
        if !(w > 0.0 && w.is_finite()) {
            continue;
        }
        for (s, v) in sum.iter_mut().zip(c) {
            *s += f64::from(v) * w;
        }
        weight += w;
    }
    if weight <= 0.0 {
        return None;
    }
    let ch = |v: f64| (v / weight * 255.0).round().clamp(0.0, 255.0) as u8;
    let alpha = (sum[3] / weight * opacity * 255.0).round().clamp(0.0, 255.0) as u8;
    Some(([ch(sum[0]), ch(sum[1]), ch(sum[2])], alpha))
}

/// A gradient's mean over box `over`: its colour at points of the box, each where the shading puts it.
fn shading(s: &ShadingPattern, over: [f64; 4]) -> Option<Rgba> {
    let sh = &s.shading;
    let cs = &sh.color_space;
    let color = |c: &[f32]| cs.to_rgba(c, 1.0).components();
    let opacity = f64::from(s.opacity);
    let back = s.matrix.inverse();
    if !back.as_coeffs().iter().all(|v| v.is_finite()) {
        return None;
    }
    // The points of the box, in the shading's own space.
    let points = || {
        (0..SAMPLES).flat_map(move |i| {
            (0..SAMPLES).map(move |j| {
                let (u, v) = ((i as f64 + 0.5) / SAMPLES as f64, (j as f64 + 0.5) / SAMPLES as f64);
                back * Point::new(over[0] + (over[2] - over[0]) * u, over[1] + (over[3] - over[1]) * v)
            })
        })
    };
    let eval = |f: &hayro::hayro_interpret::shading::ShadingFunction, input: &[f32]| -> Option<[f32; 4]> {
        let mut out = vec![0.0_f32; f.num_outputs().clamp(1, 32)];
        f.eval(input, &mut out)?;
        Some(color(&out))
    };
    match &*sh.shading_type {
        ShadingType::RadialAxial { coords, domain, function, extend, axial } => {
            let [x0, y0, x1, y1, r0, r1] = coords.map(f64::from);
            let at = |p: Point| -> Option<f64> {
                let t = if *axial {
                    let (dx, dy) = (x1 - x0, y1 - y0);
                    let len = dx * dx + dy * dy;
                    if len <= 0.0 {
                        return None;
                    }
                    ((p.x - x0) * dx + (p.y - y0) * dy) / len
                } else {
                    // The larger t whose circle passes through the point.
                    let (cx, cy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                    let (px, py) = (p.x - x0, p.y - y0);
                    let a = cx * cx + cy * cy - dr * dr;
                    let b = px * cx + py * cy + r0 * dr;
                    let c = px * px + py * py - r0 * r0;
                    if a.abs() < 1e-12 {
                        if b.abs() < 1e-12 {
                            return None;
                        }
                        c / (2.0 * b)
                    } else {
                        let disc = b * b - a * c;
                        if disc < 0.0 {
                            return None;
                        }
                        (b + disc.sqrt()) / a
                    }
                };
                match t {
                    t if t < 0.0 && !extend[0] => None,
                    t if t > 1.0 && !extend[1] => None,
                    t => Some(t.clamp(0.0, 1.0)),
                }
            };
            let [d0, d1] = domain.map(f64::from);
            mean(points().filter_map(|p| Some((eval(function, &[(d0 + (d1 - d0) * at(p)?) as f32])?, 1.0))), opacity)
        }
        ShadingType::FunctionBased { domain, matrix, function } => {
            let inner = matrix.inverse();
            mean(
                points().filter_map(|p| {
                    let q = inner * p;
                    let [a, b, c, d] = domain.map(f64::from);
                    let inside = q.x >= a.min(b) && q.x <= a.max(b) && q.y >= c.min(d) && q.y <= c.max(d);
                    inside.then(|| Some((eval(function, &[q.x as f32, q.y as f32])?, 1.0))).flatten()
                }),
                opacity,
            )
        }
        ShadingType::TriangleMesh { triangles, function } => {
            let of = |c: &[f32]| match function {
                Some(f) => eval(f, &c[..1.min(c.len())]),
                None => Some(color(c)),
            };
            mean(triangles.iter().take(100_000).flat_map(|t| [&t.p0, &t.p1, &t.p2]).filter_map(|v| Some((of(&v.colors)?, 1.0))), opacity)
        }
        ShadingType::CoonsPatchMesh { patches, function } => {
            let of = |c: &[f32]| match function {
                Some(f) => eval(f, &c[..1.min(c.len())]),
                None => Some(color(c)),
            };
            mean(patches.iter().take(100_000).flat_map(|p| p.colors.iter()).filter_map(|c| Some((of(c)?, 1.0))), opacity)
        }
        ShadingType::TensorProductPatchMesh { patches, function } => {
            let of = |c: &[f32]| match function {
                Some(f) => eval(f, &c[..1.min(c.len())]),
                None => Some(color(c)),
            };
            mean(patches.iter().take(100_000).flat_map(|p| p.colors.iter()).filter_map(|c| Some((of(c)?, 1.0))), opacity)
        }
        ShadingType::Dummy => None,
    }
}

/// A tiling pattern's mean: the colours its cell paints, each by the area it covers, as opaque as their share of the cell.
fn tiling(t: &TilingPattern<'_>, depth: u32) -> Option<Rgba> {
    if depth >= DEPTH {
        return None;
    }
    let cell = f64::from(t.x_step).abs() * f64::from(t.y_step).abs();
    if !(cell > 0.0 && cell.is_finite()) {
        return None;
    }
    let mut device = Cover { painted: Vec::new(), depth, groups: vec![1.0] };
    t.interpret(&mut device, Affine::IDENTITY, false)?;
    let covered: f64 = device.painted.iter().map(|(_, a)| *a).sum();
    let share = (covered / cell).clamp(0.0, 1.0);
    let colors = device.painted.into_iter().map(|((c, a), area)| ([c[0], c[1], c[2]].map(|v| f32::from(v) / 255.0), area, a));
    let mut out = mean(colors.map(|(rgb, area, a)| ([rgb[0], rgb[1], rgb[2], f32::from(a) / 255.0], area)), 1.0)?;
    out.1 = (f64::from(out.1) * share * f64::from(t.opacity)).round().clamp(0.0, 255.0) as u8;
    Some(out)
}

/// A device that reads what a pattern's cell paints: each colour and the area it covers, in the cell's units.
struct Cover {
    painted: Vec<(Rgba, f64)>,
    depth: u32,
    groups: Vec<f32>,
}

impl Cover {
    fn opacity(&self) -> f32 {
        self.groups.last().copied().unwrap_or(1.0)
    }
}

/// The area a path fills, by the shoelace of its outline's chords (what its curves bulge is near enough for a mean).
fn area(path: &BezPath) -> f64 {
    let mut total = 0.0_f64;
    let (mut start, mut last) = (Point::ZERO, Point::ZERO);
    let mut ring = 0.0_f64;
    for el in path.elements() {
        use hayro::kurbo::PathEl;
        let next = match *el {
            PathEl::MoveTo(p) => {
                total += ring.abs();
                ring = 0.0;
                start = p;
                last = p;
                continue;
            }
            PathEl::LineTo(p) | PathEl::QuadTo(_, p) | PathEl::CurveTo(_, _, p) => p,
            PathEl::ClosePath => start,
        };
        ring += (last.x * next.y - next.x * last.y) / 2.0;
        last = next;
    }
    total += (ring + (last.x * start.y - start.x * last.y) / 2.0).abs();
    total
}

impl<'a> Device<'a> for Cover {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let placed = props.transform * path.clone();
        let covered = match mode {
            DrawMode::Fill(_) | DrawMode::FillAndStroke(..) => area(&placed),
            DrawMode::Stroke(s) => {
                let k = props.transform.determinant().abs().sqrt();
                placed.segments().map(|s| s.arclen(0.1)).sum::<f64>() * f64::from(s.line_width) * k
            }
            DrawMode::Invisible => return,
        };
        let b = placed.bounding_box();
        if let Some((c, a)) = rgba_at(&props.paint, [b.x0, b.y0, b.x1, b.y1], self.depth + 1) {
            let a = (f32::from(a) * self.opacity()).round().clamp(0.0, 255.0) as u8;
            if self.painted.len() < 10_000 && covered.is_finite() {
                self.painted.push(((c, a), covered));
            }
        }
    }

    fn push_clip_path(&mut self, _: &ClipPath) {}

    fn push_transparency_group(&mut self, opacity: f32, _: Option<SoftMask<'a>>, _: BlendMode) {
        let o = self.opacity() * opacity;
        self.groups.push(o);
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

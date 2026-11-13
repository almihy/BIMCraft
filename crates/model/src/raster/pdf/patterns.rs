//! A fill painted with a pattern, kept as the page paints it, on the layer it is drawn on: a tiling pattern (a brick, a batt
//! of insulation, earth) as its cell's own lines and fills, repeated over what it fills and cut to it; a shading (a gradient)
//! as bands of colour, each so near the next that no zoom tells them apart (a step of at most one in 255 in each channel, and
//! none narrower than [`NARROWEST`]), cut to what it fills. A fill whose pattern would make more than [`MOST_POINTS`] points,
//! or a pattern nested past [`DEPTH`], is filled in the pattern's mean colour instead ([`super::paint`]).

use hayro::hayro_interpret::pattern::{Pattern, ShadingPattern, TilingPattern};
use hayro::hayro_interpret::shading::{ShadingFunction, ShadingType, Triangle};
use hayro::kurbo::{Affine, Point, Vec2};

use super::shapes::{Pt, Region};
use super::{Collect, Run, Vectors, pt};

/// The most points the lines and fills of one pattern fill become.
const MOST_POINTS: usize = 400_000;
/// How deep patterns painted with patterns are worked out.
pub(super) const DEPTH: u32 = 3;
/// The narrowest band a gradient is cut into, mm on the paper, and the most bands one gradient is cut into.
const NARROWEST: f64 = 0.05;
const MOST_BANDS: usize = 2048;
/// The most triangles a mesh gradient (a shading of triangles or patches) is cut into, and the most cells a side a gradient of
/// a function of two inputs is.
const MOST_TRIANGLES: usize = 60_000;
const MOST_CELLS: usize = 96;
/// How far a circle's chords may stray from it, mm.
const ROUND: f64 = 0.02;
/// How far round any page a pattern is worked out, mm.
const FAR: f64 = 1e7;

/// A colour and opacity, each 0 to 1.
type Rgbaf = [f32; 4];

impl Collect {
    /// Keeps a fill of `rings` (mm, filling by the even-odd rule or the non-zero one) painted with pattern `p`, cut to the clip:
    /// true when it is kept as the page paints it, false when it is to be filled in the pattern's mean colour.
    pub(super) fn fill_pattern(&mut self, rings: &[Vec<Pt>], even_odd: bool, p: &Pattern<'_>) -> bool {
        if self.depth >= DEPTH {
            return false;
        }
        let rings: Vec<Vec<Pt>> = rings.iter().filter(|r| r.len() >= 3).cloned().collect();
        if rings.is_empty() {
            return true;
        }
        let region = self.clip().within(&rings, even_odd);
        let [x0, y0, x1, y1] = region.bounds();
        if !(x0 < x1 && y0 < y1 && x0 > -FAR && y0 > -FAR && x1 < FAR && y1 < FAR) {
            return true;
        }
        self.begun = true;
        self.clips.push(region);
        let done = match p {
            Pattern::Tiling(t) => self.tile(t, [x0, y0, x1, y1]),
            Pattern::Shading(s) => self.shade(s, [x0, y0, x1, y1]),
        };
        self.clips.pop();
        done
    }

    /// The next place in the page's order: each band and each tile's fill is a fill of its own.
    fn next_seq(&mut self) -> usize {
        self.seq += 1;
        self.seq
    }

    /// Keeps a filled ring in colour `c` (its opacity times the shading's and the groups'), at the next place in the order.
    fn band(&mut self, rings: Vec<Vec<Pt>>, even_odd: bool, c: Rgbaf, opacity: f32) {
        let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let alpha = self.seen(to(c[3] * opacity));
        if alpha == 0 {
            return;
        }
        let seq = self.next_seq();
        self.keep_fill(rings, even_odd, ([to(c[0]), to(c[1]), to(c[2])], alpha), seq);
    }

    /// A tiling pattern's cell, read once as lines and fills where the pattern puts its first cell, then laid again at every
    /// step of the pattern over box `b` (mm), each cut to what it fills.
    fn tile(&mut self, t: &TilingPattern<'_>, b: [f64; 4]) -> bool {
        let m = t.matrix;
        let back = m.inverse();
        let (xs, ys) = (f64::from(t.x_step), f64::from(t.y_step));
        if !(m.as_coeffs().iter().chain(back.as_coeffs().iter()).all(|v| v.is_finite()) && xs.is_finite() && ys.is_finite())
            || xs.abs() < 1e-9
            || ys.abs() < 1e-9
        {
            return false;
        }
        // The cell, on the layer the fill is on, clipped to its own box (the pattern clips it), its layers kept as they are.
        let mut cell = Collect {
            out: Vectors::default(),
            held: 0,
            worked: 0,
            clips: vec![Region::Box([-FAR, -FAR, FAR, FAR])],
            ids: std::mem::take(&mut self.ids),
            names: std::mem::take(&mut self.names),
            oc: vec![self.layer()],
            seq: 0,
            groups: vec![1.0],
            page: self.page,
            begun: true,
            depth: self.depth + 1,
        };
        let read = t.interpret(&mut cell, m, false);
        self.ids = std::mem::take(&mut cell.ids);
        self.names = std::mem::take(&mut cell.names);
        if read.is_none() || cell.out.left_out > 0 {
            return false;
        }
        let Vectors { strokes, fills, .. } = cell.out;
        let points: usize = strokes.iter().chain(&fills).map(|r| r.pts.len()).sum();
        if points == 0 {
            return true;
        }
        // The steps over the box: its corners in the pattern's space, against the cell's box there.
        let corners = [Point::new(b[0], b[1]), Point::new(b[2], b[1]), Point::new(b[2], b[3]), Point::new(b[0], b[3])].map(|q| back * q);
        let (px0, px1) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), q| (a.min(q.x), z.max(q.x)));
        let (py0, py1) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, z), q| (a.min(q.y), z.max(q.y)));
        let cb = t.bbox;
        let steps = |lo: f64, hi: f64, c0: f64, c1: f64, step: f64| -> (f64, f64) {
            let (a, z) = ((lo - c1) / step, (hi - c0) / step);
            (a.min(z).floor(), a.max(z).ceil())
        };
        let (i0, i1) = steps(px0, px1, cb.x0, cb.x1, xs);
        let (j0, j1) = steps(py0, py1, cb.y0, cb.y1, ys);
        let tiles = (i1 - i0 + 1.0) * (j1 - j0 + 1.0);
        if !(tiles.is_finite() && tiles >= 1.0) || tiles * points as f64 > MOST_POINTS as f64 {
            return false;
        }
        let opacity = t.opacity.clamp(0.0, 1.0);
        let step_x = m * Point::new(xs, 0.0) - m * Point::ORIGIN;
        let step_y = m * Point::new(0.0, ys) - m * Point::ORIGIN;
        // The cell's fills, ring by ring as each fill has them, in the cell's order.
        let mut groups: Vec<(usize, Vec<&Run>)> = Vec::new();
        for f in &fills {
            match groups.last_mut() {
                Some((seq, rings)) if *seq == f.seq => rings.push(f),
                _ => groups.push((f.seq, vec![f])),
            }
        }
        let see = |a: u8| (f32::from(a) * opacity).round().clamp(0.0, 255.0) as u8;
        for j in j0 as i64..=j1 as i64 {
            for i in i0 as i64..=i1 as i64 {
                let by: Vec2 = step_x * i as f64 + step_y * j as f64;
                let moved = |r: &Run| -> Vec<Pt> { r.pts.iter().map(|p| [p[0] + by.x, p[1] + by.y]).collect() };
                for (_, rings) in &groups {
                    let Some(first) = rings.first() else { continue };
                    let alpha = self.seen(see(first.alpha));
                    if alpha == 0 {
                        continue;
                    }
                    let seq = self.next_seq();
                    self.keep_fill(rings.iter().map(|r| moved(r)).collect(), true, (first.color, alpha), seq);
                }
                let seq = self.next_seq();
                for s in &strokes {
                    let alpha = self.seen(see(s.alpha));
                    if alpha == 0 {
                        continue;
                    }
                    self.keep_lines(Some(vec![(moved(s), s.closed)]), (s.color, alpha), s.width, s.dash.clone(), seq);
                }
            }
        }
        true
    }

    /// A shading over box `b` (mm), as bands, cells or triangles of its colours, each cut to what it fills: false when it is
    /// none it can be cut into.
    fn shade(&mut self, s: &ShadingPattern, b: [f64; 4]) -> bool {
        let m = s.matrix;
        let back = m.inverse();
        if !m.as_coeffs().iter().chain(back.as_coeffs().iter()).all(|v| v.is_finite()) {
            return false;
        }
        let opacity = s.opacity.clamp(0.0, 1.0);
        let sh = &s.shading;
        // A shading's own box clips it, in its space.
        let mut clipped = false;
        if let Some(path) = &sh.clip_path {
            match self.runs(path, m) {
                Some(runs) => {
                    let rings: Vec<Vec<Pt>> = runs.into_iter().map(|(p, _)| p).filter(|p| p.len() >= 3).collect();
                    let next = if rings.is_empty() { Region::nothing() } else { self.clip().within(&rings, false) };
                    self.clips.push(next);
                    clipped = true;
                }
                None => return false,
            }
        }
        // A pattern's background fills what it fills first.
        if let Some(bg) = &sh.background {
            let c = sh.color_space.to_rgba(bg, 1.0).components();
            let [x0, y0, x1, y1] = self.clip().bounds();
            self.band(vec![vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]], false, c, opacity);
        }
        let color = |comps: &[f32]| -> Rgbaf { sh.color_space.to_rgba(comps, 1.0).components() };
        let eval = |f: &ShadingFunction, input: &[f32]| -> Option<Rgbaf> {
            let mut out = vec![0.0_f32; f.num_outputs().clamp(1, 32)];
            f.eval(input, &mut out)?;
            Some(color(&out))
        };
        let done = match &*sh.shading_type {
            ShadingType::RadialAxial { coords, domain, function, extend, axial } => {
                let [d0, d1] = domain.map(f64::from);
                let at = |t: f64| eval(function, &[(d0 + (d1 - d0) * t.clamp(0.0, 1.0)) as f32]);
                if *axial {
                    self.axial(m, back, b, coords, *extend, &at, opacity)
                } else {
                    self.radial(m, b, coords, *extend, &at, opacity)
                }
            }
            ShadingType::FunctionBased { domain, matrix, function } => {
                self.function_based(m * *matrix, b, *domain, &|x, y| eval(function, &[x as f32, y as f32]), opacity)
            }
            ShadingType::TriangleMesh { triangles, function } => {
                let of = |c: &[f32]| match function {
                    Some(f) => eval(f, &c[..1.min(c.len())]),
                    None => Some(color(c)),
                };
                self.triangles(m, triangles.iter(), &of, opacity)
            }
            ShadingType::CoonsPatchMesh { patches, function } => {
                let of = |c: &[f32]| match function {
                    Some(f) => eval(f, &c[..1.min(c.len())]),
                    None => Some(color(c)),
                };
                let mut tris = Vec::new();
                for p in patches.iter().take(MOST_TRIANGLES) {
                    p.to_triangles(&mut tris);
                    if tris.len() > MOST_TRIANGLES {
                        break;
                    }
                }
                tris.len() <= MOST_TRIANGLES && self.triangles(m, tris.iter(), &of, opacity)
            }
            ShadingType::TensorProductPatchMesh { patches, function } => {
                let of = |c: &[f32]| match function {
                    Some(f) => eval(f, &c[..1.min(c.len())]),
                    None => Some(color(c)),
                };
                let mut tris = Vec::new();
                for p in patches.iter().take(MOST_TRIANGLES) {
                    p.to_triangles(&mut tris);
                    if tris.len() > MOST_TRIANGLES {
                        break;
                    }
                }
                tris.len() <= MOST_TRIANGLES && self.triangles(m, tris.iter(), &of, opacity)
            }
            ShadingType::Dummy => true,
        };
        if clipped {
            self.clips.pop();
        }
        done
    }

    /// An axial gradient from (x0, y0) to (x1, y1) of the shading's space (`m` to mm, `back` from it) over box `b`: bands across
    /// its axis, as many as its colours need, out to the box's ends where it extends.
    #[allow(clippy::too_many_arguments)]
    fn axial(
        &mut self,
        m: Affine,
        back: Affine,
        b: [f64; 4],
        coords: &[f32; 6],
        extend: [bool; 2],
        at: &dyn Fn(f64) -> Option<Rgbaf>,
        opacity: f32,
    ) -> bool {
        let [x0, y0, x1, y1, _, _] = coords.map(f64::from);
        let start = Point::new(x0, y0);
        let axis = Vec2::new(x1 - x0, y1 - y0);
        let len2 = axis.hypot2();
        if !(len2 > 0.0 && len2.is_finite()) {
            return true;
        }
        let across = Vec2::new(-axis.y, axis.x) / len2.sqrt();
        // The box's corners along the axis (t) and across it (u), in the shading's space.
        let corners = [Point::new(b[0], b[1]), Point::new(b[2], b[1]), Point::new(b[2], b[3]), Point::new(b[0], b[3])].map(|q| back * q);
        let (mut t0, mut t1, mut u0, mut u1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for q in corners {
            let v = q - start;
            let (t, u) = (v.dot(axis) / len2, v.dot(across));
            (t0, t1, u0, u1) = (t0.min(t), t1.max(t), u0.min(u), u1.max(u));
        }
        let (lo, hi) = (if extend[0] { t0 } else { t0.max(0.0) }, if extend[1] { t1 } else { t1.min(1.0) });
        if !(lo < hi && u0 < u1) {
            return true;
        }
        // Past its ends one colour each: the ends' bands go as far as the box.
        let mm_long = (m * (start + axis) - m * start).hypot();
        let place = |t: f64, u: f64| pt(m * (start + axis * t + across * u));
        let ts = cuts(lo.max(0.0).min(hi), hi.min(1.0).max(lo), mm_long, at);
        let mut edges: Vec<(f64, f64, Rgbaf)> = Vec::new();
        if lo < 0.0
            && let Some(c) = at(0.0)
        {
            edges.push((lo, 0.0, c));
        }
        edges.extend(ts);
        if hi > 1.0
            && let Some(c) = at(1.0)
        {
            edges.push((1.0, hi, c));
        }
        let solid = opacity >= 1.0 && edges.iter().all(|(_, _, c)| c[3] >= 1.0);
        let end = edges.last().map_or(hi, |e| e.1);
        for (a, z, c) in edges {
            // An opaque band goes on under all those after it, which draw over it, so no seam shows between at any zoom.
            let z = if solid { end } else { z };
            let ring = vec![place(a, u0), place(z, u0), place(z, u1), place(a, u1)];
            self.band(vec![ring], false, c, opacity);
        }
        true
    }

    /// A radial gradient between two circles of the shading's space (`m` to mm), over box `b`: each band the ring between two of
    /// its circles, the larger drawn under the smaller where its circles nest, as PDF paints the later circle over the
    /// earlier.
    fn radial(
        &mut self,
        m: Affine,
        b: [f64; 4],
        coords: &[f32; 6],
        extend: [bool; 2],
        at: &dyn Fn(f64) -> Option<Rgbaf>,
        opacity: f32,
    ) -> bool {
        let [x0, y0, r0, x1, y1, r1] = coords.map(f64::from);
        if !(r0 >= 0.0 && r1 >= 0.0) {
            return true;
        }
        let (c0, c1) = (Point::new(x0, y0), Point::new(x1, y1));
        let centre = |t: f64| c0 + (c1 - c0) * t;
        let radius = |t: f64| r0 + (r1 - r0) * t;
        let k = (m.determinant().abs()).sqrt();
        let circle = |t: f64| -> Vec<Pt> {
            let r = radius(t).max(0.0);
            let rm = (r * k).max(1e-6);
            let n = ((std::f64::consts::PI / (1.0 - (ROUND / rm).min(1.0)).acos()).ceil() as usize).clamp(16, 720);
            let c = centre(t);
            (0..n)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / n as f64;
                    pt(m * (c + Vec2::new(a.cos(), a.sin()) * r))
                })
                .collect()
        };
        // Whether each circle lies inside the next (their centres move less than their radii grow): the usual cone.
        let grows = r1 >= r0;
        let nested = (c1 - c0).hypot() <= (r1 - r0).abs() + 1e-9;
        let mm_long = ((c1 - c0).hypot() + (r1 - r0).abs()) * k;
        let edges = cuts(0.0, 1.0, mm_long, at);
        let solid = opacity >= 1.0 && edges.iter().all(|(_, _, c)| c[3] >= 1.0);
        let around_box = vec![[b[0], b[1]], [b[2], b[1]], [b[2], b[3]], [b[0], b[3]]];
        // The outer end extended: all of the box outside the larger end circle.
        let (big, small) = if grows { (1.0, 0.0) } else { (0.0, 1.0) };
        let ext_big = if grows { extend[1] } else { extend[0] };
        let ext_small = if grows { extend[0] } else { extend[1] };
        if nested
            && ext_big
            && let Some(c) = at(big)
        {
            // Opaque, the whole box, the discs drawn over it with no seam at their edge; else the box round the end circle.
            let rings = if solid { vec![around_box] } else { vec![around_box, circle(big)] };
            self.band(rings, true, c, opacity);
        }
        // Largest first, each smaller drawn over it.
        let mut order: Vec<(f64, f64, Rgbaf)> = edges;
        if grows {
            order.reverse();
        }
        let n = order.len();
        for (i, (a, z, c)) in order.into_iter().enumerate() {
            let (outer, inner) = if grows { (z, a) } else { (a, z) };
            let last = i + 1 == n;
            // Opaque and nested, a disc each, drawn over by the next; else the ring between its circles. The smallest is a ring
            // round the end circle when nothing fills inside it.
            let rings = if nested && solid && !(last && !ext_small && radius(small) > 0.0) {
                vec![circle(outer)]
            } else {
                vec![circle(outer), circle(inner)]
            };
            self.band(rings, true, c, opacity);
        }
        if nested
            && ext_small
            && radius(small) > 0.0
            && !solid
            && let Some(c) = at(small)
        {
            self.band(vec![circle(small)], false, c, opacity);
        }
        true
    }

    /// A gradient of a function of two inputs over its `domain` (x0 x1 y0 y1), placed by `m`: cells of its colours over the part
    /// of box `b` it covers, those in a row of one colour joined.
    fn function_based(&mut self, m: Affine, b: [f64; 4], domain: [f32; 4], at: &dyn Fn(f64, f64) -> Option<Rgbaf>, opacity: f32) -> bool {
        let back = m.inverse();
        if !back.as_coeffs().iter().all(|v| v.is_finite()) {
            return false;
        }
        let [a, bb, c, d] = domain.map(f64::from);
        let corners = [Point::new(b[0], b[1]), Point::new(b[2], b[1]), Point::new(b[2], b[3]), Point::new(b[0], b[3])].map(|q| back * q);
        let (lx, hx) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q.x), h.max(q.x)));
        let (ly, hy) = corners.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), q| (l.min(q.y), h.max(q.y)));
        let (x0, x1, y0, y1) = (lx.max(a.min(bb)), hx.min(a.max(bb)), ly.max(c.min(d)), hy.min(c.max(d)));
        if !(x0 < x1 && y0 < y1) {
            return true;
        }
        let side = |v: Vec2| v.hypot();
        let (wx, wy) = (side(m * Point::new(x1, y0) - m * Point::new(x0, y0)), side(m * Point::new(x0, y1) - m * Point::new(x0, y0)));
        let (nx, ny) = (((wx / 0.5).ceil() as usize).clamp(2, MOST_CELLS), ((wy / 0.5).ceil() as usize).clamp(2, MOST_CELLS));
        let (dx, dy) = ((x1 - x0) / nx as f64, (y1 - y0) / ny as f64);
        let near = |p: &Rgbaf, q: &Rgbaf| p.iter().zip(q).all(|(a, b)| (a - b).abs() <= 1.0 / 255.0);
        for j in 0..ny {
            let (ya, yz) = (y0 + dy * j as f64, y0 + dy * (j + 1) as f64);
            let mut run: Option<(usize, Rgbaf)> = None;
            for i in 0..=nx {
                let here = (i < nx).then(|| at(x0 + dx * (i as f64 + 0.5), (ya + yz) / 2.0)).flatten();
                match (&run, here) {
                    (Some((_, c)), Some(h)) if near(c, &h) => {}
                    _ => {
                        if let Some((from, c)) = run.take() {
                            let (xa, xz) = (x0 + dx * from as f64, x0 + dx * i as f64);
                            let ring = [(xa, ya), (xz, ya), (xz, yz), (xa, yz)].map(|(x, y)| pt(m * Point::new(x, y))).to_vec();
                            self.band(vec![ring], false, c, opacity);
                        }
                        run = here.map(|h| (i, h));
                    }
                }
            }
        }
        true
    }

    /// A mesh gradient's triangles (the shading's space, `m` to mm), each cut into smaller ones as its colours need, each of
    /// those filled in the colour at its middle.
    fn triangles<'t>(
        &mut self,
        m: Affine,
        tris: impl Iterator<Item = &'t Triangle>,
        of: &dyn Fn(&[f32]) -> Option<Rgbaf>,
        opacity: f32,
    ) -> bool {
        let mut made = 0_usize;
        for tri in tris {
            let v = [&tri.p0, &tri.p1, &tri.p2];
            let (Some(c0), Some(c1), Some(c2)) = (of(&v[0].colors), of(&v[1].colors), of(&v[2].colors)) else { continue };
            let spread = [c0, c1, c2]
                .iter()
                .flat_map(|a| [c0, c1, c2].map(move |b| (0..3).map(|k| (a[k] - b[k]).abs()).fold(0.0, f32::max)))
                .fold(0.0_f32, f32::max);
            let n = ((spread * 255.0 / 3.0).ceil() as usize).clamp(1, 8);
            made += n * n;
            if made > MOST_TRIANGLES {
                return false;
            }
            let p = [v[0].point, v[1].point, v[2].point];
            // A point and its components at barycentric (i, j) of n steps.
            let point = |i: usize, j: usize| -> Point {
                let (a, b) = (i as f64 / n as f64, j as f64 / n as f64);
                (p[0].to_vec2() + (p[1] - p[0]) * a + (p[2] - p[0]) * b).to_point()
            };
            let comps = |a: f64, b: f64| -> Vec<f32> {
                let len = v[0].colors.len().min(v[1].colors.len()).min(v[2].colors.len());
                (0..len)
                    .map(|k| {
                        let (x, y, z) = (f64::from(v[0].colors[k]), f64::from(v[1].colors[k]), f64::from(v[2].colors[k]));
                        (x + (y - x) * a + (z - x) * b) as f32
                    })
                    .collect()
            };
            for i in 0..n {
                for j in 0..n - i {
                    // The triangle pointing one way, and (but on the edge) the one pointing the other.
                    let up = [point(i, j), point(i + 1, j), point(i, j + 1)];
                    let mid = ((i as f64 + 1.0 / 3.0) / n as f64, (j as f64 + 1.0 / 3.0) / n as f64);
                    if let Some(c) = of(&comps(mid.0, mid.1)) {
                        self.band(vec![up.iter().map(|q| pt(m * *q)).collect()], false, c, opacity);
                    }
                    if i + j + 1 < n {
                        let down = [point(i + 1, j), point(i + 1, j + 1), point(i, j + 1)];
                        let mid = ((i as f64 + 2.0 / 3.0) / n as f64, (j as f64 + 2.0 / 3.0) / n as f64);
                        if let Some(c) = of(&comps(mid.0, mid.1)) {
                            self.band(vec![down.iter().map(|q| pt(m * *q)).collect()], false, c, opacity);
                        }
                    }
                }
            }
        }
        true
    }
}

/// A gradient's stretch from `lo` to `hi` (of its 0 to 1), `long` mm on the paper over 0 to 1, cut into bands, each from where
/// it starts to where the next does and in the colour at its middle: a band ends where the colour has moved one in 255 in any
/// channel, and none is narrower than [`NARROWEST`] or more than [`MOST_BANDS`] are made.
fn cuts(lo: f64, hi: f64, long: f64, at: &dyn Fn(f64) -> Option<Rgbaf>) -> Vec<(f64, f64, Rgbaf)> {
    if lo >= hi || lo.is_nan() || hi.is_nan() {
        return Vec::new();
    }
    let mm = (hi - lo) * long;
    let samples = if mm.is_finite() { ((mm / NARROWEST).ceil() as usize).clamp(2, MOST_BANDS) } else { 2 };
    let t_of = |k: usize| lo + (hi - lo) * k as f64 / samples as f64;
    let mut out = Vec::new();
    let mut from = 0_usize;
    let mut first = at(t_of(0));
    for k in 1..=samples {
        let here = if k < samples { at(t_of(k)) } else { None };
        let moved = match (&first, &here) {
            (Some(a), Some(b)) => a.iter().zip(b).any(|(x, y)| (x - y).abs() > 1.0 / 255.0),
            _ => true,
        };
        if moved {
            let (a, z) = (t_of(from), t_of(k));
            if let Some(c) = at((a + z) / 2.0) {
                out.push((a, z, c));
            }
            from = k;
            first = here;
        }
    }
    out
}

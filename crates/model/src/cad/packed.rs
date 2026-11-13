//! A drawing read from a PDF's page, packed as a project holds it: its layers, its linetypes and its entities (lines, fills
//! and texts) in a few bytes each, every point to a thousandth of a millimetre as the step from the one before. A page of a
//! million and a half points holds a few megabytes this way where its DXF would hold forty, and reads back in a fraction of
//! the time, so a dense sheet keeps its lines and layers. Read back as hostile, as any file is: every count and length is
//! bounded before anything is made of it.

use crate::dxf::{Align, Color, Dxf, Entity, Horizontal, Layer, Linetype, Shape, Vertical};
use crate::geom::P2;
use std::collections::HashMap;

/// What a packed drawing starts with, and its version.
const MAGIC: &[u8; 4] = b"BCL1";
/// The most entities, and the most points among them, a packed drawing holds.
pub const MAX_ENTITIES: usize = 8_000_000;
pub const MAX_POINTS: usize = 32_000_000;
/// The most layers and linetypes a packed drawing holds, the most dashes a linetype, rings a fill and bytes a name or a
/// text. What packs a drawing and what reads it back go by these same limits, so whatever is written reads back: past them, a
/// linetype keeps its first dashes, an entity on a layer or a linetype past the last held goes on the first layer or none,
/// and an entity past the last held is left out.
pub const MAX_LAYERS: usize = 1 << 16;
pub const MAX_LINETYPES: usize = 1 << 16;
pub const MAX_DASHES: usize = 64;
pub const MAX_RINGS: usize = 1 << 20;
const MAX_STRING: usize = 1 << 20;
/// The widest line held, mm.
pub const MAX_WEIGHT: f64 = 100.0;
/// A point's step, mm: what a coordinate is held to.
const STEP: f64 = 0.001;
/// The farthest a point may lie, mm, and the most a text's height or width may be.
const FAR: f64 = 1e8;

const POLYLINE: u8 = 0;
const FILL: u8 = 1;
const TEXT: u8 = 2;

/// Drawing `d` packed: its layers (name, colour, off), linetypes (name, dashes) and its polylines, fills and texts. What else it
/// holds (circles, arcs, points, blocks) a page never makes, and is left out. Held to the limits [`unpack`] reads by, so it
/// always reads back: a value out of range (a text a billion millimetres tall, a linetype of a hundred dashes) is brought
/// within them, and only what is past a count is left out.
pub fn pack(d: &Dxf) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + d.entities.len() * 24);
    out.extend_from_slice(MAGIC);
    let layers = &d.layers[..d.layers.len().min(MAX_LAYERS)];
    varint(&mut out, layers.len() as u64);
    for l in layers {
        string(&mut out, &l.name);
        let (r, g, b) = match l.color {
            Color::True(r, g, b) => (r, g, b),
            c => c.rgb().map_or((0, 0, 0), |c| (c.0, c.1, c.2)),
        };
        out.extend_from_slice(&[r, g, b, u8::from(l.off), u8::from(matches!(l.color, Color::Index(7)))]);
        string(&mut out, &l.linetype);
    }
    let linetypes = &d.linetypes[..d.linetypes.len().min(MAX_LINETYPES)];
    varint(&mut out, linetypes.len() as u64);
    for lt in linetypes {
        string(&mut out, &lt.name);
        let dashes = &lt.dashes[..lt.dashes.len().min(MAX_DASHES)];
        varint(&mut out, dashes.len() as u64);
        for x in dashes {
            let x = if x.is_finite() { x.clamp(-FAR, FAR) } else { 0.0 };
            out.extend_from_slice(&(x as f32).to_le_bytes());
        }
    }
    // Each layer and linetype by its name, the first of a name, as a reader finds it.
    let mut layer_at: HashMap<&str, u64> = HashMap::with_capacity(layers.len());
    for (i, l) in layers.iter().enumerate() {
        layer_at.entry(l.name.as_str()).or_insert(i as u64);
    }
    let mut linetype_at: HashMap<&str, u64> = HashMap::with_capacity(linetypes.len());
    for (i, l) in linetypes.iter().enumerate() {
        linetype_at.entry(l.name.as_str()).or_insert(i as u64 + 1);
    }
    let layer = |n: &str| layer_at.get(n).copied().unwrap_or(0);
    let linetype = |n: &Option<String>| n.as_deref().and_then(|n| linetype_at.get(n).copied()).unwrap_or(0);
    // What is held: the entities a page makes, up to the most entities and the most points among them.
    let mut held = 0_usize;
    let kept: Vec<&Entity> = d
        .entities
        .iter()
        .filter(|e| {
            let n = match &e.shape {
                Shape::Line(..) => 2,
                Shape::Polyline { pts, .. } => pts.len(),
                Shape::Fill { rings, .. } => rings.iter().take(MAX_RINGS).map(Vec::len).sum(),
                Shape::Text { .. } => 0,
                _ => return false,
            };
            if held.saturating_add(n) > MAX_POINTS {
                return false;
            }
            held += n;
            true
        })
        .take(MAX_ENTITIES)
        .collect();
    varint(&mut out, kept.len() as u64);
    // Each point the step from the last one written, of any entity.
    let mut last = (0_i64, 0_i64);
    for e in kept {
        let (r, g, b) = match e.color {
            Color::True(r, g, b) => (r, g, b),
            c => c.rgb().map_or((0, 0, 0), |c| (c.0, c.1, c.2)),
        };
        let kind = match e.shape {
            Shape::Fill { .. } => FILL,
            Shape::Text { .. } => TEXT,
            _ => POLYLINE,
        };
        out.push(kind);
        varint(&mut out, layer(&e.layer));
        out.extend_from_slice(&[r, g, b, e.alpha]);
        match &e.shape {
            Shape::Line(a, b) => {
                varint(&mut out, weight(e.weight));
                varint(&mut out, linetype(&e.linetype));
                out.push(0);
                points(&mut out, &[*a, *b], &mut last);
            }
            Shape::Polyline { pts, closed } => {
                varint(&mut out, weight(e.weight));
                varint(&mut out, linetype(&e.linetype));
                out.push(u8::from(*closed));
                points(&mut out, pts, &mut last);
            }
            Shape::Fill { rings, solid } => {
                out.push(u8::from(*solid));
                let rings = &rings[..rings.len().min(MAX_RINGS)];
                varint(&mut out, rings.len() as u64);
                for r in rings {
                    points(&mut out, r, &mut last);
                }
            }
            Shape::Text { at, height, angle, text, align, wide, mirrored, .. } => {
                // Each a number within reach: a height that isn't a number is none, which reads back as a text left out.
                let within = |x: f64| if x.is_finite() { x.clamp(-FAR, FAR) } else { 0.0 };
                let angle = if angle.is_finite() { angle.rem_euclid(360.0) } else { 0.0 };
                for x in [within(at.0), within(at.1), within(*height), angle, within(*wide)] {
                    out.extend_from_slice(&x.to_le_bytes());
                }
                let h = match align.h {
                    Horizontal::Left => 0,
                    Horizontal::Center => 1,
                    Horizontal::Right => 2,
                };
                let v = match align.v {
                    Vertical::Baseline => 0,
                    Vertical::Bottom => 1,
                    Vertical::Middle => 2,
                    Vertical::Top => 3,
                };
                out.push(h | (v << 2) | (u8::from(*mirrored) << 4));
                string(&mut out, text);
            }
            _ => {}
        }
    }
    out
}

/// A line weight as packed: a thousandth of a millimetre past one, 0 for none; at most [`MAX_WEIGHT`].
fn weight(w: Option<f64>) -> u64 {
    w.filter(|w| w.is_finite() && *w >= 0.0).map_or(0, |w| (w.min(MAX_WEIGHT) * 1000.0).round() as u64 + 1)
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// A string, its first [`MAX_STRING`] bytes at most (cut where a character ends).
fn string(out: &mut Vec<u8>, s: &str) {
    let mut end = s.len().min(MAX_STRING);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    varint(out, end as u64);
    out.extend_from_slice(&s.as_bytes()[..end]);
}

fn points(out: &mut Vec<u8>, pts: &[P2], last: &mut (i64, i64)) {
    varint(out, pts.len() as u64);
    for p in pts {
        let q = ((p.0.clamp(-FAR, FAR) / STEP).round() as i64, (p.1.clamp(-FAR, FAR) / STEP).round() as i64);
        varint(out, zigzag(q.0 - last.0));
        varint(out, zigzag(q.1 - last.1));
        *last = q;
    }
}

/// Reads a packed drawing back, bounded at every step.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
    points: usize,
}

impl Reader<'_> {
    fn bad() -> String {
        "its packed lines are cut short or aren't lines".to_string()
    }

    fn byte(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.at).ok_or_else(Self::bad)?;
        self.at += 1;
        Ok(v)
    }

    fn bytes(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).ok_or_else(Self::bad)?;
        let v = self.b.get(self.at..end).ok_or_else(Self::bad)?;
        self.at = end;
        Ok(v)
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0_u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            v |= u64::from(b & 0x7F) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(Self::bad())
    }

    /// A count, at most `most`.
    fn count(&mut self, most: usize) -> Result<usize, String> {
        let n = self.varint()?;
        usize::try_from(n).ok().filter(|n| *n <= most).ok_or_else(|| format!("its packed lines hold {n} of what is held to {most}"))
    }

    fn string(&mut self) -> Result<String, String> {
        let n = self.count(MAX_STRING)?;
        Ok(String::from_utf8_lossy(self.bytes(n)?).chars().take(crate::model::MAX_TEXT * 4).collect())
    }

    /// A number; none when it isn't one within [`FAR`], which leaves out what it belongs to, not the drawing.
    fn f64(&mut self) -> Result<Option<f64>, String> {
        let b = self.bytes(8)?;
        let v = f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        Ok((v.is_finite() && v.abs() <= FAR).then_some(v))
    }

    fn points(&mut self, last: &mut (i64, i64)) -> Result<Vec<P2>, String> {
        let n = self.count(MAX_POINTS - self.points)?;
        self.points += n;
        let mut pts = Vec::with_capacity(n.min(1 << 20));
        let far = (FAR / STEP) as i64;
        for _ in 0..n {
            let (dx, dy) = (self.varint()?, self.varint()?);
            let un = |z: u64| ((z >> 1) as i64) ^ -((z & 1) as i64);
            let q = (last.0.saturating_add(un(dx)), last.1.saturating_add(un(dy)));
            if q.0.abs() > far || q.1.abs() > far {
                return Err(Self::bad());
            }
            *last = q;
            pts.push(P2(q.0 as f64 * STEP, q.1 as f64 * STEP));
        }
        Ok(pts)
    }
}

/// The drawing packed `bytes` hold, in mm, or why they hold none.
pub fn unpack(bytes: &[u8]) -> Result<Dxf, String> {
    let mut r = Reader { b: bytes, at: 0, points: 0 };
    if r.bytes(4)? != MAGIC {
        return Err("its packed lines are of a kind this build doesn't read".into());
    }
    let n = r.count(MAX_LAYERS)?;
    let mut layers = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.string()?;
        let c = r.bytes(5)?;
        let (rgb, off, ink) = ([c[0], c[1], c[2]], c[3] != 0, c[4] != 0);
        let linetype = r.string()?;
        let color = if ink { Color::Index(7) } else { Color::True(rgb[0], rgb[1], rgb[2]) };
        layers.push(Layer { name, color, linetype, weight: None, off, frozen: false });
    }
    let n = r.count(MAX_LINETYPES)?;
    let mut linetypes = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.string()?;
        let k = r.count(MAX_DASHES)?;
        let mut dashes = Vec::with_capacity(k);
        for _ in 0..k {
            let b = r.bytes(4)?;
            let x = f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
            dashes.push(if x.is_finite() { x.clamp(-FAR, FAR) } else { 0.0 });
        }
        linetypes.push(Linetype { name, dashes });
    }
    let n = r.count(MAX_ENTITIES)?;
    let mut entities = Vec::with_capacity(n.min(1 << 20));
    let mut last = (0, 0);
    for _ in 0..n {
        let kind = r.byte()?;
        let layer = layers.get(r.count(MAX_LAYERS)?).map(|l| l.name.clone()).unwrap_or_else(|| "0".into());
        let c = r.bytes(4)?;
        let (color, alpha) = (Color::True(c[0], c[1], c[2]), c[3]);
        let (shape, weight, linetype) = match kind {
            POLYLINE => {
                let w = r.varint()?;
                let weight = (w > 0).then(|| ((w - 1) as f64 / 1000.0).min(MAX_WEIGHT));
                let lt = r.varint()?;
                let linetype =
                    (lt > 0).then(|| usize::try_from(lt - 1).ok().and_then(|i| linetypes.get(i)).map(|l| l.name.clone())).flatten();
                let closed = r.byte()? != 0;
                let pts = r.points(&mut last)?;
                if pts.len() < 2 {
                    continue;
                }
                (Shape::Polyline { pts, closed }, weight, linetype)
            }
            FILL => {
                let solid = r.byte()? != 0;
                let k = r.count(MAX_RINGS)?;
                let mut rings = Vec::with_capacity(k.min(1024));
                for _ in 0..k {
                    let ring = r.points(&mut last)?;
                    if ring.len() >= 3 {
                        rings.push(ring);
                    }
                }
                if rings.is_empty() {
                    continue;
                }
                (Shape::Fill { rings, solid }, None, None)
            }
            TEXT => {
                let (x, y, height, angle, wide) = (r.f64()?, r.f64()?, r.f64()?, r.f64()?, r.f64()?);
                let flags = r.byte()?;
                let h = match flags & 3 {
                    1 => Horizontal::Center,
                    2 => Horizontal::Right,
                    _ => Horizontal::Left,
                };
                let v = match (flags >> 2) & 3 {
                    1 => Vertical::Bottom,
                    2 => Vertical::Middle,
                    3 => Vertical::Top,
                    _ => Vertical::Baseline,
                };
                let text = r.string()?;
                let (Some(x), Some(y), Some(height), Some(angle), Some(wide)) = (x, y, height, angle, wide) else { continue };
                if height <= 0.0 {
                    continue;
                }
                let wide = if wide > 0.0 { wide.clamp(0.01, 100.0) } else { 1.0 };
                let shape =
                    Shape::Text { at: P2(x, y), height, angle, text, align: Align { h, v }, tag: None, wide, mirrored: flags & 16 != 0 };
                (shape, None, None)
            }
            _ => return Err(Reader::bad()),
        };
        entities.push(Entity { layer, color, linetype, weight, alpha, shape });
    }
    Ok(Dxf {
        scale: Some(1.0),
        unit: "mm".into(),
        version: String::new(),
        layers,
        linetypes,
        ltscale: 1.0,
        entities,
        inserts: Vec::new(),
        extents: None,
        left_out: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drawing_packs_and_comes_back_as_it_was_to_a_micron() {
        let layer = |n: &str, off| Layer {
            name: n.into(),
            color: Color::True(10, 20, 30),
            linetype: "Continuous".into(),
            weight: None,
            off,
            frozen: false,
        };
        let d = Dxf {
            scale: Some(1.0),
            unit: "mm".into(),
            version: String::new(),
            layers: vec![layer("Walls", false), layer("Notes", true)],
            linetypes: vec![Linetype { name: "DASH1".into(), dashes: vec![4.233, -1.058, 0.7, -1.058] }],
            ltscale: 1.0,
            entities: vec![
                Entity {
                    layer: "Walls".into(),
                    color: Color::True(255, 0, 0),
                    linetype: Some("DASH1".into()),
                    weight: Some(0.35),
                    alpha: 128,
                    shape: Shape::Polyline { pts: vec![P2(1.001, 2.0), P2(300.25, -4.5)], closed: true },
                },
                Entity {
                    layer: "Walls".into(),
                    color: Color::True(0, 0, 0),
                    linetype: None,
                    weight: None,
                    alpha: 255,
                    shape: Shape::Fill { rings: vec![vec![P2(0.0, 0.0), P2(10.0, 0.0), P2(0.0, 10.0)]], solid: true },
                },
                Entity {
                    layer: "Notes".into(),
                    color: Color::True(0, 0, 255),
                    linetype: None,
                    weight: None,
                    alpha: 255,
                    shape: Shape::Text {
                        at: P2(5.0, 6.0),
                        height: 2.5,
                        angle: 30.0,
                        text: "Kitchen".into(),
                        align: Align { h: Horizontal::Left, v: Vertical::Baseline },
                        tag: None,
                        wide: 0.5,
                        mirrored: true,
                    },
                },
            ],
            inserts: Vec::new(),
            extents: None,
            left_out: Default::default(),
        };
        let back = unpack(&pack(&d)).unwrap();
        assert_eq!(back.layers, d.layers);
        assert_eq!(back.linetypes.len(), 1);
        assert!((back.linetypes[0].dashes[0] - 4.233).abs() < 1e-5);
        assert_eq!(back.entities.len(), 3);
        assert_eq!(
            (back.entities[0].weight, back.entities[0].alpha, back.entities[0].linetype.as_deref()),
            (Some(0.35), 128, Some("DASH1"))
        );
        let Shape::Polyline { pts, closed: true } = &back.entities[0].shape else { panic!("{:?}", back.entities[0]) };
        assert!((pts[0].0 - 1.001).abs() < 1e-9 && (pts[1].1 + 4.5).abs() < 1e-9, "{pts:?}");
        assert_eq!(back.entities[2].shape, d.entities[2].shape);
        // Cut short, or another kind of bytes: refused, never a panic.
        let bytes = pack(&d);
        for cut in 0..bytes.len() {
            let _ = unpack(&bytes[..cut]);
        }
        assert!(unpack(b"BCL1\xff\xff\xff\xff\xff\xff\xff\xff\xff\xff").is_err());
    }

    /// A drawing of `n` random entities on random layers and linetypes, with values a hostile page might give: dashes of any
    /// count, texts of any height, NaN and infinities, far points.
    fn random(seed: u64, n: usize) -> Dxf {
        let mut x = seed.max(1);
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let wild = |next: &mut dyn FnMut() -> u64| -> f64 {
            match next() % 10 {
                0 => f64::NAN,
                1 => f64::INFINITY,
                2 => -1e300,
                3 => 1e9 * (next() % 1000) as f64,
                4 => -((next() % 1000) as f64),
                _ => (next() % 1_000_000) as f64 / 1000.0,
            }
        };
        let layers: Vec<Layer> = (0..1 + next() % 8)
            .map(|i| Layer {
                name: format!("L{i}"),
                color: Color::True(1, 2, 3),
                linetype: "Continuous".into(),
                weight: None,
                off: false,
                frozen: false,
            })
            .collect();
        let linetypes: Vec<Linetype> = (0..next() % 6)
            .map(|i| Linetype { name: format!("T{i}"), dashes: (0..next() % 140).map(|_| wild(&mut next)).collect() })
            .collect();
        let mut entities = Vec::new();
        for _ in 0..n {
            let layer = format!("L{}", next() % 10);
            let linetype = (next() % 2 == 0).then(|| format!("T{}", next() % 8));
            let weight = (next() % 2 == 0).then(|| wild(&mut next));
            let pts =
                |k: u64, next: &mut dyn FnMut() -> u64| -> Vec<P2> { (0..k).map(|_| P2(wild(&mut *next), wild(&mut *next))).collect() };
            let shape = match next() % 4 {
                0 => Shape::Line(P2(wild(&mut next), wild(&mut next)), P2(wild(&mut next), wild(&mut next))),
                1 => {
                    let k = next() % 6;
                    Shape::Polyline { pts: pts(k, &mut next), closed: next() % 2 == 0 }
                }
                2 => {
                    let rings = (0..next() % 4)
                        .map(|_| {
                            let k = next() % 7;
                            pts(k, &mut next)
                        })
                        .collect();
                    Shape::Fill { rings, solid: next() % 2 == 0 }
                }
                _ => Shape::Text {
                    at: P2(wild(&mut next), wild(&mut next)),
                    height: wild(&mut next),
                    angle: wild(&mut next),
                    text: "é".repeat((next() % 5) as usize),
                    align: Align { h: Horizontal::Center, v: Vertical::Top },
                    tag: None,
                    wide: wild(&mut next),
                    mirrored: next() % 2 == 0,
                },
            };
            entities.push(Entity { layer, color: Color::True(9, 9, 9), linetype, weight, alpha: (next() % 256) as u8, shape });
        }
        Dxf {
            scale: Some(1.0),
            unit: "mm".into(),
            version: String::new(),
            layers,
            linetypes,
            ltscale: 1.0,
            entities,
            inserts: Vec::new(),
            extents: None,
            left_out: Default::default(),
        }
    }

    #[test]
    fn whatever_is_packed_reads_back() {
        for seed in 1..=2_000_u64 {
            let d = random(seed, (seed % 40) as usize);
            let back = unpack(&pack(&d)).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            // Every line and fill with points enough comes back; a text only with a height.
            let lines = |d: &Dxf| {
                d.entities
                    .iter()
                    .filter(|e| match &e.shape {
                        Shape::Line(..) => true,
                        Shape::Polyline { pts, .. } => pts.len() >= 2,
                        Shape::Fill { rings, .. } => rings.iter().any(|r| r.len() >= 3),
                        _ => false,
                    })
                    .count()
            };
            assert_eq!(lines(&back), lines(&d), "seed {seed}");
            for e in &back.entities {
                assert!(e.weight.is_none_or(|w| (0.0..=MAX_WEIGHT).contains(&w)), "seed {seed}: {e:?}");
                if let Shape::Text { at, height, wide, .. } = &e.shape {
                    assert!(at.0.is_finite() && *height > 0.0 && *height <= FAR && wide.is_finite(), "seed {seed}: {e:?}");
                }
            }
            assert!(back.linetypes.iter().all(|l| l.dashes.len() <= MAX_DASHES && l.dashes.iter().all(|x| x.is_finite())));
        }
    }

    #[test]
    fn what_is_past_a_limit_is_brought_within_it_and_the_rest_reads_back() {
        let line = |layer: &str, linetype: Option<&str>| Entity {
            layer: layer.into(),
            color: Color::True(0, 0, 0),
            linetype: linetype.map(str::to_string),
            weight: Some(1e18),
            alpha: 255,
            shape: Shape::Line(P2(0.0, 0.0), P2(10.0, 0.0)),
        };
        let mut d = random(7, 0);
        // A linetype of 33 dashes doubled and phased, a text a billion millimetres tall, more layers and linetypes than held.
        d.linetypes = vec![Linetype { name: "LONG".into(), dashes: vec![1.0; 133] }];
        d.linetypes.extend((0..MAX_LINETYPES + 10).map(|i| Linetype { name: format!("P{i}"), dashes: vec![2.0, -1.0] }));
        d.layers.extend((0..MAX_LAYERS + 10).map(|i| Layer {
            name: format!("Many {i}"),
            color: Color::Index(7),
            linetype: "Continuous".into(),
            weight: None,
            off: false,
            frozen: false,
        }));
        d.entities = vec![
            line("L0", Some("LONG")),
            line("L0", None),
            line(&format!("Many {}", MAX_LAYERS + 5), Some(&format!("P{}", MAX_LINETYPES + 5))),
            Entity {
                shape: Shape::Text {
                    at: P2(50.0, 50.0),
                    height: 1e9,
                    angle: 0.0,
                    text: "HUGE".into(),
                    align: Align { h: Horizontal::Left, v: Vertical::Baseline },
                    tag: None,
                    wide: 1.0,
                    mirrored: false,
                },
                ..line("L0", None)
            },
        ];
        let back = unpack(&pack(&d)).unwrap();
        assert_eq!(back.entities.len(), 4);
        assert_eq!(back.linetypes.len(), MAX_LINETYPES);
        assert_eq!(back.linetypes[0].dashes.len(), MAX_DASHES);
        assert_eq!(back.layers.len(), MAX_LAYERS);
        assert_eq!(back.entities[0].linetype.as_deref(), Some("LONG"));
        assert_eq!((back.entities[2].layer.as_str(), back.entities[2].linetype.as_deref()), ("L0", None));
        assert!(back.entities[..3].iter().all(|e| e.weight == Some(MAX_WEIGHT)));
        let Shape::Text { height, .. } = back.entities[3].shape else { panic!() };
        assert_eq!(height, FAR);
    }
}

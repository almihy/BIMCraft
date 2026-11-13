//! A drawing read from a PDF's page, packed as a project holds it: its layers, its linetypes and its entities (lines, fills
//! and texts) in a few bytes each, every point to a thousandth of a millimetre as the step from the one before. A page of a
//! million and a half points holds a few megabytes this way where its DXF would hold forty, and reads back in a fraction of
//! the time, so a dense sheet keeps its lines and layers. Read back as hostile, as any file is: every count and length is
//! bounded before anything is made of it.

use crate::dxf::{Align, Color, Dxf, Entity, Horizontal, Layer, Linetype, Shape, Vertical};
use crate::geom::P2;

/// What a packed drawing starts with, and its version.
const MAGIC: &[u8; 4] = b"BCL1";
/// The most entities, and the most points among them, a packed drawing reads back.
pub const MAX_ENTITIES: usize = 8_000_000;
pub const MAX_POINTS: usize = 32_000_000;
/// A point's step, mm: what a coordinate is held to.
const STEP: f64 = 0.001;
/// The farthest a point may lie, mm.
const FAR: f64 = 1e8;

const POLYLINE: u8 = 0;
const FILL: u8 = 1;
const TEXT: u8 = 2;

/// Drawing `d` packed: its layers (name, colour, off), linetypes (name, dashes) and its polylines, fills and texts. What else it
/// holds (circles, arcs, points, blocks) a page never makes, and is left out.
pub fn pack(d: &Dxf) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + d.entities.len() * 24);
    out.extend_from_slice(MAGIC);
    varint(&mut out, d.layers.len() as u64);
    for l in &d.layers {
        string(&mut out, &l.name);
        let (r, g, b) = match l.color {
            Color::True(r, g, b) => (r, g, b),
            c => c.rgb().map_or((0, 0, 0), |c| (c.0, c.1, c.2)),
        };
        out.extend_from_slice(&[r, g, b, u8::from(l.off), u8::from(matches!(l.color, Color::Index(7)))]);
        string(&mut out, &l.linetype);
    }
    varint(&mut out, d.linetypes.len() as u64);
    for lt in &d.linetypes {
        string(&mut out, &lt.name);
        varint(&mut out, lt.dashes.len() as u64);
        for x in &lt.dashes {
            out.extend_from_slice(&(*x as f32).to_le_bytes());
        }
    }
    let layer = |n: &str| d.layers.iter().position(|l| l.name == n).unwrap_or(0) as u64;
    let linetype = |n: &Option<String>| n.as_ref().and_then(|n| d.linetypes.iter().position(|l| &l.name == n)).map_or(0, |i| i as u64 + 1);
    let kept: Vec<&Entity> = d
        .entities
        .iter()
        .filter(|e| matches!(e.shape, Shape::Polyline { .. } | Shape::Line(..) | Shape::Fill { .. } | Shape::Text { .. }))
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
                varint(&mut out, rings.len() as u64);
                for r in rings {
                    points(&mut out, r, &mut last);
                }
            }
            Shape::Text { at, height, angle, text, align, wide, mirrored, .. } => {
                for x in [at.0, at.1, *height, *angle, *wide] {
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

/// A line weight as packed: a thousandth of a millimetre past one, 0 for none.
fn weight(w: Option<f64>) -> u64 {
    w.filter(|w| w.is_finite() && *w >= 0.0).map_or(0, |w| (w * 1000.0).round().min(1e9) as u64 + 1)
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

fn string(out: &mut Vec<u8>, s: &str) {
    varint(out, s.len() as u64);
    out.extend_from_slice(s.as_bytes());
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
        let n = self.count(1 << 20)?;
        Ok(String::from_utf8_lossy(self.bytes(n)?).chars().take(crate::model::MAX_TEXT * 4).collect())
    }

    fn f64(&mut self) -> Result<f64, String> {
        let b = self.bytes(8)?;
        let v = f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
        if v.is_finite() && v.abs() <= FAR { Ok(v) } else { Err(Self::bad()) }
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
    let n = r.count(1 << 16)?;
    let mut layers = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.string()?;
        let c = r.bytes(5)?;
        let (rgb, off, ink) = ([c[0], c[1], c[2]], c[3] != 0, c[4] != 0);
        let linetype = r.string()?;
        let color = if ink { Color::Index(7) } else { Color::True(rgb[0], rgb[1], rgb[2]) };
        layers.push(Layer { name, color, linetype, weight: None, off, frozen: false });
    }
    let n = r.count(1 << 16)?;
    let mut linetypes = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.string()?;
        let k = r.count(64)?;
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
        let layer = layers.get(r.count(1 << 16)?).map(|l| l.name.clone()).unwrap_or_else(|| "0".into());
        let c = r.bytes(4)?;
        let (color, alpha) = (Color::True(c[0], c[1], c[2]), c[3]);
        let (shape, weight, linetype) = match kind {
            POLYLINE => {
                let w = r.varint()?;
                let weight = (w > 0).then(|| (w - 1) as f64 / 1000.0);
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
                let k = r.count(1 << 20)?;
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
}

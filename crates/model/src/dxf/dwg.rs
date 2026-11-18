//! DWG drawings, read straight from the file: CADCraft's DWG reader (`cadcraft-dwg`, from our fork, over acadrust, MPL-2.0,
//! unmodified) reads it into CADCraft's drawing (`cadcraft_doc::Drawing`), which is made into the [`Dxf`] an import draws,
//! as a DXF file's is: the same shapes by the same rules (a polyline's bulges, an ellipse, a spline, a hatch's style, text
//! aligned, multi-line text by its lines), each block model space places made once and placed by reference. There is no DXF
//! text in between.
//!
//! The reader is a third party's and the file hostile: its size is checked before it is decoded, and the decoding runs
//! under `catch_unwind` (`cadcraft_dwg::read`). A native app that called `cadcraft_dwg::worker::use_own_worker` reads each
//! file first in a child process of its own, so a file that brings the decoder down costs only that child.

use cadcraft_doc::{Common, Drawing, Entity as Item, EntityKind as K, HAlign, Lineweight, Transparency, VAlign};

use super::{
    Color, Dxf, Inherit, Layer, Line, Linetype, MAX_BYTES, MAX_CHARS, MAX_DEPTH, MAX_WORK, Out, P2, PLACED, Shape, Style, Xf, arc,
    ellipse_shape, insunits, mtext_lines, polyline, spline_shape, styled,
};

/// Whether `bytes` are a DWG file's (`AC10` and a version).
pub fn is_dwg(bytes: &[u8]) -> bool {
    cadcraft_dwg::is_dwg(bytes)
}

/// The drawing a DWG file's `bytes` hold, with notes on what the reader left out of it; or why it can't be read. Where DWG
/// files are read elsewhere first ([`read_elsewhere`]), only a file read there safely is read here; any other is asked for
/// ([`take_wanted`]) and refused for now.
pub fn read(bytes: &[u8]) -> Result<(Dxf, Vec<String>), String> {
    match check(bytes) {
        Check::Read => read_here(bytes),
        Check::Refused(e) => Err(e),
        Check::Waiting => Err(WAITING.into()),
    }
}

/// [`read`] in this instance, whether or not files are read elsewhere first: what the place that reads them first calls.
pub fn read_here(bytes: &[u8]) -> Result<(Dxf, Vec<String>), String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("it's {} bytes; up to {MAX_BYTES}", bytes.len()));
    }
    let limits = cadcraft_dwg::Limits { max_dwg_bytes: MAX_BYTES, ..cadcraft_dwg::Limits::DEFAULT };
    let (d, notes) = cadcraft_dwg::read(bytes, limits)?;
    Ok((of_drawing(&d)?, notes))
}

// Reading elsewhere first. In the browser a panic aborts the whole WebAssembly instance (there is no unwinding), so the
// decoder's `catch_unwind` holds nothing there, and a hostile DWG would end the editor. A page that hosts the engine reads
// each DWG first in a Web Worker of its own (as the desktop reads it first in a child process, `cadcraft_dwg::worker`), and
// tells the engine how that went; the engine reads only a file the worker read safely. acadrust's decoding is
// deterministic, so a file read safely there is read safely here.

/// What [`read`] says of a file still being read elsewhere first.
pub const WAITING: &str = "the drawing is still being read; try again in a moment";
/// Files whose reading elsewhere is remembered, the latest kept.
const MOST_CHECKED: usize = 256;

static ELSEWHERE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Files read elsewhere, by [`key`], and how that went: `None`, safely; else why not.
static CHECKED: std::sync::Mutex<Vec<(u64, Option<String>)>> = std::sync::Mutex::new(Vec::new());
/// A file asked to be read elsewhere and not answered yet: its key, and its bytes until they are taken.
type Wanted = (u64, Option<std::sync::Arc<Vec<u8>>>);
/// The files asked to be read elsewhere and not answered yet.
static WANTED: std::sync::Mutex<Vec<Wanted>> = std::sync::Mutex::new(Vec::new());

/// Whether a DWG file may be read here, as [`check`] finds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// Read it: it was read safely elsewhere, or nothing is read elsewhere.
    Read,
    /// It could not be read elsewhere: why, for people ("This drawing could not be read: …").
    Refused(String),
    /// It is being read elsewhere.
    Waiting,
}

/// From now on, each DWG is read elsewhere first (`on`), or here alone. The browser's engine turns it on, once the page
/// can read DWG files in a worker of its own.
pub fn read_elsewhere(on: bool) {
    ELSEWHERE.store(on, std::sync::atomic::Ordering::SeqCst);
}

/// Whether each DWG is read elsewhere first.
pub fn reads_elsewhere() -> bool {
    ELSEWHERE.load(std::sync::atomic::Ordering::SeqCst)
}

/// What a file is known by: the hash of its bytes.
pub fn key(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// Whether DWG `bytes` may be read here. A file not read elsewhere yet is asked for ([`take_wanted`]) and waits.
pub fn check(bytes: &[u8]) -> Check {
    if !reads_elsewhere() {
        return Check::Read;
    }
    let k = key(bytes);
    if let Some(said) = CHECKED.lock().ok().and_then(|c| c.iter().rev().find(|(key, _)| *key == k).map(|(_, said)| said.clone())) {
        return said.map_or(Check::Read, Check::Refused);
    }
    if let Ok(mut wanted) = WANTED.lock()
        && !wanted.iter().any(|(key, _)| *key == k)
    {
        wanted.push((k, Some(std::sync::Arc::new(bytes.to_vec()))));
    }
    Check::Waiting
}

/// The files to read elsewhere, asked for since the last call: each one's key (for [`checked`]) and bytes.
pub fn take_wanted() -> Vec<(u64, std::sync::Arc<Vec<u8>>)> {
    let Ok(mut wanted) = WANTED.lock() else { return Vec::new() };
    wanted.iter_mut().filter_map(|(k, bytes)| Some((*k, bytes.take()?))).collect()
}

/// Whether any file is still being read elsewhere.
pub fn waiting() -> bool {
    WANTED.lock().is_ok_and(|w| !w.is_empty())
}

/// How file `key` was read elsewhere: safely (`Ok`), or why not, said to people as "This drawing could not be read: …".
pub fn checked(key: u64, read: Result<(), String>) {
    if let Ok(mut wanted) = WANTED.lock() {
        wanted.retain(|(k, _)| *k != key);
    }
    let said = read.err().map(|e| {
        let e = e.trim().trim_end_matches('.');
        format!("This drawing could not be read: {}", if e.is_empty() { "the reader stopped on it" } else { e })
    });
    if let Ok(mut c) = CHECKED.lock() {
        c.retain(|(k, _)| *k != key);
        c.push((key, said));
        let over = c.len().saturating_sub(MOST_CHECKED);
        c.drain(..over);
    }
}

/// Decodes DWG `bytes` with the third-party reader alone, as the desktop's worker does (`cadcraft_dwg::worker`): what a
/// place that reads a file apart first runs, since the decoder is where a hostile file can bring an instance down; the
/// import made of the drawing after it is this crate's own code. The notes on what it left out, or why it can't be read.
pub fn probe(bytes: &[u8]) -> Result<Vec<String>, String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!("it's {} bytes; up to {MAX_BYTES}", bytes.len()));
    }
    let limits = cadcraft_dwg::Limits { max_dwg_bytes: MAX_BYTES, ..cadcraft_dwg::Limits::DEFAULT };
    cadcraft_dwg::read(bytes, limits).map(|(_, notes)| notes)
}

/// The import CADCraft's drawing `d` makes: its model space, in its own unit.
pub fn of_drawing(d: &Drawing) -> Result<Dxf, String> {
    let walk = Walk { d };
    let mut out = Out::default();
    walk.place(&mut out, d.model.iter().map(|e| &**e), Xf::ID, None, 0)?;
    let (scale, unit) = insunits(d.header.i64("INSUNITS", 0));
    let ltscale = Some(d.header.f64("LTSCALE", 1.0)).filter(|x| *x > 0.0).unwrap_or(1.0);
    let corner = |name: &str| d.header.point(name).map(|p| P2(p.x, p.y)).filter(|p| p.0.is_finite() && p.1.is_finite());
    let extents = match (corner("EXTMIN"), corner("EXTMAX")) {
        (Some(lo), Some(hi)) if lo.0 <= hi.0 && lo.1 <= hi.1 => Some([lo, hi]),
        _ => None,
    };
    let mut layers: Vec<Layer> = d
        .layers
        .iter()
        .map(|l| Layer {
            name: l.name.chars().take(MAX_CHARS).collect(),
            color: color(l.color),
            linetype: if l.linetype.trim().is_empty() { "Continuous".into() } else { l.linetype.clone() },
            weight: weight(l.lineweight),
            off: !l.on,
            frozen: l.frozen,
        })
        .collect();
    out.missing_layers(&mut layers);
    let linetypes =
        d.linetypes.iter().map(|l| Linetype { name: l.name.clone(), dashes: l.pattern.iter().map(|p| p.length).collect() }).collect();
    Ok(Dxf {
        scale,
        unit: unit.into(),
        version: d.header.str("ACADVER", "").trim().to_string(),
        layers,
        linetypes,
        ltscale,
        entities: out.entities,
        blocks: out.blocks,
        inserts: out.inserts,
        extents,
        left_out: out.left_out,
    })
}

fn color(c: cadcraft_doc::color::Color) -> Color {
    use cadcraft_doc::color::Color as C;
    match c {
        C::ByLayer => Color::ByLayer,
        C::ByBlock => Color::ByBlock,
        C::Index(i) => Color::Index(i),
        C::True(rgb) => Color::True(rgb.0, rgb.1, rgb.2),
    }
}

fn weight(w: Lineweight) -> Option<f64> {
    match w {
        Lineweight::Mm100(v) => Some(f64::from(v) / 100.0),
        _ => None,
    }
}

/// An entity's layer, colour, linetype, weight and opacity, with what it takes from the block it's in, as [`Style::of`]
/// takes them from a file's groups.
fn style_of(c: &Common, inh: Option<&Inherit>) -> Style {
    let own = if c.layer.is_empty() { "0" } else { c.layer.as_str() };
    let layer = match (own, inh) {
        ("0", Some(i)) => i.layer,
        (l, _) => l,
    };
    let color = match (color(c.color), inh) {
        (Color::ByBlock, Some(i)) => i.color,
        (c, _) => c,
    };
    let linetype = match c.linetype.trim() {
        "" => None,
        lt if lt.eq_ignore_ascii_case("BYLAYER") => None,
        lt if lt.eq_ignore_ascii_case("BYBLOCK") => inh.and_then(|i| i.linetype).map(String::from),
        lt => Some(lt.to_string()),
    };
    let alpha = match c.transparency {
        Transparency::Percent(p) => {
            let opaque = 100u32.saturating_sub(u32::from(p.min(100)));
            u8::try_from((opaque * 255 + 50) / 100).unwrap_or(255)
        }
        _ => 255,
    };
    Style { layer: layer.chars().take(MAX_CHARS).collect(), color, linetype, weight: weight(c.lineweight), alpha }
}

/// A walk through CADCraft's drawing, placing its entities as [`super::Reader::place`] places a DXF file's.
struct Walk<'d> {
    d: &'d Drawing,
}

impl<'d> Walk<'d> {
    /// `items` placed through `xf`, those of a block with what they inherit from its INSERT.
    fn place(
        &self,
        out: &mut Out,
        items: impl Iterator<Item = &'d Item>,
        xf: Xf,
        inh: Option<&Inherit>,
        depth: usize,
    ) -> Result<(), String> {
        for e in items {
            out.work += 1;
            if out.work > MAX_WORK {
                return Err(format!("its blocks place more than {MAX_WORK} entities; up to that many"));
            }
            if !e.common.visible {
                continue;
            }
            let style = style_of(&e.common, inh);
            let put = |out: &mut Out, shape: Option<Shape>, color: Color| match shape {
                Some(shape) => out.push(super::Entity {
                    layer: style.layer.clone(),
                    color,
                    linetype: style.linetype.clone(),
                    weight: style.weight,
                    alpha: style.alpha,
                    shape: super::transformed(shape, xf),
                }),
                None => {
                    out.left("empty");
                    Ok(())
                }
            };
            let p = |v: cadcraft_doc::geom::Vec3| P2(v.x, v.y);
            let q = |v: cadcraft_doc::geom::Vec2| P2(v.x, v.y);
            let shape = match &e.kind {
                K::Line(l) => Some(Shape::Line(p(l.a), p(l.b))),
                K::Circle(c) => (c.radius > 0.0).then(|| Shape::Circle { c: p(c.center), r: c.radius }),
                K::Arc(a) => (a.radius > 0.0).then(|| arc(p(a.center), a.radius, a.start.to_degrees(), a.end.to_degrees())),
                K::LwPolyline(l) => polyline(l.vertices.iter().map(|v| (q(v.p), v.bulge)).collect(), l.closed),
                K::Polyline3d(l) => polyline(l.points.iter().map(|v| (p(*v), 0.0)).collect(), l.closed),
                K::PolyfaceMesh(_) => {
                    out.left("POLYLINE mesh");
                    continue;
                }
                K::Point(pt) => Some(Shape::Point(p(pt.p))),
                K::Ellipse(el) => ellipse_shape(p(el.center), p(el.major), el.ratio, el.start, el.end),
                K::Spline(s) => {
                    let w = |i: usize| s.weights.get(i).copied().unwrap_or(1.0);
                    let controls: Vec<(P2, f64)> = s.control.iter().enumerate().map(|(i, c)| (q(*c), w(i))).collect();
                    spline_shape(s.degree as i64, s.closed, &s.knots, &controls, s.fit.iter().map(|f| q(*f)).collect())
                }
                K::Text(t) => self.text(t, None),
                K::MText(t) => {
                    let color = super::leading_color(&t.contents).unwrap_or(style.color);
                    let lines =
                        mtext_lines(p(t.insert), t.height, t.rotation.to_degrees(), i64::from(t.attach), Some(t.line_spacing), &t.contents);
                    for line in lines {
                        put(out, Some(line), color)?;
                    }
                    continue;
                }
                K::Hatch(h) => {
                    let rings: Vec<Vec<P2>> = h
                        .loops
                        .iter()
                        .filter_map(|l| match polyline(l.vertices.iter().map(|v| (q(v.p), v.bulge)).collect(), true) {
                            Some(Shape::Polyline { mut pts, .. }) => {
                                if pts.len() >= 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| a.dist(*b) <= 1e-9) {
                                    pts.pop();
                                }
                                (pts.len() >= 3).then_some(pts)
                            }
                            _ => None,
                        })
                        .collect();
                    let rings = styled(rings, i64::from(h.style));
                    let solid = h.solid || h.pattern.eq_ignore_ascii_case("SOLID");
                    (!rings.is_empty()).then_some(Shape::Fill { rings, solid, tile: None, shade: false })
                }
                K::Solid(s) | K::Trace(s) => {
                    let [a, b, c, d] = s.corners.map(p);
                    let mut ring = vec![a, b];
                    if d.dist(c) > 0.0 {
                        ring.push(d);
                    }
                    ring.push(c);
                    Some(Shape::Fill { rings: vec![ring], solid: true, tile: None, shade: false })
                }
                K::Insert(ins) => {
                    self.insert(out, ins, &style, xf, depth)?;
                    // Its attributes, each a text of its own, after it, as a file writes them.
                    for a in ins.attribs.iter().filter(|a| !a.invisible) {
                        let st = style_of(&a.props.over(&e.common), inh);
                        let tag = Some(a.tag.trim().to_string()).filter(|t| !t.is_empty());
                        if let Some(shape) = self.text(&a.text, tag) {
                            out.push(super::Entity {
                                layer: st.layer,
                                color: st.color,
                                linetype: st.linetype,
                                weight: st.weight,
                                alpha: st.alpha,
                                shape: super::transformed(shape, xf),
                            })?;
                        }
                    }
                    continue;
                }
                K::Dimension(dim) => {
                    if let Some(name) = &dim.block {
                        let moved = Xf { o: p(dim.insertion_point), ..Xf::ID };
                        self.block(out, name, &style, xf.then(moved), depth)?;
                    }
                    continue;
                }
                K::AttDef(_) => continue,
                other => {
                    out.left(other.dxf_name());
                    continue;
                }
            };
            put(out, shape, style.color)?;
        }
        Ok(())
    }

    /// A line of text, with its style's backward and upside-down flags, and its `tag` (an attribute's).
    fn text(&self, t: &cadcraft_doc::Text, tag: Option<String>) -> Option<Shape> {
        let h = match t.halign {
            HAlign::Left => 0,
            HAlign::Center => 1,
            HAlign::Right => 2,
            HAlign::Aligned => 3,
            HAlign::Middle => 4,
            HAlign::Fit => 5,
        };
        let v = match t.valign {
            VAlign::Baseline => 0,
            VAlign::Bottom => 1,
            VAlign::Middle => 2,
            VAlign::Top => 3,
        };
        let ts = self.d.text_style(&t.style);
        let flags = ts.map_or(0, |s| if s.backwards { 2 } else { 0 } | if s.upside_down { 4 } else { 0 });
        let line = Line {
            first: P2(t.insert.x, t.insert.y),
            second: t.align_pt.map(|a| P2(a.x, a.y)),
            height: t.height,
            align: (h, v),
            wide: Some(t.width_factor),
            flags,
            angle: t.rotation.to_degrees(),
        };
        line.shape(&t.value, tag)
    }

    /// The block an INSERT names, placed at its point, scaled, turned, and as many times as its array says: made once and
    /// placed by reference when model space places it, its entities placed into the block that holds it when nested.
    fn insert(&self, out: &mut Out, ins: &cadcraft_doc::Insert, style: &Style, xf: Xf, depth: usize) -> Result<(), String> {
        let (sx, sy) = (ins.scale.x, ins.scale.y);
        if sx == 0.0 || sy == 0.0 || !sx.is_finite() || !sy.is_finite() {
            out.left("empty");
            return Ok(());
        }
        let turn = ins.rotation;
        let (cols, rows) = (ins.cols.clamp(1, super::MAX_ARRAY as u32), ins.rows.clamp(1, super::MAX_ARRAY as u32));
        let (ex, ey) = (P2(turn.cos(), turn.sin()), P2(-turn.sin(), turn.cos()));
        let angle = turn.to_degrees().rem_euclid(360.0);
        let at = P2(ins.insert.x, ins.insert.y);
        let made = if depth == 0 { self.made(out, &ins.block)? } else { None };
        for row in 0..rows {
            for col in 0..cols {
                let o = at + ex * (ins.col_spacing * f64::from(col)) + ey * (ins.row_spacing * f64::from(row));
                let local = Xf { o, x: ex * sx, y: ey * sy };
                match made {
                    Some(b) => out.place(b, xf.then(local), style, xf.at(o), angle, [sx, sy])?,
                    None if depth == 0 => {}
                    None => self.block(out, &ins.block, style, xf.then(local), depth)?,
                }
            }
        }
        Ok(())
    }

    /// Block `name` made for placements ([`Out::block`]).
    fn made(&self, out: &mut Out, name: &str) -> Result<Option<usize>, String> {
        let Some(block) = self.d.block(name) else {
            out.left("INSERT of a block the file doesn't define");
            return Ok(None);
        };
        let unbased = Xf { o: P2(-block.base.x, -block.base.y), ..Xf::ID };
        out.block(name, |inner| self.place(inner, block.entities.iter().map(|e| &**e), unbased, Some(&PLACED), 1).map(|()| true))
    }

    /// The block named, its base on `xf`'s origin, its entities into what holds it.
    fn block(&self, out: &mut Out, name: &str, style: &Style, xf: Xf, depth: usize) -> Result<(), String> {
        let Some(block) = self.d.block(name) else {
            out.left("INSERT of a block the file doesn't define");
            return Ok(());
        };
        if depth >= MAX_DEPTH {
            out.left(&format!("a block nested deeper than {MAX_DEPTH}"));
            return Ok(());
        }
        let unbased = Xf { o: P2(-block.base.x, -block.base.y), ..Xf::ID };
        let inh = Inherit { layer: &style.layer, color: style.color, linetype: style.linetype.as_deref() };
        self.place(out, block.entities.iter().map(|e| &**e), xf.then(unbased), Some(&inh), depth + 1)
    }
}

/// A small plan made as a DWG file, for tests (the model's and the commands'): Autodesk's files are never committed.
#[doc(hidden)]
pub mod sample {
    use std::sync::Arc;

    use cadcraft_doc::color::Color as C;
    use cadcraft_doc::geom::Vec3;
    use cadcraft_doc::{
        Block, Circle, Common, Drawing, Entity, EntityKind as K, EntityStore, HVal, Handle, Insert, Layer, Line, Space, Text,
    };

    /// The plan, in mm: on WALLS (red), a room of four lines 4000 by 3000 from the origin and a text "Kitchen" in it; a block
    /// CHAIR (based on its middle: a circle of 200 on layer 0 by block, a line on FURN) placed three times by model space: on
    /// FURN at (1000, 1000), turned a quarter at (2000, 1000) on layer 0 in green, and twice its size at (3000, 2000).
    pub fn drawing() -> Drawing {
        let mut d = Drawing::new_metric();
        d.header.set("INSUNITS", HVal::Int(4));
        d.layers.push(Layer { name: "WALLS".into(), color: C::Index(1), ..Layer::default() });
        d.layers.push(Layer { name: "FURN".into(), color: C::Index(5), ..Layer::default() });
        let on = |layer: &str| Common { layer: layer.into(), ..Common::default() };
        let v = |x: f64, y: f64| Vec3::new(x, y, 0.0);
        let mut chair = EntityStore::new();
        let by_block = Common { color: C::ByBlock, ..on("0") };
        chair.push(Entity { handle: Handle(0x500), common: by_block, kind: K::Circle(Circle { center: v(50.0, 50.0), radius: 200.0 }) });
        chair.push(Entity { handle: Handle(0x501), common: on("FURN"), kind: K::Line(Line { a: v(50.0, 50.0), b: v(250.0, 50.0) }) });
        let block = Block {
            name: "CHAIR".into(),
            annotative: false,
            base: v(50.0, 50.0),
            entities: chair,
            description: String::new(),
            xref_overlay: false,
            xref_status: Default::default(),
            anonymous: false,
            xref_path: None,
            explodable: true,
            units: 4,
            array: None,
        };
        d.blocks.insert("CHAIR".into(), Arc::new(block));
        let model = Space::Model;
        let walls = [
            ((0.0, 0.0), (4000.0, 0.0)),
            ((4000.0, 0.0), (4000.0, 3000.0)),
            ((4000.0, 3000.0), (0.0, 3000.0)),
            ((0.0, 3000.0), (0.0, 0.0)),
        ];
        for (a, b) in walls {
            let _ = d.add(&model, on("WALLS"), K::Line(Line { a: v(a.0, a.1), b: v(b.0, b.1) }));
        }
        let text = Text {
            insert: v(1500.0, 1500.0),
            align_pt: None,
            height: 250.0,
            value: "Kitchen".into(),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: Default::default(),
            valign: Default::default(),
        };
        let _ = d.add(&model, on("WALLS"), K::Text(text));
        let place = |at: (f64, f64), turn: f64, k: f64| {
            K::Insert(Insert {
                block: "CHAIR".into(),
                insert: v(at.0, at.1),
                scale: Vec3::new(k, k, 1.0),
                rotation: turn,
                ..Insert::default()
            })
        };
        let _ = d.add(&model, on("FURN"), place((1000.0, 1000.0), 0.0, 1.0));
        let _ = d.add(&model, Common { color: C::Index(3), ..on("0") }, place((2000.0, 1000.0), std::f64::consts::FRAC_PI_2, 1.0));
        let _ = d.add(&model, on("FURN"), place((3000.0, 2000.0), 0.0, 2.0));
        d
    }

    /// [`drawing`] written as a DWG (AutoCAD 2018's), or why it couldn't be.
    pub fn plan() -> Result<Vec<u8>, String> {
        cadcraft_dwg::write(&drawing(), "AC1032").map(|(bytes, _)| bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: P2, b: P2) -> bool {
        a.dist(b) < 1e-6
    }

    #[test]
    fn a_dwg_is_read_straight_with_its_block_held_once_and_placed_three_times() {
        let bytes = sample::plan().unwrap();
        assert!(is_dwg(&bytes));
        let (d, notes) = read(&bytes).unwrap();
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!((d.unit.as_str(), d.scale), ("mm", Some(1.0)));
        assert_eq!((d.entities.len(), d.blocks.len(), d.inserts.len(), d.count()), (5, 1, 3, 11));
        assert_eq!(d.blocks[0].entities.len(), 2, "the block is held once");
        assert!(d.layer("WALLS").is_some_and(|l| l.color == Color::Index(1)));
        // Model space's own, in the file's order, the placements' after them.
        assert!(matches!(&d.entity(4).unwrap().shape, Shape::Text { text, .. } if text == "Kitchen"));
        let circle = |n: usize| match d.entity(n).unwrap().into_owned() {
            super::super::Entity { shape: Shape::Circle { c, r }, layer, color, .. } => (c, r, layer, color),
            e => panic!("{n}: {e:?}"),
        };
        // On FURN, its circle on layer 0 takes FURN and by block the placement's colour, by layer.
        let (c, r, layer, color) = circle(5);
        assert!(near(c, P2(1000.0, 1000.0)) && r == 200.0 && layer == "FURN" && color == Color::ByLayer, "{c:?} {layer} {color:?}");
        // Turned a quarter, on layer 0 in green: its line runs north.
        let (c, _, layer, color) = circle(7);
        assert!(near(c, P2(2000.0, 1000.0)) && layer == "0" && color == Color::Index(3), "{c:?} {layer} {color:?}");
        let Shape::Line(a, b) = d.entity(8).unwrap().shape.clone() else { panic!() };
        assert!(near(a, P2(2000.0, 1000.0)) && near(b, P2(2000.0, 1200.0)), "{a:?} {b:?}");
        assert_eq!(d.layer_of(8), Some("FURN"));
        // Twice its size.
        let (c, r, ..) = circle(9);
        assert!(near(c, P2(3000.0, 2000.0)) && r == 400.0);
        assert!(d.entity(11).is_none());
        // Read from the drawing itself it is the same.
        assert_eq!(of_drawing(&sample::drawing()).unwrap().count(), 11);
        // Written where another program's tests want it (Arqiflow's server/tests/fixtures/plan.dwg).
        if let Ok(path) = std::env::var("BIMCRAFT_SAMPLE_DWG") {
            std::fs::write(path, &bytes).unwrap();
        }
    }

    #[test]
    fn what_isnt_a_dwg_says_why() {
        let e = read(b"AC1032 not really a drawing").unwrap_err();
        assert!(!e.is_empty());
        assert!(!is_dwg(b"0\nSECTION\n"));
    }
}

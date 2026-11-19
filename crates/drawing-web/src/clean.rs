//! The drawing reader's instruments (Arqiflow's docs/superpowers/specs/2026-11-19-drawing-reader-design.md, section 1):
//! what a cheaper agent uses to work out what a client's drawing is and to hand over a clean version of each plan.
//! Three questions of the drawing already open, none of which reads the file again:
//!
//! - [`counts`]: how many items a rule selects, by kind, by layer and by block;
//! - [`extract`]: a part of the drawing cropped, filtered, re-sorted onto other layers and moved, written as a DWG;
//! - [`look`]: a picture of only what is selected, fitted to it, each layer in a colour of its own.
//!
//! A **rule** selects items of model space, and is the same in all three. Every part given must hold, and inside a list
//! any may: `{layers, kinds, blocks, box, min_size, max_size, handles, colors, linetypes}`. `box` holds when the centre
//! of the item's own box is inside; `min_size` and `max_size` measure the longer side of that box, mm; `blocks` names
//! block placements, a name ending in `*` matching what begins so. An item that draws nothing, or is endless (a
//! construction line), has no box: no `box`, `min_size` or `max_size` selects it. `colors` and `linetypes` are of the
//! item as it is drawn: its own, or its layer's when it is ByLayer (a colour as an index 1 to 255, `"#rrggbb"`, or one
//! of red, yellow, green, cyan, blue, magenta, white); `"bylayer"` in either selects the items that leave it to their
//! layer.
//!
//! Coordinates in and out are the ones `query` answers and takes: the drawing's millimetres, less its offset when it
//! lies far out. A box given far out is taken as the drawing's own, as `query` and `outlines` take it.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use cadcraft_doc::color::{Color, Rgb};
use cadcraft_doc::geom::{Bounds2, Mat3, Vec2, Vec3};
use cadcraft_doc::{BlockBoundsCache, Drawing as Doc, Entity, EntityKind, EntityStore, HVal, Layer, Layout, Space, entity_bounds_cached};
use cadcraft_index::{FAR_MM, KINDS, Opened, kind_of};
use cadcraft_io::SaveOptions;
use cadcraft_render::raster::{RasterOptions, View, render_png};
use serde_json::{Value, json};

/// The blocks `counts` names, the most placed first.
pub const BLOCKS_LISTED: usize = 200;
/// The colours `counts` names, the most used first.
pub const COLORS_LISTED: usize = 40;
/// The longest side a picture may be asked for, and the shortest (as the index's own pictures).
const MOST_LONG: u32 = 8192;
const LEAST_LONG: u32 = 200;
/// The margin round what a picture shows, a share of its larger side.
const MARGIN: f64 = 0.02;

/// What the instruments measure of an open drawing once, and keep: the index keeps the same to itself.
pub(crate) struct Measured {
    /// Millimetres to one of the drawing's units.
    mm: f64,
    /// The box of each model space item in the store's order, drawing units (empty: it draws nothing).
    boxes: Vec<Bounds2>,
}

impl Measured {
    pub(crate) fn of(o: &Opened) -> Measured {
        let mm = o.summary().get("mm").and_then(Value::as_f64).filter(|m| m.is_finite() && *m > 0.0).unwrap_or(1.0);
        let mut cache = BlockBoundsCache::default();
        let boxes = o.drawing.model.iter().map(|e| entity_bounds_cached(&o.drawing, e, 0, &mut cache)).collect();
        Measured { mm, boxes }
    }
}

/// The open drawing with its measures, as the instruments see it.
struct Seen<'a> {
    d: &'a Doc,
    m: &'a Measured,
    /// Taken off every point given out, mm.
    offset: Vec2,
    /// Its layers by name, lower case.
    layers: HashMap<String, &'a Layer>,
}

/// A colour as it is drawn, for telling two apart: white (7) for what names none.
fn rgb(c: Color) -> Rgb {
    c.resolve(Color::Index(7), Color::Index(7))
}

/// A layer's own colour (white for one that names none, as a damaged table may).
fn layer_own(l: Option<&Layer>) -> Color {
    match l.map(|l| l.color) {
        Some(c @ (Color::Index(_) | Color::True(_))) => c,
        _ => Color::Index(7),
    }
}

fn hundredth(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// A box that has a size: not empty, and not endless.
fn real(b: &Bounds2) -> bool {
    !b.is_empty() && b.min.is_finite() && b.max.is_finite()
}

impl<'a> Seen<'a> {
    fn of(o: &'a Opened, m: &'a Measured) -> Seen<'a> {
        let off = o.offset();
        Seen { d: &o.drawing, m, offset: Vec2::new(off[0], off[1]), layers: o.drawing.layers.iter().map(|l| (l.name.to_lowercase(), l)).collect() }
    }

    fn layer(&self, e: &Entity) -> Option<&'a Layer> {
        self.layers.get(&e.common.layer.to_lowercase()).copied()
    }

    /// The colour item `e` is drawn in: its own, its layer's when it is ByLayer, and white when it is ByBlock in no
    /// block (as AutoCAD draws it).
    fn color(&self, e: &Entity) -> Color {
        match e.common.color {
            Color::ByLayer => layer_own(self.layer(e)),
            Color::ByBlock => Color::Index(7),
            c => c,
        }
    }

    /// The linetype item `e` is drawn in, lower case: its own, its layer's when it is ByLayer, continuous when it is
    /// ByBlock in no block.
    fn linetype(&self, e: &Entity) -> String {
        let own = e.common.linetype.trim().to_lowercase();
        match own.as_str() {
            "bylayer" | "" => self.layer(e).map_or_else(|| "continuous".into(), |l| l.linetype.trim().to_lowercase()),
            "byblock" => "continuous".into(),
            _ => own,
        }
    }

    fn items(&self) -> impl Iterator<Item = (&'a Entity, &'a Bounds2)> {
        self.d.model.iter().map(|e| &**e).zip(self.m.boxes.iter())
    }

    /// A point of the drawing given out: mm, less the offset, to the hundredth.
    fn out(&self, p: Vec2) -> [f64; 2] {
        [hundredth(p.x * self.m.mm - self.offset.x), hundredth(p.y * self.m.mm - self.offset.y)]
    }

    fn box_out(&self, b: &Bounds2) -> Value {
        if real(b) { json!([self.out(b.min), self.out(b.max)]) } else { Value::Null }
    }

    /// A box asked for, mm, in the drawing's units: less the offset, or a point far out taken as the drawing's own.
    fn box_in(&self, b: [[f64; 2]; 2]) -> Bounds2 {
        let one = |p: [f64; 2]| {
            let far = p[0].abs().max(p[1].abs()) > FAR_MM;
            let p = if far || self.offset == Vec2::ZERO { Vec2::new(p[0], p[1]) } else { Vec2::new(p[0], p[1]) + self.offset };
            p * (1.0 / self.m.mm)
        };
        Bounds2::new(one(b[0]), one(b[1]))
    }
}

/// A block's name asked for: the name, or what names begin with (lower case).
enum Name {
    Is(String),
    Begins(String),
}

/// A rule, checked against the drawing.
#[derive(Default)]
struct Rule {
    layers: Option<BTreeSet<String>>,
    kinds: Option<BTreeSet<&'static str>>,
    blocks: Option<Vec<Name>>,
    area: Option<Bounds2>,
    /// The longer side of the item's box, drawing units.
    min: Option<f64>,
    max: Option<f64>,
    handles: Option<BTreeSet<u64>>,
    colors: Option<Colors>,
    linetypes: Option<Linetypes>,
}

/// The colours a rule asks for: the drawn colours, and whether the items that leave theirs to their layer.
#[derive(Default)]
struct Colors {
    drawn: BTreeSet<u32>,
    by_layer: bool,
}

/// The linetypes a rule asks for (lower case), and whether the items that leave theirs to their layer.
#[derive(Default)]
struct Linetypes {
    drawn: BTreeSet<String>,
    by_layer: bool,
}

/// What a rule may hold.
const RULE: [&str; 9] = ["layers", "kinds", "blocks", "box", "min_size", "max_size", "handles", "colors", "linetypes"];

/// `v[key]`, when it is given (null is not).
fn given<'v>(v: &'v Value, key: &str) -> Option<&'v Value> {
    v.get(key).filter(|x| !x.is_null())
}

/// `v` as the parameters `what` takes: an object with no key but `allowed`'s, or the refusal naming the stray one.
fn checked(v: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(m) = v.as_object() else {
        return Err(format!("{what} is an object ({{{}}}), not {v}", allowed.join(", ")));
    };
    match m.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("unknown parameter `{k}`; {what} takes: {}", allowed.join(", "))),
        None => Ok(()),
    }
}

fn names(v: &Value, key: &str, of: &str) -> Result<Vec<String>, String> {
    serde_json::from_value(v.clone()).map_err(|_| format!("`{key}` is a list of {of}, not {v}"))
}

/// The kind a word names (any case, "blocks" or "texts" too), as `query` takes it.
fn kind_named(word: &str) -> Option<&'static str> {
    let w = word.trim().to_ascii_lowercase();
    let ones = [Some(w.as_str()), w.strip_suffix("es"), w.strip_suffix('s')];
    ones.into_iter().flatten().find_map(|one| KINDS.iter().find(|k| **k == one).copied())
}

fn area_of(s: &Seen, v: &Value) -> Result<Bounds2, String> {
    let b: [[f64; 2]; 2] = serde_json::from_value(v.clone()).map_err(|_| format!("`box` is two corners [[x0, y0], [x1, y1]] in mm, not {v}"))?;
    if !b.iter().flatten().all(|x| x.is_finite()) {
        return Err(format!("`box` is two corners [[x0, y0], [x1, y1]] in mm, not {v}"));
    }
    Ok(s.box_in(b))
}

impl Rule {
    /// The rule `v` holds, read as `what` (which may hold `more` beside a rule's parts).
    fn read(s: &Seen, v: &Value, what: &str, more: &[&str]) -> Result<Rule, String> {
        let allowed: Vec<&str> = more.iter().chain(RULE.iter()).copied().collect();
        checked(v, &allowed, what)?;
        let mut r = Rule::default();
        if let Some(x) = given(v, "layers") {
            let mut keep = BTreeSet::new();
            for n in names(x, "layers", "layer names")? {
                match s.d.layer(n.trim()) {
                    Some(l) => {
                        keep.insert(l.name.to_lowercase());
                    }
                    None => {
                        let all: Vec<&str> = s.d.layers.iter().map(|l| l.name.as_str()).take(60).collect();
                        let rest = if s.d.layers.len() > 60 { ", …" } else { "" };
                        return Err(format!("the drawing has no layer `{n}` (it has: {}{rest})", all.join(", ")));
                    }
                }
            }
            r.layers = Some(keep);
        }
        if let Some(x) = given(v, "kinds") {
            let ks = names(x, "kinds", "kinds")?;
            r.kinds = Some(
                ks.iter()
                    .map(|k| kind_named(k).ok_or_else(|| format!("`{k}` is not a kind; the kinds are {}", KINDS.join(", "))))
                    .collect::<Result<_, _>>()?,
            );
        }
        if let Some(x) = given(v, "blocks") {
            let ns = names(x, "blocks", "block names (one ending in * for all that begin so)")?;
            r.blocks = Some(
                ns.iter()
                    .map(|n| {
                        let n = n.trim().to_lowercase();
                        match n.strip_suffix('*') {
                            Some(start) => Name::Begins(start.to_string()),
                            None => Name::Is(n),
                        }
                    })
                    .collect(),
            );
        }
        if let Some(x) = given(v, "box") {
            r.area = Some(area_of(s, x)?);
        }
        let size = |key: &str| -> Result<Option<f64>, String> {
            match given(v, key) {
                None => Ok(None),
                Some(x) => x
                    .as_f64()
                    .filter(|a| a.is_finite() && *a >= 0.0)
                    .map(|a| Some(a / s.m.mm))
                    .ok_or_else(|| format!("`{key}` is the longer side of an item's box in mm, 0 or more, not {x}")),
            }
        };
        r.min = size("min_size")?;
        r.max = size("max_size")?;
        if let Some(x) = given(v, "handles") {
            let hs = names(x, "handles", "handles (the `h` of an item)")?;
            r.handles = Some(
                hs.iter()
                    .map(|h| cadcraft_doc::Handle::parse_hex(h).map(|h| h.0).ok_or_else(|| format!("`{h}` is not a handle (the `h` of an item, as 1A2F)")))
                    .collect::<Result<_, _>>()?,
            );
        }
        if let Some(x) = given(v, "colors") {
            let not = || format!("`colors` is a list of colours (an index 1 to 255, \"#rrggbb\", red, yellow, green, cyan, blue, magenta, white, or \"bylayer\"), not {x}");
            let mut c = Colors::default();
            for one in x.as_array().ok_or_else(not)? {
                let word = match one {
                    Value::String(w) => w.clone(),
                    Value::Number(n) => n.to_string(),
                    _ => return Err(not()),
                };
                match Color::parse(&word) {
                    Some(Color::ByLayer) => c.by_layer = true,
                    Some(k @ (Color::Index(_) | Color::True(_))) => {
                        c.drawn.insert(rgb(k).to_u32());
                    }
                    _ => return Err(format!("`{word}` is not a colour: an index 1 to 255, \"#rrggbb\", red, yellow, green, cyan, blue, magenta, white, or \"bylayer\"")),
                }
            }
            r.colors = Some(c);
        }
        if let Some(x) = given(v, "linetypes") {
            let mut l = Linetypes::default();
            for n in names(x, "linetypes", "linetype names (or \"bylayer\")")? {
                let low = n.trim().to_lowercase();
                if low == "bylayer" {
                    l.by_layer = true;
                } else if s.d.linetype(&low).is_some() {
                    l.drawn.insert(low);
                } else {
                    let all: Vec<&str> = s.d.linetypes.iter().map(|l| l.name.as_str()).take(60).collect();
                    return Err(format!("the drawing has no linetype `{n}` (it has: {})", all.join(", ")));
                }
            }
            r.linetypes = Some(l);
        }
        Ok(r)
    }

    /// Whether the rule selects item `e`, whose box is `b`.
    fn holds(&self, s: &Seen, e: &Entity, b: &Bounds2) -> bool {
        if let Some(c) = &self.colors
            && !((c.by_layer && e.common.color == Color::ByLayer) || c.drawn.contains(&rgb(s.color(e)).to_u32()))
        {
            return false;
        }
        if let Some(l) = &self.linetypes
            && !((l.by_layer && matches!(e.common.linetype.trim().to_lowercase().as_str(), "bylayer" | "")) || l.drawn.contains(&s.linetype(e)))
        {
            return false;
        }
        if self.layers.as_ref().is_some_and(|l| !l.contains(&e.common.layer.to_lowercase())) {
            return false;
        }
        if self.kinds.as_ref().is_some_and(|k| !k.contains(kind_of(&e.kind))) {
            return false;
        }
        if let Some(ns) = &self.blocks {
            let EntityKind::Insert(i) = &e.kind else { return false };
            let name = i.block.to_lowercase();
            let named = |n: &Name| match n {
                Name::Is(x) => *x == name,
                Name::Begins(x) => name.starts_with(x.as_str()),
            };
            if !ns.iter().any(named) {
                return false;
            }
        }
        if self.handles.as_ref().is_some_and(|h| !h.contains(&e.handle.0)) {
            return false;
        }
        if self.area.is_some() || self.min.is_some() || self.max.is_some() {
            if !real(b) {
                return false;
            }
            let long = b.width().max(b.height());
            if self.area.as_ref().is_some_and(|a| !a.contains(b.center())) || self.min.is_some_and(|m| long < m) || self.max.is_some_and(|m| long > m) {
                return false;
            }
        }
        true
    }
}

/// The rules `p[key]` lists (one rule alone is taken as a list of one). None when it is not given, or is an empty list.
fn rules(s: &Seen, p: &Value, key: &str) -> Result<Option<Vec<Rule>>, String> {
    let Some(v) = given(p, key) else { return Ok(None) };
    let list: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        one => vec![one],
    };
    let read: Vec<Rule> = list.into_iter().map(|r| Rule::read(s, r, &format!("a rule of `{key}`"), &[])).collect::<Result<_, _>>()?;
    Ok((!read.is_empty()).then_some(read))
}

/// The model space items `{box?, keep?, drop?}` selects, by their place in the store: those whose centre is in `box`,
/// that one of `keep`'s rules selects (when it is given), and none of `drop`'s does.
fn selected(s: &Seen, p: &Value) -> Result<Vec<usize>, String> {
    let area = given(p, "box").map(|v| area_of(s, v)).transpose()?;
    let keep = rules(s, p, "keep")?;
    let drop = rules(s, p, "drop")?;
    let any = |rs: &[Rule], e: &Entity, b: &Bounds2| rs.iter().any(|r| r.holds(s, e, b));
    Ok(s.items()
        .enumerate()
        .filter(|(_, (e, b))| {
            area.as_ref().is_none_or(|a| real(b) && a.contains(b.center()))
                && keep.as_ref().is_none_or(|k| any(k, e, b))
                && !drop.as_ref().is_some_and(|d| any(d, e, b))
        })
        .map(|(n, _)| n)
        .collect())
}

fn parsed(params: &str) -> Result<Value, String> {
    if params.trim().is_empty() { Ok(json!({})) } else { serde_json::from_str(params).map_err(|e| format!("the parameters are not JSON ({e})")) }
}

/// `counts`: what a rule (`params`, JSON; empty: everything) selects of model space, counted:
/// `{total, kinds: {kind: n}, layers: [{name, items, kinds, color, colors?}], colors: [{color, aci, items, layers}],
/// blocks: [{name, placed, size: [w, h], layers}], extents}`.
/// The layers with the most items first: `color` is the layer's own, and `colors` (`{"#rrggbb": n}`) the colours its
/// items are drawn in, when they are not all the layer's. `colors`: the colours the items are drawn in (their own, or
/// their layer's), the most used first, [`COLORS_LISTED`] at most (`colors_more`), each with its index when it is one
/// of AutoCAD's 255 (`aci`, else null) and the layers of its items. The blocks most placed first, [`BLOCKS_LISTED`] at
/// most (`blocks_more`: how many more there are); `size` is the box of what the definition draws at scale 1, mm, and
/// `layers` the layers its placements are on. `extents` is the box round what was counted (null when nothing has one).
pub(crate) fn counts(o: &Opened, m: &Measured, params: &str) -> Result<Value, String> {
    let s = Seen::of(o, m);
    let rule = Rule::read(&s, &parsed(params)?, "counts", &[])?;
    let mut total = 0_usize;
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_layer: HashMap<&str, (usize, BTreeMap<&str, usize>, BTreeMap<String, usize>)> = HashMap::new();
    let mut placed: HashMap<&str, (usize, BTreeSet<&str>)> = HashMap::new();
    let mut per_color: HashMap<u32, (usize, Option<u8>, BTreeSet<&str>)> = HashMap::new();
    let mut round = Bounds2::EMPTY;
    for (e, b) in s.items().filter(|(e, b)| rule.holds(&s, e, b)) {
        total += 1;
        let k = kind_of(&e.kind);
        *kinds.entry(k).or_default() += 1;
        let color = s.color(e);
        let (n, ks, cs) = per_layer.entry(e.common.layer.as_str()).or_default();
        *n += 1;
        *ks.entry(k).or_default() += 1;
        *cs.entry(rgb(color).hex()).or_default() += 1;
        let (n, aci, on) = per_color.entry(rgb(color).to_u32()).or_default();
        *n += 1;
        if let Color::Index(i) = color {
            aci.get_or_insert(i);
        }
        on.insert(e.common.layer.as_str());
        if let EntityKind::Insert(i) = &e.kind {
            let (n, on) = placed.entry(i.block.as_str()).or_default();
            *n += 1;
            on.insert(e.common.layer.as_str());
        }
        if real(b) {
            round = round.union(b);
        }
    }
    let mut layers: Vec<_> = per_layer.into_iter().collect();
    layers.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    let layers: Vec<Value> = layers
        .into_iter()
        .map(|(name, (n, ks, cs))| {
            let own = rgb(layer_own(s.layers.get(&name.to_lowercase()).copied())).hex();
            let mut row = json!({"name": name, "items": n, "kinds": ks, "color": own});
            if cs.keys().any(|c| *c != own) {
                row["colors"] = json!(cs);
            }
            row
        })
        .collect();
    let mut colors: Vec<_> = per_color.into_iter().collect();
    colors.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    let colors_more = colors.len().saturating_sub(COLORS_LISTED);
    let colors: Vec<Value> = colors
        .into_iter()
        .take(COLORS_LISTED)
        .map(|(c, (n, aci, on))| json!({"color": Rgb::from_u32(c).hex(), "aci": aci, "items": n, "layers": on}))
        .collect();
    let mut blocks: Vec<_> = placed.into_iter().collect();
    blocks.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(b.0)));
    let more = blocks.len().saturating_sub(BLOCKS_LISTED);
    let mut cache = BlockBoundsCache::default();
    let listed: Vec<Value> = blocks
        .into_iter()
        .take(BLOCKS_LISTED)
        .map(|(name, (n, on))| {
            // What the definition draws in its own frame, as the index measures a block.
            let own = s.d.block(name).map(|blk| blk.entities.iter().fold(Bounds2::EMPTY, |acc, e| acc.union(&entity_bounds_cached(s.d, e, 1, &mut cache))));
            let size = match own {
                Some(b) if real(&b) => [hundredth(b.width() * m.mm), hundredth(b.height() * m.mm)],
                _ => [0.0, 0.0],
            };
            json!({"name": name, "placed": n, "size": size, "layers": on})
        })
        .collect();
    let mut out = json!({"total": total, "kinds": kinds, "layers": layers, "colors": colors, "blocks": listed, "extents": s.box_out(&round)});
    if colors_more > 0 {
        out["colors_more"] = json!(colors_more);
    }
    if more > 0 {
        out["blocks_more"] = json!(more);
    }
    Ok(out)
}

/// What a drawing's model space uses of its tables, its blocks' contents followed: the blocks (by their own names),
/// the layers and the text styles (lower case), the dimension styles kept (lower case), and whether it holds an item
/// whose styles are not followed here (a dimension, a leader, a table), for which every style is kept.
struct Used {
    blocks: BTreeSet<String>,
    layers: BTreeSet<String>,
    text_styles: BTreeSet<String>,
    dim_styles: BTreeSet<String>,
    every_style: bool,
}

fn used(d: &Doc) -> Used {
    let mut u = Used { blocks: BTreeSet::new(), layers: BTreeSet::new(), text_styles: BTreeSet::new(), dim_styles: BTreeSet::new(), every_style: false };
    // Blocks named and not yet gone through.
    let mut waiting: Vec<Arc<cadcraft_doc::Block>> = Vec::new();
    fn name(d: &Doc, u: &mut Used, waiting: &mut Vec<Arc<cadcraft_doc::Block>>, block: &str) {
        if let Some(b) = d.block(block)
            && u.blocks.insert(b.name.clone())
        {
            waiting.push(b.clone());
        }
    }
    fn item(d: &Doc, u: &mut Used, waiting: &mut Vec<Arc<cadcraft_doc::Block>>, e: &Entity) {
        u.layers.insert(e.common.layer.to_lowercase());
        match &e.kind {
            EntityKind::Insert(i) => {
                name(d, u, waiting, &i.block);
                for a in &i.attribs {
                    u.text_styles.insert(a.text.style.to_lowercase());
                    if let Some(l) = &a.props.layer {
                        u.layers.insert(l.to_lowercase());
                    }
                }
            }
            EntityKind::Text(t) => {
                u.text_styles.insert(t.style.to_lowercase());
            }
            EntityKind::MText(t) => {
                u.text_styles.insert(t.style.to_lowercase());
            }
            EntityKind::AttDef(a) => {
                u.text_styles.insert(a.text.style.to_lowercase());
            }
            EntityKind::Dimension(dim) => {
                u.every_style = true;
                if let Some(b) = &dim.block {
                    name(d, u, waiting, b);
                }
            }
            EntityKind::MLeader(ml) => {
                u.every_style = true;
                if let Some(i) = &ml.block {
                    name(d, u, waiting, &i.block);
                }
            }
            EntityKind::Leader(_) | EntityKind::Table(_) | EntityKind::Tolerance(_) | EntityKind::Unknown(_) => u.every_style = true,
            _ => {}
        }
    }
    for e in d.model.iter() {
        item(d, &mut u, &mut waiting, e);
    }
    loop {
        while let Some(b) = waiting.pop() {
            for e in b.entities.iter() {
                item(d, &mut u, &mut waiting, e);
            }
        }
        // The dimension styles kept name arrowhead blocks and a text style: every one when a dimension may use it,
        // else Standard and the current one.
        let current = d.header.str("DIMSTYLE", "Standard").to_lowercase();
        for st in &d.dim_styles {
            let low = st.name.to_lowercase();
            if u.every_style || low == "standard" || low == current {
                u.dim_styles.insert(low);
                u.text_styles.insert(st.text_style.to_lowercase());
                for b in [&st.arrow_block, &st.arrow_block1, &st.arrow_block2, &st.leader_arrow_block] {
                    if !b.is_empty() {
                        name(d, &mut u, &mut waiting, b);
                    }
                }
            }
        }
        if waiting.is_empty() {
            break;
        }
    }
    u
}

/// What nothing in model space uses any more left out of `d`: layers (but `0`), block definitions, text styles and
/// dimension styles (but Standard and the current ones). Linetypes and the multileader, table and multiline styles
/// are few and small, and stay.
fn purge(d: &mut Doc) {
    let u = used(d);
    d.blocks.retain(|n, _| u.blocks.contains(n) || n.starts_with("*Model_Space") || n.starts_with("*Paper_Space"));
    d.layers.retain(|l| l.name == "0" || u.layers.contains(&l.name.to_lowercase()));
    d.dim_styles.retain(|st| u.dim_styles.contains(&st.name.to_lowercase()));
    if !u.every_style {
        let current = d.header.str("TEXTSTYLE", "Standard").to_lowercase();
        let named: BTreeSet<String> = d.mleader_styles.iter().map(|st| st.text_style.to_lowercase()).collect();
        d.text_styles.retain(|st| {
            let low = st.name.to_lowercase();
            low == "standard" || low == current || u.text_styles.contains(&low) || named.contains(&low)
        });
    }
}

/// A part of the open drawing as a drawing of its own, and what it holds: see [`extract`], which writes it.
pub(crate) fn cut(o: &Opened, m: &Measured, params: &str) -> Result<(Doc, Value), String> {
    let s = Seen::of(o, m);
    let p = parsed(params)?;
    checked(&p, &["box", "keep", "drop", "sort", "move"], "extract")?;
    let chosen = selected(&s, &p)?;
    if chosen.is_empty() {
        return Err("nothing is selected: no item of the drawing is in `box`, kept by `keep` and not dropped by `drop`".into());
    }
    // Where each rule of `sort` sends what it selects.
    let sort: Vec<(String, Rule)> = match given(&p, "sort") {
        None => Vec::new(),
        Some(v) => {
            let list: Vec<&Value> = match v {
                Value::Array(a) => a.iter().collect(),
                one => vec![one],
            };
            list.into_iter()
                .map(|r| {
                    let rule = Rule::read(&s, r, "a rule of `sort`", &["to"])?;
                    let to = r.get("to").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty());
                    let to = to.ok_or_else(|| format!("a rule of `sort` names the layer its items go to (`to`): {r}"))?;
                    // The drawing's own spelling of a layer it has.
                    Ok((s.d.layer(to).map_or_else(|| to.to_string(), |l| l.name.clone()), rule))
                })
                .collect::<Result<_, String>>()?
        }
    };
    // `move`, mm, added to every coordinate `query` answers: so a drawing far out, whose coordinates are given less
    // its offset, is brought to them (its offset taken off too), and the point chosen lands where it was asked to.
    let shift: Option<Vec2> = match given(&p, "move") {
        None => None,
        Some(v) => {
            let [dx, dy]: [f64; 2] = serde_json::from_value(v.clone()).map_err(|_| format!("`move` is [dx, dy] in mm, not {v}"))?;
            if !(dx.is_finite() && dy.is_finite()) {
                return Err(format!("`move` is [dx, dy] in mm, not {v}"));
            }
            let far = dx.abs().max(dy.abs()) > FAR_MM;
            let mm = if far { Vec2::new(dx, dy) } else { Vec2::new(dx, dy) - s.offset };
            Some(mm * (1.0 / m.mm))
        }
    };
    let by = shift.map(Mat3::translate);

    let mut model = EntityStore::new();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_layer: BTreeMap<String, usize> = BTreeMap::new();
    let mut round = Bounds2::EMPTY;
    let mut made: Vec<String> = Vec::new();
    for (_, (src, b)) in s.items().enumerate().filter(|(n, _)| chosen.binary_search(n).is_ok()) {
        let mut e = src.clone();
        // The first rule that selects the item as it was (its own layer, not one an earlier rule gave it).
        if let Some((to, _)) = sort.iter().find(|(_, r)| r.holds(&s, src, b)) {
            // The item looks as it did: a colour or a linetype it left to its layer is written on it when its new
            // layer's is another (a layer made here is white and continuous).
            let new = s.layers.get(&to.to_lowercase()).copied();
            if e.common.color == Color::ByLayer && rgb(s.color(src)) != rgb(layer_own(new)) {
                e.common.color = s.color(src);
            }
            if matches!(e.common.linetype.trim().to_lowercase().as_str(), "bylayer" | "") {
                let was = s.layer(src).map_or("Continuous", |l| l.linetype.as_str());
                let will = new.map_or("Continuous", |l| l.linetype.as_str());
                if !was.trim().eq_ignore_ascii_case(will.trim()) {
                    e.common.linetype = was.to_string();
                }
            }
            e.common.layer.clone_from(to);
            // A placement stays a block, and only its own layer changes: what its definition draws on layer `0` is
            // drawn on the placement's layer, in any DWG, so it follows; what the definition draws on a layer of its
            // own stays on it. Its attributes on layers of their own are put with it.
            if let EntityKind::Insert(i) = &mut e.kind {
                for a in i.attribs.iter_mut().filter(|a| a.props.layer.is_some()) {
                    a.props.layer = Some(to.clone());
                }
            }
            if !made.contains(to) {
                made.push(to.clone());
            }
        }
        if let Some(by) = &by {
            e.kind.transform(by);
        }
        *kinds.entry(kind_of(&e.kind)).or_default() += 1;
        *per_layer.entry(e.common.layer.clone()).or_default() += 1;
        if real(b) {
            round = round.union(b);
        }
        model.push(e);
    }
    if let Some(v) = shift
        && real(&round)
    {
        round = Bounds2::new(round.min + v, round.max + v);
    }

    // The drawing's tables and blocks come with it (a clone shares them until one is changed), and what is not used
    // any more is taken out. Sheets are left out: one empty sheet, as a new drawing has.
    let mut d = s.d.clone();
    d.model = model;
    d.layouts = vec![Layout::new("Layout1", 1)];
    d.active_layout = None;
    // What the file kept of other programs' data belongs to the whole drawing, not to a part of it; and a DWG is
    // written into the document it was read from when that is still held.
    d.foreign = Arc::default();
    let kept: BTreeSet<cadcraft_doc::Handle> = d.model.handles().into_iter().collect();
    for g in &mut d.groups {
        g.members.retain(|h| kept.contains(h));
    }
    d.groups.retain(|g| !g.members.is_empty());
    d.constraints.clear();
    d.layer_states.clear();
    d.views.clear();
    d.hidden_objects.clear();
    d.refedit = None;
    purge(&mut d);
    for to in &made {
        d.ensure_layer(to);
    }
    d.header.set("CLAYER", HVal::Str("0".into()));
    if real(&round) {
        d.header.set("EXTMIN", HVal::Point(Vec3::new(round.min.x, round.min.y, 0.0)));
        d.header.set("EXTMAX", HVal::Point(Vec3::new(round.max.x, round.max.y, 0.0)));
    }

    let mut layers: Vec<(String, usize)> = per_layer.into_iter().collect();
    layers.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let layers: Vec<Value> = layers.into_iter().map(|(name, n)| json!({"name": name, "items": n})).collect();
    // The extents in the new drawing's own coordinates when it was moved (it is no longer far out); else as `query`
    // gives the open drawing's, less its offset, which is then said too.
    let extents = match (shift, real(&round)) {
        (_, false) => Value::Null,
        (Some(_), true) => {
            json!([[hundredth(round.min.x * m.mm), hundredth(round.min.y * m.mm)], [hundredth(round.max.x * m.mm), hundredth(round.max.y * m.mm)]])
        }
        (None, true) => s.box_out(&round),
    };
    let mut about = json!({"items": d.model.len(), "kinds": kinds, "layers": layers, "extents": extents});
    if shift.is_none() && s.offset != Vec2::ZERO {
        about["offset"] = json!([s.offset.x, s.offset.y]);
    }
    Ok((d, about))
}

/// `extract`: a part of the open drawing written as a DWG of `version` ("2018" ... "2000", or its code; empty: 2018),
/// and what it holds. `params` (JSON): `{box?, keep?: [rule], drop?: [rule], sort?: [{to, ...rule}], move?: [dx, dy]}`.
/// In order: `box` crops (the items whose centre is inside); `keep`, when given, keeps only what one of its rules
/// selects; `drop` takes out what one of its rules selects; `sort` puts each item left on the layer `to` of the first
/// rule that selects it (the layer is made when the drawing lacks it; an item no rule selects keeps its layer); `move`
/// is added to every coordinate. A block placement is kept whole, as a block, with its definition and those nested in
/// it. Layers, blocks, text styles and dimension styles nothing uses any more are left out, and so are the sheets.
/// An item re-sorted keeps the colour and the linetype it was drawn in. Every item keeps its handle. Answers the bytes and `{items, kinds, layers: [{name, items}], extents}` (`offset`
/// too, for a drawing far out that was not moved: its extents are less it, as `query`'s are).
pub(crate) fn extract(o: &Opened, m: &Measured, params: &str, version: &str) -> Result<(Vec<u8>, Value), String> {
    let v = crate::convert::version(version)?;
    let (d, about) = cut(o, m, params)?;
    let bytes = cadcraft_io::write_with(&d, "drawing.dwg", &SaveOptions { version: Some(v), binary: false }).map_err(|e| e.to_string())?;
    Ok((bytes, about))
}

/// A colour for the `n`th layer of a picture: hues a golden angle apart, in three depths, each dark enough to read
/// on white.
fn layer_color(n: usize) -> Rgb {
    let hue = (n as f64 * 137.508) % 360.0;
    let light = [0.36, 0.48, 0.27][(n / 12) % 3];
    let c = (1.0 - (2.0 * light - 1.0_f64).abs()) * 0.9;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let lift = light - c / 2.0;
    let to = |v: f64| ((v + lift) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(to(r), to(g), to(b))
}

/// A picture drawn: the PNG, and what it shows.
pub(crate) struct Looked {
    pub png: Vec<u8>,
    pub about: Value,
}

/// `look`: a picture of only what `params` (JSON `{box?, keep?, drop?, color_by?}`; the first three as [`extract`]
/// takes them) selects, fitted to it, `long` pixels on its longer side, on white.
///
/// `color_by` "own" (when not given): the drawing's own colours, layers that are off or frozen not drawn, as the
/// index's own pictures and the viewer show it: what a person sees. "layer": each layer in a colour of its own, every
/// selected item in its layer's colour whatever colour of its own it has, and a layer that is off or frozen drawn too;
/// inside a block placement, what is on layer `0` takes the placement's colour and what is on a layer of its own that
/// layer's (in the legend with `in_blocks`); only a colour set on an item inside a block stays.
///
/// `about`: `{width, height, extents (the box the picture's edges are at, mm), selected (the box round the items),
/// items, color_by, legend: [{layer, color: "#rrggbb", items}]}`, the layers with the most items first. By "own", a
/// layer's `color` is its own, with `colors` (`{"#rrggbb": n}`) when its items are drawn in others, and `off` or
/// `frozen` when it is and so is not in the picture.
pub(crate) fn look(o: &Opened, m: &Measured, params: &str, long: u32) -> Result<Looked, String> {
    let s = Seen::of(o, m);
    let p = parsed(params)?;
    checked(&p, &["box", "keep", "drop", "color_by"], "look")?;
    let by_layer = match given(&p, "color_by").map(|v| v.as_str().unwrap_or("")) {
        None | Some("own") => false,
        Some("layer") => true,
        Some(_) => return Err(format!("`color_by` is \"own\" (the drawing's own colours) or \"layer\" (each layer in a colour of its own), not {}", p["color_by"])),
    };
    let chosen = selected(&s, &p)?;
    let mut model = EntityStore::new();
    // By layer (lower case): how many items, and how many in each colour drawn.
    let mut per_layer: HashMap<String, (usize, BTreeMap<String, usize>)> = HashMap::new();
    let mut round = Bounds2::EMPTY;
    for (_, (src, b)) in s.items().enumerate().filter(|(n, _)| chosen.binary_search(n).is_ok()) {
        let mut e = src.clone();
        let (n, cs) = per_layer.entry(e.common.layer.to_lowercase()).or_default();
        *n += 1;
        *cs.entry(rgb(s.color(src)).hex()).or_default() += 1;
        if by_layer {
            e.common.color = Color::ByLayer;
            e.common.visible = true;
        }
        if real(b) {
            round = round.union(b);
        }
        model.push(e);
    }
    if !real(&round) {
        return Err("nothing to draw: no item with a box is in `box`, kept by `keep` and not dropped by `drop`".into());
    }
    let mut d = s.d.clone();
    d.model = model;
    // The layers drawn: those the items are on, the most used first; by layer, then those only blocks' contents are on.
    let mut order: Vec<(String, usize)> = per_layer.iter().map(|(l, (n, _))| (l.clone(), *n)).collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    if by_layer {
        d.hidden_objects.clear();
        let inside = used(&d).layers;
        order.extend(inside.iter().filter(|l| l.as_str() != "0" && !per_layer.contains_key(*l)).map(|l| (l.clone(), 0)));
    }
    let mut legend: Vec<Value> = Vec::new();
    for (n, (low, items)) in order.iter().enumerate() {
        // The layer's name as the drawing spells it (an item may name a layer the table lacks).
        let name = s.layers.get(low).map(|l| l.name.clone()).or_else(|| d.model.iter().find(|e| e.common.layer.to_lowercase() == *low).map(|e| e.common.layer.clone()));
        let name = name.unwrap_or_else(|| low.clone());
        if by_layer {
            let color = layer_color(n);
            match d.layers.iter_mut().find(|l| l.name.to_lowercase() == *low) {
                Some(l) => {
                    l.color = Color::True(color);
                    l.on = true;
                    l.frozen = false;
                    l.transparency = 0;
                }
                None => d.layers.push(Layer { color: Color::True(color), ..Layer::new(&name) }),
            }
            let mut row = json!({"layer": name, "color": color.hex(), "items": items});
            if *items == 0 {
                row["in_blocks"] = json!(true);
            }
            legend.push(row);
        } else {
            let layer = s.layers.get(low).copied();
            let own = rgb(layer_own(layer)).hex();
            let mut row = json!({"layer": name, "color": own, "items": items});
            if let Some((_, cs)) = per_layer.get(low)
                && cs.keys().any(|c| *c != own)
            {
                row["colors"] = json!(cs);
            }
            if layer.is_some_and(|l| !l.on) {
                row["off"] = json!(true);
            }
            if layer.is_some_and(|l| l.frozen) {
                row["frozen"] = json!(true);
            }
            legend.push(row);
        }
    }

    let long = long.clamp(LEAST_LONG, MOST_LONG);
    let (w, h) = (round.width().max(1e-9), round.height().max(1e-9));
    let k = f64::from(long) / (w.max(h) * (1.0 + 2.0 * MARGIN));
    // The margin is a share of the larger side on both, so a thin selection (one wall) still has a picture to be in.
    let pad = w.max(h) * MARGIN;
    let width = (((w + 2.0 * pad) * k).round() as u32).max(1);
    let height = (((h + 2.0 * pad) * k).round() as u32).max(1);
    let view = View { center: round.center(), scale: k, width, height };
    let px = 1.0 / k; // a pixel in drawing units
    let opts = cadcraft_render::Options {
        tolerance: px * 0.5,
        min_dash: px * 2.0,
        view_height: f64::from(height) * px,
        view: Some(view.world_bounds()),
        transparency: true,
        ..cadcraft_render::Options::default()
    };
    let list = cadcraft_render::build(&d, &Space::Model, &opts);
    let raster = RasterOptions { background: Rgb(255, 255, 255), hairline: 1.0, px_per_mm: 0.0, antialias: true };
    let png = render_png(&list, &view, &raster).ok_or("the picture could not be drawn")?;
    let about = json!({
        "width": width, "height": height, "extents": s.box_out(&view.world_bounds()), "selected": s.box_out(&round),
        "items": chosen.len(), "color_by": if by_layer { "layer" } else { "own" }, "legend": legend,
    });
    Ok(Looked { png, about })
}

#[cfg(test)]
mod tests;

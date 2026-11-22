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
//! All three take `open` first (`open: [rule]`, `depth?`): the block placements its rules select are taken apart as
//! AutoCAD's EXPLODE does, for that question only, and what they hold is counted, drawn, cropped and sorted as items of
//! the drawing (see [`open`]); the answer then says `opened: {placements, items}`. Much of an office's plan lies in a
//! few large blocks placed once (a floor's columns, its furniture, its sheet frame).
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
    /// The handles of the items that came out of placements taken apart (`open`), as a rule names them.
    paths: Option<&'a HashMap<u64, String>>,
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

/// A number to the hundredth (and never a zero with a minus sign).
fn hundredth(x: f64) -> f64 {
    (x * 100.0).round() / 100.0 + 0.0
}

/// A box that has a size: not empty, and not endless.
fn real(b: &Bounds2) -> bool {
    !b.is_empty() && b.min.is_finite() && b.max.is_finite()
}

impl<'a> Seen<'a> {
    fn of(o: &'a Opened, m: &'a Measured) -> Seen<'a> {
        let off = o.offset();
        Seen {
            d: &o.drawing,
            m,
            offset: Vec2::new(off[0], off[1]),
            layers: o.drawing.layers.iter().map(|l| (l.name.to_lowercase(), l)).collect(),
            paths: None,
        }
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
    /// Upper case: an item's own handle, or an opened item's (`1A2F.17`).
    handles: Option<BTreeSet<String>>,
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
    let b: [[f64; 2]; 2] =
        serde_json::from_value(v.clone()).map_err(|_| format!("`box` is two corners [[x0, y0], [x1, y1]] in mm, not {v}"))?;
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
            let one = |h: &String| {
                let h = h.trim().to_uppercase();
                let own = h.split('.').next().unwrap_or("");
                let ok =
                    cadcraft_doc::Handle::parse_hex(own).is_some() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '@');
                ok.then_some(h.clone()).ok_or_else(|| {
                    format!("`{h}` is not a handle (the `h` of an item, as 1A2F; of an item out of an opened placement, as 1A2F.17)")
                })
            };
            r.handles = Some(hs.iter().map(one).collect::<Result<_, _>>()?);
        }
        if let Some(x) = given(v, "colors") {
            let not = || {
                format!(
                    "`colors` is a list of colours (an index 1 to 255, \"#rrggbb\", red, yellow, green, cyan, blue, magenta, white, or \"bylayer\"), not {x}"
                )
            };
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
                    _ => {
                        return Err(format!(
                            "`{word}` is not a colour: an index 1 to 255, \"#rrggbb\", red, yellow, green, cyan, blue, magenta, white, or \"bylayer\""
                        ));
                    }
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
            && !((l.by_layer && matches!(e.common.linetype.trim().to_lowercase().as_str(), "bylayer" | ""))
                || l.drawn.contains(&s.linetype(e)))
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
        if let Some(hs) = &self.handles {
            let named = match s.paths.and_then(|p| p.get(&e.handle.0)) {
                Some(path) => hs.contains(path),
                None => hs.contains(&format!("{:X}", e.handle.0)),
            };
            if !named {
                return false;
            }
        }
        if self.area.is_some() || self.min.is_some() || self.max.is_some() {
            if !real(b) {
                return false;
            }
            let long = b.width().max(b.height());
            if self.area.as_ref().is_some_and(|a| !a.contains(b.center()))
                || self.min.is_some_and(|m| long < m)
                || self.max.is_some_and(|m| long > m)
            {
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
    selected_by(s, p, false)
}

/// [`selected`], with `meet`: an item is in `box` when its box meets it at all (a picture of the box shows every wall
/// that runs into it), not only when its centre is in it (what a cut takes).
fn selected_by(s: &Seen, p: &Value, meet: bool) -> Result<Vec<usize>, String> {
    let area = given(p, "box").map(|v| area_of(s, v)).transpose()?;
    let keep = rules(s, p, "keep")?;
    let drop = rules(s, p, "drop")?;
    let any = |rs: &[Rule], e: &Entity, b: &Bounds2| rs.iter().any(|r| r.holds(s, e, b));
    // What will be built of a clean plan lies over it on layers that are off (`X-BUILD-...`, src/standard.rs): it is
    // no part of the drawing unless a `keep` rule names those layers.
    let marks = given(p, "keep").is_some_and(|k| k.to_string().to_uppercase().contains("X-BUILD"));
    Ok(s.items()
        .enumerate()
        .filter(|(_, (e, b))| {
            (marks || !e.common.layer.to_uppercase().starts_with("X-BUILD"))
                && area.as_ref().is_none_or(|a| real(b) && if meet { a.intersects(b) } else { a.contains(b.center()) })
                && keep.as_ref().is_none_or(|k| any(k, e, b))
                && !drop.as_ref().is_some_and(|d| any(d, e, b))
        })
        .map(|(n, _)| n)
        .collect())
}

/// What `box` holds that `keep` and `drop` left out, so that whoever cleans a drawing sees what the rules cost:
/// `{items, layers: [{name, items}], blocks: [{name, placed, layers}]}`, the most first, the 25 layers and the 40
/// blocks with the most (with `more_layers` and `more_blocks` when there are others). Blocks are placements left out,
/// by name, with the layers they were placed on and what each `holds` (its own items by their layer); `layers` counts everything else by the layer it was on.
fn left_out(s: &Seen, p: &Value, chosen: &[usize]) -> Result<Value, String> {
    let area = given(p, "box").map(|v| area_of(s, v)).transpose()?;
    let mut layers: BTreeMap<String, usize> = BTreeMap::new();
    let mut blocks: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();
    let mut items = 0usize;
    for (n, (e, b)) in s.items().enumerate() {
        if chosen.binary_search(&n).is_ok() || !area.as_ref().is_none_or(|a| real(b) && a.contains(b.center())) {
            continue;
        }
        items += 1;
        match &e.kind {
            EntityKind::Insert(i) => {
                let (placed, on) = blocks.entry(i.block.clone()).or_default();
                *placed += 1;
                on.insert(e.common.layer.clone());
            }
            _ => *layers.entry(e.common.layer.clone()).or_default() += 1,
        }
    }
    let mut ls: Vec<(String, usize)> = layers.into_iter().collect();
    ls.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut bs: Vec<(String, (usize, BTreeSet<String>))> = blocks.into_iter().collect();
    bs.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    let mut out = json!({
        "items": items,
        "layers": ls.iter().take(25).map(|(name, n)| json!({"name": name, "items": n})).collect::<Vec<_>>(),
        "blocks": bs
            .iter()
            .take(40)
            .map(|(name, (n, on))| {
                // What the block holds, by the layer its own items are on (the six most): a block named "014" that
                // holds glazing lines says what it is.
                let mut holds: BTreeMap<&str, usize> = BTreeMap::new();
                if let Some(blk) = s.d.block(name) {
                    for e in blk.entities.iter() {
                        *holds.entry(e.common.layer.as_str()).or_default() += 1;
                    }
                }
                let mut holds: Vec<(&str, usize)> = holds.into_iter().collect();
                holds.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
                let holds: BTreeMap<&str, usize> = holds.into_iter().take(6).collect();
                json!({"name": name, "placed": n, "layers": on, "holds": holds})
            })
            .collect::<Vec<_>>(),
    });
    if ls.len() > 25 {
        out["more_layers"] = json!(ls.len() - 25);
    }
    if bs.len() > 40 {
        out["more_blocks"] = json!(bs.len() - 40);
    }
    Ok(out)
}

/// The most items `open` may make of the placements it takes apart: past it the call is refused.
pub const MOST_OPENED: usize = 400_000;
/// How deep `open` may go (`depth`): a block in a block in a block in a block.
const DEEPEST: u64 = 4;

/// The drawing with the placements `open` selects taken apart: a drawing of its own beside the open one, which is as
/// it was, with its measures, and what an opened item is called.
struct Apart {
    d: Doc,
    m: Measured,
    paths: HashMap<u64, String>,
    placements: usize,
    items: usize,
}

impl Apart {
    fn seen<'a>(&'a self, o: &Opened) -> Seen<'a> {
        let off = o.offset();
        Seen {
            d: &self.d,
            m: &self.m,
            offset: Vec2::new(off[0], off[1]),
            layers: self.d.layers.iter().map(|l| (l.name.to_lowercase(), l)).collect(),
            paths: Some(&self.paths),
        }
    }

    /// `opened` in an answer: how many placements were taken apart (those nested in them too), and how many items
    /// they became.
    fn said(&self) -> Value {
        json!({"placements": self.placements, "items": self.items})
    }
}

/// The copies of an array placement (MINSERT), each by how far it lies from the first, in the drawing's frame.
fn cells(i: &cadcraft_doc::Insert) -> impl Iterator<Item = Vec2> + '_ {
    let (cols, rows) = (i.cols.max(1), i.rows.max(1));
    (0..rows)
        .flat_map(move |r| (0..cols).map(move |c| Vec2::new(f64::from(c) * i.col_spacing, f64::from(r) * i.row_spacing).rotate(i.rotation)))
}

/// How many items a placement of block `name` becomes, opened `depth` deep (its attributes apart).
fn becomes(d: &Doc, name: &str, depth: u64, known: &mut HashMap<(String, u64), usize>) -> usize {
    let Some(blk) = d.block(name) else { return 1 };
    let key = (blk.name.clone(), depth);
    if let Some(n) = known.get(&key) {
        return *n;
    }
    let mut n = 0_usize;
    for e in blk.entities.iter() {
        n = n.saturating_add(match &e.kind {
            EntityKind::AttDef(a) => usize::from(a.constant),
            EntityKind::Insert(i) if depth > 1 && d.block(&i.block).is_some() => {
                let each = becomes(d, &i.block, depth - 1, known).saturating_add(i.attribs.iter().filter(|a| !a.invisible).count());
                each.saturating_mul(i.cols.max(1) as usize).saturating_mul(i.rows.max(1) as usize)
            }
            _ => 1,
        });
    }
    known.insert(key, n);
    n
}

/// What an item of a block's definition has when the placement `by` is taken apart, as EXPLODE leaves it: on layer
/// `0` it takes the placement's layer; a colour, linetype or lineweight ByBlock takes the placement's; ByLayer stays,
/// of the item's own layer. (When the placement leaves its colour or linetype to its layer and the item is on another,
/// the placement's layer's is written on the item, so that it is drawn as it was.)
fn inherited(s: &Seen, by: &cadcraft_doc::Common, own: &cadcraft_doc::Common) -> cadcraft_doc::Common {
    let mut c = own.clone();
    if c.layer == "0" {
        c.layer.clone_from(&by.layer);
    }
    let same_layer = c.layer.eq_ignore_ascii_case(&by.layer);
    let by_layer = s.layers.get(&by.layer.to_lowercase()).copied();
    if c.color == Color::ByBlock {
        c.color = match by.color {
            Color::ByLayer if !same_layer => layer_own(by_layer),
            other => other,
        };
    }
    if c.linetype.trim().eq_ignore_ascii_case("byblock") {
        let theirs = by.linetype.trim();
        c.linetype = if (theirs.is_empty() || theirs.eq_ignore_ascii_case("bylayer")) && !same_layer {
            by_layer.map_or_else(|| "Continuous".to_string(), |l| l.linetype.clone())
        } else {
            by.linetype.clone()
        };
    }
    if c.lineweight == cadcraft_doc::Lineweight::ByBlock {
        c.lineweight = by.lineweight;
    }
    c.visible = c.visible && by.visible;
    c
}

/// The placement `i` (with `common`, called `path`) taken apart into `out`, each item with what it is called: its
/// block's items placed by `place` (the placement's own scale, rotation and insertion, after those of what it was
/// nested in), `depth` deep; and its attributes, as texts. A block placed in it stays a placement at the last depth.
#[allow(clippy::too_many_arguments)]
fn take_apart(
    s: &Seen,
    common: &cadcraft_doc::Common,
    i: &cadcraft_doc::Insert,
    place: &Mat3,
    path: &str,
    depth: u64,
    out: &mut Vec<(cadcraft_doc::Common, EntityKind, String)>,
    placements: &mut usize,
) {
    let Some(blk) = s.d.block(&i.block) else { return };
    *placements += 1;
    let base = blk.base.xy();
    let len = blk.entities.len();
    for (cell, by) in cells(i).enumerate() {
        // The block's frame to the drawing's, for this copy.
        let m = place.then_before(Mat3::translate(by)).then_before(i.transform(base));
        for (n, e) in blk.entities.iter().enumerate() {
            let called = if cell == 0 { format!("{path}.{n}") } else { format!("{path}.{n}@{cell}") };
            let c = inherited(s, common, &e.common);
            match &e.kind {
                // An attribute's definition is not drawn in a placement (its attribute is), but a constant one is, as text.
                EntityKind::AttDef(a) => {
                    if a.constant {
                        let mut k = EntityKind::Text(a.text.clone());
                        k.transform(&m);
                        out.push((c, k, called));
                    }
                }
                EntityKind::Insert(nested) if depth > 1 && s.d.block(&nested.block).is_some() => {
                    take_apart(s, &c, nested, &m, &called, depth - 1, out, placements);
                }
                kind => {
                    let mut k = kind.clone();
                    k.transform(&m);
                    out.push((c, k, called));
                }
            }
        }
    }
    // Its attributes, as the texts they show: each with its definition's properties and those set on it.
    for (n, a) in i.attribs.iter().enumerate().filter(|(_, a)| !a.invisible) {
        let def = blk.entities.iter().find(|e| matches!(&e.kind, EntityKind::AttDef(d) if d.tag.eq_ignore_ascii_case(&a.tag)));
        let c = a.props.over(&def.map_or_else(|| common.clone(), |e| inherited(s, common, &e.common)));
        let mut k = match a.multiline_text() {
            Some(t) => EntityKind::MText(t),
            None => EntityKind::Text(a.text.clone()),
        };
        // An attribute is kept in the frame its placement is in: the drawing's, or that of the block it was nested in.
        k.transform(place);
        out.push((c, k, format!("{path}.{}", len + n)));
    }
}

/// `open`: the drawing with every block placement one of `p.open`'s rules selects taken apart, as AutoCAD's EXPLODE
/// does, `p.depth` deep (1 when not given: the blocks placed in an opened block stay placements; 2 opens those too,
/// whichever they are; [`DEEPEST`] at most). None when `open` is not given. The items that come out are items of the
/// drawing from then on, with handles of their own in it; a rule names one by its placement's handle and its place in
/// the block, from 0, as `query` lists the block: `1A2F.17` (`1A2F.17.3` for item 3 of the block that item 17 placed;
/// `1A2F.17@2` in the third copy of an array; an attribute's place follows the block's items). Refused when more than
/// [`MOST_OPENED`] items would come out, with the blocks that make the most.
fn open(s: &Seen, p: &Value) -> Result<Option<Apart>, String> {
    let Some(rules) = rules(s, p, "open")? else { return Ok(None) };
    let depth = match given(p, "depth") {
        None => 1,
        Some(v) => v
            .as_u64()
            .filter(|d| (1..=DEEPEST).contains(d))
            .ok_or_else(|| format!("`depth` is how deep `open` goes, 1 to {DEEPEST}, not {v}"))?,
    };
    let chosen: Vec<bool> = s
        .items()
        .map(|(e, b)| matches!(&e.kind, EntityKind::Insert(i) if s.d.block(&i.block).is_some()) && rules.iter().any(|r| r.holds(s, e, b)))
        .collect();
    // Counted before anything is made, so that a careless `open` (every seat of a hall) costs nothing.
    let mut known = HashMap::new();
    let mut by_block: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut total = 0_usize;
    for ((e, _), _) in s.items().zip(&chosen).filter(|(_, c)| **c) {
        let EntityKind::Insert(i) = &e.kind else { continue };
        let each = becomes(s.d, &i.block, depth, &mut known).saturating_add(i.attribs.iter().filter(|a| !a.invisible).count());
        let n = each.saturating_mul(i.cols.max(1) as usize).saturating_mul(i.rows.max(1) as usize);
        total = total.saturating_add(n);
        let (placed, items) = by_block.entry(i.block.as_str()).or_default();
        *placed += 1;
        *items = items.saturating_add(n);
    }
    if total > MOST_OPENED {
        let mut most: Vec<_> = by_block.into_iter().collect();
        most.sort_by(|a, b| b.1.1.cmp(&a.1.1).then_with(|| a.0.cmp(b.0)));
        let named: Vec<String> =
            most.iter().take(8).map(|(name, (placed, items))| format!("{name} ({placed} placed, {items} items)")).collect();
        return Err(format!(
            "`open` would take {} placements apart into {total} items, more than the {MOST_OPENED} it may: open fewer (by `blocks`, `box` or `min_size`). The blocks that make the most: {}",
            chosen.iter().filter(|c| **c).count(),
            named.join(", ")
        ));
    }
    let mut d = s.d.clone();
    let mut model = EntityStore::new();
    let mut boxes: Vec<Option<Bounds2>> = Vec::with_capacity(s.m.boxes.len() + total);
    let mut paths = HashMap::with_capacity(total);
    let (mut placements, mut items) = (0_usize, 0_usize);
    let mut out = Vec::new();
    for ((e, b), open) in s.items().zip(&chosen) {
        match &e.kind {
            EntityKind::Insert(i) if *open => {
                take_apart(s, &e.common, i, &Mat3::translate(Vec2::ZERO), &format!("{:X}", e.handle.0), depth, &mut out, &mut placements);
                for (common, kind, called) in out.drain(..) {
                    let handle = d.new_handle();
                    paths.insert(handle.0, called);
                    model.push(Entity { handle, common, kind });
                    boxes.push(None);
                    items += 1;
                }
            }
            _ => {
                model.push(e.clone());
                boxes.push(Some(*b));
            }
        }
    }
    d.model = model;
    // The boxes of what stayed are known; those of what came out are measured.
    let mut cache = BlockBoundsCache::default();
    let boxes = d.model.iter().zip(boxes).map(|(e, known)| known.unwrap_or_else(|| entity_bounds_cached(&d, e, 0, &mut cache))).collect();
    Ok(Some(Apart { m: Measured { mm: s.m.mm, boxes }, d, paths, placements, items }))
}

fn parsed(params: &str) -> Result<Value, String> {
    if params.trim().is_empty() {
        Ok(json!({}))
    } else {
        serde_json::from_str(params).map_err(|e| format!("the parameters are not JSON ({e})"))
    }
}

/// `counts`: what a rule (`params`, JSON; empty: everything) selects of model space, counted:
/// `{total, kinds: {kind: n}, layers: [{name, items, kinds, color, colors?, linetypes?}], colors: [{color, aci, items, layers}],
/// blocks: [{name, placed, size: [w, h], layers}], extents}`.
/// The layers with the most items first: `color` is the layer's own, and `colors` (`{"#rrggbb": n}`) the colours its
/// items are drawn in, when they are not all the layer's. `colors`: the colours the items are drawn in (their own, or
/// their layer's), the most used first (a layer's `linetypes`, `{name: n}` in lower case, the same of the linetypes
/// they are drawn in, when any is not continuous), [`COLORS_LISTED`] at most (`colors_more`), each with its index when it is one
/// of AutoCAD's 255 (`aci`, else null) and the layers of its items. The blocks most placed first, [`BLOCKS_LISTED`] at
/// most (`blocks_more`: how many more there are); `size` is the box of what the definition draws at scale 1, mm,
/// `holds` how many items the definition holds (a block placed in it is one), and
/// `layers` the layers its placements are on. `extents` is the box round what was counted (null when nothing has one).
pub(crate) fn counts(o: &Opened, m: &Measured, params: &str) -> Result<Value, String> {
    let p = parsed(params)?;
    let whole = Seen::of(o, m);
    let apart = open(&whole, &p)?;
    let s = apart.as_ref().map_or(whole, |a| a.seen(o));
    let rule = Rule::read(&s, &p, "counts", &["open", "depth"])?;
    let mut total = 0_usize;
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut per_layer: HashMap<&str, (usize, BTreeMap<&str, usize>, BTreeMap<String, usize>)> = HashMap::new();
    let mut per_linetype: HashMap<&str, BTreeMap<String, usize>> = HashMap::new();
    let mut placed: HashMap<&str, (usize, BTreeSet<&str>)> = HashMap::new();
    let mut per_color: HashMap<u32, (usize, Option<u8>, BTreeSet<&str>)> = HashMap::new();
    let mut round = Bounds2::EMPTY;
    // The build marks of a clean plan (`X-BUILD-...`) are counted only when the rule names their layers.
    let marks = given(&p, "layers").is_some_and(|l| l.to_string().to_uppercase().contains("X-BUILD"));
    for (e, b) in s.items().filter(|(e, b)| (marks || !e.common.layer.to_uppercase().starts_with("X-BUILD")) && rule.holds(&s, e, b)) {
        total += 1;
        let k = kind_of(&e.kind);
        *kinds.entry(k).or_default() += 1;
        let color = s.color(e);
        let (n, ks, cs) = per_layer.entry(e.common.layer.as_str()).or_default();
        *n += 1;
        *ks.entry(k).or_default() += 1;
        *cs.entry(rgb(color).hex()).or_default() += 1;
        *per_linetype.entry(e.common.layer.as_str()).or_default().entry(s.linetype(e)).or_default() += 1;
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
            // The linetypes its items are drawn in, when any is not solid: a dashed line is what is above or below the cut.
            if let Some(lt) = per_linetype.get(name)
                && lt.keys().any(|l| l != "continuous")
            {
                row["linetypes"] = json!(lt);
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
            let own =
                s.d.block(name)
                    .map(|blk| blk.entities.iter().fold(Bounds2::EMPTY, |acc, e| acc.union(&entity_bounds_cached(s.d, e, 1, &mut cache))));
            let size = match own {
                Some(b) if real(&b) => [hundredth(b.width() * m.mm), hundredth(b.height() * m.mm)],
                _ => [0.0, 0.0],
            };
            let holds = s.d.block(name).map_or(0, |blk| blk.entities.len());
            json!({"name": name, "placed": n, "size": size, "holds": holds, "layers": on})
        })
        .collect();
    let mut out =
        json!({"total": total, "kinds": kinds, "layers": layers, "colors": colors, "blocks": listed, "extents": s.box_out(&round)});
    if colors_more > 0 {
        out["colors_more"] = json!(colors_more);
    }
    if more > 0 {
        out["blocks_more"] = json!(more);
    }
    if let Some(a) = &apart {
        out["opened"] = a.said();
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
    let mut u = Used {
        blocks: BTreeSet::new(),
        layers: BTreeSet::new(),
        text_styles: BTreeSet::new(),
        dim_styles: BTreeSet::new(),
        every_style: false,
    };
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
                    if let Some(ml) = &a.multiline {
                        u.text_styles.insert(ml.mtext.style.to_lowercase());
                    }
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
                if let Some(ml) = &a.multiline {
                    u.text_styles.insert(ml.mtext.style.to_lowercase());
                }
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
    // The table styles stay, and their cells name text styles: with any that has cells of its own, every text style stays.
    if !u.every_style && d.table_styles.iter().all(|t| t.cell_styles.is_empty()) {
        let current = d.header.str("TEXTSTYLE", "Standard").to_lowercase();
        // The styles that stay name text styles too: the multileader styles, and a linetype with letters or a shape in it.
        let in_linetypes = d.linetypes.iter().flat_map(|l| l.pattern.iter()).filter_map(|e| e.style.as_ref());
        let named: BTreeSet<String> =
            d.mleader_styles.iter().map(|st| &st.text_style).chain(in_linetypes).map(|n| n.to_lowercase()).collect();
        d.text_styles.retain(|st| {
            let low = st.name.to_lowercase();
            low == "standard" || low == current || u.text_styles.contains(&low) || named.contains(&low)
        });
    }
}

/// A part of the open drawing as a drawing of its own, and what it holds: see [`extract`], which writes it.
pub(crate) fn cut(o: &Opened, m: &Measured, params: &str) -> Result<(Doc, Value), String> {
    let p = parsed(params)?;
    checked(&p, &["open", "depth", "box", "keep", "drop", "sort", "move"], "extract")?;
    let whole = Seen::of(o, m);
    let apart = open(&whole, &p)?;
    let s = apart.as_ref().map_or(whole, |a| a.seen(o));
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
    // By clean layer: the blocks placed on it, by name.
    let mut placed_on: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
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
        if let EntityKind::Insert(i) = &e.kind {
            *placed_on.entry(e.common.layer.clone()).or_default().entry(i.block.clone()).or_default() += 1;
        }
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
    // Each layer with the blocks placed on it, the most placed first (the 15 most, and how many more names).
    let layers: Vec<Value> = layers
        .into_iter()
        .map(|(name, n)| {
            let mut row = json!({"name": name, "items": n});
            if let Some(bs) = placed_on.get(&name) {
                let mut bs: Vec<(&String, &usize)> = bs.iter().collect();
                bs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                // Each with the kinds of item it holds: a block with arcs among windows is likely a door's swing.
                row["blocks"] = bs
                    .iter()
                    .take(15)
                    .map(|(b, k)| {
                        let mut holds: BTreeMap<&str, usize> = BTreeMap::new();
                        if let Some(blk) = d.block(b) {
                            for e in blk.entities.iter() {
                                *holds.entry(kind_of(&e.kind)).or_default() += 1;
                            }
                        }
                        json!({"name": b, "placed": k, "kinds": holds})
                    })
                    .collect();
                if bs.len() > 15 {
                    row["more_blocks"] = json!(bs.len() - 15);
                }
            }
            row
        })
        .collect();
    // The extents in the new drawing's own coordinates when it was moved (it is no longer far out); else as `query`
    // gives the open drawing's, less its offset, which is then said too.
    let extents = match (shift, real(&round)) {
        (_, false) => Value::Null,
        (Some(_), true) => {
            json!([
                [hundredth(round.min.x * m.mm), hundredth(round.min.y * m.mm)],
                [hundredth(round.max.x * m.mm), hundredth(round.max.y * m.mm)]
            ])
        }
        (None, true) => s.box_out(&round),
    };
    let mut about = json!({"items": d.model.len(), "kinds": kinds, "layers": layers, "extents": extents});
    about["left_out"] = left_out(&s, &p, &chosen)?;
    if shift.is_none() && s.offset != Vec2::ZERO {
        about["offset"] = json!([s.offset.x, s.offset.y]);
    }
    if let Some(a) = &apart {
        about["opened"] = a.said();
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

/// `look`: a picture of only what `params` (JSON `{box?, keep?, drop?, color_by?}`; `keep` and `drop` as [`extract`]
/// takes them) selects, fitted to it, `long` pixels on its longer side, on white. A `box` is a view: every item that
/// runs into it is drawn, cut at its edges, and the picture is the box (a cut takes only what is centred in it).
///
/// `color_by` "own" (when not given): the drawing's own colours, layers that are off or frozen not drawn, as the
/// index's own pictures and the viewer show it: what a person sees. "layer": each layer in a colour of its own, every
/// selected item in its layer's colour whatever colour of its own it has, and a layer that is off or frozen drawn too;
/// inside a block placement, what is on layer `0` takes the placement's colour and what is on a layer of its own that
/// layer's (in the legend with `in_blocks`); only a colour set on an item inside a block stays.
///
/// A colour given as `#ffffff` with index 7 (AutoCAD's white, the colour of most layers) is drawn black on the picture's
/// white, as the viewer and a print draw it; every other colour is drawn as it is given.
///
/// `about`: `{width, height, extents (the box the picture's edges are at, mm), selected (the box round the items),
/// items, color_by, legend: [{layer, color: "#rrggbb", items}]}`, the layers with the most items first. By "own", a
/// layer's `color` is its own, with `colors` (`{"#rrggbb": n}`) when its items are drawn in others, and `off` or
/// `frozen` when it is and so is not in the picture.
pub(crate) fn look(o: &Opened, m: &Measured, params: &str, long: u32) -> Result<Looked, String> {
    let p = parsed(params)?;
    checked(&p, &["open", "depth", "box", "keep", "drop", "color_by"], "look")?;
    let whole = Seen::of(o, m);
    let apart = open(&whole, &p)?;
    let s = apart.as_ref().map_or(whole, |a| a.seen(o));
    let by_layer = match given(&p, "color_by").map(|v| v.as_str().unwrap_or("")) {
        None | Some("own") => false,
        Some("layer") => true,
        Some(_) => {
            return Err(format!(
                "`color_by` is \"own\" (the drawing's own colours) or \"layer\" (each layer in a colour of its own), not {}",
                p["color_by"]
            ));
        }
    };
    // A `box` is a view: what runs into it is drawn, cut at its edges, and the picture is the box.
    let area = given(&p, "box").map(|v| area_of(&s, v)).transpose()?;
    let chosen = selected_by(&s, &p, true)?;
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
        let name = s
            .layers
            .get(low)
            .map(|l| l.name.clone())
            .or_else(|| d.model.iter().find(|e| e.common.layer.to_lowercase() == *low).map(|e| e.common.layer.clone()));
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

    let selected_box = round;
    let margin = if let Some(a) = area {
        round = a;
        0.0
    } else {
        MARGIN
    };
    let long = long.clamp(LEAST_LONG, MOST_LONG);
    let (w, h) = (round.width().max(1e-9), round.height().max(1e-9));
    let k = f64::from(long) / (w.max(h) * (1.0 + 2.0 * margin));
    // The margin is a share of the larger side on both, so a thin selection (one wall) still has a picture to be in.
    let pad = w.max(h) * margin;
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
    let mut about = json!({
        "width": width, "height": height, "extents": s.box_out(&view.world_bounds()), "selected": s.box_out(&selected_box),
        "items": chosen.len(), "color_by": if by_layer { "layer" } else { "own" }, "legend": legend,
    });
    if let Some(a) = &apart {
        about["opened"] = a.said();
    }
    Ok(Looked { png, about })
}

/// A multi-line text's words without its inline codes, near enough to read a title by: a paragraph break is a space,
/// a code that runs to a semicolon (a font, a height, a colour) is left out, and so are the braces.
fn plain_words(raw: &str) -> String {
    let mut out = String::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '{' | '}' => {}
            '\\' => match it.next() {
                Some('P') | Some('N') | Some('n') => out.push(' '),
                Some('f' | 'F' | 'H' | 'h' | 'C' | 'c' | 'T' | 'Q' | 'W' | 'A' | 'S' | 'p') => {
                    for d in it.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                Some(d) => out.push(d),
                None => {}
            },
            c => out.push(c),
        }
    }
    out
}

/// The grid `pieces` lays over the drawing, cells on its longer side.
const PIECE_GRID: usize = 1400;
/// An item whose box holds this many others' centres is a frame, a title block or a block as large as a plan: it joins
/// nothing to anything.
const FRAME_HOLDS: u32 = 30;
/// A piece with fewer items, or shorter than this share of the drawing's longer side, is a stray.
const PIECE_ITEMS: usize = 6;
const PIECE_SHARE: f64 = 0.012;

/// `pieces`: the separate drawings lying in the model space, found by where the drawing is drawn and where it is
/// empty, each numbered on one picture of the whole. `params` (JSON): `{box?, gap?}`: only what has its centre in
/// `box`; `gap` (mm), how far apart two things are still one piece (not given: 0.6% of the longer side of what is
/// looked at). Frames, title blocks and blocks as large as a plan join nothing: what they hold is told apart by the
/// gaps inside them. A proposal for whoever looks at the picture: two pieces may be one drawing, and one piece two.
///
/// `about`: `{width, height, extents, gap, pieces: [{n, box, items, texts (its largest words, the largest first),
/// marks (words that read as level marks, the commonest first)}], strays (how many items are in no piece)}`, the
/// pieces in reading order: rows from the top, each from the left.
pub(crate) fn pieces(o: &Opened, m: &Measured, params: &str, long: u32) -> Result<Looked, String> {
    let p = parsed(params)?;
    checked(&p, &["box", "gap"], "pieces")?;
    let s = Seen::of(o, m);
    let area = given(&p, "box").map(|v| area_of(&s, v)).transpose()?;
    let all: Vec<(&Entity, &Bounds2)> =
        s.items().filter(|(_, b)| real(b) && area.as_ref().is_none_or(|a| a.contains(b.center()))).collect();
    if all.is_empty() {
        return Err("nothing to look at: no item with a box is in `box`".into());
    }
    let mut round = Bounds2::EMPTY;
    for (_, b) in &all {
        round = round.union(b);
    }
    let longer = round.width().max(round.height()).max(1e-9);
    let cell = longer / PIECE_GRID as f64;
    let (nx, ny) = (((round.width() / cell).ceil() as usize).max(1) + 1, ((round.height() / cell).ceil() as usize).max(1) + 1);
    let gap = match given(&p, "gap") {
        None => longer * 0.006,
        Some(v) => match v.as_f64() {
            Some(g) if g.is_finite() && g > 0.0 => g / m.mm,
            _ => return Err(format!("`gap` is a length in mm, not {v}")),
        },
    };
    let reach = ((gap / cell).ceil() as isize).clamp(1, 40);
    let at = |q: Vec2| -> (usize, usize) {
        ((((q.x - round.min.x) / cell) as usize).min(nx - 1), (((q.y - round.min.y) / cell) as usize).min(ny - 1))
    };
    // How many centres lie in each cell, summed from the corner, so that a box's are counted at once.
    let mut sum = vec![0u32; (nx + 1) * (ny + 1)];
    for (_, b) in &all {
        let (i, j) = at(b.center());
        sum[(j + 1) * (nx + 1) + i + 1] += 1;
    }
    for j in 1..=ny {
        for i in 1..=nx {
            sum[j * (nx + 1) + i] = sum[j * (nx + 1) + i] + sum[(j - 1) * (nx + 1) + i] + sum[j * (nx + 1) + i - 1] - sum[(j - 1) * (nx + 1) + i - 1];
        }
    }
    let holds = |b: &Bounds2| -> u32 {
        let ((i0, j0), (i1, j1)) = (at(b.min), at(b.max));
        let (i1, j1) = (i1 + 1, j1 + 1);
        sum[j1 * (nx + 1) + i1] + sum[j0 * (nx + 1) + i0] - sum[j0 * (nx + 1) + i1] - sum[j1 * (nx + 1) + i0]
    };
    // Where the drawing is drawn: a line along itself, anything else over its box; a frame nowhere.
    let mut drawn = vec![false; nx * ny];
    let mut frame = vec![false; all.len()];
    for (n, (e, b)) in all.iter().enumerate() {
        let big = b.width().max(b.height()) > 12.0 * cell;
        match &e.kind {
            EntityKind::Line(l) => {
                let (a, z) = (l.a.xy(), l.b.xy());
                let steps = ((((z.x - a.x).abs().max((z.y - a.y).abs())) / cell).ceil() as usize).max(1) * 2;
                // A line that runs most of the way across is a sheet's border or a cut line: it joins nothing.
                if b.width().max(b.height()) > 0.25 * longer {
                    frame[n] = true;
                    continue;
                }
                for k in 0..=steps {
                    let t = k as f64 / steps as f64;
                    let (i, j) = at(Vec2::new(a.x + (z.x - a.x) * t, a.y + (z.y - a.y) * t));
                    drawn[j * nx + i] = true;
                }
            }
            _ if big && holds(b) >= FRAME_HOLDS => frame[n] = true,
            _ => {
                let ((i0, j0), (i1, j1)) = (at(b.min), at(b.max));
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        drawn[j * nx + i] = true;
                    }
                }
            }
        }
    }
    // Cells within reach of one another are one piece.
    let mut piece = vec![0u32; nx * ny];
    let mut boxes: Vec<(usize, usize, usize, usize)> = Vec::new(); // of each piece, in cells
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            if !drawn[j * nx + i] || piece[j * nx + i] != 0 {
                continue;
            }
            let id = boxes.len() as u32 + 1;
            let mut bx = (i, j, i, j);
            piece[j * nx + i] = id;
            stack.push((i, j));
            while let Some((ci, cj)) = stack.pop() {
                bx = (bx.0.min(ci), bx.1.min(cj), bx.2.max(ci), bx.3.max(cj));
                for dj in -reach..=reach {
                    for di in -reach..=reach {
                        let (qi, qj) = (ci as isize + di, cj as isize + dj);
                        if qi < 0 || qj < 0 || qi as usize >= nx || qj as usize >= ny {
                            continue;
                        }
                        let q = qj as usize * nx + qi as usize;
                        if drawn[q] && piece[q] == 0 {
                            piece[q] = id;
                            stack.push((qi as usize, qj as usize));
                        }
                    }
                }
            }
            boxes.push(bx);
        }
    }
    // Each piece's box in the drawing, and one inside another is that other's (a courtyard's planting in its plan).
    let mut found: Vec<Bounds2> = boxes
        .iter()
        .map(|&(i0, j0, i1, j1)| {
            Bounds2::new(
                Vec2::new(round.min.x + i0 as f64 * cell, round.min.y + j0 as f64 * cell),
                Vec2::new(round.min.x + (i1 + 1) as f64 * cell, round.min.y + (j1 + 1) as f64 * cell),
            )
        })
        .collect();
    found.sort_by(|a, b| (b.width() * b.height()).total_cmp(&(a.width() * a.height())));
    let mut kept: Vec<Bounds2> = Vec::new();
    for b in found {
        match kept.iter_mut().find(|k| k.contains(b.center())) {
            Some(k) => *k = k.union(&b),
            None => kept.push(b),
        }
    }
    // What each holds: its items, its largest words and what reads as level marks.
    struct Piece {
        b: Bounds2,
        items: usize,
        texts: Vec<(f64, String)>,
        marks: BTreeMap<String, usize>,
    }
    let mut out: Vec<Piece> = kept.into_iter().map(|b| Piece { b, items: 0, texts: Vec::new(), marks: BTreeMap::new() }).collect();
    let mut strays = 0usize;
    for (n, (e, b)) in all.iter().enumerate() {
        if frame[n] {
            continue;
        }
        let Some(pc) = out.iter_mut().find(|pc| pc.b.contains(b.center())) else {
            strays += 1;
            continue;
        };
        pc.items += 1;
        let words = match &e.kind {
            EntityKind::Text(t) => Some((t.height, t.value.clone())),
            EntityKind::MText(t) => Some((t.height, plain_words(&t.contents))),
            _ => None,
        };
        if let Some((h, w)) = words {
            let w = w.split_whitespace().collect::<Vec<_>>().join(" ");
            if w.is_empty() {
                continue;
            }
            let mark = w.len() <= 8 && (w.starts_with('+') || w.starts_with('-')) && w.contains('.') && w[1..].chars().all(|c| c.is_ascii_digit() || c == '.');
            if mark {
                *pc.marks.entry(w).or_default() += 1;
            } else if w.chars().count() >= 3 {
                pc.texts.push((h, w));
            }
        }
    }
    let before = out.len();
    out.retain(|pc| pc.items >= PIECE_ITEMS && pc.b.width().max(pc.b.height()) >= PIECE_SHARE * longer);
    strays += before - out.len();
    if out.is_empty() {
        return Err("no separate drawing was found: look at the drawing itself".into());
    }
    // Reading order: rows from the top (a row is what lies within half the usual height of its first), each from the left.
    let mut hs: Vec<f64> = out.iter().map(|pc| pc.b.height()).collect();
    hs.sort_by(f64::total_cmp);
    let row = hs[hs.len() / 2] * 0.5;
    out.sort_by(|a, b| b.b.center().y.total_cmp(&a.b.center().y));
    let mut rows: Vec<Vec<Piece>> = Vec::new();
    for pc in out {
        match rows.last_mut() {
            Some(r) if (r[0].b.center().y - pc.b.center().y).abs() <= row => r.push(pc),
            _ => rows.push(vec![pc]),
        }
    }
    let mut out: Vec<Piece> = Vec::new();
    for mut r in rows {
        r.sort_by(|a, b| a.b.min.x.total_cmp(&b.b.min.x));
        out.extend(r);
    }

    // The picture: the drawing as it is, each piece boxed and numbered in red.
    let mut model = EntityStore::new();
    for (e, _) in &all {
        model.push((*e).clone());
    }
    let red = Color::True(Rgb(225, 0, 0));
    let base = all[0].0;
    let mut said: Vec<Value> = Vec::new();
    for (k, pc) in out.iter_mut().enumerate() {
        let b = pc.b;
        let corners = [b.min, Vec2::new(b.max.x, b.min.y), b.max, Vec2::new(b.min.x, b.max.y)];
        for c in 0..4 {
            let (a, z) = (corners[c], corners[(c + 1) % 4]);
            let mut e = base.clone();
            e.common = cadcraft_doc::Common { layer: "0".into(), color: red, ..cadcraft_doc::Common::default() };
            e.kind = EntityKind::Line(cadcraft_doc::Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(z.x, z.y, 0.0) });
            model.push(e);
        }
        // Large enough to read on a picture of the whole file, and above the box so that it covers nothing.
        let h = (b.height().min(b.width()) * 0.3).max(longer * 0.012);
        let mut e = base.clone();
        e.common = cadcraft_doc::Common { layer: "0".into(), color: red, ..cadcraft_doc::Common::default() };
        e.kind = EntityKind::Text(cadcraft_doc::Text {
            insert: Vec3::new(b.min.x, b.max.y + h * 0.2, 0.0),
            align_pt: None,
            height: h,
            value: format!("{}", k + 1),
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            style: "Standard".into(),
            halign: cadcraft_doc::HAlign::default(),
            valign: cadcraft_doc::VAlign::default(),
        });
        model.push(e);
        pc.texts.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut texts: Vec<&str> = Vec::new();
        for (_, w) in &pc.texts {
            if !texts.contains(&w.as_str()) {
                texts.push(w);
            }
            if texts.len() == 6 {
                break;
            }
        }
        let mut marks: Vec<(&String, &usize)> = pc.marks.iter().collect();
        marks.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        said.push(json!({
            "n": k + 1, "box": s.box_out(&b), "items": pc.items, "texts": texts,
            "marks": marks.iter().take(4).map(|(w, _)| w.as_str()).collect::<Vec<_>>(),
        }));
    }
    let mut d = s.d.clone();
    d.model = model;
    let long = long.clamp(LEAST_LONG, MOST_LONG);
    let (w, h) = (round.width().max(1e-9), round.height().max(1e-9));
    let k = f64::from(long) / (w.max(h) * (1.0 + 2.0 * MARGIN));
    let pad = w.max(h) * MARGIN;
    let width = (((w + 2.0 * pad) * k).round() as u32).max(1);
    let height = (((h + 2.0 * pad) * k).round() as u32).max(1);
    let view = View { center: round.center(), scale: k, width, height };
    let px = 1.0 / k;
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
        "width": width, "height": height, "extents": s.box_out(&view.world_bounds()), "gap": hundredth(gap * m.mm),
        "pieces": said, "strays": strays,
    });
    Ok(Looked { png, about })
}

#[cfg(test)]
mod tests;

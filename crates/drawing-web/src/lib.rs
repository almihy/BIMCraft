//! The agent's drawings (Arqiflow's `drawing_read` and `file_view`, docs/superpowers/specs/2026-11-16-dwg-drawings-design.md in
//! Arqiflow, section 3): a DWG or DXF read once into an index by CADCraft's reader (`cadcraft-index`, from our fork), and then
//! asked of from that index, kept open between questions. It runs in the person's own Arqiflow (a Web Worker of a browser
//! tab or the desktop app's hidden page: Arqiflow's web/lib/compute/worker.ts), as a WebAssembly of its own, small beside the
//! engine's. Reading a large office drawing this way keeps its blocks as blocks: nothing is exploded but what a question asks for.
//!
//! The closed outlines a drawing's lines make are found as `cad_outlines` finds them in a model: the lines asked for, blocks
//! exploded, go to this engine's own Pick Lines ([`bimcraft_core::cad::outlines`]), so the agent gets the same outlines
//! whether it reads a drawing alone or one imported into a model.

use std::collections::BTreeSet;

use bimcraft_core::cad::{Cad, outlines};
use bimcraft_core::dxf::{self, Align, Color, Dxf, Horizontal, Vertical};
use bimcraft_core::geom::P2;
use bimcraft_core::json::hundredth;
use bimcraft_core::model::CadImport;
use cadcraft_index::{Opened, Query, Shape};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

mod clean;
mod convert;
pub use convert::{dwg_of, dxf_of};

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

/// A drawing open from its index.
#[wasm_bindgen]
pub struct Drawing {
    o: Opened,
    /// What the reader's instruments measure of it, once, when the first of them is asked (src/clean.rs).
    measured: std::sync::OnceLock<clean::Measured>,
}

/// A file read and indexed: the drawing packed, and its summary (JSON).
#[wasm_bindgen(getter_with_clone)]
pub struct Indexed {
    pub pack: Vec<u8>,
    pub summary: String,
}

/// A page of the drawing drawn: the PNG, and what it shows (JSON: page, pages, name, width, height).
#[wasm_bindgen(getter_with_clone)]
pub struct Picture {
    pub png: Vec<u8>,
    pub about: String,
}

/// A part of the drawing written as a DWG: its bytes, and what it holds (JSON: items, kinds, layers, extents).
#[wasm_bindgen(getter_with_clone)]
pub struct Extracted {
    pub bytes: Vec<u8>,
    pub about: String,
}

/// A font file given by the host (the firm's, the computer's, or a stand-in from Arqiflow's package), registered under
/// its file name for every drawing drawn after: "shx" or "outline", or why it was refused. The fonts a drawing's styles
/// name and lack are [`Drawing::fonts_missing`]; Arqiflow's runner finds them as the Files viewer does.
#[wasm_bindgen]
pub fn add_font(name: &str, bytes: &[u8]) -> Result<String, JsError> {
    match cadcraft_index::given::add_font(name, bytes) {
        Ok(cadcraft_index::given::Added::Shx) => Ok("shx".into()),
        Ok(cadcraft_index::given::Added::Outline) => Ok("outline".into()),
        Err(e) => Err(JsError::new(&e)),
    }
}

/// Whether a font a text style names is here (given, or built in).
#[wasm_bindgen]
pub fn has_font(name: &str) -> bool {
    cadcraft_index::given::found(name)
}

#[wasm_bindgen]
impl Drawing {
    /// The fonts its text styles name that are not here (JSON list, as the styles spell them).
    pub fn fonts_missing(&self) -> String {
        serde_json::to_string(&self.o.fonts_missing()).unwrap_or_else(|_| "[]".into())
    }

    /// A DWG or DXF file's `bytes` (`name` says which by its extension; a DWG is known by its first bytes too) read and indexed.
    pub fn index(bytes: &[u8], name: &str) -> Result<Indexed, JsError> {
        let (pack, summary) = cadcraft_index::index(bytes, name).map_err(err)?;
        Ok(Indexed { pack, summary: summary.to_string() })
    }

    /// The drawing an index's `pack` holds, open.
    #[wasm_bindgen(constructor)]
    pub fn open(pack: &[u8]) -> Result<Drawing, JsError> {
        Ok(Drawing { o: Opened::unpack(pack).map_err(err)?, measured: Default::default() })
    }

    /// What it holds, in short (JSON).
    pub fn summary(&self) -> String {
        self.o.summary().to_string()
    }

    /// The items a query (JSON: layers, kinds, box, text, sheet, block, start, limit) asks for, a page of them (JSON).
    pub fn query(&self, query: &str) -> Result<String, JsError> {
        let q: Query = if query.trim().is_empty() { Query::default() } else { serde_json::from_str(query).map_err(err)? };
        self.o.query(&q).map(|v| v.to_string()).map_err(err)
    }

    /// Page `page` (1 its model, then each sheet) drawn `long` pixels on its longer side; `part` (JSON [left, top, right,
    /// bottom], shares of the page from its top left; empty: the whole page).
    pub fn picture(&self, page: u32, part: &str, long: u32) -> Result<Picture, JsError> {
        let part: Option<[f64; 4]> = if part.trim().is_empty() { None } else { serde_json::from_str(part).map_err(err)? };
        let p = self.o.picture(page as usize, part, long).map_err(err)?;
        let about = json!({"page": p.page, "pages": p.pages, "name": p.name, "width": p.width, "height": p.height}).to_string();
        Ok(Picture { png: p.png, about })
    }

    /// The closed outlines the lines on `layers` make (JSON params: layers?, box?, min_area?), as `cad_outlines` finds them in
    /// a model; JSON {outlines, open}.
    pub fn outlines(&self, params: &str) -> Result<String, JsError> {
        let p: Value = if params.trim().is_empty() { json!({}) } else { serde_json::from_str(params).map_err(err)? };
        found_outlines(&self.o, &p).map(|v| v.to_string()).map_err(err)
    }

    /// What a rule selects, counted (JSON params, a rule: layers?, kinds?, blocks?, box?, min_size?, max_size?, handles?,
    /// colors?, linetypes?; with open?: [rule] and depth?, the block placements those select are taken apart first;
    /// empty: everything): JSON {total, kinds, layers: [{name, items, kinds, color, colors?}], colors, blocks: [{name,
    /// placed, size, holds, layers}], blocks_more?, extents, opened?}. The drawing reader's first question of a drawing,
    /// and of every box it takes for a plan.
    pub fn counts(&self, params: &str) -> Result<String, JsError> {
        clean::counts(&self.o, self.measured(), params).map(|v| v.to_string()).map_err(err)
    }

    /// A part of the drawing written as a DWG of `version` ("2018" ... "2000"; empty: 2018), from the drawing open here
    /// and never from its file read again (JSON params: open?: [rule], depth?, box?, keep?: [rule], drop?: [rule],
    /// sort?: [{to, ...rule}], move?: [dx, dy]): its chosen block placements taken apart, cropped, filtered, re-sorted
    /// onto other layers and moved, in that order, its other blocks kept as blocks, and what nothing uses any more left
    /// out.
    pub fn extract(&self, params: &str, version: &str) -> Result<Extracted, JsError> {
        let (bytes, about) = clean::extract(&self.o, self.measured(), params, version).map_err(err)?;
        Ok(Extracted { bytes, about: about.to_string() })
    }

    /// A picture of only what is selected (JSON params: open?: [rule], depth?, box?, keep?: [rule], drop?: [rule],
    /// color_by?: "own" | "layer"), fitted to it, `long` pixels on its longer side, in the drawing's own colours or each
    /// layer in a colour of its own; `about` (JSON): width, height, extents (the box the picture's edges are at),
    /// selected, items, color_by, legend: [{layer, color, items}], opened?.
    pub fn look(&self, params: &str, long: u32) -> Result<Picture, JsError> {
        let p = clean::look(&self.o, self.measured(), params, long).map_err(err)?;
        Ok(Picture { png: p.png, about: p.about.to_string() })
    }
}

impl Drawing {
    fn measured(&self) -> &clean::Measured {
        self.measured.get_or_init(|| clean::Measured::of(&self.o))
    }
}

/// `outlines`: the lines asked for flattened from the drawing, given to the engine's Pick Lines.
fn found_outlines(o: &Opened, p: &Value) -> Result<Value, String> {
    if let Some(k) = p.as_object().and_then(|m| m.keys().find(|k| !["layers", "box", "min_area"].contains(&k.as_str()))) {
        return Err(format!("unknown parameter `{k}`; outlines take: layers, box, min_area"));
    }
    let layers: Option<Vec<String>> = match p.get("layers") {
        None | Some(Value::Null) => None,
        Some(v) => Some(serde_json::from_value(v.clone()).map_err(|_| format!("`layers` is a list of layer names, not {v}"))?),
    };
    let area: Option<[[f64; 2]; 2]> = match p.get("box") {
        None | Some(Value::Null) => None,
        Some(v) => {
            Some(serde_json::from_value(v.clone()).map_err(|_| format!("`box` is two corners [[x0, y0], [x1, y1]] in mm, not {v}"))?)
        }
    };
    let min_area = match p.get("min_area") {
        None | Some(Value::Null) => 0.0,
        Some(v) => v.as_f64().filter(|a| a.is_finite() && *a >= 0.0).ok_or_else(|| format!("`min_area` is mm², 0 or more, not {v}"))?,
    };
    let flat = o.shapes(layers.as_deref(), area, true, outlines::MAX_PICKED)?;
    // Each shape an entity of a drawing in mm placed as it is: the lines asked for, and every text, for the labels.
    let handles: Vec<String> = flat.iter().map(|f| f.handle.clone()).collect();
    let entities: Vec<dxf::Entity> = flat
        .into_iter()
        .map(|f| dxf::Entity { layer: f.layer, color: Color::ByLayer, linetype: None, weight: None, alpha: 255, shape: shape(f.shape) })
        .collect();
    if !entities.iter().any(|e| !matches!(e.shape, dxf::Shape::Text { .. })) {
        return Ok(json!({"outlines": [], "open": []}));
    }
    let drawing = Dxf {
        scale: Some(1.0),
        unit: "mm".into(),
        version: String::new(),
        layers: Vec::new(),
        linetypes: Vec::new(),
        ltscale: 1.0,
        entities,
        blocks: Vec::new(),
        inserts: Vec::new(),
        extents: None,
        left_out: Default::default(),
    };
    // Held as it is: nothing of it is kept or saved, so it is not packed.
    let cad = Cad::of_unpacked("drawing", &std::sync::Arc::new(drawing), Vec::new());
    let ci = CadImport {
        cad: "drawing".into(),
        level: Default::default(),
        at: P2(0.0, 0.0),
        origin: P2(0.0, 0.0),
        scale: 1.0,
        stretch: 1.0,
        angle: 0.0,
        view: None,
        hidden: BTreeSet::new(),
        colors: Default::default(),
    };
    let window = area.map(|[a, b]| [P2(a[0].min(b[0]), a[1].min(b[1])), P2(a[0].max(b[0]), a[1].max(b[1]))]);
    // The offset is already taken off what `shapes` gave, so a window far out is brought to it too.
    let window = window.map(|[lo, hi]| {
        let off = o.offset();
        let far = |q: P2| q.0.abs().max(q.1.abs()) > cadcraft_index::FAR_MM && (off[0] != 0.0 || off[1] != 0.0);
        let local = |q: P2| if far(q) { P2(q.0 - off[0], q.1 - off[1]) } else { q };
        [local(lo), local(hi)]
    });
    let pick = outlines::Pick { entities: None, layers: None, window };
    let chosen = outlines::picked(&cad, &ci, &pick);
    let found = outlines::outlines(&cad, &ci, &chosen)?;
    let labels = outlines::labels(&cad, &ci, &found.outlines);
    let ents = &cad.dxf().entities;
    // What made an outline: the drawing's items by handle (a block placement for what its block draws), each once.
    let made_of = |ns: &[usize]| -> Vec<String> {
        let mut seen = BTreeSet::new();
        ns.iter().filter_map(|n| handles.get(*n)).filter(|h| seen.insert((*h).clone())).cloned().collect()
    };
    let ring = |r: &[P2]| json!(r.iter().map(|q| [hundredth(q.0), hundredth(q.1)]).collect::<Vec<_>>());
    let rows: Vec<Value> = found
        .outlines
        .iter()
        .zip(&labels)
        .filter(|(ol, _)| ol.area() >= min_area)
        .map(|(ol, labels)| {
            let mut row = json!({
                "points": ring(&ol.ring), "area": hundredth(ol.area()), "area_m2": bimcraft_core::units::m2(ol.area()),
                "from": ol.from.name(), "items": made_of(&ol.entities), "labels": labels,
            });
            let layers: BTreeSet<&str> = ol.entities.iter().filter_map(|n| ents.get(*n)).map(|e| e.layer.as_str()).collect();
            match layers.len() {
                1 => row["layer"] = json!(layers.iter().next()),
                _ => row["layers"] = json!(layers),
            }
            if !ol.holes.is_empty() {
                row["holes"] = json!(ol.holes.iter().map(|h| ring(h)).collect::<Vec<_>>());
            }
            if ol.whole {
                row["whole"] = json!(true);
            }
            row
        })
        .collect();
    let open: Vec<Value> = found.open.iter().map(|op| json!({"points": ring(&op.points), "items": made_of(&op.entities)})).collect();
    Ok(json!({"outlines": rows, "open": open}))
}

/// A flattened shape as the engine's drawings hold it.
fn shape(s: Shape) -> dxf::Shape {
    let p = |q: [f64; 2]| P2(q[0], q[1]);
    match s {
        Shape::Line(a, b) => dxf::Shape::Line(p(a), p(b)),
        Shape::Polyline { points, closed } => dxf::Shape::Polyline { pts: points.into_iter().map(p).collect(), closed },
        Shape::Circle { c, r } => dxf::Shape::Circle { c: p(c), r },
        Shape::Arc { c, r, start, end } => dxf::Shape::Arc { c: p(c), r, start, end },
        Shape::Text { at, height, angle, text } => dxf::Shape::Text {
            at: p(at),
            height,
            angle,
            text,
            align: Align { h: Horizontal::Left, v: Vertical::Baseline },
            tag: None,
            wide: 1.0,
            mirrored: false,
            font: None,
        },
        Shape::Fill { rings, solid } => {
            dxf::Shape::Fill { rings: rings.into_iter().map(|r| r.into_iter().map(p).collect()).collect(), solid, tile: None, shade: false }
        }
    }
}

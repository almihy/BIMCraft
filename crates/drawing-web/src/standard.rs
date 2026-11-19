//! `standard`: a clean plan with what will be built of it laid over it, on layers of its own that are off and do not
//! plot (`X-BUILD-...`). The drawing stays the person's own, as they drew it; switched on, those layers show each wall
//! as one band at its thickness, whole through its doors and windows, each glass wall as a band, each door and window
//! as a mark at its opening (a door's with its leaf and swing: where it hinges and which way it opens), each column as
//! its rectangle. A level of the model is built from those marks alone ([`crate::build`] reads them back when a drawing
//! has them), so what a person moves, adds or deletes there in a CAD editor is what is built.

use std::sync::Arc as Rc;

use cadcraft_doc::color::Color;
use cadcraft_doc::geom::{PolyVertex, Vec2, Vec3};
use cadcraft_doc::{Arc, Block, Common, Entity, EntityKind, EntityStore, Insert, Line, LwPolyline};
use cadcraft_index::Opened;
use cadcraft_io::SaveOptions;
use serde_json::{Value, json};

pub(crate) const WALL: &str = "X-BUILD-WALL";
pub(crate) const GLASS: &str = "X-BUILD-GLASS";
pub(crate) const DOOR: &str = "X-BUILD-DOOR";
pub(crate) const WINDOW: &str = "X-BUILD-WIND";
pub(crate) const COLUMN: &str = "X-BUILD-COLS";
pub(crate) const DOOR_MARK: &str = "ARQ-DOOR";
pub(crate) const WINDOW_MARK: &str = "ARQ-WINDOW";
pub(crate) const COLUMN_MARK: &str = "ARQ-COLUMN";
pub(crate) const ROUND_MARK: &str = "ARQ-COLUMN-ROUND";
/// How wide a glass wall's band is drawn, mm.
const GLASS_BAND: f64 = 60.0;

fn on(layer: &str) -> Common {
    Common { layer: layer.into(), ..Common::default() }
}

fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
}

/// A mark's block, drawn one unit long (and one deep): a placement's scale is its size.
fn mark(d: &mut cadcraft_doc::Drawing, name: &str, items: Vec<EntityKind>) {
    let mut b = Block::new(name);
    for kind in items {
        b.entities.push(Entity { handle: d.new_handle(), common: on("0"), kind });
    }
    d.blocks.insert(name.into(), Rc::new(b));
}

/// The clean plan open as `o` with its build marks, written as a DWG of `version`. JSON params: `k` (as `build`).
/// Answers what the marks hold, by kind, and `left`: what the plan draws that made none.
pub(crate) fn standard(o: &Opened, params: &str, version: &str) -> Result<(Vec<u8>, Value), String> {
    let v = crate::convert::version(version)?;
    let p: Value = if params.trim().is_empty() { json!({}) } else { serde_json::from_str(params).map_err(|e| format!("the parameters are not JSON: {e}"))? };
    let k = p.get("k").and_then(Value::as_f64).filter(|x| x.is_finite() && *x > 0.0).unwrap_or(1.0);
    let built = crate::build::computed(o, params)?;
    let mm = o.summary().get("mm").and_then(Value::as_f64).filter(|m| m.is_finite() && *m > 0.0).unwrap_or(1.0);
    let off = o.offset();
    // What `build` answers (real millimetres, less the offset) as the drawing's own numbers.
    let at = |q: &Value| -> Option<Vec2> { Some(Vec2::new((q.get(0)?.as_f64()? / k + off[0]) / mm, (q.get(1)?.as_f64()? / k + off[1]) / mm)) };
    let long = |l: f64| l / k / mm;

    let mut d = o.drawing.clone();
    let mut model = EntityStore::new();
    for e in d.model.iter().filter(|e| !e.common.layer.to_uppercase().starts_with("X-BUILD")) {
        model.push((**e).clone());
    }
    for (name, color) in [(WALL, 8), (GLASS, 4), (DOOR, 3), (WINDOW, 5), (COLUMN, 1)] {
        d.ensure_layer(name);
        if let Some(l) = d.layer_mut(name) {
            l.color = Color::Index(color);
            l.on = false;
            l.plot = false;
        }
    }
    let quarter = EntityKind::Arc(Arc { center: Vec3::new(0.0, 0.0, 0.0), radius: 1.0, start: 0.0, end: std::f64::consts::FRAC_PI_2 });
    mark(&mut d, DOOR_MARK, vec![line((0.0, 0.0), (1.0, 0.0)), line((0.0, 0.0), (0.0, 1.0)), quarter]);
    mark(&mut d, WINDOW_MARK, vec![line((0.0, 0.0), (1.0, 0.0)), line((0.0, -0.08), (0.0, 0.08)), line((1.0, -0.08), (1.0, 0.08))]);
    mark(&mut d, COLUMN_MARK, vec![line((-0.5, -0.5), (0.5, -0.5)), line((0.5, -0.5), (0.5, 0.5)), line((0.5, 0.5), (-0.5, 0.5)), line((-0.5, 0.5), (-0.5, -0.5))]);
    mark(&mut d, ROUND_MARK, vec![EntityKind::Circle(cadcraft_doc::Circle { center: Vec3::new(0.0, 0.0, 0.0), radius: 0.5 })]);

    let mut put = |d: &mut cadcraft_doc::Drawing, layer: &str, kind: EntityKind| {
        model.push(Entity { handle: d.new_handle(), common: on(layer), kind });
    };
    let rows = |key: &str| built.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut said = serde_json::Map::new();
    // A wall, a glass wall: one band from end to end, as wide as it is thick; an arc's by its bulge.
    for (key, layer) in [("walls", WALL), ("curtain", GLASS)] {
        let mut n = 0;
        for w in rows(key) {
            let (Some(a), Some(b)) = (w.get("a").and_then(&at), w.get("b").and_then(&at)) else { continue };
            let wide = long(w.get("thickness").and_then(Value::as_f64).unwrap_or(GLASS_BAND));
            let bulge = w.get("arc").and_then(Value::as_f64).map_or(0.0, |sweep| (sweep.to_radians() / 4.0).tan());
            let vertices = vec![PolyVertex { p: a, bulge, start_width: wide, end_width: wide }, PolyVertex { p: b, bulge: 0.0, start_width: wide, end_width: wide }];
            put(&mut d, layer, EntityKind::LwPolyline(LwPolyline { vertices, closed: false, const_width: wide, elevation: 0.0, plinegen: false }));
            n += 1;
        }
        said.insert(key.into(), json!(n));
    }
    // A door: its mark hinged at one end of its opening, turned along it, as large as the door is wide, and on the
    // side it opens to. A window: its mark from one end of its opening to the other.
    for (key, layer, block) in [("doors", DOOR, DOOR_MARK), ("windows", WINDOW, WINDOW_MARK)] {
        let mut n = 0;
        for w in rows(key) {
            let (Some(mut a), Some(mut b)) = (w.get("a").and_then(&at), w.get("b").and_then(&at)) else { continue };
            let tip = w.get("toward").and_then(&at);
            // The hinge is the end its leaf's tip is nearer to.
            if tip.is_some_and(|t| (t - b).len() < (t - a).len()) {
                std::mem::swap(&mut a, &mut b);
            }
            let along = b - a;
            let width = along.len();
            if width <= 0.0 {
                continue;
            }
            let side = tip.map_or(1.0, |t| if along.x * (t.y - a.y) - along.y * (t.x - a.x) >= 0.0 { 1.0 } else { -1.0 });
            put(
                &mut d,
                layer,
                EntityKind::Insert(Insert { block: block.into(), insert: Vec3::new(a.x, a.y, 0.0), scale: Vec3::new(width, width * side, 1.0), rotation: along.y.atan2(along.x), ..Insert::default() }),
            );
            n += 1;
        }
        said.insert(key.into(), json!(n));
    }
    let mut n = 0;
    for c in rows("columns") {
        let Some(q) = c.get("at").and_then(&at) else { continue };
        let wide = long(c.get("width").and_then(Value::as_f64).unwrap_or(0.0));
        let round = c.get("round").and_then(Value::as_bool).unwrap_or(false);
        let deep = if round { wide } else { long(c.get("depth").and_then(Value::as_f64).unwrap_or(0.0)) };
        if wide <= 0.0 || deep <= 0.0 {
            continue;
        }
        let rotation = c.get("angle").and_then(Value::as_f64).unwrap_or(0.0).to_radians();
        let block = if round { ROUND_MARK } else { COLUMN_MARK };
        put(&mut d, COLUMN, EntityKind::Insert(Insert { block: block.into(), insert: Vec3::new(q.x, q.y, 0.0), scale: Vec3::new(wide, deep, 1.0), rotation, ..Insert::default() }));
        n += 1;
    }
    said.insert("columns".into(), json!(n));
    d.model = model;
    let bytes = cadcraft_io::write_with(&d, "drawing.dwg", &SaveOptions { version: Some(v), binary: false }).map_err(|e| e.to_string())?;
    let mut about = Value::Object(said);
    about["left"] = built.get("left").cloned().unwrap_or(Value::Null);
    about["thicknesses"] = built.get("thicknesses").cloned().unwrap_or(Value::Null);
    Ok((bytes, about))
}

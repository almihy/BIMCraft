//! `graphics`'s tests that drive the Session (moved out of `graphics.rs`).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::prim::{Prim, Rgb};
use crate::render::{self, Drawing};

pub fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// An 8 x 6 m box of walls on Level 1 (ids 3 to 6, the bottom one first).
pub fn house(s: &mut Session) -> Vec<u64> {
    let r = run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    r["ids"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
}

pub fn plan(s: &Session, level: u64) -> Drawing {
    render::plan(&s.model, level, 1600, 1000)
}

/// The primitives `id` owns.
pub fn owned(d: &Drawing, id: u64) -> Vec<&Prim> {
    d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(id)).map(|(p, _)| p).collect()
}

/// Pen, colour and dash of the lines `id` owns: `u8::MAX` for a line no pen draws (lettering's).
pub fn lines(d: &Drawing, id: u64) -> Vec<(u8, Rgb, Option<Vec<f32>>)> {
    (d.prims.iter().zip(&d.owners).zip(&d.pens))
        .filter_map(|((p, o), pen)| match p {
            Prim::Line { color, dash, .. } if *o == Some(id) => Some((pen.map_or(u8::MAX, |p| p.0), *color, dash.clone())),
            _ => None,
        })
        .collect()
}

/// How many primitives of `d` have a point within `r` px of model point `at`: what is drawn there.
pub fn drawn_near(d: &Drawing, at: P2, r: f32) -> usize {
    let (x, y) = d.view.px(at);
    let near = |p: &(f32, f32)| (p.0 - x).hypot(p.1 - y) <= r;
    d.prims
        .iter()
        .filter(|p| match p {
            Prim::Fill { rings, .. } => rings.iter().flatten().any(near),
            Prim::Line { pts, .. } => pts.iter().any(near),
            Prim::Image { .. } => false,
        })
        .count()
}

pub fn pixel(d: &Drawing, p: P2) -> Rgb {
    let pm = tiny_skia::Pixmap::decode_png(&render::png(d).unwrap()).unwrap();
    let (x, y) = d.view.px(p);
    let c = pm.pixel(x as u32, y as u32).unwrap();
    Rgb(c.red(), c.green(), c.blue())
}

pub const RED: Rgb = Rgb(200, 0, 0);
pub const PAPER: Rgb = render::PAPER;

pub fn picture(s: &mut Session, view: &str, name: &str) -> tiny_skia::Pixmap {
    let path = std::env::temp_dir().join(format!("bimcraft-gfx-{}-{name}.png", std::process::id()));
    run(s, "render", json!({"view": view, "width": 240, "height": 180, "path": path.to_str().unwrap()}));
    let pm = tiny_skia::Pixmap::load_png(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    pm
}

pub fn painted(pm: &tiny_skia::Pixmap, keep: impl Fn(u8, u8, u8) -> bool) -> usize {
    pm.pixels().iter().filter(|c| keep(c.red(), c.green(), c.blue())).count()
}

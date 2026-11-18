//! Drawings to test with: a layer, an entity, a drawing of them, and a dense sheet, for this crate's tests and the drawing
//! crate's (`bimcraft-render`), which draw them.

use crate::dxf::{Color, Dxf, Entity, Layer, Shape};
use crate::geom::P2;

/// A layer of colour `color`.
pub fn layer(name: &str, color: Color) -> Layer {
    Layer { name: name.into(), color, linetype: "Continuous".into(), weight: None, off: false, frozen: false }
}

/// An entity of `layer` in its layer's colour.
pub fn entity(layer: &str, shape: Shape) -> Entity {
    Entity { layer: layer.into(), color: Color::ByLayer, linetype: None, weight: None, alpha: 255, shape }
}

/// A drawing of `entities` on the layers named, each in its colour.
pub fn drawing(layers: &[(&str, Color)], entities: Vec<Entity>) -> Dxf {
    Dxf {
        scale: Some(1.0),
        unit: "mm".into(),
        version: String::new(),
        layers: layers.iter().map(|(n, c)| layer(n, *c)).collect(),
        linetypes: Vec::new(),
        ltscale: 1.0,
        entities,
        blocks: Vec::new(),
        inserts: Vec::new(),
        extents: None,
        left_out: Default::default(),
    }
}

/// A random number in [0, 1) from `seed`, which it moves on.
pub fn random(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// A dense sheet `size` units across: `n` short strokes (a unit or three long) on layers Walls (black ink), Doors (red) and
/// Hatch (blue), a small solid fill and a patterned one among every hundred, in a pen of 0.2 mm, and a frame round it all: a sheet whose
/// entities are under a pixel when it is a few hundred pixels wide.
pub fn dense(n: usize, size: P2) -> Dxf {
    let mut seed = 0x9e37_79b9_7f4a_7c15;
    let mut entities = Vec::with_capacity(n + 1);
    for i in 0..n {
        // Clustered as a plan is: rooms dense, corridors sparse.
        let (cx, cy) = (random(&mut seed), random(&mut seed));
        let at = P2(cx * cx * size.0 * 0.98 + size.0 * 0.01, cy.sqrt() * size.1 * 0.98 + size.1 * 0.01);
        let layer = ["Walls", "Doors", "Hatch"][i % 3];
        let shape = if i % 100 == 7 {
            let r = 0.5 + random(&mut seed);
            Shape::Fill {
                rings: vec![vec![at, at + P2(r, 0.0), at + P2(r, r), at + P2(0.0, r)]],
                solid: i % 200 == 7,
                tile: None,
                shade: false,
            }
        } else {
            let (len, turn) = (1.0 + random(&mut seed) * 2.0, random(&mut seed) * std::f64::consts::TAU);
            Shape::Polyline { pts: vec![at, at + P2(turn.cos(), turn.sin()) * len], closed: false }
        };
        // A pen a pixel wide at 1 : 100, as every painter draws it.
        entities.push(Entity { weight: Some(0.2), ..entity(layer, shape) });
    }
    let frame = vec![P2(0.0, 0.0), P2(size.0, 0.0), size, P2(0.0, size.1)];
    entities.push(entity("Walls", Shape::Polyline { pts: frame, closed: true }));
    drawing(&[("Walls", Color::Index(7)), ("Doors", Color::True(220, 30, 30)), ("Hatch", Color::True(30, 60, 220))], entities)
}

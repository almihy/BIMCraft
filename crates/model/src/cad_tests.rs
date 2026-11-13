//! `cad`'s tests that need only the model: the grid of a drawing's entities, and its level of detail.

use super::*;
use crate::dxf::{Layer, Shape};

/// A layer of colour `color`.
fn layer(name: &str, color: Color) -> Layer {
    Layer { name: name.into(), color, linetype: "Continuous".into(), weight: None, off: false, frozen: false }
}

/// An entity of `layer` in its layer's colour.
fn entity(layer: &str, shape: Shape) -> Entity {
    Entity { layer: layer.into(), color: Color::ByLayer, linetype: None, weight: None, alpha: 255, shape }
}

/// A drawing of `entities` on the layers named, each in its colour.
pub(crate) fn drawing(layers: &[(&str, Color)], entities: Vec<Entity>) -> Dxf {
    Dxf {
        scale: Some(1.0),
        unit: "mm".into(),
        version: String::new(),
        layers: layers.iter().map(|(n, c)| layer(n, *c)).collect(),
        linetypes: Vec::new(),
        ltscale: 1.0,
        entities,
        inserts: Vec::new(),
        extents: None,
        left_out: Default::default(),
    }
}

/// A random number in [0, 1) from `seed`, which it moves on.
pub(crate) fn random(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed >> 11) as f64 / (1u64 << 53) as f64
}

/// A dense sheet `size` units across: `n` short strokes (a unit or three long) on layers Walls (black ink), Doors (red) and
/// Hatch (blue), a small solid fill and a patterned one among every hundred, in a pen of 0.2 mm, and a frame round it all: a sheet whose
/// entities are under a pixel when it is a few hundred pixels wide.
pub(crate) fn dense(n: usize, size: P2) -> Dxf {
    let mut seed = 0x9e37_79b9_7f4a_7c15;
    let mut entities = Vec::with_capacity(n + 1);
    for i in 0..n {
        // Clustered as a plan is: rooms dense, corridors sparse.
        let (cx, cy) = (random(&mut seed), random(&mut seed));
        let at = P2(cx * cx * size.0 * 0.98 + size.0 * 0.01, cy.sqrt() * size.1 * 0.98 + size.1 * 0.01);
        let layer = ["Walls", "Doors", "Hatch"][i % 3];
        let shape = if i % 100 == 7 {
            let r = 0.5 + random(&mut seed);
            Shape::Fill { rings: vec![vec![at, at + P2(r, 0.0), at + P2(r, r), at + P2(0.0, r)]], solid: i % 200 == 7, tile: None }
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

#[test]
fn what_lies_in_a_box_is_what_a_scan_of_every_entity_finds() {
    // Small entities, wide ones (past the most cells one is put in), far outliers, a point, and one that isn't a number's.
    let mut seed = 42;
    let mut entities = Vec::new();
    for i in 0..5_000 {
        let at = P2(random(&mut seed) * 1000.0, random(&mut seed) * 800.0);
        let shape = match i % 50 {
            0 => Shape::Polyline { pts: vec![at, P2(1000.0 - at.0, 800.0 - at.1)], closed: false },
            1 => Shape::Polyline { pts: vec![at, at + P2(random(&mut seed) * 400.0, 3.0)], closed: false },
            2 => Shape::Point(at),
            3 => Shape::Fill { rings: vec![vec![at, at + P2(30.0, 0.0), at + P2(0.0, 30.0)]], solid: true, tile: None },
            _ => Shape::Polyline { pts: vec![at, at + P2(random(&mut seed) * 4.0, random(&mut seed) * 4.0)], closed: false },
        };
        entities.push(entity("A", shape));
    }
    for far in [P2(1e7, 5.0), P2(-3e6, -2e6), P2(500.0, 4e8)] {
        entities.push(entity("A", Shape::Polyline { pts: vec![far, far + P2(1.0, 1.0)], closed: false }));
    }
    entities.push(entity("A", Shape::Polyline { pts: vec![P2(0.0, 0.0), P2(-1e6, 1e7)], closed: false }));
    entities.push(entity("A", Shape::Polyline { pts: vec![P2(f64::NAN, 0.0), P2(1.0, 1.0)], closed: false }));
    let d = drawing(&[("A", Color::Index(7))], entities);
    let boxes: Vec<[P2; 2]> = d.entities.iter().map(|e| e.shape.bounds()).collect();
    let cad = Cad::of_drawing("grid.pdf", d);
    let scan = |lo: P2, hi: P2| -> Vec<usize> {
        (0..boxes.len()).filter(|i| boxes.get(*i).is_some_and(|[a, b]| a.0 <= hi.0 && b.0 >= lo.0 && a.1 <= hi.1 && b.1 >= lo.1)).collect()
    };
    let mut queries = vec![
        (P2(-1e9, -1e9), P2(1e9, 1e9)),
        (P2(0.0, 0.0), P2(1000.0, 800.0)),
        (P2(9.99e6, 0.0), P2(1.01e7, 10.0)),
        (P2(-1.0, -1.0), P2(-0.5, -0.5)),
        (P2(2000.0, 2000.0), P2(3000.0, 3000.0)),
        (P2(500.0, 400.0), P2(500.0, 400.0)),
    ];
    for _ in 0..300 {
        let (x, y, w, h) = (random(&mut seed) * 1100.0 - 50.0, random(&mut seed) * 900.0 - 50.0, random(&mut seed), random(&mut seed));
        queries.push((P2(x, y), P2(x + w * w * 600.0, y + h * h * 600.0)));
    }
    for (lo, hi) in queries {
        assert_eq!(cad.entities_in(lo, hi), scan(lo, hi), "the box {lo:?} to {hi:?}");
    }
    for (i, b) in boxes.iter().enumerate().step_by(97) {
        assert!(cad.entity_box(i).is_some_and(|e| e[0].0.to_bits() == b[0].0.to_bits() && e[1].1.to_bits() == b[1].1.to_bits()));
    }
}

#[test]
fn a_dense_drawing_has_bands_of_cells_each_under_a_pixel_and_a_sparse_one_none() {
    let cad = Cad::of_drawing("dense.pdf", dense(20_000, P2(1000.0, 700.0)));
    // Its bulk is 1000 units across: 0.4 px a unit wants cells of no more than 2.5 units, the 512 band's of under 2.
    let (lod, band) = cad.lod_band(0.4).unwrap();
    assert_eq!(lod.layers, ["Walls", "Doors", "Hatch"]);
    assert!(band.cell * 0.4 <= 1.0 && band.cell * 0.4 > 0.5, "{}", band.cell);
    // The frame is too big for its cells and drawn itself; every stroke is in them.
    assert_eq!(band.big, vec![20_000]);
    assert!(band.len() > 1000 && band.len() < 20_000 * 3, "{}", band.len());
    // Its ink is all there: as much line as the strokes are long (a cell a cell), and their ends; and the hatches' area.
    let as_drawn = |_: usize, c: Option<Rgb>| Some(c.unwrap_or(crate::render::INK));
    let all = lod.picture(band, 1, &as_drawn).unwrap();
    let ink: f64 = all.cells_in(P2(-1e9, -1e9), P2(1e9, 1e9)).map(|(_, c)| lod::ink_of(c.ink)).sum();
    let want: f64 = cad
        .dxf()
        .entities
        .iter()
        .take(20_000)
        .map(|e| match &e.shape {
            Shape::Polyline { pts, .. } => {
                pts.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>() / band.cell + std::f64::consts::FRAC_PI_4 / 0.75
            }
            Shape::Fill { rings, .. } => crate::geom::area(&rings[0]).abs() / (band.cell * band.cell) * 0.75,
            _ => 0.0,
        })
        .sum();
    assert!((ink - want).abs() < want * 0.03, "{ink} of {want}");
    // A cell lies in the box asked for, or a cell round it.
    let (lo, hi) = (P2(20.0, 600.0), P2(60.0, 620.0));
    assert!(all.cells_in(lo, hi).count() > 50, "{}", all.cells_in(lo, hi).count());
    for (c, _) in all.cells_in(lo, hi) {
        assert!(c.0 >= lo.0 - band.cell && c.0 <= hi.0 + band.cell && c.1 >= lo.1 - band.cell && c.1 <= hi.1 + band.cell, "{c:?}");
    }
    // A layer hidden lays no ink: Doors, red, is gone; the rest the same.
    let doors = lod.layers.iter().position(|l| l == "Doors").unwrap();
    let hidden = lod.picture(band, 2, &|l, c| (l != doors).then(|| c.unwrap_or(crate::render::INK))).unwrap();
    assert!(hidden.cells_in(P2(-1e9, -1e9), P2(1e9, 1e9)).all(|(_, c)| c.rgb.0 < 150 || c.rgb.2 > 100), "no red left");
    let left: f64 = hidden.cells_in(P2(-1e9, -1e9), P2(1e9, 1e9)).map(|(_, c)| lod::ink_of(c.ink)).sum();
    assert!(left < ink * 0.72 && left > ink * 0.6, "{left} of {ink}");
    // Kept by its look: asked again, the same picture; and the band made once.
    assert!(Arc::ptr_eq(&lod.picture(band, 1, &as_drawn).unwrap(), &all));
    assert!(std::ptr::eq(cad.lod_band(0.45).unwrap().1, band));
    // Near (8 px a unit: the finest band's cells are 2 px), there's none fine enough; a sparse drawing has none.
    assert!(cad.lod_band(8.0).is_none());
    let sparse = Cad::of_drawing("sparse.pdf", dense(2_000, P2(1000.0, 700.0)));
    assert!(sparse.lod_band(0.4).is_none());
}

#[test]
fn a_band_past_what_it_may_hold_is_dropped() {
    // Strokes ten cells of the finest band across, each through twenty of its cells: more marks than a band may hold here,
    // so that zoom draws every entity; a coarser band holds them.
    let mut seed = 7;
    let entities = (0..20_000)
        .map(|_| {
            let at = P2(random(&mut seed) * 4000.0, random(&mut seed) * 4000.0);
            entity("A", Shape::Polyline { pts: vec![at, at + P2(10.0, 10.0)], closed: false })
        })
        .collect();
    let d = drawing(&[("A", Color::Index(7))], entities);
    let (bulk, _) = bulk(&d);
    let mut lod = lod::Lod::new(&d, bulk, &|_| None).unwrap();
    lod.most.0 = 200_000;
    assert!(lod.band(&d, 1.0).is_none());
    assert!(lod.band(&d, 0.25).is_some_and(|b| b.len() <= 200_000));
}

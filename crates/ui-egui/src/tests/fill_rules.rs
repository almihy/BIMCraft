//! A fill's rule on the canvas: what the canvas hands egui covers what the rule fills, and a hole stays a hole however many
//! edges the fill has.

use crate::canvas::{self, Place};
use bimcraft_core::render::{self, Prim, Rgb, Rule};
use egui::epaint::Tessellator;
use egui::{Pos2, Rect, pos2, vec2};

/// Whether the triangles egui makes of what the canvas hands it for `prim` cover each of `probes` (screen px).
fn covered(prim: &Prim, probes: &[(f32, f32)]) -> Vec<bool> {
    let clip = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 700.0));
    let mut shapes = Vec::new();
    canvas::paint_prim(
        &mut |s| shapes.push(s),
        clip,
        Place::at(pos2(0.0, 0.0)),
        (prim, None, render::Part::Projection),
        &Default::default(),
        None,
        false,
        &mut [].iter(),
    );
    let mut mesh = egui::Mesh::default();
    let mut tessellator = Tessellator::new(1.0, Default::default(), [64, 64], Vec::new());
    for s in shapes {
        tessellator.tessellate_shape(s, &mut mesh);
    }
    let side = |a: Pos2, b: Pos2, p: Pos2| (b - a).x * (p - a).y - (b - a).y * (p - a).x;
    probes
        .iter()
        .map(|&(x, y)| {
            let p = pos2(x, y);
            mesh.indices.chunks_exact(3).any(|t| {
                let [a, b, c] = [t[0], t[1], t[2]].map(|i| mesh.vertices[i as usize]);
                if a.color.a() == 0 && b.color.a() == 0 && c.color.a() == 0 {
                    return false;
                }
                let (u, v, w) = (side(a.pos, b.pos, p), side(b.pos, c.pos, p), side(c.pos, a.pos, p));
                (u >= 0.0 && v >= 0.0 && w >= 0.0) || (u <= 0.0 && v <= 0.0 && w <= 0.0)
            })
        })
        .collect()
}

fn square(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<(f32, f32)> {
    vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
}

/// A circle of `n` points round (450, 350), turning one way or the other.
fn circle(r: f32, n: usize, back: bool) -> Vec<(f32, f32)> {
    (0..n)
        .map(|i| {
            let t = std::f32::consts::TAU * i as f32 / n as f32 * if back { -1.0 } else { 1.0 };
            (450.0 + r * t.cos(), 350.0 + r * t.sin())
        })
        .collect()
}

#[test]
fn an_even_odd_fill_leaves_its_hole_and_a_non_zero_one_fills_where_its_rings_overlap() {
    // A square with a square hole turning the same way: even-odd leaves the hole.
    let holed = Prim::Fill {
        rings: vec![square(100.0, 100.0, 500.0, 500.0), square(200.0, 200.0, 400.0, 400.0)],
        color: Rgb(0, 0, 255),
        rule: Rule::EvenOdd,
        alpha: render::OPAQUE,
    };
    assert_eq!(covered(&holed, &[(150.0, 300.0), (300.0, 300.0)]), [true, false]);
    // Two squares turning the same way, overlapping: non-zero fills both and where they overlap.
    let two = Prim::Fill {
        rings: vec![square(100.0, 100.0, 300.0, 300.0), square(200.0, 200.0, 400.0, 400.0)],
        color: Rgb(0, 0, 255),
        rule: Rule::NonZero,
        alpha: render::OPAQUE,
    };
    assert_eq!(covered(&two, &[(150.0, 150.0), (250.0, 250.0), (350.0, 350.0), (350.0, 150.0)]), [true, true, true, false]);
}

#[test]
fn an_even_odd_fill_of_more_edges_than_one_sweep_takes_keeps_its_hole() {
    // Two circles of 300,000 points each, both the same way round: well past the edges one sweep takes.
    let n = 300_000;
    let ring = Prim::Fill {
        rings: vec![circle(300.0, n, false), circle(100.0, n, false)],
        color: Rgb(0, 0, 255),
        rule: Rule::EvenOdd,
        alpha: render::OPAQUE,
    };
    assert_eq!(covered(&ring, &[(450.0, 350.0), (650.0, 350.0), (450.0, 150.0), (250.0, 360.0)]), [false, true, true, true]);
    // Non-zero, the inner circle turning back: still a hole, no triangle reaching into it.
    let rings = vec![circle(300.0, n, false), circle(100.0, n, true)];
    let triangles = bimcraft_core::text::fill_triangles(&rings, Rule::NonZero);
    assert!(!triangles.is_empty());
    assert!(triangles.iter().flatten().all(|q| (q.0 - 450.0).hypot(q.1 - 350.0) > 99.0));
}

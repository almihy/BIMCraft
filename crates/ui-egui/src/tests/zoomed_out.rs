//! A plan zoomed far out: what the canvas hands egui draws inside the elements it draws, at any zoom. egui feathers a convex fill
//! and mitres a closed line along each corner's normal, divided by how far the corner turns back: where an outline nearly doubles
//! back on itself (a sliver, a hatch's spur, points a few thousandths of a pixel apart), that reaches across the view.

use crate::canvas::{self, Place};
use bimcraft_core::geom::P2;
use bimcraft_core::render::{self, Prim, Rgb, View};
use egui::epaint::Tessellator;
use egui::{Rect, pos2, vec2};

/// The outline of a hatch in Hauts de Magny's masterplan (mm, from its first point): a curved edge, then a spur that turns
/// back on itself where the hatch's boundary meets an arc.
const HATCH: [(f64, f64); 28] = [
    (0.0, 0.0),
    (-26.8, 91.8),
    (-56.0, 181.3),
    (-87.5, 267.0),
    (-120.8, 348.8),
    (-155.6, 425.7),
    (-192.0, 496.8),
    (-228.8, 561.5),
    (-266.4, 619.3),
    (-304.0, 669.5),
    (-341.6, 712.0),
    (-378.8, 745.8),
    (-414.8, 771.3),
    (-449.9, 787.3),
    (-483.4, 794.7),
    (-514.9, 792.8),
    (-212.4, 1888.3),
    (-283.8, 1828.0),
    (-346.0, 1745.9),
    (-399.0, 1642.1),
    (-335.7, 1761.8),
    (-559.4, 1454.3),
    (-754.1, 1127.9),
    (-918.6, 785.3),
    (-1050.8, 428.8),
    (-1150.1, 61.9),
    (-1215.9, -312.5),
    (18.6, -72.3),
];

/// How far past `prim`'s own points (screen px) the triangles egui makes of what the canvas hands it reach, with any part of them
/// showing; infinite for a point that isn't a number.
fn reach(prim: &Prim) -> f32 {
    let pts: Vec<(f32, f32)> = match prim {
        Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect(),
        Prim::Line { pts, .. } => pts.clone(),
        Prim::Image { .. } => return 0.0,
    };
    let lo = pts.iter().fold(pos2(f32::INFINITY, f32::INFINITY), |a, q| a.min(pos2(q.0, q.1)));
    let hi = pts.iter().fold(pos2(f32::NEG_INFINITY, f32::NEG_INFINITY), |a, q| a.max(pos2(q.0, q.1)));
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
    mesh.vertices.iter().filter(|v| v.color.a() > 0).fold(0.0, |far, v| {
        let past = (lo.x - v.pos.x).max(v.pos.x - hi.x).max(lo.y - v.pos.y).max(v.pos.y - hi.y).max(0.0);
        if past.is_nan() { f32::INFINITY } else { far.max(past) }
    })
}

/// `pts` (mm) on screen in a plan `scale` px a mm, centred on them.
fn on_screen(pts: &[(f64, f64)], scale: f64) -> Vec<(f32, f32)> {
    let view = View { scale, cx: -400.0, cy: 800.0, width: 900, height: 700 };
    pts.iter().map(|q| view.px(P2(q.0, q.1))).collect()
}

#[test]
fn a_fill_that_turns_back_on_itself_stays_where_it_is_at_every_zoom() {
    // From a plan at 1 : 100 to one of a whole region: the hatch goes from a few hundred pixels to a few thousandths of one.
    for scale in [0.5, 0.05, 0.005, 0.001, 0.0005, 0.0002, 0.0001, 0.00005, 0.00002, 0.00001] {
        let fill = Prim::fill(vec![on_screen(&HATCH, scale)], Rgb(222, 205, 160));
        let far = reach(&fill);
        assert!(far <= 2.0, "at {scale} px a mm the hatch's fill reaches {far} px past it");
    }
}

#[test]
fn a_sliver_and_a_spur_are_filled_without_a_band_across_the_view() {
    // A sliver one degree wide, and a square with a spur that goes out and comes straight back, at any size.
    let sliver = [(0.0, 0.0), (10_000.0, 0.0), (0.0, 175.0)];
    let spur = [(0.0, 0.0), (1000.0, 0.0), (1000.0, 500.0), (3000.0, 500.0), (1000.0, 500.0001), (1000.0, 1000.0), (0.0, 1000.0)];
    for (name, ring) in [("sliver", &sliver[..]), ("spur", &spur[..])] {
        for scale in [0.1, 0.01, 0.001, 0.0001] {
            let far = reach(&Prim::fill(vec![on_screen(ring, scale)], Rgb(222, 205, 160)));
            assert!(far <= 2.0, "at {scale} px a mm the {name}'s fill reaches {far} px past it");
        }
    }
}

#[test]
fn a_closed_line_that_turns_back_on_itself_is_mitred_no_further_than_its_width() {
    for scale in [0.5, 0.05, 0.005, 0.0005, 0.00005] {
        for width in [0.0, 1.0, 3.0] {
            let line = Prim::line(on_screen(&HATCH, scale), width, Rgb(0, 0, 0), None, true);
            let far = reach(&line);
            assert!(far <= 2.0 + 2.0 * width.max(1.0), "at {scale} px a mm a closed line {width} px wide reaches {far} px past it");
        }
    }
}

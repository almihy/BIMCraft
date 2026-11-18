//! `view3d`'s tests that need only the model.
use crate::geom::{self, P2};
use crate::model::Id;
use crate::view3d::*;
use std::sync::Arc;

#[test]
fn background_coverage_includes_geometry_without_a_named_face() {
    let image = Image {
        width: 2,
        height: 1,
        rgba: vec![255; 8],
        hit: Arc::new(vec![0, u32::MAX]),
        depth: Arc::new(vec![1.0, f32::INFINITY]),
        owners: Arc::new(vec![Id::MAX]),
        faces: Arc::new(vec![""]),
        section_paint: None,
    };
    assert!(image.covered(0.0, 0.0));
    assert_eq!(image.face(0.0, 0.0), None);
    assert_eq!(image.pick(0.0, 0.0), None, "linked geometry is covered even though it cannot be picked");
    assert!(!image.covered(1.0, 0.0));
    assert!(!image.covered(-1.0, 0.0));
    assert!(!image.covered(2.0, 0.0));
}

#[test]
fn a_picture_draws_every_point_up_to_its_budget_and_an_even_share_past_it() {
    assert_eq!((every(0), every(1), every(DRAWN)), (1, 1, 1));
    assert_eq!((every(DRAWN + 1), every(2 * DRAWN), every(5 * DRAWN - 1)), (2, 2, 5));
    assert!((1..=6).all(|k| (k * DRAWN + 7).div_ceil(every(k * DRAWN + 7)) <= DRAWN));
}

#[test]
fn a_section_cuts_a_tube_in_its_ring_an_oblique_one_in_an_ellipse_and_leaves_other_meshes_whole() {
    use crate::runs::Geom;
    // A pipe 200 across along east at 500 up, from x = 0 to 4000, and a section 2000 along it, level and then at 45°.
    let g = Geom { a: [0.0, 0.0, 500.0], b: [4000.0, 0.0, 500.0], width: 200.0, depth: 200.0, round: true };
    let (tris, caps) = g.tube();
    let tube = |cut| Solid::Mesh { owner: 1, color: EDGE, tex: 0, tris: tris.clone(), caps, round: true, cut, vis: Vis::ALWAYS };
    let extent = |rings: &[Vec<P2>]| {
        let pts: Vec<P2> = rings.iter().flatten().copied().collect();
        let (lo, hi) = pts.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        });
        (hi.0 - lo.0, hi.1 - lo.1, lo.0, lo.1)
    };
    let square = tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 2000.0);
    assert_eq!(square.len(), 1);
    let (w, h, x0, z0) = extent(&square);
    assert!(
        (w - 200.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6 && (x0 - 900.0).abs() < 1e-6 && (z0 - 400.0).abs() < 1e-6,
        "{w} {h} {x0} {z0}"
    );
    // A regular 24-gon in a circle of radius 100: its area, and counter-clockwise.
    let area = 12.0 * 100.0 * 100.0 * (std::f64::consts::TAU / 24.0).sin();
    assert!((geom::area(&square[0]) - area).abs() < 1e-6 * area, "{}", geom::area(&square[0]));
    // Slanting, its ring is longer along the line by 1 / sin 45°, as high as before.
    let slanted = tube(true).cuts(P2(1000.0, -1000.0), P2(1.0, 1.0).unit(), 3000.0);
    let (w, h, ..) = extent(&slanted);
    assert!((w - 200.0 * 2f64.sqrt()).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
    // Past the section's end it is clipped; clear of the pipe, or not allowed to be cut, it leaves nothing.
    let (w, ..) = extent(&tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 1050.0));
    assert!((w - 150.0).abs() < 1e-6, "{w}");
    // A line that starts inside the pipe cuts what is past its start only.
    let (w, _, x0, _) = extent(&tube(true).cuts(P2(2000.0, -50.0), P2(0.0, 1.0), 2000.0));
    assert!((w - 150.0).abs() < 1e-6 && x0.abs() < 1e-6, "{w} {x0}");
    assert!(tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 500.0).is_empty());
    assert!(tube(true).cuts(P2(5000.0, -1000.0), P2(0.0, 1.0), 2000.0).is_empty());
    assert!(tube(false).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 2000.0).is_empty());
    // A plane that lies in a face of a convex mesh (vertices exactly in it, no edge crossing it) gives that face.
    let corner = |x: f64, y: f64, z: f64| [x, y, z];
    let (a, b, c, d) = (corner(0.0, -100.0, 0.0), corner(200.0, -100.0, 0.0), corner(200.0, 100.0, 0.0), corner(0.0, 100.0, 0.0));
    let (e, f, g, h) = (corner(0.0, -100.0, 200.0), corner(200.0, -100.0, 200.0), corner(200.0, 100.0, 200.0), corner(0.0, 100.0, 200.0));
    let tris = vec![
        [a, c, b],
        [a, d, c],
        [e, f, g],
        [e, g, h],
        [a, b, f],
        [a, f, e],
        [b, c, g],
        [b, g, f],
        [c, d, h],
        [c, h, g],
        [d, a, e],
        [d, e, h],
    ];
    let cube = Solid::Mesh { owner: 1, color: EDGE, tex: 0, tris, caps: 0, round: false, cut: true, vis: Vis::ALWAYS };
    let (w, h, ..) = extent(&cube.cuts(P2(0.0, 100.0), P2(1.0, 0.0), 1000.0));
    assert!((w - 200.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
    // A section along the pipe cuts it lengthwise: a strip as long as the pipe is and as high as it is round.
    let along = tube(true).cuts(P2(0.0, 0.0), P2(1.0, 0.0), 4000.0);
    let (w, h, ..) = extent(&along);
    assert!((w - 4000.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
}

//! `view3d`'s tests that need only the model.
use crate::geom::v3::{P3, dot, unit};
use crate::geom::{self, P2};
use crate::model::Id;
use crate::view3d::*;
use std::collections::BTreeSet;
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
fn coplanar_triangles_share_a_plane_and_degenerate_ones_each_their_own() {
    let tri = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, tex: 0, curve: None, vis: Vis::ALWAYS };
    // A face's triangles a micron off its plane, as a file rounds them, are one plane;
    // triangles with no area are each their own, and many of them stay quick.
    let mut tris: Vec<Tri> = (0..200)
        .map(|k| {
            let (x, e) = (f64::from(k) * 100.0, f64::from(k % 7) * 1e-4);
            tri([[x, 0.0, 1000.0 + e], [x + 100.0, 0.0, 1000.0], [x, 100.0, 1000.0 - e]])
        })
        .collect();
    tris.extend((0..20_000).map(|k| tri([[f64::from(k), 0.0, 0.0]; 3])));
    let started = std::time::Instant::now();
    let ids = planes(&tris);
    assert!(started.elapsed().as_secs_f64() < 10.0, "{:?}: a hang, not a slow machine", started.elapsed());
    assert!(ids.iter().take(200).all(|id| *id == ids[0]), "one plane");
    let degenerate: std::collections::BTreeSet<usize> = ids.iter().skip(200).copied().collect();
    assert_eq!(degenerate.len(), 20_000);
}

/// A cylinder of `n` sides standing on the origin, turned out: its `n` top triangles, its `n`
/// bottom ones, then its `2n` sides. The last corner of a ring is a hair off the first, as a
/// file's rounding leaves it. Four sides make a box.
fn cylinder(n: usize, r: f64, h: f64) -> Vec<[P3; 3]> {
    let ring = |z: f64, k: usize| {
        let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
        [r * c, r * s, z]
    };
    let (mut top, mut bottom, mut sides) = (Vec::new(), Vec::new(), Vec::new());
    for k in 0..n {
        let (a, b, c, d) = (ring(0.0, k), ring(0.0, k + 1), ring(h, k), ring(h, k + 1));
        top.push([[0.0, 0.0, h], c, d]);
        bottom.push([[0.0, 0.0, 0.0], b, a]);
        sides.extend([[a, b, d], [a, d, c]]);
    }
    [top, bottom, sides].concat()
}

#[test]
fn a_meshs_facets_that_turn_little_read_as_one_surface_and_its_creases_keep_their_edges() {
    let mesh = |owner, tris: Vec<[P3; 3]>| Solid::Mesh { owner, color: EDGE, tex: 0, caps: tris.len(), round: false, cut: false, tris, vis: Vis::ALWAYS };
    // Each cylinder's planes: its sides one surface where they turn 20° or less at their edges,
    // else a plane to each side; and a plane to each cap, however many triangles make it.
    for (n, planes_of) in [(4, 6), (8, 10), (16, 18), (20, 3), (24, 3), (64, 3)] {
        let mut out = Vec::new();
        mesh(1, cylinder(n, 200.0, 450.0)).tris(&mut out);
        let ids = planes(&out);
        let (top, bottom, sides) = (&ids[..n], &ids[n..2 * n], &ids[2 * n..]);
        assert!(top.iter().all(|id| *id == top[0]) && bottom.iter().all(|id| *id == bottom[0]), "{n} sides: a cap is one plane");
        assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), planes_of, "{n} sides: {ids:?}");
        assert!(top[0] != bottom[0] && !sides.contains(&top[0]) && !sides.contains(&bottom[0]), "{n} sides: the caps keep their edges");
    }
    // Two forms of one element, one on the other, each its own surface where their facets touch.
    let n = 24;
    let mut out = Vec::new();
    for _ in 0..2 {
        mesh(1, cylinder(n, 200.0, 450.0)).tris(&mut out);
    }
    let ids = planes(&out);
    assert_ne!(ids[2 * n], ids[6 * n], "{ids:?}");
    // A mesh as big as a file may bring stays quick.
    let started = std::time::Instant::now();
    let mut out = Vec::new();
    mesh(1, cylinder(25_000, 200.0, 450.0)).tris(&mut out);
    let (ids, normals) = (planes(&out), corner_normals(&out));
    assert!(started.elapsed().as_secs_f64() < 10.0, "{:?}: a hang, not a slow machine", started.elapsed());
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(normals.iter().flatten().count(), 50_000, "its sides, and not its caps");
    // Corners that aren't numbers, and triangles with no area, join nothing, and don't crash it.
    let odd = [[[f64::NAN; 3]; 3], [[f64::INFINITY; 3]; 3], [[1e300, 0.0, 0.0], [-1e300, 1e300, 0.0], [0.0; 3]], [[0.0; 3]; 3]];
    assert_eq!(facets(&odd), vec![None; 4]);
}

#[test]
fn a_curves_corners_take_the_normals_of_the_surface_they_are_on() {
    let n = 24;
    let mut out = Vec::new();
    Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: cylinder(n, 200.0, 450.0), vis: Vis::ALWAYS }.tris(&mut out);
    let normals = corner_normals(&out);
    // Its caps are flat faces: none. Each corner of its sides points straight out from the axis, the top
    // ones as the bottom ones (two triangles meet at some corners, one at others).
    assert!(normals[..2 * n].iter().all(Option::is_none));
    for (t, ns) in out[2 * n..].iter().zip(&normals[2 * n..]) {
        for (p, v) in t.p.iter().zip(ns.as_deref().unwrap()) {
            assert!(dot(*v, unit([p[0], p[1], 0.0])) > 1.0 - 1e-9, "{p:?} {v:?}");
        }
    }
    // A cone on it, a surface of its own: the ring they share keeps a step, the sides' corners
    // level and the cone's tilted up, its apex straight up.
    let ring = |k: usize| {
        let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
        [200.0 * c, 200.0 * s, 450.0]
    };
    let mut roofed = cylinder(n, 200.0, 450.0);
    roofed.extend((0..n).map(|k| [ring(k), ring(k + 1), [0.0, 0.0, 850.0]]));
    let mut out = Vec::new();
    Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: roofed, vis: Vis::ALWAYS }.tris(&mut out);
    let normals = corner_normals(&out);
    let normal = |i: usize| normals[i].as_deref().unwrap();
    assert!((2 * n..4 * n).all(|i| normal(i).iter().all(|v| v[2].abs() < 1e-9)));
    assert!((4 * n..5 * n).all(|i| normal(i)[0][2] > 0.4 && normal(i)[1][2] > 0.4 && normal(i)[2][2] > 1.0 - 1e-9));
    // Triangles with corners that aren't numbers, or no area, have none, and don't crash it.
    let odd = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, tex: 0, curve: Some((1, 0)), vis: Vis::ALWAYS };
    let odd = [
        odd([[f64::NAN; 3]; 3]),
        odd([[f64::INFINITY; 3]; 3]),
        odd([[0.0; 3]; 3]),
        odd([[1e300, 0.0, 0.0], [-1e300, 1e300, 0.0], [0.0; 3]]),
    ];
    assert!(corner_normals(&odd).iter().all(Option::is_none));
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

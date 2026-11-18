//! `solids`' tests that need only the model.
use crate::geom::v3::{P3, dot, unit};
use crate::prim::Rgb;
use crate::solids::*;
use std::collections::BTreeSet;

/// A triangle's colour: any will do.
const EDGE: Rgb = Rgb(48, 48, 48);

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
    let mesh = |owner, tris: Vec<[P3; 3]>| Solid::Mesh {
        owner,
        color: EDGE,
        tex: 0,
        caps: tris.len(),
        round: false,
        cut: false,
        tris,
        vis: Vis::ALWAYS,
    };
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
    Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: cylinder(n, 200.0, 450.0), vis: Vis::ALWAYS }
        .tris(&mut out);
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

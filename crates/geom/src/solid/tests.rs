#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The doorway's tests: the geometry bake-off's cases (G:/Software Development/geometry-bakeoff, harness/cases.json and
//! analytic.py) built through [`Solid3`], their volumes against the analytic ones, origins through booleans, and sections.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use super::*;

const HALF_PI: f64 = FRAC_PI_2;

fn rel(got: f64, want: f64) -> f64 {
    (got - want).abs() / want.abs()
}

fn org(element: u64) -> Origin {
    Origin::new(element, 0)
}

fn bx(x0: f64, y0: f64, z0: f64, x1: f64, y1: f64, z1: f64, o: Origin) -> Solid3 {
    Solid3::cuboid([x0, y0, z0], [x1, y1, z1], o).unwrap()
}

/// Segments so an arc of `ang` at radius `r` is no more than `tol` off its chords (the bake-off's rule).
fn arc_segments(r: f64, ang: f64, tol: f64) -> usize {
    let step = 2.0 * (1.0 - tol / r).acos();
    ((ang / step).ceil() as usize).max(1)
}

fn arc_pts(c: P2, r: f64, a0: f64, a1: f64, n: usize) -> Vec<P2> {
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f64 / n as f64;
            P2(c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// An annular sector about `c`, from `r0` to `r1` and `a0` to `a1`, from `z0` to `z1`; tagged curved when `curved`.
#[allow(clippy::too_many_arguments)]
fn sector(c: P2, r0: f64, r1: f64, a0: f64, a1: f64, z0: f64, z1: f64, o: Origin, curved: bool) -> Solid3 {
    let n = arc_segments(r1, a1 - a0, 1.0);
    let mut ring = arc_pts(c, r1, a0, a1, n);
    ring.extend(arc_pts(c, r0, a1, a0, n));
    let o = if curved { o.with_curve(Curve::band(c, r0, r1)) } else { o };
    Solid3::prism(&ring, Level::flat(z0), Level::flat(z1), o).unwrap()
}

fn all_closed(solids: &[Solid3]) {
    for (i, s) in solids.iter().enumerate() {
        assert!(!s.is_empty(), "solid {i} is empty");
        assert!(s.is_closed(), "solid {i} is not closed");
    }
}

fn total_volume(solids: &[Solid3]) -> f64 {
    solids.iter().map(Solid3::volume).sum()
}

// ----- the trial ------------------------------------------------------------------------------------------------

#[test]
fn solid_trial_box_minus_cylinder() {
    let t = solid_trial();
    // The 64-gon's area: n / 2 x r^2 x sin(2 pi / n).
    let ring = 32.0 * 25.0 * 25.0 * (TAU / 64.0).sin();
    let want = 1_000_000.0 - 100.0 * ring;
    assert!((t.volume - want).abs() < 1e-6, "volume {} against {want}", t.volume);
    assert!(t.triangles > 100, "{} triangles", t.triangles);
}

// ----- case 1: a curved wall with a door ------------------------------------------------------------------------------

const WALL: u64 = 1;
const DOOR: u64 = 2;
const HALF_DOOR: f64 = 900.0 / 6000.0 / 2.0;

fn curved_wall_door() -> Solid3 {
    let c = P2(0.0, 0.0);
    let wall = sector(c, 5850.0, 6150.0, 0.0, HALF_PI, 0.0, 3000.0, org(WALL), true);
    // The door's side planes are radial; it starts 10 mm below the floor, a clean through-cut.
    let door = sector(c, 5700.0, 6300.0, FRAC_PI_4 - HALF_DOOR, FRAC_PI_4 + HALF_DOOR, -10.0, 2100.0, org(DOOR), false);
    wall.difference(&door)
}

#[test]
fn case1_curved_wall_with_door_volume() {
    let s = curved_wall_door();
    let (r0, r1) = (5850.0, 6150.0);
    let wall = HALF_PI / 2.0 * (r1 * r1 - r0 * r0) * 3000.0;
    let door = 2.0 * HALF_DOOR / 2.0 * (r1 * r1 - r0 * r0) * 2100.0;
    let want = wall - door;
    // The harness: 0.5 % where an arc is tessellated.
    assert!(rel(s.volume(), want) < 0.005, "volume {} against {want}", s.volume());
    assert!((want - 7915300164.692).abs() < 1.0, "the analytic value is the harness's: {want}");
    all_closed(&[s]);
}

#[test]
fn case1_plan_slice_is_exact_arcs() {
    let s = curved_wall_door();
    let loops = s.slice(&Frame::plan(1200.0));
    // The door splits the wall in plan into two pieces.
    assert_eq!(loops.len(), 2, "{loops:#?}");
    let mut arcs = Vec::new();
    let mut lines = Vec::new();
    for l in &loops {
        assert!(!l.is_hole());
        for seg in &l.segs {
            match seg {
                Seg::Arc { centre, radius, start, end, ccw, origin } => arcs.push((*centre, *radius, *start, *end, *ccw, *origin)),
                Seg::Line { a, b, origin } => lines.push((*a, *b, *origin)),
            }
        }
        // Closed end to end.
        for (a, b) in l.segs.iter().zip(l.segs.iter().cycle().skip(1)) {
            assert!(a.end().dist(b.start()) < 1e-9, "{a:?} then {b:?}");
        }
    }
    // Four arcs: inner and outer, each split at the door.
    assert_eq!(arcs.len(), 4, "{arcs:#?}");
    assert_eq!(lines.len(), 4, "a jamb and an end of the wall for each piece: {lines:#?}");
    let (lo, hi) = (FRAC_PI_4 - HALF_DOOR, FRAC_PI_4 + HALF_DOOR);
    // Each arc wanted: (radius, from angle, to angle) in the order the wall goes round it ccw.
    let want = [(6150.0, 0.0, lo), (6150.0, hi, HALF_PI), (5850.0, 0.0, lo), (5850.0, hi, HALF_PI)];
    for (r, a0, a1) in want {
        let on = |p: P2, a: f64| p.dist(P2(r * a.cos(), r * a.sin())) < 1e-6;
        let found = arcs.iter().find(|(c, rad, s, e, ccw, _)| {
            let (s, e) = if *ccw { (*s, *e) } else { (*e, *s) };
            c.dist(P2(0.0, 0.0)) < 1e-9 && (rad - r).abs() < 1e-6 && on(s, a0) && on(e, a1)
        });
        assert!(found.is_some(), "no arc of {r} from {a0} to {a1} in {arcs:#?}");
    }
    for (c, r, s, e, _, origin) in &arcs {
        assert_eq!(origin.element, WALL);
        assert!((s.dist(*c) - r).abs() < 1e-6 && (e.dist(*c) - r).abs() < 1e-6, "ends on the circle: {s:?} {e:?}");
    }
    // The outer arcs run counter-clockwise (the wall is on their left), the inner ones clockwise.
    for (_, r, _, _, ccw, _) in &arcs {
        assert_eq!(*ccw, *r > 6000.0);
    }
    // The door's jambs are the door's; the wall's ends and the rest are the wall's.
    let door_lines: Vec<_> = lines.iter().filter(|l| l.2.element == DOOR).collect();
    assert_eq!(door_lines.len(), 2, "{lines:#?}");
    // The bake-off's jamb point (4010.634, 4662.329) at the outer radius, and its mirror.
    let outer_jamb = P2(6150.0 * hi.cos(), 6150.0 * hi.sin());
    assert!((outer_jamb.0 - 4010.634).abs() < 1e-3 && (outer_jamb.1 - 4662.329).abs() < 1e-3, "{outer_jamb:?}");
    let touches = |p: P2| door_lines.iter().any(|l| l.0.dist(p) < 1e-6 || l.1.dist(p) < 1e-6);
    assert!(touches(outer_jamb), "{door_lines:#?}");
    assert!(touches(P2(outer_jamb.1, outer_jamb.0)), "{door_lines:#?}");
    // Jambs are straight radial lines between the circles.
    for l in &door_lines {
        let (d0, d1) = (l.0.dist(P2(0.0, 0.0)), l.1.dist(P2(0.0, 0.0)));
        assert!((d0.min(d1) - 5850.0).abs() < 1e-6 && (d0.max(d1) - 6150.0).abs() < 1e-6, "{l:?}");
    }
    // The area, arcs and all, is the sector area exactly.
    let area: f64 = loops.iter().map(Loop::area).sum();
    let want = (HALF_PI - 2.0 * HALF_DOOR) / 2.0 * (6150.0f64.powi(2) - 5850.0f64.powi(2));
    assert!(rel(area, want) < 1e-9, "area {area} against {want}");
}

#[test]
fn case1_slice_through_the_door_head_keeps_facets_as_lines_in_a_vertical_plane() {
    // A vertical plane through the centre along 45 degrees goes through the door: radial, so lines only.
    let s = curved_wall_door();
    let f = Frame::vertical(P2(0.0, 0.0), P2(1.0, 1.0)).unwrap();
    let loops = s.slice(&f);
    assert!(!loops.is_empty());
    assert!(loops.iter().flat_map(|l| &l.segs).all(|g| matches!(g, Seg::Line { .. })));
    // The lintel above the door: from r 5850 to 6150 (300 across) and 2100 to 3000 up.
    let area: f64 = loops.iter().map(Loop::area).sum();
    assert!(rel(area, 300.0 * 900.0) < 0.01, "lintel area {area}");
}

// ----- case 2: walls trimmed to a sloped roof, by intersection --------------------------------------------------------

#[test]
fn case2_walls_trimmed_by_a_sloped_plane() {
    let (w, d, t, wh) = (10000.0, 8000.0, 250.0, 6000.0);
    let h = t / 2.0;
    let tan = 30.0f64.to_radians().tan();
    // Everything under z = 3000 + (x + 125) tan 30: a big prism whose top slopes.
    let under = Solid3::prism(
        &[P2(-1000.0, -1000.0), P2(11000.0, -1000.0), P2(11000.0, 9000.0), P2(-1000.0, 9000.0)],
        Level::flat(-1.0),
        Level { z: 3000.0 + 125.0 * tan, g: P2(tan, 0.0) },
        org(99),
    )
    .unwrap();
    let walls = [
        bx(-h, -h, 0.0, w + h, h, wh, org(1)),
        bx(-h, d - h, 0.0, w + h, d + h, wh, org(2)),
        bx(-h, h, 0.0, h, d - h, wh, org(3)),
        bx(w - h, h, 0.0, w + h, d - h, wh, org(4)),
    ];
    let trimmed: Vec<Solid3> = walls.iter().map(|wl| wl.intersection(&under)).collect();
    all_closed(&trimmed);
    // analytic.py roof_walls_volume.
    let trim = |x0: f64, x1: f64| {
        let f = |x: f64| 3000.0 + (x + 125.0) * tan;
        let xc = (-125.0 + (wh - 3000.0) / tan).clamp(x0, x1);
        (f(x0) + f(xc)) / 2.0 * (xc - x0) + wh * (x1 - xc)
    };
    let want = 2.0 * t * trim(-h, w + h) + (d - t) * (trim(-h, h) + trim(w - h, w + h));
    assert!(rel(total_volume(&trimmed), want) < 1e-4, "{} against {want}", total_volume(&trimmed));
    // Origins: a trimmed wall's triangles are the wall's or the roof plane's.
    for (k, s) in trimmed.iter().enumerate() {
        for tri in s.triangles() {
            assert!(tri.origin.element == k as u64 + 1 || tri.origin.element == 99, "{:?}", tri.origin);
        }
    }
}

// ----- case 3: a slab with a shaft and three sleeves ------------------------------------------------------------------

#[test]
fn case3_slab_with_shaft_and_sleeves() {
    let slab = bx(0.0, 0.0, 0.0, 20000.0, 15000.0, 250.0, org(1));
    let shaft = bx(4000.0, 2500.0, -10.0, 6000.0, 5500.0, 260.0, org(2));
    let segs = arc_segments(200.0, TAU, 1.0);
    let sleeves: Vec<Solid3> = [12000.0, 13000.0, 14000.0]
        .iter()
        .enumerate()
        .map(|(i, x)| Solid3::cylinder(P2(*x, 10000.0), 200.0, -10.0, 260.0, segs, org(3 + i as u64)).unwrap())
        .collect();
    let cut = slab.difference_all(std::iter::once(&shaft).chain(sleeves.iter()));
    let want = 20000.0 * 15000.0 * 250.0 - 2000.0 * 3000.0 * 250.0 - 3.0 * PI * 200.0 * 200.0 * 250.0;
    assert!((want - 73405752220.392).abs() < 1.0);
    // The harness: 0.05 % where round sleeves are tessellated.
    assert!(rel(cut.volume(), want) < 0.0005, "{} against {want}", cut.volume());
    all_closed(&[cut.clone()]);
    assert_eq!(cut.bounds().unwrap(), Bounds3 { min: [0.0, 0.0, 0.0], max: [20000.0, 15000.0, 250.0] });

    // The plan: the slab's outline, the shaft's hole, three round holes each two exact arcs.
    let loops = cut.slice(&Frame::plan(125.0));
    assert_eq!(loops.len(), 5, "{loops:#?}");
    assert_eq!(loops.iter().filter(|l| l.is_hole()).count(), 4);
    let arcs: Vec<&Seg> = loops.iter().flat_map(|l| &l.segs).filter(|g| matches!(g, Seg::Arc { .. })).collect();
    assert_eq!(arcs.len(), 6);
    for a in &arcs {
        let Seg::Arc { centre, radius, start, end, ccw, origin } = a else { unreachable!() };
        assert!((radius - 200.0).abs() < 1e-9 && !ccw, "a hole's arcs run clockwise");
        assert!(start.dist(*centre) - 200.0 < 1e-6 && end.dist(*centre) - 200.0 < 1e-6);
        assert!((3..=5).contains(&origin.element), "the sleeve's: {origin:?}");
        assert!((centre.1 - 10000.0).abs() < 1e-9);
    }
    // Exactly the plan's area, arcs exact: the circle's pi r^2, not the 32-gon's.
    let area: f64 = loops.iter().map(Loop::area).sum();
    let want = 20000.0 * 15000.0 - 2000.0 * 3000.0 - 3.0 * PI * 200.0 * 200.0;
    assert!(rel(area, want) < 1e-9, "area {area} against {want}");
}

// ----- case 4: layered wall joins ---------------------------------------------------------------------------------------

/// One layered wall (core 200, finish 50) running along x (`x_dir`) or y: `c` is its centreline coordinate across, `a0..a1`
/// along it. Finish on the left of the direction of travel: +y for a wall along +x, -x for a wall along +y. The layers are
/// layers 0 and 1 of element `id`.
fn layered_wall(id: u64, x_dir: bool, a0: f64, a1: f64, c: f64, z: f64) -> Solid3 {
    let b = |lo: f64, hi: f64, layer: u16| {
        let o = Origin::new(id, layer);
        if x_dir { bx(a0, lo, 0.0, a1, hi, z, o) } else { bx(lo, a0, 0.0, hi, a1, z, o) }
    };
    let (core, fin) = if x_dir { (b(c - 125.0, c + 75.0, 0), b(c + 75.0, c + 125.0, 1)) } else { (b(c - 75.0, c + 125.0, 0), b(c - 125.0, c - 75.0, 1)) };
    core.union(&fin)
}

#[test]
fn case4_layered_joins_are_one_closed_solid_each() {
    let z = 3000.0;
    let l = layered_wall(1, true, 0.0, 4125.0, 0.0, z).union(&layered_wall(2, false, -125.0, 4000.0, 4000.0, z));
    let t = layered_wall(3, true, 10000.0, 18000.0, 0.0, z).union(&layered_wall(4, false, -125.0, 4000.0, 14000.0, z));
    let x = layered_wall(5, true, 22000.0, 30000.0, 0.0, z).union(&layered_wall(6, false, -4000.0, 4000.0, 26000.0, z));
    let joins = [l, t, x];
    all_closed(&joins);
    // analytic.py: 26,718,750,000 mm3 in all, 0.01 %.
    assert!(rel(total_volume(&joins), 26_718_750_000.0) < 1e-4, "{}", total_volume(&joins));
    // Every triangle is one of the walls' layers.
    for (j, s) in joins.iter().enumerate() {
        let ids: std::collections::BTreeSet<(u64, u16)> = s.triangles().iter().map(|t| (t.origin.element, t.origin.layer)).collect();
        assert!(ids.iter().all(|(e, _)| *e >= 2 * j as u64 + 1 && *e <= 2 * j as u64 + 2), "{ids:?}");
        assert!(ids.contains(&(2 * j as u64 + 1, 0)) && ids.contains(&(2 * j as u64 + 1, 1)), "both layers show: {ids:?}");
    }
    // The L's plan: one outline of straight edges, each the layer it came from, the corner square.
    let loops = joins[0].slice(&Frame::plan(1500.0));
    assert_eq!(loops.len(), 1);
    assert!(loops[0].segs.iter().all(|g| matches!(g, Seg::Line { .. })));
    let l_area = 4125.0 * 250.0 + (4000.0 - 125.0) * 250.0;
    assert!(rel(loops[0].area(), l_area) < 1e-9, "{} against {l_area}", loops[0].area());
}

// ----- case 5: a curtain wall -------------------------------------------------------------------------------------------

#[test]
fn case5_curtain_walls_straight_and_on_an_arc_are_fifty_solids() {
    let (len, ht, gx, gz, mw, md, gt) = (12000.0, 9000.0, 1500.0, 3000.0, 50.0, 150.0, 24.0);
    let (nx, nz) = (8usize, 3usize);
    let (hm, hg) = (mw / 2.0, gt / 2.0);
    let mullion = |i: u64| org(1000 + i);
    // Straight: nine verticals and four horizontals, one solid; 24 glass panels.
    let mut parts = Vec::new();
    for i in 0..=nx {
        let x = i as f64 * gx;
        parts.push(bx(x - hm, -md / 2.0, -hm, x + hm, md / 2.0, ht + hm, mullion(i as u64)));
    }
    for j in 0..=nz {
        let z = j as f64 * gz;
        parts.push(bx(-hm, -md / 2.0, z - hm, len + hm, md / 2.0, z + hm, mullion(100 + j as u64)));
    }
    let straight = Solid3::union_all(&parts);
    let mut solids = vec![straight.clone()];
    let mut glass_volume = 0.0;
    for i in 0..nx {
        for j in 0..nz {
            let (x, z) = (i as f64 * gx, j as f64 * gz);
            let g = bx(x + hm, -hg, z + hm, x + gx - hm, hg, z + gz - hm, org(2000 + (i * nz + j) as u64));
            glass_volume += g.volume();
            solids.push(g);
        }
    }
    // The harness's per-wall parts, planar so 0.01 %.
    assert!(rel(straight.volume(), 958_875_000.0) < 1e-4, "{}", straight.volume());
    assert!(rel(glass_volume, 2_463_840_000.0) < 1e-4, "{glass_volume}");
    // Every mullion box shows in the union's triangles.
    let ids: std::collections::BTreeSet<u64> = straight.triangles().iter().map(|t| t.origin.element).collect();
    assert_eq!(ids.len(), 13, "{ids:?}");
    assert!(!ids.contains(&u64::MAX));

    // On the arc: centre (6000, 25000), R 15000 to the mullion centres, start -1.9707963 rad, 0.1 rad a bay.
    let (cen, r) = (P2(6000.0, 25000.0), 15000.0);
    let (a_start, span, da) = (-FRAC_PI_2 - 0.4, 0.8, 0.1);
    let pad = hm / r;
    let mut ms = Vec::new();
    for i in 0..=nx {
        let a = a_start + i as f64 * da;
        let m = Matrix::rotation_z(a).then(&Matrix::translation([cen.0, cen.1, 0.0]));
        ms.push(Solid3::cuboid_transformed([r - md / 2.0, -hm, -hm], [r + md / 2.0, hm, ht + hm], &m, mullion(200 + i as u64)).unwrap());
    }
    for j in 0..=nz {
        let z = j as f64 * gz;
        ms.push(sector(cen, r - md / 2.0, r + md / 2.0, a_start - pad, a_start + span + pad, z - hm, z + hm, mullion(300 + j as u64), true));
    }
    let arc_mullions = Solid3::union_all(&ms);
    solids.push(arc_mullions.clone());
    let mut arc_glass = 0.0;
    for i in 0..nx {
        let (a0, a1) = (a_start + i as f64 * da + pad, a_start + (i + 1) as f64 * da - pad);
        for j in 0..nz {
            let z = j as f64 * gz;
            let g = sector(cen, r - hg, r + hg, a0, a1, z + hm, z + gz - hm, org(4000 + (i * nz + j) as u64), true);
            arc_glass += g.volume();
            solids.push(g);
        }
    }
    // The harness counts 52 solids; 2 + 2 x 24 is 50.
    assert_eq!(solids.len(), 50);
    all_closed(&solids);
    // The arc wall's parts, 0.5 % (tessellated; the harness takes the same volume as the straight wall's).
    assert!(rel(arc_mullions.volume(), 958_875_000.0) < 0.005, "{}", arc_mullions.volume());
    assert!(rel(arc_glass, 2_463_840_000.0) < 0.005, "{arc_glass}");
    assert!(rel(total_volume(&solids), 6_845_430_000.0) < 0.005, "{}", total_volume(&solids));
}

// ----- case 6: a tower (three storeys; the harness has twenty) ------------------------------------------------------------

#[test]
fn case6_tower_three_storeys() {
    let storeys = 3;
    let mut solids = Vec::new();
    let mut id = 0u64;
    let mut next = || {
        id += 1;
        org(id)
    };
    let ys = [-13000.0, -9000.0, -5000.0, 5000.0, 9000.0, 13000.0];
    for s in 0..storeys {
        let z0 = s as f64 * 3500.0;
        let slab = bx(-15000.0, -15000.0, z0, 15000.0, 15000.0, z0 + 250.0, next());
        let core = bx(-3000.0, -4000.0, z0 - 10.0, 3000.0, 4000.0, z0 + 260.0, next());
        solids.push(slab.difference(&core));
        let (zb, zt) = (z0 + 250.0, z0 + 3500.0);
        let perimeter = [
            (bx(-15000.0, -15000.0, zb, 15000.0, -14700.0, zt, next()), true, -15000.0, -14700.0),
            (bx(-15000.0, 14700.0, zb, 15000.0, 15000.0, zt, next()), true, 14700.0, 15000.0),
            (bx(-15000.0, -14700.0, zb, -14700.0, 14700.0, zt, next()), false, -15000.0, -14700.0),
            (bx(14700.0, -14700.0, zb, 15000.0, 14700.0, zt, next()), false, 14700.0, 15000.0),
        ];
        for (wall, x_wall, c0, c1) in perimeter {
            let windows: Vec<Solid3> = (0..12)
                .map(|k| {
                    let c = (k as f64 - 5.5) * 2400.0;
                    let (zl, zh) = (zb + 900.0, zb + 900.0 + 1500.0);
                    if x_wall {
                        bx(c - 900.0, c0 - 50.0, zl, c + 900.0, c1 + 50.0, zh, next())
                    } else {
                        bx(c0 - 50.0, c - 900.0, zl, c1 + 50.0, c + 900.0, zh, next())
                    }
                })
                .collect();
            solids.push(wall.difference_all(&windows));
        }
        for i in 0..6 {
            for &cy in &ys {
                let cx = -12500.0 + 5000.0 * i as f64;
                let wall = bx(cx - 2000.0, cy - 75.0, zb, cx + 2000.0, cy + 75.0, zt, next());
                let door = bx(cx - 450.0, cy - 125.0, zb - 10.0, cx + 450.0, cy + 125.0, zb + 2100.0, next());
                solids.push(wall.difference(&door));
            }
        }
    }
    assert_eq!(solids.len(), storeys * 41);
    all_closed(&solids);
    // analytic.py tower().
    let (slab_t, wall_h) = (250.0, 3250.0);
    let slab = 30000.0 * 30000.0 * slab_t - 6000.0 * 8000.0 * slab_t;
    let perimeter = 4.0 * (30000.0 - 300.0) * 300.0 * wall_h - 4.0 * 12.0 * 1800.0 * 1500.0 * 300.0;
    let interior = 36.0 * (4000.0 * 150.0 * wall_h - 900.0 * 2100.0 * 150.0);
    let want = storeys as f64 * (slab + perimeter + interior);
    assert!(rel(total_volume(&solids), want) < 1e-4, "{} against {want}", total_volume(&solids));
}

// ----- origins -----------------------------------------------------------------------------------------------------------

#[test]
fn origins_survive_a_wall_minus_a_door() {
    let wall = bx(0.0, 0.0, 0.0, 4000.0, 300.0, 3000.0, org(10));
    let door = bx(1000.0, -50.0, -10.0, 1900.0, 350.0, 2100.0, org(20));
    let cut = wall.difference(&door);
    let tris = cut.triangles();
    assert!(!tris.is_empty());
    for t in &tris {
        assert!(t.origin.element == 10 || t.origin.element == 20, "{:?}", t.origin);
        let len = (t.normal[0].powi(2) + t.normal[1].powi(2) + t.normal[2].powi(2)).sqrt();
        assert!((len - 1.0).abs() < 1e-9, "unit normal");
    }
    // The door's jamb faces are the door's, facing into the opening (+x at x = 1000, -x at x = 1900).
    let jambs: Vec<&Tri> = tris.iter().filter(|t| t.origin.element == 20).collect();
    assert!(!jambs.is_empty());
    assert!(jambs.iter().any(|t| t.normal[0] > 0.99 && t.p.iter().all(|p| (p[0] - 1000.0).abs() < 1e-9)));
    assert!(jambs.iter().any(|t| t.normal[0] < -0.99 && t.p.iter().all(|p| (p[0] - 1900.0).abs() < 1e-9)));
    assert!(tris.iter().any(|t| t.origin.element == 10));
}

#[test]
fn origins_survive_any_chain() {
    let a = bx(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0, Origin::new(1, 2));
    let b = bx(500.0, 500.0, 500.0, 1500.0, 1500.0, 1500.0, Origin::new(2, 0));
    let c = bx(-100.0, 200.0, 200.0, 2000.0, 300.0, 300.0, Origin::new(3, 1));
    let s = a.union(&b).difference(&c).translate([10.0, 20.0, 30.0]).intersection(&bx(0.0, 0.0, 0.0, 1200.0, 1200.0, 1200.0, org(4)));
    let tris = s.triangles();
    assert!(!tris.is_empty());
    let seen: std::collections::BTreeSet<(u64, u16)> = tris.iter().map(|t| (t.origin.element, t.origin.layer)).collect();
    assert!(seen.iter().all(|(e, _)| (1..=4).contains(e)), "{seen:?}");
    // The layer travels with the element.
    assert!(seen.contains(&(1, 2)) && !seen.contains(&(1, 0)), "{seen:?}");
    assert!(seen.contains(&(3, 1)), "the cut's faces: {seen:?}");
    // A solid transformed again, then cut: still traced.
    let t = s.transform(&Matrix::rotation_z(0.7)).difference(&bx(-5000.0, -5000.0, 100.0, 5000.0, 5000.0, 200.0, org(5)));
    assert!(t.triangles().iter().all(|t| (1..=5).contains(&t.origin.element)));
    assert!(t.triangles().iter().any(|t| t.origin.element == 5));
    assert!(t.is_closed());
}

#[test]
fn curve_tags_follow_moves_and_turns() {
    let col = Solid3::cylinder(P2(100.0, 50.0), 200.0, 0.0, 3000.0, 48, org(7)).unwrap();
    let centre_of = |s: &Solid3| {
        let loops = s.slice(&Frame::plan(1000.0));
        assert_eq!(loops.len(), 1);
        let arcs: Vec<(P2, f64)> = loops[0].segs.iter().filter_map(|g| if let Seg::Arc { centre, radius, .. } = g { Some((*centre, *radius)) } else { None }).collect();
        assert_eq!(arcs.len(), 2, "{:?}", loops[0]);
        arcs[0]
    };
    let (c, r) = centre_of(&col);
    assert!(c.dist(P2(100.0, 50.0)) < 1e-9 && (r - 200.0).abs() < 1e-9);
    let (c, r) = centre_of(&col.translate([1000.0, -500.0, 0.0]));
    assert!(c.dist(P2(1100.0, -450.0)) < 1e-9 && (r - 200.0).abs() < 1e-9);
    let a = 0.6f64;
    let (c, _) = centre_of(&col.transform(&Matrix::rotation_z(a)));
    let want = P2(100.0 * a.cos() - 50.0 * a.sin(), 100.0 * a.sin() + 50.0 * a.cos());
    assert!(c.dist(want) < 1e-9, "{c:?} against {want:?}");
    // Scaled alike, the radius follows; scaled unalike the tag is gone and the faces are lines.
    let (_, r) = centre_of(&col.transform(&Matrix::scale([2.0, 2.0, 1.0])));
    assert!((r - 400.0).abs() < 1e-9);
    let loops = col.transform(&Matrix::scale([2.0, 1.0, 1.0])).slice(&Frame::plan(1000.0));
    assert!(loops[0].segs.iter().all(|g| matches!(g, Seg::Line { .. })));
}

// ----- sections -----------------------------------------------------------------------------------------------------------

#[test]
fn vertical_section_through_a_box_is_its_rectangle() {
    let b = bx(0.0, 0.0, 0.0, 4000.0, 300.0, 3000.0, org(1));
    let loops = b.slice(&Frame::vertical(P2(0.0, 150.0), P2(1.0, 0.0)).unwrap());
    assert_eq!(loops.len(), 1);
    let l = &loops[0];
    assert_eq!(l.segs.len(), 4, "{l:?}");
    assert!(rel(l.area(), 4000.0 * 3000.0) < 1e-12);
    let corners: Vec<P2> = l.segs.iter().map(Seg::start).collect();
    for p in [P2(0.0, 0.0), P2(4000.0, 0.0), P2(4000.0, 3000.0), P2(0.0, 3000.0)] {
        assert!(corners.iter().any(|c| c.dist(p) < 1e-9), "{p:?} in {corners:?}");
    }
    assert!(l.segs.iter().all(|g| g.origin().element == 1));
}

#[test]
fn vertical_section_through_a_sloped_top_prism_shows_the_slope() {
    let ring = [P2(0.0, 0.0), P2(10000.0, 0.0), P2(10000.0, 4000.0), P2(0.0, 4000.0)];
    let top = Level { z: 3000.0, g: P2(0.1, 0.0) };
    let p = Solid3::prism(&ring, Level::flat(0.0), top, org(1)).unwrap();
    assert!(p.is_closed());
    assert!(rel(p.volume(), 4000.0 * (3000.0 * 10000.0 + 0.1 * 10000.0 * 10000.0 / 2.0)) < 1e-12);
    let loops = p.slice(&Frame::vertical(P2(0.0, 2000.0), P2(1.0, 0.0)).unwrap());
    assert_eq!(loops.len(), 1);
    let l = &loops[0];
    assert_eq!(l.segs.len(), 4, "{l:?}");
    let want = [P2(0.0, 0.0), P2(10000.0, 0.0), P2(10000.0, 4000.0), P2(0.0, 3000.0)];
    for w in want {
        assert!(l.segs.iter().any(|g| g.start().dist(w) < 1e-9), "{w:?} in {l:?}");
    }
    assert!(rel(l.area(), 10000.0 * 3500.0) < 1e-12);
    // The sloped edge is the one from (10000, 4000) to (0, 3000).
    assert!(l.segs.iter().any(|g| matches!(g, Seg::Line { a, b, .. } if a.dist(P2(10000.0, 4000.0)) < 1e-9 && b.dist(P2(0.0, 3000.0)) < 1e-9)));
    // A slanted prism with a sloped bottom too (a roof slab): thickness 300 down the slope.
    let slab = Solid3::prism(&ring, Level { z: 3000.0, g: P2(0.1, 0.0) }, Level { z: 3300.0, g: P2(0.1, 0.0) }, org(2)).unwrap();
    assert!(rel(slab.volume(), 10000.0 * 4000.0 * 300.0) < 1e-12);
}

#[test]
fn oblique_section_through_a_cube() {
    let cube = bx(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0, org(1));
    // The plane y + z = 1000: a rectangle 1000 along x by 1000 sqrt 2 up the slope.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let f = Frame::new([0.0, 500.0, 500.0], [1.0, 0.0, 0.0], [0.0, -s, s]).unwrap();
    let loops = cube.slice(&f);
    assert_eq!(loops.len(), 1, "{loops:?}");
    assert!(rel(loops[0].area(), 1000.0 * 1000.0 * 2.0f64.sqrt()) < 1e-9, "{}", loops[0].area());
    assert_eq!(loops[0].segs.len(), 4);
}

#[test]
fn a_plane_through_a_face_cuts_behind_itself() {
    let cube = bx(0.0, 0.0, 0.0, 1000.0, 1000.0, 1000.0, org(1));
    // At its top, seen from above: the solid is behind the plane, so it is cut at its face.
    assert_eq!(cube.slice(&Frame::plan(1000.0)).len(), 1);
    // At its bottom: the solid is in front of it, so it is not.
    assert!(cube.slice(&Frame::plan(0.0)).is_empty());
    // Missing it altogether.
    assert!(cube.slice(&Frame::plan(5000.0)).is_empty());
    assert!(Solid3::empty().slice(&Frame::plan(0.0)).is_empty());
}

// ----- meshes ---------------------------------------------------------------------------------------------------------------

/// A box's twelve triangles as a soup, counter-clockwise from outside.
fn cube_soup(s: f64) -> Vec<[P3; 3]> {
    let p = |x: f64, y: f64, z: f64| [x * s, y * s, z * s];
    let q = |a: P3, b: P3, c: P3, d: P3| [[a, b, c], [a, c, d]];
    let mut t = Vec::new();
    t.extend(q(p(0., 0., 0.), p(0., 1., 0.), p(1., 1., 0.), p(1., 0., 0.))); // bottom
    t.extend(q(p(0., 0., 1.), p(1., 0., 1.), p(1., 1., 1.), p(0., 1., 1.))); // top
    t.extend(q(p(0., 0., 0.), p(1., 0., 0.), p(1., 0., 1.), p(0., 0., 1.))); // y = 0
    t.extend(q(p(1., 0., 0.), p(1., 1., 0.), p(1., 1., 1.), p(1., 0., 1.))); // x = 1
    t.extend(q(p(1., 1., 0.), p(0., 1., 0.), p(0., 1., 1.), p(1., 1., 1.))); // y = 1
    t.extend(q(p(0., 1., 0.), p(0., 0., 0.), p(0., 0., 1.), p(0., 1., 1.))); // x = 0
    t
}

#[test]
fn a_closed_mesh_becomes_a_solid() {
    let soup = cube_soup(1000.0);
    let s = Solid3::from_triangles(&soup, org(5)).unwrap();
    assert!(rel(s.volume(), 1e9) < 1e-12);
    assert!(s.is_closed());
    // Corners a hair apart are welded; a mesh wound inside out is turned over.
    let noisy: Vec<[P3; 3]> = soup.iter().enumerate().map(|(i, t)| t.map(|p| [p[0] + (i % 3) as f64 * 1e-5, p[1], p[2]])).collect();
    assert!(rel(Solid3::from_triangles(&noisy, org(5)).unwrap().volume(), 1e9) < 1e-6);
    let inside_out: Vec<[P3; 3]> = soup.iter().map(|t| [t[0], t[2], t[1]]).collect();
    assert!(rel(Solid3::from_triangles(&inside_out, org(5)).unwrap().volume(), 1e9) < 1e-12);
    // And it cuts and keeps its origin.
    let cut = s.difference(&bx(400.0, 400.0, -10.0, 600.0, 600.0, 1010.0, org(6)));
    assert!(rel(cut.volume(), 1e9 - 200.0 * 200.0 * 1000.0) < 1e-12);
    assert!(cut.triangles().iter().all(|t| t.origin.element == 5 || t.origin.element == 6));
}

#[test]
fn a_mesh_that_is_not_closed_is_refused_with_why() {
    let mut soup = cube_soup(1000.0);
    soup.pop();
    let err = Solid3::from_triangles(&soup, org(1)).unwrap_err();
    assert_eq!(err, SolidError::NotClosed { open_edges: 3, non_manifold_edges: 0, triangles: 11 });
    let msg = err.to_string();
    assert!(msg.contains("not a closed solid") && msg.contains("3 open edge"), "{msg}");
    // Two copies of the same triangle: edges used three times.
    let mut doubled = cube_soup(1000.0);
    doubled.push(doubled[0]);
    let err = Solid3::from_triangles(&doubled, org(1)).unwrap_err();
    match err {
        SolidError::NotClosed { non_manifold_edges, .. } => assert!(non_manifold_edges > 0),
        other => panic!("{other:?}"),
    }
    // Nonsense in: refused, never a panic.
    assert!(Solid3::from_triangles(&[], org(1)).is_err());
    assert!(Solid3::from_triangles(&[[[f64::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]], org(1)).is_err());
    assert!(Solid3::from_triangles(&[[[0.0; 3], [0.0; 3], [0.0; 3]]], org(1)).is_err());
}

#[test]
fn bad_inputs_are_refused() {
    let ring = [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 1000.0), P2(0.0, 1000.0)];
    assert!(Solid3::prism(&ring[..2], Level::flat(0.0), Level::flat(100.0), org(1)).is_err());
    assert!(Solid3::prism(&ring, Level::flat(100.0), Level::flat(0.0), org(1)).is_err(), "top below bottom");
    assert!(Solid3::prism(&ring, Level::flat(0.0), Level::flat(0.0), org(1)).is_err(), "no thickness");
    let bowtie = [P2(0.0, 0.0), P2(1000.0, 1000.0), P2(1000.0, 0.0), P2(0.0, 1000.0)];
    assert!(Solid3::prism(&bowtie, Level::flat(0.0), Level::flat(100.0), org(1)).is_err(), "crosses itself");
    assert!(Solid3::cuboid([0.0; 3], [0.0, 1.0, 1.0], org(1)).is_err());
    assert!(Solid3::cuboid([f64::NAN, 0.0, 0.0], [1.0; 3], org(1)).is_err());
    assert!(Solid3::cylinder(P2(0.0, 0.0), 0.0, 0.0, 1.0, 8, org(1)).is_err());
    // A clockwise ring, a repeated corner and a corner on a straight edge are fine.
    let messy = [P2(0.0, 0.0), P2(0.0, 1000.0), P2(0.0, 1000.0), P2(500.0, 1000.0), P2(1000.0, 1000.0), P2(1000.0, 0.0)];
    let s = Solid3::prism(&messy, Level::flat(0.0), Level::flat(100.0), org(1)).unwrap();
    assert!(rel(s.volume(), 1000.0 * 1000.0 * 100.0) < 1e-12);
    // A wedge: the top meets the bottom along one edge.
    let wedge = Solid3::prism(&ring, Level::flat(0.0), Level { z: 0.0, g: P2(0.1, 0.0) }, org(1)).unwrap();
    assert!(wedge.is_closed());
    assert!(rel(wedge.volume(), 1000.0 * 0.1 * 1000.0 * 1000.0 / 2.0) < 1e-9, "{}", wedge.volume());
}

#[test]
fn batch_booleans_and_queries() {
    let boxes: Vec<Solid3> = (0..4).map(|i| bx(i as f64 * 500.0, 0.0, 0.0, i as f64 * 500.0 + 1000.0, 1000.0, 1000.0, org(i + 1))).collect();
    let u = Solid3::union_all(&boxes);
    assert!(rel(u.volume(), 2500.0 * 1000.0 * 1000.0) < 1e-12);
    assert_eq!(u.bounds().unwrap(), Bounds3 { min: [0.0; 3], max: [2500.0, 1000.0, 1000.0] });
    let i = Solid3::intersection_all(&boxes);
    assert!(i.is_empty(), "the first and the last do not touch");
    let i = Solid3::intersection_all(&boxes[..2]);
    assert!(rel(i.volume(), 500.0 * 1000.0 * 1000.0) < 1e-12, "{}", i.volume());
    assert!(rel(u.surface_area(), 2.0 * (2500.0 * 1000.0 * 2.0 + 1000.0 * 1000.0)) < 1e-12, "{}", u.surface_area());
    assert!(Solid3::union_all(std::iter::empty()).is_empty());
    assert!(Solid3::empty().bounds().is_none());
    assert!(u.triangle_count() > 12);
    let mesh = u.mesh();
    assert_eq!(mesh.tris.len(), u.triangle_count());
    assert_eq!(mesh.origins.len(), mesh.tris.len());
    // The empty solid is the identity of union and has no triangles.
    assert!(rel(Solid3::empty().union(&boxes[0]).volume(), 1e9) < 1e-12);
    assert!(Solid3::empty().triangles().is_empty());
    // A reflection keeps the solid sound.
    let mirrored = boxes[0].transform(&Matrix::scale([-1.0, 1.0, 1.0]));
    assert!(rel(mirrored.volume(), 1e9) < 1e-12 && mirrored.is_closed());
    assert_eq!(mirrored.bounds().unwrap(), Bounds3 { min: [-1000.0, 0.0, 0.0], max: [0.0, 1000.0, 1000.0] });
    // Outward normals still point out after the mirror: the one at x = 0 points +x.
    assert!(mirrored.triangles().iter().filter(|t| t.p.iter().all(|p| p[0].abs() < 1e-9)).all(|t| t.normal[0] > 0.99));
}

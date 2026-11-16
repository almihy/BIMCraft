//! The car park solver tested as Arqiflow tests it (web/lib/parking/solver.test.ts and lab/check.ts, ported), and
//! against Arqiflow's own layouts (fixtures/, made by fixtures/make.ts from the TypeScript): the same bays, the same
//! accessible bays, every bay within 5 mm.

use serde_json::{Value, json};

use super::geom::*;
use super::solve::{Access, AccessKind, Input, Solved, solve};
use super::standards::Given;
use super::*;

fn bx(x0: f64, y0: f64, x1: f64, y1: f64) -> Ring {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn bbox(p: &[Pt]) -> Bounds {
    bounds(p)
}

fn overlap(a: &[Pt], b: &[Pt]) -> bool {
    let p = bbox(a);
    let q = bbox(b);
    p.x1.min(q.x1) - p.x0.max(q.x0) > 0.05 && p.y1.min(q.y1) - p.y0.max(q.y0) > 0.05
}

fn plot(site: Ring, streets: &[usize]) -> Input {
    Input { site, streets: streets.to_vec(), ..Input::default() }
}

fn angle(a: f64) -> Given {
    Given { angle: Some(a), ..Given::default() }
}

fn centre(p: &[Pt]) -> Pt {
    let n = p.len() as f64;
    [p.iter().map(|q| q[0]).sum::<f64>() / n, p.iter().map(|q| q[1]).sum::<f64>() / n]
}

// ---------------------------------------------------------------------------------------------------- lab/check.ts
/// What is wrong with a layout, in words (lab/check.ts `faults`).
fn faults(l: &Solved, buildings: &[Ring]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let at = |p: &[Pt]| {
        let c = centre(p);
        format!("{},{}", to_fixed(c[0], 1), to_fixed(c[1], 1))
    };
    let mut all: Vec<&Ring> = l.stalls.iter().map(|s| &s.poly).collect();
    all.extend(l.access_aisles.iter());
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            if shared_area(all[i], all[j]) > 0.02 {
                out.push(format!("bays overlap at {}", at(all[i])));
            }
        }
    }
    for p in &all {
        for a in &l.aisles {
            if shared_area(p, &a.poly) > 0.05 {
                out.push(format!("a bay on a {} at {}", a.kind.name(), at(p)));
            }
        }
    }
    for c in &l.columns {
        if all.iter().any(|p| shared_area(p, c) > 0.005) {
            out.push(format!("a column on a bay at {}", at(c)));
        }
        if l.aisles.iter().any(|a| shared_area(&a.poly, c) > 0.005) {
            out.push(format!("a column in an aisle at {}", at(c)));
        }
    }
    for r in &l.ramps {
        for p in &all {
            if shared_area(p, &r.poly) > 0.005 {
                out.push(format!("a bay on a ramp at {}", at(p)));
            }
        }
    }
    for p in &all {
        for b in buildings {
            if shared_area(p, b) > 0.005 {
                out.push(format!("a bay on a building at {}", at(p)));
            }
        }
    }
    for p in &all {
        for (il, _) in &l.islands {
            if shared_area(p, il) > 0.05 {
                out.push(format!("a bay on an island at {}", at(p)));
            }
        }
    }
    for p in &all {
        let c = centre(p);
        let ok = l.parts.iter().any(|g| p.iter().all(|q| point_in_region([q[0] + (c[0] - q[0]) * 0.01, q[1] + (c[1] - q[1]) * 0.01], g)));
        if !ok {
            out.push(format!("a bay off the ground at {}", at(p)));
        }
    }
    for s in &l.stalls {
        let c = centre(&s.poly);
        let d = or1(hypot(s.mouth[0] - c[0], s.mouth[1] - c[1]));
        let m = [s.mouth[0] + ((s.mouth[0] - c[0]) / d) * 0.4, s.mouth[1] + ((s.mouth[1] - c[1]) / d) * 0.4];
        let on: Vec<_> = l.aisles.iter().filter(|a| point_in_ring(m, &a.poly)).collect();
        if on.is_empty() && !l.access_aisles.iter().any(|a| point_in_ring(m, a)) {
            out.push(format!("a bay opens onto no aisle at {}", at(&s.poly)));
        }
        let back = [2.0 * c[0] - s.mouth[0], 2.0 * c[1] - s.mouth[1]];
        if let Some(f) = on.iter().find_map(|x| x.flow)
            && on.iter().all(|x| x.flow.is_some())
            && (back[0] - s.mouth[0]) * f[0] + (back[1] - s.mouth[1]) * f[1] < -0.05
        {
            out.push(format!("a bay leans against its aisle's traffic at {}", at(&s.poly)));
        }
    }
    let mut seen: Vec<String> = Vec::new();
    for f in out {
        if !seen.contains(&f) {
            seen.push(f);
        }
    }
    seen
}

/// A repeatable random plot (lab/check.ts `randomPlot`).
struct Rnd(f64);
impl Rnd {
    fn next(&mut self) -> f64 {
        self.0 = (self.0 * 16807.0) % 2147483647.0;
        self.0 / 2147483647.0
    }
}

fn random_plot(r: &mut Rnd) -> Input {
    let kind = (r.next() * 4.0).floor() as i64;
    let rr = 35.0 + r.next() * 50.0;
    let site: Ring = match kind {
        0 => {
            let n = 4 + (r.next() * 4.0).floor() as usize;
            let mut angles: Vec<f64> = (0..n).map(|_| r.next() * std::f64::consts::PI * 2.0).collect();
            angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            angles.iter().map(|a| [a.cos() * rr * (0.8 + r.next() * 0.4), a.sin() * rr * (0.8 + r.next() * 0.4)]).collect()
        }
        1 => {
            let (w, h, cw, ch) = (60.0 + r.next() * 60.0, 60.0 + r.next() * 60.0, 25.0 + r.next() * 30.0, 25.0 + r.next() * 30.0);
            let t = r.next() * std::f64::consts::PI;
            [[0.0, 0.0], [w, 0.0], [w, ch], [cw, ch], [cw, h], [0.0, h]]
                .iter()
                .map(|&[x, y]: &[f64; 2]| [x * t.cos() - y * t.sin(), x * t.sin() + y * t.cos()])
                .collect()
        }
        2 => {
            let (w, h) = (60.0 + r.next() * 80.0, 50.0 + r.next() * 70.0);
            let a = w + r.next() * 30.0 - 15.0;
            let b = w - r.next() * 20.0;
            let c = r.next() * 20.0;
            let d = h - r.next() * 15.0;
            vec![[0.0, 0.0], [w, 0.0], [a, h * 0.6], [b, h], [c, h], [0.0, d]]
        }
        _ => {
            let w = 60.0 + r.next() * 40.0;
            let (a, b) = (w + 30.0 + r.next() * 20.0, 50.0 + r.next() * 30.0);
            let (c, d) = (w + r.next() * 20.0, 90.0 + r.next() * 10.0);
            vec![[0.0, 0.0], [w, 0.0], [w, 40.0], [a, b], [c, d], [40.0, 70.0], [0.0, 70.0]]
        }
    };
    let mut obstacles: Vec<Ring> = Vec::new();
    if r.next() < 0.3 {
        let b = bounds(&site);
        let (cx, cy) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
        obstacles.push(bx(cx - 8.0, cy - 6.0, cx + 8.0, cy + 6.0));
    }
    let settings = if r.next() < 0.3 { angle([60.0, 45.0, 75.0][(r.next() * 3.0).floor() as usize]) } else { Given::default() };
    if r.next() < 0.2 {
        return Input { site, obstacles, basement: Some(Vec::new()), ..Input::default() };
    }
    Input { site, obstacles, streets: vec![0], settings, ..Input::default() }
}

fn samples() -> Vec<Input> {
    vec![
        Input {
            obstacles: vec![bx(50.0, 28.0, 80.0, 106.0)],
            target: Some([50.0, 60.0]),
            ..plot(vec![[0.0, 0.0], [80.0, 0.0], [80.0, 100.0], [74.0, 106.0], [6.0, 106.0], [0.0, 100.0]], &[0, 1, 3, 5])
        },
        plot(bx(0.0, 0.0, 80.0, 100.0), &[0]),
        plot(vec![[0.0, 0.0], [90.0, 0.0], [90.0, 40.0], [40.0, 40.0], [40.0, 90.0], [0.0, 90.0]], &[0]),
        plot(vec![[0.0, 0.0], [70.0, 15.0], [60.0, 80.0], [-10.0, 70.0]], &[0]),
        Input { obstacles: vec![bx(10.0, 45.0, 110.0, 70.0)], target: Some([60.0, 45.0]), ..plot(bx(0.0, 0.0, 120.0, 70.0), &[0]) },
        Input {
            access: vec![Access { at: [25.0, 0.0], kind: AccessKind::In }, Access { at: [100.0, 50.0], kind: AccessKind::Out }],
            ..plot(bx(0.0, 0.0, 100.0, 70.0), &[0, 1])
        },
        plot(vec![[0.0, 0.0], [70.0, 0.0], [70.0, 40.0], [110.0, 63.0], [90.0, 98.0], [40.0, 70.0], [0.0, 70.0]], &[0]),
        plot(bx(0.0, 0.0, 150.0, 120.0), &[0, 2]),
        Input { access: vec![Access { at: [110.5, 25.0], kind: AccessKind::Both }], ..plot(slanted(), &[0, 1]) },
        plot(bx(0.0, 0.0, 60.0, 19.0), &[3]),
    ]
}

fn slanted() -> Ring {
    vec![[0.0, 0.0], [102.6, 0.0], [114.7, 32.3], [102.1, 101.6], [45.4, 102.2], [0.0, 102.2]]
}

// ---------------------------------------------------------------------------------------------------- solver.test.ts
#[test]
fn a_strips_spans_round_a_hole_and_a_notch() {
    let g = Region::new(
        vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [60.0, 50.0], [60.0, 100.0], [0.0, 100.0]],
        vec![bx(20.0, 20.0, 30.0, 30.0)],
    );
    let r = |s: Vec<Span>| s.iter().map(|x| [js_round(x[0]), js_round(x[1])]).collect::<Vec<_>>();
    assert_eq!(r(strip_spans(&g, 10.0, 15.0)), vec![[0.0, 100.0]]);
    assert_eq!(r(strip_spans(&g, 18.0, 25.0)), vec![[0.0, 20.0], [30.0, 100.0]]);
    assert_eq!(r(strip_spans(&g, 45.0, 55.0)), vec![[0.0, 60.0]]);
    assert!(box_inside(&g, 70.0, 0.0, 99.0, 49.0) && !box_inside(&g, 70.0, 40.0, 99.0, 60.0));
}

#[test]
fn a_rectangle_fills_with_modules_every_bay_inside_and_none_overlapping() {
    let l = solve(&plot(bx(0.0, 0.0, 80.0, 100.0), &[0]));
    assert!(l.metrics.stalls >= 240, "{} bays", l.metrics.stalls);
    assert!(l.metrics.area_per_stall < 34.0);
    assert!(l.aisles.iter().any(|a| a.kind == fields::AisleKind::Cross) && l.aisles.iter().any(|a| a.kind == fields::AisleKind::Throat));
    let mut all: Vec<&Ring> = l.stalls.iter().map(|s| &s.poly).collect();
    all.extend(l.access_aisles.iter());
    for s in &all {
        for q in s.iter() {
            assert!(q[0] > -0.01 && q[0] < 80.01 && q[1] > -0.01 && q[1] < 100.01);
        }
    }
    for i in 0..all.len() {
        for j in i + 1..all.len() {
            assert!(!overlap(all[i], all[j]), "bays overlap");
        }
    }
    for s in &all {
        for a in &l.aisles {
            assert!(!overlap(s, &a.poly), "a bay on an aisle");
        }
    }
    assert_eq!(l.metrics.accessible, 5);
    assert_eq!(l.metrics.accessible_needed, 5);
}

#[test]
fn the_bays_of_every_row_lie_on_one_grid() {
    let l = solve(&Input { direction: Some(0.0), ..plot(bx(0.0, 0.0, 80.0, 100.0), &[0]) });
    let mut xs: Vec<String> = Vec::new();
    for s in &l.stalls {
        let b = bbox(&s.poly);
        if b.x1 - b.x0 < 3.0 {
            let v = to_fixed(((b.x0 % 2.8) + 2.8) % 2.8, 2);
            if !xs.contains(&v) {
                xs.push(v);
            }
        }
    }
    assert_eq!(xs.len(), 1, "{}", xs.join(" "));
}

#[test]
fn an_l_shaped_plot_is_set_out_in_both_its_arms() {
    let l = solve(&plot(vec![[0.0, 0.0], [90.0, 0.0], [90.0, 40.0], [40.0, 40.0], [40.0, 90.0], [0.0, 90.0]], &[0]));
    assert_eq!(faults(&l, &[]), Vec::<String>::new());
    assert!(l.metrics.stalls >= 150, "{}", l.metrics.stalls);
    // Bays in the arm along the street and in the arm away from it.
    assert!(l.stalls.iter().filter(|st| centre(&st.poly)[0] > 45.0).count() >= 30);
    assert!(l.stalls.iter().filter(|st| centre(&st.poly)[1] > 45.0).count() >= 30);
}

#[test]
fn buildings_and_the_footway_round_them_are_kept_clear() {
    let building = bx(50.0, 28.0, 80.0, 106.0);
    let l = solve(&Input {
        obstacles: vec![building],
        target: Some([50.0, 60.0]),
        ..plot(vec![[0.0, 0.0], [80.0, 0.0], [80.0, 100.0], [74.0, 106.0], [6.0, 106.0], [0.0, 100.0]], &[0])
    });
    let near = bx(48.5, 26.5, 81.5, 107.5);
    for s in l.stalls.iter().map(|x| &x.poly).chain(l.aisles.iter().filter(|a| a.kind != fields::AisleKind::Throat).map(|a| &a.poly)) {
        assert!(!overlap(s, &near));
    }
    assert!(l.metrics.stalls > 150);
}

#[test]
fn a_narrow_strip_takes_one_aisle_with_bays_on_both_sides() {
    let l = solve(&plot(bx(0.0, 0.0, 60.0, 19.0), &[3]));
    assert!(l.metrics.stalls >= 38, "{}", l.metrics.stalls);
}

#[test]
fn a_way_in_placed_by_the_person_is_where_the_throat_starts() {
    let l =
        solve(&Input { obstacles: vec![bx(10.0, 45.0, 110.0, 70.0)], entries: vec![[60.0, 0.0]], ..plot(bx(0.0, 0.0, 120.0, 70.0), &[0]) });
    assert_eq!(l.entries.len(), 1);
    assert!((l.entries[0].at[0] - 60.0).abs() < 0.5 && l.entries[0].at[1].abs() < 0.5);
    let throat = l.aisles.iter().find(|a| a.kind == fields::AisleKind::Throat).map(|a| a.poly.clone()).unwrap_or_default();
    for s in &l.stalls {
        assert!(!overlap(&s.poly, &throat));
    }
}

#[test]
fn it_is_quick_enough_to_follow_a_drag() {
    let t = std::time::Instant::now();
    solve(&Input {
        obstacles: vec![bx(50.0, 28.0, 80.0, 106.0)],
        ..plot(vec![[0.0, 0.0], [80.0, 0.0], [80.0, 100.0], [74.0, 106.0], [6.0, 106.0], [0.0, 100.0]], &[0, 1, 3, 5])
    });
    assert!(t.elapsed().as_millis() < 1500, "{:?}", t.elapsed());
}

#[test]
fn angled_bays_lean_with_their_one_way_aisles_traffic() {
    let l = solve(&Input { settings: angle(60.0), ..plot(bx(0.0, 0.0, 80.0, 100.0), &[0]) });
    assert!(l.settings.one_way && l.settings.aisle == 5.0);
    let flows: Vec<Option<Pt>> = l.aisles.iter().filter(|a| a.kind == fields::AisleKind::Aisle).map(|a| a.flow).collect();
    assert!(flows.len() >= 2 && flows.iter().all(Option::is_some));
    let f: Vec<Pt> = flows.into_iter().flatten().collect();
    assert!(f.iter().any(|f| f[0] + f[1] > 0.0) && f.iter().any(|f| f[0] + f[1] < 0.0), "aisles alternate");
    assert!(l.stalls.iter().any(|x| {
        let (a, b, c) = (x.poly[0], x.poly[1], x.poly[2]);
        ((b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1])).abs() > 1.0
    }));
    assert!(l.metrics.stalls > 180, "{}", l.metrics.stalls);
}

#[test]
fn a_plot_given_clockwise_keeps_its_street_sides() {
    let l = solve(&plot(vec![[0.0, 0.0], [0.0, 100.0], [80.0, 100.0], [80.0, 0.0]], &[3]));
    assert!(!l.entries.is_empty() && l.entries.iter().all(|e| e.at[1].abs() < 0.5));
}

#[test]
fn an_entrance_and_an_exit_the_person_placed() {
    let l = solve(&samples()[5]);
    assert_eq!(l.entries.iter().map(|e| e.kind).collect::<Vec<_>>(), vec![AccessKind::In, AccessKind::Out]);
    let (inn, out) = (&l.entries[0], &l.entries[1]);
    assert!((inn.at[0] - 25.0).abs() < 0.5 && (out.at[1] - 50.0).abs() < 0.5);
    assert!(inn.into[1] > 0.9 && out.into[0] < -0.9);
    let w = l.aisles.iter().find(|a| a.kind == fields::AisleKind::Throat).map(|a| bbox(&a.poly));
    assert!(w.is_some_and(|w| (w.x1 - w.x0 - 3.5).abs() < 0.05));
    assert!(l.notes.is_empty(), "{}", l.notes.join(" "));
}

#[test]
fn a_way_in_with_no_way_out_is_said() {
    let l = solve(&Input { access: vec![Access { at: [25.0, 0.0], kind: AccessKind::In }], ..plot(bx(0.0, 0.0, 100.0, 70.0), &[0]) });
    assert!(l.notes.iter().any(|n| n.contains("no way out")));
}

#[test]
fn a_big_car_park_gets_a_second_way_in() {
    let l = solve(&plot(bx(0.0, 0.0, 150.0, 120.0), &[0, 2]));
    assert!(l.metrics.stalls > 250 && l.entries.len() == 2, "{} bays, {} ways in", l.metrics.stalls, l.entries.len());
}

#[test]
fn a_field_with_a_single_angled_aisle_has_it_two_way() {
    let l = solve(&Input { settings: angle(60.0), ..plot(bx(0.0, 0.0, 60.0, 14.0), &[3]) });
    let aisles: Vec<_> = l.aisles.iter().filter(|a| a.kind == fields::AisleKind::Aisle).collect();
    assert!(aisles.len() == 1 && aisles[0].flow.is_none());
}

#[test]
fn bays_taken_out_are_planted_and_an_area_kept_clear_is_kept_clear() {
    let before = solve(&plot(bx(0.0, 0.0, 80.0, 100.0), &[0]));
    let c = centre(&before.stalls[10].poly);
    let after = solve(&Input { removed: vec![c], ..plot(bx(0.0, 0.0, 80.0, 100.0), &[0]) });
    assert_eq!(after.metrics.stalls, before.metrics.stalls - 1);
    let kept = solve(&Input { keep_clear: vec![bx(30.0, 40.0, 50.0, 60.0)], ..plot(bx(0.0, 0.0, 80.0, 100.0), &[0]) });
    for s in &kept.stalls {
        assert!(!overlap(&s.poly, &bx(30.0, 40.0, 50.0, 60.0)));
    }
}

#[test]
fn a_share_of_bays_get_a_charger_next_nearest_the_building() {
    let l = solve(&Input {
        target: Some([40.0, 100.0]),
        settings: Given { ev_share: Some(0.1), ..Given::default() },
        ..plot(bx(0.0, 0.0, 80.0, 100.0), &[0])
    });
    assert!((l.metrics.ev as f64 - (l.metrics.stalls as f64 * 0.1).ceil()).abs() <= 1.0);
    assert!(l.metrics.ev > 20 && l.stalls.iter().filter(|s| s.kind == fields::StallKind::Ev).all(|s| s.poly[0][1] > 40.0));
}

#[test]
fn a_plot_with_slanted_sides_gets_a_row_along_every_side() {
    let l = solve(&samples()[8]);
    assert!(l.metrics.stalls >= 330, "{}", l.metrics.stalls);
    let t = [102.1 - 114.7, 101.6 - 32.3];
    let len = hypot(t[0], t[1]);
    let along = l
        .stalls
        .iter()
        .filter(|st| {
            let (a, b) = (st.poly[0], st.poly[1]);
            let e = hypot(b[0] - a[0], b[1] - a[1]);
            (((b[0] - a[0]) * t[0] + (b[1] - a[1]) * t[1]) / (e * len)).abs() > 0.999 && (e - 2.8).abs() < 0.01
        })
        .count();
    assert!(along >= 20, "{along}");
    for st in &l.stalls {
        assert!(point_in_ring(centre(&st.poly), &slanted()));
    }
    assert!(l.notes.is_empty(), "{}", l.notes.join(" "));
}

#[test]
fn every_sample_square_and_angled_and_odd_plots_have_no_faults() {
    let mut plots: Vec<Input> = samples();
    plots.extend(samples().into_iter().map(|p| Input { settings: angle(60.0), ..p }));
    let mut r = Rnd(7.0);
    plots.extend((0..16).map(|_| random_plot(&mut r)));
    let mut bad = Vec::new();
    for (i, p) in plots.iter().enumerate() {
        let l = solve(p);
        let f = faults(&l, &p.obstacles);
        if !f.is_empty() {
            bad.push(format!("plot {i}: {}", f.join("; ")));
        }
        if l.metrics.stalls == 0 && i < 20 {
            bad.push(format!("plot {i} has no bays"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn a_building_in_the_middle_of_the_plot() {
    let l = solve(&Input {
        obstacles: vec![bx(27.7, 20.6, 43.7, 32.6)],
        ..plot(vec![[0.0, 0.0], [68.0, 0.0], [71.4, 31.9], [63.5, 53.2], [11.6, 53.2], [0.0, 49.4]], &[0])
    });
    assert_eq!(faults(&l, &[]), Vec::<String>::new());
    assert!(l.metrics.stalls >= 95, "{}", l.metrics.stalls);
}

#[test]
fn a_building_a_few_metres_off_the_plots_edge() {
    let l = solve(&Input {
        obstacles: vec![bx(52.4, 41.0, 68.4, 53.0)],
        ..plot(vec![[0.0, 0.0], [72.3, 0.0], [72.3, 40.0], [120.7, 50.0], [74.9, 94.0], [40.0, 70.0], [0.0, 70.0]], &[0])
    });
    assert_eq!(faults(&l, &[]), Vec::<String>::new());
    assert!(l.metrics.stalls >= 160, "{}", l.metrics.stalls);
}

#[test]
fn angled_bays_round_a_ring_lean_with_the_loops_traffic() {
    let l = solve(&Input { settings: angle(60.0), ..samples()[8].clone() });
    assert_eq!(faults(&l, &[]), Vec::<String>::new());
    assert!(l.metrics.stalls >= 320, "{}", l.metrics.stalls);
}

#[test]
fn a_street_side_too_short_for_a_way_in_by_the_rules() {
    let l = solve(&plot(vec![[0.0, 0.0], [8.0, -3.0], [70.0, 30.0], [60.0, 90.0], [-10.0, 80.0]], &[0]));
    assert!(l.metrics.stalls > 100, "{}", l.metrics.stalls);
    assert!(!l.notes.is_empty());
}

#[test]
fn a_quick_look_while_dragging_is_a_fraction_of_the_full_search() {
    let p = plot(vec![[0.0, 0.0], [70.0, 0.0], [70.0, 40.0], [110.0, 63.0], [90.0, 98.0], [40.0, 70.0], [0.0, 70.0]], &[0]);
    let t = std::time::Instant::now();
    let full = solve(&p);
    let t_full = t.elapsed();
    let t = std::time::Instant::now();
    let quick = solve(&Input { quick: true, ..p });
    let t_quick = t.elapsed();
    assert_eq!(faults(&quick, &[]), Vec::<String>::new());
    assert!(t_quick < t_full, "{t_quick:?} {t_full:?}");
    assert!(quick.metrics.stalls as f64 >= full.metrics.stalls as f64 * 0.85);
}

#[test]
fn a_building_across_the_plot_cuts_it_in_two() {
    let building = bx(60.0, -5.0, 80.0, 45.0);
    let l = solve(&Input { obstacles: vec![building.clone()], ..plot(bx(0.0, 0.0, 150.0, 40.0), &[0]) });
    assert_eq!(faults(&l, &[building]), Vec::<String>::new());
    assert!(l.stalls.iter().any(|st| st.poly[0][0] < 58.0) && l.stalls.iter().any(|st| st.poly[0][0] > 82.0));
    assert_eq!(l.entries.len(), 2);
    assert!(l.notes.iter().any(|n| n.contains("2 parts")));
}

#[test]
fn below_ground_with_no_ramp_drawn_none_is_made_up() {
    let core = bx(36.0, 26.0, 44.0, 34.0);
    let l = solve(&Input { site: bx(0.0, 0.0, 80.0, 60.0), obstacles: vec![core.clone()], basement: Some(Vec::new()), ..Input::default() });
    assert!(l.ramps.is_empty(), "no ramp is invented");
    assert!(l.notes.iter().any(|n| n.contains("No ramp reaches")), "{:?}", l.notes);
}

#[test]
fn below_ground_a_ramps_end_given_is_the_way_in() {
    // A curved ramp's end, as the generator gives it: the line across it and the way off it.
    let l = solve(&Input {
        site: bx(0.0, 0.0, 90.0, 64.0),
        basement: Some(Vec::new()),
        feet: vec![(([0.0, 30.0], [0.0, 36.0]), [1.0, 0.0])],
        ..Input::default()
    });
    assert_eq!(l.entries.len(), 1, "{:?}", l.notes);
    assert!(!l.notes.iter().any(|n| n.contains("No ramp reaches")));
}

#[test]
fn below_ground_with_a_ramp_drawn_its_foot_is_the_way_in() {
    let ramp = bx(0.0, 0.0, 7.0, 27.0);
    let l = solve(&Input {
        site: bx(0.0, 0.0, 90.0, 64.0),
        obstacles: vec![bx(40.0, 28.0, 48.0, 36.0)],
        basement: Some(vec![ramp.clone()]),
        ..Input::default()
    });
    assert_eq!(faults(&l, &[]), Vec::<String>::new());
    assert!(l.entries.len() == 1);
    assert!(l.stalls.iter().all(|st| !overlap(&st.poly, &ramp)));
    let mut xs: Vec<f64> = Vec::new();
    for c in &l.columns {
        let v = js_round(c.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min) * 10.0) / 10.0;
        if !xs.contains(&v) {
            xs.push(v);
        }
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if xs.len() > 1 {
        assert!(xs.windows(2).any(|w| (w[1] - w[0] - (3.0 * 2.8 + 0.6)).abs() < 0.05), "{xs:?}");
    }
}

// ---------------------------------------------------------------------------------------------------- the JavaScript
#[test]
fn javascript_numbers_round_and_print_as_javascript_does() {
    assert_eq!(js_round(2.5), 3.0);
    assert_eq!(js_round(-2.5), -2.0);
    assert_eq!(js_round(-2.6), -3.0);
    assert_eq!(to_fixed(0.125, 2), "0.13");
    assert_eq!(to_fixed(1.005, 2), "1.00");
    assert_eq!(to_fixed(-0.125, 2), "-0.13");
    assert_eq!(to_fixed(13.375, 2), "13.38");
    assert_eq!(to_fixed(2.0, 1), "2.0");
    assert_eq!(hypot(3.0, 4.0), 5.0);
    // V8's own, where a plain square root differs in the last place.
    assert_eq!(hypot(-35.58827996514192, 25.119284067824154), 43.56035012436616);
}

// ---------------------------------------------------------------------------------------------------- Arqiflow's own layouts
fn fixtures() -> Vec<(String, Value)> {
    // PARKING_FIXTURES names another folder of them (a wider check, made by `make.ts more`).
    let dir = match std::env::var_os("PARKING_FIXTURES") {
        Some(d) => std::path::PathBuf::from(d),
        None => std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/site/parking/fixtures"),
    };
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    names.sort();
    names
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let v: Value = serde_json::from_str(&quote_numbers(&text)).ok()?;
            Some((p.file_name()?.to_string_lossy().to_string(), unquote(v)))
        })
        .collect()
}

/// The JSON with every number written as a string, so it can be read to the last bit: serde_json (without its
/// `float_roundtrip` feature) reads some decimals a place off, where JavaScript and `str::parse` read them exactly.
fn quote_numbers(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    let mut in_str = false;
    let mut escaped = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
        } else if c == '-' || c.is_ascii_digit() {
            out.push('"');
            out.push('#');
            out.push(c);
            while let Some(&d) = chars.peek() {
                if d.is_ascii_digit() || "+-.eE".contains(d) {
                    out.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push('"');
        } else {
            out.push(c);
        }
    }
    out
}

/// The numbers `quote_numbers` wrote as strings, back as numbers, read exactly.
fn unquote(v: Value) -> Value {
    match v {
        Value::String(s) => match s.strip_prefix('#').and_then(|t| t.parse::<f64>().ok()).and_then(serde_json::Number::from_f64) {
            Some(n) => Value::Number(n),
            None => Value::String(s),
        },
        Value::Array(a) => Value::Array(a.into_iter().map(unquote).collect()),
        Value::Object(m) => Value::Object(m.into_iter().map(|(k, v)| (k, unquote(v))).collect()),
        other => other,
    }
}

fn pt_of(v: &Value) -> Pt {
    [v[0].as_f64().unwrap_or(f64::NAN), v[1].as_f64().unwrap_or(f64::NAN)]
}

fn ring_of(v: &Value) -> Ring {
    v.as_array().map(|a| a.iter().map(pt_of).collect()).unwrap_or_default()
}

fn rings_of(v: &Value) -> Vec<Ring> {
    v.as_array().map(|a| a.iter().map(ring_of).collect()).unwrap_or_default()
}

/// The solver's input from Arqiflow's `ParkingInput` as JSON.
fn input_from(v: &Value) -> Input {
    let s = &v["settings"];
    let f = |k: &str| s[k].as_f64();
    Input {
        site: ring_of(&v["site"]),
        obstacles: rings_of(&v["obstacles"]),
        streets: v["streets"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64().map(|x| x as usize)).collect()).unwrap_or_default(),
        access: v["access"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| Some(Access { at: pt_of(&x["at"]), kind: AccessKind::parse(x["kind"].as_str()?)? })).collect())
            .unwrap_or_default(),
        entries: v["entries"].as_array().map(|a| a.iter().map(pt_of).collect()).unwrap_or_default(),
        keep_clear: rings_of(&v["keepClear"]),
        removed: v["removed"].as_array().map(|a| a.iter().map(pt_of).collect()).unwrap_or_default(),
        target: v.get("target").filter(|t| t.is_array()).map(pt_of),
        direction: v["direction"].as_f64(),
        quick: v["quick"].as_bool().unwrap_or(false),
        basement: v.get("basement").filter(|b| b.is_object()).map(|b| rings_of(&b["ramps"])),
        feet: Vec::new(),
        cores: Vec::new(),
        settings: Given {
            standard: s["standard"].as_str().map(str::to_string),
            angle: f("angle"),
            one_way: s["oneWay"].as_bool(),
            stall_width: f("stallWidth"),
            stall_depth: f("stallDepth"),
            aisle: f("aisle"),
            setback: f("setback"),
            end_island: f("endIsland"),
            island_every: f("islandEvery"),
            ev_share: f("evShare"),
            column_every: f("columnEvery"),
            column_size: f("columnSize"),
            floor_to_floor: f("floorToFloor"),
            ramp_slope: f("rampSlope"),
            ..Given::default()
        },
    }
}

/// Where two lists of rings part by more than `tol`, in words.
fn rings_differ(what: &str, ours: &[Ring], theirs: &[Ring], tol: f64) -> Option<String> {
    if ours.len() != theirs.len() {
        return Some(format!("{} {what}, Arqiflow {}", ours.len(), theirs.len()));
    }
    for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
        if a.len() != b.len() || a.iter().zip(b).any(|(p, q)| (p[0] - q[0]).abs() > tol || (p[1] - q[1]).abs() > tol) {
            return Some(format!("{what} {i} at {:?}, Arqiflow's at {:?}", a.first(), b.first()));
        }
    }
    None
}

fn compare_solved(l: &Solved, want: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let m = &want["metrics"];
    let n = |k: &str| m[k].as_f64().unwrap_or(f64::NAN);
    if l.metrics.stalls as f64 != n("stalls") {
        out.push(format!("{} bays, Arqiflow {}", l.metrics.stalls, n("stalls")));
    }
    if l.metrics.accessible as f64 != n("accessible") || l.metrics.accessible_needed as f64 != n("accessibleNeeded") {
        out.push(format!(
            "{}/{} accessible, Arqiflow {}/{}",
            l.metrics.accessible,
            l.metrics.accessible_needed,
            n("accessible"),
            n("accessibleNeeded")
        ));
    }
    if l.metrics.ev as f64 != n("ev") {
        out.push(format!("{} with a charger, Arqiflow {}", l.metrics.ev, n("ev")));
    }
    let stalls = want["stalls"].as_array().cloned().unwrap_or_default();
    let ours: Vec<Ring> = l.stalls.iter().map(|s| s.poly.clone()).collect();
    let theirs: Vec<Ring> = stalls.iter().map(|s| ring_of(&s["poly"])).collect();
    out.extend(rings_differ("bays", &ours, &theirs, 0.005));
    if l.stalls.len() == stalls.len() {
        for (i, (s, w)) in l.stalls.iter().zip(&stalls).enumerate() {
            if s.kind.name() != w["kind"].as_str().unwrap_or("") {
                out.push(format!("bay {i} is {}, Arqiflow's {}", s.kind.name(), w["kind"]));
                break;
            }
            let wm = pt_of(&w["mouth"]);
            if (s.mouth[0] - wm[0]).abs() > 0.005 || (s.mouth[1] - wm[1]).abs() > 0.005 {
                out.push(format!("bay {i}'s mouth at {:?}, Arqiflow's {wm:?}", s.mouth));
                break;
            }
        }
    }
    out.extend(rings_differ("access aisles", &l.access_aisles, &rings_of(&want["accessAisles"]), 0.005));
    let aisles = want["aisles"].as_array().cloned().unwrap_or_default();
    out.extend(rings_differ(
        "aisles",
        &l.aisles.iter().map(|a| a.poly.clone()).collect::<Vec<_>>(),
        &aisles.iter().map(|a| ring_of(&a["poly"])).collect::<Vec<_>>(),
        0.005,
    ));
    if l.aisles.len() == aisles.len() {
        for (i, (a, w)) in l.aisles.iter().zip(&aisles).enumerate() {
            let wf = w.get("flow").map(pt_of);
            let same_flow = match (a.flow, wf) {
                (None, None) => true,
                (Some(f), Some(g)) => (f[0] - g[0]).abs() < 1e-3 && (f[1] - g[1]).abs() < 1e-3,
                _ => false,
            };
            if a.kind.name() != w["kind"].as_str().unwrap_or("") || !same_flow {
                out.push(format!("aisle {i} is {} {:?}, Arqiflow's {} {wf:?}", a.kind.name(), a.flow, w["kind"]));
                break;
            }
        }
    }
    out.extend(rings_differ("columns", &l.columns, &rings_of(&want["columns"]), 0.005));
    let entries = want["entries"].as_array().cloned().unwrap_or_default();
    let same_ways = l.entries.len() == entries.len()
        && l.entries.iter().zip(&entries).all(|(e, w)| {
            let at = pt_of(&w["at"]);
            e.kind.name() == w["kind"].as_str().unwrap_or("") && (e.at[0] - at[0]).abs() <= 0.005 && (e.at[1] - at[1]).abs() <= 0.005
        });
    if !same_ways {
        let ours: Vec<String> = l.entries.iter().map(|e| format!("{} {:?}", e.kind.name(), e.at)).collect();
        out.push(format!("ways in {ours:?}, Arqiflow's {entries:?}"));
    }
    let ramps = want["ramps"].as_array().cloned().unwrap_or_default();
    out.extend(rings_differ(
        "ramps",
        &l.ramps.iter().map(|r| r.poly.clone()).collect::<Vec<_>>(),
        &ramps.iter().map(|r| ring_of(&r["poly"])).collect::<Vec<_>>(),
        0.005,
    ));
    let notes: Vec<String> =
        want["notes"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if l.notes != notes {
        out.push(format!("notes {:?}, Arqiflow's {notes:?}", l.notes));
    }
    if (if l.ring { "ring" } else { "fields" }) != want["arrangement"].as_str().unwrap_or("") {
        out.push(format!("arrangement {}, Arqiflow's {}", if l.ring { "ring" } else { "fields" }, want["arrangement"]));
    }
    let dirs: Vec<f64> = want["directions"].as_array().map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    if l.directions.len() != dirs.len() || l.directions.iter().zip(&dirs).any(|(a, b)| (a - b).abs() > 1e-6) {
        out.push(format!("directions {:?}, Arqiflow's {dirs:?}", l.directions));
    }
    if l.islands.len() as f64 != want["islands"].as_f64().unwrap_or(f64::NAN) {
        out.push(format!("{} islands, Arqiflow {}", l.islands.len(), want["islands"]));
    }
    out
}

/// The car park element's props in mm from Arqiflow's in metres.
fn props_mm(p: &Value) -> Props {
    let mm = |v: &Value| v.as_f64().map(|x| json!(x * 1000.0)).unwrap_or(Value::Null);
    let pt = |v: &Value| json!([v[0].as_f64().unwrap_or(0.0) * 1000.0, v[1].as_f64().unwrap_or(0.0) * 1000.0]);
    let mut out = Props::new();
    for (k, v) in p.as_object().cloned().unwrap_or_default() {
        let nv = match k.as_str() {
            "control" | "removed" => Value::Array(v.as_array().map(|a| a.iter().map(pt).collect()).unwrap_or_default()),
            "target" => pt(&v),
            "access" => Value::Array(
                v.as_array().map(|a| a.iter().map(|x| json!({"at": pt(&x["at"]), "kind": x["kind"]})).collect()).unwrap_or_default(),
            ),
            "stall_width" | "stall_depth" | "aisle" | "setback" | "end_island" | "column_size" | "floor_to_floor" => mm(&v),
            _ => v,
        };
        out.insert(k, nv);
    }
    out
}

fn car_park_layout(k: &Value) -> Layout {
    let levels: Vec<LevelIn> = k["levels"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|l| LevelIn {
                    name: l["id"].as_str().unwrap_or("").into(),
                    elevation: l["elevation"].as_f64().unwrap_or(0.0) * 1000.0,
                })
                .collect()
        })
        .unwrap_or_default();
    let obstacles: Vec<Obstacle> = k["elements"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|e| {
                    let ring = ring_of(&e["geometry"]["coordinates"][0]);
                    let kind = match e["use"].as_str() {
                        Some(u) if !u.is_empty() => format!("{}/{u}", e["kind"].as_str().unwrap_or("")),
                        _ => e["kind"].as_str().unwrap_or("").to_string(),
                    };
                    Obstacle {
                        kind,
                        level: e["level"].as_str().unwrap_or("").into(),
                        outline: vec![Poly { outer: ring.iter().map(|p| P2(p[0] * 1000.0, p[1] * 1000.0)).collect(), holes: vec![] }],
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let streets: Vec<(P2, P2)> = k["streets"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| {
                    (
                        P2(s[0][0].as_f64().unwrap_or(0.0) * 1000.0, s[0][1].as_f64().unwrap_or(0.0) * 1000.0),
                        P2(s[1][0].as_f64().unwrap_or(0.0) * 1000.0, s[1][1].as_f64().unwrap_or(0.0) * 1000.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    lay_out(&props_mm(&k["cp"]["props"]), k["cp"]["level"].as_str().unwrap_or(""), &levels, &obstacles, &streets)
}

fn compare_layout(l: &Layout, want: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let pieces = want["pieces"].as_array().cloned().unwrap_or_default();
    if l.pieces.len() != pieces.len() {
        out.push(format!("{} pieces, Arqiflow {}", l.pieces.len(), pieces.len()));
    } else {
        for (i, (p, w)) in l.pieces.iter().zip(&pieces).enumerate() {
            let ring: Vec<Pt> =
                p.outline.first().map(|o| o.outer.iter().map(|q| [q.0 / 1000.0, q.1 / 1000.0]).collect()).unwrap_or_default();
            let mut props = w["props"].clone();
            if let Some(o) = props.as_object_mut() {
                o.remove("band");
                if let Some(m) = o.get_mut("mouth") {
                    *m = json!([m[0].as_f64().unwrap_or(0.0) * 1000.0, m[1].as_f64().unwrap_or(0.0) * 1000.0]);
                }
            }
            let same_props = p.props.iter().all(|(k, v)| match (v.as_array(), props[k].as_array()) {
                (Some(a), Some(b)) => {
                    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(1.0)).abs() < 5.0)
                }
                _ => v.as_f64().zip(props[k].as_f64()).map(|(a, b)| a == b).unwrap_or(*v == props[k]),
            }) && props.as_object().is_some_and(|o| o.len() == p.props.len());
            if p.kind != w["kind"].as_str().unwrap_or("") || !same_props {
                out.push(format!("piece {i}: {} {:?}, Arqiflow's {} {}", p.kind, p.props, w["kind"], props));
                break;
            }
            if let Some(d) = rings_differ("piece", &[ring], &[ring_of(&w["ring"])], 0.005) {
                out.push(format!("piece {i} ({}): {d}", p.kind));
                break;
            }
        }
    }
    let r = &want["result"];
    for k in ["stalls", "accessible", "accessible_needed", "ev", "columns", "arrangement", "basement", "notes", "area_per_stall"] {
        if l.result.get(k).map(|v| v.as_f64().map(|x| json!(x)).unwrap_or(v.clone()))
            != Some(r[k].as_f64().map(|x| json!(x)).unwrap_or(r[k].clone()))
        {
            out.push(format!("{k}: {:?}, Arqiflow's {}", l.result.get(k), r[k]));
        }
    }
    out
}

/// Each of Arqiflow's cases laid out here: the bays it reached, Arqiflow's, and the differences (none when it is laid out as
/// Arqiflow's solver laid it).
fn against_arqiflow() -> Vec<(String, f64, f64, Vec<String>, Vec<String>)> {
    let mut out = Vec::new();
    for (name, case) in fixtures() {
        // Below ground with no ramp drawn, Arqiflow's solver made one up; ours never does (the owner, 16 November):
        // those cases are no longer the port's to match.
        if case.get("input").is_some_and(|i| i["basement"].is_object() && i["basement"]["ramps"].as_array().is_none_or(Vec::is_empty)) {
            continue;
        }
        let label = format!("{name} ({})", case["name"]);
        if let Some(input) = case.get("input") {
            let p = input_from(input);
            let l = solve(&p);
            let theirs = case["out"]["metrics"]["stalls"].as_f64().unwrap_or(f64::NAN);
            out.push((label, l.metrics.stalls as f64, theirs, compare_solved(&l, &case["out"]), faults(&l, &p.obstacles)));
        } else {
            let l = car_park_layout(&case["car_park"]);
            let theirs = case["result"]["stalls"].as_f64().unwrap_or(f64::NAN);
            let ours = l.result.get("stalls").and_then(Value::as_f64).unwrap_or(0.0);
            out.push((label, ours, theirs, compare_layout(&l, &case), Vec::new()));
        }
    }
    out
}

/// Arqiflow's layouts (its solver, ported, the one this crate had): each laid out as it laid it, or, where the modules on the
/// grid reach clearly more bays (docs/parking-ramps-plan.md, "Solver rework"), with more bays and no fault.
#[test]
fn the_layouts_match_arqiflows_or_reach_more_bays() {
    assert!(fixtures().len() >= 50 || std::env::var_os("PARKING_FIXTURES").is_some(), "{} fixtures", fixtures().len());
    let cases = against_arqiflow();
    let mut report = Vec::new();
    let (mut same, mut more) = (0, 0);
    for (name, ours, theirs, diffs, faults) in &cases {
        if diffs.is_empty() {
            same += 1;
        } else if ours > theirs && faults.is_empty() {
            more += 1;
        } else {
            report.push(format!("{name}: {ours} bays, Arqiflow {theirs}; {}; {}", diffs.join("; "), faults.join("; ")));
        }
    }
    assert!(report.is_empty(), "{same} as Arqiflow's, {more} with more bays; these neither:
{}", report.join("
"));
}

/// The bays each of Arqiflow's cases gets here and got there (`--ignored --nocapture`).
#[test]
#[ignore]
fn yields() {
    let (mut a, mut b) = (0.0, 0.0);
    for (name, ours, theirs, diffs, _) in against_arqiflow() {
        a += ours;
        b += theirs;
        eprintln!("{name}: {theirs} -> {ours}{}", if diffs.is_empty() { "" } else { " (laid out anew)" });
    }
    eprintln!("all: {b} -> {a}");
}

// ---------------------------------------------------------------------------------------------------- the element's way in
fn mm_props(v: Value) -> Props {
    match v {
        Value::Object(m) => m,
        _ => Props::new(),
    }
}

#[test]
fn lay_out_reads_millimetres_and_writes_arqiflows_pieces() {
    let props = mm_props(json!({ "control": [[0, 0], [80000, 0], [80000, 100000], [0, 100000]] }));
    let streets = [(P2(-5000.0, 0.0), P2(85000.0, 0.0))];
    let l = lay_out(&props, "Level 1", &[LevelIn { name: "Level 1".into(), elevation: 0.0 }], &[], &streets);
    let stalls: Vec<&Piece> = l.pieces.iter().filter(|p| p.kind == "stall").collect();
    assert_eq!(l.result["stalls"], json!(stalls.len()));
    assert!(stalls.len() >= 240);
    assert_eq!(stalls.iter().filter(|p| p.props.get("accessible") == Some(&json!(true))).count(), 5);
    assert!(l.pieces.iter().all(|p| p.level == "Level 1"));
    // A bay is 2.8 m by 5.5 m, in mm, counter-clockwise.
    let o = &stalls[0].outline[0].outer;
    let pts: Ring = o.iter().map(|p| [p.0, p.1]).collect();
    assert!(ring_area(&pts) > 0.0 && (ring_area(&pts) - 2800.0 * 5500.0).abs() < 1.0);
    assert!(l.pieces.iter().any(|p| p.kind == "aisle" && p.props["part"] == "throat"));
    assert_eq!(l.result["ways"].as_array().map(Vec::len), Some(1));
    assert_eq!(l.result["arrangement"], "fields");
}

#[test]
fn lay_out_below_ground_with_no_ramp_lays_out_none_reached_and_says_so() {
    let props = mm_props(json!({ "control": [[0, 0], [80000, 0], [80000, 60000], [0, 60000]] }));
    let levels = [LevelIn { name: "B1".into(), elevation: -3500.0 }, LevelIn { name: "L0".into(), elevation: 0.0 }];
    let core = Obstacle {
        kind: "mass".into(),
        level: "B1".into(),
        outline: vec![Poly {
            outer: vec![P2(36000.0, 26000.0), P2(44000.0, 26000.0), P2(44000.0, 34000.0), P2(36000.0, 34000.0)],
            holes: vec![],
        }],
    };
    let elsewhere = Obstacle {
        kind: "mass".into(),
        level: "L0".into(),
        outline: vec![Poly { outer: vec![P2(0.0, 0.0), P2(80000.0, 0.0), P2(80000.0, 60000.0)], holes: vec![] }],
    };
    let l = lay_out(&props, "B1", &levels, &[core, elsewhere], &[]);
    assert_eq!(l.result["basement"], json!(true));
    assert!(!l.pieces.iter().any(|p| p.kind == "aisle" && p.props["part"] == "ramp"), "no ramp is invented");
    assert!(l.result["notes"].to_string().contains("No ramp reaches"), "{:?}", l.result);
    assert_eq!(l.result["ways"], json!([]));
}

#[test]
fn hostile_props_never_panic() {
    let bad = [
        json!({}),
        json!({ "control": "x" }),
        json!({ "control": [[0, 0], [1, 1]] }),
        json!({ "control": [[0, 0], [1e300, 0], [0, 1e300]] }),
        json!({ "control": [[0, 0], [0, 0], [0, 0], [0, 0]] }),
        json!({ "control": [[0, 0], [10, 0], [10, 10], [0, 10]] }),
        json!({ "control": [[0, 0], [5e9, 0], [5e9, 5e9]] }),
        json!({ "control": [[0, 0], [60000, 0], [60000, 40000], [0, 40000]], "stall_width": -1, "aisle": 1e300, "angle": "x", "island_every": 1e9, "column_every": -3, "ev_share": 5, "direction": 1e300, "access": [{"at": [1, "a"], "kind": "in"}, {"at": [0, 0], "kind": "sideways"}], "removed": [[null, 1]], "target": [1] }),
        json!({ "control": [[0, 0], [60000, 0], [0, 40000], [60000, 40000]] }),
        json!({ "control": [[0, 0], [60000, 0], [60000, 40000], [0, 40000]], "angle": 1, "stall_width": 1500, "stall_depth": 2500, "aisle": 2500, "end_island": 0, "island_every": 0.5 }),
    ];
    for p in bad {
        let l = lay_out(
            &mm_props(p),
            "L",
            &[LevelIn { name: "L".into(), elevation: f64::NAN }],
            &[],
            &[(P2(f64::NAN, 0.0), P2(0.0, f64::INFINITY))],
        );
        assert!(l.result.contains_key("stalls"));
    }
}

/// A large car park (3 ha, over 800 bays, a building on it) in well under a second (the core is built optimised in
/// debug too; release is quicker still).
#[test]
fn a_large_car_park_is_quick() {
    let props = mm_props(json!({ "control": [[0, 0], [200000, 0], [200000, 150000], [0, 150000]] }));
    let building = Obstacle {
        kind: "mass".into(),
        level: "L".into(),
        outline: vec![Poly {
            outer: vec![P2(150000.0, 100000.0), P2(190000.0, 100000.0), P2(190000.0, 140000.0), P2(150000.0, 140000.0)],
            holes: vec![],
        }],
    };
    let t = std::time::Instant::now();
    let l = lay_out(&props, "L", &[], &[building], &[(P2(0.0, 0.0), P2(200000.0, 0.0))]);
    let took = t.elapsed();
    assert!(l.result["stalls"].as_u64().unwrap_or(0) > 800, "{}", l.result["stalls"]);
    // About a quarter of a second on its own; a test build shares the machine with every other test, so it is given room.
    let limit = if cfg!(debug_assertions) { 10_000 } else { 1000 };
    assert!(took.as_millis() < limit, "{took:?}");
}

/// How long each of Arqiflow's cases takes here (`--ignored --nocapture`).
#[test]
#[ignore]
fn timings() {
    for (name, case) in fixtures() {
        if std::env::var_os("PARKING_ONE").is_some_and(|n| n != name.as_str()) {
            continue;
        }
        let mut best = std::time::Duration::MAX;
        for _ in 0..3 {
            let t = std::time::Instant::now();
            if let Some(input) = case.get("input") {
                solve(&input_from(input));
            } else {
                car_park_layout(&case["car_park"]);
            }
            best = best.min(t.elapsed());
        }
        eprintln!("{name} {best:?} (Arqiflow {} ms)", case["ms"]);
    }
}

#[test]
fn below_ground_a_column_grid_is_stepped_round_and_its_ways_in_miss_the_columns() {
    // A basement 60 by 40 m in its walls, a column grid 8.1 by 10 m, a ramp's foot on its east side, between two rows of columns.
    let mut keep = vec![bx(-0.1, -0.1, 60.1, 0.1), bx(59.9, -0.1, 60.1, 40.1), bx(-0.1, 39.9, 60.1, 40.1), bx(-0.1, -0.1, 0.1, 40.1)];
    let mut posts = Vec::new();
    for x in [8.1, 16.2, 24.3, 32.4, 40.5, 48.6] {
        for y in [10.0, 20.0, 30.0] {
            posts.push(bx(x - 0.15, y - 0.15, x + 0.15, y + 0.15));
        }
    }
    keep.extend(posts.iter().cloned());
    let t = std::time::Instant::now();
    let l = solve(&Input {
        site: bx(0.0, 0.0, 60.0, 40.0),
        basement: Some(Vec::new()),
        feet: vec![(([59.9, 22.0], [59.9, 28.0]), [-1.0, 0.0])],
        keep_clear: keep,
        settings: Given { column_every: Some(0.0), ..Given::default() },
        ..Input::default()
    });
    assert!(t.elapsed().as_secs_f64() < 2.0, "{:?}", t.elapsed());
    assert_eq!(l.entries.len(), 1, "{:?}", l.notes);
    // Some bays: a designer fits some 60 here; the fields set out round a grid of posts are still few (plan, "Left").
    assert!(l.stalls.len() > 10, "{} bays", l.stalls.len());
    for p in &posts {
        assert!(l.stalls.iter().all(|st| !overlap(&st.poly, p)), "a bay over a column");
        for a in &l.aisles {
            assert!(!overlap(&a.poly, p), "{:?} {:?} through the column {p:?}", a.kind, a.poly);
        }
    }
}

// ---------------------------------------------------------------------------------------------------- modules on the grid
/// The reviewer's basement as the generator hands it to the solver: 60 by 40 m in its walls, a column grid 8.1 by 10 m, a
/// straight ramp along the north wall (its foot at x 31, cars coming off it east) and a curved one in the south-east corner
/// (its foot at x 36, cars coming off it west). `sizes`: the firm's stall width, depth and aisle (m), or the standard's.
fn real_basement(sizes: Option<(f64, f64, f64)>) -> (Input, Vec<Ring>) {
    let mut keep = vec![bx(-0.1, -0.1, 60.1, 0.1), bx(59.9, -0.1, 60.1, 40.1), bx(-0.1, 39.9, 60.1, 40.1), bx(-0.1, -0.1, 0.1, 40.1)];
    let mut posts = Vec::new();
    for x in [8.1, 16.2, 24.3, 32.4, 40.5, 48.6] {
        for y in [10.0, 20.0, 30.0] {
            posts.push(bx(x - 0.15, y - 0.15, x + 0.15, y + 0.15));
        }
    }
    keep.extend(posts.iter().cloned());
    // The straight ramp, and the curved one's runs (a straight run, then a quarter turn of 4 m radius to its middle line).
    let mut ramps = vec![bx(7.67, 33.5, 31.0, 39.5), bx(36.0, 0.5, 52.15, 6.5)];
    let (cx, cy) = (52.0, 7.5);
    let k = 8;
    for i in 0..k {
        let t0 = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * i as f64 / k as f64;
        let t1 = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * (i + 1) as f64 / k as f64;
        let p = |r: f64, t: f64| [cx + r * t.cos(), cy + r * t.sin()];
        ramps.push(vec![p(1.0, t0), p(7.0, t0), p(7.0, t1), p(1.0, t1)]);
    }
    keep.extend(ramps.iter().cloned());
    let settings = match sizes {
        Some((w, d, a)) => Given { stall_width: Some(w), stall_depth: Some(d), aisle: Some(a), column_every: Some(0.0), ..Given::default() },
        None => Given { column_every: Some(0.0), ..Given::default() },
    };
    let input = Input {
        site: bx(0.0, 0.0, 60.0, 40.0),
        basement: Some(Vec::new()),
        feet: vec![(([31.0, 39.5], [31.0, 33.5]), [1.0, 0.0]), (([36.0, 0.5], [36.0, 6.5]), [-1.0, 0.0])],
        keep_clear: keep,
        settings,
        ..Input::default()
    };
    let mut solid = posts;
    solid.extend(ramps);
    (input, solid)
}

#[test]
fn the_reviewers_basement_is_filled_from_both_its_ramps() {
    for (sizes, least) in [(Some((2.5, 5.0, 6.0)), 50), (None, 33)] {
        let (input, solid) = real_basement(sizes);
        let t = std::time::Instant::now();
        let l = solve(&input);
        let took = t.elapsed();
        picture(&l, &input, &format!("real_{}", if sizes.is_some() { "firm" } else { "saudi" }));
        eprintln!("the reviewer's basement, {sizes:?}: {} bays in {took:?}", l.stalls.len());
        assert_eq!(faults(&l, &solid), Vec::<String>::new());
        assert!(l.stalls.len() >= least, "{} bays: {:?}", l.stalls.len(), l.notes);
        assert_eq!(l.entries.len(), 2, "a way in off each ramp: {:?}", l.notes);
        assert!(l.notes.is_empty(), "{:?}", l.notes);
        for a in &l.aisles {
            for h in &solid {
                assert!(!overlap(&a.poly, h), "{:?} {:?} through {h:?}", a.kind, a.poly);
            }
        }
    }
}

#[test]
fn a_basement_100_by_60_on_a_grid_is_laid_out_in_well_under_a_second() {
    let mut keep = vec![bx(-0.1, -0.1, 100.1, 0.1), bx(99.9, -0.1, 100.1, 60.1), bx(-0.1, 59.9, 100.1, 60.1), bx(-0.1, -0.1, 0.1, 60.1)];
    for i in 1..13 {
        for y in [16.0, 32.0, 48.0] {
            let x = 8.1 * i as f64;
            keep.push(bx(x - 0.2, y - 0.2, x + 0.2, y + 0.2));
        }
    }
    let input = Input {
        site: bx(0.0, 0.0, 100.0, 60.0),
        basement: Some(Vec::new()),
        feet: vec![(([99.9, 5.0], [99.9, 11.0]), [-1.0, 0.0])],
        keep_clear: keep.clone(),
        settings: Given { stall_width: Some(2.5), stall_depth: Some(5.0), aisle: Some(6.0), column_every: Some(0.0), ..Given::default() },
        ..Input::default()
    };
    let t = std::time::Instant::now();
    let l = solve(&input);
    let took = t.elapsed();
    eprintln!("a basement 100 x 60 m on a grid: {} bays in {took:?}", l.stalls.len());
    assert_eq!(faults(&l, &keep), Vec::<String>::new());
    assert!(l.stalls.len() >= 200, "{} bays", l.stalls.len());
    let limit = if cfg!(debug_assertions) { 3000 } else { 1000 };
    assert!(took.as_millis() < limit, "{took:?}");
}

/// A picture of a layout (PARKING_PICTURE names the file): the ground grey, what stands on it black, aisles blue, ways in
/// green, bays outlined, accessible bays and access aisles orange.
fn picture(l: &Solved, input: &Input, name: &str) {
    let Some(dir) = std::env::var_os("PARKING_PICTURE") else { return };
    use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};
    let b = bounds(&input.site);
    let k = 1200.0 / (b.x1 - b.x0).max(b.y1 - b.y0);
    let (w, h) = (((b.x1 - b.x0) * k) as u32 + 40, ((b.y1 - b.y0) * k) as u32 + 40);
    let Some(mut pm) = Pixmap::new(w, h) else { return };
    pm.fill(Color::WHITE);
    let tf = |p: Pt| (20.0 + ((p[0] - b.x0) * k) as f32, (h as f64 - 20.0 - (p[1] - b.y0) * k) as f32);
    let mut draw = |r: &[Pt], rgb: [u8; 3], fill: bool| {
        let mut pb = PathBuilder::new();
        for (i, p) in r.iter().enumerate() {
            let (x, y) = tf(*p);
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        pb.close();
        let Some(path) = pb.finish() else { return };
        let mut paint = Paint::default();
        paint.set_color_rgba8(rgb[0], rgb[1], rgb[2], 255);
        if fill {
            pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        } else {
            pm.stroke_path(&path, &paint, &Stroke { width: 1.5, ..Stroke::default() }, Transform::identity(), None);
        }
    };
    draw(&input.site, [235, 235, 235], true);
    for a in &l.aisles {
        draw(&a.poly, if a.kind == super::fields::AisleKind::Throat { [150, 220, 150] } else { [190, 215, 245] }, true);
    }
    for r in input.keep_clear.iter().chain(input.obstacles.iter()) {
        draw(r, [40, 40, 40], true);
    }
    for r in input.basement.iter().flatten() {
        draw(r, [120, 120, 120], true);
    }
    for a in &l.access_aisles {
        draw(a, [250, 200, 140], true);
    }
    for st in &l.stalls {
        if st.kind == StallKind::Accessible {
            draw(&st.poly, [240, 150, 60], true);
        }
        draw(&st.poly, [90, 90, 90], false);
    }
    let _ = pm.save_png(std::path::Path::new(&dir).join(format!("{name}.png")));
}

// ---------------------------------------------------------------------------------------------------- the review's checks
/// A repeatable random basement: a rectangle, an L, a notched or a slanted outline in its walls; a column grid (most of
/// the time) at a random spacing; a core; one or two straight ramps along its walls, their feet into it.
fn random_basement(r: &mut Rnd) -> (Input, Vec<Ring>) {
    let w = 40.0 + r.next() * 70.0;
    let h = 30.0 + r.next() * 50.0;
    let site: Ring = match (r.next() * 4.0).floor() as i64 {
        0 => bx(0.0, 0.0, w, h),
        1 => {
            let (cw, ch) = (w * (0.35 + r.next() * 0.3), h * (0.35 + r.next() * 0.3));
            vec![[0.0, 0.0], [w, 0.0], [w, ch], [cw, ch], [cw, h], [0.0, h]]
        }
        2 => {
            let (nx, ny) = (w * (0.3 + r.next() * 0.3), h * (0.2 + r.next() * 0.2));
            vec![[0.0, 0.0], [w, 0.0], [w, h], [nx + 12.0, h], [nx + 12.0, h - ny], [nx, h - ny], [nx, h], [0.0, h]]
        }
        _ => vec![[0.0, 0.0], [w, 0.0], [w + 8.0 * r.next(), h * 0.6], [w * 0.8, h], [0.0, h]],
    };
    let mut solid: Vec<Ring> = Vec::new();
    if r.next() < 0.8 {
        let (sx, sy) = (7.5 + r.next() * 3.0, 7.5 + r.next() * 5.0);
        let c = 0.3 + r.next() * 0.3;
        let (mut x, x1) = (sx, w + 8.0);
        while x < x1 {
            let mut y = sy;
            while y < h {
                let post = bx(x - c / 2.0, y - c / 2.0, x + c / 2.0, y + c / 2.0);
                if post.iter().all(|p| point_in_ring(*p, &site)) && dist_to_ring([x, y], &site) > 1.0 {
                    solid.push(post);
                }
                y += sy;
            }
            x += sx;
        }
    }
    if r.next() < 0.5 {
        let (cx, cy) = (w * (0.2 + r.next() * 0.6), h * (0.2 + r.next() * 0.6));
        let core = bx(cx, cy, cx + 6.0, cy + 7.0);
        if core.iter().all(|p| point_in_ring(*p, &site)) {
            solid.retain(|p| !overlap(p, &core));
            solid.push(core);
        }
    }
    let mut ramps: Vec<Ring> = Vec::new();
    for k in 0..(1 + (r.next() * 2.0).floor() as usize) {
        // Along the south wall from the west, or along the west wall from the south.
        let len = 18.0 + r.next() * 8.0;
        let at = 0.15 + r.next() * 0.4;
        let ramp = if k == 0 { bx(w * at, 0.5, w * at + len, 6.5) } else { bx(0.5, h * at, 6.5, h * at + len) };
        if ramp.iter().all(|p| point_in_ring(*p, &site)) {
            // No column stands just off either end, where cars come off it (as a structure is drawn round a ramp).
            let b = bounds(&ramp);
            let clear = if k == 0 { bx(b.x0 - 8.0, b.y0, b.x1 + 8.0, b.y1) } else { bx(b.x0, b.y0 - 8.0, b.x1, b.y1 + 8.0) };
            solid.retain(|p| !overlap(p, &clear));
            ramps.push(ramp);
        }
    }
    let settings = if r.next() < 0.5 {
        Given { stall_width: Some(2.5), stall_depth: Some(5.0), aisle: Some(6.0), column_every: Some(0.0), ..Given::default() }
    } else {
        Given { column_every: Some(0.0), ..Given::default() }
    };
    let mut keep = solid.clone();
    keep.extend(ramps.iter().cloned());
    let input = Input { site, basement: Some(ramps.clone()), keep_clear: solid.clone(), settings, ..Input::default() };
    let mut all = solid;
    all.extend(ramps);
    let _ = keep;
    (input, all)
}

/// Random basements: no bay over a column, a core, a ramp or another bay, none off the ground, every bay opening onto
/// an aisle; no aisle through what stands; quick.
#[test]
fn random_basements_have_no_faults() {
    let mut r = Rnd(11.0);
    let mut bad = Vec::new();
    let mut worst = std::time::Duration::ZERO;
    let mut total = 0;
    for i in 0..40 {
        let (input, solid) = random_basement(&mut r);
        if std::env::var("PARKING_ONE").is_ok_and(|n| n != i.to_string()) {
            continue;
        }
        let t = std::time::Instant::now();
        let l = solve(&input);
        worst = worst.max(t.elapsed());
        total += l.stalls.len();
        let mut f = faults(&l, &solid);
        for a in l.aisles.iter().filter(|a| a.kind != super::fields::AisleKind::Throat) {
            for h in &solid {
                if shared_area(&a.poly, h) > 0.01 {
                    f.push(format!("an aisle through {:?}", centre(h)));
                }
            }
        }
        if std::env::var_os("PARKING_PICTURE").is_some() {
            picture(&l, &input, &format!("random_{i}"));
        }
        if !f.is_empty() {
            bad.push(format!("basement {i}: {}", f.join("; ")));
        }
        if std::env::var_os("PARKING_DEBUG_YIELD").is_some() {
            eprintln!("basement {i}: {} bays, notes {:?}", l.stalls.len(), l.notes);
        }
    }
    eprintln!("random basements: {total} bays, the slowest {worst:?}");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The biggest a basement gets: 200 by 150 m on an 8.1 m grid (over 400 columns), three ramps, a core.
#[test]
fn a_huge_basement_on_a_grid_is_laid_out_in_time() {
    let mut keep = Vec::new();
    for i in 1..25 {
        for j in 1..19 {
            let (x, y) = (8.1 * i as f64, 8.1 * j as f64);
            if x < 199.0 && y < 149.0 {
                keep.push(bx(x - 0.3, y - 0.3, x + 0.3, y + 0.3));
            }
        }
    }
    let core = bx(90.0, 70.0, 98.0, 78.0);
    keep.retain(|p| !overlap(p, &core));
    keep.push(core);
    let ramps = vec![bx(20.0, 0.5, 44.0, 6.5), bx(193.5, 60.0, 199.5, 84.0), bx(120.0, 143.5, 144.0, 149.5)];
    // Clear of the ramps and just off their ends.
    let clear = [bx(12.0, 0.5, 52.0, 6.5), bx(193.5, 52.0, 199.5, 92.0), bx(112.0, 143.5, 152.0, 149.5)];
    keep.retain(|p| clear.iter().all(|r| !overlap(p, r)));
    let mut kept = keep.clone();
    kept.extend(ramps.iter().cloned());
    let input = Input {
        site: bx(0.0, 0.0, 200.0, 150.0),
        basement: Some(Vec::new()),
        feet: vec![
            (([44.0, 0.5], [44.0, 6.5]), [1.0, 0.0]),
            (([193.5, 60.0], [199.5, 60.0]), [0.0, -1.0]),
            (([120.0, 143.5], [120.0, 149.5]), [-1.0, 0.0]),
        ],
        keep_clear: kept,
        settings: Given { column_every: Some(0.0), ..Given::default() },
        ..Input::default()
    };
    let t = std::time::Instant::now();
    let l = solve(&input);
    let took = t.elapsed();
    picture(&l, &input, "huge");
    eprintln!("a basement 200 x 150 m on a grid: {} bays in {took:?}, notes {:?}", l.stalls.len(), l.notes);
    let mut solid = keep;
    solid.extend(ramps);
    assert_eq!(faults(&l, &solid), Vec::<String>::new());
    assert!(l.stalls.len() >= 500, "{} bays", l.stalls.len());
    let limit = if cfg!(debug_assertions) { 15_000 } else { 5000 };
    assert!(took.as_millis() < limit, "{took:?}");
}

/// No aisle runs further past its last junction (a cross aisle, another aisle, a ramp's way, a turning head) than a dead
/// end may: a long basement with its ramp at one end gets its loops.
#[test]
fn dead_ends_are_no_longer_than_the_standard_allows() {
    for sizes in [None, Some((2.5, 5.0, 6.0))] {
        let given = match sizes {
            Some((w, d, a)) => Given { stall_width: Some(w), stall_depth: Some(d), aisle: Some(a), column_every: Some(0.0), ..Given::default() },
            None => Given { column_every: Some(0.0), ..Given::default() },
        };
        let input = Input {
            site: bx(0.0, 0.0, 120.0, 45.0),
            basement: Some(Vec::new()),
            feet: vec![(([0.1, 3.0], [0.1, 9.0]), [1.0, 0.0])],
            settings: given,
            ..Input::default()
        };
        let l = solve(&input);
        picture(&l, &input, &format!("dead_ends_{}", sizes.is_some()));
        assert!(l.stalls.len() > 100, "{} bays", l.stalls.len());
        let limit = l.settings.dead_end;
        let mut ways: Vec<&Ring> = l.aisles.iter().map(|a| &a.poly).collect();
        ways.extend(l.entries.iter().map(|e| &e.poly));
        for a in l.aisles.iter().filter(|a| a.kind == super::fields::AisleKind::Aisle) {
            let b = bbox(&a.poly);
            let along_x = b.x1 - b.x0 >= b.y1 - b.y0;
            let (lo, hi) = if along_x { (b.x0, b.x1) } else { (b.y0, b.y1) };
            let (mut first, mut last) = (f64::INFINITY, f64::NEG_INFINITY);
            for w in ways.iter().filter(|w| !std::ptr::eq(**w, &a.poly) && touch_along(w, &a.poly, 2.0)) {
                let q = bbox(w);
                let (p0, p1) = if along_x { (q.x0.max(lo), q.x1.min(hi)) } else { (q.y0.max(lo), q.y1.min(hi)) };
                first = first.min(p0);
                last = last.max(p1);
            }
            assert!(first.is_finite(), "an aisle with no junction: {:?}", a.poly);
            assert!(first - lo <= limit + 0.5 && hi - last <= limit + 0.5, "a dead end of {:.1} m and {:.1} m on {:?}", first - lo, hi - last, a.poly);
        }
    }
}

/// An L whose arms want their aisles at right angles (a long arm east, a long arm north, 2.5 x 5 m bays on 6 m aisles, its
/// ramp in the east arm): each arm laid out in its own direction, off a drive aisle at the corner.
#[test]
fn an_l_gets_each_arm_its_own_direction() {
    let site = vec![[0.0, 0.0], [90.0, 0.0], [90.0, 33.0], [33.0, 33.0], [33.0, 90.0], [0.0, 90.0]];
    let ramp = bx(84.0, 10.0, 90.0, 30.0);
    let input = Input {
        site: site.clone(),
        basement: Some(Vec::new()),
        feet: vec![(([84.0, 10.0], [90.0, 10.0]), [0.0, -1.0])],
        keep_clear: vec![ramp.clone()],
        settings: Given { stall_width: Some(2.5), stall_depth: Some(5.0), aisle: Some(6.0), column_every: Some(0.0), ..Given::default() },
        ..Input::default()
    };
    let l = solve(&input);
    picture(&l, &input, "l_split");
    eprintln!("an L: {} bays, directions {:?}, notes {:?}", l.stalls.len(), l.directions, l.notes);
    assert_eq!(faults(&l, &[ramp]), Vec::<String>::new());
    assert!(l.notes.is_empty(), "{:?}", l.notes);
    // One direction for both arms gets 185; each its own, 193.
    assert!(l.stalls.len() >= 190 && l.directions.len() == 2, "{} bays, {:?}", l.stalls.len(), l.directions);
}

/// One way round: every one-way aisle has a way on at its far end and a way in at its near end (a cross aisle, another
/// aisle, a ramp's way): no car is led into a dead end it may not turn back out of.
#[test]
fn one_way_aisles_always_lead_on() {
    let mut r = Rnd(5.0);
    let mut inputs: Vec<Input> = (0..12).map(|_| random_basement(&mut r).0).collect();
    inputs.push(real_basement(Some((2.5, 5.0, 6.0))).0);
    let mut bad = Vec::new();
    for (i, mut input) in inputs.into_iter().enumerate() {
        input.settings.one_way = Some(true);
        let l = solve(&input);
        let mut ways: Vec<&Ring> = l.aisles.iter().map(|a| &a.poly).collect();
        ways.extend(l.entries.iter().map(|e| &e.poly));
        for a in l.aisles.iter().filter(|a| a.kind == super::fields::AisleKind::Aisle) {
            let Some(f) = a.flow else { continue };
            let along = |p: Pt| p[0] * f[0] + p[1] * f[1];
            let (lo, hi) = a.poly.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(along(*p)), hi.max(along(*p))));
            let reach = l.settings.aisle + 0.5;
            let (mut ins, mut outs) = (false, false);
            for w in ways.iter().filter(|w| !std::ptr::eq(**w, &a.poly) && touch_along(w, &a.poly, 2.0)) {
                let (wlo, whi) = w.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(along(*p)), hi.max(along(*p))));
                ins |= wlo <= lo + reach;
                outs |= whi >= hi - reach;
            }
            if !(ins && outs) {
                bad.push(format!("basement {i}: an aisle {:?} running {f:?} with {}", centre(&a.poly), if outs { "no way in" } else { "no way on" }));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// What can't hold a car park lays out none and never panics: a tiny basement, one filled by a core, one that is all
/// columns, a sliver, a ramp's foot facing out of the area.
#[test]
fn degenerate_basements_never_panic() {
    let posts: Vec<Ring> = (0..40).flat_map(|i| (0..30).map(move |j| bx(i as f64 * 1.5, j as f64 * 1.5, i as f64 * 1.5 + 0.6, j as f64 * 1.5 + 0.6))).collect();
    let cases = [
        Input { site: bx(0.0, 0.0, 5.0, 5.0), basement: Some(Vec::new()), feet: vec![(([0.1, 1.0], [0.1, 4.0]), [1.0, 0.0])], ..Input::default() },
        Input { site: bx(0.0, 0.0, 40.0, 30.0), basement: Some(Vec::new()), keep_clear: vec![bx(0.5, 0.5, 39.5, 29.5)], ..Input::default() },
        Input { site: bx(0.0, 0.0, 60.0, 45.0), basement: Some(Vec::new()), keep_clear: posts, ..Input::default() },
        Input { site: bx(0.0, 0.0, 200.0, 4.0), basement: Some(Vec::new()), ..Input::default() },
        Input { site: bx(0.0, 0.0, 60.0, 40.0), basement: Some(Vec::new()), feet: vec![(([0.1, 10.0], [0.1, 16.0]), [-1.0, 0.0])], ..Input::default() },
        Input { site: vec![[0.0, 0.0], [60.0, 0.0], [60.0, 40.0], [30.0, 0.1], [0.0, 40.0]], basement: Some(Vec::new()), ..Input::default() },
    ];
    for (i, input) in cases.iter().enumerate() {
        if std::env::var("PARKING_ONE").is_ok_and(|n| n != i.to_string()) {
            continue;
        }
        let l = solve(input);
        if std::env::var_os("PARKING_DEBUG").is_some() {
            eprintln!("case {i}: parts {:?}", l.parts.iter().map(|p| (p.outer.len(), p.holes.len())).collect::<Vec<_>>());
        }
        assert_eq!(faults(&l, &input.keep_clear), Vec::<String>::new(), "case {i}");
        if i < 4 {
            assert!(l.stalls.len() <= 2, "case {i}: {} bays", l.stalls.len());
        }
    }
}

//! The car park solver tested as Arqiflow tests it (web/lib/parking/solver.test.ts and lab/check.ts, ported), and
//! against Arqiflow's own layouts (fixtures/, made by fixtures/make.ts from the TypeScript): the same bays, the same
//! accessible bays, every bay within 5 mm.

use serde_json::{json, Value};

use super::geom::*;
use super::solve::{solve, Access, AccessKind, Input, Solved};
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
        if let Some(f) = on.iter().find_map(|x| x.flow) {
            if on.iter().all(|x| x.flow.is_some()) && (back[0] - s.mouth[0]) * f[0] + (back[1] - s.mouth[1]) * f[1] < -0.05 {
                out.push(format!("a bay leans against its aisle's traffic at {}", at(&s.poly)));
            }
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
            [[0.0, 0.0], [w, 0.0], [w, ch], [cw, ch], [cw, h], [0.0, h]].iter().map(|&[x, y]: &[f64; 2]| [x * t.cos() - y * t.sin(), x * t.sin() + y * t.cos()]).collect()
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
        Input { obstacles: vec![bx(50.0, 28.0, 80.0, 106.0)], target: Some([50.0, 60.0]), ..plot(vec![[0.0, 0.0], [80.0, 0.0], [80.0, 100.0], [74.0, 106.0], [6.0, 106.0], [0.0, 100.0]], &[0, 1, 3, 5]) },
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
    let g = Region::new(vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0], [60.0, 50.0], [60.0, 100.0], [0.0, 100.0]], vec![bx(20.0, 20.0, 30.0, 30.0)]);
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
fn an_l_shaped_plot_is_set_out_as_two_fields() {
    let l = solve(&plot(vec![[0.0, 0.0], [90.0, 0.0], [90.0, 40.0], [40.0, 40.0], [40.0, 90.0], [0.0, 90.0]], &[0]));
    assert_eq!(l.metrics.fields, 2);
    assert!(l.metrics.stalls >= 150, "{}", l.metrics.stalls);
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
    let l = solve(&Input { obstacles: vec![bx(10.0, 45.0, 110.0, 70.0)], entries: vec![[60.0, 0.0]], ..plot(bx(0.0, 0.0, 120.0, 70.0), &[0]) });
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
    let l = solve(&Input { target: Some([40.0, 100.0]), settings: Given { ev_share: Some(0.1), ..Given::default() }, ..plot(bx(0.0, 0.0, 80.0, 100.0), &[0]) });
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
    let l = solve(&Input { obstacles: vec![bx(27.7, 20.6, 43.7, 32.6)], ..plot(vec![[0.0, 0.0], [68.0, 0.0], [71.4, 31.9], [63.5, 53.2], [11.6, 53.2], [0.0, 49.4]], &[0]) });
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
fn below_ground_with_no_ramp_drawn_one_is_placed() {
    let core = bx(36.0, 26.0, 44.0, 34.0);
    let l = solve(&Input { site: bx(0.0, 0.0, 80.0, 60.0), obstacles: vec![core.clone()], basement: Some(Vec::new()), ..Input::default() });
    assert_eq!(faults(&l, &[core]), Vec::<String>::new());
    assert_eq!(l.ramps.len(), 1);
    assert!(l.ramps[0].found && l.notes[0].contains("ramp"));
    assert!(!l.columns.is_empty() && l.metrics.stalls > 80, "{} bays, {} columns", l.metrics.stalls, l.columns.len());
}

#[test]
fn below_ground_with_a_ramp_drawn_its_foot_is_the_way_in() {
    let ramp = bx(0.0, 0.0, 7.0, 27.0);
    let l = solve(&Input { site: bx(0.0, 0.0, 90.0, 64.0), obstacles: vec![bx(40.0, 28.0, 48.0, 36.0)], basement: Some(vec![ramp.clone()]), ..Input::default() });
    assert_eq!(faults(&l, &[]), Vec::<String>::new());
    assert!(!l.ramps[0].found && l.entries.len() == 1);
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
}

// ---------------------------------------------------------------------------------------------------- Arqiflow's own layouts
fn fixtures() -> Vec<(String, Value)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/site/parking/fixtures");
    let mut names: Vec<_> = std::fs::read_dir(&dir).map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
    names.sort();
    names
        .into_iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(&p).ok()?;
            let v: Value = serde_json::from_str(&text).ok()?;
            Some((p.file_name()?.to_string_lossy().to_string(), v))
        })
        .collect()
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
        streets: v["streets"].as_array().map(|a| a.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect()).unwrap_or_default(),
        access: v["access"].as_array().map(|a| a.iter().filter_map(|x| Some(Access { at: pt_of(&x["at"]), kind: AccessKind::parse(x["kind"].as_str()?)? })).collect()).unwrap_or_default(),
        entries: v["entries"].as_array().map(|a| a.iter().map(pt_of).collect()).unwrap_or_default(),
        keep_clear: rings_of(&v["keepClear"]),
        removed: v["removed"].as_array().map(|a| a.iter().map(pt_of).collect()).unwrap_or_default(),
        target: v.get("target").filter(|t| t.is_array()).map(pt_of),
        direction: v["direction"].as_f64(),
        quick: v["quick"].as_bool().unwrap_or(false),
        basement: v.get("basement").filter(|b| b.is_object()).map(|b| rings_of(&b["ramps"])),
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
        out.push(format!("{}/{} accessible, Arqiflow {}/{}", l.metrics.accessible, l.metrics.accessible_needed, n("accessible"), n("accessibleNeeded")));
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
    out.extend(rings_differ("aisles", &l.aisles.iter().map(|a| a.poly.clone()).collect::<Vec<_>>(), &aisles.iter().map(|a| ring_of(&a["poly"])).collect::<Vec<_>>(), 0.005));
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
    let ours: Vec<String> = l.entries.iter().map(|e| format!("{} {} {}", e.kind.name(), to_fixed(e.at[0], 2), to_fixed(e.at[1], 2))).collect();
    let theirs: Vec<String> =
        entries.iter().map(|e| format!("{} {} {}", e["kind"].as_str().unwrap_or(""), to_fixed(pt_of(&e["at"])[0], 2), to_fixed(pt_of(&e["at"])[1], 2))).collect();
    if ours != theirs {
        out.push(format!("ways in {ours:?}, Arqiflow's {theirs:?}"));
    }
    let ramps = want["ramps"].as_array().cloned().unwrap_or_default();
    out.extend(rings_differ("ramps", &l.ramps.iter().map(|r| r.poly.clone()).collect::<Vec<_>>(), &ramps.iter().map(|r| ring_of(&r["poly"])).collect::<Vec<_>>(), 0.005));
    let notes: Vec<String> = want["notes"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
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
            "access" => Value::Array(v.as_array().map(|a| a.iter().map(|x| json!({"at": pt(&x["at"]), "kind": x["kind"]})).collect()).unwrap_or_default()),
            "stall_width" | "stall_depth" | "aisle" | "setback" | "end_island" | "column_size" | "floor_to_floor" => mm(&v),
            _ => v,
        };
        out.insert(k, nv);
    }
    out
}

fn car_park_layout(k: &Value) -> Layout {
    let levels: Vec<LevelIn> =
        k["levels"].as_array().map(|a| a.iter().map(|l| LevelIn { name: l["id"].as_str().unwrap_or("").into(), elevation: l["elevation"].as_f64().unwrap_or(0.0) * 1000.0 }).collect()).unwrap_or_default();
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
                    Obstacle { kind, level: e["level"].as_str().unwrap_or("").into(), outline: vec![Poly { outer: ring.iter().map(|p| P2(p[0] * 1000.0, p[1] * 1000.0)).collect(), holes: vec![] }] }
                })
                .collect()
        })
        .unwrap_or_default();
    let streets: Vec<(P2, P2)> = k["streets"]
        .as_array()
        .map(|a| a.iter().map(|s| (P2(s[0][0].as_f64().unwrap_or(0.0) * 1000.0, s[0][1].as_f64().unwrap_or(0.0) * 1000.0), P2(s[1][0].as_f64().unwrap_or(0.0) * 1000.0, s[1][1].as_f64().unwrap_or(0.0) * 1000.0))).collect())
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
            let ring: Vec<Pt> = p.outline.first().map(|o| o.outer.iter().map(|q| [q.0 / 1000.0, q.1 / 1000.0]).collect()).unwrap_or_default();
            let mut props = w["props"].clone();
            if let Some(o) = props.as_object_mut() {
                o.remove("band");
                if let Some(m) = o.get_mut("mouth") {
                    *m = json!([m[0].as_f64().unwrap_or(0.0) * 1000.0, m[1].as_f64().unwrap_or(0.0) * 1000.0]);
                }
            }
            let same_props = p.props.iter().all(|(k, v)| match (v.as_array(), props[k].as_array()) {
                (Some(a), Some(b)) => a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x.as_f64().unwrap_or(0.0) - y.as_f64().unwrap_or(1.0)).abs() < 5.0),
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
        if l.result.get(k).map(|v| v.as_f64().map(|x| json!(x)).unwrap_or(v.clone())) != Some(r[k].as_f64().map(|x| json!(x)).unwrap_or(r[k].clone())) {
            out.push(format!("{k}: {:?}, Arqiflow's {}", l.result.get(k), r[k]));
        }
    }
    out
}

#[test]
fn the_layouts_match_arqiflows() {
    let cases = fixtures();
    assert!(cases.len() >= 50, "{} fixtures", cases.len());
    let mut report = Vec::new();
    let mut matched = 0;
    for (name, case) in &cases {
        let diffs = if let Some(input) = case.get("input") {
            compare_solved(&solve(&input_from(input)), &case["out"])
        } else {
            compare_layout(&car_park_layout(&case["car_park"]), case)
        };
        if diffs.is_empty() {
            matched += 1;
        } else {
            report.push(format!("{name} ({}): {}", case["name"], diffs.join("; ")));
        }
    }
    assert!(report.is_empty(), "{matched} of {} match Arqiflow's; these do not:\n{}", cases.len(), report.join("\n"));
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
fn lay_out_below_ground_places_a_ramp_and_columns() {
    let props = mm_props(json!({ "control": [[0, 0], [80000, 0], [80000, 60000], [0, 60000]] }));
    let levels = [LevelIn { name: "B1".into(), elevation: -3500.0 }, LevelIn { name: "L0".into(), elevation: 0.0 }];
    let core = Obstacle { kind: "mass".into(), level: "B1".into(), outline: vec![Poly { outer: vec![P2(36000.0, 26000.0), P2(44000.0, 26000.0), P2(44000.0, 34000.0), P2(36000.0, 34000.0)], holes: vec![] }] };
    let elsewhere = Obstacle { kind: "mass".into(), level: "L0".into(), outline: vec![Poly { outer: vec![P2(0.0, 0.0), P2(80000.0, 0.0), P2(80000.0, 60000.0)], holes: vec![] }] };
    let l = lay_out(&props, "B1", &levels, &[core, elsewhere], &[]);
    assert_eq!(l.result["basement"], json!(true));
    assert!(l.pieces.iter().any(|p| p.kind == "aisle" && p.props["part"] == "ramp"));
    assert!(l.pieces.iter().filter(|p| p.kind == "column").count() > 0);
    assert!(l.result["stalls"].as_u64().unwrap_or(0) > 80);
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
        let l = lay_out(&mm_props(p), "L", &[LevelIn { name: "L".into(), elevation: f64::NAN }], &[], &[(P2(f64::NAN, 0.0), P2(0.0, f64::INFINITY))]);
        assert!(l.result.contains_key("stalls"));
    }
}

#[test]
fn a_large_car_park_is_quick() {
    if cfg!(debug_assertions) {
        return;
    }
    let props = mm_props(json!({ "control": [[0, 0], [200000, 0], [200000, 150000], [0, 150000]] }));
    let t = std::time::Instant::now();
    let l = lay_out(&props, "L", &[], &[], &[(P2(0.0, 0.0), P2(200000.0, 0.0))]);
    assert!(l.result["stalls"].as_u64().unwrap_or(0) > 800);
    assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
}

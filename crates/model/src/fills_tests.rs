//! `fills`'s tests that need only the model.
use crate::fills::*;
use crate::geom::P2;

fn square(x: f64, y: f64, s: f64) -> Vec<P2> {
    vec![P2(x, y), P2(x + s, y), P2(x + s, y + s), P2(x, y + s)]
}

const ALL: (P2, P2) = (P2(-1e6, -1e6), P2(1e6, 1e6));

#[test]
fn a_family_of_lines_fills_inside_the_outline_and_not_its_hole() {
    let p = FillPattern { target: Target::Model, lines: vec![HatchLine::simple(0.0, 100.0)], drawn: None };
    let rings = vec![square(0.0, 0.0, 1000.0), square(400.0, 400.0, 200.0)];
    let s = strokes(&p, &rings, 1.0, ALL, 10_000).unwrap();
    // Lines at y = 0 … 900 (an edge's lower side is inside, its upper side out, so 1000 is left out); those through the
    // hole broken round it.
    let at = |y: f64| s.iter().filter(|l| (l[0].1 - y).abs() < 1e-9).count();
    assert_eq!((at(0.0), at(100.0), at(500.0), at(1000.0)), (1, 1, 2, 0));
    let length: f64 = s.iter().map(|l| l[0].dist(l[1])).sum();
    assert!((length - (10.0 * 1000.0 - 2.0 * 200.0)).abs() < 1e-6, "{length}");
    assert!(s.iter().all(|l| !(l[0].0 > 400.0 && l[1].0 < 600.0 && l[0].1 > 400.0 && l[0].1 < 600.0)), "none inside the hole");
}

#[test]
fn a_drafting_patterns_lengths_grow_with_the_scale_and_dashes_and_dots_repeat_along_it() {
    let p = FillPattern {
        target: Target::Drafting,
        lines: vec![HatchLine { dashes: vec![2.0, -1.0, 0.0, -1.0], ..HatchLine::simple(0.0, 1.0) }],
        drawn: None,
    };
    let ring = vec![square(0.0, 0.0, 400.0)];
    // At 1 : 100 a mm of paper is 100 mm: lines 100 apart, a 200 dash, a dot, every 400.
    let s = strokes(&p, &ring, 100.0, ALL, 10_000).unwrap();
    let row: Vec<&[P2; 2]> = s.iter().filter(|l| (l[0].1 - 100.0).abs() < 1e-9).collect();
    assert_eq!(row.len(), 2, "{row:?}");
    assert!((row[0][1].0 - row[0][0].0 - 200.0).abs() < 1e-9 && (row[1][1].0 - row[1][0].0 - 15.0).abs() < 1e-9, "{row:?}");
    assert_eq!(s.iter().map(|l| l[0].1).fold(0.0, f64::max), 300.0);
}

#[test]
fn running_bond_staggers_its_joints_half_a_brick_course_by_course() {
    let brick = &default_patterns()["Brick 225 x 75"];
    let s = strokes(brick, &[square(1.0, 1.0, 1000.0)], 1.0, ALL, 100_000).unwrap();
    let joints = |y: f64| {
        let mut x: Vec<f64> =
            s.iter().filter(|l| l[0].0 == l[1].0 && l[0].1.min(l[1].1) < y && l[0].1.max(l[1].1) > y).map(|l| l[0].0).collect();
        x.sort_by(f64::total_cmp);
        x
    };
    let (first, second) = (joints(40.0), joints(115.0));
    assert!(first.iter().all(|x| (x / 225.0).fract().abs() < 1e-9 || (x / 225.0).fract() > 1.0 - 1e-9), "{first:?}");
    assert!(second.iter().all(|x| ((x - 112.5) / 225.0).fract().abs() < 1e-9), "{second:?}");
}

#[test]
fn too_dense_a_pattern_draws_nothing_rather_than_too_much() {
    let p = &default_patterns()["Diagonal Up"];
    assert_eq!(strokes(p, &[square(0.0, 0.0, 1e6)], 0.001, ALL, 10_000), None);
    assert!(strokes(&default_patterns()[SOLID_FILL], &[square(0.0, 0.0, 10.0)], 1.0, ALL, 10).is_some_and(|s| s.is_empty()));
}

#[test]
fn patterns_are_checked_and_a_simple_one_is_read_back_as_its_angle_spacing_and_crosshatch() {
    assert_eq!(default_patterns()["Crosshatch"].simple(), Some((0.0, 2.0, true)));
    assert_eq!(default_patterns()["Diagonal Down"].simple(), Some((135.0, 1.5, false)));
    assert_eq!(default_patterns()["Brick 225 x 75"].simple(), None);
    for (n, p) in default_patterns() {
        assert_eq!(p.check(&n), Ok(()));
    }
    let bad = FillPattern { target: Target::Drafting, lines: vec![HatchLine::simple(0.0, 0.0)], drawn: None };
    assert!(bad.check("x").unwrap_err().contains("apart"));
    let gapless = FillPattern { lines: vec![HatchLine { dashes: vec![1.0, 2.0], ..HatchLine::simple(0.0, 1.0) }], ..Default::default() };
    assert!(gapless.check("x").unwrap_err().contains("space"));
    for (n, t) in default_region_types() {
        assert_eq!(t.check(&n, &default_patterns()), Ok(()));
    }
}

/// What `f` did, in [`crate::fills::done`]'s steps.
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let before = crate::fills::DONE.with(std::cell::Cell::get);
    (f(), crate::fills::DONE.with(std::cell::Cell::get) - before)
}

#[test]
fn a_pattern_over_more_than_the_view_shows_draws_only_what_the_view_shows() {
    // Sand at 1 : 100 over 200 m square, seen through a window 20 by 15 m: 200 rows of 200 dots, not 2000 dots in each of them.
    let sand = &default_patterns()["Sand"];
    let window = (P2(90_000.0, 90_000.0), P2(110_000.0, 105_000.0));
    let s = strokes(sand, &[square(0.0, 0.0, 200_000.0)], 100.0, window, 50_000).expect("what the window holds is under the cap");
    assert!((30_000..=50_000).contains(&s.len()), "{}", s.len());
    assert!(s.iter().all(|l| l.iter().all(|q| q.0 >= 90_000.0 - 1e-6 && q.0 <= 110_000.0 + 1e-6)), "none past the window");
}

#[test]
fn a_dense_pattern_over_an_outline_of_thousands_of_corners_is_refused_in_time() {
    let p =
        FillPattern { target: Target::Model, lines: (0..32).map(|i| HatchLine::simple(f64::from(i) * 5.0, 20.0)).collect(), drawn: None };
    let ring: Vec<P2> = (0..10_000)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 10_000.0;
            P2(1e5 * a.cos(), 1e5 * a.sin())
        })
        .collect();
    let (r, steps) = counted(|| strokes(&p, &[ring], 1.0, (P2(-1e5, -1e5), P2(1e5, 1e5)), 50_000));
    assert_eq!(r, None);
    // Refused from the count of lines times the count of edges, before any line was tried against one.
    assert_eq!(steps, 0, "{steps} steps");
}

#[test]
fn dots_a_twentieth_of_a_millimetre_apart_cost_a_bounded_amount() {
    let p = FillPattern {
        target: Target::Model,
        lines: vec![HatchLine { dashes: vec![0.0, -0.05], ..HatchLine::simple(0.0, 10_000.0) }],
        drawn: None,
    };
    assert_eq!(p.check("dots"), Ok(()));
    let (r, steps) = counted(|| strokes(&p, &[square(0.0, 0.0, 100_000.0)], 1.0, (P2(-1.0, -1.0), P2(101_000.0, 101_000.0)), 50_000));
    assert_eq!(r, None);
    assert!(steps <= crate::fills::MAX_WORK, "{steps} steps");
}

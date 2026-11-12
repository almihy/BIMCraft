//! `sketch`'s tests that need only the model.

/// A regular `n`-gon 10 m across to the micron, with the corners `swap` and the next swapped: a small bow tie in a ring of
/// `n - 3` edges of one length.
fn regular_bow(n: usize, at: [f64; 2], swap: usize) -> Vec<[f64; 2]> {
    let micron = |v: f64| (v * 1000.0).round() / 1000.0;
    let mut ring: Vec<[f64; 2]> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [at[0] + micron(5000.0 * a.cos()), at[1] + micron(5000.0 * a.sin())]
        })
        .collect();
    ring.swap(swap, swap + 1);
    ring
}

#[test]
fn comparing_two_outlines_of_thousands_of_equal_edges_stops_at_its_budget() {
    // Rings of 5,000 edges of one length, a bow tie at the corner 4,000: every start passes the first corner and runs on for
    // thousands before it fails, which is tens of millions of corners over the starts.
    let points = |ring: Vec<[f64; 2]>| ring.into_iter().map(|p| crate::geom::P2(p[0], p[1])).collect::<Vec<_>>();
    let a = points(regular_bow(5000, [0.0, 0.0], 4000));
    // The same ring with its bow tie at another corner is found within the budget.
    let (mut work, mut same) = (0, points(regular_bow(5000, [0.0, 0.0], 4500)));
    assert!(crate::sketch::congruent(&a, &same, &mut work), "{work} corners compared");
    // With a second bow tie it is another ring, and the budget is spent before every start is tried.
    same.swap(1000, 1001);
    let (mut work, most) = (0, crate::sketch::MAX_COMPARED);
    assert!(!crate::sketch::congruent(&a, &same, &mut work));
    assert!((most..=most + a.len()).contains(&work), "{work} corners compared, of a budget of {most}");
    // Once it is spent, a comparison looks at nothing.
    let spent = work;
    assert!(!crate::sketch::congruent(&a, &a, &mut work));
    assert_eq!(work, spent);
}

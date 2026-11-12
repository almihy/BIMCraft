//! IT 22's Tabelas 1, 2 and 3 as printed, and the pipe checks by hand.

use super::*;

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

#[test]
fn tabela_2_is_as_printed() {
    let t = |k: &str| kind(k).map(|t| (t.1, t.2, t.5, t.6));
    assert_eq!(t("1"), Some((1, 25.0, 100.0, 80.0)));
    assert_eq!(t("2"), Some((2, 40.0, 150.0, 30.0)));
    assert_eq!(t("3"), Some((3, 40.0, 200.0, 40.0)));
    assert_eq!(t("4-40"), Some((4, 40.0, 300.0, 65.0)));
    assert_eq!(t("4-65"), Some((4, 65.0, 300.0, 30.0)));
    assert_eq!(t("5"), Some((5, 65.0, 600.0, 60.0)));
    assert_eq!(TYPE_KEYS.len(), TYPES.len());
    assert!(TYPE_KEYS.iter().zip(TYPES).all(|(k, t)| *k == t.0));
    assert!(kind("6").is_none());
}

#[test]
fn tabela_3_is_as_printed() {
    // (column, area, type, reserve m3)
    for (c, a, ty, r) in [
        (1, 100.0, 1, 5.0),
        (1, 2500.0, 1, 5.0),
        (1, 2500.5, 1, 8.0),
        (1, 60000.0, 1, 35.0),
        (2, 10000.0, 2, 18.0),
        (3, 20000.0, 3, 35.0),
        (3, 50001.0, 3, 70.0),
        (4, 5000.0, 4, 32.0),
        (4, 50000.0, 4, 96.0),
        (5, 5000.0, 4, 48.0),
        (5, 5001.0, 5, 64.0),
        (5, 60000.0, 5, 180.0),
    ] {
        assert_eq!(system_for(c, a), Some((ty, r)), "column {c}, {a} m2");
    }
    assert_eq!(system_for(0, 100.0), None);
    assert_eq!(system_for(6, 100.0), None);
    assert_eq!(system_for(1, 0.0), None);
    assert_eq!(system_for(1, f64::NAN), None);
}

#[test]
fn the_reserve_never_falls_with_the_area() {
    for c in 1..=5u8 {
        let mut last = 0.0;
        for a in [100.0, 2500.0, 5000.0, 10000.0, 20000.0, 50000.0, 90000.0] {
            let r = system_for(c, a).map_or(f64::NAN, |x| x.1);
            assert!(r >= last, "column {c} at {a}");
            last = r;
        }
    }
}

#[test]
fn hazen_williams_by_hand() {
    // 600 L/min, C 120, 68 mm inside: 605 * 600^1.85 * 120^-1.85 * 68^-4.87 * 1e4
    assert!(near(hazen_j(600.0, 120.0, 68.0), 0.14143, 1e-4));
    assert!(near(hazen_j(300.0, 150.0, 53.0), 0.08739, 1e-4));
    assert!(near(speed(600.0, 68.0), 2.7535, 1e-3));
    assert!(hazen_j(600.0, 120.0, 100.0) < hazen_j(600.0, 120.0, 68.0), "a larger pipe loses less");
    assert_eq!(hazen_c("steel"), Some(100.0), "black steel in a dry system: the lowest steel row");
    assert_eq!(hazen_c("steel-wet"), Some(120.0));
    assert_eq!(hazen_c("copper"), Some(150.0));
    assert_eq!(hazen_c("lead"), None);
}

#[test]
fn two_jets_at_most() {
    assert_eq!(design_flow("5", 1), 600.0);
    assert_eq!(design_flow("5", 2), 1200.0);
    assert_eq!(design_flow("5", 7), 1200.0);
    assert_eq!(design_flow("2", 2), 300.0);
    assert_eq!(design_flow("2", 0), 0.0);
    assert_eq!(design_flow("9", 2), 0.0);
    assert_eq!(min_nominal("1"), 50.0);
    assert_eq!(min_nominal("2"), 50.0);
    assert_eq!(min_nominal("3"), 65.0);
    assert_eq!(min_nominal("5"), 65.0);
}

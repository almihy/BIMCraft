//! The design flow, the head loss and the PVC tables.

use super::*;

fn near(a: f64, b: f64, e: f64) -> bool {
    (a - b).abs() <= e
}

#[test]
fn the_design_flow_follows_the_method() {
    let f = [("lav", 1.0), ("wc", 1.0), ("shower", 1.0)];
    assert!(near(design_flow(Method::Weights, &f), 0.3, 1e-9), "0.3 sqrt(1.0)");
    assert!(near(design_flow(Method::Sum, &f), 0.5, 1e-9));
    assert!(near(design_flow(Method::Weights, &[("lav", 1.0)]), 0.15, 1e-9), "never more than the fixtures' own");
    assert!(near(design_flow(Method::Weights, &[("sink", 2.0)]), 0.3 * 1.4f64.sqrt(), 1e-9));
    assert_eq!(design_flow(Method::Weights, &[]), 0.0);
}

#[test]
fn the_head_loss_is_darcy_weisbach() {
    let (v, j) = head_loss(0.3, 21.6, EPS_PVC);
    assert!(near(v, 0.3e-3 / (std::f64::consts::PI * 0.0216 * 0.0216 / 4.0), 1e-9));
    let re = v * 0.0216 / NU;
    assert!(re > 17000.0 && re < 18000.0);
    let f = colebrook(re, EPS_PVC / 21.6);
    assert!(f > 0.026 && f < 0.028, "{f}");
    assert!(near(j, f / 0.0216 * v * v / (2.0 * G), 1e-12));
    assert_eq!(head_loss(0.0, 21.6, EPS_PVC), (0.0, 0.0));
    assert!(near(colebrook(1000.0, 1e-4), 0.064, 1e-12), "laminar: 64 / Re");
}

#[test]
fn the_pvc_tables_are_as_printed() {
    assert_eq!(inside(25.0), Some(21.6));
    assert!(inside(110.0).is_some_and(|d| near(d, 97.8, 1e-9)));
    assert_eq!(inside(30.0), None);
    assert_eq!(equivalent_length(25.0, "elbow90"), Some(1.2));
    assert_eq!(equivalent_length(110.0, "check"), Some(10.4));
    assert_eq!(equivalent_length(25.0, "tap"), None);
    assert_eq!(equivalent_length(30.0, "gate"), None);
    for f in FITTINGS {
        assert!(PVC.iter().all(|p| equivalent_length(p.0, f).is_some()), "{f}");
    }
}

#[test]
fn the_smallest_dn_keeps_the_speed_within_three_metres_per_second() {
    assert_eq!(smallest_dn(0.15), Some(20.0), "0.15 L/s at 17 mm inside: 0.66 m/s");
    let dn = smallest_dn(1.5).unwrap_or(0.0);
    let d = inside(dn).unwrap_or(0.0);
    assert!(head_loss(1.5, d, EPS_PVC).0 <= MAX_SPEED);
    let smaller = PVC.iter().map(|p| p.0).rfind(|x| *x < dn).and_then(inside).unwrap_or(0.0);
    assert!(head_loss(1.5, smaller, EPS_PVC).0 > MAX_SPEED, "the one below is too fast");
    assert_eq!(smallest_dn(100.0), None, "no DN of the table carries it");
}

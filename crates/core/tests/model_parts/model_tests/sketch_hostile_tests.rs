//! `sketch`'s tests that drive the Session (moved out of `sketch.rs`).

use std::time::Instant;

use serde_json::{Value, json};

use crate::engine::Session;

/// Runs `id` and says how many steps the sketches it solved took ([`crate::sketch::SPENT`]). Not how long: that varies with the
/// load of the machine, steps do not (the seconds go to stderr, for whoever reads them).
fn timed(s: &mut Session, id: &str, p: &Value) -> (Result<Value, String>, usize) {
    let (t, before) = (Instant::now(), crate::sketch::SPENT.with(std::cell::Cell::get));
    let r = s.execute(id, p);
    let steps = crate::sketch::SPENT.with(std::cell::Cell::get) - before;
    eprintln!("{id}: {steps} steps, {:.3} s", t.elapsed().as_secs_f64());
    (r, steps)
}

/// `n` concentric circles from radius `r0`, `step` mm apart, as sketch lines.
fn circles(n: usize, r0: f64, step: f64) -> Value {
    Value::Array((0..n).map(|k| json!({"centre": [0, 0], "radius": r0 + step * k as f64})).collect())
}

#[test]
fn ten_thousand_parallel_lines_are_named_open_at_once() {
    let lines: Vec<Value> = (0..10_000).map(|i| json!([[0, i * 5], [1000, i * 5]])).collect();
    let (r, steps) = timed(&mut Session::new(), "sketch", &json!({"lines": lines}));
    assert!(r.unwrap_err().contains("open"));
    assert!(steps <= 700_000, "{steps} steps");
}

#[test]
fn five_rings_of_ten_thousand_points_nest_at_once() {
    let ring = |r: f64| -> Value {
        let pts: Vec<Value> = (0..10_000)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 10_000.0;
                json!([(r * a.cos()).round(), (r * a.sin()).round()])
            })
            .collect();
        json!({"points": pts, "closed": true})
    };
    let lines: Vec<Value> = (0..5).map(|k| ring(1.0e6 + 2.0e4 * k as f64)).collect();
    let (r, steps) = timed(&mut Session::new(), "sketch", &json!({"lines": lines}));
    assert!(r.is_ok(), "{r:?}");
    assert!(steps <= 1_500_000, "{steps} steps");
}

#[test]
fn concentric_circles_at_the_cap_solve_at_once_and_past_it_are_refused_at_once() {
    let mut s = Session::new();
    let (r, steps) = timed(&mut s, "sketch", &json!({"lines": circles(10, 1.0e6, 1000.0)}));
    assert_eq!(r.map(|v| v["regions"].as_array().map(Vec::len)), Ok(Some(5)));
    assert!(steps <= 2_000_000, "{steps} steps");
    let (r, steps) = timed(&mut s, "sketch", &json!({"lines": circles(40, 1.0e6, 1000.0)}));
    assert!(r.is_ok() || r.as_ref().is_err_and(|e| e.contains("pieces")), "{r:?}");
    assert!(steps <= 14_000_000, "{steps} steps");
    let (r, steps) = timed(&mut s, "sketch", &json!({"lines": circles(100, 1.0e6, 1000.0)}));
    assert!(r.unwrap_err().contains("pieces"));
    assert!(steps <= 1_000, "{steps} steps");
}

/// `n` rectangles inside one another, 20 mm apart, each a closed polyline of 640 points, as sketch lines.
fn nested_rectangles(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(|k| {
                let h = 1000.0 + 20.0 * k as f64;
                // 160 points along each side, going round counter-clockwise from the south-west corner.
                let side = |from: [f64; 2], to: [f64; 2]| {
                    (0..160).map(move |i| {
                        json!([from[0] + (to[0] - from[0]) * f64::from(i) / 160.0, from[1] + (to[1] - from[1]) * f64::from(i) / 160.0])
                    })
                };
                let corners = [[-h, -h], [h, -h], [h, h], [-h, h]];
                let pts: Vec<Value> = (0..4).flat_map(|c| side(corners[c], corners[(c + 1) % 4])).collect();
                json!({"points": pts, "closed": true})
            })
            .collect(),
    )
}

#[test]
fn rectangles_of_six_hundred_points_inside_one_another_are_refused_not_waited_for() {
    // 180,000 pieces, each loop inside every one outside it: 25 million points to ask about.
    let (r, steps) = timed(&mut Session::new(), "sketch", &json!({"lines": nested_rectangles(280)}));
    let e = r.unwrap_err();
    assert!(e.contains("nested too deeply"), "{e}");
    assert!(steps <= 36_000_000, "{steps} steps");
    // Half as many are a quarter of that: solved, every other one a hole.
    let (r, steps) = timed(&mut Session::new(), "sketch", &json!({"lines": nested_rectangles(140)}));
    assert_eq!(r.map(|v| v["regions"].as_array().map(Vec::len)), Ok(Some(70)));
    assert!(steps <= 16_000_000, "{steps} steps");
}

#[test]
fn a_hundred_wide_arcs_are_solved_or_refused_at_once() {
    let arcs: Vec<Value> = (0..100).map(|k| json!([[0, k], [2.0e6, k], [1.0e6, 1.0e6 + k as f64]])).collect();
    let (r, steps) = timed(&mut Session::new(), "sketch", &json!({"lines": arcs}));
    assert!(r.is_err());
    assert!(steps <= 1_000, "{steps} steps");
}

#[test]
fn a_floor_of_ten_thousand_corners_stays_quick() {
    let pts: Vec<Value> = (0..10_000)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / 10_000.0;
            json!([(1.0e6 * a.cos()).round(), (1.0e6 * a.sin()).round()])
        })
        .collect();
    let (r, steps) = timed(&mut Session::new(), "floor", &json!({"boundary": pts}));
    assert!(r.is_ok(), "{r:?}");
    assert!(steps <= 200_000, "{steps} steps");
}

#[test]
fn ten_thousand_parallel_diagonals_are_refused_not_waited_for() {
    // Every box holds every other: fifty million pairs to look at, none of them crossing.
    let lines: Vec<Value> = (0..10_000).map(|i| json!([[0, i * 100], [1.0e6, 1.0e6 + i as f64 * 100.0]])).collect();
    let (r, steps) = timed(&mut Session::new(), "sketch", &json!({"lines": lines}));
    let e = r.unwrap_err();
    assert!(e.contains("too close together"), "{e}");
    assert!(steps <= 12_000_000, "{steps} steps");
}

#[test]
fn a_sketch_kept_with_too_many_pieces_is_refused_by_the_model_as_the_command_refuses_it() {
    let mut s = Session::new();
    let f = s.execute("floor", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]]})).unwrap()["id"].clone();
    let (r, steps) = timed(&mut s, "modify", &json!({"id": f, "sketch": circles(100, 1.0e6, 1000.0)}));
    assert!(r.unwrap_err().contains("straight pieces"));
    assert!(steps <= 1_000, "{steps} steps");
}

#[test]
fn twenty_circles_in_a_filled_region_solve_at_once() {
    let (r, steps) = timed(&mut Session::new(), "filled_region", &json!({"sketch": circles(20, 1.0e6, 1000.0)}));
    assert!(r.is_ok(), "{r:?}");
    assert!(steps <= 10_000_000, "{steps} steps");
}

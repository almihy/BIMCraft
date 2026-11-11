//! What walls, floors, ceilings and roofs measure, against figures worked by hand: a bill of quantities is built from these
//! numbers, so each one is checked against a sum a person can do on paper, to a hundredth of a percent or better.

use std::f64::consts::PI;

use serde_json::json;

use crate::engine::Session;
use crate::graphics::testing::run;
use crate::quantities::{self, WallQ};

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// A project with the wall type T (200 mm thick) and the openings D (a door 1000 x 2100), W (a window 1200 x 1500, its sill at
/// 900), Tall (1200 x 2000, sill 900) and Small (600 x 900, sill 1100).
fn project() -> Session {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "T", "thickness": 200}));
    run(&mut s, "type", json!({"category": "door", "name": "D", "width": 1000, "height": 2100}));
    run(&mut s, "type", json!({"category": "window", "name": "W", "width": 1200, "height": 1500, "sill": 900}));
    run(&mut s, "type", json!({"category": "window", "name": "Tall", "width": 1200, "height": 2000, "sill": 900}));
    run(&mut s, "type", json!({"category": "window", "name": "Small", "width": 600, "height": 900, "sill": 1100}));
    s
}

/// A 3000 mm high wall of type T along `points`' first segment: its id.
fn wall(s: &mut Session, from: [i32; 2], to: [i32; 2]) -> u64 {
    run(s, "wall", json!({"points": [from, to], "type": "T", "height": 3000}))["ids"][0].as_u64().unwrap_or_default()
}

fn put(s: &mut Session, what: &str, wall: u64, offset: f64, kind: &str) -> u64 {
    run(s, what, json!({"wall": wall, "offset": offset, "type": kind}))["id"].as_u64().unwrap_or_default()
}

fn measure(s: &Session, id: u64) -> WallQ {
    quantities::wall(&s.model, id).expect("a wall")
}

/// The wall's figures, in m² and m³ (side gross, side net, volume gross, volume net).
fn figures(q: &WallQ) -> [f64; 4] {
    [q.gross_side / 1e6, q.net_side / 1e6, q.gross_volume / 1e9, q.net_volume / 1e9]
}

fn assert_figures(q: &WallQ, want: [f64; 4]) {
    let got = figures(q);
    assert!(got.iter().zip(want).all(|(g, w)| near(*g, w)), "{got:?} is not {want:?}: {q:?}");
}

#[test]
fn a_wall_with_no_openings_is_its_length_by_its_height() {
    let mut s = project();
    let w = wall(&mut s, [0, 0], [8000, 0]);
    let q = measure(&s, w);
    // 8 m by 3 m is 24 m² a side; through 200 mm, 4.8 m³.
    assert_figures(&q, [24.0, 24.0, 4.8, 4.8]);
    assert_eq!((q.length, q.width, q.height), (8000.0, 200.0, Some(3000.0)));
}

#[test]
fn a_door_half_past_the_walls_end_cuts_only_the_half_in_the_wall() {
    let mut s = project();
    let w = wall(&mut s, [0, 0], [5000, 0]);
    // 1 m wide, centred on the end: 500 mm of it in the wall, 2100 high: 1.05 m².
    put(&mut s, "door", w, 5000.0, "D");
    let q = measure(&s, w);
    assert_figures(&q, [15.0, 15.0 - 1.05, 3.0, (15.0 - 1.05) * 0.2]);
    // And the same at the start.
    let w = wall(&mut s, [0, 4000], [5000, 4000]);
    put(&mut s, "door", w, 0.0, "D");
    assert_figures(&measure(&s, w), [15.0, 15.0 - 1.05, 3.0, (15.0 - 1.05) * 0.2]);
    // One wholly past the end cuts nothing.
    let w = wall(&mut s, [0, 8000], [5000, 8000]);
    put(&mut s, "door", w, 6000.0, "D");
    assert_figures(&measure(&s, w), [15.0, 15.0, 3.0, 3.0]);
}

#[test]
fn two_overlapping_windows_cut_what_they_cover_once() {
    let mut s = project();
    let w = wall(&mut s, [0, 0], [8000, 0]);
    // 1200 wide at 3000 and at 3600 along: 2400 to 3600, and 3000 to 4200. Together 2400 to 4200, 1800 wide, 1500 high.
    put(&mut s, "window", w, 3000.0, "W");
    put(&mut s, "window", w, 3600.0, "W");
    let cut = 1.8 * 1.5;
    assert_figures(&measure(&s, w), [24.0, 24.0 - cut, 4.8, (24.0 - cut) * 0.2]);
}

#[test]
fn a_window_wholly_inside_a_doors_span_is_not_counted_again() {
    let mut s = project();
    let w = wall(&mut s, [0, 0], [8000, 0]);
    // A door 2500 to 3500 along, 2100 high; a window 2700 to 3300 along and 1100 to 2000 up, inside it.
    put(&mut s, "door", w, 3000.0, "D");
    put(&mut s, "window", w, 3000.0, "Small");
    let q = measure(&s, w);
    assert_figures(&q, [24.0, 24.0 - 2.1, 4.8, (24.0 - 2.1) * 0.2]);
    // Over the door there is wall from its head up (2100), and none through it: what the window held in the door's span is no
    // wall standing again.
    let over_the_door = crate::walls::prisms(&s.model, w, true)
        .iter()
        .filter(|p| p.ring.iter().all(|q| q.0 >= 2500.0 - 1e-6 && q.0 <= 3500.0 + 1e-6))
        .filter(|p| p.top.at(p.ring[0]) - p.bottom.at(p.ring[0]) > 1.0)
        .map(|p| p.bottom.at(p.ring[0]))
        .fold(f64::INFINITY, f64::min);
    assert!(near(over_the_door, 2100.0), "the wall over the door starts at its head, not {over_the_door}");
}

#[test]
fn a_window_below_the_walls_base_cuts_only_what_is_in_the_wall() {
    let mut s = project();
    let w = wall(&mut s, [0, 0], [8000, 0]);
    // 1200 wide, 1500 high, its sill 300 under the base: it reaches 1200 up the wall, 1.2 by 1.2.
    run(&mut s, "window", json!({"wall": w, "offset": 4000, "type": "W", "sill": -300}));
    let cut = 1.2 * 1.2;
    assert_figures(&measure(&s, w), [24.0, 24.0 - cut, 4.8, (24.0 - cut) * 0.2]);
}

#[test]
fn an_l_of_two_walls_counts_its_corner_once() {
    // Two walls of 4000 along their location lines, mitred at the corner (0, 0): each is 4000 x 200 in plan, as the corner
    // counts once, the mitre sharing the 200 x 200 square between them.
    let mut s = project();
    let ids = run(&mut s, "wall", json!({"points": [[4000, 0], [0, 0], [0, 4000]], "type": "T", "height": 3000}))["ids"].clone();
    let (a, b) = (ids[0].as_u64().unwrap_or_default(), ids[1].as_u64().unwrap_or_default());
    let (qa, qb) = (measure(&s, a), measure(&s, b));
    assert_figures(&qa, [12.0, 12.0, 2.4, 2.4]);
    assert_figures(&qb, [12.0, 12.0, 2.4, 2.4]);
    // The L of them: its footprint, two 4000 arms of 200 whose corner square is counted once, 1.6 m², 3 m high.
    assert!(near(qa.gross_volume + qb.gross_volume, 1.6e6 * 3000.0));
}

#[test]
fn a_wall_under_a_sloped_roof_is_what_stands_up_to_the_roof() {
    let mut s = project();
    // A gable roof over a 10 x 8 m footprint, its eaves at 2000 on a level 2000 up, 30° (the ridge runs along x at y = 3000,
    // 4000 from each eave).
    run(&mut s, "level", json!({"name": "Eaves", "elevation": 2000}));
    let roof = run(
        &mut s,
        "roof",
        json!({"boundary": [[-1000, -1000], [9000, -1000], [9000, 7000], [-1000, 7000]], "overhang": 0, "slope": 30, "gable": true}),
    )["id"]
        .as_u64()
        .unwrap_or_default();
    // A wall 6000 long under it at x = 8000: the roof is 2000 + tan 30° (1000 + y) over y up to 3000, then falls to the other eave.
    let w = run(&mut s, "wall", json!({"points": [[8000, 0], [8000, 6000]], "type": "T", "attach_top": roof}))["ids"][0]
        .as_u64()
        .unwrap_or_default();
    let tan = 30f64.to_radians().tan();
    // Its side: 2000 up for 6000, and the roof's rise over it: tan 30° times the sum of (1000 + y) up to the ridge and back down,
    // 2 x (1000 x 3000 + 3000² / 2) = 15e6.
    let side = 2000.0 * 6000.0 + tan * 15e6;
    let q = measure(&s, w);
    assert_eq!(q.height, None);
    assert_figures(&q, [side / 1e6, side / 1e6, side * 200.0 / 1e9, side * 200.0 / 1e9]);
    // A window 1200 wide and 2000 high, its sill 900 up, 300 to 1500 along: its head, 2900, is over the roof's underside at 300
    // (2000 + tan 30° x 1300 = 2750.6) up to where the slope reaches it, at y = (2900 - 2000) / tan 30° - 1000 = 558.8, and the
    // wall is cut only as high as it goes there.
    let window = put(&mut s, "window", w, 900.0, "Tall");
    let _ = window;
    let y1 = (2900.0 - 2000.0) / tan - 1000.0;
    // Over 300 to y1, from 900 up to the roof 2000 + tan (1000 + y); from y1 to 1500, 900 to 2900: 2000.
    let slope_part = (2000.0 - 900.0 + tan * 1000.0) * (y1 - 300.0) + tan * (y1 * y1 - 300.0 * 300.0) / 2.0;
    let cut = slope_part + 2000.0 * (1500.0 - y1);
    let q = measure(&s, w);
    assert_figures(&q, [side / 1e6, (side - cut) / 1e6, side * 200.0 / 1e9, (side - cut) * 200.0 / 1e9]);
}

#[test]
fn a_curved_wall_is_the_area_of_its_middle_through_its_height() {
    let mut s = project();
    // A half circle, 2000 radius on its centre line, 200 thick and 3000 high: half a ring of radii 1900 to 2100.
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "arc": 180, "type": "T", "height": 3000}))["ids"][0]
        .as_u64()
        .unwrap_or_default();
    let q = measure(&s, w);
    let (side, volume) = (PI * 2000.0 * 3000.0, PI * 2000.0 * 200.0 * 3000.0);
    assert_figures(&q, [side / 1e6, side / 1e6, volume / 1e9, volume / 1e9]);
    assert!(near(q.length, PI * 2000.0), "its length is along its location line: {q:?}");
    // The same half circle with its exterior face on the line (the left, toward the centre): it thickens away from it, so the
    // middle of it is at 2100 and its side is that arc's, not the location line's.
    let w = run(
        &mut s,
        "wall",
        json!({"points": [[0, 6000], [4000, 6000]], "arc": 180, "type": "T", "height": 3000, "location": "exterior-finish"}),
    )["ids"][0]
        .as_u64()
        .unwrap_or_default();
    let q = measure(&s, w);
    let (side, volume) = (PI * 2100.0 * 3000.0, PI * 2100.0 * 200.0 * 3000.0);
    assert_figures(&q, [side / 1e6, side / 1e6, volume / 1e9, volume / 1e9]);
    // A window 1200 along its location line, 0.6 rad of its 2000, 1500 high: 0.6 rad of the ring between 2000 and 2200, half
    // (R² - r²) x 0.6 x 1500 of volume, through 200: 0.6 x 2100 x 1500 = 1.89 m².
    put(&mut s, "window", w, 2000.0, "W");
    let cut = 0.6 * 2100.0 * 1500.0;
    let q = measure(&s, w);
    assert_figures(&q, [side / 1e6, (side - cut) / 1e6, volume / 1e9, (volume - cut * 200.0) / 1e9]);
}

#[test]
fn a_wall_that_is_not_in_the_model_measures_nothing() {
    let s = project();
    assert_eq!(quantities::wall(&s.model, 424242), None);
}

/// A project with a roof level 3000 up and the roof type R, 300 thick.
fn roofed() -> Session {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "roof", "name": "R", "thickness": 300}));
    run(&mut s, "level", json!({"name": "Roof level", "elevation": 3000}));
    s
}

const PLAN: [[i32; 2]; 4] = [[0, 0], [8000, 0], [8000, 6000], [0, 6000]];
/// A shaft 1 m square, 4 m high from the ground: through whatever is over it at 3000.
const SHAFT: [[i32; 2]; 4] = [[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]];

fn roof_of(s: &Session, id: u64) -> quantities::RoofQ {
    match s.model.elements.get(id) {
        Some(crate::model::Element::Roof(r)) => quantities::roof(&s.model, r),
        _ => panic!("no roof"),
    }
}

#[test]
fn a_flat_roof_less_a_shaft_through_it_is_its_area_less_the_shaft() {
    let mut s = roofed();
    let roof = run(&mut s, "roof", json!({"boundary": PLAN, "overhang": 0, "slope": 0, "type": "R"}))["id"].as_u64().unwrap_or_default();
    let q = roof_of(&s, roof);
    assert!(near(q.gross_surface_area, 48e6) && near(q.surface_area, 48e6) && near(q.volume, 48e6 * 300.0), "{q:?}");
    run(&mut s, "shaft", json!({"boundary": SHAFT, "height": 4000}));
    // 8 x 6 m is 48 m², the shaft 1 m²: 47 m² of surface and plan, through 300 mm: 14.1 m³. Before the shaft's: 48 m².
    let q = roof_of(&s, roof);
    assert!(near(q.gross_surface_area, 48e6), "{q:?}");
    assert!(near(q.surface_area, 47e6) && near(q.plan_area, 47e6) && near(q.volume, 47e6 * 300.0), "{q:?}");
    // An opening of the roof's own is taken off the same way.
    let with = run(
        &mut s,
        "roof",
        json!({"boundary": [[0, 8000], [8000, 8000], [8000, 14000], [0, 14000]], "overhang": 0, "slope": 0,
        "type": "R", "openings": [[[1000, 9000], [3000, 9000], [3000, 10000], [1000, 10000]]]}),
    )["id"]
        .as_u64()
        .unwrap_or_default();
    let q = roof_of(&s, with);
    assert!(near(q.gross_surface_area, 48e6) && near(q.surface_area, 46e6) && near(q.volume, 46e6 * 300.0), "{q:?}");
}

#[test]
fn a_sloped_roofs_surface_is_its_plan_over_the_cosine_of_its_slope_and_its_volume_goes_through_its_thickness() {
    let mut s = roofed();
    let roof = run(&mut s, "roof", json!({"boundary": PLAN, "overhang": 0, "slope": 30, "type": "R"}))["id"].as_u64().unwrap_or_default();
    run(&mut s, "shaft", json!({"boundary": SHAFT, "height": 4000}));
    // Every face of a hip roof at 30° is 1 / cos 30° of its plan; 47 m² of plan stand, 48 before the shaft. Its 300 mm is square
    // to the slope, so plumb 300 / cos 30°: the volume is the plan through that, or the surface through 300.
    let cos = 30f64.to_radians().cos();
    let q = roof_of(&s, roof);
    assert!(near(q.gross_surface_area, 48e6 / cos) && near(q.surface_area, 47e6 / cos) && near(q.plan_area, 47e6), "{q:?}");
    assert!(near(q.volume, 47e6 * 300.0 / cos), "{q:?}");
}

#[test]
fn a_shaped_roofs_volume_is_its_plan_through_its_thickness_plumb_not_its_surface() {
    let mut s = roofed();
    let roof = run(&mut s, "roof", json!({"boundary": PLAN, "overhang": 0, "slope": 0, "type": "R"}))["id"].as_u64().unwrap_or_default();
    // A drain at its middle 1000 down: four planes. The north and south ones fall 1000 over 3000, the east and west ones 1000 over
    // 4000; each triangle's area is 12 m² in plan (8000 x 3000 / 2 and 6000 x 4000 / 2), stretched by sqrt(1 + (1/3)²) and by
    // sqrt(1 + (1/4)²).
    run(&mut s, "edit_shape", json!({"id": roof, "points": [{"at": [4000, 3000], "z": -1000}]}));
    let surface = 24e6 * ((1.0f64 + 1.0 / 9.0).sqrt() + (1.0f64 + 1.0 / 16.0).sqrt());
    let q = roof_of(&s, roof);
    assert!(near(q.surface_area, surface) && near(q.gross_surface_area, surface), "{q:?}");
    // Its solid is the 48 m² of plan through the 300 plumb, as the closed solid says.
    assert!(near(q.volume, 48e6 * 300.0), "{q:?} not plan x thickness");
    let Some(crate::model::Element::Roof(r)) = s.model.elements.get(roof) else { panic!("no roof") };
    let solid = s.model.roof_solid(r).expect("a shaped roof's solid");
    assert!(near(q.volume, crate::model::volume(&solid)), "{q:?} {}", crate::model::volume(&solid));
}

#[test]
fn a_sloped_floors_area_is_its_top_stretched_by_the_slope() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "floor", "name": "F", "thickness": 200}));
    // 8 x 6 m, rising 10° to the east, with a 1 m² opening.
    let floor = run(
        &mut s,
        "floor",
        json!({"boundary": PLAN, "type": "F", "slope": 10, "arrow": [[0, 0], [8000, 0]], "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
    )["id"]
        .as_u64()
        .unwrap_or_default();
    let Some(crate::model::Element::Floor(f)) = s.model.elements.get(floor) else { panic!("no floor") };
    let q = quantities::floor(&s.model, f);
    let cos = 10f64.to_radians().cos();
    assert!(near(q.gross_area, 48e6 / cos) && near(q.net_area, 47e6 / cos), "{q:?}");
    // Its 200 is square to the slope: 200 / cos 10° plumb, through the 47 m² of plan.
    assert!(near(q.gross_volume, 48e6 * 200.0 / cos) && near(q.net_volume, 47e6 * 200.0 / cos), "{q:?}");
    assert!(!q.level);
    // A level one's area is its plan.
    let level = run(&mut s, "floor", json!({"boundary": [[0, 8000], [8000, 8000], [8000, 14000], [0, 14000]], "type": "F"}))["id"]
        .as_u64()
        .unwrap_or_default();
    let Some(crate::model::Element::Floor(f)) = s.model.elements.get(level) else { panic!("no floor") };
    let q = quantities::floor(&s.model, f);
    assert!(near(q.gross_area, 48e6) && near(q.net_area, 48e6) && q.level, "{q:?}");
}

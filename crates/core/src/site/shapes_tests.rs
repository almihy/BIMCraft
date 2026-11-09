//! The site's shapes held to Arqiflow's fixtures (fixtures/cases.json and roads.json, copied from Arqiflow's
//! server/tests/fixtures/site_kinds/, metres): each outline within 5 mm of the server's, as Arqiflow's browser is.

use serde_json::{Value, json};

use super::poly::{self, Ring, Xy};
use super::*;

const CASES: &str = include_str!("fixtures/cases.json");
const ROADS: &str = include_str!("fixtures/roads.json");
/// Arqiflow's parity: 5 mm.
const TOL: f64 = 5.0;

fn rings_of_geojson(g: &Value) -> Vec<Ring> {
    let ring = |r: &Value| -> Ring {
        let pts: Ring = r.as_array().into_iter().flatten().filter_map(|q| Some([q[0].as_f64()? * 1000.0, q[1].as_f64()? * 1000.0])).collect();
        poly::open(&pts)
    };
    match g["type"].as_str() {
        Some("Polygon") => g["coordinates"].as_array().into_iter().flatten().map(ring).collect(),
        Some("MultiPolygon") => g["coordinates"].as_array().into_iter().flatten().flat_map(|p| p.as_array().into_iter().flatten().map(ring)).collect(),
        _ => Vec::new(),
    }
}

fn edges(rings: &[Ring]) -> Vec<(Xy, Xy)> {
    rings.iter().flat_map(|r| (0..r.len()).map(move |i| (r[i], r[(i + 1) % r.len()]))).collect()
}

fn samples(rings: &[Ring]) -> Vec<Xy> {
    let mut out = Vec::new();
    for (a, b) in edges(rings) {
        let n = (poly::dist(a, b) / 500.0).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = k as f64 / n as f64;
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// The Hausdorff distance between two sets of rings' edges, sampled every half metre.
fn hausdorff(x: &[Ring], y: &[Ring]) -> f64 {
    let one = |p: &[Ring], q: &[Ring]| {
        let e = edges(q);
        samples(p).iter().map(|s| e.iter().map(|(a, b)| poly::seg_point(*a, *b, *s).0).fold(f64::INFINITY, f64::min)).fold(0.0, f64::max)
    };
    one(x, y).max(one(y, x))
}

fn props_mm(kind_: &str, p: &Value) -> Props {
    convert(kind_, p.as_object().unwrap_or(&Props::new()), 1000.0)
}

#[test]
fn every_kind_matches_arqiflow_s_fixtures_within_5_mm() {
    let data: Value = serde_json::from_str(CASES).expect("the fixtures read");
    let mut checked = 0;
    for c in data["cases"].as_array().expect("cases") {
        let (name, kind_, use_) = (c["name"].as_str().unwrap_or(""), c["kind"].as_str().unwrap_or(""), c["use"].as_str().unwrap_or(""));
        let p = props_mm(kind_, &c["props"]);
        if kind_ == "parking_band" {
            // The same bays by the same ids (Arqiflow's: "k" + the band's id less "pb" + piece + side + index).
            let mut all = p.clone();
            all.extend(fill_defaults(kind_, use_, &p, &Figures::default()));
            let (aisles, bays) = band_bays("fix01", &all, &[]).expect("the band lays out");
            let mut ids: Vec<String> = bays.iter().map(|b| b.id.clone()).collect();
            ids.sort();
            let want: Vec<String> = c["bays"].as_array().expect("bays").iter().filter_map(|v| v.as_str().map(String::from)).collect();
            assert_eq!(ids, want, "{name}");
            assert_eq!(aisles.len(), c["aisles"].as_array().map_or(0, Vec::len), "{name}");
            checked += 1;
            continue;
        }
        let want = rings_of_geojson(&c["geometry"]);
        let got: Vec<Ring> = if kind_ == "mass" {
            let arc = p.get("arc").and_then(Value::as_object).expect("an arc");
            vec![arc_footprint(arc).expect("a curved building").0]
        } else {
            let item = Item { key: "x", kind: kind_, use_, props: &p, phase: "new", context: false };
            let s = normalise(&[item], &Figures::default());
            assert!(s[0].notes.is_empty() || kind_ == "pitch", "{name}: {:?}", s[0].notes);
            poly::rings_of(&s[0].outline)
        };
        let h = hausdorff(&got, &want);
        if h > TOL {
            eprintln!("{name}\n got {got:?}\n want {want:?}");
        }
        assert!(h <= TOL, "{name}: {h:.1} mm off");
        let a = c["area"].as_f64().unwrap_or(0.0) * 1e6;
        assert!((poly::area(&got) - a).abs() <= 0.002 * a + 1e4, "{name}: area {} against {a}", poly::area(&got));
        checked += 1;
    }
    assert_eq!(checked, 12);
}

#[test]
fn every_junction_matches_arqiflow_s_fixtures_within_5_mm() {
    let data: Value = serde_json::from_str(ROADS).expect("the fixtures read");
    let mut checked = 0;
    for c in data["cases"].as_array().expect("cases") {
        let name = c["name"].as_str().unwrap_or("");
        let rs = c["roads"].as_array().expect("roads");
        let props: Vec<Props> = rs.iter().map(|r| props_mm("lane", &r["props"])).collect();
        let ids: Vec<&str> = rs.iter().map(|r| r["id"].as_str().unwrap_or("")).collect();
        let items: Vec<Item<'_>> = props
            .iter()
            .zip(&ids)
            .map(|(p, id)| Item { key: id, kind: "lane", use_: "road", props: p, phase: "new", context: false })
            .collect();
        let shaped = normalise(&items, &Figures::default());
        for ((r, s), id) in rs.iter().zip(&shaped).zip(&ids) {
            let want = rings_of_geojson(&r["geometry"]);
            let got = poly::rings_of(&s.outline);
            let h = hausdorff(&got, &want);
            if h > TOL {
                eprintln!("{name}/{id}\n got {got:?}\n want {want:?}");
            }
            assert!(h <= TOL, "{name}/{id}: {h:.1} mm off");
            checked += 1;
        }
    }
    assert_eq!(checked, 27);
}

#[test]
fn roads_that_meet_make_one_paved_area_drawn_with_its_parts() {
    let data: Value = serde_json::from_str(ROADS).expect("the fixtures read");
    for c in data["cases"].as_array().expect("cases") {
        let name = c["name"].as_str().unwrap_or("");
        let props: Vec<Props> = c["roads"].as_array().expect("roads").iter().map(|r| props_mm("lane", &r["props"])).collect();
        let items: Vec<Item<'_>> = props.iter().enumerate().map(|(i, p)| Item { key: ["a", "b", "c", "d"][i], kind: "lane", use_: "road", props: p, phase: "new", context: false }).collect();
        let shaped = normalise(&items, &Figures::default());
        let mut all: Vec<Ring> = Vec::new();
        for s in &shaped {
            all.extend(poly::rings_of(&s.outline));
            let roles: Vec<&str> = s.parts.iter().map(|p| p.role.as_str()).collect();
            for want in ["carriageway", "pavement", "kerb", "pavement_edge", "path"] {
                assert!(roles.contains(&want), "{name}: {roles:?}");
            }
        }
        let whole = poly::union(&all);
        assert_eq!(whole.iter().filter(|r| poly::ring_area(r) > 0.0).count(), 1, "{name}: one paved area");
        assert!(whole.iter().all(|r| poly::ring_area(r) > 0.0), "{name}: no holes");
    }
}

#[test]
fn the_figures_fill_in_what_was_not_given_and_a_firm_s_own_are_taken() {
    let p: Props = serde_json::from_value(json!({"path": [[0, 0], [40000, 0]]})).expect("props");
    let item = Item { key: "l", kind: "lane", use_: "", props: &p, phase: "new", context: false };
    let s = normalise(&[item.clone()], &Figures::default());
    assert_eq!(s[0].filled["width"], json!(5500.0));
    assert_eq!(s[0].filled["corner_radius"], json!(3000.0));
    assert!((poly::area(&poly::rings_of(&s[0].outline)) - 40000.0 * 5500.0).abs() < 10.0);
    let firm = Figures([("lane.width".to_string(), 4000.0)].into_iter().collect());
    let s = normalise(&[item], &firm);
    assert_eq!(s[0].filled["width"], json!(4000.0));
    // A road's cross-section from its right of way.
    let r = normalise(&[Item { key: "r", kind: "lane", use_: "road", props: &p, phase: "new", context: false }], &Figures::default());
    for k in ["lanes", "lane_width", "oneway", "median", "markings", "kerb", "corner_radius"] {
        assert!(r[0].filled.contains_key(k), "{k}");
    }
    // What was given stays; a tree's crown fills in.
    let t: Props = serde_json::from_value(json!({"at": [1000, 2000]})).expect("props");
    let s = normalise(&[Item { key: "t", kind: "tree", use_: "", props: &t, phase: "new", context: false }], &Figures::default());
    assert_eq!(s[0].filled["crown"], json!(6000.0));
    assert!((poly::area(&poly::rings_of(&s[0].outline)) - std::f64::consts::PI * 3000.0 * 3000.0).abs() < 0.02 * 3e7);
}

#[test]
fn a_lane_ending_on_another_takes_the_rounded_corners() {
    let a: Props = serde_json::from_value(json!({"path": [[-30000, 0], [30000, 0]], "width": 5500})).expect("props");
    let b: Props = serde_json::from_value(json!({"path": [[0, 0], [0, 30000]], "width": 5500})).expect("props");
    let items = [
        Item { key: "a", kind: "lane", use_: "lane", props: &a, phase: "new", context: false },
        Item { key: "b", kind: "lane", use_: "lane", props: &b, phase: "new", context: false },
    ];
    let s = normalise(&items, &Figures::default());
    assert!(s[0].parts.iter().all(|p| p.role != "fillet"), "the lane running through keeps its band");
    let f = s[1].parts.iter().find(|p| p.role == "fillet").expect("the lane ending there takes the corners");
    let fa: f64 = poly::area(&poly::rings_of(&f.polys));
    let want = 2.0 * (1.0 - std::f64::consts::PI / 4.0) * 3000.0 * 3000.0;
    assert!((fa - want).abs() < 0.03 * want, "{fa} {want}");
}

#[test]
fn a_pitch_a_ramp_water_and_a_bike_rack_have_their_parts() {
    let p: Props = serde_json::from_value(json!({"at": [0, 0], "length": 50000, "width": 30000})).expect("props");
    let s = normalise(&[Item { key: "p", kind: "pitch", use_: "tennis", props: &p, phase: "new", context: false }], &Figures::default());
    assert!(s[0].parts.iter().any(|p| p.role == "run_off") && s[0].parts.iter().any(|p| p.role == "marking"));
    assert_eq!(s[0].notes, vec!["A tennis court is 23.77 × 10.97 m.".to_string()]);
    assert_eq!(s[0].filled["margin"], json!([6400.0, 3660.0]));
    let r: Props = serde_json::from_value(json!({"path": [[0, 0], [0, -20000]]})).expect("props");
    let s = normalise(&[Item { key: "r", kind: "ramp", use_: "", props: &r, phase: "new", context: false }], &Figures::default());
    let open = s[0].parts.iter().find(|p| p.role == "open").expect("open");
    assert!((poly::area(&poly::rings_of(&open.polys)) - 12000.0 * 3500.0).abs() < 10.0);
    assert!(s[0].parts.iter().any(|p| p.role == "covered"));
    let w: Props = serde_json::from_value(json!({"control": [[0, 0], [30000, 0], [40000, 20000], [10000, 25000]], "bank": 1000})).expect("props");
    let s = normalise(&[Item { key: "w", kind: "water", use_: "", props: &w, phase: "new", context: false }], &Figures::default());
    assert_eq!(s[0].filled["smooth"], json!(3));
    assert!(s[0].parts.iter().any(|p| p.role == "bank"));
    let b: Props = serde_json::from_value(json!({"at": [0, 0]})).expect("props");
    let s = normalise(&[Item { key: "b", kind: "bike_rack", use_: "", props: &b, phase: "new", context: false }], &Figures::default());
    assert!((poly::area(&poly::rings_of(&s[0].outline)) - 4000.0 * 2000.0).abs() < 1.0);
    let c: Props = serde_json::from_value(json!({"control": [[0.4, 0], [0, 10000.2], [10000, 10000], [10000, 0]]})).expect("props");
    let s = normalise(&[Item { key: "c", kind: "car_park", use_: "", props: &c, phase: "new", context: false }], &Figures::default());
    assert!(s[0].outline[0].outer.contains(&P2(0.0, 0.0)) && s[0].outline[0].outer.contains(&P2(0.0, 10000.0)));
    assert!(poly::ring_area(&s[0].outline[0].outer.iter().map(|q| poly::of_p2(*q)).collect::<Vec<_>>()) > 0.0);
}

#[test]
fn a_parking_band_keeps_the_bays_a_person_removed_out() {
    let p: Props = serde_json::from_value(json!({"path": [[0, 0], [40000, 0]], "omit": [{"piece": 0, "side": "left", "index": 2}],
        "accessible": [{"piece": 0, "side": "right", "index": 0}]}))
    .expect("props");
    let s = normalise(&[Item { key: "x", kind: "parking_band", use_: "", props: &p, phase: "new", context: false }], &Figures::default());
    let stalls = s[0].parts.iter().find(|p| p.role == "stall").map_or(0, |p| p.polys.len());
    let acc = s[0].parts.iter().find(|p| p.role == "accessible_stall").map_or(0, |p| p.polys.len());
    assert_eq!((stalls, acc), (16 + 16 - 2, 1));
}

#[test]
fn hostile_props_give_notes_never_a_panic() {
    let bad = [
        json!({"path": [[0, 0]]}),
        json!({"path": [[0, 0], [1e300, 0]]}),
        json!({"path": "nope"}),
        json!({"path": [[0, 0, 1e9], [1000, 0]], "width": -5}),
        json!({"path": [[0, 0, 1.0], [1, 0]], "width": 1e12}),
        json!({"control": [[0, 0], [1, 1]], "smooth": 99}),
        json!({"at": [null, 1], "radius": -1}),
        json!({"at": [0, 0], "length": f64::MAX, "width": 1, "margin": [1, "x"]}),
        json!({"path": [[0, 0], [10000, 0]], "angle": 0, "aisle": 1e9}),
        json!({"path": [[0, 0], [10000, 0]], "heads": {"start": {"shape": "hammerhead", "arm": 1e30}, "end": 7}}),
    ];
    for kind_ in KINDS.iter().map(|k| k.kind).chain(["nothing"]) {
        for b in &bad {
            let p = b.as_object().cloned().unwrap_or_default();
            for use_ in ["", "road"] {
                let s = normalise(&[Item { key: "h", kind: kind_, use_, props: &p, phase: "new", context: false }], &Figures::default());
                assert_eq!(s.len(), 1);
            }
        }
    }
}

#[test]
fn the_catalogue_says_every_kind_s_props_and_figures() {
    let c = catalog();
    for k in KINDS.iter().map(|k| k.kind).chain(["mass"]) {
        let e = &c["kinds"][k];
        assert!(e["label"].is_string() && e["revit"].is_string() && e["shape"].is_string(), "{k}");
        assert!(e["props"]["phase"].is_string(), "{k}");
    }
    assert_eq!(c["kinds"]["lane"]["figures"]["lane.width"], json!(5500.0));
    assert_eq!(c["kinds"]["lane"]["props"]["path"], json!("path"));
    assert_eq!(c["kinds"]["ramp"]["props"]["gradient"], json!("percent"));
    assert_eq!(c["kinds"]["pitch"]["sizes"]["tennis"]["margin"], json!([6400.0, 3660.0]));
    assert_eq!(c["figures"]["roof.pitch"]["type"], json!("angle"));
    // Every figure a kind's defaults fill in is in the catalogue's figures.
    for (n, _, _) in FIGURES {
        assert!(c["figures"][n].is_object(), "{n}");
    }
    // Converting a lane from metres and back gives it again; the bulge and the counts are left alone.
    let m: Props = serde_json::from_value(json!({"path": [[1.5, 2, 0.4], [3, 4]], "width": 5.5, "lanes": 2,
        "heads": {"start": {"shape": "round", "radius": 9, "side": "left"}, "end": null}, "margin": [6.4, 3.66]}))
    .expect("props");
    let mm = convert("lane", &m, 1000.0);
    assert_eq!(mm["path"], json!([[1500.0, 2000.0, 0.4], [3000.0, 4000.0]]));
    assert_eq!(mm["lanes"], json!(2));
    assert_eq!(mm["heads"]["start"]["radius"], json!(9000.0));
    let back = convert("lane", &mm, 0.001);
    assert_eq!(back["width"], json!(5.5));
    let cp: Props = serde_json::from_value(json!({"control": [[1, 2]], "result": {"area_per_stall": 25.0, "ways": [{"at": [1, 1], "kind": "in"}]}})).expect("props");
    let cp = convert("car_park", &cp, 1000.0);
    assert_eq!(cp["result"]["area_per_stall"], json!(25.0e6));
    assert_eq!(cp["result"]["ways"][0]["at"], json!([1000.0, 1000.0]));
}

/// A level of a hundred and fifty roads, crossing in a grid, the outlines and their parts. Run with `--release` (and
/// `--ignored`) to see its time: well under 100 ms.
#[test]
#[ignore]
fn a_hundred_and_fifty_roads_are_shaped_quickly() {
    let mut props: Vec<Props> = Vec::new();
    // Ten streets each way, cut at every crossing into pieces that join end to end: 2 × 10 × 8 pieces.
    for i in 0..10 {
        for j in 0..8 {
            let (a, b) = (j as f64 * 60000.0, (j + 1) as f64 * 60000.0);
            let y = i as f64 * 60000.0 + 30000.0;
            let w = if i % 3 == 0 { 20000 } else { 11000 };
            props.push(serde_json::from_value(json!({"path": [[a, y], [b, y]], "width": w})).expect("props"));
            props.push(serde_json::from_value(json!({"path": [[y, a], [y, b]], "width": 11000})).expect("props"));
        }
    }
    props.truncate(150);
    let keys: Vec<String> = (0..props.len()).map(|i| format!("r{i:03}")).collect();
    let items: Vec<Item<'_>> = props.iter().zip(&keys).map(|(p, k)| Item { key: k, kind: "lane", use_: "road", props: p, phase: "existing", context: true }).collect();
    let t = std::time::Instant::now();
    let s = normalise(&items, &Figures::default());
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("150 roads: {ms:.1} ms");
    assert_eq!(s.len(), 150);
    assert!(s.iter().all(|x| !x.outline.is_empty()));
}

//! What the editor's writer of a CAD plan's build list needs of the engine, as Revit has it: wall ends carried on with a
//! step across joined, what each wall end is joined to said, a wall's location line neither making nor parting a join,
//! a stair by component (its runs and its sketched landing), and a wall opening's parameter values.

use serde_json::{Value, json};

use crate::curtain_tests::{fails, run, wall};
use crate::engine::Session;
use crate::model::Id;

fn listed(s: &mut Session, id: Id) -> Value {
    let all = run(s, "elements", json!({}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"].as_u64() == Some(id)).cloned().expect("listed")
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

fn types(s: &mut Session) {
    for t in [300, 220, 200] {
        run(s, "type", json!({"category": "wall", "name": format!("G{t}"), "thickness": t}));
    }
}

#[test]
fn walls_carried_on_end_to_end_with_a_step_across_join_square() {
    let mut s = Session::new();
    types(&mut s);
    // a 300 wall running on as a 220 one, their faces flush on one side (their centre lines 40 mm apart)
    let a = wall(&mut s, json!({"points": [[0, 0], [5000, 0]], "type": "G300", "height": 3000}));
    let b = wall(&mut s, json!({"points": [[5000, 40], [9000, 40]], "type": "G220", "height": 3000}));
    assert_eq!(ids(&listed(&mut s, a)["joined_at_end"]), vec![b]);
    assert_eq!(ids(&listed(&mut s, b)["joined_at_start"]), vec![a]);
    assert!(ids(&listed(&mut s, a)["joined_at_start"]).is_empty());
    // square ends: each its own length
    assert!((listed(&mut s, a)["volume_m3"].as_f64().unwrap() - 4.5).abs() < 1e-6);
    assert!((listed(&mut s, b)["volume_m3"].as_f64().unwrap() - 2.64).abs() < 1e-6);
    // a straight wall running into a curved one along its tangent, 3 mm off across, joins too
    let c = wall(&mut s, json!({"points": [[0, 10000], [5000, 10000]], "type": "G200", "height": 3000}));
    let d = wall(&mut s, json!({"points": [[5000, 10003], [9000, 14003]], "arc": 90, "type": "G200", "height": 3000}));
    assert_eq!(ids(&listed(&mut s, c)["joined_at_end"]), vec![d]);
    // ends a few centimetres apart along their line do not, as Revit keeps no join there
    let e = wall(&mut s, json!({"points": [[0, 20000], [5000, 20000]], "type": "G200", "height": 3000}));
    wall(&mut s, json!({"points": [[5030, 20000], [9000, 20000]], "type": "G200", "height": 3000}));
    assert!(ids(&listed(&mut s, e)["joined_at_end"]).is_empty());
}

#[test]
fn a_wall_put_on_a_face_keeps_its_joins_and_its_neighbours_bodies() {
    let mut s = Session::new();
    types(&mut s);
    // an L, a tee into its long side and a wall carried on from its end with a step
    let a = wall(&mut s, json!({"points": [[0, 0], [8000, 0]], "type": "G300", "height": 3000}));
    let b = wall(&mut s, json!({"points": [[0, 6000], [0, 0]], "type": "G200", "height": 3000}));
    let c = wall(&mut s, json!({"points": [[4000, 4000], [4000, 0]], "type": "G200", "height": 3000}));
    let d = wall(&mut s, json!({"points": [[8000, 40], [12000, 40]], "type": "G220", "height": 3000}));
    let state = |s: &mut Session| -> Vec<(Vec<u64>, Vec<u64>, f64)> {
        [a, b, c, d]
            .iter()
            .map(|id| {
                let e = listed(s, *id);
                (ids(&e["joined_at_start"]), ids(&e["joined_at_end"]), (e["volume_m3"].as_f64().unwrap() * 1e6).round())
            })
            .collect()
    };
    let before = state(&mut s);
    for id in [a, b, c, d] {
        for loc in ["exterior-finish", "interior-finish", "centre"] {
            run(&mut s, "modify", json!({"id": id, "location": loc}));
            assert_eq!(state(&mut s), before, "wall {id} on {loc}");
        }
    }
}

#[test]
fn a_stair_by_its_runs_climbs_them_with_its_landing_between() {
    let mut s = Session::new();
    run(&mut s, "level", json!({"name": "L", "elevation": 10000}));
    run(&mut s, "level", json!({"name": "M", "elevation": 14200}));
    let r = run(
        &mut s,
        "stair",
        json!({"level": "L", "runs": [{"start": [300, 900], "end": [4200, 900], "width": 1800, "risers": 14},
                                      {"start": [4200, 3100], "end": [300, 3100], "width": 1700, "risers": 14}],
               "landings": [{"outline": [[4200, 0], [6000, 0], [6000, 4000], [4200, 4000]]}]}),
    );
    assert_eq!(r["risers"], 28);
    assert!((r["riser"].as_f64().unwrap() - 150.0).abs() < 1e-6);
    let id = r["id"].as_u64().unwrap();
    let st = match s.model.elements.get(id) {
        Some(crate::model::Element::Stair(st)) => st.clone(),
        _ => panic!("a stair"),
    };
    let l = crate::stairs::stair(&s.model, &st);
    assert_eq!(l.runs.len(), 2);
    assert_eq!((l.runs[0].z, l.runs[1].z), ((10000.0, 12100.0), (12100.0, 14200.0)));
    assert_eq!((l.runs[0].width, l.runs[1].width), (Some(1800.0), Some(1700.0)));
    assert_eq!(l.pads.len(), 1);
    assert!((l.pads[0].z - 12100.0).abs() < 1e-9);
    // its second run 1700 wide
    let ring = l.runs[1].ring(st.width);
    assert!((ring[0].dist(ring[3]) - 1700.0).abs() < 1e-6);
    assert_eq!(s.model.written_format(), crate::model::STAIR_FORMAT);
    // moved, its runs and landing go with it
    run(&mut s, "move", json!({"ids": [id], "by": [1000, 0]}));
    let e = listed(&mut s, id);
    assert_eq!(e["flights"][0]["start"], json!([1300.0, 900.0]));
    assert_eq!(e["sketched"][0][0], json!([5200.0, 0.0]));
    // saved and opened as it was
    let path = std::env::temp_dir().join(format!("bimcraft-stair-runs-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let (m, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m, *s.model);
    // a landing with no run after it is refused
    let refused = fails(
        &mut s,
        "stair",
        json!({"level": "L", "runs": [{"start": [0, 9000], "end": [3900, 9000], "risers": 14}],
               "landings": [{"outline": [[3900, 8000], [5000, 8000], [5000, 10000]]}]}),
    );
    assert!(refused.contains("a landing goes between two runs"), "{refused}");
}

#[test]
fn a_wall_opening_takes_project_parameter_values() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "G200", "thickness": 200}));
    let w = wall(&mut s, json!({"points": [[0, 0], [10000, 0]], "type": "G200", "height": 3000}));
    run(&mut s, "parameter", json!({"name": "Arqiflow id", "kind": "text", "categories": ["wall", "wallopening"]}));
    let o = run(&mut s, "wall_opening", json!({"wall": w, "offset": 3000, "width": 1000, "height": 2440}))["id"].as_u64().unwrap();
    assert!(s.model.written_format() < crate::model::STAIR_FORMAT);
    run(&mut s, "modify", json!({"id": o, "params": {"Arqiflow id": "o:1"}}));
    assert_eq!(listed(&mut s, o)["params"]["Arqiflow id"], "o:1");
    assert_eq!(s.model.written_format(), crate::model::STAIR_FORMAT);
}

#[test]
fn a_curved_wall_a_wall_tees_into_holds_its_body_wherever_the_tee_ends_in_it() {
    // The School's F1 w:be418c889062 and the wall teeing into it, w:c367f95bcedb: the arc's volume swung from 3.3 to 4.3 m3
    // as the tee's end moved by less than a micron (its location line put on a face and back, rounded).
    let mut s = Session::new();
    types(&mut s);
    let arc = wall(
        &mut s,
        json!({"points": [[193807.5142853367, 72486.72833137562], [197120.33124730273, 75667.53814323182]],
               "arc": 68.1160990852998, "type": "G200", "height": 4000}),
    );
    let alone = listed(&mut s, arc)["volume_m3"].as_f64().unwrap();
    assert!((alone - 3.9).abs() < 0.001, "{alone}");
    for end in [[196678.85306574675, 74490.45824618677], [196678.853, 74490.459]] {
        let t = wall(&mut s, json!({"points": [[195744.73762617406, 81143.6581043662], end], "type": "G200", "height": 4000}));
        let v = listed(&mut s, arc)["volume_m3"].as_f64().unwrap();
        assert!((v - alone).abs() < 0.001, "the arc with the tee ending at {end:?}: {v} m3, alone {alone}");
        run(&mut s, "delete", json!({"ids": [t]}));
    }
}

#[test]
fn walls_join_by_the_lines_they_were_drawn_on_whatever_line_they_are_put_on_after() {
    let mut s = Session::new();
    types(&mut s);
    let state = |s: &mut Session, ids: &[Id]| -> Vec<(Vec<u64>, Vec<u64>, f64)> {
        ids.iter()
            .map(|id| {
                let e = listed(s, *id);
                (ids_of(&e["joined_at_start"]), ids_of(&e["joined_at_end"]), (e["volume_m3"].as_f64().unwrap() * 1e6).round())
            })
            .collect()
    };
    // Drawn on their faces, their face lines ending at one point: they join there, as Revit joins location curves' ends.
    let a = wall(&mut s, json!({"points": [[0, 150], [5000, 150]], "type": "G300", "height": 3000, "location": "exterior-finish"}));
    let b = wall(&mut s, json!({"points": [[5000, 3000], [5000, 150]], "type": "G200", "height": 3000, "location": "interior-finish"}));
    assert_eq!(ids(&listed(&mut s, a)["joined_at_end"]), vec![b]);
    let drawn = state(&mut s, &[a, b]);
    // Put on their centre lines and back, they keep that join and their bodies.
    for loc in ["centre", "interior-finish", "exterior-finish"] {
        for id in [a, b] {
            run(&mut s, "modify", json!({"id": id, "location": loc}));
            assert_eq!(state(&mut s, &[a, b]), drawn, "wall {id} on {loc}");
        }
    }
    // Drawn on their centre lines, not meeting: once both are on faces their location lines end at one point (the School's F1
    // w:50da009f684f and w:392badd6aaa8), and still they do not join, as Revit makes no join when a Location Line changes.
    let c = wall(&mut s, json!({"points": [[0, 10000], [5000, 10000]], "type": "G300", "height": 3000}));
    let d = wall(&mut s, json!({"points": [[5100, 13000], [5100, 10150]], "type": "G200", "height": 3000}));
    let apart = state(&mut s, &[c, d]);
    assert!(apart.iter().all(|(s0, s1, _)| s0.is_empty() && s1.is_empty()), "{apart:?}");
    run(&mut s, "modify", json!({"id": c, "location": "exterior-finish"}));
    run(&mut s, "modify", json!({"id": d, "location": "interior-finish"}));
    let (ce, de) = (listed(&mut s, c)["end"].clone(), listed(&mut s, d)["end"].clone());
    assert_eq!((ce.clone(), de.clone()), (json!([5000.0, 10150.0]), json!([5000.0, 10150.0])), "{ce} {de}");
    assert_eq!(state(&mut s, &[c, d]), apart);
    // Its ends placed again, a wall is drawn on its location line from then on: placed where the other's drawn line ends,
    // there it joins.
    run(&mut s, "modify", json!({"id": d, "end": [5000, 10000]}));
    assert!(listed(&mut s, d).get("drawn_on").is_none_or(Value::is_null));
    assert_eq!(listed(&mut s, c)["drawn_on"], "centre");
    assert_eq!(ids(&listed(&mut s, c)["joined_at_end"]), vec![d]);
    // Saved and opened, the line each was drawn on is kept, and the file says what reads it.
    let path = std::env::temp_dir().join(format!("bimcraft-drawn-on-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let (m, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m, *s.model);
    assert_eq!(s.model.written_format(), crate::model::DRAWN_FORMAT);
}

fn ids_of(v: &Value) -> Vec<u64> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

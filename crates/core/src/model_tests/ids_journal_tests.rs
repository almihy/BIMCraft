//! `ids`'s tests that drive the Session (moved out of `ids.rs`).

use crate::model::Element;

use crate::engine::Session;
use crate::ids::*;
use crate::model::Model;
use serde_json::json;
use std::sync::Arc;

/// What the command `cmd` changed, as the model it left says.
fn changed(s: &mut Session, cmd: &str, p: serde_json::Value) -> Changed {
    let before = Arc::clone(&s.model);
    s.execute(cmd, &p).unwrap();
    s.model.changed_since(&before)
}

fn ids(v: &[Id]) -> Changed {
    Changed::Ids(v.iter().copied().collect())
}

#[test]
fn a_command_leaves_a_journal_of_the_elements_it_added_changed_and_removed() {
    let mut s = Session::new();
    let made = s.execute("wall", &json!({"points": [[0, 0], [5000, 0], [5000, 4000]]})).unwrap();
    let [a, b] = [0, 1].map(|i| made["ids"][i].as_u64().unwrap());
    let before = Arc::clone(&s.model);
    let door = s.execute("door", &json!({"at": [2500, 0]})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(s.model.changed_since(&before), ids(&[door]));
    // Moving a wall stretches the one joined to it, and carries its door.
    let moved = changed(&mut s, "move", json!({"ids": [b], "by": [1000, 0]}));
    let Changed::Ids(moved) = moved else { panic!("{moved:?}") };
    assert!(moved.contains(&a) && moved.contains(&b), "{moved:?}");
    assert_eq!(changed(&mut s, "delete", json!({"ids": [door]})), ids(&[door]));
    // Undo and redo go to a model of the history, which shares the rest still: what differs.
    assert_eq!(changed(&mut s, "undo", json!({})), ids(&[door]));
    assert_eq!(changed(&mut s, "redo", json!({})), ids(&[door]));
    // A command that changes a setting changes everything there is to check, and so does undoing it.
    assert_eq!(changed(&mut s, "room_area", json!({"at": "finish"})), Changed::All);
    assert_eq!(changed(&mut s, "undo", json!({})), Changed::All);
    // Another model's elements: everything.
    assert_eq!(Model::new("x").changed_since(&s.model), Changed::All);
}

#[test]
fn the_journal_of_a_clone_starts_empty_names_what_changed_and_finds_what_a_sweep_changed() {
    let mut es = Elements::new();
    let g =
        |at: f64| Element::Grid(crate::model::Grid { start: crate::geom::P2(at, 0.0), end: crate::geom::P2(at, 1.0), name: String::new() });
    for id in 100_000..100_005 {
        es.insert(id, g(0.0));
    }
    let base = es.clone();
    let mut c = base.clone();
    assert_eq!(c.changed_since(&base), ids(&[]));
    if let Some(e) = c.get_mut(100_001) {
        *e = g(1.0);
    }
    c.insert(100_009, g(2.0));
    c.remove(100_000);
    assert_eq!(c.changed_since(&base), ids(&[100_000, 100_001, 100_009]));
    // Gone through from an id on, or all of them: those that differ.
    for (_, e) in c.range_mut(100_003..) {
        if let Element::Grid(gr) = e {
            gr.name = "C".into();
        }
    }
    assert_eq!(c.changed_since(&base), ids(&[100_000, 100_001, 100_003, 100_004, 100_009]));
    // A clone of a clone isn't base's: what differs, found in the chunks they don't share.
    assert_eq!(c.clone().changed_since(&base), ids(&[100_000, 100_001, 100_003, 100_004, 100_009]));
    let mut d = base.clone();
    for e in d.values_mut() {
        if let Element::Grid(gr) = e
            && gr.start.0 == 0.0
        {
            gr.start.0 = 5.0;
        }
    }
    assert_eq!(d.changed_since(&base), ids(&[100_000, 100_001, 100_002, 100_003, 100_004]));
    let mut d = base.clone();
    let _ = d.values_mut();
    assert_eq!(d.changed_since(&base), ids(&[]), "gone through, nothing changed");
    d.clear();
    assert_eq!(d.changed_since(&base), ids(&[100_000, 100_001, 100_002, 100_003, 100_004]));
}

#[test]
fn copies_share_their_chunks_until_one_changes_and_the_other_stays_as_it_was() {
    let g =
        |at: f64| Element::Grid(crate::model::Grid { start: crate::geom::P2(at, 0.0), end: crate::geom::P2(at, 1.0), name: String::new() });
    let mut es = Elements::new();
    // Across three chunks, with a gap.
    let ids: Vec<Id> = (0..5).map(|i| 100_000 + i * (CHUNK / 2) + 3).collect();
    for &id in &ids {
        es.insert(id, g(id as f64));
    }
    let mut copy = es.clone();
    assert_eq!(copy, es);
    if let Some(e) = copy.get_mut(ids[2]) {
        *e = g(-1.0);
    }
    copy.remove(ids[4]);
    copy.insert(ids[4] + 1, g(7.0));
    assert_eq!(es.len(), 5);
    assert_eq!(es.get(ids[2]), Some(&g(ids[2] as f64)), "the original is as it was");
    assert!(es.contains_key(ids[4]) && !es.contains_key(ids[4] + 1));
    assert_eq!(copy.get(ids[2]), Some(&g(-1.0)));
    assert_eq!(copy.len(), 5);
    assert_ne!(copy, es);
    // In order, across the chunks; from an id inside one; the last.
    assert_eq!(es.keys().copied().collect::<Vec<_>>(), ids);
    assert_eq!(es.range(ids[1] + 1..).map(|(id, _)| *id).collect::<Vec<_>>(), ids[2..].to_vec());
    assert_eq!(copy.keys().next_back(), Some(&(ids[4] + 1)));
    // GUIDs find their elements in each, and the copy's change set is what it changed.
    let guid = es.guid(ids[3]).unwrap();
    assert_eq!((es.id(guid), copy.id(guid)), (Some(ids[3]), Some(ids[3])));
    assert_eq!(copy.changed_since(&es), ids_of(&[ids[2], ids[4], ids[4] + 1]));
}

fn ids_of(v: &[Id]) -> Changed {
    Changed::Ids(v.iter().copied().collect())
}

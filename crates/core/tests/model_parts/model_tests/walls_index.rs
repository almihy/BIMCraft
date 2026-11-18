//! `walls::index`'s tests that drive the Session (moved out of `walls/index.rs`).

use crate::model::Element;

use crate::walls::index::*;
use crate::{engine::Session, model::Model, walls};
use serde_json::json;

#[test]
fn indexed_joins_match_brute_force_for_storeys_arcs_tall_and_attached_walls() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s.execute("wall", &json!({"points":[[14000,0],[18000,0]],"arc":90,"height":9000})).unwrap();
    s.execute("wall", &json!({"points":[[0,0],[0,-5000]],"base_offset":-10000,"height":1000000})).unwrap();
    for level in ["Level 1", "Level 2"] {
        s.execute("wall", &json!({"points":[[0,0],[6000,0],[6000,4000]],"level":level})).unwrap();
    }
    agree(&s.model);
    // Walls meeting on the tiles' edges (every 6 m), crossing, and ending on each other, storeys apart.
    agree(&crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap());
}

/// Each wall's geometry found through the index is what it is found among every wall.
fn agree(m: &Model) {
    let index = Index::new(m);
    for (&id, e) in &m.elements {
        if !matches!(e, Element::Wall(_)) {
            continue;
        }
        assert_eq!(walls::centre_line(m, id), walls::centre_line_in(m, id, Some(&index)), "wall {id}");
        for end in [false, true] {
            assert_eq!(walls::cap(m, id, end), walls::cap_in(m, id, end, Some(&index)));
        }
        assert_eq!(walls::outline(m, id), walls::outline_in(m, id, Some(&index)));
        assert_eq!(walls::layer_lines(m, id), walls::layer_lines_in(m, id, Some(&index)));
        for openings in [false, true] {
            let a = walls::prisms(m, id, openings);
            let b = walls::prisms_in(m, id, openings, Some(&index));
            assert_eq!(a, b);
        }
    }
}

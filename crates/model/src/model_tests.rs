//! `model`'s tests that need only the model.

use crate::geom::P2;
use crate::ids::{Guid, Id};
use crate::model::*;
use crate::walls;

fn wall(level: Guid, a: P2, b: P2) -> Element {
    Element::Wall(Wall {
        params: Default::default(),
        type_name: "Generic - 200mm".into(),
        level,
        start: a,
        end: b,
        top_level: None,
        height: 3000.0,
        base_offset: 0.0,
        top_offset: 0.0,
        arc: 0.0,
        location: Location::Centre,
        attach_top: None,
        attach_base: None,
        room_bounding: true,
        join: true,
    })
}

/// A 6 x 4 m box of walls on Level 1 (id 100000), returning the bottom wall's id.
fn boxed(m: &mut Model) -> Id {
    let (a, b, c, d) = (P2(0.0, 0.0), P2(6000.0, 0.0), P2(6000.0, 4000.0), P2(0.0, 4000.0));
    let bottom = m.insert(wall(m.guid(100000), a, b));
    for (s, e) in [(b, c), (c, d), (d, a)] {
        m.insert(wall(m.guid(100000), s, e));
    }
    bottom
}

fn door(host: Guid, offset: f64) -> Element {
    Element::Door(Opening {
        params: Default::default(),
        family: None,
        type_name: "Single 900 x 2100".into(),
        host,
        offset,
        sill: 0.0,
        mark: String::new(),
        flip_facing: false,
        flip_hand: false,
        to_room_side: Side::Left,
    })
}

#[test]
fn a_new_model_has_two_levels_and_checks_clean() {
    let m = Model::new("House");
    let names: Vec<&str> = m.levels().iter().map(|(_, l)| l.name.as_str()).collect();
    assert_eq!(names, ["Level 1", "Level 2"]);
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn walls_constrained_to_a_level_take_its_height() {
    let mut m = Model::new("t");
    let id = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    let two = m.guid(100001);
    if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
        w.top_level = Some(two);
        w.base_offset = 100.0;
    }
    let w = m.wall(id).unwrap();
    assert_eq!((m.base_z(w), m.top_z(w)), (100.0, 3000.0));
}

#[test]
fn plans_cut_the_walls_that_span_their_cut_plane() {
    let mut m = Model::new("t");
    let one = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    let tall = m.insert(wall(m.guid(100000), P2(0.0, 1000.0), P2(5000.0, 1000.0)));
    if let Some(Element::Wall(w)) = m.elements.get_mut(tall) {
        w.height = 6000.0;
    }
    let (one, tall) = (m.wall(one).unwrap(), m.wall(tall).unwrap());
    assert!(m.cut_by(one, 100000) && !m.cut_by(one, 100001));
    assert!(m.cut_by(tall, 100000) && m.cut_by(tall, 100001));
}

#[test]
fn walls_join_where_they_meet_but_not_across_levels() {
    let mut m = Model::new("t");
    let bottom = boxed(&mut m);
    // Mitred at the corners: the outer corner where the outer faces cross.
    assert!(walls::cap(&m, bottom, false).contains(&P2(-100.0, -100.0)));
    // A partition ending mid-span on the bottom wall stops at its face.
    let t = m.insert(wall(m.guid(100000), P2(3000.0, 0.0), P2(3000.0, 4000.0)));
    assert_eq!(walls::cap(&m, t, false), [P2(3100.0, 100.0), P2(2900.0, 100.0)]);
    // A wall upstairs meeting the same corner doesn't join walls downstairs.
    let up = m.insert(wall(m.guid(100001), P2(0.0, 0.0), P2(0.0, -3000.0)));
    assert_eq!(walls::cap(&m, up, false), [P2(-100.0, 0.0), P2(100.0, 0.0)]);
}

#[test]
fn rooms_are_bounded_by_the_walls_their_level_cuts() {
    let mut m = Model::new("t");
    boxed(&mut m);
    let room = Room {
        params: Default::default(),
        level: m.guid(100000),
        point: P2(3000.0, 2000.0),
        name: "Hall".into(),
        number: "1".into(),
        ..Room::new(m.guid(100000), P2(0.0, 0.0), "", "")
    };
    assert_eq!(m.room_region(&room).map(|r| r.area()), Some(24e6));
    let upstairs = Room { level: m.guid(100001), ..room };
    assert_eq!(m.room_region(&upstairs), None, "Level 2 has no walls");
}

#[test]
fn check_names_unknown_types_and_lists_the_known_ones() {
    let mut m = Model::new("t");
    let id = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    if let Some(Element::Wall(w)) = m.elements.get_mut(id) {
        w.type_name = "Brick".into();
    }
    let e = m.check().unwrap_err();
    assert!(e.contains(&format!("wall {id}")) && e.contains("Brick") && e.contains("Generic - 200mm"), "{e}");
}

#[test]
fn check_allows_a_door_that_does_not_fit_its_wall_and_warns() {
    let mut m = Model::new("t");
    let host = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    let door = m.insert(door(m.guid(host), 300.0));
    assert_eq!(m.check(), Ok(()), "a design judgment is a warning, not a refusal");
    let w = m.review();
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!((w[0].rule, w[0].elements.clone()), ("opening_past_wall", vec![m.guid(door)]));
    assert!(w[0].message.contains("past the start"), "{}", w[0].message);
}

#[test]
fn check_allows_overlapping_openings_and_warns() {
    let mut m = Model::new("t");
    let host = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    m.insert(door(m.guid(host), 1000.0));
    m.insert(door(m.guid(host), 2000.0));
    assert_eq!((m.check(), m.review()), (Ok(()), Vec::new()));
    m.insert(door(m.guid(host), 2600.0));
    assert_eq!(m.check(), Ok(()));
    let w = m.review();
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].rule == "openings_overlap" && w[0].message.contains("overlap"), "{w:?}");
}

#[test]
fn openings_stacked_in_one_wall_do_not_overlap_and_ones_side_by_side_do() {
    // A wall six metres tall with a door and, over it, another: the Snowdon sample's storefront walls hold a window per
    // storey at the same place along the wall.
    let mut m = Model::new("t");
    let host = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    if let Some(Element::Wall(w)) = m.elements.get_mut(host) {
        w.height = 6000.0;
    }
    m.insert(door(m.guid(host), 1000.0));
    let mut above = door(m.guid(host), 1000.0);
    if let Element::Door(o) = &mut above {
        o.sill = 2500.0;
    }
    m.insert(above);
    assert_eq!((m.check(), m.review()), (Ok(()), Vec::new()), "openings that share their place but not their height do not overlap");
    m.insert(door(m.guid(host), 1000.0));
    assert_eq!(m.check(), Ok(()));
    assert!(m.review().iter().any(|w| w.rule == "openings_overlap"));
}

#[test]
fn check_refuses_dangling_references_and_bad_numbers() {
    let mut m = Model::new("t");
    m.insert(wall(Guid::of_id(99), P2(0.0, 0.0), P2(5000.0, 0.0)));
    assert!(m.check().unwrap_err().contains("level 99 doesn't exist"));
    let mut m = Model::new("t");
    let nowhere = Guid::new();
    m.insert(wall(nowhere, P2(0.0, 0.0), P2(5000.0, 0.0)));
    assert!(m.check().unwrap_err().contains(&format!("level {nowhere} doesn't exist")));
    // An element pointing at another by its id, not its GUID, is one a command forgot to resolve.
    let mut m = Model::new("t");
    m.insert(wall(Guid::of_id(100_000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    assert!(m.check().unwrap_err().contains("points at element 100000 by its id, not its GUID"));
    m.resolve();
    assert_eq!(m.check(), Ok(()));
    let mut m = Model::new("t");
    m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(f64::NAN, 0.0)));
    assert!(m.check().is_err());
    let mut m = Model::new("t");
    m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(0.5, 0.0)));
    assert!(m.check().unwrap_err().contains("short"));
}

#[test]
fn element_json_rejects_unknown_fields() {
    let ok = r#"{"category":"level","name":"Roof","elevation":6000}"#;
    assert!(serde_json::from_str::<Element>(ok).is_ok());
    let typo = r#"{"category":"level","name":"Roof","elevaton":6000}"#;
    let e = serde_json::from_str::<Element>(typo).unwrap_err().to_string();
    assert!(e.contains("elevaton"), "{e}");
}

#[test]
fn new_elements_default_to_the_types_a_new_model_starts_with() {
    let mut m = Model::new("t");
    let defaults = ["wall", "floor", "door", "window"].map(|c| m.default_type(c));
    assert_eq!(defaults, [Some("Generic - 200mm"), Some("Generic - 200mm"), Some("Single 900 x 2100"), Some("Fixed 1200 x 1500")]);
    assert_eq!(m.type_names("doors"), ["Single 800 x 2100", "Single 900 x 2100"], "any case, singular or plural");
    m.types.wall.remove("Generic - 200mm");
    assert_eq!(m.default_type("wall"), Some("Exterior - 300mm"), "else the first by name");
    assert_eq!(m.default_type("room"), None, "rooms have no types");
}

#[test]
fn insert_never_reuses_an_id_from_a_stale_counter() {
    let mut m = Model::new("t");
    m.next_id = 1;
    let id = m.insert(wall(m.guid(100000), P2(0.0, 0.0), P2(5000.0, 0.0)));
    assert_eq!(id, 100_002, "past the levels, and never under six digits");
}

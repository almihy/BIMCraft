//! Curtain walls (file format 30, `curtain.rs`): a wall of a curtain type lays out its grid's panels and mullions as
//! elements, again after whatever changes it.

use serde_json::{Value, json};

use crate::curtain::{self, Mullion, Panel};
use crate::engine::Session;
use crate::model::{CURTAIN_FORMAT, Element, Id};

pub(crate) fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

pub(crate) fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

pub(crate) fn wall(s: &mut Session, p: Value) -> Id {
    run(s, "wall", p)["ids"][0].as_u64().unwrap()
}

pub(crate) fn panels(s: &Session, wall: Id) -> Vec<(Id, Panel)> {
    let host = s.model.guid(wall);
    s.model
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::CurtainPanel(p) if p.host == host => Some((*id, p.clone())),
            _ => None,
        })
        .collect()
}

pub(crate) fn mullions(s: &Session, wall: Id) -> Vec<(Id, Mullion)> {
    let host = s.model.guid(wall);
    s.model
        .elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Mullion(x) if x.host == host => Some((*id, x.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_plain_curtain_wall_is_one_panel_of_glass_and_a_basic_wall_has_none() {
    let mut s = Session::new();
    assert!(s.model.types.curtain_panel.is_empty(), "a project that draws no curtain wall holds nothing of them");
    assert!(s.model.written_format() < CURTAIN_FORMAT);
    let basic = wall(&mut s, json!({"points": [[0, 5000], [6000, 5000]]}));
    assert!(panels(&s, basic).is_empty());
    // The starter type is made when first drawn with.
    let w = wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "type": "Curtain Wall", "height": 3000}));
    assert!(s.model.types.wall["Curtain Wall"].is_curtain());
    let ps = panels(&s, w);
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].1.span, [0.0, 6000.0, 0.0, 3000.0]);
    assert_eq!(ps[0].1.type_name, "Glazed");
    assert!(mullions(&s, w).is_empty());
    assert_eq!(s.model.written_format(), CURTAIN_FORMAT);
    // It has no solid of its own; its panel has one.
    assert!(crate::walls::prisms(&s.model, w, true).is_empty());
    assert!(!crate::solids::solids_in(&s.model, &[(ps[0].0, s.model.elements.get(ps[0].0).unwrap())], None).is_empty());
}

#[test]
fn a_storefront_lays_out_its_grid_with_mullions_and_again_when_the_wall_changes() {
    let mut s = Session::new();
    // Vertical: maximum spacing 1500 over 6000 is four panels. Horizontal: every 2400 from the base over 3000 is two.
    let w = wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}));
    let ps = panels(&s, w);
    assert_eq!(ps.len(), 8);
    let g = curtain::grid(&s.model, w).unwrap();
    assert_eq!(g.u.iter().map(|l| l.at).collect::<Vec<_>>(), [0.0, 1500.0, 3000.0, 4500.0, 6000.0]);
    assert_eq!(g.v.iter().map(|l| l.at).collect::<Vec<_>>(), [0.0, 2400.0, 3000.0]);
    // Five standing lines of two segments, three lying lines of four.
    let ms = mullions(&s, w);
    assert_eq!(ms.iter().filter(|(_, x)| x.vertical).count(), 10);
    assert_eq!(ms.iter().filter(|(_, x)| !x.vertical).count(), 12);
    // What the wall placed is pinned, as Revit's.
    assert!(s.model.is_pinned(ps[0].0) && s.model.is_pinned(ms[0].0));
    // Longer: five panels along, the ones that were there keeping their ids.
    let before: Vec<Id> = ps.iter().map(|(id, _)| *id).collect();
    run(&mut s, "modify", json!({"id": w, "end": [7500, 0]}));
    let after = panels(&s, w);
    assert_eq!(after.len(), 10);
    assert!(before.iter().all(|id| after.iter().any(|(a, _)| a == id)));
    // Undone, as it was.
    run(&mut s, "undo", json!({}));
    assert_eq!(panels(&s, w).len(), 8);
    // Deleted, its panels and mullions go with it.
    run(&mut s, "delete", json!({"ids": [w]}));
    assert!(s.model.elements.values().all(|e| curtain::host_of(e).is_none()));
}

#[test]
fn a_curtain_type_is_made_and_changed_by_the_type_command() {
    let mut s = Session::new();
    let made = run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Facade", "curtain": {
            "vertical": {"layout": "Fixed Distance", "spacing": 1200},
            "horizontal": {"layout": "fixed-number"},
            "vertical_mullions": {"interior": "50 x 150mm"},
        }}),
    );
    assert_eq!(made["type"]["curtain"]["vertical"]["layout"], "fixed-distance");
    assert_eq!(s.model.types.wall["Facade"].thickness, 150.0, "as deep as its mullions");
    let w = wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "type": "Facade", "height": 3000}));
    // Lines every 1200 from the start: four between the borders; four lying lines (the wall's own Number): five rows.
    assert_eq!(panels(&s, w).len(), 25);
    assert_eq!(mullions(&s, w).len(), 4 * 5);
    // The type changed: every wall of it laid out again.
    run(&mut s, "type", json!({"category": "wall", "name": "Facade", "curtain": {"horizontal": {"layout": "none"}}}));
    assert_eq!(panels(&s, w).len(), 5);
    assert_eq!(s.model.types.wall["Facade"].curtain.as_ref().unwrap().vertical.spacing, 1200.0, "kept when not given");
    // The wall's own Number and Justification.
    run(&mut s, "type", json!({"category": "wall", "name": "Facade", "curtain": {"vertical": {"layout": "fixed-number"}}}));
    run(&mut s, "modify", json!({"id": w, "curtain": {"vertical": {"number": 2}}}));
    assert_eq!(panels(&s, w).len(), 3);
    // What it names must be there; a curtain type has no layers.
    let e = fails(&mut s, "type", json!({"category": "wall", "name": "Facade", "curtain": {"panel": "Nope"}}));
    assert!(e.contains("Nope"), "{e}");
    let e = fails(&mut s, "type", json!({"category": "wall", "name": "Facade", "layers": [{"thickness": 100}]}));
    assert!(e.contains("no layers"), "{e}");
}

#[test]
fn a_curtain_wall_joins_nothing_and_still_bounds_rooms() {
    let mut s = Session::new();
    let a = wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "type": "Curtain Wall", "height": 3000}));
    let b = wall(&mut s, json!({"points": [[6000, 0], [6000, 4000], [0, 4000], [0, 0]], "height": 3000}));
    assert!(crate::walls::joined_at(&s.model, a, true).is_empty());
    assert!(!crate::walls::joined_at(&s.model, b, true).contains(&a));
    let room = run(&mut s, "room", json!({"point": [3000, 2000], "name": "Hall"}));
    let id = room["id"].as_u64().unwrap();
    let area = crate::commands::element_json(&s.model, id)["area_m2"].as_f64().unwrap();
    assert!((area - 23.0).abs() < 1.5, "{area}");
}

#[test]
fn a_saved_curtain_wall_opens_as_it_was() {
    let mut s = Session::new();
    let w = wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}));
    let text = s.model.to_json().unwrap();
    let back = crate::model::Model::read(&text).unwrap().0;
    assert_eq!(back.elements.len(), s.model.elements.len());
    let host = back.guid(w);
    assert_eq!(back.hosted_by(host).len(), 8 + 22);
    back.check().unwrap();
}

//! `derived`'s tests that drive the Session (moved out of `derived.rs`).

use crate::ids::Id;
use crate::model::Element;
use std::collections::BTreeSet;

use crate::derived::*;
use crate::engine::Session;
use crate::sample::tower::{Plate, tower_on};
use serde_json::json;

#[test]
fn removing_the_last_reference_or_rare_kind_removes_empty_lookup_buckets() {
    for (command, params) in
        [("text", json!({"view":100000,"at":[0,0],"text":"Only reference"})), ("section", json!({"start":[0,0],"end":[1000,0]}))]
    {
        let mut s = Session::new();
        let id = s.execute(command, &params).unwrap()["id"].as_u64().unwrap();
        let before = Lookup::of(&s.model.elements);
        let mut after = s.model.elements.clone();
        after.remove(id);
        let updated = before.updated(&after, &BTreeSet::from([id]));
        assert_eq!(updated, Lookup::of(&after), "remove only {command}");
        assert!(updated.referrers.values().all(|bucket| !bucket.is_empty()));
        assert!(updated.rare.values().all(|ids| !ids.is_empty()));
    }
}

#[test]
fn a_lookup_brought_up_to_date_is_the_one_made_again() {
    let mut s = Session::new();
    s.reset(tower_on(Plate::SMALL, 1).unwrap(), None);
    let first = s.model.lookup();
    assert_eq!(*first, Lookup::of(&s.model.elements));
    let wall = first.walls().nth(5).unwrap();
    let door = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).unwrap();
    for (cmd, p) in [
        ("move", json!({"ids": [wall], "by": [100, 0]})),
        ("copy", json!({"ids": [wall], "by": [0, 50]})),
        ("delete", json!({"ids": [door]})),
        ("wall", json!({"points": [[1000, 1000], [2000, 1000]]})),
        ("undo", json!({})),
    ] {
        let was = s.model.lookup();
        s.execute(cmd, &p).unwrap();
        let kept = s.model.lookup();
        assert_eq!(*kept, Lookup::of(&s.model.elements), "after {cmd}");
        // Deletion, like the other local edits, shares the untouched lookup columns.
        assert!(kept.shares(&was), "after {cmd}");
    }
    // A level that moves moves its walls' heights: the lookup is made again.
    let level = s.model.levels()[0].0;
    let m = s.model_mut();
    if let Some(Element::Level(l)) = m.elements.get_mut(level) {
        l.elevation += 2500.0;
    }
    assert_eq!(*s.model.lookup(), Lookup::of(&s.model.elements), "after a level moved");
    // What points at a wall: its doors, windows and tags.
    let guid = s.model.elements.guid(wall).unwrap();
    let referrers: Vec<&str> = s.model.lookup().referrers(&guid).filter_map(|id| s.model.elements.get(id)).map(Element::category).collect();
    assert!(referrers.iter().all(|c| ["door", "window", "tag", "dimension", "room"].contains(c)), "{referrers:?}");
}

#[test]
fn a_wall_raised_past_its_top_level_by_its_top_offset_is_near_at_the_heights_it_reaches() {
    let mut s = Session::new();
    let level = s.model.levels()[0].0;
    // Its top 3 m over its top level, the level it stands on: it reaches from 0 to 3000.
    let made = s.execute("wall", &json!({"points": [[0, 0], [5000, 0]], "top_level": level, "top_offset": 3000})).unwrap();
    let wall = made["ids"][0].as_u64().or_else(|| made["id"].as_u64()).unwrap();
    let w = s.model.wall(wall).unwrap();
    assert_eq!((s.model.base_z(w), s.model.top_z(w)), (0.0, 3000.0));
    let near = s.model.lookup().walls_near_at(&[(axis_box(w), 2500.0, 2600.0)], 0.0);
    assert!(near.contains(&wall), "a wall 3 m tall isn't near at 2.5 m");
    assert_eq!(*s.model.lookup(), Lookup::of(&s.model.elements));
}

#[test]
fn the_walls_near_a_wall_are_every_wall_its_ends_lie_on_and_that_end_on_it() {
    let m = tower_on(Plate::SMALL, 2).unwrap();
    let l = m.lookup();
    let walls: Vec<(Id, &crate::model::Wall)> = l.walls().filter_map(|id| Some((id, m.wall(id)?))).collect();
    let on = |p: crate::geom::P2, w: &crate::model::Wall| crate::walls::Axis::of(w).distance(p, 0.0) <= crate::model::TOL;
    let mut pairs = 0;
    for (a, wa) in &walls {
        let near = l.walls_near(axis_box(wa), 2.0 * crate::model::TOL);
        for (b, wb) in &walls {
            if a != b && (on(wb.start, wa) || on(wb.end, wa) || on(wa.start, wb) || on(wa.end, wb)) {
                assert!(near.contains(b), "wall {b} meets wall {a} but isn't near it");
                pairs += 1;
            }
        }
        // Near, not everything: a wall's tiles are its neighbourhood.
        assert!(near.len() < walls.len() / 2, "wall {a}: {} near of {}", near.len(), walls.len());
    }
    assert!(pairs > 500, "{pairs}");
}

#[test]
fn the_walls_near_a_wall_at_its_heights_are_every_wall_it_meets_and_is_stacked_with() {
    let mut m = tower_on(Plate::SMALL, 4).unwrap();
    // One wall attached at its top: the lookup doesn't know how high it goes, so it is near at any height.
    let floor = m.elements.iter().find(|(_, e)| matches!(e, Element::Floor(_))).map(|(id, _)| m.guid(*id)).unwrap();
    let attached = m.lookup().walls().next().unwrap();
    if let Some(Element::Wall(w)) = m.elements.get_mut(attached) {
        w.attach_top = Some(floor);
    }
    let l = m.lookup();
    let walls: Vec<(Id, &crate::model::Wall)> = l.walls().filter_map(|id| Some((id, m.wall(id)?))).collect();
    let on = |p: crate::geom::P2, w: &crate::model::Wall| crate::walls::Axis::of(w).distance(p, 0.0) <= crate::model::TOL;
    let mut pairs = 0;
    for (a, wa) in &walls {
        let at = (axis_box(wa), m.base_z(wa), m.top_z(wa));
        let near = l.walls_near_at(&[at], 2.0 * crate::model::TOL);
        for (b, wb) in &walls {
            let meet = on(wb.start, wa) || on(wb.end, wa) || on(wa.start, wb) || on(wa.end, wb);
            if a != b && meet && crate::walls::stacked(&m, wa, wb) {
                assert!(near.contains(b), "wall {b} meets wall {a} and is stacked with it, but isn't near it");
                pairs += 1;
            }
        }
        // Near in height too: of the walls near in plan, those of the storeys about it.
        let plan = l.walls_near(axis_box(wa), 2.0 * crate::model::TOL);
        assert!(near.is_subset(&plan));
        assert!(near.len() * 3 < plan.len() * 2, "wall {a}: {} near of {} in plan", near.len(), plan.len());
    }
    assert!(pairs > 500, "{pairs}");
    // The attached wall, at any height in its tiles.
    let wa = m.wall(attached).unwrap();
    assert!(l.walls_near_at(&[(axis_box(wa), 1e6, 1e6 + 1.0)], 0.0).contains(&attached));
    assert!(!l.walls_near_at(&[(axis_box(wa), 1e6, 1e6 + 1.0)], 0.0).contains(&walls[1].0) || walls[1].0 == attached);
}

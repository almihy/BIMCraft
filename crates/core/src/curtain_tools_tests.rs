//! A curtain wall worked on by hand (`commands/curtain.rs`): Curtain Grid, Add/Remove Segments, Mullion, and what Delete,
//! Pin, Unpin, a change of type, a copy, a mirror, a flip and a split do to one.

use serde_json::{Value, json};

use crate::curtain;
use crate::curtain_tests::{fails, mullions, panels, run, wall};
use crate::engine::Session;
use crate::model::{Element, Id};

fn ids(v: &Value) -> Vec<Id> {
    v.as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default()
}

/// A plain curtain wall 6000 long and 3000 tall along x, from the origin.
fn plain(s: &mut Session) -> Id {
    wall(s, json!({"points": [[0, 0], [6000, 0]], "type": "Curtain Wall", "height": 3000}))
}

fn spans(s: &Session, w: Id) -> Vec<[f64; 4]> {
    let mut all: Vec<[f64; 4]> = panels(s, w).into_iter().map(|(_, p)| p.span).collect();
    all.sort_by(|a, b| a[2].total_cmp(&b[2]).then(a[0].total_cmp(&b[0])));
    all
}

#[test]
fn curtain_grid_adds_lines_by_hand_and_they_move_and_go() {
    let mut s = Session::new();
    let w = plain(&mut s);
    let made = run(&mut s, "curtain_grid", json!({"wall": w, "vertical": true, "at": 2000}));
    let line = ids(&made["ids"])[0];
    assert!(matches!(s.model.elements.get(line), Some(Element::CurtainGrid(l)) if l.vertical && l.at == 2000.0));
    assert_eq!(ids(&made["panels"]).len(), 2);
    assert_eq!(spans(&s, w), [[0.0, 2000.0, 0.0, 3000.0], [2000.0, 6000.0, 0.0, 3000.0]]);
    // By place: a point in plan is a standing line, an elevation a lying one; several at once.
    let made = run(&mut s, "curtain_grid", json!({"host": w, "lines": [{"point": [4000, 300]}, {"z": 1000}]}));
    assert_eq!(ids(&made["ids"]).len(), 2);
    assert_eq!(ids(&made["panels"]).len(), 6);
    let g = curtain::grid(&s.model, w).unwrap();
    assert_eq!(g.u.iter().map(|l| l.at).collect::<Vec<_>>(), [0.0, 2000.0, 4000.0, 6000.0]);
    assert_eq!(g.v.iter().map(|l| l.at).collect::<Vec<_>>(), [0.0, 1000.0, 3000.0]);
    // Outside the wall, on a line that is there, or on no curtain wall: refused, saying what to do.
    let e = fails(&mut s, "curtain_grid", json!({"wall": w, "vertical": true, "at": 6000}));
    assert!(e.contains("outside wall") && e.contains("between"), "{e}");
    let e = fails(&mut s, "curtain_grid", json!({"wall": w, "vertical": false, "at": 3500}));
    assert!(e.contains("outside wall"), "{e}");
    let e = fails(&mut s, "curtain_grid", json!({"wall": w, "vertical": true, "at": 2000.5}));
    assert!(e.contains("already"), "{e}");
    let e = fails(&mut s, "curtain_grid", json!({"wall": w, "lines": [{"vertical": true, "at": 3000}, {"vertical": true, "at": 3000.2}]}));
    assert!(e.contains("already"), "{e}");
    let e = fails(&mut s, "curtain_grid", json!({"wall": w, "at": 3000}));
    assert!(e.contains("`vertical` is required"), "{e}");
    let basic = wall(&mut s, json!({"points": [[0, 5000], [6000, 5000]]}));
    let e = fails(&mut s, "curtain_grid", json!({"wall": basic, "vertical": true, "at": 1000}));
    assert!(e.contains("isn't a curtain wall"), "{e}");
    // Moved by `modify`, and a standing one in plan by `move`: it slides along its wall.
    run(&mut s, "modify", json!({"id": line, "at": 2500}));
    assert_eq!(spans(&s, w)[0], [0.0, 2500.0, 0.0, 1000.0]);
    run(&mut s, "move", json!({"ids": [line], "by": [-500, 800]}));
    assert_eq!(spans(&s, w)[0], [0.0, 2000.0, 0.0, 1000.0]);
    // Deleted, the panels either side are one again.
    run(&mut s, "delete", json!({"ids": [line]}));
    assert_eq!(panels(&s, w).len(), 4);
    assert_eq!(spans(&s, w)[0], [0.0, 4000.0, 0.0, 1000.0]);
}

#[test]
fn a_segment_removed_joins_two_panels_and_put_back_parts_them() {
    let mut s = Session::new();
    let w = plain(&mut s);
    run(
        &mut s,
        "curtain_grid",
        json!({"wall": w, "lines": [{"vertical": true, "at": 2000}, {"vertical": true, "at": 4000}, {"vertical": false, "at": 1000}]}),
    );
    assert_eq!(panels(&s, w).len(), 6);
    // The standing line at 2000, beside the lower row: the two lower left panels become one.
    let done = run(&mut s, "add_remove_segments", json!({"wall": w, "at": [2000, 500]}));
    assert_eq!(done["removed"], json!([[2000.0, 500.0]]));
    assert_eq!(ids(&done["panels"]).len(), 5);
    assert_eq!(spans(&s, w)[0], [0.0, 4000.0, 0.0, 1000.0]);
    assert_eq!(s.model.wall(w).unwrap().curtain.merged.len(), 1);
    // Again is no change; a border's segment stays.
    run(&mut s, "add_remove_segments", json!({"wall": w, "at": [2000, 500]}));
    assert_eq!(panels(&s, w).len(), 5);
    let e = fails(&mut s, "add_remove_segments", json!({"wall": w, "at": [10, 500]}));
    assert!(e.contains("border"), "{e}");
    // The lying line over the joined panel's left half alone would make an L: refused, naming what to give together.
    let e = fails(&mut s, "add_remove_segments", json!({"wall": w, "at": [1000, 1000]}));
    assert!(e.contains("not a rectangle") && e.contains("segments"), "{e}");
    let list: Value = serde_json::from_str(&e[e.find("[[").unwrap()..=e.rfind("]]").unwrap() + 1]).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 3, "{e}");
    // Given together, they make one panel of four cells.
    run(&mut s, "add_remove_segments", json!({"wall": w, "segments": list}));
    assert_eq!(panels(&s, w).len(), 3);
    assert_eq!(spans(&s, w)[0], [0.0, 4000.0, 0.0, 3000.0]);
    // One put back alone would stand inside the panel: refused; all put back, six panels again.
    let e = fails(&mut s, "add_remove_segments", json!({"wall": w, "at": [2000, 500], "remove": false}));
    assert!(e.contains("not a rectangle"), "{e}");
    let back = run(
        &mut s,
        "add_remove_segments",
        json!({"wall": w, "remove": false, "segments": [[2000, 500], [2000, 2000], [1000, 1000], [3000, 1000]]}),
    );
    assert_eq!(ids(&back["panels"]).len(), 6);
    assert!(s.model.wall(w).unwrap().curtain.merged.is_empty());
    // By a point in plan and an elevation; the mullion on a removed segment goes with it.
    run(&mut s, "mullion", json!({"wall": w, "on": "segment", "at": [4000, 500]}));
    assert_eq!(mullions(&s, w).len(), 1);
    run(&mut s, "add_remove_segments", json!({"wall": w, "point": [4000, 200], "z": 500}));
    assert_eq!(panels(&s, w).len(), 5);
    assert!(mullions(&s, w).is_empty());
}

#[test]
fn the_mullion_tool_puts_them_on_a_line_a_segment_or_every_line() {
    let mut s = Session::new();
    let w = plain(&mut s);
    assert!(s.model.types.mullion.is_empty());
    run(&mut s, "curtain_grid", json!({"wall": w, "lines": [{"vertical": true, "at": 3000}, {"vertical": false, "at": 1000}]}));
    // One segment: the standing line's upper one. The starter types are made, and the default is taken.
    let made = run(&mut s, "mullion", json!({"wall": w, "on": "segment", "at": [3000, 2000]}));
    assert_eq!(made["type"], "50 x 150mm");
    let one = ids(&made["ids"]);
    assert_eq!(one.len(), 1);
    let ms = mullions(&s, w);
    assert_eq!(ms.len(), 1);
    assert!(ms[0].1.free && ms[0].1.vertical && (ms[0].1.at, ms[0].1.from, ms[0].1.to) == (3000.0, 1000.0, 3000.0));
    assert!(!s.model.is_pinned(one[0]), "put on by hand, it is no type's");
    // The whole line: the segment that has one keeps it.
    let made = run(&mut s, "mullion", json!({"wall": w, "at": [3000, 100], "type": "30mm Square"}));
    assert_eq!((ids(&made["ids"]).len(), made["skipped"].as_u64()), (1, Some(1)));
    assert_eq!(mullions(&s, w).iter().filter(|(_, x)| x.type_name == "30mm Square").count(), 1);
    // Every line, the borders too: two standing borders and the line of two segments each, three lying lines of two.
    let made = run(&mut s, "mullion", json!({"wall": w, "on": "all"}));
    assert_eq!(ids(&made["ids"]).len(), 12 - 2);
    assert_eq!(mullions(&s, w).len(), 12);
    // They stay through a layout, and a type that is not there is refused.
    run(&mut s, "modify", json!({"id": w, "end": [9000, 0]}));
    assert_eq!(mullions(&s, w).len(), 12);
    let e = fails(&mut s, "mullion", json!({"wall": w, "on": "all", "type": "Nope"}));
    assert!(e.contains("Nope") && e.contains("50 x 150mm"), "{e}");
    let e = fails(&mut s, "mullion", json!({"wall": w, "on": "sideways", "at": [0, 0]}));
    assert!(e.contains("grid_line"), "{e}");
    let e = fails(&mut s, "mullion", json!({"wall": w}));
    assert!(e.contains("say where"), "{e}");
    s.model.check().unwrap();
}

fn storefront(s: &mut Session) -> Id {
    wall(s, json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}))
}

#[test]
fn a_mullion_deleted_stays_gone_a_panel_is_not_deleted_and_a_wall_takes_them_all() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let (ps, ms) = (panels(&s, w), mullions(&s, w));
    assert_eq!((ps.len(), ms.len()), (8, 22));
    // What the type placed is pinned: Revit's own words, naming `unpin`.
    let e = fails(&mut s, "delete", json!({"ids": [ms[0].0]}));
    assert!(e.contains("Pinned objects were not deleted") && e.contains("unpin"), "{e}");
    // Unpinned, it goes, and a layout does not bring it back.
    run(&mut s, "unpin", json!({"ids": [ms[0].0]}));
    let gone = run(&mut s, "delete", json!({"ids": [ms[0].0]}));
    assert_eq!(ids(&gone["deleted"]), [ms[0].0]);
    assert_eq!(mullions(&s, w).len(), 21);
    assert_eq!(s.model.wall(w).unwrap().curtain.no_mullion.len(), 1);
    run(&mut s, "modify", json!({"id": w, "height": 3200}));
    assert_eq!(mullions(&s, w).len(), 21);
    // The Mullion tool puts one back there.
    let mid = (ms[0].1.from + ms[0].1.to) / 2.0;
    let (along, up) = if ms[0].1.vertical { (ms[0].1.at, mid) } else { (mid, ms[0].1.at) };
    let back = run(&mut s, "mullion", json!({"wall": w, "on": "segment", "at": [along, up], "vertical": ms[0].1.vertical}));
    assert_eq!(ids(&back["ids"]).len(), 1);
    assert!(s.model.wall(w).unwrap().curtain.no_mullion.is_empty());
    // A panel is never deleted on its own, pinned or freed.
    let e = fails(&mut s, "delete", json!({"ids": [ps[0].0]}));
    assert!(e.contains("can't be deleted") && e.contains("Empty") && e.contains("add_remove_segments"), "{e}");
    run(&mut s, "unpin", json!({"ids": [ps[0].0]}));
    let e = fails(&mut s, "delete", json!({"ids": [ps[0].0]}));
    assert!(e.contains("can't be deleted"), "{e}");
    // The wall takes its panels, mullions and grid lines, listed, with no word of pins nobody set.
    let line = ids(&run(&mut s, "curtain_grid", json!({"wall": w, "vertical": false, "at": 1000}))["ids"])[0];
    let mine: Vec<Id> = s.model.hosted_by(s.model.guid(w));
    let gone = run(&mut s, "delete", json!({"ids": [w, ps[1].0]}));
    let deleted = ids(&gone["deleted"]);
    assert!(deleted.contains(&w) && deleted.contains(&line) && mine.iter().all(|id| deleted.contains(id)), "{gone}");
    assert!(gone.get("pinned").is_none() && gone.get("note").is_none(), "{gone}");
    assert!(s.model.elements.values().all(|e| curtain::host_of(e).is_none()));
}

#[test]
fn unpin_frees_a_panel_or_a_mullion_to_take_a_type_and_pin_gives_it_back_to_the_wall() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let (p, x) = (panels(&s, w)[0].0, mullions(&s, w)[0].0);
    // Pinned, its type is the wall's.
    let e = fails(&mut s, "modify", json!({"id": p, "type": "Solid"}));
    assert!(e.contains("pinned") && e.contains("unpin"), "{e}");
    let second = panels(&s, w)[1].0;
    let e = fails(&mut s, "match_properties", json!({"from": second, "ids": [p]}));
    assert!(e.contains("unpin"), "{e}");
    let done = run(&mut s, "unpin", json!({"ids": [p, x]}));
    assert_eq!(ids(&done["unpinned"]), [p, x]);
    assert!(!s.model.is_pinned(p) && !s.model.is_pinned(x));
    // Freed, each takes a type and keeps it through a layout.
    run(&mut s, "modify", json!({"id": p, "type": "Solid"}));
    run(&mut s, "modify", json!({"id": x, "type": "30mm Square"}));
    run(&mut s, "modify", json!({"id": w, "height": 3300}));
    assert_eq!(panels(&s, w).iter().find(|(id, _)| *id == p).unwrap().1.type_name, "Solid");
    assert_eq!(mullions(&s, w).iter().find(|(id, _)| *id == x).unwrap().1.type_name, "30mm Square");
    // An empty panel leaves its cell open; Match Properties gives a freed panel another's type.
    run(&mut s, "modify", json!({"id": p, "type": "Empty"}));
    assert!(crate::solids::solids_in(&s.model, &[(p, s.model.elements.get(p).unwrap())], None).is_empty());
    let other = panels(&s, w)[1].0;
    run(&mut s, "match_properties", json!({"from": other, "ids": [p]}));
    assert_eq!(panels(&s, w).iter().find(|(id, _)| *id == p).unwrap().1.type_name, "Glazed");
    run(&mut s, "modify", json!({"id": p, "type": "Solid"}));
    // Pinned again, each is the type's again.
    let done = run(&mut s, "pin", json!({"ids": [p, x]}));
    assert_eq!((ids(&done["pinned"]), ids(&done["already"])), (vec![p, x], vec![]));
    assert!(s.model.is_pinned(p) && s.model.is_pinned(x) && s.model.pinned.is_empty());
    assert_eq!(panels(&s, w).iter().find(|(id, _)| *id == p).unwrap().1.type_name, "Glazed");
    assert_eq!(mullions(&s, w).iter().find(|(id, _)| *id == x).unwrap().1.type_name, "50 x 150mm");
    // A mullion put on by hand where the type has none has nothing to be pinned to.
    let plain = plain(&mut s);
    run(&mut s, "curtain_grid", json!({"wall": plain, "vertical": true, "at": 3000}));
    let hand = ids(&run(&mut s, "mullion", json!({"wall": plain, "at": [3000, 100]}))["ids"])[0];
    let e = fails(&mut s, "pin", json!({"ids": [hand]}));
    assert!(e.contains("by hand"), "{e}");
}

#[test]
fn a_wall_given_a_curtain_type_loses_its_doors_and_one_given_a_basic_type_its_grid() {
    let mut s = Session::new();
    let w = wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "height": 3000}));
    let door = run(&mut s, "door", json!({"wall": w, "offset": 1500}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"wall": w, "offset": 4000}))["id"].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [door]}));
    // As Revit: the inserts go, pinned or not, and the answer names them.
    let said = run(&mut s, "modify", json!({"id": w, "type": "Storefront"}));
    let deleted = ids(&said["deleted"]);
    assert!(deleted.contains(&door) && deleted.contains(&window) && deleted.len() == 2, "{said}");
    assert!(said["note"].as_str().unwrap().contains("Switching wall type to family curtain wall"), "{said}");
    assert_eq!(panels(&s, w).len(), 8);
    // A curtain wall hosts no ordinary door or window.
    let e = fails(&mut s, "door", json!({"wall": w, "offset": 1500}));
    assert!(e.contains("curtain wall") && e.contains("panel"), "{e}");
    let e = fails(&mut s, "window", json!({"at": [3000, 0]}));
    assert!(e.contains("curtain wall"), "{e}");
    // Back to a basic type: its panels, mullions and hand-made lines go.
    run(&mut s, "curtain_grid", json!({"wall": w, "vertical": true, "at": 700}));
    run(&mut s, "modify", json!({"id": w, "type": "Generic - 200mm"}));
    assert!(s.model.hosted_by(s.model.guid(w)).is_empty());
    s.model.check().unwrap();
    run(&mut s, "door", json!({"wall": w, "offset": 1500}));
    // A basic type in use made a curtain type by the `type` command does the same; and back again.
    let said = run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "curtain": true}));
    assert_eq!(ids(&said["deleted"]).len(), 1, "{said}");
    assert_eq!(panels(&s, w).len(), 1);
    run(&mut s, "curtain_grid", json!({"wall": w, "vertical": true, "at": 700}));
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "curtain": null, "thickness": 200}));
    assert!(s.model.hosted_by(s.model.guid(w)).is_empty());
    s.model.check().unwrap();
}

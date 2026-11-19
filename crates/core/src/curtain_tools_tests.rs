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

/// A curtain wall worked on by hand: standing lines at 2000 and 4000 and a lying one at 1000 (six cells), the segment
/// between the two lower right cells removed, a mullion on the upper segment of the line at 2000, the upper left panel
/// freed and made Solid, and the mullion put on the lower segment of the line at 2000 deleted.
fn worked(s: &mut Session) -> Id {
    let w = plain(s);
    run(
        s,
        "curtain_grid",
        json!({"wall": w, "lines": [{"vertical": true, "at": 2000}, {"vertical": true, "at": 4000}, {"vertical": false, "at": 1000}]}),
    );
    run(s, "add_remove_segments", json!({"wall": w, "at": [4000, 500]}));
    run(s, "mullion", json!({"wall": w, "at": [2000, 2000]}));
    let low = mullions(s, w).into_iter().find(|(_, x)| x.from == 0.0).unwrap().0;
    run(s, "delete", json!({"ids": [low]}));
    let panel = panels(s, w).into_iter().find(|(_, p)| p.span == [0.0, 2000.0, 1000.0, 3000.0]).unwrap().0;
    run(s, "unpin", json!({"ids": [panel]}));
    run(s, "modify", json!({"id": panel, "type": "Solid"}));
    w
}

/// Where a curtain wall's panels and mullions are in the model, with their types: each panel's two ends in plan and its
/// heights, each mullion's middle. Sorted, to the millimetre, so two walls that stand alike read alike.
fn picture(s: &Session, w: Id) -> Vec<String> {
    let axis = crate::walls::Axis::of(s.model.wall(w).unwrap());
    let at = |along: f64| {
        let p = axis.at(along, 0.0);
        (p.0.round() as i64, p.1.round() as i64)
    };
    let mut out: Vec<String> = panels(s, w)
        .into_iter()
        .map(|(_, p)| {
            let (mut a, mut b) = (at(p.span[0]), at(p.span[1]));
            if b < a {
                std::mem::swap(&mut a, &mut b);
            }
            format!("panel {a:?} {b:?} {} {} {} {}", p.span[2], p.span[3], p.type_name, p.free)
        })
        .collect();
    out.extend(mullions(s, w).into_iter().map(|(_, x)| {
        let mid = (x.from + x.to) / 2.0;
        let (place, up) = if x.vertical { (at(x.at), mid) } else { (at(mid), x.at) };
        format!("mullion {place:?} {up} {} {}", x.type_name, x.free)
    }));
    out.sort();
    out
}

fn shifted(picture: &[String], from: &str, to: &str) -> Vec<String> {
    let mut out: Vec<String> = picture.iter().map(|l| l.replace(from, to)).collect();
    out.sort();
    out
}

fn lines(s: &Session, w: Id) -> usize {
    s.model.hosted_by(s.model.guid(w)).into_iter().filter(|id| matches!(s.model.elements.get(*id), Some(Element::CurtainGrid(_)))).count()
}

#[test]
fn a_copy_of_a_curtain_wall_lays_out_its_own_panels_with_what_was_done_by_hand() {
    let mut s = Session::new();
    let w = worked(&mut s);
    let was = picture(&s, w);
    assert_eq!(was.len(), 5 + 1, "{was:#?}");
    assert!(was.iter().any(|l| l.contains("Solid true")) && was.iter().any(|l| l.starts_with("mullion")));
    // Copied with a panel and a mullion of its own picked too: none is copied itself, none doubles.
    let panel = panels(&s, w)[0].0;
    let mullion = mullions(&s, w)[0].0;
    let made = ids(&run(&mut s, "copy", json!({"ids": [w, panel, mullion], "by": [0, 5000]}))["ids"]);
    let copy = *made.iter().find(|id| s.model.wall(**id).is_some()).unwrap();
    assert_eq!(lines(&s, copy), 3);
    assert_eq!(picture(&s, copy), shifted(&was, ", 0)", ", 5000)"));
    assert_eq!(picture(&s, w), was, "the wall copied is as it was");
    let count = |s: &Session, w: Id| s.model.hosted_by(s.model.guid(w)).len();
    assert_eq!(count(&s, copy), count(&s, w));
    // Its lines are its own: moving the copy's changes the copy alone.
    let host = s.model.guid(copy);
    let line = made
        .iter()
        .copied()
        .find(|id| matches!(s.model.elements.get(*id), Some(Element::CurtainGrid(l)) if l.host == host && l.vertical && l.at == 2000.0))
        .unwrap();
    run(&mut s, "modify", json!({"id": line, "at": 2500}));
    assert_eq!(picture(&s, w), was);
    // The clipboard, an array and a paste to another level do the same.
    run(&mut s, "copy_to_clipboard", json!({"ids": [w]}));
    let pasted = ids(&run(&mut s, "paste", json!({"by": [0, 9000]}))["ids"]);
    let pasted = *pasted.iter().find(|id| s.model.wall(**id).is_some()).unwrap();
    assert_eq!(picture(&s, pasted), shifted(&was, ", 0)", ", 9000)"));
    let arrayed = run(&mut s, "array", json!({"ids": [w], "by": [0, -4000], "count": 3}));
    let members = arrayed["members"].as_array().unwrap();
    assert_eq!(members.len(), 2);
    for (n, member) in members.iter().enumerate() {
        let wall = ids(member).into_iter().find(|id| s.model.wall(*id).is_some()).unwrap();
        assert_eq!(picture(&s, wall), shifted(&was, ", 0)", &format!(", {})", -4000 * (n as i64 + 1))));
    }
    let up = ids(&run(&mut s, "paste", json!({"ids": [w], "levels": ["Level 2"]}))["ids"]);
    let up = *up.iter().find(|id| s.model.wall(**id).is_some()).unwrap();
    assert_eq!(picture(&s, up), was);
    // A panel or a mullion alone is not copied, moved, turned or mirrored.
    for (command, more) in [
        ("copy", json!({"by": [0, 100]})),
        ("move", json!({"by": [0, 100]})),
        ("rotate", json!({"about": [0, 0], "angle": 90, "copy": true})),
    ] {
        let mut p = more;
        p["ids"] = json!([panels(&s, w).into_iter().find(|(_, p)| p.free).unwrap().0]);
        let e = fails(&mut s, command, p);
        assert!(e.contains("curtain wall's grid"), "{command}: {e}");
    }
    s.model.check().unwrap();
}

#[test]
fn a_curtain_wall_mirrored_flipped_or_moved_keeps_its_grid_where_it_is() {
    let mut s = Session::new();
    let w = worked(&mut s);
    let was = picture(&s, w);
    // Flipped, it runs the other way and everything stands where it stood.
    let start = s.model.wall(w).unwrap().start;
    run(&mut s, "flip", json!({"ids": [w]}));
    assert_ne!(s.model.wall(w).unwrap().start, start);
    assert_eq!(picture(&s, w), was);
    assert_eq!(s.model.wall(w).unwrap().curtain.merged.len(), 1);
    assert_eq!(s.model.wall(w).unwrap().curtain.no_mullion.len(), 1);
    run(&mut s, "flip", json!({"ids": [w]}));
    assert_eq!(picture(&s, w), was);
    // Mirrored across x = 5000: the mirror image has the removed segment on the left.
    let mirrored = shifted(
        &was.iter()
            .map(|l| {
                // Each x to 10000 - x.
                let mut out = String::new();
                let mut rest = l.as_str();
                while let Some(i) = rest.find('(') {
                    let j = i + rest[i..].find(',').unwrap();
                    let x: i64 = rest[i + 1..j].parse().unwrap();
                    out.push_str(&rest[..=i]);
                    out.push_str(&(10000 - x).to_string());
                    rest = &rest[j..];
                }
                out.push_str(rest);
                out
            })
            .collect::<Vec<_>>(),
        "",
        "",
    );
    let image = ids(&run(&mut s, "mirror", json!({"ids": [w], "axis": [[5000, 0], [5000, 1000]], "copy": true}))["ids"]);
    let image = *image.iter().find(|id| s.model.wall(**id).is_some()).unwrap();
    let ends = |l: &String| -> String {
        // A panel's two ends, lower first, as `picture` gives them.
        if !l.starts_with("panel") {
            return l.clone();
        }
        let parts: Vec<&str> = l.splitn(2, ") (").collect();
        let (a, rest) = (parts[0].trim_start_matches("panel ("), parts[1]);
        let (b, tail) = rest.split_once(") ").unwrap();
        let (mut a, mut b) = (a.to_string(), b.to_string());
        let x = |v: &str| v.split(',').next().unwrap().trim().parse::<i64>().unwrap();
        if x(&b) < x(&a) {
            std::mem::swap(&mut a, &mut b);
        }
        format!("panel ({a}) ({b}) {tail}")
    };
    let mut mirrored: Vec<String> = mirrored.iter().map(ends).collect();
    mirrored.sort();
    assert_eq!(picture(&s, image), mirrored);
    run(&mut s, "delete", json!({"ids": [image]}));
    run(&mut s, "mirror", json!({"ids": [w], "axis": [[5000, 0], [5000, 1000]]}));
    assert_eq!(picture(&s, w), mirrored);
    // The type's own lines stay too: Exterior Glazing's stand every 1500 from the start, and after a flip from the end.
    let g = wall(&mut s, json!({"points": [[0, 20000], [5000, 20000]], "type": "Exterior Glazing", "height": 3000}));
    let glazing = picture(&s, g);
    assert_eq!(glazing.len(), 4);
    run(&mut s, "flip", json!({"ids": [g]}));
    assert_eq!(picture(&s, g), glazing);
    assert_eq!(s.model.wall(g).unwrap().curtain.vertical.justification, curtain::Justification::End);
    // A wall whose start is dragged keeps its hand-made lines where they are in the model.
    let h = wall(&mut s, json!({"points": [[0, 30000], [6000, 30000]], "type": "Curtain Wall", "height": 3000}));
    let line = ids(&run(&mut s, "curtain_grid", json!({"wall": h, "vertical": true, "at": 2000}))["ids"])[0];
    run(&mut s, "move", json!({"ids": [h], "by": [-1000, 0], "end": "start"}));
    assert!(matches!(s.model.elements.get(line), Some(Element::CurtainGrid(l)) if l.at == 3000.0));
    // Moved or turned whole, with its panels picked too, it goes as one.
    let mut all = vec![h];
    all.extend(s.model.hosted_by(s.model.guid(h)));
    run(&mut s, "move", json!({"ids": all, "by": [0, -7000]}));
    run(&mut s, "rotate", json!({"ids": all, "about": [0, 0], "angle": 90}));
    assert!(matches!(s.model.elements.get(line), Some(Element::CurtainGrid(l)) if l.at == 3000.0));
    assert_eq!(panels(&s, h).len(), 2);
    s.model.check().unwrap();
}

#[test]
fn a_curtain_wall_split_lays_out_each_half_with_what_falls_in_it() {
    let mut s = Session::new();
    let w = worked(&mut s);
    let was = picture(&s, w);
    let halves = ids(&run(&mut s, "split", json!({"id": w, "at": [3000, 0]}))["ids"]);
    let (first, second) = (halves[0], halves[1]);
    assert_eq!((lines(&s, first), lines(&s, second)), (2, 2), "a standing line each, and the lying line in both");
    // Together they stand as the wall did, but for the cut: the panels it crosses are two each.
    let (a, b) = (picture(&s, first), picture(&s, second));
    assert_eq!(a.iter().filter(|l| l.starts_with("panel")).count(), 4);
    assert_eq!(b.iter().filter(|l| l.starts_with("panel")).count(), 3, "the removed segment stays removed: {b:#?}");
    for l in was.iter().filter(|l| l.starts_with("mullion") || l.contains("Solid")) {
        assert!(a.contains(l), "{l} in {a:#?}");
    }
    assert_eq!(s.model.wall(second).unwrap().curtain.merged.len(), 1);
    assert!(s.model.wall(first).unwrap().curtain.merged.is_empty());
    assert_eq!(s.model.wall(first).unwrap().curtain.no_mullion.len(), 1);
    // A line on the cut is a border of both.
    let on = ids(&run(&mut s, "curtain_grid", json!({"wall": second, "vertical": true, "at": 2000}))["ids"])[0];
    let again = ids(&run(&mut s, "split", json!({"id": second, "at": [5000, 0]}))["ids"]);
    assert!(!s.model.elements.contains_key(on));
    assert_eq!((lines(&s, again[0]), lines(&s, again[1])), (2, 1));
    s.model.check().unwrap();
}

/// An edit of a curtain wall, or of something of one, picked by the dice.
fn edit(s: &mut Session, dice: &mut crate::oracle_tests::Dice) -> (String, Value) {
    let of = |s: &Session, dice: &mut crate::oracle_tests::Dice, categories: &[&str]| {
        let ids: Vec<Id> = s.model.elements.iter().filter(|(_, e)| categories.contains(&e.category())).map(|(id, _)| *id).collect();
        ids.get(dice.below(ids.len())).copied()
    };
    let curtain = |s: &Session, dice: &mut crate::oracle_tests::Dice| {
        let ids: Vec<Id> = s.model.elements.iter().filter(|(id, _)| curtain::grid(&s.model, **id).is_some()).map(|(id, _)| *id).collect();
        ids.get(dice.below(ids.len())).copied()
    };
    let place = |dice: &mut crate::oracle_tests::Dice| json!([dice.below(7) * 1000, dice.below(7) * 500]);
    let by = |dice: &mut crate::oracle_tests::Dice| json!([dice.mm(3000), dice.mm(3000)]);
    let child = ["curtainpanel", "mullion", "curtaingrid"];
    match dice.below(22) {
        0 | 1 => {
            ("curtain_grid".into(), json!({"wall": curtain(s, dice), "vertical": dice.below(2) == 0, "at": 250 * (1 + dice.below(20))}))
        }
        2 | 3 => ("add_remove_segments".into(), json!({"wall": curtain(s, dice), "at": place(dice), "remove": dice.below(3) > 0})),
        4 => {
            ("mullion".into(), json!({"wall": curtain(s, dice), "at": place(dice), "on": (["grid_line", "segment", "all"][dice.below(3)])}))
        }
        5 => ("unpin".into(), json!({"ids": [of(s, dice, &child)]})),
        6 => ("pin".into(), json!({"ids": [of(s, dice, &child)]})),
        7 => ("delete".into(), json!({"ids": [of(s, dice, &["mullion", "curtaingrid", "wall"])]})),
        8 => ("modify".into(), json!({"id": of(s, dice, &["curtainpanel"]), "type": (["Solid", "Empty", "Glazed"][dice.below(3)])})),
        9 => ("modify".into(), json!({"id": of(s, dice, &["mullion"]), "type": (["30mm Square", "25mm Radius"][dice.below(2)])})),
        10 => ("modify".into(), json!({"id": of(s, dice, &["curtaingrid"]), "at": 250 * (1 + dice.below(20))})),
        11 => ("move".into(), json!({"ids": [of(s, dice, &["wall", "curtaingrid", "curtainpanel"])], "by": by(dice)})),
        12 => ("copy".into(), json!({"ids": [of(s, dice, &["wall", "curtaingrid"])], "by": [dice.mm(3000), 9000 + dice.mm(3000)]})),
        13 => ("mirror".into(), json!({"ids": [curtain(s, dice)], "axis": [[0, 0], [1000, dice.mm(1000)]], "copy": dice.below(2) == 0})),
        14 => ("flip".into(), json!({"ids": [curtain(s, dice)]})),
        15 => {
            let wall = curtain(s, dice);
            let at = wall.and_then(|w| s.model.wall(w)).map(|w| w.start + (w.end - w.start) * (0.2 + 0.1 * dice.below(6) as f64));
            ("split".into(), json!({"id": wall, "at": at.map(|p| [p.0, p.1])}))
        }
        16 => ("modify".into(), json!({"id": curtain(s, dice), "height": 2500 + 250 * dice.below(8)})),
        17 => {
            let types = ["Storefront", "Exterior Glazing", "Curtain Wall", "Generic - 200mm"];
            ("modify".into(), json!({"id": of(s, dice, &["wall"]), "type": types[dice.below(4)]}))
        }
        18 => ("move".into(), json!({"ids": [curtain(s, dice)], "by": by(dice), "end": if dice.below(2) == 0 { "start" } else { "end" }})),
        19 => ("door".into(), json!({"wall": of(s, dice, &["wall"]), "offset": 1000 + dice.below(3000)})),
        20 => {
            let rule = json!({"layout": (["none", "fixed-distance", "fixed-number", "maximum-spacing"][dice.below(4)]), "spacing": 900 + 300 * dice.below(4)});
            ("type".into(), json!({"category": "wall", "name": "Storefront", "curtain": {"vertical": rule}}))
        }
        _ => (if dice.below(2) == 0 { "undo" } else { "redo" }.into(), json!({})),
    }
}

/// Curtain walls edited at random: the engine holds each command's check by what changed to the whole check, and its
/// library synced both ways (in tests), and a wall is always laid out as its grid says.
#[test]
fn random_edits_of_curtain_walls_check_the_same_by_what_changed_as_whole() {
    let mut kinds = std::collections::BTreeSet::new();
    for seed in [1u64, 2, 3, 4] {
        let mut s = Session::new();
        wall(&mut s, json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}));
        wall(&mut s, json!({"points": [[0, 4000], [6000, 4000]], "type": "Exterior Glazing", "height": 3000}));
        wall(&mut s, json!({"points": [[8000, 0], [8000, 6000]], "type": "Curtain Wall", "height": 3000, "arc": 30}));
        wall(&mut s, json!({"points": [[0, -4000], [6000, -4000]], "height": 3000}));
        let mut dice = crate::oracle_tests::Dice(0x5eed_c0ff_ee11_d00d ^ seed);
        let (mut done, mut refused) = (0, 0);
        for _ in 0..120 {
            let (cmd, p) = edit(&mut s, &mut dice);
            match s.execute(&cmd, &p) {
                Ok(_) => {
                    done += 1;
                    kinds.insert(cmd.clone());
                }
                Err(_) => refused += 1,
            }
            // Laid out: nothing left for the upkeep to do.
            let walls: Vec<Id> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id).collect();
            for w in walls {
                let want = curtain::laid(&s.model, w);
                let have = panels(&s, w).len() + mullions(&s, w).len();
                let same = want.iter().all(|(g, e)| s.model.elements.id(*g).and_then(|id| s.model.elements.get(id)) == Some(e));
                assert!(same && have == want.len(), "seed {seed}: after {cmd} {p}, wall {w} is not as its grid lays it out");
            }
        }
        assert!(done > 30 && refused > 0, "seed {seed}: {done} done, {refused} refused: the edits should do both");
        assert_eq!(s.model.check(), Ok(()));
    }
    for kind in ["curtain_grid", "add_remove_segments", "mullion", "unpin", "delete", "modify", "move", "copy", "mirror", "flip", "split"] {
        assert!(kinds.contains(kind), "no {kind} was done");
    }
}

#[test]
fn a_curtain_wall_inserted_from_a_file_is_laid_out_here_with_what_was_done_by_hand() {
    let mut a = Session::new();
    let w = worked(&mut a);
    let was = picture(&a, w);
    let path = std::env::temp_dir().join(format!("bimcraft-curtain-insert-{}.bimcraft", std::process::id()));
    run(&mut a, "save", json!({"path": path.to_str().unwrap()}));
    // The whole file, and the wall alone: its lines come with it, and its panels and mullions are this project's.
    for ids in [json!(null), json!([w])] {
        let mut b = Session::new();
        wall(&mut b, json!({"points": [[0, 9000], [6000, 9000]], "type": "Storefront", "height": 3000}));
        let made = run(&mut b, "insert", json!({"path": path.to_str().unwrap(), "ids": ids}));
        let here = made["ids"][w.to_string()].as_u64().unwrap();
        assert_eq!(picture(&b, here), was, "{made}");
        assert_eq!(lines(&b, here), 3);
        let host = b.model.guid(here);
        for (id, e) in b.model.elements.iter().filter(|(_, e)| matches!(e, Element::CurtainPanel(_) | Element::Mullion(_))) {
            let key = match e {
                Element::CurtainPanel(p) if p.host == host => p.key.clone(),
                Element::Mullion(x) if x.host == host => x.key.clone(),
                _ => continue,
            };
            assert_eq!(b.model.elements.guid(*id), Some(curtain::child_guid(b.model.uid, host, &key)));
        }
        assert!(made["ids"].as_object().unwrap().values().all(|id| b.model.elements.contains_key(id.as_u64().unwrap())), "{made}");
        b.model.check().unwrap();
    }
    let _ = std::fs::remove_file(&path);
}

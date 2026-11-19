//! A door in a curtain wall (`curtain_door`, `curtain.rs`): a panel swapped for a door, as Revit's storefront entrance. It
//! is a door (schedules, rooms, plans, IFC) that fills its cell: as large as the cell, and wherever the cell goes.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::curtain_tests::{fails, panels, run, wall};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{CURTAIN_FORMAT, Element, Id, Model, Opening};
use crate::prim::Prim;
use crate::solids::Solid;
use crate::sync::{self, Changes};

/// A storefront 6 m long and 3 m tall: four cells along (1500 each) by two up (2400 and 600).
fn storefront(s: &mut Session) -> Id {
    wall(s, json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}))
}

/// The panel of wall `w` whose span starts at `s0` along it and `z0` up it.
fn panel_at(s: &Session, w: Id, s0: f64, z0: f64) -> Id {
    panels(s, w).into_iter().find(|(_, p)| p.span[0] == s0 && p.span[2] == z0).map(|(id, _)| id).expect("a panel there")
}

fn door(s: &Session, id: Id) -> Opening {
    match s.model.elements.get(id) {
        Some(Element::Door(o)) => o.clone(),
        e => panic!("{id} is no door: {e:?}"),
    }
}

fn doors(s: &Session) -> Vec<Id> {
    s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).collect()
}

/// A storefront with a door in its second cell along, the low row: the wall and the door.
fn entrance(s: &mut Session) -> (Id, Id) {
    let w = storefront(s);
    let panel = panel_at(s, w, 1500.0, 0.0);
    let d = run(s, "curtain_door", json!({"panel": panel}))["id"].as_u64().unwrap();
    (w, d)
}

fn size(s: &Session, id: Id) -> (f64, f64) {
    let t = s.model.opening_dims(s.model.elements.get(id).unwrap()).unwrap();
    (t.width, t.height)
}

#[test]
fn a_panel_is_swapped_for_a_door_that_fills_its_cell() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let panel = panel_at(&s, w, 1500.0, 0.0);
    let key = panels(&s, w).into_iter().find(|(id, _)| *id == panel).unwrap().1.key;
    // The panel is pinned by its wall: the swap stands for Unpin and the Type Selector.
    let r = run(&mut s, "curtain_door", json!({"panel": panel}));
    let d = r["id"].as_u64().unwrap();
    assert_eq!((r["wall"].as_u64(), r["panel_key"].as_str()), (Some(w), Some(key.as_str())));
    assert_eq!((r["width"].as_f64(), r["height"].as_f64()), (Some(1500.0), Some(2400.0)));
    let o = door(&s, d);
    assert_eq!((o.offset, o.sill, o.cell, o.panel.as_deref()), (2250.0, 0.0, Some([1500.0, 3000.0, 0.0, 2400.0]), Some(key.as_str())));
    assert_eq!(o.host, s.model.guid(w));
    // Its size is the cell's, whatever its type says.
    let own = s.model.types.door[&o.type_name].clone();
    assert_ne!((own.width, own.height), (1500.0, 2400.0));
    assert_eq!(size(&s, d), (1500.0, 2400.0));
    // The panel is gone, and nothing else of the wall: its mullions stay.
    assert_eq!(panels(&s, w).len(), 7);
    assert!(!s.model.elements.contains_key(panel));
    assert_eq!(crate::curtain_tests::mullions(&s, w).len(), 22);
    assert_eq!(s.model.check(), Ok(()));
    assert_eq!(s.model.written_format(), CURTAIN_FORMAT);
    // As agents read it.
    let j = crate::commands::element_json(&s.model, d);
    assert_eq!((j["width"].as_f64(), j["height"].as_f64(), j["panel"].as_str()), (Some(1500.0), Some(2400.0), Some(key.as_str())), "{j}");
    let j = crate::commands::element_json(&s.model, w);
    assert_eq!((j["panels"].as_u64(), j["doors"].as_u64()), (Some(7), Some(1)), "{j}");
    // Undone, the panel is back at its id; done again, the door is.
    run(&mut s, "undo", json!({}));
    assert!(doors(&s).is_empty() && panels(&s, w).len() == 8 && s.model.elements.contains_key(panel));
    run(&mut s, "redo", json!({}));
    assert_eq!((doors(&s), panels(&s, w).len()), (vec![d], 7));
}

#[test]
fn the_cell_is_named_by_a_place_and_a_cell_takes_one_door() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let d =
        run(&mut s, "curtain_door", json!({"wall": w, "at": [5000, 1000], "flip_facing": true, "flip_hand": true}))["id"].as_u64().unwrap();
    let o = door(&s, d);
    assert_eq!((o.cell, o.flip_facing, o.flip_hand), (Some([4500.0, 6000.0, 0.0, 2400.0]), true, true));
    let e = run(&mut s, "curtain_door", json!({"wall": w, "point": [700, 300]}))["id"].as_u64().unwrap();
    assert_eq!(door(&s, e).cell, Some([0.0, 1500.0, 0.0, 2400.0]));
    assert_ne!(door(&s, e).mark, door(&s, d).mark);
    // A cell that has a door, a place off the wall, something that is no panel, a type that isn't.
    assert!(fails(&mut s, "curtain_door", json!({"wall": w, "at": [5200, 100]})).contains("already"));
    assert!(fails(&mut s, "curtain_door", json!({"panel": d})).contains("already"));
    assert!(fails(&mut s, "curtain_door", json!({"wall": w, "at": [9000, 100]})).contains("outside"));
    assert!(fails(&mut s, "curtain_door", json!({"panel": w})).contains("isn't a curtain panel"));
    let free = panel_at(&s, w, 1500.0, 0.0);
    assert!(fails(&mut s, "curtain_door", json!({"panel": free, "type": "Nope"})).contains("Nope"));
    // An ordinary door or window is still refused, and the door's refusal names the tool.
    assert!(fails(&mut s, "door", json!({"wall": w, "offset": 2000})).contains("curtain_door"));
    assert!(fails(&mut s, "window", json!({"wall": w, "offset": 2000})).contains("curtain wall"));
    // Its type changes as any door's does; its size stays its cell's.
    run(&mut s, "type", json!({"category": "door", "name": "Entrance", "width": 1000, "height": 2100, "operation": "double-swing"}));
    run(&mut s, "modify", json!({"id": d, "type": "Entrance"}));
    assert_eq!((door(&s, d).type_name.as_str(), size(&s, d)), ("Entrance", (1500.0, 2400.0)));
}

#[test]
fn the_door_follows_its_cell_when_the_grid_changes() {
    let mut s = Session::new();
    let (w, d) = entrance(&mut s);
    // The type's spacing: three cells of 2000, and the door's is the second still.
    let r = run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Storefront", "curtain": {"vertical": {"layout": "maximum-spacing", "spacing": 2000}}}),
    );
    assert!(r.get("deleted").is_none(), "the door is the wall's own: {r}");
    assert_eq!((door(&s, d).cell, door(&s, d).offset, size(&s, d)), (Some([2000.0, 4000.0, 0.0, 2400.0]), 3000.0, (2000.0, 2400.0)));
    assert_eq!(panels(&s, w).len(), 5);
    // The wall lengthened and raised off its level: its cells move, and the door with its own.
    run(&mut s, "modify", json!({"id": w, "end": [9000, 0]}));
    assert_eq!((door(&s, d).cell, size(&s, d)), (Some([1800.0, 3600.0, 0.0, 2400.0]), (1800.0, 2400.0)));
    // Sliding it along the wall is not how it moves.
    let e = fails(&mut s, "move", json!({"ids": [d], "by": [500, 0]}));
    assert!(e.contains("grid line") && e.contains("curtain_door"), "{e}");
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn deleting_the_door_gives_the_cell_its_panel_back() {
    let mut s = Session::new();
    let (w, d) = entrance(&mut s);
    run(&mut s, "delete", json!({"ids": [d]}));
    assert!(doors(&s).is_empty());
    let ps = panels(&s, w);
    assert_eq!(ps.len(), 8);
    assert!(ps.iter().any(|(_, p)| p.span == [1500.0, 3000.0, 0.0, 2400.0] && p.type_name == "Glazed" && !p.free));
    // The wall deleted takes its door.
    let (w, d) = entrance(&mut s);
    run(&mut s, "delete", json!({"ids": [w]}));
    assert!(!s.model.elements.contains_key(d));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn a_door_whose_cell_goes_is_in_the_cell_that_holds_its_place_or_goes_with_the_grid() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    // A line by hand splits the second cell; the door fills the part past it.
    let line = run(&mut s, "curtain_grid", json!({"wall": w, "vertical": true, "at": 2000}))["ids"][0].as_u64().unwrap();
    let d = run(&mut s, "curtain_door", json!({"wall": w, "at": [2500, 1000]}))["id"].as_u64().unwrap();
    assert_eq!((door(&s, d).cell, size(&s, d)), (Some([2000.0, 3000.0, 0.0, 2400.0]), (1000.0, 2400.0)));
    assert_eq!(panels(&s, w).len(), 9);
    // The line moves: the door's cell is narrower, and the door.
    run(&mut s, "modify", json!({"id": line, "at": 2200}));
    assert_eq!((door(&s, d).offset, size(&s, d)), (2600.0, (800.0, 2400.0)));
    // The line deleted: its cell is no more, and the door fills the one that holds its place.
    run(&mut s, "delete", json!({"ids": [line]}));
    assert_eq!((door(&s, d).cell, door(&s, d).panel.as_deref()), (Some([1500.0, 3000.0, 0.0, 2400.0]), Some("ut1|v0")));
    assert_eq!(panels(&s, w).len(), 7);
    assert_eq!(s.model.check(), Ok(()));
    // The lying line under the cell's top removed with its neighbour's: a door's cell joined to the one over it is taller.
    run(&mut s, "add_remove_segments", json!({"wall": w, "at": [2250, 2400]}));
    assert_eq!((door(&s, d).cell, size(&s, d)), (Some([1500.0, 3000.0, 0.0, 3000.0]), (1500.0, 3000.0)));
    assert_eq!(panels(&s, w).len(), 6);
    // Tagged, in a wall given a basic type: the grid goes, and the door and its tag with it, said in the answer.
    let tag = run(&mut s, "tag", json!({"ids": [d]}))["ids"][0].as_u64().unwrap();
    let r = run(&mut s, "modify", json!({"id": w, "type": "Generic - 200mm"}));
    assert!(r["deleted"].as_array().is_some_and(|a| a.contains(&json!(d))), "{r}");
    assert!(!s.model.elements.contains_key(d) && !s.model.elements.contains_key(tag));
    assert!(panels(&s, w).is_empty());
    assert_eq!(s.model.check(), Ok(()));
    // And when the type itself is made a basic one.
    let mut s = Session::new();
    let (w, d) = entrance(&mut s);
    let r = run(&mut s, "type", json!({"category": "wall", "name": "Storefront", "curtain": null, "thickness": 200}));
    assert!(r["deleted"].as_array().is_some_and(|a| a.contains(&json!(d))), "{r}");
    assert!(doors(&s).is_empty() && panels(&s, w).is_empty());
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn the_door_stays_where_it_stands_when_its_wall_is_flipped_mirrored_split_or_copied() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    // In the first cell: after a flip the wall runs back, and `u0|v0` names the cell at its other end.
    let d = run(&mut s, "curtain_door", json!({"wall": w, "at": [700, 1000]}))["id"].as_u64().unwrap();
    let at = |s: &Session, d: Id| s.model.anchor(d).map(|p| ((p.0 * 10.0).round() / 10.0, (p.1 * 10.0).round() / 10.0));
    assert_eq!(at(&s, d), Some((750.0, 0.0)));
    run(&mut s, "flip", json!({"ids": [w]}));
    assert_eq!((at(&s, d), door(&s, d).cell), (Some((750.0, 0.0)), Some([4500.0, 6000.0, 0.0, 2400.0])));
    assert_eq!((doors(&s).len(), panels(&s, w).len()), (1, 7));
    // A copy has a door of its own in the same cell, and seven panels.
    let made: Vec<Id> =
        run(&mut s, "copy", json!({"ids": [w], "by": [0, 5000]}))["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    let copy = *made.iter().find(|id| s.model.wall(**id).is_some()).unwrap();
    let twin =
        *made.iter().find(|id| matches!(s.model.elements.get(**id), Some(Element::Door(_)))).expect("the door is copied with its wall");
    assert_eq!((at(&s, twin), door(&s, twin).cell, door(&s, twin).host), (Some((750.0, 5000.0)), door(&s, d).cell, s.model.guid(copy)));
    assert_eq!(panels(&s, copy).len(), 7);
    // Mirrored about x = 5000, it stands at the mirror image of where it stood.
    run(&mut s, "mirror", json!({"ids": [copy], "axis": [[5000, 0], [5000, 1000]]}));
    assert_eq!(at(&s, twin), Some((9250.0, 5000.0)));
    assert_eq!((size(&s, twin), panels(&s, copy).len()), ((1500.0, 2400.0), 7));
    // Split, the half the door stands in has it.
    let halves: Vec<Id> =
        run(&mut s, "split", json!({"id": w, "at": [3000, 0]}))["ids"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    assert_eq!(at(&s, d), Some((750.0, 0.0)));
    let host = s.model.elements.id(door(&s, d).host).unwrap();
    assert!(halves.contains(&host));
    assert_eq!(size(&s, d), (1500.0, 2400.0));
    assert_eq!(halves.iter().map(|h| panels(&s, *h).len()).sum::<usize>(), 7);
    assert_eq!(s.model.check(), Ok(()));
    // Pasted alone, it is an ordinary door of the basic wall it lands in.
    let basic = wall(&mut s, json!({"points": [[0, -5000], [6000, -5000]]}));
    run(&mut s, "copy_to_clipboard", json!({"ids": [d]}));
    let r = run(&mut s, "paste", json!({"by": [0, -5000]}));
    let pasted = r["ids"].as_array().and_then(|a| a.first()).and_then(Value::as_u64).unwrap_or_else(|| panic!("{r}"));
    let o = door(&s, pasted);
    assert_eq!((o.host, o.panel, o.cell), (s.model.guid(basic), None, None));
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn it_is_a_door_in_the_schedule_with_its_cells_size_and_the_rooms_either_side() {
    let mut s = Session::new();
    // A box of walls with the storefront across its middle: a room on each side of it.
    run(&mut s, "wall", json!({"points": [[0, -4000], [6000, -4000], [6000, 4000], [0, 4000]], "closed": true}));
    let (_, d) = entrance(&mut s);
    let north = run(&mut s, "room", json!({"point": [3000, 2000], "name": "Shop"}))["id"].as_u64().unwrap();
    let south = run(&mut s, "room", json!({"point": [3000, -2000], "name": "Street"}))["id"].as_u64().unwrap();
    let t = run(
        &mut s,
        "schedule",
        json!({"name": "Door Schedule", "category": "door", "fields": ["Mark", "Width", "Height", "From Room: Name", "To Room: Name", "Wall"]}),
    );
    assert_eq!(t["rows"].as_array().map(Vec::len), Some(1), "{t}");
    let row = &t["rows"][0];
    assert_eq!((row[1].as_f64(), row[2].as_f64()), (Some(1500.0), Some(2400.0)), "{t}");
    assert_eq!((row[3].as_str(), row[4].as_str()), (Some("Street"), Some("Shop")), "a room either side: {t}");
    // It swings to the wall's left (north) as placed: its To Room, and From Room the other.
    let j = crate::commands::element_json(&s.model, d);
    assert_eq!(
        (j["to_room"]["id"].as_u64(), j["from_room"]["id"].as_u64(), j["swings_into"]["id"].as_u64()),
        (Some(north), Some(south), Some(north)),
        "{j}"
    );
    // Flipped, it swings the other way, and its rooms stay.
    run(&mut s, "flip", json!({"ids": [d], "facing": true, "hand": false}));
    let j = crate::commands::element_json(&s.model, d);
    assert_eq!((j["to_room"]["id"].as_u64(), j["swings_into"]["id"].as_u64()), (Some(north), Some(south)), "{j}");
    // Nothing warns of it as of a door that doesn't fit its wall.
    let warned = run(&mut s, "warnings", json!({}));
    assert!(!warned.to_string().contains(&format!("door {d}")), "{warned}");
}

#[test]
fn it_has_a_leaf_of_its_cells_size_and_a_swing_in_plan() {
    let mut s = Session::new();
    let (w, d) = entrance(&mut s);
    let e = s.model.elements.get(d).unwrap();
    let solids = crate::solids::solids_in(&s.model, &[(d, e)], None);
    let [Solid::Box { a, z, .. }] = solids.as_slice() else { panic!("one leaf: {}", solids.len()) };
    assert_eq!((*a, *z), ((-750.0, 750.0), (0.0, 2400.0)));
    // Its wall has no solid to cut, and the panel that was there none.
    assert!(crate::walls::prisms(&s.model, w, true).is_empty());
    // In plan: its leaf, open, and the arc it sweeps, from its hinge at the cell's start to 1500 out to the wall's left.
    let level = s.model.levels()[0].0;
    let plan = crate::render::plan(&s.model, level, 1600, 1000);
    let mine: Vec<Vec<P2>> = (plan.prims.iter().zip(&plan.owners))
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, .. } if *o == Some(d) => Some(pts.iter().map(|q| plan.view.mm(*q)).collect()),
            _ => None,
        })
        .collect();
    let arc = mine.iter().find(|l| l.len() > 10).unwrap_or_else(|| panic!("a swing: {mine:?}"));
    let (lo, hi) = arc.iter().fold((f64::MAX, f64::MIN), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
    let reach = arc.iter().map(|q| q.1).fold(f64::MIN, f64::max);
    assert!((lo - 1500.0).abs() < 5.0 && (hi - 3000.0).abs() < 5.0, "{lo} {hi}");
    assert!(reach > 1400.0 && reach < 1700.0, "{reach}");
    // The plan of a model with one draws without it too (every view of the sweep).
    assert!(!mine.is_empty());
}

#[test]
fn ifc_has_it_as_a_door_of_its_cells_size_under_its_curtain_wall_with_no_void() {
    let mut s = Session::new();
    let (_, d) = entrance(&mut s);
    let text = crate::ifc::export(&s.model);
    let of = |class: &str| -> Vec<&str> {
        text.lines().filter(|l| l.split_once('=').is_some_and(|(_, r)| r.starts_with(&format!("{class}(")))).collect()
    };
    let doors = of("IFCDOOR");
    assert_eq!(doors.len(), 1, "{doors:?}");
    // OverallHeight, then OverallWidth.
    assert!(doors[0].contains(",2400.,1500.,.DOOR."), "{}", doors[0]);
    assert!(of("IFCRELVOIDSELEMENT").is_empty() && of("IFCOPENINGELEMENT").is_empty() && of("IFCRELFILLSELEMENT").is_empty());
    // Seven plates, and the door aggregated with them under the curtain wall.
    assert_eq!(of("IFCPLATE").len(), 7);
    let number = doors[0].split_once('=').unwrap().0;
    let curtain = of("IFCCURTAINWALL")[0].split_once('=').unwrap().0;
    let whole = of("IFCRELAGGREGATES")
        .into_iter()
        .find(|l| l.contains(&format!("{number},")) || l.contains(&format!("{number})")))
        .expect("aggregated");
    assert!(whole.contains(&format!(",{curtain},(")), "{whole}");
    // Not also contained in the storey.
    let contained = of("IFCRELCONTAINEDINSPATIALSTRUCTURE");
    assert!(!contained.iter().any(|l| l.contains(&format!("{number},")) || l.contains(&format!("{number})"))), "{contained:?}");
    assert!(of("IFCDOORTYPE").len() == 1);
    let _ = d;
}

#[test]
fn it_is_saved_and_opened_as_it_was() {
    let mut s = Session::new();
    let (w, d) = entrance(&mut s);
    let path = std::env::temp_dir().join(format!("bimcraft-curtain-door-{}.bimcraft", std::process::id()));
    run(&mut s, "save", json!({"path": path.to_str().unwrap()}));
    let text = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m, *s.model);
    assert_eq!(m.written_format(), CURTAIN_FORMAT);
    assert_eq!(m.check(), Ok(()));
    // Opened, it is laid out as it was: nothing to do.
    let mut t = Session::new();
    t.reset(m, None);
    run(&mut t, "modify", json!({"id": w, "base_offset": 0}));
    assert_eq!((door(&t, d), panels(&t, w).len()), (door(&s, d), 7));
    // A door of an ordinary wall with a cell, or a curtain wall's door without one, is refused.
    let basic = wall(&mut s, json!({"points": [[0, 5000], [6000, 5000]]}));
    let mut m = (*s.model).clone();
    let other = m.guid(basic);
    if let Some(Element::Door(o)) = m.elements.get_mut(d) {
        o.host = other;
    }
    assert!(m.check().unwrap_err().contains("isn't a curtain wall"));
    let mut m = (*s.model).clone();
    if let Some(Element::Door(o)) = m.elements.get_mut(d) {
        (o.panel, o.cell) = (None, None);
    }
    assert!(m.check().unwrap_err().contains("curtain_door"));
}

#[test]
fn two_copies_never_have_a_panel_and_a_door_in_one_cell() {
    let mut a = Session::new();
    run(&mut a, "new", json!({}));
    let w = storefront(&mut a);
    let text = a.model.to_json().unwrap();
    let mut b = Session::new();
    b.reset(Model::read(&text).unwrap().0, None);
    // One swaps a panel for a door while the other makes the wall taller.
    let (a0, b0) = (a.model.clone(), b.model.clone());
    let d = run(&mut a, "curtain_door", json!({"wall": w, "at": [2000, 1000]}))["id"].as_u64().unwrap();
    run(&mut b, "modify", json!({"id": w, "height": 5000}));
    let to_b = Changes::read(&sync::changes(&a0, &a.model).unwrap().to_json()).unwrap();
    let to_a = Changes::read(&sync::changes(&b0, &b.model).unwrap().to_json()).unwrap();
    b.merge(&to_b, &BTreeSet::new()).unwrap();
    a.merge(&to_a, &BTreeSet::new()).unwrap();
    let guid = a.model.guid(d);
    for s in [&mut a, &mut b] {
        // The next command on the wall settles it as it now is.
        let wall = s.model.elements.id(a0.guid(w)).unwrap();
        run(s, "modify", json!({"id": wall, "base_offset": 0}));
        let door = s.model.elements.id(guid).expect("the door is in both copies");
        let o = door_of(&s.model, door);
        assert_eq!(o.cell, Some([1500.0, 3000.0, 0.0, 2400.0]));
        let cells: Vec<[f64; 4]> = panels(s, wall).into_iter().map(|(_, p)| p.span).collect();
        assert!(!cells.contains(&[1500.0, 3000.0, 0.0, 2400.0]), "a panel and a door in one cell");
        assert_eq!(cells.len(), 4 * 3 - 1);
        assert_eq!(s.model.check(), Ok(()));
    }
    // Both swap the same panel at once: one door stays, the same in both.
    let (a0, b0) = (a.model.clone(), b.model.clone());
    let in_b = b.model.elements.id(a.model.guid(w)).unwrap();
    run(&mut a, "curtain_door", json!({"wall": w, "at": [5000, 1000]}));
    run(&mut b, "curtain_door", json!({"wall": in_b, "at": [5000, 1000]}));
    let to_b = Changes::read(&sync::changes(&a0, &a.model).unwrap().to_json()).unwrap();
    let to_a = Changes::read(&sync::changes(&b0, &b.model).unwrap().to_json()).unwrap();
    b.merge(&to_b, &BTreeSet::new()).unwrap();
    a.merge(&to_a, &BTreeSet::new()).unwrap();
    let mut kept = Vec::new();
    for s in [&mut a, &mut b] {
        let wall = s.model.elements.id(a0.guid(w)).unwrap();
        run(s, "modify", json!({"id": wall, "base_offset": 0}));
        let there: Vec<_> = s
            .model
            .elements
            .with_guids()
            .filter(|(_, _, e)| matches!(e, Element::Door(o) if o.cell == Some([4500.0, 6000.0, 0.0, 2400.0])))
            .map(|(_, g, _)| g)
            .collect();
        assert_eq!(there.len(), 1);
        kept.push(there[0]);
        assert_eq!(s.model.check(), Ok(()));
    }
    assert_eq!(kept[0], kept[1]);
}

fn door_of(m: &Model, id: Id) -> Opening {
    match m.elements.get(id) {
        Some(Element::Door(o)) => o.clone(),
        e => panic!("{id} is no door: {e:?}"),
    }
}

#[test]
fn a_tagged_and_dimensioned_door_goes_cleanly_when_match_properties_makes_its_wall_basic() {
    let mut s = Session::new();
    let (w, d) = entrance(&mut s);
    let basic = wall(&mut s, json!({"points": [[0, 5000], [6000, 5000]]}));
    let side = wall(&mut s, json!({"points": [[-2000, 0], [-2000, 5000]]}));
    let tag = run(&mut s, "tag", json!({"ids": [d]}))["ids"][0].as_u64().unwrap();
    run(&mut s, "pin", json!({"ids": [d]}));
    run(&mut s, "dimension", json!({"refs": [{"id": side}, {"id": d, "face": "centre"}], "at": [0, -1000]}));
    run(&mut s, "match_properties", json!({"from": basic, "ids": [w]}));
    assert!(!s.model.elements.contains_key(d) && !s.model.elements.contains_key(tag));
    assert!(panels(&s, w).is_empty() && doors(&s).is_empty());
    assert_eq!(s.model.check(), Ok(()));
    // And comes back with its panelless cell when that is undone.
    run(&mut s, "undo", json!({}));
    assert_eq!((doors(&s), panels(&s, w).len()), (vec![d], 7));
}

#[test]
fn an_ifc_file_with_the_door_reads_back_as_a_sound_model() {
    let mut s = Session::new();
    entrance(&mut s);
    let text = crate::ifc::export(&s.model);
    let back = crate::import::ifc(text.as_bytes(), "x").unwrap();
    assert_eq!(back.model.check(), Ok(()));
}

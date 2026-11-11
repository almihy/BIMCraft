//! Copy to Clipboard and Paste, as Revit's Ctrl+C and Ctrl+V: pasted at an offset, at the same place, or aligned to
//! levels (the same plan position on each level chosen, the heights moved by the levels' difference).

use crate::engine::Session;
use crate::model::{Element, Guid};
use serde_json::{Value, json};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

/// Level 1 (0), Level 2 (3000) and Levels 3 to 5 (6000, 9000, 12000), and an 8 x 6 m box of walls on Level 1 with a door
/// in its first wall and a window in its second: the ids of the levels, of the walls and of the door and window.
fn building() -> (Session, Vec<u64>, Vec<u64>, (u64, u64)) {
    let mut s = Session::new();
    let mut levels = vec![100_000, 100_001];
    for z in [6000, 9000, 12000] {
        levels.push(run(&mut s, "level", json!({"elevation": z}))["id"].as_u64().unwrap());
    }
    let walls = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}))["ids"]);
    let door = run(&mut s, "door", json!({"at": [2000, 30]}))["id"].as_u64().unwrap();
    let window = run(&mut s, "window", json!({"at": [8000, 3000]}))["id"].as_u64().unwrap();
    (s, levels, walls, (door, window))
}

fn wall(s: &Session, id: u64) -> crate::model::Wall {
    s.model.wall(id).cloned().unwrap_or_else(|| panic!("no wall {id}"))
}

fn opening(s: &Session, id: u64) -> crate::model::Opening {
    match s.model.elements.get(id) {
        Some(Element::Door(o) | Element::Window(o)) => o.clone(),
        other => panic!("{id} is no door or window: {other:?}"),
    }
}

fn count(s: &Session, category: &str) -> usize {
    s.model.elements.values().filter(|e| e.category() == category).count()
}

#[test]
fn what_is_copied_is_pasted_beside_it_with_its_doors_and_windows_and_new_guids() {
    let (mut s, _, walls, (door, _)) = building();
    let copied = run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    assert_eq!(ids(&copied["ids"]), vec![walls[0], door], "the wall, and the door in it");
    let before = (count(&s, "wall"), count(&s, "door"));
    let pasted = run(&mut s, "paste", json!({"by": [0, -3000]}));
    assert_eq!((count(&s, "wall"), count(&s, "door")), (before.0 + 1, before.1 + 1));
    let made = ids(&pasted["ids"]);
    let new_wall = made.iter().copied().find(|id| s.model.wall(*id).is_some()).unwrap();
    let new_door = made.iter().copied().find(|id| matches!(s.model.elements.get(*id), Some(Element::Door(_)))).unwrap();
    assert_eq!((wall(&s, new_wall).start.0, wall(&s, new_wall).start.1), (0.0, -3000.0));
    assert_eq!(wall(&s, new_wall).end.1, -3000.0);
    // The door is in the new wall, where it was in the old one: not in the old.
    assert_eq!(opening(&s, new_door).host, s.model.guid(new_wall));
    assert_eq!(opening(&s, new_door).offset, opening(&s, door).offset);
    assert_eq!(opening(&s, door).host, s.model.guid(walls[0]), "the original keeps its wall");
    assert_ne!(s.model.guid(new_wall), s.model.guid(walls[0]), "a copy is a new element");
    assert_ne!(s.model.guid(new_door), s.model.guid(door));
    assert_eq!(wall(&s, walls[0]).start.1, 0.0, "the original stays");
}

#[test]
fn the_clipboard_is_kept_and_pastes_again_and_again() {
    let (mut s, _, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[2]]}));
    let before = count(&s, "wall");
    run(&mut s, "paste", json!({"by": [0, 1000]}));
    run(&mut s, "paste", json!({"by": [0, 2000]}));
    assert_eq!(count(&s, "wall"), before + 2);
}

#[test]
fn a_paste_is_one_undo_step() {
    let (mut s, levels, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": walls}));
    let before = s.model.elements.len();
    run(&mut s, "paste", json!({"levels": [levels[1], levels[2]]}));
    assert!(s.model.elements.len() > before);
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.elements.len(), before);
}

#[test]
fn pasting_with_nothing_copied_says_to_copy_first() {
    let (mut s, ..) = building();
    let e = s.execute("paste", &json!({"by": [0, 0]})).unwrap_err();
    assert!(e.contains("copy"), "{e}");
}

#[test]
fn elements_can_be_pasted_by_id_without_the_clipboard() {
    let (mut s, _, walls, _) = building();
    let before = count(&s, "wall");
    run(&mut s, "paste", json!({"ids": [walls[0]], "by": [0, -5000]}));
    assert_eq!(count(&s, "wall"), before + 1);
}

#[test]
fn what_was_copied_and_since_deleted_is_left_out_and_the_rest_pasted() {
    let (mut s, _, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0], walls[2]]}));
    run(&mut s, "delete", json!({"ids": [walls[2]]}));
    let before = count(&s, "wall");
    let r = run(&mut s, "paste", json!({"by": [0, -5000]}));
    assert_eq!(count(&s, "wall"), before + 1);
    assert!(r["left_out"].to_string().contains("no longer"), "{r}");
}

#[test]
fn paste_at_the_same_place_makes_new_elements_on_top_of_the_old() {
    let (mut s, _, walls, (door, _)) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    let r = run(&mut s, "paste", json!({"same_place": true}));
    let made = ids(&r["ids"]);
    assert_eq!(made.len(), 2, "the wall and its door");
    let new_wall = made.iter().copied().find(|id| s.model.wall(*id).is_some()).unwrap();
    assert_eq!((wall(&s, new_wall).start, wall(&s, new_wall).end), (wall(&s, walls[0]).start, wall(&s, walls[0]).end));
    assert!(!made.contains(&door));
    assert_ne!(s.model.guid(new_wall), s.model.guid(walls[0]));
}

#[test]
fn paste_aligned_to_three_levels_puts_the_walls_on_each_with_the_heights_moved_and_the_doors_in_them() {
    let (mut s, levels, walls, (door, window)) = building();
    let mut all: Vec<u64> = walls.clone();
    all.extend([door, window]);
    run(&mut s, "copy_to_clipboard", json!({"ids": all}));
    let targets = [levels[1], levels[2], levels[4]]; // Level 2 (3000), Level 3 (6000), Level 5 (12000)
    let r = run(&mut s, "paste", json!({"levels": targets}));
    let groups = r["pasted"].as_array().unwrap();
    assert_eq!(groups.len(), 3);
    for (group, target) in groups.iter().zip(targets) {
        let made = ids(&group["ids"]);
        let new_walls: Vec<u64> = made.iter().copied().filter(|id| s.model.wall(*id).is_some()).collect();
        assert_eq!(new_walls.len(), 4, "a box of walls on each level");
        let z = s.model.elevation(target);
        for w in &new_walls {
            let w = wall(&s, *w);
            assert_eq!(w.level, s.model.guid(target), "on the level chosen");
            assert_eq!(s.model.elevation(w.level), z);
            assert_eq!(w.base_offset, 0.0);
        }
        // The same place in plan.
        let mut ends: Vec<(f64, f64)> = new_walls.iter().map(|w| (wall(&s, *w).start.0, wall(&s, *w).start.1)).collect();
        ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(ends, vec![(0.0, 0.0), (0.0, 6000.0), (8000.0, 0.0), (8000.0, 6000.0)]);
        // The door and the window went with their walls.
        let new_door = made.iter().copied().find(|id| matches!(s.model.elements.get(*id), Some(Element::Door(_)))).unwrap();
        let new_window = made.iter().copied().find(|id| matches!(s.model.elements.get(*id), Some(Element::Window(_)))).unwrap();
        assert!(new_walls.iter().any(|w| s.model.guid(*w) == opening(&s, new_door).host));
        assert!(new_walls.iter().any(|w| s.model.guid(*w) == opening(&s, new_window).host));
        assert_eq!(opening(&s, new_door).offset, 2000.0);
    }
    // Their tops: the level above where there is one (3000 over), else unconnected and as tall as before.
    let wall_on = |target: u64| -> crate::model::Wall {
        let g = s.model.guid(target);
        s.model.elements.iter().filter_map(|(id, _)| s.model.wall(*id)).find(|w| w.level == g).cloned().unwrap()
    };
    assert_eq!(wall_on(levels[1]).top_level.map(|t| s.model.elevation(t)), Some(6000.0));
    assert_eq!(wall_on(levels[2]).top_level.map(|t| s.model.elevation(t)), Some(9000.0));
    let top = wall_on(levels[4]);
    assert_eq!((top.top_level, top.height), (None, 3000.0), "no level above Level 5: unconnected, 3000 tall");
    assert!(r["notes"].to_string().contains("unconnected"), "{r}");
}

#[test]
fn paste_aligned_to_a_level_keeps_a_walls_base_offset_and_unconnected_height() {
    let (mut s, levels, _, _) = building();
    let w = ids(&run(&mut s, "wall", json!({"points": [[0, 7000], [3000, 7000]], "height": 2400, "base_offset": 150}))["ids"])[0];
    run(&mut s, "copy_to_clipboard", json!({"ids": [w]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[2]]}));
    let made = ids(&r["ids"])[0];
    let new = wall(&s, made);
    assert_eq!(new.level, s.model.guid(levels[2]));
    assert_eq!((new.base_offset, new.height, new.top_level), (150.0, 2400.0, None));
}

#[test]
fn a_wall_attached_to_a_roof_that_was_not_copied_is_pasted_to_another_level_unattached_and_the_result_says_so() {
    let (mut s, levels, walls, _) = building();
    let roof =
        run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "level": levels[1]}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": walls[0], "attach_top": roof}));
    assert!(wall(&s, walls[0]).attach_top.is_some());
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[2]]}));
    let new = ids(&r["ids"]).into_iter().find(|id| s.model.wall(*id).is_some()).unwrap();
    assert_eq!(wall(&s, new).attach_top, None);
    assert!(r["notes"].to_string().contains("attach"), "{r}");
    assert!(wall(&s, walls[0]).attach_top.is_some(), "the original stays attached");
}

#[test]
fn a_wall_attached_to_a_roof_copied_with_it_is_attached_to_the_roofs_copy_on_every_level() {
    let (mut s, levels, walls, _) = building();
    let roof =
        run(&mut s, "roof", json!({"boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "level": levels[1]}))["id"].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": walls[0], "attach_top": roof}));
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0], roof]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[2]]}));
    let made = ids(&r["ids"]);
    let new_wall = made.iter().copied().find(|id| s.model.wall(*id).is_some()).unwrap();
    let new_roof = made.iter().copied().find(|id| matches!(s.model.elements.get(*id), Some(Element::Roof(_)))).unwrap();
    assert_eq!(wall(&s, new_wall).attach_top, Some(s.model.guid(new_roof)));
    match s.model.elements.get(new_roof) {
        // The roof stood a level above the wall; it stands as far above the wall's new level.
        Some(Element::Roof(r)) => assert_eq!(r.level, s.model.guid(levels[3])),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_door_copied_alone_goes_to_another_level_only_with_its_wall_and_the_result_says_so() {
    let (mut s, levels, _, (door, _)) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [door]}));
    let before = count(&s, "door");
    let e = s.execute("paste", &json!({"levels": [levels[1]]})).unwrap_err();
    assert_eq!(count(&s, "door"), before, "nothing to host it on the new level");
    assert!(e.contains("wall"), "{e}");
}

#[test]
fn a_room_pasted_to_a_level_stands_on_it_and_one_with_no_walls_there_says_it_is_not_enclosed() {
    let (mut s, levels, walls, _) = building();
    let room = run(&mut s, "room", json!({"point": [4000, 3000], "name": "Hall"}))["id"].as_u64().unwrap();
    // With its walls, the room is bounded on the new level too.
    let mut with = walls.clone();
    with.push(room);
    run(&mut s, "copy_to_clipboard", json!({"ids": with}));
    let r = run(&mut s, "paste", json!({"levels": [levels[1]]}));
    let new_room = ids(&r["ids"]).into_iter().find(|id| matches!(s.model.elements.get(*id), Some(Element::Room(_)))).unwrap();
    match s.model.elements.get(new_room) {
        Some(Element::Room(rm)) => assert_eq!((rm.level, rm.name.as_str()), (s.model.guid(levels[1]), "Hall")),
        other => panic!("{other:?}"),
    }
    assert!(!r["notes"].to_string().contains("enclosed"), "{r}");
    // Alone, the room finds no walls up there.
    run(&mut s, "copy_to_clipboard", json!({"ids": [room]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[3]]}));
    assert!(r["notes"].to_string().contains("enclosed"), "{r}");
}

#[test]
fn annotations_are_left_out_of_a_paste_to_levels() {
    let (mut s, levels, walls, _) = building();
    let note = run(&mut s, "text", json!({"text": "Hi", "at": [100, 100]}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0], note]}));
    let before = count(&s, "text");
    let r = run(&mut s, "paste", json!({"levels": [levels[1]]}));
    assert_eq!(count(&s, "text"), before);
    assert!(r["left_out"].to_string().contains("view"), "{r}");
}

#[test]
fn a_current_view_paste_is_a_paste_to_the_views_level() {
    let (mut s, levels, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    let r = run(&mut s, "paste", json!({"view": levels[2]}));
    let new = ids(&r["ids"]).into_iter().find(|id| s.model.wall(*id).is_some()).unwrap();
    assert_eq!(wall(&s, new).level, s.model.guid(levels[2]));
}

#[test]
fn walls_pasted_on_two_levels_come_with_their_own_numbers_for_doors_and_rooms() {
    let (mut s, levels, walls, (door, _)) = building();
    let mut all = walls.clone();
    all.push(door);
    run(&mut s, "copy_to_clipboard", json!({"ids": all}));
    let r = run(&mut s, "paste", json!({"levels": [levels[1], levels[2]]}));
    let marks: Vec<String> = ids(&r["ids"])
        .into_iter()
        .filter_map(|id| match s.model.elements.get(id) {
            Some(Element::Door(o)) => Some(o.mark.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(marks.len(), 2);
    assert_ne!(marks[0], marks[1], "each door has a mark of its own");
    assert_ne!(marks[0], opening(&s, door).mark);
}

#[test]
fn the_clipboard_says_where_its_middle_is_for_placing_it() {
    let (mut s, _, walls, _) = building();
    let r = run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0], walls[1]]}));
    // The walls run (0,0)-(8000,0) and (8000,0)-(8000,6000).
    assert_eq!(r["anchor"], json!([4000.0, 3000.0]));
    assert_eq!(s.clipboard_ids().len(), 4, "the two walls, and the door and the window in them");
}

#[test]
fn a_paste_takes_one_way_of_placing_only() {
    let (mut s, levels, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    let e = s.execute("paste", &json!({"same_place": true, "levels": [levels[1]]})).unwrap_err();
    assert!(e.contains("one"), "{e}");
    let e = s.execute("paste", &json!({"by": [0, 0], "same_place": true})).unwrap_err();
    assert!(e.contains("one"), "{e}");
    let e = s.execute("paste", &json!({"by": [1e300, 0]})).unwrap_err();
    assert!(!e.is_empty());
    let e = s.execute("paste", &json!({"levels": []})).unwrap_err();
    assert!(e.contains("level"), "{e}");
}

#[test]
fn a_new_project_empties_the_clipboard() {
    let (mut s, _, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    run(&mut s, "new", json!({}));
    assert!(s.clipboard_ids().is_empty());
}

#[test]
fn copies_hold_guids_of_their_own_not_the_originals() {
    let (mut s, levels, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": walls}));
    run(&mut s, "paste", json!({"levels": [levels[1]]}));
    let guids: Vec<Guid> = s.model.elements.iter().map(|(id, _)| s.model.guid(*id)).collect();
    let mut sorted = guids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), guids.len());
}

#[test]
fn a_paste_to_a_level_can_also_move_in_plan_as_a_click_with_the_cursor_elsewhere_does() {
    let (mut s, levels, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[1]], "by": [0, 10000]}));
    let new = ids(&r["ids"]).into_iter().find(|id| s.model.wall(*id).is_some()).unwrap();
    assert_eq!(wall(&s, new).level, s.model.guid(levels[1]));
    assert_eq!((wall(&s, new).start.0, wall(&s, new).start.1), (0.0, 10000.0));
}

#[test]
fn the_clipboard_knows_its_box_and_the_level_it_came_from() {
    let (mut s, levels, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": walls}));
    assert_eq!(s.clipboard_extent(), Some([crate::geom::P2(0.0, 0.0), crate::geom::P2(8000.0, 6000.0)]));
    assert_eq!(s.clipboard_level(), Some(s.model.guid(levels[0])));
}

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

/// Level 1 (0), Level 2 (3000) and a Level 3 at 6000, and nothing above.
fn three_levels() -> (Session, Vec<u64>) {
    let mut s = Session::new();
    let l3 = run(&mut s, "level", json!({"elevation": 6000}))["id"].as_u64().unwrap();
    (s, vec![100_000, 100_001, l3])
}

fn made_of(s: &Session, r: &Value, category: &str) -> Vec<u64> {
    ids(&r["ids"]).into_iter().filter(|id| s.model.elements.get(*id).is_some_and(|e| e.category() == category)).collect()
}

fn room_span(s: &Session, id: u64) -> (f64, f64) {
    match s.model.elements.get(id) {
        Some(Element::Room(r)) => s.model.room_span(r),
        other => panic!("{id}: {other:?}"),
    }
}

#[test]
fn a_paste_says_where_it_goes_and_does_not_paste_in_place_when_it_is_not_told() {
    let (mut s, _, walls, _) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0]]}));
    let before = s.model.elements.len();
    let e = s.execute("paste", &json!({})).unwrap_err();
    assert!(e.contains("same_place") && e.contains("by"), "{e}");
    assert_eq!(s.model.elements.len(), before);
}

#[test]
fn a_room_pasted_where_there_is_no_level_for_its_top_keeps_its_top_above_its_bottom() {
    // Rooms on Levels 1 and 3, each with no upper limit; the paste to Level 2 moves the Level 3 one up to 9000, where there is no level.
    let (mut s, l) = three_levels();
    let a = run(&mut s, "room", json!({"point": [1000, 1000], "level": l[0], "limit_offset": 2400}))["id"].as_u64().unwrap();
    let b = run(&mut s, "room", json!({"point": [5000, 1000], "level": l[2], "limit_offset": 2400}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [a, b]}));
    let r = run(&mut s, "paste", json!({"levels": [l[1]]}));
    let rooms = made_of(&s, &r, "room");
    assert_eq!(rooms.len(), 2);
    let spans: Vec<(f64, f64)> = rooms.iter().map(|id| room_span(&s, *id)).collect();
    assert!(spans.contains(&(3000.0, 5400.0)), "the Level 1 room, on Level 2: {spans:?}");
    assert!(spans.contains(&(9000.0, 11400.0)), "the Level 3 room, 3000 up: {spans:?}");
    assert!(r["notes"].to_string().contains("no level at 9000"), "{r}");
}

#[test]
fn a_rooms_upper_limit_with_no_level_where_it_goes_becomes_its_limit_offset_and_the_room_still_lifts() {
    let mut s = Session::new();
    let l3 = run(&mut s, "level", json!({"elevation": 6000}))["id"].as_u64().unwrap();
    let l4 = run(&mut s, "level", json!({"elevation": 12000}))["id"].as_u64().unwrap();
    let a = run(&mut s, "room", json!({"point": [1000, 1000], "level": 100_000}))["id"].as_u64().unwrap();
    let b = run(&mut s, "room", json!({"point": [5000, 1000], "level": l3, "top_level": l4, "limit_offset": -300}))["id"].as_u64().unwrap();
    assert_eq!(room_span(&s, b), (6000.0, 11700.0));
    run(&mut s, "copy_to_clipboard", json!({"ids": [a, b]}));
    let r = run(&mut s, "paste", json!({"levels": [100_001]}));
    let copy = made_of(&s, &r, "room").into_iter().find(|id| room_span(&s, *id).0 == 9000.0).expect("the Level 3 room, 3000 up");
    assert_eq!(room_span(&s, copy), (9000.0, 14700.0), "its top moved with its bottom");
}

#[test]
fn a_roof_joined_to_a_roof_copied_with_it_is_joined_to_its_copy_and_the_original_stays_joined_to_its_own() {
    let (mut s, l) = three_levels();
    let boundary = |x: i64| json!([[x, 0], [x + 4000, 0], [x + 4000, 4000], [x, 4000]]);
    let a = run(&mut s, "roof", json!({"boundary": boundary(0), "level": l[1]}))["id"].as_u64().unwrap();
    let b = run(&mut s, "roof", json!({"boundary": boundary(4000), "level": l[1], "join": a}))["id"].as_u64().unwrap();
    let join_of = |s: &Session, id: u64| match s.model.elements.get(id) {
        Some(Element::Roof(r)) => r.join,
        other => panic!("{other:?}"),
    };
    assert_eq!(join_of(&s, b), Some(s.model.guid(a)));
    run(&mut s, "copy_to_clipboard", json!({"ids": [a, b]}));
    // Beside.
    let r = run(&mut s, "paste", json!({"by": [0, 10000]}));
    let roofs = made_of(&s, &r, "roof");
    let copy_a = roofs.iter().copied().find(|id| join_of(&s, *id).is_none()).unwrap();
    let copy_b = roofs.iter().copied().find(|id| join_of(&s, *id).is_some()).unwrap();
    assert_eq!(join_of(&s, copy_b), Some(s.model.guid(copy_a)), "the copy is joined to the copy");
    assert_eq!(join_of(&s, b), Some(s.model.guid(a)), "the original keeps its own");
    // To a level.
    let r = run(&mut s, "paste", json!({"levels": [l[2]]}));
    let roofs = made_of(&s, &r, "roof");
    let copy_a = roofs.iter().copied().find(|id| join_of(&s, *id).is_none()).unwrap();
    let copy_b = roofs.iter().copied().find(|id| join_of(&s, *id).is_some()).unwrap();
    assert_eq!(join_of(&s, copy_b), Some(s.model.guid(copy_a)));
    assert!(!r["notes"].to_string().contains("joined"), "{r}");
}

#[test]
fn what_has_no_level_at_the_new_height_and_no_offset_to_be_raised_by_is_left_out_and_not_set_too_low() {
    let (mut s, l) = three_levels();
    let wall = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": l[0]}))["ids"])[0];
    let stair = run(&mut s, "stair", json!({"start": [1000, 1000], "end": [1000, 2000], "level": l[2], "height": 3000, "railings": true}))
        ["id"]
        .as_u64()
        .unwrap();
    let ramp =
        run(&mut s, "ramp", json!({"start": [5000, 1000], "end": [5000, 2000], "level": l[2], "height": 600}))["id"].as_u64().unwrap();
    run(&mut s, "room_separator", json!({"points": [[0, 3000], [3000, 3000]], "level": l[2]}));
    let sep = s.model.elements.iter().find(|(_, e)| e.category() == "roomseparator").map(|(id, _)| *id).unwrap();
    assert_eq!(count(&s, "railing"), 2);
    run(&mut s, "copy_to_clipboard", json!({"ids": [wall, stair, ramp, sep]}));
    let (stairs, ramps, seps, rails) = (count(&s, "stair"), count(&s, "ramp"), count(&s, "roomseparator"), count(&s, "railing"));
    let r = run(&mut s, "paste", json!({"levels": [l[1]]}));
    // The wall comes to Level 2; the others would stand at 9000, where there is no level.
    assert_eq!(made_of(&s, &r, "wall").len(), 1);
    assert_eq!((count(&s, "stair"), count(&s, "ramp"), count(&s, "roomseparator"), count(&s, "railing")), (stairs, ramps, seps, rails));
    let said = r["left_out"].to_string();
    for what in [format!("stair {stair}"), format!("ramp {ramp}"), format!("roomseparator {sep}")] {
        assert!(said.contains(&what) && said.contains("no level at 9000"), "{what}: {said}");
    }
    assert!(said.contains("railing"), "the stair's railings go with it: {said}");
    // Pasted down to Level 1 they land on a level that is there.
    let r = run(&mut s, "paste", json!({"levels": [l[0]]}));
    assert_eq!(made_of(&s, &r, "stair").len(), 1, "{r}");
}

#[test]
fn what_has_an_offset_is_raised_by_it_where_there_is_no_level_at_the_new_height() {
    let (mut s, l) = three_levels();
    let wall = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": l[0]}))["ids"])[0];
    let pad =
        run(&mut s, "building_pad", json!({"boundary": [[0, 5000], [3000, 5000], [3000, 8000], [0, 8000]], "level": l[2], "offset": 200}))
            ["id"]
            .as_u64()
            .unwrap();
    let beam = ids(&run(&mut s, "beam", json!({"points": [[0, 10000], [4000, 10000]], "level": l[2]}))["ids"])[0];
    let bar = run(&mut s, "rebar", json!({"points": [[50, 10000, -40], [3950, 10000, -40]], "diameter": 16, "host": beam}));
    let bar = bar["id"].as_u64().or_else(|| bar["ids"][0].as_u64()).unwrap();
    let scan = std::env::temp_dir().join(format!("bimcraft-clipboard-{}.xyz", std::process::id()));
    std::fs::write(&scan, "0 0 0\n1 0 0\n").unwrap();
    let cloud = run(&mut s, "point_cloud", json!({"path": scan.to_str().unwrap(), "name": "dot", "level": l[2], "offset": 100}));
    let _ = std::fs::remove_file(&scan);
    let cloud = cloud["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [wall, pad, beam, bar, cloud]}));
    let r = run(&mut s, "paste", json!({"levels": [l[1]]}));
    assert!(r["left_out"].as_array().unwrap().is_empty(), "{r}");
    let pad = made_of(&s, &r, "buildingpad")[0];
    match s.model.elements.get(pad) {
        Some(Element::BuildingPad(p)) => assert_eq!((p.level, p.offset), (s.model.guid(l[2]), 3200.0), "3000 up on its own level"),
        other => panic!("{other:?}"),
    }
    let bar = made_of(&s, &r, "rebar")[0];
    match s.model.elements.get(bar) {
        Some(Element::Rebar(b)) => assert_eq!(b.path.iter().map(|q| q[2]).collect::<Vec<_>>(), vec![2960.0, 2960.0]),
        other => panic!("{other:?}"),
    }
    let cloud = made_of(&s, &r, "pointcloud")[0];
    match s.model.elements.get(cloud) {
        Some(Element::PointCloud(c)) => assert_eq!((c.level, c.offset), (s.model.guid(l[2]), 3100.0)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_paste_of_what_stood_on_more_than_one_level_goes_from_the_lowest_and_keeps_the_gap() {
    let (mut s, l) = three_levels();
    let low = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "level": l[0], "height": 2800}))["ids"])[0];
    let high = ids(&run(&mut s, "wall", json!({"points": [[0, 5000], [4000, 5000]], "level": l[2], "height": 2800}))["ids"])[0];
    run(&mut s, "copy_to_clipboard", json!({"ids": [low, high]}));
    let r = run(&mut s, "paste", json!({"levels": [l[1]]}));
    let made = made_of(&s, &r, "wall");
    assert_eq!(made.len(), 2);
    let new_low = made.iter().copied().find(|id| wall(&s, *id).start.1 == 0.0).unwrap();
    let new_high = made.iter().copied().find(|id| wall(&s, *id).start.1 == 5000.0).unwrap();
    assert_eq!((wall(&s, new_low).level, wall(&s, new_low).base_offset), (s.model.guid(l[1]), 0.0));
    // Level 3 plus 3000 is 9000: no level there, so it stays on Level 3, 3000 up: the two keep their 6000 apart.
    assert_eq!((wall(&s, new_high).level, wall(&s, new_high).base_offset), (s.model.guid(l[2]), 3000.0));
    assert_eq!(s.model.base_z(&wall(&s, new_high)) - s.model.base_z(&wall(&s, new_low)), 6000.0);
}

#[test]
fn a_wall_whose_top_level_is_let_go_keeps_its_offsets_in_its_height() {
    let (mut s, levels, _, _) = building();
    let w = ids(&run(
        &mut s,
        "wall",
        json!({"points": [[0, 7000], [3000, 7000]], "level": levels[0], "top_level": levels[1], "base_offset": 50, "top_offset": -100}),
    )["ids"])[0];
    run(&mut s, "copy_to_clipboard", json!({"ids": [w]}));
    // Level 5 (12000) has nothing above it.
    let r = run(&mut s, "paste", json!({"levels": [levels[4]]}));
    let new = wall(&s, made_of(&s, &r, "wall")[0]);
    assert_eq!((new.level, new.top_level, new.base_offset, new.top_offset), (s.model.guid(levels[4]), None, 50.0, 0.0));
    assert_eq!(new.height, 3000.0 - 100.0 - 50.0, "from its base to where its top was");
    assert!(r["notes"].to_string().contains("unconnected"), "{r}");
}

#[test]
fn columns_stairs_and_ramps_to_a_level_with_nothing_above_it_are_as_tall_as_they_were() {
    let (mut s, levels, _, _) = building();
    let column = run(&mut s, "column", json!({"at": [1000, 7000], "level": levels[0], "base_offset": 100}))["id"].as_u64().unwrap();
    let stair = run(&mut s, "stair", json!({"start": [3000, 7000], "end": [3000, 8000], "level": levels[0]}))["id"].as_u64().unwrap();
    let ramp = run(&mut s, "ramp", json!({"start": [6000, 7000], "end": [6000, 8000], "level": levels[0]}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [column, stair, ramp]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[4]]}));
    let top = s.model.guid(levels[4]);
    match s.model.elements.get(made_of(&s, &r, "column")[0]) {
        Some(Element::Column(c)) => assert_eq!((c.level, c.top_level, c.height, c.base_offset), (top, None, 2900.0, 100.0)),
        other => panic!("{other:?}"),
    }
    match s.model.elements.get(made_of(&s, &r, "stair")[0]) {
        Some(Element::Stair(st)) => assert_eq!((st.level, st.top_level, st.height), (top, None, 3000.0)),
        other => panic!("{other:?}"),
    }
    match s.model.elements.get(made_of(&s, &r, "ramp")[0]) {
        Some(Element::Ramp(rp)) => assert_eq!((rp.level, rp.top_level, rp.height), (top, None, 3000.0)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn floors_and_roofs_keep_their_offsets_to_another_level_and_gain_the_gap_where_there_is_no_level() {
    let (mut s, l) = three_levels();
    let square = json!([[0, 0], [4000, 0], [4000, 4000], [0, 4000]]);
    let wall = ids(&run(&mut s, "wall", json!({"points": [[0, 20000], [1000, 20000]], "level": l[0]}))["ids"])[0];
    let floor = run(&mut s, "floor", json!({"boundary": square, "level": l[0], "offset": -50}))["id"].as_u64().unwrap();
    let roof = run(&mut s, "roof", json!({"boundary": square, "level": l[0], "offset": 120}))["id"].as_u64().unwrap();
    let floor3 = run(&mut s, "floor", json!({"boundary": square, "level": l[2], "offset": -50}))["id"].as_u64().unwrap();
    let roof3 = run(&mut s, "roof", json!({"boundary": square, "level": l[2], "offset": 120}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [wall, floor, roof, floor3, roof3]}));
    let r = run(&mut s, "paste", json!({"levels": [l[1]]}));
    let (floors, roofs) = (made_of(&s, &r, "floor"), made_of(&s, &r, "roof"));
    let floor_at = |level: u64| {
        floors.iter().find_map(|id| match s.model.elements.get(*id) {
            Some(Element::Floor(f)) if f.level == s.model.guid(level) => Some(f.offset),
            _ => None,
        })
    };
    let roof_at = |level: u64| {
        roofs.iter().find_map(|id| match s.model.elements.get(*id) {
            Some(Element::Roof(f)) if f.level == s.model.guid(level) => Some(f.offset),
            _ => None,
        })
    };
    assert_eq!((floor_at(l[1]), roof_at(l[1])), (Some(-50.0), Some(120.0)), "Level 1's, on Level 2, as they were");
    assert_eq!((floor_at(l[2]), roof_at(l[2])), (Some(2950.0), Some(3120.0)), "Level 3's, 3000 up: no level at 9000");
}

#[test]
fn a_door_copied_alone_is_put_in_the_wall_it_lands_on_beside_it() {
    let (mut s, _, walls, (door, window)) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [door, window]}));
    // The door stood at (2000, 0) in the south wall, the window at (8000, 3000) in the east one. Pasted 4000 east, the door lands at
    // (6000, 0), still in the south wall; the window at (12000, 3000), in no wall.
    let r = run(&mut s, "paste", json!({"by": [4000, 0]}));
    let copy = made_of(&s, &r, "door");
    assert_eq!(copy.len(), 1, "{r}");
    assert_eq!((opening(&s, copy[0]).host, opening(&s, copy[0]).offset), (s.model.guid(walls[0]), 6000.0));
    assert_ne!(opening(&s, copy[0]).mark, opening(&s, door).mark);
    assert!(made_of(&s, &r, "window").is_empty());
    let said = r["left_out"].to_string();
    assert!(said.contains(&format!("window {window}")) && said.contains("no wall"), "{r}");
    // By (0, 6000) the door lands at (2000, 6000): in the wall on the far side.
    let r = run(&mut s, "paste", json!({"ids": [door], "by": [0, 6000]}));
    let copy = made_of(&s, &r, "door")[0];
    assert_eq!((opening(&s, copy).host, opening(&s, copy).offset), (s.model.guid(walls[2]), 6000.0));
    assert_eq!(opening(&s, door).host, s.model.guid(walls[0]), "the original stays");
}

#[test]
fn a_door_copied_alone_and_pasted_to_a_level_with_a_wall_under_it_goes_in_that_wall() {
    let (mut s, levels, walls, (door, _)) = building();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0], door]}));
    let up = run(&mut s, "paste", json!({"levels": [levels[1]]}));
    let new_wall = made_of(&s, &up, "wall").into_iter().find(|w| wall(&s, *w).start.1 == 0.0 && wall(&s, *w).end.0 == 8000.0).unwrap();
    // The door alone, to Level 2: the wall that is there takes it.
    run(&mut s, "copy_to_clipboard", json!({"ids": [door]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[1]]}));
    let copy = made_of(&s, &r, "door");
    assert_eq!(copy.len(), 1, "{r}");
    assert_eq!(opening(&s, copy[0]).host, s.model.guid(new_wall));
}

#[test]
fn a_mixed_paste_to_another_level_puts_the_grid_down_beside_in_the_same_step() {
    let (mut s, levels, walls, _) = building();
    let grid = run(&mut s, "grid", json!({"start": [-2000, -1000], "end": [-2000, 7000]}))["id"].as_u64().unwrap();
    run(&mut s, "copy_to_clipboard", json!({"ids": [walls[0], grid]}));
    let (grids, steps) = (count(&s, "grid"), s.history().len());
    let r = run(&mut s, "paste", json!({"levels": [levels[1]], "by": [0, 10000]}));
    // The wall comes to Level 2; the grid, which stands on no level, is put 10000 north beside its original, once.
    assert_eq!(made_of(&s, &r, "wall").len(), 1, "{r}");
    assert_eq!(count(&s, "grid"), grids + 1, "the grid was dropped: {r}");
    let copy = made_of(&s, &r, "grid")[0];
    let Some(Element::Grid(g)) = s.model.elements.get(copy) else { panic!("a grid") };
    assert_eq!((g.start.0, g.start.1, g.end.1), (-2000.0, 9000.0, 17000.0));
    assert!(!r["left_out"].to_string().contains("no level"), "{r}");
    assert_eq!(s.history().len(), steps + 1, "one undo step");
    // Two levels: still one grid.
    let r = run(&mut s, "paste", json!({"levels": [levels[1], levels[2]], "by": [0, 20000]}));
    assert_eq!(made_of(&s, &r, "grid").len(), 1, "{r}");
    // A grid alone goes too; with no `by` it would lie on its original, so it is left out and the reply says so.
    run(&mut s, "copy_to_clipboard", json!({"ids": [grid]}));
    let r = run(&mut s, "paste", json!({"levels": [levels[1]], "by": [5000, 0]}));
    assert_eq!(made_of(&s, &r, "grid").len(), 1, "{r}");
    let e = s.execute("paste", &json!({"levels": [levels[1]]})).unwrap_err();
    assert!(e.contains("stands on a level"), "{e}");
}

/// A window and a door in a wall at y = 10000 on Level 1, and a tall wall from Level 1 up past Level 2 along y = 0: the ids of
/// the window and the door, and of the tall wall.
fn tall_wall_below() -> (Session, [u64; 2], u64) {
    let mut s = Session::new();
    let far = ids(&run(&mut s, "wall", json!({"points": [[0, 10000], [8000, 10000]]}))["ids"])[0];
    let window = run(&mut s, "window", json!({"wall": far, "offset": 4000, "sill": 900}))["id"].as_u64().unwrap();
    let door = run(&mut s, "door", json!({"wall": far, "offset": 6500}))["id"].as_u64().unwrap();
    let tall = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "height": 8000}))["ids"])[0];
    (s, [window, door], tall)
}

#[test]
fn an_opening_pasted_to_a_level_goes_in_the_wall_based_on_that_level_not_a_lower_one_running_up_through_it() {
    let (mut s, [window, door], tall) = tall_wall_below();
    let upper = ids(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "level": "Level 2"}))["ids"])[0];
    run(&mut s, "copy_to_clipboard", json!({"ids": [window, door]}));
    let r = run(&mut s, "paste", json!({"levels": ["Level 2"], "by": [0, -10000]}));
    for category in ["window", "door"] {
        let copy = made_of(&s, &r, category);
        assert_eq!(copy.len(), 1, "{category}: {r}");
        let o = opening(&s, copy[0]);
        assert_eq!(o.host, s.model.guid(upper), "{category} went in the wall of Level 2, not in wall {tall}: {r}");
    }
}

#[test]
fn a_window_pasted_into_a_lower_walls_run_keeps_its_height_above_the_level_it_was_pasted_to() {
    let (mut s, [window, door], tall) = tall_wall_below();
    run(&mut s, "copy_to_clipboard", json!({"ids": [window, door]}));
    // Only the tall wall from Level 1 is there at Level 2's plan: the window's sill is measured from its base, so it is
    // Level 2's 3000 and its 900 higher, and the door stands on Level 2's floor, which is 3000 up this wall.
    let r = run(&mut s, "paste", json!({"levels": ["Level 2"], "by": [0, -10000]}));
    let (w, d) = (opening(&s, made_of(&s, &r, "window")[0]), opening(&s, made_of(&s, &r, "door")[0]));
    assert_eq!((w.host, w.sill), (s.model.guid(tall), 3900.0), "{r}");
    assert_eq!((d.host, d.sill), (s.model.guid(tall), 3000.0), "{r}");
    // Beside, on the same level, nothing changes.
    let r = run(&mut s, "paste", json!({"ids": [window], "by": [0, -10000]}));
    assert_eq!(opening(&s, made_of(&s, &r, "window")[0]).sill, 900.0);
}

#[test]
fn an_opening_pasted_on_a_curved_wall_goes_in_it_at_the_arc_length() {
    let (mut s, [window, _], _) = tall_wall_below();
    let curved = ids(&run(&mut s, "wall", json!({"points": [[0, -20000], [8000, -20000]], "arc": 90}))["ids"])[0];
    let (axis, length) = {
        let w = wall(&s, curved);
        (crate::walls::Axis::of(&w), crate::walls::Axis::of(&w).length())
    };
    let mid = axis.at(length / 2.0, 0.0);
    run(&mut s, "copy_to_clipboard", json!({"ids": [window]}));
    let r = run(&mut s, "paste", json!({"by": [mid.0 - 4000.0, mid.1 - 10000.0]}));
    let o = opening(&s, made_of(&s, &r, "window")[0]);
    assert_eq!(o.host, s.model.guid(curved), "{r}");
    assert!((o.offset - length / 2.0).abs() < 1.0, "{} of {length}", o.offset);
}

#[test]
fn an_opening_pasted_at_a_wall_join_goes_in_the_nearer_wall_and_stays_within_its_run() {
    let (mut s, [window, _], _) = tall_wall_below();
    let east = ids(&run(&mut s, "wall", json!({"points": [[20000, 0], [24000, 0]]}))["ids"])[0];
    let north = ids(&run(&mut s, "wall", json!({"points": [[24000, 0], [24000, 4000]]}))["ids"])[0];
    run(&mut s, "copy_to_clipboard", json!({"ids": [window]}));
    // 150 off the south-east corner: 150 from the east wall's axis? No: (23900, 150) is 150 from it and 100 from the north wall's.
    let r = run(&mut s, "paste", json!({"by": [23900.0 - 4000.0, 150.0 - 10000.0]}));
    let o = opening(&s, made_of(&s, &r, "window")[0]);
    assert_eq!(o.host, s.model.guid(north), "the nearer axis: {r}");
    let width = s.model.opening_type(s.model.elements.get(made_of(&s, &r, "window")[0]).unwrap()).unwrap().width;
    assert!(o.offset >= width / 2.0 - 1e-6 && o.offset <= 4000.0 - width / 2.0 + 1e-6, "kept within the wall's run: {}", o.offset);
    let _ = east;
}

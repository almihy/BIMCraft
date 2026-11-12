//! `insert`'s tests that drive the Session (moved out of `insert.rs`).

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Element, FIRST_ID, Guid};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box of walls with a door, saved to a file of its own: the session and the path.
fn saved(name: &str) -> (Session, String) {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    run(&mut s, "door", json!({"at": [2000, 0]}));
    run(&mut s, "grid", json!({"start": [-1000, 3000], "end": [9000, 3000]}));
    let path = std::env::temp_dir().join(format!("bimcraft-insert-{name}-{}.bimcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": path}));
    (s, path)
}

fn guids(s: &Session) -> Vec<Guid> {
    s.model.elements.with_guids().map(|(_, g, _)| g).collect()
}

#[test]
fn a_file_inserted_into_itself_comes_in_as_copies_with_new_ids_and_new_guids_pointing_at_each_other() {
    let (mut s, path) = saved("self");
    let before = guids(&s);
    let n = s.model.elements.len();
    let r = run(&mut s, "insert", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(s.model.elements.len(), 2 * n, "all of it, again");
    let ids = r["ids"].as_object().unwrap();
    assert_eq!(ids.len(), n);
    // Every id was taken, so each came in under a new one past the last; every GUID was here, so each is a copy.
    for (there, here) in ids {
        assert!(here.as_u64().unwrap() > there.parse::<u64>().unwrap() && here.as_u64().unwrap() >= FIRST_ID + n as u64);
    }
    assert_eq!(r["copies"].as_array().unwrap().len(), n);
    let after = guids(&s);
    assert!(before.iter().all(|g| after.contains(g)), "the originals keep theirs");
    // The copied door is in the copied wall, which stands on the copied level, renamed apart.
    let door = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).max().unwrap();
    let Some(Element::Door(d)) = s.model.elements.get(door) else { panic!() };
    let wall = s.model.wall(d.host).unwrap();
    assert!(!before.contains(&d.host) && !before.contains(&wall.level));
    assert_eq!(s.model.level(wall.level).unwrap().name, "Level 1 2");
    assert_eq!(s.model.check(), Ok(()));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.elements.len(), n, "one undo step");
}

#[test]
fn elements_of_another_project_keep_their_guids_take_new_ids_where_taken_and_stand_on_levels_of_the_same_name() {
    let (other, path) = saved("other");
    let mut s = Session::new();
    let level_here = s.model.guid(FIRST_ID);
    // Both projects number from 100000: this one's ids are taken.
    for x in 0..6 {
        run(&mut s, "grid", json!({"start": [x * 1000, -9000], "end": [x * 1000, -8000]}));
    }
    let door = other.model.elements.iter().find(|(_, e)| matches!(e, Element::Door(_))).map(|(id, _)| *id).unwrap();
    let r = run(&mut s, "insert", json!({"path": path, "ids": [door]}));
    // The door brings its wall, which stands on Level 1: this project's.
    assert_eq!(r["ids"].as_object().unwrap().len(), 2, "{r}");
    assert_eq!(r["copies"], json!([]));
    let here = r["ids"][door.to_string()].as_u64().unwrap();
    assert_ne!(here, door, "its id was taken");
    assert_eq!(s.model.guid(here), other.model.guid(door), "a GUID is the element's for good");
    let Some(Element::Door(d)) = s.model.elements.get(here).cloned() else { panic!() };
    assert_eq!(s.model.wall(d.host).unwrap().level, level_here);
    assert_eq!(s.model.levels().len(), 2, "no level came in");
    assert!(s.model.check().is_ok());
    // Inserted again, the grid is here already: a copy, with a GUID of its own, named apart (as the first was, from
    // this project's grids).
    let grid = other.model.elements.iter().find(|(_, e)| matches!(e, Element::Grid(_))).map(|(id, _)| *id).unwrap();
    let name_there = match other.model.elements.get(grid) {
        Some(Element::Grid(g)) => g.name.clone(),
        _ => panic!(),
    };
    let first = run(&mut s, "insert", json!({"path": path, "ids": [grid]}))["ids"][grid.to_string()].as_u64().unwrap();
    let again = run(&mut s, "insert", json!({"path": path, "ids": [grid]}));
    let _ = std::fs::remove_file(&path);
    let copy = again["ids"][grid.to_string()].as_u64().unwrap();
    assert_eq!(again["copies"], json!([copy]));
    assert_eq!(s.model.guid(first), other.model.guid(grid));
    assert_ne!(s.model.guid(copy), s.model.guid(first));
    let name = |id| match s.model.elements.get(id) {
        Some(Element::Grid(g)) => g.name.clone(),
        _ => panic!(),
    };
    assert!(name(first).starts_with(&name_there) && name(copy).starts_with(&name_there) && name(copy) != name(first));
}

#[test]
fn what_isnt_there_or_isnt_a_project_is_refused() {
    let mut s = Session::new();
    assert!(s.execute("insert", &json!({})).unwrap_err().contains("`path`"));
    assert!(s.execute("insert", &json!({"path": "/nowhere/x.bimcraft"})).is_err());
    let (_, path) = saved("missing");
    let e = s.execute("insert", &json!({"path": path, "ids": [5]})).unwrap_err();
    let _ = std::fs::remove_file(&path);
    assert!(e.contains("no element 5"), "{e}");
}

#[test]
fn a_camera_view_comes_in_with_its_camera_and_its_own_name() {
    let mut other = Session::new();
    run(&mut other, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let made = run(&mut other, "camera", json!({"eye": [-5000.0, -4000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0], "lens": 24}));
    let camera_there = made["id"].as_u64().unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-insert-camera-{}.bimcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut other, "save", json!({"path": path}));
    // This project already has a Camera 1, so the inserted one is named apart.
    let mut s = Session::new();
    run(&mut s, "camera", json!({"eye": [0.0, -9000.0, 1700.0], "target": [4000.0, 3000.0, 1000.0]}));
    let r = run(&mut s, "insert", json!({"path": path, "ids": [camera_there]}));
    let _ = std::fs::remove_file(&path);
    let here = r["ids"][camera_there.to_string()].as_u64().unwrap();
    let Some(Element::View3D(v)) = s.model.elements.get(here) else { panic!("the camera view came in") };
    assert_eq!(v.camera, Some(crate::model::ViewCamera::new([-5000.0, -4000.0, 1600.0], [4000.0, 3000.0, 1200.0])));
    assert_eq!(v.perspective.filter(|p| p.on).map(|p| p.lens), Some(24.0));
    assert!(v.name.starts_with("Camera 1") && v.name != "Camera 1", "named apart from this project's: {}", v.name);
    assert_eq!(s.model.check(), Ok(()));
}

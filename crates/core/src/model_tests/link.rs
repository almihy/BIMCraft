//! `link`'s tests that drive the Session (moved out of `link.rs`).

use crate::geom::P2;
use crate::ids::Id;
use crate::model::{Element, Model};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::link::*;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box of walls with a door, saved to a file: the path.
fn architect(name: &str) -> String {
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    run(&mut s, "door", json!({"at": [2000, 0]}));
    let path = std::env::temp_dir().join(format!("bimcraft-link-{name}-{}.bimcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": path}));
    path
}

fn walls(m: &Model) -> Vec<(Id, crate::model::Wall)> {
    m.elements
        .iter()
        .filter_map(|(id, e)| match e {
            Element::Wall(w) => Some((*id, w.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_link_is_drawn_in_the_views_and_is_not_the_projects() {
    let path = architect("seen");
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path, "name": "Architecture", "at": [1000, 2000]}));
    let _ = std::fs::remove_file(&path);
    assert!(walls(&s.model).is_empty(), "the project has no wall of its own");
    let seen = seen(&s.model);
    let linked = walls(&seen);
    assert_eq!(linked.len(), 4);
    assert!(linked.iter().all(|(id, _)| is_linked(*id)), "{linked:?}");
    // Moved by the link's place, on the level of this project at the same height.
    let (_, w) = linked.iter().min_by(|a, b| a.1.start.0.total_cmp(&b.1.start.0)).unwrap();
    assert_eq!(w.start, P2(1000.0, 2000.0));
    let level = s.model.levels()[0].0;
    assert!(linked.iter().all(|(_, w)| seen.same(w.level, level)));
    assert_eq!(seen.levels().len(), 2, "no level came in");
    // What the project would count, check or write doesn't have it.
    assert_eq!(s.model.check(), Ok(()));
    assert!(!crate::ifc::export(&s.model).contains("IFCWALL"));
}

#[test]
fn a_link_turns_about_its_origin() {
    let path = architect("turned");
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path, "angle": 90}));
    let _ = std::fs::remove_file(&path);
    let seen = seen(&s.model);
    let ends: Vec<(i64, i64)> = walls(&seen).iter().map(|(_, w)| (w.end.0.round() as i64, w.end.1.round() as i64)).collect();
    // The corner (8000, 0) of the box is at (0, 8000) after a quarter turn.
    assert!(ends.contains(&(0, 8000)), "{ends:?}");
}

#[test]
fn a_level_at_another_height_comes_in_named_after_the_link() {
    let mut s = Session::new();
    let mut other = Session::new();
    run(&mut other, "level", json!({"name": "Roof deck", "elevation": 9100}));
    run(&mut other, "wall", json!({"points": [[0, 0], [4000, 0]], "level": "Roof deck"}));
    let path = std::env::temp_dir().join(format!("bimcraft-link-levels-{}.bimcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut other, "save", json!({"path": path}));
    run(&mut s, "link", json!({"path": path, "name": "Shell"}));
    let _ = std::fs::remove_file(&path);
    let seen = seen(&s.model);
    let names: Vec<String> = seen.levels().iter().map(|(_, l)| l.name.clone()).collect();
    assert!(names.contains(&"Shell: Roof deck".to_string()), "{names:?}");
    assert_eq!(s.model.levels().len(), 2);
}

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("bimcraft-link-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn plans_draw_a_link_in_halftone_and_never_pick_it() {
    use crate::prim::{Prim, View};
    use crate::render;
    let path = architect("plan");
    let mut direct = Session::new();
    run(&mut direct, "open", json!({"path": path}));
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let view = View { scale: 0.08, cx: 4000.0, cy: 3000.0, width: 800, height: 600 };
    let level = s.model.levels()[0].0;
    let own = render::draw(&direct.model, level, view);
    let linked = render::draw(&seen(&s.model), level, view);
    let darkest = |d: &render::Drawing| {
        d.prims
            .iter()
            .map(|p| match p {
                Prim::Fill { color, .. } | Prim::Line { color, .. } => color.0.min(color.1).min(color.2),
                Prim::Image { .. } => 255,
            })
            .min()
    };
    assert!(darkest(&own) < Some(60), "a plan's lines are dark");
    assert!(darkest(&linked) > Some(100), "a link is halftone: {:?}", darkest(&linked));
    // The wall stands at (6000, 0): picked in the project's plan, not through the link.
    let at = view.px(P2(6000.0, 0.0));
    assert!(own.pick(at, 4.0).is_some());
    assert_eq!(linked.pick(at, 4.0), None);
    assert!(linked.in_box((0.0, 0.0), (800.0, 600.0), true).is_empty());
}

#[test]
fn the_3d_view_draws_a_link_and_never_picks_it() {
    use crate::graphics::Look;
    use crate::view3d::Scene;
    let path = architect("3d");
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let m = seen(&s.model);
    let scene = Scene::new(&m);
    let cam = scene.fit(30.0, 30.0, 400, 300);
    let image = scene.render_in(&cam, &[], &Look::of_3d(&m));
    let mut drawn = 0;
    for x in (0..400).step_by(5) {
        for y in (0..300).step_by(5) {
            drawn += usize::from(image.color(f64::from(x), f64::from(y)) != crate::prim::PAPER);
            assert!(image.pick(f64::from(x), f64::from(y)).is_none_or(|id| !is_linked(id)));
        }
    }
    assert!(drawn > 100, "the walls show: {drawn} pixels");
    assert!(image.shows().iter().all(|id| !is_linked(*id)));
    assert!(image.in_box((0.0, 0.0), (400.0, 300.0), true).iter().all(|id| !is_linked(*id)));
}

#[test]
fn a_link_to_an_ifc_file_is_drawn_too() {
    let mut other = Session::new();
    run(&mut other, "wall", json!({"points": [[0, 0], [5000, 0], [5000, 4000]]}));
    let path = std::env::temp_dir().join(format!("bimcraft-link-ifc-{}.ifc", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut other, "save", json!({"path": path}));
    let mut s = Session::new();
    let made = run(&mut s, "link", json!({"path": path, "at": [100, 200]}));
    let _ = std::fs::remove_file(&path);
    assert!(made["elements"].as_u64().unwrap() > 0, "{made}");
    assert!(!walls(&seen(&s.model)).is_empty());
}

#[test]
fn a_link_is_kept_by_its_path_and_read_again_when_the_project_opens() {
    let d = dir("file");
    let (arch, project) = (d.join("arch.bimcraft"), d.join("project.bimcraft"));
    let mut other = Session::new();
    run(&mut other, "wall", json!({"points": [[0, 0], [8000, 0]]}));
    run(&mut other, "save", json!({"path": arch}));
    let mut s = Session::new();
    run(&mut s, "save", json!({"path": project}));
    // A relative path is read from the project's folder, and kept as it was given.
    run(&mut s, "link", json!({"path": "arch.bimcraft", "name": "Arch"}));
    run(&mut s, "save", json!({"path": project}));
    let text = std::fs::read_to_string(&project).unwrap();
    assert!(text.contains("arch.bimcraft") && !text.contains("\"loaded\"") && !text.contains("\"Wall\""), "the file holds the path alone");
    let mut again = Session::new();
    let reply = run(&mut again, "open", json!({"path": project}));
    assert!(reply.get("links_not_loaded").is_none(), "{reply}");
    assert_eq!(walls(&seen(&again.model)).len(), 1);
    // The file moves away: the link is there and draws nothing; the project still checks.
    std::fs::remove_file(&arch).unwrap();
    let mut lost = Session::new();
    let reply = run(&mut lost, "open", json!({"path": project}));
    assert_eq!(reply["links_not_loaded"][0]["name"], "Arch", "{reply}");
    assert!(walls(&seen(&lost.model)).is_empty());
    assert_eq!(lost.model.check(), Ok(()));
    // Back again, and read again.
    run(&mut other, "save", json!({"path": arch}));
    let r = run(&mut lost, "reload", json!({}));
    assert_eq!(r["reloaded"], json!(["Arch"]), "{r}");
    assert_eq!(walls(&seen(&lost.model)).len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn reload_reads_a_changed_file_and_only_for_links() {
    let path = architect("reload");
    let mut s = Session::new();
    let id = run(&mut s, "link", json!({"path": path}))["id"].as_u64().unwrap();
    assert_eq!(walls(&seen(&s.model)).len(), 4);
    let mut bigger = Session::new();
    run(&mut bigger, "open", json!({"path": path}));
    run(&mut bigger, "wall", json!({"points": [[0, 0], [0, -3000]]}));
    run(&mut bigger, "save", json!({"path": path}));
    run(&mut s, "reload", json!({"id": id}));
    let _ = std::fs::remove_file(&path);
    assert_eq!(walls(&seen(&s.model)).len(), 5);
    let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id);
    assert_eq!(wall, None);
    let level = s.model.levels()[0].0;
    assert!(s.execute("reload", &json!({"id": level})).unwrap_err().contains("not a link"));
}

#[test]
fn a_link_moves_and_turns_with_the_commands_that_move_and_turn() {
    let path = architect("moved");
    let mut s = Session::new();
    let id = run(&mut s, "link", json!({"path": path}))["id"].as_u64().unwrap();
    let _ = std::fs::remove_file(&path);
    run(&mut s, "move", json!({"ids": [id], "by": [500, 700]}));
    let Some(Element::Link(l)) = s.model.elements.get(id) else { panic!() };
    assert_eq!(l.at, P2(500.0, 700.0));
    run(&mut s, "modify", json!({"id": id, "at": [0, 0], "angle": 180}));
    let corner = walls(&seen(&s.model)).iter().map(|(_, w)| w.start).fold(P2(0.0, 0.0), |a, b| P2(a.0.min(b.0), a.1.min(b.1)));
    assert_eq!((corner.0.round(), corner.1.round()), (-8000.0, -6000.0), "turned half way round");
    run(&mut s, "undo", json!({}));
    run(&mut s, "delete", json!({"ids": [id]}));
    assert!(walls(&seen(&s.model)).is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(walls(&seen(&s.model)).len(), 4, "undo brings the link back with what it read");
}

#[test]
fn what_cant_be_a_link_is_refused() {
    let path = architect("refused");
    let mut s = Session::new();
    assert!(s.execute("link", &json!({})).unwrap_err().contains("`path`"));
    assert!(s.execute("link", &json!({"path": "/nowhere/x.bimcraft"})).is_err());
    run(&mut s, "link", json!({"path": path, "name": "A"}));
    assert!(s.execute("link", &json!({"path": path, "name": "A"})).unwrap_err().contains("already named"));
    assert!(s.execute("link", &json!({"path": path, "name": "B", "angle": 1e9})).unwrap_err().contains("angle"));
    assert!(s.execute("link", &json!({"path": path, "name": "C", "at": [1e12, 0]})).is_err());
    let _ = std::fs::remove_file(&path);
    // The project that is itself a link, and the project that is not a project at all.
    assert_eq!(s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Link(_))).count(), 1);
    let junk = std::env::temp_dir().join(format!("bimcraft-link-junk-{}.bimcraft", std::process::id()));
    std::fs::write(&junk, "not a project").unwrap();
    assert!(s.execute("link", &json!({"path": junk})).is_err());
    let _ = std::fs::remove_file(&junk);
}

#[test]
fn a_linked_project_s_own_links_are_not_followed() {
    let inner = architect("inner");
    let mut mid = Session::new();
    run(&mut mid, "link", json!({"path": inner}));
    let outer_path = std::env::temp_dir().join(format!("bimcraft-link-mid-{}.bimcraft", std::process::id()));
    run(&mut mid, "save", json!({"path": outer_path}));
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": outer_path}));
    let _ = (std::fs::remove_file(&inner), std::fs::remove_file(&outer_path));
    assert!(walls(&seen(&s.model)).is_empty(), "the link of a link draws nothing");
}

#[test]
fn a_pick_snaps_to_what_a_link_builds() {
    use crate::snap::{self, Kind, Reach, Snaps};
    let path = architect("snap");
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path, "at": [1000, 0]}));
    let _ = std::fs::remove_file(&path);
    let level = s.model.levels()[0].0;
    let reach = Reach { point: 300.0, line: 300.0 };
    // The linked box's corner is at (9000, 0) here: a pick near it lands on it, which the project alone can't do.
    let near = P2(9060.0, 40.0);
    let own = snap::snap(&s.model, level, near, reach, &[], false, &Snaps::default());
    let found = snap::snap(&seen(&s.model), level, near, reach, &[], false, &Snaps::default());
    assert_eq!((own.kind, found.kind), (Kind::Free, Kind::Endpoint), "{own:?} {found:?}");
    assert_eq!(found.at, P2(9000.0, 0.0));
}

#[test]
fn the_render_command_shows_a_link_in_plans_and_in_3d() {
    let path = architect("render");
    let mut bare = Session::new();
    let without = run(&mut bare, "render", json!({"view": "Level 1", "width": 400, "height": 300}));
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let with = run(&mut s, "render", json!({"view": "Level 1", "width": 400, "height": 300}));
    assert_ne!(with["pngBase64"], without["pngBase64"], "the linked walls are in the picture");
    let three_d = run(&mut s, "render", json!({"view": "3D", "width": 300, "height": 200}));
    let three_d_bare = run(&mut bare, "render", json!({"view": "3D", "width": 300, "height": 200}));
    assert_ne!(three_d["pngBase64"], three_d_bare["pngBase64"]);
}

#[test]
fn a_recovered_project_reads_its_links_from_where_the_project_lives() {
    let (d, elsewhere) = (dir("recover"), dir("backups"));
    let (arch, project, backup) = (d.join("arch.bimcraft"), d.join("project.bimcraft"), elsewhere.join("backup.bimcraft"));
    let mut other = Session::new();
    run(&mut other, "wall", json!({"points": [[0, 0], [8000, 0]]}));
    run(&mut other, "save", json!({"path": arch}));
    let mut s = Session::new();
    run(&mut s, "save", json!({"path": project}));
    run(&mut s, "link", json!({"path": "arch.bimcraft"}));
    run(&mut s, "autosave", json!({"path": backup}));
    let mut back = Session::new();
    run(&mut back, "recover", json!({"path": backup, "project": project}));
    assert_eq!(walls(&seen(&back.model)).len(), 1, "the backup's folder has no arch.bimcraft; the project's has");
    let _ = (std::fs::remove_dir_all(&d), std::fs::remove_dir_all(&elsewhere));
}

#[test]
fn what_a_link_builds_lies_under_what_the_project_draws() {
    use crate::prim::View;
    use crate::render;
    let path = architect("under");
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[1000, 1000], [7000, 1000]]}));
    run(&mut s, "link", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let view = View { scale: 0.08, cx: 4000.0, cy: 3000.0, width: 800, height: 600 };
    let level = s.model.levels()[0].0;
    let d = render::draw(&seen(&s.model), level, view);
    let first_own = d.owners.iter().position(|o| o.is_some_and(|id| !is_linked(id)));
    let last_linked = d.owners.iter().rposition(|o| o.is_some_and(is_linked));
    assert!(first_own.is_some() && last_linked.is_some());
    assert!(last_linked < first_own, "the link's {last_linked:?} comes before the project's {first_own:?}");
}

#[test]
fn a_link_brings_no_detail_lines_or_regions_into_the_projects_views() {
    use crate::prim::View;
    use crate::render;
    let mut other = Session::new();
    run(&mut other, "wall", json!({"points": [[0, 0], [8000, 0]]}));
    run(&mut other, "detail_line", json!({"points": [[0, 500], [6000, 500]]}));
    let square: Vec<Value> = [[[0, 1000], [3000, 1000]], [[3000, 1000], [3000, 3000]], [[3000, 3000], [0, 3000]], [[0, 3000], [0, 1000]]]
        .into_iter()
        .map(|l| json!(l))
        .collect();
    run(&mut other, "filled_region", json!({"sketch": square, "type": "Masking"}));
    let path = std::env::temp_dir().join(format!("bimcraft-link-detail-{}.bimcraft", std::process::id()));
    let path = path.to_string_lossy().into_owned();
    run(&mut other, "save", json!({"path": path}));
    let mut s = Session::new();
    run(&mut s, "link", json!({"path": path}));
    let _ = std::fs::remove_file(&path);
    let seen = seen(&s.model);
    let kinds: Vec<&str> = seen.elements.iter().filter(|(id, _)| is_linked(**id)).map(|(_, e)| e.category()).collect();
    assert!(kinds.contains(&"wall"), "its walls come in: {kinds:?}");
    assert!(!kinds.contains(&"line") && !kinds.contains(&"region"), "its annotation stays in it: {kinds:?}");
    // A masking region of the link's would hide this project's wall.
    run(&mut s, "wall", json!({"points": [[500, 1500], [2500, 1500]]}));
    let view = View { scale: 0.08, cx: 3000.0, cy: 1500.0, width: 600, height: 400 };
    let level = s.model.levels()[0].0;
    let d = render::draw(&seen, level, view);
    let linked_fills = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(p, o)| matches!(p, render::Prim::Fill { color, .. } if *color == render::PAPER) && o.is_some_and(is_linked))
        .count();
    assert_eq!(linked_fills, 0);
}

/// A box of walls as a project's text.
fn box_text(width: f64) -> String {
    let mut other = Session::new();
    run(&mut other, "wall", json!({"points": [[0, 0], [width, 0], [width, 6000], [0, 6000]], "closed": true}));
    other.model.to_json().unwrap()
}

#[test]
fn a_link_from_text_is_drawn_and_linked_again_takes_its_place() {
    let mut s = Session::new();
    let out = run(&mut s, "link", json!({"text": box_text(8000.0), "name": "Site", "at": [1000, 0]}));
    assert_eq!((out["name"].as_str(), out["replaced"].as_bool()), (Some("Site"), Some(false)));
    let linked = walls(&seen(&s.model));
    assert_eq!(linked.len(), 4);
    assert!(linked.iter().all(|(id, _)| is_linked(*id)));
    assert!(linked.iter().any(|(_, w)| w.start == P2(1000.0, 0.0)), "it stands at `at`");
    assert!(walls(&s.model).is_empty(), "nothing of it is the project's");
    // The same name again: the newer text, in the same link.
    let again = run(&mut s, "link", json!({"text": box_text(12000.0), "name": "Site"}));
    assert_eq!((again["id"].clone(), again["replaced"].as_bool()), (out["id"].clone(), Some(true)));
    let links = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Link(_))).count();
    assert_eq!(links, 1);
    let ends: Vec<f64> = walls(&seen(&s.model)).iter().map(|(_, w)| w.end.0.max(w.start.0)).collect();
    assert!(ends.iter().any(|x| (*x - 12000.0).abs() < 1e-6), "{ends:?}");
    // Undo brings back the first text; and the project keeps the link, whose text no file holds.
    run(&mut s, "undo", json!({}));
    assert_eq!(walls(&seen(&s.model)).len(), 4);
    let reload = run(&mut s, "reload", json!({}));
    assert!(reload["failed"][0]["error"].as_str().is_some_and(|e| e.contains("text")), "{reload}");
}

#[test]
fn what_cant_be_a_link_from_text_is_refused() {
    let mut s = Session::new();
    assert!(s.execute("link", &json!({"text": "not a project"})).unwrap_err().contains("not a BIMCraft project"));
    assert!(s.execute("link", &json!({"text": 42})).is_err());
    assert!(s.execute("link", &json!({"text": box_text(8000.0), "path": "x.bimcraft"})).unwrap_err().contains("not both"));
    assert!(s.execute("link", &json!({"path": "text:Site"})).unwrap_err().contains("text:"));
    assert!(s.execute("link", &json!({"text": box_text(8000.0), "angle": f64::MAX})).is_err());
    assert!(s.model.elements.iter().all(|(_, e)| !matches!(e, Element::Link(_))), "the model is as it was");
}

//! `view3d::update`'s tests that drive the Session (moved out of `view3d/update.rs`).

use crate::model::Id;
use crate::view3d::{Scene, Tri};
use std::collections::{BTreeSet, HashMap};

use std::sync::Arc;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::oracle_tests::{Dice, edit};
use crate::view3d::update::*;
use crate::view3d::{AZIMUTH, ELEVATION};

/// That `a`, a scene brought up to date, is `b`, the whole scene made again: the same triangles, faces, normals, textures'
/// ways and copies, planes numbered alike (the same triangles share one), and the same picture.
fn same(a: &Scene, b: &Scene, what: &str) {
    let differ: Vec<(Id, usize, usize)> = {
        let ids: BTreeSet<Id> = a.spans.iter().chain(&b.spans).map(|s| s.id).collect();
        fn own(x: &Scene, id: Id) -> &[Tri] {
            x.span(id).and_then(|s| x.tris.get(s.at.clone())).unwrap_or_default()
        }
        ids.into_iter().filter(|id| own(a, *id) != own(b, *id)).map(|id| (id, own(a, id).len(), own(b, id).len())).collect()
    };
    if !differ.is_empty() {
        for (id, ..) in &differ {
            let (x, y) = (a.span(*id), b.span(*id));
            eprintln!("{id}: {x:?} {y:?}");
            if let (Some(x), Some(y)) = (x, y) {
                for (p, q) in a.tris[x.at.clone()].iter().zip(&b.tris[y.at.clone()]) {
                    if p != q {
                        eprintln!("  {p:?}\n  {q:?}");
                        break;
                    }
                }
            }
        }
    }
    assert!(differ.is_empty(), "{what}: the triangles of (element, brought up to date, made whole) {differ:?}");
    assert!(a.tris == b.tris, "{what}: triangles");
    assert!(a.faces == b.faces, "{what}: faces");
    assert!(a.smooth == b.smooth, "{what}: smooth normals");
    assert_eq!(a.spans, b.spans, "{what}: spans");
    assert_eq!((&a.masses, &a.levels), (&b.masses, &b.levels), "{what}: masses and levels");
    let (mut there, mut back) = (HashMap::new(), HashMap::new());
    for (k, (p, q)) in a.planes.iter().zip(&b.planes).enumerate() {
        assert_eq!(*there.entry(*p).or_insert(*q), *q, "{what}: triangle {k}'s plane is another's");
        assert_eq!(*back.entry(*q).or_insert(*p), *p, "{what}: triangle {k}'s plane is another's");
    }
    assert_eq!(a.placements.len(), b.placements.len(), "{what}: copies");
    for ((i, c), (j, d)) in a.placements.iter().zip(&b.placements) {
        assert_eq!((i, c.owner, c.at, c.axes), (j, d.owner, d.at, d.axes), "{what}: a copy");
        assert_eq!(a.shapes.get(c.shape), b.shapes.get(d.shape), "{what}: a copy's shape");
    }
    for t in &b.tris {
        let k = t.texture() as usize;
        assert_eq!(a.skins.get(k).is_some_and(Option::is_some), b.skins.get(k).is_some_and(Option::is_some), "{what}: texture {k}");
    }
    let cam = b.fit(AZIMUTH, ELEVATION, 240, 180);
    assert!(a.render(&cam, &[]).png() == b.render(&cam, &[]).png(), "{what}: the picture");
}

/// Runs `cmd` and holds the scene brought up to date to the whole one: the scene after.
fn held(s: &mut Session, scene: Scene, cmd: &str, p: &Value) -> Scene {
    let before = Arc::clone(&s.model);
    if s.execute(cmd, p).is_err() {
        return scene;
    }
    let up = scene.update(&before, &s.model);
    same(&up, &Scene::new(&s.model), &format!("{cmd} {p}"));
    up
}

fn of(s: &Session, category: &str) -> Vec<Id> {
    s.model.elements.iter().filter(|(_, e)| e.category() == category).map(|(id, _)| *id).collect()
}

#[test]
fn a_scene_brought_up_to_date_by_an_edit_is_the_scene_made_whole() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0]}
           window {"at": [8000, 3000]}
           floor {}
           component {"family": "Table", "type": "1600 x 900", "at": [2000, 3000], "angle": 30}
           component {"family": "Table", "type": "1600 x 900", "at": [5000, 3000]}"#,
    )
    .unwrap();
    let mut scene = Scene::new(&s.model);
    let walls = of(&s, "wall");
    let level = of(&s, "level")[0];
    let table = of(&s, "component")[0];
    let edits = [
        ("move", json!({"ids": [walls[1]], "by": [500, 0]})),
        ("move", json!({"ids": [walls[0]], "by": [0, 300], "end": "end"})),
        ("modify", json!({"id": walls[2], "height": 4000})),
        ("modify", json!({"id": level, "elevation": 500})),
        ("paint", json!({"ids": [walls[0]], "face": "exterior", "material": "Timber"})),
        ("shaft", json!({"boundary": [[3000, 3000], [4000, 3000], [4000, 4000], [3000, 4000]]})),
        ("move", json!({"ids": [table], "by": [1000, 500]})),
        ("copy", json!({"ids": [table], "by": [0, 1500]})),
        ("component", json!({"family": "Chair", "at": [6000, 1000]})),
        ("delete", json!({"ids": [of(&s, "door")[0]]})),
        ("wall", json!({"points": [[4000, 0], [4000, 6000]]})),
        ("material", json!({"name": "Timber", "color": [200, 40, 40]})),
        ("delete", json!({"ids": [walls[3]]})),
    ];
    for (cmd, p) in &edits {
        scene = held(&mut s, scene, cmd, p);
    }
    // An undo and a redo: one model of the history for another.
    for step in ["undo", "redo", "undo"] {
        let before = Arc::clone(&s.model);
        s.execute(step, &json!({})).unwrap();
        let up = scene.update(&before, &s.model);
        same(&up, &Scene::new(&s.model), step);
        scene = up;
    }
}

/// A window rising past its wall's top (issue #215) notches the wall: each edit of the window, of the wall's height or of
/// what its top follows brings the notch up to date, as the scene made whole has it.
#[test]
fn a_window_past_its_walls_top_keeps_its_notch_up_to_date_through_edits() {
    let mut s = Session::new();
    s.script(
        r#"type {"category": "window", "name": "Tall", "width": 1200, "height": 1100, "sill": 30}
           wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "height": 1060}
           window {"wall": 100002, "offset": 4000, "type": "Tall"}
           roof {"boundary": [[-600, -600], [8600, -600], [8600, 6600], [-600, 6600]], "gable": true}"#,
    )
    .unwrap();
    let mut scene = Scene::new(&s.model);
    let (walls, window, roof) = (of(&s, "wall"), of(&s, "window")[0], of(&s, "roof")[0]);
    let edits = [
        ("modify", json!({"id": walls[0], "height": 900})),
        ("modify", json!({"id": window, "sill": 500})),
        ("modify", json!({"id": walls[0], "height": 2000})),
        ("modify", json!({"id": walls[0], "height": 1060})),
        ("move", json!({"ids": [window], "by": [1500, 0]})),
        ("modify", json!({"ids": walls, "attach_top": roof})),
        ("modify", json!({"ids": walls, "attach_top": null})),
        ("copy", json!({"ids": [walls[0]], "by": [0, -3000]})),
        ("delete", json!({"ids": [window]})),
    ];
    let steps = s.history().len();
    for (cmd, p) in &edits {
        scene = held(&mut s, scene, cmd, p);
    }
    assert_eq!(s.history().len(), steps + edits.len(), "every edit is made: {:?}", &s.history()[steps..]);
    assert!(s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Window(_))).count() == 1, "the copy's");
}

#[test]
fn random_edits_of_a_tower_bring_its_scene_up_to_date_as_made_whole() {
    let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap();
    for seed in [1u64, 2] {
        let mut s = Session::new();
        s.reset(model.clone(), None);
        let mut scene = Scene::new(&s.model);
        let mut dice = Dice(0x9e37_79b9_7f4a_7c15 ^ seed);
        for _ in 0..25 {
            let (cmd, p) = edit(&mut s, &mut dice);
            scene = held(&mut s, scene, &cmd, &p);
        }
    }
}

/// The other kinds and commands (3D dimensions, camera views, shaped floors, roofs with a slope for each edge, a wall's top
/// offset, bars, plan symbols…) edited at random: the scene brought up to date by each is the one made whole.
#[test]
fn random_edits_of_the_other_kinds_bring_the_scene_up_to_date_as_made_whole() {
    let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap();
    for seed in [1u64, 2] {
        let mut s = Session::new();
        s.reset(model.clone(), None);
        let mut scene = Scene::new(&s.model);
        let mut dice = Dice(0x3d3d_7a11_c0de_0188 ^ seed);
        for _ in 0..30 {
            let (cmd, p) = crate::oracle_tests::edit_more(&mut s, &mut dice);
            scene = held(&mut s, scene, &cmd, &p);
        }
    }
}

#[test]
fn an_edit_makes_again_what_it_touched_not_the_model() {
    let model = crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 2).unwrap();
    let mut s = Session::new();
    s.reset(model, None);
    let scene = Scene::new(&s.model);
    let before = Arc::clone(&s.model);
    let wall = of(&s, "wall").into_iter().find(|w| s.execute("move", &json!({"ids": [w], "by": [0, 200]})).is_ok()).unwrap();
    let Made::Some(made) = scene.made_again(&before, &s.model, &|| false) else { panic!("the whole scene made again") };
    let all = s.model.elements.len();
    assert!(made.contains_key(&wall));
    assert!(made.len() * 20 < all, "{} of {all} made again", made.len());
}

/// What an edit costs the 10-storey annotated tower's scene, against making it whole: `cargo test --release -p bimcraft-core
/// update_speed -- --ignored --nocapture`.
#[test]
#[ignore]
fn update_speed() {
    use serde_json::json;
    use std::time::Instant;
    let model = crate::sample::tower::annotated(crate::sample::tower::Plate::REFERENCE, 10).unwrap();
    let mut s = crate::engine::Session::new();
    s.reset(model, None);
    s.model.lookup();
    let t = Instant::now();
    let mut scene = crate::view3d::Scene::new(&s.model);
    eprintln!("whole scene: {:.2} s, {} triangles", t.elapsed().as_secs_f64(), scene.tris.len());
    let of = |s: &crate::engine::Session, c: &str| s.model.elements.iter().find(|(_, e)| e.category() == c).map(|(id, _)| *id);
    let level = s.model.levels().into_iter().nth(5).map(|(id, _)| id);
    let edits = [
        ("move a wall", "move", json!({"ids": [of(&s, "wall")], "by": [0, 100]})),
        ("move a component", "move", json!({"ids": [of(&s, "component")], "by": [300, 0]})),
        ("a level's elevation", "modify", json!({"id": level, "elevation": 20_100})),
    ];
    for (what, cmd, p) in edits {
        let before = std::sync::Arc::clone(&s.model);
        if let Err(e) = s.execute(cmd, &p) {
            eprintln!("{what}: {e}");
            continue;
        }
        s.model.lookup();
        let t = Instant::now();
        let made = match scene.made_again(&before, &s.model, &|| false) {
            Made::Some(m) => m.len(),
            _ => usize::MAX,
        };
        let found = t.elapsed().as_secs_f64();
        let up = scene.update(&before, &s.model);
        let updated = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let mesh = crate::view3d::gpu::Mesh::new(&up, &crate::graphics::Look::of_3d(&s.model));
        let meshed = t.elapsed().as_secs_f64();
        let t = Instant::now();
        let whole = crate::view3d::Scene::new(&s.model);
        eprintln!(
            "{what}: {made} elements made again in {found:.3} s; brought up to date {:.3} s; whole {:.2} s; mesh {meshed:.2} s ({})",
            updated - found,
            t.elapsed().as_secs_f64(),
            mesh.is_ok()
        );
        drop(whole);
        scene = up;
    }
}

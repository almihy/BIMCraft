//! `view3d::gpu`'s tests that drive the Session (moved out of `view3d/gpu.rs`).

use crate::graphics::Look;
use crate::model::Id;
use crate::prim::Rgb;
use crate::view3d::{Camera, LevelMarks, P3, Scene, add, dot, sub};

use crate::engine::Session;
use crate::view3d::gpu::*;
use crate::view3d::{AZIMUTH, ELEVATION};
use serde_json::json;
use std::cell::Cell;

fn sample() -> Session {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    s
}

#[test]
fn cancelled_mesh_and_picking_builds_publish_nothing_and_can_retry() {
    let s = sample();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let calls = Cell::new(0);
    let cancel = || {
        calls.set(calls.get() + 1);
        calls.get() > 2
    };
    assert!(Mesh::new_cancellable(&scene, &look, &cancel).unwrap().is_none());
    let mesh = Mesh::new(&scene, &look).unwrap();
    calls.set(0);
    assert!(!mesh.prepare_picking_cancellable(&cancel));
    assert!(mesh.bvh.get().is_none());
    assert!(mesh.prepare_picking_cancellable(&|| false));
    assert!(mesh.bvh.get().is_some());
}

/// Whether picking `s`'s 3D view through its mesh finds what the processor's picture shows, at every third pixel.
fn picks_as_the_picture(s: &Session) {
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    picks_through(s, scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look));
}

/// Whether picking `s`'s 3D view through its mesh, seen through `cam`, finds what the processor's picture shows.
fn picks_through(s: &Session, cam: Camera) {
    picks_tinted(s, cam, &[(s.model.levels().first().unwrap().0, Rgb(0, 120, 215))]);
}

/// [`picks_through`] with `tints`.
fn picks_tinted(s: &Session, cam: Camera, tints: &[(Id, Rgb)]) {
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let image = scene.render_in(&cam, tints, &look);
    let (mut agree, mut total) = (0, 0);
    for y in (0..480).step_by(3) {
        for x in (0..640).step_by(3) {
            let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let cpu = image.pick(x, y).map(|id| Hit { id, face: image.face(x, y).unwrap_or("") });
            total += 1;
            // Where a level's mark is picked, the CPU has no face either.
            agree += usize::from(mesh.pick(&cam, tints, x, y) == cpu);
        }
    }
    assert_eq!(agree, total, "only {agree} of {total} pixels pick what the picture shows");
}

#[test]
fn picking_the_mesh_finds_what_the_cpu_picture_shows() {
    picks_as_the_picture(&sample());
}

#[test]
fn picking_the_mesh_in_perspective_finds_what_the_cpu_picture_shows_from_afar_and_from_within() {
    let s = sample();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
    picks_through(&s, cam);
    // The eye inside the house, 1.6 m up, through a wide lens: what is behind it is cut away alike.
    picks_through(&s, crate::view3d::inside(&s.model, cam));
}

#[test]
fn the_surface_under_a_pixel_is_the_same_through_the_mesh_as_the_picture_and_an_orbit_or_a_zoom_about_it_keeps_it_there() {
    let s = sample();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let near = |a: P3, b: P3, d: f64| (0..3).all(|i| (a[i] - b[i]).abs() <= d);
    for cam in [flat, scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look)] {
        let image = scene.render_in(&cam, &[], &look);
        let (mut found, mut levels) = (0, 0);
        for y in (0..480).step_by(5) {
            for x in (0..640).step_by(5) {
                let at = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                match (scene.surface_in(&image, &cam, at), mesh.surface(&cam, &[], at.0, at.1)) {
                    (Some(a), Some(b)) => {
                        assert_eq!(a.id, b.id, "at {at:?}");
                        assert!(near(a.at, b.at, 1e-3) && near(a.normal, b.normal, 1e-9), "{a:?} {b:?}");
                        found += 1;
                        levels += usize::from(s.model.level(a.id).is_some());
                        // The point is on the line of sight through the pixel, and its normal faces the camera.
                        let shown = |c: Camera| {
                            let (px, py, _) = c.project(a.at);
                            (px - at.0).abs() < 1e-6 && (py - at.1).abs() < 1e-6
                        };
                        assert!(shown(cam) && dot(a.normal, cam.toward(a.at)) > 0.0, "{a:?} at {at:?}");
                        assert!(shown(cam.orbit_about(30.0, -10.0, a.at)), "orbited about it, it stays at {at:?}");
                        assert!(shown(cam.zoom_about(1.7, a.at)) && shown(cam.zoom_about(0.4, a.at)), "zoomed toward it");
                    }
                    (None, None) => {}
                    other => panic!("at {at:?} the picture and the mesh differ: {other:?}"),
                }
            }
        }
        assert!(found > 500 && levels > 0, "{found} surfaces, {levels} on a level's plane");
    }
}

#[test]
fn picking_through_what_the_view_sees_through_finds_what_the_cpu_picture_shows() {
    let mut s = sample();
    s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
    let mesh = Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
    assert!(mesh.clear_from > 0 && mesh.clear_from < mesh.vertices.len(), "opaque first, then the walls");
    let wall = mesh.owners.iter().find(|o| s.model.elements.get(o.id).is_some_and(|e| e.category() == "wall")).unwrap();
    assert_eq!(wall.transparency, 50);
    picks_as_the_picture(&s);
    // Everything seen through: nothing opaque to stand in front of.
    s.execute("graphics", &json!({"view": "{3D}", "categories": {"floors": {"transparency": 80}, "roofs": {"transparency": 30}}})).unwrap();
    picks_as_the_picture(&s);
}

/// Each copy of each shape, placed, against the flat mesh's own triangles of that element: the same count, owner, plane,
/// colour and texture, the corners within a hundredth of a millimetre and the normals within a 16-bit fraction.
fn copies_hold_the_flat_triangles(mesh: &Mesh, flat: &Mesh) {
    assert_eq!(mesh.triangles(), flat.triangles());
    assert_eq!(mesh.owners, flat.owners, "the owners numbered alike");
    assert!(flat.shapes.is_empty() && flat.instances.is_empty());
    for shape in &mesh.shapes {
        let local = &mesh.shape_vertices[shape.vertices.clone()];
        for inst in &mesh.instances[shape.instances.clone()] {
            // The flat mesh's opaque ones first, then those seen through: the shape's order.
            let own: Vec<&Vertex> = flat.vertices.iter().filter(|v| v.owner == inst.owner).collect();
            let clear = flat.vertices.iter().filter(|v| inst.clear_owner != 0 && v.owner == inst.clear_owner);
            let own: Vec<&Vertex> = own.into_iter().chain(clear).collect();
            assert_eq!(own.len(), local.len());
            let [a, b, c] = inst.axes;
            let turn = |v: [f32; 3]| -> [f32; 3] { std::array::from_fn(|i| v[0] * a[i] + v[1] * b[i] + v[2] * c[i]) };
            let w = |v: [f32; 3]| 1.0 / (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
            let normal = |n: [f32; 3]| {
                let v = turn([n[0] * w(a), n[1] * w(b), n[2] * w(c)]);
                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                v.map(|x| x / l)
            };
            for (k, (l, f)) in local.iter().zip(own).enumerate() {
                let p = turn(l.position);
                let plane = mesh.instance_planes[inst.planes as usize + k / 3];
                assert!((0..3).all(|i| (p[i] + inst.at[i] - f.position[i]).abs() < 0.01), "{k}: {p:?} {:?}", f.position);
                assert!((0..3).all(|i| (normal(l.normal)[i] - f.normal[i]).abs() < 1e-4 && (normal(l.face)[i] - f.face[i]).abs() < 1e-4));
                assert_eq!((l.color, l.tex, plane), (f.color, f.tex, f.plane));
            }
        }
    }
}

#[test]
fn the_sample_s_chairs_are_one_shape_placed_again_and_again() {
    let s = sample();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
    assert!(mesh.shapes.iter().any(|sh| sh.instances.len() >= 4), "{:?}", mesh.shapes);
    let components = s.model.elements.values().filter(|e| matches!(e, crate::model::Element::Component(_))).count();
    assert_eq!(mesh.instances.len(), components, "every component a copy");
    assert!(mesh.vertices.len() < flat.vertices.len());
    copies_hold_the_flat_triangles(&mesh, &flat);
}

/// The sample with three lamps: a frame, and a lens seen through.
fn with_lamps() -> Session {
    let mut s = sample();
    s.execute("material", &json!({"name": "Lens", "color": [250, 250, 230], "transparency": 60})).unwrap();
    s.execute(
        "family",
        &json!({"name": "Lamp", "types": {"A": {}}, "forms": [
            {"profile": [[0,0],[600,0],[600,600],[0,600]], "from": 2400, "to": 2500, "material": "Concrete"},
            {"profile": [[50,50],[550,50],[550,550],[50,550]], "from": 2300, "to": 2400, "material": "Lens"}]}),
    )
    .unwrap();
    for (x, angle) in [(2000, 0), (5000, 30), (8000, 90)] {
        s.execute("component", &json!({"family": "Lamp", "at": [x, 3000], "angle": angle})).unwrap();
    }
    s
}

#[test]
fn a_copy_seen_through_in_part_is_a_copy_with_two_owners() {
    let s = with_lamps();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
    let lamps: Vec<&Instance> = mesh.instances.iter().filter(|i| i.clear_owner != 0).collect();
    assert_eq!(lamps.len(), 3);
    let shape = mesh.shapes.iter().find(|sh| sh.instances.len() == 3 && sh.clear_from < sh.vertices.end).unwrap();
    assert!(shape.clear_from > shape.vertices.start, "the frame first, then the lens");
    assert!(lamps.iter().all(|l| mesh.owners[l.clear_owner as usize - 1].transparency == 60));
    copies_hold_the_flat_triangles(&mesh, &flat);
    picks_as_the_picture(&s);
}

#[test]
fn a_painted_or_hidden_copy_is_drawn_of_its_own() {
    let mut s = sample();
    let chairs: Vec<Id> = s
        .model
        .elements
        .iter()
        .filter(|(_, e)| matches!(e, crate::model::Element::Component(c) if c.family == "Chair"))
        .map(|(id, _)| *id)
        .collect();
    s.execute("material", &json!({"name": "Test paint", "color": [90, 120, 150]})).unwrap();
    s.execute("paint", &json!({"id": chairs[1], "face": "all", "material": "Test paint"})).unwrap();
    s.execute("graphics", &json!({"view": "{3D}", "elements": {chairs[2].to_string(): {"transparency": 40}}})).unwrap();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
    let copy_of = |id: Id| mesh.instances.iter().any(|i| mesh.owners[i.owner as usize - 1].id == id);
    assert!(copy_of(chairs[0]) && !copy_of(chairs[1]), "the painted chair is its own");
    assert!(!copy_of(chairs[2]), "and the one seen through, which its shape isn't");
    copies_hold_the_flat_triangles(&mesh, &flat);
    picks_as_the_picture(&s);
}

/// The sample with round runs of two materials: level, sloped, upright and at an angle in plan.
pub fn with_runs() -> Session {
    let mut s = sample();
    for (command, points, offset, end) in [
        ("pipe", [[1000, 1000], [7000, 1000]], 2600, 2600),
        ("pipe", [[1000, 2000], [6000, 5000]], 2700, 2200),
        ("pipe", [[3000, 3000], [3000, 3000]], 0, 2800),
        ("conduit", [[1000, 4000], [8000, 4000]], 2900, 2900),
        ("conduit", [[2000, 6000], [2000, 1000]], 2800, 2500),
    ] {
        s.execute(command, &json!({"points": points, "offset": offset, "end_offset": end})).unwrap();
    }
    s
}

#[test]
fn round_runs_are_one_tube_placed_stretched_and_turned() {
    let s = with_runs();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let (mesh, flat) = (Mesh::new(&scene, &look).unwrap(), Mesh::flat(&scene, &look).unwrap());
    let copy_of = |id: Id| mesh.instances.iter().any(|i| mesh.owners[i.owner as usize - 1].id == id);
    let runs: Vec<Id> = s.model.elements.iter().filter(|(_, e)| matches!(e.category(), "pipe" | "conduit")).map(|(id, _)| *id).collect();
    assert_eq!(runs.len(), 5);
    assert!(runs.iter().all(|id| copy_of(*id)), "every round run a copy");
    let tube = 3 * 4 * crate::runs::SIDES;
    assert!(mesh.shapes.iter().any(|sh| sh.vertices.len() == tube && sh.instances.len() == 5), "one tube: {:?}", mesh.shapes);
    copies_hold_the_flat_triangles(&mesh, &flat);
    picks_as_the_picture(&s);
}

/// Where a card puts `d` (from the mesh's origin) in clip space, in f64: as `to_clip` in the shader.
fn clip_of(mesh: &Mesh, cam: &Camera, d: P3) -> [f64; 4] {
    let view = mesh.view(cam);
    let f = |v: [f32; 3]| v.map(f64::from);
    let d = sub(d, f(view.target));
    let (x, y, z) = (dot(d, f(view.right)), dot(d, f(view.up)), dot(d, f(view.back)));
    let across = view.perspective.map_or(f64::from(view.scale), |p| f64::from(p.focal));
    let (sx, sy) = (2.0 * across / f64::from(cam.width), 2.0 * across / f64::from(cam.height));
    let (hx, hy) = (2.0 * f64::from(view.shift[0]) / f64::from(cam.width), -2.0 * f64::from(view.shift[1]) / f64::from(cam.height));
    match view.perspective {
        Some(p) => {
            let ahead = f64::from(p.eye) - z;
            [x * sx + hx * ahead, y * sy + hy * ahead, f64::from(p.a) * ahead + f64::from(p.b), ahead]
        }
        None => [x * sx + hx, y * sy + hy, 0.5 + z * 0.5 / f64::from(view.depth), 1.0],
    }
}

/// Whether every one of `points` lies past the same side of the clip volume, so none of what they bound reaches a pixel.
fn all_outside(clips: &[[f64; 4]]) -> bool {
    let sides: [fn(&[f64; 4]) -> bool; 6] =
        [|c| c[0] > c[3], |c| c[0] < -c[3], |c| c[1] > c[3], |c| c[1] < -c[3], |c| c[2] < 0.0, |c| c[2] > c[3]];
    sides.iter().any(|side| clips.iter().all(side))
}

#[test]
fn groups_hold_what_is_drawn_in_order_and_a_view_leaves_out_only_what_it_cannot_see() {
    let s = with_runs();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let within = |g: &Group, p: P3| (0..3).all(|i| g.lo[i] - 1e-6 <= p[i] && p[i] <= g.hi[i] + 1e-6);
    // The triangles drawn of their own: whole triangles, in the order drawn, never across where those seen through start.
    let mut at = 0;
    for g in &mesh.groups {
        assert_eq!(g.range.start, at);
        assert!(g.range.end > at && g.range.len() % 3 == 0, "{:?}", g.range);
        assert!(!(g.range.start < mesh.clear_from && mesh.clear_from < g.range.end), "{:?} across {}", g.range, mesh.clear_from);
        assert!(mesh.vertices[g.range.clone()].iter().all(|v| within(g, v.position.map(f64::from))));
        at = g.range.end;
    }
    assert_eq!(at, mesh.vertices.len());
    assert!(mesh.groups.len() > 1, "{} groups", mesh.groups.len());
    // What a view leaves out lies wholly past one side of what it sees; seen whole, nothing is left out.
    let fitted = scene.fit_in(AZIMUTH, ELEVATION, 800, 600, &look);
    let near = Camera { scale: fitted.scale * 8.0, target: add(fitted.target, [-3000.0, -2000.0, 0.0]), ..fitted };
    let inside = crate::view3d::inside(&s.model, Camera { lens: Some(35.0), ..fitted });
    let expanded = |ranges: &[std::ops::Range<usize>]| ranges.iter().flat_map(Clone::clone).collect::<std::collections::BTreeSet<_>>();
    for (name, cam) in
        [("fitted", fitted), ("near", near), ("inside", inside), ("fitted in perspective", Camera { lens: Some(35.0), ..fitted })]
    {
        let seen = mesh.visible(&cam);
        let (own, copies) = (expanded(&seen.vertices), expanded(&seen.instances));
        for w in seen.vertices.windows(2).chain(seen.instances.windows(2)) {
            assert!(w[0].end < w[1].start, "{name}: {w:?} in order and apart");
        }
        for t in mesh.vertices.chunks(3).enumerate().filter(|(k, _)| !own.contains(&(k * 3))) {
            let clips: Vec<_> = t.1.iter().map(|v| clip_of(&mesh, &cam, v.position.map(f64::from))).collect();
            assert!(all_outside(&clips), "{name}: triangle {} left out but seen: {clips:?}", t.0);
            assert!((0..3).all(|c| !own.contains(&(t.0 * 3 + c))), "{name}: a triangle split");
        }
        for (k, c) in mesh.instances.iter().enumerate().filter(|(k, _)| !copies.contains(k)) {
            let shape = mesh.shapes.iter().find(|sh| sh.instances.contains(&k)).unwrap();
            let clips: Vec<_> = mesh.shape_vertices[shape.vertices.clone()]
                .iter()
                .map(|v| {
                    let p: P3 = std::array::from_fn(|i| {
                        f64::from(c.at[i]) + (0..3).map(|a| f64::from(v.position[a]) * f64::from(c.axes[a][i])).sum::<f64>()
                    });
                    clip_of(&mesh, &cam, p)
                })
                .collect();
            assert!(all_outside(&clips), "{name}: copy {k} left out but seen");
        }
        let left_out = mesh.vertices.len() - own.len() + mesh.instances.len() - copies.len();
        match name {
            "fitted" | "fitted in perspective" => assert_eq!(left_out, 0, "{name}: all of it is in view"),
            _ => assert!(left_out > 0, "{name}: something is out of view"),
        }
    }
}

#[test]
fn nearest_first_draws_the_nearest_within_a_budget_and_a_box_for_each_of_the_rest() {
    let s = with_runs();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let fitted = scene.fit_in(AZIMUTH, ELEVATION, 800, 600, &look);
    let expanded = |ranges: &[std::ops::Range<usize>]| ranges.iter().flat_map(Clone::clone).collect::<std::collections::BTreeSet<_>>();
    let tris_of = |k: usize| mesh.shapes.iter().find(|sh| sh.instances.contains(&k)).map_or(0, |sh| sh.vertices.len() / 3);
    // The cube a box is drawn from: twelve triangles, opaque, with planes of its own.
    assert_eq!(mesh.cube.len(), 36);
    for cam in [fitted, Camera { lens: Some(35.0), ..fitted }] {
        let seen = mesh.visible(&cam);
        let all = mesh.cost(&seen);
        assert_eq!(
            all,
            seen.vertices.iter().map(|r| r.len() / 3).sum::<usize>()
                + seen.instances.iter().flat_map(Clone::clone).map(tris_of).sum::<usize>()
        );
        // Within the budget, all of it as it is.
        let whole = mesh.nearest_first(&cam, &seen, all);
        assert_eq!((whole.seen.clone(), whole.stand_ins.len()), (seen.clone(), 0));
        // Past it, the nearest within it and a box for each group or copy left over, nearer ones never boxed before farther ones drawn.
        let near = mesh.nearest_first(&cam, &seen, all / 3);
        assert!(mesh.cost(&near.seen) <= all / 3 && mesh.cost(&near.seen) > 0, "{} of {all}", mesh.cost(&near.seen));
        let (drawn_own, drawn_copies) = (expanded(&near.seen.vertices), expanded(&near.seen.instances));
        let (seen_own, seen_copies) = (expanded(&seen.vertices), expanded(&seen.instances));
        assert!(drawn_own.is_subset(&seen_own) && drawn_copies.is_subset(&seen_copies));
        let left_groups: Vec<&Group> =
            mesh.groups.iter().filter(|g| seen_own.contains(&g.range.start) && !drawn_own.contains(&g.range.start)).collect();
        let left_copies: Vec<usize> = seen_copies.difference(&drawn_copies).copied().collect();
        assert_eq!(near.stand_ins.len(), left_groups.len() + left_copies.len());
        let far = |k: usize, copy: bool| mesh.nearness(&cam, if copy { mesh.copy_spheres[k] } else { mesh.group_spheres[k] });
        let group_of = |start: usize| mesh.groups.iter().position(|g| g.range.start == start).unwrap();
        let drawn_far = drawn_own.iter().filter(|v| mesh.groups.iter().any(|g| g.range.start == **v)).map(|v| far(group_of(*v), false));
        let drawn_far = drawn_far.chain(drawn_copies.iter().map(|k| far(*k, true))).fold(f64::MIN, f64::max);
        let boxed_near = left_groups.iter().map(|g| far(group_of(g.range.start), false));
        let boxed_near = boxed_near.chain(left_copies.iter().map(|k| far(*k, true))).fold(f64::MAX, f64::min);
        assert!(drawn_far <= boxed_near, "drawn as far as {drawn_far}, boxed from {boxed_near}");
        assert!(!near.stand_ins.is_empty());
        let placed = |c: &Instance, p: [f32; 3]| -> P3 {
            std::array::from_fn(|i| f64::from(c.at[i]) + (0..3).map(|a| f64::from(p[a]) * f64::from(c.axes[a][i])).sum::<f64>())
        };
        let inside = |b: &Instance, p: P3| {
            // The box's own coordinates of `p`: within 0 to 1 along each of its axes.
            let d = sub(p, b.at.map(f64::from));
            (0..3).all(|a| {
                let axis = b.axes[a].map(f64::from);
                let t = dot(d, axis) / dot(axis, axis);
                (-1e-3..=1.0 + 1e-3).contains(&t)
            })
        };
        for (b, g) in near.stand_ins.iter().zip(&left_groups) {
            assert_eq!(b.planes, mesh.cube_planes);
            assert!(mesh.vertices[g.range.clone()].iter().all(|v| inside(b, v.position.map(f64::from))), "a group outside its box");
        }
        for (b, &k) in near.stand_ins[left_groups.len()..].iter().zip(&left_copies) {
            let (c, shape) = (&mesh.instances[k], mesh.shapes.iter().find(|sh| sh.instances.contains(&k)).unwrap());
            assert_eq!(b.owner, c.owner);
            assert!(
                mesh.shape_vertices[shape.vertices.clone()].iter().all(|v| inside(b, placed(c, v.position))),
                "copy {k} outside its box"
            );
        }
    }
}

/// The sample house with a section box through it: from its least corner to `(x, y, z)` of the way across what it holds.
fn cut_through(s: &mut Session, [x, y, z]: [f64; 3]) -> (P3, P3) {
    s.execute("section_box", &json!({"on": true})).unwrap();
    let (lo, hi) = Look::of_3d(&s.model).section_box().unwrap();
    let to = [lo[0] + (hi[0] - lo[0]) * x, lo[1] + (hi[1] - lo[1]) * y, lo[2] + (hi[2] - lo[2]) * z];
    s.execute("section_box", &json!({"min": lo, "max": to})).unwrap();
    Look::of_3d(&s.model).section_box().unwrap()
}

#[test]
fn picking_the_mesh_through_a_section_box_finds_what_the_cpu_picture_shows() {
    let mut s = sample();
    cut_through(&mut s, [0.6, 0.7, 0.45]);
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    picks_through(&s, flat);
    let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
    picks_through(&s, cam);
    picks_through(&s, crate::view3d::inside(&s.model, cam));
}

#[test]
fn a_section_box_leaves_out_of_a_frame_only_what_lies_wholly_outside_it() {
    let mut s = sample();
    let whole = {
        let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
        let mesh = Mesh::new(&scene, &look).unwrap();
        mesh.cost(&mesh.visible(&scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look)))
    };
    let (lo, hi) = cut_through(&mut s, [0.3, 0.3, 0.3]);
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let seen = mesh.visible(&scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look));
    assert!(mesh.cost(&seen) < whole, "{} triangles drawn, of {whole}", mesh.cost(&seen));
    // Any triangle with a corner in the box is drawn.
    for (n, t) in mesh.vertices.as_chunks::<3>().0.iter().enumerate() {
        let inside = t.iter().any(|v| (0..3).all(|i| (lo[i]..=hi[i]).contains(&(f64::from(v.position[i]) + mesh.origin[i]))));
        assert!(!inside || seen.vertices.iter().any(|r| r.contains(&(n * 3))), "triangle {n} is in the box and left out");
    }
}

#[test]
fn a_mesh_picks_through_any_box_it_is_asked_as_a_mesh_made_for_that_box_does() {
    let mut s = sample();
    let (whole, whole_look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let kept = Mesh::new(&whole, &whole_look).unwrap();
    let cam = whole.fit_in(AZIMUTH, ELEVATION, 640, 480, &whole_look);
    let cut = cut_through(&mut s, [0.6, 0.7, 0.45]);
    let own = Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
    let mut differ_from_uncut = 0;
    for y in (0..480).step_by(4) {
        for x in (0..640).step_by(4) {
            let (x, y) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let asked = kept.pick_cut(&cam, &[], Some(cut), x, y);
            assert_eq!(asked, own.pick(&cam, &[], x, y), "at {x}, {y}");
            differ_from_uncut += usize::from(asked != kept.pick(&cam, &[], x, y));
        }
    }
    assert!(differ_from_uncut > 100, "the box changes what is picked: {differ_from_uncut}");
}

#[test]
fn a_section_box_leaves_out_a_level_past_it_and_a_box_being_dragged_asks_of_the_same_mesh() {
    let mut s = sample();
    let (lo, hi) = cut_through(&mut s, [0.6, 0.7, 0.45]);
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let within = |(lo, hi): (P3, P3)| -> Vec<Id> {
        scene.levels.iter().filter(|(.., c)| (lo[2]..=hi[2]).contains(&c[0][2])).map(|(id, ..)| *id).collect()
    };
    let ids = |marks: Vec<LevelMarks>| marks.iter().map(|m| m.id).collect::<Vec<Id>>();
    let kept = ids(mesh.marks(&cam, &[]));
    assert!(!kept.is_empty() && kept.len() < scene.levels.len(), "{} of {} levels", kept.len(), scene.levels.len());
    assert_eq!(kept, within((lo, hi)));
    let off = (
        lo,
        [
            hi[0],
            hi[1],
            kept.iter().filter_map(|k| scene.levels.iter().find(|(id, ..)| id == k)).map(|(.., c)| c[0][2]).fold(f64::MAX, f64::min) - 1.0,
        ],
    );
    assert!(ids(mesh.marks_cut(&cam, &[], Some(off))).len() < kept.len(), "a box dragged below a level leaves it out");
}

#[test]
fn picking_the_mesh_in_hidden_line_finds_what_the_cpu_picture_shows_glass_and_all() {
    let mut s = sample();
    s.execute("visual_style", &json!({"style": "hidden_line"})).unwrap();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    picks_through(&s, flat);
    picks_through(&s, scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look));
    // A section's hidden line is opaque, glass too: nothing is drawn seen through.
    let mesh = Mesh::new(&scene, &look).unwrap();
    assert!(mesh.lines);
    assert_eq!(mesh.clear_from, mesh.vertices.len());
    assert!(mesh.owners.iter().all(|o| o.transparency == 0));
    assert!(mesh.shapes.iter().all(|s| s.clear_from == s.vertices.end));
    assert!(mesh.vertices.iter().all(|v| v.tex == 0), "no texture on a paper face");
    cut_through(&mut s, [0.6, 0.7, 0.45]);
    picks_through(&s, flat);
}

#[test]
fn picking_the_mesh_in_wireframe_finds_what_the_cpu_picture_shows_within_two_pixels_of_a_line() {
    let mut s = sample();
    s.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
    let wall = *s.model.elements.iter().find(|(_, e)| e.category() == "wall").unwrap().0;
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
    picks_tinted(&s, flat, &[]);
    // What is tinted is drawn over the rest, so it is what a pixel near both picks.
    picks_tinted(&s, flat, &[(wall, Rgb(255, 160, 0))]);
    picks_tinted(&s, cam, &[]);
    picks_tinted(&s, crate::view3d::inside(&s.model, cam), &[]);
    // Nothing but lines is on the card, and no level's mark.
    let mesh = Mesh::new(&scene, &look).unwrap();
    assert!(mesh.wireframe && mesh.lines);
    assert!(mesh.triangles() == 0 && !mesh.wires.is_empty());
    assert!(mesh.marks(&flat, &[]).is_empty());
    cut_through(&mut s, [0.6, 0.7, 0.45]);
    picks_tinted(&s, flat, &[]);
    // What the view hides has no lines to pick.
    s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"hidden": true}}})).unwrap();
    picks_tinted(&s, flat, &[]);
}

/// A scan of a red face 4 m across standing in the sample house, as a surveyor's points come in: `test` names its file.
fn scanned(s: &mut Session, test: &str) -> Id {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{test}.xyz"));
    let text: String =
        (0..=200).flat_map(|i| (0..=150).map(move |k| format!("{} 0 {} 220 30 30\n", f64::from(i) * 0.02, f64::from(k) * 0.02))).collect();
    std::fs::write(&path, text).unwrap();
    let out = s.execute("point_cloud", &json!({"path": path, "at": [4000, 3000]})).unwrap();
    s.model.elements.iter().rev().find(|(_, e)| e.category() == "pointcloud").map(|(id, _)| *id).unwrap_or_else(|| panic!("{out}"))
}

#[test]
fn picking_the_mesh_with_a_scan_finds_what_the_cpu_picture_shows() {
    let mut s = sample();
    let scan = scanned(&mut s, "picking");
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let flat = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let cam = scene.fit_from(Camera { lens: Some(crate::view3d::LENS), ..flat }, &look);
    picks_through(&s, flat);
    picks_tinted(&s, flat, &[(scan, Rgb(0, 120, 215))]);
    picks_through(&s, cam);
    picks_through(&s, crate::view3d::inside(&s.model, cam));
    // The points are on the card, and the box round the view holds them.
    let mesh = Mesh::new(&scene, &look).unwrap();
    assert_eq!(mesh.dots.len(), 201 * 151);
    let (lo, hi) = scene.scans[0].corners.iter().fold(([f64::MAX; 3], [f64::MIN; 3]), |(lo, hi), p| {
        (std::array::from_fn(|i| lo[i].min(p[i])), std::array::from_fn(|i| hi[i].max(p[i])))
    });
    let far = (0..3).map(|i| (lo[i] - mesh.origin[i]).abs().max((hi[i] - mesh.origin[i]).abs())).fold(0.0, f64::max);
    assert!(far <= mesh.radius + 1.0, "{far} from the middle, of {}", mesh.radius);
    // Hidden line: paper faces, the points in their colours.
    s.execute("visual_style", &json!({"style": "hidden_line"})).unwrap();
    picks_through(&s, flat);
    s.execute("visual_style", &json!({"style": "shaded"})).unwrap();
    // Seen through, in front of the points and behind them.
    s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
    picks_through(&s, flat);
    // Cut to a box, a point being kept or not whole.
    cut_through(&mut s, [0.6, 0.7, 0.45]);
    picks_through(&s, flat);
}

#[test]
fn the_mesh_reaches_a_scan_far_past_the_model_so_a_card_s_depth_holds_it() {
    let mut s = Session::new();
    s.execute("wall", &json!({"points": [[0, 0], [3000, 0]]})).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("far.xyz");
    // Far above: a level's plane reaches a scan across, not up.
    std::fs::write(&path, "0 0 300\n1 0 300\n0 1 300\n0 0 301\n").unwrap();
    s.execute("point_cloud", &json!({"path": path, "origin": "file"})).unwrap();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    assert_eq!(mesh.dots.len(), 4);
    for d in &mesh.dots {
        let off = sub(d.at, mesh.origin);
        assert!(dot(off, off).sqrt() <= mesh.radius + 1.0, "{off:?} from the middle, of {}", mesh.radius);
    }
}

#[test]
fn a_scan_alone_picks_its_points_and_what_the_view_hides_has_none() {
    let mut s = Session::new();
    let scan = scanned(&mut s, "alone");
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    picks_tinted(&s, cam, &[]);
    let mesh = Mesh::new(&scene, &look).unwrap();
    let image = scene.render_in(&cam, &[], &look);
    let on = (0..480)
        .flat_map(|y| (0..640).map(move |x| (f64::from(x) + 0.5, f64::from(y) + 0.5)))
        .find(|&(x, y)| image.pick(x, y) == Some(scan))
        .unwrap();
    assert_eq!(mesh.pick(&cam, &[], on.0, on.1).map(|h| h.id), Some(scan));
    s.execute("graphics", &json!({"view": "{3D}", "categories": {"pointcloud": {"hidden": true}}})).unwrap();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    assert!(Mesh::new(&scene, &look).unwrap().dots.is_empty());
}

#[test]
fn a_wireframe_abandoned_while_its_edges_are_found_makes_no_mesh() {
    let mut s = sample();
    s.execute("visual_style", &json!({"style": "wireframe"})).unwrap();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let asked = Cell::new(0);
    let later = || {
        asked.set(asked.get() + 1);
        asked.get() > 1
    };
    assert!(Mesh::new_cancellable(&scene, &look, &later).unwrap().is_none());
    // What was abandoned isn't kept as the answer.
    assert!(!Mesh::new(&scene, &look).unwrap().wires.is_empty());
}

#[test]
fn what_a_card_does_not_hold_stands_in_however_near_it_is() {
    let s = sample();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    let cam = scene.fit_in(AZIMUTH, ELEVATION, 640, 480, &look);
    let seen = mesh.visible(&cam);
    let all = mesh.cost(&seen);
    assert!(mesh.groups.len() >= 2, "{} groups", mesh.groups.len());
    // The first group alone is held: with room for everything, it is all that is drawn of the triangles of their own.
    let first = mesh.groups[0].range.clone();
    let held = |r: &std::ops::Range<usize>| r.start >= first.start && r.end <= first.end;
    let near = mesh.nearest_first_held(&cam, &seen, all, &held);
    let drawn: usize = near.seen.vertices.iter().map(|r| r.len()).sum();
    assert_eq!(drawn, first.len());
    let own_left = mesh.groups.len() - 1;
    assert!(near.stand_ins.len() >= own_left, "{} boxes", near.stand_ins.len());
    // With all held it is as ever.
    let every = mesh.nearest_first_held(&cam, &seen, all, &|_| true);
    assert_eq!(every.seen, seen);
    assert!(every.stand_ins.is_empty());
}

#[test]
fn a_mesh_holds_a_tower_however_many_triangles_it_has() {
    let m = crate::sample::tower::drawn(crate::sample::tower::Plate::REFERENCE, 2, crate::sample::tower::Drawings::default()).unwrap();
    let scene = Scene::new(&m);
    let mesh = Mesh::new(&scene, &Look::of_3d(&m)).unwrap();
    assert!(mesh.triangles() > 1_600_000, "{} triangles, more than it was once given", mesh.triangles());
}

#[test]
fn a_mesh_draws_every_style_and_what_is_seen_through() {
    let mut s = sample();
    for style in ["wireframe", "hidden_line", "shaded"] {
        s.execute("visual_style", &json!({"style": style})).unwrap();
        assert!(Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).is_ok(), "{style}");
    }
    s.execute("graphics", &json!({"view": "{3D}", "categories": {"walls": {"transparency": 50}}})).unwrap();
    assert!(Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).is_ok(), "the card draws what is seen through");
}

#[test]
fn vertices_are_near_the_origin_whatever_the_model_is_far_from_it() {
    let s = sample();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let mesh = Mesh::new(&scene, &look).unwrap();
    assert_eq!(mesh.vertices.len() % 3, 0);
    let reach = mesh.radius as f32 * 1.001 + 1.0;
    assert!(mesh.vertices.iter().all(|v| v.position.iter().all(|c| c.abs() <= reach)));
    assert!(mesh.vertices.iter().all(|v| v.owner >= 1 && v.owner as usize <= mesh.owners.len() && v.plane >= 1));
    // Only the glass of the windows is seen through, and it is drawn last.
    let clear: Vec<_> = mesh.owners.iter().filter(|o| o.transparency > 0).collect();
    assert!(!clear.is_empty() && clear.iter().all(|o| s.model.elements.get(o.id).is_some_and(|e| e.category() == "window")));
    assert!(mesh.clear_from < mesh.vertices.len());
}

#[test]
fn a_glass_layer_is_an_owner_of_its_own_with_the_materials_transparency() {
    let mut s = Session::new();
    s.script(
        r#"type {"category": "wall", "name": "Curtain", "layers": [{"material": "Glass", "thickness": 50}, {"material": "Concrete block", "thickness": 100}]}
           wall {"points": [[0,0],[8000,0]], "type": "Curtain"}"#,
    )
    .unwrap();
    let mesh = Mesh::new(&Scene::new(&s.model), &Look::of_3d(&s.model)).unwrap();
    let mut alphas: Vec<u8> = mesh.owners.iter().map(|o| o.transparency).collect();
    alphas.sort_unstable();
    assert_eq!(alphas, vec![0, crate::model::GLASS_TRANSPARENCY], "one wall, drawn in two parts");
    assert!(mesh.clear_from > 0 && mesh.clear_from < mesh.vertices.len(), "the opaque layer first, then the glass");
}

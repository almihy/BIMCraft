//! `view3d`'s tests that drive the Session (moved out of `view3d.rs`).

use crate::camera::{AZIMUTH, DisplayOptions, ELEVATION, FILM, LENS};
use crate::geom::v3::{P3, add, cross, dot, mul, sub, unit};
use crate::geom::{self, P2};
use crate::graphics::Look;
use crate::model::{Element, Id, Model};
use crate::prim::{PAPER, Rgb};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::view3d::*;

#[test]
fn a_sections_scene_holds_what_can_show_and_its_picture_is_the_whole_scenes() {
    use crate::sample::tower::{Drawings, Plate, drawn};
    let m = drawn(Plate::SMALL, 3, Drawings { annotation: false, sheets: true }).unwrap();
    let whole = Scene::new(&m);
    let (solids, placed, boxes) = solids_and_extremes(&m);
    let solids = PreparedSolids::Owned(solids);
    let mut ranges: HashMap<Id, std::ops::Range<usize>> = HashMap::new();
    for (i, s) in solids.iter().enumerate() {
        ranges.entry(s.owner()).and_modify(|r| r.end = i + 1).or_insert(i..i + 1);
    }
    for name in ["North Elevation", "South Elevation", "East Elevation", "West Elevation", "Section A"] {
        let id = m
            .elements
            .iter()
            .find_map(|(id, e)| matches!(e, Element::Section(v) | Element::Elevation(v) if v.name == name).then_some(*id))
            .unwrap();
        let view = crate::render::section_fit(&m, id, 900, 600);
        let (cam, clip) = crate::render::section_camera(&m, id, view).unwrap();
        let look = Look::of(&m, id);
        let lean = Scene::section(&m, (&solids, &ranges, &placed, &boxes), &cam, &clip, &look);
        lean.cut_in(&cam, &clip, &[], &look).same_as(&whole.cut_in(&cam, &clip, &[], &look)).unwrap();
        // The furniture behind the facade, and the rooms' services, are left out.
        assert!(lean.tris.len() * 4 < whole.tris.len(), "{name}: {} of {} triangles", lean.tris.len(), whole.tris.len());
    }
}

#[test]
fn background_coverage_includes_geometry_without_a_named_face() {
    let image = Image {
        width: 2,
        height: 1,
        rgba: vec![255; 8],
        hit: Arc::new(vec![0, u32::MAX]),
        depth: Arc::new(vec![1.0, f32::INFINITY]),
        owners: Arc::new(vec![Id::MAX]),
        faces: Arc::new(vec![""]),
        section_paint: None,
    };
    assert!(image.covered(0.0, 0.0));
    assert_eq!(image.face(0.0, 0.0), None);
    assert_eq!(image.pick(0.0, 0.0), None, "linked geometry is covered even though it cannot be picked");
    assert!(!image.covered(1.0, 0.0));
    assert!(!image.covered(-1.0, 0.0));
    assert!(!image.covered(2.0, 0.0));
}

#[test]
fn a_picture_draws_every_point_up_to_its_budget_and_an_even_share_past_it() {
    assert_eq!((every(0), every(1), every(DRAWN)), (1, 1, 1));
    assert_eq!((every(DRAWN + 1), every(2 * DRAWN), every(5 * DRAWN - 1)), (2, 2, 5));
    assert!((1..=6).all(|k| (k * DRAWN + 7).div_ceil(every(k * DRAWN + 7)) <= DRAWN));
}

#[test]
fn parallel_geometry_keeps_exact_triangle_order_and_ownership() {
    let m = crate::sample::highrise(3).unwrap();
    assert!(m.elements.len() >= 1000);
    let elements: Vec<_> = m.elements.iter().map(|(&id, e)| (id, e)).collect();
    let triangles = |solids: Vec<Solid>| {
        let mut tris = Vec::new();
        for solid in solids {
            solid.tris(&mut tris);
        }
        tris
    };
    assert_eq!(triangles(solids(&m)), triangles(solids_in(&m, &elements, None)));
}

#[test]
fn tiled_raster_matches_serial_depth_picking_and_lighting_exactly() {
    let mut s = crate::engine::Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let scene = Scene::new(&s.model);
    for (w, h) in [(1, 1), (513, 257), (800, 600)] {
        let cam = scene.fit(AZIMUTH, ELEVATION, w as u32, h as u32);
        let clip = Clip { near: 0.0, far: Some(5000.0), x: (10.0, w as f64 - 10.0), y: (10.0, h as f64 - 10.0) };
        for clipping in [None, Some(&clip)] {
            for transparent in [false, true] {
                let front = vec![-1000.0; w * h];
                let front = transparent.then_some(front.as_slice());
                let buffers = || (vec![f32::NEG_INFINITY; w * h], vec![u32::MAX; w * h], vec![0.0; w * h]);
                let (mut d, mut hit, mut l) = buffers();
                let (mut rd, mut rh, mut rl) = buffers();
                let keep = |t: &Tri| !t.owner.is_multiple_of(3);
                let light = |i| Some([0.65 + (i % 7) as f64 * 0.01, 0.75, 0.95]);
                scene.raster_reference((&cam, clipping, None), (w, h), keep, front, &light, (&mut rd, &mut rh, &mut rl));
                scene.raster((&cam, clipping, None), (w, h), keep, front, &light, (&mut d, &mut hit, &mut l));
                assert_eq!(d, rd, "depth {w}x{h}");
                assert_eq!(hit, rh, "picking {w}x{h}");
                assert_eq!(l, rl, "lighting {w}x{h}");
            }
        }
    }
}

#[test]
fn coplanar_triangles_share_a_plane_and_degenerate_ones_each_their_own() {
    let tri = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, tex: 0, curve: None };
    // A face's triangles a micron off its plane, as a file rounds them, are one plane;
    // triangles with no area are each their own, and many of them stay quick.
    let mut tris: Vec<Tri> = (0..200)
        .map(|k| {
            let (x, e) = (f64::from(k) * 100.0, f64::from(k % 7) * 1e-4);
            tri([[x, 0.0, 1000.0 + e], [x + 100.0, 0.0, 1000.0], [x, 100.0, 1000.0 - e]])
        })
        .collect();
    tris.extend((0..20_000).map(|k| tri([[f64::from(k), 0.0, 0.0]; 3])));
    let started = std::time::Instant::now();
    let ids = planes(&tris);
    assert!(started.elapsed().as_secs_f64() < 10.0, "{:?}: a hang, not a slow machine", started.elapsed());
    assert!(ids.iter().take(200).all(|id| *id == ids[0]), "one plane");
    let degenerate: std::collections::BTreeSet<usize> = ids.iter().skip(200).copied().collect();
    assert_eq!(degenerate.len(), 20_000);
}

/// A cylinder of `n` sides standing on the origin, turned out: its `n` top triangles, its `n`
/// bottom ones, then its `2n` sides. The last corner of a ring is a hair off the first, as a
/// file's rounding leaves it. Four sides make a box.
fn cylinder(n: usize, r: f64, h: f64) -> Vec<[P3; 3]> {
    let ring = |z: f64, k: usize| {
        let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
        [r * c, r * s, z]
    };
    let (mut top, mut bottom, mut sides) = (Vec::new(), Vec::new(), Vec::new());
    for k in 0..n {
        let (a, b, c, d) = (ring(0.0, k), ring(0.0, k + 1), ring(h, k), ring(h, k + 1));
        top.push([[0.0, 0.0, h], c, d]);
        bottom.push([[0.0, 0.0, 0.0], b, a]);
        sides.extend([[a, b, d], [a, d, c]]);
    }
    [top, bottom, sides].concat()
}

#[test]
fn a_meshs_facets_that_turn_little_read_as_one_surface_and_its_creases_keep_their_edges() {
    let mesh = |owner, tris: Vec<[P3; 3]>| Solid::Mesh { owner, color: EDGE, tex: 0, caps: tris.len(), round: false, cut: false, tris };
    // Each cylinder's planes: its sides one surface where they turn 20° or less at their edges,
    // else a plane to each side; and a plane to each cap, however many triangles make it.
    for (n, planes_of) in [(4, 6), (8, 10), (16, 18), (20, 3), (24, 3), (64, 3)] {
        let mut out = Vec::new();
        mesh(1, cylinder(n, 200.0, 450.0)).tris(&mut out);
        let ids = planes(&out);
        let (top, bottom, sides) = (&ids[..n], &ids[n..2 * n], &ids[2 * n..]);
        assert!(top.iter().all(|id| *id == top[0]) && bottom.iter().all(|id| *id == bottom[0]), "{n} sides: a cap is one plane");
        assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), planes_of, "{n} sides: {ids:?}");
        assert!(top[0] != bottom[0] && !sides.contains(&top[0]) && !sides.contains(&bottom[0]), "{n} sides: the caps keep their edges");
    }
    // Two forms of one element, one on the other, each its own surface where their facets touch.
    let n = 24;
    let mut out = Vec::new();
    for _ in 0..2 {
        mesh(1, cylinder(n, 200.0, 450.0)).tris(&mut out);
    }
    let ids = planes(&out);
    assert_ne!(ids[2 * n], ids[6 * n], "{ids:?}");
    // A mesh as big as a file may bring stays quick.
    let started = std::time::Instant::now();
    let mut out = Vec::new();
    mesh(1, cylinder(25_000, 200.0, 450.0)).tris(&mut out);
    let (ids, normals) = (planes(&out), corner_normals(&out));
    assert!(started.elapsed().as_secs_f64() < 10.0, "{:?}: a hang, not a slow machine", started.elapsed());
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), 3);
    assert_eq!(normals.iter().flatten().count(), 50_000, "its sides, and not its caps");
    // Corners that aren't numbers, and triangles with no area, join nothing, and don't crash it.
    let odd = [[[f64::NAN; 3]; 3], [[f64::INFINITY; 3]; 3], [[1e300, 0.0, 0.0], [-1e300, 1e300, 0.0], [0.0; 3]], [[0.0; 3]; 3]];
    assert_eq!(facets(&odd), vec![None; 4]);
}

use crate::engine::Session;

/// The 8 x 6 m box of 200 mm walls on Level 1 (walls 3 south, 4 east, 5 north, 6 west), a door
/// (7) in the south wall, a window (8) in the east one, and a floor (9).
fn house() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0]}
           window {"at": [8000, 3000]}
           floor {}"#,
    )
    .unwrap();
    s
}

fn view(s: &Session) -> (Camera, Image) {
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
    let img = scene.render(&cam, &[]);
    (cam, img)
}

fn at(cam: &Camera, p: P3) -> (f64, f64) {
    let (x, y, _) = cam.project(p);
    (x, y)
}

#[test]
fn each_pixel_shows_the_nearest_surface_and_the_element_it_belongs_to() {
    let s = house();
    let (cam, img) = view(&s);
    let pick = |p: P3| {
        let (x, y) = at(&cam, p);
        img.pick(x, y)
    };
    assert_eq!(pick([5000.0, -100.0, 1500.0]), Some(100002), "the south wall's outer face");
    assert_eq!(pick([2000.0, -25.0, 1000.0]), Some(100006), "the door, seen through its opening");
    assert_eq!(pick([8025.0, 3000.0, 1650.0]), Some(100007), "the window's glass");
    assert_eq!(pick([2000.0, 4500.0, 0.0]), Some(100008), "the floor, seen over the walls");
    assert_eq!(pick([4000.0, 1000.0, 0.0]), Some(100002), "the floor behind the south wall: the wall hides it");
    assert_eq!(img.pick(1.0, 1.0), None, "the background");
}

#[test]
fn faces_shade_evenly_and_edges_mark_where_they_meet() {
    let s = house();
    let (cam, img) = view(&s);
    let color = |p: P3| {
        let (x, y) = at(&cam, p);
        img.color(x, y)
    };
    // Along the south wall's face past the door: one shade, no seams from the triangles under it.
    let row: Vec<Rgb> = (0..20).map(|i| color([3000.0 + 140.0 * f64::from(i), -100.0, 1800.0])).collect();
    assert!(row.iter().all(|c| *c == row[0]) && row[0] != EDGE, "{row:?}");
    // Over the corner where the south and east walls overlap, their tops make one face: zoomed in
    // on it, across it, away from its outer edges.
    let close = cam.zoom(4.0, at(&cam, [8000.0, 0.0, 3000.0]));
    let img4 = Scene::new(&s.model).render(&close, &[]);
    let top: Vec<Rgb> = (0..9)
        .map(|i| {
            let (x, y) = at(&close, [7860.0 + 20.0 * f64::from(i), 40.0, 3000.0]);
            img4.color(x, y)
        })
        .collect();
    assert!(top.iter().all(|c| *c == top[0]) && top[0] != EDGE, "{top:?}");
    // Where the south wall's top meets its face, an edge.
    let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
    let near: Vec<Rgb> = [-1.0, 0.0, 1.0].iter().flat_map(|dx| [-1.0, 0.0, 1.0].map(|dy| img.color(x + dx, y + dy))).collect();
    assert!(near.contains(&EDGE), "{near:?}");
}

/// A session whose one component is a post: a mesh form, a cylinder of 24 sides, 400 mm across and a metre tall.
fn post() -> Session {
    let mut s = Session::new();
    let post = serde_json::json!({"name": "Post", "types": {"A": {}}, "forms": [{"mesh": cylinder(24, 200.0, 1000.0)}]});
    s.execute("family", &post).unwrap();
    s.execute("component", &serde_json::json!({"family": "Post", "at": [0, 0]})).unwrap();
    s
}

/// The most any channel moves between neighbours along `colors`.
fn step(colors: &[Rgb]) -> u8 {
    colors.windows(2).map(|w| w[0].0.abs_diff(w[1].0).max(w[0].1.abs_diff(w[1].1)).max(w[0].2.abs_diff(w[1].2))).max().unwrap_or(0)
}

#[test]
fn a_curves_corners_take_the_normals_of_the_surface_they_are_on() {
    let n = 24;
    let mut out = Vec::new();
    Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: cylinder(n, 200.0, 450.0) }.tris(&mut out);
    let normals = corner_normals(&out);
    // Its caps are flat faces: none. Each corner of its sides points straight out from the axis, the top
    // ones as the bottom ones (two triangles meet at some corners, one at others).
    assert!(normals[..2 * n].iter().all(Option::is_none));
    for (t, ns) in out[2 * n..].iter().zip(&normals[2 * n..]) {
        for (p, v) in t.p.iter().zip(ns.as_deref().unwrap()) {
            assert!(dot(*v, unit([p[0], p[1], 0.0])) > 1.0 - 1e-9, "{p:?} {v:?}");
        }
    }
    // A cone on it, a surface of its own: the ring they share keeps a step, the sides' corners
    // level and the cone's tilted up, its apex straight up.
    let ring = |k: usize| {
        let (s, c) = (k as f64 * std::f64::consts::TAU / n as f64).sin_cos();
        [200.0 * c, 200.0 * s, 450.0]
    };
    let mut roofed = cylinder(n, 200.0, 450.0);
    roofed.extend((0..n).map(|k| [ring(k), ring(k + 1), [0.0, 0.0, 850.0]]));
    let mut out = Vec::new();
    Solid::Mesh { owner: 1, color: EDGE, tex: 0, caps: 0, round: false, cut: false, tris: roofed }.tris(&mut out);
    let normals = corner_normals(&out);
    let normal = |i: usize| normals[i].as_deref().unwrap();
    assert!((2 * n..4 * n).all(|i| normal(i).iter().all(|v| v[2].abs() < 1e-9)));
    assert!((4 * n..5 * n).all(|i| normal(i)[0][2] > 0.4 && normal(i)[1][2] > 0.4 && normal(i)[2][2] > 1.0 - 1e-9));
    // Triangles with corners that aren't numbers, or no area, have none, and don't crash it.
    let odd = |p: [P3; 3]| Tri { p, owner: 1, color: EDGE, tex: 0, curve: Some((1, 0)) };
    let odd = [
        odd([[f64::NAN; 3]; 3]),
        odd([[f64::INFINITY; 3]; 3]),
        odd([[0.0; 3]; 3]),
        odd([[1e300, 0.0, 0.0], [-1e300, 1e300, 0.0], [0.0; 3]]),
    ];
    assert!(corner_normals(&odd).iter().all(Option::is_none));
}

#[test]
fn a_curve_shades_smoothly_across_its_facets_and_a_flat_face_in_one_shade() {
    let s = post();
    let mut scene = Scene::new(&s.model);
    // Its shading, not the levels' planes.
    scene.levels.clear();
    let cam = scene.fit(AZIMUTH, ELEVATION, 800, 600);
    let img = scene.render(&cam, &[]);
    // Across its side, edge to edge: no step between neighbours bigger than a level, though it does change.
    let (x, y) = at(&cam, [0.0, 0.0, 500.0]);
    let reach = (190.0 * cam.scale) as i32;
    let across: Vec<Rgb> = (-reach..=reach).map(|dx| img.color(x.floor() + f64::from(dx) + 0.5, y)).collect();
    let (first, last) = (across[0], across[across.len() - 1]);
    assert!(step(&across) <= 1 && step(&[first, last]) > 15, "{} px, a step of {}, {first:?} to {last:?}", across.len(), step(&across));
    // Across its top, one shade.
    let (x, y) = at(&cam, [0.0, 0.0, 1000.0]);
    let top: Vec<Rgb> = (-40..=40).map(|dx| img.color(x.floor() + f64::from(dx) + 0.5, y)).collect();
    assert!(top.iter().all(|c| *c == top[0]), "{top:?}");
}

#[test]
fn a_curve_shades_as_its_own_facets_would_to_within_their_turn_whichever_side_is_seen() {
    // An open tube: the far wall of it is seen from inside, its normals turned away.
    let mut s = Session::new();
    let tube = serde_json::json!({"name": "Tube", "types": {"A": {}}, "forms": [{"mesh": cylinder(24, 200.0, 1000.0)[48..]}]});
    s.execute("family", &tube).unwrap();
    s.execute("component", &serde_json::json!({"family": "Tube", "at": [0, 0]})).unwrap();
    let (smooth, mut flat) = (Scene::new(&s.model), Scene::new(&s.model));
    flat.smooth.clear();
    let cam = smooth.fit(AZIMUTH, ELEVATION, 800, 600);
    let (a, b) = (smooth.render(&cam, &[]), flat.render(&cam, &[]));
    let apart: Vec<u8> = a.rgba.iter().zip(&b.rgba).map(|(x, y)| x.abs_diff(*y)).collect();
    assert!(apart.iter().max().is_some_and(|d| (1..=6).contains(d)), "{:?}", apart.iter().max());
}

#[test]
fn a_curved_wall_shades_smoothly_along_its_face() {
    let mut s = Session::new();
    s.script(r#"wall {"points": [[0, 0], [4000, 0]], "arc": 180}"#).unwrap();
    let mut scene = Scene::new(&s.model);
    scene.levels.clear();
    let cam = scene.fit(AZIMUTH, ELEVATION, 800, 600);
    let img = scene.render(&cam, &[]);
    // Along its face at mid-height, between its ends: the row's pixels that show the wall.
    let (_, y) = at(&cam, [2000.0, -2000.0, 1500.0]);
    let shown: Vec<i32> = (0..800).filter(|px| img.pick(f64::from(*px) + 0.5, y.floor() + 0.5).is_some()).collect();
    let (lo, hi) = (shown.iter().min().unwrap() + 6, shown.iter().max().unwrap() - 6);
    let along: Vec<Rgb> = (lo..=hi).map(|px| img.color(f64::from(px) + 0.5, y.floor() + 0.5)).collect();
    let (first, last) = (along[0], along[along.len() - 1]);
    assert!(
        along.len() > 200 && step(&along) <= 1 && step(&[first, last]) > 8,
        "{} px, a step of {}, {first:?} to {last:?}",
        along.len(),
        step(&along)
    );
}

#[test]
fn an_imported_curve_shows_no_facet_lines_and_keeps_the_edge_of_its_rim() {
    let s = post();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 800, 600);
    let img = scene.render(&cam, &[]);
    // Across its side at mid-height, every pixel from one edge of it to the other.
    let (x, y) = at(&cam, [0.0, 0.0, 500.0]);
    let reach = (190.0 * cam.scale) as i32;
    let across: Vec<Rgb> = (-reach..=reach).map(|dx| img.color(x.floor() + f64::from(dx) + 0.5, y)).collect();
    assert!(!across.contains(&EDGE), "{across:?}");
    // Where its top meets its side, nearest the camera, an edge.
    let (x, y) = at(&cam, [141.0, -141.0, 1000.0]);
    let rim: Vec<Rgb> = (-3..=3).map(|dy| img.color(x, y + f64::from(dy))).collect();
    assert!(rim.contains(&EDGE), "{rim:?}");
}

#[test]
fn a_family_door_and_window_show_their_forms_in_the_wall_and_mirror_with_their_flips() {
    let mut s = house();
    s.script(
        r#"delete {"ids": [100006, 100007]}
           door {"at": [2000, 0], "family": "Single Door", "type": "800 x 2100"}
           window {"at": [8000, 3000], "family": "Fixed Window", "type": "600 x 900"}"#,
    )
    .unwrap();
    // The door and the window are 10 and 11 now (9 is the floor).
    let (door, window) = (100009, 100010);
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 1600, 1200);
    let pick_at = |cam: &Camera, img: &Image, p: P3| {
        let (x, y) = at(cam, p);
        img.pick(x, y)
    };
    let near = |p: P3| {
        let close = cam.zoom(6.0, at(&cam, p));
        let img = scene.render(&close, &[]);
        pick_at(&close, &img, p)
    };
    // Seen through the opening: the leaf's face (40 thick, in the middle of the wall), a jamb of the frame, a handle.
    assert_eq!(near([2000.0, -20.0, 1000.0]), Some(door), "the leaf");
    assert_eq!(near([1625.0, -100.0, 1000.0]), Some(door), "the jamb");
    assert_eq!(near([2250.0, -75.0, 1015.0]), Some(door), "the handle, 55 proud of the leaf");
    // The window's glass is 6 thick, in the middle of its wall, and not what a classic window's 50 mm panel is.
    assert_eq!(near([8003.0, 3000.0, 1550.0]), Some(window), "the glass");
    assert_eq!(near([5000.0, -100.0, 1500.0]), Some(100002), "the wall beside them");
    // Its hinge side's jamb is still there, on the other jamb's side, once flipped.
    run_flip(&mut s);
    let scene = Scene::new(&s.model);
    let close = cam.zoom(6.0, at(&cam, [2000.0, -20.0, 1000.0]));
    let img = scene.render(&close, &[]);
    // The handle moved to the other side of the leaf: where it was is only the leaf's face now.
    assert_eq!(pick_at(&close, &img, [1750.0, -75.0, 1015.0]), Some(door), "the handle on the other side");
}

fn run_flip(s: &mut Session) {
    s.execute("modify", &serde_json::json!({"id": 100009, "flip_hand": true})).unwrap();
}

/// The house with its walls of two layers: brick on the left of each, and a block to its right.
fn brick_house() -> Session {
    let mut s = house();
    s.script(
        r#"type {"category": "wall", "name": "Brick wall", "layers": [{"function": "finish", "material": "Brick", "thickness": 100}, {"function": "structure", "material": "Concrete block", "thickness": 100}]}
           modify {"ids": [100002, 100003, 100004, 100005], "type": "Brick wall"}"#,
    )
    .unwrap();
    s
}

#[test]
fn a_glass_layer_lets_what_is_behind_a_wall_show_through_in_a_plain_view() {
    let build = |layer: &str, columns: bool| {
        let mut s = Session::new();
        s.script(&format!(
            r#"type {{"category": "wall", "name": "Curtain", "layers": [{{"function": "structure", "material": "{layer}", "thickness": 100}}]}}
               wall {{"points": [[0,0],[8000,0]], "type": "Curtain"}}"#
        ))
        .unwrap();
        if columns {
            s.script("column {\"at\": [3500, -900]}\ncolumn {\"at\": [3500, 900]}").unwrap();
        }
        s
    };
    let (glass, plain_wall) = (build("Glass", true), build("Concrete block", true));
    // One camera for all four, so the pictures line up.
    let cam = Scene::new(&glass.model).fit(AZIMUTH, ELEVATION, 400, 300);
    let render = |s: &Session| {
        let img = Scene::new(&s.model).render(&cam, &[]);
        let wall = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Wall(_))).map(|(id, _)| *id);
        (img, wall)
    };
    let (g, gw) = render(&glass);
    let (o, ow) = render(&plain_wall);
    let (g0, _) = render(&build("Glass", false));
    let (o0, _) = render(&build("Concrete block", false));
    let covered = |img: &Image, wall| (0..400 * 300).filter(|i| img.pick((i % 400) as f64, (i / 400) as f64) == wall).collect::<Vec<_>>();
    let differs =
        |a: &Image, b: &Image, px: &[usize]| px.iter().filter(|i| a.rgba[**i * 4..**i * 4 + 3] != b.rgba[**i * 4..**i * 4 + 3]).count();
    let (gc, oc) = (covered(&g, gw), covered(&o, ow));
    assert!(gc.len() > 300 && oc.len() > 300);
    assert!(differs(&g, &g0, &gc) > 50, "a column behind the glass shows through it");
    let (seen, not) = (differs(&g, &g0, &gc), differs(&o, &o0, &oc));
    assert!(not * 10 < seen, "and not through an opaque wall: {not} against {seen} pixels");
}

/// A height in the middle of a brick course (each 75 mm), up a wall.
const COURSE: f64 = 1537.5;

/// The view of `s` from the default angle, a picture 1600 by 1200 centred on `target`, `scale` px to the mm.
fn closeup(s: &Session, target: P3, scale: f64) -> (Scene, Camera) {
    let scene = Scene::new(&s.model);
    let cam = Camera { target, scale, ..scene.fit(AZIMUTH, ELEVATION, 1600, 1200) };
    (scene, cam)
}

/// The colours along the north wall's face toward the house (its brick), from `x0` to `x1` mm at 1537.5 up (the middle of a course: 1500 is
/// a joint), seen close up.
fn along_the_brick(s: &Session, x0: f64, x1: f64, n: i32) -> Vec<Rgb> {
    let (scene, close) = closeup(s, [4000.0, 5900.0, COURSE], 0.8);
    let img = scene.render(&close, &[]);
    (0..n)
        .map(|i| {
            let (x, y) = at(&close, [x0 + (x1 - x0) * f64::from(i) / f64::from(n - 1), 5900.0, COURSE]);
            img.color(x, y)
        })
        .collect()
}

#[test]
fn a_textured_material_varies_over_its_faces_at_its_real_size_and_none_is_one_shade() {
    let mut s = brick_house();
    let row = along_the_brick(&s, 3200.0, 4800.0, 120);
    let distinct: BTreeSet<(u8, u8, u8)> = row.iter().map(|c| (c.0, c.1, c.2)).collect();
    assert!(distinct.len() > 8, "bricks and joints: {} colours", distinct.len());
    assert!(row.iter().all(|c| c.0 > c.2), "brick red and the mortar's cream, never blue: {:?}", &row[..4]);
    // Its tile is 900 mm across: what's at one point is at the next, a tile along, within a level or two.
    let (a, b) = (along_the_brick(&s, 3300.0, 3300.0 + 1e-9, 2), along_the_brick(&s, 4200.0, 4200.0 + 1e-9, 2));
    let near = |p: Rgb, q: Rgb| p.0.abs_diff(q.0) <= 4 && p.1.abs_diff(q.1) <= 4 && p.2.abs_diff(q.2) <= 4;
    assert!(near(a[0], b[0]), "{:?} {:?}", a[0], b[0]);
    // A size of half that, and the pattern repeats twice as often: now a half tile along is the same.
    s.execute("material", &serde_json::json!({"name": "Brick", "texture": {"size": [450, 300]}})).unwrap();
    let (c, d) = (along_the_brick(&s, 3300.0, 3300.0 + 1e-9, 2), along_the_brick(&s, 3750.0, 3750.0 + 1e-9, 2));
    assert!(near(c[0], d[0]), "{:?} {:?}", c[0], d[0]);
    // Without its texture, one shade along the whole face, as before.
    s.execute("material", &serde_json::json!({"name": "Brick", "texture": null})).unwrap();
    let plain = along_the_brick(&s, 3200.0, 4800.0, 120);
    assert!(plain.iter().all(|c| *c == plain[0]), "{:?}", &plain[..4]);
    // The same wall in a section is paper and line, as it was: a texture is for the shaded view.
    s.execute("material", &serde_json::json!({"name": "Brick", "texture": {"kind": "brick"}})).unwrap();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
    let clip = Clip { near: f64::MAX, far: None, x: (0.0, 400.0), y: (0.0, 300.0) };
    let cut = scene.cut(&cam, &clip, &[]);
    assert!(cut.rgba.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]), "grey, never a colour");
}

/// A PNG file whose left half is red and whose right half is blue, in the scratch folder.
fn halves(name: &str) -> String {
    let (w, h) = (64u32, 8u32);
    let rgba: Vec<u8> = (0..w * h).flat_map(|k| if k % w < w / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] }).collect();
    let png = tiny_skia::Pixmap::from_vec(rgba, tiny_skia::IntSize::from_wh(w, h).unwrap()).unwrap().encode_png().unwrap();
    let path = std::env::temp_dir().join(format!("bimcraft-{name}-{}.png", std::process::id()));
    std::fs::write(&path, png).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn a_picture_made_a_texture_lies_on_the_floor_as_laid_and_paint_gives_a_face_its_own_material() {
    let mut s = house();
    let path = halves("floor");
    // A floor of a material textured with the picture: a tile of 1000 by 1000 mm, red where x is under 500 from a metre's mark.
    s.script(&format!(
        r##"material {{"name": "Stripes", "color": "#808080", "texture": {{"kind": "image", "path": {path:?}, "size": [1000, 1000]}}}}
           type {{"category": "floor", "name": "Striped", "layers": [{{"material": "Stripes", "thickness": 200}}]}}
           delete {{"ids": [100008]}}
           floor {{"type": "Striped"}}"##
    ))
    .unwrap();
    // Seen from over the house's south-east, a floor point is hidden by the south wall unless it's over 3.7 m north of it, and by
    // the east wall unless it's over 3.7 m west of it: these, 4500 north and 1250 to 3750 east, aren't.
    let (scene, close) = closeup(&s, [2500.0, 4500.0, 0.0], 0.4);
    let img = scene.render(&close, &[]);
    let at_floor = |x: f64| {
        let (px, py) = at(&close, [x, 4500.0, 0.0]);
        img.color(px, py)
    };
    let (red, blue) = (|c: Rgb| i32::from(c.0) - i32::from(c.2), |c: Rgb| i32::from(c.2) - i32::from(c.0));
    for x in [1250.0, 2250.0, 3250.0] {
        let c = at_floor(x);
        assert!(red(c) > 80, "{x}: red, {c:?}");
    }
    for x in [1750.0, 2750.0, 3750.0] {
        let c = at_floor(x);
        assert!(blue(c) > 80, "{x}: blue, {c:?}");
    }
    // Moved by 500 mm, the halves swap; the picture itself is the project's, once.
    s.execute("material", &serde_json::json!({"name": "Stripes", "texture": {"offset": [500, 0]}})).unwrap();
    let (scene, close) = closeup(&s, [2500.0, 4500.0, 0.0], 0.4);
    let img = scene.render(&close, &[]);
    let (px, py) = at(&close, [3250.0, 4500.0, 0.0]);
    let c = img.color(px, py);
    assert!(blue(c) > 80, "blue now: {c:?}");
    assert_eq!(s.model.images.len(), 1);
    std::fs::remove_file(path).unwrap();
    // Paint: the face of a wall toward the house (the exterior of these walls, drawn the way round they are) in a material
    // of its own; the face away from it, the other.
    let mut s = brick_house();
    s.execute("paint", &serde_json::json!({"ids": [100004], "face": "exterior", "material": "Timber"})).unwrap();
    let (scene, close) = closeup(&s, [4000.0, 5900.0, COURSE], 0.8);
    let img = scene.render(&close, &[]);
    let (x, y) = at(&close, [4000.0, 5900.0, COURSE]);
    assert_eq!((img.pick(x, y), img.face(x, y)), (Some(100004), Some("exterior")));
    let row: Vec<Rgb> = (0..60).map(|i| img.color(at(&close, [3200.0 + 25.0 * f64::from(i), 5900.0, COURSE]).0, y)).collect();
    // Timber's tan, in boards (a gap between two, a joint along one, darker): where brick was red.
    let tan = row.iter().filter(|c| i32::from(c.0) - i32::from(c.2) > 30 && u32::from(c.1) * 100 > u32::from(c.0) * 70).count();
    assert!(tan >= 50 && row.iter().all(|c| c.0 >= c.2), "{tan} of 60 tan: {:?}", &row[..3]);
    // The outer face of the south wall is on its interior (a wall's exterior is to the left of its start to its end), and the
    // floor and the sky have faces of their own or none.
    let far = scene.fit(AZIMUTH, ELEVATION, 1600, 1200);
    let seen = scene.render(&far, &[]);
    let (fx, fy) = at(&far, [4000.0, -100.0, 1500.0]);
    assert_eq!((seen.pick(fx, fy), seen.face(fx, fy)), (Some(100002), Some("interior")));
    assert_eq!(seen.face(1.0, 1.0), None, "the sky");
    let top = at(&far, [4000.0, 4500.0, 0.0]);
    assert_eq!((seen.pick(top.0, top.1), seen.face(top.0, top.1)), (Some(100008), Some("top")), "the floor's");
}

#[test]
fn paint_changes_only_the_face_it_names() {
    let mut s = brick_house();
    let before = Scene::new(&s.model);
    s.execute("paint", &serde_json::json!({"ids": [100004], "face": "exterior", "material": "Timber"})).unwrap();
    let after = Scene::new(&s.model);
    let timber = s.model.materials["Timber"].color;
    assert_eq!(before.tris.len(), after.tris.len());
    let (mut painted, mut kept) = (0, 0);
    for (i, (a, b)) in before.tris.iter().zip(&after.tris).enumerate() {
        if a.owner == 100004 && after.faces[i] == "exterior" {
            assert_eq!(b.color, Rgb(timber[0], timber[1], timber[2]), "triangle {i}");
            painted += 1;
        } else {
            assert_eq!((a.color, a.tex), (b.color, b.tex), "triangle {i}: the {:?} face of {} keeps what it was", after.faces[i], a.owner);
            kept += usize::from(a.owner == 100004);
        }
    }
    assert!(painted > 0 && kept > 0, "{painted} painted, {kept} left of the wall's others");
}

#[test]
fn a_texture_on_a_wall_runs_across_to_the_right_of_someone_looking_at_its_face_from_outside() {
    let mut s = house();
    let path = halves("wall");
    // Red where x is under 500 from a metre's mark, laid on the south wall's outer face: its face looks south, so across is east.
    s.script(&format!(
        r##"material {{"name": "Stripes", "color": "#808080", "texture": {{"kind": "image", "path": {path:?}, "size": [1000, 1000]}}}}
           paint {{"ids": [100002], "face": "interior", "material": "Stripes"}}"##
    ))
    .unwrap();
    let scene = Scene::new(&s.model);
    let far = scene.fit(AZIMUTH, ELEVATION, 1600, 1200);
    let img = scene.render(&far, &[]);
    let colour = |x: f64| {
        let (px, py) = at(&far, [x, -100.0, 1500.0]);
        let c = img.color(px, py);
        i32::from(c.0) - i32::from(c.2)
    };
    for x in [4250.0, 5250.0, 6250.0] {
        assert!(colour(x) > 80, "{x}: red");
    }
    for x in [4750.0, 5750.0, 6750.0] {
        assert!(colour(x) < -80, "{x}: blue");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn tinted_elements_show_in_their_tint() {
    let s = house();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
    let (x, y) = at(&cam, [5000.0, -100.0, 1500.0]);
    let plain = scene.render(&cam, &[]).color(x, y);
    let tinted = scene.render(&cam, &[(100002, Rgb(37, 99, 235))]).color(x, y);
    assert!(plain.2 <= plain.0 && tinted.2 > tinted.0 + 40, "{plain:?} {tinted:?}");
}

#[test]
fn the_camera_fits_the_model_and_orbits_pans_and_zooms() {
    let s = house();
    let cam = Scene::new(&s.model).fit(AZIMUTH, ELEVATION, 400, 300);
    for (x, y) in [(0.0, 0.0), (8000.0, 0.0), (8000.0, 6000.0), (0.0, 6000.0)] {
        for z in [0.0, 3000.0] {
            let (px, py) = at(&cam, [x, y, z]);
            assert!((0.0..400.0).contains(&px) && (0.0..300.0).contains(&py), "[{x}, {y}, {z}] at {px}, {py}");
        }
    }
    // Orbiting turns about the target, which stays in the middle of the image.
    let turned = cam.orbit(90.0, 100.0);
    assert_eq!((turned.azimuth, turned.elevation), (45.0, 89.0), "never quite straight down");
    let (x, y) = at(&turned, turned.target);
    assert!((x - 200.0).abs() < 1e-9 && (y - 150.0).abs() < 1e-9);
    // A pan carries the point under one pixel to another; a zoom keeps the point under the cursor.
    let p = cam.unproject((100.0, 80.0));
    let (x, y) = at(&cam.pan((100.0, 80.0), (150.0, 120.0)), p);
    assert!((x - 150.0).abs() < 1e-6 && (y - 120.0).abs() < 1e-6);
    let zoomed = cam.zoom(2.0, (100.0, 80.0));
    let (x, y) = at(&zoomed, p);
    assert!((x - 100.0).abs() < 1e-6 && (y - 80.0).abs() < 1e-6 && zoomed.scale == cam.scale * 2.0);
}

#[test]
fn a_perspective_camera_sees_the_nearer_of_two_like_things_larger_and_orbits_pans_and_zooms_as_ever() {
    let s = house();
    let scene = Scene::new(&s.model);
    let flat = scene.fit(AZIMUTH, ELEVATION, 400, 300);
    let cam = scene.fit_from(Camera { lens: Some(LENS), ..flat }, &Look::default());
    // All the house is in the picture, and the eye stands back from the target as far as the lens and scale say.
    for (x, y) in [(0.0, 0.0), (8000.0, 0.0), (8000.0, 6000.0), (0.0, 6000.0)] {
        for z in [0.0, 3000.0] {
            let (px, py) = at(&cam, [x, y, z]);
            assert!((0.0..400.0).contains(&px) && (0.0..300.0).contains(&py), "[{x}, {y}, {z}] at {px}, {py}");
        }
    }
    let d = cam.eye_distance().unwrap();
    assert!((d - 400.0 * LENS / FILM / cam.scale).abs() < 1e-6);
    assert_eq!(flat.eye_distance(), None, "a parallel view has no eye");
    // The south-east corner is nearer the eye than the north-west one: its 3 m of height is longer in the picture.
    let tall = |x: f64, y: f64| at(&cam, [x, y, 0.0]).1 - at(&cam, [x, y, 3000.0]).1;
    assert!(tall(8000.0, 0.0) > tall(0.0, 6000.0) * 1.1, "{} {}", tall(8000.0, 0.0), tall(0.0, 6000.0));
    let even = |x: f64, y: f64| at(&flat, [x, y, 0.0]).1 - at(&flat, [x, y, 3000.0]).1;
    assert!((even(8000.0, 0.0) - even(0.0, 6000.0)).abs() < 1e-9, "parallel, they are as long");
    // Nearer is larger in depth, and a pixel's depth gives back the point it shows.
    let (near, far) = (cam.project([8000.0, 0.0, 0.0]), cam.project([0.0, 6000.0, 0.0]));
    assert!(near.2 > far.2);
    let back = cam.point_at((near.0, near.1), near.2);
    assert!(dist(back, [8000.0, 0.0, 0.0]) < 1e-6, "{back:?}");
    // Behind the eye, nothing.
    let behind = add(cam.eye().unwrap(), mul(sub(cam.eye().unwrap(), cam.target), 0.5));
    assert!(cam.project(behind).0.is_nan());
    // Orbit, pan and zoom as a parallel view does: the target in the middle, the point under the cursor kept.
    let turned = cam.orbit(90.0, 10.0);
    let (x, y) = at(&turned, turned.target);
    assert!((x - 200.0).abs() < 1e-9 && (y - 150.0).abs() < 1e-9);
    assert!((turned.eye_distance().unwrap() - d).abs() < 1e-9, "orbiting keeps the eye as far off");
    let p = cam.unproject((100.0, 80.0));
    let (x, y) = at(&cam.pan((100.0, 80.0), (150.0, 120.0)), p);
    assert!((x - 150.0).abs() < 1e-6 && (y - 120.0).abs() < 1e-6);
    let zoomed = cam.zoom(2.0, (100.0, 80.0));
    let (x, y) = at(&zoomed, p);
    assert!((x - 100.0).abs() < 1e-6 && (y - 80.0).abs() < 1e-6);
    assert!((zoomed.eye_distance().unwrap() - d / 2.0).abs() < 1e-6, "zooming in walks the eye in");
    // A wider picture or a longer lens leaves the eye where it stood.
    for next in [cam.sized(800, 300, Some(LENS)), cam.sized(400, 300, Some(85.0))] {
        assert!(dist(next.eye().unwrap(), cam.eye().unwrap()) < 1e-6);
    }
    assert_eq!(cam.sized(400, 300, None).eye(), None, "back to parallel");
    assert_eq!(flat.sized(400, 300, Some(LENS)).scale, flat.scale, "into perspective, the target keeps its scale");
    // The eye raised to 1.6 m: the target stays, and the eye is that high, as far across the plan from it.
    let low = cam.with_eye_elevation(1600.0);
    let eye = low.eye().unwrap();
    assert!((eye[2] - 1600.0).abs() < 1e-6 && dist(low.target, cam.target) < 1e-9, "{eye:?}");
    let across = |c: &Camera| (c.eye().unwrap()[0] - c.target[0]).hypot(c.eye().unwrap()[1] - c.target[1]);
    assert!((across(&low) - across(&cam)).abs() < 1e-6);
    assert_eq!(low.eye_elevation(), Some(eye[2]));
    // A drag from a point's picture to another's moves it along an axis by as far as they are apart, however foreshortened.
    let (a, b) = ([8000.0, 0.0, 3000.0], [8000.0, 0.0, 500.0]);
    let moved = cam.along(2, a, at(&cam, a), at(&cam, b)).unwrap();
    assert!((moved + 2500.0).abs() < 1e-6, "{moved}");
    let moved = cam.along(1, a, at(&cam, a), at(&cam, [8000.0, 4000.0, 3000.0])).unwrap();
    assert!((moved - 4000.0).abs() < 1e-6, "{moved}");
}

#[test]
fn a_3d_view_is_turned_to_perspective_through_a_lens_and_back_and_renders_so() {
    use serde_json::json;
    let mut s = house();
    let lens = |s: &Session| match s.model.default_3d().and_then(|v| s.model.elements.get(v)) {
        Some(Element::View3D(v)) => v.perspective.filter(|p| p.on).map(|p| p.lens),
        _ => None,
    };
    assert_eq!(lens(&s), None, "parallel to begin with");
    let r = s.execute("perspective", &json!({})).unwrap();
    assert_eq!((r["on"].as_bool(), r["lens"].as_f64()), (Some(true), Some(LENS)));
    assert_eq!(lens(&s), Some(LENS));
    s.execute("perspective", &json!({"lens": 50})).unwrap();
    assert_eq!(lens(&s), Some(50.0));
    // A lens no camera has, or a view that isn't 3D, is refused and changes nothing.
    for bad in [json!({"lens": 2}), json!({"lens": 5000}), json!({"lens": "long"}), json!({"on": "yes"}), json!({"view": "Level 1"})] {
        assert!(s.execute("perspective", &bad).is_err(), "{bad}");
    }
    assert_eq!(lens(&s), Some(50.0));
    // The 3D view renders as its camera sees it, and from as high as the eye is put.
    let png = |r: &serde_json::Value| r["pngBase64"].as_str().map(str::len);
    let seen = s.execute("render", &json!({"view": "{3D}", "width": 200, "height": 150})).unwrap();
    assert_eq!(seen["lens"].as_f64(), Some(50.0));
    let low = s.execute("render", &json!({"view": "{3D}", "width": 200, "height": 150, "eye_elevation": 1600})).unwrap();
    assert!((low["eye_elevation"].as_f64().unwrap() - 1600.0).abs() < 1e-6);
    assert_ne!(png(&seen), png(&low));
    // Turned off, it keeps its lens for when it's turned on again.
    let r = s.execute("perspective", &json!({"on": false})).unwrap();
    assert_eq!((r["on"].as_bool(), r["lens"].as_f64()), (Some(false), Some(50.0)));
    assert_eq!(lens(&s), None);
    let flat = s.execute("render", &json!({"view": "{3D}", "width": 200, "height": 150})).unwrap();
    assert!(flat["lens"].is_null() && flat["eye_elevation"].is_null());
    assert!(s.execute("render", &json!({"view": "{3D}", "eye_elevation": 1600})).is_err(), "a parallel view has no eye");
    s.execute("perspective", &json!({"on": true})).unwrap();
    assert_eq!(lens(&s), Some(50.0));
    // It is the view's, and saved with it.
    let reopened = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    assert!(matches!(reopened.default_3d().and_then(|v| reopened.elements.get(v)), Some(Element::View3D(v)) if v.perspective.is_some()));
}

fn dist(a: P3, b: P3) -> f64 {
    dot(sub(a, b), sub(a, b)).sqrt()
}

/// The `camera` command: it creates a named perspective 3D view from an eye and a target, changes an
/// existing one, and reads a camera back, as MCP and the desktop use it.
#[test]
fn a_camera_command_makes_and_reads_a_camera_view_from_an_eye_and_a_target() {
    use serde_json::json;
    let mut s = house();
    // The camera and the lit lens of the view named `name`, if it is a 3D view with one.
    let camera = |s: &Session, name: &str| -> Option<(crate::model::ViewCamera, Option<f64>)> {
        s.model.elements.iter().find_map(|(_, e)| match e {
            Element::View3D(v) if v.name == name => Some((v.camera?, v.perspective.filter(|p| p.on).map(|p| p.lens))),
            _ => None,
        })
    };
    // Creation: a new view stands where the eye looks at the target, in perspective, named by number.
    let eye = [-5000.0, -4000.0, 1600.0];
    let target = [4000.0, 3000.0, 1200.0];
    let r = s.execute("camera", &json!({"eye": eye, "target": target})).unwrap();
    let id = r["id"].as_u64().unwrap();
    assert_eq!(r["name"].as_str(), Some("Camera 1"));
    assert_eq!((r["eye"].clone(), r["target"].clone()), (json!(eye), json!(target)));
    assert_eq!((r["lens"].as_f64(), r["perspective"].as_bool()), (Some(LENS), Some(true)));
    assert_eq!(camera(&s, "Camera 1").map(|(_, l)| l), Some(Some(LENS)));
    assert!(matches!(s.model.elements.get(id), Some(Element::View3D(v)) if v.name == "Camera 1"));
    // A second camera is named after the first's.
    let r2 = s.execute("camera", &json!({"eye": [0.0, -8000.0, 1700.0], "target": [4000.0, 3000.0, 1000.0], "lens": 24})).unwrap();
    assert_eq!(r2["name"].as_str(), Some("Camera 2"));
    assert_eq!(r2["lens"].as_f64(), Some(24.0));
    // Changing a view's camera keeps its id and its number.
    let moved = s.execute("camera", &json!({"view": "Camera 1", "eye": [-6000.0, -5000.0, 2000.0], "target": target})).unwrap();
    assert_eq!(moved["view"].as_u64(), Some(id));
    assert_eq!(camera(&s, "Camera 1").map(|(c, _)| c.eye), Some([-6000.0, -5000.0, 2000.0]));
    // Its lens (its angle of view) changes on the same view, keeping its id and its points.
    let relensed =
        s.execute("camera", &json!({"view": "Camera 1", "eye": [-6000.0, -5000.0, 2000.0], "target": target, "lens": 50})).unwrap();
    assert_eq!((relensed["view"].as_u64(), relensed["lens"].as_f64()), (Some(id), Some(50.0)));
    assert_eq!(camera(&s, "Camera 1").map(|(c, l)| (c.eye, l)), Some(([-6000.0, -5000.0, 2000.0], Some(50.0))));
    // Reading the default `{3D}` before it was ever made: no camera, not in perspective, and reading made nothing.
    let before = s.execute("camera", &json!({"view": "{3D}"})).unwrap();
    assert_eq!((before["eye"].clone(), before["target"].clone(), before["perspective"].clone()), (json!(null), json!(null), json!(false)));
    assert!(s.model.default_3d().is_none(), "reading made nothing");
    // A lens alone on a view with no camera yet has no points to keep: refused, and it makes nothing.
    assert!(s.execute("camera", &json!({"view": "{3D}", "lens": 50})).is_err());
    assert!(s.model.default_3d().is_none(), "a refused change made nothing");
    // The default 3D view can be given a camera too, by name.
    s.execute("camera", &json!({"view": "{3D}", "eye": [0.0, -12000.0, 9000.0], "target": [4000.0, 3000.0, 1500.0]})).unwrap();
    assert!(matches!(s.model.default_3d().and_then(|v| s.model.elements.get(v)), Some(Element::View3D(v)) if v.camera.is_some()));
    // Reading: the camera comes back with its lens; a view without one says so.
    let read = s.execute("camera", &json!({"view": "Camera 1"})).unwrap();
    assert_eq!((read["eye"].clone(), read["target"].clone()), (json!([-6000.0, -5000.0, 2000.0]), json!(target)));
    assert_eq!((read["lens"].as_f64(), read["perspective"].as_bool()), (Some(50.0), Some(true)));
    // A lens alone is a change, not a read: it keeps the eye and the target.
    let relensed_only = s.execute("camera", &json!({"view": "Camera 1", "lens": 80})).unwrap();
    assert_eq!(
        (relensed_only["eye"].clone(), relensed_only["target"].clone(), relensed_only["lens"].as_f64()),
        (json!([-6000.0, -5000.0, 2000.0]), json!(target), Some(80.0))
    );
    // A point alone moves just that point; the other stays where the camera had it.
    let eyed_only = s.execute("camera", &json!({"view": "Camera 1", "eye": [-7000.0, -4000.0, 2100.0]})).unwrap();
    assert_eq!(eyed_only["target"], json!(target));
    assert_eq!(camera(&s, "Camera 1").map(|(c, l)| (c.eye, l)), Some(([-7000.0, -4000.0, 2100.0], Some(80.0))));
    // Changed camera, still read back.
    let plain = s.execute("camera", &json!({"view": "Camera 2"})).unwrap();
    assert_eq!(plain["eye"], json!([0.0, -8000.0, 1700.0]));
    // The built camera's eye and target are the ones given (the render path uses them).
    let built = Camera::at(eye, target, LENS, 400, 300);
    assert!((built.eye().unwrap()[0] - eye[0]).abs() < 1e-6 && dist(built.eye().unwrap(), eye) < 1e-6, "{:?}", built.eye());
    assert_eq!(built.target, target);
    assert_eq!(built.lens, Some(LENS));
    // It is saved with the project and read back.
    let reopened = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap().0;
    let again = reopened.elements.iter().find(|(_, e)| matches!(e, Element::View3D(v) if v.name == "Camera 1"));
    assert!(again.is_some_and(|(_, e)| matches!(e, Element::View3D(v) if v.camera.is_some())), "the camera is saved");
}

/// What the `camera` command refuses: an eye at the target, bad coordinates, a lens no camera has, a view that
/// isn't 3D, and a missing eye or target when one is asked for.
#[test]
fn the_camera_command_refuses_what_no_camera_would_be() {
    use serde_json::json;
    let mut s = house();
    let eye = [0.0, -8000.0, 1600.0];
    let target = [4000.0, 3000.0, 1200.0];
    for bad in [
        json!({"eye": eye, "target": eye}),
        json!({"eye": eye}),
        json!({"target": target}),
        json!({"eye": [f64::MAX, 0.0, 0.0], "target": target}),
        json!({"eye": "far", "target": target}),
        json!({"eye": [0.0, -8000.0], "target": target}),
        json!({"eye": eye, "target": target, "lens": 2}),
        json!({"eye": eye, "target": target, "lens": 5000}),
        json!({"view": "Level 1", "eye": eye, "target": target}),
        json!({"view": "nowhere", "eye": eye, "target": target}),
    ] {
        assert!(s.execute("camera", &bad).is_err(), "{bad}");
    }
    // A changed view stays as it was after a refused change.
    s.execute("camera", &json!({"eye": eye, "target": target})).unwrap();
    s.execute("camera", &json!({"view": "Camera 1", "eye": [1000.0, -9000.0, 1700.0], "target": target})).unwrap();
    assert!(s.execute("camera", &json!({"view": "Camera 1", "eye": eye, "target": eye})).is_err());
    assert!(
        matches!(
            s.model.elements.iter().find(|(_, e)| matches!(e, Element::View3D(v) if v.name == "Camera 1")),
            Some((_, Element::View3D(v))) if v.camera == Some(crate::model::ViewCamera::new([1000.0, -9000.0, 1700.0], target))
        ),
        "the refused change left the camera alone"
    );
}

/// A camera view renders from its own eye through its lens, and its picture differs from the default's.
#[test]
fn a_camera_view_renders_from_where_its_camera_stands() {
    use serde_json::json;
    let mut s = house();
    s.execute("camera", &json!({"eye": [-5000.0, -4000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0]})).unwrap();
    let png = |r: &serde_json::Value| r["pngBase64"].as_str().map(str::len);
    let named = s.execute("render", &json!({"view": "Camera 1", "width": 300, "height": 200})).unwrap();
    assert_eq!(named["kind"].as_str(), Some("3D view"));
    assert_eq!(named["eye"], json!([-5000.0, -4000.0, 1600.0]));
    assert_eq!(named["lens"].as_f64(), Some(LENS));
    assert!(png(&named).is_some());
    // The default 3D view, fitted over the model, looks different.
    let flat = s.execute("render", &json!({"view": "{3D}", "width": 300, "height": 200})).unwrap();
    assert_ne!(png(&named), png(&flat));
    // Rendering the camera again is stable.
    let again = s.execute("render", &json!({"view": "Camera 1", "width": 300, "height": 200})).unwrap();
    assert_eq!(png(&named), png(&again));
    // A camera view renders through its lens, and asks for a usable picture.
    assert!(s.execute("render", &json!({"view": "Camera 1", "width": 5})).is_ok());
}

/// A camera stands on its level's plan, drawn there only while its view is selected, as Revit's (`render::camera_marks`):
/// the plan itself never draws it, its mark picks as the view, and another level's plan doesn't show it.
#[test]
fn a_camera_shows_on_its_levels_plan_while_selected_and_a_pick_there_finds_it() {
    use serde_json::json;
    let mut s = house();
    let r = s.execute("camera", &json!({"eye": [12000.0, 3000.0, 1600.0], "target": [4000.0, 3000.0, 1200.0]})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let d = crate::render::plan(&s.model, 100000, 900, 700);
    assert!(!d.owners.iter().flatten().any(|o| *o == id), "a plan doesn't draw a camera of itself");
    let marks = crate::render::camera_marks(&s.model, 100000, d.view, &[id]);
    assert!(marks.owners.iter().flatten().any(|o| *o == id), "its mark is on Level 1's plan while shown");
    assert_eq!(marks.pick(d.view.px(P2(12000.0, 3000.0)), 8.0), Some(id), "a pick at its eye finds the view");
    assert!(crate::render::camera_marks(&s.model, 100000, d.view, &[]).prims.is_empty(), "none while none is selected");
    // Level 2's plan doesn't show it: the camera stands on Level 1.
    let up = crate::render::plan(&s.model, 100001, 900, 700);
    assert!(crate::render::camera_marks(&s.model, 100001, up.view, &[id]).prims.is_empty(), "not on another level's plan");
    // Raised through Level 2's mark, it stands on Level 2's plan at once: the level follows the eye.
    s.execute("camera", &json!({"view": "Camera 1", "eye": [12000.0, 3000.0, 4000.0]})).unwrap();
    assert!(crate::render::camera_marks(&s.model, 100000, d.view, &[id]).prims.is_empty(), "gone from Level 1");
    assert!(!crate::render::camera_marks(&s.model, 100001, up.view, &[id]).prims.is_empty(), "its mark follows its eye to Level 2");
}

/// Moving or mirroring a camera's mark on the plan takes the camera as a whole (each point keeping its height); a view
/// with no camera of its own still has no place in plan.
#[test]
fn a_camera_view_moves_on_the_plan_as_a_whole() {
    use serde_json::json;
    let mut s = house();
    let r = s.execute("camera", &json!({"eye": [1500.0, -2200.0, 1600.0], "target": [5000.0, 3000.0, 1200.0]})).unwrap();
    let id = r["id"].as_u64().unwrap();
    let cam = |s: &Session| match s.model.elements.get(id) {
        Some(Element::View3D(v)) => v.camera,
        _ => None,
    };
    // Dragged east and north: both points go, each keeping its height.
    s.execute("move", &json!({"ids": [id], "by": [2000.0, 500.0]})).unwrap();
    assert_eq!(
        cam(&s),
        Some(crate::model::ViewCamera::new([3500.0, -1700.0, 1600.0], [7000.0, 3500.0, 1200.0])),
        "the mark drags the camera with it"
    );
    assert_eq!(s.model.check(), Ok(()));
    // Mirrored over the east-west line: it looks back the other way, still at its heights.
    s.execute("mirror", &json!({"ids": [id], "axis": [[0.0, 0.0], [1000.0, 0.0]]})).unwrap();
    assert_eq!(
        cam(&s),
        Some(crate::model::ViewCamera::new([3500.0, 1700.0, 1600.0], [7000.0, -3500.0, 1200.0])),
        "a mirror image flips it in plan, keeping its heights"
    );
    // The default `{3D}`, with no camera of its own, still has nothing to move.
    s.execute("perspective", &json!({})).unwrap();
    let flat = s.model.default_3d().unwrap_or_default();
    let e = s.execute("move", &json!({"ids": [flat], "by": [100.0, 0.0]})).unwrap_err();
    assert!(e.contains("has no place in plan"), "{e}");
}

#[test]
fn a_perspective_picture_shows_and_picks_what_it_sees_and_cuts_away_what_is_behind_the_eye() {
    let s = house();
    let scene = Scene::new(&s.model);
    let cam = scene.fit_from(Camera { lens: Some(LENS), ..scene.fit(AZIMUTH, ELEVATION, 400, 300) }, &Look::default());
    let img = scene.render(&cam, &[]);
    // The wall along the south, at its middle, picks as a wall.
    let (x, y) = at(&cam, [4000.0, 0.0, 1500.0]);
    let wall = img.pick(x, y).and_then(|id| s.model.elements.get(id)).map(|e| e.category());
    assert_eq!(wall, Some("wall"));
    // The eye inside the house, at a person's height: the walls all round fill the picture, and the ones behind it
    // and those it stands across are drawn as far as they are in front of it.
    let inside = Camera { target: [4000.0, 3000.0, 1600.0], elevation: 0.0, scale: 400.0 * 20.0 / FILM / 1000.0, ..cam };
    assert!(inside.eye().is_some_and(|e| (0.0..8000.0).contains(&e[0]) && (0.0..6000.0).contains(&e[1])));
    let img = scene.render(&inside, &[]);
    let shown = (0..300).flat_map(|y| (0..400).map(move |x| (x, y))).filter(|&(x, y)| img.pick(f64::from(x), f64::from(y)).is_some());
    assert!(shown.count() > 400 * 300 / 2, "the walls are round the eye");
    // The floor runs on under the eye and behind it: what is in front of it is drawn at the picture's foot.
    let foot = img.pick(200.5, 299.5).and_then(|id| s.model.elements.get(id)).map(|e| e.category());
    assert_eq!(foot, Some("floor"));
}

#[test]
fn the_picture_saves_as_png() {
    let s = house();
    let (_, img) = view(&s);
    let pm = tiny_skia::Pixmap::decode_png(&img.png().unwrap()).unwrap();
    assert_eq!((pm.width(), pm.height()), (400, 300));
}

/// The house drawn in `style`, from the default view, through the default 3D view's look.
fn styled(s: &mut Session, style: &str) -> (Camera, Image) {
    s.execute("visual_style", &serde_json::json!({"style": style})).unwrap();
    let scene = Scene::new(&s.model);
    let cam = scene.fit(AZIMUTH, ELEVATION, 400, 300);
    let img = scene.render_in(&cam, &[], &Look::of_3d(&s.model));
    (cam, img)
}

/// The pixels of `img` that are lines or shading: not paper.
fn inked(img: &Image) -> usize {
    img.rgba.chunks(4).filter(|c| c[..3] != [255, 255, 255]).count()
}

/// Whether a line passes within a pixel of `p`.
fn near_a_line(cam: &Camera, img: &Image, p: P3) -> bool {
    let (x, y) = at(cam, p);
    (-1..=1).any(|dy| (-1..=1).any(|dx| img.color(x + f64::from(dx), y + f64::from(dy)) != PAPER))
}

/// The edges by the whole computation: a map from each side to the triangles on it.
fn edges_by_a_map(scene: &Scene) -> Vec<(usize, [P3; 2])> {
    let corner = |t: &Tri, k: usize| t.p[k % 3];
    let q = |p: P3| p.map(|x| (x * 1e3).round() as i64);
    type Corner = [i64; 3];
    let mut on: HashMap<(Corner, Corner), Vec<(usize, usize)>> = HashMap::new();
    for (i, t) in scene.tris.iter().enumerate() {
        for k in 0..3 {
            let (qa, qb) = (q(corner(t, k)), q(corner(t, k + 1)));
            if qa != qb {
                on.entry((qa.min(qb), qa.max(qb))).or_default().push((i, k));
            }
        }
    }
    let plane = |i: usize| scene.planes.get(i).copied();
    let mut edges: Vec<(usize, [P3; 2])> = on
        .into_values()
        .filter(|on| !matches!(on.as_slice(), [(x, _), (y, _)] if plane(*x) == plane(*y)))
        .map(|on| (on[0].0, [corner(&scene.tris[on[0].0], on[0].1), corner(&scene.tris[on[0].0], on[0].1 + 1)]))
        .collect();
    edges.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.partial_cmp(&y.1).unwrap()));
    edges
}

#[test]
fn the_edges_found_are_those_of_a_map_from_each_side_to_its_triangles() {
    let mut highrise = Session::new();
    highrise.execute("highrise", &serde_json::json!({})).unwrap();
    for (name, s) in [("house", house()), ("highrise", highrise)] {
        let scene = Scene::new(&s.model);
        let by_map = edges_by_a_map(&scene);
        assert!(by_map.len() > 100, "{name}: {} edges", by_map.len());
        assert_eq!(scene.edges(), by_map.as_slice(), "{name}");
    }
}

#[test]
fn edges_asked_for_a_view_already_gone_are_not_kept_and_the_next_to_ask_finds_them() {
    let scene = Scene::new(&house().model);
    assert!(scene.edges_cancellable(&|| true).is_none());
    // Cancelled only after the sides are gathered: still nothing kept.
    let asked = std::cell::Cell::new(0);
    let later = || {
        asked.set(asked.get() + 1);
        asked.get() > 2
    };
    assert!(scene.edges_cancellable(&later).is_none());
    assert_eq!(scene.edges_cancellable(&|| false).unwrap(), edges_by_a_map(&scene).as_slice());
}

#[test]
fn hidden_line_draws_faces_as_paper_with_their_edges_and_wireframe_every_edge_through_them() {
    let mut s = house();
    let (cam, shaded) = view(&s);
    let south = at(&cam, [5000.0, -100.0, 1500.0]);
    assert_ne!(shaded.color(south.0, south.1), PAPER, "shaded: a lit face");
    // Hidden line: paper faces, and their edges dark, as a section's picture has them.
    let (_, hidden) = styled(&mut s, "hidden_line");
    assert_eq!(hidden.color(south.0, south.1), PAPER);
    assert!(inked(&hidden) > 300 && inked(&hidden) < inked(&shaded) / 4, "{} of {}", inked(&hidden), inked(&shaded));
    let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
    assert_ne!(hidden.color(x, y), PAPER, "the south wall's top edge");
    // The outer corner of the north west walls stands at the back, behind the walls: wireframe draws it through them.
    let hidden_edge = [-100.0, 6100.0, 1500.0];
    let (_, wire) = styled(&mut s, "wireframe");
    assert!(near_a_line(&cam, &wire, hidden_edge), "wireframe shows every edge");
    let through = wire.rgba.chunks(4).zip(hidden.rgba.chunks(4)).filter(|(w, h)| w[..3] != [255; 3] && h[..3] == [255; 3]).count();
    assert!(through > 50, "lines hidden line keeps hidden: {through}");
    // Faces stay paper; picking an edge finds its wall, a face nothing.
    assert_eq!(wire.color(south.0, south.1), PAPER);
    assert!(inked(&wire) > 300 && inked(&wire) < inked(&hidden) * 2, "{}", inked(&wire));
    assert_eq!(wire.pick(south.0, south.1), None, "a face isn't there to pick");
    assert_eq!(wire.pick(x, y), Some(100002), "the top edge of the south wall");
    // Back to shaded, the picture is as it was.
    let (_, again) = styled(&mut s, "shaded");
    assert_eq!(again.rgba, shaded.rgba);
}

#[test]
fn wireframe_keeps_what_the_view_hides_hidden_and_takes_a_tint_and_an_edge_colour() {
    let mut s = house();
    let (cam, all) = styled(&mut s, "wireframe");
    s.script(r#"graphics {"view": "{3D}", "hide": ["door"]}"#).unwrap();
    let scene = Scene::new(&s.model);
    let look = Look::of_3d(&s.model);
    let plain = scene.render_in(&cam, &[], &look);
    assert!(inked(&plain) < inked(&all), "the door's edges are gone: {} of {}", inked(&plain), inked(&all));
    let tinted = scene.render_in(&cam, &[(100002, Rgb(255, 0, 0))], &look);
    let (x, y) = at(&cam, [5000.0, -100.0, 3000.0]);
    assert_eq!(tinted.color(x, y), Rgb(255, 0, 0), "a selected wall's edges in the selection's colour");
    let colours = plain.rgba.chunks(4).filter(|c| c[..3] != [255, 255, 255]).map(|c| (c[0], c[1], c[2])).collect::<BTreeSet<_>>();
    assert_eq!(colours, BTreeSet::from([(EDGE.0, EDGE.1, EDGE.2)]), "lines in the edge colour, nothing else");
}

#[test]
fn a_section_cuts_a_tube_in_its_ring_an_oblique_one_in_an_ellipse_and_leaves_other_meshes_whole() {
    use crate::runs::Geom;
    // A pipe 200 across along east at 500 up, from x = 0 to 4000, and a section 2000 along it, level and then at 45°.
    let g = Geom { a: [0.0, 0.0, 500.0], b: [4000.0, 0.0, 500.0], width: 200.0, depth: 200.0, round: true };
    let (tris, caps) = g.tube();
    let tube = |cut| Solid::Mesh { owner: 1, color: EDGE, tex: 0, tris: tris.clone(), caps, round: true, cut };
    let extent = |rings: &[Vec<P2>]| {
        let pts: Vec<P2> = rings.iter().flatten().copied().collect();
        let (lo, hi) = pts.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
            (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
        });
        (hi.0 - lo.0, hi.1 - lo.1, lo.0, lo.1)
    };
    let square = tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 2000.0);
    assert_eq!(square.len(), 1);
    let (w, h, x0, z0) = extent(&square);
    assert!(
        (w - 200.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6 && (x0 - 900.0).abs() < 1e-6 && (z0 - 400.0).abs() < 1e-6,
        "{w} {h} {x0} {z0}"
    );
    // A regular 24-gon in a circle of radius 100: its area, and counter-clockwise.
    let area = 12.0 * 100.0 * 100.0 * (std::f64::consts::TAU / 24.0).sin();
    assert!((geom::area(&square[0]) - area).abs() < 1e-6 * area, "{}", geom::area(&square[0]));
    // Slanting, its ring is longer along the line by 1 / sin 45°, as high as before.
    let slanted = tube(true).cuts(P2(1000.0, -1000.0), P2(1.0, 1.0).unit(), 3000.0);
    let (w, h, ..) = extent(&slanted);
    assert!((w - 200.0 * 2f64.sqrt()).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
    // Past the section's end it is clipped; clear of the pipe, or not allowed to be cut, it leaves nothing.
    let (w, ..) = extent(&tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 1050.0));
    assert!((w - 150.0).abs() < 1e-6, "{w}");
    // A line that starts inside the pipe cuts what is past its start only.
    let (w, _, x0, _) = extent(&tube(true).cuts(P2(2000.0, -50.0), P2(0.0, 1.0), 2000.0));
    assert!((w - 150.0).abs() < 1e-6 && x0.abs() < 1e-6, "{w} {x0}");
    assert!(tube(true).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 500.0).is_empty());
    assert!(tube(true).cuts(P2(5000.0, -1000.0), P2(0.0, 1.0), 2000.0).is_empty());
    assert!(tube(false).cuts(P2(2000.0, -1000.0), P2(0.0, 1.0), 2000.0).is_empty());
    // A plane that lies in a face of a convex mesh (vertices exactly in it, no edge crossing it) gives that face.
    let corner = |x: f64, y: f64, z: f64| [x, y, z];
    let (a, b, c, d) = (corner(0.0, -100.0, 0.0), corner(200.0, -100.0, 0.0), corner(200.0, 100.0, 0.0), corner(0.0, 100.0, 0.0));
    let (e, f, g, h) = (corner(0.0, -100.0, 200.0), corner(200.0, -100.0, 200.0), corner(200.0, 100.0, 200.0), corner(0.0, 100.0, 200.0));
    let tris = vec![
        [a, c, b],
        [a, d, c],
        [e, f, g],
        [e, g, h],
        [a, b, f],
        [a, f, e],
        [b, c, g],
        [b, g, f],
        [c, d, h],
        [c, h, g],
        [d, a, e],
        [d, e, h],
    ];
    let cube = Solid::Mesh { owner: 1, color: EDGE, tex: 0, tris, caps: 0, round: false, cut: true };
    let (w, h, ..) = extent(&cube.cuts(P2(0.0, 100.0), P2(1.0, 0.0), 1000.0));
    assert!((w - 200.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
    // A section along the pipe cuts it lengthwise: a strip as long as the pipe is and as high as it is round.
    let along = tube(true).cuts(P2(0.0, 0.0), P2(1.0, 0.0), 4000.0);
    let (w, h, ..) = extent(&along);
    assert!((w - 4000.0).abs() < 1e-6 && (h - 200.0).abs() < 1e-6, "{w} {h}");
}

#[test]
fn a_lofted_mass_is_a_closed_skin_of_triangles_turned_out_that_encloses_its_exact_volume() {
    use serde_json::json;
    let mut s = crate::engine::Session::new();
    s.execute("level", &json!({"elevation": 6000, "name": "Level 3"})).unwrap();
    let (base, crown) =
        (json!([[0, 0], [10000, 0], [10000, 6000], [0, 6000]]), json!([[2000, 2000], [8000, 2000], [8000, 4000], [2000, 4000]]));
    let r = s.execute("mass", &json!({"boundary": base, "height": 6000, "top": crown})).unwrap();
    let id = r["id"].as_u64().unwrap_or_default();
    let mut tris = Vec::new();
    let solids = solids(&s.model);
    assert!(solids.iter().filter(|x| x.owner() == id).all(|x| matches!(x, Solid::Mesh { .. })));
    solids.iter().filter(|x| x.owner() == id).for_each(|x| x.tris(&mut tris));
    // Turned out, the sum of the tetrahedra to the origin is the volume: 6 m x (60 + 4 x 32 + 12) / 6 m².
    let volume: f64 = tris.iter().map(|t| dot(t.p[0], cross(t.p[1], t.p[2])) / 6.0).sum();
    assert!((volume - 200e9).abs() < 1.0, "{volume}");
    // Closed: each edge is walked once one way and once the other.
    let key = |p: P3| p.map(|x| (x * 1e3).round() as i64);
    let mut walked: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
    for t in &tris {
        for k in 0..3 {
            *walked.entry((key(t.p[k]), key(t.p[(k + 1) % 3]))).or_default() += 1;
        }
    }
    assert!(walked.iter().all(|((a, b), n)| *n == 1 && walked.get(&(*b, *a)) == Some(&1)), "an open or doubled edge");
    // Over its middle it stands 6 m; over a corner of its footprint, past the top outline, it is lower.
    assert_eq!(tops_at(&s.model, P2(5000.0, 3000.0)).get(&id), Some(&6000.0));
    let corner = tops_at(&s.model, P2(500.0, 500.0)).get(&id).copied().unwrap_or(f64::MAX);
    assert!(corner > 0.0 && corner < 6000.0, "{corner}");
    // A flat-sided loft is twelve triangles: two to each of four sides, and two to each cap.
    assert_eq!(tris.len(), 12);
}

#[test]
fn a_twisted_loft_is_cut_into_strips_that_keep_its_volume_within_two_percent() {
    use serde_json::json;
    let mut s = crate::engine::Session::new();
    // 4 x 4 m at the base, turned 45° over 3 m: the exact volume is 3 m x (16 + 4 x 13.657 + 16) / 6 = 43.31 m³.
    let top = json!([[2000.0, -828.427], [4828.427, 2000.0], [2000.0, 4828.427], [-828.427, 2000.0]]);
    let r = s.execute("mass", &json!({"boundary": [[0, 0], [4000, 0], [4000, 4000], [0, 4000]], "height": 3000, "top": top})).unwrap();
    let id = r["id"].as_u64().unwrap_or_default();
    let mut tris = Vec::new();
    solids(&s.model).iter().filter(|x| x.owner() == id).for_each(|x| x.tris(&mut tris));
    let volume: f64 = tris.iter().map(|t| dot(t.p[0], cross(t.p[1], t.p[2])) / 6.0).sum();
    let exact = r["volume_m3"].as_f64().unwrap_or_default() * 1e9;
    assert!((exact - 43.31e9).abs() < 0.01e9, "{exact}");
    assert!(volume < exact && volume > exact * 0.98, "{volume} of {exact}");
    assert!(tris.len() > 12 * 4, "its sides are strips: {}", tris.len());
}

/// The house's default 3D view with its Graphic Display Options set to `options`, 800 x 600, and its levels hidden so
/// their lines draw over nothing; in perspective with `lens`.
fn displayed(s: &mut Session, options: serde_json::Value, lens: Option<f64>) -> (Camera, Image) {
    s.execute("graphic_display_options", &options).unwrap();
    s.execute("graphics", &serde_json::json!({"view": "{3D}", "hide": ["levels"]})).unwrap();
    let (scene, look) = (Scene::new(&s.model), Look::of_3d(&s.model));
    let cam = scene.fit_in(AZIMUTH, ELEVATION, 800, 600, &look);
    let cam = if lens.is_some() { scene.fit_from(Camera { lens, ..cam }, &look) } else { cam };
    (cam, scene.render_in(&cam, &[], &look))
}

/// How many of `img`'s pixels are drawn in the edge colour.
fn edged(img: &Image) -> usize {
    img.rgba.chunks(4).filter(|c| c[..3] == [EDGE.0, EDGE.1, EDGE.2]).count()
}

#[test]
fn ambient_shadows_darken_where_the_floor_meets_a_wall_and_leave_flat_faces_and_outer_edges_as_they_were() {
    for lens in [None, Some(LENS)] {
        let mut s = house();
        let (cam, plain) = displayed(&mut s, serde_json::json!({}), lens);
        let (_, shadowed) = displayed(&mut s, serde_json::json!({"ambient_shadows": true}), lens);
        let colour = |img: &Image, p: P3| {
            let (x, y) = at(&cam, p);
            img.color(x, y)
        };
        let darker = |p: P3| {
            let (a, b) = (colour(&plain, p), colour(&shadowed, p));
            [(a.0, b.0), (a.1, b.1), (a.2, b.2)].iter().map(|(a, b)| i32::from(*a) - i32::from(*b)).min().unwrap_or(0)
        };
        // Inside, the floor along the foot of the north wall (5) and the wall over it (seen over the south one), a few
        // pixels off the edge where they meet: in the corner they make.
        for (p, owner) in [([3000.0, 5840.0, 0.0], 100008), ([3000.0, 5900.0, 60.0], 100004)] {
            let (x, y) = at(&cam, p);
            assert_eq!(plain.pick(x, y), Some(owner), "{p:?}, {lens:?}");
            assert!(darker(p) >= 12, "{p:?}, {lens:?}: {}", darker(p));
        }
        // The middle of the floor, the south wall's outer face, and that face just under its top edge, which stands out.
        for p in [[2000.0, 4000.0, 0.0], [5000.0, -100.0, 1500.0], [5000.0, -100.0, 2950.0]] {
            assert_eq!(colour(&plain, p), colour(&shadowed, p), "{p:?}, {lens:?}");
        }
        // Turned off again, the picture is as it was.
        let (_, again) = displayed(&mut s, serde_json::json!({"ambient_shadows": false}), lens);
        assert_eq!(again.rgba, plain.rgba);
    }
}

#[test]
fn edges_turned_off_leave_out_only_the_lines_and_a_silhouette_outlines_what_stands_against_what_is_behind_as_wide_as_set() {
    let mut s = house();
    let (cam, plain) = displayed(&mut s, serde_json::json!({}), None);
    assert!(edged(&plain) > 500, "{}", edged(&plain));
    let (_, bare) = displayed(&mut s, serde_json::json!({"edges": false}), None);
    assert_eq!(edged(&bare), 0, "no edges");
    // What changed was an edge, or one seen through the window's glass (100007), which is blended over it.
    let w = plain.width as usize;
    let changed: Vec<usize> =
        (0..plain.hit.len()).filter(|k| plain.rgba.get(4 * k..4 * k + 3) != bare.rgba.get(4 * k..4 * k + 3)).collect();
    let at_k = |k: usize| ((k % w) as f64, (k / w) as f64);
    let other: Vec<(Rgb, Option<Id>)> = changed
        .iter()
        .map(|&k| (plain.color(at_k(k).0, at_k(k).1), plain.pick(at_k(k).0, at_k(k).1)))
        .filter(|(c, id)| *c != EDGE && *id != Some(100007))
        .collect();
    assert!(changed.len() > 500 && other.is_empty(), "{} changed, of them not edges: {:?}", changed.len(), &other[..other.len().min(8)]);
    // Hidden Line draws its edges whatever the view says.
    s.execute("visual_style", &serde_json::json!({"style": "hidden_line"})).unwrap();
    let (_, hidden) = displayed(&mut s, serde_json::json!({}), None);
    assert!(edged(&hidden) > 500);
    s.execute("visual_style", &serde_json::json!({"style": "shaded"})).unwrap();
    // The south west corner, the house's left end, stands against nothing: across it, as many outline pixels as the width.
    let (x, y) = at(&cam, [-100.0, -100.0, 1500.0]);
    let across = |img: &Image| (-12..=12).filter(|dx| img.color(x + f64::from(*dx), y) == EDGE).count();
    let mut counts = Vec::new();
    for width in [1, 3, 6] {
        let (_, outlined) = displayed(&mut s, serde_json::json!({"silhouette": width}), None);
        assert_eq!(across(&outlined), width as usize, "{width} px");
        counts.push(edged(&outlined));
    }
    assert!(counts[0] > 300 && counts[1] > 2 * counts[0] && counts[2] > counts[1] + counts[0], "{counts:?}");
    // The south wall's face is no outline: the faces of one thing meet with no gap between them.
    let (fx, fy) = at(&cam, [5000.0, -100.0, 1500.0]);
    let (_, outlined) = displayed(&mut s, serde_json::json!({"silhouette": 6}), None);
    assert_ne!(outlined.color(fx, fy), EDGE);
    // The edges back on, and the silhouette drawn over them.
    let (_, both) = displayed(&mut s, serde_json::json!({"edges": true}), None);
    assert!(edged(&both) > edged(&outlined), "{} {}", edged(&both), edged(&outlined));
}

#[test]
fn graphic_display_options_are_set_read_kept_in_the_file_undone_and_refused_when_wrong() {
    use serde_json::json;
    let mut s = house();
    let none = json!({"view": null, "edges": true, "silhouette": 0, "ambient_shadows": false});
    assert_eq!(s.execute("graphic_display_options", &json!({})).unwrap(), none);
    assert!(s.model.default_3d().is_none(), "reading makes no view");
    let r = s.execute("graphic_display_options", &json!({"silhouette": 3, "ambient_shadows": true})).unwrap();
    let v = s.model.default_3d().unwrap();
    assert_eq!(r, json!({"view": v, "edges": true, "silhouette": 3, "ambient_shadows": true}));
    // What is left out stays; by GD, as Revit's shortcut, and by the view's name.
    let r = s.execute("GD", &json!({"view": "{3D}", "edges": false})).unwrap();
    assert_eq!(r, json!({"view": v, "edges": false, "silhouette": 3, "ambient_shadows": true}));
    let shown = Look::of_3d(&s.model).display();
    assert_eq!(shown, DisplayOptions { edges: false, silhouette: 3, ambient_shadows: true });
    // Kept in the file, and read back.
    let (m, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(Look::of_3d(&m).display(), shown);
    // Refused: what no outline is, what isn't on or off, a view that isn't 3D. Nothing changes.
    for p in [
        json!({"silhouette": 11}),
        json!({"silhouette": -1}),
        json!({"silhouette": 2.5}),
        json!({"silhouette": "wide"}),
        json!({"silhouette": 1e300}),
        json!({"edges": "no"}),
        json!({"ambient_shadows": 1}),
        json!({"view": "Level 1", "edges": true}),
    ] {
        assert!(s.execute("graphic_display_options", &p).is_err(), "{p}");
        assert_eq!(Look::of_3d(&s.model).display(), shown, "{p}");
    }
    // A file that says a wider one is refused as the model's check says.
    let mut file: serde_json::Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let text = file.to_string().replace("\"silhouette\":3", "\"silhouette\":40");
    assert_ne!(text, file.to_string());
    file = serde_json::from_str(&text).unwrap();
    let (wide, _) = Model::read(&file.to_string()).unwrap();
    assert!(wide.check().unwrap_err().contains("silhouette 40 px wide"));
    // Undone, a step at a time.
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(Look::of_3d(&s.model).display(), DisplayOptions { edges: true, silhouette: 3, ambient_shadows: true });
    s.execute("undo", &json!({})).unwrap();
    assert_eq!(Look::of_3d(&s.model).display(), DisplayOptions::default());
}

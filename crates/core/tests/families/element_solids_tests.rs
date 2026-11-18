#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! One closed solid per element, per material layer (`solids::element_solids`): the volumes of each layer against figures worked
//! by hand, closedness, origins and curve tags, the cache, and every element of the sample models.

use std::f64::consts::PI;

use serde_json::json;

use crate::engine::Session;
use crate::graphics_testing::run;
use crate::model::{Element, Id};
use crate::quantities;
use crate::solid::{Frame, Origin, Seg};
use crate::solids::{ElementSolids, element_solids};
use crate::view3d::Scene;

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[track_caller]
fn assert_near(what: &str, got: f64, want: f64, tol: f64) {
    eprintln!("{what}: {got:.6} against {want:.6}, off by {:.2e}", (got - want).abs() / want.abs().max(1.0));
    assert!(near(got, want, tol), "{what}: {got} is not {want}");
}

/// Every layer's solid closed, non-empty and the origin of every triangle its element's and its layer's.
#[track_caller]
fn assert_sound(id: Id, es: &ElementSolids) {
    for (layer, solid) in &es.layers {
        assert!(!solid.is_empty(), "element {id} layer {layer} is empty");
        assert!(solid.is_closed(), "element {id} layer {layer} is not closed");
        for t in solid.triangles() {
            assert_eq!((t.origin.element, t.origin.layer), (id, *layer), "a triangle of element {id} layer {layer} says {:?}", t.origin);
        }
    }
}

/// Wall type L: a 20 mm exterior finish, a 160 mm core, a 20 mm interior finish; and the openings of the quantities' tests.
fn project() -> Session {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Render"}));
    run(&mut s, "material", json!({"name": "Block"}));
    run(&mut s, "material", json!({"name": "Plaster"}));
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "L", "layers": [
            {"function": "finish", "material": "Render", "thickness": 20},
            {"function": "structure", "material": "Block", "thickness": 160},
            {"function": "finish", "material": "Plaster", "thickness": 20}]}),
    );
    run(&mut s, "type", json!({"category": "wall", "name": "T", "thickness": 200}));
    run(&mut s, "type", json!({"category": "door", "name": "D", "width": 1000, "height": 2100}));
    run(&mut s, "type", json!({"category": "window", "name": "W", "width": 1200, "height": 1500, "sill": 900}));
    s
}

fn id_of(v: &serde_json::Value) -> u64 {
    v["id"].as_u64().or_else(|| v["ids"][0].as_u64()).unwrap_or_default()
}

// ----- a straight layered wall with a door and a window --------------------------------------------------------------------

#[test]
fn a_layered_wall_with_a_door_and_a_window_is_three_exact_solids() {
    let mut s = project();
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "type": "L", "height": 3000, "location": "centre"})));
    let door = id_of(&run(&mut s, "door", json!({"wall": w, "offset": 2000, "type": "D"})));
    let window = id_of(&run(&mut s, "window", json!({"wall": w, "offset": 5000, "type": "W"})));
    let es = element_solids(&s.model, w);
    assert!(es.is_whole(), "{:?}", es.open);
    assert_sound(w, &es);
    assert_eq!(es.layers.iter().map(|(k, _)| *k).collect::<Vec<_>>(), vec![0, 1, 2]);
    // Each layer: its thickness through the wall's 8 x 3 m less the door's 1 x 2.1 and the window's 1.2 x 1.5.
    let face = 8000.0 * 3000.0 - 1000.0 * 2100.0 - 1200.0 * 1500.0;
    for (k, t) in [20.0, 160.0, 20.0].into_iter().enumerate() {
        let layer = es.layer(k as u16).expect("a layer");
        assert_near(&format!("layer {k}"), layer.volume(), face * t, 1e-9);
        assert_eq!(layer.volume_on_curves(), layer.volume(), "a straight wall has no arcs to add");
        let b = layer.bounds().unwrap();
        assert_near("its length", b.max[0] - b.min[0], 8000.0, 1e-9);
        assert_near("its height", b.max[2] - b.min[2], 3000.0, 1e-9);
        assert_near("its thickness", b.max[1] - b.min[1], t, 1e-9);
    }
    assert_near("the wall", es.volume(), face * 200.0, 1e-9);
    // The quantities read it, and it is what the prisms made.
    let q = quantities::wall(&s.model, w).unwrap();
    assert_near("net volume", q.net_volume, face * 200.0, 1e-9);
    assert_near("gross volume", q.gross_volume, 8000.0 * 3000.0 * 200.0, 1e-9);
    assert_near("as the prisms summed it", quantities::wall_volume_by_prisms(&s.model, w, true), q.net_volume, 1e-9);
    // The door's leaf and the window's glass are boxes of their own: one closed solid each.
    let leaf = element_solids(&s.model, door);
    assert_sound(door, &leaf);
    assert_eq!(leaf.layers.len(), 1);
    let t = s.model.opening_type(s.model.elements.get(door).unwrap()).unwrap();
    assert_near("a door's leaf", leaf.volume(), 1000.0 * t.leaf() * 2100.0, 1e-9);
    let glass = element_solids(&s.model, window);
    assert_sound(window, &glass);
    assert_near("a window's glass", glass.volume(), 1200.0 * 50.0 * 1500.0, 1e-9);
}

// ----- a curved wall with a door ------------------------------------------------------------------------------------------

#[test]
fn a_curved_wall_with_a_door_is_closed_on_its_arcs() {
    let mut s = project();
    // A half circle, 2000 radius on its centre line, 200 thick and 3000 high: half a ring of radii 1900 to 2100, from (0, 0)
    // round to (4000, 0), the centre at (2000, 0).
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "arc": 180, "type": "T", "height": 3000})));
    run(&mut s, "door", json!({"wall": w, "offset": 1500, "type": "D"}));
    let es = element_solids(&s.model, w);
    assert!(es.is_whole(), "{:?}", es.open);
    assert_sound(w, &es);
    let solid = es.layer(0).expect("one layer");
    // Half the ring of 1900 to 2100, 3000 high, less the door's sector: 1000 along the centre line is 0.5 rad, 2100 high.
    let ring = PI / 2.0 * (2100.0 * 2100.0 - 1900.0 * 1900.0);
    let door = 0.5 / 2.0 * (2100.0 * 2100.0 - 1900.0 * 1900.0);
    let want = ring * 3000.0 - door * 2100.0;
    assert_near("on its arcs", es.volume(), want, 1e-9);
    // The facets alone are short of it, by what lies between each and its arc: about 0.13 % at a facet's 5 degrees.
    assert!(solid.volume() < want * (1.0 - 5e-4), "{} against {want}", solid.volume());
    assert_near("the facets", solid.volume(), want, 3e-3);
    // The curve tag is there: its triangles say the wall's two cylinders about its centre.
    let curve = solid.triangles().iter().find_map(|t| t.origin.curve).expect("a curve tag");
    assert!(near(curve.centre.0, 2000.0, 1e-9) && near(curve.centre.1, 0.0, 1e-9), "{curve:?}");
    assert!(near(curve.radii[0], 1900.0, 1e-9) && near(curve.radii[1], 2100.0, 1e-9), "{curve:?}");
    // A section through it at mid height shows arcs of exactly those radii: the two faces of each side of the door.
    let loops = solid.slice(&Frame::plan(1500.0));
    let arcs: Vec<f64> = loops
        .iter()
        .flat_map(|l| l.segs.iter())
        .filter_map(|g| if let Seg::Arc { radius, .. } = g { Some(*radius) } else { None })
        .collect();
    assert_eq!(arcs.len(), 4, "{loops:?}");
    assert!(arcs.iter().all(|r| near(*r, 1900.0, 1e-9) || near(*r, 2100.0, 1e-9)), "{arcs:?}");
    // Through the door the section is the one run of wall over its head.
    let over = solid.slice(&Frame::plan(2500.0));
    assert_eq!(over.len(), 1, "{over:?}");
    // The quantities, which the arcs make exact.
    let q = quantities::wall(&s.model, w).unwrap();
    assert_near("net volume", q.net_volume, want, 1e-9);
    assert_near("gross volume", q.gross_volume, ring * 3000.0, 1e-9);
}

// ----- a floor with a hole and a shaft ------------------------------------------------------------------------------------

const PLAN: [[i32; 2]; 4] = [[0, 0], [8000, 0], [8000, 6000], [0, 6000]];

#[test]
fn a_holed_floor_less_a_shaft_is_one_closed_solid() {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Screed"}));
    run(&mut s, "material", json!({"name": "Slab"}));
    run(
        &mut s,
        "type",
        json!({"category": "floor", "name": "F", "layers": [
            {"function": "finish", "material": "Screed", "thickness": 50},
            {"function": "structure", "material": "Slab", "thickness": 150}]}),
    );
    let f = id_of(&run(
        &mut s,
        "floor",
        json!({"boundary": PLAN, "type": "F", "openings": [[[1000, 1000], [2000, 1000], [2000, 2000], [1000, 2000]]]}),
    ));
    let whole = element_solids(&s.model, f);
    assert!(whole.is_whole());
    assert_sound(f, &whole);
    // 8 x 6 m less 1 m², through 200: the layers 50 and 150.
    assert_near("without a shaft", whole.volume(), 47e6 * 200.0, 1e-9);
    assert_near("its top layer", whole.layer(0).unwrap().volume(), 47e6 * 50.0, 1e-9);
    assert_near("its bottom layer", whole.layer(1).unwrap().volume(), 47e6 * 150.0, 1e-9);
    // A shaft 1.5 m x 1 m through its middle, from below to above.
    run(
        &mut s,
        "shaft",
        json!({"boundary": [[4000, 2000], [5500, 2000], [5500, 3000], [4000, 3000]], "base_offset": -1000, "height": 3000}),
    );
    let es = element_solids(&s.model, f);
    assert!(es.is_whole(), "{:?}", es.open);
    assert_sound(f, &es);
    assert_near("less the shaft", es.volume(), (47e6 - 1.5e6) * 200.0, 1e-9);
    assert_near("its top layer", es.layer(0).unwrap().volume(), (47e6 - 1.5e6) * 50.0, 1e-9);
    let Some(Element::Floor(fl)) = s.model.elements.get(f) else { panic!("no floor") };
    assert_near("as the floor measured it", s.model.floor_volume(fl), es.volume(), 1e-9);
    assert_near("quantities", quantities::floor_at(&s.model, f, fl).net_volume, (47e6 - 1.5e6) * 200.0, 1e-9);
    assert_near("quantities, from the reference", quantities::floor(&s.model, fl).net_volume, (47e6 - 1.5e6) * 200.0, 1e-9);
}

// ----- a sloped roof with walls attached to it ----------------------------------------------------------------------------

#[test]
fn a_sloped_roof_and_the_walls_attached_to_it_are_closed_and_exact() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "T", "thickness": 200}));
    run(&mut s, "type", json!({"category": "roof", "name": "R", "thickness": 300}));
    run(&mut s, "level", json!({"name": "Eaves", "elevation": 2000}));
    let roof = id_of(&run(
        &mut s,
        "roof",
        json!({"boundary": [[-1000, -1000], [9000, -1000], [9000, 7000], [-1000, 7000]], "overhang": 0, "slope": 30, "gable": true, "type": "R"}),
    ));
    // A wall 6000 long at x = 8000 under the gable, its top the roof's underside.
    let w = id_of(&run(&mut s, "wall", json!({"points": [[8000, 0], [8000, 6000]], "type": "T", "attach_top": roof})));
    let tan = 30f64.to_radians().tan();
    let cos = 30f64.to_radians().cos();
    let r = element_solids(&s.model, roof);
    assert!(r.is_whole(), "{:?}", r.open);
    assert_sound(roof, &r);
    // Its plan 10 x 8 m through 300 square to the slope, 300 / cos 30 plumb.
    assert_near("the roof", r.volume(), 80e6 * 300.0 / cos, 1e-9);
    let es = element_solids(&s.model, w);
    assert!(es.is_whole(), "{:?}", es.open);
    assert_sound(w, &es);
    // The wall stands from its base to the roof's underside, up to the ridge at y = 3000 and back down.
    // (the roof's underside is on the level, as the quantities' tests have it: 2000 + tan 30 (1000 + y)).
    let side = 2000.0 * 6000.0 + tan * 15e6;
    assert_near("the wall", es.volume(), side * 200.0, 1e-6);
    let q = quantities::wall(&s.model, w).unwrap();
    assert_near("quantities", q.net_volume, es.volume(), 1e-12);
    assert_near("as the prisms summed it", quantities::wall_volume_by_prisms(&s.model, w, true), q.net_volume, 1e-9);
}

// ----- the cache ----------------------------------------------------------------------------------------------------------

#[test]
fn the_scene_keeps_each_elements_solids_until_an_edit_touches_it() {
    let mut s = project();
    // Walls far apart, so an edit to one makes only it again (a scene made again past a quarter of its elements is made whole).
    let walls: Vec<u64> = (0..16)
        .map(|k| id_of(&run(&mut s, "wall", json!({"points": [[0, k * 9000], [5000, k * 9000]], "type": "T", "height": 3000}))))
        .collect();
    let (a, b) = (walls[0], walls[1]);
    let before = s.model.clone();
    let scene = Scene::new(&before);
    assert_eq!(scene.solids_kept(), 0);
    let kept: Vec<_> = walls.iter().map(|&w| scene.element_solids(&before, w)).collect();
    assert_eq!(scene.solids_kept(), 16);
    assert!(std::sync::Arc::ptr_eq(&kept[0], &scene.element_solids(&before, a)), "asked again, it is the one kept");
    // A door in wall a: the scene makes a's triangles again (and its door's), and keeps the other walls' solids.
    run(&mut s, "door", json!({"wall": a, "offset": 2500, "type": "D"}));
    let after = s.model.clone();
    let next = scene.update(&before, &after);
    assert_eq!(next.solids_kept(), 15, "every wall the edit did not touch is kept");
    assert!(std::sync::Arc::ptr_eq(&kept[1], &next.element_solids(&after, b)), "an untouched wall's solids are the same ones");
    let again = next.element_solids(&after, a);
    assert!(!std::sync::Arc::ptr_eq(&kept[0], &again));
    assert_near("a with its door", again.volume(), (5000.0 * 3000.0 - 1000.0 * 2100.0) * 200.0, 1e-9);
    assert_near("a before", kept[0].volume(), 5000.0 * 3000.0 * 200.0, 1e-9);
    // The same update from a full scene gives the same solids.
    let whole = Scene::new(&after);
    assert_near("as a whole scene makes it", whole.element_solids(&after, a).volume(), again.volume(), 1e-12);
    // An edit that touches a wall's neighbour makes the neighbour's solids again: a wall ending on another is joined to it.
    run(&mut s, "wall", json!({"points": [[5000, 0], [5000, 4000]], "type": "T", "height": 3000}));
    let joined = s.model.clone();
    let third = next.update(&after, &joined);
    assert!(
        third.solids_kept() < next.solids_kept(),
        "{} of {}: the wall it ends on is made again",
        third.solids_kept(),
        next.solids_kept()
    );
    assert_near("a, joined", third.element_solids(&joined, a).volume(), Scene::new(&joined).element_solids(&joined, a).volume(), 1e-12);
}

// ----- every element of the sample models ---------------------------------------------------------------------------------

/// What making every element's solids found: how many of what, and the open pieces by why.
#[derive(Default, Debug)]
struct Census {
    elements: usize,
    with_solids: usize,
    layers: usize,
    open: usize,
    nothing: Vec<(Id, &'static str)>,
    open_by: std::collections::BTreeMap<String, usize>,
}

fn census(m: &crate::model::Model) -> (Census, std::collections::BTreeMap<Id, ElementSolids>) {
    let mut c = Census::default();
    let mut all = std::collections::BTreeMap::new();
    let index = crate::walls::Index::new(m);
    let mut components = crate::component_geometry::Components::new(m);
    for (&id, e) in m.elements.iter() {
        let es = crate::solids::element_solids_in(m, id, Some(&index), &mut components);
        c.elements += 1;
        if !es.layers.is_empty() {
            c.with_solids += 1;
        } else if es.open.is_empty() {
            c.nothing.push((id, e.category()));
        }
        c.layers += es.layers.len();
        c.open += es.open.len();
        for o in &es.open {
            let kind = match &o.piece {
                crate::solids::Solid::Box { .. } => "box",
                crate::solids::Solid::Slab { .. } => "slab",
                crate::solids::Solid::Prism { .. } => "prism",
                crate::solids::Solid::Mesh { .. } => "mesh",
            };
            let why = match &o.why {
                crate::solid::SolidError::NotClosed { .. } => "not closed".to_string(),
                crate::solid::SolidError::Degenerate(d) => d.clone(),
                crate::solid::SolidError::Manifold(d) => format!("refused: {d}"),
            };
            *c.open_by.entry(format!("{} {kind}: {why}", e.category())).or_default() += 1;
        }
        all.insert(id, es);
    }
    (c, all)
}

#[test]
fn every_element_of_the_sample_house_has_closed_solids_or_a_recorded_open_piece() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let m = &s.model;
    let (c, all) = census(m);
    eprintln!("{c:#?}");
    for (id, es) in &all {
        assert_sound(*id, es);
    }
    assert!(c.with_solids > 25, "{c:?}");
    // Every door and window of the house, the family ones too, is closed: nothing is left open (the windows' forms once were).
    assert_eq!(c.open, 0, "{c:?}");
    // Walls: the sum of their volumes is what the quantities were (the prisms summed), and what they are now.
    let (mut by_solids, mut by_prisms, mut walls) = (0.0, 0.0, 0);
    for (&id, e) in m.elements.iter() {
        if matches!(e, Element::Wall(_)) {
            walls += 1;
            let q = quantities::wall(m, id).unwrap();
            by_solids += q.net_volume;
            by_prisms += quantities::wall_volume_by_prisms(m, id, true);
            assert_near(&format!("wall {id}"), all[&id].volume(), q.net_volume, 1e-9);
        }
    }
    eprintln!("{walls} walls: {by_solids:.3} by their solids, {by_prisms:.3} by their prisms");
    assert!(walls >= 7, "{walls} walls");
    assert_near("the walls' volumes", by_solids, by_prisms, 1e-9);
    // Floors, roofs and ceilings: the solid's volume is what each measured before it.
    for (&id, e) in m.elements.iter() {
        match e {
            Element::Floor(f) => assert_near("a floor", all[&id].volume(), m.floor_volume(f), 1e-9),
            Element::Ceiling(cl) => {
                let net: f64 = m
                    .ceiling_slabs(cl)
                    .iter()
                    .map(|(face, rise)| m.less_shafts(face, *rise).0.iter().map(|r| crate::geom::area(r).abs()).sum::<f64>())
                    .sum();
                let width = m.types.ceiling.get(&cl.type_name).map_or(0.0, |t| t.thickness);
                assert_near("a ceiling", all[&id].volume(), net * width, 1e-9);
            }
            _ => {}
        }
    }
}

#[test]
fn every_element_of_a_tower_and_a_highrise_has_closed_solids_or_a_recorded_open_piece() {
    for (name, model) in [
        ("tower, small plate, one storey", crate::sample::tower::tower_on(crate::sample::tower::Plate::SMALL, 1).unwrap()),
        ("highrise, two storeys", crate::sample::highrise(2).unwrap()),
    ] {
        let (c, all) = census(&model);
        eprintln!("{name}: {} elements, {} with solids, {} layers, {} open pieces", c.elements, c.with_solids, c.layers, c.open);
        eprintln!("  nothing: {:?}", {
            let mut kinds: std::collections::BTreeMap<&str, usize> = Default::default();
            c.nothing.iter().for_each(|(_, k)| *kinds.entry(k).or_default() += 1);
            kinds
        });
        eprintln!("  open: {:?}", c.open_by);
        for (id, es) in &all {
            assert_sound(*id, es);
        }
        let (mut by_solids, mut by_prisms) = (0.0, 0.0);
        for (&id, e) in model.elements.iter() {
            if matches!(e, Element::Wall(_)) {
                by_solids += all[&id].volume();
                by_prisms += quantities::wall_volume_by_prisms(&model, id, true);
            }
        }
        eprintln!("  walls: {by_solids:.3} by solids, {by_prisms:.3} by prisms");
        assert_near("wall volumes", by_solids, by_prisms, 1e-9);
        assert!(c.with_solids > 100, "{name}: {c:?}");
    }
}

/// Origins on a solid made of several elements' layers survive: a union of two elements' solids still says which triangle is whose.
#[test]
fn origins_name_the_element_and_layer() {
    let mut s = project();
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0]], "type": "L", "height": 2000})));
    let es = element_solids(&s.model, w);
    let all = es.layers.iter().map(|(_, s)| s.clone()).collect::<Vec<_>>();
    let union = crate::solid::Solid3::union_all(&all);
    assert!(union.is_closed());
    let layers: std::collections::BTreeSet<u16> = union.triangles().iter().map(|t| t.origin.layer).collect();
    assert_eq!(layers, [0, 1, 2].into_iter().collect());
    assert!(union.triangles().iter().all(|t| t.origin != Origin::NONE && t.origin.element == w));
}

// ----- the other kinds ---------------------------------------------------------------------------------------------------

#[test]
fn columns_stairs_ramps_beams_and_pads_are_solids_and_a_round_column_is_the_circles() {
    let mut s = Session::new();
    let round = id_of(&run(&mut s, "column", json!({"at": [5000, 3000], "type": "Round 300"})));
    let square = id_of(&run(&mut s, "column", json!({"at": [1000, 7000]})));
    let structural = id_of(&run(&mut s, "structural_column", json!({"at": [3000, 7000]})));
    let stair = id_of(&run(&mut s, "stair", json!({"start": [3000, 9000], "end": [3000, 12000], "height": 3000, "railings": true})));
    let ramp = id_of(&run(&mut s, "ramp", json!({"start": [6000, 9000], "end": [6000, 12000], "height": 600})));
    let beam = id_of(&run(&mut s, "beam", json!({"points": [[0, 15000], [4000, 15000]]})));
    let pad = id_of(&run(&mut s, "floor", json!({"boundary": [[0, 18000], [3000, 18000], [3000, 21000], [0, 21000]], "offset": 200})));
    for id in [round, square, structural, stair, ramp, beam, pad] {
        let es = element_solids(&s.model, id);
        assert!(es.is_whole(), "element {id}: {:?}", es.open);
        assert!(!es.layers.is_empty(), "element {id} has no solid");
        assert_sound(id, &es);
    }
    // A round column is a 24-gon of its circle's radius, taken as the circle it is: pi r² through its height.
    let q = quantities::column(&s.model, round).unwrap();
    assert_near("a round column", q.volume, PI * 150.0 * 150.0 * q.height, 1e-9);
    let facets = element_solids(&s.model, round).layers[0].1.volume();
    assert!(facets < q.volume && facets > q.volume * 0.988, "{facets} against {}", q.volume);
    let curve = element_solids(&s.model, round).layers[0].1.triangles().iter().find_map(|t| t.origin.curve).expect("a curve tag");
    assert!(near(curve.radii[0], 150.0, 1e-9) && near(curve.centre.0, 5000.0, 1e-9), "{curve:?}");
    // A rectangular one is its width by its depth by its height.
    let q = quantities::column(&s.model, square).unwrap();
    let e = s.model.elements.get(square).unwrap();
    let t = s.model.column_type(e).unwrap();
    assert_near("a square column", q.volume, t.width * t.depth * q.height, 1e-9);
    assert!(quantities::column(&s.model, stair).is_none());
}

// ----- the time it takes --------------------------------------------------------------------------------------------------

/// What making every element's solids takes, cold (every one built) and warm (every one asked of a scene that holds them), on the
/// largest sample models. `cargo test -p bimcraft-core element_solids_timings --release -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn element_solids_timings() {
    use std::time::Instant;
    for (name, model) in [
        ("highrise, 20 storeys", crate::sample::highrise(20).unwrap()),
        ("tower, reference plate, one storey", crate::sample::tower::tower(1).unwrap()),
        ("sample house", {
            let mut s = Session::new();
            s.script(crate::sample::SAMPLE).unwrap();
            (*s.model).clone()
        }),
    ] {
        let ids: Vec<Id> = model.elements.iter().map(|(&id, _)| id).collect();
        let started = Instant::now();
        let (index, mut components) = (crate::walls::Index::new(&model), crate::component_geometry::Components::new(&model));
        let setup = started.elapsed();
        // The pieces alone, as the 3D view makes them: what the solids are built from.
        let started = Instant::now();
        let mut pieces = 0;
        for (&id, e) in model.elements.iter() {
            pieces += crate::solids::solids_in_with(&model, &[(id, e)], Some(&index), &mut components).len();
        }
        let pieces_time = started.elapsed();
        let started = Instant::now();
        let mut made = 0;
        let mut triangles = 0;
        for &id in &ids {
            let es = crate::solids::element_solids_in(&model, id, Some(&index), &mut components);
            made += es.layers.len();
            // Asking for the triangles makes Manifold do its booleans, which are lazy.
            triangles += es.layers.iter().map(|(_, s)| s.triangle_count()).sum::<usize>();
        }
        let cold = started.elapsed();
        let scene = Scene::new(&model);
        let started = Instant::now();
        for &id in &ids {
            scene.element_solids(&model, id);
        }
        let first = started.elapsed();
        let started = Instant::now();
        for &id in &ids {
            scene.element_solids(&model, id);
        }
        let warm = started.elapsed();
        let scene_started = Instant::now();
        let _ = Scene::new(&model);
        let scene_time = scene_started.elapsed();
        eprintln!(
            "{name}: {} elements, {pieces} pieces made in {pieces_time:.2?} (the 3D view's whole scene, triangles and planes: {scene_time:.2?});              {made} layer solids, {triangles} triangles. Cold {cold:.2?} (index and families {setup:.2?} more);              through a scene: first {first:.2?}, warm {warm:.2?}",
            ids.len()
        );
    }
}

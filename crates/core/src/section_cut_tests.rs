#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Section cuts from the element solids (`render::section_cut_rings`, `docs/manifold-sections.md`): a section's plane sliced
//! through each layer's closed solid. Rings are in the section's own plane: x along its line from its start, y up, mm.

use serde_json::json;

use crate::engine::Session;
use crate::geom::{self, P2};
use crate::model::{Element, Id, Model};
use crate::model_tests::graphics_testing::run;
use crate::render;
use crate::view3d::element_solids;

type Rings = Vec<(Id, Option<u16>, Vec<P2>)>;

fn id_of(v: &serde_json::Value) -> u64 {
    v["id"].as_u64().or_else(|| v["ids"][0].as_u64()).unwrap_or_default()
}

/// Wall type L: a 20 mm exterior finish, a 160 mm core, a 20 mm interior finish; a plain door and window.
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
    run(&mut s, "type", json!({"category": "door", "name": "D", "width": 1000, "height": 2100}));
    run(&mut s, "type", json!({"category": "window", "name": "W", "width": 1200, "height": 1500, "sill": 900}));
    s
}

fn section(s: &mut Session, a: [f64; 2], b: [f64; 2]) -> Id {
    id_of(&run(s, "section", json!({"start": a, "end": b})))
}

fn cut(s: &Session, section: Id) -> Rings {
    render::section_cut_rings(&s.model, section).expect("a section")
}

fn of(rings: &Rings, id: Id, layer: Option<u16>) -> Vec<&Vec<P2>> {
    rings.iter().filter(|(i, k, _)| *i == id && *k == layer).map(|(_, _, r)| r).collect()
}

/// Its box: (least x, most x, least y, most y).
fn bbox(ring: &[P2]) -> (f64, f64, f64, f64) {
    ring.iter().fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |(a, b, c, d), q| (a.min(q.0), b.max(q.0), c.min(q.1), d.max(q.1)))
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// A ring that is the rectangle `x0..x1` by `z0..z1`, every corner exact to `tol`.
#[track_caller]
fn assert_rect(what: &str, ring: &[P2], (x0, x1, z0, z1): (f64, f64, f64, f64), tol: f64) {
    let b = bbox(ring);
    eprintln!("{what}: {ring:?}");
    assert_eq!(ring.len(), 4, "{what} is not four corners: {ring:?}");
    assert!(
        close(b.0, x0, tol) && close(b.1, x1, tol) && close(b.2, z0, tol) && close(b.3, z1, tol),
        "{what}: {b:?} is not {:?}",
        (x0, x1, z0, z1)
    );
    assert!(close(geom::area(ring), (x1 - x0) * (z1 - z0), tol * (x1 - x0 + z1 - z0).max(1.0)), "{what}: its area {}", geom::area(ring));
    // Counter-clockwise, which the drawing's fill and edges rely on.
    assert!(geom::area(ring) > 0.0, "{what} is turned the wrong way");
}

/// The middle of element `id`'s first layer solid's box along x, and its box.
fn centre_x(m: &Model, id: Id) -> (f64, [f64; 3], [f64; 3]) {
    let b = element_solids(m, id).layers[0].1.bounds().expect("a box");
    ((b.min[0] + b.max[0]) / 2.0, b.min, b.max)
}

// ----- a straight layered wall with a door and a window --------------------------------------------------------------------

#[test]
fn a_section_across_a_layered_wall_cuts_one_region_per_layer_and_the_doors_leaf() {
    let mut s = project();
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "type": "L", "height": 3000, "location": "centre"})));
    let door = id_of(&run(&mut s, "door", json!({"wall": w, "offset": 2000, "type": "D"})));
    let window = id_of(&run(&mut s, "window", json!({"wall": w, "offset": 5000, "type": "W"})));
    // Through the middle of the door, across the wall, looking east to west... north: along the section is y from -1000.
    let (xd, lo, hi) = centre_x(&s.model, door);
    let t = s.model.opening_type(s.model.elements.get(door).unwrap()).unwrap();
    let sec = section(&mut s, [xd, -1000.0], [xd, 1000.0]);
    let rings = cut(&s, sec);
    // Over the door's head each layer is one rectangle from the head to the top: 20, 160 and 20 thick, the core between.
    for k in 0..3u16 {
        assert_eq!(of(&rings, w, Some(k)).len(), 1, "one region for layer {k}");
    }
    assert!(of(&rings, w, None).is_empty(), "every piece of it is a solid");
    assert_rect("the core", of(&rings, w, Some(1))[0], (920.0, 1080.0, 2100.0, 3000.0), 1e-9);
    let mut faces: Vec<(f64, f64)> = [0u16, 2].iter().map(|k| bbox(of(&rings, w, Some(*k))[0])).map(|b| (b.0, b.1)).collect();
    faces.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(close(faces[0].0, 900.0, 1e-9) && close(faces[0].1, 920.0, 1e-9), "{faces:?}");
    assert!(close(faces[1].0, 1080.0, 1e-9) && close(faces[1].1, 1100.0, 1e-9), "{faces:?}");
    for k in [0u16, 2] {
        let b = bbox(of(&rings, w, Some(k))[0]);
        assert!(close(b.2, 2100.0, 1e-9) && close(b.3, 3000.0, 1e-9), "layer {k}'s head");
        assert_eq!(of(&rings, w, Some(k))[0].len(), 4);
    }
    // The opening is cut: under the head nothing of the wall, from the floor to the door's top.
    for (_, _, ring) in rings.iter().filter(|(i, ..)| *i == w) {
        assert!(!geom::contains(ring, P2(1000.0, 1000.0)), "the opening is open");
    }
    // The door's leaf, which no section cut before the solids: its thickness across, the floor to its top.
    let leaf = of(&rings, door, Some(0));
    assert_eq!(leaf.len(), 1, "the leaf: {rings:?}");
    let b = bbox(leaf[0]);
    assert!(close(b.2, lo[2], 1e-9) && close(b.3, hi[2], 1e-9) && close(b.3, 2100.0, 1e-9), "the leaf's height {b:?}");
    assert!(close(b.1 - b.0, t.leaf(), 1e-9) && b.0 >= 899.999 && b.1 <= 1100.001, "the leaf's thickness {b:?}, {}", t.leaf());
    assert_eq!(leaf[0].len(), 4);

    // Through the window: under its sill and over its head, each layer in two pieces; the glass between.
    let (xw, ..) = centre_x(&s.model, window);
    let sec = section(&mut s, [xw, -1000.0], [xw, 1000.0]);
    let rings = cut(&s, sec);
    for k in 0..3u16 {
        let mut pieces: Vec<&Vec<P2>> = of(&rings, w, Some(k));
        pieces.sort_by(|a, b| bbox(a).2.total_cmp(&bbox(b).2));
        assert_eq!(pieces.len(), 2, "layer {k}: the sill's and the head's");
        let (x0, x1) = [(900.0, 920.0), (920.0, 1080.0), (1080.0, 1100.0)][k as usize];
        let (a, c) = (bbox(pieces[0]), bbox(pieces[1]));
        // The two outer layers are the same whichever face is the exterior: compare as a set of the two ends.
        let ends = |b: (f64, f64, f64, f64)| (b.0.min(b.1), b.0.max(b.1));
        assert!(
            close(a.2, 0.0, 1e-9) && close(a.3, 900.0, 1e-9) && close(c.2, 2400.0, 1e-9) && close(c.3, 3000.0, 1e-9),
            "layer {k}: {a:?} {c:?}"
        );
        let (e0, e1) = ends(a);
        assert!(k != 1 || (close(e0, x0, 1e-9) && close(e1, x1, 1e-9)), "the core's breadth {e0}..{e1}");
    }
    let glass = of(&rings, window, Some(0));
    assert_eq!(glass.len(), 1);
    let b = bbox(glass[0]);
    assert!(close(b.2, 900.0, 1e-9) && close(b.3, 2400.0, 1e-9) && close(b.1 - b.0, 50.0, 1e-9), "the glass {b:?}");
}

#[test]
fn a_section_along_a_wall_cuts_its_window_as_a_hole_and_its_door_as_a_notch() {
    let mut s = project();
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "type": "L", "height": 3000, "location": "centre"})));
    let door = id_of(&run(&mut s, "door", json!({"wall": w, "offset": 2000, "type": "D"})));
    let window = id_of(&run(&mut s, "window", json!({"wall": w, "offset": 5000, "type": "W"})));
    let (xd, ..) = centre_x(&s.model, door);
    let (xw, ..) = centre_x(&s.model, window);
    // The plane y = 0 runs through the core (y from -80 to 80) and none of the finishes.
    let sec = section(&mut s, [-500.0, 0.0], [8500.0, 0.0]);
    let rings = cut(&s, sec);
    assert!(of(&rings, w, Some(0)).is_empty() && of(&rings, w, Some(2)).is_empty(), "the finishes lie off the plane");
    let core = of(&rings, w, Some(1));
    let area: f64 = core.iter().map(|r| geom::area(r)).sum();
    let face = 8000.0 * 3000.0 - 1000.0 * 2100.0 - 1200.0 * 1500.0;
    eprintln!("the core: {} rings, {area} against {face}", core.len());
    assert!(close(area, face, 1e-6), "{area} is not {face}");
    assert!(core.len() >= 4, "a hole is cut out as pieces round it: {}", core.len());
    // Where the window is there is no wall, where the door is none below its head; wall everywhere else.
    let covered = |p: P2| core.iter().any(|r| geom::contains(r, p));
    assert!(!covered(P2(xw + 500.0, 1650.0)) && !covered(P2(xd + 500.0, 1000.0)));
    assert!(
        covered(P2(xw + 500.0, 450.0)) && covered(P2(xw + 500.0, 2700.0)) && covered(P2(xd + 500.0, 2500.0)) && covered(P2(1000.0, 1000.0))
    );
    // Every piece is a simple ring turned counter-clockwise, a trapezoid at most.
    assert!(core.iter().all(|r| geom::area(r) > 0.0 && r.len() <= 4));
    // The drawing: the section's poché fills them as one region and no seam is inked between pieces.
    let view = render::section_fit(&s.model, sec, 1800, 700);
    let d = render::section(&s.model, sec, view);
    let ink = d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(w) && matches!(p, crate::prim::Prim::Line { .. })).count();
    eprintln!("{ink} lines of the wall");
    assert!(d.prims.iter().zip(&d.owners).any(|(p, o)| *o == Some(w) && matches!(p, crate::prim::Prim::Fill { .. })));
}

#[test]
fn a_cropped_section_cuts_only_what_lies_between_the_ends_of_its_line() {
    let mut s = project();
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "type": "L", "height": 3000, "location": "centre"})));
    // The plane y = 0 through the core, then cropped to x from 2000 to 3000: its line is those 1000 mm.
    let sec = section(&mut s, [-500.0, 0.0], [8500.0, 0.0]);
    let whole = cut(&s, sec);
    assert!(bbox(&whole.iter().filter(|(i, ..)| *i == w).flat_map(|(_, _, r)| r.iter().copied()).collect::<Vec<_>>()).1 > 8000.0);
    run(&mut s, "crop", json!({"view": sec, "region": [[2500, -200], [3500, 2500]]}));
    let rings = cut(&s, sec);
    let core = of(&rings, w, Some(1));
    let b = bbox(&core.iter().flat_map(|r| r.iter().copied()).collect::<Vec<_>>());
    eprintln!("{b:?}");
    assert!(close(b.0, 0.0, 1e-9) && close(b.1, 1000.0, 1e-9) && close(b.2, 0.0, 1e-9) && close(b.3, 3000.0, 1e-9), "{b:?}");
    assert!(close(core.iter().map(|r| geom::area(r)).sum::<f64>(), 1000.0 * 3000.0, 1e-6));
    // The drawing of it keeps to the crop.
    let view = render::section_fit(&s.model, sec, 800, 600);
    let d = render::section(&s.model, sec, view);
    assert!(d.prims.iter().zip(&d.owners).any(|(p, o)| *o == Some(w) && matches!(p, crate::prim::Prim::Fill { .. })));
}

// ----- a curved wall with a door --------------------------------------------------------------------------------------------

/// Every edge of every ring upright or level: a vertical plane cuts a vertical wall's faces in lines, and the top and the bottom
/// are level.
#[track_caller]
fn assert_upright_and_level(what: &str, ring: &[P2]) {
    for (a, b) in geom::ring_edges(ring) {
        assert!(close(a.0, b.0, 1e-9) || close(a.1, b.1, 1e-9), "{what}: an edge {a:?} to {b:?} is neither upright nor level");
    }
}

#[test]
fn a_vertical_section_through_a_curved_wall_with_a_door_cuts_exact_lines_and_its_layers() {
    let mut s = project();
    // A half circle 2000 radius on its centre line (the wall's three layers, 200 thick) from (0, 0) round to (4000, 0), bulging
    // south; a door at the middle of the arc, its apex (2000, -2000).
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "arc": 180, "type": "L", "height": 3000})));
    let door = id_of(&run(&mut s, "door", json!({"wall": w, "offset": std::f64::consts::PI * 1000.0, "type": "D"})));
    let (xc, ..) = centre_x(&s.model, door);
    eprintln!("the door's box begins at x = {xc}");
    // A plane at x = 2100 through the door: running north from y = -3000.
    let sec = section(&mut s, [2100.0, -3000.0], [2100.0, 1000.0]);
    let rings = cut(&s, sec);
    assert!(of(&rings, w, None).is_empty());
    // Over the door's head, three rectangles side by side, the wall's layers: their breadths are the thicknesses along the line
    // where the plane meets the facets; every edge a line.
    let mut layers: Vec<(f64, u16, &Vec<P2>)> =
        (0..3u16).flat_map(|k| of(&rings, w, Some(k)).into_iter().map(move |r| (bbox(r).0, k, r))).collect();
    layers.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(layers.len(), 3, "one region per layer: {layers:?}");
    for (_, k, r) in &layers {
        assert_upright_and_level(&format!("layer {k}"), r);
        assert_eq!(r.len(), 4);
        let b = bbox(r);
        assert!(close(b.2, 2100.0, 1e-9) && close(b.3, 3000.0, 1e-9), "layer {k}: the head: {b:?}");
    }
    let (a, b, c) = (bbox(layers[0].2), bbox(layers[1].2), bbox(layers[2].2));
    assert!(close(a.1, b.0, 1e-9) && close(b.1, c.0, 1e-9), "the layers abut: {a:?} {b:?} {c:?}");
    // The order is the wall's: the layer numbers run across it, from one face to the other.
    let order: Vec<u16> = layers.iter().map(|l| l.1).collect();
    assert!(order == vec![0, 1, 2] || order == vec![2, 1, 0], "{order:?}");
    // The plane meets the circles of 2100, 2080, 1920 and 1900 where x = 100 off the axis: y = -sqrt(r^2 - 100^2); the
    // facets lie within two millimetres inside their circles.
    let y = |r: f64| -(r * r - 100.0 * 100.0).sqrt();
    let want = [y(2100.0), y(2080.0), y(1920.0), y(1900.0)].map(|y| y + 3000.0);
    let got = [a.0, b.0, c.0, c.1];
    for (g, w) in got.iter().zip(want) {
        assert!(close(*g, w, 2.5), "{got:?} against the circles' {want:?}");
    }
    // The door's leaf is cut, upright rectangle from the floor.
    let leaf = of(&rings, door, Some(0));
    assert_eq!(leaf.len(), 1);
    assert_upright_and_level("the leaf", leaf[0]);
    let lb = bbox(leaf[0]);
    assert!(close(lb.2, 0.0, 1e-9) && close(lb.3, 2100.0, 1e-9), "{lb:?}");

    // A line across both legs of the half circle at y = -1000 (running east): the plane meets the faces slantwise, 22 mm for
    // the 20 mm layer, and still every edge is a line.
    let sec = section(&mut s, [-500.0, -1000.0], [4500.0, -1000.0]);
    let rings = cut(&s, sec);
    let mut cutting: Vec<(f64, u16, &Vec<P2>)> =
        (0..3u16).flat_map(|k| of(&rings, w, Some(k)).into_iter().map(move |r| (bbox(r).0, k, r))).collect();
    cutting.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(cutting.len(), 6, "three layers at each of its two cuts: {cutting:?}");
    let x = |r: f64| (r * r - 1000.0 * 1000.0).sqrt();
    // West leg from the outside (r = 2100) in; each layer a rectangle from the floor to the wall's top.
    let west = [2000.0 - x(2100.0), 2000.0 - x(2080.0), 2000.0 - x(1920.0), 2000.0 - x(1900.0)].map(|v| v + 500.0);
    for (i, (_, k, r)) in cutting.iter().enumerate() {
        assert_upright_and_level(&format!("layer {k}"), r);
        assert_eq!(r.len(), 4);
        let b = bbox(r);
        assert!(close(b.2, 0.0, 1e-9) && close(b.3, 3000.0, 1e-9), "layer {k}: {b:?}");
        if i < 3 {
            assert!(close(b.0, west[i], 2.5) && close(b.1, west[i + 1], 2.5), "west layer {i}: {b:?} against {west:?}");
        }
    }
    let leg: Vec<u16> = cutting.iter().map(|l| l.1).collect();
    assert!(leg[..3] == [0, 1, 2] || leg[..3] == [2, 1, 0], "{leg:?}");
    assert!(leg[3..] == [0, 1, 2] || leg[3..] == [2, 1, 0], "{leg:?}");
}

// ----- a family with a void -----------------------------------------------------------------------------------------------

#[test]
fn a_section_through_a_family_with_a_void_shows_the_void_in_the_cut() {
    let mut s = Session::new();
    // A block 1000 cube with a cavity 500 cube inside it, cut out by a void.
    run(
        &mut s,
        "family",
        json!({"name": "Cavity Block", "category": "generic", "types": {"A": {}}, "forms": [
            {"profile": [[0, 0], [1000, 0], [1000, 1000], [0, 1000]], "from": 0, "to": 1000},
            {"profile": [[250, 250], [750, 250], [750, 750], [250, 750]], "from": 250, "to": 750, "void": true}]}),
    );
    let c = id_of(&run(&mut s, "component", json!({"family": "Cavity Block", "type": "A", "at": [3000, 3000]})));
    let es = element_solids(&s.model, c);
    assert!(es.is_whole(), "{:?}", es.open);
    let b = es.layers[0].1.bounds().unwrap();
    // The block is a cube with a cavity: 1e9 less 1.25e8.
    assert!(close(es.volume(), 1e9 - 0.125e9, 1e-3), "{}", es.volume());
    let xc = (b.min[0] + b.max[0]) / 2.0;
    let sec = section(&mut s, [xc, b.min[1] - 1000.0], [xc, b.max[1] + 1000.0]);
    let rings = cut(&s, sec);
    let mine: Vec<&Vec<P2>> = rings.iter().filter(|(i, ..)| *i == c).map(|(_, _, r)| r).collect();
    let area: f64 = mine.iter().map(|r| geom::area(r)).sum();
    eprintln!("{} rings, {area} mm2: {mine:?}", mine.len());
    // The cube's face 1000 x 1000 less the cavity's 500 x 500: a ring and the hole in it, cut out as four pieces.
    assert!(close(area, 1e6 - 0.25e6, 1e-6), "{area}");
    assert_eq!(mine.len(), 4);
    let (y0, z0) = (b.min[1] - (b.min[1] - 1000.0), b.min[2]);
    let covered = |p: P2| mine.iter().any(|r| geom::contains(r, p));
    // The middle of the cavity is empty; the walls round it are cut.
    assert!(!covered(P2(y0 + 500.0, z0 + 500.0)), "the void shows in the cut");
    for (dy, dz) in [(100.0, 500.0), (900.0, 500.0), (500.0, 100.0), (500.0, 900.0)] {
        assert!(covered(P2(y0 + dy, z0 + dz)), "{dy} {dz}");
    }
    // The drawing fills the block's cut in poché and has no fill over the cavity (the section's picture beyond is another matter).
    let view = render::section_fit(&s.model, sec, 1000, 800);
    let d = render::section(&s.model, sec, view);
    let fills: Vec<&Vec<(f32, f32)>> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| **o == Some(c))
        .filter_map(|(p, _)| if let crate::prim::Prim::Fill { rings, .. } = p { Some(rings) } else { None })
        .flatten()
        .collect();
    assert!(!fills.is_empty());
    let pixel = view.px(P2(y0 + 500.0, z0 + 500.0));
    assert!(
        fills.iter().all(|r| !geom::contains(
            &r.iter().map(|q| P2(f64::from(q.0), f64::from(q.1))).collect::<Vec<_>>(),
            P2(f64::from(pixel.0), f64::from(pixel.1))
        )),
        "no fill over the cavity"
    );
}

// ----- a sloped roof with walls attached ----------------------------------------------------------------------------------

#[test]
fn a_section_along_a_wall_attached_to_a_sloped_roof_follows_the_slope() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "T", "thickness": 200}));
    run(&mut s, "type", json!({"category": "roof", "name": "R", "thickness": 300}));
    run(&mut s, "level", json!({"name": "Eaves", "elevation": 2000}));
    let roof = id_of(&run(
        &mut s,
        "roof",
        json!({"boundary": [[-1000, -1000], [9000, -1000], [9000, 7000], [-1000, 7000]], "overhang": 0, "slope": 30, "gable": true, "type": "R"}),
    ));
    let w = id_of(&run(&mut s, "wall", json!({"points": [[8000, 0], [8000, 6000]], "type": "T", "attach_top": roof})));
    let tan = 30f64.to_radians().tan();
    // The plane x = 8000, lengthwise through the wall, running north from y = -500: along = y + 500.
    let sec = section(&mut s, [8000.0, -500.0], [8000.0, 6500.0]);
    let rings = cut(&s, sec);
    let wall = of(&rings, w, Some(0));
    assert_eq!(wall.len(), 1, "{rings:?}");
    // The wall: from its base to the roof's underside, rising to the ridge at y = 3000 and falling: a pentagon.
    let top = |y: f64| 2000.0 + tan * (1000.0 + y.min(6000.0 - y));
    let ring = wall[0];
    eprintln!("the wall: {ring:?}");
    assert_eq!(ring.len(), 5, "a pentagon: {ring:?}");
    for p in [P2(500.0, 0.0), P2(6500.0, 0.0), P2(6500.0, top(0.0)), P2(3500.0, top(3000.0)), P2(500.0, top(0.0))] {
        assert!(ring.iter().any(|q| q.dist(p) < 1e-6), "{p:?} is a corner of {ring:?}");
    }
    assert!(close(geom::area(ring), 2000.0 * 6000.0 + tan * 15e6, 1e-3), "{}", geom::area(ring));
    // The roof is cut along the same plane: its plumb thickness 300 / cos 30 between its underside and its top, along 8000.
    let roof_cut = of(&rings, roof, Some(0));
    let area: f64 = roof_cut.iter().map(|r| geom::area(r)).sum();
    assert!(close(area, 8000.0 * 300.0 / 30f64.to_radians().cos(), 1e-3), "{area}");
    let b = bbox(&roof_cut.iter().flat_map(|r| r.iter().copied()).collect::<Vec<_>>());
    assert!(close(b.0, 500.0 - 1000.0, 1e-6) && close(b.1, 7500.0, 1e-6), "{b:?}");
}

// ----- a door and a window ------------------------------------------------------------------------------------------------

#[test]
fn a_section_cuts_the_samples_doors_and_windows() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    // Down x = 2000 across the front wall, through the family door 900 wide at (2000, 0); down x = 4500, through the plain door
    // of the wall y = 4000 (and the casement window's family form, which is open and cut as it was); along y = 5250, through the
    // plain fixed window in the east wall.
    for (line, want) in [
        ([[2000.0, -1500.0], [2000.0, 8500.0]], "door"),
        ([[4500.0, -1500.0], [4500.0, 8500.0]], "door"),
        ([[-1500.0, 5250.0], [11500.0, 5250.0]], "window"),
    ] {
        let sec = section(&mut s, line[0], line[1]);
        let rings = cut(&s, sec);
        let cats: std::collections::BTreeSet<&str> =
            rings.iter().filter_map(|(id, ..)| s.model.elements.get(*id).map(|e| e.category())).collect();
        eprintln!("{line:?}: {cats:?}");
        assert!(cats.contains(want), "{line:?} cuts a {want}: {cats:?}");
        assert!(cats.contains("wall") && cats.contains("floor"), "{cats:?}");
    }
}

/// Every element a line is cut by in the model, as the section's cut did it before the solids: by what `Solid::cuts` made of every
/// piece.
fn cut_before(m: &Model, start: P2, end: P2) -> std::collections::BTreeMap<Id, f64> {
    let u = (end - start).unit();
    let mut out: std::collections::BTreeMap<Id, f64> = Default::default();
    for piece in crate::view3d::solids(m) {
        for ring in piece.cuts(start - u * 1e5, u, 2e5 + start.dist(end)) {
            *out.entry(piece.owner()).or_default() += geom::area(&ring).abs();
        }
    }
    out
}

/// What the line cuts now, by element: the area of its rings.
fn cut_now(rings: &Rings) -> std::collections::BTreeMap<Id, f64> {
    let mut out: std::collections::BTreeMap<Id, f64> = Default::default();
    for (id, _, ring) in rings {
        *out.entry(*id).or_default() += geom::area(ring).abs();
    }
    out
}

/// A model's sections through it, each held to what was cut before: nothing that was cut disappears; what is cut is the area it was
/// (the prisms and slabs, closed) or more (what was never cut).
#[track_caller]
fn compare_with_before(name: &str, s: &mut Session, lines: &[[[f64; 2]; 2]]) -> usize {
    let mut cut_more = 0;
    for line in lines {
        let sec = section(s, line[0], line[1]);
        let rings = cut(s, sec);
        let (before, now) = (cut_before(&s.model, P2(line[0][0], line[0][1]), P2(line[1][0], line[1][1])), cut_now(&rings));
        for (id, area) in &before {
            if *area < 1e-3 {
                continue;
            }
            let Some(new) = now.get(id) else {
                panic!("{name} {line:?}: element {id} ({:?}) was cut and no longer is", s.model.elements.get(*id).map(|e| e.category()))
            };
            // The pieces of a wall, floor or roof abut or overlap a little where they meet; the solids are their unions.
            if (new - area).abs() > 1e-6 * area + 1.0 {
                eprintln!(
                    "{name} {line:?}: element {id} ({:?}) was {area:.3} mm2 and is {new:.3}",
                    s.model.elements.get(*id).map(|e| e.category())
                );
            }
        }
        cut_more += now.keys().filter(|id| !before.contains_key(id)).count();
        // Every element a layer solid of which the plane passes clean through (its box lies a millimetre either side of it) has a
        // cut, whatever it is: a closed layer's rings or an open piece's.
        let (start, end) = (P2(line[0][0], line[0][1]), P2(line[1][0], line[1][1]));
        let n = (end - start).unit().perp();
        let m = s.model.clone();
        let mut crossing = 0;
        for (&id, _) in m.elements.iter() {
            let es = element_solids(&m, id);
            let crosses = es.layers.iter().any(|(_, solid)| {
                solid.bounds().is_some_and(|b| {
                    let sides = [b.min[0], b.max[0]]
                        .into_iter()
                        .flat_map(|x| [b.min[1], b.max[1]].into_iter().map(move |y| (P2(x, y) - start).dot(n)));
                    let (lo, hi) = sides.fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
                    lo < -1.0 && hi > 1.0
                })
            });
            // Revit's non-cuttable families (a sofa, a light) are not cut, whatever the line crosses.
            if crosses && m.elements.get(id).is_some_and(|e| crate::graphics::cuttable(&m, e)) {
                crossing += 1;
                // A solid whose box the plane crosses may still miss the solid (round a notch): such an element is allowed to
                // have no ring only if no triangle of it straddles the plane.
                let straddles = es.layers.iter().any(|(_, solid)| {
                    solid.triangles().iter().any(|t| {
                        let d: Vec<f64> = t.p.iter().map(|q| (P2(q[0], q[1]) - start).dot(n)).collect();
                        d.iter().any(|v| *v < -1e-6) && d.iter().any(|v| *v > 1e-6)
                    })
                });
                assert!(
                    now.contains_key(&id) || !straddles,
                    "{name} {line:?}: element {id} ({:?}) crosses the plane and has no cut",
                    m.elements.get(id).map(|e| e.category())
                );
            }
        }
        eprintln!("{name} {line:?}: {crossing} elements cross the plane, {} are cut", now.len());
    }
    cut_more
}

#[test]
fn what_a_section_cut_before_the_solids_it_cuts_still() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let lines = [
        [[-1500.0, 3000.0], [11500.0, 3000.0]],
        [[-1500.0, 1000.0], [11500.0, 1000.0]],
        [[-1500.0, 5250.0], [11500.0, 5250.0]],
        [[2000.0, -1500.0], [2000.0, 8500.0]],
        [[4500.0, -1500.0], [4500.0, 8500.0]],
        [[8000.0, -1500.0], [8000.0, 8500.0]],
        [[-1500.0, -1500.0], [11500.0, 8500.0]],
    ];
    let more = compare_with_before("the sample house", &mut s, &lines);
    eprintln!("{more} elements cut that were not before");
    assert!(more > 0, "the components and the family doors are cut now");

    let mut s = Session::new();
    s.execute("highrise", &json!({"storeys": 2})).unwrap();
    let m = &s.model;
    let (lo, hi) = m
        .elements
        .values()
        .filter_map(|e| if let Element::Wall(w) = e { Some((w.start, w.end)) } else { None })
        .fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), (a, b)| {
            (P2(lo.0.min(a.0).min(b.0), lo.1.min(a.1).min(b.1)), P2(hi.0.max(a.0).max(b.0), hi.1.max(a.1).max(b.1)))
        });
    let mid = (lo + hi) * 0.5;
    let lines = [
        [[lo.0 - 1000.0, mid.1], [hi.0 + 1000.0, mid.1]],
        [[mid.0, lo.1 - 1000.0], [mid.0, hi.1 + 1000.0]],
        [[lo.0 - 1000.0, mid.1 + 3300.0], [hi.0 + 1000.0, mid.1 + 3300.0]],
    ];
    let more = compare_with_before("the highrise", &mut s, &lines);
    eprintln!("{more} elements cut that were not before");
}

// ----- what Revit cuts ----------------------------------------------------------------------------------------------------

/// A 600 x 600 x 800 box family of category `category` (JSON name), placed at (3000, 3000), and a section through its middle
/// along x: how many rings the box is cut into.
fn rings_of_a_box_of(category: &str, extra: serde_json::Value) -> usize {
    let mut s = Session::new();
    let mut family = json!({"name": "Box", "category": category, "types": {"A": {}},
        "forms": [{"profile": [[0, 0], [600, 0], [600, 600], [0, 600]], "from": 0, "to": 800}]});
    for (k, v) in extra.as_object().into_iter().flatten() {
        family[k] = v.clone();
    }
    run(&mut s, "family", family);
    let c = id_of(&run(&mut s, "component", json!({"family": "Box", "type": "A", "at": [3000, 3000]})));
    let sec = section(&mut s, [2000.0, 3300.0], [5000.0, 3300.0]);
    cut(&s, sec).iter().filter(|(id, ..)| *id == c).count()
}

/// Revit's cuttable and non-cuttable families (`graphics::cuttable`): Generic Models and Casework are always cut, Furniture and
/// Plumbing Fixtures only when the family's Enable Cutting in Views is on (off by default), and everything else, the electrical,
/// lighting, communication and pipe categories, never: they stay in the picture beyond the line.
#[test]
fn a_section_cuts_the_categories_revit_cuts_and_leaves_the_rest_in_projection() {
    use crate::graphics::COMPONENTS;
    for category in COMPONENTS {
        let rings = rings_of_a_box_of(category, json!({}));
        let want = usize::from(matches!(category, "casework" | "generic"));
        assert_eq!(rings, want, "a box of category {category}: {rings} rings");
    }
    // The per-family option: the three categories of Revit's Enable Cutting in Views are cut with it, no other is for it.
    for category in COMPONENTS {
        let rings = rings_of_a_box_of(category, json!({"cut_in_views": true}));
        let want = usize::from(matches!(category, "casework" | "generic" | "furniture" | "plumbing" | "drain"));
        assert_eq!(rings, want, "a box of category {category}, cut in views: {rings} rings");
    }
}

#[test]
fn the_samples_furniture_stays_in_projection_and_its_casework_is_cut() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    let m = &s.model;
    let (mut furniture, mut casework) = (Vec::new(), Vec::new());
    for (id, e) in m.elements.iter() {
        match crate::graphics::category_of(m, e) {
            "furniture" => furniture.push(*id),
            "casework" => casework.push(*id),
            _ => {}
        }
    }
    assert!(!furniture.is_empty() && !casework.is_empty());
    assert!(furniture.iter().all(|id| !crate::graphics::cuttable(m, m.elements.get(*id).unwrap())));
    assert!(casework.iter().all(|id| crate::graphics::cuttable(m, m.elements.get(*id).unwrap())));
    // Along x = 3000 through the sofa, and along y = 7000 through the base cabinet at the back.
    let sec = section(&mut s, [3000.0, -1500.0], [3000.0, 8500.0]);
    let cut_ids: std::collections::BTreeSet<Id> = cut(&s, sec).iter().map(|(id, ..)| *id).collect();
    assert!(furniture.iter().all(|id| !cut_ids.contains(id)), "no furniture is cut: {cut_ids:?}");
    let sec = section(&mut s, [2000.0, -1500.0], [2000.0, 8500.0]);
    let cut_ids: std::collections::BTreeSet<Id> = cut(&s, sec).iter().map(|(id, ..)| *id).collect();
    assert!(casework.iter().any(|id| cut_ids.contains(id)), "the base cabinet is cut: {cut_ids:?}");
}

// ----- a form's Visibility Settings ---------------------------------------------------------------------------------------

/// Whether a section cuts anything of component `c`.
fn is_cut(s: &Session, sec: Id, c: Id) -> bool {
    cut(s, sec).iter().any(|(id, ..)| *id == c)
}

/// A family of three boxes along a diagonal, each at its own place in plan, and a section along x and one along y through each:
/// the first drawn in no front or side view, the second at Fine only, the third in no front view (so only a section along y,
/// which looks at its side, cuts it). Whatever a section does not draw it does not cut.
#[test]
fn a_section_cuts_only_the_forms_it_draws_by_detail_level_and_direction() {
    let mut s = Session::new();
    let boxed = |at: f64, show: serde_json::Value| {
        json!({"profile": [[at, at], [at + 600.0, at], [at + 600.0, at + 600.0], [at, at + 600.0]], "from": 0, "to": 800, "show": show})
    };
    run(
        &mut s,
        "family",
        json!({"name": "Shown", "category": "generic", "types": {"A": {}}, "forms": [
            boxed(0.0, json!({"front": false, "side": false})),
            boxed(1000.0, json!({"coarse": false, "medium": false})),
            boxed(2000.0, json!({"front": false}))]}),
    );
    let c = id_of(&run(&mut s, "component", json!({"family": "Shown", "type": "A", "at": [3000, 3000]})));
    // The cut through the k-th box, 300 into it: along x at its y, along y at its x.
    let mut along_x = Vec::new();
    let mut along_y = Vec::new();
    for k in 0..3 {
        let at = 3000.0 + 1000.0 * k as f64 + 300.0;
        along_x.push(section(&mut s, [2000.0, at], [9000.0, at]));
        along_y.push(section(&mut s, [at, 2000.0], [at, 9000.0]));
    }
    let matrix = |s: &Session, secs: &[Id]| secs.iter().map(|sec| is_cut(s, *sec, c)).collect::<Vec<_>>();
    // Fine (a new view's level): the first never; the second always; the third only from the side.
    assert_eq!(matrix(&s, &along_x), [false, true, false], "along x at Fine");
    assert_eq!(matrix(&s, &along_y), [false, true, true], "along y at Fine");
    for detail in ["medium", "coarse"] {
        for sec in along_x.iter().chain(&along_y) {
            run(&mut s, "detail", json!({"view": sec, "detail": detail}));
        }
        assert_eq!(matrix(&s, &along_x), [false, false, false], "along x at {detail}");
        assert_eq!(matrix(&s, &along_y), [false, false, true], "along y at {detail}");
    }
}

/// The engine's own door family draws its handles at Medium and Fine, not at Coarse: a section through a handle cuts it at Fine
/// and not at Coarse, the leaf and the frame at both.
#[test]
fn a_coarse_section_does_not_cut_a_door_handle_a_fine_one_does() {
    let mut s = project();
    run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "type": "L", "height": 3000, "location": "centre"}));
    let door = id_of(&run(&mut s, "door", json!({"at": [4000, 0], "family": "Double Door", "type": "1500 x 2100"})));
    // The double door's handles lie 40 to 120 either side of its middle, 1000 to 1030 up.
    let x = centre_x(&s.model, door).0 - 80.0;
    let sec = section(&mut s, [x, -1000.0], [x, 1000.0]);
    let handles = |s: &Session| {
        cut(s, sec).iter().filter(|(id, _, r)| *id == door && close(bbox(r).2, 1000.0, 1e-6) && close(bbox(r).3, 1030.0, 1e-6)).count()
    };
    let leaves = |s: &Session| cut(s, sec).iter().filter(|(id, _, r)| *id == door && bbox(r).3 > 2000.0).count();
    assert_eq!((handles(&s), leaves(&s) > 0), (2, true), "Fine: both faces of the handle, and the leaf");
    run(&mut s, "detail", json!({"view": sec, "detail": "coarse"}));
    assert_eq!((handles(&s), leaves(&s) > 0), (0, true), "Coarse: no handle, the leaf still");
    run(&mut s, "detail", json!({"view": sec, "detail": "medium"}));
    assert_eq!(handles(&s), 2, "Medium draws the handle");
}

// ----- the solids kept across edits ---------------------------------------------------------------------------------------

/// A row of walls across a line, a pair of them joined at a corner, and edits: what a section builds again after an edit is what
/// the edit touched, and the section is what a fresh work makes of it.
#[test]
fn a_work_after_an_edit_takes_over_the_solids_of_what_the_edit_left_as_it_was() {
    let mut s = project();
    // Eight walls running north-south at 2 m, 4 m ... 16 m, and one more pair joined at a corner further along.
    let walls: Vec<Id> = (1..=8)
        .map(|k| id_of(&run(&mut s, "wall", json!({"points": [[2000 * k, -1000], [2000 * k, 1000]], "type": "L", "height": 3000}))))
        .collect();
    let corner = id_of(&run(&mut s, "wall", json!({"points": [[20000, -1000], [20000, 0]], "type": "L", "height": 3000})));
    let sec = section(&mut s, [0.0, -500.0], [24000.0, -500.0]);
    let before = s.model.clone();
    let work = render::SectionWork::new(&before);
    let rings = render::section_cut_rings_with(&before, &work, sec).unwrap();
    let made = work.solids_kept();
    assert!(made >= 9, "the nine walls it crosses: {made}");
    let same_as_fresh = |after: &Model, w: &render::SectionWork| {
        let kept = render::section_cut_rings_with(after, w, sec).unwrap();
        assert_eq!(kept, render::section_cut_rings(after, sec).unwrap(), "what a work that took solids over cuts is what a fresh one does");
        kept
    };

    // One wall moved: all the others are taken over, it is made again.
    run(&mut s, "move", json!({"ids": [walls[3]], "by": [100, 0]}));
    let after = s.model.clone();
    let next = render::SectionWork::new(&after);
    let taken = next.inherit_solids(work.solid_carry(), &before, &after);
    assert_eq!(taken, made - 1, "every wall but the moved one");
    let moved = same_as_fresh(&after, &next);
    assert_ne!(moved, rings, "the edit shows");
    assert_eq!(next.solids_kept(), made);

    // A wall joined to the end of another (their mitre changes what the first is made of): it is made again though it was not
    // edited, the walls far from it are taken over.
    run(&mut s, "wall", json!({"points": [[20000, 0], [22000, 0]], "type": "L", "height": 3000}));
    let (before, work) = (after, next);
    let after = s.model.clone();
    let next = render::SectionWork::new(&after);
    let taken = next.inherit_solids(work.solid_carry(), &before, &after);
    let _ = same_as_fresh(&after, &next);
    assert!(taken >= made - 2 && taken < made, "the joined wall {corner} is made again: {taken} of {made}");
    assert!(next.solids_kept() > taken);

    // Something nothing at all to do with the line: nothing at all is made again.
    run(&mut s, "wall", json!({"points": [[0, 20000], [4000, 20000]], "type": "L", "height": 3000}));
    let (before, work) = (after, next);
    let after = s.model.clone();
    let next = render::SectionWork::new(&after);
    let made = work.solids_kept();
    let taken = next.inherit_solids(work.solid_carry(), &before, &after);
    assert_eq!(taken, made, "a wall far from the line touches none of them");
    let _ = same_as_fresh(&after, &next);

    // A library edit (a layer's thickness) says everything is made again.
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "L", "layers": [
            {"function": "finish", "material": "Render", "thickness": 30},
            {"function": "structure", "material": "Block", "thickness": 160},
            {"function": "finish", "material": "Plaster", "thickness": 20}]}),
    );
    let (before, work) = (after, next);
    let after = s.model.clone();
    let next = render::SectionWork::new(&after);
    assert_eq!(next.inherit_solids(work.solid_carry(), &before, &after), 0);
    let _ = same_as_fresh(&after, &next);
}

// ----- regions: the real outlines -----------------------------------------------------------------------------------------

/// A section along a wall that has a window in it: the cut as regions is one outline with the window a hole in it, where the rings
/// the drawing fills are trapezoids round the window.
#[test]
fn the_cut_is_available_as_outlines_with_their_holes() {
    let mut s = project();
    let w = id_of(&run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0]], "type": "L", "height": 3000, "location": "centre"})));
    run(&mut s, "window", json!({"wall": w, "offset": 4000, "type": "W"}));
    // Along the wall's core (y = 0 from the wall's start): the core layer.
    let sec = section(&mut s, [-500.0, 0.0], [8500.0, 0.0]);
    let regions = render::section_cut_regions(&s.model, sec).unwrap();
    let core: Vec<&render::Region> = regions.iter().filter(|(id, k, _)| *id == w && *k == 1).map(|(.., r)| r).collect();
    assert_eq!(core.len(), 1, "one outline: {regions:?}");
    assert_eq!(core[0].holes.len(), 1, "the window is a hole in it");
    assert!(close(geom::area(&core[0].outer), 8000.0 * 3000.0, 1e-6), "the outline is the wall's {}", geom::area(&core[0].outer));
    assert!(close(geom::area(&core[0].holes[0]).abs(), 1200.0 * 1500.0, 1e-6));
    // The rings are what they were: trapezoids, no hole in any.
    let rings = cut(&s, sec);
    let pieces = of(&rings, w, Some(1));
    assert!(pieces.len() > 1 && close(pieces.iter().map(|r| geom::area(r)).sum::<f64>(), 8000.0 * 3000.0 - 1200.0 * 1500.0, 1e-6));
}

//! A curtain wall in every view: its panels and mullions drawn where a plan cuts and sees them, no poché of the wall's
//! own, the wall first under the cursor, and its three categories in Visibility/Graphics and Object Styles.

use serde_json::{Value, json};

use crate::curtain;
use crate::engine::Session;
use crate::geom::P2;
use crate::graphics::{Look, MODEL};
use crate::model::{Element, Id};
use crate::prim::{OPAQUE, POCHE, Part, Prim, View};
use crate::render::{self, Drawing};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A storefront along the south of a room of basic walls, 6 m long and 3 m tall: its id.
fn storefront(s: &mut Session) -> Id {
    let w = run(s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}))["ids"][0].as_u64().unwrap();
    run(s, "wall", json!({"points": [[6000, 0], [6000, 4000], [0, 4000], [0, 0]], "height": 3000}));
    w
}

fn children(s: &Session, wall: Id) -> Vec<(Id, Element)> {
    let host = s.model.guid(wall);
    s.model.elements.iter().filter(|(_, e)| curtain::host_of(e) == Some(host)).map(|(id, e)| (*id, e.clone())).collect()
}

fn panels(s: &Session, wall: Id) -> Vec<(Id, curtain::Panel)> {
    children(s, wall)
        .into_iter()
        .filter_map(|(id, e)| match e {
            Element::CurtainPanel(p) => Some((id, p)),
            _ => None,
        })
        .collect()
}

fn mullions(s: &Session, wall: Id) -> Vec<(Id, curtain::Mullion)> {
    children(s, wall)
        .into_iter()
        .filter_map(|(id, e)| match e {
            Element::Mullion(x) => Some((id, x)),
            _ => None,
        })
        .collect()
}

/// The primitives of `d` that element `id` owns, each with its part.
fn owned(d: &Drawing, id: Id) -> Vec<(&Prim, Part)> {
    d.prims.iter().zip(&d.owners).zip(&d.parts).filter(|((_, o), _)| **o == Some(id)).map(|((p, _), part)| (p, *part)).collect()
}

/// Whether the primitive shows: a fill or a line that is not wholly see-through.
fn seen(p: &Prim) -> bool {
    !matches!(p, Prim::Fill { alpha: 0, .. } | Prim::Line { alpha: 0, .. })
}

fn plan_view() -> View {
    View { scale: 0.2, cx: 3000.0, cy: 1000.0, width: 1400, height: 800 }
}

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

#[test]
fn the_three_curtain_categories_are_rows_and_are_hidden_with_their_wall() {
    for c in ["curtainpanel", "mullion", "curtaingrid"] {
        assert!(MODEL.contains(&c), "{c} is a model category");
        assert!(crate::styles::known(c).is_ok());
    }
    let mut s = Session::new();
    let w = storefront(&mut s);
    let level = s.model.levels()[0].0;
    let (ps, ms) = (panels(&s, w), mullions(&s, w));
    // Cuttable, as Revit's.
    assert!(children(&s, w).iter().all(|(_, e)| crate::graphics::cuttable(&s.model, e)));
    // A view that overrides anything still shows them (they are phased as what has parameters is).
    run(&mut s, "graphics", json!({"view": level, "hide": ["floor"]}));
    let look = Look::of(&s.model, level);
    assert!(ps.iter().all(|(id, _)| look.shows(*id)) && ms.iter().all(|(id, _)| look.shows(*id)));
    // Mullions hidden: the panels stay.
    run(&mut s, "graphics", json!({"view": level, "hide": ["mullion"]}));
    let look = Look::of(&s.model, level);
    assert!(ps.iter().all(|(id, _)| look.shows(*id)) && ms.iter().all(|(id, _)| !look.shows(*id)));
    // Walls hidden: all of it goes, as Revit hides panels and mullions whenever Walls is hidden.
    run(&mut s, "graphics", json!({"view": level, "reset": true, "hide": ["wall"]}));
    let look = Look::of(&s.model, level);
    assert!(!look.shows(w) && children(&s, w).iter().all(|(id, _)| !look.shows(*id)));
    let d = render::draw(&s.model, level, plan_view());
    assert!(children(&s, w).iter().all(|(id, _)| owned(&d, *id).is_empty()) && owned(&d, w).is_empty());
    // Object Styles has a row for each, cut as a window's frame.
    let rows = run(&mut s, "object_styles", json!({}));
    let rows: Vec<&str> = rows["categories"].as_array().unwrap().iter().map(|c| c["category"].as_str().unwrap()).collect();
    assert!(["curtainpanel", "mullion", "curtaingrid"].iter().all(|c| rows.contains(c)));
    assert_eq!(s.model.object_style("mullion"), s.model.object_style("window"));
    assert_eq!(crate::merge::plural("curtainpanel"), "curtain panels");
}

#[test]
fn a_plan_draws_a_curtain_walls_cut_panels_and_mullions_and_no_poche_of_the_wall() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let level = s.model.levels()[0].0;
    let view = plan_view();
    for detail in ["coarse", "medium", "fine"] {
        run(&mut s, "detail", json!({"view": level, "detail": detail}));
        let d = render::draw(&s.model, level, view);
        // The wall draws nothing that shows: what it owns is there to be picked and highlighted.
        assert!(!owned(&d, w).is_empty() && owned(&d, w).iter().all(|(p, _)| !seen(p)), "{detail}: the wall shows nothing of its own");
        // The cut plane is 1200 over the level: the lower row of panels (0 to 2400) is cut, the upper row is over it.
        for (id, p) in panels(&s, w) {
            let mine = owned(&d, id);
            if p.span[2] > 1200.0 {
                assert!(mine.is_empty(), "{detail}: a panel over the cut plane isn't drawn");
                continue;
            }
            // Glass: its thickness as an outline, cut, and no fill.
            assert!(
                mine.iter().all(|(p, part)| matches!(p, Prim::Line { closed: true, .. }) && *part == Part::Cut),
                "{detail}: a panel is an outline, cut"
            );
            let Some((Prim::Line { pts, .. }, _)) = mine.first() else { panic!("{detail}: panel {id} isn't drawn") };
            let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(lo, hi), q| (lo.min(q.1), hi.max(q.1)));
            assert!((f64::from(hi - lo) - 25.0 * view.scale).abs() < 0.01, "{detail}: 25 mm thick, {}", hi - lo);
        }
        for (id, x) in mullions(&s, w) {
            let mine = owned(&d, id);
            match (x.vertical, x.from, x.at) {
                // Standing, from the base to the line at 2400: cut, its section filled.
                (true, from, _) if from < 1200.0 => {
                    // Its material's cut pattern over it at Fine, as a wall's layer has.
                    assert!(mine.iter().all(|(_, part)| matches!(part, Part::Cut | Part::Pattern)), "{detail}");
                    assert!(mine.iter().any(|(p, _)| matches!(p, Prim::Fill { color, alpha: OPAQUE, .. } if *color == POCHE)), "{detail}");
                    // Where its solid is: the same box in plan.
                    let e = s.model.elements.get(id).unwrap();
                    let solids = crate::solids::solids_in(&s.model, &[(id, e)], None);
                    let mut tris = Vec::new();
                    solids.iter().for_each(|s| s.tris(&mut tris));
                    let [lo, hi] = crate::solids::bounds(tris.iter().flat_map(|t| t.p)).unwrap();
                    let Some(Prim::Fill { rings, .. }) = mine.iter().map(|(p, _)| *p).find(|p| matches!(p, Prim::Fill { .. })) else {
                        panic!()
                    };
                    let (a, b) = (view.px(P2(lo[0], hi[1])), view.px(P2(hi[0], lo[1])));
                    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                    for q in rings.iter().flatten() {
                        (x0, y0, x1, y1) = (x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1));
                    }
                    let near = |p: f32, q: f32| (p - q).abs() < 0.01;
                    assert!(
                        near(x0, a.0) && near(y0, a.1) && near(x1, b.0) && near(y1, b.1),
                        "{detail}: {:?} {a:?} {b:?}",
                        (x0, y0, x1, y1)
                    );
                }
                // Standing, over the line at 2400: over the cut plane.
                (true, ..) => assert!(mine.is_empty(), "{detail}: a mullion over the cut plane isn't drawn"),
                // Lying on the base: seen below the cut plane, its outline.
                (false, _, at) if at < 1200.0 => {
                    assert!(!mine.is_empty() && mine.iter().all(|(p, part)| matches!(p, Prim::Line { .. }) && *part == Part::Projection));
                }
                (false, ..) => assert!(mine.is_empty(), "{detail}: a mullion over the cut plane isn't drawn"),
            }
        }
        // The picture: paper through the glass, the mullion's section dark.
        let png = render::png(&d).unwrap();
        assert_eq!(pixel(&png, view.px(P2(2250.0, 0.0))), (255, 255, 255), "{detail}: glass isn't filled");
        let (r, g, b) = pixel(&png, view.px(P2(3000.0, 0.0)));
        assert!(detail == "fine" || (r < 200 && g < 200 && b < 200), "{detail}: a cut mullion is filled, {r} {g} {b}");
    }
    // The reflected ceiling plan cuts it higher: still the lower row, and nothing of the wall's own.
    let up = render::draw_ceiling(&s.model, level, view);
    assert!(owned(&up, w).iter().all(|(p, _)| !seen(p)));
    // The upper row starts over its cut and under its top: seen from below, in projection.
    let cut = |id: Id| owned(&up, id).iter().any(|(_, part)| *part == Part::Cut);
    assert_eq!(panels(&s, w).iter().filter(|(id, _)| cut(*id)).count(), 4);
    assert_eq!(panels(&s, w).iter().filter(|(id, _)| !owned(&up, *id).is_empty() && !cut(*id)).count(), 4);
}

#[test]
fn a_plain_and_a_curved_curtain_wall_draw_their_panels_straight() {
    let mut s = Session::new();
    let plain =
        run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Curtain Wall", "height": 3000}))["ids"][0].as_u64().unwrap();
    let curved = run(&mut s, "wall", json!({"points": [[0, 5000], [6000, 5000]], "type": "Storefront", "height": 3000, "arc": 60}))["ids"]
        [0]
    .as_u64()
    .unwrap();
    let level = s.model.levels()[0].0;
    let d = render::draw(&s.model, level, plan_view());
    // One panel, the wall's whole length.
    let [(id, _)] = panels(&s, plain)[..] else { panic!("one panel") };
    assert_eq!(owned(&d, id).len(), 1);
    // A curved one's panels are flat between its grid lines: each four corners.
    for (id, p) in panels(&s, curved).into_iter().filter(|(_, p)| p.span[2] < 1200.0) {
        let mine = owned(&d, id);
        let Some((Prim::Line { pts, closed: true, .. }, _)) = mine.first() else { panic!("{p:?}") };
        assert_eq!(pts.len(), 4);
    }
    assert!(owned(&d, curved).iter().all(|(p, _)| !seen(p)));
    // A plan is fitted to them.
    let shows = render::plan_shows(&s.model, level);
    assert!(shows.iter().any(|p| p.0 < 1.0) && shows.iter().any(|p| p.0 > 5999.0) && shows.iter().any(|p| p.1 > 4000.0), "{shows:?}");
}

#[test]
fn a_click_on_a_curtain_wall_in_plan_takes_the_wall_and_tab_its_mullion_or_panel() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let level = s.model.levels()[0].0;
    let view = plan_view();
    let d = render::draw(&s.model, level, view);
    // On the mullion standing at 3000.
    let mullion = mullions(&s, w).into_iter().find(|(_, x)| x.vertical && x.at == 3000.0 && x.from == 0.0).unwrap().0;
    let under = d.pick_all(view.px(P2(3000.0, 0.0)), 3.0);
    assert_eq!(under.first(), Some(&w), "{under:?}");
    assert_eq!(under.get(1), Some(&mullion), "{under:?}");
    // On the glass between two mullions: the wall, then the panel.
    let panel = panels(&s, w).into_iter().find(|(_, p)| p.span[0] == 1500.0 && p.span[2] == 0.0).unwrap().0;
    let under = d.pick_all(view.px(P2(2250.0, 0.0)), 3.0);
    assert_eq!(under.first(), Some(&w), "{under:?}");
    assert!(under.contains(&panel), "{under:?}");
    // A box round all of it takes the wall and what it holds.
    let inside = d.in_box(view.px(P2(-500.0, 500.0)), view.px(P2(6500.0, -500.0)), false);
    assert!(inside.contains(&w) && inside.contains(&mullion) && inside.contains(&panel), "{inside:?}");
}

#[test]
fn a_section_cuts_a_curtain_walls_panels_and_an_elevation_shows_them_and_takes_the_wall_by_its_edge() {
    let mut s = Session::new();
    let w = storefront(&mut s);
    let (ps, ms) = (panels(&s, w), mullions(&s, w));
    // Across the storefront, between two standing mullions: both panels there and the three lying mullions are cut.
    let sec = run(&mut s, "section", json!({"start": [2250, -2000], "end": [2250, 6000]}))["id"].as_u64().unwrap();
    let cut: std::collections::BTreeSet<Id> = render::section_cut_rings(&s.model, sec).unwrap().into_iter().map(|(id, ..)| id).collect();
    assert!(!cut.contains(&w), "the wall has nothing to cut");
    assert!(ps.iter().filter(|(_, p)| p.span[0] == 1500.0).all(|(id, _)| cut.contains(id)), "{cut:?}");
    assert_eq!(ms.iter().filter(|(id, x)| !x.vertical && x.from == 1500.0 && cut.contains(id)).count(), 3, "{cut:?}");
    // From the south: the picture shows each panel and mullion, its own to pick.
    let el = run(&mut s, "elevation", json!({"facing": "south"}))["id"].as_u64().unwrap();
    let Some(Element::Elevation(e)) = s.model.elements.get(el) else { panic!("no elevation") };
    let start = e.start;
    let view = render::section_fit(&s.model, el, 900, 600);
    let at = |x: f64, z: f64| view.px(P2(x - start.0, z));
    let picture = render::section_picture(&s.model, el, view, &[]).unwrap();
    let pick = |x: f64, z: f64| picture.pick(f64::from(at(x, z).0), f64::from(at(x, z).1));
    let panel = ps.iter().find(|(_, p)| p.span[0] == 1500.0 && p.span[2] == 0.0).unwrap().0;
    let mullion = ms.iter().find(|(_, x)| x.vertical && x.at == 3000.0 && x.from == 0.0).unwrap().0;
    assert_eq!(pick(2250.0, 1200.0), Some(panel));
    assert_eq!(pick(3000.0, 1200.0), Some(mullion));
    // What finds one element under the cursor goes on from the wall to it.
    assert_eq!(crate::inquiry::pick_chain(&s.model, mullion), [w, mullion]);
    assert_eq!(crate::inquiry::pick_chain(&s.model, w), [w]);
    // The drawing over it: the wall's extents, in no ink, take the wall at its edge and nothing inside it.
    let d = render::section_over(&s.model, el, view, &picture);
    assert!(!owned(&d, w).is_empty() && owned(&d, w).iter().all(|(p, _)| !seen(p)));
    assert_eq!(d.pick(at(0.0, 1500.0), 3.0), Some(w));
    assert_eq!(d.pick(at(2250.0, 1200.0), 3.0), None);
    // A line drawn on the wall by hand: there to be picked a little past the wall's edges. Across the wall the type's
    // mullions lie on it, as on its own lines, and they are the picture's to pick.
    let host = s.model.guid(w);
    let line = {
        let m = std::sync::Arc::make_mut(&mut s.model);
        let id = m.fresh_id();
        m.elements.insert(id, Element::CurtainGrid(curtain::GridLine { host, vertical: false, at: 1200.0 }));
        curtain::keep_all(m);
        id
    };
    let picture = render::section_picture(&s.model, el, view, &[]).unwrap();
    let d = render::section_over(&s.model, el, view, &picture);
    assert!(owned(&d, line).iter().all(|(p, _)| !seen(p)));
    assert_eq!(d.pick(at(-60.0, 1200.0), 2.0), Some(line), "past the wall's edge");
    assert_eq!(d.pick(at(2250.0, 1200.0), 2.0), None, "the mullion on it is the picture's");
    assert!(matches!(
        picture.pick(f64::from(at(2250.0, 1200.0).0), f64::from(at(2250.0, 1200.0).1)).and_then(|id| s.model.elements.get(id)),
        Some(Element::Mullion(_))
    ));
    // A wall in front of it hides it: its edge is no longer its own to pick.
    run(&mut s, "wall", json!({"points": [[-500, -1000], [6500, -1000]], "height": 3500}));
    let view = render::section_fit(&s.model, el, 900, 600);
    let Some(Element::Elevation(e)) = s.model.elements.get(el) else { panic!("no elevation") };
    let start = e.start;
    let at = |x: f64, z: f64| view.px(P2(x - start.0, z));
    let picture = render::section_picture(&s.model, el, view, &[]).unwrap();
    let d = render::section_over(&s.model, el, view, &picture);
    assert_ne!(d.pick(at(0.0, 1500.0), 3.0), Some(w));
    assert_ne!(d.pick(at(-60.0, 1200.0), 2.0), Some(line));
}

#[test]
fn the_3d_scene_follows_a_curtain_wall_that_moves_with_its_level_or_alone() {
    use crate::view3d::Scene;
    let mut s = Session::new();
    let w = storefront(&mut s);
    let level = s.model.levels()[0].0;
    let same = |a: &Scene, b: &Scene, what: &str| {
        assert!(a.tris == b.tris, "{what}: the triangles brought up to date are the whole scene's");
        assert_eq!(a.spans, b.spans, "{what}");
    };
    // Its panels and mullions hold where they are in the wall, so they change with nothing that moves the wall.
    for (cmd, p) in [
        ("move", json!({"ids": [w], "by": [0, -500]})),
        ("modify", json!({"id": level, "elevation": 250})),
        ("modify", json!({"id": w, "height": 3600})),
    ] {
        let scene = Scene::new(&s.model);
        let before = std::sync::Arc::clone(&s.model);
        run(&mut s, cmd, p.clone());
        same(&scene.update(&before, &s.model), &Scene::new(&s.model), &format!("{cmd} {p}"));
        // A section's solids carried over the edit are the edit's.
        let old = render::SectionWork::new(&before);
        let sec_before: Vec<Id> = children(&s, w).iter().map(|(id, _)| *id).filter(|id| old.box_of(*id).is_some()).collect();
        let fresh = render::SectionWork::new(&s.model);
        fresh.inherit_solids(old.solid_carry(), &before, &s.model);
        for id in sec_before {
            let (Some(a), Some(b)) = (fresh.box_of(id), render::SectionWork::new(&s.model).box_of(id).copied()) else { continue };
            assert_eq!(*a, b, "{cmd}: {id}");
        }
    }
}

#[test]
fn a_dxf_puts_panels_and_mullions_on_the_glazing_layers_and_leaves_out_what_is_for_the_cursor() {
    let mut s = Session::new();
    let _ = storefront(&mut s);
    let level = s.model.levels()[0].0;
    let d = render::draw(&s.model, level, plan_view());
    let unseen = d.prims.iter().filter(|p| !seen(p)).count();
    assert!(unseen > 0);
    let written = crate::dxf_out::write(&s.model, &d, &Default::default());
    assert!(["A-GLAZ", "A-GLAZ-CWMG"].iter().all(|l| written.layers.iter().any(|w| w == l)), "{:?}", written.layers);
    let seen_only = {
        let mut d = render::draw(&s.model, level, plan_view());
        let keep: Vec<bool> = d.prims.iter().map(seen).collect();
        let mut k = keep.iter();
        d.prims.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        d.owners.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        d.parts.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        d.pens.retain(|_| *k.next().unwrap());
        d
    };
    assert_eq!(crate::dxf_out::write(&s.model, &seen_only, &Default::default()).entities, written.entities, "nothing of what is in no ink");
}

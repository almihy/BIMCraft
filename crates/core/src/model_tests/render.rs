//! `render`'s tests that drive the Session (moved out of `render.rs`).

use crate::geom::{self, P2};
use crate::model::{Element, Id, Model};
use crate::pen::Pen;
use crate::prim::{LIGHT_POCHE, PAPER, POCHE, Part, Prim, Rgb, View};
use crate::units::Units;
use crate::view3d::SectionCache;
use crate::view3d::{self};

use crate::engine::Session;
use crate::render::*;

/// A room's colour fills its region on the plan, under its name, and a project file keeps it.
#[test]
fn a_rooms_colour_fills_its_region_on_the_plan_and_the_file_keeps_it() {
    let mut s = house();
    let room = s.model.elements.iter().find(|(_, e)| matches!(e, Element::Room(_))).map(|(id, _)| *id).unwrap();
    let level = s.model.levels()[0].0;
    let view = View { scale: 0.05, cx: 4000.0, cy: 3000.0, width: 800, height: 600 };
    let fills = |m: &Model| {
        draw(m, level, view).prims.iter().filter(|p| matches!(p, Prim::Fill { color, .. } if *color == Rgb(200, 120, 40))).count()
    };
    assert_eq!(fills(&s.model), 0);
    s.execute("modify", &serde_json::json!({"ids": [room], "colour": [200, 120, 40]})).unwrap();
    assert_eq!(fills(&s.model), 1, "its region, in its colour");
    let (back, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty() && fills(&back) == 1, "{left_out:?}");
}

fn house() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           door {"at": [2000, 0]}
           window {"at": [8000, 3000]}
           room {"point": [4000, 3000], "name": "Living"}
           grid {"start": [0, -2000], "end": [0, 8000]}"#,
    )
    .unwrap();
    s
}

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

/// Without its lookup a model finds its levels and hosts by going through every element, for each element it prepares:
/// 116 s for the 100-storey tower, not 2 s. Every way of making a work builds it first.
#[test]
fn a_works_elements_are_in_id_order_and_each_finds_its_box() {
    let m = crate::sample::tower::drawn(crate::sample::tower::Plate::SMALL, 2, Default::default()).unwrap();
    for work in [SectionWork::new(&m), SectionWork::new_deferred(&m), SectionCache::default().work(&m)] {
        assert!(work.placed.windows(2).all(|w| w[0].0 < w[1].0), "by id, as a box is searched for");
        for ((id, _), b) in work.placed.iter().zip(&work.boxes) {
            assert_eq!(work.box_of(*id), b.as_ref());
        }
    }
}

#[test]
fn every_section_work_prepares_on_a_built_lookup() {
    let s = house();
    let never = || false;
    type Maker<'a> = (&'a str, &'a dyn Fn(&Model));
    let makers: [Maker; 5] = [
        ("new", &|m| drop(SectionWork::new(m))),
        ("new_cancellable", &|m| drop(SectionWork::new_cancellable(m, &never))),
        ("new_deferred", &|m| drop(SectionWork::new_deferred(m))),
        ("new_deferred_cancellable", &|m| drop(SectionWork::new_deferred_cancellable(m, &never))),
        ("SectionCache::work", &|m| drop(SectionCache::default().work(m))),
    ];
    for (name, make) in makers {
        let bare = (*s.model).clone();
        assert!(bare.derived.made(&bare.elements).is_none(), "a copy starts without its lookup");
        make(&bare);
        assert!(bare.derived.made(&bare.elements).is_some(), "{name} prepared without the model's lookup");
    }
}

#[test]
fn deferred_component_queries_keep_the_full_geometry_oracles() {
    let mut s = house();
    s.execute("component", &serde_json::json!({"family":"Desk","type":"1400 x 700","at":[4000,3000],"offset":300})).unwrap();
    let id = s.execute("section", &serde_json::json!({"start":[-1000,-1000],"end":[9000,-1000]})).unwrap()["id"].as_u64().unwrap();
    let work = SectionWork::new_deferred(&s.model);
    let view = section_fit_with(&s.model, &work, id, 400, 300);
    let overlay = SectionOverlay::new(&s.model, &work, id, view);
    let picture = section_picture_with(&s.model, &work, id, view, &[]).unwrap();
    assert!(!png(&overlay.draw(&s.model, Some(&picture))).unwrap().is_empty());
    assert_eq!(work.preparation_stats().resolved_components, 0);
}

#[test]
fn cancelled_section_stages_do_not_publish_partial_geometry_or_pictures() {
    let mut s = house();
    let id = s.execute("section", &serde_json::json!({"start": [-1000,3000], "end": [9000,3000]})).unwrap()["id"].as_u64().unwrap();
    assert!(SectionWork::new_cancellable(&s.model, &|| true).is_none());
    let calls = std::sync::atomic::AtomicUsize::new(0);
    assert!(SectionWork::new_cancellable(&s.model, &|| calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 3).is_none());
    assert!(calls.load(std::sync::atomic::Ordering::Relaxed) <= 6, "geometry must stop within a few cancellation checkpoints");
    let work = SectionWork::new(&s.model);
    let view = section_fit(&s.model, id, 400, 300);
    assert!(SectionOverlay::new_cancellable(&s.model, &work, id, view, &|| true).is_none());
    assert!(section_picture_cancellable(&s.model, &work, id, view, &[], &|| true).is_none());
    let picture = section_picture_cancellable(&s.model, &work, id, view, &[], &|| false).unwrap();
    picture.same_as(&section_picture(&s.model, id, view, &[]).unwrap()).unwrap();
}

#[test]
fn retained_section_tints_match_fresh_pictures_and_region_ink() {
    let mut s = house();
    let id = s.execute("section", &serde_json::json!({"start": [-1000,3000], "end": [9000,3000]})).unwrap()["id"].as_u64().unwrap();
    s.execute(
        "filled_region",
        &serde_json::json!({"view": id, "type": "Crosshatch", "sketch": [
            [[0,0],[10000,0]], [[10000,0],[10000,4000]], [[10000,4000],[0,4000]], [[0,4000],[0,0]]
        ]}),
    )
    .unwrap();
    let view = section_fit(&s.model, id, 400, 300);
    let work = SectionWork::new(&s.model);
    let overlay = SectionOverlay::new(&s.model, &work, id, view);
    for (level, ends) in overlay.levels() {
        assert_eq!(Some(*ends), level_ends(&s.model, *level, id));
    }
    let mut image = section_picture(&s.model, id, view, &[]).unwrap();
    let original = image.rgba.clone();
    let visible: Vec<_> = image.shows().into_iter().collect();
    assert!(!visible.is_empty());
    let mut paints: Vec<_> = visible.iter().map(|id| vec![(*id, Rgb(37, 99, 235))]).collect();
    paints.push(visible.iter().map(|id| (*id, Rgb(220, 40, 60))).collect());
    paints.push(vec![]);
    for tints in paints {
        assert!(image.retint_section(&tints));
        if !tints.is_empty() {
            assert_ne!(image.rgba, original, "the fixture must exercise tinted pixels");
        }
        let fresh = section_picture(&s.model, id, view, &tints).unwrap();
        image.same_as(&fresh).unwrap();
        assert_eq!(png(&overlay.draw(&s.model, Some(&image))).unwrap(), png(&section_over(&s.model, id, view, &fresh)).unwrap());
    }
    assert_eq!(image.rgba, original, "clearing selection restores the original paint");
}

#[test]
fn plans_fill_cut_walls_with_poche_and_open_them_at_doors() {
    let s = house();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    assert_eq!(pixel(&png, d.view.px(P2(6000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "solid wall");
    assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (255, 255, 255), "the door's gap");
    assert_eq!(pixel(&png, d.view.px(P2(1000.0, 5000.0))), (255, 255, 255), "the room, away from its tag");
}

#[test]
fn wall_corners_close_solid_without_a_notch() {
    let s = house();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    // Where the top and right walls overlap, and the square their ends stretch over, inside their outline.
    assert_eq!(pixel(&png, d.view.px(P2(8000.0, 6000.0))), (POCHE.0, POCHE.1, POCHE.2));
    assert_eq!(pixel(&png, d.view.px(P2(8020.0, 6020.0))), (POCHE.0, POCHE.1, POCHE.2));
}

/// Every pixel of the plan inside `outline` (`MARGIN` mm in from its edge, where its own inked edge ends) is poche: nothing is
/// drawn inside a column.
fn solid_inside(d: &Drawing, png: &[u8], outline: &[P2], what: &str) {
    let (lo, hi) = outline.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
        (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
    });
    // Decoded once: `pixel` would decode the picture for each point.
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let (mut x, mut seen) = (lo.0 + MARGIN, 0);
    while x < hi.0 - MARGIN {
        let mut y = lo.1 + MARGIN;
        while y < hi.1 - MARGIN {
            if geom::contains(outline, P2(x, y)) {
                let (px, py) = d.view.px(P2(x, y));
                let c = pm.pixel(px as u32, py as u32).unwrap();
                assert_eq!((c.red(), c.green(), c.blue()), (POCHE.0, POCHE.1, POCHE.2), "{what}: ({x}, {y}) is inked");
                seen += 1;
            }
            y += 10.0;
        }
        x += 10.0;
    }
    assert!(seen > 100, "{what}: only {seen} points were looked at");
}

#[test]
fn a_wall_that_meets_a_column_leaves_the_column_solid_and_unlined() {
    let mut s = Session::new();
    // A 300 x 300 column at 3000, 0. A plain wall runs into it from the west, ending at its centre; a layered one (a finish layer
    // and a structure layer, 150 mm each, which draw a line between them) starts at its right edge's middle, flush with it, and runs east;
    // a third runs right through it, north to south.
    s.script(
        r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "thickness": 150}, {"function": "structure", "thickness": 150}]}
           type {"category": "column", "name": "Square 300", "width": 300, "depth": 300}
           column {"at": [3000, 0], "type": "Square 300"}
           wall {"points": [[0, 0], [3000, 0]], "type": "Generic - 200mm"}
           wall {"points": [[3150, 0], [6000, 0]], "type": "Cavity"}
           wall {"points": [[3000, -2000], [3000, 2000]], "type": "Cavity"}"#,
    )
    .unwrap();
    let column = s.model.elements.values().find(|e| e.column().is_some()).unwrap();
    let outline = s.model.column_outline(column);
    let d = plan(&s.model, 100000, 1600, 500);
    let png = png(&d).unwrap();
    solid_inside(&d, &png, &outline, "the column");
    // And where the flush wall's end meets the column's edge there is no seam either, on its structure side (the finish side is
    // the layer's own tint, and the line between the layers, at 0, starts there).
    for (x, y) in [(3140.0, -100.0), (3140.0, -50.0), (3150.0, -100.0), (3150.0, -50.0)] {
        assert_eq!(pixel(&png, d.view.px(P2(x, y))), (POCHE.0, POCHE.1, POCHE.2), "no seam at {x}, {y}");
    }
}

#[test]
fn walls_meet_at_any_angle_without_a_seam_and_show_their_layers_and_arcs() {
    let mut s = Session::new();
    s.script(
        r#"type {"category": "wall", "name": "Cavity", "layers": [{"function": "finish", "thickness": 100}, {"function": "structure", "thickness": 200}]}
           wall {"points": [[0, 0], [0, 4000], [4000, 6309.401]], "type": "Cavity"}
           wall {"points": [[6000, 0], [10000, 0]], "arc": 90}"#,
    )
    .unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    let at = |x: f64, y: f64| pixel(&png, d.view.px(P2(x, y)));
    // Where the two walls meet at 120°, solid: no seam down their mitre.
    assert_eq!(at(0.0, 4000.0), (POCHE.0, POCHE.1, POCHE.2));
    // Going north, its exterior is west: the finish there lighter than the structure, a line between.
    assert_eq!((at(-100.0, 2000.0), at(50.0, 2000.0)), ((LIGHT_POCHE.0, LIGHT_POCHE.1, LIGHT_POCHE.2), (POCHE.0, POCHE.1, POCHE.2)));
    assert!(at(-50.0, 2000.0).0 < 100);
    // A quarter circle turning left from west to east bulges south: its crown solid, its chord's middle clear.
    assert_eq!((at(8000.0, 2000.0 - 2828.427), at(8000.0, 0.0)), ((POCHE.0, POCHE.1, POCHE.2), (PAPER.0, PAPER.1, PAPER.2)));
}

#[test]
fn a_plan_of_another_level_leaves_these_walls_out() {
    let s = house();
    let d = plan(&s.model, 100001, 800, 600);
    assert!(!d.prims.is_empty(), "the grid and the view title still show");
    assert!(!d.prims.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == POCHE)));
}

#[test]
fn a_plan_drawn_at_a_given_view_maps_the_model_through_it() {
    let s = house();
    let fitted = plan(&s.model, 100000, 800, 600);
    let window = draw(&s.model, 100000, fitted.view);
    let n = window.prims.len();
    assert!(n < fitted.prims.len() && fitted.owners[..n] == window.owners[..], "the same display list, with an image's title after it");
    assert!(fitted.owners[n..].iter().all(Option::is_none));
    // Zoomed in twice on the house's top right corner: the corner lands mid-image.
    let view = View { scale: fitted.view.scale * 2.0, cx: 8000.0, cy: 6000.0, ..fitted.view };
    let png = png(&draw(&s.model, 100000, view)).unwrap();
    assert_eq!(pixel(&png, (400.0, 300.0)), (POCHE.0, POCHE.1, POCHE.2));
    let back = view.mm(view.px(P2(1234.0, -567.0)));
    assert!(back.dist(P2(1234.0, -567.0)) < 0.01, "{back:?}");
}

#[test]
fn an_image_of_a_plan_letters_its_name_and_the_window_and_a_sheet_do_not() {
    let s = house();
    // The lettering is lines of no element's, drawn last.
    let title = |d: &Drawing| d.owners.last() == Some(&None);
    let fitted = plan(&s.model, 100000, 800, 600);
    assert!(title(&fitted), "an image names its view at its bottom left");
    assert!(title(&ceiling_plan(&s.model, 100000, 800, 600)));
    assert!(!title(&draw(&s.model, 100000, fitted.view)), "the tab names it in the window");
    assert!(!title(&draw_ceiling(&s.model, 100000, fitted.view)));
    assert!(!title(&draw_at(&s.model, 100000, fitted.view, 1.0)), "and the viewport under it on a sheet");
}

#[test]
fn every_primitive_names_the_element_it_draws() {
    let s = house();
    let d = plan(&s.model, 100000, 800, 600);
    assert_eq!(d.owners.len(), d.prims.len());
    // Walls 100002 to 100005, then the door, the window, the room and the grid.
    for id in 100_002..=100_009 {
        assert!(d.owners.contains(&Some(id)), "element {id} draws nothing");
    }
    assert_eq!(d.owners.last(), Some(&None), "the view title belongs to no element");
}

#[test]
fn pick_finds_the_element_drawn_nearest_a_point() {
    let s = house();
    let d = plan(&s.model, 100000, 800, 600);
    let at = |x, y| d.view.px(P2(x, y));
    assert_eq!(d.pick(at(2000.0, 0.0), 4.0), Some(100006), "the door, drawn over its wall");
    assert_eq!(d.pick(at(6000.0, 0.0), 4.0), Some(100002), "the wall");
    assert_eq!(d.pick(at(0.0, 3000.0), 4.0), Some(100005), "the wall, under a grid line on its centre line");
    let (x, y) = at(6000.0, -100.0);
    assert_eq!(d.pick((x, y + 3.0), 4.0), Some(100002), "just outside the wall's face");
    assert_eq!(d.pick(at(8000.0, 3000.0), 4.0), Some(100007), "the window");
    assert_eq!(d.pick(at(4000.0, 3000.0), 10.0), Some(100008), "the room, by its tag");
    assert_eq!(d.pick(at(0.0, -1500.0), 4.0), Some(100009), "the grid line");
    assert_eq!(d.pick(at(4000.0, 1200.0), 4.0), None, "the floor of the room, away from its tag");
}

#[test]
fn pick_all_lists_what_is_near_and_boxes_select_inside_or_crossing() {
    let s = house();
    let d = plan(&s.model, 100000, 800, 600);
    let at = |x, y| d.view.px(P2(x, y));
    assert_eq!(d.pick_all(at(2000.0, 0.0), 4.0), [100006, 100002], "the door, then its wall: Tab's order");
    // A box around the window, across the wall it's in.
    let (a, b) = (at(7000.0, 2000.0), at(9000.0, 4000.0));
    assert_eq!(d.in_box(a, b, false), [100007], "a window selects what is all inside");
    assert_eq!(d.in_box(b, a, true), [100003, 100007], "a crossing box also what it touches");
}

#[test]
fn a_tag_on_a_wall_or_a_room_draws_its_label_as_plain_text_over_the_plan() {
    let mut s = house();
    // The wall's type, and the room's area, each a little up and to the right of the point it names.
    let first = |cat: &str| s.model.elements.iter().find(|(_, e)| e.category() == cat).map(|(id, _)| *id).unwrap();
    let (wall_id, room_id) = (first("wall"), first("room"));
    s.script(&format!("tag {{\"ids\": [{wall_id}], \"leader\": true}}\ntag {{\"ids\": [{room_id}], \"label\": \"Area\"}}")).unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    let tag_on = |host: Id| {
        s.model.elements.iter().find_map(|(id, e)| match e {
            Element::Tag(t) if t.host == s.model.guid(host) => Some((*id, s.model.anchor(host).unwrap() + t.offset)),
            _ => None,
        })
    };
    let (wall, room) = (tag_on(wall_id), tag_on(room_id));
    let ((wall_tag, wall_head), (room_tag, room_head)) = (wall.unwrap(), room.unwrap());
    assert_eq!((wall_head, room_head), (P2(4400.0, 400.0), P2(4400.0, 3400.0)));
    // Picked where it is drawn, and no box round it: the paper fill stops short of a ring the way a door's box has.
    assert_eq!(d.pick(d.view.px(wall_head), 2.0), Some(wall_tag));
    assert_eq!(d.pick(d.view.px(room_head), 2.0), Some(room_tag));
    let (x, y) = d.view.px(wall_head);
    let (r, g, b) = pixel(&png, (x, y - 8.0));
    assert!(r > 200 && g > 200 && b > 200, "no box above a plain label: {:?}", (r, g, b));
    // Its leader runs from the wall's middle up to the label.
    let (lx, ly) = d.view.px(P2(4200.0, 200.0));
    let (r, g, b) = pixel(&png, (lx, ly));
    assert!(r < 200 && g < 200 && b < 200, "the leader line");
}

#[test]
fn dimensions_and_tags_draw_over_the_plan() {
    let mut s = house();
    // The outer faces of the left and right walls, measured below the house; then every door and window tagged.
    s.script("dimension {\"refs\": [[-100, 3000], [8100, 3000]], \"at\": [4000, -1500]}\ntag {}").unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    let dark = |at: (f32, f32)| {
        let (r, g, b) = pixel(&png, at);
        r < 200 && g < 200 && b < 200
    };
    assert!(dark(d.view.px(P2(1000.0, -1500.0))), "the dimension line");
    assert!(dark(d.view.px(P2(-100.0, -1000.0))), "a witness line, down from the left wall's outer face");
    assert_eq!(d.pick(d.view.px(P2(1000.0, -1500.0)), 4.0), Some(100010));
    // The door's tag, a box outside the wall (the door swings in). The window faces its wall's
    // exterior side, its left: this house is drawn counter-clockwise, so that's in, and so is its tag.
    let (x, y) = d.view.px(P2(2000.0, -500.0));
    assert!(dark((x, y - 8.0)), "the top of the door tag's box");
    assert_eq!(d.pick((x, y), 2.0), Some(100011));
    assert_eq!(d.pick(d.view.px(P2(7500.0, 3000.0)), 2.0), Some(100012));
}

/// The house with a section (11) across it, west to east at y = 3000 so it looks north, and
/// the South elevation (12).
fn cut() -> Session {
    let mut s = house();
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nelevation {\"facing\": \"south\"}").unwrap();
    s
}

#[test]
fn sections_cut_walls_in_poche_and_show_what_is_beyond() {
    let s = cut();
    let view = section_fit(&s.model, 100010, 800, 600);
    let png = section_png(&s.model, 100010, view).unwrap();
    // Along the section from its start at x = -1000; up from Level 1.
    let at = |x: f64, z: f64| view.px(P2(x + 1000.0, z));
    assert_eq!(pixel(&png, at(40.0, 1500.0)), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut (beside grid 1, on its centre line)");
    assert_eq!(pixel(&png, at(4000.0, 1500.0)), (255, 255, 255), "the north wall beyond, in hidden line");
    let d = section(&s.model, 100010, view);
    assert_eq!(d.pick(at(0.0, 1500.0), 2.0), Some(100005), "the cut is the wall's to pick");
    assert_eq!(d.pick(at(4000.0, 3000.0), 3.0), Some(100001), "Level 2's line");
    assert_eq!(d.pick(at(0.0, 3700.0), 3.0), Some(100009), "grid 1, where it crosses the section");
}

#[test]
fn a_wall_cut_in_section_draws_the_pen_it_draws_cut_in_plan() {
    let s = cut();
    let view = section_fit(&s.model, 100010, 800, 600);
    let d = section(&s.model, 100010, view);
    let widest = (d.prims.iter().zip(&d.owners))
        .filter_map(|(p, o)| match p {
            Prim::Line { width, .. } if *o == Some(100005) => Some(*width),
            _ => None,
        })
        .fold(0.0, f32::max);
    assert!((widest - d.width(Pen::CUT)).abs() < 1e-4, "{widest} against {}", d.width(Pen::CUT));
}

#[test]
fn elevations_show_the_walls_they_look_at_and_cut_nothing() {
    let s = cut();
    let Some(Element::Elevation(e)) = s.model.elements.get(100011) else { panic!("no elevation") };
    let start = e.start;
    let view = section_fit(&s.model, 100011, 800, 600);
    let png = section_png(&s.model, 100011, view).unwrap();
    // The south wall's face, between the door and the corner, from the south.
    let p = view.px(P2(6000.0 - start.0, 1500.0));
    assert_eq!(pixel(&png, p), (255, 255, 255));
    let d = section(&s.model, 100011, view);
    assert!(d.prims.iter().all(|p| !matches!(p, Prim::Fill { color, .. } if *color == POCHE)), "no cut");
    assert_eq!(d.pick(view.px(P2(0.0 - start.0, 3700.0)), 3.0), Some(100009), "grid 1 runs into what it sees");
    assert_eq!(
        view3d::Scene::new(&s.model)
            .cut(&section_camera(&s.model, 100011, view).unwrap().0, &section_camera(&s.model, 100011, view).unwrap().1, &[])
            .pick(f64::from(p.0), f64::from(p.1)),
        Some(100_002)
    );
}

#[test]
fn plans_mark_sections_and_elevations() {
    let s = cut();
    let d = plan(&s.model, 100000, 800, 600);
    assert_eq!(d.pick(d.view.px(P2(-500.0, 3000.0)), 3.0), Some(100010), "the section's line");
    let Some(Element::Elevation(e)) = s.model.elements.get(100011) else { panic!("no elevation") };
    let mid = e.start + (e.end - e.start) * 0.5;
    assert_eq!(d.pick(d.view.px(mid), 3.0), Some(100011), "the elevation's marker");
}

#[test]
fn plans_cut_columns_draw_stairs_and_show_roofs_from_both_sides() {
    let mut s = house();
    // Walls 3 to 6, door 7, window 8, room 9, grid 10; then a column 11, a stair 12 and a roof 13.
    s.script("column {\"at\": [6000, 4000]}\nstair {\"start\": [1000, 1500], \"end\": [1000, 2500]}\nroof {}").unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    assert_eq!(pixel(&png, d.view.px(P2(6000.0, 4000.0))), (POCHE.0, POCHE.1, POCHE.2), "the column, cut");
    // The stair: its first treads, and its arrow up the middle.
    assert_eq!(d.pick(d.view.px(P2(1400.0, 1500.0 + 280.0)), 2.0), Some(100011), "a tread");
    // The roof overhead, dashed, from below; seen from its own level, its hips run to the corners.
    assert_eq!(d.pick(d.view.px(P2(4000.0, -600.0)), 2.0), Some(100012), "the roof's edge, overhead");
    let up = plan(&s.model, 100001, 800, 600);
    let corner = up.view.px(P2(-600.0, -600.0));
    let hip = up.view.px(P2(-600.0 + 600.0, -600.0 + 600.0));
    assert_eq!(up.pick(((corner.0 + hip.0) / 2.0, (corner.1 + hip.1) / 2.0), 2.0), Some(100012), "a hip");
}

#[test]
fn sections_cut_roofs_along_their_slope() {
    let mut s = house();
    s.script("roof {}\nsection {\"start\": [4000, -2000], \"end\": [4000, 8000]}").unwrap();
    // The roof 11, the section 12 (running north, looking west): its cut of the roof at the ridge.
    let view = section_fit(&s.model, 100011, 800, 600);
    let d = section(&s.model, 100011, view);
    let ridge = 3000.0 + 3100.0 * 30f64.to_radians().tan();
    let at = view.px(P2(2000.0 + 3000.0, ridge + 100.0));
    assert_eq!(d.pick(at, 2.0), Some(100010));
    let png = section_png(&s.model, 100011, view).unwrap();
    assert_eq!(pixel(&png, at), (POCHE.0, POCHE.1, POCHE.2));
}

#[test]
fn plans_draw_what_their_view_range_sees() {
    let mut s = house();
    // A low wall across the room (11), 900 mm tall; a floor (12); the roof over it all (13).
    s.script("wall {\"points\": [[1000, 4500], [5000, 4500]], \"height\": 900}\nfloor {}\nroof {}").unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    // Cut at 1200: the window (900 to 2400 up) is open, and the low wall below the cut is seen from
    // above, its outline thin and no poché.
    assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening");
    assert_eq!(d.pick(d.view.px(P2(3000.0, 4600.0)), 2.0), Some(100010));
    assert_eq!(pixel(&png, d.view.px(P2(3000.0, 4500.0))), (255, 255, 255));
    assert!(d.owners.contains(&Some(100012)), "the roof overhead, under the level above");
    // Cut above the window's head, the wall runs solid past it; with the top under the roof's
    // eaves, the roof isn't seen.
    s.execute("modify", &serde_json::json!({"id": 100000, "view_range": {"cut": 2500, "top": 2600}})).unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    assert_eq!(pixel(&png_of(&d), d.view.px(P2(8000.0, 3000.0))), (POCHE.0, POCHE.1, POCHE.2));
    assert!(!d.owners.contains(&Some(100012)));
    // From Level 2, the floor below shows once the view depth reaches down to it.
    assert!(!plan(&s.model, 100001, 800, 600).owners.contains(&Some(100011)));
    s.execute("modify", &serde_json::json!({"id": 100001, "view_range": {"depth": -3000}})).unwrap();
    assert!(plan(&s.model, 100001, 800, 600).owners.contains(&Some(100011)));
}

fn png_of(d: &Drawing) -> Vec<u8> {
    png(d).unwrap()
}

#[test]
fn crop_regions_clip_plans_and_sections() {
    let mut s = house();
    s.execute("crop", &serde_json::json!({"view": 100000, "region": [[-1000, -1000], [4000, 4000]]})).unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    assert!((d.view.cx - 1500.0).abs() < 1.0 && (d.view.cy - 1500.0).abs() < 1.0, "fitted to the crop: {:?}", d.view);
    assert!(!d.owners.contains(&Some(100003)), "the east wall lies outside");
    assert_eq!(d.pick(d.view.px(P2(3000.0, 0.0)), 2.0), Some(100002), "the south wall, up to the crop's edge");
    assert_eq!(d.pick(d.view.px(P2(5000.0, 0.0)), 2.0), None);
    // Annotation is whole or not there: the room's tag, its point on the crop's edge, keeps its name whole.
    assert!(d.owners.contains(&Some(100008)));
    let name = d.view.px(P2(4000.0 + 300.0, 3000.0));
    assert!(d.pick((name.0, name.1 - 16.0 * d.k), 3.0) == Some(100008), "the name past the crop's edge");
    // Grid 1 stops at the crop's edge, its bubble there.
    let (x, y) = d.view.px(P2(0.0, 4000.0));
    assert_eq!(d.pick((x + 11.0 * d.k, y - 11.0 * d.k), 2.0), Some(100009));
    // A section cropped to the ground floor's lower part: Level 2 isn't in it, and the cut wall stops.
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
    s.execute("crop", &serde_json::json!({"view": 100010, "region": [[0, -100], [5000, 2000]]})).unwrap();
    let view = section_fit(&s.model, 100010, 800, 600);
    let d = section(&s.model, 100010, view);
    assert!(!d.owners.contains(&Some(100001)) && d.owners.contains(&Some(100000)), "only the levels inside");
    let png = section_png(&s.model, 100010, view).unwrap();
    // (Beside grid 1, on the wall's centre line.)
    assert_eq!(pixel(&png, view.px(P2(950.0, 1500.0))), (POCHE.0, POCHE.1, POCHE.2), "the west wall, cut");
    assert_eq!(pixel(&png, view.px(P2(950.0, 2500.0))), (255, 255, 255), "above the crop");
}

#[test]
fn notes_spot_elevations_and_dimensions_draw_in_their_views() {
    let mut s = house();
    // A note (11), a floor (12) and a spot elevation on it (13); a slanted wall (14) and an angular
    // dimension (15) between it and the south wall; a round column (16) and a radial dimension (17).
    s.script(
        r#"text {"text": "NOTE", "at": [1000, 5000]}
           floor {}
           spot {"at": [2000, 2000]}
           wall {"points": [[0, 0], [3000, 3000]], "type": "Interior - 100mm"}
           dimension {"kind": "angular", "refs": [[4000, 0], [1500, 1500]], "at": [2500, 1000]}
           column {"at": [6000, 4000], "type": "Round 300"}
           dimension {"kind": "radial", "refs": [[6150, 4000]], "at": [6800, 4800]}"#,
    )
    .unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    let dark = |at: (f32, f32)| {
        let (r, g, b) = pixel(&png, at);
        r < 200 && g < 200 && b < 200
    };
    // The note: its first letter's upright, just right of and below its top left corner.
    let (x, y) = d.view.px(P2(1000.0, 5000.0));
    assert_eq!(d.pick((x + 1.0, y + 6.0), 2.0), Some(100010));
    // The spot elevation's mark: a triangle standing on the point.
    let (x, y) = d.view.px(P2(2000.0, 2000.0));
    assert!(dark((x, y - 4.0)), "its triangle");
    assert_eq!(d.pick((x, y - 3.0), 2.0), Some(100012));
    // The angular dimension's arc, 1000 mm out from the corner at 45° / 2 round from east.
    let r = P2(2500.0, 1000.0).length();
    let mid = P2(22.5f64.to_radians().cos(), 22.5f64.to_radians().sin()) * r;
    assert_eq!(d.pick(d.view.px(mid), 2.0), Some(100014));
    // The radial one's line, from the column's centre toward `at`.
    let along = P2(6000.0, 4000.0) + (P2(6800.0, 4800.0) - P2(6000.0, 4000.0)) * 0.5;
    assert_eq!(d.pick(d.view.px(along), 2.0), Some(100016));
    // In a section: a spot elevation at its point, and a note.
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nspot {\"at\": [3000, 2000], \"view\": 100017}\ntext {\"text\": \"N\", \"at\": [5000, 2500], \"view\": 100017}").unwrap();
    let view = section_fit(&s.model, 100017, 800, 600);
    let d = section(&s.model, 100017, view);
    let (x, y) = view.px(P2(3000.0, 2000.0));
    assert_eq!(d.pick((x, y - 3.0), 2.0), Some(100018));
    assert!(d.owners.contains(&Some(100019)));
}

#[test]
fn spot_elevations_read_signed_heights_in_the_project_units() {
    assert_eq!(
        [height(Units::Millimetres, 0.3), height(Units::Millimetres, 3000.0), height(Units::Millimetres, -600.0)],
        ["±0", "+3000", "-600"]
    );
    assert_eq!([height(Units::Metres, 0.0), height(Units::FeetInches, 3000.0)], ["±0.00", "+9'-10 1/8\""]);
}

#[test]
fn plans_show_slope_arrows_and_shafts_and_the_holes_they_cut() {
    let mut s = house();
    // A ramp (11) rising east; an upper floor (12); a shaft (13) from Level 1 up through it.
    s.script(
        r#"floor {"boundary": [[1000, 500], [7000, 500], [7000, 2000], [1000, 2000]], "slope": 5, "arrow": [[1000, 1250], [7000, 1250]]}
           floor {"level": 100001, "boundary": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]]}
           shaft {"boundary": [[3000, 3000], [4000, 3000], [4000, 5000], [3000, 5000]]}"#,
    )
    .unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let ramp: Vec<&Prim> = d.prims.iter().zip(&d.owners).filter(|(_, o)| **o == Some(100010)).map(|(p, _)| p).collect();
    assert!(ramp.iter().any(|p| matches!(p, Prim::Fill { color, .. } if *color == INK)), "its slope arrow's head");
    // Cut by Level 1's plan, the shaft shows its outline and a cross; from Level 2, the hole in its floor.
    assert_eq!(d.pick(d.view.px(P2(3500.0, 4000.0)), 2.0), Some(100012), "where its cross crosses");
    let up = plan(&s.model, 100001, 800, 600);
    assert!(!up.owners.contains(&Some(100012)));
    assert_eq!(up.pick(up.view.px(P2(3500.0, 3000.0)), 2.0), Some(100011), "the hole's edge");
}

#[test]
fn ceiling_plans_look_up_at_ceilings_over_walls_cut_above_the_doors() {
    let mut s = house();
    // A tiled ceiling (11) in the room, its grid through its middle: 600 mm squares from (4000, 3000).
    s.script(r#"ceiling {"point": [4000, 1500], "type": "Tile 600 x 600"}"#).unwrap();
    let d = ceiling_plan(&s.model, 100000, 800, 600);
    let png = png(&d).unwrap();
    assert_eq!(pixel(&png, d.view.px(P2(2000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2), "cut at 2300 mm, over the door");
    assert_eq!(pixel(&png, d.view.px(P2(8050.0, 3000.0))), (255, 255, 255), "the window's opening, cut");
    let (r, g, b) = pixel(&png, d.view.px(P2(4000.0, 1500.0)));
    assert!(r < 200 && g < 200 && b < 200, "a grid line: {:?}", (r, g, b));
    assert_eq!(d.pick(d.view.px(P2(4300.0, 1500.0)), 2.0), None, "the middle of a tile");
    assert!(d.owners.contains(&Some(100010)) && !d.owners.contains(&Some(100008)), "the ceiling; no room tag");
    assert!(!plan(&s.model, 100000, 800, 600).owners.contains(&Some(100010)), "a floor plan looks down");
    // A section cuts it.
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
    let view = section_fit(&s.model, 100011, 800, 600);
    assert_eq!(section(&s.model, 100011, view).pick(view.px(P2(5000.0, 2610.0)), 2.0), Some(100010));
}

#[test]
fn ceiling_plans_see_the_beams_over_their_cut_up_to_the_ceiling() {
    let mut s = house();
    // A ceiling at 2600 mm. Beams: one hanging below it, its bottom at 2450; one over it, its bottom at 2700; one
    // outside the room, over no ceiling, its bottom at 2700 too.
    s.script(
        r#"ceiling {"point": [4000, 1500]}
           beam {"points": [[1000, 1500], [7000, 1500]], "offset": 2450, "justify": "bottom"}
           beam {"points": [[1000, 4500], [7000, 4500]], "offset": 2700, "justify": "bottom"}
           beam {"points": [[9000, 0], [9000, 6000]], "offset": 2700, "justify": "bottom"}"#,
    )
    .unwrap();
    let beams: Vec<u64> = s.model.elements.iter().filter(|(_, e)| matches!(e, Element::Beam(_))).map(|(id, _)| *id).collect();
    let [under, over, outside] = beams[..] else { panic!("{beams:?}") };
    let d = ceiling_plan(&s.model, 100000, 800, 600);
    assert!(d.owners.contains(&Some(under)), "seen below the ceiling, in projection");
    assert!(!d.owners.contains(&Some(over)), "hidden by the ceiling under it");
    assert!(d.owners.contains(&Some(outside)), "over no ceiling, seen");
    // Its outline, not its centre line: an edge picks it, its middle doesn't.
    let half = s.model.elements.iter().find_map(|(id, e)| match e {
        Element::Beam(_) => stands(&s.model, *id, e, None).map(|(o, ..)| {
            let ys = o.iter().map(|p| p.1);
            (ys.clone().fold(f64::NEG_INFINITY, f64::max) - ys.fold(f64::INFINITY, f64::min)) / 2.0
        }),
        _ => None,
    });
    let half = half.unwrap();
    assert_eq!(d.pick(d.view.px(P2(4000.0, 1500.0 + half)), 2.0), Some(under));
}

#[test]
fn plans_draw_stairs_that_turn_ramps_and_railings() {
    let mut s = house();
    // A stair (11) turning right at a landing and its railings (12, 13); a ramp (14) turning back
    // at a landing; a railing (15) along a path.
    s.script(
        r#"stair {"start": [1000, 1000], "angle": 90, "landings": [{"at": 8, "turn": -90}]}
           railing {"host": 100010}
           ramp {"start": [-6000, -3000], "angle": 0, "height": 600, "landings": [{"at": 300, "turn": 180}]}
           railing {"path": [[0, 7000], [4000, 7000]]}"#,
    )
    .unwrap();
    let d = plan(&s.model, 100000, 800, 600);
    let at = |x, y| d.view.px(P2(x, y));
    assert_eq!(d.pick(at(2340.0, 3200.0), 2.0), Some(100010), "a riser of the run past the landing");
    assert_eq!(d.pick(at(550.0, 2000.0), 1.0), Some(100011), "the left railing, in from the stair's edge");
    assert_eq!(d.pick(at(-4200.0, -3600.0), 2.0), Some(100013), "the ramp's edge");
    assert_eq!(d.pick(at(2000.0, 7000.0), 2.0), Some(100014));
    let drawn = |d: &Drawing, id, f: &dyn Fn(&Prim) -> bool| d.prims.iter().zip(&d.owners).any(|(p, o)| *o == Some(id) && f(p));
    let arrow = |p: &Prim| matches!(p, Prim::Fill { color, .. } if *color == INK);
    assert!(drawn(&d, 100010, &arrow) && drawn(&d, 100013, &arrow), "walk lines ending in arrows");
    assert!(drawn(&d, 100010, &|p| matches!(p, Prim::Line { dash: Some(_), .. })), "above the cut plane, dashed");
    let up = plan(&s.model, 100001, 800, 600);
    assert!(up.owners.contains(&Some(100010)) && !up.owners.contains(&Some(100013)), "from above: the stair, not the ramp");
}

#[test]
fn svg_and_png_are_well_formed() {
    let s = house();
    let d = plan(&s.model, 100000, 640, 480);
    let svg = svg(&d);
    assert!(svg.starts_with("<svg") && svg.trim_end().ends_with("</svg>"), "{svg}");
    assert_eq!(svg.matches("<path").count(), d.prims.len());
    let pm = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
    assert_eq!((pm.width(), pm.height()), (640, 480));
}

/// How tall the lettering element `id` draws is, px: its tallest stroke.
fn lettering(d: &Drawing, id: Id) -> f32 {
    (d.prims.iter().zip(&d.owners).zip(&d.parts))
        .filter(|((_, o), p)| **o == Some(id) && **p == Part::Text)
        .filter_map(|((prim, _), _)| match prim {
            Prim::Line { pts, .. } => {
                Some(pts.iter().map(|p| p.1).fold(f32::MIN, f32::max) - pts.iter().map(|p| p.1).fold(f32::MAX, f32::min))
            }
            _ => None,
        })
        .fold(0.0, f32::max)
}

/// Eight by six metres of walls built of layers, with a door (7) and a window (8).
fn layered_house() -> Session {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true, "type": "Exterior - 300mm"}
           door {"at": [2000, 0]}
           window {"at": [8000, 3000]}"#,
    )
    .unwrap();
    s
}

/// How wide the bubble element `id` draws is (its widest closed line), px.
fn bubble(d: &Drawing, id: Id) -> f32 {
    (d.prims.iter().zip(&d.owners))
        .filter(|(_, o)| **o == Some(id))
        .filter_map(|(prim, _)| match prim {
            Prim::Line { pts, closed: true, .. } => {
                Some(pts.iter().map(|p| p.0).fold(f32::MIN, f32::max) - pts.iter().map(|p| p.0).fold(f32::MAX, f32::min))
            }
            _ => None,
        })
        .fold(0.0, f32::max)
}

/// How far, px, the middle of the bubble element `id` draws is from the nearer of the line's `ends`.
fn bubble_gap(d: &Drawing, id: Id, ends: [(f32, f32); 2]) -> f32 {
    (d.prims.iter().zip(&d.owners))
        .filter(|(_, o)| **o == Some(id))
        .filter_map(|(prim, _)| match prim {
            Prim::Line { pts, closed: true, .. } => {
                let n = pts.len() as f32;
                Some((pts.iter().map(|p| p.0).sum::<f32>() / n, pts.iter().map(|p| p.1).sum::<f32>() / n))
            }
            _ => None,
        })
        .flat_map(|c| ends.map(|e| (c.0 - e.0).hypot(c.1 - e.1)))
        .fold(f32::MAX, f32::min)
}

#[test]
fn a_views_scale_sizes_its_annotation_as_paper_at_that_scale_holds_it() {
    let mut s = house();
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 50})).unwrap();
    // A tenth of a pixel to the mm: at 1 : 50, annotation is the size it has been all along (10 px lettering is
    // 2 mm of paper, DENSITY px to the mm).
    let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    let grid = |s: &Session, view: View| {
        let d = draw(&s.model, 100000, view);
        (lettering(&d, 100009), bubble(&d, 100009))
    };
    let (text, ring) = grid(&s, view);
    assert!((text - 10.0).abs() < 1.0 && (ring - 22.0).abs() < 1.0, "{text} {ring}");
    // The bubble stands where it did, its middle a bubble's radius, times k, past the grid's end.
    let ends = [view.px(P2(0.0, -2000.0)), view.px(P2(0.0, 8000.0))];
    assert!((bubble_gap(&draw(&s.model, 100000, view), 100009, ends) - 11.0).abs() < 0.5);
    for (n, k) in [(50, 1.0), (100, 2.0), (200, 4.0), (25, 0.5)] {
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": n})).unwrap();
        let (t, r) = grid(&s, view);
        assert!((t - text * k).abs() < 1.0 && (r - ring * k).abs() < 1.0, "1 : {n}: {t} {r}");
        let gap = bubble_gap(&draw(&s.model, 100000, view), 100009, ends);
        assert!((gap - 11.0 * k).abs() < 0.5, "1 : {n}: the bubble grows about the grid's end: {gap}");
    }
    // Zoomed in twice, the same view's annotation is twice the size: a screen showing 1 mm as 0.2 px, at 1 : 100.
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
    let (t, _) = grid(&s, View { scale: 0.2, ..view });
    assert!((t - text * 4.0).abs() < 2.0, "{t}");
}

#[test]
fn a_sections_scale_sizes_its_annotation_and_a_sheet_keeps_paper_size() {
    let mut s = house();
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
    // The section's own line, from its start, and up.
    let view = View { scale: 0.1, cx: 5000.0, cy: 1500.0, width: 1200, height: 800 };
    s.execute("scale", &serde_json::json!({"view": 100010, "scale": 50})).unwrap();
    let none = lettering(&section(&s.model, 100010, view), 100009);
    assert!((none - 10.0).abs() < 1.0, "{none}");
    s.execute("scale", &serde_json::json!({"view": 100010, "scale": 100})).unwrap();
    let hundred = lettering(&section(&s.model, 100010, view), 100009);
    assert!((hundred - 2.0 * none).abs() < 1.0, "{hundred}");
    // On paper, at the viewport's scale, annotation is paper size whatever the view's own says.
    s.script("sheet {\"name\": \"Plans\"}\nplace {\"sheet\": \"A101\", \"view\": 100000, \"scale\": 100}").unwrap();
    // The sheet is 12, its viewport 13.
    let before = png(&crate::sheet::draw(&s.model, 100011).unwrap()).unwrap();
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 20})).unwrap();
    assert_eq!(png(&crate::sheet::draw(&s.model, 100011).unwrap()).unwrap(), before);
}

/// How many of the hatching lines (thin), then of the lines between layers, element `id` draws.
fn layer_detail(d: &Drawing, id: Id) -> (usize, usize) {
    let lines = |wide: bool| {
        (d.prims.iter().zip(&d.owners).zip(&d.parts))
            .filter(|((prim, o), p)| {
                **o == Some(id)
                    && **p == Part::Pattern
                    && matches!(prim, Prim::Line { width, .. } if (*width >= d.width(Pen::PROJECTION) - 1e-4) == wide)
            })
            .count()
    };
    (lines(false), lines(true))
}

/// How many primitives element `id` draws.
fn drawn_by(d: &Drawing, id: Id) -> usize {
    d.owners.iter().filter(|o| **o == Some(id)).count()
}

#[test]
fn a_plans_detail_level_takes_patterns_then_layers_then_symbols_away() {
    let mut s = layered_house();
    s.script(
        r#"type {"category": "floor", "name": "Tiled", "layers": [{"material": "Ceramic tile", "thickness": 20}, {"material": "Concrete", "thickness": 180}]}
           floor {"type": "Tiled"}"#,
    )
    .unwrap();
    // Walls 3 to 6, the door 7, the window 8, the floor 9; layers 100 mm wide are 20 px, wide enough for their patterns.
    let view = View { scale: 0.2, cx: 4000.0, cy: 3000.0, width: 2000, height: 1400 };
    let all = |s: &Session| {
        let d = draw(&s.model, 100000, view);
        (layer_detail(&d, 100002), drawn_by(&d, 100006), drawn_by(&d, 100007), layer_detail(&d, 100008).0)
    };
    let (fine, fine_door, fine_window, fine_floor) = all(&s);
    assert!(fine.0 > 0 && fine.1 > 0 && fine_floor > 0, "{fine:?} {fine_floor}: its patterns, and the lines between layers");
    s.execute("detail", &serde_json::json!({"view": 100000, "detail": "medium"})).unwrap();
    // The layers stay, the patterns go.
    assert_eq!(all(&s), ((0, fine.1), fine_door, fine_window, 0));
    s.execute("detail", &serde_json::json!({"view": 100000, "detail": "coarse"})).unwrap();
    let (coarse, door, window, floor) = all(&s);
    assert_eq!((coarse, floor), ((0, 0), 0), "one poché");
    // A window is its gap and jambs; a door keeps its leaf and swing, as Revit's door families do at every level.
    assert!(door == fine_door && window < fine_window, "a window is its gap and jambs, a door all of it: {door} {window}");
    assert!(door >= 3 && window >= 3, "which are still there");
    // The ceiling plan is the same level's.
    assert!(drawn_by(&ceiling_plan(&s.model, 100000, 800, 600), 100002) > 0);
}

/// The extremes of what `d` draws, px: left, top, right, bottom.
fn drawn_extent(d: &Drawing) -> [f32; 4] {
    let mut e = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    let pts = d.prims.iter().flat_map(|p| match p {
        Prim::Fill { rings, .. } => rings.iter().flatten().copied().collect::<Vec<_>>(),
        Prim::Line { pts, .. } => pts.clone(),
        Prim::Image { .. } => Vec::new(),
    });
    for (x, y) in pts {
        e = [e[0].min(x), e[1].min(y), e[2].max(x), e[3].max(y)];
    }
    e
}

#[test]
fn a_big_scale_leaves_room_round_a_view_for_the_annotation_it_makes_big() {
    let mut s = house();
    // The section is 11, the elevation 12.
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}\nelevation {\"facing\": \"south\"}").unwrap();
    let (w, h) = (1000, 800);
    let mut reached = Vec::new();
    for n in [10, 20, 100, 500, 5000] {
        for view in [100000, 100010, 100011] {
            s.execute("scale", &serde_json::json!({"view": view, "scale": n})).unwrap();
        }
        let (flat, ceiling) = (plan(&s.model, 100000, w, h), ceiling_plan(&s.model, 100000, w, h));
        let cut = |id| section(&s.model, id, section_fit(&s.model, id, w, h));
        for (what, d) in [("plan", flat), ("ceiling plan", ceiling), ("section", cut(11)), ("elevation", cut(12))] {
            let [l, t, r, b] = drawn_extent(&d);
            assert!(l >= 0.0 && t >= 0.0 && r <= w as f32 && b <= h as f32, "1 : {n}, {what}: {l} {t} {r} {b} in {w} x {h}");
            if what == "plan" {
                reached.push((n, d.view.scale));
            }
        }
    }
    // The room is the annotation's, no more: a plan at 1 : 20 is as large as one at 1 : 10, whose annotation is smaller
    // than the least room, and at 1 : 500 not so small that the drawing is a dot.
    let at = |n| reached.iter().find(|(k, _)| *k == n).map(|(_, s)| *s).unwrap();
    assert_eq!(at(20), at(10));
    assert!(at(500) > at(10) / 4.0 && at(500) < at(10), "{reached:?}");
}

#[test]
fn a_tools_sketch_of_annotation_is_the_size_its_view_would_draw_it() {
    let mut s = house();
    let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    let spot = Element::Spot(crate::model::Spot { view: s.model.guid(100000), at: P2(4000.0, 3000.0), host: None, slope: false });
    let text = |s: &Session| lettering(&sketch(&s.model, view, 99, &spot), 99);
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 50})).unwrap();
    let none = text(&s);
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
    assert!((text(&s) - 2.0 * none).abs() < 1.0, "{none} {}", text(&s));
}

#[test]
fn a_sections_detail_level_takes_the_same_away_of_what_it_cuts() {
    let mut s = layered_house();
    s.script("section {\"start\": [-1000, 3000], \"end\": [9000, 3000]}").unwrap();
    // The section is 9.
    let view = section_fit(&s.model, 100008, 1600, 1000);
    // Walls 6 and 4 are cut, west and east.
    let all = |s: &Session| layer_detail(&section(&s.model, 100008, view), 100005);
    let fine = all(&s);
    assert!(fine.0 > 0 && fine.1 > 0, "{fine:?}");
    s.execute("detail", &serde_json::json!({"view": "Section 1", "detail": "medium"})).unwrap();
    assert_eq!(all(&s), (0, fine.1));
    s.execute("detail", &serde_json::json!({"view": 100008, "detail": "coarse"})).unwrap();
    assert_eq!(all(&s), (0, 0));
}

/// The widest line wall `id` draws through `view`, px.
fn widest(s: &Session, view: View, id: Id) -> f32 {
    let d = draw(&s.model, 100000, view);
    (d.prims.iter().zip(&d.owners))
        .filter_map(|(p, o)| match p {
            Prim::Line { width, .. } if *o == Some(id) => Some(*width),
            _ => None,
        })
        .fold(0.0, f32::max)
}

fn first_wall(s: &Session) -> Id {
    s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Wall(_)).then_some(*id)).unwrap()
}

#[test]
fn a_line_is_its_pens_width_on_the_paper_its_view_stands_for() {
    let mut s = house();
    let wall = first_wall(&s);
    // A tenth of a pixel to the mm at 1 : 50 is 5 px to a mm of paper: a cut wall's outline, pen 10, is as wide as the
    // project's line weights say at 1 : 50.
    let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 50})).unwrap();
    let at_50 = widest(&s, view, wall);
    let mm = s.model.line_weights().at(50.0).mm(Pen::CUT, false) as f32;
    assert!((at_50 - mm * 5.0).abs() < 1e-4, "{at_50}");
    // At 1 : 100 the same screen stands for half the paper, so the pen is twice the pixels; zoomed in twice, twice again.
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
    assert!((widest(&s, view, wall) - 3.5).abs() < 1e-4, "pen 10, 0.35 mm: {}", widest(&s, view, wall));
    assert!((widest(&s, View { scale: 0.2, ..view }, wall) - 7.0).abs() < 1e-4);
    // However far in or out: a pixel to the mm is 20 times the paper (35 px), a 500th of one a 25th (0.07 px).
    assert!((widest(&s, View { scale: 1.0, ..view }, wall) - 35.0).abs() < 1e-3, "{}", widest(&s, View { scale: 1.0, ..view }, wall));
    assert!((widest(&s, View { scale: 0.002, ..view }, wall) - 0.07).abs() < 1e-5);
    // A weight overridden is a pen too.
    s.execute("graphics", &serde_json::json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 13}}}})).unwrap();
    assert!((widest(&s, view, wall) - 0.7 * 5.0 * 2.0).abs() < 1e-4, "pen 13, 0.7 mm: {}", widest(&s, view, wall));
}

#[test]
fn pen_0_is_a_hairline_at_any_scale_and_zoom_and_still_prints() {
    let mut s = house();
    let wall = first_wall(&s);
    s.execute("graphics", &serde_json::json!({"view": 100000, "categories": {"walls": {"cut": {"weight": 0}}}})).unwrap();
    for (n, zoom) in [(50, 0.1), (200, 0.1), (100, 0.5)] {
        s.execute("scale", &serde_json::json!({"view": 100000, "scale": n})).unwrap();
        let view = View { scale: zoom, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
        assert_eq!(widest(&s, view, wall), 0.0, "1 : {n} at {zoom}");
    }
    // Width 0 is the thinnest line the device draws: the PNG draws it, the SVG a pixel that zooming doesn't widen, the
    // PDF its own hairline (`0 w`).
    let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    let d = draw(&s.model, 100000, view);
    let img = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
    // The north wall, 200 mm thick on its axis: its outside face.
    let (x, y) = view.px(P2(5000.0, 6100.0));
    let dark = (y as u32 - 3..y as u32 + 3).filter_map(|y| img.pixel(x as u32, y)).filter(|c| c.red() < 128).count();
    assert!(dark >= 1, "the wall's outside face is drawn");
    assert!(svg(&d).contains(r#"stroke-width="1" vector-effect="non-scaling-stroke""#));
    assert!(crate::pdf::contents(&d, P2(200.0, 160.0)).0.contains(" 0 w "));
}

#[test]
fn a_view_always_has_a_scale_and_one_without_its_own_is_at_1_100() {
    let mut s = house();
    let view = View { scale: 0.1, cx: 4000.0, cy: 3000.0, width: 1000, height: 800 };
    let none = png(&draw(&s.model, 100000, view)).unwrap();
    assert_eq!(s.execute("scale", &serde_json::json!({"view": 100000})).unwrap()["scale"], 100.0);
    s.execute("scale", &serde_json::json!({"view": 100000, "scale": 100})).unwrap();
    assert_eq!(png(&draw(&s.model, 100000, view)).unwrap(), none);
    let e = s.execute("scale", &serde_json::json!({"view": 100000, "scale": null})).unwrap_err();
    assert!(e.contains("always has a scale"), "{e}");
}

#[test]
fn a_cut_walls_outline_is_inked_over_its_poche_and_not_across_its_joins() {
    let s = house();
    // At 1 : 100 a cut outline (pen 10, 0.35 mm on paper) is 35 mm of the model wide; at a fifth of a pixel to the mm, 7 px.
    let view = View { scale: 0.2, cx: 6000.0, cy: 1000.0, width: 1000, height: 800 };
    let d = draw(&s.model, 100000, view);
    let png = png(&d).unwrap();
    // Two pixels inside the south wall's outer face (its axis on y = 0, 200 mm thick): the outline, whole.
    assert_eq!(pixel(&png, view.px(P2(6000.0, -90.0))), (INK.0, INK.1, INK.2), "the outline over the poché");
    assert_eq!(pixel(&png, view.px(P2(6000.0, 90.0))), (INK.0, INK.1, INK.2), "and on its inner face");
    // Where two walls meet at a corner, the mitre between them is poché, no line.
    assert_eq!(pixel(&png, view.px(P2(8000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2));
    assert_eq!(pixel(&png, view.px(P2(8040.0, 40.0))), (POCHE.0, POCHE.1, POCHE.2));
}

#[test]
fn lettering_turns_its_corners_round_while_a_pens_lines_turn_them_square() {
    let view = View { scale: 1.0, cx: 0.0, cy: 0.0, width: 400, height: 200 };
    let mut d = Drawing::new(view);
    d.text("VW", (200.0, 100.0), 80.0, INK);
    d.line(&[P2(-150.0, 0.0), P2(-100.0, 50.0), P2(-50.0, 0.0)], Pen::MEDIUM, INK, false);
    let svg = svg(&d);
    let joins: Vec<&str> = svg.split("stroke-linejoin=\"").skip(1).filter_map(|s| s.split('"').next()).collect();
    assert_eq!(joins.last(), Some(&"miter"), "the pen's line");
    assert!(joins.len() > 2 && joins[..joins.len() - 1].iter().all(|j| *j == "round"), "{joins:?}");
    let (pdf, _) = crate::pdf::contents(&d, P2(100.0, 50.0));
    assert!(pdf.contains(" 1 j ") && pdf.trim_end().rsplit('\n').next().is_some_and(|l| l.contains(" 0 j ")), "{pdf}");
}

#[test]
fn a_wall_on_its_own_is_inked_all_round_as_one_closed_line_square_at_every_corner() {
    let mut s = Session::new();
    s.script(r#"wall {"points": [[0, 0], [4000, 0]]}"#).unwrap();
    // Half a pixel to the mm: the outline, 35 mm of the model wide at 1 : 100, reaches 17.5 mm past each corner, so 15 mm
    // out each way is ink at a square corner and paper at a round one (21 mm from it).
    let view = View { scale: 0.5, cx: 2000.0, cy: 0.0, width: 2400, height: 400 };
    let d = draw(&s.model, 100000, view);
    let cut: Vec<bool> = d
        .prims
        .iter()
        .zip(&d.parts)
        .filter_map(|(p, part)| match p {
            Prim::Line { closed, .. } if *part == Part::Cut => Some(*closed),
            _ => None,
        })
        .collect();
    assert_eq!(cut, [true], "one closed line, no ends");
    let png = png(&d).unwrap();
    for (x, y) in [(-15.0, -115.0), (4015.0, -115.0), (4015.0, 115.0), (-15.0, 115.0)] {
        assert_eq!(pixel(&png, view.px(P2(x, y))), (INK.0, INK.1, INK.2), "the corner by ({x}, {y}), square");
    }
}

#[test]
fn where_two_walls_cut_outlines_meet_at_a_corner_they_join_square_with_no_notch() {
    let s = house();
    // The house's south-east corner: the south wall's axis on y = 0, the east's on x = 8000, both 200 mm thick, so the
    // outer corner is at (8100, -100). The outline, 35 mm of the model wide at 1 : 100, reaches 17.5 mm past it each way,
    // a square corner: 15 mm out each way is inside that square, outside a rounded one (21 mm from the corner, past its
    // 17.5 mm radius). Half a pixel to the mm, to see it.
    let view = View { scale: 0.5, cx: 8000.0, cy: 0.0, width: 1000, height: 800 };
    let d = draw(&s.model, 100000, view);
    let png = png(&d).unwrap();
    assert_eq!(pixel(&png, view.px(P2(8115.0, -115.0))), (INK.0, INK.1, INK.2), "the outer corner, square");
    // Each wall's run goes on round the corner into the other's: one line, joined, not two ends meeting.
    let corner = view.px(P2(8100.0, -100.0));
    let through = d.prims.iter().zip(&d.parts).any(|(p, part)| match p {
        Prim::Line { pts, .. } if *part == Part::Cut => {
            pts.len() > 2 && pts[1..pts.len() - 1].iter().any(|q| (q.0 - corner.0).abs() < 0.01 && (q.1 - corner.1).abs() < 0.01)
        }
        _ => false,
    });
    assert!(through, "a run turns the corner");
}

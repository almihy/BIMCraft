//! `sheet`'s tests that drive the Session (moved out of `sheet.rs`).

use crate::model::Element;

use crate::engine::Session;
use crate::geom::P2;
use crate::prim::POCHE;
use crate::render::png;
use crate::sheet::*;

fn pixel(png: &[u8], (x, y): (f32, f32)) -> (u8, u8, u8) {
    let pm = tiny_skia::Pixmap::decode_png(png).unwrap();
    let c = pm.pixel(x as u32, y as u32).unwrap();
    (c.red(), c.green(), c.blue())
}

#[test]
fn sheets_draw_a_title_block_and_their_views_at_scale() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           sheet {"name": "Plans"}
           place {"sheet": "A101", "view": 100000}"#,
    )
    .unwrap();
    // Walls 3 to 6, the sheet 7, its viewport 8.
    let d = draw(&s.model, 100006).unwrap();
    let png = png(&d).unwrap();
    let dark = |at: (f32, f32)| {
        let (r, g, b) = pixel(&png, at);
        r < 120 && g < 120 && b < 120
    };
    assert!(dark(d.view.px(P2(20.0, 150.0))), "the border, 20 mm in from the left");
    assert!(dark(d.view.px(P2(415.0 - 5.0, 10.0))) || dark(d.view.px(P2(410.0, 10.0))), "the border's bottom right");
    // The plan at 1 : 100, where the viewport puts it: a wall in poché.
    let (v, (ox, oy)) = frame(&s.model, 100007).unwrap();
    let at = |p: P2| {
        let (x, y) = v.px(p);
        (x + ox, y + oy)
    };
    assert_eq!(pixel(&png, at(P2(4000.0, 0.0))), (POCHE.0, POCHE.1, POCHE.2));
    assert!((v.scale - DENSITY / 100.0).abs() < 1e-12, "1 : 100");
    // What a viewport draws is the viewport's to pick; the title block is the sheet's.
    assert_eq!(d.pick(at(P2(4000.0, 0.0)), 2.0), Some(100007));
    assert_eq!(d.pick(d.view.px(P2(20.0, 150.0)), 2.0), Some(100006));
}

#[test]
fn a_schedule_on_a_sheet_shows_its_own_fields_and_its_totals_in_the_projects_language() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           room {"point": [4000, 3000], "name": "Living"}
           sheet {"name": "Areas"}
           schedule {"name": "Room Areas", "category": "rooms", "fields": ["Name", {"name": "Area", "total": true}, {"name": "Big", "formula": "Area > 40", "kind": "yesno"}]}
           place {"sheet": "A101", "view": "Room Areas", "at": [100, 220]}"#,
    )
    .unwrap();
    let sc = s.model.elements.values().find_map(|e| if let Element::Schedule(sc) = e { Some(sc.clone()) } else { None }).unwrap();
    let t = table_of(&s.model, &sc);
    assert_eq!(t.headings, ["Name", "Area (m²)", "Big"]);
    assert_eq!(t.rows, [vec!["Living", "48.00", "Yes"], vec!["Total", "48.00", ""]]);
    assert!(t.totals);
    // A row more than its rows and headings: its totals.
    let vp = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::Viewport(_)).then_some(*id)).unwrap();
    let (frame, _) = frame(&s.model, vp).unwrap();
    assert_eq!(frame.height as f32, TITLE_ROW + 3.0 * ROW);
    s.execute("language", &serde_json::json!({"code": "pt"})).unwrap();
    let t = table_of(&s.model, &sc);
    assert_eq!((t.headings[1].as_str(), t.rows[0][2].as_str(), t.rows[1][0].as_str()), ("Área (m²)", "Sim", "Total"));
}

#[test]
fn schedules_3d_views_and_notes_go_on_sheets() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           room {"point": [4000, 3000], "name": "Living"}
           sheet {"name": "Schedules"}
           place {"sheet": "A101", "view": "Room Schedule", "at": [100, 220]}
           place {"sheet": "A101", "view": "{3D}", "at": [300, 150], "scale": 200}
           text {"view": "A101", "text": "Not for construction", "at": [30, 40]}"#,
    )
    .unwrap();
    // Walls 3 to 6, the room 7, the sheet 8; the schedule 9 and its viewport 10; the 3D view 11
    // and its viewport 12; the note 13.
    let d = draw(&s.model, 100007).unwrap();
    let png = png(&d).unwrap();
    let dark = |at: (f32, f32)| {
        let (r, g, b) = pixel(&png, at);
        r < 120 && g < 120 && b < 120
    };
    // The schedule's rules: its outline's top left corner, and the line under its headings.
    let (table, (ox, oy)) = frame(&s.model, 100009).unwrap();
    assert!(table.width > 200 && table.height > 100, "{table:?}");
    assert!(dark((ox + 0.5, oy + 0.5)), "its outline");
    assert_eq!(d.pick((ox + 4.0, oy + 4.0), 2.0), Some(100009));
    // The 3D view's picture: shaded faces, not paper, somewhere in its frame.
    let (picture, (x3, y3)) = frame(&s.model, 100011).unwrap();
    let pm = tiny_skia::Pixmap::decode_png(&png).unwrap();
    let shaded = (0..picture.width)
        .flat_map(|x| (0..picture.height).map(move |y| (x, y)))
        .filter_map(|(x, y)| pm.pixel(x3 as u32 + x, y3 as u32 + y))
        .filter(|c| (c.red(), c.green(), c.blue()) != (255, 255, 255))
        .count();
    assert!(shaded > 100, "{shaded}");
    // The note, on the paper.
    let at = d.view.px(P2(30.0, 40.0));
    assert_eq!(d.pick((at.0 + 1.0, at.1 + 6.0), 2.0), Some(100012));
}

#[test]
fn a_3d_view_on_a_sheet_is_drawn_in_its_visual_style() {
    let mut s = Session::new();
    s.script(
        r#"wall {"points": [[0,0],[8000,0],[8000,6000],[0,6000]], "closed": true}
           sheet {"name": "Views"}
           place {"sheet": "A101", "view": "{3D}", "at": [300, 150], "scale": 200}"#,
    )
    .unwrap();
    // Walls 3 to 6, the sheet 7, the 3D view 8 and its viewport 9.
    let inked = |s: &Session| {
        let d = draw(&s.model, 100006).unwrap();
        let (picture, (x, y)) = frame(&s.model, 100008).unwrap();
        let pm = tiny_skia::Pixmap::decode_png(&png(&d).unwrap()).unwrap();
        (0..picture.width)
            .flat_map(|px| (0..picture.height).map(move |py| (px, py)))
            .filter_map(|(px, py)| pm.pixel(x as u32 + px, y as u32 + py))
            .filter(|c| (c.red(), c.green(), c.blue()) != (255, 255, 255))
            .count()
    };
    let shaded = inked(&s);
    s.execute("visual_style", &serde_json::json!({"view": 100007, "style": "wireframe"})).unwrap();
    let wire = inked(&s);
    assert!(wire > 50 && wire < shaded / 4, "{wire} of {shaded}");
}

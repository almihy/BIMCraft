//! Import PDF and Insert > Image through the commands every front end drives: a page comes in at true size with its lines, an
//! agent reads them and builds walls on them, walls drawn over it snap to them, an image is sized, scaled, turned, layered,
//! listed and deleted, and the project file keeps it.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::raster::pdf::sample;
use crate::snap::{Kind, Reach, Snaps, snap};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// Points in a mm, for a content stream drawn in mm.
const PT: f64 = 72.0 / 25.4;

/// A consultant's plan at 1 : 100 on a page 200 by 150 mm: a room 60 by 40 mm (6 by 4 m) from (20, 20) mm, a door's leaf
/// across its bottom side, and "WC" written in it, in mm through the page's transform; its viewport says a point is
/// 0.0352778 m.
fn consultant() -> Vec<u8> {
    let content = format!(
        "{PT} 0 0 {PT} 0 0 cm 0.3 w 0 0 0 RG 20 20 m 80 20 l 80 60 l 20 60 l h S 30 20 m 38 28 l S BT /F1 4 Tf 45 40 Td (WC) Tj ET"
    );
    sample(&content, [200.0 * PT, 150.0 * PT], Some((25.4 / 72.0 * 100.0 / 1000.0, "m")))
}

/// A session holding `bytes` as the file `name`, as Arqiflow gives the engine a project's file.
fn with_file(name: &str, bytes: Vec<u8>) -> Session {
    let mut s = Session::new();
    s.files.give(name, bytes);
    s
}

/// Image `id` as `elements` lists it.
fn el(s: &mut Session, id: &Value) -> Value {
    let all = run(s, "elements", json!({"category": "image"}));
    all["elements"].as_array().unwrap().iter().find(|e| e["id"] == *id).cloned().unwrap_or(Value::Null)
}

fn near(a: &Value, b: [f64; 2]) -> bool {
    (a[0].as_f64().unwrap() - b[0]).abs() < 0.05 && (a[1].as_f64().unwrap() - b[1]).abs() < 0.05
}

#[test]
fn a_pdf_page_comes_in_at_the_scale_it_states_with_its_lines_where_it_shows_them() {
    let mut s = with_file("files/doc_1/WC.pdf", consultant());
    let pages = run(&mut s, "pdf_pages", json!({"path": "files/doc_1/WC.pdf"}));
    assert_eq!(pages["pages"], 1);
    assert!((pages["list"][0]["stated_scale"].as_f64().unwrap() - 100.0).abs() < 1e-6, "{pages}");
    // The page's middle, (100, 75) mm, on (5000, 3500): the room's corner (20, 20) lands on (-3000, -2000).
    let r = run(&mut s, "import_pdf", json!({"path": "files/doc_1/WC.pdf", "at": [5000, 3500]}));
    assert_eq!((r["image"].as_str(), r["page"].as_u64(), r["dpi"].as_u64()), (Some("WC.pdf"), Some(1), Some(150)), "{r}");
    assert!((r["width"].as_f64().unwrap() - 20_000.0).abs() < 0.01 && (r["height"].as_f64().unwrap() - 15_000.0).abs() < 0.01, "{r}");
    assert_eq!(r["pixels"], json!([1181, 885]), "200 by 150 mm at 150 dpi");
    assert_eq!(r["lines"], 3, "the room, the leaf and the text: {r}");
    let id = r["id"].clone();
    // The agent reads its lines as an import's, in plan mm.
    let read = run(&mut s, "cad_read", json!({"id": id, "kinds": ["polyline"]}));
    let room = &read["items"][0];
    assert_eq!(room["layer"], "Lines");
    assert_eq!(room["closed"], true);
    let pts = room["points"].as_array().unwrap();
    assert!(near(&pts[0], [-3000.0, -2000.0]) && near(&pts[2], [3000.0, 2000.0]), "{room}");
    let leaf = &run(&mut s, "cad_read", json!({"id": id, "kinds": ["line", "polyline"], "start": 1}))["items"][0];
    assert!(near(&leaf["points"][0], [-2000.0, -2000.0]) && near(&leaf["points"][1], [-1200.0, -1200.0]), "{leaf}");
    let summary = run(&mut s, "cad_read", json!({"id": id}));
    assert_eq!(summary["texts"][0]["text"], "WC", "{summary}");
    // The room it outlines, with its label, as Pick Lines finds it.
    let outlines = run(&mut s, "cad_outlines", json!({"id": id, "layers": ["Lines"]}));
    let room = outlines["outlines"].as_array().unwrap().iter().find(|o| o["labels"] == json!(["WC"])).expect("the WC");
    assert!((room["area_m2"].as_f64().unwrap() - 24.0).abs() < 0.01, "{room}");
    // And the walls go on its lines.
    let walls = run(&mut s, "wall", json!({"cad": {"id": id, "entities": [0]}}));
    assert_eq!(walls["ids"].as_array().unwrap().len(), 4, "{walls}");
    let w = &run(&mut s, "elements", json!({"category": "wall"}))["elements"][0];
    assert!(near(&w["start"], [-3000.0, -2000.0]) && near(&w["end"], [3000.0, -2000.0]), "{w}");
    let q = run(&mut s, "cad_query", json!({"id": id, "at": [-1600, -1590]}));
    assert_eq!((q["found"].as_bool(), q["kind"].as_str()), (Some(true), Some("polyline")), "the leaf, a stroke: {q}");
}

#[test]
fn walls_drawn_over_a_pdf_snap_to_its_lines_unless_its_snaps_are_off() {
    let mut s = with_file("WC.pdf", consultant());
    let id = run(&mut s, "import_pdf", json!({"path": "WC.pdf", "at": [5000, 3500]}))["id"].clone();
    let level = s.model.levels()[0].0;
    let reach = Reach { point: 100.0, line: 100.0 };
    let at = |s: &Session, p: P2| snap(&s.model, level, p, reach, &[], false, &Snaps::default());
    let f = at(&s, P2(-2970.0, -1980.0));
    assert_eq!(f.kind, Kind::Endpoint, "{f:?}");
    assert!(f.at.dist(P2(-3000.0, -2000.0)) < 0.05, "{f:?}");
    assert_eq!(f.source, id.as_u64());
    let f = at(&s, P2(20.0, -1970.0));
    assert_eq!(f.kind, Kind::Midpoint, "{f:?}");
    assert!(f.at.dist(P2(0.0, -2000.0)) < 0.05, "{f:?}");
    let f = at(&s, P2(-1990.0, -1960.0));
    assert!(matches!(f.kind, Kind::Endpoint | Kind::Intersection), "the leaf meets the room's side: {f:?}");
    assert!(f.at.dist(P2(-2000.0, -2000.0)) < 0.05, "{f:?}");
    run(&mut s, "modify", json!({"id": id, "snaps": false}));
    assert_eq!(at(&s, P2(-2970.0, -1980.0)).kind, Kind::Free);
}

#[test]
fn a_scale_given_wins_and_a_sheet_takes_its_paper_size() {
    let mut s = with_file("WC.pdf", consultant());
    let r = run(&mut s, "import_pdf", json!({"path": "WC.pdf", "scale": 50, "resolution": 72}));
    assert!((r["width"].as_f64().unwrap() - 10_000.0).abs() < 0.01, "{r}");
    assert_eq!(r["pixels"], json!([566, 425]));
    let e = fails(&mut s, "import_pdf", json!({"path": "WC.pdf", "resolution": 200}));
    assert!(e.contains("72, 150, 300, 600"), "{e}");
    let e = fails(&mut s, "import_pdf", json!({"path": "WC.pdf", "page": 2}));
    assert!(e.contains("no page 2"), "{e}");
    let sheet = run(&mut s, "sheet", json!({"number": "A101", "size": "A3"}))["id"].clone();
    let r = run(&mut s, "import_pdf", json!({"image": "WC.pdf", "view": sheet}));
    assert!((r["width"].as_f64().unwrap() - 200.0).abs() < 0.01, "on a sheet, its paper: {r}");
    assert!(near(&r["corners"][0], [110.0, 223.5]), "in the A3's middle (420 by 297): {r}");
    let e = fails(&mut s, "import_pdf", json!({"image": "WC.pdf", "view": sheet, "scale": 100}));
    assert!(e.contains("paper"), "{e}");
    // Placed again, it's the same image: one in Manage Images, two instances.
    let images = run(&mut s, "manage_images", json!({}));
    assert_eq!(images["images"].as_array().unwrap().len(), 1, "{images}");
    assert_eq!(images["images"][0]["instances"].as_array().unwrap().len(), 2);
    let e = fails(&mut s, "import_pdf", json!({"path": "nothere.pdf"}));
    assert!(e.contains("nothere.pdf"), "{e}");
}

/// A PNG `w` by `h` px, red on its left half and blue on its right.
fn picture(w: u32, h: u32) -> Vec<u8> {
    let mut img = image::RgbImage::new(w, h);
    for (x, _, p) in img.enumerate_pixels_mut() {
        *p = if x < w / 2 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) };
    }
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

#[test]
fn a_picture_comes_in_at_72_dpi_and_sizes_with_its_proportions_locked() {
    let mut s = with_file("site.png", picture(720, 360));
    let r = run(&mut s, "import_image", json!({"path": "site.png"}));
    // 254 by 127 mm of paper, at the plan's 1 : 100.
    assert!((r["width"].as_f64().unwrap() - 25_400.0).abs() < 1e-6 && (r["height"].as_f64().unwrap() - 12_700.0).abs() < 1e-6, "{r}");
    let id = r["id"].clone();
    run(&mut s, "modify", json!({"id": id, "width": 1000}));
    let e = &el(&mut s, &id);
    assert!((e["height"].as_f64().unwrap() - 500.0).abs() < 1e-6, "{e}");
    run(&mut s, "modify", json!({"id": id, "lock": false, "height": 100}));
    let e = &el(&mut s, &id);
    assert_eq!((e["width"].as_f64(), e["height"].as_f64()), (Some(1000.0), Some(100.0)));
    run(&mut s, "modify", json!({"id": id, "horizontal_scale": 2}));
    let e = &el(&mut s, &id);
    assert!((e["width"].as_f64().unwrap() - 508.0).abs() < 1e-6 && (e["horizontal_scale"].as_f64().unwrap() - 2.0).abs() < 1e-6, "{e}");
    run(&mut s, "modify", json!({"id": id, "layer": "foreground"}));
    let e = &el(&mut s, &id);
    assert_eq!(e["layer"], "foreground");
    let e = fails(&mut s, "modify", json!({"id": id, "width": -5}));
    assert!(e.contains("width"), "{e}");
    let e = fails(&mut s, "import_image", json!({"path": "site.gif"}));
    assert!(e.contains("PNG, JPEG and BMP"), "{e}");
    let e = fails(&mut s, "cad_read", json!({"id": id}));
    assert!(e.contains("no vector lines"), "{e}");
}

#[test]
fn scale_by_two_reference_points_and_a_length_brings_a_pdf_to_true_size() {
    let mut s = with_file("WC.pdf", consultant());
    // Come in at its paper's size: the room's bottom side is 60 mm long.
    let id = run(&mut s, "import_pdf", json!({"path": "WC.pdf", "scale": 1}))["id"].clone();
    let room = |s: &mut Session| run(s, "cad_read", json!({"id": id, "kinds": ["polyline"]}))["items"][0]["points"].clone();
    let pts = room(&mut s);
    let (a, b) = ([pts[0][0].as_f64().unwrap(), pts[0][1].as_f64().unwrap()], [pts[1][0].as_f64().unwrap(), pts[1][1].as_f64().unwrap()]);
    assert!((b[0] - a[0] - 60.0).abs() < 0.01);
    // The side is 6000 mm, it says: Scale about its one end, reference the other, type the length.
    run(&mut s, "scale_elements", json!({"ids": [id], "origin": a, "from": b, "length": 6000}));
    let pts = room(&mut s);
    assert!(near(&pts[0], a) && near(&pts[1], [a[0] + 6000.0, a[1]]), "{pts}");
    run(&mut s, "pin", json!({"ids": [id]}));
    let e = fails(&mut s, "scale_elements", json!({"ids": [id], "origin": a, "factor": 2}));
    assert!(e.contains("pinned"), "{e}");
}

#[test]
fn an_image_turns_mirrors_and_moves_and_its_lines_go_with_it() {
    let mut s = with_file("WC.pdf", consultant());
    let id = run(&mut s, "import_pdf", json!({"path": "WC.pdf", "at": [5000, 3500]}))["id"].clone();
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": 90}));
    let pts = run(&mut s, "cad_read", json!({"id": id, "kinds": ["polyline"]}))["items"][0]["points"].clone();
    assert!(near(&pts[0], [2000.0, -3000.0]), "(-3000, -2000) a quarter turn round: {pts}");
    run(&mut s, "rotate", json!({"ids": [id], "about": [0, 0], "angle": -90}));
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[0, -1000], [0, 1000]]}));
    let pts = run(&mut s, "cad_read", json!({"id": id, "kinds": ["polyline"]}))["items"][0]["points"].clone();
    assert!(near(&pts[0], [3000.0, -2000.0]), "mirrored across x = 0: {pts}");
    run(&mut s, "move", json!({"ids": [id], "by": [100, 0]}));
    let pts = run(&mut s, "cad_read", json!({"id": id, "kinds": ["polyline"]}))["items"][0]["points"].clone();
    assert!(near(&pts[0], [3100.0, -2000.0]), "{pts}");
}

#[test]
fn an_image_shows_only_in_its_view_behind_the_model_and_is_kept_in_the_file() {
    let mut s = with_file("site.png", picture(72, 72));
    // 25.4 mm a side, made 4000 mm: red west of x = 0, blue east of it.
    let id = run(&mut s, "import_image", json!({"path": "site.png", "width": 4000}))["id"].clone();
    let (one, two) = (s.model.levels()[0].0, s.model.levels()[1].0);
    let view = crate::prim::View { scale: 0.05, cx: 0.0, cy: 0.0, width: 400, height: 400 };
    let px = |s: &Session, level: u64, x: u32, y: u32| {
        let d = crate::render::draw_at(&s.model, level, view, 1.0);
        let pm = tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap();
        let c = pm.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue()]
    };
    assert_eq!(px(&s, one, 150, 200), [255, 0, 0], "red west of the middle");
    assert_eq!(px(&s, one, 250, 200), [0, 0, 255]);
    assert_ne!(px(&s, two, 150, 200), [255, 0, 0], "Level 2's plan doesn't show it");
    // A wall drawn over it draws in front of it.
    run(&mut s, "wall", json!({"points": [[-1500, 0], [1500, 0]], "level": one}));
    let wall = px(&s, one, 150, 200);
    assert_ne!(wall, [255, 0, 0], "{wall:?}");
    // Brought to the front, it hides the wall.
    run(&mut s, "modify", json!({"id": id, "layer": "foreground"}));
    assert_eq!(px(&s, one, 150, 200), [255, 0, 0]);
    // Saved and opened, it's the same.
    let text = s.model.to_json().unwrap();
    let (m, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.image_types, s.model.image_types);
    assert_eq!(m.elements.get(id.as_u64().unwrap()), s.model.elements.get(id.as_u64().unwrap()));
    // Deleted from Manage Images, it goes with its instances.
    let r = run(&mut s, "manage_images", json!({"delete": ["site.png"]}));
    assert_eq!((r["deleted"].clone(), r["removed"].clone()), (json!(["site.png"]), json!([id])));
    assert!(s.model.image_types.is_empty() && s.model.elements.get(id.as_u64().unwrap()).is_none());
}

#[test]
fn an_image_goes_in_a_section_and_not_in_a_3d_view() {
    let mut s = with_file("site.png", picture(72, 72));
    let section = run(&mut s, "section", json!({"start": [0, -5000], "end": [0, 5000]}))["id"].clone();
    let r = run(&mut s, "import_image", json!({"path": "site.png", "view": section, "at": [0, 1500], "width": 2000}));
    let e = &el(&mut s, &r["id"]);
    assert_eq!(e["view"], s.model.guid(section.as_u64().unwrap()).to_string());
    let d = crate::render::section(
        &s.model,
        section.as_u64().unwrap(),
        crate::render::section_fit(&s.model, section.as_u64().unwrap(), 400, 300),
    );
    assert!(d.owners.contains(&r["id"].as_u64()), "the section draws it");
    let v3 = run(&mut s, "camera", json!({"eye": [10000, -10000, 5000], "target": [0, 0, 0], "name": "Mine"}))["id"].clone();
    let e = fails(&mut s, "import_image", json!({"image": "site.png", "view": v3}));
    assert!(e.contains("2D view") || e.contains("isn't a plan"), "{e}");
}

/// A PNG `w` by `h` px of one colour.
fn plain(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb(rgb));
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// The colour of the plan of the lowest level at (`x`, `y`) px, 400 px a side at 20 mm a pixel round the origin.
fn plan_px(s: &Session, x: u32, y: u32) -> [u8; 3] {
    plan_px_at(s, 0.05, x, y)
}

/// The colour of the plan of the lowest level at (`x`, `y`) px, 400 px a side at `k` px a mm round the origin.
fn plan_px_at(s: &Session, k: f64, x: u32, y: u32) -> [u8; 3] {
    let c = plan_at(s, k).pixel(x, y).unwrap();
    [c.red(), c.green(), c.blue()]
}

/// The plan of the lowest level, 400 px a side at `k` px a mm round the origin.
fn plan_at(s: &Session, k: f64) -> tiny_skia::Pixmap {
    let view = crate::prim::View { scale: k, cx: 0.0, cy: 0.0, width: 400, height: 400 };
    let d = crate::render::draw_at(&s.model, s.model.levels()[0].0, view, 1.0);
    tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap()
}

#[test]
fn a_pdf_or_a_picture_comes_in_at_its_paper_times_the_view_scale_as_revit_brings_it_in() {
    // The page says 1 : 100, the plan is at 1 : 50: Revit takes the view's scale, so its 200 mm of paper are 10 m.
    let mut s = with_file("WC.pdf", consultant());
    s.files.give("site.png", picture(720, 360));
    run(&mut s, "scale", json!({"scale": 50}));
    let r = run(&mut s, "import_pdf", json!({"path": "WC.pdf"}));
    assert!((r["width"].as_f64().unwrap() - 10_000.0).abs() < 0.01, "{r}");
    assert_eq!(r["scale"], 50.0);
    assert!((r["stated_scale"].as_f64().unwrap() - 100.0).abs() < 1e-6, "what it says is still read: {r}");
    // The scale it says, given, brings it in at true size.
    let r = run(&mut s, "import_pdf", json!({"image": "WC.pdf", "scale": 100}));
    assert!((r["width"].as_f64().unwrap() - 20_000.0).abs() < 0.01, "{r}");
    // A picture's 254 mm at 72 dpi, at 1 : 50.
    let r = run(&mut s, "import_image", json!({"path": "site.png"}));
    assert!((r["width"].as_f64().unwrap() - 12_700.0).abs() < 0.01, "{r}");
}

#[test]
fn a_page_too_fine_for_36_million_pixels_comes_in_coarser_and_says_so() {
    // 1100 mm a side at 150 dpi would be 6496 px a side, 42 million pixels.
    let side = 1100.0 * PT;
    let mut s = with_file("big.pdf", sample("0 0 0 RG 10 w 100 100 m 1000 1000 l S", [side, side], None));
    let r = run(&mut s, "import_pdf", json!({"path": "big.pdf"}));
    let dpi = r["dpi"].as_u64().unwrap();
    assert!(dpi < 150 && dpi > 100, "{r}");
    let [w, h] = [r["pixels"][0].as_u64().unwrap(), r["pixels"][1].as_u64().unwrap()];
    assert!(w * h <= crate::raster::MAX_PIXELS, "{r}");
    let notes = r["notes"].to_string();
    assert!(notes.contains("36000000") && notes.contains(&format!("{dpi} dpi")), "{notes}");
}

#[test]
fn an_import_is_one_undo_step_and_so_is_deleting_an_image() {
    let mut s = with_file("WC.pdf", consultant());
    let id = run(&mut s, "import_pdf", json!({"path": "WC.pdf"}))["id"].as_u64().unwrap();
    run(&mut s, "undo", json!({}));
    assert!(s.model.image_types.is_empty() && s.model.elements.get(id).is_none());
    run(&mut s, "redo", json!({}));
    assert!(s.model.image_types.contains_key("WC.pdf") && s.model.elements.get(id).is_some());
    run(&mut s, "manage_images", json!({"delete": ["WC.pdf"]}));
    assert!(s.model.image_types.is_empty() && s.model.elements.get(id).is_none());
    run(&mut s, "undo", json!({}));
    assert!(s.model.image_types.contains_key("WC.pdf") && s.model.elements.get(id).is_some(), "back, with its instance");
    assert_eq!(run(&mut s, "cad_read", json!({"id": id}))["texts"][0]["text"], "WC");
}

#[test]
fn a_mirrored_pdf_shows_its_pixels_where_its_lines_are() {
    let mut s = with_file("WC.pdf", consultant());
    // At 1 : 10 (its 200 mm, 2 m) round the origin: the leaf from (30, 20) to (38, 28) mm of its page.
    let id = run(&mut s, "import_pdf", json!({"path": "WC.pdf", "scale": 10, "resolution": 300}))["id"].clone();
    run(&mut s, "mirror", json!({"ids": [id], "axis": [[0, -1000], [0, 1000]]}));
    let leaf = run(&mut s, "cad_read", json!({"id": id, "kinds": ["line", "polyline"], "start": 1}))["items"][0]["points"].clone();
    let at = |i: usize| [leaf[i][0].as_f64().unwrap(), leaf[i][1].as_f64().unwrap()];
    assert!(near(&leaf[0], [700.0, -550.0]) && near(&leaf[1], [620.0, -470.0]), "mirrored across x = 0: {leaf}");
    // The middle of the leaf, on the plan at 5 mm a pixel: dark round it, as the line says; a little off it, the paper.
    let (a, b) = (at(0), at(1));
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let plan = plan_at(&s, 0.2);
    let dark = |q: [f64; 2]| {
        let (x, y) = ((200.0 + q[0] / 5.0) as u32, (200.0 - q[1] / 5.0) as u32);
        (x - 1..=x + 1)
            .any(|x| (y - 1..=y + 1).any(|y| plan.pixel(x, y).is_some_and(|c| c.red() < 128 && c.green() < 128 && c.blue() < 128)))
    };
    assert!(dark(mid), "the leaf's pixels are where its line is");
    assert!(!dark([mid[0] + 60.0, mid[1] + 60.0]), "and not off it");
}

#[test]
fn a_file_that_isnt_a_pdf_bimcraft_reads_is_refused_and_the_model_is_as_it_was() {
    let mut s =
        with_file("bad.pdf", b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 9 0 R >> endobj trailer << /Root 1 0 R >> %%EOF".to_vec());
    s.files.give("junk.pdf", (0..20_000_u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect());
    for path in ["bad.pdf", "junk.pdf"] {
        let e = fails(&mut s, "import_pdf", json!({"path": path}));
        assert!(e.contains(path) && !e.contains("internal error"), "{e}");
        let e = fails(&mut s, "pdf_pages", json!({"path": path}));
        assert!(e.contains(path), "{e}");
    }
    assert!(s.model.image_types.is_empty());
}

#[test]
fn a_stretched_image_finds_its_lines_at_the_distance_they_show() {
    let mut s = with_file("WC.pdf", consultant());
    // At 1 : 100, made twice as tall: the room's bottom side, 20 mm up its 150 mm page, is 11 m below its middle.
    let id = run(&mut s, "import_pdf", json!({"path": "WC.pdf", "at": [0, 0]}))["id"].clone();
    run(&mut s, "modify", json!({"id": id, "lock": false, "height": 30_000}));
    let q = run(&mut s, "cad_query", json!({"id": id, "at": [-5000, -10_950], "reach": 100}));
    assert_eq!(q["found"], true, "{q}");
    assert!((q["distance"].as_f64().unwrap() - 50.0).abs() < 0.01, "50 mm above it: {q}");
    let q = run(&mut s, "cad_query", json!({"id": id, "at": [-5000, -10_850], "reach": 100}));
    assert_eq!(q["found"], false, "150 mm above it, past the reach: {q}");
}

#[test]
fn arrange_orders_images_among_themselves_apart_from_their_draw_layer() {
    let mut s = Session::new();
    let mut ids = Vec::new();
    for (name, rgb) in [("red.png", [255, 0, 0]), ("green.png", [0, 255, 0]), ("blue.png", [0, 0, 255])] {
        s.files.give(name, plain(8, 8, rgb));
        ids.push(run(&mut s, "import_image", json!({"path": name, "width": 2000}))["id"].as_u64().unwrap());
    }
    let [red, green, blue] = [ids[0], ids[1], ids[2]];
    let top = |s: &Session| plan_px(s, 200, 200);
    assert_eq!(top(&s), [0, 0, 255], "the one placed last is in front");
    let order = |s: &mut Session| -> Vec<u64> {
        let r = run(s, "arrange", json!({"ids": [red]}));
        r["order"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect()
    };
    assert_eq!(order(&mut s), vec![red, green, blue], "back to front");
    run(&mut s, "arrange", json!({"ids": [red], "to": "front"}));
    assert_eq!((order(&mut s), top(&s)), (vec![green, blue, red], [255, 0, 0]));
    run(&mut s, "arrange", json!({"ids": [red], "to": "backward"}));
    assert_eq!((order(&mut s), top(&s)), (vec![green, red, blue], [0, 0, 255]));
    run(&mut s, "arrange", json!({"ids": [green], "to": "forward"}));
    assert_eq!(order(&mut s), vec![red, green, blue]);
    run(&mut s, "arrange", json!({"ids": [blue], "to": "back"}));
    assert_eq!((order(&mut s), top(&s)), (vec![blue, red, green], [0, 255, 0]));
    // Its Draw Layer is apart: in the foreground, the one at the back still draws in front of those behind the model.
    run(&mut s, "modify", json!({"id": blue, "layer": "foreground"}));
    assert_eq!(top(&s), [0, 0, 255]);
    run(&mut s, "arrange", json!({"ids": [green], "to": "front"}));
    assert_eq!(top(&s), [0, 0, 255], "Arrange leaves the layer be");
    assert_eq!(el(&mut s, &json!(blue))["layer"], "foreground");
    assert_ne!(el(&mut s, &json!(green))["layer"], "foreground");
    let e = fails(&mut s, "arrange", json!({"ids": [red], "to": "up"}));
    assert!(e.contains("front, forward, backward or back"), "{e}");
    let wall = run(&mut s, "wall", json!({"points": [[0, 0], [1000, 0]]}))["ids"][0].clone();
    let e = fails(&mut s, "arrange", json!({"ids": [wall], "to": "front"}));
    assert!(e.contains("image"), "{e}");
}

#[test]
fn a_project_with_no_images_is_written_as_format_19_so_older_builds_open_it() {
    let mut s = with_file("site.png", picture(72, 72));
    let format = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap()["format"].clone();
    assert_eq!(format(&s), 19);
    run(&mut s, "import_image", json!({"path": "site.png"}));
    assert_eq!(format(&s), 20);
    let (m, _) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(m.image_types.len(), 1);
}

#[test]
fn an_image_whose_size_resolution_or_scale_is_wrong_is_refused_on_opening() {
    let mut s = with_file("WC.pdf", consultant());
    run(&mut s, "import_pdf", json!({"path": "WC.pdf"}));
    let good: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    for (key, bad, says) in [
        ("size", json!([0, 150]), "size"),
        ("size", json!([200, -1]), "size"),
        ("size", json!([1e300, 150]), "size"),
        ("dpi", json!(0), "resolution"),
        ("dpi", json!(100_000), "resolution"),
        ("scale", json!(-100), "scale"),
        ("scale", json!(0), "scale"),
    ] {
        let mut v = good.clone();
        v["image_types"]["WC.pdf"][key] = bad.clone();
        let e = crate::model::Model::read(&v.to_string()).map(|_| ()).unwrap_err();
        assert!(e.contains("WC.pdf") && e.contains(says), "{key} {bad}: {e}");
    }
}

#[test]
fn the_image_commands_help_is_one_line_each_and_names_the_commands_there_are() {
    for c in crate::commands::ALL
        .iter()
        .filter(|c| ["import_pdf", "import_image", "pdf_pages", "scale_elements", "manage_images", "arrange"].contains(&c.id))
    {
        assert!(!c.params.contains("  "), "`{}`'s help runs spaces together: {}", c.id, c.params);
    }
    let doc = |id: &str| crate::commands::ALL.iter().find(|c| c.id == id).unwrap().params;
    assert!(doc("import_image").contains("`scale_elements`") && !doc("import_image").contains("scale_by_reference"));
    assert!(doc("scale_elements").contains("only"), "it says which elements it scales: {}", doc("scale_elements"));
    let mut s = Session::new();
    let e = fails(&mut s, "scale_elements", json!({"ids": [], "origin": [0, 0]}));
    assert!(!e.contains("  "), "{e}");
}

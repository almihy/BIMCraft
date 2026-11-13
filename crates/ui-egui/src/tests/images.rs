//! Import PDF and Image in the window, driven as a user drives them: the box picks the page and resolution, the page follows
//! the cursor and a click places it, a corner dragged scales it in proportion, walls drawn over it snap to its lines, the
//! Arrange panel orders the view's images, and Manage Images places it again. The file is read apart from the window: in a
//! thread here, and in a hosting page by the page, whose reader may stop without the window.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bimcraft_core::raster::DrawLayer;
use bimcraft_core::raster::pdf::sample;

use super::*;

/// Points in a mm.
const PT: f64 = 72.0 / 25.4;

/// A consultant's plan at 1 : 100, 200 by 150 mm: a room 60 by 40 mm from (20, 20) mm.
fn consultant() -> Vec<u8> {
    let content = format!("{PT} 0 0 {PT} 0 0 cm 0.3 w 20 20 m 80 20 l 80 60 l 20 60 l h S");
    sample(&content, [200.0 * PT, 150.0 * PT], Some((25.4 / 72.0 * 100.0 / 1000.0, "m")))
}

/// The app with the plan of a room's walls and the PDF given to it as `files/WC.pdf`, as a page hosting it does.
fn with_pdf() -> Harness<'static, BimApp> {
    let mut h = app(BOX);
    h.state_mut().session.files.give("files/WC.pdf", consultant());
    h
}

fn images(h: &Harness<BimApp>) -> Vec<(Id, bimcraft_core::raster::ImageInstance)> {
    let m = &h.state().session.model;
    m.elements.iter().filter_map(|(id, e)| if let Element::Image(im) = e { Some((*id, im.clone())) } else { None }).collect()
}

fn close(a: P2, b: P2, mm: f64) -> bool {
    a.dist(b) <= mm
}

/// Frames run until the PDF reads asked for are answered (they are read in threads of their own).
fn settle(h: &mut Harness<BimApp>) {
    let start = Instant::now();
    h.run();
    while h.state().pdf_reading() {
        assert!(start.elapsed() < Duration::from_secs(60), "a PDF read never came back");
        std::thread::sleep(Duration::from_millis(5));
        h.run();
    }
    h.run();
}

/// The PDF placed by its box and a click at (4000, 3000): the room, 6 by 4 m at the plan's 1 : 100, from (2000, 1500) to
/// (8000, 5500).
fn placed(h: &mut Harness<BimApp>) -> Id {
    h.state_mut().import_pdf_path(PathBuf::from("files/WC.pdf"));
    // Its pages and their thumbnails are read apart, one at a time.
    settle(h);
    assert!(h.state().pdf_box.is_some(), "the box is open: {:?}", h.state().status.text);
    assert!(h.query_by_label("Page to Import").is_some(), "Revit's box");
    assert_eq!(combo_value(h, "Resolution"), "150 DPI");
    assert!(h.query_by_label("200 × 150 mm").is_some(), "its page size");
    assert!(h.query_by_label("Page 1").is_some(), "its page's thumbnail");
    press(h, "OK");
    assert_eq!(h.state().tool, Tool::PlaceImage, "it follows the cursor");
    click(h, P2(4000.0, 3000.0));
    // A click before its page is read places it once it is.
    settle(h);
    let all = images(h);
    assert_eq!(all.len(), 1, "a click placed it");
    all[0].0
}

#[test]
fn import_pdf_picks_a_page_and_a_resolution_and_a_click_places_it_at_true_size() {
    let mut h = with_pdf();
    press(&mut h, "Insert");
    assert!(h.query_by_label("Import PDF").is_some() && h.query_by_label("Image").is_some(), "on the Insert tab");
    let id = placed(&mut h);
    let im = images(&h)[0].1.clone();
    assert!(close(im.at, P2(4000.0, 3000.0), 30.0), "its middle where it was clicked: {:?}", im.at);
    assert!((im.width - 20_000.0).abs() < 0.01 && (im.height - 15_000.0).abs() < 0.01, "its paper at the plan's 1 : 100: {im:?}");
    assert_eq!(h.state().selection, vec![id], "selected, as Revit leaves it");
    assert_eq!(h.state().tool, Tool::Modify);
    let t = &h.state().session.model.image_types["WC.pdf"];
    assert_eq!(t.dpi(), 150);
}

#[test]
fn a_corner_dragged_scales_the_image_about_the_corner_across_in_proportion() {
    let mut h = with_pdf();
    let id = placed(&mut h);
    // Made small enough to see whole in the plan on screen: 4 m by 3 m round (4000, 3000).
    h.state_mut().session.execute("modify", &json!({"id": id, "width": 4000})).unwrap();
    h.run();
    let before = images(&h)[0].1.clone();
    let [_, tr, _, bl] = before.corners();
    h.state_mut().selection = vec![id];
    h.run();
    // The top right corner, dragged out by 2 m and up by a little: in proportion, along its diagonal.
    drag(&mut h, tr, tr + P2(2000.0, 200.0));
    assert!(h.state().session.model.pinned_ids().is_empty());
    let after = images(&h)[0].1.clone();
    let [_, _, _, bl2] = after.corners();
    assert!(close(bl2, bl, 30.0), "the corner across stays: {bl:?} {bl2:?}");
    assert!(after.width > before.width + 1000.0, "{before:?} {after:?}");
    assert!((after.width / after.height - before.width / before.height).abs() < 1e-6, "Lock Proportions: {after:?}");
    // Unlocked, a corner sizes each side on its own.
    h.state_mut().session.execute("modify", &json!({"id": id, "lock": false})).unwrap();
    h.run();
    let [_, tr, _, _] = images(&h)[0].1.corners();
    drag(&mut h, tr, tr + P2(0.0, 1000.0));
    let free = images(&h)[0].1.clone();
    assert!((free.width - after.width).abs() < 30.0 && free.height > after.height + 900.0, "{free:?}");
}

#[test]
fn walls_drawn_over_a_pdf_snap_to_its_lines() {
    let mut h = with_pdf();
    placed(&mut h);
    // The room's corner (20, 20) mm of the page lands on (2000, 1500); a wall started near it starts on it.
    tool(&mut h, "Wall");
    click(&mut h, P2(2040.0, 1530.0));
    click(&mut h, P2(5980.0, 1520.0));
    h.key_press(Key::Escape);
    h.run();
    let m = &h.state().session.model;
    let w = m.elements.values().filter_map(|e| if let Element::Wall(w) = e { Some(w) } else { None }).next_back().unwrap();
    assert!(close(w.start, P2(2000.0, 1500.0), 0.05), "on the PDF's corner: {:?}", w.start);
    assert!((w.end.1 - 1500.0).abs() < 0.05, "along its line: {:?}", w.end);
}

#[test]
fn the_arrange_panel_orders_the_views_images_and_leaves_their_draw_layer_be() {
    let mut h = with_pdf();
    let first = placed(&mut h);
    let second =
        h.state_mut().session.execute("import_pdf", &json!({"image": "WC.pdf", "at": [6000, 3000]})).unwrap()["id"].as_u64().unwrap();
    let order = |h: &Harness<BimApp>| {
        let mut all = images(h);
        all.sort_by_key(|(id, im)| (im.order, *id));
        all.iter().map(|(id, _)| *id).collect::<Vec<_>>()
    };
    assert_eq!(order(&h), vec![first, second], "the one placed last in front");
    h.state_mut().selection = vec![second];
    h.run();
    for button in ["Bring to Front", "Bring Forward", "Send Backward", "Send to Back"] {
        assert!(h.query_by_label(button).is_some(), "{button}, as Revit's Arrange panel has");
    }
    press(&mut h, "Send to Back");
    assert_eq!(order(&h), vec![second, first]);
    press(&mut h, "Bring Forward");
    assert_eq!(order(&h), vec![first, second]);
    press(&mut h, "Send Backward");
    assert_eq!(order(&h), vec![second, first]);
    press(&mut h, "Bring to Front");
    assert_eq!(order(&h), vec![first, second]);
    assert!(images(&h).iter().all(|(_, im)| im.layer == DrawLayer::Background), "Draw Layer is Properties', not Arrange's");
}

#[test]
fn a_pdf_too_large_to_import_is_refused_before_it_is_read() {
    let mut h = app(BOX);
    h.state_mut().session.files.give("files/Huge.pdf", vec![b' '; bimcraft_core::raster::MAX_FILE + 1]);
    h.state_mut().import_pdf_path(PathBuf::from("files/Huge.pdf"));
    h.run();
    assert!(!h.state().pdf_reading(), "nothing is read");
    assert!(h.query_by_label_contains("a file to import is up to").is_some(), "the box says why");
}

/// A window inside a page, on the plan of a room's walls, with the PDF given to it, as the page gives one.
fn hosted_with_pdf() -> Harness<'static, BimApp> {
    let mut s = Session::new();
    s.script(BOX).unwrap();
    s.files.give("files/WC.pdf", consultant());
    let app = BimApp::hosted_on(s);
    let mut h = Harness::builder()
        .with_size([1280.0, 800.0])
        .with_max_steps(32)
        .with_step_dt(0.25)
        .build_ui_state(|ui, app: &mut BimApp| app.show(ui), app);
    h.run();
    h
}

/// Plays the hosting page: each ask the window made, done in a reader apart (`run_pdf_job`, as the page's worker does).
fn page_reads(h: &mut Harness<BimApp>) -> usize {
    let asks = h.state_mut().take_pdf_requests();
    let n = asks.len();
    for ask in asks {
        let job: bimcraft_core::raster::PdfJob = serde_json::from_str(&ask.job).unwrap();
        let answer = bimcraft_core::raster::run_pdf_job(&job, ask.bytes);
        h.state_mut().answer_pdf_request(ask.id, answer);
    }
    h.run();
    n
}

#[test]
fn in_a_hosting_page_the_page_reads_the_pdf_and_the_window_never_does() {
    let mut h = hosted_with_pdf();
    // Asked from the page's side, the engine won't read a PDF itself.
    let e = h.state_mut().session.execute("import_pdf", &json!({"path": "files/WC.pdf"})).unwrap_err();
    assert!(e.contains("read apart from the editor"), "{e}");
    h.state_mut().import_pdf_path(PathBuf::from("files/WC.pdf"));
    h.run();
    let asks = h.state_mut().take_pdf_requests();
    assert_eq!(asks.len(), 1, "its pages, asked of the page");
    assert_eq!((asks[0].path.as_str(), asks[0].job.as_str()), ("files/WC.pdf", r#"{"job":"outline"}"#));
    assert_eq!(asks[0].bytes, consultant(), "with the file's bytes");
    let job: bimcraft_core::raster::PdfJob = serde_json::from_str(&asks[0].job).unwrap();
    h.state_mut().answer_pdf_request(asks[0].id, bimcraft_core::raster::run_pdf_job(&job, asks[0].bytes.clone()));
    h.run();
    assert!(h.query_by_label("200 × 150 mm").is_some(), "its page size, as the page read it");
    // Its thumbnail, then the page itself once the box is OK'd; a click places it.
    while page_reads(&mut h) > 0 {}
    assert!(h.query_by_label("Page 1").is_some());
    press(&mut h, "OK");
    click(&mut h, P2(4000.0, 3000.0));
    assert!(images(&h).is_empty(), "not until the page is read");
    assert_eq!(page_reads(&mut h), 1);
    let all = images(&h);
    assert_eq!(all.len(), 1, "placed where it was clicked once the page came back");
    assert!(close(all[0].1.at, P2(4000.0, 3000.0), 30.0));
    assert_eq!(h.state().session.model.image_types["WC.pdf"].lines().map(|l| l.dxf().entities.len()), Some(1));
}

#[test]
fn in_a_hosting_page_a_reader_that_stopped_leaves_the_window_saying_the_file_could_not_be_read() {
    let mut h = hosted_with_pdf();
    h.state_mut().import_pdf_path(PathBuf::from("files/WC.pdf"));
    h.run();
    let asks = h.state_mut().take_pdf_requests();
    // The page's worker died on it (a panic aborted its instance): the page says so.
    h.state_mut().answer_pdf_request(asks[0].id, Err("the reader stopped on it".into()));
    h.run();
    assert!(h.query_by_label_contains("WC.pdf could not be read: the reader stopped on it").is_some());
    assert!(!h.state().pdf_reading());
    press(&mut h, "Cancel");
    assert!(h.state().pdf_box.is_none());
    // The window goes on: a wall is drawn.
    h.state_mut().session.execute("wall", &json!({"points": [[0, 0], [3000, 0]]})).unwrap();
    // And a page that stops while it is being placed ends the placing, saying why.
    h.state_mut().import_pdf_path(PathBuf::from("files/WC.pdf"));
    h.run();
    while page_reads(&mut h) > 0 {}
    press(&mut h, "OK");
    let asks = h.state_mut().take_pdf_requests();
    assert_eq!(asks.len(), 1);
    h.state_mut().answer_pdf_request(asks[0].id, Err("the reader stopped on it".into()));
    h.run();
    assert_eq!(h.state().tool, Tool::Modify);
    assert!(h.state().status.error && h.state().status.text.contains("could not be read"), "{:?}", h.state().status.text);
    assert!(images(&h).is_empty());
}

#[test]
fn manage_images_lists_them_and_places_one_again() {
    let mut h = with_pdf();
    placed(&mut h);
    // Nothing selected, so the Properties palette doesn't name the image too.
    h.state_mut().selection.clear();
    h.run();
    press(&mut h, "Insert");
    press(&mut h, "Manage Images");
    assert!(h.query_by_label("Place Instance").is_some());
    // Its name, and the file it was loaded from, say the same.
    h.get_all_by_label("WC.pdf").next().unwrap().click();
    h.run();
    press(&mut h, "Place Instance");
    assert_eq!(h.state().tool, Tool::PlaceImage);
    click(&mut h, P2(1000.0, 1000.0));
    assert_eq!(images(&h).len(), 2, "two instances of one image");
    assert_eq!(h.state().session.model.image_types.len(), 1);
}

#[test]
fn an_image_comes_in_by_a_click_at_72_dpi() {
    let mut h = app(BOX);
    let mut png = Vec::new();
    image::RgbImage::from_pixel(720, 360, image::Rgb([200, 30, 30]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    h.state_mut().session.files.give("site.png", png);
    h.state_mut().import_image_path(PathBuf::from("site.png"));
    h.run();
    assert_eq!(h.state().tool, Tool::PlaceImage);
    click(&mut h, P2(4000.0, 3000.0));
    let im = images(&h)[0].1.clone();
    assert!((im.width - 25_400.0).abs() < 1e-6 && (im.height - 12_700.0).abs() < 1e-6, "254 mm at the plan's 1 : 100: {im:?}");
}

#[test]
fn scale_takes_an_origin_and_two_reference_points_or_a_length_typed() {
    let mut h = with_pdf();
    let id = placed(&mut h);
    h.state_mut().selection = vec![id];
    h.state_mut().start_tool(Tool::Scale(Vec::new()));
    h.run();
    // The room's bottom side, (2000, 1500) to (8000, 1500): its far end taken to 11000 makes it half as long again.
    click(&mut h, P2(2000.0, 1500.0));
    click(&mut h, P2(8000.0, 1500.0));
    click(&mut h, P2(11_000.0, 1500.0));
    let im = images(&h)[0].1.clone();
    assert!((im.width - 30_000.0).abs() < 20.0, "{im:?}");
    // Typed: the side, now 9000 mm, made 6000 again.
    h.state_mut().selection = vec![id];
    h.state_mut().start_tool(Tool::Scale(Vec::new()));
    h.run();
    click(&mut h, P2(2000.0, 1500.0));
    click(&mut h, P2(11_000.0, 1500.0));
    type_into(&mut h, "Length:", "6000");
    h.key_press(Key::Enter);
    h.run();
    let im = images(&h)[0].1.clone();
    assert!((im.width - 20_000.0).abs() < 20.0, "{im:?}");
}

#[test]
fn imported_categories_hides_a_pdfs_layers_in_the_view() {
    // A plan on two layers, Walls and Notes, given to the window as a page hosting it gives a file.
    let content = "/OC /L0 BDC 0 0 0 RG 2 w 10 10 m 190 10 l S EMC /OC /L1 BDC BT /F1 12 Tf 20 100 Td (Kitchen) Tj ET EMC";
    let pdf = bimcraft_core::raster::pdf::sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Notes", false)]);
    let mut h = app(BOX);
    h.state_mut().session.files.give("files/plan.pdf", pdf);
    h.state_mut().session.execute("import_pdf", &json!({"path": "files/plan.pdf", "view": 100000})).unwrap();
    assert_eq!(images(&h).len(), 1);
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    // Its layers, under the PDF's row, unfolded: Notes cleared hides it in this view.
    press(&mut h, "plan.pdf: Unfold");
    pick(&mut h, Role::CheckBox, "Notes");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert_eq!(g.pdf_layers.get("plan.pdf").map(|l| l.iter().cloned().collect::<Vec<_>>()), Some(vec!["Notes".to_string()]));
    // Shown again.
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    press(&mut h, "plan.pdf: Unfold");
    pick(&mut h, Role::CheckBox, "Notes");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100000).pdf_layers.is_empty());
}

/// Whether the check box named `label` is ticked.
fn ticked(h: &Harness<BimApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT as _;
    h.get_by_role_and_label(Role::CheckBox, label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn a_layer_the_pdf_has_off_shows_unticked_in_the_view_and_ticking_it_shows_it() {
    // Furniture is off in the file: it comes in hidden in the view it is placed in, unticked there, as Revit shows a DWG's.
    let content = "/OC /L0 BDC 0 0 0 RG 2 w 10 10 m 190 10 l S EMC /OC /L1 BDC 1 0 0 RG 2 w 40 150 m 160 150 l S EMC";
    let pdf = bimcraft_core::raster::pdf::sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Furniture", true)]);
    let mut h = app(BOX);
    h.state_mut().session.files.give("files/plan.pdf", pdf);
    h.state_mut().session.execute("import_pdf", &json!({"path": "files/plan.pdf", "view": 100000})).unwrap();
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    press(&mut h, "plan.pdf: Unfold");
    assert!(ticked(&h, "Walls") && !ticked(&h, "Furniture"), "the layer the file has off is unticked");
    pick(&mut h, Role::CheckBox, "Furniture");
    press(&mut h, "OK");
    assert!(plan_graphics(&h, 100000).pdf_layers.is_empty(), "ticked, it shows");
    let drawn = bimcraft_core::render::draw(
        &h.state().session.model,
        100000,
        bimcraft_core::prim::View { scale: 0.05, cx: 0.0, cy: 0.0, width: 400, height: 400 },
    );
    assert!(
        drawn
            .prims
            .iter()
            .any(|p| matches!(p, bimcraft_core::render::Prim::Line { color, .. } if *color == bimcraft_core::prim::Rgb(255, 0, 0))),
        "the chair is drawn"
    );
}

#[test]
fn imported_categories_gives_a_pdf_and_each_of_its_layers_their_lines_and_halftone_as_a_dwgs() {
    let content = "/OC /L0 BDC 0 0 0 RG 2 w 10 10 m 190 10 l S EMC /OC /L1 BDC 1 0 0 RG 2 w 40 150 m 160 150 l S EMC";
    let pdf = bimcraft_core::raster::pdf::sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Furniture", false)]);
    let mut h = app(BOX);
    h.state_mut().session.files.give("files/plan.pdf", pdf);
    h.state_mut().session.execute("import_pdf", &json!({"path": "files/plan.pdf", "view": 100000})).unwrap();
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    press(&mut h, "plan.pdf: Unfold");
    // Furniture in halftone, the whole page hidden by its own row's box.
    pick(&mut h, Role::CheckBox, "Furniture: Halftone");
    pick(&mut h, Role::CheckBox, "plan.pdf");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    let look = g.pdf_imports.get("plan.pdf").cloned().unwrap_or_default();
    assert!(look.all.hidden && look.layers.get("Furniture").is_some_and(|o| o.halftone), "{look:?}");
    let view = bimcraft_core::prim::View { scale: 0.05, cx: 0.0, cy: 0.0, width: 400, height: 400 };
    let drawn = bimcraft_core::render::draw(&h.state().session.model, 100000, view);
    assert!(
        !drawn.prims.iter().any(|p| matches!(p, bimcraft_core::render::Prim::Line { color, .. } if color.0 > 200 && color.1 < 160)),
        "the page is hidden"
    );
    // Shown again, the chair in halftone: half way to the paper.
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    pick(&mut h, Role::CheckBox, "plan.pdf");
    press(&mut h, "OK");
    let drawn = bimcraft_core::render::draw(&h.state().session.model, 100000, view);
    assert!(
        drawn
            .prims
            .iter()
            .any(|p| matches!(p, bimcraft_core::render::Prim::Line { color, .. } if *color == bimcraft_core::prim::Rgb(255, 127, 127))),
        "halftone"
    );
}

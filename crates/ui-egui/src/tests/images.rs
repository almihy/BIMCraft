//! Import PDF and Image in the window, driven as a user drives them: the box picks the page and resolution, the page follows
//! the cursor and a click places it, a corner dragged scales it in proportion, walls drawn over it snap to its lines, the
//! Arrange panel brings it to the front, and Manage Images places it again.

use std::path::PathBuf;

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

/// The PDF placed by its box and a click at (4000, 3000): the room, 6 by 4 m at its stated 1 : 100, from (2000, 1500) to
/// (8000, 5500).
fn placed(h: &mut Harness<BimApp>) -> Id {
    h.state_mut().import_pdf_path(PathBuf::from("files/WC.pdf"));
    // Its thumbnail is made a frame after the box opens.
    h.run();
    h.run();
    assert!(h.state().pdf_box.is_some(), "the box is open: {:?}", h.state().status.text);
    assert!(h.query_by_label("Page to Import").is_some(), "Revit's box");
    assert_eq!(combo_value(h, "Resolution"), "150 DPI");
    assert!(h.query_by_label("200 × 150 mm").is_some(), "its page size");
    assert!(h.query_by_label("Page 1").is_some(), "its page's thumbnail");
    press(h, "OK");
    assert_eq!(h.state().tool, Tool::PlaceImage, "it follows the cursor");
    click(h, P2(4000.0, 3000.0));
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
    assert!((im.width - 20_000.0).abs() < 0.01 && (im.height - 15_000.0).abs() < 0.01, "1 : 100, as the page says: {im:?}");
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
fn bring_to_front_and_send_to_back_set_its_draw_layer() {
    let mut h = with_pdf();
    let id = placed(&mut h);
    h.state_mut().selection = vec![id];
    h.run();
    press(&mut h, "Bring to Front");
    assert_eq!(images(&h)[0].1.layer, DrawLayer::Foreground);
    press(&mut h, "Send to Back");
    assert_eq!(images(&h)[0].1.layer, DrawLayer::Background);
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
    assert!((im.width - 254.0).abs() < 1e-6 && (im.height - 127.0).abs() < 1e-6, "{im:?}");
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

//! A PDF's page drawn from its lines as the PDF draws it, through the commands every front end drives: fills keep their holes
//! by either rule, see-through paint stays see-through, a clip of any shape cuts what it clips, a form's own layer holds what
//! it draws, a page's white ground is no page, gradients and patterns are drawn as the PDF renderer paints them, a picture is
//! cut to its clip and as see-through as the page makes it, a dash-dot keeps its dots, text keeps its squeeze, its spaces and
//! its mirror, a line keeps its width however the page stretches it, and a purged page leaves nothing behind for an older
//! build to stumble on. Each was seen failing before its fix.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A one-page PDF 200 points a side: its content stream `content`, what its resources hold besides the font `/F1`
/// (Helvetica), what its catalog holds besides its pages, and objects of its own from 5 on (4 is the content).
fn pdf(content: &str, resources: &str, catalog: &str, more: &[&str]) -> Vec<u8> {
    pdf_of(200.0, content, resources, catalog, more)
}

/// [`pdf`], its page `side` points a side.
fn pdf_of(side: f64, content: &str, resources: &str, catalog: &str, more: &[&str]) -> Vec<u8> {
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {side} {side}] /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font \
             /Subtype /Type1 /BaseFont /Helvetica >> >> {resources} >> >>"
        ),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
    ];
    objects.extend(more.iter().map(|o| o.to_string()));
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).bytes());
    out
}

/// A session that imported `bytes` at 1 : 100, its middle on the origin, and the import's reply.
fn imported(bytes: Vec<u8>) -> (Session, Value) {
    let mut s = Session::new();
    s.files.give("p.pdf", bytes);
    let r = run(&mut s, "import_pdf", json!({"path": "p.pdf", "scale": 100}));
    (s, r)
}

/// The plan of the lowest level, 400 px a side at 20 mm a pixel round the origin.
fn plan(s: &Session) -> tiny_skia::Pixmap {
    let view = crate::prim::View { scale: 0.05, cx: 0.0, cy: 0.0, width: 400, height: 400 };
    let d = crate::render::draw_at(&s.model, s.model.levels()[0].0, view, 1.0);
    tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap()
}

/// Where a point of the page (points from its bottom left) shows on the plan: px.
fn at(x: f64, y: f64) -> (u32, u32) {
    let mm = |v: f64| (v - 100.0) * 25.4 / 72.0 * 100.0;
    ((200.0 + mm(x) * 0.05).round() as u32, (200.0 - mm(y) * 0.05).round() as u32)
}

/// The plan's pixel at page point (`x`, `y`), RGB.
fn px(p: &tiny_skia::Pixmap, x: f64, y: f64) -> [u8; 3] {
    let (i, j) = at(x, y);
    let c = p.pixel(i, j).unwrap();
    [c.red(), c.green(), c.blue()]
}

fn dark(c: [u8; 3]) -> bool {
    c.iter().all(|v| *v < 90)
}

fn white(c: [u8; 3]) -> bool {
    c.iter().all(|v| *v > 235)
}

#[test]
fn a_fill_with_a_hole_is_hollow_by_either_rule_and_overlaps_stay_solid() {
    // Even-odd: a square ring. Non-zero: the same, its hole wound the other way. Non-zero again: two squares wound alike that
    // overlap, their overlap filled.
    let content = "0 0 0 rg 10 10 m 60 10 l 60 60 l 10 60 l h 25 25 m 45 25 l 45 45 l 25 45 l h f* \
                   70 10 m 120 10 l 120 60 l 70 60 l h 85 25 m 85 45 l 105 45 l 105 25 l h f \
                   130 10 m 170 10 l 170 50 l 130 50 l h 150 30 m 190 30 l 190 70 l 150 70 l h f";
    let (s, _) = imported(pdf(content, "", "", &[]));
    let p = plan(&s);
    assert!(dark(px(&p, 15.0, 35.0)), "the even-odd ring is filled: {:?}", px(&p, 15.0, 35.0));
    assert!(white(px(&p, 35.0, 35.0)), "and its hole isn't: {:?}", px(&p, 35.0, 35.0));
    assert!(dark(px(&p, 75.0, 35.0)), "the non-zero ring is filled");
    assert!(white(px(&p, 95.0, 35.0)), "and its hole, wound the other way, isn't: {:?}", px(&p, 95.0, 35.0));
    assert!(dark(px(&p, 160.0, 40.0)), "two squares wound alike fill their overlap: {:?}", px(&p, 160.0, 40.0));
    assert!(dark(px(&p, 135.0, 15.0)) && dark(px(&p, 185.0, 65.0)));
}

#[test]
fn a_see_through_fill_shows_the_line_under_it() {
    let content = "0 0 0 RG 4 w 10 100 m 190 100 l S q /GS0 gs 1 0 0 rg 50 70 100 60 re f Q";
    let (s, _) = imported(pdf(content, "/ExtGState << /GS0 << /Type /ExtGState /ca 0.3 >> >>", "", &[]));
    let p = plan(&s);
    let under = px(&p, 100.0, 100.0);
    assert!(under[0] < 140 && under[1] < 60 && under[2] < 60, "the line shows through the red: {under:?}");
    let pink = px(&p, 100.0, 120.0);
    assert!(pink[0] > 230 && pink[1] > 150 && pink[1] < 210, "30% red over white is pink: {pink:?}");
    assert!(dark(px(&p, 20.0, 100.0)), "the line outside it");
}

#[test]
fn a_clip_that_is_no_rectangle_cuts_what_it_clips_and_what_snaps() {
    // A triangle from (20, 20) to (180, 20) to (20, 180): the line across it at 150 shows from 20 to 50 only.
    let content = "q 20 20 m 180 20 l 20 180 l h W n 0 0 0 RG 4 w 0 150 m 200 150 l S 0 0 0 rg 0 0 200 40 re f Q";
    let (mut s, r) = imported(pdf(content, "", "", &[]));
    let p = plan(&s);
    assert!(dark(px(&p, 35.0, 150.0)), "inside the triangle");
    assert!(white(px(&p, 100.0, 150.0)), "outside it: {:?}", px(&p, 100.0, 150.0));
    assert!(dark(px(&p, 100.0, 30.0)), "the fill inside it");
    assert!(white(px(&p, 170.0, 35.0)), "the fill outside it (above its slope): {:?}", px(&p, 170.0, 35.0));
    // What an agent reads, and what snaps, is only what shows.
    let read = run(&mut s, "cad_read", json!({"id": r["id"], "kinds": ["polyline"]}));
    let items = read["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{read}");
    let xs: Vec<f64> = items[0]["points"].as_array().unwrap().iter().map(|q| q[0].as_f64().unwrap()).collect();
    let page_x = |mm: f64| mm / 100.0 * 72.0 / 25.4 + 100.0;
    let (lo, hi) = (xs.iter().copied().fold(f64::INFINITY, f64::min), xs.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    assert!((page_x(lo) - 20.0).abs() < 0.5 && (page_x(hi) - 50.0).abs() < 0.5, "from 20 to 50: {lo} {hi}");
}

#[test]
fn what_a_form_draws_on_its_own_layer_is_on_that_layer() {
    let drawn = "0 0 1 RG 3 w 10 10 m 190 10 l S";
    let form =
        format!("<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] /OC 5 0 R /Length {} >>\nstream\n{drawn}\nendstream", drawn.len());
    let content = "/Fm1 Do 0 0 0 RG 1 w 10 100 m 190 100 l S";
    let bytes = pdf(
        content,
        "/XObject << /Fm1 6 0 R >>",
        "/OCProperties << /OCGs [5 0 R] /D << /Order [5 0 R] >> >>",
        &["<< /Type /OCG /Name (FORM-LAYER) >>", &form],
    );
    let (mut s, r) = imported(bytes);
    assert_eq!(r["layers"], json!(["FORM-LAYER", "No layer"]), "{r}");
    let on = run(&mut s, "cad_read", json!({"id": r["id"], "layers": ["FORM-LAYER"]}));
    assert_eq!(on["total"], 1, "the form's line is on its layer: {on}");
}

#[test]
fn a_pages_white_ground_is_no_page() {
    let content = "1 1 1 rg 0 0 200 200 re f 0 0 0 RG 2 w 10 100 m 190 100 l S";
    let (mut s, r) = imported(pdf(content, "", "", &[]));
    let read = run(&mut s, "cad_read", json!({"id": r["id"]}));
    assert!(!read.to_string().contains("hatch"), "the white page isn't kept as a fill: {read}");
    assert_eq!(r["layers"], json!(["Black 0.71"]), "{r}");
}

/// The page as the PDF renderer itself paints it, at the plan's own scale (5 px a mm of paper: 127 dpi), its pixel at page point
/// (`x`, `y`), RGB: what the page drawn from its lines is held to.
fn reference(bytes: &[u8]) -> impl Fn(f64, f64) -> [u8; 3] {
    let doc = crate::raster::pdf::Document::open(bytes.to_vec()).unwrap();
    let (w, h, rgba) = doc.render(1, 127.0).unwrap();
    move |x: f64, y: f64| {
        let (i, j) = ((x * 127.0 / 72.0).floor() as u32, ((200.0 - y) * 127.0 / 72.0).floor() as u32);
        let k = ((j.min(h - 1) * w + i.min(w - 1)) * 4) as usize;
        [rgba[k], rgba[k + 1], rgba[k + 2]]
    }
}

/// The mean difference, each channel 0 to 255, between the plan and the reference over page box `b` (x0 y0 x1 y1, points),
/// each compared as the mean of the pixels within half a point of it, so a line's edge a pixel over is no difference; and how
/// many distinct colours the plan shows there, to a step of 16.
fn compared(p: &tiny_skia::Pixmap, want: &dyn Fn(f64, f64) -> [u8; 3], b: [f64; 4]) -> (f64, usize) {
    let mean = |f: &dyn Fn(f64, f64) -> [u8; 3], x: f64, y: f64| -> [f64; 3] {
        let mut sum = [0.0; 3];
        for (dx, dy) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5), (0.0, 0.0)] {
            let c = f(x + dx, y + dy);
            (0..3).for_each(|k| sum[k] += f64::from(c[k]) / 5.0);
        }
        sum
    };
    let (mut total, mut n) = (0.0, 0.0);
    let mut seen = std::collections::BTreeSet::new();
    let mut y = b[1];
    while y <= b[3] {
        let mut x = b[0];
        while x <= b[2] {
            let (a, z) = (mean(&|x, y| px(p, x, y), x, y), mean(want, x, y));
            total += (0..3).map(|k| (a[k] - z[k]).abs()).sum::<f64>() / 3.0;
            n += 1.0;
            let c = px(p, x, y);
            seen.insert(c.map(|v| v / 16));
            x += 1.0;
        }
        y += 1.0;
    }
    (total / n, seen.len())
}

/// How alike the plan and the reference are over page box `b` (points): the correlation of their lightness, sampled every half
/// point, 1 the same picture however heavy its lines, 0 nothing alike (a flat tint, or the pattern laid out of step).
fn alike(p: &tiny_skia::Pixmap, want: &dyn Fn(f64, f64) -> [u8; 3], b: [f64; 4]) -> f64 {
    let light = |c: [u8; 3]| c.iter().map(|v| f64::from(*v)).sum::<f64>() / 3.0;
    let mut pairs = Vec::new();
    let mut y = b[1];
    while y <= b[3] {
        let mut x = b[0];
        while x <= b[2] {
            pairs.push((light(px(p, x, y)), light(want(x, y))));
            x += 0.5;
        }
        y += 0.5;
    }
    let n = pairs.len() as f64;
    let (ma, mb) = (pairs.iter().map(|q| q.0).sum::<f64>() / n, pairs.iter().map(|q| q.1).sum::<f64>() / n);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (a, b) in pairs {
        (ab, aa, bb) = (ab + (a - ma) * (b - mb), aa + (a - ma).powi(2), bb + (b - mb).powi(2));
    }
    ab / (aa * bb).sqrt().max(1e-9)
}

/// A tiling pattern object: its cell's content, `w` by `h` points a step.
fn tiling(cell: &str, w: f64, h: f64) -> String {
    format!(
        "<< /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 {w} {h}] /XStep {w} /YStep {h} /Resources << >> \
         /Length {} >>\nstream\n{cell}\nendstream",
        cell.len()
    )
}

#[test]
fn a_brick_and_an_insulation_hatch_are_drawn_as_their_cells_repeated_and_cut_to_what_they_fill() {
    // Brick in running bond (12 by 6 points, its mortar lines 1 point) in a wall 160 by 50; a batt of insulation (a zigzag
    // 8 points a step) in a slanted strip, which cuts it.
    let brick = tiling(
        "0 0 0 RG 1 w 0 0.5 m 12 0.5 l S 0 3.5 m 12 3.5 l S 0.5 0.5 m 0.5 3.5 l S 6.5 3.5 m 6.5 6 l S 6.5 0 m 6.5 0.5 l S",
        12.0,
        6.0,
    );
    let batt = tiling("0 0 0 RG 1 w 0 1 m 4 9 l 8 1 l S", 8.0, 10.0);
    let content = "/Pattern cs /P0 scn 20 120 160 50 re f /Pattern cs /P1 scn 20 20 m 180 40 l 180 90 l 20 70 l h f";
    let bytes = pdf(content, "/Pattern << /P0 5 0 R /P1 6 0 R >>", "", &[&brick, &batt]);
    let (mut s, r) = imported(bytes.clone());
    let p = plan(&s);
    let want = reference(&bytes);
    for (what, b) in [("brick", [22.0, 122.0, 178.0, 168.0]), ("insulation", [22.0, 45.0, 178.0, 65.0])] {
        // Laid out as the renderer lays it: its lines where the renderer's are (a line drawn a little heavier is no matter).
        let r = alike(&p, &want, b);
        assert!(r > 0.6, "the {what} is drawn as the renderer paints it: correlated by {r:.2}");
        let (_, colours) = compared(&p, &want, b);
        assert!(colours >= 3, "the {what} is lines on white, not a tint: {colours} colours");
    }
    // Outside what they fill, nothing: the batt is cut to its slanted strip.
    assert!(white(px(&p, 100.0, 100.0)) && white(px(&p, 25.0, 85.0)), "{:?}", px(&p, 25.0, 85.0));
    // Each is one hatch of the drawing, what it fills and its pattern's cell, not a line for each brick.
    let read = run(&mut s, "cad_read", json!({"id": r["id"], "kinds": ["hatch"]}));
    assert_eq!(read["total"], 2, "{read}");
    let cells: Vec<u64> = read["items"].as_array().unwrap().iter().map(|h| h["pattern"]["lines"].as_u64().unwrap()).collect();
    assert_eq!(cells, [5, 1], "the brick's five mortar lines and the batt's one zigzag: {read}");
    let read = run(&mut s, "cad_read", json!({"id": r["id"], "kinds": ["polyline"]}));
    assert_eq!(read["total"], 0, "{read}");
}

#[test]
fn rooms_hatched_with_one_pattern_are_a_hatch_each_sharing_its_cell_and_drawn_as_it_zoomed_in_and_its_tint_zoomed_out() {
    // 750 rooms of brick, a cell of 0.8 mm, on an A1 page: the page that, with a stroke for each brick, was too dense to keep.
    let mm = 72.0 / 25.4;
    let s = 0.8 * mm;
    let brick =
        tiling(&format!("0 0 0 RG 0.1 w 0 0 m {s} 0 l S 0 {h} m {s} {h} l S 0 0 m 0 {h} l S {h} {h} m {h} {s} l S", h = s / 2.0), s, s);
    let mut content = String::from("0 0 0 RG 0.35 w ");
    for k in 0..750 {
        let (x, y) = ((20.0 + f64::from(k % 25) * 32.0) * mm, (20.0 + f64::from(k / 25) * 27.0) * mm);
        let (w, h) = (30.0 * mm, 25.0 * mm);
        content += &format!("/Pattern cs /P0 scn {x:.2} {y:.2} {w:.2} {h:.2} re f {x:.2} {y:.2} {w:.2} {h:.2} re S ");
    }
    let bytes = pdf_of(841.0 * mm, &content, "/Pattern << /P0 5 0 R >>", "", &[&brick]);
    let (mut s, r) = imported(bytes);
    assert!(r["pixels"].is_null() && r["lines"] == 1500, "kept as lines, a hatch and an outline a room: {r}");
    assert!(r["notes"].as_array().is_none_or(|n| n.iter().all(|n| !n.as_str().unwrap_or("").contains("dense"))), "{r}");
    let read = run(&mut s, "cad_read", json!({"id": r["id"], "kinds": ["hatch"]}));
    assert_eq!(read["total"], 750, "a hatch for each room: {}", read["total"]);
    let t = s.model.image_types.values().next().unwrap();
    let cad = t.lines().unwrap();
    let tiles: Vec<&std::sync::Arc<crate::dxf::Tile>> = cad
        .dxf()
        .entities
        .iter()
        .filter_map(|e| match &e.shape {
            crate::dxf::Shape::Fill { tile: Some(t), .. } => Some(t),
            _ => None,
        })
        .collect();
    assert_eq!(tiles.len(), 750);
    assert!(tiles.iter().all(|t| std::sync::Arc::ptr_eq(t, tiles[0])), "one cell, shared");
    assert!(cad.dxf().entities.len() < 2000, "{} entities", cad.dxf().entities.len());
    // On a screen round the first room's middle (page 35, 32.5 mm): zoomed in, its bricks, lines on white; zoomed out, where
    // a brick would be under a few pixels, its tint, a grey with no white in it.
    let level = s.model.levels()[0].0;
    let middle = ((35.0 - 420.5) * 100.0, (32.5 - 420.5) * 100.0);
    let shot = |scale: f64| {
        let view = crate::prim::View { scale, cx: middle.0, cy: middle.1, width: 60, height: 60 };
        let p = tiny_skia::Pixmap::decode_png(&crate::render::png(&crate::render::draw(&s.model, level, view)).unwrap()).unwrap();
        let light = |c: tiny_skia::PremultipliedColorU8| (u32::from(c.red()) + u32::from(c.green()) + u32::from(c.blue())) / 3;
        // The middle of the view, inside the room either way.
        let inside: Vec<_> = p
            .pixels()
            .iter()
            .enumerate()
            .filter(|(i, _)| (i % 60).abs_diff(30) < 10 && (i / 60).abs_diff(30) < 10)
            .map(|(_, c)| *c)
            .collect();
        (inside.iter().filter(|c| light(**c) < 100).count(), inside.iter().filter(|c| light(**c) > 240).count(), inside.len())
    };
    let (dark, white, n) = shot(0.2);
    assert!(dark > n / 20 && white > n / 4, "zoomed in, the bricks: {dark} dark and {white} white of {n}");
    let (dark, white, n) = shot(0.01);
    assert!(dark == 0 && white == 0, "zoomed out, the tint: {dark} dark and {white} white of {n}");
}

#[test]
fn a_gradient_is_drawn_as_a_gradient_axial_or_radial() {
    // An axial gradient from red to blue over the left half, extended; a radial one from white at the middle to green at
    // 40 points on the right, cut to a square.
    let content = "q 10 50 80 100 re W n /Sh0 sh Q q 105 55 90 90 re W n /Sh1 sh Q";
    let resources = "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 0 90 0] /Extend [true true] /Function << \
                     /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> \
                     /Sh1 << /ShadingType 3 /ColorSpace /DeviceRGB /Coords [150 100 0 150 100 40] /Extend [false true] /Function << \
                     /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [0 0.6 0] /N 1 >> >> >>";
    let bytes = pdf(content, resources, "", &[]);
    let (s, _) = imported(bytes.clone());
    let p = plan(&s);
    let want = reference(&bytes);
    for (what, b) in [("axial", [12.0, 52.0, 88.0, 148.0]), ("radial", [107.0, 57.0, 193.0, 143.0])] {
        let (diff, colours) = compared(&p, &want, b);
        assert!(diff < 6.0, "the {what} gradient is drawn as the renderer paints it: a mean difference of {diff:.1}");
        assert!(colours >= 8, "the {what} gradient goes from one colour to the other, not one flat colour: {colours}");
    }
    // Its bands are a gradient's, which keep their colours on a dark canvas, drawn as such.
    let t = s.model.image_types.values().next().unwrap();
    let bands = t.lines().unwrap().dxf().entities.iter().filter(|e| matches!(e.shape, crate::dxf::Shape::Fill { shade: true, .. })).count();
    assert!(bands > 50, "{bands} bands");
    let d =
        crate::render::draw(&s.model, s.model.levels()[0].0, crate::prim::View { scale: 0.05, cx: 0.0, cy: 0.0, width: 400, height: 400 });
    assert!(d.parts.iter().filter(|p| **p == crate::prim::Part::Shade).count() > 50, "drawn as a gradient's");
    // Smooth: along the axial one, each pixel moves by little from the one before.
    let row: Vec<[u8; 3]> = (0..140).map(|i| px(&p, 15.0 + i as f64 * 0.5, 100.0)).collect();
    let jump = row.windows(2).map(|w| (0..3).map(|k| (i32::from(w[0][k]) - i32::from(w[1][k])).abs()).max().unwrap_or(0)).max();
    assert!(jump.unwrap_or(0) <= 12, "no band shows: a step of {jump:?}");
}

/// The lengths of the dark runs along row `y` (page points) of the plan, from page x `from` to `to`, px.
fn runs(p: &tiny_skia::Pixmap, y: f64, from: f64, to: f64) -> Vec<u32> {
    let ((x0, j), (x1, _)) = (at(from, y), at(to, y));
    let mut out = vec![];
    let mut n = 0;
    for i in x0..=x1 {
        let c = p.pixel(i, j).unwrap();
        if dark([c.red(), c.green(), c.blue()]) {
            n += 1;
        } else if n > 0 {
            out.push(n);
            n = 0;
        }
    }
    if n > 0 {
        out.push(n);
    }
    out
}

#[test]
fn a_dash_dot_keeps_its_dots_and_a_dash_its_phase() {
    // 12 on, 3 off, 2 on, 3 off; and 10 on, 10 off begun 5 into it.
    let content = "0 0 0 RG 2 w 0 J [12 3 2 3] 0 d 10 100 m 190 100 l S [10 10] 5 d 10 150 m 190 150 l S";
    let (s, _) = imported(pdf(content, "", "", &[]));
    let p = plan(&s);
    let dash_dot = runs(&p, 100.0, 5.0, 195.0);
    assert!(dash_dot.iter().any(|n| *n <= 6) && dash_dot.iter().any(|n| *n >= 15), "long dashes and dots: {dash_dot:?}");
    let phased = runs(&p, 150.0, 5.0, 195.0);
    let (first, next) = (phased[0], phased[1]);
    assert!(first * 3 < next * 2, "the first dash is half of the others: {phased:?}");
}

/// The box round the dark pixels of the plan between page points `lo` and `hi`: page points, x0 y0 x1 y1.
fn inked(p: &tiny_skia::Pixmap, lo: (f64, f64), hi: (f64, f64)) -> Option<[f64; 4]> {
    let ((i0, j1), (i1, j0)) = (at(lo.0, lo.1), at(hi.0, hi.1));
    let page = |i: u32, j: u32| {
        let k = 72.0 / 25.4 / 100.0 / 0.05;
        ((f64::from(i) - 200.0) * k + 100.0, (200.0 - f64::from(j)) * k + 100.0)
    };
    let mut b: Option<[f64; 4]> = None;
    for j in j0..=j1 {
        for i in i0..=i1 {
            let c = p.pixel(i, j).unwrap();
            if dark([c.red(), c.green(), c.blue()]) {
                let (x, y) = page(i, j);
                b = Some(match b {
                    None => [x, y, x, y],
                    Some([a, c, d, e]) => [a.min(x), c.min(y), d.max(x), e.max(y)],
                });
            }
        }
    }
    b
}

#[test]
fn text_keeps_its_squeeze_its_spaces_and_its_mirror() {
    // A squeeze holds till it is set again, past the end of its text object.
    let content = "0 0 0 rg BT /F1 20 Tf 10 170 Td (WIDE) Tj ET BT /F1 20 Tf 50 Tz 10 140 Td (WIDE) Tj 100 Tz ET \
                   BT /F1 20 Tf 10 100 Td (   SPACED) Tj ET BT /F1 20 Tf -1 0 0 1 190 40 Tm (MIRROR) Tj ET";
    let (s, _) = imported(pdf(content, "", "", &[]));
    let p = plan(&s);
    let wide = inked(&p, (5.0, 165.0), (195.0, 190.0)).unwrap();
    let squeezed = inked(&p, (5.0, 135.0), (195.0, 160.0)).unwrap();
    let (w, sq) = (wide[2] - wide[0], squeezed[2] - squeezed[0]);
    assert!((sq / w - 0.5).abs() < 0.12, "50 Tz draws it half as wide: {sq} of {w}");
    // Three spaces of Helvetica at 20 points are 16.7 points: its first letter stands there, not at 10.
    let spaced = inked(&p, (5.0, 95.0), (195.0, 125.0)).unwrap();
    assert!((spaced[0] - 26.7).abs() < 3.0, "it starts after its spaces: {spaced:?}");
    // Mirrored, it reads backwards from 190 to the left, standing on its baseline at 40, not hanging under it.
    let mirror = inked(&p, (5.0, 20.0), (195.0, 70.0)).unwrap();
    assert!(mirror[2] < 192.0 && mirror[0] > 90.0, "to the left of 190: {mirror:?}");
    assert!(mirror[1] > 37.0 && mirror[3] > 50.0, "above its baseline: {mirror:?}");
}

#[test]
fn a_line_keeps_its_width_however_the_page_stretches_it() {
    // Stretched four times across: a 2 point line along it is 2 points thick, one up it 8.
    let content = "q 4 0 0 1 0 0 cm 0 0 0 RG 2 w 5 100 m 40 100 l S 45 20 m 45 180 l S Q";
    let (s, _) = imported(pdf(content, "", "", &[]));
    let p = plan(&s);
    let along = inked(&p, (30.0, 90.0), (150.0, 110.0)).unwrap();
    let up = inked(&p, (170.0, 40.0), (190.0, 160.0)).unwrap();
    let (thick_along, thick_up) = (along[3] - along[1], up[2] - up[0]);
    assert!((thick_along - 2.0).abs() < 1.0, "2 points: {thick_along}");
    assert!((thick_up - 8.0).abs() < 1.5, "8 points: {thick_up}");
}

#[test]
fn a_purged_page_leaves_no_layers_for_an_older_build_to_trip_on() {
    let content = "/OC /L0 BDC 0 0 0 RG 4 w 10 10 m 190 10 l S EMC /OC /L1 BDC 1 0 0 RG 4 w 40 150 m 160 150 l S EMC";
    let bytes = crate::raster::pdf::sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Furniture", false)]);
    let (mut s, r) = imported(bytes);
    let level = s.model.guid(s.model.levels()[0].0).to_string();
    run(&mut s, "cad_layers", json!({"id": r["id"], "view": level, "hide": ["Walls"]}));
    run(&mut s, "delete", json!({"ids": [r["id"]]}));
    // Its image type taken out of the library, as a purge or a merge does.
    assert!(s.model_mut().remove_entry(&crate::library::Kind::Image, "p.pdf"));
    let g = run(&mut s, "graphics", json!({"view": level}));
    assert_eq!(g["pdf_layers"], json!({}), "the view forgets what it hid of it: {g}");
    let v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(v["format"], crate::model::PREVIOUS_FORMAT, "nothing in it needs a newer build");
    // A view that hides a page's layers needs the build that knows them, whatever else it holds.
    let (mut s, r) = imported(crate::raster::pdf::sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Furniture", false)]));
    let level = s.model.guid(s.model.levels()[0].0).to_string();
    run(&mut s, "cad_layers", json!({"id": r["id"], "view": level, "hide": ["Walls"]}));
    let mut m = (*s.model).clone();
    m.image_types.clear();
    let v: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
    assert_eq!(v["format"], crate::model::LAYERS_FORMAT, "pdf_layers needs format 21");
}

#[test]
fn a_picture_is_cut_to_a_clip_of_any_shape_and_as_see_through_as_the_page_makes_it() {
    // A black picture 160 points a side three times over, each over the line along the top that makes the page one drawn from
    // its lines: in a triangle; at half its opacity; and under a soft mask that lets its left half through.
    let image = "BI /W 2 /H 2 /CS /RGB /BPC 8 /F /AHx ID 000000000000000000000000> EI";
    let line = "0 0 0 RG 1 w 10 195 m 190 195 l S";
    let resources = "/ExtGState << /GS0 << /Type /ExtGState /ca 0.5 >> /GS1 << /Type /ExtGState /SMask << /S /Luminosity /G 5 0 R >> >> >>";
    let mask = "1 1 1 rg 0 0 100 200 re f";
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 200 200] /Group << /S /Transparency /CS /DeviceRGB >> /Length {} >>\nstream\n{mask}\nendstream",
        mask.len()
    );
    let page = |content: String| pdf(&content, resources, "", &[&form]);
    let placed = format!("q 160 0 0 160 20 20 cm {image} Q");
    for (what, content) in [
        ("triangle", format!("{line} q 20 20 m 180 20 l 20 180 l h W n {placed} Q")),
        ("half", format!("{line} q /GS0 gs {placed} Q")),
        ("masked", format!("{line} q /GS1 gs {placed} Q")),
    ] {
        let bytes = page(content);
        let (s, r) = imported(bytes.clone());
        assert!(r["layers"].as_array().is_some_and(|l| !l.is_empty()), "{what}: drawn from its lines: {r}");
        let p = plan(&s);
        let want = reference(&bytes);
        let (diff, _) = compared(&p, &want, [22.0, 22.0, 178.0, 178.0]);
        assert!(diff < 8.0, "{what}: the picture shows as the renderer paints it: a mean difference of {diff:.1}");
        match what {
            "triangle" => {
                assert!(dark(px(&p, 40.0, 40.0)), "inside the triangle");
                assert!(white(px(&p, 160.0, 160.0)), "outside it, in the picture's box: {:?}", px(&p, 160.0, 160.0));
            }
            "half" => {
                let g = px(&p, 100.0, 100.0);
                assert!(g.iter().all(|v| (110..=145).contains(v)), "half of black over white is grey: {g:?}");
            }
            _ => {
                assert!(dark(px(&p, 60.0, 100.0)), "the mask lets the left half through");
                assert!(white(px(&p, 140.0, 100.0)), "and not the right: {:?}", px(&p, 140.0, 100.0));
            }
        }
    }
}

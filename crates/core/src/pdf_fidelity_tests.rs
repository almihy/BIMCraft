//! A PDF's page drawn from its lines as the PDF draws it, through the commands every front end drives: fills keep their holes
//! by either rule, see-through paint stays see-through, a clip of any shape cuts what it clips, a form's own layer holds what
//! it draws, a page's white ground is no page, gradients and patterns fill in their colours, a dash-dot keeps its dots, text
//! keeps its squeeze, its spaces and its mirror, a line keeps its width however the page stretches it, and a purged page
//! leaves nothing behind for an older build to stumble on. Each was seen failing before its fix.

use serde_json::{Value, json};

use crate::engine::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A one-page PDF 200 points a side: its content stream `content`, what its resources hold besides the font `/F1`
/// (Helvetica), what its catalog holds besides its pages, and objects of its own from 5 on (4 is the content).
fn pdf(content: &str, resources: &str, catalog: &str, more: &[&str]) -> Vec<u8> {
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>"),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 << /Type /Font \
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

#[test]
fn a_gradient_and_a_pattern_fill_in_their_colours() {
    // An axial gradient from red to blue over the left half; a red square on half of each cell of a pattern on the right.
    let content = "q 10 50 80 100 re W n /Sh0 sh Q /Pattern cs /P0 scn 110 50 80 100 re f";
    let resources = "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 0 90 0] /Extend [true true] /Function << \
                     /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >> >> /Pattern << /P0 5 0 R >>";
    let cell = "1 0 0 rg 0 0 10 5 re f";
    let pattern = format!(
        "<< /Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 /Resources << >> /Length {} >>\nstream\n{cell}\nendstream",
        cell.len()
    );
    let (s, _) = imported(pdf(content, resources, "", &[&pattern]));
    let p = plan(&s);
    let g = px(&p, 50.0, 100.0);
    assert!(g[0] > 80 && g[2] > 80 && g[1] < 60, "the gradient's middle is between red and blue: {g:?}");
    let t = px(&p, 150.0, 100.0);
    assert!(t[0] > 230 && t[1] > 90 && t[1] < 170 && (i32::from(t[1]) - i32::from(t[2])).abs() < 20, "half red is pink: {t:?}");
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
    assert_eq!(v["format"], crate::model::FORMAT, "pdf_layers is this build's");
}

//! A PDF's layers through the commands every front end drives, as a DWG import's are: a page comes in with its optional
//! content groups as layers (those the file has off hidden), drawn from its lines so a layer hidden goes at once, in every
//! view (`cad_layers`) or in one (its Visibility/Graphics' Imported Categories, `graphics` `pdf_layers`), an agent reads one
//! layer's lines and words, and the project file keeps it all.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::raster::pdf::{sample, sample_layered};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A plan 200 points a side on three layers: Walls (a thick black line along its bottom, 10 points up), Furniture (a red
/// chair, off in the file) and Notes ("Kitchen"); a hairline border on no layer.
fn plan_pdf() -> Vec<u8> {
    let content = "/OC /L0 BDC 0 0 0 RG 4 w 10 10 m 190 10 l S EMC /OC /L1 BDC 1 0 0 RG 4 w 40 150 m 160 150 l S EMC \
                   /OC /L2 BDC BT /F1 12 Tf 20 100 Td (Kitchen) Tj ET EMC 0 0 0 RG 0 w 0 0 200 200 re S";
    sample_layered(content, [200.0, 200.0], &[("Walls", false), ("Furniture", true), ("Notes", false)])
}

fn with_file(name: &str, bytes: Vec<u8>) -> Session {
    let mut s = Session::new();
    s.files.give(name, bytes);
    s
}

/// The plan of the lowest level, 400 px a side at 20 mm a pixel round the origin.
fn plan(s: &Session) -> tiny_skia::Pixmap {
    let view = crate::prim::View { scale: 0.05, cx: 0.0, cy: 0.0, width: 400, height: 400 };
    let d = crate::render::draw_at(&s.model, s.model.levels()[0].0, view, 1.0);
    tiny_skia::Pixmap::decode_png(&crate::render::png(&d).unwrap()).unwrap()
}

/// Whether the plan has a pixel of colour `want` (each channel within 60) within two of (`x`, `y`).
fn has(p: &tiny_skia::Pixmap, x: u32, y: u32, want: [u8; 3]) -> bool {
    (x - 2..=x + 2).any(|x| {
        (y - 2..=y + 2).any(|y| {
            p.pixel(x, y)
                .is_some_and(|c| [c.red(), c.green(), c.blue()].iter().zip(want).all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() < 60))
        })
    })
}

/// Where a point of the page (points from its bottom left) shows on the plan, the page at 1 : 100 round the origin: px.
fn at(x: f64, y: f64) -> (u32, u32) {
    let mm = |v: f64| (v - 100.0) * 25.4 / 72.0 * 100.0;
    ((200.0 + mm(x) * 0.05).round() as u32, (200.0 - mm(y) * 0.05).round() as u32)
}

#[test]
fn a_pdfs_layers_come_in_and_show_and_hide_as_a_dwgs_do() {
    let mut s = with_file("plan.pdf", plan_pdf());
    let r = run(&mut s, "import_pdf", json!({"path": "plan.pdf", "scale": 100}));
    assert_eq!(r["layers"], json!(["Walls", "Furniture", "Notes", "No layer"]), "{r}");
    let id = r["id"].clone();
    let level = s.model.guid(s.model.levels()[0].0).to_string();
    // Furniture, off in the file, comes in hidden in the view it is placed in, unticked in its Visibility/Graphics, as Revit
    // shows a DWG's layers that are off; each layer has the colour of what is on it.
    let l = run(&mut s, "cad_layers", json!({"id": id, "view": level}));
    assert_eq!(l["image"], "plan.pdf");
    let shown: Vec<(String, bool)> = l["layers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["name"].as_str().unwrap().to_string(), r["shown"] == true && r["shown_in_view"] == true))
        .collect();
    assert_eq!(shown[..3], [("Walls".into(), true), ("Furniture".into(), false), ("Notes".into(), true)]);
    let colour = |n: &str| l["layers"].as_array().unwrap().iter().find(|r| r["name"] == n).unwrap()["color"].clone();
    assert_eq!((colour("Walls"), colour("Furniture")), (json!([0, 0, 0]), json!([255, 0, 0])), "{l}");
    assert_eq!(run(&mut s, "graphics", json!({"view": level}))["pdf_layers"], json!({"plan.pdf": ["Furniture"]}));
    let (wx, wy) = at(100.0, 10.0);
    let (cx, cy) = at(100.0, 150.0);
    let p = plan(&s);
    assert!(has(&p, wx, wy, [0, 0, 0]), "the wall shows");
    assert!(!has(&p, cx, cy, [255, 0, 0]), "the chair, off in the file, doesn't");
    // Shown everywhere, the chair is drawn; Walls hidden in this view only, the wall goes at once.
    run(&mut s, "cad_layers", json!({"id": id, "show": ["Furniture"]}));
    let l = run(&mut s, "cad_layers", json!({"id": id, "view": level, "hide": ["Walls"]}));
    let walls = l["layers"].as_array().unwrap().iter().find(|r| r["name"] == "Walls").unwrap().clone();
    assert_eq!((walls["shown"].clone(), walls["shown_in_view"].clone()), (json!(true), json!(false)), "{l}");
    let p = plan(&s);
    assert!(has(&p, cx, cy, [255, 0, 0]), "the chair shows");
    assert!(!has(&p, wx, wy, [0, 0, 0]), "the wall is hidden in the view");
    let g = run(&mut s, "graphics", json!({"view": level}));
    assert_eq!(g["pdf_layers"], json!({"plan.pdf": ["Walls"]}), "{g}");
    // The view's Visibility/Graphics shows it again.
    run(&mut s, "graphics", json!({"view": level, "pdf_layers": {"plan.pdf": null}}));
    assert!(has(&plan(&s), wx, wy, [0, 0, 0]));
    let e = fails(&mut s, "graphics", json!({"view": level, "pdf_layers": {"plan.pdf": ["Doors"]}}));
    assert!(e.contains("no layer `Doors`"), "{e}");
    // An agent asks for one layer's lines and words.
    let notes = run(&mut s, "cad_read", json!({"id": id, "layers": ["Notes"]}));
    let items = notes["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{notes}");
    assert_eq!((items[0]["kind"].clone(), items[0]["text"].clone()), (json!("text"), json!("Kitchen")));
    let walls = run(&mut s, "cad_read", json!({"id": id, "layers": ["Walls"]}));
    assert_eq!(walls["total"], 1, "{walls}");
}

#[test]
fn a_pdf_with_no_layers_has_its_lines_by_colour_and_weight_to_hide() {
    let content = "0 0 0 RG 4 w 10 10 m 190 10 l S 1 0 0 RG 1 w 40 150 m 160 150 l S";
    let mut s = with_file("plain.pdf", sample(content, [200.0, 200.0], None));
    let r = run(&mut s, "import_pdf", json!({"path": "plain.pdf", "scale": 100}));
    assert_eq!(r["layers"], json!(["Black 1.41", "Red 0.35"]), "{r}");
    let id = r["id"].clone();
    let (cx, cy) = at(100.0, 150.0);
    assert!(has(&plan(&s), cx, cy, [255, 0, 0]));
    run(&mut s, "cad_layers", json!({"id": id, "hide": ["Red 0.35"]}));
    assert!(!has(&plan(&s), cx, cy, [255, 0, 0]), "hidden wherever it shows");
}

#[test]
fn a_pdfs_layers_are_kept_in_the_file_and_forgotten_with_the_image() {
    let mut s = with_file("plan.pdf", plan_pdf());
    let id = run(&mut s, "import_pdf", json!({"path": "plan.pdf", "scale": 100}))["id"].clone();
    let level = s.model.guid(s.model.levels()[0].0).to_string();
    run(&mut s, "cad_layers", json!({"id": id, "view": level, "hide": ["Notes"]}));
    let text = s.model.to_json().unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["format"], crate::model::FORMAT, "a page drawn from its lines needs this build's format");
    let (m, left_out) = crate::model::Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let t = m.image_types.get("plan.pdf").unwrap();
    assert!(t.drawn() && t.thumbnail(64).is_some());
    let mut back = Session::new();
    back.model = std::sync::Arc::new(m);
    let l = run(&mut back, "cad_layers", json!({"id": id, "view": level}));
    let row = |n: &str| l["layers"].as_array().unwrap().iter().find(|r| r["name"] == n).unwrap().clone();
    assert_eq!((row("Furniture")["shown_in_view"].clone(), row("Notes")["shown_in_view"].clone()), (json!(false), json!(false)), "{l}");
    // Deleting the image forgets the layers views hid of it.
    run(&mut s, "manage_images", json!({"delete": ["plan.pdf"]}));
    let g = run(&mut s, "graphics", json!({"view": level}));
    assert_eq!(g["pdf_layers"], json!({}), "{g}");
}

#[test]
fn a_picture_has_no_layers_to_hide() {
    let mut s =
        with_file("scan.pdf", sample("q 200 0 0 200 0 0 cm BI /W 2 /H 2 /CS /G /BPC 8 /F /AHx ID 00FF00FF> EI Q", [200.0; 2], None));
    let r = run(&mut s, "import_pdf", json!({"path": "scan.pdf"}));
    assert!(r.get("layers").is_none(), "{r}");
    let e = fails(&mut s, "cad_layers", json!({"id": r["id"]}));
    assert!(e.contains("no vector lines"), "{e}");
}

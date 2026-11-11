//! CAD imports through the commands every front end drives: a DXF comes in, is placed as an element on a level, goes where the
//! element goes, is saved with the project, and is drawn and snapped to in its plans.

use std::path::PathBuf;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::Model;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

/// A folder of this test's own.
fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bimcraft-cad-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A file in this test's folder, written.
fn file(test: &str, name: &str, text: &str) -> String {
    let path = dir(test).join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

/// Pairs as a DXF writes them: a code line, then its value.
fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A DXF of `units` ($INSUNITS) with these layers (name, colour, off) and entities.
fn dxf(units: &str, layers: &[(&str, &str, bool)], entities: &str) -> String {
    let header = pairs(&[(9, "$INSUNITS"), (70, units)]);
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER")]);
    for (name, color, off) in layers {
        let color = if *off { format!("-{color}") } else { color.to_string() };
        table += &pairs(&[(0, "LAYER"), (2, name), (70, "0"), (62, &color), (6, "Continuous")]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{table}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    )
}

fn line(layer: &str, a: (f64, f64), b: (f64, f64)) -> String {
    let (ax, ay, bx, by) = (a.0.to_string(), a.1.to_string(), b.0.to_string(), b.1.to_string());
    pairs(&[(0, "LINE"), (8, layer), (10, &ax), (20, &ay), (11, &bx), (21, &by)])
}

/// A plan of a room 10 m by 6 m drawn in metres, its walls on WALLS (red), its furniture on FURN (off), a note on TEXT.
fn plan() -> String {
    let walls: String = [((0.0, 0.0), (10.0, 0.0)), ((10.0, 0.0), (10.0, 6.0)), ((10.0, 6.0), (0.0, 6.0)), ((0.0, 6.0), (0.0, 0.0))]
        .iter()
        .map(|(a, b)| line("WALLS", *a, *b))
        .collect();
    let furn = line("FURN", (1.0, 1.0), (2.0, 1.0));
    let note = pairs(&[(0, "TEXT"), (8, "TEXT"), (10, "1"), (20, "5"), (40, "0.25"), (1, "Kitchen")]);
    dxf("6", &[("WALLS", "1", false), ("FURN", "3", true), ("TEXT", "7", false)], &(walls + &furn + &note))
}

fn one(s: &mut Session) -> Value {
    let all = run(s, "elements", json!({"category": "cadimport"}));
    assert_eq!(all["count"], 1, "{all}");
    all["elements"][0].clone()
}

fn near(v: &Value, want: [[f64; 2]; 2]) -> bool {
    (0..2).all(|i| (0..2).all(|k| v[i][k].as_f64().is_some_and(|x| (x - want[i][k]).abs() < 1e-6)))
}

#[test]
fn a_drawing_comes_in_in_its_files_unit_its_middle_on_the_origin_with_its_layers_off_hidden() {
    let mut s = Session::new();
    let r = run(&mut s, "import_cad", json!({"path": file("in", "ground floor.dxf", &plan())}));
    assert_eq!(
        (r["cad"].as_str(), r["source"].as_str(), r["unit"].as_str()),
        (Some("ground floor"), Some("ground floor.dxf"), Some("m")),
        "{r}"
    );
    assert_eq!(
        (r["scale"].as_f64(), r["entities"].as_u64(), r["layers"].as_u64(), r["far"].as_u64()),
        (Some(1000.0), Some(6), Some(3), Some(0))
    );
    // Metres, so 10 m by 6 m, its middle on the origin.
    assert!(near(&r["extents"], [[-5000.0, -3000.0], [5000.0, 3000.0]]), "{r}");
    let e = one(&mut s);
    assert_eq!(
        (e["id"].as_u64(), e["cad"].as_str(), &e["hidden"], e["layers"].as_u64()),
        (r["id"].as_u64(), Some("ground floor"), &json!(["FURN"]), Some(3)),
        "{e}"
    );
    assert_eq!(e["level"], json!(s.model.guid(s.model.levels()[0].0)));
    assert!(e.get("view").is_none() && e.get("colors").is_none(), "every plan of its level, its colours as they are: {e}");
}

#[test]
fn units_scale_position_angle_layers_and_colours_are_the_users_to_say() {
    let mut s = Session::new();
    let path = file("say", "plan.dxf", &plan());
    // In centimetres instead, its origin on (1000, 2000), turned a quarter.
    let r = run(&mut s, "import_cad", json!({"path": path, "units": "cm", "position": "origin", "at": [1000, 2000], "angle": 90}));
    assert!(near(&r["extents"], [[940.0, 2000.0], [1000.0, 2100.0]]), "{r}");
    assert_eq!(r["unit"], "cm (its file says m)", "the unit it was read in");
    // A scale of its own, all its layers, in black.
    let r = run(&mut s, "import_cad", json!({"cad": "plan", "scale": 2, "position": "origin", "layers": "all", "colors": "black"}));
    assert!(near(&r["extents"], [[0.0, 0.0], [20.0, 12.0]]), "{r}");
    assert_eq!(r["unit"], "a scale of its own (its file says m)");
    let e = &run(&mut s, "elements", json!({"category": "cadimport"}))["elements"][1];
    assert_eq!((e.get("hidden"), e["colors"].as_str()), (None, Some("black")), "{e}");
    // A list of the layers to show hides the rest.
    let r = run(&mut s, "import_cad", json!({"cad": "plan", "layers": ["WALLS"]}));
    let e = crate::element_json(&s.model, r["id"].as_u64().unwrap());
    assert_eq!(e["hidden"], json!(["FURN", "TEXT"]));
    for (p, says) in [
        (json!({"cad": "plan", "units": "furlong"}), "units"),
        (json!({"cad": "plan", "units": "mm", "scale": 2}), "not both"),
        (json!({"cad": "plan", "scale": 0}), "scale"),
        (json!({"cad": "plan", "position": "left"}), "position"),
        (json!({"cad": "plan", "colors": "pink"}), "colors"),
        (json!({"cad": "plan", "layers": ["NOPE"]}), "no layer `NOPE`"),
        (json!({"cad": "plan", "layers": 5}), "layers"),
        (json!({"cad": "plan", "link": true}), "link"),
        (json!({"cad": "nope"}), "holds no CAD drawing named `nope`"),
        (json!({"path": path, "cad": "plan"}), "not both"),
        (json!({}), "give `path`"),
        (json!({"path": path}), "already holds a CAD drawing named `plan`"),
    ] {
        let e = fails(&mut s, "import_cad", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
}

#[test]
fn a_dwg_says_to_save_it_as_dxf_and_a_file_that_isnt_one_says_why() {
    let mut s = Session::new();
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "plan.dwg", "AC1032")}));
    assert!(e.contains("DWG") && e.contains("save it as DXF"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "plan.pdf", "%PDF")}));
    assert!(e.contains("reads DXF"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "broken.dxf", "0\nSECTION\n2\nENTITIES\nhello\n")}));
    assert!(e.contains("broken.dxf") && e.contains("group code"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": file("dwg", "empty.dxf", &dxf("4", &[], ""))}));
    assert!(e.contains("draws nothing"), "{e}");
    let e = fails(&mut s, "import_cad", json!({"path": dir("dwg").join("nowhere.dxf")}));
    assert!(e.contains("nowhere.dxf"), "{e}");
    assert!(s.model.cads.is_empty());
}

/// A DWG reader as the apps hand a session one: here one that gives `plan()`'s DXF for a file that begins as a DWG does, with
/// a note, and says why for any other.
fn dwg_reader() -> crate::cad::DwgReader {
    std::sync::Arc::new(|path: &std::path::Path| {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        if !bytes.starts_with(b"AC10") {
            return Err("it isn't a DWG file".into());
        }
        Ok((plan().into_bytes(), vec!["2 of the DWG's object records couldn't be read and were left out".into()]))
    })
}

#[test]
fn a_dwg_comes_in_through_the_reader_a_session_is_given_and_is_held_as_the_dxf_it_became() {
    let mut s = Session::new();
    s.dwg = Some(dwg_reader());
    let path = file("dwg-reader", "site.dwg", "AC1032 a drawing");
    let r = run(&mut s, "import_cad", json!({"path": path, "link": true, "layers": "all"}));
    assert_eq!((r["cad"].as_str(), r["source"].as_str(), r["unit"].as_str()), (Some("site"), Some("site.dwg"), Some("m")), "{r}");
    assert_eq!(r["notes"], json!(["2 of the DWG's object records couldn't be read and were left out"]), "{r}");
    assert_eq!(s.model.cads["site"].dxf().entities.len(), 6);
    // Reloaded through it too; a file it can't read says why, and the drawing keeps what it had.
    assert_eq!(run(&mut s, "reload_cad", json!({"cad": "site"}))["changed"], false);
    std::fs::write(&path, "not a drawing").unwrap();
    let e = fails(&mut s, "reload_cad", json!({"cad": "site"}));
    assert!(e.contains("site.dwg") && e.contains("isn't a DWG"), "{e}");
    assert_eq!(s.model.cads["site"].dxf().entities.len(), 6);
    // A project file holds the DXF, so one opened where no DWG reader is still has the drawing; the link says why it wasn't
    // read again.
    let saved = dir("dwg-reader").join("p.bim");
    run(&mut s, "save", json!({"path": saved}));
    let mut plain = Session::new();
    let r = run(&mut plain, "open", json!({"path": saved}));
    assert_eq!(plain.model.cads["site"].dxf().entities.len(), 6);
    let why = r["cads_not_reloaded"].to_string();
    assert!(why.contains("site") && why.contains("save it as DXF"), "{r}");
}

#[test]
fn current_view_only_is_one_plan_of_its_level() {
    let mut s = Session::new();
    let path = file("view", "plan.dxf", &plan());
    let level = s.model.levels()[0].0;
    let r = run(&mut s, "import_cad", json!({"path": path, "view": level}));
    let e = crate::element_json(&s.model, r["id"].as_u64().unwrap());
    assert_eq!(e["view"], json!(s.model.guid(level)));
    // Level 2's plan, while saying level 1, is refused.
    let upper = s.model.levels()[1].0;
    let e = fails(&mut s, "import_cad", json!({"cad": "plan", "level": level, "view": upper}));
    assert!(e.contains("another level"), "{e}");
    // A plan view of a level names its level too.
    let pv = run(&mut s, "duplicate_view", json!({"view": upper}))["id"].as_u64().unwrap();
    let r = run(&mut s, "import_cad", json!({"cad": "plan", "view": pv}));
    let e = crate::element_json(&s.model, r["id"].as_u64().unwrap());
    assert_eq!((e["view"].clone(), e["level"].clone()), (json!(s.model.guid(pv)), json!(s.model.guid(upper))));
}

#[test]
fn an_import_moves_turns_copies_and_refuses_to_mirror() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("move", "plan.dxf", &plan())}))["id"].as_u64().unwrap();
    run(&mut s, "move", json!({"ids": [id], "by": [100, 200]}));
    run(&mut s, "rotate", json!({"ids": [id], "about": [100, 200], "angle": 90}));
    let e = one(&mut s);
    assert_eq!((&e["at"], e["angle"].as_f64()), (&json!([100.0, 200.0]), Some(90.0)), "{e}");
    assert!(near(&e["extents"], [[-2900.0, -4800.0], [3100.0, 5200.0]]), "{e}");
    let err = fails(&mut s, "mirror", json!({"ids": [id], "axis": [[0, 0], [0, 1000]]}));
    assert!(err.contains("can't be mirrored"), "{err}");
    let copies = run(&mut s, "copy", json!({"ids": [id], "by": [20000, 0]}));
    assert_eq!(run(&mut s, "elements", json!({"category": "cadimport"}))["count"], 2, "{copies}");
    assert_eq!(s.model.cads.len(), 1, "both place the one drawing");
}

#[test]
fn a_drawing_goes_when_nothing_places_it_and_undo_brings_it_back() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("delete", "plan.dxf", &plan())}))["id"].as_u64().unwrap();
    let again = run(&mut s, "import_cad", json!({"cad": "plan", "at": [20000, 0]}))["id"].as_u64().unwrap();
    run(&mut s, "delete", json!({"ids": [id]}));
    assert_eq!(s.model.cads.len(), 1, "one still places it");
    run(&mut s, "delete", json!({"ids": [again]}));
    assert!(s.model.cads.is_empty());
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.cads.len(), 1);
    run(&mut s, "undo", json!({"count": 2}));
    run(&mut s, "undo", json!({}));
    assert!(s.model.cads.is_empty(), "before it came in");
}

#[test]
fn a_drawing_is_saved_with_the_project_and_opens_as_it_was_and_insert_brings_it() {
    let mut s = Session::new();
    let path = file("save", "plan.dxf", &plan());
    let id = run(&mut s, "import_cad", json!({"path": path, "angle": 30, "at": [100, 200], "colors": "invert"}))["id"].as_u64().unwrap();
    let before = (one(&mut s), s.model.cads.clone());
    let project = dir("save").join("project.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let text = std::fs::read_to_string(&project).unwrap();
    let saved: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(saved["format"], json!(crate::model::FORMAT));
    assert!(saved["cads"]["plan"]["dxf"].is_string() && saved["cads"]["plan"]["source"] == "plan.dxf", "{}", saved["cads"]);
    assert!(saved["library"].to_string().contains("\"cad\""), "the drawing is a library entry: {}", saved["library"]);
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": project}));
    assert_eq!((one(&mut t), t.model.cads.clone()), before);
    // Insert from File brings the drawing with its import.
    let mut u = Session::new();
    run(&mut u, "insert", json!({"path": project, "ids": [id]}));
    assert_eq!((one(&mut u)["cad"].clone(), u.model.cads.get("plan")), (json!("plan"), s.model.cads.get("plan")));
}

#[test]
fn a_file_of_5_opens_and_what_a_file_holds_of_a_drawing_is_checked_as_it_is_read() {
    // A file of 5 or 6 holds no drawing: it opens, and is this build's format once read.
    let mut s = Session::new();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    for older in [5, 6] {
        v["format"] = json!(older);
        let (m, left_out) = Model::read(&v.to_string()).unwrap();
        assert_eq!((m.format, left_out.len()), (crate::model::FORMAT, 0));
    }
    // A drawing whose data isn't base64, isn't zlib, or isn't a DXF is refused, saying whose it is.
    run(&mut s, "import_cad", json!({"path": file("read", "plan.dxf", &plan())}));
    let good: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    for (data, says) in [
        (json!("!!!"), "base64"),
        (json!("aGVsbG8="), "zlib"),
        (json!(crate::commands::base64(&crate::cloud::deflate(b"0\nEOF\n"))), "ENTITIES"),
    ] {
        let mut bad = good.clone();
        bad["cads"]["plan"]["dxf"] = data;
        let e = Model::read(&bad.to_string()).unwrap_err();
        assert!(e.contains(says), "{e}");
    }
    // More drawings than a project holds is refused before the extra one is unpacked.
    let mut many = good.clone();
    for i in 0..crate::cad::MAX_CADS {
        many["cads"][format!("copy {i}")] = good["cads"]["plan"].clone();
    }
    let e = Model::read(&many.to_string()).unwrap_err();
    assert!(e.contains("CAD drawings") && e.contains(&crate::cad::MAX_CADS.to_string()), "{e}");
    // An import of a drawing the project doesn't hold is refused by the check.
    let mut orphan = good.clone();
    orphan["cads"] = json!({});
    let (m, _) = Model::read(&orphan.to_string()).unwrap();
    let e = m.check().unwrap_err();
    assert!(e.contains("no CAD drawing named `plan`"), "{e}");
}

#[test]
fn layers_are_hidden_and_shown_everywhere_or_in_one_view() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("layers", "plan.dxf", &plan())}))["id"].as_u64().unwrap();
    let shown = |r: &Value| -> Vec<(String, bool)> {
        r["layers"].as_array().unwrap().iter().map(|l| (l["name"].as_str().unwrap().to_string(), l["shown"].as_bool().unwrap())).collect()
    };
    let r = run(&mut s, "cad_layers", json!({"id": id}));
    assert_eq!(shown(&r), [("WALLS".into(), true), ("FURN".into(), false), ("TEXT".into(), true)]);
    assert_eq!(r["layers"][0]["color"], json!([255, 0, 0]));
    let r = run(&mut s, "cad_layers", json!({"id": id, "hide": ["TEXT"], "show": ["FURN"]}));
    assert_eq!(shown(&r), [("WALLS".into(), true), ("FURN".into(), true), ("TEXT".into(), false)]);
    // In one view: its graphics, by the drawing's name.
    let level = s.model.levels()[0].0;
    let r = run(&mut s, "cad_layers", json!({"id": id, "view": level, "hide": ["WALLS"]}));
    assert_eq!(r["layers"][0]["shown_in_view"], false);
    assert_eq!(r["layers"][1]["shown_in_view"], true);
    let g = crate::graphics::graphics_of(&s.model, level).unwrap();
    assert_eq!(g.layers.get("plan").map(|l| l.iter().cloned().collect::<Vec<_>>()), Some(vec!["WALLS".to_string()]));
    run(&mut s, "undo", json!({}));
    assert!(crate::graphics::graphics_of(&s.model, level).unwrap().layers.is_empty());
    // Visibility/Graphics says the same, and its reply lists them.
    let r = run(&mut s, "graphics", json!({"view": level, "layers": {"plan": ["FURN", "TEXT"]}}));
    assert_eq!(r["layers"], json!({"plan": ["FURN", "TEXT"]}));
    // A file holds the drawing by its GUID, and opens with it named again.
    let text = s.model.to_json().unwrap();
    let guid = s.model.library.guid(&crate::library::Kind::Cad, "plan").unwrap().to_string();
    assert!(text.contains(&format!("\"{guid}\":")), "keyed by GUID");
    let (m, _) = Model::read(&text).unwrap();
    assert_eq!(crate::graphics::graphics_of(&m, level).unwrap().layers.keys().collect::<Vec<_>>(), ["plan"]);
    let r = run(&mut s, "graphics", json!({"view": level, "layers": {"plan": null}}));
    assert_eq!(r["layers"], json!({}));
    for (p, says) in [
        (json!({"view": level, "layers": {"plan": ["NOPE"]}}), "NOPE"),
        (json!({"view": level, "layers": {"other": ["WALLS"]}}), "other"),
        (json!({"view": level, "layers": {"plan": [5]}}), "by name"),
        (json!({"view": level, "layers": 5}), "`layers`"),
    ] {
        let e = fails(&mut s, "graphics", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    for (p, says) in [
        (json!({"id": id, "hide": ["NOPE"]}), "no layer `NOPE`"),
        (json!({"id": id, "hide": ["FURN"], "show": ["FURN"]}), "in `hide` and in `show`"),
        (json!({"id": level}), "isn't a CAD import"),
        (json!({}), "`id`"),
    ] {
        let e = fails(&mut s, "cad_layers", p.clone());
        assert!(e.contains(says), "{p}: {e}");
    }
    // The drawing gone, no view hides its layers.
    run(&mut s, "graphics", json!({"view": level, "layers": {"plan": ["FURN"]}}));
    run(&mut s, "delete", json!({"ids": [id]}));
    assert!(crate::graphics::graphics_of(&s.model, level).unwrap().layers.is_empty());
}

#[test]
fn a_linked_drawing_is_read_again_by_reload_and_by_opening_the_project() {
    let mut s = Session::new();
    let path = file("link", "plan.dxf", &plan());
    run(&mut s, "import_cad", json!({"path": path, "link": true}));
    assert_eq!(one(&mut s)["linked"].as_str(), Some(path.as_str()));
    assert_eq!(run(&mut s, "reload_cad", json!({"cad": "plan"}))["changed"], false);
    // The file changes: one more line.
    let more = plan().replace("0\nENDSEC\n0\nEOF", &(line("WALLS", (0.0, 0.0), (10.0, 6.0)) + "0\nENDSEC\n0\nEOF"));
    std::fs::write(&path, &more).unwrap();
    let r = run(&mut s, "reload_cad", json!({"cad": "plan"}));
    assert_eq!((r["changed"].as_bool(), r["entities"].as_u64()), (Some(true), Some(7)));
    run(&mut s, "undo", json!({}));
    assert_eq!(s.model.cads["plan"].dxf().entities.len(), 6, "undo takes the reload back");
    // Saved, then the file changes again: opening reads it.
    let project = dir("link").join("project.bimcraft");
    run(&mut s, "save", json!({"path": project}));
    let mut t = Session::new();
    run(&mut t, "open", json!({"path": project}));
    assert_eq!(t.model.cads["plan"].dxf().entities.len(), 7);
    // Its file gone, the project keeps what it holds and says so.
    std::fs::remove_file(&path).unwrap();
    let r = run(&mut t, "open", json!({"path": project}));
    assert_eq!(t.model.cads["plan"].dxf().entities.len(), 6, "what the project file holds");
    assert_eq!(r["cads_not_reloaded"][0]["cad"], "plan", "{r}");
    // An imported (not linked) drawing has no file to read again.
    let mut u = Session::new();
    run(&mut u, "import_cad", json!({"path": file("link", "other.dxf", &plan())}));
    let e = fails(&mut u, "reload_cad", json!({"cad": "other"}));
    assert!(e.contains("isn't linked"), "{e}");
}

#[test]
fn query_names_the_entity_nearest_a_point() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("query", "plan.dxf", &plan()), "position": "origin"}))["id"].as_u64().unwrap();
    let r = run(&mut s, "cad_query", json!({"id": id, "at": [5000, 30]}));
    assert_eq!(
        (r["kind"].as_str(), r["layer"].as_str(), &r["color"], r["distance"].as_f64()),
        (Some("line"), Some("WALLS"), &json!([255, 0, 0]), Some(30.0)),
        "{r}"
    );
    // FURN is hidden: its line isn't found, the wall's is further off.
    assert_eq!(run(&mut s, "cad_query", json!({"id": id, "at": [1500, 1000], "reach": 50}))["found"], false);
    assert_eq!(run(&mut s, "cad_query", json!({"id": id, "at": [1000, 5000]}))["kind"], "text");
}

#[test]
fn stray_entities_far_off_are_counted_and_left_out_of_its_extents() {
    // The bulk: a 10 m by 6 m room of many short lines, and one line a hundred kilometres away.
    let mut entities: String = (0..40).map(|i| line("WALLS", (f64::from(i) * 0.25, 0.0), (f64::from(i) * 0.25 + 0.25, 6.0))).collect();
    entities += &line("JUNK", (100_000.0, 100_000.0), (100_001.0, 100_000.0));
    let mut s = Session::new();
    let r = run(&mut s, "import_cad", json!({"path": file("far", "plan.dxf", &dxf("6", &[], &entities))}));
    assert_eq!(r["far"], 1, "{r}");
    assert!(r["warning"].as_str().is_some_and(|w| w.contains("far from the rest")), "{r}");
    assert!(near(&r["extents"], [[-5000.0, -3000.0], [5000.0, 3000.0]]), "the bulk's, centred: {r}");
    // So placed far away, what's past the model's reach is refused.
    let e = fails(&mut s, "import_cad", json!({"cad": "plan", "at": [1e9, 0]}));
    assert!(e.contains("place it nearer"), "{e}");
}

// ---------- plans ----------

use crate::geom::P2;
use crate::render::{self, Rgb};
use crate::snap::{Kind, Reach, Snaps, snap};

/// How many pixels of a drawing's PNG are `colour`'s.
fn drawn(d: &render::Drawing, colour: impl Fn(Rgb) -> bool) -> usize {
    let pm = tiny_skia::Pixmap::decode_png(&render::png(d).unwrap()).unwrap();
    pm.pixels().iter().map(|p| p.demultiply()).filter(|p| colour(Rgb(p.red(), p.green(), p.blue()))).count()
}

/// Red, or a thin red line's pixels on the paper: the finest pen is less than a pixel wide in a plan fitted to the drawing.
fn reddish(c: Rgb) -> bool {
    c.0 > 200 && i32::from(c.0) - i32::from(c.1) > 40 && c.1.abs_diff(c.2) < 20
}

fn greenish(c: Rgb) -> bool {
    c.1 > 200 && i32::from(c.1) - i32::from(c.0) > 40 && c.0.abs_diff(c.2) < 20
}

fn levels(s: &Session) -> (u64, u64) {
    let l = s.model.levels();
    (l[0].0, l[1].0)
}

#[test]
fn a_plan_of_its_level_draws_it_in_its_colours_and_picks_it_as_one() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("draw", "plan.dxf", &plan()), "position": "origin"}))["id"].as_u64().unwrap();
    let (one, two) = levels(&s);
    let d = render::plan(&s.model, one, 600, 600);
    let reds = drawn(&d, reddish);
    assert!(reds > 300, "the red walls: {reds}");
    assert_eq!(drawn(&d, greenish), 0, "FURN is off in its file");
    // Picked where its lines are, as one element; nothing between them.
    assert_eq!(d.pick(d.view.px(P2(5000.0, 0.0)), 3.0), Some(id));
    assert_eq!(d.pick(d.view.px(P2(5000.0, 3000.0)), 3.0), None);
    // Its note is lettered.
    assert!(d.owners.iter().zip(&d.parts).any(|(o, p)| *o == Some(id) && *p == render::Part::Text), "the note");
    // Level 2's plan doesn't show it.
    assert!(!render::plan(&s.model, two, 600, 600).owners.contains(&Some(id)));
    // All its layers: the furniture too.
    run(&mut s, "cad_layers", json!({"id": id, "show": ["FURN"]}));
    assert!(drawn(&render::plan(&s.model, one, 600, 600), greenish) > 10);
    // In black, no colour at all.
    run(&mut s, "modify", json!({"id": id, "colors": "black"}));
    let d = render::plan(&s.model, one, 600, 600);
    assert_eq!((drawn(&d, reddish), drawn(&d, greenish)), (0, 0));
    // Inverted, red is cyan.
    run(&mut s, "modify", json!({"id": id, "colors": "invert"}));
    let cyan = |c: Rgb| c.2 > 200 && c.1 > 200 && i32::from(c.2) - i32::from(c.0) > 40;
    assert!(drawn(&render::plan(&s.model, one, 600, 600), cyan) > 300);
}

#[test]
fn a_view_hides_its_layers_or_all_of_it_and_current_view_only_is_that_view() {
    let mut s = Session::new();
    let path = file("hide", "plan.dxf", &plan());
    let id = run(&mut s, "import_cad", json!({"path": path, "position": "origin"}))["id"].as_u64().unwrap();
    let (one, _) = levels(&s);
    let other = run(&mut s, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    // Level 1's plan hides WALLS; the plan view of the same level still shows them.
    run(&mut s, "cad_layers", json!({"id": id, "view": one, "hide": ["WALLS"]}));
    assert_eq!(drawn(&render::plan(&s.model, one, 600, 600), reddish), 0);
    assert!(drawn(&render::plan(&s.model, other, 600, 600), reddish) > 300);
    // The category hidden in a view, as Revit's Imported Categories.
    run(&mut s, "graphics", json!({"view": other, "categories": {"cadimport": {"hidden": true}}}));
    assert!(!render::plan(&s.model, other, 600, 600).owners.contains(&Some(id)));
    // Current view only: in that plan, not in the other plan of its level.
    let mut t = Session::new();
    let (one, _) = levels(&t);
    let other = run(&mut t, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    let id = run(&mut t, "import_cad", json!({"path": path, "view": other}))["id"].as_u64().unwrap();
    assert!(render::plan(&t.model, other, 600, 600).owners.contains(&Some(id)));
    assert!(!render::plan(&t.model, one, 600, 600).owners.contains(&Some(id)));
    // It goes with its view, and its drawing with it.
    run(&mut t, "delete", json!({"ids": [other]}));
    assert!(t.model.elements.get(id).is_none() && t.model.cads.is_empty());
}

#[test]
fn a_hatch_fills_text_is_aligned_and_a_dashed_layer_draws_dashed() {
    // A solid square, a note centred on a point, and a dashed line, in mm.
    let solid = pairs(&[
        (0, "SOLID"),
        (8, "FILL"),
        (62, "5"),
        (10, "0"),
        (20, "0"),
        (11, "1000"),
        (21, "0"),
        (12, "0"),
        (22, "1000"),
        (13, "1000"),
        (23, "1000"),
    ]);
    let note =
        pairs(&[(0, "TEXT"), (8, "0"), (10, "0"), (20, "0"), (11, "3000"), (21, "500"), (40, "200"), (72, "1"), (73, "2"), (1, "MID")]);
    let dashed = pairs(&[(0, "LINE"), (8, "0"), (6, "DASHED"), (10, "0"), (20, "-2000"), (11, "4000"), (21, "-2000")]);
    let ltype = pairs(&[(0, "TABLE"), (2, "LTYPE"), (0, "LTYPE"), (2, "DASHED"), (49, "200"), (49, "-100"), (0, "ENDTAB")]);
    let text = dxf("4", &[], &(solid + &note + &dashed)).replace("0\nSECTION\n2\nTABLES\n", &format!("0\nSECTION\n2\nTABLES\n{ltype}"));
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("hatch", "detail.dxf", &text), "position": "origin"}))["id"].as_u64().unwrap();
    let (one, _) = levels(&s);
    let d = render::plan(&s.model, one, 800, 800);
    let blue = |c: Rgb| c.2 > 200 && c.0 < 60 && c.1 < 60;
    // The square is a metre a side: filled through.
    let side = 1000.0 * d.view.scale;
    assert!(drawn(&d, blue) as f64 > side * side * 0.8, "{} of {}", drawn(&d, blue), side * side);
    // The note's middle is on its point: its strokes lie either side of it.
    let (x, y) = d.view.px(P2(3000.0, 500.0));
    let strokes: Vec<(f32, f32)> = d
        .prims
        .iter()
        .zip(&d.parts)
        .zip(&d.owners)
        .filter(|((_, part), o)| **part == render::Part::Text && **o == Some(id))
        .flat_map(|((p, _), _)| p.points())
        .collect();
    let (lo, hi) = strokes.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.0), hi.max(p.0)));
    assert!(lo < x && hi > x && ((lo + hi) / 2.0 - x).abs() < 3.0, "{lo} {hi} about {x}");
    let (top, bottom) = strokes.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
    assert!(((top + bottom) / 2.0 - y).abs() < 3.0, "{top} {bottom} about {y}");
    // The dashed line carries its dashes, scaled to the view.
    let dash = d.prims.iter().find_map(|p| match p {
        render::Prim::Line { dash: Some(dash), pts, .. } if pts.len() == 2 => Some(dash.clone()),
        _ => None,
    });
    let k = d.view.scale as f32;
    assert!(
        dash.as_deref().is_some_and(|d| d.len() == 2 && (d[0] - 200.0 * k).abs() < 0.01 && (d[1] - 100.0 * k).abs() < 0.01),
        "{dash:?}"
    );
}

#[test]
fn a_plan_snaps_to_an_imports_ends_middles_crossings_and_centres_so_walls_can_be_traced() {
    let mut s = Session::new();
    let circle = pairs(&[(0, "CIRCLE"), (8, "WALLS"), (10, "20"), (20, "3"), (40, "0.5")]);
    let text = plan().replace("0\nENDSEC\n0\nEOF", &(circle + &line("WALLS", (2.0, -1.0), (2.0, 7.0)) + "0\nENDSEC\n0\nEOF"));
    run(&mut s, "import_cad", json!({"path": file("snap", "plan.dxf", &text), "position": "origin", "at": [1000, 1000]}));
    let (one, two) = levels(&s);
    let reach = Reach { point: 100.0, line: 100.0 };
    let at = |s: &Session, level: u64, p: P2| snap(&s.model, level, p, reach, &[], false, &Snaps::default());
    let f = at(&s, one, P2(1030.0, 1020.0));
    assert_eq!((f.kind, f.at), (Kind::Endpoint, P2(1000.0, 1000.0)), "a corner: {f:?}");
    let f = at(&s, one, P2(6020.0, 990.0));
    assert_eq!((f.kind, f.at), (Kind::Midpoint, P2(6000.0, 1000.0)), "{f:?}");
    let f = at(&s, one, P2(3020.0, 7010.0));
    assert_eq!((f.kind, f.at), (Kind::Intersection, P2(3000.0, 7000.0)), "the cross line and the north wall: {f:?}");
    let f = at(&s, one, P2(21_020.0, 4_030.0));
    assert_eq!((f.kind, f.at), (Kind::Centre, P2(21_000.0, 4_000.0)), "{f:?}");
    assert_eq!(at(&s, two, P2(1030.0, 1020.0)).kind, Kind::Free, "Level 2 has no import");
    // FURN, off in its file, isn't snapped to; a layer hidden isn't either.
    assert_eq!(at(&s, one, P2(2030.0, 2010.0)).kind, Kind::Free, "FURN's end");
    let id = run(&mut s, "elements", json!({"category": "cadimport"}))["elements"][0]["id"].clone();
    run(&mut s, "cad_layers", json!({"id": id, "hide": ["WALLS"]}));
    assert_eq!(at(&s, one, P2(1030.0, 1020.0)).kind, Kind::Free);
    run(&mut s, "cad_layers", json!({"id": id, "show": ["WALLS"]}));
    // A wall traced from corner to corner of the drawing lies on its line.
    let (a, b) = (at(&s, one, P2(1020.0, 1010.0)).at, at(&s, one, P2(10_990.0, 1020.0)).at);
    run(&mut s, "wall", json!({"points": [[a.0, a.1], [b.0, b.1]]}));
    let w = &run(&mut s, "elements", json!({"category": "wall"}))["elements"][0];
    assert_eq!((&w["start"], &w["end"]), (&json!([1000.0, 1000.0]), &json!([11000.0, 1000.0])));
}

#[test]
fn a_plan_fits_to_its_imports_as_to_the_rest_it_shows() {
    let mut s = Session::new();
    let id = run(&mut s, "import_cad", json!({"path": file("fit", "plan.dxf", &plan()), "at": [80_000, -40_000]}))["id"].as_u64().unwrap();
    let (one, two) = levels(&s);
    let d = render::plan(&s.model, one, 600, 600);
    assert!(drawn(&d, reddish) > 300, "the plan is fitted to the drawing, far from the origin");
    assert!(d.pick(d.view.px(P2(80_000.0, -43_000.0)), 3.0) == Some(id), "its south wall is on screen");
    // A plan of another level, or one an import isn't in, isn't fitted to it.
    assert!(render::plan_shows(&s.model, two).is_empty());
    let other = run(&mut s, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    let mut t = Session::new();
    let (one, _) = levels(&t);
    let other_t = run(&mut t, "duplicate_view", json!({"view": one}))["id"].as_u64().unwrap();
    run(&mut t, "import_cad", json!({"path": file("fit", "plan.dxf", &plan()), "view": other_t, "at": [80_000, 0]}));
    assert!(render::plan_shows(&t.model, one).is_empty());
    assert!(!render::plan_shows(&t.model, other_t).is_empty());
    assert!(!render::plan_shows(&s.model, other).is_empty());
}

// ---------- Pick Lines: outlines and elements from an import's lines ----------

/// A site drawn in metres, as Revit exports one: a building of four loose lines on A-WALL with a fifth across it (entities 0
/// to 4), its labels inside (an MTEXT of two lines, 5 and 6, and a TEXT, 7), a ground hatch on Toposolid (8), and a road of a
/// line, an arc and a line on A-FLOR going nowhere (9 to 11).
fn site_plan() -> String {
    let walls: String = [((0.0, 0.0), (20.0, 0.0)), ((20.0, 0.0), (20.0, 10.0)), ((20.0, 10.0), (0.0, 10.0)), ((0.0, 10.0), (0.0, 0.0))]
        .iter()
        .map(|(a, b)| line("A-WALL", *a, *b))
        .collect();
    let across = line("A-WALL", (8.0, 0.0), (8.0, 10.0));
    let label = pairs(&[(0, "MTEXT"), (8, "A-TEXT"), (10, "14"), (20, "6"), (40, "0.5"), (71, "1"), (1, "Residential A\\P428.64 m²")]);
    let lobby = pairs(&[(0, "TEXT"), (8, "A-TEXT"), (10, "4"), (20, "5"), (40, "0.25"), (1, "Lobby")]);
    let hatch = pairs(&[
        (0, "HATCH"),
        (8, "Toposolid"),
        (2, "SOLID"),
        (70, "1"),
        (91, "1"),
        (92, "2"),
        (72, "0"),
        (73, "1"),
        (93, "4"),
        (10, "30"),
        (20, "0"),
        (10, "40"),
        (20, "0"),
        (10, "40"),
        (20, "8"),
        (10, "30"),
        (20, "8"),
        (97, "0"),
    ]);
    let road = line("A-FLOR", (0.0, -5.0), (20.0, -5.0))
        + &pairs(&[(0, "ARC"), (8, "A-FLOR"), (10, "20"), (20, "0"), (40, "5"), (50, "270"), (51, "360")])
        + &line("A-FLOR", (25.0, 0.0), (25.0, 3.0));
    let layers = [("A-WALL", "1", false), ("A-TEXT", "7", false), ("Toposolid", "3", false), ("A-FLOR", "8", false)];
    dxf("6", &layers, &(walls + &across + &label + &lobby + &hatch + &road))
}

/// A point of the site plan (metres) in the model (mm): brought in turned 30° about [5000, 2000], its origin there.
fn placed(x: f64, y: f64) -> [f64; 2] {
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let (x, y) = (x * 1000.0, y * 1000.0);
    [5000.0 + x * cos - y * sin, 2000.0 + x * sin + y * cos]
}

fn site_session(test: &str) -> (Session, u64) {
    let mut s = Session::new();
    let path = file(test, "site.dxf", &site_plan());
    let r = run(&mut s, "import_cad", json!({"path": path, "position": "origin", "at": [5000, 2000], "angle": 30}));
    (s, r["id"].as_u64().unwrap())
}

fn has_point(ring: &Value, want: [f64; 2]) -> bool {
    ring.as_array()
        .unwrap()
        .iter()
        .any(|p| (p[0].as_f64().unwrap() - want[0]).abs() < 0.02 && (p[1].as_f64().unwrap() - want[1]).abs() < 0.02)
}

fn by_area(r: &Value) -> Vec<Value> {
    let mut v = r["outlines"].as_array().unwrap().clone();
    v.sort_by(|a, b| b["area"].as_f64().unwrap().total_cmp(&a["area"].as_f64().unwrap()));
    v
}

fn near2(v: &Value, p: [f64; 2]) -> bool {
    (v[0].as_f64().unwrap() - p[0]).abs() < 1e-6 && (v[1].as_f64().unwrap() - p[1]).abs() < 1e-6
}

#[test]
fn an_imports_lines_make_outlines_each_face_and_the_whole_with_the_labels_inside_in_model_coordinates() {
    let (mut s, id) = site_session("outlines");
    let r = run(&mut s, "cad_outlines", json!({"id": id, "layers": ["A-WALL"]}));
    assert_eq!((r["id"].as_u64(), r["cad"].as_str()), (Some(id), Some("site")), "{r}");
    let o = by_area(&r);
    assert_eq!(o.len(), 3, "the whole and its two faces: {r}");
    let areas: Vec<f64> = o.iter().map(|x| x["area"].as_f64().unwrap()).collect();
    assert!(areas.iter().zip([200e6, 120e6, 80e6]).all(|(a, b)| (a - b).abs() < 1.0), "{areas:?}");
    assert_eq!(o[0]["area_m2"], 200.0);
    assert_eq!((o[0]["whole"].as_bool(), o[1]["whole"].as_bool()), (Some(true), None), "{r}");
    for x in &o {
        assert_eq!((x["from"].as_str(), x["layer"].as_str()), (Some("lines"), Some("A-WALL")), "{x}");
    }
    assert_eq!(o[0]["entities"], json!([0, 1, 2, 3, 4]));
    assert_eq!(o[1]["entities"], json!([0, 1, 2, 4]));
    assert_eq!(o[2]["entities"], json!([0, 2, 3, 4]));
    // Placed: turned 30° about [5000, 2000], metres to mm, to the hundredth.
    for corner in [(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (0.0, 10.0)] {
        assert!(has_point(&o[0]["points"], placed(corner.0, corner.1)), "{corner:?} in {}", o[0]["points"]);
    }
    assert!(has_point(&o[2]["points"], placed(8.0, 10.0)) && !has_point(&o[2]["points"], placed(20.0, 0.0)));
    let x = o[0]["points"][1][0].as_f64().unwrap();
    assert_eq!((x * 100.0).round() / 100.0, x, "rounded to the hundredth");
    // The texts inside each: the whole has all, each face its own; each line of an MTEXT is a label.
    assert_eq!(o[0]["labels"], json!(["Residential A", "428.64 m²", "Lobby"]), "{r}");
    assert_eq!(o[1]["labels"], json!(["Residential A", "428.64 m²"]));
    assert_eq!(o[2]["labels"], json!(["Lobby"]));
    assert_eq!(r["open"], json!([]));
    // `min_area` (mm²) leaves the small face out.
    let r = run(&mut s, "cad_outlines", json!({"id": id, "layers": ["A-WALL"], "min_area": 100e6}));
    assert_eq!(by_area(&r).len(), 2, "{r}");
}

#[test]
fn a_hatch_is_an_outline_and_lines_going_nowhere_are_open_chains_and_picks_narrow_by_entity_and_box() {
    let (mut s, id) = site_session("open");
    // Everything it shows: the building's three, the hatch, and the road open.
    let r = run(&mut s, "cad_outlines", json!({"id": id}));
    let o = by_area(&r);
    assert_eq!(o.len(), 4, "{r}");
    let hatch = o.iter().find(|x| x["from"] == "hatch").unwrap();
    assert_eq!((hatch["area_m2"].as_f64(), hatch["layer"].as_str(), &hatch["entities"]), (Some(80.0), Some("Toposolid"), &json!([8])));
    assert!(hatch["labels"].as_array().unwrap().is_empty());
    let open = r["open"].as_array().unwrap();
    assert_eq!(open.len(), 1, "{r}");
    assert_eq!(open[0]["entities"], json!([9, 10, 11]));
    let pts = open[0]["points"].as_array().unwrap();
    let ends = json!([pts.first().unwrap(), pts.last().unwrap()]);
    assert!(has_point(&ends, placed(0.0, -5.0)) && has_point(&ends, placed(25.0, 3.0)), "{ends}");
    // By entity: the four outer lines alone make one outline.
    let r = run(&mut s, "cad_outlines", json!({"id": id, "entities": [0, 1, 2, 3]}));
    assert_eq!(by_area(&r).len(), 1, "{r}");
    assert!((by_area(&r)[0]["area"].as_f64().unwrap() - 200e6).abs() < 1.0);
    // By a box, model mm: only what lies wholly inside it, here the hatch.
    let corners = [placed(29.0, -1.0), placed(41.0, -1.0), placed(41.0, 9.0), placed(29.0, 9.0)];
    let lo = [corners.iter().map(|c| c[0]).fold(f64::MAX, f64::min), corners.iter().map(|c| c[1]).fold(f64::MAX, f64::min)];
    let hi = [corners.iter().map(|c| c[0]).fold(f64::MIN, f64::max), corners.iter().map(|c| c[1]).fold(f64::MIN, f64::max)];
    let r = run(&mut s, "cad_outlines", json!({"id": id, "box": [lo, hi]}));
    let from: Vec<Value> = r["outlines"].as_array().unwrap().iter().map(|x| x["from"].clone()).collect();
    assert_eq!(from, [json!("hatch")], "{r}");
}

#[test]
fn a_property_line_a_floor_a_pad_and_a_toposolid_take_their_boundary_from_the_lines_picked() {
    let (mut s, id) = site_session("boundary");
    let r = run(&mut s, "property_line", json!({"cad": {"id": id, "layers": ["A-WALL"]}}));
    assert_eq!(r["area_m2"], 200.0, "the whole of the building, not its faces: {r}");
    let r = run(&mut s, "floor", json!({"cad": {"id": id, "entities": [0, 1, 2, 3, 4]}, "offset": 100}));
    assert_eq!(r["area_m2"], 200.0, "{r}");
    let f = s.model.elements.get(r["id"].as_u64().unwrap()).unwrap().clone();
    let crate::model::Element::Floor(f) = f else { panic!("{f:?}") };
    let corner = placed(20.0, 10.0);
    assert!(f.boundary.iter().any(|p| (p.0 - corner[0]).abs() < 0.02 && (p.1 - corner[1]).abs() < 0.02), "{:?}", f.boundary);
    assert_eq!(f.level, s.model.guid(s.model.levels()[0].0), "the import's level");
    let r = run(&mut s, "floor", json!({"cad": {"id": id, "entities": [0, 2, 3, 4]}}));
    assert_eq!(r["area_m2"], 80.0, "a face picked by its lines alone, what runs on past it left out: {r}");
    let r = run(&mut s, "building_pad", json!({"cad": {"id": id, "layers": ["Toposolid"]}}));
    assert_eq!(r["area_m2"], 80.0, "{r}");
    let r = run(&mut s, "toposolid", json!({"cad": {"id": id, "layers": ["Toposolid"]}}));
    assert_eq!(r["area_m2"], 80.0, "{r}");
}

#[test]
fn a_pick_that_makes_no_outline_or_several_says_so_and_points_to_cad_outlines() {
    let (mut s, id) = site_session("errors");
    let e = fails(&mut s, "floor", json!({"cad": {"id": id, "layers": ["A-FLOR"]}}));
    assert!(e.contains("no closed outline") && e.contains("cad_outlines"), "{e}");
    let e = fails(&mut s, "property_line", json!({"cad": {"id": id, "entities": [0, 1, 2, 3, 8]}}));
    assert!(e.contains("2 closed outlines") && e.contains("cad_outlines"), "{e}");
    let e = fails(&mut s, "floor", json!({"cad": {"id": id, "layers": ["A-WALL"]}, "boundary": [[0, 0], [1, 0], [1, 1]]}));
    assert!(e.contains("not both"), "{e}");
    let e = fails(&mut s, "wall", json!({"cad": {"id": id}, "points": [[0, 0], [1000, 0]]}));
    assert!(e.contains("not both"), "{e}");
    let e = fails(&mut s, "cad_outlines", json!({"id": id, "entities": [99]}));
    assert!(e.contains("no entity 99") && e.contains("it has 12"), "{e}");
    let e = fails(&mut s, "floor", json!({"cad": {"id": id, "colour": 5}}));
    assert!(e.contains("`colour`"), "{e}");
    let e = fails(&mut s, "floor", json!({"cad": {"id": id, "layers": ["NOPE"]}}));
    assert!(e.contains("no layer `NOPE`"), "{e}");
    let level = s.model.levels()[0].0;
    let e = fails(&mut s, "wall", json!({"cad": {"id": level}}));
    assert!(e.contains("isn't a CAD import"), "{e}");
    let e = fails(&mut s, "cad_outlines", json!({"id": id, "box": [[0, 0]]}));
    assert!(e.contains("`box`"), "{e}");
    let e = fails(&mut s, "cad_outlines", json!({"id": id, "min_area": -1}));
    assert!(e.contains("`min_area`"), "{e}");
    let e = fails(&mut s, "wall", json!({"cad": {"id": id, "layers": ["A-TEXT"]}}));
    assert!(e.contains("no line"), "{e}");
}

#[test]
fn walls_are_drawn_along_each_line_and_arc_picked_as_revits_pick_lines() {
    let (mut s, id) = site_session("walls");
    let r = run(&mut s, "wall", json!({"cad": {"id": id, "layers": ["A-WALL"]}, "height": 3000}));
    assert_eq!(r["ids"].as_array().unwrap().len(), 5, "{r}");
    let walls = run(&mut s, "elements", json!({"category": "wall"}));
    let w = &walls["elements"][0];
    assert!(near2(&w["start"], placed(0.0, 0.0)) && near2(&w["end"], placed(20.0, 0.0)), "{w}");
    // The road's arc is a wall that turns as it does, round the drawn arc's centre.
    let r = run(&mut s, "wall", json!({"cad": {"id": id, "entities": [10]}}));
    let wid = r["ids"][0].as_u64().unwrap();
    let arc = s.model.wall(wid).unwrap().clone();
    assert!(near2(&json!([arc.start.0, arc.start.1]), placed(20.0, -5.0)) && near2(&json!([arc.end.0, arc.end.1]), placed(25.0, 0.0)));
    assert!((arc.arc - 90.0).abs() < 1e-9, "{}", arc.arc);
    let (c, radius, ..) = crate::walls::Axis::of(&arc).arc().unwrap();
    assert!(near2(&json!([c.0, c.1]), placed(20.0, 0.0)) && (radius - 5000.0).abs() < 1e-6, "{c:?} {radius}");
    // A text draws no line: it's counted, and walls go along the rest.
    let r = run(&mut s, "wall", json!({"cad": {"id": id, "entities": [7, 9]}}));
    assert_eq!((r["ids"].as_array().unwrap().len(), r["skipped"].as_u64()), (1, Some(1)), "{r}");
}

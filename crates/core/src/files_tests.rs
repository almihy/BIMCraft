//! Files given to a session as bytes, by a front end with no disk (a browser page, a server with a file store), read by the
//! commands that read files as they read one on disk; and what the commands write kept for the front end to take.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::sample::SAMPLE;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} was taken: {v}"),
        Err(e) => e,
    }
}

fn count(s: &mut Session, category: &str) -> u64 {
    run(s, "elements", json!({"category": category}))["count"].as_u64().unwrap()
}

/// A session that keeps what it writes, as a browser's does.
fn keeping() -> Session {
    let mut s = Session::new();
    s.files.keep_writes = true;
    s
}

/// The sample house as IFC, as the engine writes it.
fn house_ifc() -> Vec<u8> {
    let mut s = keeping();
    s.script(SAMPLE).unwrap();
    run(&mut s, "save", json!({"path": "out/house.ifc"}));
    let written = s.files.take_written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, "out/house.ifc");
    written.into_iter().next().unwrap().1
}

fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A room 10 m by 6 m drawn in metres on WALLS.
fn plan_dxf() -> String {
    let lines: String = [((0, 0), (10, 0)), ((10, 0), (10, 6)), ((10, 6), (0, 6)), ((0, 6), (0, 0))]
        .iter()
        .map(|((ax, ay), (bx, by))| {
            pairs(&[(0, "LINE"), (8, "WALLS"), (10, &ax.to_string()), (20, &ay.to_string()), (11, &bx.to_string()), (21, &by.to_string())])
        })
        .collect();
    let header = pairs(&[(9, "$INSUNITS"), (70, "6")]);
    format!("0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{lines}0\nENDSEC\n0\nEOF\n")
}

#[test]
fn an_ifc_given_as_bytes_opens_as_the_project_and_keeps_no_path_to_save_to() {
    let ifc = house_ifc();
    let mut s = keeping();
    s.files.give("files/doc_1/House.ifc", ifc);
    let r = run(&mut s, "open", json!({"path": "files/doc_1/House.ifc"}));
    assert_eq!(r["format"], "IFC4", "{r}");
    assert!(count(&mut s, "wall") > 0, "the house's walls came in");
    assert!(s.path.is_none(), "a given file is no file on disk to save back to");
}

#[test]
fn a_project_given_as_bytes_opens_and_one_given_with_backslashes_is_the_same_file() {
    let mut house = Session::new();
    house.script(SAMPLE).unwrap();
    let text = house.model.to_json().unwrap();
    let mut s = keeping();
    s.files.give("files\\doc_2\\House.bimcraft", text.into_bytes());
    run(&mut s, "open", json!({"path": "files/doc_2/House.bimcraft"}));
    assert_eq!(count(&mut s, "wall"), count(&mut house, "wall"));
    assert!(s.path.is_none());
}

#[test]
fn a_model_given_as_bytes_is_linked_and_read_again_when_given_again() {
    let mut s = keeping();
    s.files.give("files/doc_3/House.ifc", house_ifc());
    let r = run(&mut s, "link", json!({"path": "files/doc_3/House.ifc"}));
    assert_eq!(r["name"], "House", "{r}");
    assert!(r["elements"].as_u64().unwrap() > 0, "{r}");
    // A model reopened from its text has the link and not what it read: given again, `reload` reads it.
    let text = s.model.to_json().unwrap();
    let (m, _) = crate::model::Model::read(&text).unwrap();
    let mut again = keeping();
    again.reset(m, None);
    let r = run(&mut again, "reload", json!({}));
    assert_eq!(r["failed"].as_array().unwrap().len(), 1, "nothing given yet: {r}");
    again.files.give("files/doc_3/House.ifc", house_ifc());
    let r = run(&mut again, "reload", json!({}));
    assert_eq!(r["reloaded"], json!(["House"]), "{r}");
}

#[test]
fn a_drawing_given_as_bytes_comes_in_and_a_dwg_given_unconverted_says_so() {
    let mut s = keeping();
    s.files.give("files/doc_4/Survey.dxf", plan_dxf().into_bytes());
    let r = run(&mut s, "import_cad", json!({"path": "files/doc_4/Survey.dxf"}));
    assert_eq!((r["cad"].as_str(), r["entities"].as_u64()), (Some("Survey"), Some(4)), "{r}");
    assert_eq!(count(&mut s, "cadimport"), 1);
    // Linked, it keeps where it was read from, for a reload.
    let r = run(&mut s, "import_cad", json!({"path": "files/doc_4/Survey.dxf", "name": "Survey linked", "link": true}));
    assert_eq!(s.model.cads[r["cad"].as_str().unwrap()].path(), Some("files/doc_4/Survey.dxf"));
    s.files.give("files/doc_5/Old.dwg", b"AC1032 not converted".to_vec());
    let e = fails(&mut s, "import_cad", json!({"path": "files/doc_5/Old.dwg"}));
    assert!(e.contains("give it as the DXF it becomes"), "{e}");
}

#[test]
fn a_scan_given_as_bytes_comes_in() {
    let mut s = keeping();
    s.files.give("files/doc_6/Scan.xyz", b"0 0 0\n4 0 0\n4 3 0\n0 3 2.5\n".to_vec());
    let r = run(&mut s, "point_cloud", json!({"path": "files/doc_6/Scan.xyz"}));
    assert_eq!((r["cloud"].as_str(), r["points"].as_u64()), (Some("Scan"), Some(4)), "{r}");
}

#[test]
fn a_picture_given_as_bytes_is_a_materials_texture() {
    let mut s = keeping();
    s.script(SAMPLE).unwrap();
    run(&mut s, "render", json!({"view": "Level 1", "path": "out/plan.png", "width": 64, "height": 64}));
    let (_, png) = s.files.take_written().into_iter().next().unwrap();
    s.files.give("files/doc_7/Brick.png", png);
    run(&mut s, "material", json!({"name": "Brick", "texture": {"kind": "image", "path": "files/doc_7/Brick.png"}}));
    assert_eq!(s.model.images.len(), 1);
}

#[test]
fn what_is_saved_while_the_session_keeps_its_writes_is_taken_and_never_reaches_the_disk() {
    let mut s = keeping();
    s.script(SAMPLE).unwrap();
    let dir = std::env::temp_dir().join(format!("bimcraft-files-{}", std::process::id()));
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    run(&mut s, "save", json!({"path": at("house.glb")}));
    run(&mut s, "save", json!({"path": at("plan.svg")}));
    let r = run(&mut s, "save", json!({"path": at("house.bimcraft")}));
    assert_eq!(r["format"], "bimcraft");
    assert!(s.path.is_none(), "a project written for the front end isn't the session's file");
    let sheets = run(&mut s, "elements", json!({"category": "sheet"}))["count"].as_u64().unwrap();
    if sheets > 0 {
        let r = run(&mut s, "save", json!({"path": at("sheets.pdf")}));
        assert_eq!(r["format"], "PDF");
    }
    let written = s.files.take_written();
    let names: Vec<&str> = written.iter().map(|(n, _)| n.rsplit('/').next().unwrap()).collect();
    assert_eq!(&names[..3], ["house.glb", "plan.svg", "house.bimcraft"]);
    assert!(written[0].1.starts_with(b"glTF"));
    assert!(written[1].1.starts_with(b"<svg") || written[1].1.starts_with(b"<?xml"));
    if sheets > 0 {
        assert!(written[3].1.starts_with(b"%PDF"));
    }
    assert!(!dir.exists(), "nothing was written to disk");
    assert!(s.files.take_written().is_empty(), "taken once");
}

#[test]
fn a_file_neither_given_nor_on_disk_is_refused_by_its_name() {
    let mut s = keeping();
    let e = fails(&mut s, "open", json!({"path": "files/doc_9/Nothing.ifc"}));
    assert!(e.contains("Nothing.ifc"), "{e}");
}

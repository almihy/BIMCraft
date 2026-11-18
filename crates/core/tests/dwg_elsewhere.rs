//! A DWG read elsewhere first, as the browser's engine reads one (`dxf::dwg::read_elsewhere`): the engine decodes only a
//! file read safely there; a project holding a DWG opens while it is read, and opens all the same when it can't be. Its own
//! test binary, since turning reading elsewhere on holds for the whole process.
use bimcraft_core::cad;
use bimcraft_core::dxf::dwg::{self, Check};
use bimcraft_core::engine::Session;
use bimcraft_core::model::{Element, Model};
use serde_json::json;

/// A project holding `bytes` as the DWG of one import, as its file's text, written with reading elsewhere off.
#[cfg(test)]
fn project_holding(name: &str, bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("bimcraft-dwg-elsewhere-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.dwg"));
    let good = dwg::sample::plan().unwrap();
    std::fs::write(&path, &good).unwrap();
    let mut s = Session::new();
    s.execute("import_cad", &json!({"path": path, "at": [0, 0]})).unwrap();
    let text = s.model.to_json().unwrap();
    if bytes == good.as_slice() {
        return text;
    }
    // The same project, its drawing's file swapped for `bytes`.
    let held = |b: &[u8]| bimcraft_core::files::base64(&bimcraft_core::cloud::deflate(b));
    let (was, now) = (held(&good), held(bytes));
    assert!(text.contains(&was));
    text.replace(&was, &now)
}

/// The sample plan with its object records cut short: what the decoder refuses.
#[cfg(test)]
fn hostile() -> Vec<u8> {
    let mut b = dwg::sample::plan().unwrap();
    b.truncate(b.len() / 2);
    b
}

#[test]
fn a_dwg_is_read_here_only_once_read_safely_elsewhere_and_a_project_holding_one_opens_either_way() {
    let good = dwg::sample::plan().unwrap();
    let text = project_holding("good", &good);
    let broken = project_holding("broken", &hostile());
    // Off (the desktop, the command line): a project holding a DWG the decoder refuses still opens, the drawing saying why.
    let (m, _) = Model::read(&broken).unwrap();
    let cad = m.cads.values().next().unwrap();
    assert!(cad.unread().is_some() && cad.dxf().count() == 0, "{:?}", cad.unread());
    m.check().unwrap();
    drop(m);

    dwg::read_elsewhere(true);
    // Opened: the drawing waits, drawing nothing, and is asked for once.
    let (m, _) = Model::read(&text).unwrap();
    let copy = m.clone();
    let cad = m.cads.values().next().unwrap();
    assert_eq!((cad.unread(), cad.dxf().count(), cad.bulk()), (Some(dwg::WAITING), 0, None));
    assert!(m.elements.values().any(|e| matches!(e, Element::CadImport(_))), "the import stays");
    m.check().unwrap();
    let asked = dwg::take_wanted();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].1.as_slice(), good.as_slice());
    assert!(dwg::take_wanted().is_empty(), "asked once");
    assert!(dwg::waiting());
    assert!(!cad::read_checked(), "nothing to read before the answer");
    // Read safely elsewhere: read here, in every copy at once.
    dwg::checked(asked[0].0, Ok(()));
    assert!(!dwg::waiting());
    assert!(cad::read_checked());
    assert_eq!((cad.unread(), cad.dxf().count()), (None, 11));
    assert_eq!(copy.cads.values().next().unwrap().dxf().count(), 11, "the copy shares it");
    assert_eq!(dwg::check(&good), Check::Read);

    // A file the reader stopped on: the project opens, the import draws nothing and says why; importing it again says so.
    let (m, _) = Model::read(&broken).unwrap();
    let asked = dwg::take_wanted();
    assert_eq!(asked.len(), 1);
    dwg::checked(asked[0].0, Err("the reader stopped on it".into()));
    assert!(cad::read_checked());
    let cad = m.cads.values().next().unwrap();
    assert_eq!(cad.unread(), Some("This drawing could not be read: the reader stopped on it"));
    m.check().unwrap();
    let mut s = Session::new();
    s.reset(m, None);
    let id = s.model.elements.iter().find_map(|(id, e)| matches!(e, Element::CadImport(_)).then_some(*id)).unwrap();
    let r = bimcraft_core::json::element_json(&s.model, id);
    assert_eq!(r["unread"], "This drawing could not be read: the reader stopped on it", "{r}");
    // Work goes on.
    s.execute("wall", &json!({"points": [[0, 0], [5000, 0]]})).unwrap();

    // Imported while it is being read: refused for now, then brought in once read.
    let dir = std::env::temp_dir().join(format!("bimcraft-dwg-elsewhere-{}-new", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut other = good.clone();
    other.extend_from_slice(&[0; 16]);
    let path = dir.join("new.dwg");
    std::fs::write(&path, &other).unwrap();
    let e = s.execute("import_cad", &json!({"path": path})).unwrap_err();
    assert!(e.contains(dwg::WAITING), "{e}");
    let asked = dwg::take_wanted();
    assert_eq!(asked.len(), 1);
    dwg::checked(asked[0].0, Ok(()));
    let r = s.execute("import_cad", &json!({"path": path})).unwrap();
    assert_eq!(r["entities"], 11, "{r}");
    dwg::read_elsewhere(false);
}

//! `lines`'s tests that drive the Session (moved out of `lines.rs`).

use crate::language::Language;
use crate::pen::Pen;

use crate::engine::Session;
use crate::geom::P2;
use crate::library::Kind;
use crate::lines::*;
use crate::model::{Element, Id, Model};
use crate::prim::{Prim, View};
use crate::render::Drawing;
use serde_json::json;

/// The lines `id` draws in `d`: their width, colour and dash.
fn lines_of(d: &Drawing, id: Id) -> Vec<(f32, [u8; 3], Option<Vec<f32>>)> {
    (d.prims.iter().zip(&d.owners))
        .filter_map(|(p, o)| match p {
            Prim::Line { width, color, dash, .. } if *o == Some(id) => Some((*width, [color.0, color.1, color.2], dash.clone())),
            _ => None,
        })
        .collect()
}

fn plan(m: &Model, level: Id, scale: f64) -> Drawing {
    crate::render::draw(m, level, View { scale, cx: 500.0, cy: 0.0, width: 800, height: 600 })
}

#[test]
fn line_patterns_and_styles_are_the_projects_named_in_its_language_saved_renamed_and_kept_while_used() {
    let mut s = Session::new();
    // A project starts with its own, and asking changes nothing.
    let r = s.execute("line_style", &json!({})).unwrap();
    assert_eq!(r["styles"]["Thin Lines"], json!({"pen": 1, "color": [0, 0, 0]}));
    assert_eq!(r["styles"]["Hidden Lines"]["pattern"], "Hidden");
    let r = s.execute("line_pattern", &json!({})).unwrap();
    assert_eq!(r["patterns"]["Dash"], json!([{"dash": 3.0}, {"space": 1.5}]));
    assert!(s.history().is_empty());
    assert_eq!(Model::new_in("p", Language::Portuguese).line_styles.keys().filter(|n| n.starts_with("Linhas")).count(), 6);
    // Made, each an entry of the library with an id and a GUID.
    s.execute("line_pattern", &json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}, "dot", {"space": 1}]})).unwrap();
    s.execute("line_style", &json!({"name": "Fence Line", "pen": 3, "color": "#c80000", "pattern": "Fence"})).unwrap();
    assert_eq!(s.history(), ["Line Patterns", "Line Styles"]);
    assert!(s.model.library.id(&Kind::LinePattern, "Fence").is_some());
    assert!(s.model.library.id(&Kind::LineStyle, "Fence Line").is_some());
    let line = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]], "style": "Fence Line"})).unwrap()["id"].as_u64().unwrap();
    // A style changed keeps what it isn't given.
    s.execute("line_style", &json!({"name": "Fence Line", "pen": 5})).unwrap();
    assert_eq!(s.model.line_styles["Fence Line"], LineStyle { pen: Pen(5), color: [200, 0, 0], pattern: Some("Fence".into()) });
    // Renamed, what names them follows, and the file holds GUIDs, so it reads back the same.
    s.execute("line_pattern", &json!({"name": "Fence", "rename": "Boundary"})).unwrap();
    s.execute("line_style", &json!({"name": "Fence Line", "rename": "Boundary Line"})).unwrap();
    assert_eq!(s.model.line_styles["Boundary Line"].pattern.as_deref(), Some("Boundary"));
    let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
    assert_eq!(l.style, "Boundary Line");
    let file = s.model.to_json().unwrap();
    assert!(!file.contains("\"Boundary Line\"\n") && !file.contains("\"style\": \"Boundary Line\""), "a file names a style by GUID");
    let (m, _) = Model::read(&file).unwrap();
    assert_eq!((&m.line_styles, &m.line_patterns, &m.elements), (&s.model.line_styles, &s.model.line_patterns, &s.model.elements));
    // In use, kept; once not, deleted.
    let e = s.execute("line_pattern", &json!({"name": "Boundary", "delete": true})).unwrap_err();
    assert!(e.contains("line style `Boundary Line`"), "{e}");
    let e = s.execute("line_style", &json!({"name": "Boundary Line", "delete": true})).unwrap_err();
    assert!(e.contains(&format!("detail line {line}")), "{e}");
    s.execute("delete", &json!({"ids": [line]})).unwrap();
    s.execute("line_style", &json!({"name": "Boundary Line", "delete": true})).unwrap();
    s.execute("line_pattern", &json!({"name": "Boundary", "delete": true})).unwrap();
    assert!(s.model.library.id(&Kind::LinePattern, "Boundary").is_none());
    // What isn't one is refused, saying why, and changes nothing.
    let before = s.model.clone();
    for (cmd, p, says) in [
        ("line_pattern", json!({"name": "Solid", "segments": [{"dash": 1}, {"space": 1}]}), "Solid"),
        ("line_pattern", json!({"name": "X", "segments": [{"dash": 0}, {"space": 1}]}), "0.05 to 100"),
        ("line_pattern", json!({"name": "X", "segments": [{"dash": 1e300}, {"space": 1}]}), "0.05 to 100"),
        ("line_pattern", json!({"name": "X", "segments": [{"dash": 1}, {"gap": 1}]}), "segments"),
        ("line_pattern", json!({"name": "X"}), "no line pattern `X`"),
        ("line_pattern", json!({"name": "Dash", "rename": "Dot"}), "already"),
        ("line_style", json!({"name": "X", "pen": 2, "pattern": "Nope"}), "no line pattern `Nope`"),
        ("line_style", json!({"name": "X", "pen": 17}), "0 (a hairline) to 16"),
        ("line_style", json!({"name": "Thin Lines", "color": "red"}), "color"),
        ("line_style", json!({"name": "X"}), "`pen`"),
        ("line_style", json!({"name": "Thin Lines", "colour": [1, 2, 3]}), "colour"),
        ("detail_line", json!({"points": [[0, 0], [1000, 0]], "style": "Nope"}), "no line style `Nope`"),
        ("detail_line", json!({"points": [[0, 0]]}), "two points"),
        ("detail_line", json!({"points": [[0, 0], [0, 0]]}), "apart"),
        ("detail_line", json!({"points": [[0, 0], [5, 0]], "closed": true}), "three points"),
        ("detail_line", json!({"points": [[0, 0], [5, 0]], "view": "Nowhere"}), "Nowhere"),
    ] {
        let e = s.execute(cmd, &p).unwrap_err();
        assert!(e.contains(says), "{cmd} {p}: {e}");
    }
    assert_eq!(*s.model, *before);
}

#[test]
fn a_detail_line_draws_in_its_styles_pen_colour_and_pattern_in_its_own_view_only_at_any_zoom() {
    let mut s = Session::new();
    s.execute("line_pattern", &json!({"name": "Fence", "segments": [{"dash": 4}, {"space": 1}, "dot", {"space": 1}]})).unwrap();
    s.execute("line_style", &json!({"name": "Fence Line", "pen": 7, "color": [200, 0, 0], "pattern": "Fence"})).unwrap();
    let line = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0], [1000, 500]], "style": "Fence Line"})).unwrap()["id"]
        .as_u64()
        .unwrap();
    let (ground, upper) = (100000, 100001);
    for zoom in [0.2, 0.8] {
        let d = plan(&s.model, ground, zoom);
        let drawn = lines_of(&d, line);
        assert_eq!(drawn.len(), 1, "{drawn:?}");
        let (width, color, dash) = &drawn[0];
        assert!((width - d.width(Pen(7))).abs() < 1e-4);
        assert_eq!(*color, [200, 0, 0]);
        // Dashes as long on paper as the pattern says, as the pen's width is: 4 mm of dash, 1 mm of space, a dot.
        let dash = dash.as_ref().expect("dashed");
        // Pixels to a mm of the paper: the pen's width over its width on paper at the plan's 1 : 100.
        let mm = width / s.model.line_weights().at(100.0).mm(Pen(7), false) as f32;
        assert!((dash[0] / mm - 4.0).abs() < 1e-3 && (dash[1] / mm - 1.0).abs() < 1e-3 && dash[2] < dash[1] / 10.0, "{dash:?}");
    }
    // Not in another level's plan.
    assert!(lines_of(&plan(&s.model, upper, 0.2), line).is_empty());
    // So small it can't be seen, a pattern draws solid.
    assert_eq!(lines_of(&plan(&s.model, ground, 0.0005), line)[0].2, None);
    // In a section, along and up; on a sheet, in paper mm.
    let section = s.execute("section", &json!({"start": [-1000, 2000], "end": [3000, 2000], "name": "S"})).unwrap()["id"].as_u64().unwrap();
    s.execute("sheet", &json!({"number": "A1", "name": "Details"})).unwrap();
    let cut = s.execute("detail_line", &json!({"view": "S", "points": [[0, 0], [500, 500]]})).unwrap()["id"].as_u64().unwrap();
    let paper = s.execute("detail_line", &json!({"view": "A1", "points": [[20, 20], [80, 20]], "style": "Wide Lines"})).unwrap()["id"]
        .as_u64()
        .unwrap();
    let d = crate::render::section(&s.model, section, crate::render::section_fit(&s.model, section, 800, 600));
    assert_eq!(lines_of(&d, cut).len(), 1);
    assert!(lines_of(&d, line).is_empty());
    let sheet =
        crate::sheet::draw(&s.model, s.model.elements.iter().find(|(_, e)| matches!(e, Element::Sheet(_))).map(|(i, _)| *i).unwrap())
            .unwrap();
    assert_eq!(lines_of(&sheet, paper).len(), 1);
    assert_eq!(lines_of(&sheet, paper)[0].1, [0, 0, 0]);
    // It goes with its view.
    s.execute("delete", &json!({"ids": [section]})).unwrap();
    assert!(s.model.elements.get(cut).is_none());
}

#[test]
fn a_detail_line_moves_turns_and_copies_in_its_view() {
    let mut s = Session::new();
    let line = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]]})).unwrap()["id"].as_u64().unwrap();
    let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
    assert_eq!(l.style, THIN);
    s.execute("move", &json!({"ids": [line], "by": [100, 200]})).unwrap();
    s.execute("rotate", &json!({"ids": [line], "about": [100, 200], "angle": 90})).unwrap();
    let Some(Element::DetailLine(l)) = s.model.elements.get(line) else { panic!("no line") };
    assert!(l.curve.start().dist(P2(100.0, 200.0)) < 1e-6 && l.curve.end().dist(P2(100.0, 1200.0)) < 1e-6, "{:?}", l.curve);
    let r = s.execute("copy", &json!({"ids": [line], "by": [0, 3000]})).unwrap();
    assert_eq!(r["ids"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_detail_line_with_no_style_is_drawn_in_thin_lines_whatever_the_language_of_the_project_or_its_file() {
    let thin = |s: &Session, id: Id| match s.model.elements.get(id) {
        Some(Element::DetailLine(l)) => l.style.clone(),
        e => panic!("{e:?}"),
    };
    for (code, name) in [("en", "Thin Lines"), ("pt", "Linhas finas"), ("es", "Líneas finas")] {
        let mut s = Session::new();
        s.execute("new", &json!({"language": code})).unwrap();
        let id = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]]})).unwrap()["id"].as_u64().unwrap();
        assert_eq!(thin(&s, id), name, "{code}");
        let region = s
            .execute("filled_region", &json!({"sketch": [[[0, 0], [1000, 0]], [[1000, 0], [1000, 1000]], [[1000, 1000], [0, 0]]]}))
            .unwrap();
        let Some(Element::FilledRegion(r)) = s.model.elements.get(region["id"].as_u64().unwrap()) else { panic!("a region") };
        assert_eq!(r.line_style.as_deref(), Some(name), "{code}");
    }
    // A Portuguese file from before line styles holds none: it reads with the English ones, which are named in Portuguese.
    let file = Model::new_in("p", Language::Portuguese).to_json().unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&file).unwrap();
    for key in ["line_styles", "line_patterns"] {
        assert!(v.as_object_mut().and_then(|o| o.remove(key)).is_some(), "{key}");
    }
    let (m, left_out) = Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.line_styles, styles(Language::Portuguese));
    assert_eq!(m.check(), Ok(()));
    let mut s = Session::new();
    s.reset(m, None);
    let id = s.execute("detail_line", &json!({"points": [[0, 0], [1000, 0]]})).unwrap()["id"].as_u64().unwrap();
    assert_eq!(thin(&s, id), "Linhas finas");
    // Whatever the names, the style that is a thin black solid line is the default; else the first.
    let mut renamed = default_styles();
    let first = renamed.remove(THIN).unwrap();
    renamed.insert("Zeta".into(), first);
    assert_eq!(default_style(&renamed, Language::Portuguese).map(String::as_str), Some("Zeta"));
    renamed.remove("Zeta");
    assert_eq!(default_style(&renamed, Language::English).map(String::as_str), renamed.keys().next().map(String::as_str));
}

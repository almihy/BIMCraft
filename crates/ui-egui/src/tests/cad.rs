//! CAD drawings in the window, driven as a user drives them: Insert > Import CAD and Link CAD bring a DXF in through their
//! box, the Properties palette shows one and changes its colours, and Visibility/Graphics' Imported Categories hides the
//! category or a drawing's layers in a view.

use std::path::PathBuf;

use bimcraft_core::cad::Colors;
use bimcraft_core::render::Rgb;

use super::*;

/// Pairs as a DXF writes them: a code line, then its value.
fn pairs(list: &[(i32, &str)]) -> String {
    list.iter().map(|(c, v)| format!("{c}\n{v}\n")).collect()
}

/// A plan of a room 10 m by 6 m in metres: its walls on WALLS (red), a table on FURN (off in its file).
fn drawing(test: &str, name: &str) -> PathBuf {
    let line = |layer: &str, a: (f64, f64), b: (f64, f64)| {
        let (ax, ay, bx, by) = (a.0.to_string(), a.1.to_string(), b.0.to_string(), b.1.to_string());
        pairs(&[(0, "LINE"), (8, layer), (10, &ax), (20, &ay), (11, &bx), (21, &by)])
    };
    let walls: String = [((0.0, 0.0), (10.0, 0.0)), ((10.0, 0.0), (10.0, 6.0)), ((10.0, 6.0), (0.0, 6.0)), ((0.0, 6.0), (0.0, 0.0))]
        .iter()
        .map(|(a, b)| line("WALLS", *a, *b))
        .collect();
    let entities = walls + &line("FURN", (1.0, 1.0), (2.0, 1.0));
    let header = pairs(&[(9, "$INSUNITS"), (70, "6")]);
    let mut table = pairs(&[(0, "TABLE"), (2, "LAYER")]);
    for (layer, color) in [("WALLS", "1"), ("FURN", "-3")] {
        table += &pairs(&[(0, "LAYER"), (2, layer), (70, "0"), (62, color), (6, "Continuous")]);
    }
    table += &pairs(&[(0, "ENDTAB")]);
    let text = format!(
        "0\nSECTION\n2\nHEADER\n{header}0\nENDSEC\n0\nSECTION\n2\nTABLES\n{table}0\nENDSEC\n0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n"
    );
    let dir = std::env::temp_dir().join(format!("bimcraft-ui-cad-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn imports(h: &Harness<BimApp>) -> Vec<Id> {
    let m = &h.state().session.model;
    m.elements.iter().filter(|(_, e)| matches!(e, Element::CadImport(_))).map(|(id, _)| *id).collect()
}

fn import(h: &Harness<BimApp>, id: Id) -> bimcraft_core::model::CadImport {
    let Some(Element::CadImport(ci)) = h.state().session.model.elements.get(id) else { panic!("{id} isn't an import") };
    ci.clone()
}

#[test]
fn import_cad_brings_the_file_picked_in_through_its_box_and_selects_it() {
    let mut h = app(BOX);
    press(&mut h, "Insert");
    assert!(h.query_by_label("Link CAD").is_some(), "Link CAD, beside Link Model");
    // The system's dialog gives the file; a test hands it over as it would.
    h.state_mut().import_cad_path(drawing("import", "ground.dxf"), false);
    h.run();
    assert!(h.query_by_label("ground.dxf").is_some());
    assert_eq!(combo_value(&h, "Import units"), "Auto-Detect");
    type_into(&mut h, "Name", "ground floor");
    choose(&mut h, "Colors", "Black and White");
    choose(&mut h, "Layers", "All");
    pick(&mut h, Role::RadioButton, "Auto - Origin to Origin");
    pick(&mut h, Role::CheckBox, "Current view only");
    press(&mut h, "OK");
    let placed = imports(&h);
    assert_eq!(placed.len(), 1);
    let ci = import(&h, placed[0]);
    assert_eq!((ci.cad.as_str(), ci.colors, ci.hidden.is_empty()), ("ground floor", Colors::Black, true), "all its layers");
    assert!(ci.view.is_some(), "in this plan only");
    let m = &h.state().session.model;
    assert!(!m.cads["ground floor"].path().is_some(), "imported, not linked");
    // Origin to Origin: its corner on the project's origin, its metres read as such.
    let [lo, hi] = m.cad_box(placed[0]).unwrap();
    assert!(lo.0.abs() < 1.0 && lo.1.abs() < 1.0 && (hi.0 - 10_000.0).abs() < 1.0, "{lo:?} {hi:?}");
    assert_eq!(h.state().selection, placed, "what was brought in is selected, as Revit does");
    assert!(h.query_by_label("Import units").is_none(), "the box closed");
}

#[test]
fn link_cad_remembers_the_file_and_center_to_center_puts_its_middle_on_the_origin() {
    let mut h = app(BOX);
    h.state_mut().import_cad_path(drawing("link", "site.dxf"), true);
    h.run();
    assert!(h.query_by_label("Link CAD").is_some(), "the box says what it does");
    press(&mut h, "OK");
    let placed = imports(&h);
    let m = &h.state().session.model;
    assert!(m.cads["site"].path().is_some());
    let ci = import(&h, placed[0]);
    assert!(ci.view.is_none() && ci.hidden.contains("FURN"), "every plan of its level; the layers its file has off are hidden");
    let [lo, hi] = m.cad_box(placed[0]).unwrap();
    assert!((lo.0 + hi.0).abs() < 1.0 && (lo.1 + hi.1).abs() < 1.0, "{lo:?} {hi:?}");
}

#[test]
fn a_drawing_that_cant_come_in_says_why_in_its_box() {
    let mut h = app(BOX);
    let dir = drawing("dwg", "plan.dxf");
    let dwg = dir.with_file_name("plan.dwg");
    std::fs::write(&dwg, "AC1032 not a dxf").unwrap();
    h.state_mut().import_cad_path(dwg, false);
    h.run();
    press(&mut h, "OK");
    assert!(imports(&h).is_empty());
    assert!(h.query_by_label_contains("save it as DXF").is_some(), "the reason shows in the box, which stays open");
    press(&mut h, "Cancel");
    assert!(h.query_by_label("plan.dwg").is_none());
}

#[test]
fn properties_show_an_import_and_change_its_colours() {
    let path = drawing("properties", "plan.dxf");
    let mut h = app(&format!("import_cad {}", json!({"path": path})));
    let id = imports(&h)[0];
    h.state_mut().selection = vec![id];
    h.run();
    for label in ["Drawing", "Entities", "Position", "Linked File"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    assert_eq!(combo_value(&h, "Colors"), "Preserve");
    choose(&mut h, "Colors", "Invert");
    assert_eq!(import(&h, id).colors, Colors::Invert);
}

#[test]
fn imported_categories_hides_a_drawings_layers_or_the_category_in_the_view() {
    let path = drawing("vg", "plan.dxf");
    let mut h = app(&format!("{BOX}\nimport_cad {}", json!({"path": path, "layers": "all"})));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    assert!(h.query_by_label("Model Categories").is_some(), "the tabs stay");
    // Its layers, under the drawing's row, unfolded as Revit's: WALLS cleared hides it in this view.
    press(&mut h, "plan: Unfold");
    pick(&mut h, Role::CheckBox, "WALLS");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert_eq!(g.layers.get("plan").map(|l| l.iter().cloned().collect::<Vec<_>>()), Some(vec!["WALLS".to_string()]));
    // Shown again, and the category hidden as a whole.
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    press(&mut h, "plan: Unfold");
    pick(&mut h, Role::CheckBox, "WALLS");
    pick(&mut h, Role::CheckBox, "Show imported categories in this view");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert!(g.layers.is_empty(), "{:?}", g.layers);
    assert!(g.categories.get("cadimport").is_some_and(|o| o.hidden), "{g:?}");
    let look = bimcraft_core::graphics::Look::of(&h.state().session.model, 100000);
    assert!(imports(&h).iter().all(|id| !look.shows(*id)), "every import is hidden");
}

/// Whether the check box named `label` is ticked.
fn ticked(h: &Harness<BimApp>, label: &str) -> bool {
    use egui_kittest::kittest::NodeT as _;
    h.get_by_role_and_label(Role::CheckBox, label).accesskit_node().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn imported_categories_is_one_table_of_the_imports_and_imports_in_families_as_revits() {
    let path = drawing("vg-table", "plan.dxf");
    let mut h = app(&format!("{BOX}\nimport_cad {}", json!({"path": path, "layers": "all"})));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    // The box over the table, ticked; the import and Imports in Families as rows; no CAD Imports row any more.
    assert!(ticked(&h, "Show imported categories in this view"));
    assert!(ticked(&h, "plan") && ticked(&h, "Imports in Families"));
    assert!(h.query_by_label("CAD Imports").is_none(), "the old row is gone");
    // Lines and Halftone on the import's row and on Imports in Families; Patterns can't be overridden, as Revit greys it out.
    for label in ["plan: Projection Lines", "plan: Halftone", "Imports in Families: Projection Lines", "Imports in Families: Halftone"] {
        assert!(h.query_by_label(label).is_some(), "{label}");
    }
    for label in ["plan: Projection Patterns", "Imports in Families: Projection Patterns"] {
        assert!(h.query_by_label(label).is_none(), "{label} is shown hatched");
    }
    // Expand All unfolds the import: its layers have Lines and no Halftone, which is the whole file's.
    press(&mut h, "Expand All");
    assert!(h.query_by_label("plan: Fold").is_some(), "unfolded");
    assert!(h.query_by_label("WALLS: Projection Lines").is_some() && h.query_by_label("WALLS: Projection Patterns").is_none());
    assert!(h.query_by_label("WALLS: Halftone").is_none(), "a layer has no halftone of its own");
    // A note and Object Styles… under the buttons.
    assert!(h.query_by_label("Object Styles…").is_some());
}

#[test]
fn all_none_and_invert_reach_every_import_its_layers_folded_or_not_and_imports_in_families() {
    let path = drawing("vg-all", "plan.dxf");
    let mut h = app(&format!("{BOX}\nimport_cad {}", json!({"path": path, "layers": "all"})));
    key(&mut h, Key::Escape);
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    // Folded: None clears the import, both its layers and Imports in Families.
    press(&mut h, "None");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    let hidden: Vec<String> = g.layers.get("plan").map(|l| l.iter().cloned().collect()).unwrap_or_default();
    assert_eq!(hidden, ["FURN", "WALLS"], "{g:?}");
    assert!(g.imports.get("plan").is_some_and(|l| l.all.hidden), "{g:?}");
    assert!(g.categories.get("familyimport").is_some_and(|o| o.hidden), "{g:?}");
    assert!(!g.categories.get("cadimport").is_some_and(|o| o.hidden), "the box over the table isn't a row");
    // Unfolded, one layer shown again, then Invert: that layer hidden, the rest shown.
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    press(&mut h, "plan: Unfold");
    pick(&mut h, Role::CheckBox, "WALLS");
    press(&mut h, "Invert");
    assert!(!ticked(&h, "WALLS") && ticked(&h, "FURN") && ticked(&h, "plan") && ticked(&h, "Imports in Families"));
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    let hidden: Vec<String> = g.layers.get("plan").map(|l| l.iter().cloned().collect()).unwrap_or_default();
    assert_eq!(hidden, ["WALLS"], "{g:?}");
    assert!(g.imports.is_empty() && !g.categories.contains_key("familyimport"), "{g:?}");
    // All shows every one.
    shortcut(&mut h, "VV");
    press(&mut h, "Imported Categories");
    press(&mut h, "All");
    press(&mut h, "OK");
    let g = plan_graphics(&h, 100000);
    assert!(g.layers.is_empty() && g.imports.is_empty() && g.categories.is_empty(), "{g:?}");
}

#[test]
fn a_right_click_reloads_a_linked_drawing_from_its_file() {
    let path = drawing("reload", "site.dxf");
    let mut h = app(&format!("import_cad {}", json!({"path": path, "link": true, "layers": "all"})));
    let id = imports(&h)[0];
    let count = |h: &Harness<BimApp>| h.state().session.model.cads["site"].dxf().entities.len();
    assert_eq!(count(&h), 5);
    // The file changes: a line more.
    let text = std::fs::read_to_string(&path).unwrap();
    let more = pairs(&[(0, "LINE"), (8, "WALLS"), (10, "5"), (20, "0"), (11, "5"), (21, "6")]);
    std::fs::write(&path, text.replacen("0\nENDSEC\n0\nEOF", &format!("{more}0\nENDSEC\n0\nEOF"), 1)).unwrap();
    // Not offered for what isn't a linked drawing; offered for one.
    let at = h.state().screen(P2(0.0, 0.0));
    assert!(offered(&mut h, at, &["Reload CAD"]).is_empty());
    key(&mut h, Key::Escape);
    h.state_mut().selection = vec![id];
    h.run();
    assert_eq!(offered(&mut h, at, &["Reload CAD"]), ["Reload CAD"]);
    choose_item(&mut h, "Reload CAD");
    assert_eq!(count(&h), 6, "read again");
}

#[test]
fn a_dwg_comes_in_through_the_box_with_the_reader_the_app_is_given_and_its_notes_on_the_status_bar() {
    let mut h = app(BOX);
    // The desktop hands its session `bimcraft-dwg`'s reader; a test hands it one that gives a DXF's text.
    let dxf = std::fs::read(drawing("dwg-reader", "plan.dxf")).unwrap();
    h.state_mut().session.dwg = Some(std::sync::Arc::new(move |_: &std::path::Path| {
        Ok((dxf.clone(), vec!["3 of the DWG's object records couldn't be read and were left out".into()]))
    }));
    let dwg = drawing("dwg-reader", "plan.dxf").with_file_name("site.dwg");
    std::fs::write(&dwg, "AC1032").unwrap();
    h.state_mut().import_cad_path(dwg, false);
    h.run();
    press(&mut h, "OK");
    let placed = imports(&h);
    assert_eq!(placed.len(), 1);
    assert_eq!(h.state().session.model.cads["site"].source(), "site.dwg");
    let status = &h.state().status;
    assert!(!status.error && status.text.contains("3 of the DWG's object records"), "{status:?}");
}

#[test]
fn a_dense_drawings_cells_are_lightened_on_a_dark_canvas_as_its_lines_are() {
    // A pixel of the view's ink and a red one, as a dense drawing zoomed out draws them in its picture of cells.
    let ink = bimcraft_core::render::INK;
    let rgba = [ink.0, ink.1, ink.2, 200, 200, 30, 30, 255];
    let line = |c: Rgb| {
        let c = crate::canvas::color(c);
        [c.r(), c.g(), c.b()]
    };
    crate::style::set_canvas(true, None);
    let seen = crate::canvas::cells_seen(&rgba, bimcraft_core::render::Part::Cells).into_owned();
    // Each pixel as a line of its colour shows on the dark canvas, as opaque as it was.
    assert_eq!(seen[..3], line(ink));
    assert_eq!(seen[4..7], line(Rgb(200, 30, 30)));
    assert_eq!((seen[3], seen[7]), (200, 255));
    assert!(seen[0] > 150, "the ink reads light on the dark canvas: {seen:?}");
    // Another picture (a scan, a page's raster) is left as it is; and on a light canvas, the cells too.
    assert_eq!(*crate::canvas::cells_seen(&rgba, bimcraft_core::render::Part::Picture), rgba);
    crate::style::set_canvas(false, None);
    assert_eq!(*crate::canvas::cells_seen(&rgba, bimcraft_core::render::Part::Cells), rgba);
}

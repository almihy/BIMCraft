//! A view written as DXF (`save` and `render` to a .dxf): in the model's own coordinates and the unit asked for, each element on
//! the layer its category names, and read back by the engine's own DXF reader.

use serde_json::{Value, json};

use crate::dxf::{self, Dxf, Shape};
use crate::engine::Session;
use crate::sample::SAMPLE;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A session that keeps what it writes, as a browser's or a server's does.
fn keeping() -> Session {
    let mut s = Session::new();
    s.files.keep_writes = true;
    s
}

/// What `save` to `out/plan.dxf` with `p` wrote, read back, and what it said.
fn saved(s: &mut Session, mut p: Value) -> (Dxf, Value, String) {
    p["path"] = json!("out/plan.dxf");
    let said = run(s, "save", p);
    let written = s.files.take_written();
    assert_eq!(written.len(), 1);
    assert_eq!(written[0].0, "out/plan.dxf");
    let text = String::from_utf8(written[0].1.clone()).expect("a DXF is text");
    (dxf::read_text(&text).unwrap_or_else(|e| panic!("the DXF does not read: {e}")), said, text)
}

/// A 5 m wall from the origin east, in a session that keeps its writes.
fn one_wall() -> Session {
    let mut s = keeping();
    run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]]}));
    s
}

/// The box round what `layer` holds.
fn bounds(d: &Dxf, layer: &str) -> [[f64; 2]; 2] {
    let boxes = d.entities.iter().filter(|e| e.layer == layer).map(|e| e.shape.bounds());
    let [lo, hi] =
        dxf::union(boxes).unwrap_or_else(|| panic!("nothing on {layer}: {:?}", d.layers.iter().map(|l| &l.name).collect::<Vec<_>>()));
    [[lo.0, lo.1], [hi.0, hi.1]]
}

fn within(got: [[f64; 2]; 2], want: [[f64; 2]; 2], tolerance: f64) -> bool {
    (0..2).all(|i| (0..2).all(|k| (got[i][k] - want[i][k]).abs() <= tolerance))
}

#[test]
fn a_walls_lines_are_on_the_wall_layer_in_the_models_millimetres() {
    let mut s = one_wall();
    let (d, said, _) = saved(&mut s, json!({}));
    assert_eq!((d.unit.as_str(), d.scale, d.version.as_str()), ("mm", Some(1.0), "AC1032"));
    assert_eq!((said["format"].as_str(), said["units"].as_str(), said["coordinates"].as_str()), (Some("DXF"), Some("mm"), Some("project")));
    // The wall's centre line runs 0 to 5000 east, its faces half its thickness either side.
    let [lo, hi] = bounds(&d, "A-WALL");
    assert!(lo[0].abs() <= 1.0 && (hi[0] - 5000.0).abs() <= 1.0, "{lo:?} {hi:?}");
    assert!(lo[1] < -50.0 && hi[1] > 50.0 && lo[1] > -400.0 && hi[1] < 400.0, "a wall's thickness: {lo:?} {hi:?}");
    // Its cut is poché: a solid hatch, as well as lines.
    assert!(d.entities.iter().any(|e| e.layer == "A-WALL" && matches!(e.shape, Shape::Fill { solid: true, .. })), "poché");
    assert!(d.entities.iter().any(|e| e.layer == "A-WALL" && matches!(e.shape, Shape::Line(..) | Shape::Polyline { .. })), "outline");
    assert!(said["layers"].as_array().unwrap().iter().any(|l| l == "A-WALL"), "{said}");
    assert!(said["entities"].as_u64().unwrap() >= 2);
}

#[test]
fn the_unit_asked_for_scales_the_numbers_and_the_header_says_it() {
    let mut s = one_wall();
    let (d, said, _) = saved(&mut s, json!({"units": "m"}));
    assert_eq!((d.unit.as_str(), d.scale, said["units"].as_str()), ("m", Some(1000.0), Some("m")));
    let [lo, hi] = bounds(&d, "A-WALL");
    assert!(lo[0].abs() <= 0.001 && (hi[0] - 5.0).abs() <= 0.001, "{lo:?} {hi:?}");
    let (d, _, _) = saved(&mut s, json!({"units": "ft"}));
    assert_eq!((d.unit.as_str(), d.scale), ("ft", Some(304.8)));
    let [_, hi] = bounds(&d, "A-WALL");
    assert!((hi[0] - 5000.0 / 304.8).abs() <= 0.001, "{hi:?}");
    let e = s.execute("save", &json!({"path": "out/x.dxf", "units": "furlongs"})).unwrap_err();
    assert!(e.contains("furlongs") && e.contains("mm"), "{e}");
}

#[test]
fn survey_coordinates_add_the_survey_points_east_and_north_and_shared_ones_turn_them_to_true_north() {
    let mut s = one_wall();
    // The project's origin is at east 520 000 m, north 7 100 000 m on the survey.
    run(&mut s, "base_point", json!({"which": "survey", "east": 520_000_000_i64, "north": 7_100_000_000_i64}));
    let (d, said, _) = saved(&mut s, json!({"units": "m", "coordinates": "survey"}));
    assert_eq!(said["coordinates"], "survey");
    let [lo, hi] = bounds(&d, "A-WALL");
    assert!(within([[lo[0], 0.0], [hi[0], 0.0]], [[520_000.0, 0.0], [520_005.0, 0.0]], 0.001), "{lo:?} {hi:?}");
    assert!(
        lo[1] > 7_099_999.7 && lo[1] < 7_099_999.95 && hi[1] > 7_100_000.05 && hi[1] < 7_100_000.3,
        "its thickness on the north axis: {lo:?} {hi:?}"
    );
    // Project north 90° from true north: the wall that runs east in the project runs north on the survey.
    run(&mut s, "base_point", json!({"which": "project", "angle": 90}));
    let (d, _, _) = saved(&mut s, json!({"units": "m", "coordinates": "shared"}));
    let [lo, hi] = bounds(&d, "A-WALL");
    assert!(within([[0.0, lo[1]], [0.0, hi[1]]], [[0.0, 7_100_000.0], [0.0, 7_100_005.0]], 0.001), "{lo:?} {hi:?}");
    assert!(
        lo[0] > 519_999.7 && lo[0] < 519_999.95 && hi[0] > 520_000.05 && hi[0] < 520_000.3,
        "its thickness on the east axis: {lo:?} {hi:?}"
    );
    // The project's own, as they were.
    let (d, _, _) = saved(&mut s, json!({"units": "m"}));
    assert!(bounds(&d, "A-WALL")[1][0] < 6.0);
}

#[test]
fn a_house_is_on_the_layers_an_office_knows_each_dashed_line_has_its_own_linetype_and_lettering_is_lines() {
    let mut s = keeping();
    s.script(SAMPLE).unwrap();
    let (d, said, text) = saved(&mut s, json!({"units": "m"}));
    let layers: Vec<&str> = said["layers"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for want in ["A-WALL", "A-DOOR", "A-GLAZ", "A-GRID", "A-AREA-IDEN", "A-ANNO-DIMS", "A-ANNO-TEXT"] {
        assert!(layers.contains(&want), "{want} is not among {layers:?}");
    }
    // Every layer the entities name is in the file's own table, and the house is 10 m by 7 m.
    assert!(d.entities.iter().all(|e| d.layer(&e.layer).is_some_and(|l| l.name == e.layer)));
    let [lo, hi] = bounds(&d, "A-WALL");
    assert!(within([lo, hi], [[-0.15, -0.15], [10.15, 7.15]], 0.01), "{lo:?} {hi:?}");
    // The grids are dashed: a linetype of its own, its dashes on, off, on, off, in metres.
    let dashed: Vec<&str> = d.entities.iter().filter(|e| e.layer == "A-GRID").filter_map(|e| e.linetype.as_deref()).collect();
    assert!(!dashed.is_empty() && dashed.iter().all(|n| n.starts_with("BC-DASH-")), "{dashed:?}");
    let lt = d.linetypes.iter().find(|l| l.name == dashed[0]).expect("its linetype is in the table");
    assert!(
        lt.dashes.len() % 2 == 0 && lt.dashes.iter().step_by(2).all(|x| *x > 0.0) && lt.dashes.iter().skip(1).step_by(2).all(|x| *x < 0.0),
        "{lt:?}"
    );
    // The roof overhead is dashed too, a house's pens have weights, and no picture is left out of a plan.
    assert!(text.contains("370"), "line weights");
    assert!(said.get("notes").is_none(), "{said}");
    // Lettering is outlined, never TEXT: the drawing holds the strokes, not the string.
    assert!(!d.entities.iter().any(|e| matches!(e.shape, Shape::Text { .. })));
    assert!(d.entities.iter().any(|e| e.layer == "A-ANNO-TEXT" && matches!(e.shape, Shape::Line(..) | Shape::Polyline { .. })));
}

#[test]
fn a_dxf_opens_in_the_engine_as_a_cad_import_where_it_was_drawn() {
    let mut s = keeping();
    s.script(SAMPLE).unwrap();
    let (_, _, text) = saved(&mut s, json!({"units": "m"}));
    // Given back as a file, as another project or another program's drawing would be.
    let mut other = keeping();
    other.files.give("in/house.dxf", text.into_bytes());
    let r = run(&mut other, "import_cad", json!({"path": "in/house.dxf"}));
    assert_eq!((r["unit"].as_str(), r["scale"].as_f64()), (Some("m"), Some(1000.0)), "{r}");
    assert!(r["entities"].as_u64().unwrap() > 100 && r["layers"].as_u64().unwrap() >= 7, "{r}");
    // The 10 m by 7 m house with its grids, dimensions and tags round it: more than the house, less than twice it.
    let side = |k: usize| r["extents"][1][k].as_f64().unwrap() - r["extents"][0][k].as_f64().unwrap();
    assert!((12_000.0..20_000.0).contains(&side(0)) && (9_000.0..16_000.0).contains(&side(1)), "{r}");
    assert!(r["left_out"].as_object().is_some_and(|l| l.is_empty()), "{r}");
}

#[test]
fn render_writes_a_dxf_too_and_a_view_that_is_no_drawing_is_refused() {
    let mut s = one_wall();
    let r = run(&mut s, "render", json!({"path": "out/render.dxf", "units": "m"}));
    assert_eq!((r["format"].as_str(), r["path"].as_str(), r["units"].as_str()), (Some("DXF"), Some("out/render.dxf"), Some("m")), "{r}");
    assert_eq!(s.files.take_written().len(), 1);
    let e = s.execute("save", &json!({"path": "out/x.dxf", "view": "No such view"})).unwrap_err();
    assert!(e.contains("No such view"), "{e}");
}

#[test]
fn a_named_plan_view_is_written_as_its_crop_shows_it() {
    let mut s = one_wall();
    let made = run(&mut s, "duplicate_view", json!({"view": "Level 1", "name": "Entrance"}));
    run(&mut s, "crop", json!({"view": made["id"], "region": [[-1000, -1000], [2000, 1000]]}));
    let (d, said, _) = saved(&mut s, json!({"view": "Entrance"}));
    assert_eq!(said["view"], "Entrance");
    let [lo, hi] = bounds(&d, "A-WALL");
    assert!(lo[0].abs() <= 1.0 && (hi[0] - 2000.0).abs() <= 1.0, "the wall as far as the crop: {lo:?} {hi:?}");
}

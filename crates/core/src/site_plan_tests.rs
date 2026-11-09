//! A site plan in the engine, as Arqiflow links one under a building (`link {text}`): floors whose material fills their plan
//! in its surface colour (lawns green, roads grey), halftone through the link; and a ground shaped to the terrain, drawn by
//! its contours as Revit draws a toposolid.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::ids::Id;
use crate::render::{self, Prim, Rgb, View};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

/// A lawn 10 m square at the origin, in a material whose solid surface pattern is green.
fn lawn(s: &mut Session) -> Id {
    run(s, "material", json!({"name": "Lawn", "color": [92, 138, 70], "surface": "solid", "surface_color": [120, 180, 90]}));
    run(s, "type", json!({"category": "floor", "name": "Lawn 50", "layers": [{"function": "structure", "material": "Lawn", "thickness": 50}]}));
    let id = run(s, "floor", json!({"boundary": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]], "type": "Lawn 50"}))["id"].clone();
    serde_json::from_value(id).unwrap()
}

fn view() -> View {
    View { scale: 0.04, cx: 5000.0, cy: 5000.0, width: 800, height: 800 }
}

fn fills(d: &render::Drawing, owner: impl Fn(Id) -> bool) -> Vec<Rgb> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Fill { color, .. } if o.is_some_and(&owner) => Some(*color),
            _ => None,
        })
        .collect()
}

#[test]
fn a_floor_whose_material_has_a_solid_surface_in_a_colour_fills_its_plan_with_it_and_a_link_draws_it_halftone() {
    let mut s = Session::new();
    let id = lawn(&mut s);
    let level = s.model.levels()[0].0;
    let own = render::draw(&s.model, level, view());
    assert!(fills(&own, |o| o == id).contains(&Rgb(120, 180, 90)), "filled in its surface colour");
    // Without a colour of its own, the drawing's grey, as before.
    let mut grey = Session::new();
    grey.execute("material", &json!({"name": "Lawn", "surface": "solid"})).unwrap();
    grey.execute("type", &json!({"category": "floor", "name": "Lawn 50", "layers": [{"function": "structure", "material": "Lawn", "thickness": 50}]}))
        .unwrap();
    let gid: Id = serde_json::from_value(
        grey.execute("floor", &json!({"boundary": [[0, 0], [10000, 0], [10000, 10000], [0, 10000]], "type": "Lawn 50"})).unwrap()["id"].clone(),
    )
    .unwrap();
    let plain = render::draw(&grey.model, grey.model.levels()[0].0, view());
    assert!(fills(&plain, |o| o == gid).iter().all(|c| *c == Rgb(120, 120, 120)), "{:?}", fills(&plain, |o| o == gid));
    // `null` takes it back.
    run(&mut s, "material", json!({"name": "Lawn", "surface_color": null}));
    assert_eq!(s.model.materials["Lawn"].surface_color, None);
    run(&mut s, "material", json!({"name": "Lawn", "surface_color": "#78b45a"}));
    assert_eq!(s.model.materials["Lawn"].surface_color, Some([120, 180, 90]));
    assert!(s.execute("material", &json!({"name": "Lawn", "surface_color": "green"})).unwrap_err().contains("surface_color"));

    // Linked as a site: the same fill, halfway to the paper.
    let text = s.model.to_json().unwrap();
    let mut b = Session::new();
    run(&mut b, "link", json!({"text": text, "name": "Site"}));
    let seen = crate::link::seen(&b.model);
    let linked = render::draw(&seen, b.model.levels()[0].0, view());
    let got = fills(&linked, crate::link::is_linked);
    assert!(got.contains(&Rgb(187, 217, 172)), "halftone green: {got:?}");
}

#[test]
fn a_floor_with_contours_draws_them_where_its_shaped_top_crosses_each_interval_and_not_its_creases() {
    let mut s = Session::new();
    let id = lawn(&mut s);
    // A slope rising 4.5 m from its west edge to its east.
    let points: Vec<Value> = [[0, 0, 0], [0, 10000, 0], [10000, 0, 4500], [10000, 10000, 4500], [5000, 5000, 2250]]
        .iter()
        .map(|[x, y, z]| json!({"at": [x, y], "z": z}))
        .collect();
    run(&mut s, "edit_shape", json!({"id": id, "points": points}));
    let level = s.model.levels()[0].0;
    let lines = |s: &Session| -> Vec<[f32; 4]> {
        let d = render::draw(&s.model, level, view());
        d.prims
            .iter()
            .zip(&d.owners)
            .zip(&d.parts)
            .filter_map(|((p, o), part)| match p {
                Prim::Line { pts, .. } if *o == Some(id) && *part != render::Part::Pattern && pts.len() == 2 => {
                    Some([pts[0].0, pts[0].1, pts[1].0, pts[1].1])
                }
                _ => None,
            })
            .collect()
    };
    let creases = lines(&s).len();
    run(&mut s, "modify", json!({"id": id, "contours": 1000}));
    let drawn = lines(&s);
    // At 1, 2, 3 and 4 m: each a north-south line, crossing whichever planes it does.
    let xs: std::collections::BTreeSet<i64> = drawn.iter().filter(|l| (l[0] - l[2]).abs() < 0.5).map(|l| (l[0] / 4.0).round() as i64).collect();
    assert_eq!(xs.len(), 4, "a contour a metre: {drawn:?}");
    assert!(drawn.iter().all(|l| (l[0] - l[2]).abs() < 0.5), "every one runs north-south, no crease is drawn: {drawn:?}");
    assert_ne!(drawn.len(), creases);
    assert!(s.execute("modify", &json!({"id": id, "contours": 10})).unwrap_err().contains("contours"));
    // The floor command takes them too.
    let g = run(&mut s, "floor", json!({"boundary": [[20000, 0], [30000, 0], [30000, 10000], [20000, 10000]], "contours": 500}));
    let all = run(&mut s, "elements", json!({"category": "floor"}));
    let f = all["elements"].as_array().unwrap().iter().find(|e| e["id"] == g["id"]).cloned().unwrap();
    assert_eq!(f["contours"], 500.0, "{f}");
}

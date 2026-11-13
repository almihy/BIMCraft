//! Families as an exporter brings them from Revit (original content: `testdata/families/sample-two-type-flush-door.json`):
//! each type's own forms, Revit's Visibility Settings of forms and symbols (detail levels; plan, front and side views), a
//! door's Elevation Swing in elevations, a traced outline, a wall opening other than the leaf (`cut`), flip controls, where
//! it came from, text parameters, materials brought with it, Duplicate's `like`, and BIMCraft's own families listed for
//! Load Family. Each checked through what the commands reply and what the views draw.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::graphics::{Detail, Look};
use crate::model::{Element, Id};
use crate::prim::Prim;
use crate::render::{self, Drawing};

const SAMPLE: &str = include_str!("../testdata/families/sample-two-type-flush-door.json");
const NAME: &str = "Sample Two-Type Flush Door";

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// A project with the sample family loaded from its file, a 4 m wall along x (its left, +y, the door's exterior) and the
/// sample door of `ty` at x 2000. The door's id.
fn door(s: &mut Session, ty: &str) -> Id {
    s.files.give("door.json", SAMPLE.as_bytes().to_vec());
    run(s, "family", json!({"path": "door.json"}));
    run(s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    id_of(&run(s, "door", json!({"at": [2000, 0], "family": NAME, "type": ty})))
}

fn plan(s: &Session) -> Drawing {
    render::plan(&s.model, 100000, 3000, 2000)
}

/// The lines `id` draws, in model mm, and whether each is closed.
fn lines(d: &Drawing, id: Id) -> Vec<(Vec<P2>, bool)> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, closed, .. } if *o == Some(id) => Some((pts.iter().map(|q| d.view.mm(*q)).collect(), *closed)),
            _ => None,
        })
        .collect()
}

/// A ring's extent along x and y.
fn spans(ring: &[P2]) -> (f64, f64) {
    let (lo, hi) = ring.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), q| {
        (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1)))
    });
    (hi.0 - lo.0, hi.1 - lo.1)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0
}

#[test]
fn a_family_file_brings_its_materials_and_says_where_it_came_from() {
    let mut s = Session::new();
    // The project's own Sample Oak wins over the one the family brings.
    run(&mut s, "material", json!({"name": "Sample Oak", "color": [10, 20, 30]}));
    s.files.give("door.json", SAMPLE.as_bytes().to_vec());
    let r = run(&mut s, "family", json!({"path": "door.json", "source": {"library": "fam_123", "version": 3}}));
    assert_eq!(r["materials"], json!(["Sample Steel"]), "{r}");
    assert_eq!(s.model.materials["Sample Oak"].color, [10, 20, 30]);
    assert_eq!(s.model.materials["Sample Steel"].color, [150, 152, 158]);
    let f = &s.model.families[NAME];
    let src = f.source.clone().unwrap_or_default();
    assert_eq!(
        (src.library.as_deref(), src.version, src.app.as_deref(), src.file.as_deref()),
        (Some("fam_123"), Some(3), Some("BIMCraft sample"), Some("Sample Two-Type Flush Door.rfa"))
    );
    assert_eq!((f.host, f.flips.hand, f.flips.facing, f.subcategories.len()), (crate::family::Host::Wall, true, false, 4));
    // The list and the read both say it.
    let list = run(&mut s, "family", json!({}));
    let mine = list["families"].as_array().unwrap().iter().find(|x| x["name"] == NAME).cloned().unwrap();
    assert_eq!(mine["source"]["library"], "fam_123");
    let read = run(&mut s, "family", json!({"name": NAME}));
    assert_eq!(read["family"]["source"]["version"], 3);
    // A text parameter is each type's words, read as they are and never by a formula.
    assert_eq!(read["values"]["1000 x 2100"]["Fire Rating"], "EI 60");
    assert_eq!(read["values"]["900 x 2100"]["Fire Rating"], "EI 30");
    assert!((read["values"]["900 x 2100"]["Leaf Area"].as_f64().unwrap() - 1.89).abs() < 1e-9);
    let e = s
        .execute(
            "family",
            &json!({"name": NAME, "parameters": [{"name": "Width", "value": 900}, {"name": "Height", "value": 2100},
            {"name": "Frame Width", "value": 50}, {"name": "Fire Rating", "kind": "text", "value": "x"},
            {"name": "Twice", "formula": "Fire Rating * 2"}]}),
        )
        .unwrap_err();
    assert!(e.contains("not a number"), "{e}");
}

#[test]
fn each_type_builds_its_own_forms_only() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    let e = s.model.elements[&d].clone();
    let leaves = |s: &Session, e: &Element| -> Vec<f64> {
        let (pieces, _) = s.model.opening_local(e).unwrap();
        // The leaf: the only form 40 thick and 2100 tall.
        pieces.iter().filter(|p| near(p.top - p.bottom, 2100.0) && near(spans(&p.outline).1, 40.0)).map(|p| spans(&p.outline).0).collect()
    };
    assert_eq!(s.model.opening_local(&e).unwrap().0.len(), 5, "three frame pieces, its type's leaf, the handle");
    assert_eq!(leaves(&s, &e), vec![900.0]);
    run(&mut s, "modify", json!({"id": d, "type": "1000 x 2100"}));
    let e = s.model.elements[&d].clone();
    assert_eq!(leaves(&s, &e), vec![1000.0]);
    // Its traced outline is the leaf's own, not a hull.
    let (pieces, _) = s.model.opening_local(&e).unwrap();
    assert!(pieces.iter().any(|p| p.outline.len() == 4 && near(spans(&p.outline).0, 1000.0)));
}

#[test]
fn the_wall_opening_is_the_familys_cut_and_its_width_stays_the_leafs() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    let t = s.model.opening_type(&s.model.elements[&d]).unwrap();
    assert_eq!((t.width, t.height), (900.0, 2100.0), "its Width and Height are the leaf's");
    assert_eq!(t.cut(), (1000.0, 2150.0), "Width + 2 x Frame Width, Height + Frame Width");
    // The wall's pieces leave a gap 1000 wide round x 2000.
    let Element::Door(o) = &s.model.elements[&d] else { panic!("a door") };
    let wall = s.model.elements.id(o.host).unwrap();
    let solids = crate::view3d::solids(&s.model);
    let xs: Vec<f64> = solids
        .iter()
        .filter_map(|x| match x {
            crate::view3d::Solid::Prism { owner, ring, .. } if *owner == wall => Some(ring.clone()),
            _ => None,
        })
        .flatten()
        .map(|p| p.0)
        .filter(|x| (1400.0..2600.0).contains(x))
        .collect();
    assert!(xs.iter().any(|x| near(*x, 1500.0)) && xs.iter().any(|x| near(*x, 2500.0)), "{xs:?}");
    assert!(!xs.iter().any(|x| near(*x, 1550.0) || near(*x, 2450.0)), "{xs:?}");
}

#[test]
fn a_plan_draws_the_swing_at_every_level_and_never_the_leaf_mesh() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    let open_with = |dr: &Drawing, n: usize| lines(dr, d).iter().filter(|(p, closed)| !closed && p.len() == n).count();
    for detail in ["fine", "medium", "coarse"] {
        run(&mut s, "detail", json!({"view": 100000, "detail": detail}));
        let dr = plan(&s);
        let all = lines(&dr, d);
        // Its swing (an arc of many chords) and the open leaf (five points), at Coarse too: their `show` says so.
        assert_eq!(all.iter().filter(|(p, closed)| !closed && p.len() > 5).count(), 1, "{detail}: the swing");
        assert_eq!(open_with(&dr, 5), 1, "{detail}: the open leaf");
        // The leaf mesh, 900 by 40 seen from above, is never outlined: its `show.plan` is false.
        let leaf = all.iter().filter(|(p, closed)| {
            *closed && {
                let (a, b) = spans(p);
                near(a, 900.0) && near(b, 40.0)
            }
        });
        assert_eq!(leaf.count(), 0, "{detail}");
        // The frame's jambs are cut and outlined at every level: a form that says nothing is drawn everywhere, as Revit's default.
        let jambs = all.iter().filter(|(p, closed)| *closed && near(spans(p).0, 50.0)).count();
        assert_eq!(jambs, 2, "{detail}");
    }
}

#[test]
fn an_elevation_facing_the_door_draws_its_elevation_swing_and_a_side_view_does_not() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    // From the exterior (+y), looking south along -y; and along the wall, looking along x.
    let front = id_of(&run(&mut s, "section", json!({"start": [4000, 3000], "end": [0, 3000], "name": "Front"})));
    let back = id_of(&run(&mut s, "section", json!({"start": [0, -3000], "end": [4000, -3000], "name": "Back"})));
    let side = id_of(&run(&mut s, "section", json!({"start": [-3000, -1500], "end": [-3000, 1500], "name": "Side"})));
    let swing = |s: &Session, view: Id| -> Vec<Vec<P2>> {
        let fit = render::section_fit(&s.model, view, 1200, 900);
        let dr = render::section(&s.model, view, fit);
        lines(&dr, d).into_iter().filter(|(p, closed)| !closed && p.len() == 3).map(|(p, _)| p).collect()
    };
    for (view, name) in [(front, "front"), (back, "back")] {
        let v = swing(&s, view);
        assert_eq!(v.len(), 1, "{name}: the Elevation Swing, {v:?}");
        let p = &v[0];
        // From the free edge at the sill to the hinged edge half way up, back to the free edge at the head.
        assert!(near(p[1].1 - p[0].1, 1050.0) && near(p[2].1 - p[0].1, 2100.0), "{name}: {p:?}");
        assert!(near((p[1].0 - p[0].0).abs(), 900.0), "{name}: {p:?}");
    }
    // Seen from the front and the back the hinge is on opposite sides of the picture: the back mirrors it.
    let (f, b) = (&swing(&s, front)[0], &swing(&s, back)[0]);
    let hinge_left = |p: &Vec<P2>| p[1].0 < p[0].0;
    assert_ne!(hinge_left(f), hinge_left(b));
    assert!(swing(&s, side).is_empty(), "a view along the wall sees the door's side");
    // Coarse: a symbol that says nothing of its levels isn't drawn.
    run(&mut s, "detail", json!({"view": front, "detail": "coarse"}));
    assert!(swing(&s, front).is_empty());
}

#[test]
fn views_draw_a_form_at_the_detail_levels_and_directions_it_says() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    let scene = crate::view3d::Scene::new(&s.model);
    let view3d = run(&mut s, "detail", json!({"view": "{3D}", "detail": "fine"}))["view"].as_u64().unwrap();
    let drawn = |s: &Session, view: Id| scene.tris.iter().filter(|t| t.owner == d && Look::of(&s.model, view).draws(t)).count();
    let fine = drawn(&s, view3d);
    run(&mut s, "detail", json!({"view": "{3D}", "detail": "coarse"}));
    assert_eq!(Look::of(&s.model, view3d).detail(), Detail::Coarse);
    // The handle's two faces' 12 triangles each are gone at Coarse.
    assert_eq!(fine - drawn(&s, view3d), 12, "the handle, at Medium and Fine only");
    // A form kept from side views is drawn looking at the door's face, not along the wall.
    let mut f = s.model.families[NAME].clone();
    f.forms[5].show = Some(crate::family::Show { side: false, ..Default::default() });
    let forms = serde_json::to_value(&f.forms).unwrap();
    run(&mut s, "family", json!({"name": NAME, "forms": forms}));
    let scene = crate::view3d::Scene::new(&s.model);
    let front = id_of(&run(&mut s, "section", json!({"start": [4000, 3000], "end": [0, 3000]})));
    let side = id_of(&run(&mut s, "section", json!({"start": [-3000, -1500], "end": [-3000, 1500]})));
    let count = |view: Id| scene.tris.iter().filter(|t| t.owner == d && Look::of(&s.model, view).draws(t)).count();
    assert_eq!(count(front) - count(side), 12);
}

#[test]
fn duplicating_a_type_keeps_its_forms_and_renaming_or_deleting_one_follows() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    let leaf_types = |s: &Session| -> Vec<Vec<String>> {
        s.model.families[NAME].forms.iter().filter(|f| !f.types.is_empty()).map(|f| f.types.clone()).collect()
    };
    run(&mut s, "family", json!({"name": NAME, "types": {"900 x 2100 Copy": {"Width": 900}}, "like": {"900 x 2100 Copy": "900 x 2100"}}));
    assert_eq!(leaf_types(&s), vec![vec!["900 x 2100".to_string(), "900 x 2100 Copy".into()], vec!["1000 x 2100".into()]]);
    run(&mut s, "modify", json!({"id": d, "type": "900 x 2100 Copy"}));
    assert_eq!(s.model.opening_local(&s.model.elements[&d]).unwrap().0.len(), 5, "the copy has the leaf");
    run(&mut s, "rename_type", json!({"family": NAME, "type": "900 x 2100 Copy", "name": "Narrow"}));
    assert_eq!(leaf_types(&s)[0], vec!["900 x 2100".to_string(), "Narrow".into()]);
    assert_eq!(s.model.opening_local(&s.model.elements[&d]).unwrap().0.len(), 5, "renamed, it keeps its leaf");
    // A type taken away takes the forms of it alone: none is left of no type, drawn in every one.
    run(&mut s, "family", json!({"name": NAME, "types": {"1000 x 2100": null}}));
    assert_eq!(leaf_types(&s), vec![vec!["900 x 2100".to_string(), "Narrow".into()]]);
    assert_eq!(s.model.families[NAME].forms.len(), 5);
    // `like` names a type the family has.
    let e = s.execute("family", &json!({"name": NAME, "types": {"X": {}}, "like": {"X": "Nope"}})).unwrap_err();
    assert!(e.contains("Nope"), "{e}");
    // Duplicate from Type Properties is undone in one step.
    run(&mut s, "undo", json!({}));
    assert!(s.model.families[NAME].types.contains_key("1000 x 2100"));
}

#[test]
fn a_project_using_the_new_fields_is_written_as_23_and_one_without_as_before() {
    use crate::model::{Model, PREVIOUS_FORMAT, TYPES_FORMAT as FORMAT};
    let mut s = Session::new();
    let format = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap()["format"].clone();
    assert_eq!(format(&s), json!(PREVIOUS_FORMAT));
    // Where it came from and its flip controls an older build leaves out: no new format for them.
    run(
        &mut s,
        "family",
        json!({"name": "Plain", "types": {"A": {}}, "source": {"app": "Revit 2025"}, "flips": {"hand": false},
        "forms": [{"profile": [[0, 0], [100, 0], [100, 100]], "to": 100}]}),
    );
    assert_eq!(format(&s), json!(PREVIOUS_FORMAT));
    let door = door(&mut s, "900 x 2100");
    assert_eq!(format(&s), json!(FORMAT));
    assert_eq!(FORMAT, 23, "a family from Revit that is no more than its result per type");
    // Read back, it is as it was.
    let (m, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert_eq!(m.families[NAME], s.model.families[NAME]);
    assert_eq!(m.families["Plain"].source, s.model.families["Plain"].source);
    assert_eq!(m.opening_local(&m.elements[&door]).unwrap().0.len(), 5);
    // A build that reads up to 22 refuses it rather than drawing both leaves at once.
    let mut v: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    v["format"] = json!(crate::model::FORMAT + 1);
    assert!(Model::read(&v.to_string()).unwrap_err().contains("newer"));
}

/// The formats after PDF layers (21): a template that chose not to control Imported Categories keeps that through a file of
/// 22, 23 or 24, which can name the group; only a file older than 21 has it given back where it is implied.
#[test]
fn a_file_of_a_family_format_keeps_a_templates_imported_categories_as_it_was() {
    use crate::graphics::Group;
    use crate::model::{FORMAT, Model, PDF_FORMAT, TYPES_FORMAT};
    assert_eq!((PDF_FORMAT, TYPES_FORMAT, FORMAT), (21, 23, 24));
    let mut s = Session::new();
    door(&mut s, "900 x 2100");
    let level = s.model.guid(s.model.levels()[0].0).to_string();
    // Every group of a plan but Imported Categories: a file older than 21 would give it back.
    let t = run(&mut s, "template", json!({"name": "No Imports", "from": level}));
    let all: Vec<String> = t["controls"].as_array().unwrap().iter().filter_map(|g| g.as_str().map(str::to_string)).collect();
    let wanted: Vec<&str> = all.iter().map(String::as_str).filter(|g| *g != "imports").collect();
    assert_eq!(wanted.len() + 1, all.len(), "{all:?}");
    run(&mut s, "template", json!({"name": "No Imports", "controls": wanted}));
    assert!(!s.model.view_templates["No Imports"].controls.contains(&Group::Imports));
    let text = s.model.to_json().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap()["format"], json!(TYPES_FORMAT));
    let (m, left_out) = Model::read(&text).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    assert!(!m.view_templates["No Imports"].controls.contains(&Group::Imports), "a file of 23 says what it controls");
    // A file of 20, written the same, has it back.
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["format"] = json!(20);
    let (m, _) = Model::read(&v.to_string()).unwrap();
    assert!(m.view_templates["No Imports"].controls.contains(&Group::Imports), "implied where a file of 20 can't say");
}

#[test]
fn a_family_says_which_flips_it_has() {
    let mut s = Session::new();
    let d = door(&mut s, "900 x 2100");
    let f = &s.model.families[NAME];
    assert!(f.flips.hand && !f.flips.facing);
    // The command flips it all the same, as Revit's API does.
    run(&mut s, "flip", json!({"ids": [d], "facing": true}));
}

#[test]
fn bimcraft_families_are_listed_and_read_for_load_family_at_no_undo_step() {
    let mut s = Session::new();
    let steps = run(&mut s, "inspect", json!({"brief": true}))["history"].clone();
    let all = run(&mut s, "family_library", json!({}));
    let list = all["families"].as_array().unwrap();
    assert!(list.iter().any(|f| f["name"] == "Single Door" && f["set"] == "bimcraft" && f["category"] == "door" && f["loaded"] == true));
    assert!(list.iter().any(|f| f["set"] == "arei" && f["loaded"] == false));
    let doors = run(&mut s, "family_library", json!({"category": "door"}));
    assert!(doors["families"].as_array().unwrap().iter().all(|f| f["category"] == "door"));
    let one = run(&mut s, "family_library", json!({"name": "Single Door"}));
    assert_eq!(one["family"]["name"], "Single Door");
    assert!(one["family"]["types"].as_object().unwrap().contains_key("900 x 2100"));
    // Loaded under another name with `family`, it is the same family.
    let mut def = one["family"].clone();
    def["name"] = json!("Single Door 2");
    run(&mut s, "family", def);
    let (a, b) = (&s.model.families["Single Door 2"], &s.model.families["Single Door"]);
    assert_eq!((&a.types, &a.parameters, a.forms.len(), a.symbols.len()), (&b.types, &b.parameters, b.forms.len(), b.symbols.len()));
    run(&mut s, "undo", json!({}));
    assert!(!s.model.families.contains_key("Single Door 2"));
    assert_eq!(run(&mut s, "inspect", json!({"brief": true}))["history"], steps, "reading cost no step");
    assert!(s.execute("family_library", &json!({"name": "Nope"})).unwrap_err().contains("no family"));
    assert!(s.execute("family_library", &json!({"set": "nope"})).unwrap_err().contains("no set"));
}

#[test]
fn a_door_schedule_lists_a_familys_text_parameter_with_each_doors_words() {
    let mut s = Session::new();
    let d = door(&mut s, "1000 x 2100");
    let r = run(&mut s, "schedule", json!({"category": "door", "fields": ["Mark", "Fire Rating", "Leaf Area"]}));
    let rows = r["rows"].as_array().unwrap_or_else(|| panic!("{r}"));
    assert_eq!(rows.len(), 1, "{r}");
    assert!(rows[0].to_string().contains("EI 60"), "{r}");
    assert!(r["available"].as_array().is_some_and(|a| a.iter().any(|f| f.to_string().contains("Fire Rating"))), "{r}");
    let _ = d;
}

/// A form of Revit's definition that says every view and every detail level (all eight boxes ticked, which is Revit's
/// default) is drawn in a plan at Coarse when it crosses the cut: a form that says everything is a form that says nothing.
#[test]
fn a_revit_form_saying_every_view_is_drawn_in_a_coarse_plan() {
    // A jamb 50 by 200 in plan, 2100 tall, as the exporter writes it: Revit's result (a mesh, per type) and the definition
    // (an extrusion bound to the door's planes) that rebuilds it.
    let cuboid = |lo: [f64; 3], hi: [f64; 3]| {
        let p = |i: usize| {
            [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]
        };
        let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
        json!(quads.iter().flat_map(|[a, b, c, d]| [json!([p(*a), p(*b), p(*c)]), json!([p(*a), p(*c), p(*d)])]).collect::<Vec<_>>())
    };
    let at = |v: f64| json!({"900": v});
    let side = |a: Value, b: Value| json!({"line": [a, b]});
    let corner = |x: Value, y: Value| json!([x, y, 0]);
    let (l0, l1) = (json!({"ref": "l", "offset": -50.0}), json!({"ref": "l", "offset": 0.0}));
    let (i, e) = (json!({"ref": "i", "offset": 0.0}), json!({"ref": "e", "offset": 0.0}));
    let ring = json!([
        side(corner(l0.clone(), i.clone()), corner(l1.clone(), i.clone())),
        side(corner(l1.clone(), i.clone()), corner(l1.clone(), e.clone())),
        side(corner(l1.clone(), e.clone()), corner(l0.clone(), e.clone())),
        side(corner(l0.clone(), e), corner(l0, i)),
    ]);
    let everything = json!({"coarse": true, "medium": true, "fine": true, "plan": true, "front": true, "side": true,
                            "plan_top": true, "plan_cut": true});
    let package = json!({
        "format": 2,
        "family": {"category": "door", "host": "wall",
                   "parameters": [{"name": "Width", "kind": "length", "value": 900}, {"name": "Height", "kind": "length", "value": 2100}],
                   "types": {"900": {"Width": 900}},
                   "forms": [{"mesh": cuboid([-500.0, -100.0, 0.0], [-450.0, 100.0, 2100.0]), "types": ["900"], "snapshot": true}]},
        "definition": {
            "to_bimcraft": {"x_offset": 0, "y_sign": 1, "z_minus": 0, "wall": at(200.0)},
            "reference_planes": [
                {"id": "c", "name": "Center", "normal": [1, 0, 0], "defines_origin": true, "at": at(0.0)},
                {"id": "l", "name": "Left", "normal": [1, 0, 0], "at": at(-450.0)},
                {"id": "r", "name": "Right", "normal": [1, 0, 0], "at": at(450.0)},
                {"id": "e", "name": "Exterior", "normal": [0, 1, 0], "is_reference": "host_exterior", "at": at(100.0)},
                {"id": "i", "name": "Interior", "normal": [0, 1, 0], "is_reference": "host_interior", "at": at(-100.0)},
                {"id": "z", "name": "Ref Level", "normal": [0, 0, 1], "defines_origin": true, "at": at(0.0)}
            ],
            "dimensions": [{"refs": ["l", "r"], "label": "Width"}, {"refs": ["l", "c", "r"], "eq": true}],
            "forms": [{"kind": "extrusion", "normal": [0, 0, 1], "start": 0, "end": 2100, "loops": [ring], "show": everything}]
        }
    });
    let mut s = Session::new();
    s.files.give("jamb.arqiflow-family.json", package.to_string().into_bytes());
    let r = run(&mut s, "family", json!({"path": "jamb.arqiflow-family.json"}));
    assert_eq!(r["check"]["900"]["matched"], true, "{}", r["check"]);
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]]}));
    let d = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "family": "jamb", "type": "900"})));
    for detail in ["coarse", "medium", "fine"] {
        run(&mut s, "detail", json!({"view": 100000, "detail": detail}));
        let jamb = lines(&plan(&s), d).into_iter().filter(|(p, closed)| *closed && near(spans(p).0, 50.0) && near(spans(p).1, 200.0)).count();
        assert_eq!(jamb, 1, "{detail}: the jamb the plane cuts is outlined");
    }
}

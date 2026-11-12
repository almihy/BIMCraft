//! `library`'s tests that drive the Session (moved out of `library.rs`).

use crate::ids::{Guid, Id};
use crate::model::Model;

use serde_json::{Value, json};

use crate::engine::Session;
use crate::library::*;
use crate::model::{Element, FIRST_ID};

#[test]
fn parts_are_kept_by_id_and_by_guid_and_a_copy_shares_what_it_didnt_change() {
    let part = |i: usize, guid: Guid| Part { what: "sketch line", of: "floor x".into(), owner: "x".into(), index: i, guid };
    let (a, b, c) = (Guid::new(), Guid::new(), Guid::new());
    let mut parts: Parts = [(999_000, part(0, a)), (999_001, part(1, b)), (5_000, part(2, c))].into_iter().collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts.iter().map(|(id, _)| id).collect::<Vec<_>>(), [5_000, 999_000, 999_001]);
    assert_eq!((parts.first_id(), parts.last_id()), (Some(5_000), Some(999_001)));
    assert_eq!((parts.by_guid(a), parts.by_guid(c)), (Some(999_000), Some(5_000)));
    let copy = parts.clone();
    assert_eq!(copy, parts);
    // Two parts of one GUID (out of step): the other keeps it when one goes.
    parts.insert(999_002, part(3, a));
    assert_ne!(copy, parts);
    assert_eq!(parts.remove(999_000).map(|p| p.index), Some(0));
    assert_eq!(parts.by_guid(a), Some(999_002));
    assert_eq!(parts.remove(999_002).map(|p| p.guid), Some(a));
    assert_eq!(parts.by_guid(a), None);
    assert_eq!(parts.remove(999_002), None);
    // A part given again with another GUID.
    parts.insert(5_000, part(2, b));
    assert_eq!((parts.by_guid(c), parts.len()), (None, 2));
    parts.remove(999_001);
    assert_eq!(parts.by_guid(b), Some(5_000));
    // What the copy didn't change, it shares; and it holds what it held.
    assert_eq!(copy.len(), 3);
    assert_eq!(copy.by_guid(a), Some(999_000));
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn temp(name: &str) -> String {
    std::env::temp_dir().join(format!("bimcraft-library-{name}-{}.bimcraft", std::process::id())).to_string_lossy().into_owned()
}

/// A project with something of every kind its elements name: a wall of a layered type, a project parameter it holds, a
/// phase it's made in, a design option, a material painted on it, a view with a template, a filter and a phase filter.
fn project() -> (Session, Id) {
    let mut s = Session::new();
    s.script(
        r#"phase {"name": "Survey", "before": "Existing"}
option {"set": "Roofs", "options": ["Flat", "Pitched"]}
parameter {"name": "Fire Rating", "kind": "text", "categories": ["wall"]}
filter {"name": "Long", "categories": ["wall"]}
template {"name": "Plans", "from": 100000}"#,
    )
    .unwrap();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [5000, 0]], "type": "Exterior - 300mm"}))["ids"][0].as_u64().unwrap();
    run(&mut s, "modify", json!({"id": w, "params": {"Fire Rating": "EI 60", "Phase Created": "Survey", "Design Option": "Roofs: Flat"}}));
    run(&mut s, "paint", json!({"id": w, "face": "exterior", "material": "Timber"}));
    run(&mut s, "graphics", json!({"view": 100000, "template": "Plans"}));
    run(
        &mut s,
        "graphics",
        json!({"view": 100001, "filters": [{"filter": "Long", "hidden": true}], "phase_filter": "Show New", "options": {"Roofs": "Pitched"}}),
    );
    (s, w)
}

#[test]
fn every_library_entry_has_an_id_counting_down_and_a_guid_and_elements_still_count_up() {
    let s = Session::new();
    let m = &s.model;
    assert!(m.library_in_step() && m.library.len() > 50, "{}", m.library.len());
    let ids: Vec<Id> = m.library.entries().map(|(id, _)| id).collect();
    assert_eq!(ids.last(), Some(&LAST_ID), "from the top down");
    assert!(ids.iter().all(|id| (FIRST_ID..=LAST_ID).contains(id) && !m.elements.contains_key(*id)));
    assert_eq!(m.elements.keys().copied().collect::<Vec<_>>(), [100_000, 100_001], "the levels: elements count up");
    let brick = m.library.id(&Kind::Material, "Brick").unwrap();
    assert_eq!(m.library.get(brick).map(|e| e.name.as_str()), Some("Brick"));
    assert!(m.library.guid(&Kind::Type("wall".into()), "Generic - 200mm").is_some());
    assert!(m.library.id(&Kind::Phase, "New Construction").is_some());
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn a_file_holds_the_guid_of_whatever_it_names_and_opens_as_it_was() {
    let (s, w) = project();
    let m = &s.model;
    let g = |kind: Kind, name: &str| json!(m.library.guid(&kind, name).unwrap().to_string());
    let file: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
    assert_eq!(file["format"], crate::model::PREVIOUS_FORMAT);
    let wall = &file["elements"][w.to_string()];
    assert_eq!(wall["type"], g(Kind::Type("wall".into()), "Exterior - 300mm"));
    let params = wall["params"].as_object().unwrap();
    let rating = m.library.guid(&Kind::Parameter, "Fire Rating").unwrap().to_string();
    assert_eq!(params[&rating], "EI 60", "keyed by the parameter's GUID: {params:?}");
    assert_eq!(params["Phase Created"], g(Kind::Phase, "Survey"));
    assert_eq!(params["Design Option"], g(Kind::DesignOption, "Roofs: Flat"));
    assert_eq!(params["Paint"]["exterior"], g(Kind::Material, "Timber"));
    assert_eq!(file["types"]["wall"]["Exterior - 300mm"]["layers"][0]["material"], g(Kind::Material, "Brick"));
    assert_eq!(file["elements"]["100000"]["graphics"]["template"], g(Kind::ViewTemplate, "Plans"));
    let up = &file["elements"]["100001"]["graphics"];
    assert_eq!(up["filters"][0]["filter"], g(Kind::ViewFilter, "Long"));
    assert_eq!(up["phase_filter"], g(Kind::PhaseFilter, "Show New"));
    let roofs = m.library.guid(&Kind::OptionSet, "Roofs").unwrap().to_string();
    assert_eq!(up["options"][&roofs], g(Kind::DesignOption, "Roofs: Pitched"));
    assert_eq!(file["option_sets"]["Roofs"]["primary"], g(Kind::DesignOption, "Roofs: Flat"));
    assert!(file["library"].as_object().is_some_and(|l| l.len() == m.library.len()));
    // Read back: the same project, names and all.
    let back = Model::from_json(&m.to_json().unwrap()).unwrap();
    assert_eq!(back, **m);
}

#[test]
fn a_rename_keeps_an_entrys_id_and_guid_and_changes_nothing_a_file_holds_but_its_name() {
    let (mut s, _) = project();
    let before: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let key = |m: &Model, kind: Kind, name: &str| (m.library.id(&kind, name), m.library.guid(&kind, name));
    let was = [
        key(&s.model, Kind::Material, "Brick"),
        key(&s.model, Kind::Parameter, "Fire Rating"),
        key(&s.model, Kind::Phase, "Survey"),
        key(&s.model, Kind::OptionSet, "Roofs"),
        key(&s.model, Kind::DesignOption, "Roofs: Flat"),
        key(&s.model, Kind::Family, "Sofa"),
    ];
    s.script(
        r#"material {"name": "Brick", "rename": "Clay brick"}
parameter {"name": "Fire Rating", "rename": "Fire Resistance"}
phase {"name": "Survey", "rename": "As Found"}
option {"set": "Roofs", "option": "Flat", "rename": "Level"}
option {"set": "Roofs", "rename": "Roof Studies"}
family {"name": "Sofa", "rename": "Couch"}"#,
    )
    .unwrap();
    let now = [
        key(&s.model, Kind::Material, "Clay brick"),
        key(&s.model, Kind::Parameter, "Fire Resistance"),
        key(&s.model, Kind::Phase, "As Found"),
        key(&s.model, Kind::OptionSet, "Roof Studies"),
        key(&s.model, Kind::DesignOption, "Roof Studies: Level"),
        key(&s.model, Kind::Family, "Couch"),
    ];
    assert_eq!(now, was);
    // What the elements, the types and the views hold is as it was: only the names, and the definitions keyed by them,
    // say otherwise.
    let after: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(after["elements"], before["elements"]);
    assert_eq!(after["types"], before["types"]);
}

#[test]
fn a_command_takes_a_library_entry_by_its_guid_wherever_it_takes_its_name_and_elements_show_their_types() {
    let mut s = Session::new();
    let exterior = s.model.library.guid(&Kind::Type("wall".into()), "Exterior - 300mm").unwrap();
    let w = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": exterior.to_string()}))["ids"][0].as_u64().unwrap();
    let Some(Element::Wall(wall)) = s.model.elements.get(w) else { panic!() };
    assert_eq!(wall.type_name, "Exterior - 300mm");
    let e = crate::element_json(&s.model, w);
    assert_eq!((e["type"].clone(), e["type_guid"].clone()), (json!("Exterior - 300mm"), json!(exterior.to_string())));
    let r = run(&mut s, "library", json!({"kind": "material", "name": "Brick"}));
    assert_eq!(r["count"], 1);
    assert_eq!(r["library"][0]["guid"], json!(s.model.library.guid(&Kind::Material, "Brick").unwrap().to_string()));
    // Select by ID finds the library too, which isn't selected; IDs of Selection names it.
    let brick = s.model.library.id(&Kind::Material, "Brick").unwrap();
    let r = run(&mut s, "select_by_id", json!({"text": format!("{brick} {w}")}));
    assert_eq!((r["ids"].clone(), r["library"].clone()), (json!([w]), json!([brick])));
    let r = run(&mut s, "ids_of", json!({"ids": [brick]}));
    assert_eq!((r["elements"][0]["category"].clone(), r["elements"][0]["name"].clone()), (json!("material"), json!("Brick")));
    // A name that is a GUID can't be told from one: refused.
    let e = s.execute("material", &json!({"name": "3f2a9c1e-0000-4000-8000-000000000001", "color": [1, 2, 3]})).unwrap_err();
    assert!(e.contains("a name isn't a GUID"), "{e}");
}

#[test]
fn a_file_from_before_library_guids_opens_with_them() {
    let (s, w) = project();
    // As format 1 wrote it: names where GUIDs are now, no library.
    let mut old = serde_json::to_value(&*s.model).unwrap();
    old["format"] = json!(1);
    old.as_object_mut().unwrap().remove("library");
    let m = Model::from_json(&old.to_string()).unwrap();
    assert!(m.library_in_step());
    let Some(Element::Wall(wall)) = m.elements.get(w) else { panic!() };
    assert_eq!((wall.type_name.as_str(), wall.params["Fire Rating"].as_str()), ("Exterior - 300mm", Some("EI 60")));
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn insert_from_file_matches_the_library_by_guid_then_by_name_and_brings_the_rest_with_its_guids() {
    // Two projects from one: this one, and the other, which renames a material, makes a type of its own made of it,
    // and a type of the name of one this project makes apart.
    let (base, _) = project();
    let mut here = Session::new();
    here.reset(Model::from_json(&base.model.to_json().unwrap()).unwrap(), None);
    let mut other = Session::new();
    other.reset(Model::from_json(&base.model.to_json().unwrap()).unwrap(), None);
    other
        .script(
            r#"material {"name": "Brick", "rename": "Red brick"}
type {"category": "wall", "name": "Thin", "layers": [{"material": "Red brick", "thickness": 90}]}
type {"category": "wall", "name": "Partition", "thickness": 110}"#,
        )
        .unwrap();
    let thin = run(&mut other, "wall", json!({"points": [[0, 2000], [4000, 2000]], "type": "Thin"}))["ids"][0].as_u64().unwrap();
    let inner = run(&mut other, "wall", json!({"points": [[0, 4000], [4000, 4000]], "type": "Partition"}))["ids"][0].as_u64().unwrap();
    // Here, a type of that name too, made apart: another GUID.
    run(&mut here, "type", json!({"category": "wall", "name": "Partition", "thickness": 100}));
    let path = temp("other");
    run(&mut other, "save", json!({"path": path}));
    let r = run(&mut here, "insert", json!({"path": path, "ids": [thin, inner]}));
    let _ = std::fs::remove_file(&path);
    let (m, o) = (&here.model, &other.model);
    let wall = Kind::Type("wall".into());
    // A material this project has by GUID is its own, under the name it has here: the type that came in is made of it.
    assert_eq!(m.library.guid(&Kind::Material, "Brick"), o.library.guid(&Kind::Material, "Red brick"));
    assert!(!m.materials.contains_key("Red brick"));
    assert_eq!(m.types.wall["Thin"].layers[0].material, "Brick");
    // A type it hasn't comes in, with its GUID.
    assert_eq!(m.library.guid(&wall, "Thin"), o.library.guid(&wall, "Thin"));
    assert!(r["library"].as_array().unwrap().contains(&json!("wall type: Thin")), "{r}");
    // One it has by name, though not by GUID, is this project's, as it is here.
    assert_ne!(m.library.guid(&wall, "Partition"), o.library.guid(&wall, "Partition"));
    assert_eq!(m.types.wall["Partition"].thickness, 100.0);
    let id = r["ids"][inner.to_string()].as_u64().unwrap();
    assert_eq!(crate::element_json(m, id)["thickness"], 100.0);
    assert_eq!(m.check(), Ok(()));
}

/// A family of a box, a type of it and a component of the type, its instance parameter of a material set, with a
/// plan symbol: the parts and the references a family makes.
fn boxed() -> (Session, Id) {
    let mut s = Session::new();
    run(
        &mut s,
        "family",
        json!({
            "name": "Box",
            "parameters": [{"name": "Side", "value": 600}, {"name": "Finish", "kind": "material", "instance": true, "value": "Timber"}],
            "types": {"Small": {"Side": 400}, "Large": {}},
            "forms": [
                {"profile": [[0, 0], ["Side", 0], ["Side", "Side"], [0, "Side"]], "to": "Side", "material": "Finish"},
                {"profile": [[0, 0], [100, 0], [100, 100], [0, 100]], "to": 50, "material": "Metal"}
            ],
        }),
    );
    let c = run(&mut s, "component", json!({"family": "Box", "type": "Small", "at": [0, 0], "params": {"Finish": "Brick"}}))["id"]
        .as_u64()
        .unwrap();
    (s, c)
}

#[test]
fn every_layer_form_and_symbol_and_every_family_type_and_parameter_has_an_id_and_a_guid() {
    let (s, _) = boxed();
    let m = &s.model;
    let parts: Vec<(Id, Part)> = m.parts_now();
    assert!(parts.iter().any(|(_, p)| p.what == "layer" && p.of == "wall type Exterior - 300mm" && p.index == 3));
    assert!(parts.iter().any(|(_, p)| p.what == "form" && p.of == "family Box" && p.index == 1));
    assert!(parts.iter().any(|(_, p)| p.what == "symbol"), "the door families' swings");
    let mut ids: Vec<Id> = parts.iter().map(|(id, _)| *id).collect();
    ids.extend(m.library.entries().map(|(id, _)| id));
    ids.extend(m.elements.keys());
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "one id space: no id twice");
    assert!(parts.iter().all(|(id, p)| *id >= FIRST_ID && !p.guid.is_nil() && m.library.part(*id) == Some(p)));
    for (kind, name) in [(Kind::FamilyType, "Box: Small"), (Kind::FamilyParameter, "Box: Finish")] {
        assert!(m.library.guid(&kind, name).is_some(), "{name}");
    }
    assert_eq!(m.elements.keys().next_back(), Some(&100_002), "elements still count up: the levels, then the component");
    assert_eq!(m.check(), Ok(()));
    let l = s.model.library.part(parts[0].0).unwrap().name();
    assert!(l.starts_with("layer 1 of "), "{l}");
}

#[test]
fn a_file_names_a_family_type_parameter_and_material_by_guid_and_its_parts_hold_theirs() {
    let (s, c) = boxed();
    let m = &s.model;
    let g = |kind: Kind, name: &str| json!(m.library.guid(&kind, name).unwrap().to_string());
    let file: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
    let comp = &file["elements"][c.to_string()];
    assert_eq!((comp["family"].clone(), comp["type"].clone()), (g(Kind::Family, "Box"), g(Kind::FamilyType, "Box: Small")));
    let finish = m.library.guid(&Kind::FamilyParameter, "Box: Finish").unwrap().to_string();
    assert_eq!(comp["params"][&finish], g(Kind::Material, "Brick"), "its instance value, keyed by the parameter's GUID: {comp}");
    let family = &file["families"]["Box"];
    assert_eq!(family["forms"][1]["material"], g(Kind::Material, "Metal"));
    assert_eq!(family["forms"][0]["material"], "Finish", "a family's own parameter, by name, as its formulas name it");
    assert_eq!(family["parameters"][1]["value"], g(Kind::Material, "Timber"));
    let form = m.families["Box"].forms[0].clone();
    assert_eq!((family["forms"][0]["id"].clone(), family["forms"][0]["guid"].clone()), (json!(form.id), json!(form.guid.to_string())));
    let back = Model::from_json(&m.to_json().unwrap()).unwrap();
    assert_eq!(back, **m);
}

#[test]
fn a_part_keeps_its_id_and_guid_where_it_stays_in_its_list_and_a_new_one_gets_its_own() {
    let (mut s, _) = boxed();
    let ident = |m: &Model| m.types.wall["Exterior - 300mm"].layers.iter().map(|l| (l.id, l.guid)).collect::<Vec<_>>();
    let before = ident(&s.model);
    // The layers given again, as an agent or the type dialog gives them: no ids. The plaster thicker, a layer more.
    let layers = json!([
        {"function": "finish", "material": "Brick", "thickness": 100},
        {"function": "thermal", "material": "Insulation", "thickness": 50},
        {"function": "structure", "material": "Concrete block", "thickness": 140},
        {"function": "finish", "material": "Plaster", "thickness": 15},
        {"function": "finish", "material": "Plaster", "thickness": 5}
    ]);
    run(&mut s, "type", json!({"category": "wall", "name": "Exterior - 300mm", "layers": layers}));
    let after = ident(&s.model);
    assert_eq!(after[..4], before[..], "each at its place keeps them");
    assert!(!before.contains(&after[4]) && after[4].0 >= FIRST_ID);
    // A family's forms, the same.
    let forms = |m: &Model| m.families["Box"].forms.iter().map(|f| (f.id, f.guid)).collect::<Vec<_>>();
    let was = forms(&s.model);
    let mut given = serde_json::to_value(&s.model.families["Box"].forms).unwrap();
    for f in given.as_array_mut().unwrap() {
        f.as_object_mut().unwrap().retain(|k, _| k != "id" && k != "guid");
    }
    given[1]["to"] = json!(80);
    run(&mut s, "family", json!({"name": "Box", "forms": given}));
    assert_eq!(forms(&s.model), was);
    // A type made from another's layers as `inspect` shows them, ids and all: a copy, with ids and GUIDs of its own.
    let copied = serde_json::to_value(&s.model.types.wall["Exterior - 300mm"].layers).unwrap();
    run(&mut s, "type", json!({"category": "wall", "name": "Copy", "layers": copied}));
    let copy: Vec<(Id, Guid)> = s.model.types.wall["Copy"].layers.iter().map(|l| (l.id, l.guid)).collect();
    assert!(copy.iter().all(|c| !after.contains(c) && !after.iter().any(|a| a.0 == c.0 || a.1 == c.1)), "{copy:?}");
    assert_eq!(ident(&s.model), after, "the original keeps its own");
    assert_eq!(s.model.check(), Ok(()));
}

#[test]
fn a_file_from_before_the_line_and_fill_styles_opens_with_every_part_keeping_its_id() {
    let mut s = Session::new();
    s.script(crate::sample::SAMPLE).unwrap();
    // Written as 0.8 writes a project: no line patterns or styles, fill patterns or region types, and so none of their
    // entries; the parts (layers, forms, symbols) hold ids below the entries that were there.
    let mut m = (*s.model).clone();
    (m.line_patterns, m.line_styles, m.fill_patterns, m.region_types) = Default::default();
    // The library made again, as 0.8 made it: the entries from the top down, and the parts right below the last of them.
    m.library = Default::default();
    m.forget_parts();
    m.sync_library();
    let mut file: Value = serde_json::from_str(&m.to_json().unwrap()).unwrap();
    for key in ["line_patterns", "line_styles", "fill_patterns", "region_types"] {
        file.as_object_mut().unwrap().remove(key);
    }
    file["format"] = json!(5);
    let (opened, left_out) = Model::read(&file.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let ids = |m: &Model| m.parts_now().into_iter().map(|(id, p)| (id, p.guid)).collect::<Vec<_>>();
    let (was, now) = (ids(&m), ids(&opened));
    assert!(was.len() > 100, "{} parts", was.len());
    let renumbered = was.iter().zip(&now).filter(|(a, b)| a != b).count();
    assert_eq!((renumbered, was.len()), (0, now.len()), "{renumbered} of {} parts have an id they did not have", was.len());
    // The entries the file lacked are there, below the parts, and nothing has an id twice.
    assert!(opened.library.id(&Kind::LineStyle, "Thin Lines").is_some());
    assert!(opened.library_in_step());
    let all: std::collections::BTreeSet<Id> = opened.library.entries().map(|(id, _)| id).chain(now.iter().map(|(id, _)| *id)).collect();
    assert_eq!(all.len(), opened.library.entries().count() + now.len());
}

#[test]
fn a_family_renamed_keeps_its_types_and_parameters_guids_and_what_uses_them_is_unchanged_in_a_file() {
    let (mut s, c) = boxed();
    let before: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    let ty = s.model.library.guid(&Kind::FamilyType, "Box: Small");
    let param = s.model.library.guid(&Kind::FamilyParameter, "Box: Finish");
    run(&mut s, "family", json!({"name": "Box", "rename": "Crate"}));
    assert_eq!(s.model.library.guid(&Kind::FamilyType, "Crate: Small"), ty);
    assert_eq!(s.model.library.guid(&Kind::FamilyParameter, "Crate: Finish"), param);
    let after: Value = serde_json::from_str(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(after["elements"][c.to_string()], before["elements"][c.to_string()]);
}

#[test]
fn insert_from_file_brings_a_family_with_its_types_parameters_and_forms_guids() {
    let (other, c) = boxed();
    let path = temp("family");
    let mut o = Session::new();
    o.reset((*other.model).clone(), None);
    run(&mut o, "save", json!({"path": path}));
    let mut here = Session::new();
    let r = run(&mut here, "insert", json!({"path": path, "ids": [c]}));
    let _ = std::fs::remove_file(&path);
    let (m, from) = (&here.model, &other.model);
    for (kind, name) in [(Kind::Family, "Box"), (Kind::FamilyType, "Box: Small"), (Kind::FamilyParameter, "Box: Finish")] {
        assert_eq!(m.library.guid(&kind, name), from.library.guid(&kind, name), "{name}: {r}");
    }
    let guids = |m: &Model| m.families["Box"].forms.iter().map(|f| f.guid).collect::<Vec<_>>();
    assert_eq!(guids(m), guids(from), "the forms keep theirs");
    let id = r["ids"][c.to_string()].as_u64().unwrap();
    let Some(Element::Component(comp)) = m.elements.get(id) else { panic!() };
    assert_eq!((comp.family.as_str(), comp.type_name.as_str(), comp.params["Finish"].as_str()), ("Box", "Small", Some("Brick")));
    assert_eq!(m.check(), Ok(()));
}

#[test]
fn select_by_id_finds_a_layer_and_ids_of_names_it() {
    let (mut s, _) = boxed();
    let layer = s.model.types.wall["Exterior - 300mm"].layers[1].clone();
    let r = run(&mut s, "select_by_id", json!({"text": layer.guid.to_string()}));
    assert_eq!(r["library"], json!([layer.id]));
    let r = run(&mut s, "ids_of", json!({"ids": [layer.id]}));
    assert_eq!(
        (r["elements"][0]["category"].clone(), r["elements"][0]["name"].clone()),
        (json!("layer"), json!("layer 2 of wall type Exterior - 300mm"))
    );
    let r = run(&mut s, "library", json!({"kind": "form", "name": "Box"}));
    assert_eq!(r["parts"].as_array().map(Vec::len), Some(2));
}

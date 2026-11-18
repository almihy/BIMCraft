//! A wall type as Revit's (file format 29): Edit Assembly's Core Boundary rows, Finish 2, Wraps and Structural Material, and
//! Type Properties' Function, Wrapping at Inserts and at Ends and Coarse Scale Fill, through the `type` command.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::model::{Core, Element, Function, Location, Model, WALL_FORMAT, WallFunction, Wrapping};
use crate::walls;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    let before = s.model.clone();
    let e = s.execute(id, &p).expect_err(&format!("{id} {p} should fail"));
    assert_eq!(*s.model, *before, "a refused {id} changes nothing");
    e
}

/// Render 20, insulation 60, sheathing 15 (a substrate), block 140, plaster 15: 250 mm, exterior side in.
fn layers() -> Value {
    json!([
        {"function": "finish", "thickness": 20, "wraps": true},
        {"function": "thermal", "thickness": 60},
        {"function": "substrate", "thickness": 15},
        {"function": "structure", "thickness": 140, "structural": true},
        {"function": "finish2", "thickness": 15, "wraps": true},
    ])
}

#[test]
fn a_types_core_lies_between_its_core_boundary_rows() {
    let mut s = Session::new();
    // None set: from its first structure layer to its last, as before.
    let made = run(&mut s, "type", json!({"category": "wall", "name": "W", "layers": layers()}));
    assert_eq!(made["type"]["core_layers"], json!([3, 4]));
    assert_eq!(s.model.types.wall["W"].core_span(), (95.0, 235.0));
    // The substrate taken into the core, as Edit Assembly's boundary row moved up one: two layers outside it on the
    // exterior side, one on the interior side.
    let made = run(&mut s, "type", json!({"category": "wall", "name": "W", "core": {"exterior": 2, "interior": 1}}));
    assert_eq!(made["type"]["core_layers"], json!([2, 4]));
    let t = &s.model.types.wall["W"];
    assert_eq!((t.core, t.core_span()), (Some(Core { exterior: 2, interior: 1 }), (80.0, 235.0)));
    // The location lines that name the core follow it.
    assert_eq!(
        [Location::ExteriorCore, Location::CoreCentre, Location::InteriorCore].map(|l| walls::depth(t, l)),
        [80.0, 157.5, 235.0]
    );
    let id = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "W", "location": "exterior-core"}))["ids"][0]
        .as_u64()
        .unwrap();
    let Some(Element::Wall(w)) = s.model.elements.get(id) else { panic!("no wall") };
    assert_eq!(walls::faces(&s.model, w), (80.0, -170.0));
    assert_eq!(walls::core_faces(&s.model, w), Some((0.0, -155.0)));
    // Kept when the type changes otherwise; cleared by null; new layers without it start from the structure again.
    run(&mut s, "type", json!({"category": "wall", "name": "W", "function": "interior"}));
    assert_eq!(s.model.types.wall["W"].core, Some(Core { exterior: 2, interior: 1 }));
    run(&mut s, "type", json!({"category": "wall", "name": "W", "core": null}));
    assert_eq!(s.model.types.wall["W"].core, None);
    run(&mut s, "type", json!({"category": "wall", "name": "W", "core": {"exterior": 1, "interior": 1}}));
    run(&mut s, "type", json!({"category": "wall", "name": "W", "layers": layers()}));
    assert_eq!(s.model.types.wall["W"].core, None);
}

#[test]
fn a_core_holds_a_layer_with_a_thickness() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "W", "layers": layers()}));
    let e = fails(&mut s, "type", json!({"category": "wall", "name": "W", "core": {"exterior": 3, "interior": 2}}));
    assert!(e.contains("it holds one layer at least"), "{e}");
    // A membrane of no thickness alone is no core.
    let thin = json!([{"function": "finish", "thickness": 20}, {"function": "membrane", "thickness": 0}, {"thickness": 100}]);
    let e = fails(&mut s, "type", json!({"category": "wall", "name": "M", "layers": thin, "core": {"exterior": 1, "interior": 1}}));
    assert!(e.contains("its core has no thickness"), "{e}");
    // A type without layers has no rows to set.
    let e = fails(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "core": {"exterior": 0, "interior": 0}}));
    assert!(e.contains("it holds one layer at least"), "{e}");
}

#[test]
fn a_layer_is_finish_1_or_finish_2_wraps_and_one_is_the_structural_material() {
    let mut s = Session::new();
    run(&mut s, "material", json!({"name": "Block"}));
    let mut ls = layers();
    ls[3]["material"] = json!("Block");
    let made = run(&mut s, "type", json!({"category": "wall", "name": "W", "layers": ls}));
    assert_eq!(made["type"]["structural_material"], "Block");
    let t = &s.model.types.wall["W"];
    assert_eq!(t.layers.iter().map(|l| l.function).collect::<Vec<_>>(), [
        Function::Finish,
        Function::Thermal,
        Function::Substrate,
        Function::Structure,
        Function::Finish2
    ]);
    assert_eq!(t.layers.iter().map(|l| l.wraps).collect::<Vec<_>>(), [true, false, false, false, true]);
    // Revit's priorities: a lower number runs through a higher one; a membrane has none.
    assert_eq!(Function::ALL.map(Function::priority), [Some(1), Some(2), Some(3), Some(4), Some(5), None]);
    // `finish1` reads as `finish`.
    run(&mut s, "type", json!({"category": "wall", "name": "F", "layers": [{"function": "finish1", "thickness": 10}, {"thickness": 90}]}));
    assert_eq!(s.model.types.wall["F"].layers[0].function, Function::Finish);
    // One structural material at most; only a wall's layer wraps.
    let mut two = layers();
    two[2]["structural"] = json!(true);
    let e = fails(&mut s, "type", json!({"category": "wall", "name": "W", "layers": two}));
    assert!(e.contains("2 layers are its structural material"), "{e}");
    let e = fails(&mut s, "type", json!({"category": "floor", "name": "Slab", "layers": [{"thickness": 200, "wraps": true}]}));
    assert!(e.contains("only a wall's layer wraps"), "{e}");
}

#[test]
fn a_type_has_revits_function_wrapping_and_coarse_fill() {
    let mut s = Session::new();
    let t = &s.model.types.wall["Generic - 200mm"];
    assert_eq!((t.function, t.wrap_inserts, t.wrap_ends, &t.coarse_fill, t.coarse_color), (
        WallFunction::Exterior,
        Wrapping::None,
        Wrapping::None,
        &None,
        [0, 0, 0]
    ));
    let pattern = s.model.fill_patterns.keys().next().expect("the starter project has fill patterns").clone();
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Generic - 200mm", "function": "Core-shaft", "wrap_inserts": "both", "wrap_ends": "interior",
               "coarse_fill": pattern, "coarse_color": [128, 128, 128]}),
    );
    let t = &s.model.types.wall["Generic - 200mm"];
    assert_eq!((t.function, t.wrap_inserts, t.wrap_ends, t.coarse_fill.as_deref(), t.coarse_color, t.thickness), (
        WallFunction::CoreShaft,
        Wrapping::Both,
        Wrapping::Interior,
        Some(pattern.as_str()),
        [128, 128, 128],
        200.0
    ));
    // Each kept when not given; Revit's "Do not wrap" and null clear.
    run(&mut s, "type", json!({"category": "wall", "name": "Generic - 200mm", "wrap_inserts": "Do not wrap", "coarse_fill": null}));
    let t = &s.model.types.wall["Generic - 200mm"];
    assert_eq!((t.function, t.wrap_inserts, t.wrap_ends, &t.coarse_fill), (WallFunction::CoreShaft, Wrapping::None, Wrapping::Interior, &None));
    for (p, why) in [
        (json!({"function": "load-bearing"}), "`function` is interior, exterior, foundation, retaining, soffit or core-shaft"),
        (json!({"wrap_ends": "both"}), "wrapping at ends is none, exterior or interior, not both"),
        (json!({"wrap_inserts": "outside"}), "`wrap_inserts` is none, exterior, interior or both"),
        (json!({"coarse_fill": "No such pattern"}), "fill pattern `No such pattern` isn't in the project"),
        (json!({"coarse_color": [0, 0, 300]}), "`coarse_color` is [red, green, blue]"),
    ] {
        let mut p = p;
        p["category"] = json!("wall");
        p["name"] = json!("Generic - 200mm");
        let e = fails(&mut s, "type", p);
        assert!(e.contains(why), "{e}");
    }
}

#[test]
fn a_file_is_written_as_29_only_when_a_type_needs_it_and_reads_back_the_same() {
    let mut s = Session::new();
    run(&mut s, "type", json!({"category": "wall", "name": "W", "layers": [{"function": "finish", "thickness": 20}, {"thickness": 180}]}));
    assert!(s.model.written_format() < WALL_FORMAT, "nothing of 29's: an older build opens it");
    for p in [
        json!({"core": {"exterior": 0, "interior": 0}}),
        json!({"function": "interior"}),
        json!({"wrap_ends": "exterior"}),
        json!({"layers": [{"function": "finish2", "thickness": 20}, {"thickness": 180}]}),
        json!({"layers": [{"function": "finish", "thickness": 20, "wraps": true}, {"thickness": 180}]}),
        json!({"layers": [{"function": "finish", "thickness": 20}, {"thickness": 180, "structural": true}]}),
    ] {
        let mut t = Session::new();
        let mut p = p;
        p["category"] = json!("wall");
        p["name"] = json!("W");
        if p.get("layers").is_none() {
            p["layers"] = json!([{"function": "finish", "thickness": 20}, {"thickness": 180}]);
        }
        run(&mut t, "type", p.clone());
        assert_eq!(t.model.written_format(), WALL_FORMAT, "{p}");
        let file = t.model.to_json().unwrap();
        let (back, left_out) = Model::read(&file).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert_eq!((&back.types.wall["W"], left_out.as_slice()), (&t.model.types.wall["W"], &[][..]), "{p}");
    }
}

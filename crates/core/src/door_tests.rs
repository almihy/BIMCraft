//! Doors in plan as Revit's door families draw them (a leaf a panel of its type's thickness, its swing, each way of
//! opening), and which way a door opens said back: its From Room and To Room, its hinge and its swing, checked against
//! the plan it draws.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::model::Id;
use crate::prim::Prim;
use crate::render::{self, Drawing};

fn run(s: &mut Session, c: &str, p: Value) -> Value {
    s.execute(c, &p).unwrap_or_else(|e| panic!("{c} {p}: {e}"))
}

fn id_of(v: &Value) -> Id {
    v["id"].as_u64().unwrap_or_else(|| panic!("no id in {v}"))
}

/// An 8 x 6 m box of walls (south, east, north, west, drawn anticlockwise), split by a wall at x 4000 drawn north, and
/// two rooms: WC to the west, Hall to the east. The ids of the walls, the dividing one last.
fn rooms(s: &mut Session) -> Vec<Id> {
    let r = run(s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
    let mut ids: Vec<Id> = r["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64).collect()).unwrap_or_default();
    let mid = run(s, "wall", json!({"points": [[4000, 0], [4000, 6000]]}));
    ids.push(mid["ids"][0].as_u64().unwrap_or_default());
    run(s, "room", json!({"point": [2000, 3000], "name": "WC", "number": "1"}));
    run(s, "room", json!({"point": [6000, 3000], "name": "Hall", "number": "2"}));
    ids
}

fn plan(s: &Session) -> Drawing {
    render::plan(&s.model, 100000, 4000, 3000)
}

/// The outlined panels `id` draws, in model mm.
fn panels(d: &Drawing, id: Id) -> Vec<Vec<P2>> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, closed: true, .. } if *o == Some(id) => Some(pts.iter().map(|q| d.view.mm(*q)).collect()),
            _ => None,
        })
        .collect()
}

/// The arcs `id` draws (open lines of more than two points), in model mm.
fn arcs(d: &Drawing, id: Id) -> Vec<Vec<P2>> {
    d.prims
        .iter()
        .zip(&d.owners)
        .filter_map(|(p, o)| match p {
            Prim::Line { pts, closed: false, .. } if *o == Some(id) && pts.len() > 2 => Some(pts.iter().map(|q| d.view.mm(*q)).collect()),
            _ => None,
        })
        .collect()
}

/// A panel's two side lengths, the shorter first.
fn sides(ring: &[P2]) -> (f64, f64) {
    let (a, b) = (ring[0].dist(ring[1]), ring[1].dist(ring[2]));
    (a.min(b), a.max(b))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1.0
}

#[test]
fn a_doors_leaf_is_drawn_as_a_panel_of_revits_default_thickness_with_its_swing() {
    let mut s = Session::new();
    rooms(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "type": "Single 900 x 2100"})));
    let d = plan(&s);
    let p = panels(&d, door);
    assert_eq!(p.len(), 1, "one leaf, a closed panel: {p:?}");
    assert_eq!(p[0].len(), 4);
    let (thin, long) = sides(&p[0]);
    assert!(near(thin, 50.0) && near(long, 900.0), "a 50 mm leaf as wide as the door: {thin} x {long}");
    assert_eq!(arcs(&d, door).len(), 1, "its swing");
    assert_eq!(crate::element_json(&s.model, door)["thickness"], json!(50.0));
}

#[test]
fn a_door_types_thickness_is_its_leafs_in_plan_3d_and_ifc() {
    let mut s = Session::new();
    rooms(&mut s);
    run(&mut s, "type", json!({"category": "door", "name": "Thick", "width": 1000, "height": 2100, "thickness": 40}));
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "type": "Thick"})));
    let (thin, long) = sides(&panels(&plan(&s), door)[0]);
    assert!(near(thin, 40.0) && near(long, 1000.0), "{thin} x {long}");
    assert_eq!(s.model.types.door["Thick"].thickness, Some(40.0));
    // Changing the type's width keeps its thickness.
    run(&mut s, "type", json!({"category": "door", "name": "Thick", "width": 1100}));
    assert_eq!(s.model.types.door["Thick"].thickness, Some(40.0));
    // In 3D, a box 40 thick.
    let scene = crate::view3d::solids(&s.model);
    let leaf = scene.iter().find_map(|x| match x {
        crate::view3d::Solid::Box { owner, c, .. } if *owner == door => Some(*c),
        _ => None,
    });
    assert_eq!(leaf, Some((-20.0, 20.0)));
    // In IFC, the door's panel profile 40 across.
    let ifc = crate::ifc::export(&s.model);
    assert!(ifc.contains(",1100.,40.)"), "a 1100 x 40 rectangle profile");
    // A window has no leaf to be thick.
    let e = s.execute("type", &json!({"category": "window", "name": "W", "width": 1000, "height": 1000, "thickness": 40})).unwrap_err();
    assert!(e.contains("thickness"), "{e}");
}

#[test]
fn every_way_of_opening_draws_its_own_leaves() {
    let mut s = Session::new();
    rooms(&mut s);
    // (operation, panels, arcs, each panel's length along or open)
    let cases = [
        ("swing", 1, 1, vec![1200.0]),
        ("double-swing", 2, 2, vec![600.0, 600.0]),
        ("sliding", 2, 0, vec![650.0, 650.0]),
        ("double-sliding", 2, 0, vec![600.0, 600.0]),
        ("folding", 2, 0, vec![600.0, 600.0]),
        ("fixed", 1, 0, vec![1200.0]),
    ];
    for (i, (op, n, arcs_n, lengths)) in cases.iter().enumerate() {
        let name = format!("T {op}");
        run(&mut s, "type", json!({"category": "door", "name": name, "width": 1200, "height": 2100, "operation": op}));
        let door = id_of(&run(&mut s, "door", json!({"at": [1000 + 1500 * i as i64 % 3000, if i < 2 { 0 } else { 6000 }], "type": name})));
        let d = plan(&s);
        let p = panels(&d, door);
        assert_eq!(p.len(), *n, "{op}: {p:?}");
        assert_eq!(arcs(&d, door).len(), *arcs_n, "{op}");
        let mut got: Vec<f64> = p.iter().map(|r| sides(r).1).collect();
        got.sort_by(f64::total_cmp);
        assert!(got.iter().zip(lengths).all(|(a, b)| near(*a, *b)), "{op}: {got:?}");
        assert!(p.iter().all(|r| near(sides(r).0, 50.0)), "{op}: 50 thick");
        run(&mut s, "delete", json!({"ids": [door]}));
    }
}

#[test]
fn a_doors_leaf_and_swing_show_at_coarse_as_revits_do() {
    let mut s = Session::new();
    rooms(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0]})));
    let window = id_of(&run(&mut s, "window", json!({"at": [6000, 0]})));
    run(&mut s, "detail", json!({"view": 100000, "detail": "coarse"}));
    let d = plan(&s);
    assert_eq!((panels(&d, door).len(), arcs(&d, door).len()), (1, 1));
    // A window at coarse is its gap and jambs only.
    let glass = d.prims.iter().zip(&d.owners).filter(|(p, o)| **o == Some(window) && matches!(p, Prim::Line { pts, .. } if pts.len() == 2));
    assert_eq!(glass.count(), 2, "its two jambs");
}

/// The room named `name`'s JSON, or null.
fn room_name(v: &Value) -> Option<&str> {
    v["name"].as_str()
}

#[test]
fn a_door_says_which_room_it_opens_into_and_where_its_hinge_is_as_the_plan_draws_it() {
    let mut s = Session::new();
    let walls = rooms(&mut s);
    let mid = walls[4];
    // The dividing wall runs north, so its left is the west, the WC.
    for (ff, fh) in [(false, false), (true, false), (false, true), (true, true)] {
        let door = id_of(&run(
            &mut s,
            "door",
            json!({"wall": mid, "offset": 3000, "type": "Single 900 x 2100", "flip_facing": ff, "flip_hand": fh}),
        ));
        let e = crate::element_json(&s.model, door);
        let (into, from) = if ff { ("Hall", "WC") } else { ("WC", "Hall") };
        assert_eq!((room_name(&e["to_room"]), room_name(&e["from_room"])), (Some(into), Some(from)), "flips {ff} {fh}: {e}");
        assert_eq!(e["swing"], json!(if ff { "right" } else { "left" }));
        let t = s.model.thickness(s.model.wall(s.model.guid(mid)).unwrap());
        let x = if ff { 4000.0 + t / 2.0 } else { 4000.0 - t / 2.0 };
        let y = if fh { 3450.0 } else { 2550.0 };
        assert_eq!(e["hinge"], json!([x, y]), "flips {ff} {fh}");
        // The plan agrees: its leaf is hung at the hinge, and its arc sweeps through the room it says it opens into.
        let d = plan(&s);
        let leaf = &panels(&d, door)[0];
        let hinge = P2(x, y);
        assert!(leaf.iter().any(|q| q.dist(hinge) < 1.0), "the leaf at the hinge {hinge:?}: {leaf:?}");
        let arc = &arcs(&d, door)[0];
        let swept = arc[arc.len() / 2];
        let rooms = crate::rooms::Rooms::new(&s.model);
        let level = s.model.levels()[0].0;
        let at = rooms.room_at(s.model.guid(level), swept).and_then(|r| s.model.elements.get(r).cloned());
        assert!(matches!(at, Some(crate::model::Element::Room(r)) if r.name == into), "the arc at {swept:?} in {into}");
        run(&mut s, "delete", json!({"ids": [door]}));
    }
}

#[test]
fn a_door_in_an_outside_wall_opens_from_or_to_outside() {
    let mut s = Session::new();
    rooms(&mut s);
    // The south wall runs east, so its left is the north: in the house, the WC.
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0]})));
    let e = crate::element_json(&s.model, door);
    assert_eq!((room_name(&e["to_room"]), e["from_room"].clone()), (Some("WC"), Value::Null), "{e}");
    let flipped = id_of(&run(&mut s, "door", json!({"at": [6000, 0], "flip_facing": true})));
    let e = crate::element_json(&s.model, flipped);
    assert_eq!((e["to_room"].clone(), room_name(&e["from_room"])), (Value::Null, Some("Hall")), "{e}");
    // `elements` says the same.
    let all = run(&mut s, "elements", json!({"category": "doors"}));
    let row = all["elements"].as_array().unwrap().iter().find(|r| r["id"] == json!(door)).unwrap().clone();
    assert_eq!(row["to_room"]["number"], json!("1"));
    assert!(row["to_room"]["id"].is_u64() && row["to_room"]["guid"].is_string());
}

#[test]
fn a_windows_to_room_is_the_side_it_faces_its_exterior_as_revits() {
    let mut s = Session::new();
    rooms(&mut s);
    // Revit gives a window's To Room on its exterior side, the side it faces, and its From Room on the other: a window in
    // an outside wall faced out schedules its room as From Room. This one faces the south wall's left (the north, the
    // WC), so its To Room is the WC and its From Room outside.
    let w = id_of(&run(&mut s, "window", json!({"at": [2000, 0]})));
    let e = crate::element_json(&s.model, w);
    assert_eq!((room_name(&e["to_room"]), e["from_room"].clone()), (Some("WC"), Value::Null), "{e}");
    assert!(e.get("hinge").is_none() && e.get("swing").is_none());
    // Faced out (flipped, to the south): its room is its From Room.
    let w = id_of(&run(&mut s, "window", json!({"at": [6000, 0], "flip_facing": true})));
    let e = crate::element_json(&s.model, w);
    assert_eq!((e["to_room"].clone(), room_name(&e["from_room"])), (Value::Null, Some("Hall")), "{e}");
}

#[test]
fn a_door_schedule_lists_from_room_and_to_room() {
    let mut s = Session::new();
    let walls = rooms(&mut s);
    run(&mut s, "door", json!({"wall": walls[4], "offset": 3000, "mark": "D1"}));
    run(&mut s, "door", json!({"at": [6000, 0], "mark": "D2", "flip_facing": true}));
    let fields = ["Mark", "From Room: Number", "From Room: Name", "To Room: Number", "To Room: Name"];
    let r = run(&mut s, "schedule", json!({"name": "Doors by room", "category": "doors", "fields": fields, "sort": ["Mark"]}));
    assert_eq!(r["rows"], json!([["D1", "2", "Hall", "1", "WC"], ["D2", "2", "Hall", null, null]]), "{r}");
}

/// A door's or window's From Room, To Room and the room it swings into, by name.
fn between(s: &Session, id: Id) -> (Option<String>, Option<String>, Option<String>) {
    let e = crate::element_json(&s.model, id);
    let name = |k: &str| room_name(&e[k]).map(String::from);
    (name("from_room"), name("to_room"), name("swings_into"))
}

fn named(a: Option<&str>, b: Option<&str>, c: Option<&str>) -> (Option<String>, Option<String>, Option<String>) {
    (a.map(String::from), b.map(String::from), c.map(String::from))
}

#[test]
fn a_doors_from_and_to_room_are_set_as_it_is_placed_and_kept_through_its_flips_as_revits() {
    let mut s = Session::new();
    let walls = rooms(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"wall": walls[4], "offset": 3000})));
    // Placed swinging into the WC, the dividing wall's left.
    assert_eq!(between(&s, door), named(Some("Hall"), Some("WC"), Some("WC")));
    // Flipped to swing into the Hall: its rooms stay as placed, as Revit's; what it swings into is said apart.
    run(&mut s, "flip", json!({"ids": [door]}));
    assert_eq!(between(&s, door), named(Some("Hall"), Some("WC"), Some("Hall")));
    run(&mut s, "modify", json!({"id": door, "flip_facing": false}));
    assert_eq!(between(&s, door), named(Some("Hall"), Some("WC"), Some("WC")));
    // Flip From/To Room swaps them, and nothing else.
    let before = s.model.elements.get(door).cloned();
    run(&mut s, "flip_from_to_room", json!({"ids": [door]}));
    assert_eq!(between(&s, door), named(Some("WC"), Some("Hall"), Some("WC")));
    match (before, s.model.elements.get(door)) {
        (Some(crate::model::Element::Door(a)), Some(crate::model::Element::Door(b))) => {
            assert_eq!((a.flip_facing, a.flip_hand, a.offset), (b.flip_facing, b.flip_hand, b.offset));
        }
        other => panic!("{other:?}"),
    }
    // Its wall turned round: the door stays where it is, and so do its rooms.
    run(&mut s, "flip", json!({"ids": [walls[4]]}));
    assert_eq!(between(&s, door), named(Some("WC"), Some("Hall"), Some("WC")));
    // Undone, as any command.
    run(&mut s, "undo", json!({}));
    run(&mut s, "undo", json!({}));
    assert_eq!(between(&s, door), named(Some("Hall"), Some("WC"), Some("WC")));
    // A window's too; anything else is refused.
    let w = id_of(&run(&mut s, "window", json!({"at": [2000, 6000]})));
    let (from, to, _) = between(&s, w);
    run(&mut s, "flip_from_to_room", json!({"ids": [w]}));
    assert_eq!(between(&s, w), (to, from, None));
    let e = s.execute("flip_from_to_room", &json!({"ids": [walls[0]]})).unwrap_err();
    assert!(e.contains("isn't a door or a window"), "{e}");
}

#[test]
fn a_door_from_before_from_and_to_room_were_kept_has_them_from_its_swing() {
    let mut s = Session::new();
    rooms(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [6000, 0], "flip_facing": true})));
    let mut v = serde_json::to_value(&s.model).unwrap();
    v["format"] = json!(19);
    let e = v["elements"][door.to_string()].as_object_mut().unwrap();
    assert!(e.remove("to_room_side").is_some(), "kept in the file: {e:?}");
    let (m, left_out) = crate::model::Model::read(&v.to_string()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    // It swung out of the house, to the south wall's right: its To Room is outside, its From Room the Hall.
    match m.elements.get(door) {
        Some(crate::model::Element::Door(o)) => assert_eq!(o.to_room_side, crate::model::Side::Right),
        other => panic!("{other:?}"),
    }
    assert_eq!(m.format, crate::model::FORMAT);
}

#[test]
fn a_project_is_written_as_format_20_only_when_a_doors_rooms_are_not_where_its_swing_puts_them() {
    use crate::model::{IMAGES_FORMAT, Model, PREVIOUS_FORMAT};
    let mut s = Session::new();
    rooms(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [6000, 0]})));
    let format = |s: &Session| serde_json::from_str::<Value>(&s.model.to_json().unwrap()).unwrap()["format"].clone();
    // Its To Room where it swings: a build that reads up to 19 gives it back from the swing, so 19 holds it.
    assert_eq!(format(&s), json!(PREVIOUS_FORMAT));
    let (m, left_out) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert!(left_out.is_empty(), "{left_out:?}");
    let rooms_of = |m: &Model| match m.elements.get(door) {
        Some(crate::model::Element::Door(o)) => o.to_room_side,
        other => panic!("{other:?}"),
    };
    assert_eq!(rooms_of(&m), rooms_of(&s.model));
    // Flipped, it keeps its rooms against its swing: only 20 (or later) holds that, and it reads back as it was.
    run(&mut s, "flip", json!({"ids": [door]}));
    assert_eq!(format(&s), json!(IMAGES_FORMAT));
    let (m, _) = Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!(rooms_of(&m), rooms_of(&s.model));
    // Its rooms flipped back to its swing, 19 again.
    run(&mut s, "flip_from_to_room", json!({"ids": [door]}));
    assert_eq!(format(&s), json!(PREVIOUS_FORMAT));
}

#[test]
fn from_and_to_room_are_the_rooms_of_the_projects_last_phase_as_revits() {
    let mut s = Session::new();
    let walls = rooms(&mut s);
    // The WC was there before and comes down in New Construction; a Bath takes its place.
    let wc = s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Room(r) if r.name == "WC")).map(|(id, _)| *id);
    let wc = wc.unwrap();
    run(&mut s, "modify", json!({"id": wc, "params": {"Phase Created": "Existing", "Phase Demolished": "New Construction"}}));
    let bath = id_of(&run(&mut s, "room", json!({"point": [2000, 3000], "name": "Bath", "number": "3"})));
    run(&mut s, "modify", json!({"id": bath, "params": {"Phase Created": "New Construction"}}));
    let door = id_of(&run(&mut s, "door", json!({"wall": walls[4], "offset": 3000})));
    assert_eq!(between(&s, door), named(Some("Hall"), Some("Bath"), Some("Bath")));
    // Each room has its own phase's space: the Bath is bounded, not the WC's redundant room.
    let e = crate::element_json(&s.model, bath);
    assert_eq!((e["enclosed"].clone(), e.get("shares_space_with").cloned()), (json!(true), None), "{e}");
    // Two rooms of the same phase still share it.
    let twin = id_of(&run(&mut s, "room", json!({"point": [2500, 3000], "name": "Twin", "number": "4"})));
    run(&mut s, "modify", json!({"id": twin, "params": {"Phase Created": "New Construction"}}));
    let e = crate::element_json(&s.model, twin);
    assert_eq!(e["shares_space_with"], json!(bath), "{e}");
}

#[test]
fn a_doors_name_and_a_tag_of_its_mark_never_look_for_its_rooms() {
    let mut s = Session::new();
    rooms(&mut s);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "mark": "D7"})));
    let tag = run(&mut s, "tag", json!({"ids": [door]}))["ids"][0].as_u64().unwrap();
    let before = crate::rooms::openings_looked_up();
    let crate::model::Element::Tag(t) = s.model.elements.get(tag).cloned().unwrap() else { panic!() };
    assert_eq!(s.model.tag_text(&t), "D7");
    assert_eq!(crate::inquiry::names(&s.model, door).map(|n| n.name), Some("Single 900 x 2100".into()));
    assert_eq!(crate::json::element_json_bare(&s.model, door)["mark"], json!("D7"));
    crate::render::plan(&s.model, 100000, 4000, 3000);
    assert_eq!(crate::rooms::openings_looked_up(), before, "no room looked for");
    // What asks for them finds them.
    assert_eq!(crate::schedule::field_text(&s.model, door, "To Room: Name").as_deref(), Some("WC"));
    assert!(crate::rooms::openings_looked_up() > before);
}

#[test]
fn a_doors_rooms_are_found_past_a_lining_wall_alongside_its_own() {
    let mut s = Session::new();
    let walls = rooms(&mut s);
    // A 300 mm wall against the dividing wall's east face, from its face out: its centre line 250 mm from the dividing
    // wall's, past where the rooms were looked for.
    let t = s.model.thickness(s.model.wall(s.model.guid(walls[4])).unwrap());
    let x = 4000.0 + t / 2.0 + 150.0;
    run(&mut s, "wall", json!({"points": [[x, 0], [x, 6000]], "type": "Exterior - 300mm"}));
    let door = id_of(&run(&mut s, "door", json!({"wall": walls[4], "offset": 3000, "flip_facing": true})));
    assert_eq!(between(&s, door), named(Some("WC"), Some("Hall"), Some("Hall")));
}

/// Panels' points, rounded to the millimetre and sorted: a drawing to compare with another.
fn shape(panels: &[Vec<P2>]) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> = panels.iter().flatten().map(|p| (p.0.round() as i64, p.1.round() as i64)).collect();
    v.sort_unstable();
    v
}

#[test]
fn every_way_of_opening_lies_on_the_side_it_opens_to_and_mirrors_with_its_flips() {
    let mut s = Session::new();
    let walls = rooms(&mut s);
    // In the south wall (along y 0, its left the north), centred at x 2000: 1200 wide, hinged toward the west jamb.
    let t = s.model.thickness(s.model.wall(s.model.guid(walls[0])).unwrap());
    for op in ["swing", "double-swing", "sliding", "double-sliding", "folding", "fixed"] {
        let name = format!("M {op}");
        run(&mut s, "type", json!({"category": "door", "name": name, "width": 1200, "height": 2100, "operation": op}));
        let draw = |s: &mut Session, ff: bool, fh: bool| {
            let door = id_of(&run(s, "door", json!({"at": [2000, 0], "type": name, "flip_facing": ff, "flip_hand": fh})));
            let p = panels(&plan(s), door);
            run(s, "delete", json!({"ids": [door]}));
            p
        };
        let plain = draw(&mut s, false, false);
        // Flipped hand, it is its mirror image across the door's middle; flipped facing, across the wall's centre line.
        let across_middle: Vec<Vec<P2>> = plain.iter().map(|r| r.iter().map(|p| P2(4000.0 - p.0, p.1)).collect()).collect();
        assert_eq!(shape(&draw(&mut s, false, true)), shape(&across_middle), "{op}: flip hand");
        let across_wall: Vec<Vec<P2>> = plain.iter().map(|r| r.iter().map(|p| P2(p.0, -p.1)).collect()).collect();
        assert_eq!(shape(&draw(&mut s, true, false)), shape(&across_wall), "{op}: flip facing");
        let ys: Vec<f64> = plain.iter().flatten().map(|p| p.1).collect();
        let (lo, hi) = (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
        match op {
            // Open, out of the wall into the room it swings to, never into the wall or behind it.
            "swing" | "double-swing" => assert!(lo > t / 2.0 - 1.0 && hi > 500.0, "{op}: {lo} to {hi}"),
            // Folded out to the north, its knee well past the face.
            "folding" => assert!(hi > t / 2.0 + 300.0 && lo > -1.0, "{op}: {lo} to {hi}"),
            // The sliding leaf over the free (east) half on the north of the middle, the fixed one on the south.
            "sliding" => {
                let east = plain.iter().find(|r| r.iter().any(|p| near(p.0, 2600.0))).unwrap();
                let west = plain.iter().find(|r| r.iter().any(|p| near(p.0, 1400.0))).unwrap();
                assert!(east.iter().all(|p| p.1 > -1.0) && west.iter().all(|p| p.1 < 1.0), "{op}: {plain:?}");
            }
            // In the wall, shut.
            _ => assert!(lo > -t / 2.0 && hi < t / 2.0, "{op}: {lo} to {hi}"),
        }
    }
}

#[test]
fn a_door_in_a_curved_wall_or_off_its_location_line_says_its_rooms_and_hinge_as_the_plan_draws_them() {
    // A compound wall drawn on its exterior finish face, and a curved one bulging east; each splits the box.
    for (wall, offset) in [
        (json!({"points": [[4000, 0], [4000, 6000]], "type": "Exterior - 300mm", "location": "exterior-finish"}), 3000.0),
        (json!({"points": [[4000, 0], [4000, 6000]], "arc": 90}), 3000.0),
    ] {
        let mut s = Session::new();
        let r = run(&mut s, "wall", json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true}));
        assert!(r["ids"].is_array());
        let mid = run(&mut s, "wall", wall.clone())["ids"][0].as_u64().unwrap();
        run(&mut s, "room", json!({"point": [1000, 3000], "name": "WC", "number": "1"}));
        run(&mut s, "room", json!({"point": [7000, 3000], "name": "Hall", "number": "2"}));
        for (ff, fh) in [(false, false), (true, true)] {
            let door = id_of(&run(&mut s, "door", json!({"wall": mid, "offset": offset, "flip_facing": ff, "flip_hand": fh})));
            let into = if ff { "Hall" } else { "WC" };
            let other = if ff { "WC" } else { "Hall" };
            assert_eq!(between(&s, door), named(Some(other), Some(into), Some(into)), "{wall} flips {ff} {fh}");
            // The hinge lies on the face it swings to, at a jamb, and the drawn leaf hangs there.
            let e = crate::element_json(&s.model, door);
            let hinge = P2(e["hinge"][0].as_f64().unwrap(), e["hinge"][1].as_f64().unwrap());
            let w = s.model.wall(s.model.guid(mid)).unwrap();
            let (left, right) = crate::walls::faces(&s.model, w);
            let axis = crate::walls::Axis::of(w);
            let (along, off) = axis.project(hinge);
            assert!(near(off, if ff { right } else { left }), "{wall}: on the face it swings to: {off}");
            assert!(near((along - offset).abs(), 450.0), "{wall}: at a jamb: {along}");
            assert!(near(along, if fh { offset + 450.0 } else { offset - 450.0 }), "{wall}: its hand");
            let leaf = &panels(&plan(&s), door)[0];
            assert!(leaf.iter().any(|q| q.dist(hinge) < 1.0), "{wall}: the leaf at {hinge:?}: {leaf:?}");
            run(&mut s, "delete", json!({"ids": [door]}));
        }
    }
}

#[test]
fn a_door_types_thickness_clears_back_to_revits_and_no_window_type_holds_one() {
    let mut s = Session::new();
    rooms(&mut s);
    run(&mut s, "type", json!({"category": "door", "name": "Thick", "width": 1000, "height": 2100, "thickness": 40}));
    run(&mut s, "type", json!({"category": "door", "name": "Thick", "thickness": null}));
    assert_eq!(s.model.types.door["Thick"].thickness, None);
    let door = id_of(&run(&mut s, "door", json!({"at": [2000, 0], "type": "Thick"})));
    assert_eq!(crate::element_json(&s.model, door)["thickness"], json!(50.0));
    // A window type's thickness is refused wherever it comes from: a command, or a file.
    let mut m = (*s.model).clone();
    if let Some(t) = m.types.window.values_mut().next() {
        t.thickness = Some(40.0);
    }
    let e = m.check().unwrap_err();
    assert!(e.contains("window type") && e.contains("thickness"), "{e}");
}

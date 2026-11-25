use std::sync::Arc;

use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{
    Block, Circle, Common, Drawing as Doc, Entity, EntityKind, HAlign, Handle, Insert, Layout, Line, Linetype, Space, Text, VAlign,
};
use cadcraft_index::Opened;
use serde_json::{Value, json};

use super::*;

fn on(layer: &str) -> Common {
    Common { layer: layer.into(), ..Common::default() }
}

fn line(a: (f64, f64), b: (f64, f64)) -> EntityKind {
    EntityKind::Line(Line { a: Vec3::new(a.0, a.1, 0.0), b: Vec3::new(b.0, b.1, 0.0) })
}

fn circle(c: (f64, f64), r: f64) -> EntityKind {
    EntityKind::Circle(Circle { center: Vec3::new(c.0, c.1, 0.0), radius: r })
}

fn text(at: (f64, f64), words: &str) -> EntityKind {
    EntityKind::Text(Text {
        insert: Vec3::new(at.0, at.1, 0.0),
        align_pt: None,
        height: 200.0,
        value: words.into(),
        rotation: 0.0,
        width_factor: 1.0,
        oblique: 0.0,
        style: "Standard".into(),
        halign: HAlign::default(),
        valign: VAlign::default(),
    })
}

fn placed(block: &str, at: (f64, f64)) -> EntityKind {
    EntityKind::Insert(Insert { block: block.into(), insert: Vec3::new(at.0, at.1, 0.0), ..Insert::default() })
}

fn block(d: &mut Doc, name: &str, items: Vec<(Common, EntityKind)>) {
    let mut b = Block::new(name);
    for (common, kind) in items {
        b.entities.push(Entity { handle: d.new_handle(), common, kind });
    }
    d.blocks.insert(name.into(), Arc::new(b));
}

/// A room drawn in mm with no order: two walls on WALL and one on layer 0, a table and a chair (a block holding
/// another) on FURN, two doors (a block: its leaf on layer 0, its swing on a layer of its own) on DOORS, a room name;
/// a second drawing 50 m to the right; a sheet; and a layer and a block nothing uses. Colour tells as much as layer:
/// WALL is red, and the wall on layer 0 is red by itself; FURN is green, and the table cyan and dashed by itself;
/// DOORS is blue and dashed. The handles of its first wall and of its first door come with it.
fn room() -> (Doc, Handle, Handle) {
    let mut d = Doc::new_metric();
    for l in ["WALL", "FURN", "DOORS", "SWING", "TEXT", "UNUSED"] {
        d.ensure_layer(l);
    }
    d.linetypes.push(Linetype::simple("DASHED", "Dashed", &[12.0, -6.0]));
    for (l, color, linetype) in [("WALL", 1, "Continuous"), ("FURN", 3, "Continuous"), ("DOORS", 5, "DASHED")] {
        let layer = d.layer_mut(l).expect("the layer");
        (layer.color, layer.linetype) = (Color::Index(color), linetype.into());
    }
    block(&mut d, "DR_D01", vec![(on("0"), line((0.0, 0.0), (900.0, 0.0))), (on("SWING"), circle((0.0, 0.0), 100.0))]);
    block(&mut d, "DR_D02", vec![(on("0"), line((0.0, 0.0), (800.0, 0.0)))]);
    block(&mut d, "LEG", vec![(on("0"), circle((0.0, 0.0), 20.0))]);
    block(&mut d, "CHAIR", vec![(on("0"), line((0.0, 0.0), (400.0, 0.0))), (on("0"), placed("LEG", (0.0, 0.0)))]);
    block(&mut d, "CAR", vec![(on("0"), line((0.0, 0.0), (4500.0, 0.0)))]);
    let m = Space::Model;
    let mut add = |c: Common, k: EntityKind| d.add(&m, c, k).expect("added");
    let wall = add(on("WALL"), line((0.0, 0.0), (6000.0, 0.0)));
    add(on("WALL"), line((0.0, 0.0), (0.0, 4000.0)));
    add(Common { color: Color::Index(1), ..on("0") }, line((0.0, 4000.0), (6000.0, 4000.0)));
    add(Common { color: Color::Index(4), linetype: "DASHED".into(), ..on("FURN") }, circle((1000.0, 1000.0), 300.0));
    add(on("FURN"), placed("CHAIR", (4000.0, 3000.0)));
    let door = add(on("DOORS"), placed("DR_D01", (3000.0, 0.0)));
    add(on("DOORS"), placed("DR_D01", (0.0, 2000.0)));
    add(on("DOORS"), placed("DR_D02", (5000.0, 4000.0)));
    add(on("TEXT"), text((2000.0, 2000.0), "Bedroom"));
    // The second drawing, and a wall that reaches from the first into it: its centre is in neither.
    add(on("WALL"), line((50_000.0, 0.0), (56_000.0, 0.0)));
    add(on("WALL"), line((5000.0, 2000.0), (51_000.0, 2000.0)));
    let mut sheet = Layout::new("A1", 1);
    sheet.entities.push(Entity { handle: d.new_handle(), common: on("TEXT"), kind: text((10.0, 10.0), "Ground floor") });
    d.layouts = vec![sheet];
    (d, wall, door)
}

/// The drawing opened as Arqiflow's worker opens it: from its pack.
fn open(d: &Doc) -> (Opened, Measured) {
    let o = Opened::unpack(&cadcraft_index::pack(d).expect("packed")).expect("opened");
    let m = Measured::of(&o);
    (o, m)
}

fn count(d: &Doc, rule: Value) -> Value {
    let (o, m) = open(d);
    counts(&o, &m, &rule.to_string()).expect("counted")
}

fn total(d: &Doc, rule: Value) -> u64 {
    count(d, rule)["total"].as_u64().expect("a total")
}

/// The first room's box, mm.
const ROOM: [[f64; 2]; 2] = [[-500.0, -500.0], [6500.0, 4500.0]];

#[test]
fn a_rule_s_parts_each_select_and_together_all_must_hold() {
    let (d, wall, _) = room();
    assert_eq!(total(&d, json!({})), 11);
    // Layers: any of the list, by name in any case.
    assert_eq!(total(&d, json!({"layers": ["wall"]})), 4);
    assert_eq!(total(&d, json!({"layers": ["WALL", "0"]})), 5);
    // Kinds, as `query` names them.
    assert_eq!(total(&d, json!({"kinds": ["line"]})), 5);
    assert_eq!(total(&d, json!({"kinds": ["blocks", "text"]})), 5);
    // Blocks: by name, or by how the name begins; never another kind of item.
    assert_eq!(total(&d, json!({"blocks": ["dr_d01"]})), 2);
    assert_eq!(total(&d, json!({"blocks": ["DR_*"]})), 3);
    assert_eq!(total(&d, json!({"blocks": ["DR_*", "CHAIR"]})), 4);
    // A box selects by the centre of the item's own box: the wall reaching out of the room is not in it.
    assert_eq!(total(&d, json!({"box": ROOM})), 9);
    assert_eq!(total(&d, json!({"box": [[49_000, -500], [57_000, 500]]})), 1);
    // The longer side of the item's box, mm.
    assert_eq!(total(&d, json!({"min_size": 5000})), 4);
    assert_eq!(total(&d, json!({"min_size": 5000, "max_size": 6000})), 3);
    assert_eq!(total(&d, json!({"max_size": 600, "kinds": ["circle", "line"]})), 1);
    // Handles, as `query` gives them.
    assert_eq!(total(&d, json!({"handles": [format!("{:x}", wall.0)]})), 1);
    // Every part given must hold.
    assert_eq!(total(&d, json!({"layers": ["WALL"], "box": ROOM})), 2);
    assert_eq!(total(&d, json!({"layers": ["WALL", "0"], "kinds": ["line"], "box": ROOM, "min_size": 5000})), 2);
    assert_eq!(total(&d, json!({"layers": ["FURN"], "blocks": ["DR_*"]})), 0);
}

#[test]
fn a_rule_selects_by_the_colour_and_the_linetype_an_item_is_drawn_in() {
    let (d, ..) = room();
    // Red: the walls on WALL, red by their layer, and the wall on layer 0, red by itself; however red is named.
    for red in [json!("red"), json!(1), json!("#FF0000"), json!("1")] {
        assert_eq!(total(&d, json!({"colors": [red]})), 5);
    }
    assert_eq!(total(&d, json!({"colors": ["red"], "layers": ["0"]})), 1);
    assert_eq!(total(&d, json!({"colors": ["cyan"]})), 1);
    assert_eq!(total(&d, json!({"colors": ["cyan", 5]})), 4);
    // A true colour that is not in the drawing selects nothing; a layer's colour is not its items' when they have their own.
    assert_eq!(total(&d, json!({"colors": ["#123456"]})), 0);
    assert_eq!(total(&d, json!({"colors": ["green"]})), 1);
    // "bylayer": the items that leave their colour to their layer, alone or beside colours.
    assert_eq!(total(&d, json!({"colors": ["bylayer"]})), 9);
    assert_eq!(total(&d, json!({"colors": ["bylayer", "cyan"]})), 10);
    // Linetypes the same way: the doors are dashed by their layer, the table by itself.
    assert_eq!(total(&d, json!({"linetypes": ["dashed"]})), 4);
    assert_eq!(total(&d, json!({"linetypes": ["Continuous"]})), 7);
    assert_eq!(total(&d, json!({"linetypes": ["bylayer"]})), 10);
    assert_eq!(total(&d, json!({"linetypes": ["DASHED"], "colors": ["blue"], "kinds": ["block"]})), 3);
    let (o, m) = open(&d);
    assert!(counts(&o, &m, r#"{"colors": ["mauve"]}"#).expect_err("refused").contains("`mauve` is not a colour"));
    assert!(counts(&o, &m, r#"{"colors": "red"}"#).expect_err("refused").contains("`colors` is a list of colours"));
    assert!(
        counts(&o, &m, r#"{"linetypes": ["DOTTY"]}"#)
            .expect_err("refused")
            .contains("the drawing has no linetype `DOTTY` (it has: ByBlock, ByLayer, Continuous, DASHED)")
    );
}

#[test]
fn counts_total_by_kind_by_layer_and_by_block() {
    let (d, ..) = room();
    let c = count(&d, json!({"box": ROOM}));
    assert_eq!(c["total"], 9);
    assert_eq!(c["kinds"], json!({"block": 4, "circle": 1, "line": 3, "text": 1}));
    // The layers with the most items first, then by name.
    // Each layer's own colour, and the colours its items are drawn in when they are not all the layer's.
    // And the linetypes they are drawn in, when any is not solid: DOORS is dashed, and so is the table on FURN.
    assert_eq!(
        c["layers"][0],
        json!({"name": "DOORS", "items": 3, "kinds": {"block": 3}, "color": "#0000ff", "linetypes": {"dashed": 3}})
    );
    assert_eq!(
        c["layers"][1],
        json!({"name": "FURN", "items": 2, "kinds": {"block": 1, "circle": 1}, "color": "#00ff00", "colors": {"#00ff00": 1, "#00ffff": 1},
               "linetypes": {"continuous": 1, "dashed": 1}})
    );
    assert_eq!(c["layers"][3], json!({"name": "0", "items": 1, "kinds": {"line": 1}, "color": "#ffffff", "colors": {"#ff0000": 1}}));
    // The colours the items are drawn in, the most used first: red is two walls on WALL and the one on layer 0.
    assert_eq!(c["colors"][0], json!({"color": "#0000ff", "aci": 5, "items": 3, "layers": ["DOORS"]}));
    assert_eq!(c["colors"][1], json!({"color": "#ff0000", "aci": 1, "items": 3, "layers": ["0", "WALL"]}));
    let rest: Vec<&str> = c["colors"].as_array().expect("colours").iter().skip(2).filter_map(|c| c["color"].as_str()).collect();
    assert_eq!(rest, ["#00ff00", "#00ffff", "#ffffff"]);
    assert_eq!(c["layers"].as_array().expect("layers").len(), 5);
    // The blocks most placed first: the size of what the definition draws at scale 1, the layers its placements are on.
    assert_eq!(c["blocks"][0], json!({"name": "DR_D01", "placed": 2, "size": [1000.0, 200.0], "holds": 2, "layers": ["DOORS"]}));
    assert_eq!(c["blocks"][1]["name"], "CHAIR");
    assert_eq!(c["blocks"][1]["size"], json!([420.0, 40.0]));
    assert!(c.get("blocks_more").is_none());
    assert_eq!(c["extents"], json!([[-100.0, -100.0], [6000.0, 4000.0]]));
    // Nothing selected is counted as nothing.
    let none = count(&d, json!({"box": [[100_000, 0], [101_000, 1000]]}));
    assert_eq!((none["total"].as_u64(), none["extents"].is_null()), (Some(0), true));
}

#[test]
fn more_blocks_than_are_listed_are_said() {
    let mut d = Doc::new_metric();
    for n in 0..BLOCKS_LISTED + 5 {
        let name = format!("B{n}");
        block(&mut d, &name, vec![(on("0"), line((0.0, 0.0), (100.0, 0.0)))]);
        d.add(&Space::Model, on("0"), placed(&name, (n as f64 * 200.0, 0.0))).expect("added");
    }
    let c = count(&d, json!({}));
    assert_eq!(c["blocks"].as_array().expect("blocks").len(), BLOCKS_LISTED);
    assert_eq!(c["blocks_more"], 5);
}

#[test]
fn an_unknown_parameter_is_refused_by_name() {
    let (d, ..) = room();
    let (o, m) = open(&d);
    let refused = |r: Result<Value, String>| r.expect_err("refused");
    assert!(
        refused(counts(&o, &m, r#"{"layer": ["WALL"]}"#))
            .contains("unknown parameter `layer`; counts takes: open, depth, layers, kinds, blocks")
    );
    assert!(
        refused(cut(&o, &m, r#"{"crop": [[0, 0], [1, 1]]}"#).map(|c| c.1))
            .contains("unknown parameter `crop`; extract takes: open, depth, box, keep, drop, sort, move")
    );
    assert!(
        refused(cut(&o, &m, r#"{"keep": [{"colour": 1}]}"#).map(|c| c.1)).contains("unknown parameter `colour`; a rule of `keep` takes")
    );
    assert!(
        refused(cut(&o, &m, r#"{"sort": [{"layer": "A-WALL", "layers": ["WALL"]}]}"#).map(|c| c.1))
            .contains("unknown parameter `layer`; a rule of `sort` takes: to, layers")
    );
    assert!(refused(cut(&o, &m, r#"{"sort": [{"layers": ["WALL"]}]}"#).map(|c| c.1)).contains("names the layer its items go to (`to`)"));
    assert!(
        look(&o, &m, r#"{"sort": []}"#, 400)
            .map(|p| p.about)
            .expect_err("refused")
            .contains("unknown parameter `sort`; look takes: open, depth, box, keep, drop")
    );
    // What is asked for and is not in the drawing, or is not what a part holds, is said too.
    assert!(refused(counts(&o, &m, r#"{"layers": ["WALLS"]}"#)).contains("the drawing has no layer `WALLS` (it has: 0, Defpoints, WALL"));
    assert!(refused(counts(&o, &m, r#"{"kinds": ["wall"]}"#)).contains("`wall` is not a kind"));
    assert!(refused(counts(&o, &m, r#"{"box": [0, 0, 1, 1]}"#)).contains("`box` is two corners"));
    assert!(refused(counts(&o, &m, r#"{"min_size": "big"}"#)).contains("`min_size` is the longer side"));
    assert!(refused(cut(&o, &m, r#"{"move": 5}"#).map(|c| c.1)).contains("`move` is [dx, dy]"));
    assert!(refused(cut(&o, &m, r#"{"box": [[100000, 0], [101000, 1000]]}"#).map(|c| c.1)).contains("nothing is selected"));
    assert!(extract(&o, &m, "{}", "R14").expect_err("refused").contains("not one CADCraft writes"));
}

fn layers_of(d: &Doc) -> Vec<String> {
    let mut l: Vec<String> = d.model.iter().map(|e| format!("{}@{}", kind_of(&e.kind), e.common.layer)).collect();
    l.sort();
    l
}

#[test]
fn a_part_is_cropped_kept_dropped_sorted_and_moved_in_that_order() {
    let (d, ..) = room();
    let (o, m) = open(&d);
    // Cropped: the room alone, without the wall that reaches out of it or the second drawing.
    let (part, about) = cut(&o, &m, &json!({"box": ROOM}).to_string()).expect("cut");
    assert_eq!((part.model.len(), about["items"].as_u64()), (9, Some(9)));
    assert_eq!(about["extents"], json!([[-100.0, -100.0], [6000.0, 4000.0]]));
    // Kept: only what one of the rules selects; then dropped: what one of the rules selects of those.
    let (part, _) = cut(&o, &m, &json!({"box": ROOM, "keep": [{"layers": ["WALL", "0"]}, {"kinds": ["block"]}]}).to_string()).expect("cut");
    assert_eq!(part.model.len(), 7);
    let (part, _) = cut(
        &o,
        &m,
        &json!({"box": ROOM, "keep": [{"layers": ["WALL", "0"]}, {"kinds": ["block"]}], "drop": [{"blocks": ["CHAIR"]}]}).to_string(),
    )
    .expect("cut");
    assert_eq!(layers_of(&part), ["block@DOORS", "block@DOORS", "block@DOORS", "line@0", "line@WALL", "line@WALL"]);
    // Sorted: each item on the layer of the first rule that selects it as it was; an item no rule selects keeps its
    // layer. The long line on layer 0 is a wall by its size; the first rule takes the doors before the second could.
    let sort = json!([
        {"to": "A-DOOR", "blocks": ["DR_*"]},
        {"to": "X-UNSURE", "kinds": ["block"]},
        {"to": "A-WALL", "layers": ["WALL"]},
        {"to": "A-WALL", "layers": ["0"], "kinds": ["line"], "min_size": 3000},
    ]);
    let (part, about) =
        cut(&o, &m, &json!({"box": ROOM, "drop": [{"layers": ["TEXT"]}], "sort": sort, "move": [-1000, 500]}).to_string()).expect("cut");
    assert_eq!(
        layers_of(&part),
        ["block@A-DOOR", "block@A-DOOR", "block@A-DOOR", "block@X-UNSURE", "circle@FURN", "line@A-WALL", "line@A-WALL", "line@A-WALL"]
    );
    // A clean layer says the blocks placed on it, each with the kinds it holds.
    assert_eq!((about["layers"][0]["name"].clone(), about["layers"][0]["items"].clone()), (json!("A-DOOR"), json!(3)));
    assert_eq!(about["layers"][0]["blocks"][0], json!({"name": "DR_D01", "placed": 2, "kinds": {"circle": 1, "line": 1}}));
    assert_eq!(about["kinds"], json!({"block": 4, "circle": 1, "line": 3}));
    // The layers made, and those nothing is on any more left out (but 0, and SWING, which the door's block draws on).
    let mut names: Vec<&str> = part.layers.iter().map(|l| l.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["0", "A-DOOR", "A-WALL", "FURN", "SWING", "X-UNSURE"]);
    // Each looks as it did on its new layer, which is white and continuous: the walls that were red by their layer are
    // red by themselves, the doors blue and dashed; the wall that was red by itself still is.
    let drawn: BTreeSet<(String, String, String)> = part
        .model
        .iter()
        .filter(|e| e.common.layer.starts_with("A-"))
        .map(|e| (e.common.layer.clone(), e.common.color.name(), e.common.linetype.clone()))
        .collect();
    assert_eq!(
        drawn,
        BTreeSet::from([("A-DOOR".into(), "Blue".into(), "DASHED".into()), ("A-WALL".into(), "Red".into(), "ByLayer".into())])
    );
    assert!(part.layer("A-WALL").is_some_and(|l| l.color == Color::Index(7) && l.linetype == "Continuous"));
    // Sorted onto a layer of the same colour, an item still leaves its colour to its layer.
    let (same, _) =
        cut(&o, &m, &json!({"box": ROOM, "sort": [{"to": "WALL", "colors": ["red"]}, {"to": "WALL", "kinds": ["text"]}]}).to_string())
            .expect("cut");
    let on_wall: Vec<Color> = same.model.iter().filter(|e| e.common.layer == "WALL").map(|e| e.common.color).collect();
    assert_eq!(on_wall, [Color::ByLayer, Color::ByLayer, Color::Index(1), Color::Index(7)]);
    // Moved: added to every coordinate, the extents with it.
    assert_eq!(about["extents"], json!([[-1100.0, 400.0], [5000.0, 4500.0]]));
    let first = part.model.iter().next().expect("the first wall");
    assert_eq!(first.kind, line((-1000.0, 500.0), (5000.0, 500.0)));
    let door =
        part.model.iter().find_map(|e| if let EntityKind::Insert(i) = &e.kind { Some(i.insert) } else { None }).expect("a placement");
    assert_eq!((door.x, door.y), (3000.0, 3500.0));
    // The open drawing is as it was.
    assert_eq!(o.drawing.model.len(), 11);
    assert_eq!(o.drawing.model.iter().next().expect("the first wall").kind, line((0.0, 0.0), (6000.0, 0.0)));
}

#[test]
fn a_placement_is_kept_whole_and_re_sorted_and_what_nothing_uses_is_left_out() {
    let (d, _, door) = room();
    let (o, m) = open(&d);
    let params = json!({"box": ROOM, "drop": [{"layers": ["TEXT"]}], "sort": [{"to": "A-DOOR", "blocks": ["DR_D01"]}]}).to_string();
    let (bytes, about) = extract(&o, &m, &params, "2018").expect("extracted");
    assert!(cadcraft_dwg::is_dwg(&bytes));
    assert_eq!(cadcraft_dwg::version(&bytes).as_deref(), Some("AC1032"));
    let back = cadcraft_index::read(&bytes, "part.dwg").expect("read back");
    assert_eq!(back.model.len(), 8);
    assert_eq!(about["items"], 8);
    // The door is still a placement of its block, with the handle it had, on its new layer.
    let e = back.model.get(door).expect("the door, by its handle");
    assert!(matches!(&e.kind, EntityKind::Insert(i) if i.block == "DR_D01"));
    assert_eq!(e.common.layer, "A-DOOR");
    // And drawn as it was, blue and dashed, which it had left to its old layer.
    assert_eq!((e.common.color, e.common.linetype.as_str()), (Color::Index(5), "DASHED"));
    // Its definition came with it, as it was: the leaf on layer 0, which a DWG draws on the placement's layer (so it
    // follows the door to A-DOOR with nothing rewritten), and the swing on its own layer, which stays.
    let def = back.block("DR_D01").expect("the door's block");
    let inside: Vec<&str> = def.entities.iter().map(|e| e.common.layer.as_str()).collect();
    assert_eq!(inside, ["0", "SWING"]);
    // A block in a block comes too; a block nothing places does not.
    assert!(back.block("CHAIR").is_some() && back.block("LEG").is_some() && back.block("DR_D02").is_some());
    assert!(back.block("CAR").is_none());
    // Layers nothing is on are left out, and the sheet's items.
    let has = |n: &str| back.layers.iter().any(|l| l.name == n);
    assert!(has("0") && has("A-DOOR") && has("WALL") && has("FURN") && has("SWING") && has("DOORS"));
    assert!(!has("UNUSED") && !has("TEXT"));
    assert!(back.layouts.iter().all(|l| l.entities.iter().all(|e| matches!(e.kind, EntityKind::Viewport(_)))));
    assert!(back.layouts.iter().all(|l| l.name != "A1"));
}

#[test]
fn the_extract_indexed_again_holds_what_was_asked() {
    let (d, ..) = room();
    let (o, m) = open(&d);
    let params = json!({
        "box": ROOM,
        "keep": [{"layers": ["WALL", "0", "DOORS"]}],
        "sort": [{"to": "A-WALL", "kinds": ["line"]}, {"to": "A-DOOR", "kinds": ["block"]}],
        "move": [-3000, -2000],
    })
    .to_string();
    let (bytes, about) = extract(&o, &m, &params, "2013").expect("extracted");
    // Indexed as any drawing is, and asked of with the same instruments.
    let (pack, summary) = cadcraft_index::index(&bytes, "part.dwg").expect("indexed");
    let o2 = Opened::unpack(&pack).expect("opened");
    let m2 = Measured::of(&o2);
    let c = counts(&o2, &m2, "").expect("counted");
    assert_eq!(c["total"], 6);
    assert_eq!(c["total"], about["items"]);
    assert_eq!(c["kinds"], about["kinds"]);
    assert_eq!(c["kinds"], json!({"block": 3, "line": 3}));
    let by_layer = |v: &Value| -> Vec<(String, u64)> {
        v["layers"]
            .as_array()
            .expect("layers")
            .iter()
            .map(|l| (l["name"].as_str().unwrap_or("").to_string(), l["items"].as_u64().unwrap_or(0)))
            .collect()
    };
    assert_eq!(by_layer(&c), by_layer(&about));
    assert_eq!(by_layer(&c), [("A-DOOR".to_string(), 3), ("A-WALL".to_string(), 3)]);
    assert_eq!(c["extents"], about["extents"]);
    assert_eq!(c["extents"], json!([[-3100.0, -2100.0], [3000.0, 2000.0]]));
    assert_eq!(c["blocks"][0]["name"], "DR_D01");
    assert_eq!(summary["items"], 6);
    assert_eq!(summary["sheets"], json!([]));
}

/// The room drawn in metres in a real grid, 400 km east and 5,000 km north: given out in mm less an offset.
fn far() -> Doc {
    let mut d = Doc::new_metric();
    d.header.set("INSUNITS", HVal::Int(6));
    let (x, y) = (400_000.0, 5_000_000.0);
    block(&mut d, "DOOR", vec![(on("0"), line((0.0, 0.0), (0.9, 0.0)))]);
    for (a, b) in [((0.0, 0.0), (6.0, 0.0)), ((6.0, 0.0), (6.0, 4.0)), ((6.0, 4.0), (0.0, 4.0)), ((0.0, 4.0), (0.0, 0.0))] {
        d.add(&Space::Model, on("WALL"), line((x + a.0, y + a.1), (x + b.0, y + b.1))).expect("added");
    }
    d.add(&Space::Model, on("DOORS"), placed("DOOR", (x + 3.0, y))).expect("added");
    d
}

#[test]
fn a_drawing_far_out_is_asked_and_moved_in_the_coordinates_query_gives() {
    let d = far();
    let (o, m) = open(&d);
    let off = o.offset();
    assert_eq!(off, [400_000_000.0, 5_000_000_000.0]);
    // A box is taken less the offset, as `query` gives points, or with it.
    let near = json!({"box": [[2000, -1000], [7000, 5000]]});
    assert_eq!(counts(&o, &m, &near.to_string()).expect("counted")["total"], 4);
    let true_box = json!({"box": [[off[0] + 2000.0, off[1] - 1000.0], [off[0] + 7000.0, off[1] + 5000.0]]});
    assert_eq!(counts(&o, &m, &true_box.to_string()).expect("counted")["total"], 4);
    let all = counts(&o, &m, "").expect("counted");
    assert_eq!(all["extents"], json!([[0.0, 0.0], [6000.0, 4000.0]]));
    assert_eq!(all["blocks"][0]["size"], json!([900.0, 0.0]));
    assert_eq!(counts(&o, &m, r#"{"min_size": 5000}"#).expect("counted")["total"], 2);
    // Not moved, the part lies where it lay: its extents less the offset, which is said.
    let (part, about) = cut(&o, &m, "{}").expect("cut");
    assert_eq!(about["extents"], json!([[0.0, 0.0], [6000.0, 4000.0]]));
    assert_eq!(about["offset"], json!(off));
    assert_eq!(part.model.iter().next().expect("a wall").kind, line((400_000.0, 5_000_000.0), (400_006.0, 5_000_000.0)));
    // Moved so that the point `query` gives as (6000, 4000) lands on 0, 0: the part is no longer far out.
    let (bytes, about) = extract(&o, &m, r#"{"move": [-6000, -4000]}"#, "").expect("extracted");
    assert_eq!(about["extents"], json!([[-6000.0, -4000.0], [0.0, 0.0]]));
    assert!(about.get("offset").is_none());
    let (pack, summary) = cadcraft_index::index(&bytes, "part.dwg").expect("indexed");
    assert_eq!((summary["unit"].as_str(), summary.get("offset")), (Some("m"), None));
    assert_eq!(summary["extents"], json!([[-6000.0, -4000.0], [0.0, 0.0]]));
    let o2 = Opened::unpack(&pack).expect("opened");
    let doors = o2.query(&cadcraft_index::Query { kinds: Some(vec!["block".into()]), ..Default::default() }).expect("asked");
    assert_eq!(doors["items"][0]["at"], json!([-3000.0, -4000.0]));
}

#[test]
fn a_picture_shows_only_what_is_selected_in_the_drawing_s_own_colours() {
    let (d, ..) = room();
    let (o, m) = open(&d);
    let p = look(&o, &m, &json!({"box": ROOM, "drop": [{"layers": ["TEXT"]}]}).to_string(), 600).expect("drawn");
    assert_eq!(&p.png[..8], b"\x89PNG\r\n\x1a\n");
    let a = &p.about;
    assert_eq!((a["width"].as_u64(), a["height"].as_u64()), (Some(600), Some(429)));
    // A box is a view: the wall that runs out of the room into the second drawing is in it too, cut at its edge.
    assert_eq!((a["items"].as_u64(), a["color_by"].as_str()), (Some(9), Some("own")));
    assert_eq!(a["selected"], json!([[-100.0, -100.0], [51_000.0, 4000.0]]));
    // The picture's edges are the box's (to the pixel: 5000 mm high at 600 px for 7000 mm is 428.6 px, drawn as 429).
    assert_eq!(a["extents"], json!([[-500.0, -502.5], [6500.0, 4502.5]]));
    // The legend: the layers the items are on, the most used first, each with its own colour, and the colours its
    // items are drawn in when they are not all the layer's.
    assert_eq!(
        a["legend"],
        json!([
            {"layer": "DOORS", "color": "#0000ff", "items": 3},
            {"layer": "WALL", "color": "#ff0000", "items": 3},
            {"layer": "FURN", "color": "#00ff00", "items": 2, "colors": {"#00ff00": 1, "#00ffff": 1}},
            {"layer": "0", "color": "#ffffff", "items": 1, "colors": {"#ff0000": 1}},
        ])
    );
    assert!(holds_color(&p.png, Rgb(255, 0, 0)) && holds_color(&p.png, Rgb(0, 0, 255)) && holds_color(&p.png, Rgb(0, 255, 255)));
    // White (index 7: here the doors' swings, on SWING) is drawn black on the picture's white.
    assert!(holds_color(&p.png, Rgb(0, 0, 0)));
    // Only layer WALL: fitted to those lines.
    let p = look(&o, &m, r#"{"keep": [{"layers": ["WALL"], "max_size": 6000}], "color_by": "own"}"#, 100).expect("drawn");
    assert_eq!(p.about["items"], 3);
    assert_eq!(p.about["selected"], json!([[0.0, 0.0], [56_000.0, 4000.0]]));
    assert_eq!(p.about["width"], 200);
    assert!(look(&o, &m, r#"{"box": [[100000, 0], [101000, 1000]]}"#, 400).is_err());
    assert!(look(&o, &m, r#"{"color_by": "kind"}"#, 400).map(|p| p.about).expect_err("refused").contains("`color_by` is \"own\""));
    // A layer that is off is not in a picture of the drawing as it is, and the legend says so.
    let mut dark = d.clone();
    dark.layer_mut("WALL").expect("the layer").on = false;
    let (o, m) = open(&dark);
    let p = look(&o, &m, &json!({"box": ROOM, "keep": [{"layers": ["WALL", "DOORS"]}]}).to_string(), 600).expect("drawn");
    assert_eq!(p.about["legend"][1], json!({"layer": "WALL", "color": "#ff0000", "items": 3, "off": true}));
    assert!(!holds_color(&p.png, Rgb(255, 0, 0)));
    // By layer, it is drawn all the same.
    let p =
        look(&o, &m, &json!({"box": ROOM, "keep": [{"layers": ["WALL", "DOORS"]}], "color_by": "layer"}).to_string(), 600).expect("drawn");
    let walls = Rgb::parse_hex(p.about["legend"][1]["color"].as_str().unwrap_or("")).expect("a colour");
    assert!(holds_color(&p.png, walls));
}

#[test]
fn shapes_given_over_a_picture_are_drawn_over_the_drawing_in_their_frame() {
    let (d, ..) = room();
    let (o, m) = open(&d);
    // shapes in metres, the drawing in millimetres moved by 100 mm: a wall's strip, a mark round a place, a word
    let over = json!({"scale": 1000.0, "shift": [100.0, 0.0], "shapes": [
        {"polyline": [[0.0, 0.0], [3.0, 0.0], [3.0, 0.2], [0.0, 0.2]], "closed": true, "color": "#ff00ff", "width": 0.05},
        {"circle": [2.0, 2.0, 0.4], "color": "#ff8000"},
        {"arc": [2.0, 2.0, 0.6, 0.0, 1.5], "color": "#ff8000"},
        {"line": [[0.0, 3.0], [1.0, 3.0]], "color": "#ff8000"},
        {"text": [0.5, 1.0, 0.3, "1"], "color": "#ff8000"}
    ]});
    let p = look(&o, &m, &json!({"box": ROOM, "keep": [{"layers": ["WALL"]}], "over": over}).to_string(), 600).expect("drawn");
    assert_eq!(p.about["over"], 5);
    // the legend is the drawing's alone
    assert!(p.about["legend"].as_array().expect("a legend").iter().all(|l| l["layer"] != OVER_LAYER));
    assert!(holds_color(&p.png, Rgb(255, 0, 255)) && holds_color(&p.png, Rgb(255, 128, 0)) && holds_color(&p.png, Rgb(255, 0, 0)));
    // with no box, the picture is fitted to the drawing's items and the shapes both
    let far = json!({"shapes": [{"circle": [20000.0, 20000.0, 500.0], "color": "#ff00ff"}]});
    let p = look(&o, &m, &json!({"keep": [{"layers": ["WALL"], "max_size": 6000}], "over": far}).to_string(), 400).expect("drawn");
    assert!(p.about["extents"][1][1].as_f64().expect("a number") >= 20_500.0);
    // what is not a shape is refused by name
    for (bad, says) in [
        (json!({"shapes": [{"square": [0, 0], "color": "#ff00ff"}]}), "unknown parameter `square`"),
        (json!({"shapes": [{"line": [[0, 0], [1, 1]]}]}), "`color`"),
        (json!({"shapes": [{"circle": [0, 0], "color": "#ff00ff"}]}), "a circle is 3 numbers"),
        (json!({"scale": 0, "shapes": []}), "scale is a number above 0"),
    ] {
        let e = look(&o, &m, &json!({"box": ROOM, "over": bad}).to_string(), 300).map(|p| p.about).expect_err("refused");
        assert!(e.contains(says), "{e}");
    }
}

#[test]
fn a_picture_by_layer_gives_each_layer_a_colour_of_its_own() {
    let (d, ..) = room();
    let (o, m) = open(&d);
    let p = look(&o, &m, &json!({"box": ROOM, "drop": [{"layers": ["TEXT"]}], "color_by": "layer"}).to_string(), 600).expect("drawn");
    let a = &p.about;
    assert_eq!((a["items"].as_u64(), a["color_by"].as_str()), (Some(9), Some("layer")));
    // The legend: the layers the items are on, the most used first, then those only a block draws on.
    let legend = a["legend"].as_array().expect("a legend");
    let named: Vec<(&str, u64)> = legend.iter().map(|l| (l["layer"].as_str().unwrap_or(""), l["items"].as_u64().unwrap_or(9))).collect();
    assert_eq!(named, [("DOORS", 3), ("WALL", 3), ("FURN", 2), ("0", 1), ("SWING", 0)]);
    assert_eq!(legend[4]["in_blocks"], true);
    let colors: BTreeSet<&str> = legend.iter().filter_map(|l| l["color"].as_str()).collect();
    assert_eq!(colors.len(), 5);
    assert!(colors.iter().all(|c| c.len() == 7 && c.starts_with('#')));
    // The colours are in the picture, whatever colour an item had (the wall on layer 0 is no longer red), and the open
    // drawing's layers keep their own.
    for row in legend.iter().take(4) {
        let c = Rgb::parse_hex(row["color"].as_str().unwrap_or("")).expect("a colour");
        assert!(holds_color(&p.png, c), "no pixel of {row}");
    }
    assert!(!holds_color(&p.png, Rgb(255, 0, 0)));
    assert_eq!(o.drawing.layer("WALL").map(|l| l.color), Some(Color::Index(1)));
}

/// Whether a PNG on white has a pixel of this colour: a thin line's pixels are smoothed into the white, so one at
/// least half way from white to it, and on the way.
fn holds_color(png: &[u8], c: Rgb) -> bool {
    let want = [f64::from(c.0) - 255.0, f64::from(c.1) - 255.0, f64::from(c.2) - 255.0];
    let far = want.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
    image::load_from_memory(png).expect("a PNG").to_rgb8().pixels().any(|p| {
        let got = [f64::from(p.0[0]) - 255.0, f64::from(p.0[1]) - 255.0, f64::from(p.0[2]) - 255.0];
        let share = got.iter().zip(&want).fold(0.0_f64, |m, (g, w)| if w.abs() == far { g / w } else { m });
        share >= 0.5 && got.iter().zip(&want).all(|(g, w)| (g - w * share).abs() < 12.0)
    })
}

/// Two floors, each one block placed once, as an office draws them: FLOOR holds a slab edge on layer 0, a column line on
/// COLS drawn ByBlock, a door (a block in it) and a column on COLS. The first placement is turned a quarter, twice the
/// size and yellow; the second lies as drawn and leaves its colour to its layer STRUCT (green). Their handles come too.
fn floors() -> (Doc, Handle, Handle) {
    let mut d = Doc::new_metric();
    for (l, color) in [("STRUCT", 3), ("COLS", 5), ("SWING", 7)] {
        d.ensure_layer(l);
        d.layer_mut(l).expect("the layer").color = Color::Index(color);
    }
    block(&mut d, "DR_D01", vec![(on("0"), line((0.0, 0.0), (900.0, 0.0))), (on("SWING"), circle((0.0, 0.0), 100.0))]);
    let by_block = |layer: &str| Common { color: Color::ByBlock, ..on(layer) };
    block(
        &mut d,
        "FLOOR",
        vec![
            (on("0"), line((0.0, 0.0), (1000.0, 0.0))),
            (by_block("COLS"), line((0.0, 0.0), (0.0, 500.0))),
            (by_block("0"), placed("DR_D01", (2000.0, 0.0))),
            (on("COLS"), circle((0.0, 0.0), 50.0)),
        ],
    );
    let turned = EntityKind::Insert(Insert {
        block: "FLOOR".into(),
        insert: Vec3::new(10_000.0, 0.0, 0.0),
        scale: Vec3::new(2.0, 2.0, 2.0),
        rotation: std::f64::consts::FRAC_PI_2,
        ..Insert::default()
    });
    let a = d.add(&Space::Model, Common { color: Color::Index(2), ..on("STRUCT") }, turned).expect("added");
    let b = d.add(&Space::Model, on("STRUCT"), placed("FLOOR", (0.0, 0.0))).expect("added");
    d.add(&Space::Model, on("STRUCT"), line((0.0, -1000.0), (5000.0, -1000.0))).expect("added");
    (d, a, b)
}

/// An item's points to the millimetre, with its layer and colour: what a test compares.
fn shown(e: &Entity) -> String {
    let p = |v: Vec3| format!("{:.0},{:.0}", v.x + 0.0, v.y + 0.0).replace("-0,", "0,").replace(",-0", ",0");
    let what = match &e.kind {
        EntityKind::Line(l) => format!("line {} {}", p(l.a), p(l.b)),
        EntityKind::Circle(c) => format!("circle {} r{:.0}", p(c.center), c.radius),
        EntityKind::Insert(i) => format!("{} {} x{:.0} {:.0}°", i.block, p(i.insert), i.scale.x, i.rotation.to_degrees()),
        EntityKind::Text(t) => format!("text {} {}", t.value, p(t.insert)),
        other => kind_of(other).to_string(),
    };
    format!("{what} on {} {}", e.common.layer, e.common.color.name())
}

#[test]
fn a_placement_is_opened_as_explode_takes_it_apart() {
    let (d, a, b) = floors();
    let (o, m) = open(&d);
    let whole = Seen::of(&o, &m);
    // The first floor alone, by its handle: placed, turned and scaled as the placement was. On layer 0, the slab edge
    // takes the placement's layer; drawn ByBlock, the column line and the door take its yellow; ByLayer stays ByLayer
    // of the item's own layer. The door stays a placement.
    let apart = super::open(&whole, &json!({"open": [{"handles": [a.hex()]}]})).expect("opened").expect("asked");
    let items: Vec<String> = apart.d.model.iter().map(|e| shown(e)).collect();
    assert_eq!(
        items,
        [
            "line 10000,0 10000,2000 on STRUCT ByLayer",
            "line 10000,0 9000,0 on COLS Yellow",
            "DR_D01 10000,4000 x2 90° on STRUCT Yellow",
            "circle 10000,0 r100 on COLS ByLayer",
            "FLOOR 0,0 x1 0° on STRUCT ByLayer",
            "line 0,-1000 5000,-1000 on STRUCT ByLayer",
        ]
    );
    assert_eq!(apart.said(), json!({"placements": 1, "items": 4}));
    // The open drawing is as it was, and what came out has handles of its own, past the drawing's.
    assert_eq!(o.drawing.model.len(), 3);
    assert!(apart.d.model.iter().take(4).all(|e| e.handle.0 >= o.drawing.handseed));
    // A placement that leaves its colour to its layer: what it draws ByBlock on another layer is given that layer's
    // colour, so it is drawn as it was.
    let apart = super::open(&whole, &json!({"open": [{"handles": [b.hex()]}]})).expect("opened").expect("asked");
    let items: Vec<String> = apart.d.model.iter().skip(1).take(3).map(|e| shown(e)).collect();
    assert_eq!(items, ["line 0,0 1000,0 on STRUCT ByLayer", "line 0,0 0,500 on COLS Green", "DR_D01 2000,0 x1 0° on STRUCT ByLayer"]);
    // Two deep, the door is taken apart too: its leaf, on layer 0 and ByLayer, on the layer the door took and in that
    // layer's colour; its swing on its own.
    let apart = super::open(&whole, &json!({"open": [{"handles": [a.hex()]}], "depth": 2})).expect("opened").expect("asked");
    let items: Vec<String> = apart.d.model.iter().skip(2).take(2).map(|e| shown(e)).collect();
    assert_eq!(items, ["line 10000,4000 10000,5800 on STRUCT ByLayer", "circle 10000,4000 r200 on SWING ByLayer"]);
    assert_eq!(apart.said(), json!({"placements": 2, "items": 5}));
    assert!(
        super::open(&whole, &json!({"open": [{}], "depth": 9})).err().expect("refused").contains("`depth` is how deep `open` goes, 1 to 4")
    );
    assert!(super::open(&whole, &json!({"depth": 2})).expect("not asked").is_none());
}

#[test]
fn an_opened_item_is_an_item_of_the_drawing_in_all_three() {
    let (d, a, _) = floors();
    let (o, m) = open(&d);
    let count = |p: Value| counts(&o, &m, &p.to_string()).expect("counted");
    // Not opened, the floors are two blocks; `holds` tells a floor from a door.
    let c = count(json!({}));
    assert_eq!((c["total"].as_u64(), c.get("opened")), (Some(3), None));
    assert_eq!(c["blocks"][0], json!({"name": "FLOOR", "placed": 2, "size": [2950.0, 600.0], "holds": 4, "layers": ["STRUCT"]}));
    // Opened, their items are counted on their own layers, and chosen by a rule as any item is: by layer, by colour
    // as drawn, by the handle they are given (the placement's, and their place in the block).
    let c = count(json!({"open": [{"blocks": ["FLOOR"]}]}));
    assert_eq!(c["total"], 9);
    assert_eq!(c["opened"], json!({"placements": 2, "items": 8}));
    assert_eq!(c["kinds"], json!({"block": 2, "circle": 2, "line": 5}));
    assert_eq!(c["blocks"][0], json!({"name": "DR_D01", "placed": 2, "size": [1000.0, 200.0], "holds": 2, "layers": ["STRUCT"]}));
    assert_eq!(count(json!({"open": [{"blocks": ["FLOOR"]}], "layers": ["COLS"]}))["total"], 4);
    assert_eq!(count(json!({"open": [{"blocks": ["FLOOR"]}], "layers": ["COLS"], "colors": ["yellow"]}))["total"], 1);
    assert_eq!(
        count(json!({"open": [{"blocks": ["FLOOR"]}], "handles": [format!("{}.1", a.hex()), format!("{}.3", a.hex())]}))["kinds"],
        json!({"circle": 1, "line": 1})
    );
    assert_eq!(
        count(json!({"open": [{"blocks": ["FLOOR"]}], "depth": 2, "handles": [format!("{}.2.0", a.hex().to_lowercase())]}))["layers"][0]["name"],
        "STRUCT"
    );
    assert_eq!(count(json!({"open": [{"blocks": ["FLOOR"]}], "depth": 2}))["opened"], json!({"placements": 4, "items": 10}));
    // A picture of the opened drawing's columns alone.
    let p = look(&o, &m, &json!({"open": [{"blocks": ["FLOOR"]}], "keep": [{"layers": ["COLS"]}]}).to_string(), 300).expect("drawn");
    assert_eq!((p.about["items"].as_u64(), &p.about["opened"]), (Some(4), &json!({"placements": 2, "items": 8})));
    assert_eq!(p.about["legend"][0]["colors"], json!({"#0000ff": 2, "#00ff00": 1, "#ffff00": 1}));
    // Extracted: an opened item is cropped by its own centre, sorted and written as any item; the block no longer
    // placed is left out, and the one still placed comes.
    let params = json!({
        "open": [{"blocks": ["FLOOR"]}],
        "box": [[8000, -500], [11_000, 3000]],
        "sort": [{"to": "A-COLS", "layers": ["COLS"]}, {"to": "A-FLOR", "kinds": ["line"]}],
    })
    .to_string();
    let (bytes, about) = extract(&o, &m, &params, "2018").expect("extracted");
    assert_eq!(about["opened"], json!({"placements": 2, "items": 8}));
    assert_eq!(about["layers"], json!([{"name": "A-COLS", "items": 2}, {"name": "A-FLOR", "items": 1}]));
    let back = cadcraft_index::read(&bytes, "part.dwg").expect("read back");
    let mut items: Vec<String> = back.model.iter().map(|e| shown(e)).collect();
    items.sort();
    assert_eq!(
        items,
        ["circle 10000,0 r100 on A-COLS Blue", "line 10000,0 10000,2000 on A-FLOR Green", "line 10000,0 9000,0 on A-COLS Yellow"]
    );
    assert!(back.block("FLOOR").is_none() && back.block("DR_D01").is_none());
    let (_, about) = cut(&o, &m, &json!({"open": [{"blocks": ["FLOOR"]}]}).to_string()).expect("cut");
    assert_eq!(about["kinds"], json!({"block": 2, "circle": 2, "line": 5}));
}

#[test]
fn an_array_and_attributes_are_opened_too() {
    let mut d = Doc::new_metric();
    d.ensure_layer("TAGS");
    block(&mut d, "POST", vec![(on("0"), circle((0.0, 0.0), 100.0))]);
    let row = EntityKind::Insert(Insert {
        block: "POST".into(),
        insert: Vec3::new(0.0, 1000.0, 0.0),
        cols: 3,
        col_spacing: 2000.0,
        ..Insert::default()
    });
    let row = d.add(&Space::Model, on("TAGS"), row).expect("added");
    let Some(EntityKind::Text(words)) = Some(text((500.0, 500.0), "C1")) else { return };
    let tag = cadcraft_doc::Attrib {
        tag: "NO".into(),
        text: words,
        invisible: false,
        constant: false,
        prompt: String::new(),
        props: Default::default(),
        multiline: None,
        modes: Default::default(),
    };
    let hidden = cadcraft_doc::Attrib { tag: "COST".into(), invisible: true, ..tag.clone() };
    let tagged = EntityKind::Insert(Insert { block: "POST".into(), attribs: vec![tag, hidden], ..Insert::default() });
    let tagged = d.add(&Space::Model, on("TAGS"), tagged).expect("added");
    let (o, m) = open(&d);
    let whole = Seen::of(&o, &m);
    let apart = super::open(&whole, &json!({"open": [{"kinds": ["block"]}]})).expect("opened").expect("asked");
    let items: Vec<String> = apart.d.model.iter().map(|e| shown(e)).collect();
    // Each copy of the array; and the attribute that shows, as a text (the hidden one is not drawn, so not made).
    assert_eq!(
        items,
        [
            "circle 0,1000 r100 on TAGS ByLayer",
            "circle 2000,1000 r100 on TAGS ByLayer",
            "circle 4000,1000 r100 on TAGS ByLayer",
            "circle 0,0 r100 on TAGS ByLayer",
            "text C1 500,500 on TAGS ByLayer",
        ]
    );
    let mut called: Vec<&str> = apart.paths.values().map(String::as_str).collect();
    called.sort_unstable();
    let (row, tagged) = (row.hex(), tagged.hex());
    assert_eq!(called, [format!("{row}.0"), format!("{row}.0@1"), format!("{row}.0@2"), format!("{tagged}.0"), format!("{tagged}.1")]);
}

#[test]
fn opening_too_much_is_refused_with_the_blocks_that_make_the_most() {
    let mut d = Doc::new_metric();
    block(&mut d, "SEAT", (0..1001).map(|n| (on("0"), line((0.0, f64::from(n)), (500.0, f64::from(n))))).collect());
    block(&mut d, "DESK", vec![(on("0"), line((0.0, 0.0), (1200.0, 0.0)))]);
    for n in 0..400 {
        d.add(&Space::Model, on("0"), placed("SEAT", (f64::from(n) * 600.0, 0.0))).expect("added");
    }
    d.add(&Space::Model, on("0"), placed("DESK", (0.0, 5000.0))).expect("added");
    let (o, m) = open(&d);
    let refused = counts(&o, &m, r#"{"open": [{"kinds": ["block"]}]}"#).expect_err("refused");
    assert!(refused.contains("would take 401 placements apart into 400401 items, more than the 400000 it may"), "{refused}");
    assert!(refused.contains("The blocks that make the most: SEAT (400 placed, 400400 items), DESK (1 placed, 1 items)"), "{refused}");
    assert!(
        look(&o, &m, r#"{"open": [{"kinds": ["block"]}]}"#, 300).is_err() && cut(&o, &m, r#"{"open": [{"kinds": ["block"]}]}"#).is_err()
    );
    // The seats left closed (the reader drops them by name), the rest opens.
    let c = counts(&o, &m, r#"{"open": [{"blocks": ["DESK"]}]}"#).expect("counted");
    assert_eq!((c["total"].as_u64(), &c["opened"]), (Some(401), &json!({"placements": 1, "items": 1})));
    assert_eq!(c["blocks"][0]["holds"], 1001);
}

/// A real office drawing (14 MB, 99,221 items), when it is on this computer: opened as the worker opens it, its
/// fullest 40 m by 30 m found by counting, that part extracted and indexed again. Says how long each step took.
/// `cargo test -p bimcraft-drawing-web -- --ignored --nocapture a_real_drawing`
#[test]
#[ignore = "reads a large drawing from this computer's Downloads"]
fn a_real_drawing_s_plan_is_counted_extracted_and_indexed_again() {
    let path = std::env::var("READER_TEST_DWG").unwrap_or_else(|_| r"C:\Users\al_mi\Downloads\School DWG.dwg".into());
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("no drawing at {path}: nothing tested");
        return;
    };
    let timed = |what: &str, t: std::time::Instant| eprintln!("{what}: {:.2} s", t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let (pack, summary) = cadcraft_index::index(&bytes, "school.dwg").expect("indexed");
    timed("read and indexed", t);
    let t = std::time::Instant::now();
    let o = Opened::unpack(&pack).expect("opened");
    timed("opened from its pack", t);
    let t = std::time::Instant::now();
    let m = Measured::of(&o);
    timed("measured (once, at the first question)", t);
    let t = std::time::Instant::now();
    let all = counts(&o, &m, "").expect("counted");
    timed("counts of the whole", t);
    assert_eq!(all["total"], summary["items"]);
    eprintln!("unit {} ({} mm), extents {}, offset {}", summary["unit"], summary["mm"], summary["extents"], summary["offset"]);
    eprintln!(
        "{} items, {} layers, {} colours, {} blocks listed and {} more",
        all["total"],
        all["layers"].as_array().map_or(0, Vec::len),
        all["colors"].as_array().map_or(0, Vec::len),
        all["blocks"].as_array().map_or(0, Vec::len),
        all["blocks_more"]
    );
    // The fullest 40 m by 30 m of its bulk (all of it is some 100 m by 55 m), on a 10 m grid.
    let ext: [[f64; 2]; 2] = serde_json::from_value(summary["extents"].clone()).expect("extents");
    let t = std::time::Instant::now();
    let (mut best, mut most, mut asked) = ([[0.0; 2]; 2], 0_u64, 0);
    let mut y = ext[0][1];
    while y < ext[1][1] {
        let mut x = ext[0][0];
        while x < ext[1][0] {
            let b = [[x, y], [x + 40_000.0, y + 30_000.0]];
            let n = counts(&o, &m, &json!({"box": b}).to_string()).expect("counted")["total"].as_u64().unwrap_or(0);
            asked += 1;
            if n > most {
                (best, most) = (b, n);
            }
            x += 10_000.0;
        }
        y += 10_000.0;
    }
    timed(&format!("{asked} boxes counted"), t);
    eprintln!("the fullest: {most} items in {best:?}");
    assert!(most > 1000);
    let t = std::time::Instant::now();
    let p = look(&o, &m, &json!({"box": best}).to_string(), 1600).expect("drawn");
    timed(&format!("look, own colours ({} by {}, {} KB)", p.about["width"], p.about["height"], p.png.len() / 1024), t);
    let t = std::time::Instant::now();
    let p = look(&o, &m, &json!({"box": best, "color_by": "layer"}).to_string(), 1600).expect("drawn");
    timed(&format!("look, by layer ({} layers in the legend)", p.about["legend"].as_array().map_or(0, Vec::len)), t);
    // The part as it is, its dimensions (with their blocks and styles), hatches and leaders too, written and read again.
    let t = std::time::Instant::now();
    let (dwg, about) = extract(&o, &m, &json!({"box": best}).to_string(), "2018").expect("extracted whole");
    timed(&format!("extract of the box with nothing dropped ({} items, {} KB)", about["items"], dwg.len() / 1024), t);
    let (pack1, _) = cadcraft_index::index(&dwg, "part.dwg").expect("indexed again");
    let o1 = Opened::unpack(&pack1).expect("opened");
    let c1 = counts(&o1, &Measured::of(&o1), "").expect("counted");
    assert_eq!(c1["kinds"], about["kinds"]);
    assert_eq!(c1["total"], most);
    eprintln!("its kinds: {}", c1["kinds"]);
    // The large blocks in the box (a floor's columns, furniture, annotation, each placed once) opened: what they hold
    // is counted on its own layers, the columns' lines on S-COLS.
    let lines_on = |c: &Value, layer: &str| -> u64 {
        c["layers"].as_array().and_then(|l| l.iter().find(|l| l["name"] == layer)).map_or(0, |l| l["kinds"]["line"].as_u64().unwrap_or(0))
    };
    let closed = counts(&o, &m, &json!({"box": best}).to_string()).expect("counted");
    let large: Vec<&Value> =
        closed["blocks"].as_array().expect("blocks").iter().filter(|b| b["holds"].as_u64().unwrap_or(0) >= 200).collect();
    let told: Vec<String> =
        large.iter().map(|b| format!("{} holds {}, {} placed, size {}", b["name"], b["holds"], b["placed"], b["size"])).collect();
    eprintln!("blocks of 200 items or more in the box: {}", told.join("; "));
    let names: Vec<&str> = large.iter().filter_map(|b| b["name"].as_str()).collect();
    let t = std::time::Instant::now();
    let opened = counts(&o, &m, &json!({"open": [{"blocks": names}], "box": best}).to_string()).expect("counted");
    timed(&format!("counts with those opened ({})", opened["opened"]), t);
    eprintln!(
        "items in the box: {} closed, {} opened; lines on S-COLS: {} closed, {} opened",
        closed["total"],
        opened["total"],
        lines_on(&closed, "S-COLS"),
        lines_on(&opened, "S-COLS")
    );
    assert!(opened["opened"]["placements"].as_u64().unwrap_or(0) > 0);
    assert!(opened["kinds"]["line"].as_u64() > closed["kinds"]["line"].as_u64());
    // By size, in the whole drawing: every placement 2 m or more on its longer side as this drawing measures (it says
    // it is in inches, and its sheets come out some 8 m wide), and the columns' lines on S-COLS.
    let all_lines = lines_on(&all, "S-COLS");
    let t = std::time::Instant::now();
    let by_size = counts(&o, &m, &json!({"open": [{"kinds": ["block"], "min_size": 2000}]}).to_string()).expect("counted");
    timed(&format!("counts of the whole with the 78 largest placements opened ({})", by_size["opened"]), t);
    eprintln!("lines on S-COLS in the whole: {} closed, {} opened", all_lines, lines_on(&by_size, "S-COLS"));
    assert!(lines_on(&by_size, "S-COLS") > all_lines);
    let t = std::time::Instant::now();
    let p = look(&o, &m, &json!({"open": [{"kinds": ["block"], "min_size": 2000}], "keep": [{"layers": ["S-COLS"]}]}).to_string(), 1600)
        .expect("drawn");
    timed(&format!("look at S-COLS alone, opened ({} items)", p.about["items"]), t);
    let t = std::time::Instant::now();
    let refused = counts(&o, &m, &json!({"open": [{"kinds": ["block"]}], "depth": 4}).to_string());
    timed(
        &format!(
            "every block opened four deep: {}",
            refused.map_or_else(|e| e.chars().take(260).collect::<String>(), |c| c["opened"].to_string())
        ),
        t,
    );
    // Its hatches and dimensions dropped, its blocks to one layer, its origin to the box's corner.
    let params = json!({
        "box": best,
        "drop": [{"kinds": ["hatch", "dimension"]}],
        "sort": [{"to": "X-BLOCKS", "kinds": ["block"]}],
        "move": [-best[0][0], -best[0][1]],
    })
    .to_string();
    let t = std::time::Instant::now();
    let (part, about) = cut(&o, &m, &params).expect("cut");
    timed("cut (the part as a drawing)", t);
    eprintln!(
        "the part: {} of {} blocks, {} of {} layers",
        part.blocks.len(),
        o.drawing.blocks.len(),
        part.layers.len(),
        o.drawing.layers.len()
    );
    let t = std::time::Instant::now();
    let (dwg, about2) = extract(&o, &m, &params, "2018").expect("extracted");
    timed(&format!("extract ({} items, {} KB)", about["items"], dwg.len() / 1024), t);
    assert_eq!(about, about2);
    let t = std::time::Instant::now();
    let (pack2, summary2) = cadcraft_index::index(&dwg, "part.dwg").expect("indexed again");
    timed("the part read and indexed again", t);
    assert_eq!(summary2["items"], about["items"]);
    let o2 = Opened::unpack(&pack2).expect("opened");
    let m2 = Measured::of(&o2);
    let c = counts(&o2, &m2, "").expect("counted");
    assert_eq!(c["kinds"], about["kinds"]);
    assert!(c["kinds"].get("hatch").is_none() && c["kinds"].get("dimension").is_none());
    let blocks = c["kinds"]["block"].as_u64().unwrap_or(0);
    assert_eq!(
        c["layers"].as_array().and_then(|l| l.iter().find(|l| l["name"] == "X-BLOCKS")).map(|l| l["items"].as_u64().unwrap_or(0)),
        Some(blocks)
    );
    assert_eq!(summary2["sheets"], json!([]));
    // Moved to its corner: its extents begin near 0, 0 and are no wider than the box and what reaches out of it.
    eprintln!("its extents: {} (asked: {})", c["extents"], about["extents"]);
    assert_eq!(c["extents"], about["extents"]);
}

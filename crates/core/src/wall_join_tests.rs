//! Walls joined as Revit's: a wall ending on another's side runs its layers into it by rank (`walls/join.rs`), and the
//! wall running on is broken where they pass, in what a plan draws of each layer.

use serde_json::{Value, json};

use crate::engine::Session;
use crate::geom::P2;
use crate::walls;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A room of four exterior walls (brick 100, insulation 60, block 140, plaster 15, drawn clockwise so the brick is out) and a
/// partition (plaster 15, `core` 100, plaster 15) from the middle of the south wall up. The south wall and the partition.
fn room(core: &str) -> (Session, u64, u64) {
    let mut s = Session::new();
    for name in ["Brick", "Insulation", "Block", "Plaster", "Stud"] {
        run(&mut s, "material", json!({"name": name}));
    }
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Ext", "layers": [
        {"function": "finish", "material": "Brick", "thickness": 100},
        {"function": "thermal", "material": "Insulation", "thickness": 60},
        {"function": "structure", "material": "Block", "thickness": 140},
        {"function": "finish2", "material": "Plaster", "thickness": 15}]}),
    );
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Part", "layers": [
        {"function": "finish2", "material": "Plaster", "thickness": 15},
        {"function": "structure", "material": core, "thickness": 100},
        {"function": "finish2", "material": "Plaster", "thickness": 15}]}),
    );
    let ids =
        run(&mut s, "wall", json!({"points": [[0, 0], [0, 2400], [3000, 2400], [3000, 0]], "closed": true, "type": "Ext"}))["ids"].clone();
    let south = ids[3].as_u64().unwrap();
    let part = run(&mut s, "wall", json!({"points": [[1500, 0], [1500, 1200]], "type": "Part"}))["ids"][0].as_u64().unwrap();
    (s, south, part)
}

/// The lowest and highest y of `ring`.
fn span(ring: &[P2]) -> (f64, f64) {
    ring.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)))
}

/// Whether a line of `g` lies along y = `y` across x = `x`.
fn line_at(g: &walls::LayerGeometry, x: f64, y: f64) -> bool {
    g.interfaces().any(|l| {
        l.windows(2).any(|w| (w[0].1 - y).abs() < 1e-6 && (w[1].1 - y).abs() < 1e-6 && w[0].0.min(w[1].0) < x && x < w[0].0.max(w[1].0))
    })
}

#[test]
fn a_partitions_core_passes_the_plaster_and_stops_on_the_block() {
    let (s, south, part) = room("Block");
    // The south wall stands on its centre line, 315 thick, its plaster from 142.5 to 157.5 inside the room.
    let p = walls::layer_geometry(&s.model, part, None);
    let lowest = |k: usize| p.bands(k).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min);
    assert_eq!([lowest(0), lowest(1), lowest(2)], [157.5, 142.5, 157.5], "its plaster to the face, its core through the plaster");
    // The south wall's plaster is in two pieces either side of that core; its other layers run on whole.
    let g = walls::layer_geometry(&s.model, south, None);
    assert_eq!([0, 1, 2, 3].map(|k| g.bands(k).len()), [1, 1, 1, 2]);
    let gap: Vec<(f64, f64)> =
        g.bands(3).iter().map(|r| r.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)))).collect();
    assert!(gap.iter().any(|(_, hi)| (hi - 1450.0).abs() < 1e-6) && gap.iter().any(|(lo, _)| (lo - 1550.0).abs() < 1e-6), "{gap:?}");
    // The wall's own body is as it was: the partition's core lies in its plaster's place.
    assert_eq!(walls::outline(&s.model, south).len(), walls::cap(&s.model, south, false).len() + walls::cap(&s.model, south, true).len());
}

#[test]
fn layers_of_one_material_join_with_no_line_and_of_two_with_one() {
    // Block on block: one core, no line where the partition's ends on the wall's, nor the wall's own line between its
    // block and its plaster across it.
    let (s, south, part) = room("Block");
    let (g, p) = (walls::layer_geometry(&s.model, south, None), walls::layer_geometry(&s.model, part, None));
    assert!(!line_at(&p, 1500.0, 142.5) && !line_at(&g, 1500.0, 142.5));
    // That line is there either side of the partition, and the partition's plaster ends on the wall's with none: one material.
    assert!(line_at(&g, 1000.0, 142.5) && line_at(&g, 2000.0, 142.5));
    assert!(!line_at(&p, 1440.0, 157.5));
    // The sides of the core through the plaster are block against plaster: lines.
    let side = |x: f64| p.interfaces().any(|l| l.iter().all(|q| (q.0 - x).abs() < 1e-6) && l.iter().any(|q| (q.1 - 142.5).abs() < 1e-6));
    assert!(side(1450.0) && side(1550.0));
    // A stud core against the block: a line where it ends.
    let (s, _, part) = room("Stud");
    assert!(line_at(&walls::layer_geometry(&s.model, part, None), 1500.0, 142.5));
}

#[test]
fn the_wall_running_on_gives_up_its_plaster_where_a_partitions_core_passes_into_it() {
    let (s, south, part) = room("Block");
    // In plan the two walls' solids share nothing: the partition's core takes 100 by 15 of the wall's plaster.
    let area = |id: u64| walls::prisms(&s.model, id, false).iter().map(|p| crate::geom::area(&p.ring).abs()).sum::<f64>();
    let whole = crate::geom::area(&walls::outline(&s.model, south)).abs();
    assert!((area(south) - (whole - 100.0 * 15.0)).abs() < 1.0, "{} of {whole}", area(south));
    assert!((area(part) - crate::geom::area(&walls::outline(&s.model, part)).abs()).abs() < 1.0);
    let inside = |id: u64, q: P2| walls::prisms(&s.model, id, false).iter().any(|p| crate::geom::contains(&p.ring, q));
    // In the plaster's depth under the partition's core, and beside it.
    assert!(inside(part, P2(1500.0, 150.0)) && !inside(south, P2(1500.0, 150.0)));
    assert!(inside(south, P2(1440.0, 150.0)) && !inside(part, P2(1440.0, 150.0)));
    // The block behind it is the wall's still.
    assert!(inside(south, P2(1500.0, 100.0)));
}

#[test]
fn a_low_partition_leaves_the_wall_its_plaster_over_its_top() {
    let (mut s, south, part) = room("Block");
    run(&mut s, "modify", json!({"id": part, "top_level": null, "height": 1000}));
    assert_eq!(s.model.top_z(s.model.wall(part).unwrap()), 1000.0);
    // Over the partition's top the wall's plaster is whole again: a piece of it from 1000 up, where the core passes below.
    let over: Vec<(f64, f64)> = walls::prisms(&s.model, south, false)
        .iter()
        .filter(|p| crate::geom::contains(&p.ring, P2(1480.0, 147.0)))
        .map(|p| (p.bottom.at(P2(1480.0, 147.0)), p.top.at(P2(1480.0, 147.0))))
        .collect();
    let top = s.model.top_z(s.model.wall(south).unwrap());
    assert_eq!(over, [(1000.0, top)]);
}

/// Two 200 mm walls of 4000 meeting at the origin, the first made running east to it, the second north from it.
fn corner() -> (Session, u64, u64) {
    let mut s = Session::new();
    let ids = run(&mut s, "wall", json!({"points": [[4000, 0], [0, 0], [0, 4000]]}))["ids"].clone();
    (s, ids[0].as_u64().unwrap(), ids[1].as_u64().unwrap())
}

fn plan_area(s: &Session, id: u64) -> f64 {
    crate::geom::area(&walls::outline(&s.model, id)).abs()
}

#[test]
fn the_wall_joins_tool_sets_which_wall_runs_through_and_the_kind_of_join() {
    let (mut s, a, b) = corner();
    // The default Butt: the wall made first runs through, 100 past the node; the other stops 100 short of it.
    assert_eq!((plan_area(&s, a), plan_area(&s, b)), (820_000.0, 780_000.0));
    // Next: the other order. Previous: back again. Each one undo step, kept with the model.
    let r = run(&mut s, "wall_join", json!({"walls": [a, b], "next": true}));
    assert_eq!((r["kind"].clone(), r["through"].clone(), r["ends"].clone()), (json!("butt"), json!(b), json!(["end", "start"])));
    assert_eq!((plan_area(&s, a), plan_area(&s, b)), (780_000.0, 820_000.0));
    assert_eq!(run(&mut s, "wall_join", json!({"walls": [b, a], "previous": true}))["through"], json!(a));
    run(&mut s, "wall_join", json!({"walls": [a, b], "through": b}));
    assert_eq!(plan_area(&s, b), 820_000.0);
    run(&mut s, "undo", json!({}));
    assert_eq!(plan_area(&s, a), 820_000.0);
    // A Mitre: both cut on the line from the inner corner to the outer, each its length by its thickness.
    run(&mut s, "wall_join", json!({"walls": [a, b], "kind": "Miter", "display": "Don't Clean Join"}));
    assert_eq!((plan_area(&s, a), plan_area(&s, b)), (800_000.0, 800_000.0));
    let kept = &s.model.wall_joins[0];
    assert_eq!((kept.kind, kept.display), (crate::model::JoinKind::Mitre, crate::model::JoinDisplay::DontClean));
    // It is in the file, which an older build would not read, and comes back.
    assert_eq!(s.model.written_format(), crate::model::WALL_FORMAT);
    let (back, left_out) = crate::model::Model::read(&s.model.to_json().unwrap()).unwrap();
    assert_eq!((back.wall_joins.clone(), left_out), (s.model.wall_joins.clone(), Vec::<String>::new()));
    // Reset: the default again, and nothing kept.
    run(&mut s, "wall_join", json!({"walls": [a, b], "reset": true}));
    assert!(s.model.wall_joins.is_empty() && plan_area(&s, a) == 820_000.0);
    // A join goes with its walls.
    run(&mut s, "wall_join", json!({"walls": [a, b], "kind": "mitre"}));
    run(&mut s, "delete", json!({"ids": [b]}));
    assert!(s.model.wall_joins.is_empty());
}

#[test]
fn the_wall_joins_tool_refuses_what_is_no_join_of_two_ends() {
    let (mut s, a, b) = corner();
    let t = run(&mut s, "wall", json!({"points": [[2000, 0], [2000, 3000]]}))["ids"][0].as_u64().unwrap();
    for (p, why) in [
        (json!({"walls": [a, t], "kind": "mitre"}), "a T join is a butt"),
        (json!({"walls": [a, a]}), "do not join"),
        (json!({"walls": [a]}), "`walls` is the two walls that meet"),
        (json!({"walls": [a, b], "kind": "lap"}), "`kind` is butt, mitre or square-off"),
        (json!({"walls": [a, b], "through": t}), "`through` is one of the two walls"),
        (json!({"walls": [a, b], "display": "tidy"}), "`display` is clean, dont-clean or view-setting"),
    ] {
        let before = s.model.clone();
        let e = s.execute("wall_join", &p).expect_err(&format!("{p} should fail"));
        assert!(e.contains(why), "{e}");
        assert_eq!(*s.model, *before);
    }
}

#[test]
fn allow_join_is_each_ends_own() {
    let (mut s, a, b) = corner();
    // The corner end of the first wall set not to join (Revit's Disallow Join): it stands square at the node, and so does
    // the wall that met it there.
    run(&mut s, "modify", json!({"id": a, "join_end": false}));
    assert_eq!((plan_area(&s, a), plan_area(&s, b)), (800_000.0, 800_000.0));
    assert_eq!(walls::cap(&s.model, a, true).len(), 2);
    // Its other end joins still: a wall drawn to it there butts it.
    let c = run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 3000]]}))["ids"][0].as_u64().unwrap();
    assert_eq!((plan_area(&s, a), plan_area(&s, c)), (820_000.0, 580_000.0));
    assert_eq!(s.model.written_format(), crate::model::WALL_FORMAT);
    // Allowed again, the corner joins again.
    run(&mut s, "modify", json!({"id": a, "join_end": true}));
    assert_eq!((plan_area(&s, a), plan_area(&s, b)), (840_000.0, 780_000.0));
}

#[test]
fn a_square_off_ends_the_first_wall_at_90_degrees_where_a_butt_runs_it_to_a_point() {
    // Two 200 mm walls at 60 degrees: the first east to the origin, the second on from it at 60 degrees to it.
    let mut s = Session::new();
    let (x, y) = (4000.0 * 60f64.to_radians().cos(), 4000.0 * 60f64.to_radians().sin());
    let ids = run(&mut s, "wall", json!({"points": [[4000, 0], [0, 0], [x, y]]}))["ids"].clone();
    let (a, b) = (ids[0].as_u64().unwrap(), ids[1].as_u64().unwrap());
    // A Butt: the first runs on along the second's far face to a point, far past the node.
    let reach = |s: &Session| walls::cap(&s.model, a, true).iter().map(|p| -p.0).fold(f64::NEG_INFINITY, f64::max);
    let butt = reach(&s);
    assert!((butt - (100.0 / 60f64.to_radians().sin() + 100.0 / 60f64.to_radians().tan())).abs() < 1e-6, "{butt}");
    let butted = plan_area(&s, a) + plan_area(&s, b);
    // Squared: its end is one line square to it, where the second's far face meets its nearer face; the point is gone.
    run(&mut s, "wall_join", json!({"walls": [a, b], "kind": "square-off"}));
    let cap = walls::cap(&s.model, a, true);
    let square = reach(&s);
    assert!((square - (100.0 / 60f64.to_radians().sin() - 100.0 / 60f64.to_radians().tan())).abs() < 1e-6, "{square} {cap:?}");
    assert!(cap.iter().all(|p| (p.0 + square).abs() < 1e-6), "one line across it: {cap:?}");
    assert!(plan_area(&s, a) + plan_area(&s, b) < butted, "less the point");
    // At a square corner there is nothing to square off: a Butt.
    let (mut s, a, b) = corner();
    run(&mut s, "wall_join", json!({"walls": [a, b], "kind": "square-off"}));
    assert_eq!((plan_area(&s, a), plan_area(&s, b)), (820_000.0, 780_000.0));
}

#[test]
fn a_join_set_not_to_clean_up_leaves_the_layers_whole_and_draws_the_line_they_butt_on() {
    let (mut s, south, _) = room("Block");
    let ids: Vec<u64> = s.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect();
    // The south wall is the last of the four made: at its corner with the east wall, made before it, it stops.
    let east = ids[2];
    let cleaned = walls::layer_geometry(&s.model, south, None);
    assert_eq!(cleaned.butts().count(), 0);
    run(&mut s, "wall_join", json!({"walls": [south, east], "display": "dont-clean"}));
    let g = walls::layer_geometry(&s.model, south, None);
    // One line across the wall where it stops against the east wall's inside face, and every layer ends on it.
    let butt: Vec<&[P2]> = g.butts().collect();
    assert_eq!(butt.len(), 1);
    let x = butt[0][0].0;
    assert!(butt[0].iter().all(|p| (p.0 - x).abs() < 1e-6) && (butt[0][0].1 - butt[0][1].1).abs() > 314.0, "{butt:?}");
    for k in 0..4 {
        let reach = g.bands(k).iter().flatten().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
        assert!((reach - x).abs() < 1e-6, "layer {k} ends on it: {reach} {x}");
    }
    // Cleaned again, the brick wraps the corner once more.
    run(&mut s, "wall_join", json!({"walls": [south, east], "display": "clean"}));
    assert_eq!(walls::layer_geometry(&s.model, south, None).butts().count(), 0);
}

#[test]
fn layers_wrap_at_a_walls_free_ends_and_at_its_windows_jambs_but_not_where_it_joins() {
    let mut s = Session::new();
    for name in ["Brick", "Insulation", "Block", "Plaster"] {
        run(&mut s, "material", json!({"name": name}));
    }
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Ext", "wrap_ends": "exterior", "wrap_inserts": "both", "layers": [
        {"function": "finish", "material": "Brick", "thickness": 100, "wraps": true},
        {"function": "thermal", "material": "Insulation", "thickness": 60, "wraps": true},
        {"function": "structure", "material": "Block", "thickness": 140},
        {"function": "finish2", "material": "Plaster", "thickness": 15, "wraps": true}]}),
    );
    run(&mut s, "type", json!({"category": "window", "name": "W", "width": 1000, "height": 1200, "sill": 900}));
    // East along y = 0 on its exterior face (the brick from y = 0 down to -100, the block from -160 to -300), free at its
    // start, a window in its middle, and a wall joined to its end.
    let a = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Ext", "location": "exterior-finish"}))["ids"][0]
        .as_u64()
        .unwrap();
    run(&mut s, "window", json!({"wall": a, "offset": 2000, "type": "W"}));
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, -3000]], "type": "Ext", "location": "exterior-finish"}));
    let g = walls::layer_geometry(&s.model, a, None);
    let holds = |k: usize, x: f64, y: f64| g.bands(k).iter().any(|r| crate::geom::contains(r, P2(x, y)));
    // The free end: the brick turns down it across the block's end, the insulation inside the brick, the block behind both.
    assert!(holds(0, 50.0, -230.0) && holds(1, 130.0, -230.0) && holds(2, 200.0, -230.0));
    assert!(!holds(2, 50.0, -230.0) && !holds(2, 130.0, -230.0), "the block stops short of the returns");
    // The jamb at 1500, both sides wrapping: the brick to the block's middle (-230) from outside, the plaster from inside.
    assert!(holds(0, 1450.0, -200.0) && holds(3, 1495.0, -260.0) && !holds(0, 1450.0, -260.0));
    // In the window's width there is no layer at all.
    assert!((0..4).all(|k| !holds(k, 2000.0, -230.0)));
    // The joined end is its join's: no return there, the brick running on to the corner.
    assert!(holds(0, 3950.0, -50.0) && !holds(0, 3950.0, -230.0));
}

#[test]
fn a_wall_joins_two_walls_meeting_in_line_as_it_would_the_one_wall() {
    // The south wall of `room` drawn in two lengths that meet where the partition comes in.
    let mut s = Session::new();
    for name in ["Brick", "Insulation", "Block", "Plaster"] {
        run(&mut s, "material", json!({"name": name}));
    }
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Ext", "layers": [
        {"function": "finish", "material": "Brick", "thickness": 100},
        {"function": "thermal", "material": "Insulation", "thickness": 60},
        {"function": "structure", "material": "Block", "thickness": 140},
        {"function": "finish2", "material": "Plaster", "thickness": 15}]}),
    );
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Part", "layers": [
        {"function": "finish2", "material": "Plaster", "thickness": 15},
        {"function": "structure", "material": "Block", "thickness": 100},
        {"function": "finish2", "material": "Plaster", "thickness": 15}]}),
    );
    let ids = run(&mut s, "wall", json!({"points": [[3000, 0], [1500, 0], [0, 0]], "type": "Ext"}))["ids"].clone();
    let (east, west) = (ids[0].as_u64().unwrap(), ids[1].as_u64().unwrap());
    let part = run(&mut s, "wall", json!({"points": [[1500, 0], [1500, 1200]], "type": "Part"}))["ids"][0].as_u64().unwrap();
    // The partition: its core through the plaster to the block, as against one wall.
    let p = walls::layer_geometry(&s.model, part, None);
    let lowest = |k: usize| p.bands(k).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min);
    assert_eq!([lowest(0), lowest(1), lowest(2)], [157.5, 142.5, 157.5]);
    // The two lengths stand square against each other at 1500, and each gives up its half of the plaster under the core.
    for (id, from, to) in [(east, 1550.0, 3000.0), (west, 0.0, 1450.0)] {
        assert_eq!(walls::cap(&s.model, id, id == east).len(), 2, "square at the node");
        let g = walls::layer_geometry(&s.model, id, None);
        let plaster: Vec<(f64, f64)> = g
            .bands(3)
            .iter()
            .map(|r| r.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.0), hi.max(q.0))))
            .collect();
        assert_eq!(plaster.len(), 1);
        assert!((plaster[0].0 - from).abs() < 1e-6 && (plaster[0].1 - to).abs() < 1e-6, "{plaster:?}");
        let block = g.bands(2).iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
        assert_eq!(
            block,
            (from.min(1500.0).min(if id == east { 1500.0 } else { 0.0 }), to.max(1500.0).min(if id == east { 3000.0 } else { 1500.0 }))
        );
    }
    // Their solids and the partition's share nothing: its core's 100 by 15 comes out of the plaster, half from each.
    let area = |id: u64| walls::prisms(&s.model, id, false).iter().map(|q| crate::geom::area(&q.ring).abs()).sum::<f64>();
    assert!((area(east) - (1500.0 * 315.0 - 50.0 * 15.0)).abs() < 1.0 && (area(west) - (1500.0 * 315.0 - 50.0 * 15.0)).abs() < 1.0);
}

#[test]
fn move_with_disjoin_leaves_what_the_wall_was_joined_to_as_it_is() {
    let wall = |s: &Session, id: u64| s.model.wall(id).map(|w| (w.start, w.end, w.join)).unwrap();
    // Joined, the second wall stretches after the first: its start goes with the corner.
    let (mut s, a, b) = corner();
    run(&mut s, "move", json!({"ids": [a], "by": [0, -500]}));
    assert_eq!(wall(&s, b), (P2(0.0, -500.0), P2(0.0, 4000.0), true));
    // Disjoined, it stays as it was, and the wall moved joins again whatever it comes to meet.
    let (mut s, a, b) = corner();
    run(&mut s, "move", json!({"ids": [a], "by": [0, -500], "disjoin": true}));
    assert_eq!(wall(&s, a), (P2(4000.0, -500.0), P2(0.0, -500.0), true));
    assert_eq!(wall(&s, b), (P2(0.0, 0.0), P2(0.0, 4000.0), true));
    run(&mut s, "undo", json!({}));
    assert_eq!(wall(&s, a), (P2(4000.0, 0.0), P2(0.0, 0.0), true));
}

#[test]
fn a_t_join_set_not_to_clean_leaves_both_walls_layers_whole_and_draws_the_line_they_butt_on() {
    let (mut s, south, part) = room("Block");
    let set = run(&mut s, "wall_join", json!({"walls": [south, part], "display": "dont-clean"}));
    assert_eq!(
        (set["walls"].clone(), set["ends"].clone(), set["through"].clone()),
        (json!([part, south]), json!(["start", "side"]), json!(south))
    );
    // The partition's layers all stop on the wall's face, and the wall's plaster runs on in one piece.
    let p = walls::layer_geometry(&s.model, part, None);
    let lowest = |k: usize| p.bands(k).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min);
    assert_eq!([lowest(0), lowest(1), lowest(2)], [157.5, 157.5, 157.5]);
    let g = walls::layer_geometry(&s.model, south, None);
    assert_eq!([0, 1, 2, 3].map(|k| g.bands(k).len()), [1, 1, 1, 1]);
    assert_eq!(p.butts().count(), 1, "the line the partition stops on");
    let whole = crate::geom::area(&walls::outline(&s.model, south)).abs();
    let area = walls::prisms(&s.model, south, false).iter().map(|q| crate::geom::area(&q.ring).abs()).sum::<f64>();
    assert!((area - whole).abs() < 1.0, "the wall's solid is whole: {area} of {whole}");
    // Its kind and its order are not for setting: a T join is a butt, the wall run into through.
    for wrong in [json!({"kind": "mitre"}), json!({"next": true}), json!({"through": part})] {
        let mut q = wrong.clone();
        q["walls"] = json!([south, part]);
        assert!(s.execute("wall_join", &q).is_err_and(|e| e.to_string().contains("T join")), "{wrong}");
    }
    // Clean Join, or reset: as it was.
    run(&mut s, "wall_join", json!({"walls": [part, south], "reset": true}));
    assert!(s.model.wall_joins.is_empty());
    let p = walls::layer_geometry(&s.model, part, None);
    assert_eq!(p.bands(1).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min), 142.5);
    // Two walls that do not touch have no join to set.
    let far = run(&mut s, "wall", json!({"points": [[9000, 9000], [9000, 12000]], "type": "Part"}))["ids"][0].as_u64().unwrap();
    assert!(s.execute("wall_join", &json!({"walls": [south, far], "display": "clean"})).is_err());
}

#[test]
fn a_walls_end_dragged_alone_leaves_its_join_and_several_walls_ends_at_a_point_go_together() {
    let (mut s, south, _) = room("Block");
    let walls_at = |s: &Session, q: P2| {
        s.model
            .elements
            .values()
            .filter(|e| matches!(e, crate::model::Element::Wall(w) if w.start.dist(q) < 1e-6 || w.end.dist(q) < 1e-6))
            .count()
    };
    // The south wall runs from (3000, 0) to (0, 0), where the west wall starts. Its end moved as `move` ever did: the
    // west wall's start follows it.
    run(&mut s, "move", json!({"ids": [south], "by": [0, -500], "end": "end"}));
    assert_eq!(walls_at(&s, P2(0.0, -500.0)), 2);
    run(&mut s, "undo", json!({}));
    // Dragged alone, as Revit's Drag Wall End: the west wall stays where it was.
    run(&mut s, "move", json!({"ids": [south], "by": [0, -500], "end": "end", "joined": false}));
    assert_eq!((walls_at(&s, P2(0.0, -500.0)), walls_at(&s, P2(0.0, 0.0))), (1, 1));
    run(&mut s, "undo", json!({}));
    // Both walls' ends there, by the point they share: they go together, in one step.
    let west = *s.model.elements.iter().find(|(_, e)| matches!(e, crate::model::Element::Wall(w) if w.start == P2(0.0, 0.0))).unwrap().0;
    run(&mut s, "move", json!({"ids": [south, west], "by": [-400, -500], "node": [0, 0]}));
    assert_eq!((walls_at(&s, P2(-400.0, -500.0)), walls_at(&s, P2(0.0, 0.0))), (2, 0));
    // A wall with no end at the point is refused, and nothing moves.
    assert!(s.execute("move", &json!({"ids": [south], "by": [10, 10], "node": [1500, 1500]})).is_err());
}

/// The room's south wall with a partition ending on it from each side at x = 1500: one up into the room, one down.
fn crossing(split: bool) -> (Session, Vec<u64>, u64, u64) {
    let (mut s, south, up) = room("Block");
    let down = run(&mut s, "wall", json!({"points": [[1500, 0], [1500, -1200]], "type": "Part"}))["ids"][0].as_u64().unwrap();
    let mut through = vec![south];
    if split {
        // The south wall in two lengths that meet where the partitions do: four ends at the point.
        let made = run(&mut s, "split", json!({"id": south, "at": [1500, 0]}));
        through = s
            .model
            .elements
            .iter()
            .filter(|(id, e)| {
                matches!(e, crate::model::Element::Wall(w) if w.start.1 == 0.0 && w.end.1 == 0.0) && **id != up && **id != down
            })
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(through.len(), 2, "{made}");
    }
    (s, through, up, down)
}

#[test]
fn walls_ending_on_a_wall_from_both_sides_each_join_it_as_a_t() {
    for split in [false, true] {
        let (s, through, up, down) = crossing(split);
        // Up into the room, the partition's core passes the wall's plaster to its block (142.5), its plaster stops on the
        // wall's face (157.5); down outside, its core passes the brick and the insulation to the block's other side (2.5).
        let lowest =
            |id: u64, k: usize| walls::layer_geometry(&s.model, id, None).bands(k).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min);
        let highest = |id: u64, k: usize| {
            walls::layer_geometry(&s.model, id, None).bands(k).iter().map(|r| span(r).1).fold(f64::NEG_INFINITY, f64::max)
        };
        assert_eq!([lowest(up, 0), lowest(up, 1), lowest(up, 2)], [157.5, 142.5, 157.5], "split {split}");
        assert_eq!([highest(down, 0), highest(down, 1), highest(down, 2)], [-157.5, 2.5, -157.5], "split {split}");
        // The wall run into gives up its brick, insulation and plaster where the cores pass, 100 wide, and keeps its block
        // whole; split in two there, its halves stand square against each other.
        let has = |k: usize, q: P2| {
            through.iter().any(|id| walls::layer_geometry(&s.model, *id, None).bands(k).iter().any(|r| crate::geom::contains(r, q)))
        };
        for (k, y) in [(0, -107.5), (1, -27.5), (3, 150.0)] {
            assert!(!has(k, P2(1500.0, y)) && has(k, P2(1440.0, y)) && has(k, P2(1560.0, y)), "layer {k}, split {split}");
        }
        assert!(has(2, P2(1440.0, 72.5)) && has(2, P2(1560.0, 72.5)) && (split || has(2, P2(1500.0, 72.5))), "split {split}");
    }
}

#[test]
fn a_partition_joins_a_wall_that_changes_type_where_it_meets_it() {
    // The room's south wall, its east half of a thicker block: two walls in line, of two types, and the partition at
    // the point where they meet.
    let (mut s, south, part) = room("Block");
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Thick", "layers": [
        {"function": "finish", "material": "Brick", "thickness": 100},
        {"function": "structure", "material": "Block", "thickness": 300},
        {"function": "finish2", "material": "Plaster", "thickness": 15}]}),
    );
    run(&mut s, "split", json!({"id": south, "at": [1500, 0]}));
    let halves: Vec<u64> = s
        .model
        .elements
        .iter()
        .filter(|(id, e)| matches!(e, crate::model::Element::Wall(w) if w.start.1 == 0.0 && w.end.1 == 0.0) && **id != part)
        .map(|(id, _)| *id)
        .collect();
    let [west, east] = halves[..] else { panic!("two halves: {halves:?}") };
    let (west, east) =
        if s.model.wall(west).unwrap().start.0.max(s.model.wall(west).unwrap().end.0) <= 1500.0 { (west, east) } else { (east, west) };
    run(&mut s, "modify", json!({"id": east, "type": "Thick"}));
    // The halves stand square against each other at x = 1500, each as thick as its type.
    let xs =
        |id: u64| walls::outline(&s.model, id).iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), q| (lo.min(q.0), hi.max(q.0)));
    assert_eq!((xs(west).1, xs(east).0), (1500.0, 1500.0));
    // The partition's core passes plaster to block, as on the one wall: past its own plaster's end.
    let p = walls::layer_geometry(&s.model, part, None);
    let lowest = |k: usize| p.bands(k).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min);
    assert!(lowest(1) < lowest(0) - 10.0, "{} {}", lowest(1), lowest(0));
    // And where it passes, the wall it joins gives up its plaster.
    let plaster = |id: u64, q: P2| {
        let g = walls::layer_geometry(&s.model, id, None);
        let k = s.model.types.wall[&s.model.wall(id).unwrap().type_name].layers.len() - 1;
        g.bands(k).iter().any(|r| crate::geom::contains(r, q))
    };
    assert!(plaster(west, P2(1000.0, 150.0)) && !plaster(west, P2(1475.0, 150.0)), "the west half's plaster stops at the core");
}

/// A session with the `Ext` wall type of `room`.
fn typed() -> Session {
    let mut s = Session::new();
    for name in ["Brick", "Insulation", "Block", "Plaster"] {
        run(&mut s, "material", json!({"name": name}));
    }
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Ext", "layers": [
        {"function": "finish", "material": "Brick", "thickness": 100},
        {"function": "thermal", "material": "Insulation", "thickness": 60},
        {"function": "structure", "material": "Block", "thickness": 140},
        {"function": "finish2", "material": "Plaster", "thickness": 15}]}),
    );
    s
}

#[test]
fn three_walls_ending_at_a_point_join_by_their_layers_each_to_the_one_on_either_side() {
    // Three walls of one type out of a point, 120 degrees apart, and three more a right angle and a half apart.
    for (n, angles) in [[90.0_f64, 210.0, 330.0], [0.0, 90.0, 225.0]].into_iter().enumerate() {
        let mut s = typed();
        let ids: Vec<u64> = angles
            .iter()
            .map(|a| {
                let (x, y) = (3000.0 * a.to_radians().cos(), 3000.0 * a.to_radians().sin());
                run(&mut s, "wall", json!({"points": [[0, 0], [x, y]], "type": "Ext"}))["ids"][0].as_u64().unwrap()
            })
            .collect();
        // Round the point every place is one layer of one wall: none held twice, none left empty where the walls stand.
        let mut blocks = 0;
        for i in -40..=40 {
            for j in -40..=40 {
                let q = P2(f64::from(i) * 15.0 + 3.7, f64::from(j) * 15.0 + 2.3);
                let (count, layer) = held(&s, &ids, q);
                assert!(count <= 1, "walls {n}: {count} layers at {q:?}");
                assert!(count == 1 || q.dist(P2(0.0, 0.0)) > 150.0, "walls {n}: nothing at {q:?}");
                blocks += usize::from(layer == Some(2) && q.dist(P2(0.0, 0.0)) < 60.0);
            }
        }
        // Their block, the structure, is one piece through the point; and no line is drawn inside it.
        assert!(blocks >= 40, "walls {n}: {blocks}");
        // Their blocks are one piece: from the middle of the first wall's block, 400 out, block all the way to the middle
        // of each of the others', step by step.
        let block = |i: i32, j: i32| held(&s, &ids, P2(f64::from(i) * 10.0 + 0.7, f64::from(j) * 10.0 + 0.3)) == (1, Some(2));
        let middle = |a: f64| {
            let q = P2(a.to_radians().cos(), a.to_radians().sin()) * 400.0 + P2(a.to_radians().sin(), -a.to_radians().cos()) * 72.5;
            ((q.0 / 10.0).round() as i32, (q.1 / 10.0).round() as i32)
        };
        let mut reached = std::collections::BTreeSet::from([middle(angles[0])]);
        let mut edge = vec![middle(angles[0])];
        while let Some((i, j)) = edge.pop() {
            for next in [(i + 1, j), (i - 1, j), (i, j + 1), (i, j - 1)] {
                if next.0.abs() <= 60 && next.1.abs() <= 60 && block(next.0, next.1) && reached.insert(next) {
                    edge.push(next);
                }
            }
        }
        assert!(block(middle(angles[0]).0, middle(angles[0]).1));
        for a in &angles[1..] {
            assert!(reached.contains(&middle(*a)), "walls {n}: the block of the wall at {a} degrees is apart from the first's");
        }
        let lines: Vec<Vec<P2>> = ids
            .iter()
            .flat_map(|id| walls::layer_geometry(&s.model, *id, None).interfaces().map(<[P2]>::to_vec).collect::<Vec<_>>())
            .collect();
        let inside = |q: P2| held(&s, &ids, q).1 == Some(2);
        for l in &lines {
            for w in l.windows(2) {
                let mid = P2((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0);
                let across = (w[1] - w[0]).unit().perp() * 2.0;
                assert!(!(inside(mid + across) && inside(mid - across)), "walls {n}: a line through the block at {mid:?}");
            }
        }
    }
}

/// How many layers of walls `ids` hold point `q`, and which layer the first is.
fn held(s: &Session, ids: &[u64], q: P2) -> (usize, Option<usize>) {
    let mut found = (0, None);
    for id in ids {
        let g = walls::layer_geometry(&s.model, *id, None);
        for k in 0..4 {
            let n = g.bands(k).iter().filter(|r| crate::geom::contains(r, q)).count();
            if n > 0 && found.1.is_none() {
                found.1 = Some(k);
            }
            found.0 += n;
        }
    }
    found
}

#[test]
fn a_curved_wall_joins_a_straight_one_by_its_layers_at_a_corner_and_ending_on_its_side() {
    // A straight wall east to (4000, 0) and a curved one on from there, a quarter turn up to (7000, 3000): a corner where
    // they run on smoothly is no corner, so the curve starts square to it, turning the other way: from (4000, 0) north.
    let mut s = typed();
    let straight = run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Ext"}))["ids"][0].as_u64().unwrap();
    let curved = run(&mut s, "wall", json!({"points": [[4000, 0], [1000, 3000]], "arc": 90, "type": "Ext"}))["ids"][0].as_u64().unwrap();
    let both = [straight, curved];
    assert!(s.model.wall(curved).is_some_and(|w| w.arc != 0.0));
    // Their layers are cut to each other: over the corner no place is held by two layers, and the corner itself is block.
    let mut twice = Vec::new();
    for i in 0..60 {
        for j in 0..60 {
            let q = P2(3700.0 + 10.0 * f64::from(i) + 0.37, -300.0 + 10.0 * f64::from(j) + 0.41);
            if held(&s, &both, q).0 > 1 {
                twice.push(q);
            }
        }
    }
    // (But for a hair along the curved wall's own boundaries, where a straight wall's layer ends on a chord of the curve:
    // under a millimetre for a layer this thick on a curve this wide.)
    let on_a_boundary = |q: &P2| [2842.5, 2942.5, 3002.5, 3142.5, 3157.5].iter().any(|r| (q.dist(P2(1000.0, 0.0)) - r).abs() < 1.0);
    twice.retain(|q| !on_a_boundary(q));
    assert!(twice.is_empty(), "{} places held twice, the first {:?}", twice.len(), twice.first());
    assert_eq!(held(&s, &both, P2(4070.0, -60.0)), (1, Some(2)), "where block meets block, one of the two walls holds it");
    assert_eq!(held(&s, &both, P2(3990.0, 5.0)), (1, Some(1)), "and insulation where theirs meet");
    // The curved wall's layers end on the straight wall's lines, not on its own square end: its plaster, on the outside
    // of the corner, runs past its start to the straight wall's plaster.
    let g = walls::layer_geometry(&s.model, curved, None);
    let low = g.bands(3).iter().map(|r| span(r).0).fold(f64::INFINITY, f64::min);
    assert!(low < -100.0, "its plaster reaches the outside of the corner: {low}");

    // A curved wall ending on a straight wall's side: its block passes the plaster, as a straight one's does.
    let mut s = typed();
    let south = run(&mut s, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Ext"}))["ids"][0].as_u64().unwrap();
    let stem = run(&mut s, "wall", json!({"points": [[3000, 0], [5000, -2000]], "arc": 90, "type": "Ext"}))["ids"][0].as_u64().unwrap();
    let g = walls::layer_geometry(&s.model, south, None);
    let pieces: Vec<usize> = (0..4).map(|k| g.bands(k).len()).collect();
    assert!(pieces.iter().any(|n| *n == 2), "the wall run into is broken where the curved wall's layers pass: {pieces:?}");
    let mut twice = 0;
    for i in 0..80 {
        for j in 0..40 {
            let q = P2(2600.0 + 10.0 * f64::from(i) + 0.37, -200.0 + 10.0 * f64::from(j) + 0.41);
            let hair = [1842.5, 1942.5, 2002.5, 2142.5, 2157.5].iter().any(|r| (q.dist(P2(5000.0, 0.0)) - r).abs() < 1.0);
            twice += usize::from(!hair && held(&s, &[south, stem], q).0 > 1);
        }
    }
    assert_eq!(twice, 0, "no place held by two layers where the curved wall ends on the straight one");
}

/// The three walls drawn in Revit 2025 on 19 November (its picture is with the walls design), for laying beside it: one
/// along the south, one up from its east end, one ending on it from the brick side.
#[test]
#[ignore]
fn shot_the_walls_drawn_in_revit() {
    let mut s = Session::new();
    for name in ["Brick", "Insulation", "Block", "Plaster"] {
        run(&mut s, "material", json!({"name": name}));
    }
    run(
        &mut s,
        "type",
        json!({"category": "wall", "name": "Ext", "layers": [
        {"function": "finish", "material": "Brick", "thickness": 102.5},
        {"function": "thermal", "material": "Insulation", "thickness": 75},
        {"function": "structure", "material": "Block", "thickness": 100},
        {"function": "finish2", "material": "Plaster", "thickness": 12.5}]}),
    );
    run(&mut s, "wall", json!({"points": [[0, 0], [3000, 0], [3000, 2000]], "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[1500, 2000], [1500, 0]], "type": "Ext"}));
    run(&mut s, "detail", json!({"view": 100000, "detail": "fine"}));
    let dir = std::env::var("SHOT").unwrap();
    std::fs::write(format!("{dir}/engine-walls.png"), crate::render::png(&crate::render::plan(&s.model, 100000, 1800, 1300)).unwrap())
        .unwrap();
}

#[test]
#[ignore]
fn shot_coarse_joins() {
    let (mut s, _, _) = room("Block");
    run(&mut s, "detail", json!({"view": 100000, "detail": "coarse"}));
    let dir = std::env::var("SHOT").unwrap();
    std::fs::write(format!("{dir}/coarse.png"), crate::render::png(&crate::render::plan(&s.model, 100000, 1400, 1100)).unwrap()).unwrap();
}

#[test]
fn a_wall_ending_on_a_curved_walls_side_joins_it_by_its_layers() {
    // A quarter of a circle of 3 m about (1000, 0), and a wall out from inside it, square to it, ending on its line.
    let mut s = typed();
    let curved = run(&mut s, "wall", json!({"points": [[4000, 0], [1000, 3000]], "arc": 90, "type": "Ext"}))["ids"][0].as_u64().unwrap();
    let stem = run(&mut s, "wall", json!({"points": [[2600, 1200], [3400, 1800]], "type": "Ext"}))["ids"][0].as_u64().unwrap();
    assert_eq!(walls::ends_on(&s.model, stem, true), Some(curved));
    // The curved wall is broken where the other's layers pass into it: its brick, inside, is in two pieces.
    let g = walls::layer_geometry(&s.model, curved, None);
    let pieces: Vec<usize> = (0..4).map(|k| g.bands(k).len()).collect();
    assert_eq!(pieces[0], 2, "{pieces:?}");
    assert_eq!(pieces[3], 1, "its plaster, outside, runs on whole: {pieces:?}");
    // The stem's block reaches the curved wall's block, past its brick and insulation: farther from the centre than the
    // curved wall's inner face.
    let p = walls::layer_geometry(&s.model, stem, None);
    let reach = |k: usize| p.bands(k).iter().flatten().map(|q| q.dist(P2(1000.0, 0.0))).fold(0.0, f64::max);
    assert!(reach(2) > 3000.0 && reach(0) < 2850.0, "block to {}, brick to {}", reach(2), reach(0));
    // No place is held by two layers (but for a hair along the curve, where a straight layer ends on a chord of it).
    let mut twice = Vec::new();
    for i in 0..60 {
        for j in 0..60 {
            let q = P2(3100.0 + 10.0 * f64::from(i) + 0.37, 1500.0 + 10.0 * f64::from(j) + 0.41);
            let hair = [2842.5, 2942.5, 3002.5, 3142.5, 3157.5].iter().any(|r| (q.dist(P2(1000.0, 0.0)) - r).abs() < 1.0);
            if !hair && held(&s, &[curved, stem], q).0 > 1 {
                twice.push(q);
            }
        }
    }
    assert!(twice.is_empty(), "{} places held twice, the first {:?}", twice.len(), twice.first());
    // And the curved wall's own curve is whole where nothing meets it: a place in its block, far from the join.
    assert_eq!(held(&s, &[curved, stem], P2(1000.0 + 3070.0 * 0.6, 3070.0 * 0.8)), (1, Some(2)));
}

/// How many stretches of line a plan of `s` draws along the line from `a` to `b`, within 3 mm of it.
fn inked(s: &Session, a: P2, b: P2) -> usize {
    let d = crate::render::plan(&s.model, 100_000, 1200, 900);
    let on = |q: P2| crate::geom::dist_to_segment(q, a, b) < 3.0;
    d.prims
        .iter()
        .filter_map(|p| match p {
            crate::render::Prim::Line { pts, .. } => Some(pts.iter().map(|q| d.view.mm(*q)).collect::<Vec<P2>>()),
            _ => None,
        })
        .map(|pts| pts.windows(2).filter(|w| w[0].dist(w[1]) > 1.0 && on(w[0]) && on(w[1])).count())
        .sum()
}

#[test]
fn a_coarse_view_set_to_clean_same_type_joins_shows_the_line_between_walls_of_two_types() {
    let (mut s, south, part) = room("Block");
    run(&mut s, "detail", json!({"view": 100000, "detail": "coarse"}));
    // The partition meets the south wall's inside face from x = 1435 to 1565 at y = 157.5: no line there while every join
    // is cleaned, Revit's default.
    let (a, b) = (P2(1440.0, 157.5), P2(1560.0, 157.5));
    assert_eq!(run(&mut s, "detail", json!({"view": 100000}))["wall_joins"], json!("all"));
    assert_eq!(inked(&s, a, b), 0);
    // Clean same type wall joins: the two are of two types, and the line shows.
    let set = run(&mut s, "detail", json!({"view": 100000, "wall_joins": "same-type"}));
    assert_eq!((set["detail"].clone(), set["wall_joins"].clone()), (json!("coarse"), json!("same-type")));
    assert!(inked(&s, a, b) > 0, "the line between the wall and the partition");
    // The join set to Clean Join cleans up whatever the view says.
    run(&mut s, "wall_join", json!({"walls": [south, part], "display": "clean"}));
    assert_eq!(inked(&s, a, b), 0);
    // A file that holds the setting is written in the format that knows it, and it is refused by name when wrong.
    assert_eq!(s.model.written_format(), crate::model::WALL_FORMAT);
    assert!(s.execute("detail", &json!({"view": 100000, "wall_joins": "some"})).is_err());
}

/// A curved wall at a corner with a straight one, and one ending on its side: a picture to look at.
#[test]
#[ignore]
fn shot_curved_joins() {
    let mut s = typed();
    run(&mut s, "wall", json!({"points": [[0, 0], [4000, 0]], "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[4000, 0], [1000, 3000]], "arc": 90, "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[1500, 0], [3000, -1500]], "arc": 90, "type": "Ext"}));
    run(&mut s, "detail", json!({"view": 100000, "detail": "fine"}));
    let dir = std::env::var("SHOT").unwrap();
    std::fs::write(format!("{dir}/curved.png"), crate::render::png(&crate::render::plan(&s.model, 100000, 1800, 1500)).unwrap()).unwrap();
}

/// Three walls ending at a point, 120 degrees apart and as a T's arms askew: a picture to look at.
#[test]
#[ignore]
fn shot_y_joins() {
    let mut s = typed();
    for a in [90.0_f64, 210.0, 330.0] {
        let (x, y) = (1400.0 * a.to_radians().cos(), 1400.0 * a.to_radians().sin());
        run(&mut s, "wall", json!({"points": [[0, 0], [x, y]], "type": "Ext"}));
    }
    // The three walls drawn in Revit: one in from the left, one up, one down to the right.
    run(&mut s, "wall", json!({"points": [[2600, 0], [4000, 0]], "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[4000, 0], [4000, 1400]], "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[4000, 0], [5100, -800]], "type": "Ext"}));
    run(&mut s, "detail", json!({"view": 100000, "detail": "fine"}));
    let dir = std::env::var("SHOT").unwrap();
    std::fs::write(format!("{dir}/y.png"), crate::render::png(&crate::render::plan(&s.model, 100000, 1800, 1000)).unwrap()).unwrap();
}

#[test]
#[ignore]
fn shot_y_close() {
    let mut s = typed();
    run(&mut s, "wall", json!({"points": [[-700, 0], [0, 0]], "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[0, 0], [0, 700]], "type": "Ext"}));
    run(&mut s, "wall", json!({"points": [[0, 0], [600, -430]], "type": "Ext"}));
    run(&mut s, "detail", json!({"view": 100000, "detail": "fine"}));
    let dir = std::env::var("SHOT").unwrap();
    std::fs::write(format!("{dir}/yc.png"), crate::render::png(&crate::render::plan(&s.model, 100000, 1400, 1300)).unwrap()).unwrap();
}

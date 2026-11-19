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
        (json!({"walls": [a, t]}), "do not meet end to end"),
        (json!({"walls": [a, a]}), "do not meet end to end"),
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

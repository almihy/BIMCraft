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

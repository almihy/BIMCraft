//! The walls that meet end to end with a wall: what Revit's Tab over a wall selects. A front end asks here, so any of them
//! (the desktop, the browser, an agent's "this wall run") gets the same chain.

use std::collections::BTreeSet;

use crate::geom::P2;
use crate::model::{Id, Model, Wall};

/// How near two wall ends must lie to meet, mm.
const MEET: f64 = 1.0;

fn ends(w: &Wall) -> [P2; 2] {
    [w.start, w.end]
}

/// Whether `a` and `b` meet at an end: on one level, both allowing joins, an end of each on the other's.
fn meets(a: &Wall, b: &Wall) -> bool {
    a.level == b.level && a.join && b.join && ends(a).iter().any(|p| ends(b).iter().any(|q| p.dist(*q) <= MEET))
}

/// Wall `id` and every wall joined to it, end to end and then end to end again (a room's four walls, a run of walls round a
/// curve), in id order. Walls that refuse joins (Allow Join off) stand alone, and so does anything that isn't a wall: the
/// chain is then `id` alone, or empty when `id` names nothing.
pub fn wall_chain(m: &Model, id: Id) -> Vec<Id> {
    let Some(first) = m.wall(id) else { return Vec::new() };
    let walls: Vec<(Id, &Wall)> = m.elements.iter().filter_map(|(k, _)| Some((*k, m.wall(*k)?))).collect();
    let mut chain = BTreeSet::from([id]);
    let mut frontier = vec![first];
    while let Some(w) = frontier.pop() {
        for (k, other) in &walls {
            if !chain.contains(k) && meets(w, other) {
                chain.insert(*k);
                frontier.push(other);
            }
        }
    }
    chain.into_iter().collect()
}

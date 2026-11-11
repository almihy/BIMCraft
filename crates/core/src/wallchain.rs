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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::engine::Session;

    fn walls(s: &Session) -> Vec<Id> {
        s.model.elements.iter().filter(|(_, e)| e.category() == "wall").map(|(id, _)| *id).collect()
    }

    #[test]
    fn a_room_of_walls_is_one_chain_and_a_lone_wall_is_its_own() {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true})).unwrap();
        s.execute("wall", &json!({"points": [[20000, 0], [26000, 0]]})).unwrap();
        let all = walls(&s);
        assert_eq!(all.len(), 5);
        let lone = all[4];
        for id in &all[..4] {
            assert_eq!(wall_chain(&s.model, *id), all[..4].to_vec());
        }
        assert_eq!(wall_chain(&s.model, lone), vec![lone]);
    }

    #[test]
    fn a_wall_that_refuses_joins_breaks_the_chain_and_what_is_no_wall_has_none() {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0, 0], [4000, 0], [4000, 4000], [8000, 4000]]})).unwrap();
        let all = walls(&s);
        assert_eq!(wall_chain(&s.model, all[0]), all);
        s.execute("modify", &json!({"ids": [all[1]], "join": false})).unwrap();
        assert_eq!(wall_chain(&s.model, all[0]), vec![all[0]]);
        assert_eq!(wall_chain(&s.model, all[1]), vec![all[1]]);
        assert!(wall_chain(&s.model, 424242).is_empty());
    }

    #[test]
    fn the_wall_chain_command_gives_the_agent_the_same_chain() {
        let mut s = Session::new();
        s.execute("wall", &json!({"points": [[0, 0], [8000, 0], [8000, 6000], [0, 6000]], "closed": true})).unwrap();
        s.execute("wall", &json!({"points": [[20000, 0], [26000, 0]]})).unwrap();
        let all = walls(&s);
        let got = s.execute("wall_chain", &json!({"id": all[1]})).unwrap();
        assert_eq!(got["id"], json!(all[1]));
        assert_eq!(got["ids"], json!(all[..4]));
        assert_eq!(got["count"], json!(4));
        // A lone wall is its own chain; a wall that refuses joins is too.
        assert_eq!(s.execute("wall_chain", &json!({"id": all[4]})).unwrap()["ids"], json!([all[4]]));
        s.execute("modify", &json!({"ids": [all[0]], "join": false})).unwrap();
        assert_eq!(s.execute("wall_chain", &json!({"id": all[0]})).unwrap()["ids"], json!([all[0]]));
        // It reads and changes nothing, and what is no wall, or nothing, is refused with a reason.
        let level = s.model.elements.iter().find(|(_, e)| e.category() == "level").map(|(id, _)| *id).unwrap();
        for bad in [json!({"id": level}), json!({"id": 424242}), json!({}), json!({"id": all[0], "extra": 1})] {
            assert!(s.execute("wall_chain", &bad).is_err(), "{bad}");
        }
    }
}

//! `wallchain`'s tests that drive the Session (moved out of `wallchain.rs`).

use crate::model::Id;

use serde_json::json;

use crate::engine::Session;
use crate::wallchain::*;

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

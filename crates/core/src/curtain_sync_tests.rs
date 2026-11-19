//! Curtain walls edited together (`sync`, `curtain.rs`): a wall's panels and mullions have the same GUIDs in every copy,
//! so two copies that lay one wall out make the same elements, never two of each.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::engine::Session;
use crate::ids::Guid;
use crate::model::{Element, Model};
use crate::sync::{self, Changes};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn two() -> (Session, Session) {
    let mut s = Session::new();
    run(&mut s, "new", json!({}));
    let text = s.model.to_json().unwrap();
    let open = |t: &str| {
        let mut x = Session::new();
        x.reset(Model::read(t).unwrap().0, None);
        x
    };
    (open(&text), open(&text))
}

fn send(before: &Model, s: &Session, to: &mut Session) -> sync::Merged {
    let c = sync::changes(before, &s.model).unwrap();
    let c = Changes::read(&c.to_json()).unwrap();
    to.merge(&c, &BTreeSet::new()).unwrap()
}

fn by_guid(m: &Model) -> BTreeMap<Guid, String> {
    m.elements
        .iter()
        .filter(|(_, e)| !sync::own(e))
        .map(|(id, e)| (m.elements.guid(*id).unwrap(), serde_json::to_string(e).unwrap()))
        .collect()
}

fn children(m: &Model) -> usize {
    m.elements.values().filter(|e| matches!(e, Element::CurtainPanel(_) | Element::Mullion(_))).count()
}

#[test]
fn a_storefront_drawn_in_one_copy_comes_to_the_other_with_its_types_panels_and_mullions() {
    let (mut a, mut b) = two();
    let before = a.model.clone();
    run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}));
    let got = send(&before, &a, &mut b);
    assert!(got.refused.is_empty() && got.waiting.is_empty(), "{:?} {:?}", got.refused, got.waiting);
    assert_eq!(by_guid(&a.model), by_guid(&b.model));
    assert_eq!(children(&b.model), 8 + 22);
    assert_eq!(b.model.check(), Ok(()));
    assert!(b.model.types.wall["Storefront"].is_curtain() && b.model.types.mullion.contains_key("50 x 150mm"));
}

#[test]
fn both_change_one_curtain_wall_at_once_and_neither_has_a_panel_twice() {
    let (mut a, mut b) = two();
    let start = a.model.clone();
    let wall =
        run(&mut a, "wall", json!({"points": [[0, 0], [6000, 0]], "type": "Storefront", "height": 3000}))["ids"][0].as_u64().unwrap();
    send(&start, &a, &mut b);
    let guid = a.model.guid(wall);
    let in_b = b.model.elements.id(guid).unwrap();
    // One lengthens it while the other makes it taller: each lays it out, with the same GUIDs for the same cells.
    let (a0, b0) = (a.model.clone(), b.model.clone());
    run(&mut a, "modify", json!({"id": wall, "end": [7500, 0]}));
    run(&mut b, "modify", json!({"id": in_b, "height": 5000}));
    let (a1, b1) = (a.model.clone(), b.model.clone());
    let to_b = Changes::read(&sync::changes(&a0, &a1).unwrap().to_json()).unwrap();
    let to_a = Changes::read(&sync::changes(&b0, &b1).unwrap().to_json()).unwrap();
    b.merge(&to_b, &BTreeSet::new()).unwrap();
    a.merge(&to_a, &BTreeSet::new()).unwrap();
    for m in [&a.model, &b.model] {
        let mut keys = BTreeSet::new();
        for e in m.elements.values() {
            let key = match e {
                Element::CurtainPanel(p) => format!("p {}", p.key),
                Element::Mullion(x) => x.key.clone(),
                _ => continue,
            };
            assert!(keys.insert(key.clone()), "{key} twice");
        }
    }
    // The next command on the wall in either copy settles it as the wall now is.
    run(&mut a, "modify", json!({"id": wall, "base_offset": 0}));
    assert_eq!(a.model.check(), Ok(()));
}

//! Pin and Unpin, as Revit's: a pinned element is held where it is. It cannot be moved, turned or deleted, by the window or by a
//! command (the agent acts for the person, who has the same limit): the refusal is Revit's own words and names `unpin`. A copy of
//! a pinned element is free. A project keeps its pins by the elements' GUIDs, so they survive a renumbering and go when their
//! element does.

use serde_json::{Value, json};

use super::{R, bad, ids_param, only};
use crate::curtain::{Mullion, Panel};
use crate::engine::Session;
use crate::model::{Element, Id, Model};

/// What a command refused to do to a pinned element: Revit's own failures say "Can't move pinned element." and "Can't rotate
/// pinned element."; and "Pinned objects were not deleted. To delete them, unpin them before using delete.".
#[derive(Clone, Copy)]
pub(super) enum Held {
    Move,
    Rotate,
    Delete,
}

/// `Ok` when none of `ids` is pinned, else Revit's refusal for `what`, naming the pinned elements and the command that frees them.
/// The agent acts for a person, who cannot move, turn or delete a pinned element in Revit either.
pub(super) fn refuse_pinned(c: &str, m: &Model, ids: &[Id], what: Held) -> Result<(), String> {
    let pinned = m.pinned_among(ids);
    if pinned.is_empty() {
        return Ok(());
    }
    let revit = match what {
        Held::Move => "Can't move pinned element.",
        Held::Rotate => "Can't rotate pinned element.",
        Held::Delete => "Pinned objects were not deleted. To delete them, unpin them before using delete.",
    };
    let list = pinned.iter().map(Id::to_string).collect::<Vec<_>>().join(", ");
    Err(format!(
        "{c}: {revit} Pinned: {list}. Run `unpin` with `ids: [{list}]` first{}.",
        if matches!(what, Held::Delete) { ", or leave them out of `ids`" } else { "" },
    ))
}

const HOST_WORDS: &str = "Some pinned objects were deleted because their parent or host object was deleted.";
const OWN_WORDS: &str = "Pinned objects deleted.";

/// `result` (an object) with `pinned`, those of `gone` that were pinned and went with their host (Revit warns "Some pinned objects
/// were deleted because their parent or host object was deleted."), and a note saying so. Nothing when none did.
pub(super) fn hosted_pins_deleted(result: Value, before: &Model, gone: &[Id], asked: &[Id]) -> Value {
    let went: Vec<Id> = by_hand(before, gone).into_iter().filter(|id| !asked.contains(id)).collect();
    pins_note(result, went, &[])
}

/// What `delete_type` did to pinned elements: a type's own pinned instances (`instances`) are deleted with it and Revit says
/// "Pinned objects deleted." (its EditingFailures list); those that hung on a deleted host keep the host words. `pinned` lists
/// both and the note both sentences, each when it applies.
pub(super) fn type_pins_deleted(result: Value, before: &Model, gone: &[Id], instances: &[Id]) -> Value {
    let pinned = by_hand(before, gone);
    let (own, hosted): (Vec<Id>, Vec<Id>) = pinned.into_iter().partition(|id| instances.contains(id));
    pins_note(result, hosted, &own)
}

/// Those of `ids` a person pinned: a curtain wall's panels and mullions count as pinned while their wall holds them
/// (`curtain::placed`), which is no pin anyone set, so nothing is said of them when they go with their wall.
fn by_hand(m: &Model, ids: &[Id]) -> Vec<Id> {
    ids.iter().copied().filter(|id| m.pinned.contains(&m.guid(*id))).collect()
}

/// Those of `ids` held on their own account: all but a curtain wall's panels and mullions that go with their wall (it is
/// among `ids`) and that nobody pinned by hand. What the wall placed counts as pinned only against being worked on alone.
pub(super) fn held_alone(m: &Model, ids: &[Id]) -> Vec<Id> {
    let all: std::collections::BTreeSet<Id> = ids.iter().copied().collect();
    let with_wall = |id: &Id| {
        m.elements.get(*id).and_then(crate::curtain::host_of).and_then(|h| m.elements.id(h)).is_some_and(|h| all.contains(&h))
            && !m.pinned.contains(&m.guid(*id))
    };
    ids.iter().copied().filter(|id| !with_wall(id)).collect()
}

/// Whether the element is a curtain wall's panel or mullion, which its wall pins and Unpin frees (`free`).
fn curtain_child(m: &Model, id: Id) -> bool {
    matches!(m.elements.get(id), Some(Element::CurtainPanel(_) | Element::Mullion(_)))
}

fn pins_note(mut result: Value, hosted: Vec<Id>, own: &[Id]) -> Value {
    if hosted.is_empty() && own.is_empty() {
        return result;
    }
    let mut all: Vec<Id> = own.iter().chain(&hosted).copied().collect();
    all.sort_unstable();
    let note = match (own.is_empty(), hosted.is_empty()) {
        (false, false) => format!("{OWN_WORDS} {HOST_WORDS}"),
        (false, true) => OWN_WORDS.to_string(),
        _ => HOST_WORDS.to_string(),
    };
    if let Some(o) = result.as_object_mut() {
        o.insert("pinned".into(), json!(all));
        o.insert("note".into(), json!(note));
    }
    result
}

pub(super) fn pin(s: &mut Session, p: &Value) -> R {
    let c = "pin";
    only(c, p, &["ids", "id"])?;
    let ids = ids_param(c, &s.model, p)?;
    let m = &s.model;
    let already = m.pinned_among(&ids);
    // A curtain wall's panel or mullion is pinned by being the wall's again: it goes back to what the wall's type gives it,
    // as Revit's does. One put on by hand where the type has none has nothing to be pinned to.
    let (children, others): (Vec<Id>, Vec<Id>) = ids.iter().partition(|id| curtain_child(m, **id));
    if let Some(id) =
        children.iter().find(|id| m.elements.get(**id).is_some_and(|e| !crate::curtain::placed(e) && !crate::curtain::typed(m, e)))
    {
        return Err(bad(
            c,
            format!(
                "mullion {id} was put on by hand where its curtain wall's type has no mullion, so there is nothing to pin it to: \
                 Revit pins a mullion only to what a curtain wall's type lays out. Leave it out of `ids`"
            ),
        ));
    }
    let guids: Vec<_> = others.iter().map(|id| m.guid(*id)).collect();
    if already.len() < ids.len() {
        let m = s.model_mut();
        for id in children {
            if let Some(Element::CurtainPanel(Panel { free, .. }) | Element::Mullion(Mullion { free, .. })) = m.elements.get_mut(id) {
                *free = false;
            }
        }
        m.pinned.extend(guids);
    }
    Ok(json!({"pinned": ids, "already": already}))
}

pub(super) fn unpin(s: &mut Session, p: &Value) -> R {
    let c = "unpin";
    only(c, p, &["ids", "id"])?;
    let ids = ids_param(c, &s.model, p)?;
    let was = s.model.pinned_among(&ids);
    if !was.is_empty() {
        let guids: Vec<_> = ids.iter().map(|id| s.model.guid(*id)).collect();
        // A panel or a mullion its curtain wall placed is freed: it keeps its type from here on, whatever the wall's says.
        let held: Vec<Id> = was.iter().copied().filter(|id| s.model.elements.get(*id).is_some_and(crate::curtain::placed)).collect();
        let m = s.model_mut();
        for g in guids {
            m.pinned.remove(&g);
        }
        for id in held {
            if let Some(Element::CurtainPanel(Panel { free, .. }) | Element::Mullion(Mullion { free, .. })) = m.elements.get_mut(id) {
                *free = true;
            }
        }
    }
    Ok(json!({"unpinned": was, "ids": ids}))
}

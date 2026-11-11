//! Pin and Unpin, as Revit's: a pinned element is held where it is. The window refuses to drag, move or turn it and says
//! so; a command from a script or the agent still works, and what it returns says the element was pinned. A project keeps
//! its pins by the elements' GUIDs, so they survive a renumbering and go when their element does.

use serde_json::{Value, json};

use super::{R, ids_param, only};
use crate::engine::Session;
use crate::model::{Id, Key, Model};

impl Model {
    /// Whether element `id` is pinned.
    pub fn is_pinned(&self, id: impl Key) -> bool {
        !self.pinned.is_empty() && self.elements.guid(id).is_some_and(|g| self.pinned.contains(&g))
    }

    /// Those of `ids` that are pinned, in the order given.
    pub fn pinned_among(&self, ids: &[Id]) -> Vec<Id> {
        ids.iter().copied().filter(|id| self.is_pinned(*id)).collect()
    }

    /// Every pinned element, by id.
    pub fn pinned_ids(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self.pinned.iter().filter_map(|g| self.elements.id(*g)).collect();
        ids.sort_unstable();
        ids
    }

    /// Whether a pin names an element that is gone.
    pub(crate) fn has_stale_pins(&self) -> bool {
        self.pinned.iter().any(|g| !self.elements.contains_key(*g))
    }

    /// The pins of elements that are gone, let go.
    pub(crate) fn forget_stale_pins(&mut self) {
        let gone: Vec<_> = self.pinned.iter().filter(|g| !self.elements.contains_key(**g)).copied().collect();
        for g in gone {
            self.pinned.remove(&g);
        }
    }
}

/// `result` (an object) with `pinned`, the ids among `ids` that were pinned, and a note saying what that means: nothing when
/// none were. A command done to a pinned element is done; the person is told.
pub(super) fn pinned_note(mut result: Value, before: &Model, ids: &[Id]) -> Value {
    let pinned = before.pinned_among(ids);
    if pinned.is_empty() {
        return result;
    }
    if let Some(o) = result.as_object_mut() {
        o.insert(
            "note".into(),
            json!(format!("{} {} pinned; it was changed anyway: `unpin` frees an element", noun(&pinned), verb(&pinned))),
        );
        o.insert("pinned".into(), json!(pinned));
    }
    result
}

fn noun(ids: &[Id]) -> String {
    match ids {
        [one] => format!("element {one}"),
        _ => format!("elements {}", ids.iter().map(Id::to_string).collect::<Vec<_>>().join(", ")),
    }
}

fn verb(ids: &[Id]) -> &'static str {
    if ids.len() == 1 { "was" } else { "were" }
}

pub(super) fn pin(s: &mut Session, p: &Value) -> R {
    let c = "pin";
    only(c, p, &["ids", "id"])?;
    let ids = ids_param(c, &s.model, p)?;
    let guids: Vec<_> = ids.iter().map(|id| s.model.guid(*id)).collect();
    let already = s.model.pinned_among(&ids);
    if already.len() < ids.len() {
        s.model_mut().pinned.extend(guids);
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
        let m = s.model_mut();
        for g in guids {
            m.pinned.remove(&g);
        }
    }
    Ok(json!({"unpinned": was, "ids": ids}))
}

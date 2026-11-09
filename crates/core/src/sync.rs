//! Editing together: what changed in a model, element by element, to send to another copy of the same project, and
//! another copy's changes taken in.
//!
//! Copies of one project (two people's windows, a server keeping it) agree on GUIDs, never on ids: each copy gives the
//! elements it is sent ids of its own. So a change is keyed by GUID. [`Changes`] holds:
//! - `elements`: each changed element by GUID, as its JSON (what the file holds of it, its `guid` aside), or none where
//!   it was removed;
//! - `project`: each changed setting of the project (its types, materials, families, units, …) by its key in the file,
//!   as its JSON, or none where the file leaves it out now.
//!
//! The ids, `next_id` and the library's own ids and GUIDs stay each copy's: the library is made again from the names
//! ([`Model::sync_library`]). A link made from a project's text (`link {text}`, the site Arqiflow links under the building)
//! is each copy's own too, made again from its text where it is shown, so it is never sent nor taken.
//!
//! [`Model::merged`] takes changes in and checks the model; [`Session::merge`] does it for an open session, keeping its
//! undo history its own: each model it goes back to is brought the same changes, so undo takes back this copy's own
//! commands, not what came from the others.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::engine::Session;
use crate::ids::{Changed, Elements, Guid, Id};
use crate::model::{Element, Model};

/// The keys of a project's file that are not settings to send: its elements, the id the next one takes, and the library's
/// own ids and GUIDs, each copy's.
const NOT_SETTINGS: [&str; 3] = ["elements", "next_id", "library"];

/// Most changes one call takes, and the longest JSON of one: what comes from another copy is hostile.
pub const MAX_CHANGES: usize = 200_000;
pub const MAX_ENTRY: usize = 64 << 20;

/// Changes to a project, keyed by GUID (elements) and by the file's keys (settings): see the module.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Changes {
    #[serde(default)]
    pub elements: BTreeMap<Guid, Option<String>>,
    #[serde(default)]
    pub project: BTreeMap<String, Option<String>>,
    /// Everything there is, not what changed: an element or a setting left out is one there is not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub complete: bool,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty() && self.project.is_empty()
    }

    /// Read from JSON, `{elements: {guid: json | null}, project: {key: json | null}, complete?}`, within the caps.
    pub fn read(text: &str) -> Result<Changes, String> {
        let c: Changes = serde_json::from_str(text).map_err(|e| format!("the changes aren't readable: {e}"))?;
        if c.elements.len() + c.project.len() > MAX_CHANGES {
            return Err(format!("too many changes at once ({}; at most {MAX_CHANGES})", c.elements.len() + c.project.len()));
        }
        if let Some(g) = c.elements.keys().find(|g| g.is_nil() || g.pending().is_some()) {
            return Err(format!("{g} is not an element's GUID"));
        }
        if let Some(k) = c.project.keys().find(|k| NOT_SETTINGS.contains(&k.as_str())) {
            return Err(format!("`{k}` is each copy's own, not a setting to change"));
        }
        Ok(c)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

/// Whether an element is this copy's own, never sent nor taken: a link made from a project's text.
pub fn own(e: &Element) -> bool {
    matches!(e, Element::Link(l) if l.path.starts_with(crate::link::FROM_TEXT))
}

/// An element as it is sent: its JSON.
fn element_text(e: &Element) -> Result<String, String> {
    serde_json::to_string(e).map_err(|e| e.to_string())
}

/// The project's settings as the file has them, by key: everything but [`NOT_SETTINGS`].
fn settings(m: &Model) -> Result<Map<String, Value>, String> {
    let mut bare = m.clone();
    bare.elements = Elements::new();
    let Value::Object(mut out) = serde_json::to_value(&bare).map_err(|e| e.to_string())? else {
        return Err("a project is not an object".into());
    };
    for k in NOT_SETTINGS {
        out.remove(k);
    }
    Ok(out)
}

/// Everything in `m`, as changes that make a copy say it (`complete`).
pub fn entries(m: &Model) -> Result<Changes, String> {
    let mut c = Changes { complete: true, ..Changes::default() };
    for (id, e) in m.elements.iter() {
        if own(e) {
            continue;
        }
        if let Some(g) = m.elements.guid(*id) {
            c.elements.insert(g, Some(element_text(e)?));
        }
    }
    for (k, v) in settings(m)? {
        c.project.insert(k, Some(v.to_string()));
    }
    Ok(c)
}

/// What changed from `before` to `after`, by GUID and by setting.
pub fn changes(before: &Model, after: &Model) -> Result<Changes, String> {
    let mut c = Changes::default();
    let guids: BTreeSet<Guid> = match after.elements.changed_since(&before.elements) {
        Changed::Ids(ids) => ids.iter().flat_map(|id| [before.elements.guid(*id), after.elements.guid(*id)]).flatten().collect(),
        Changed::All => {
            let mut all: BTreeSet<Guid> = BTreeSet::new();
            for m in [before, after] {
                all.extend(m.elements.keys().filter_map(|id| m.elements.guid(*id)));
            }
            all
        }
    };
    for g in guids {
        let (was, now) = (before.elements.get(g), after.elements.get(g));
        if was.is_some_and(own) || now.is_some_and(own) {
            continue;
        }
        if was != now {
            c.elements.insert(g, now.map(element_text).transpose()?);
        }
    }
    if !before.same_settings(after) {
        let (a, b) = (settings(before)?, settings(after)?);
        for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
            if a.get(k) != b.get(k) {
                c.project.insert(k.clone(), b.get(k).map(Value::to_string));
            }
        }
    }
    Ok(c)
}

/// What [`Model::merged`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Merged {
    /// The changes it took in: what was asked, but those refused and those kept as they were.
    pub taken: Changes,
    /// What it could not take, and why: an element that would break the model (a door whose wall is gone), unreadable JSON.
    pub refused: Vec<(String, String)>,
    /// The id each element new here was given.
    pub ids: BTreeMap<Guid, Id>,
}

impl Merged {
    /// `{taken, refused: [{what, error}]}`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "taken": self.taken.elements.len() + self.taken.project.len(),
            "refused": self.refused.iter().map(|(what, why)| serde_json::json!({"what": what, "error": why})).collect::<Vec<_>>(),
        })
    }
}

impl Model {
    /// This model with `c` taken in, checked. What would break the model is left out and named (`refused`), the rest taken.
    /// `keep` names the GUIDs whose elements stay as they are here (this copy's own changes, not sent yet). `ids` are the
    /// ids to give new elements where they are free (those another model of the same history gave them).
    pub fn merged(&self, c: &Changes, keep: &BTreeSet<Guid>, ids: &BTreeMap<Guid, Id>) -> Result<(Model, Merged), String> {
        let mut out = Merged::default();
        // The settings first, all or none: the elements are checked against them.
        let base = match self.with_settings(c, true) {
            Ok(m) => {
                out.taken.project = c.project.clone();
                out.taken.complete = c.complete;
                m
            }
            Err(e) => {
                out.refused.push(("the project's settings".into(), e));
                self.clone()
            }
        };
        let wanted = Changes { project: BTreeMap::new(), ..c.clone() };
        let mut all = base.clone();
        let mut got = Merged::default();
        let whole = all.take_elements(&wanted, keep, ids, &mut got).and_then(|()| {
            all.sync_library();
            all.check_changed(self)
        });
        if whole.is_ok() && got.refused.is_empty() {
            out.taken.elements = got.taken.elements;
            out.ids = got.ids;
            return Ok((all, out));
        }
        // Something would break it: each change on its own, those that pass taken.
        let mut m = base;
        let mut each: Vec<(Guid, Option<String>)> = c.elements.iter().map(|(g, v)| (*g, v.clone())).collect();
        if c.complete {
            each.extend(
                m.elements
                    .iter()
                    .filter(|(_, e)| !own(e))
                    .filter_map(|(id, _)| m.elements.guid(*id))
                    .filter(|g| !c.elements.contains_key(g))
                    .map(|g| (g, None)),
            );
        }
        for (g, v) in each {
            let one = Changes { elements: BTreeMap::from([(g, v)]), ..Changes::default() };
            let mut next = m.clone();
            let mut got = Merged::default();
            let tried = next.take_elements(&one, keep, ids, &mut got).and_then(|()| {
                if got.taken.elements.is_empty() {
                    return Ok(());
                }
                next.sync_library();
                next.check_changed(&m)
            });
            match tried {
                Ok(()) => {
                    out.taken.elements.extend(got.taken.elements);
                    out.ids.extend(got.ids);
                    out.refused.extend(got.refused);
                    m = next;
                }
                Err(e) => out.refused.push((g.to_string(), e)),
            }
        }
        Ok((m, out))
    }

    /// This model with the settings `c` changes, each copy's own kept: the library made again from the names, and checked
    /// when `check` says.
    fn with_settings(&self, c: &Changes, check: bool) -> Result<Model, String> {
        if c.project.is_empty() {
            return Ok(self.clone());
        }
        let mut map = settings(self)?;
        if c.complete {
            map.retain(|k, _| c.project.contains_key(k));
        }
        for (k, v) in &c.project {
            match v {
                Some(text) if text.len() >= MAX_ENTRY => return Err(format!("`{k}` is too large ({} bytes)", text.len())),
                Some(text) => {
                    map.insert(k.clone(), serde_json::from_str(text).map_err(|e| format!("`{k}`: {e}"))?);
                }
                None => {
                    map.remove(k);
                }
            }
        }
        map.insert("elements".into(), Value::Object(Map::new()));
        map.insert("next_id".into(), serde_json::json!(self.next_id));
        map.insert("library".into(), serde_json::to_value(&self.library).map_err(|e| e.to_string())?);
        let mut m: Model = serde_json::from_value(Value::Object(map)).map_err(|e| format!("the project's settings: {e}"))?;
        m.elements = self.elements.clone();
        m.loaded = self.loaded.clone();
        m.sync_library();
        if check {
            m.check()?;
        }
        Ok(m)
    }

    /// The elements `c` changes, put in or taken out by GUID, unchecked; and with `complete`, those it doesn't name taken
    /// out. Those whose JSON is not an element's are refused.
    fn take_elements(&mut self, c: &Changes, keep: &BTreeSet<Guid>, ids: &BTreeMap<Guid, Id>, out: &mut Merged) -> Result<(), String> {
        for (g, v) in &c.elements {
            if keep.contains(g) {
                continue;
            }
            match v {
                Some(text) => {
                    let e: Element = match (text.len() < MAX_ENTRY).then(|| serde_json::from_str::<Element>(text)) {
                        Some(Ok(e)) => e,
                        Some(Err(e)) => {
                            out.refused.push((g.to_string(), format!("not an element: {e}")));
                            continue;
                        }
                        None => {
                            out.refused.push((g.to_string(), format!("too large ({} bytes)", text.len())));
                            continue;
                        }
                    };
                    if own(&e) {
                        continue;
                    }
                    match self.elements.id(*g) {
                        Some(id) => {
                            if self.elements.get(id) != Some(&e) {
                                self.elements.insert(id, e);
                            }
                        }
                        None => {
                            let id = match ids.get(g) {
                                Some(id) if !self.elements.contains_key(*id) && !self.library.taken(*id) => *id,
                                _ => self.fresh_id(),
                            };
                            self.elements.insert_as(id, *g, e)?;
                            self.next_id = self.next_id.max(id + 1);
                            out.ids.insert(*g, id);
                        }
                    }
                }
                None => {
                    if self.elements.get(*g).is_some_and(|e| !own(e)) {
                        self.elements.remove(*g);
                    }
                }
            }
            out.taken.elements.insert(*g, v.clone());
        }
        if c.complete {
            let gone: Vec<Guid> = self
                .elements
                .iter()
                .filter(|(_, e)| !own(e))
                .filter_map(|(id, _)| self.elements.guid(*id))
                .filter(|g| !c.elements.contains_key(g) && !keep.contains(g))
                .collect();
            for g in gone {
                self.elements.remove(g);
                out.taken.elements.insert(g, None);
            }
        }
        Ok(())
    }

    /// `c` taken in without a check: for a model of the history, or what a copy last sent, what [`Model::merged`] took of it
    /// for the model now.
    pub fn with_changes(&self, c: &Changes, ids: &BTreeMap<Guid, Id>) -> Result<Model, String> {
        let mut m = self.with_settings(c, false)?;
        let elements = Changes { project: BTreeMap::new(), complete: false, ..c.clone() };
        m.take_elements(&elements, &BTreeSet::new(), ids, &mut Merged::default())?;
        m.sync_library();
        Ok(m)
    }
}

impl Session {
    /// Takes in another copy's changes ([`Model::merged`]): the model checked, what would break it refused and named. The
    /// elements of `keep` stay as they are (this copy's own changes not sent yet). The undo history stays this copy's: each
    /// model it goes back to is brought what was taken, so undo and redo take back and put back this copy's own commands
    /// alone. A model of the history the changes can't be brought to ends the history there.
    pub fn merge(&mut self, c: &Changes, keep: &BTreeSet<Guid>) -> Result<Merged, String> {
        let (model, merged) = self.model.merged(c, keep, &BTreeMap::new())?;
        if merged.taken.is_empty() {
            return Ok(merged);
        }
        // What `complete` took out is named in `taken`, so the history is brought it element by element.
        let ids: BTreeMap<Guid, Id> = merged.taken.elements.keys().filter_map(|g| model.elements.id(*g).map(|id| (*g, id))).collect();
        for stack in [&mut self.undo, &mut self.redo] {
            // Newest last: a model that can't be brought them ends the history there, the older ones with it.
            let mut cut = None;
            for (i, (step, m)) in stack.iter_mut().enumerate().rev() {
                match m.with_changes(&merged.taken, &ids) {
                    Ok(n) => {
                        *m = Arc::new(n);
                        step.checked = false;
                    }
                    Err(_) => {
                        cut = Some(i);
                        break;
                    }
                }
            }
            if let Some(i) = cut {
                stack.drain(..=i);
            }
        }
        self.model = Arc::new(model);
        self.verified = Some(Arc::clone(&self.model));
        Ok(merged)
    }
}

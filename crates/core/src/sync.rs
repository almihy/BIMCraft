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
//! The ids and `next_id` stay each copy's. The library's GUIDs are the project's, as an element's are: each entry of it is
//! told by its GUID, so a rename is a change to its name alone, and an element, which points at its type and the rest by GUID as
//! a file does, says the same whatever they are called (`docs/areas/sync.md`). A link made from a project's text (`link {text}`,
//! the site Arqiflow links under the building) is each copy's own, made again from its text where it is shown, so it is never
//! sent nor taken.
//!
//! [`Model::merged`] takes changes in and checks the model; [`Session::merge`] does it for an open session, keeping its
//! undo history its own: each model it goes back to is brought the same changes, so undo takes back this copy's own
//! commands, not what came from the others.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::engine::{Session, Step};
use crate::ids::{Guid, Id};
use crate::model::Model;
use crate::sync_settings;
pub use bimcraft_model::merge::*;

impl Session {
    /// Takes in another copy's changes ([`Model::merged`]): the model checked, what would break it refused and named. The
    /// elements of `keep` stay as they are (this copy's own changes not sent yet). The undo history stays this copy's: each
    /// model it goes back to is brought what was taken, so undo and redo take back and put back this copy's own commands
    /// alone. A model of the history the changes can't be brought to ends the history there. The warnings kept stay with the model
    /// they were worked out for: the next command or request brings them up to date from it, by what the changes touched
    /// ([`Session::warnings`]), never from nothing.
    ///
    /// An element (or setting) that names a library entry this copy has not heard of yet (a wall of a type another copy has not told
    /// this one of) waits, and is not refused: when the library changes, the waiting are offered again, as a batch of their own after
    /// the changes that came, so that the order the changes came in does not matter and a waiting one that still cannot be taken
    /// holds nothing else back. One that waits through [`WAITING_TRIES`] such changes, or that would be more than the queue holds
    /// ([`WAITING_AT_MOST`] of each, [`WAITING_BYTES`] together), is given up on and named in `refused`.
    pub fn merge(&mut self, c: &Changes, keep: &BTreeSet<Guid>) -> Result<Merged, String> {
        let (mut model, mut merged) = self.model.merged(c, keep, &BTreeMap::new())?;
        if c.complete {
            self.waiting.clear();
            self.waiting_project.clear();
        }
        // What the room says now of an element or a key is not what waited for it.
        for g in c.elements.keys() {
            self.waiting.remove(g);
        }
        for k in c.project.keys() {
            self.waiting_project.remove(k);
        }
        let mut gave_up: Vec<(String, String)> = Vec::new();
        let mut bytes = self.waiting_bytes();
        for g in &merged.waiting {
            if let Some(Some(text)) = c.elements.get(g) {
                hold(&mut self.waiting, *g, g.to_string(), text, 0, &mut bytes, &mut gave_up);
            }
        }
        for (k, text) in &merged.waiting_project {
            hold(&mut self.waiting_project, k.clone(), k.clone(), text, 0, &mut bytes, &mut gave_up);
        }
        // The library changed: what waits is offered again, on the model the changes made, apart from them. (Not what has just been
        // refused: the model it was refused by is this one.)
        let queued = !self.waiting.is_empty() || !self.waiting_project.is_empty();
        if queued && (c.complete || library_changed(&self.model, &model)) {
            let mut again = Changes::default();
            for (g, w) in self.waiting.iter().filter(|(g, _)| !merged.waiting.contains(g)) {
                again.elements.insert(*g, Some(w.text.clone()));
            }
            for (k, w) in self.waiting_project.iter().filter(|(k, _)| !merged.waiting_project.contains_key(*k)) {
                again.project.insert(k.clone(), Some(w.text.clone()));
            }
            if !again.is_empty()
                && let Ok((later_model, later)) = model.merged(&again, keep, &BTreeMap::new())
            {
                for g in again.elements.keys() {
                    let (taken, waits) = (later.taken.elements.contains_key(g), later.waiting.contains(g));
                    self.tried(g, taken, waits, &mut gave_up);
                }
                for k in again.project.keys() {
                    let (taken, waits) = (later.taken.project.contains_key(k), later.waiting_project.contains_key(k));
                    self.tried_project(k, taken, waits, &mut gave_up);
                }
                model = later_model;
                merged.absorb(later);
            }
        }
        merged.refused.extend(gave_up);
        if merged.taken.is_empty() {
            return Ok(merged);
        }
        // What `complete` took out is named in `taken`, so the history is brought it element by element.
        let ids: BTreeMap<Guid, Id> = merged.taken.elements.keys().filter_map(|g| model.elements.id(*g).map(|id| (*g, id))).collect();
        let now = Arc::clone(&self.model);
        let had = |g: &Guid| now.elements.contains_key(*g);
        for stack in [&mut self.undo, &mut self.redo] {
            // Newest last: a model that can't be brought them ends the history there, the older ones with it.
            let mut cut = None;
            for (i, (step, m)) in stack.iter_mut().enumerate().rev() {
                match m.with_changes(&m.brought(&merged.taken, had), &ids) {
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

/// Keeps `text` waiting under `key` (which `what` names, if it is given up on), `tried` times offered again so far, unless the queue
/// would hold too much: then it is named in `gave_up`. `bytes` is what the queues hold together, kept up to date.
fn hold<K: Ord>(
    map: &mut BTreeMap<K, Wait>,
    key: K,
    what: String,
    text: &str,
    tried: u32,
    bytes: &mut usize,
    gave_up: &mut Vec<(String, String)>,
) {
    let old = map.remove(&key).map_or(0, |w| w.text.len());
    *bytes = bytes.saturating_sub(old);
    if map.len() >= WAITING_AT_MOST || *bytes + text.len() > WAITING_BYTES {
        gave_up.push((what, "too many changes are waiting for what they name, so this one was dropped".into()));
        return;
    }
    *bytes += text.len();
    map.insert(key, Wait { text: text.to_string(), tried });
}

impl Session {
    /// How many bytes of JSON wait.
    fn waiting_bytes(&self) -> usize {
        self.waiting.values().map(|w| w.text.len()).chain(self.waiting_project.values().map(|w| w.text.len())).sum()
    }

    /// An element that waited has been offered again: it is no longer waiting if it was taken (or refused for another reason), and
    /// has been tried once more if it still waits for what it names, until it is given up on.
    fn tried(&mut self, g: &Guid, taken: bool, waits: bool, gave_up: &mut Vec<(String, String)>) {
        let Some(w) = self.waiting.get_mut(g) else { return };
        w.tried += 1;
        if taken || !waits {
            self.waiting.remove(g);
        } else if w.tried >= WAITING_TRIES {
            self.waiting.remove(g);
            gave_up.push((g.to_string(), given_up()));
        }
    }

    /// [`Session::tried`] for a key of the settings.
    fn tried_project(&mut self, k: &str, taken: bool, waits: bool, gave_up: &mut Vec<(String, String)>) {
        let Some(w) = self.waiting_project.get_mut(k) else { return };
        w.tried += 1;
        if taken || !waits {
            self.waiting_project.remove(k);
        } else if w.tried >= WAITING_TRIES {
            self.waiting_project.remove(k);
            gave_up.push((k.to_string(), given_up()));
        }
    }
}

fn given_up() -> String {
    format!("it names something that never came ({WAITING_TRIES} changes to the library later), so it was dropped")
}

/// A copy's undo history, kept outside it so that it outlives the copy (a page reloaded): [`Session::kept_history`] gives it,
/// [`Session::restore_history`] puts it back on the same project opened again. It holds what this copy's own commands did and
/// nothing else, by GUID: each step the changes that make the model nearer the present into the model of that step. The model
/// it is put back on holds the others' work done since, and every model of the history is brought that, as
/// [`Session::merge`] brings what comes in live: undo stays this copy's, across a reload too.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    /// The project's `uid`: a history goes back only on its own project.
    pub uid: u64,
    /// What the elements and settings the steps touch said when it was kept, from which what the others changed of them
    /// since is told.
    pub now: Changes,
    /// The undo steps, oldest first.
    pub undo: Vec<KeptStep>,
    /// The redo steps, the next to redo last.
    pub redo: Vec<KeptStep>,
}

/// One step of a [`History`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptStep {
    pub label: String,
    /// What makes the model nearer the present (the next step's, or the model's) into this step's model.
    pub changes: Changes,
    /// The ids its model gave the elements `changes` puts in, given them again where they are free.
    #[serde(default)]
    pub ids: BTreeMap<Guid, Id>,
}

impl History {
    pub fn read(text: &str) -> Result<History, String> {
        serde_json::from_str(text).map_err(|e| format!("the undo history isn't readable: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }
}

impl Session {
    /// The undo history to keep (see [`History`]). Steps that changed nothing another copy would see (the link of a site
    /// made from text, each copy's own) are left out.
    pub fn kept_history(&self) -> Result<History, String> {
        let mut h = History { uid: self.model.uid, ..History::default() };
        let (mut elements, mut touched) = (BTreeSet::new(), BTreeSet::new());
        for (stack, out) in [(&self.undo, &mut h.undo), (&self.redo, &mut h.redo)] {
            let mut nearer: &Model = &self.model;
            for (step, m) in stack.iter().rev() {
                let c = changes(nearer, m)?;
                nearer = m;
                if c.is_empty() {
                    continue;
                }
                elements.extend(c.elements.keys().copied());
                touched.extend(c.project.keys().cloned());
                let ids =
                    c.elements.iter().filter(|(_, v)| v.is_some()).filter_map(|(g, _)| m.elements.id(*g).map(|id| (*g, id))).collect();
                out.push(KeptStep { label: step.label.clone(), changes: c, ids });
            }
            out.reverse();
        }
        for g in elements {
            let e = self.model.elements.get(g).filter(|e| !own(e));
            h.now.elements.insert(g, e.map(|e| element_text(&self.model, e)).transpose()?);
        }
        if !touched.is_empty() {
            let now = sync_settings::flat(&self.model)?;
            for k in touched {
                let v = now.get(&k).map(Value::to_string);
                h.now.project.insert(k, v);
            }
        }
        Ok(h)
    }

    /// Puts back a history [`Session::kept_history`] gave, on the same project opened again; the model stays as it is. What
    /// the others changed since of what the history touches is brought to every model of it. A step that can't be brought
    /// back ends the history there, the older ones with it. Steps this session took since it opened stay on top of those put
    /// back (and the redo steps are then its own); steps of its own that changed nothing another copy would see (the site's
    /// link) go. Gives how many undo and redo steps there are.
    pub fn restore_history(&mut self, h: &History) -> Result<(usize, usize), String> {
        if h.uid != self.model.uid {
            return Err("the undo history is another project's".into());
        }
        // Where the history ends: the model as it opened, before the steps of this session that changed something.
        let mut own_steps = false;
        let mut nearer: &Model = &self.model;
        for (_, m) in self.undo.iter().rev() {
            own_steps |= !changes(m, nearer)?.is_empty();
            nearer = m;
        }
        let anchor = match self.undo.first() {
            Some((_, m)) if own_steps => Arc::clone(m),
            _ => Arc::clone(&self.model),
        };
        // That model as the history last knew it, where the history touches it, and what the others changed of that since.
        let base = Arc::new(anchor.with_changes(&h.now, &BTreeMap::new())?);
        let since = changes(&base, &anchor)?;
        let since_ids: BTreeMap<Guid, Id> = since.elements.keys().filter_map(|g| anchor.elements.id(*g).map(|id| (*g, id))).collect();
        let mut built: [Vec<(Step, Arc<Model>)>; 2] = Default::default();
        for (which, kept) in [&h.undo, &h.redo].into_iter().enumerate() {
            let redo = which == 1;
            let (mut raw_nearer, mut nearer) = (Arc::clone(&base), Arc::clone(&anchor));
            let mut out = Vec::new();
            for ks in kept.iter().rev() {
                let Ok(raw) = raw_nearer.with_changes(&ks.changes, &ks.ids).map(Arc::new) else { break };
                let model = if since.is_empty() {
                    Arc::clone(&raw)
                } else {
                    match raw.with_changes(&raw.brought(&since, |g| base.elements.contains_key(*g)), &since_ids) {
                        Ok(m) => Arc::new(m),
                        Err(_) => break,
                    }
                };
                let step = if redo { Step::new(&ks.label, &nearer, &model) } else { Step::new(&ks.label, &model, &nearer) };
                out.push((step, Arc::clone(&model)));
                (raw_nearer, nearer) = (raw, model);
            }
            out.reverse();
            built[which] = out;
        }
        let [mut undo, redo] = built;
        if own_steps {
            undo.append(&mut self.undo);
        } else {
            self.undo.clear();
            self.redo = redo;
        }
        let over = undo.len().saturating_sub(crate::engine::UNDO_LIMIT);
        undo.drain(..over);
        self.undo = undo;
        Ok((self.undo.len(), self.redo.len()))
    }
}

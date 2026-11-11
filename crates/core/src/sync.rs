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
use serde_json::{Map, Value};

use crate::engine::{Session, Step};
use crate::ids::{Changed, Guid, Id};
use crate::library::Kind;
use crate::model::{Element, Model};
use crate::sync_settings;

/// The keys of a project's file that are not settings to send: its elements, the id the next one takes, and the library's
/// ids (its entries' GUIDs travel with the entries), each copy's.
pub(crate) const NOT_SETTINGS: [&str; 3] = ["elements", "next_id", "library"];

/// How copies of a project tell each other what it holds, a number a room and a page compare: a page whose engine says
/// another is not the room's, and loads again. 1: each setting a whole key (a whole map, `types`); 3: each entry of a map on
/// its own, by its GUID, with its name apart (`types/wall/<guid>`, `types/wall/<guid>/name`), phases with their places, and the
/// elements and entries pointing at entries by GUID ([`crate::sync_settings`]). Raised when a copy of the older would misread
/// what the newer tells.
pub const FORMAT: u32 = 3;

/// Most changes one call takes, and the longest JSON of one: what comes from another copy is hostile.
pub const MAX_CHANGES: usize = 200_000;
pub const MAX_ENTRY: usize = 64 << 20;
/// Most elements (and, apart, settings) a session keeps waiting for the library entries they name.
const WAITING_AT_MOST: usize = 10_000;
/// Most bytes of JSON a session keeps waiting, elements and settings together.
const WAITING_BYTES: usize = 8 << 20;
/// How many times a waiting element or setting is offered again, when the library changes, before it is given up on.
const WAITING_TRIES: u32 = 20;

/// An element or setting another copy told that this one cannot take yet: a wall of a type, or a type made of a material, this copy
/// has not heard of. It waits in the session ([`Session::merge`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Wait {
    pub text: String,
    /// How many times it has been offered again and still waits.
    pub tried: u32,
}

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
        if let Some(k) = c.project.keys().find(|k| sync_settings::is_own(k)) {
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

/// An element as it is sent: its JSON as a file holds it, what it points at in the library by GUID
/// ([`Model::element_to_file`]), so that renaming an entry changes nothing an element says.
fn element_text(m: &Model, e: &Element) -> Result<String, String> {
    serde_json::to_string(&m.element_to_file(e)).map_err(|e| e.to_string())
}

/// Everything in `m`, as changes that make a copy say it (`complete`).
pub fn entries(m: &Model) -> Result<Changes, String> {
    let mut c = Changes { complete: true, ..Changes::default() };
    for (id, e) in m.elements.iter() {
        if own(e) {
            continue;
        }
        if let Some(g) = m.elements.guid(*id) {
            c.elements.insert(g, Some(element_text(m, e)?));
        }
    }
    for (k, v) in sync_settings::flat(m)? {
        c.project.insert(k, Some(v.to_string()));
    }
    Ok(c)
}

/// Whether the library has other entries in `after` than in `before`, by GUID, kind and name (its ids are each copy's).
fn library_changed(before: &Model, after: &Model) -> bool {
    before.library.len() != after.library.len()
        || before.library.entries().any(|(_, e)| after.library.by_guid(e.guid).is_none_or(|(_, x)| x.kind != e.kind || x.name != e.name))
}

/// Whether an entry of the library is another in `after` than `before` under one name (taken out and made again): what points at
/// it by name says the same in a model, and not in a file.
fn library_remapped(before: &Model, after: &Model) -> bool {
    library_changed(before, after)
        && before.library.entries().any(|(_, e)| after.library.guid(&e.kind, &e.name).is_some_and(|g| g != e.guid))
}

/// What changed from `before` to `after`, by GUID and by setting. An element that says the same (a rename of what it points at
/// changes its names here, not its text) is not a change.
pub fn changes(before: &Model, after: &Model) -> Result<Changes, String> {
    let mut c = Changes::default();
    let remapped = library_remapped(before, after);
    let guids: BTreeSet<Guid> = match after.elements.changed_since(&before.elements) {
        Changed::Ids(ids) if !remapped => {
            ids.iter().flat_map(|id| [before.elements.guid(*id), after.elements.guid(*id)]).flatten().collect()
        }
        _ => {
            let mut all: BTreeSet<Guid> = BTreeSet::new();
            for m in [before, after] {
                all.extend(m.elements.keys().filter_map(|id| m.elements.guid(*id)));
            }
            all
        }
    };
    for g in guids {
        let (was, now) = (before.elements.get(g), after.elements.get(g));
        if was.is_some_and(own) || now.is_some_and(own) || (was == now && !remapped) {
            continue;
        }
        let (a, b) = (was.map(|e| element_text(before, e)).transpose()?, now.map(|e| element_text(after, e)).transpose()?);
        if a != b {
            c.elements.insert(g, b);
        }
    }
    if !before.same_settings(after) || library_changed(before, after) {
        let (a, b) = (sync_settings::flat(before)?, sync_settings::flat(after)?);
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
    /// What this copy tells the room that it did not hear, key by key (none: the key is to be taken out): the names of entries it
    /// had to rename, two having asked for one name; the body and name of an entry it kept, though the room took it out, because it
    /// is in use here; a body left with no name, to take out ([`crate::sync_settings`], `docs/areas/sync.md`).
    pub fixes: BTreeMap<String, Option<String>>,
    /// An entry the changes took out that this copy kept because something here names it, with what names it: what the one who took
    /// it out is told ("Wall type Beta was kept: walls use it"). Not in `taken`; its keys are in `fixes`.
    pub kept: Vec<Kept>,
    /// The elements the model would not take because they name a library entry this copy has not heard of yet (a wall of a type
    /// that comes later): not refused, they wait, and a session offers them again when the library changes ([`Session::merge`]).
    pub waiting: BTreeSet<Guid>,
    /// The same for the settings: the keys of an entry the model would not take because it names an entry not heard of yet (a type
    /// made of a material that comes later), with their JSON.
    pub waiting_project: BTreeMap<String, String>,
}

/// A library entry that stayed because something names it, though the changes took it out (`Merged::kept`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Kept {
    /// The entry's GUID (the owner's, for a family's type): what its keys in the room are named by.
    pub entry: Guid,
    /// What it is, as people say it: `wall type`, `material`.
    pub kind: String,
    pub name: String,
    /// What names it, as plain plurals: `walls`, `doors`, `other entries` (a type made of it, a view using it).
    pub uses: Vec<String>,
}

impl Merged {
    /// `{taken, refused: [{what, error}], fixes, kept: [{entry, kind, name, uses}]}`.
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "taken": self.taken.elements.len() + self.taken.project.len(),
            "refused": self.refused.iter().map(|(what, why)| serde_json::json!({"what": what, "error": why})).collect::<Vec<_>>(),
            "fixes": self.fixes,
            "kept": self.kept,
        })
    }

    /// What `later` did after this (a second merge on the model this one made) is added to it.
    fn absorb(&mut self, later: Merged) {
        self.taken.elements.extend(later.taken.elements);
        self.taken.project.extend(later.taken.project);
        self.refused.extend(later.refused);
        self.ids.extend(later.ids);
        self.fixes.extend(later.fixes);
        self.kept.extend(later.kept);
        self.waiting.extend(later.waiting);
        self.waiting_project.extend(later.waiting_project);
    }
}

/// The settings of changes taken in entry by entry ([`Model::merged`]).
struct Settled {
    model: Model,
    /// The keys taken.
    project: BTreeMap<String, Option<String>>,
    fixes: BTreeMap<String, Option<String>>,
    /// The keys of an entry that could not be taken, each with why.
    refused: Vec<(String, String)>,
    /// The keys of an entry that only the model's check turns away, which more settings may pass (a type made of a material not
    /// heard of yet): they wait, and are not in `refused`; an entry that is not an entry's at all never waits.
    waiting: BTreeSet<String>,
}

impl Settled {
    fn new(model: Model) -> Settled {
        Settled { model, project: BTreeMap::new(), fixes: BTreeMap::new(), refused: Vec::new(), waiting: BTreeSet::new() }
    }

    /// Takes `groups` of keys (those of `all`, by key) into the model, together where they pass the check, else halved, until
    /// each one that does not is alone and refused.
    fn take(&mut self, groups: &[&[String]], all: &BTreeMap<String, Option<String>>) {
        let project: BTreeMap<String, Option<String>> =
            groups.iter().flat_map(|g| g.iter()).filter_map(|k| all.get(k).map(|v| (k.clone(), v.clone()))).collect();
        if project.is_empty() {
            return;
        }
        match self.model.with_settings(&Changes { project: project.clone(), ..Changes::default() }, true) {
            Ok((m, fixes)) => {
                self.model = m;
                self.project.extend(project);
                self.fixes.extend(fixes);
            }
            Err(e) if groups.len() == 1 => {
                // Only the model's check refuses it (a type made of a material not heard of yet): it waits, it is not refused.
                if self.model.with_settings(&Changes { project, ..Changes::default() }, false).is_ok() {
                    self.waiting.extend(groups[0].iter().cloned());
                } else {
                    self.refused.extend(groups[0].iter().map(|k| (k.clone(), e.clone())));
                }
            }
            Err(_) => {
                let (left, right) = groups.split_at(groups.len() / 2);
                self.take(left, all);
                self.take(right, all);
            }
        }
    }
}

impl Model {
    /// This model with `c` taken in, checked. What would break the model is left out and named (`refused`), the rest taken.
    /// `keep` names the GUIDs whose elements stay as they are here (this copy's own changes, not sent yet). `ids` are the
    /// ids to give new elements where they are free (those another model of the same history gave them).
    ///
    /// An entry the changes take out that something here still names (a type of a wall this copy placed meanwhile) is kept: it is
    /// not in `taken`, it is named in `kept`, and its body and name are in `fixes`, which the copy tells the room, so that every
    /// copy ends with it and nothing is lost. (In Revit, deleting a type that is in use deletes the elements of it too, after a
    /// warning. Here the command that deletes a type refuses while something names it, the choice made for this editor; this is for
    /// the race, where work that uses the type arrives after the taking out was made.) A change of one entry that does not apply is
    /// refused alone. An element that names a library entry this copy has not heard of is not refused: it is in `waiting`.
    pub fn merged(&self, c: &Changes, keep: &BTreeSet<Guid>, ids: &BTreeMap<Guid, Id>) -> Result<(Model, Merged), String> {
        let mut out = Merged::default();
        let (kept, told) = self.keeping_in_use(c, keep, ids);
        let c: &Changes = &told;
        if !kept.is_empty() {
            out.fixes = sync_settings::restoring(self, &kept.iter().map(|k| k.entry).collect())?;
            out.kept = kept;
        }
        // Settings and elements together first: another project opened in a copy (an .rvt, an .ifc) changes both, its
        // types and the walls of those types, and neither passes the check without the other.
        if !c.project.is_empty()
            && let Ok((mut all, fixes)) = self.with_settings(c, false)
        {
            let mut got = Merged::default();
            let whole = all.take_elements(c, keep, ids, &mut got).and_then(|()| {
                all.sync_library_stable();
                all.check()
            });
            if whole.is_ok() && got.refused.is_empty() {
                out.taken = Changes { elements: got.taken.elements, project: c.project.clone(), complete: c.complete };
                out.ids = got.ids;
                out.fixes.extend(fixes);
                return Ok((all, out));
            }
        }
        // The settings first: all, or each entry on its own where one would break the model; the elements are checked against them.
        let base = match self.with_settings(c, true) {
            Ok((m, fixes)) => {
                out.taken.project = c.project.clone();
                out.taken.complete = c.complete;
                out.fixes.extend(fixes);
                m
            }
            Err(_) if !c.complete => {
                let mut by = Settled::new(self.clone());
                let groups = sync_settings::groups(&c.project);
                by.take(&groups.iter().map(Vec::as_slice).collect::<Vec<_>>(), &c.project);
                out.taken.project = by.project;
                out.fixes.extend(by.fixes);
                for k in &by.waiting {
                    if let Some(Some(text)) = c.project.get(k) {
                        out.waiting_project.insert(k.clone(), text.clone());
                    }
                }
                out.refused.extend(by.refused);
                by.model
            }
            Err(e) => {
                out.refused.push(("the project's settings".into(), e));
                self.clone()
            }
        };
        let mut all = base.clone();
        let mut got = Merged::default();
        let whole = all.take_elements(c, keep, ids, &mut got).and_then(|()| {
            all.sync_library_stable();
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
            let one = Changes { elements: BTreeMap::from([(g, v.clone())]), ..Changes::default() };
            let mut next = m.clone();
            let mut got = Merged::default();
            let tried = next.take_elements(&one, keep, ids, &mut got).and_then(|()| {
                if got.taken.elements.is_empty() {
                    return Ok(());
                }
                next.sync_library_stable();
                next.check_changed(&m)
            });
            match tried {
                Ok(()) => {
                    out.taken.elements.extend(got.taken.elements);
                    out.ids.extend(got.ids);
                    out.refused.extend(got.refused);
                    m = next;
                }
                // An element that names a library entry not heard of yet waits (the entry may come); any other error is a refusal.
                Err(_) if v.is_some() && m.names_missing(&next, g) => {
                    out.waiting.insert(g);
                }
                Err(e) => out.refused.push((g.to_string(), e)),
            }
        }
        Ok((m, out))
    }

    /// `c` without what it takes out of the library that this copy still uses, and what was kept: a type some element here
    /// (or one the changes bring) is of, a material a type here is made of. What a kept entry names is kept with it, so it goes on
    /// until nothing more is.
    fn keeping_in_use<'a>(
        &self,
        c: &'a Changes,
        keep: &BTreeSet<Guid>,
        ids: &BTreeMap<Guid, Id>,
    ) -> (Vec<Kept>, std::borrow::Cow<'a, Changes>) {
        let mut kept: Vec<Kept> = Vec::new();
        let mut now = std::borrow::Cow::Borrowed(c);
        for _ in 0..8 {
            if now.complete || !now.project.values().any(Option::is_none) {
                break;
            }
            let Ok((mut all, _)) = self.with_settings(&now, false) else { break };
            if all.take_elements(&now, keep, ids, &mut Merged::default()).is_err() {
                break;
            }
            let used = self.removed_in_use(&all);
            if used.is_empty() {
                break;
            }
            let guids: BTreeSet<Guid> = used.iter().map(|k| k.entry).collect();
            kept.extend(used);
            now = std::borrow::Cow::Owned(Changes {
                elements: now.elements.clone(),
                project: sync_settings::without(&now.project, &guids),
                complete: now.complete,
            });
        }
        (kept, now)
    }

    /// The entries of this model's library that `all` (this model with changes taken in) has not, but names: what stands for each in
    /// the settings (its owner, for a family's type), with what names it. What this copy keeps to itself and never sends (a link made
    /// from text) does not count: it would keep an entry for nobody.
    fn removed_in_use(&self, all: &Model) -> Vec<Kept> {
        let gone: Vec<(Kind, String, Guid, Guid)> = self
            .library
            .entries()
            .filter(|(_, e)| all.library.by_guid(e.guid).is_none())
            .filter_map(|(_, e)| {
                Some((e.kind.clone(), e.name.clone(), e.guid, sync_settings::standing_for(self, &e.kind, &e.name, e.guid)?))
            })
            .collect();
        if gone.is_empty() {
            return Vec::new();
        }
        // What names an entry that is not there: by its old name (what this copy holds), or by its GUID (what the changes bring).
        let users = Users::of(all);
        let mut out: Vec<Kept> = Vec::new();
        for (kind, name, guid, stands) in gone {
            let by_guid = guid.to_string();
            let mut names = vec![by_guid.as_str()];
            if all.library.id(&kind, &name).is_none() {
                names.push(&name);
            }
            let mut uses = BTreeSet::new();
            let mut named = false;
            for n in names {
                named |= users.add_to(&kind, n, &mut uses);
            }
            if !named {
                continue;
            }
            let Some((_, owner)) = self.library.by_guid(stands) else { continue };
            match out.iter_mut().find(|k| k.entry == stands) {
                Some(k) => k.uses = k.uses.iter().cloned().chain(uses).collect::<BTreeSet<_>>().into_iter().collect(),
                None => {
                    out.push(Kept { entry: stands, kind: owner.kind.label(), name: owner.name.clone(), uses: uses.into_iter().collect() })
                }
            }
        }
        out
    }

    /// Whether the element `g` of `next` (this model with the element put in) names a library entry this model does not have: what
    /// a wall of a type that comes later does, which is no reason to refuse it, the type may be on its way.
    fn names_missing(&self, next: &Model, g: Guid) -> bool {
        let Some(mut e) = next.elements.get(g).cloned() else { return false };
        let mut missing = false;
        e.lib_refs_mut(&mut |kind, name| missing |= !name.is_empty() && self.library.id(kind, name).is_none());
        missing
    }

    /// This model with the settings `c` changes, each copy's own kept: the entries it tells put in, renamed or taken out by GUID,
    /// and what in this copy points at an entry by name (its elements) following the renames; checked when `check` says. With what
    /// this copy tells the room that it did not hear ([`Merged::fixes`]).
    fn with_settings(&self, c: &Changes, check: bool) -> Result<(Model, BTreeMap<String, Option<String>>), String> {
        if c.project.is_empty() {
            return Ok((self.clone(), BTreeMap::new()));
        }
        let mut map = sync_settings::settings(self)?;
        let done = sync_settings::apply(self, &mut map, &c.project, c.complete)?;
        map.insert("elements".into(), Value::Object(Map::new()));
        map.insert("next_id".into(), serde_json::json!(self.next_id));
        map.insert("library".into(), serde_json::to_value(&done.library).map_err(|e| e.to_string())?);
        let mut m: Model = serde_json::from_value(Value::Object(map)).map_err(|e| format!("the project's settings: {e}"))?;
        // The settings point at entries by GUID, as a file does: named again by the library they now have.
        m.refs_from_file();
        m.elements = self.elements.clone();
        m.loaded = self.loaded.clone();
        if !done.renames.is_empty() {
            m.follow(self);
        }
        m.sync_library_stable();
        if check {
            m.check()?;
        }
        Ok((m, done.fixes))
    }

    /// What in this model is named by what `was` (the model it was made of) called an entry that is called otherwise now is named
    /// by the new name: each element, through the file form it has in `was` (by GUID) and again in this model.
    fn follow(&mut self, was: &Model) {
        let ids: Vec<Id> = self.elements.keys().copied().collect();
        for id in ids {
            let Some(e) = self.elements.get(id) else { continue };
            let mut f = was.element_to_file(e);
            self.element_from_file(&mut f);
            if f != *e {
                self.elements.insert(id, f);
            }
        }
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
                    let mut e: Element = match (text.len() < MAX_ENTRY).then(|| serde_json::from_str::<Element>(text)) {
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
                    // What it points at by GUID is named by this model's library.
                    self.element_from_file(&mut e);
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

    /// `c`, which the others made to a model that had the elements `had`, as it is brought to this model of the undo history:
    /// what they changed of an element this model has not (one a later step of this copy's made) is left out, or going back
    /// to before that step would bring the element back. What they made and took away is brought as it is.
    pub fn brought(&self, c: &Changes, had: impl Fn(&Guid) -> bool) -> Changes {
        let mut out = c.clone();
        out.elements.retain(|g, v| v.is_none() || !had(g) || self.elements.contains_key(*g));
        out
    }

    /// `c` taken in without a check: for a model of the history, or what a copy last sent, what [`Model::merged`] took of it
    /// for the model now.
    pub fn with_changes(&self, c: &Changes, ids: &BTreeMap<Guid, Id>) -> Result<Model, String> {
        let (mut m, _) = self.with_settings(c, false)?;
        let elements = Changes { project: BTreeMap::new(), complete: false, ..c.clone() };
        m.take_elements(&elements, &BTreeSet::new(), ids, &mut Merged::default())?;
        m.sync_library_stable();
        Ok(m)
    }
}

/// What names each library entry in a model, as it would be sent: by the elements (their categories) and by the settings. An element
/// that is each copy's own (a link made from text) is never sent, so its names count for nothing.
struct Users {
    elements: BTreeMap<(Kind, String), BTreeSet<&'static str>>,
    settings: BTreeSet<(Kind, String)>,
}

impl Users {
    fn of(m: &Model) -> Users {
        let mut out = Users { elements: BTreeMap::new(), settings: BTreeSet::new() };
        for e in m.elements.values().filter(|e| !own(e)) {
            let category = e.category();
            e.clone().lib_refs_mut(&mut |kind, name| {
                out.elements.entry((kind.clone(), name.clone())).or_default().insert(category);
            });
        }
        sync_settings::without_elements(m).lib_refs_mut(&mut |kind, name| {
            out.settings.insert((kind.clone(), name.clone()));
        });
        out
    }

    /// Adds what names entry `name` of `kind` to `uses`, in plain plurals; whether anything does.
    fn add_to(&self, kind: &Kind, name: &str, uses: &mut BTreeSet<String>) -> bool {
        let key = (kind.clone(), name.to_string());
        let mut named = false;
        if let Some(categories) = self.elements.get(&key) {
            named = true;
            uses.extend(categories.iter().map(|c| plural(c)));
        }
        if self.settings.contains(&key) {
            named = true;
            uses.insert("other entries".into());
        }
        named
    }
}

/// An element's category as people say it of several: `walls`, `structural columns`.
fn plural(category: &str) -> String {
    match category {
        "structuralcolumn" => "structural columns".into(),
        "detailgroup" => "detail groups".into(),
        "line" => "detail lines".into(),
        "region" => "filled regions".into(),
        c if c.ends_with('s') => format!("{c}es"),
        c => format!("{c}s"),
    }
}

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

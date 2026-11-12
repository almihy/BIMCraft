//! The model's half of editing together (`sync` in `bimcraft-core` is the Session's): what changed between two models, as
//! entries by GUID, and a model with another copy's changes merged in.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ids::{Changed, Guid, Id};
use crate::library::Kind;
use crate::model::{Element, Model};
use crate::sync_settings;

/// The keys of a project's file that are not settings to send: its elements, the id the next one takes, and the library's
/// ids (its entries' GUIDs travel with the entries), each copy's.
pub const NOT_SETTINGS: [&str; 3] = ["elements", "next_id", "library"];

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
pub const WAITING_AT_MOST: usize = 10_000;
/// Most bytes of JSON a session keeps waiting, elements and settings together.
pub const WAITING_BYTES: usize = 8 << 20;
/// How many times a waiting element or setting is offered again, when the library changes, before it is given up on.
pub const WAITING_TRIES: u32 = 20;

/// An element or setting another copy told that this one cannot take yet: a wall of a type, or a type made of a material, this copy
/// has not heard of. It waits in the session ([`Session::merge`]).
#[derive(Clone, Debug, PartialEq)]
pub struct Wait {
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
pub fn element_text(m: &Model, e: &Element) -> Result<String, String> {
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
pub fn library_changed(before: &Model, after: &Model) -> bool {
    before.library.len() != after.library.len()
        || before.library.entries().any(|(_, e)| after.library.by_guid(e.guid).is_none_or(|(_, x)| x.kind != e.kind || x.name != e.name))
}

/// Whether an entry of the library is another in `after` than `before` under one name (taken out and made again): what points at
/// it by name says the same in a model, and not in a file.
pub fn library_remapped(before: &Model, after: &Model) -> bool {
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
    pub fn absorb(&mut self, later: Merged) {
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
    pub fn new(model: Model) -> Settled {
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
pub struct Users {
    elements: BTreeMap<(Kind, String), BTreeSet<&'static str>>,
    settings: BTreeSet<(Kind, String)>,
}

impl Users {
    pub fn of(m: &Model) -> Users {
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
    pub fn add_to(&self, kind: &Kind, name: &str, uses: &mut BTreeSet<String>) -> bool {
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
pub fn plural(category: &str) -> String {
    match category {
        "structuralcolumn" => "structural columns".into(),
        "detailgroup" => "detail groups".into(),
        "line" => "detail lines".into(),
        "region" => "filled regions".into(),
        c if c.ends_with('s') => format!("{c}es"),
        c => format!("{c}s"),
    }
}

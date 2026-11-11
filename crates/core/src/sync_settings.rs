//! Editing together, the project's settings entry by entry.
//!
//! A project's settings are mostly maps of entries by name: its materials, view templates, text types, wall types… (the
//! library of [`crate::library`]). Sent as whole maps, two people adding a wall type at the same moment would keep one
//! list and lose the other's type. So each entry travels on its own, under a key of its own in [`crate::sync::Changes`]'s
//! `project`:
//!
//! - `materials/Concrete`, `text_types/Note 2.5`: the entry's JSON, or none where it was removed. The key is the setting's
//!   key in the file, `/`, and the entry's name (a `/` or `%` in a name is written `%2F` and `%25`);
//! - `types/wall/Brick 300`, and `types/runs/beam/Beam 200 x 500` for the types of the runs: the category, then the name;
//! - `phases/Existing`: a phase is its name, and its value is its place, a number: the phases stand in the order of their
//!   places, and one of equal place after the other by name. A copy tells a phase's place when it adds the phase or when the
//!   place changes (the phases after one added or taken out in the middle move by one), so there is no list of the order for
//!   the last writer to win: two phases added at the same moment, each at the end, are told with one place and stand in the
//!   order of their names in every copy. A copy takes the place of a phase it is not told of from the order it has them in, which
//!   is the room's only while changes came one at a time: so whoever tells a copy a change of one phase tells it every phase the
//!   room holds with it (Arqiflow's page does), and the order is then the room's own, whatever order the changes came in;
//! - every other setting (units, language, line weights, standard, georeference…) stays one key, replaced whole.
//!
//! The name is what copies of one project share. A library entry's GUID is made by each copy for itself
//! ([`crate::model::Model::sync_library`] makes the library again from the names), as it always was, and what points at an
//! entry, an element's type or a layer's material, names it: so a type added in one copy and the walls of that type arrive
//! together and the walls find it by name, and every copy gives it its own GUID.
//!
//! What a copy does with an entry it is told of: a JSON puts it in (or over what it has: the last to say it wins, for that
//! entry alone), none takes it out. It is applied in the order told, so an entry one copy removed and another changed is
//! as the later of the two says; a shared room (Arqiflow's, Yjs) settles two changes to one key at the same moment, and
//! there an edit wins over a removal, as it does for an element. Entries nobody mentioned are never touched.
//!
//! **A rename** travels as the old name removed and the new one added, which alone loses what another copy does at the same
//! moment with the old name: an edit of it would bring it back beside the new one, and an element put in of it would find
//! no type. So the copy that renamed also tells a rename note, `renames/types/wall/Old`: the new name, as JSON text (made
//! from the library's GUIDs, which a rename keeps: [`renames`]). Where a copy is told a note (the room keeps it a minute,
//! and the page tells it with every change meanwhile), what is told or held of the old name that the copy has no entry for
//! goes to the new: an entry's late edit is put on the new name (and wins over the new name's value), an element's
//! reference to the old name is the new name ([`crate::model::Model::follow`]). A note is for the minute only: it is not in
//! a copy's whole (`entries`) and not kept in an undo history ([`crate::sync::Changes::without_notes`]).
//!
//! A map of entries that is empty has no entry to send: it is one key of its own, `materials: {}`, so that a copy made
//! from a whole room (`complete`) knows it is empty and not left out. Told as a change it is a no-op. So is a **whole map
//! that has entries** (`materials: {…}`, as copies of an older [`crate::sync::FORMAT`] sent every map): told as a change it
//! is left alone, or the entries it holds that were taken out meanwhile would come back at every save of the room; only a
//! whole room (`complete`) takes its entries, those told by key overruling them. A room that keeps one makes it entries
//! and removes the whole key ([`expand`]).
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::library::Kind;
use crate::model::Model;

/// The settings that are a map of entries by name.
const MAPS: [&str; 26] = [
    "materials",
    "images",
    "clouds",
    "cads",
    "families",
    "parameters",
    "view_filters",
    "view_templates",
    "phase_filters",
    "phase_overrides",
    "option_sets",
    "object_styles",
    "line_patterns",
    "line_styles",
    "fill_patterns",
    "region_types",
    "text_types",
    "dimension_types",
    "detail_groups",
    "title_blocks",
    "site_figures",
    "title_values",
    "reinforcement",
    "voltages",
    "distribution_systems",
    "wire_types",
];
const TYPES: &str = "types";
const RUNS: &str = "runs";
const PHASES: &str = "phases";
/// The key of a rename note, then the key of the entry it names.
pub(crate) const RENAMES: &str = "renames";
/// The categories whose types are fields of `types` of their own, by their key in the file and their kind in the library.
const CATEGORIES: [(&str, &str); 10] = [
    ("wall", "wall"),
    ("floor", "floor"),
    ("door", "door"),
    ("window", "window"),
    ("column", "column"),
    ("roof", "roof"),
    ("ceiling", "ceiling"),
    ("railing", "railing"),
    ("structural_column", "structuralcolumn"),
    ("footing", "footing"),
];

fn is_split(setting: &str) -> bool {
    setting == TYPES || setting == PHASES || MAPS.contains(&setting)
}

/// The key of a setting alone, or of an entry: the file's key, then what names the entry, each part's `/` written `%2F`.
fn key(parts: &[&str]) -> String {
    parts.iter().map(|p| p.replace('%', "%25").replace('/', "%2F")).collect::<Vec<_>>().join("/")
}

/// The parts of a key (the setting first), each as it was named.
fn parts(key: &str) -> Vec<String> {
    key.split('/').map(|p| p.replace("%2F", "/").replace("%25", "%")).collect()
}

/// The setting a key is of: what comes before its first `/`, as it was named.
pub fn setting_of(key: &str) -> String {
    parts(key).into_iter().next().unwrap_or_default()
}

/// Whether `key` is of what each copy has of its own (`NOT_SETTINGS`), however its slashes are written.
pub fn is_own(key: &str) -> bool {
    let plain = key.replace("%2F", "/").replace("%25", "%");
    [setting_of(key).as_str(), plain.split('/').next().unwrap_or("")].iter().any(|s| crate::sync::NOT_SETTINGS.contains(s))
}

/// A setting's entries, each by its parts (the setting, then what names it) and JSON. None for a setting that isn't a map of
/// entries; a setting that is, and has none, has an empty list.
fn entry_parts(setting: &str, v: &Value) -> Result<Vec<(Vec<String>, Value)>, String> {
    let mut out = Vec::new();
    let bad = || format!("`{setting}` is not what its entries are made of");
    if setting == PHASES {
        // The list of phases is the order they stand in: each phase's place is its number in it.
        for (i, p) in v.as_array().ok_or_else(bad)?.iter().enumerate() {
            let name = p.as_str().ok_or_else(bad)?;
            out.push((vec![setting.to_string(), name.to_string()], Value::from(i)));
        }
    } else if setting == TYPES {
        for (cat, list) in v.as_object().ok_or_else(bad)? {
            let list = list.as_object().ok_or_else(bad)?;
            if cat == RUNS {
                for (run, types) in list {
                    for (name, t) in types.as_object().ok_or_else(bad)? {
                        out.push(([setting, RUNS, run, name].map(String::from).to_vec(), t.clone()));
                    }
                }
            } else {
                for (name, t) in list {
                    out.push(([setting, cat, name].map(String::from).to_vec(), t.clone()));
                }
            }
        }
    } else if MAPS.contains(&setting) {
        for (name, e) in v.as_object().ok_or_else(bad)? {
            out.push(([setting, name].map(String::from).to_vec(), e.clone()));
        }
    }
    Ok(out)
}

/// The project's settings as the file has them, by key: everything but what each copy has of its own. A map of entries the
/// file leaves out when it is empty or the default is there all the same (as the entries it has).
pub fn settings(m: &Model) -> Result<Map<String, Value>, String> {
    let mut bare = m.clone();
    bare.elements = crate::ids::Elements::new();
    let Value::Object(mut out) = serde_json::to_value(&bare).map_err(|e| e.to_string())? else {
        return Err("a project is not an object".into());
    };
    for k in crate::sync::NOT_SETTINGS {
        out.remove(k);
    }
    for (k, v) in [
        (PHASES, serde_json::to_value(&m.phases)),
        ("phase_filters", serde_json::to_value(&m.phase_filters)),
        ("phase_overrides", serde_json::to_value(&m.phase_overrides)),
    ] {
        out.insert(k.into(), v.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// The settings as they are sent, by key: each entry of a map on its own (see the module), the rest whole.
pub fn flat(m: &Model) -> Result<BTreeMap<String, Value>, String> {
    let mut out = BTreeMap::new();
    for (k, v) in settings(m)? {
        let entries = entry_parts(&k, &v)?;
        if entries.is_empty() {
            out.insert(k.clone(), v);
        }
        for (p, e) in entries {
            out.insert(key(&p.iter().map(String::as_str).collect::<Vec<_>>()), e);
        }
    }
    Ok(out)
}

/// `project` (the keys of [`crate::sync::Changes`]) taken into `map`, the settings as the file has them: each entry put in or
/// out on its own. With `complete` it says everything there is: a setting it names nothing of is left out, and a map of
/// entries has only those it names. A whole map with entries is taken only then (see the module).
pub fn apply(map: &mut Map<String, Value>, project: &BTreeMap<String, Option<String>>, complete: bool) -> Result<(), String> {
    let notes = notes(project)?;
    let mut told: Vec<(Vec<String>, Option<Value>)> = Vec::new();
    let mut named: BTreeSet<String> = BTreeSet::new();
    let mut whole_keys: Vec<(String, Option<Value>)> = Vec::new();
    for (k, text) in project {
        let value = match text {
            Some(t) if t.len() >= crate::sync::MAX_ENTRY => return Err(format!("`{k}` is too large ({} bytes)", t.len())),
            Some(t) => Some(serde_json::from_str::<Value>(t).map_err(|e| format!("`{k}`: {e}"))?),
            None => None,
        };
        let p = parts(k);
        if p[0] == RENAMES {
            continue; // a note, read above
        }
        named.insert(p[0].clone());
        if p.len() == 1 {
            whole_keys.push((p[0].clone(), value));
        } else if is_split(&p[0]) {
            told.push((p, value));
        } else {
            return Err(format!("`{}` has no entries: `{k}`", p[0]));
        }
    }
    if complete {
        map.retain(|k, _| named.contains(k));
        for s in named.iter().filter(|s| is_split(s)) {
            map.insert(s.clone(), empty(s));
        }
        map.entry(TYPES).or_insert_with(|| empty(TYPES));
    }
    // The entries of a whole map come first, so that the entries told by key overrule them.
    let mut entries: Vec<(Vec<String>, Option<Value>)> = Vec::new();
    for (k, v) in whole_keys {
        if !is_split(&k) {
            match v {
                Some(v) => {
                    map.insert(k, v);
                }
                None => {
                    map.remove(&k);
                }
            }
        } else if let Some(v) = v {
            // A whole map: its entries, in a whole room alone. Else it is only read, to be sure of what it is.
            let all = entry_parts(&k, &v)?;
            if complete {
                entries.extend(all.into_iter().map(|(p, e)| (p, Some(e))));
            }
        }
    }
    // What is told of an old name a note says was renamed is told of the new, after the rest: the later edit wins.
    let mut late: Vec<(Vec<String>, Option<Value>)> = Vec::new();
    for (p, v) in told {
        match (v, notes.iter().find(|n| n.entry == p)) {
            (Some(v), Some(n)) => late.push((n.target(), Some(v))),
            (v, _) => entries.push((p, v)),
        }
    }
    let mut places: Vec<(String, Option<f64>)> = Vec::new();
    for (p, v) in entries.into_iter().chain(late) {
        if p[0] == PHASES {
            let [_, name] = <[String; 2]>::try_from(p).map_err(|_| "a phase is one name".to_string())?;
            let place = match v {
                Some(v) => Some(v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| format!("phase `{name}`: its place is a number"))?),
                None => None,
            };
            places.push((name, place));
        } else {
            set(map, &p, v)?;
        }
    }
    // A copy that still has an entry of the old name (an edit of it beat the removal in a shared room) moves it to the new.
    for n in notes.iter().filter(|n| n.entry[0] != PHASES) {
        if let Some(v) = get(map, &n.entry).cloned() {
            set(map, &n.target(), Some(v))?;
            set(map, &n.entry, None)?;
        }
    }
    if named.contains(PHASES) || notes.iter().any(|n| n.entry[0] == PHASES) {
        phases(map, &places, &notes);
    }
    if let Some(Value::Object(runs)) = map.get_mut(TYPES).and_then(|t| t.get_mut(RUNS)) {
        runs.retain(|_, v| v.as_object().is_none_or(|o| !o.is_empty()));
    }
    if let Some(Value::Object(types)) = map.get_mut(TYPES)
        && types.get(RUNS).and_then(Value::as_object).is_some_and(Map::is_empty)
    {
        types.remove(RUNS);
    }
    Ok(())
}

/// A rename note: the entry that was renamed, as it was named (its setting, groups and old name), and its new name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Note {
    pub entry: Vec<String>,
    pub new: String,
}

impl Note {
    /// The old name.
    pub fn old(&self) -> &str {
        self.entry.last().map_or("", String::as_str)
    }

    /// The entry as it is named now.
    pub fn target(&self) -> Vec<String> {
        let mut p = self.entry.clone();
        if let Some(last) = p.last_mut() {
            *last = self.new.clone();
        }
        p
    }

    /// What the entry is in the library.
    pub fn kind(&self) -> Option<Kind> {
        kind_of(&self.entry)
    }
}

/// The rename notes `project` tells (`renames/<the entry's key>`: the new name, as JSON text); those taken away are not
/// read. An error for a note that names no entry of the settings.
pub(crate) fn notes(project: &BTreeMap<String, Option<String>>) -> Result<Vec<Note>, String> {
    let mut out = Vec::new();
    for (k, text) in project {
        let p = parts(k);
        if p[0] != RENAMES {
            continue;
        }
        let entry = p[1..].to_vec();
        if !entry_shape(&entry) {
            return Err(format!("`{k}` is not a rename of an entry of the project's settings"));
        }
        let Some(text) = text else { continue };
        if text.len() >= crate::sync::MAX_ENTRY {
            return Err(format!("`{k}` is too large ({} bytes)", text.len()));
        }
        let new = match serde_json::from_str::<Value>(text) {
            Ok(Value::String(n)) if !n.is_empty() => n,
            _ => return Err(format!("`{k}` is the new name, as JSON text")),
        };
        if entry.last() != Some(&new) {
            out.push(Note { entry, new });
        }
    }
    Ok(out)
}

/// The renames from `before` to `after`, by the key of the entry as it was named, each with its new name: an entry of the
/// library that kept its GUID (what a rename does, [`crate::library::Library::renamed`]) under another name.
pub fn renames(before: &Model, after: &Model) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (_, e) in before.library.entries() {
        let Some((_, now)) = after.library.by_guid(e.guid) else { continue };
        if now.kind != e.kind || now.name == e.name {
            continue;
        }
        let Some(p) = parts_of(&e.kind, &e.name) else { continue };
        out.push((key(&p.iter().map(String::as_str).collect::<Vec<_>>()), now.name.clone()));
    }
    out
}

/// Whether `p` (the setting, its groups, the name) is an entry of the project's settings.
fn entry_shape(p: &[String]) -> bool {
    match (p.first().map(String::as_str), p.len()) {
        (Some(TYPES), 3) => p[1] != RUNS,
        (Some(TYPES), 4) => p[1] == RUNS,
        (Some(s), 2) => s == PHASES || MAPS.contains(&s),
        _ => false,
    }
}

/// The settings that are a map of entries, and what each entry is in the library.
fn named_kinds() -> [(&'static str, Kind); 23] {
    [
        ("materials", Kind::Material),
        ("images", Kind::Picture),
        ("clouds", Kind::Cloud),
        ("cads", Kind::Cad),
        ("families", Kind::Family),
        ("parameters", Kind::Parameter),
        ("view_filters", Kind::ViewFilter),
        ("view_templates", Kind::ViewTemplate),
        ("phase_filters", Kind::PhaseFilter),
        ("option_sets", Kind::OptionSet),
        ("line_patterns", Kind::LinePattern),
        ("line_styles", Kind::LineStyle),
        ("fill_patterns", Kind::FillPattern),
        ("region_types", Kind::FilledRegionType),
        ("text_types", Kind::TextType),
        ("dimension_types", Kind::DimensionType),
        ("detail_groups", Kind::DetailGroup),
        ("title_blocks", Kind::TitleBlock),
        ("reinforcement", Kind::ReinforcementPreset),
        ("voltages", Kind::Voltage),
        ("distribution_systems", Kind::DistributionSystem),
        ("wire_types", Kind::WireType),
        (PHASES, Kind::Phase),
    ]
}

/// What an entry named by `p` is in the library, if it is one.
fn kind_of(p: &[String]) -> Option<Kind> {
    let setting = p.first()?.as_str();
    if setting == TYPES {
        return Some(if p.get(1)? == RUNS { Kind::Type(p.get(2)?.clone()) } else { Kind::Type(library_category(p.get(1)?)?.into()) });
    }
    named_kinds().into_iter().find(|(s, _)| *s == setting).map(|(_, k)| k)
}

/// The key of a built-in category in the file (`structural_column`) as the library calls it (`structuralcolumn`).
fn library_category(file_key: &str) -> Option<&'static str> {
    CATEGORIES.iter().find(|(f, _)| *f == file_key).map(|(_, l)| *l)
}

/// The parts of the key of the entry `name` of `kind`: the inverse of [`kind_of`].
fn parts_of(kind: &Kind, name: &str) -> Option<Vec<String>> {
    let s = |x: &str| x.to_string();
    if let Kind::Type(c) = kind {
        return Some(match CATEGORIES.iter().find(|(_, l)| l == c) {
            Some((f, _)) => vec![s(TYPES), s(f), s(name)],
            None => vec![s(TYPES), s(RUNS), c.clone(), s(name)],
        });
    }
    named_kinds().into_iter().find(|(_, k)| k == kind).map(|(setting, _)| vec![s(setting), s(name)])
}

/// The entry named by `p` as the settings hold it.
fn get<'a>(map: &'a Map<String, Value>, p: &[String]) -> Option<&'a Value> {
    let (setting, rest) = p.split_first()?;
    let mut at = map.get(setting)?;
    for part in rest {
        at = at.as_object()?.get(part)?;
    }
    Some(at)
}

/// What a room that keeps `project` (its settings by key, as it holds them) does to be rid of whole maps: each whole map
/// that has entries is made entries (those the room has not already) and its whole key taken away (none). Maps with no entry
/// and the settings that are one key are left alone. What it says is changes for the room.
pub fn expand(project: &BTreeMap<String, String>) -> Result<BTreeMap<String, Option<String>>, String> {
    let mut out = BTreeMap::new();
    for (k, text) in project {
        let p = parts(k);
        if p.len() != 1 || !is_split(&p[0]) {
            continue;
        }
        if text.len() >= crate::sync::MAX_ENTRY {
            return Err(format!("`{k}` is too large ({} bytes)", text.len()));
        }
        let v = serde_json::from_str::<Value>(text).map_err(|e| format!("`{k}`: {e}"))?;
        let entries = entry_parts(&p[0], &v)?;
        if entries.is_empty() {
            continue;
        }
        out.insert(k.clone(), None);
        for (parts, e) in entries {
            let ek = key(&parts.iter().map(String::as_str).collect::<Vec<_>>());
            if !project.contains_key(&ek) {
                out.insert(ek, Some(e.to_string()));
            }
        }
    }
    Ok(out)
}

/// What a setting is when it has no entry.
fn empty(setting: &str) -> Value {
    match setting {
        PHASES => Value::Array(Vec::new()),
        TYPES => serde_json::json!({"wall": {}, "floor": {}, "door": {}, "window": {}}),
        _ => Value::Object(Map::new()),
    }
}

/// An entry put in or out, wherever its parts say.
fn set(map: &mut Map<String, Value>, p: &[String], v: Option<Value>) -> Result<(), String> {
    let (setting, rest) = p.split_first().ok_or("an entry has no key")?;
    let mut at = map.entry(setting.clone()).or_insert_with(|| empty(setting));
    let (name, groups) = rest.split_last().ok_or_else(|| format!("`{setting}` is not an entry"))?;
    if !entry_shape(p) || setting == PHASES {
        return Err(format!("`{}` is not an entry of the project's settings", p.join("/")));
    }
    for g in groups {
        at = at
            .as_object_mut()
            .ok_or_else(|| format!("`{setting}` is not a map"))?
            .entry(g.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    let o = at.as_object_mut().ok_or_else(|| format!("`{setting}` is not a map"))?;
    match v {
        Some(v) => {
            o.insert(name.clone(), v);
        }
        None => {
            o.remove(name);
        }
    }
    Ok(())
}

/// The phases that stand after the places told: those it has (each at its number in the list), less those told away, and those
/// told, each at its place; in the order of the places, a tie by the names, so that every copy has one order. A phase a note
/// says was renamed takes the place of the old one.
fn phases(map: &mut Map<String, Value>, told: &[(String, Option<f64>)], notes: &[Note]) {
    let have = map.get(PHASES).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str);
    let mut place: BTreeMap<String, f64> = have.enumerate().map(|(i, p)| (p.to_string(), i as f64)).collect();
    for (name, p) in told {
        match p {
            Some(p) => place.insert(name.clone(), *p),
            None => place.remove(name),
        };
    }
    for n in notes.iter().filter(|n| n.entry[0] == PHASES) {
        if let Some(p) = place.remove(n.old()) {
            place.insert(n.new.clone(), p);
        }
    }
    let mut order: Vec<(String, f64)> = place.into_iter().collect();
    order.sort_by(|(an, ap), (bn, bp)| ap.total_cmp(bp).then_with(|| an.cmp(bn)));
    map.insert(PHASES.into(), Value::Array(order.into_iter().map(|(n, _)| Value::String(n)).collect()));
}

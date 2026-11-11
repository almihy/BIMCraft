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
//! - `phases/Existing`: a phase is its name, and `phases` itself is the order they stand in, which is the one thing here that
//!   is a whole value (the last to change it wins), while which phases there are merges entry by entry;
//! - every other setting (units, language, line weights, standard, georeference…) stays one key, replaced whole.
//!
//! The name is what copies of one project share. A library entry's GUID is made by each copy for itself
//! ([`crate::model::Model::sync_library`] makes the library again from the names), as it always was, and what points at an
//! entry, an element's type or a layer's material, names it: so a type added in one copy and the walls of that type arrive
//! together and the walls find it by name, and every copy gives it its own GUID. A rename travels as the old name removed
//! and the new one added (the elements that name it change with it).
//!
//! What a copy does with an entry it is told of: a JSON puts it in (or over what it has: the last to say it wins, for that
//! entry alone), none takes it out. It is applied in the order told, so an entry one copy removed and another changed is
//! as the later of the two says; a shared room (Arqiflow's, Yjs) settles two changes to one key at the same moment, and
//! there an edit wins over a removal, as it does for an element. Entries nobody mentioned are never touched.
//!
//! A map of entries that is empty has no entry to send: it is one key of its own, `materials: {}`, so that a copy made
//! from a whole room (`complete`) knows it is empty and not left out. Told as a change it is a no-op, and so is a whole map
//! of a copy from before this (`materials: {…}`): its entries are put in, and none is taken away.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

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

/// The setting a key is of: what comes before its first `/`.
pub fn setting_of(key: &str) -> &str {
    key.split('/').next().unwrap_or(key)
}

/// A setting's entries, each by its parts (the setting, then what names it) and JSON. None for a setting that isn't a map of
/// entries; a setting that is, and has none, has an empty list.
fn entry_parts(setting: &str, v: &Value) -> Result<Vec<(Vec<String>, Value)>, String> {
    let mut out = Vec::new();
    let bad = || format!("`{setting}` is not what its entries are made of");
    if setting == PHASES {
        for p in v.as_array().ok_or_else(bad)? {
            let name = p.as_str().ok_or_else(bad)?;
            out.push((vec![setting.to_string(), name.to_string()], p.clone()));
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
        if entries.is_empty() || k == PHASES {
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
/// entries has only those it names.
pub fn apply(map: &mut Map<String, Value>, project: &BTreeMap<String, Option<String>>, complete: bool) -> Result<(), String> {
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
    let mut order: Option<Vec<String>> = None;
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
        } else if k == PHASES {
            if let Some(v) = v {
                order = Some(
                    v.as_array()
                        .ok_or("`phases` is not a list")?
                        .iter()
                        .map(|p| p.as_str().map(String::from).ok_or("a phase is not a name"))
                        .collect::<Result<_, _>>()?,
                );
            }
        } else if let Some(v) = v {
            // A whole map: its entries put in, none taken out.
            for (p, e) in entry_parts(&k, &v)? {
                told.push((p, Some(e)));
            }
        }
    }
    let (mut added, mut removed): (Vec<String>, BTreeSet<String>) = (Vec::new(), BTreeSet::new());
    for (p, v) in told {
        if p[0] == PHASES {
            let [_, name] = <[String; 2]>::try_from(p).map_err(|_| "a phase is one name".to_string())?;
            match v {
                Some(_) => {
                    removed.remove(&name);
                    added.push(name);
                }
                None => {
                    added.retain(|a| *a != name);
                    removed.insert(name);
                }
            }
        } else {
            set(map, &p, v)?;
        }
    }
    if named.contains(PHASES) {
        phases(map, order, added, &removed, complete);
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
    let ok = match (setting.as_str(), groups.len()) {
        (TYPES, 1) => groups[0] != RUNS,
        (TYPES, 2) => groups[0] == RUNS,
        (_, 0) => MAPS.contains(&setting.as_str()),
        _ => false,
    };
    if !ok {
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

/// The phases that stand after the entries told: those it has, less those removed, and those added; in the order told where
/// it was told, else the order they stood in with the new ones last. A copy from before this that told only the list has
/// every phase in it.
fn phases(map: &mut Map<String, Value>, order: Option<Vec<String>>, added: Vec<String>, removed: &BTreeSet<String>, complete: bool) {
    let have: Vec<String> = map
        .get(PHASES)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|p| p.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let mut members: Vec<String> = have.into_iter().filter(|p| !removed.contains(p)).collect();
    for a in added {
        if !members.contains(&a) {
            members.push(a);
        }
    }
    let mut now: Vec<String> = Vec::new();
    if let Some(order) = order {
        if complete && members.is_empty() {
            members = order.clone();
        }
        for p in order {
            if members.contains(&p) && !now.contains(&p) {
                now.push(p);
            }
        }
    }
    for p in members {
        if !now.contains(&p) {
            now.push(p);
        }
    }
    map.insert(PHASES.into(), Value::Array(now.into_iter().map(Value::String).collect()));
}

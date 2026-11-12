//! Editing together, the project's settings entry by entry. The design, with the reasons, is in `docs/areas/sync.md`.
//!
//! A project's settings are mostly maps of entries by name: its materials, view templates, text types, wall types… (the
//! library of [`crate::library`]). Each entry is held by its **GUID**, which a file stores and every copy of the project
//! shares, as an element is; the name is what a person calls it, and it can change. So the keys of [`crate::merge::Changes`]'s
//! `project` are:
//!
//! - `materials/<guid>`, `text_types/<guid>`: the entry's body, `{"entry": <its JSON>, "subs"?: [...]}`, or none where it was
//!   removed. The key is the setting's key in the file, `/`, and the entry's GUID;
//! - `materials/<guid>/name`: its name, a JSON string. Name and body are keys of their own, so that a rename and an edit of the
//!   same entry made at the same moment both stand (a shared room keeps the last write of each key, not of the entry);
//! - `types/wall/<guid>` and `types/runs/beam/<guid>`: the types of a category (the file's key for it) and of a run's;
//! - `phases/<guid>`: a phase's body is its place, a number: the phases stand in the order of their places, and one of equal
//!   place after the other by name. A copy tells a phase's place when it adds the phase or when the place changes (the phases
//!   after one added or taken out in the middle move by one), so there is no list of the order for the last writer to win: two
//!   phases added at the same moment, each at the end, are told with one place and stand in the order of their names in every
//!   copy. A copy takes the place of a phase it is not told of from the order it has them in, which is the room's only while
//!   changes came one at a time: so whoever tells a copy a change of one phase tells it every phase the room holds with it
//!   (Arqiflow's page does), and the order is then the room's own, whatever order the changes came in;
//! - `phase_overrides/<status>`, `object_styles/<category>`, `site_figures/<name>`, `title_values/<key>`: maps of a fixed
//!   vocabulary, not of names a person gives: no GUID, the key is the entry's, its value the entry's JSON;
//! - every other setting (units, language, line weights, standard, georeference…) stays one key, replaced whole.
//!
//! What a body holds that points at another entry (a layer's material, a view's template) holds its GUID, as a file does, and
//! so does an element's JSON ([`crate::model::Model::element_to_file`]): renaming an entry changes no body and no element, only
//! its `name` key. A family's types and parameters and a design option set's options are entries of the library too, with
//! GUIDs, which their owner's body names in `subs` (`{"kind", "name", "guid"}`, the name without the owner's).
//!
//! **A copy told an entry** puts it in by its GUID: a body with no entry of that GUID here makes one (it needs a name, from the
//! `name` key; a body alone, what is left of an entry taken out while another copy changed it, makes none), one that has it
//! replaces its body; a `name` renames it, and what in this copy points at it by name follows (the elements, through their file
//! form: [`crate::merge::Model::with_settings`]); none takes it out. Entries nobody mentioned are never touched.
//!
//! **Two entries that end with one name** (two copies made a type of the same name at once) are told apart by GUID: of the
//! entries of one kind with one name, the first by GUID keeps it, and the others are `Name (2)`, `Name (3)`, in the order of
//! their GUIDs, in every copy alike. A copy that renamed one so tells the room (`fixes`), so that a copy made from the room
//! later agrees.
//!
//! **An entry in use is not taken out** (the race of a taking out and work that uses the entry, `docs/areas/sync.md`):
//! [`crate::merge::Model::merged`] keeps an entry the changes take out that this copy's elements, or the changes' own, still name,
//! and tells the room its keys again (`fixes`, from [`restoring`]). The changes that fail for one entry alone are refused alone
//! ([`groups`]).
//!
//! A map of entries that is empty has no entry to send: it is one key of its own, `materials: {}`, so that a copy made from a
//! whole room (`complete`) knows it is empty and not left out. Told as a change it is a no-op. So is a **whole map that has
//! entries** (`materials: {…}`, entries by name without GUIDs): told as a change it is left alone; only a whole room
//! (`complete`) takes its entries, those told by key overruling them.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::ids::{Guid, Id};
use crate::library::{Entry, Kind, Library};
use crate::model::Model;

/// The settings that are a map of entries by name.
const MAPS: [&str; 27] = [
    "materials",
    "images",
    "clouds",
    "cads",
    "image_types",
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
/// The maps whose keys are a fixed vocabulary (a status, a category, Arqiflow's name for a figure): no library entries.
const FIXED: [&str; 4] = ["phase_overrides", "object_styles", "site_figures", "title_values"];
const TYPES: &str = "types";
const RUNS: &str = "runs";
const PHASES: &str = "phases";
/// The last part of the key of an entry's name.
const NAME: &str = "name";
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
    [setting_of(key).as_str(), plain.split('/').next().unwrap_or("")].iter().any(|s| crate::merge::NOT_SETTINGS.contains(s))
}

/// The key of the entry of GUID `guid` held at `path` (the setting, and the groups of `types`), or of its name.
fn entry_key(path: &[String], guid: Guid, name: bool) -> String {
    let g = guid.to_string();
    let mut p: Vec<&str> = path.iter().map(String::as_str).collect();
    p.push(&g);
    if name {
        p.push(NAME);
    }
    key(&p)
}

/// A setting's entries by name, each by its parts (the setting, then what names it) and JSON. None for a setting that isn't a
/// map of entries; a setting that is, and has none, has an empty list. A phase's JSON is its place in the list.
fn entry_parts(setting: &str, v: &Value) -> Result<Vec<(Vec<String>, Value)>, String> {
    let mut out = Vec::new();
    let bad = || format!("`{setting}` is not what its entries are made of");
    if setting == PHASES {
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

/// A model whose library names everything it holds: `m`, or a copy that has had it made so (what a model changed by hand, not
/// by a command, may need).
fn in_step(m: &Model) -> std::borrow::Cow<'_, Model> {
    let now = m.library_names();
    if now.len() == m.library.len() && now.iter().all(|(k, n)| m.library.id(k, n).is_some()) {
        return std::borrow::Cow::Borrowed(m);
    }
    let mut x = m.clone();
    x.sync_library_stable();
    std::borrow::Cow::Owned(x)
}

/// A copy of `m` with no elements, made without copying the elements' index (each push that changes a setting makes one). Every
/// field is named, so that a field added to the model is a compile error here until it is said what a copy without elements has.
pub fn without_elements(m: &Model) -> Model {
    Model {
        format: m.format,
        name: m.name.clone(),
        uid: m.uid,
        units: m.units,
        language: m.language,
        room_area: m.room_area,
        next_id: m.next_id,
        types: m.types.clone(),
        materials: m.materials.clone(),
        images: m.images.clone(),
        clouds: m.clouds.clone(),
        cads: m.cads.clone(),
        image_types: m.image_types.clone(),
        families: m.families.clone(),
        parameters: m.parameters.clone(),
        view_filters: m.view_filters.clone(),
        view_templates: m.view_templates.clone(),
        phases: m.phases.clone(),
        phase_filters: m.phase_filters.clone(),
        phase_overrides: m.phase_overrides.clone(),
        option_sets: m.option_sets.clone(),
        line_weights: m.line_weights.clone(),
        standard: m.standard,
        object_styles: m.object_styles.clone(),
        line_patterns: m.line_patterns.clone(),
        line_styles: m.line_styles.clone(),
        fill_patterns: m.fill_patterns.clone(),
        region_types: m.region_types.clone(),
        text_types: m.text_types.clone(),
        dimension_types: m.dimension_types.clone(),
        detail_groups: m.detail_groups.clone(),
        title_blocks: m.title_blocks.clone(),
        site_figures: m.site_figures.clone(),
        title_values: m.title_values.clone(),
        reinforcement: m.reinforcement.clone(),
        voltages: m.voltages.clone(),
        distribution_systems: m.distribution_systems.clone(),
        wire_types: m.wire_types.clone(),
        georeference: m.georeference.clone(),
        pinned: m.pinned.clone(),
        temporary_dimensions: m.temporary_dimensions,
        elements: crate::ids::Elements::new(),
        library: m.library.clone(),
        loaded: std::collections::BTreeMap::new(),
        derived: crate::derived::Derived::default(),
    }
}

/// The project's settings as the file has them, by key: everything but what each copy has of its own, and what points at a
/// library entry holding its GUID. A map of entries the file leaves out when it is empty or the default is there all the same
/// (as the entries it has).
pub fn settings(m: &Model) -> Result<Map<String, Value>, String> {
    let mut bare = without_elements(m);
    bare.refs_to_file();
    let Value::Object(mut out) = serde_json::to_value(&bare).map_err(|e| e.to_string())? else {
        return Err("a project is not an object".into());
    };
    for k in crate::merge::NOT_SETTINGS {
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

/// The settings as they are sent, by key: each entry of a map on its own, as a body and a name (see the module), the rest whole.
pub fn flat(m: &Model) -> Result<BTreeMap<String, Value>, String> {
    let m = &*in_step(m);
    let mut out = BTreeMap::new();
    for (k, v) in settings(m)? {
        let entries = entry_parts(&k, &v)?;
        if entries.is_empty() {
            out.insert(k.clone(), v);
        }
        for (p, e) in entries {
            let name = p.last().cloned().unwrap_or_default();
            if FIXED.contains(&k.as_str()) {
                out.insert(key(&p.iter().map(String::as_str).collect::<Vec<_>>()), e);
                continue;
            }
            let kind = kind_of(&p).ok_or_else(|| format!("`{}` is not an entry of the project's settings", p.join("/")))?;
            let guid = m.library.guid(&kind, &name).ok_or_else(|| format!("the {} `{name}` has no GUID", kind.label()))?;
            let at = &p[..p.len() - 1];
            let mut body = Map::new();
            body.insert("entry".into(), e);
            let subs = subs_of(m, &kind, &name);
            if !subs.is_empty() {
                body.insert("subs".into(), Value::Array(subs.iter().map(Sub::to_json).collect()));
            }
            out.insert(entry_key(at, guid, false), Value::Object(body));
            out.insert(entry_key(at, guid, true), Value::String(name));
        }
    }
    Ok(out)
}

/// An entry an entry owns: a family's type or parameter, a design option set's option.
#[derive(Clone, Debug, PartialEq)]
struct Sub {
    kind: Kind,
    /// Its name without its owner's.
    name: String,
    guid: Guid,
}

impl Sub {
    fn to_json(&self) -> Value {
        serde_json::json!({"kind": self.kind.to_string(), "name": self.name, "guid": self.guid.to_string()})
    }

    fn read(v: &Value, owner: &Kind) -> Result<Sub, String> {
        let bad = || "a sub entry is {kind, name, guid}".to_string();
        let kind = v.get("kind").and_then(Value::as_str).and_then(Kind::parse).ok_or_else(bad)?;
        let name = v.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).ok_or_else(bad)?.to_string();
        let guid = v.get("guid").and_then(Value::as_str).and_then(Guid::parse).filter(|g| g.pending().is_none() && !g.is_nil());
        if !owns(owner, &kind) {
            return Err(format!("a {} has no {}", owner.label(), kind.label()));
        }
        Ok(Sub { kind, name, guid: guid.ok_or_else(bad)? })
    }
}

/// Whether an entry of `owner` owns entries of `kind`: a family its types and parameters, a design option set its options.
fn owns(owner: &Kind, kind: &Kind) -> bool {
    matches!((owner, kind), (Kind::Family, Kind::FamilyType | Kind::FamilyParameter) | (Kind::OptionSet, Kind::DesignOption))
}

/// Whether entries of `kind` own entries.
fn is_owner(kind: &Kind) -> bool {
    matches!(kind, Kind::Family | Kind::OptionSet)
}

/// Whether entries of `kind` are owned by an entry.
fn is_owned(kind: &Kind) -> bool {
    matches!(kind, Kind::FamilyType | Kind::FamilyParameter | Kind::DesignOption)
}

/// The entries the entry `name` of `kind` owns in `m`'s library, as they are told.
fn subs_of(m: &Model, kind: &Kind, name: &str) -> Vec<Sub> {
    let prefix = format!("{name}: ");
    let mut out: Vec<Sub> = m
        .library
        .entries()
        .filter(|(_, e)| owns(kind, &e.kind))
        .filter_map(|(_, e)| Some(Sub { kind: e.kind.clone(), name: e.name.strip_prefix(&prefix)?.to_string(), guid: e.guid }))
        .collect();
    out.sort_by(|a, b| (a.kind.to_string(), &a.name).cmp(&(b.kind.to_string(), &b.name)));
    out
}

/// What a key of `project` is.
enum Told {
    /// A setting that is one key (units, language…), or a map of entries told whole.
    Setting(String),
    /// An entry of a fixed-vocabulary map, by its key.
    Fixed(String, String),
    /// An entry of the library, by its GUID: where its map is, what it is, and whether the key is its name.
    Entry { path: Vec<String>, kind: Kind, guid: Guid, name: bool },
}

fn read_key(k: &str) -> Result<Told, String> {
    let p = parts(k);
    let setting = p[0].as_str();
    if p.len() == 1 {
        return Ok(Told::Setting(p[0].clone()));
    }
    if FIXED.contains(&setting) {
        return match p.as_slice() {
            [s, key] => Ok(Told::Fixed(s.clone(), key.clone())),
            _ => Err(format!("`{k}` is not an entry of `{setting}`")),
        };
    }
    // The path of the map the entry is in, and what its entries are.
    let (depth, kind) = match setting {
        TYPES if p.get(1).map(String::as_str) == Some(RUNS) => {
            let cat = p.get(2).ok_or_else(|| format!("`{k}` names no category"))?;
            if !crate::model::RUN_CATEGORIES.contains(&cat.as_str()) {
                return Err(format!("`{cat}` is not a category of runs: `{k}`"));
            }
            (4, Kind::Type(cat.clone()))
        }
        TYPES => {
            let cat = p.get(1).ok_or_else(|| format!("`{k}` names no category"))?;
            (3, Kind::Type(library_category(cat).ok_or_else(|| format!("`{cat}` is not a category of types: `{k}`"))?.into()))
        }
        PHASES => (2, Kind::Phase),
        s => (
            2,
            named_kinds().into_iter().find(|(n, _)| *n == s).map(|(_, kind)| kind).ok_or_else(|| format!("`{s}` has no entries: `{k}`"))?,
        ),
    };
    let name = p.len() == depth + 1 && p[depth] == NAME;
    if p.len() != depth && !name {
        return Err(format!("`{k}` is not an entry of the project's settings"));
    }
    let guid = Guid::parse(&p[depth - 1])
        .filter(|g| g.pending().is_none() && !g.is_nil())
        .ok_or_else(|| format!("`{k}` is not an entry's GUID"))?;
    Ok(Told::Entry { path: p[..depth - 1].to_vec(), kind, guid, name })
}

/// What a copy was told of one entry: its body (none where it was taken out) and its name, each where it was said.
#[derive(Default)]
struct Say {
    kind: Option<Kind>,
    body: Option<Option<Body>>,
    name: Option<String>,
}

struct Body {
    value: Value,
    subs: Vec<Sub>,
}

fn read_body(text: &str, kind: &Kind, k: &str) -> Result<Body, String> {
    let v = serde_json::from_str::<Value>(text).map_err(|e| format!("`{k}`: {e}"))?;
    let entry = v.get("entry").cloned().ok_or_else(|| format!("`{k}` is {{\"entry\": …}}"))?;
    let subs = match v.get("subs") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.iter().map(|s| Sub::read(s, kind)).collect::<Result<_, _>>().map_err(|e| format!("`{k}`: {e}"))?,
        Some(_) => return Err(format!("`{k}`: `subs` is a list")),
    };
    Ok(Body { value: entry, subs })
}

/// An entry of the library as the settings hold it, while a change is taken in.
struct Held {
    kind: Kind,
    /// Where its map is: the setting, and the groups of `types`.
    path: Vec<String>,
    guid: Guid,
    /// Its id here, none for an entry new here.
    id: Option<Id>,
    /// Its name here before, none for an entry new here.
    was: Option<String>,
    /// Its name as it is asked to be.
    name: String,
    /// What it is, where it is told; else what the settings hold under `was`.
    value: Option<Value>,
    /// The entries it owns, where told; else those this copy has.
    subs: Option<Vec<Sub>>,
    /// A phase's place, where told.
    place: Option<f64>,
    /// From a whole map (an older copy's) and not by key.
    whole: bool,
}

/// What taking changes into the settings made of the library.
pub struct Applied {
    /// The library as it is now: entries put in, renamed and taken out.
    pub library: Library,
    /// Each entry that is named otherwise now (its kind, its name before, its name now): what in this copy points at it by
    /// name is to follow.
    pub renames: Vec<(Kind, String, String)>,
    /// What this copy tells the room that it did not hear: the name of an entry it had to rename, a body left with no name (none: to
    /// be taken out of the room). See the module.
    pub fixes: BTreeMap<String, Option<String>>,
}

/// `project` (the keys of [`crate::merge::Changes`]) taken into `map`, the settings as [`settings`] gives them: each entry put in
/// or out on its own, by GUID. `m` is the model they are taken into (its library and its ids). With `complete` it says everything
/// there is: a setting it names nothing of is left out, and a map of entries has only those it names. A whole map with entries
/// is taken only then (see the module).
pub fn apply(
    m: &Model,
    map: &mut Map<String, Value>,
    project: &BTreeMap<String, Option<String>>,
    complete: bool,
) -> Result<Applied, String> {
    let mut named: BTreeSet<String> = BTreeSet::new();
    let mut whole_keys: Vec<(String, Option<Value>)> = Vec::new();
    let mut fixed: Vec<(String, String, Option<Value>)> = Vec::new();
    let mut said: BTreeMap<(Vec<String>, Guid), Say> = BTreeMap::new();
    for (k, text) in project {
        if let Some(t) = text
            && t.len() >= crate::merge::MAX_ENTRY
        {
            return Err(format!("`{k}` is too large ({} bytes)", t.len()));
        }
        let told = read_key(k)?;
        let value = |text: &Option<String>| -> Result<Option<Value>, String> {
            text.as_deref().map(|t| serde_json::from_str::<Value>(t).map_err(|e| format!("`{k}`: {e}"))).transpose()
        };
        match told {
            Told::Setting(s) => {
                named.insert(s.clone());
                whole_keys.push((s, value(text)?));
            }
            Told::Fixed(s, fk) => {
                named.insert(s.clone());
                fixed.push((s, fk, value(text)?));
            }
            Told::Entry { path, kind, guid, name } => {
                named.insert(path[0].clone());
                let say = said.entry((path, guid)).or_default();
                say.kind = Some(kind.clone());
                if name {
                    // A name taken away says nothing: an entry has one while it is there.
                    if let Some(t) = text {
                        match serde_json::from_str::<Value>(t) {
                            Ok(Value::String(n)) if !n.is_empty() => say.name = Some(n),
                            _ => return Err(format!("`{k}` is the entry's name, as JSON text")),
                        }
                    }
                } else {
                    say.body = Some(text.as_deref().map(|t| read_body(t, &kind, k)).transpose()?);
                }
            }
        }
    }
    if complete {
        map.retain(|k, _| named.contains(k));
        for s in named.iter().filter(|s| is_split(s)) {
            map.insert(s.clone(), empty(s));
        }
        map.entry(TYPES).or_insert_with(|| empty(TYPES));
    }
    // What the library has, which a whole room says all of again.
    let mut held: Vec<Held> = Vec::new();
    if !complete {
        for (id, e) in m.library.entries() {
            if let Some(path) = path_of(&e.kind) {
                held.push(Held {
                    kind: e.kind.clone(),
                    path,
                    guid: e.guid,
                    id: Some(id),
                    was: Some(e.name.clone()),
                    name: e.name.clone(),
                    value: None,
                    subs: None,
                    place: None,
                    whole: false,
                });
            }
        }
    }
    // The settings that are one key; the entries of a whole map are only read to be sure of what they are, and taken in a whole room.
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
            let all = entry_parts(&k, &v)?;
            if !complete {
                continue;
            }
            for (p, e) in all {
                let name = p.last().cloned().unwrap_or_default();
                let kind = kind_of(&p).ok_or_else(|| format!("`{}` is not an entry of the project's settings", p.join("/")))?;
                if FIXED.contains(&k.as_str()) {
                    fixed.insert(0, (k.clone(), name, Some(e)));
                    continue;
                }
                let guid = m.library.guid(&kind, &name).unwrap_or_else(|| crate::library::stable_guid(m.uid, Some(&kind), &name));
                let id = m.library.by_guid(guid).filter(|(_, x)| x.kind == kind).map(|(id, _)| id);
                let (value, place) = if k == PHASES {
                    (None, Some(e.as_f64().ok_or_else(|| format!("phase `{name}`: its place is a number"))?))
                } else {
                    (Some(e), None)
                };
                held.push(Held { path: p[..p.len() - 1].to_vec(), kind, guid, id, was: None, name, value, subs: None, place, whole: true });
            }
        }
    }
    // What this copy tells the room that it did not hear: the names of entries it had to rename, bodies left with no entry.
    let mut fixes: BTreeMap<String, Option<String>> = BTreeMap::new();
    // The entries told by key.
    for ((path, guid), say) in said {
        let Some(kind) = say.kind else { continue };
        match say.body {
            Some(None) => held.retain(|h| h.guid != guid),
            Some(Some(body)) => {
                let (value, place) = if kind == Kind::Phase {
                    (None, Some(body.value.as_f64().filter(|x| x.is_finite()).ok_or_else(|| "a phase's place is a number".to_string())?))
                } else {
                    (Some(body.value), None)
                };
                if let Some(h) = held.iter_mut().find(|h| h.guid == guid) {
                    if h.kind != kind {
                        return Err(format!("{guid} is a {} here, not a {}", h.kind.label(), kind.label()));
                    }
                    (h.value, h.place, h.subs, h.whole) = (value, place, Some(body.subs), false);
                    if let Some(n) = say.name {
                        h.name = n;
                    }
                } else {
                    // A body with no name for an entry this copy does not have is what is left of an entry taken out while another
                    // copy changed it (a shared room keeps the changed key of the two): the entry stays out, and the body is taken out
                    // of the room (a `fixes` entry that says none), or a copy that keeps the room has a difference it never ends.
                    let Some(name) = say.name else {
                        fixes.insert(entry_key(&path, guid, false), None);
                        continue;
                    };
                    if m.elements.contains_key(guid) {
                        return Err(format!("{guid} is an element's GUID"));
                    }
                    held.retain(|h| !(h.whole && h.kind == kind && h.name == name));
                    held.push(Held {
                        id: m.library.by_guid(guid).filter(|(_, x)| x.kind == kind).map(|(id, _)| id),
                        kind,
                        path,
                        guid,
                        was: None,
                        name,
                        value,
                        subs: Some(body.subs),
                        place,
                        whole: false,
                    });
                }
            }
            None => {
                if let (Some(n), Some(h)) = (say.name, held.iter_mut().find(|h| h.guid == guid)) {
                    if h.kind != kind {
                        return Err(format!("{guid} is a {} here, not a {}", h.kind.label(), kind.label()));
                    }
                    h.name = n;
                }
            }
        }
    }
    // Two entries of one kind asking for one name: the first by GUID keeps it, the others take `Name (2)`, `Name (3)`.
    let mut finals: Vec<String> = held.iter().map(|h| h.name.clone()).collect();
    let mut taken: BTreeSet<(Kind, String)> = held.iter().map(|h| (h.kind.clone(), h.name.clone())).collect();
    let mut groups: BTreeMap<(Kind, String), Vec<usize>> = BTreeMap::new();
    for (i, h) in held.iter().enumerate() {
        groups.entry((h.kind.clone(), h.name.clone())).or_default().push(i);
    }
    for ((kind, name), mut at) in groups {
        if at.len() < 2 {
            continue;
        }
        at.sort_by_key(|&i| held[i].guid);
        // `Brick (2)` asked for twice is `Brick (3)` for the later, never `Brick (2) (2)`: the count is of the name without its own.
        let base = without_count(&name);
        let mut n = 2;
        for &i in &at[1..] {
            let mut candidate = format!("{base} ({n})");
            while taken.contains(&(kind.clone(), candidate.clone())) {
                n += 1;
                candidate = format!("{base} ({n})");
            }
            taken.insert((kind.clone(), candidate.clone()));
            fixes.insert(entry_key(&held[i].path, held[i].guid, true), Some(Value::String(candidate.clone()).to_string()));
            finals[i] = candidate;
            n += 1;
        }
    }
    // The maps, with each entry under its name now. A map no entry of which changed is left as it is.
    let dirty = |h: &Held, name: &str| h.value.is_some() || h.place.is_some() || h.id.is_none() || h.was.as_deref() != Some(name);
    let mut maps: BTreeSet<Vec<String>> = BTreeSet::new();
    let known: BTreeSet<Guid> = held.iter().map(|h| h.guid).collect();
    for (i, h) in held.iter().enumerate() {
        if dirty(h, &finals[i]) {
            maps.insert(h.path.clone());
        }
    }
    // A map an entry was taken out of is rebuilt too.
    for (_, e) in m.library.entries() {
        if let Some(path) = path_of(&e.kind)
            && !known.contains(&e.guid)
        {
            maps.insert(path);
        }
    }
    let mut present: BTreeSet<usize> = BTreeSet::new();
    for path in &maps {
        if path.len() == 1 && path[0] == PHASES {
            let old: Vec<String> =
                map.get(PHASES).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
            let mut order: Vec<(f64, &str, usize)> = Vec::new();
            for (i, h) in held.iter().enumerate().filter(|(_, h)| h.path == *path) {
                let place = h
                    .place
                    .or_else(|| h.was.as_ref().and_then(|w| old.iter().position(|o| o == w)).map(|p| p as f64))
                    .unwrap_or(old.len() as f64);
                order.push((place, finals[i].as_str(), i));
            }
            order.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
            present.extend(order.iter().map(|o| o.2));
            map.insert(PHASES.into(), Value::Array(order.into_iter().map(|o| Value::String(o.1.to_string())).collect()));
            continue;
        }
        let mut old = take_object(map, path);
        let mut new = Map::new();
        for (i, h) in held.iter_mut().enumerate().filter(|(_, h)| h.path == *path) {
            let value = h.value.take().or_else(|| h.was.as_ref().and_then(|w| old.remove(w)));
            if let Some(v) = value {
                new.insert(finals[i].clone(), v);
                present.insert(i);
            }
        }
        put_object(map, path, new)?;
    }
    for (i, h) in held.iter().enumerate() {
        if !maps.contains(&h.path) {
            present.insert(i);
        }
    }
    for (s, k, v) in fixed {
        let at = map.entry(s.clone()).or_insert_with(|| empty(&s));
        let o = at.as_object_mut().ok_or_else(|| format!("`{s}` is not a map"))?;
        match v {
            Some(v) => {
                o.insert(k, v);
            }
            None => {
                o.remove(&k);
            }
        }
    }
    if let Some(Value::Object(runs)) = map.get_mut(TYPES).and_then(|t| t.get_mut(RUNS)) {
        runs.retain(|_, v| v.as_object().is_none_or(|o| !o.is_empty()));
    }
    if let Some(Value::Object(types)) = map.get_mut(TYPES)
        && types.get(RUNS).and_then(Value::as_object).is_some_and(Map::is_empty)
    {
        types.remove(RUNS);
    }
    // The library: each entry under its name now, and the entries those own.
    let mut entries: Vec<(Option<Id>, Entry)> = Vec::new();
    let mut guids: BTreeSet<Guid> = BTreeSet::new();
    for (i, h) in held.iter().enumerate().filter(|(i, _)| present.contains(i)) {
        guids.insert(h.guid);
        entries.push((h.id, Entry { kind: h.kind.clone(), name: finals[i].clone(), guid: h.guid }));
        if !is_owner(&h.kind) {
            continue;
        }
        let subs: Vec<Sub> = match (&h.subs, &h.was) {
            (Some(s), _) => s.clone(),
            (None, Some(was)) => subs_of(m, &h.kind, was),
            (None, None) => Vec::new(),
        };
        for s in subs {
            let id = m.library.by_guid(s.guid).filter(|(_, x)| x.kind == s.kind).map(|(id, _)| id);
            guids.insert(s.guid);
            entries.push((id, Entry { kind: s.kind, name: format!("{}: {}", finals[i], s.name), guid: s.guid }));
        }
    }
    // What the library has that no setting is of (nothing the settings tell of) stays as it is.
    if !complete {
        for (id, e) in m.library.entries() {
            if path_of(&e.kind).is_none() && !is_owned(&e.kind) && !guids.contains(&e.guid) {
                entries.push((Some(id), e.clone()));
            }
        }
    }
    let library = m.library.rebuilt(entries, &|id| !m.elements.contains_key(id))?;
    let mut renames = Vec::new();
    for (_, e) in library.entries() {
        if let Some((_, was)) = m.library.by_guid(e.guid)
            && was.kind == e.kind
            && was.name != e.name
        {
            renames.push((e.kind.clone(), was.name.clone(), e.name.clone()));
        }
    }
    Ok(Applied { library, renames, fixes })
}

/// `name` without a count at its end (` (2)`): what `Name (2)`, `Name (3)` are counts of.
fn without_count(name: &str) -> &str {
    let Some(open) = name.strip_suffix(')').and_then(|n| n.rfind(" (")) else { return name };
    let count = &name[open + 2..name.len() - 1];
    if open > 0 && !count.is_empty() && count.bytes().all(|b| b.is_ascii_digit()) { &name[..open] } else { name }
}

/// The entries of `m`'s library that a change of the settings may take out, with what each one is called, as a map of the settings
/// holds it: an entry that is a map's, or one an entry owns, which goes with its owner (a family's type, a design option). Each
/// as the entry that stands for it in the settings: its own GUID, or its owner's.
pub fn standing_for(m: &Model, kind: &Kind, name: &str, guid: Guid) -> Option<Guid> {
    if path_of(kind).is_some() {
        return Some(guid);
    }
    let owner = match kind {
        Kind::FamilyType | Kind::FamilyParameter => Kind::Family,
        Kind::DesignOption => Kind::OptionSet,
        _ => return None,
    };
    m.library.guid(&owner, name.split_once(": ")?.0)
}

/// What a copy tells the room to put an entry back that a change of the room took out, but this copy keeps (an entry that is
/// in use): its body and its name, each by its key, as `m` says them now.
pub fn restoring(m: &Model, guids: &BTreeSet<Guid>) -> Result<BTreeMap<String, Option<String>>, String> {
    let all = flat(m)?;
    let mut out = BTreeMap::new();
    for g in guids {
        let Some((_, e)) = m.library.by_guid(*g) else { continue };
        let Some(path) = path_of(&e.kind) else { continue };
        for name in [false, true] {
            let k = entry_key(&path, *g, name);
            if let Some(v) = all.get(&k) {
                out.insert(k, Some(v.to_string()));
            }
        }
    }
    Ok(out)
}

/// `project` without what it tells of the entries `guids` (their bodies and names).
pub fn without(project: &BTreeMap<String, Option<String>>, guids: &BTreeSet<Guid>) -> BTreeMap<String, Option<String>> {
    project
        .iter()
        .filter(|(k, _)| !matches!(read_key(k), Ok(Told::Entry { guid, .. }) if guids.contains(&guid)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// The keys of `project` that stand or fall together: an entry's body and name, each other key alone. A key that cannot be read is
/// a group of its own, which then says what is wrong with it.
pub fn groups(project: &BTreeMap<String, Option<String>>) -> Vec<Vec<String>> {
    let mut by_entry: BTreeMap<(Vec<String>, Guid), Vec<String>> = BTreeMap::new();
    let mut out: Vec<Vec<String>> = Vec::new();
    for k in project.keys() {
        match read_key(k) {
            Ok(Told::Entry { path, guid, .. }) => by_entry.entry((path, guid)).or_default().push(k.clone()),
            _ => out.push(vec![k.clone()]),
        }
    }
    out.extend(by_entry.into_values());
    out
}

/// The map held at `path` (the setting, and the groups of `types`), taken out of `map`, left empty.
fn take_object(map: &mut Map<String, Value>, path: &[String]) -> Map<String, Value> {
    let mut at = map.get_mut(&path[0]);
    for part in &path[1..] {
        at = at.and_then(|v| v.as_object_mut()).and_then(|o| o.get_mut(part));
    }
    match at {
        Some(Value::Object(o)) => std::mem::take(o),
        _ => Map::new(),
    }
}

/// `entries` put as the map held at `path`, its groups made where they are not.
fn put_object(map: &mut Map<String, Value>, path: &[String], entries: Map<String, Value>) -> Result<(), String> {
    let (setting, groups) = path.split_first().ok_or("an entry has no key")?;
    let mut at = map.entry(setting.clone()).or_insert_with(|| empty(setting));
    for g in groups {
        at = at
            .as_object_mut()
            .ok_or_else(|| format!("`{setting}` is not a map"))?
            .entry(g.clone())
            .or_insert_with(|| Value::Object(Map::new()));
    }
    *at = Value::Object(entries);
    Ok(())
}

/// The settings that are a map of entries, and what each entry is in the library.
fn named_kinds() -> [(&'static str, Kind); 24] {
    [
        ("materials", Kind::Material),
        ("images", Kind::Picture),
        ("clouds", Kind::Cloud),
        ("cads", Kind::Cad),
        ("image_types", Kind::Image),
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

/// What an entry named by `p` (the setting, its groups, the name) is in the library, if it is one.
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

/// Where the map of the entries of `kind` is: its setting, and for types the category's group. None for what is not an
/// entry of a map of the settings (an entry an entry owns).
fn path_of(kind: &Kind) -> Option<Vec<String>> {
    let s = |x: &str| x.to_string();
    if let Kind::Type(c) = kind {
        return Some(match CATEGORIES.iter().find(|(_, l)| l == c) {
            Some((f, _)) => vec![s(TYPES), s(f)],
            None => vec![s(TYPES), s(RUNS), c.clone()],
        });
    }
    named_kinds().into_iter().find(|(_, k)| k == kind).map(|(setting, _)| vec![s(setting)])
}

/// What a setting is when it has no entry.
fn empty(setting: &str) -> Value {
    match setting {
        PHASES => Value::Array(Vec::new()),
        TYPES => serde_json::json!({"wall": {}, "floor": {}, "door": {}, "window": {}}),
        _ => Value::Object(Map::new()),
    }
}

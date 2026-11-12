//! Elements of another project brought into this one, as Revit's Insert from File: those asked for, and what they point at
//! that this project hasn't. An element keeps its id where this project has none of it, else gets the next; it keeps its
//! GUID, unless this project has an element of that GUID already (the same project inserted into itself, or a copy of it),
//! when it is a copy, and a copy is a new element with a new GUID. What the inserted elements point at among themselves
//! follows them; what they point at that this project has (by GUID, or a level of the same name) they point at here. The
//! library they use comes as they do: a type, a material, a family this project has by GUID is its own, whatever it's
//! called here; one it has only by name is this project's; one it hasn't comes in, with its GUID.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::graphics;
use crate::ids::LINKED;
use crate::library::{Entry, Kind};
use crate::model::{Element, Guid, Id, Model};
use crate::phasing;

/// What an insert made.
#[derive(Debug, Default, PartialEq)]
pub struct Inserted {
    /// Each element brought in: its id in the other project, and its id here.
    pub ids: BTreeMap<Id, Id>,
    /// The elements here that are copies, with a GUID of their own, because this project had theirs.
    pub copies: Vec<Id>,
    /// The types, families, materials, pictures and parameters brought in, each with its GUID, as `what: name`.
    pub library: Vec<String>,
    /// What was left off an element, and why.
    pub left_out: Vec<String>,
}

/// Elements `ids` of `from` (all of them when none), and what they point at that `to` hasn't, brought into `to`. Unchecked:
/// the engine checks the model after the command.
pub fn insert(to: &mut Model, from: &Model, ids: &[Id]) -> Result<Inserted, String> {
    insert_as(to, from, ids, false)
}

/// [`insert`], where with `linked` every element comes in under an id from [`LINKED`] up, which is how a view tells it from
/// the project's own.
pub(crate) fn insert_as(to: &mut Model, from: &Model, ids: &[Id], linked: bool) -> Result<Inserted, String> {
    if linked {
        to.next_id = to.next_id.max(LINKED);
    }
    let asked: Vec<Id> = if ids.is_empty() { from.elements.keys().copied().collect() } else { ids.to_vec() };
    if let Some(missing) = asked.iter().find(|id| !from.elements.contains_key(**id)) {
        return Err(format!("the other project has no element {missing}"));
    }
    // A level this project has by name stands in for the other's, when the other's isn't asked for.
    let levels_here: HashMap<&str, Guid> = to.levels().into_iter().map(|(id, l)| (l.name.as_str(), to.guid(id))).collect();
    let mut instead: HashMap<Guid, Guid> = HashMap::new();
    // What's asked, and what it points at that this project hasn't, and so on.
    let mut take: BTreeSet<Id> = asked.iter().copied().collect();
    let mut todo = asked.clone();
    while let Some(id) = todo.pop() {
        for g in from.elements.get(id).map(needs).unwrap_or_default() {
            let Some(rid) = from.elements.id(g) else { continue };
            if take.contains(&rid) || to.elements.contains_key(g) {
                continue;
            }
            if let (Some(l), Some(from_guid)) = (from.level(rid), from.elements.guid(rid))
                && let Some(&here) = levels_here.get(l.name.as_str())
            {
                instead.insert(from_guid, here);
                continue;
            }
            take.insert(rid);
            todo.push(rid);
        }
    }
    let mut out = Inserted::default();
    // An element whose GUID this project has is a copy: a GUID of its own.
    for &id in &take {
        let Some(g) = from.elements.guid(id) else { continue };
        if to.elements.contains_key(g) {
            let mut fresh = to.elements.fresh();
            while instead.values().any(|v| *v == fresh) {
                fresh = to.elements.fresh();
            }
            instead.insert(g, fresh);
        }
    }
    let renames = bring_library(to, from, &take, &mut out)?;
    let mut names = Names::of(to);
    for &id in &take {
        let (Some(e), Some(g)) = (from.elements.get(id), from.elements.guid(id)) else { continue };
        let mut e = e.clone();
        e.refs_mut(&mut |r| {
            if let Some(n) = instead.get(r) {
                *r = *n;
            }
        });
        names.free(&mut e);
        e.lib_refs_mut(&mut |kind, name| {
            if let Some(n) = renames.get(&(kind.clone(), name.clone())) {
                *name = n.clone();
            }
        });
        let left = phasing_here(to, &mut e);
        let taken = to.elements.contains_key(id) || to.library.get(id).is_some();
        let new = if linked || taken || id < crate::model::FIRST_ID { to.fresh_id() } else { id };
        let guid = instead.get(&g).copied().unwrap_or(g);
        to.elements.insert_as(new, guid, e)?;
        to.next_id = to.next_id.max(new + 1);
        out.left_out.extend(left.into_iter().map(|why| format!("element {new}: {why}")));
        if guid != g {
            out.copies.push(new);
        }
        out.ids.insert(id, new);
    }
    tidy_graphics(to);
    Ok(out)
}

/// What `e` can't be without: what it points at, but the elements its view's graphics override, which are left behind.
fn needs(e: &Element) -> Vec<Guid> {
    let overridden: BTreeSet<Guid> = graphics::graphics_of_element(e).map(|g| g.elements.keys().copied().collect()).unwrap_or_default();
    e.refs().into_iter().filter(|g| !overridden.contains(g)).collect()
}

/// The names each library entry `from` names goes by here, where they differ: `(kind, name there)` to the name here.
type Renames = HashMap<(Kind, String), String>;

/// The library entries of `from` that `to` hasn't: types, materials, pictures, families, project parameters, line patterns
/// and line styles come along,
/// each with its GUID. An entry this project has by GUID is its own, under the name it has here (the names in what comes in
/// follow); one it has by name, with another GUID, is this project's. What comes in names entries by the names returned.
fn bring_library(to: &mut Model, from: &Model, take: &BTreeSet<Id>, out: &mut Inserted) -> Result<Renames, String> {
    let mut renames = Renames::new();
    if take.is_empty() {
        return Ok(renames);
    }
    let mut copied = Vec::new();
    for (_, e) in from.library.entries() {
        if let Some((_, here)) = to.library.by_guid(e.guid) {
            if here.kind == e.kind && here.name != e.name {
                renames.insert((e.kind.clone(), e.name.clone()), here.name.clone());
            }
            continue;
        }
        if to.library.id(&e.kind, &e.name).is_some() || !copy_definition(to, from, &e.kind, &e.name) {
            continue;
        }
        let id = to.fresh_library_id();
        to.library.insert(id, Entry { kind: e.kind.clone(), name: e.name.clone(), guid: e.guid })?;
        out.library.push(format!("{}: {}", e.kind.label(), e.name));
        copied.push((e.kind.clone(), e.name.clone()));
    }
    // A family that came in brings its types and parameters, with their GUIDs.
    let came: BTreeSet<String> = copied.iter().filter(|(k, _)| *k == Kind::Family).map(|(_, n)| n.clone()).collect();
    for (_, e) in from.library.entries().filter(|(_, e)| matches!(e.kind, Kind::FamilyType | Kind::FamilyParameter)) {
        let of = came.iter().any(|f| e.name.starts_with(&format!("{f}: ")));
        if of && to.library.id(&e.kind, &e.name).is_none() && to.library.by_guid(e.guid).is_none() {
            let id = to.fresh_library_id();
            to.library.insert(id, Entry { kind: e.kind.clone(), name: e.name.clone(), guid: e.guid })?;
        }
    }
    // What came in names the materials, pictures and line patterns it's made of as this project does.
    let here = |kind: &Kind, name: &mut String| {
        if let Some(n) = renames.get(&(kind.clone(), name.clone())) {
            *name = n.clone();
        }
    };
    for (kind, name) in copied {
        match kind {
            Kind::Family => {
                if let Some(f) = to.families.get_mut(&name) {
                    f.material_refs_mut(&mut |m| here(&Kind::Material, m));
                }
            }
            Kind::Material => {
                if let Some(t) = to.materials.get_mut(&name).and_then(|m| m.texture.as_mut()) {
                    here(&Kind::Picture, &mut t.image);
                }
                for p in to.materials.get_mut(&name).into_iter().flat_map(|x| [&mut x.cut_pattern, &mut x.surface_pattern]).flatten() {
                    here(&Kind::FillPattern, p);
                }
            }
            Kind::LineStyle => {
                if let Some(p) = to.line_styles.get_mut(&name).and_then(|s| s.pattern.as_mut()) {
                    here(&Kind::LinePattern, p);
                }
            }
            Kind::DetailGroup => {
                for m in to.detail_groups.get_mut(&name).into_iter().flat_map(|g| g.members.iter_mut()) {
                    m.element.lib_refs_mut(&mut |k: &Kind, n: &mut String| here(k, n));
                }
            }
            Kind::FilledRegionType => {
                if let Some(t) = to.region_types.get_mut(&name) {
                    for p in [&mut t.foreground, &mut t.background].into_iter().flatten() {
                        here(&Kind::FillPattern, p);
                    }
                }
            }
            Kind::DistributionSystem => {
                for v in to.distribution_systems.get_mut(&name).into_iter().flat_map(|d| d.voltages_mut()) {
                    here(&Kind::Voltage, v);
                }
            }
            Kind::Type(c) => {
                let t = &mut to.types;
                let layers = match c.as_str() {
                    "wall" => t.wall.get_mut(&name).map(|x| &mut x.layers),
                    "floor" => t.floor.get_mut(&name).map(|x| &mut x.layers),
                    "roof" => t.roof.get_mut(&name).map(|x| &mut x.layers),
                    "ceiling" => t.ceiling.get_mut(&name).map(|x| &mut x.layers),
                    _ => None,
                };
                for l in layers.into_iter().flatten() {
                    here(&Kind::Material, &mut l.material);
                }
                if let Some(r) = t.runs.get_mut(&c).and_then(|r| r.get_mut(&name)) {
                    here(&Kind::Material, &mut r.material);
                }
            }
            _ => {}
        }
    }
    Ok(renames)
}

/// Entry `kind` `name` of `from`, put in `to` as it is there: whether it was one to put.
fn copy_definition(to: &mut Model, from: &Model, kind: &Kind, name: &str) -> bool {
    fn one<T: Clone>(to: &mut BTreeMap<String, T>, from: Option<&BTreeMap<String, T>>, name: &str) -> bool {
        from.and_then(|f| f.get(name)).map(|v| to.insert(name.to_string(), v.clone())).is_some()
    }
    let (t, f) = (&mut to.types, &from.types);
    match kind {
        Kind::Type(c) => match c.as_str() {
            "wall" => one(&mut t.wall, Some(&f.wall), name),
            "floor" => one(&mut t.floor, Some(&f.floor), name),
            "ceiling" => one(&mut t.ceiling, Some(&f.ceiling), name),
            "roof" => one(&mut t.roof, Some(&f.roof), name),
            "door" => one(&mut t.door, Some(&f.door), name),
            "window" => one(&mut t.window, Some(&f.window), name),
            "column" => one(&mut t.column, Some(&f.column), name),
            "structuralcolumn" => one(&mut t.structural_column, Some(&f.structural_column), name),
            "footing" => one(&mut t.footing, Some(&f.footing), name),
            "railing" => one(&mut t.railing, Some(&f.railing), name),
            run => one(t.runs.entry(run.to_string()).or_default(), f.runs.get(run), name),
        },
        Kind::Material => one(&mut to.materials, Some(&from.materials), name),
        Kind::Picture => one(&mut to.images, Some(&from.images), name),
        Kind::Cloud => one(&mut to.clouds, Some(&from.clouds), name),
        Kind::Cad => one(&mut to.cads, Some(&from.cads), name),
        Kind::Image => one(&mut to.image_types, Some(&from.image_types), name),
        Kind::Family => one(&mut to.families, Some(&from.families), name),
        Kind::Parameter => one(&mut to.parameters, Some(&from.parameters), name),
        Kind::LinePattern => one(&mut to.line_patterns, Some(&from.line_patterns), name),
        Kind::LineStyle => one(&mut to.line_styles, Some(&from.line_styles), name),
        Kind::FillPattern => one(&mut to.fill_patterns, Some(&from.fill_patterns), name),
        Kind::FilledRegionType => one(&mut to.region_types, Some(&from.region_types), name),
        Kind::DetailGroup => one(&mut to.detail_groups, Some(&from.detail_groups), name),
        Kind::TextType => one(&mut to.text_types, Some(&from.text_types), name),
        Kind::ReinforcementPreset => one(&mut to.reinforcement, Some(&from.reinforcement), name),
        Kind::DimensionType => one(&mut to.dimension_types, Some(&from.dimension_types), name),
        Kind::Voltage => one(&mut to.voltages, Some(&from.voltages), name),
        Kind::DistributionSystem => one(&mut to.distribution_systems, Some(&from.distribution_systems), name),
        Kind::WireType => one(&mut to.wire_types, Some(&from.wire_types), name),
        Kind::TitleBlock => one(&mut to.title_blocks, Some(&from.title_blocks), name),
        _ => false,
    }
}

/// The phases and the design option `e` holds that this project hasn't, taken off it: it is made in the first phase, in no
/// option, unless this project has them. Why, for each.
fn phasing_here(to: &Model, e: &mut Element) -> Vec<String> {
    let Some(params) = e.params_mut() else { return Vec::new() };
    let mut why = Vec::new();
    for key in [phasing::CREATED, phasing::DEMOLISHED] {
        if let Some(p) = params.get(key).and_then(|v| v.as_str()).filter(|p| !to.phases.iter().any(|q| q == p)).map(String::from) {
            params.remove(key);
            why.push(format!("`{key}` {p} isn't a phase of this project"));
        }
    }
    if let Some(o) = params.get(phasing::OPTION).and_then(|v| v.as_str()).filter(|o| !phasing::option_exists(to, o)).map(String::from) {
        params.remove(phasing::OPTION);
        why.push(format!("`{}` {o} isn't a design option of this project", phasing::OPTION));
    }
    why
}

/// The names and numbers that must differ, taken: a level's or a view's name, a grid's, a sheet's number.
struct Names {
    views: BTreeSet<String>,
    grids: BTreeSet<String>,
    sheets: BTreeSet<String>,
}

impl Names {
    fn of(m: &Model) -> Names {
        let mut n = Names { views: BTreeSet::new(), grids: BTreeSet::new(), sheets: BTreeSet::new() };
        for (id, e) in &m.elements {
            match e {
                Element::Grid(g) => n.grids.insert(g.name.clone()),
                Element::Sheet(sh) => n.sheets.insert(sh.number.clone()),
                _ => m.view_name(*id).is_some_and(|v| n.views.insert(v.to_string())),
            };
        }
        // The standard schedules are views before they're elements.
        n.views.extend(crate::schedule::VIEWS.iter().map(|(_, name)| m.language.word(name).to_string()));
        n
    }

    /// `e` named or numbered apart from what's taken (a ` 2`, ` 3`… after a name that is), and its name taken.
    fn free(&mut self, e: &mut Element) {
        let (name, taken) = match e {
            Element::Level(l) => (&mut l.name, &mut self.views),
            Element::Section(v) | Element::Elevation(v) => (&mut v.name, &mut self.views),
            Element::View3D(v) => (&mut v.name, &mut self.views),
            Element::Schedule(sc) => (&mut sc.name, &mut self.views),
            Element::Grid(g) => (&mut g.name, &mut self.grids),
            Element::Sheet(sh) => (&mut sh.number, &mut self.sheets),
            _ => return,
        };
        if taken.contains(name.as_str()) {
            let base = name.clone();
            *name = (2..).map(|n| format!("{base} {n}")).find(|n| !taken.contains(n)).unwrap_or_default();
        }
        taken.insert(name.clone());
    }
}

/// Each view's overrides of elements the project hasn't, dropped: what an inserted view overrode that wasn't brought in.
fn tidy_graphics(m: &mut Model) {
    let known: BTreeSet<Guid> = m.elements.with_guids().map(|(_, g, _)| g).collect();
    for g in m.elements.values_mut().filter_map(graphics::graphics_mut) {
        g.elements.retain(|k, _| known.contains(k));
        g.linework.retain(|l| known.contains(&l.element));
    }
}

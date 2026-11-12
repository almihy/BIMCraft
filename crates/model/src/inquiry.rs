//! Revit's Inquiry, and more: elements found by what names them (Select by ID), and what names the elements selected (IDs of
//! Selection). An element is named by its id, its GUID (with or without its dashes and braces, in either case) or its IFC
//! GlobalId (its GUID in 22 characters), any number of them at once, as pasted from a schedule, a clash report or an IFC
//! viewer: separated by spaces, commas, semicolons or lines.

use std::collections::BTreeSet;

use crate::geom::P2;
use crate::model::{Element, Guid, Id, Model};

/// The most a list may say, in characters, and the most names it may hold.
pub const MAX_LIST: usize = 1 << 20;
pub const MAX_NAMES: usize = 10_000;

/// What a list found: the elements, each once, in the order named; the library's entries (types, materials…), which are
/// no elements to select; and what named none.
#[derive(Debug, Default, PartialEq)]
pub struct Found {
    pub ids: Vec<Id>,
    pub library: Vec<Id>,
    pub missing: Vec<String>,
}

/// The elements `text` names; too long a list is cut at [`MAX_LIST`] characters and [`MAX_NAMES`] names.
pub fn find(m: &Model, text: &str) -> Found {
    let end = (0..=MAX_LIST.min(text.len())).rev().find(|i| text.is_char_boundary(*i)).unwrap_or(0);
    let text = text.get(..end).unwrap_or_default();
    let mut out = Found::default();
    let mut seen = BTreeSet::new();
    for word in text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';')).filter(|w| !w.is_empty()).take(MAX_NAMES) {
        match (one(m, word), library(m, word)) {
            (Some(id), _) if seen.insert(id) => out.ids.push(id),
            (None, Some(id)) if seen.insert(id) => out.library.push(id),
            (Some(_), _) | (None, Some(_)) => {}
            (None, None) => out.missing.push(word.to_string()),
        }
    }
    out
}

/// The element `word` names: an id, a GUID, or an IFC GlobalId.
pub fn one(m: &Model, word: &str) -> Option<Id> {
    m.elements.id(guid_of(word)?)
}

/// The library entry or part `word` names: its id, its GUID, or that GUID as an IFC GlobalId.
pub fn library(m: &Model, word: &str) -> Option<Id> {
    let g = guid_of(word)?;
    match g.pending() {
        Some(id) => m.library.taken(id).then_some(id),
        None => m.library.by_guid(g).map(|(id, _)| id).or_else(|| m.library.part_by_guid(g).map(|(id, _)| id)),
    }
}

/// The GUID `word` is, however written, or the id it is (a pending GUID).
fn guid_of(word: &str) -> Option<Guid> {
    let word = word.trim().trim_matches(|c| matches!(c, '"' | '\''));
    Guid::parse(word)
        .or_else(|| {
            // A GUID without its dashes: 32 hex digits.
            (word.len() == 32 && word.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| u128::from_str_radix(word, 16).ok())
                .flatten()
                .map(Guid::from_u128)
        })
        .or_else(|| crate::ifc::expand(word).map(Guid::from_u128))
}

/// What names element `id`: its id, its GUID, its IFC GlobalId; and what it is, its category and what it is called.
#[derive(Debug, Clone, PartialEq)]
pub struct Names {
    pub id: Id,
    pub guid: Guid,
    pub ifc: String,
    /// The element's category, or what the library entry is (`material`, `wall type`).
    pub category: String,
    pub name: String,
}

pub fn names(m: &Model, id: Id) -> Option<Names> {
    if let Some(p) = m.library.part(id) {
        let ifc = crate::ifc::compress(p.guid.as_u128());
        return Some(Names { id, guid: p.guid, ifc, category: p.what.to_string(), name: p.name() });
    }
    if let Some(e) = m.library.get(id) {
        let ifc = crate::ifc::compress(e.guid.as_u128());
        return Some(Names { id, guid: e.guid, ifc, category: e.kind.label(), name: e.name.clone() });
    }
    let e = m.elements.get(id)?;
    let guid = m.elements.guid(id)?;
    // Its name, type, number or mark, and none of its rooms.
    let j = crate::json::element_json_bare(m, id);
    let name = ["name", "type", "number", "mark", "text"].iter().find_map(|k| j[*k].as_str().filter(|s| !s.is_empty())).unwrap_or_default();
    Some(Names { id, guid, ifc: crate::ifc::compress(guid.as_u128()), category: e.category().to_string(), name: name.to_string() })
}

/// The plan to show elements `ids` in, and the rectangle they take in it, south-west corner then north-east: the plan of the
/// level the first standing on one stands on (a level's own; grids and the rest, the lowest), where they are drawn there.
/// None when the plan draws none of them (sheets, schedules, what its view hides).
pub fn shown(m: &Model, ids: &[Id]) -> Option<(Id, [P2; 2])> {
    let on = |id: &Id| match m.elements.get(*id)? {
        Element::Level(_) => Some(*id),
        e => m.level_of(e).and_then(|l| m.elements.id(l)),
    };
    let level = ids.iter().find_map(on).or_else(|| m.levels().first().map(|(id, _)| *id))?;
    let d = crate::render::plan(m, level, 2000, 2000);
    let wanted: BTreeSet<Id> = ids.iter().copied().collect();
    let pts: Vec<P2> = d
        .prims
        .iter()
        .zip(&d.owners)
        .filter(|(_, o)| o.is_some_and(|o| wanted.contains(&o)))
        .flat_map(|(p, _)| p.points())
        .map(|q| d.view.mm(q))
        .collect();
    let first = *pts.first()?;
    let (lo, hi) = pts.iter().fold((first, first), |(lo, hi), q| (P2(lo.0.min(q.0), lo.1.min(q.1)), P2(hi.0.max(q.0), hi.1.max(q.1))));
    Some((level, [lo, hi]))
}

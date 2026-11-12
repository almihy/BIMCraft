//! Linked models, as Revit's Link Revit: another project's walls, floors, roofs, columns, stairs and the rest of what it
//! builds, drawn in this one's views and taken for a reference. Nothing of it is the project's: the file holds a [`Link`]
//! (a name, a path, where it stands and how far it is turned), a command can't select or edit what it shows, and the
//! schedules, the IFC and the checks never see it.
//!
//! The views draw [`seen`], this project with its links brought in under ids from [`LINKED`] up, so plans, sections, the 3D
//! view and snapping all find a link's elements the way they find the project's own. Each link's level at an elevation this
//! project has a level at stands on that level; the others come in as levels of their own, named after the link.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::geom::P2;
use crate::ids::{Id, LINKED};
use crate::model::{Element, Model};
use crate::motion::{Motion, transformed};

/// The start of the `path` of a link made from a project's text (`link {text}`): `text:` and its name. No file is read for
/// it; a front end that keeps the project elsewhere (Arqiflow's site) links it again from its text.
pub const FROM_TEXT: &str = "text:";

/// How close two levels' elevations must be, mm, for a link's level to stand on this project's.
const SAME_LEVEL: f64 = 1.0;

/// Whether `id` is an element of a link: the views' [`seen`] model has them, the project never does.
pub fn is_linked(id: Id) -> bool {
    id >= LINKED
}

/// The owner of the fills that union what a link builds in a plan: no element has it, a pick never answers it, and the
/// drawing puts it under the project's with the rest of the link.
pub const UNION: Id = Id::MAX;

/// What of a linked project is drawn: what is built. Its views, sheets, schedules and annotation stay in it, and so do its
/// own links.
fn built(e: &Element) -> bool {
    !matches!(
        e,
        Element::Grid(_)
            | Element::Room(_)
            | Element::Dimension(_)
            | Element::Tag(_)
            | Element::Section(_)
            | Element::Elevation(_)
            | Element::Sheet(_)
            | Element::Viewport(_)
            | Element::Text(_)
            | Element::DetailLine(_)
            | Element::FilledRegion(_)
            | Element::DetailGroup(_)
            | Element::RepeatingDetail(_)
            | Element::Spot(_)
            | Element::Schedule(_)
            | Element::View3D(_)
            | Element::PlanView(_)
            | Element::Link(_)
    )
}

/// The model the views draw: `m`, with what each of its loaded links builds. The model itself where it links nothing.
/// A link that can't be brought in is left out.
pub fn seen(m: &Arc<Model>) -> Arc<Model> {
    let links: Vec<(&crate::model::Link, &Arc<Model>)> = m
        .elements
        .values()
        .filter_map(|e| match e {
            Element::Link(l) => m.loaded.get(&l.path).map(|other| (l, other)),
            _ => None,
        })
        .collect();
    if links.is_empty() {
        return Arc::clone(m);
    }
    let mut out = (**m).clone();
    for (l, other) in links {
        // ponytail: a link that fails (a file whose parts clash with this project's) is drawn as not there; the status of
        // `reload` says which links are loaded.
        let _ = bring(&mut out, l, other);
    }
    Arc::new(out)
}

/// What link `l` builds, in `to`: turned and moved as the link says, on `to`'s levels where they stand at the same height.
fn bring(to: &mut Model, l: &crate::model::Link, from: &Model) -> Result<(), String> {
    let mut other = from.clone();
    other.elements.retain(|_, e| matches!(e, Element::Level(_)) || built(e));
    let all: BTreeSet<Id> = other.elements.keys().copied().collect();
    // Turned about its origin, then moved to where it stands.
    let (sin, cos) = l.angle.to_radians().sin_cos();
    let map = |p: P2| P2(p.0 * cos - p.1 * sin + l.at.0, p.0 * sin + p.1 * cos + l.at.1);
    let motion = Motion { map: &map, slide: None, mirror: false };
    for id in &all {
        if matches!(other.elements.get(*id), Some(Element::Level(_))) {
            continue;
        }
        let e = transformed("link", &other, *id, &motion, &all)?;
        other.elements.insert(*id, e);
    }
    // A level that stands where one of this project's does is that one; another is the link's own.
    let mut matched = BTreeSet::new();
    for id in &all {
        let Some(Element::Level(lv)) = other.elements.get_mut(*id) else { continue };
        let here = to.levels().into_iter().find(|(_, t)| (t.elevation - lv.elevation).abs() < SAME_LEVEL);
        match here {
            Some((_, t)) => {
                lv.name = t.name.clone();
                matched.insert(*id);
            }
            None => lv.name = format!("{}: {}", l.name, lv.name),
        }
    }
    let ask: Vec<Id> = all.difference(&matched).copied().collect();
    if ask.is_empty() {
        return Ok(());
    }
    crate::insert::insert_as(to, &other, &ask, true).map(|_| ())
}

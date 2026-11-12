//! Revit's Filter, on a selection, and more: what is selected is counted by category, each to keep or leave out; and, by
//! any parameter the selected elements all have (a wall's Type, Base Constraint, Length; a door's Mark, Level), by the
//! values they hold. The parameters are the schedules' fields (`schedule::fields`): those every selected element's
//! category has, Category always first.

use std::collections::BTreeMap;

use crate::model::{Id, Model};
use crate::schedule::{self, Kind};

/// What every element has: its category. As a value, the category's key (`wall`, `door`, `detail_line`).
pub const CATEGORY: &str = "Category";

/// A value the selection holds, and the elements that hold it, in the order selected. An element with no value for the
/// parameter holds `""`.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub value: String,
    pub ids: Vec<Id>,
}

/// The parameters every element of `ids` has, with what each holds: Category first, then the fields their categories share,
/// in the first category's order. A count of one each is no parameter to filter by, so Count is left out.
pub fn common(m: &Model, ids: &[Id]) -> Vec<(String, Kind)> {
    let mut cats: Vec<&str> = ids.iter().filter_map(|id| m.elements.get(id)).map(|e| e.category()).collect();
    cats.sort_unstable();
    cats.dedup();
    let mut out = vec![(CATEGORY.to_string(), Kind::Text)];
    let Some((first, rest)) = cats.split_first() else { return out };
    let others: Vec<Vec<(String, Kind)>> = rest.iter().map(|c| schedule::fields(m, c)).collect();
    out.extend(
        schedule::fields(m, first)
            .into_iter()
            .filter(|(n, _)| n != "Count" && n != CATEGORY)
            .filter(|f| others.iter().all(|o| o.contains(f))),
    );
    out
}

/// The values `by`, a parameter of every element of `ids`, takes, each with the elements holding it: in natural order
/// (numbers by value), the elements with none last. Category's values are categories' keys; the others, values as a
/// schedule shows them, in the project's units.
pub fn groups(m: &Model, ids: &[Id], by: &str) -> Vec<Group> {
    let mut found: BTreeMap<String, Vec<Id>> = BTreeMap::new();
    let rooms = crate::rooms::Rooms::new(m);
    for &id in ids {
        let Some(e) = m.elements.get(id) else { continue };
        let value = match by {
            CATEGORY => e.category().to_string(),
            _ => schedule::field_text_with(m, id, by, &rooms).unwrap_or_default(),
        };
        let held = found.entry(value).or_default();
        if !held.contains(&id) {
            held.push(id);
        }
    }
    let mut out: Vec<Group> = found.into_iter().map(|(value, ids)| Group { value, ids }).collect();
    out.sort_by(|a, b| a.value.is_empty().cmp(&b.value.is_empty()).then_with(|| schedule::natural_text(&a.value, &b.value)));
    out
}

/// What of `ids` holds one of the values `keep` of `by`: the selection the filter leaves, in the order selected.
pub fn kept(m: &Model, ids: &[Id], by: &str, keep: &[String]) -> Vec<Id> {
    let held: Vec<Id> = groups(m, ids, by).into_iter().filter(|g| keep.contains(&g.value)).flat_map(|g| g.ids).collect();
    let mut out: Vec<Id> = Vec::new();
    for id in ids {
        if held.contains(id) && !out.contains(id) {
            out.push(*id);
        }
    }
    out
}

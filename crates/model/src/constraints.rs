//! Locked and equal dimensions kept, as Revit keeps them: after each command, what a constrained dimension measures to
//! moves along the way it measures so that its locked segments keep their lengths and an equal one's segments stay
//! equal. Each dimension is solved on its own line: its stations (where its references cross it) are positions; a locked
//! segment ties two together, and equality ties every segment to one length. What the command moved stays where it put
//! it, and the rest follows; an equal string with nothing locked keeps its two ends, its inner references spread evenly.
//! An element is moved as the `move` command moves it (joined walls stretch, doors slide along their walls), and a
//! dimension solved may move what another measures, so they are solved again, a few times, until all hold. Where they
//! can't, the command is refused and the project is as it was.

use std::collections::BTreeMap;

use crate::ids::{Guid, Key};
use crate::model::{DimKind, Dimension, Element, Id, Model};

/// How far a station may be from where its constraint puts it, mm.
pub const SLACK: f64 = 0.01;
/// How many times the dimensions are solved, one after another, before what still doesn't hold is refused.
pub const PASSES: usize = 8;

/// Whether dimension `d` holds constraints.
pub fn constrained(d: &Dimension) -> bool {
    matches!(d.kind, DimKind::Aligned | DimKind::Linear) && (d.equal || !d.locked.is_empty())
}

/// Where each station of `d` should be, given those of elements in `fixed` stay: none where it holds already. An error
/// when its constraints contradict each other, or what stays.
pub fn targets(m: &Model, d: &Dimension, fixed: &dyn Fn(Guid) -> bool) -> Result<Vec<(Guid, f64)>, String> {
    let (_, st) = m.stations(d);
    let k = st.len().saturating_sub(1);
    if k == 0 {
        return Ok(Vec::new());
    }
    let s: Vec<f64> = st.iter().map(|x| x.0).collect();
    // Each segment's length, where one is set: a lock's, or with equality, all of them one.
    let mut gap: Vec<Option<f64>> = vec![None; k];
    for l in d.locked.iter().filter(|l| l.segment < k) {
        if let Some(g) = gap.get_mut(l.segment) {
            *g = Some(l.length);
        }
    }
    let mut want = s.clone();
    if d.equal {
        let set: Vec<f64> = gap.iter().flatten().copied().collect();
        match set.first() {
            Some(&g) if set.iter().all(|x| (x - g).abs() <= SLACK) => gap = vec![Some(g); k],
            Some(_) => return Err("its locked segments differ in length, so they can't be equal".into()),
            // Nothing locked: its two ends stay as they are, and what lies between goes back to its even place.
            None => {
                let g = (s[k] - s[0]) / k as f64;
                for (t, w) in want.iter_mut().enumerate() {
                    *w = s[0] + g * t as f64;
                }
                return Ok(st
                    .iter()
                    .zip(want)
                    .filter(|((at, _, _), w)| (w - at).abs() > SLACK)
                    .map(|((at, _, r), w)| (r.id, w - at))
                    .collect());
            }
        }
    }
    // Stations tied by set lengths make rigid runs; each run stays where a station of it that is fixed is, else where
    // its first is.
    let mut i = 0;
    while i <= k {
        let mut j = i;
        while j < k && gap.get(j).copied().flatten().is_some() {
            j += 1;
        }
        // Stations i..=j, each from the run's first by the set lengths.
        let mut offset = vec![0.0; j - i + 1];
        for t in i..j {
            offset[t - i + 1] = offset[t - i] + gap.get(t).copied().flatten().unwrap_or(0.0);
        }
        let anchors: Vec<f64> = (i..=j).filter(|&t| fixed(st[t].2.id)).map(|t| s[t] - offset[t - i]).collect();
        let start = match anchors.first() {
            Some(&a) if anchors.iter().all(|x| (x - a).abs() <= SLACK) => a,
            Some(_) => return Err("what the command moved pulls its locked segments apart".into()),
            None => s[i],
        };
        for t in i..=j {
            want[t] = start + offset[t - i];
        }
        i = j + 1;
    }
    Ok(st.iter().zip(want).filter(|((at, _, _), w)| (w - at).abs() > SLACK).map(|((at, _, r), w)| (r.id, w - at)).collect())
}

/// The elements whose stations move how far, each once: an error when two of one element's would move it apart.
pub fn moves(m: &Model, d: &Dimension, fixed: &dyn Fn(Guid) -> bool) -> Result<BTreeMap<Guid, f64>, String> {
    let mut by: BTreeMap<Guid, f64> = BTreeMap::new();
    for (id, delta) in targets(m, d, fixed)? {
        match by.get(&id) {
            Some(was) if (was - delta).abs() > SLACK => {
                return Err(format!(
                    "{} {} would have to move two ways",
                    m.elements.get(id).map_or("element", |e| e.category()),
                    m.shown(id)
                ));
            }
            _ => {
                by.insert(id, delta);
            }
        }
    }
    Ok(by)
}

impl Model {
    /// The locked or equal dimensions that hold element `k`, by id: a dimension kept equal that measures to it, or one with
    /// a locked segment that ends at it.
    pub fn held_by(&self, k: impl Key) -> Vec<Id> {
        let Some(g) = self.elements.guid(k) else { return Vec::new() };
        let holds = |d: &Dimension| {
            let at = |i: usize| d.equal || d.locked.iter().any(|l| l.segment == i || l.segment + 1 == i);
            self.stations(d).1.iter().enumerate().any(|(i, (_, _, r))| r.id == g && at(i))
        };
        // A dimension that measures to it points at it: those are its referrers, found in the lookup, in the order of their ids.
        let lookup = self.lookup();
        let held: Vec<Id> = lookup
            .referrers(&g)
            .filter(|id| matches!(self.elements.get(*id), Some(Element::Dimension(d)) if constrained(d) && holds(d)))
            .collect();
        if crate::selfcheck::on(crate::selfcheck::Check::HeldBy) {
            assert_eq!(
                held,
                self.elements
                    .iter()
                    .filter(|(_, e)| matches!(e, Element::Dimension(d) if constrained(d) && holds(d)))
                    .map(|(id, _)| *id)
                    .collect::<Vec<_>>(),
                "the dimensions holding an element, from the lookup, aren't every one that does"
            );
        }
        held
    }
}

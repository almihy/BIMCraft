//! Copy to Clipboard and Paste, as Revit's Ctrl+C and Ctrl+V (docs/areas/desktop.md, "Copy and Paste").
//!
//! The clipboard is the session's: the GUIDs of what was copied (the walls with the doors and windows in them, a stair with its
//! railings), read from the model when pasting, so what was changed since is pasted as it is now and what was deleted is left out.
//! A paste puts new elements beside the old (`by`), on them (`same_place`), or on levels (`levels`, `view`: Revit's Paste Aligned
//! to Selected Levels and to Current View): each at the same place in plan, its heights moved by the levels' difference and what
//! it measures from a level measured from the level there. What cannot go the way it was is pasted as near as can be, and the
//! reply says what it did (`notes`) and what it left out (`left_out`).

use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{
    Motion, R, absent, along, bad, duplicate_mapped, flag, ids_param, level_param, micron, next_number, off_centre, only, point_req,
};
use crate::engine::Session;
use crate::geom::P2;
use crate::model::{Element, Guid, Id, MAX_POINTS, Model, TOL, Wall, run_arm};
use crate::walls::Axis;

/// What was copied: the GUIDs of its elements, in the order they were copied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clipboard {
    guids: Vec<Guid>,
}

impl Clipboard {
    /// The elements it holds that are still in `m`, by id.
    pub fn ids(&self, m: &Model) -> Vec<Id> {
        self.guids.iter().filter_map(|g| m.elements.id(*g)).collect()
    }

    /// The box round its elements in plan: south-west and north-east corners. None when none of them has a place in plan.
    pub fn extent(&self, m: &Model) -> Option<[P2; 2]> {
        let pts: Vec<P2> = self.ids(m).into_iter().filter_map(|id| m.elements.get(id).map(|e| plan_points(m, id, e))).flatten().collect();
        let (lo, hi) = pts.iter().fold((P2(f64::MAX, f64::MAX), P2(f64::MIN, f64::MIN)), |(lo, hi), p| {
            (P2(lo.0.min(p.0), lo.1.min(p.1)), P2(hi.0.max(p.0), hi.1.max(p.1)))
        });
        (!pts.is_empty()).then_some([lo, hi])
    }

    /// Where its middle is in plan: the middle of its box, which a paste is placed by.
    pub fn anchor(&self, m: &Model) -> Option<P2> {
        self.extent(m).map(|[lo, hi]| micron((lo + hi) * 0.5))
    }

    /// The level it was copied from: the lowest the elements stand on.
    pub fn level(&self, m: &Model) -> Option<Guid> {
        let levels: Vec<(Guid, f64)> = self
            .ids(m)
            .into_iter()
            .filter_map(|id| m.elements.get(id).and_then(|e| m.level_of(e)))
            .filter_map(|g| m.level(g).map(|l| (g, l.elevation)))
            .collect();
        levels.into_iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(g, _)| g)
    }
}

impl Session {
    /// The ids of what the clipboard holds that is still in the project, as the Paste preview draws them.
    pub fn clipboard_ids(&self) -> Vec<Id> {
        self.clipboard.as_ref().map(|c| c.ids(&self.model)).unwrap_or_default()
    }

    /// Where the middle of what the clipboard holds is in plan, which a paste is placed by.
    pub fn clipboard_anchor(&self) -> Option<P2> {
        self.clipboard.as_ref().and_then(|c| c.anchor(&self.model))
    }

    /// The box round what the clipboard holds in plan: south-west and north-east corners.
    pub fn clipboard_extent(&self) -> Option<[P2; 2]> {
        self.clipboard.as_ref().and_then(|c| c.extent(&self.model))
    }

    /// The level what the clipboard holds was copied from: the lowest it stands on.
    pub fn clipboard_level(&self) -> Option<Guid> {
        self.clipboard.as_ref().and_then(|c| c.level(&self.model))
    }
}

/// Points that say where `e` is in plan: its ends, its corners, its place. The box round them is where it is.
fn plan_points(m: &Model, id: Id, e: &Element) -> Vec<P2> {
    match e {
        Element::Wall(w) => vec![w.start, w.end],
        Element::Grid(g) => vec![g.start, g.end],
        Element::RoomSeparator(r) => vec![r.start, r.end],
        run_arm!(r) => vec![r.start, r.end],
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => vec![c.at],
        Element::Component(c) => vec![c.at],
        Element::Room(r) => vec![r.point],
        Element::Stair(s) => vec![s.start],
        Element::Ramp(r) => vec![r.start],
        Element::Floor(f) | Element::Toposolid(f) => f.boundary.clone(),
        Element::Ceiling(c) => c.boundary.clone(),
        Element::Roof(r) => r.boundary.clone(),
        Element::Shaft(s) => s.boundary.clone(),
        Element::Mass(ms) => ms.boundary.clone(),
        Element::Door(_) | Element::Window(_) => m.anchor(id).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// Whether a paste can make a copy of `e`: levels, sheets, views and schedules are made, not copied.
fn pasteable(e: &Element) -> bool {
    !matches!(
        e,
        Element::Level(_)
            | Element::Sheet(_)
            | Element::Viewport(_)
            | Element::Schedule(_)
            | Element::PlanView(_)
            | Element::View3D(_)
            | Element::Dimension3D(_)
    )
}

/// What belongs to the view it is drawn in, not to a level: a paste to levels leaves it out.
fn annotation(e: &Element) -> bool {
    matches!(
        e,
        Element::Text(_)
            | Element::Spot(_)
            | Element::DetailLine(_)
            | Element::FilledRegion(_)
            | Element::DetailGroup(_)
            | Element::RepeatingDetail(_)
            | Element::Dimension(_)
            | Element::Tag(_)
    )
}

/// `what` as a sentence names it: "wall 100002".
fn named(m: &Model, id: Id) -> String {
    format!("{} {id}", m.elements.get(id).map_or("element", Element::category))
}

/// `ids` and what goes with them (the doors and windows in walls among them, the railings of stairs and ramps among them),
/// each once, and the elements that are gone.
fn with_hosted(m: &Model, ids: &[Id]) -> Vec<Id> {
    let mut seen: BTreeSet<Id> = BTreeSet::new();
    let mut out: Vec<Id> = ids.iter().copied().filter(|id| seen.insert(*id)).collect();
    let guids: BTreeSet<Guid> = out.iter().map(|id| m.guid(*id)).collect();
    for (&id, e) in m.elements.iter() {
        let goes = match e {
            Element::Door(o) | Element::Window(o) => guids.contains(&o.host),
            Element::Railing(r) => r.host.is_some_and(|h| guids.contains(&h)),
            _ => false,
        };
        if goes && seen.insert(id) {
            out.push(id);
        }
    }
    out
}

/// What a paste can make of `ids`, which of them it does, and for each it leaves out why. `to_levels`: the paste is aligned to
/// levels, which leaves out what belongs to a view (the annotation); what stands on no level (a grid, a section) is pasted
/// once, beside, by `by`.
fn sort_out(m: &Model, ids: &[Id], to_levels: bool) -> (Vec<Id>, Vec<String>) {
    let all = with_hosted(m, ids);
    let guids: BTreeSet<Guid> = all.iter().map(|id| m.guid(*id)).collect();
    let (mut keep, mut left_out) = (Vec::new(), Vec::new());
    for id in all {
        let Some(e) = m.elements.get(id) else { continue };
        let why = match e {
            _ if !pasteable(e) => Some(format!("{} is made, not copied", e.category())),
            _ if to_levels && annotation(e) => Some("it belongs to its view, not to a level".to_string()),
            Element::Railing(r) if r.host.is_some_and(|h| !guids.contains(&h)) => {
                Some("it goes only with its stair or ramp; copy that too".to_string())
            }
            Element::Rebar(r) if r.host.is_some_and(|h| !guids.contains(&h)) => {
                Some("it goes only with the element it reinforces; copy that too".to_string())
            }
            _ => None,
        };
        match why {
            Some(why) => left_out.push(format!("{}: {why}", named(m, id))),
            None => keep.push(id),
        }
    }
    (keep, left_out)
}

/// Revit's Copy to Clipboard (Ctrl+C): the elements, and the doors and windows in the walls among them, are put on the
/// session's clipboard, which stays until another copy or a new project.
pub(super) fn copy_to_clipboard(s: &mut Session, p: &Value) -> R {
    let c = "copy_to_clipboard";
    only(c, p, &["ids", "id"])?;
    let ids = ids_param(c, &s.model, p)?;
    let (keep, left_out) = sort_out(&s.model, &ids, false);
    if keep.is_empty() {
        return Err(bad(c, format!("nothing to copy: {}", left_out.join("; "))));
    }
    let clip = Clipboard { guids: keep.iter().map(|id| s.model.guid(*id)).collect() };
    let anchor = clip.anchor(&s.model);
    s.clipboard = Some(clip);
    Ok(json!({"ids": keep, "count": keep.len(), "anchor": anchor, "left_out": left_out}))
}

/// What a paste is given to place by: a clipboard's elements, or `ids`.
fn source(c: &str, s: &Session, p: &Value) -> Result<(Vec<Id>, Vec<String>), String> {
    if !absent(p.get("ids")) || !absent(p.get("id")) {
        return Ok((ids_param(c, &s.model, p)?, Vec::new()));
    }
    let Some(clip) = &s.clipboard else {
        return Err(bad(c, "nothing to paste: copy elements first (copy_to_clipboard), or give `ids`"));
    };
    let gone = clip.guids.len() - clip.ids(&s.model).len();
    let left_out = if gone > 0 { vec![format!("{gone} of what was copied is no longer in the project")] } else { Vec::new() };
    let ids = clip.ids(&s.model);
    if ids.is_empty() {
        return Err(bad(c, "everything that was copied is gone from the project; copy again"));
    }
    Ok((ids, left_out))
}

/// Revit's Paste (Ctrl+V) and its Aligned variants.
pub(super) fn paste(s: &mut Session, p: &Value) -> R {
    let c = "paste";
    only(c, p, &["ids", "id", "by", "levels", "view", "same_place"])?;
    let to_levels = !absent(p.get("levels")) || !absent(p.get("view"));
    let (same, by_given) = (flag(c, p, "same_place")?, !absent(p.get("by")));
    if !same && !by_given && !to_levels {
        return Err(bad(c, "say where to paste: `by` (beside), `levels` or `view` (aligned to levels), or `same_place`"));
    }
    if usize::from(same) + usize::from(by_given && !to_levels) + usize::from(to_levels) > 1
        || (!absent(p.get("levels")) && !absent(p.get("view")))
    {
        return Err(bad(c, "give one way to place it: `by`, `levels` or `view` (which may go with a `by`), or `same_place`"));
    }
    if same && by_given {
        return Err(bad(c, "`same_place` is on top of the originals; `by` moves them from there: give one"));
    }
    let (wanted, mut left_out) = source(c, s, p)?;
    let by = if by_given { point_req(c, p, "by")? } else { P2(0.0, 0.0) };
    let (ids, more) = sort_out(&s.model, &wanted, to_levels);
    left_out.extend(more);
    if ids.is_empty() {
        return Err(bad(c, format!("nothing to paste: {}", left_out.join("; "))));
    }
    // A door or window copied without its wall goes in the wall it lands on, as Revit's.
    let loose = loose_openings(&s.model, &ids);
    let rest: Vec<Id> = ids.iter().copied().filter(|id| !loose.contains(id)).collect();
    if to_levels {
        let targets = target_levels(c, &s.model, p)?;
        // What stands on no level (grids, sections, links) has none to be moved to: it is pasted once, by `by`, as Revit's
        // paste to another level also puts down the datum elements it was given.
        let (mut levelless, on_levels): (Vec<Id>, Vec<Id>) =
            rest.iter().partition(|id| s.model.elements.get(**id).is_some_and(|e| s.model.level_of(e).is_none()));
        // With no `by` a copy would lie on its original: it is left out, and the reply says so.
        if !by_given {
            left_out.extend(
                levelless.drain(..).map(|id| format!("{}: it stands on no level; give `by` to paste it beside", named(&s.model, id))),
            );
        }
        return paste_to_levels(c, s, &on_levels, &levelless, &loose, &targets, by, left_out);
    }
    let mo = Motion { map: &|q| micron(q + by), slide: Some(by), mirror: false };
    let mut made = if rest.is_empty() { Vec::new() } else { duplicate_mapped(c, s, &rest, &mo)?.0 };
    let (placed, skipped) = place_openings(s, &loose, by, None);
    made.extend(placed);
    left_out.extend(skipped);
    if made.is_empty() {
        return Err(bad(c, format!("nothing to paste: {}", left_out.join("; "))));
    }
    Ok(json!({"ids": made, "pasted": [{"ids": made}], "notes": [], "left_out": left_out}))
}

/// The doors and windows among `ids` whose wall is not among them.
fn loose_openings(m: &Model, ids: &[Id]) -> Vec<Id> {
    let guids: BTreeSet<Guid> = ids.iter().map(|id| m.guid(*id)).collect();
    ids.iter()
        .copied()
        .filter(|id| matches!(m.elements.get(*id), Some(Element::Door(o) | Element::Window(o)) if !guids.contains(&o.host)))
        .collect()
}

/// A copy of each of `loose`, doors and windows without their wall, in the wall that is where it falls (`by` from where it
/// was) on `level`, or on the level of its old wall: Revit puts a pasted door in the wall it lands on. Each has its own mark;
/// one that falls on no wall is left out, and the reason is said.
fn place_openings(s: &mut Session, loose: &[Id], by: P2, level: Option<Guid>) -> (Vec<Id>, Vec<String>) {
    let (mut made, mut left_out) = (Vec::new(), Vec::new());
    for &id in loose {
        let m = &s.model;
        let label = named(m, id);
        let Some(e @ (Element::Door(o) | Element::Window(o))) = m.elements.get(id) else { continue };
        let Some(old) = m.wall(o.host) else {
            left_out.push(format!("{label}: its wall is no longer in the project"));
            continue;
        };
        let here = micron(Axis::of(old).at(o.offset, 0.0) + by);
        let width = m.opening_dims(e).map_or(0.0, |t| t.width);
        let Some((host, offset, sill)) = host_for_opening(m, level, old, o.sill, here, width) else {
            left_out.push(format!("{label}: no wall at [{}, {}] to hold it: a door or window goes only in a wall", here.0, here.1));
            continue;
        };
        let door = matches!(e, Element::Door(_));
        let mut copy = o.clone();
        (copy.host, copy.offset, copy.sill) = (host, offset, sill);
        copy.mark = next_number(m.elements.values().filter_map(|e| match e {
            Element::Door(o) if door => Some(o.mark.as_str()),
            Element::Window(o) if !door => Some(o.mark.as_str()),
            _ => None,
        }));
        let mut copy = if door { Element::Door(copy) } else { Element::Window(copy) };
        if let Some(params) = copy.params_mut() {
            crate::massing::detach(params);
        }
        made.push(s.model_mut().insert(copy));
    }
    (made, left_out)
}

/// How well a wall suits a pasted opening, the smaller the better: on another level than the plan's, how far its axis is, how far
/// the opening overhangs its run, and the axis distance again, finer.
type Rank = (bool, i64, i64, i64);

/// The wall a copy of a door or window of `old` (its sill `sill` above that wall's base) goes in when it falls at `at`, `width`
/// wide, in the plan of `level` (else of the old wall's level): how far along it, and its sill there. Of the walls the plan
/// cuts that `at` lies in, one on the plan's own level before one from a level below that runs up through it, then the one
/// whose axis is nearest (at a join, the one the opening fits in), the opening kept within its run where it can be. The sill is
/// measured from that wall's base, so the opening is as high above the level as it was above its own.
fn host_for_opening(m: &Model, level: Option<Guid>, old: &Wall, sill: f64, at: P2, width: f64) -> Option<(Guid, f64, f64)> {
    let plan = level.unwrap_or(old.level);
    let half = width.max(0.0) / 2.0;
    let mut best: Option<(Rank, Guid, &Wall, f64)> = None;
    for (_, id, e) in m.elements.with_guids() {
        let Element::Wall(w) = e else { continue };
        let d = off_centre(m, w, at);
        if !m.cut_by(w, plan) || d > m.thickness(w) / 2.0 + 100.0 {
            continue;
        }
        let (s, length) = (along(w, at), Axis::of(w).length());
        let overshoot = (half - s).max(0.0) + (s + half - length).max(0.0);
        let key = (w.level != plan, (d * 1000.0).round() as i64, (overshoot * 1000.0).round() as i64, (d * 1e6).round() as i64);
        if best.as_ref().is_none_or(|(k, ..)| key < *k) {
            best = Some((key, id, w, s));
        }
    }
    let (_, id, w, s) = best?;
    let length = Axis::of(w).length();
    let offset = if length >= width { s.clamp(half, length - half) } else { length / 2.0 };
    // Its bottom as high above the plan's level as it was above its old wall's own, whatever level this wall is based on.
    let was_above = m.own_base(old) + sill - m.elevation(old.level);
    let sill = (m.elevation(plan) + was_above - m.own_base(w)).max(0.0);
    Some((id, offset, sill))
}

/// The levels `levels` (each a level, a plan view or a name) or `view` name, as GUIDs, each once.
fn target_levels(c: &str, m: &Model, p: &Value) -> Result<Vec<Guid>, String> {
    let named: Vec<Value> = match (p.get("levels"), p.get("view")) {
        (Some(Value::Array(a)), _) if a.len() <= MAX_POINTS => a.clone(),
        (Some(Value::Array(_)), _) => return Err(bad(c, format!("`levels` lists more than {MAX_POINTS} levels"))),
        (Some(v), _) if !absent(Some(v)) => return Err(bad(c, "`levels` is a list of levels, by name, id or GUID")),
        (_, Some(v)) if !absent(Some(v)) => vec![v.clone()],
        _ => Vec::new(),
    };
    if named.is_empty() {
        return Err(bad(c, "`levels` names no level: say which levels to paste to"));
    }
    let mut out: Vec<Guid> = Vec::new();
    for v in named {
        let level = level_param(c, m, &json!({"level": v}), "level")?;
        if !out.contains(&level) {
            out.push(level);
        }
    }
    Ok(out)
}

/// Paste Aligned to Selected Levels: copies of `ids` for each of `targets`, at the same place in plan, and `loose` (doors and
/// windows without their wall) in the walls they land on there.
#[allow(clippy::too_many_arguments)]
fn paste_to_levels(
    c: &str,
    s: &mut Session,
    ids: &[Id],
    levelless: &[Id],
    loose: &[Id],
    targets: &[Guid],
    by: P2,
    mut left_out: Vec<String>,
) -> R {
    let table: Vec<(Guid, f64)> = s.model.levels().into_iter().map(|(id, l)| (s.model.guid(id), l.elevation)).collect();
    let elevation = |g: Guid| table.iter().find(|(l, _)| *l == g).map(|(_, z)| *z);
    // The level the copied elements stand on, or the lowest of them where they stand on several: what the paste is aligned from.
    let from = ids
        .iter()
        .chain(loose)
        .filter_map(|id| s.model.elements.get(*id).and_then(|e| s.model.level_of(e)))
        .filter_map(|g| elevation(g).map(|z| (g, z)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(g, _)| g);
    if from.is_none() && levelless.is_empty() {
        return Err(bad(c, "none of what was copied stands on a level"));
    }
    let from_z = from.and_then(elevation).unwrap_or_default();
    let (mut all, mut pasted, mut notes) = (Vec::new(), Vec::new(), Vec::new());
    for &target in targets {
        let Some(from) = from else { break };
        let Some(target_z) = elevation(target) else {
            notes.push(format!("level {} is not in the project, so nothing was pasted to it", s.model.shown(target)));
            continue;
        };
        let shift = Shift { table: &table, from, to: target, by: target_z - from_z };
        // What cannot stand at this height, for want of a level there and an offset to raise it by, is left out.
        let (fit, skipped) = shift.placeable(&s.model, ids);
        say_once(&mut left_out, skipped);
        let sideways = Motion { map: &|q| micron(q + by), slide: Some(by), mirror: false };
        let (mut made, copies) = if fit.is_empty() {
            (Vec::new(), BTreeSet::new())
        } else {
            let (made, mapping) = duplicate_mapped(c, s, &fit, &sideways)?;
            (made, mapping.values().copied().collect::<BTreeSet<Guid>>())
        };
        let m = s.model_mut();
        for &id in &made {
            let Some(e) = m.elements.get_mut(id) else { continue };
            let label = format!("{} {id}", e.category());
            shift.apply(e, &label, &copies, &mut notes);
        }
        // A room with no walls round it up there is placed all the same, unbounded.
        for &id in &made {
            if let Some(Element::Room(r)) = s.model.elements.get(id)
                && s.model.room_bound(r, s.model.room_area).is_err()
            {
                let level = s.model.level(r.level).map_or(String::new(), |l| l.name.clone());
                notes.push(format!("room {id} is not enclosed on {level}: copy its walls too"));
            }
        }
        let (placed, skipped) = place_openings(s, loose, by, Some(target));
        made.extend(placed);
        say_once(&mut left_out, skipped);
        let name = s.model.elements.id(target).and_then(|l| s.model.level(l)).map(|l| l.name.clone());
        pasted.push(json!({"level": s.model.elements.id(target), "name": name, "ids": made}));
        all.extend(made);
    }
    // The datum elements, once, beside: in the same command, so one undo takes them back with the rest.
    if !levelless.is_empty() {
        let sideways = Motion { map: &|q| micron(q + by), slide: Some(by), mirror: false };
        let (made, _) = duplicate_mapped(c, s, levelless, &sideways)?;
        pasted.push(json!({"level": null, "name": null, "ids": made}));
        all.extend(made);
    }
    if all.is_empty() {
        let said: Vec<String> = left_out.iter().chain(&notes).cloned().collect();
        return Err(bad(c, format!("nothing to paste: {}", said.join("; "))));
    }
    Ok(json!({"ids": all, "pasted": pasted, "notes": notes, "left_out": left_out}))
}

/// `more` added to `said`, those it does not say already.
fn say_once(said: &mut Vec<String>, more: Vec<String>) {
    for line in more {
        if !said.contains(&line) {
            said.push(line);
        }
    }
}

/// Moving what stood on level `from` to level `to`, `by` mm up: which level each level it measures from becomes.
struct Shift<'a> {
    table: &'a [(Guid, f64)],
    from: Guid,
    to: Guid,
    by: f64,
}

impl Shift<'_> {
    fn elevation(&self, g: Guid) -> Option<f64> {
        self.table.iter().find(|(l, _)| *l == g).map(|(_, z)| *z)
    }

    /// The level that stands where level `g` would, moved: `to` for `from`, else the level at the elevation `by` higher.
    fn level_for(&self, g: Guid) -> Option<Guid> {
        if g == self.from {
            return Some(self.to);
        }
        let z = self.elevation(g)? + self.by;
        self.table.iter().find(|(_, lz)| (lz - z).abs() <= TOL).map(|(l, _)| *l)
    }

    /// Every level `e` measures from, now the one that stands where it did, moved. The levels with none there are the result.
    fn relevel(&self, e: &mut Element) -> Vec<Guid> {
        let mut missing: Vec<Guid> = Vec::new();
        e.refs_mut(&mut |g| {
            if self.elevation(*g).is_some() {
                match self.level_for(*g) {
                    Some(n) => *g = n,
                    None => missing.push(*g),
                }
            }
        });
        missing
    }

    /// Of `ids`, the ones a paste can put at this height, and why it leaves out each of the others: an element that has to
    /// measure from a level where there is none, and has no offset to be raised by (a stair, a ramp, a CAD import, a site
    /// element, a room separation line), would stand too low by the levels' difference. What is left out takes its railings with it.
    fn placeable(&self, m: &Model, ids: &[Id]) -> (Vec<Id>, Vec<String>) {
        let (mut left_out, mut dropped) = (Vec::new(), BTreeSet::new());
        for &id in ids {
            let Some(e) = m.elements.get(id) else { continue };
            let mut probe = e.clone();
            self.retop(&mut probe, "", &mut Vec::new());
            if let Some(g) = self.relevel(&mut probe).first()
                && !raise(&mut probe, 0.0)
            {
                let z = self.elevation(*g).map_or(0.0, |z| z + self.by);
                left_out.push(format!("{}: no level at {z} mm, and it has no offset to raise it by", named(m, id)));
                dropped.insert(id);
            }
        }
        let guids: BTreeSet<Guid> = dropped.iter().map(|id| m.guid(*id)).collect();
        for &id in ids {
            if let Some(Element::Railing(r)) = m.elements.get(id)
                && r.host.is_some_and(|h| guids.contains(&h))
            {
                left_out.push(format!("{}: it goes with its stair or ramp, which is left out", named(m, id)));
                dropped.insert(id);
            }
        }
        (ids.iter().copied().filter(|id| !dropped.contains(id)).collect(), left_out)
    }

    /// `e`, a new copy, moved up the way the paste goes. What it can't keep it says in `notes`; `copies` are the elements this
    /// paste made.
    fn apply(&self, e: &mut Element, label: &str, copies: &BTreeSet<Guid>, notes: &mut Vec<String>) {
        self.retop(e, label, notes);
        let missing = self.relevel(e);
        if !missing.is_empty() {
            let z = missing.first().and_then(|g| self.elevation(*g)).map_or(0.0, |z| z + self.by);
            if raise(e, self.by) {
                notes.push(format!("{label}: no level at {z} mm, so it stays on its level, {} mm up", self.by));
            } else {
                e.refs_mut(&mut |g| {
                    if missing.contains(g) {
                        *g = self.to;
                    }
                });
                notes.push(format!("{label}: no level at {z} mm, so it is on the level pasted to"));
            }
        }
        // What it was attached to and wasn't copied is still where it was.
        match e {
            Element::Wall(w) => {
                let was = (w.attach_top, w.attach_base);
                for at in [&mut w.attach_top, &mut w.attach_base] {
                    if at.is_some_and(|g| !copies.contains(&g)) {
                        *at = None;
                    }
                }
                if was != (w.attach_top, w.attach_base) {
                    notes.push(format!("{label}: not attached to the roof or floor it was, which was not copied with it"));
                }
            }
            Element::Roof(r) if r.join.is_some_and(|g| !copies.contains(&g)) => {
                r.join = None;
                notes.push(format!("{label}: not joined to the roof it was, which was not copied with it"));
            }
            _ => {}
        }
    }

    /// A top level with no level at the height it would be moved to: the top is its height above the base, unconnected.
    fn retop(&self, e: &mut Element, label: &str, notes: &mut Vec<String>) {
        let z = |g: Guid| self.elevation(g);
        let mut unconnected =
            |height: f64| notes.push(format!("{label}: no level at the height of its top, so it is unconnected, {height} mm tall"));
        match e {
            Element::Wall(Wall { level, top_level, height, base_offset, top_offset, .. }) => {
                if let Some(t) = *top_level
                    && self.level_for(t).is_none()
                    && let (Some(zt), Some(zb)) = (z(t), z(*level))
                {
                    *height = zt + *top_offset - (zb + *base_offset);
                    (*top_level, *top_offset) = (None, 0.0);
                    unconnected(*height);
                }
            }
            Element::Shaft(crate::model::Shaft { level, top_level, height, base_offset, .. })
            | Element::Mass(crate::model::Mass { level, top_level, height, base_offset, .. })
            | Element::Column(crate::model::Column { level, top_level, height, base_offset, .. })
            | Element::StructuralColumn(crate::model::Column { level, top_level, height, base_offset, .. })
            | Element::Footing(crate::model::Column { level, top_level, height, base_offset, .. }) => {
                if let Some(t) = *top_level
                    && self.level_for(t).is_none()
                    && let (Some(zt), Some(zb)) = (z(t), z(*level))
                {
                    *height = zt - (zb + *base_offset);
                    *top_level = None;
                    unconnected(*height);
                }
            }
            Element::Stair(crate::model::Stair { level, top_level, height, .. })
            | Element::Ramp(crate::model::Ramp { level, top_level, height, .. }) => {
                if let Some(t) = *top_level
                    && self.level_for(t).is_none()
                    && let (Some(zt), Some(zb)) = (z(t), z(*level))
                {
                    *height = zt - zb;
                    *top_level = None;
                    unconnected(*height);
                }
            }
            // A room's Upper Limit, with no level there: its own level's, and its Limit Offset the whole way.
            Element::Room(r) => {
                if let Some(t) = r.top_level
                    && self.level_for(t).is_none()
                    && let (Some(zt), Some(zb)) = (z(t), z(r.level))
                {
                    r.limit_offset += zt - zb;
                    r.top_level = None;
                    notes.push(format!("{label}: no level at the height of its upper limit, so its limit is measured from its own level"));
                }
            }
            _ => {}
        }
    }
}

/// `e` raised `dz` mm by the offset it has from its level. False when it has none.
fn raise(e: &mut Element, dz: f64) -> bool {
    match e {
        Element::Wall(w) => w.base_offset += dz,
        Element::Shaft(crate::model::Shaft { base_offset, .. })
        | Element::Mass(crate::model::Mass { base_offset, .. })
        | Element::Column(crate::model::Column { base_offset, .. })
        | Element::StructuralColumn(crate::model::Column { base_offset, .. })
        | Element::Footing(crate::model::Column { base_offset, .. }) => *base_offset += dz,
        // Its top as well: with no upper limit it is measured from its own level.
        Element::Room(r) => {
            r.base_offset += dz;
            if r.top_level.is_none() {
                r.limit_offset += dz;
            }
        }
        Element::PointCloud(pc) => pc.offset += dz,
        Element::BuildingPad(bp) => bp.offset += dz,
        Element::Rebar(rb) => rb.path.iter_mut().for_each(|q| q[2] += dz),
        Element::Floor(f) | Element::Toposolid(f) => f.offset += dz,
        Element::Ceiling(cl) => cl.offset += dz,
        Element::Roof(r) => r.offset += dz,
        Element::Component(cp) => cp.offset += dz,
        run_arm!(r) => {
            r.offset += dz;
            r.end_offset = r.end_offset.map(|x| x + dz);
        }
        Element::Railing(r) if r.host.is_none() => r.base_offset += dz,
        _ => return false,
    }
    true
}

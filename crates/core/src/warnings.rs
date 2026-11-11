//! Revit's Review Warnings: what is questionable in a model without making it wrong.
//!
//! [`Model::check`] refuses only what would break the model (a reference to nothing, a number that is not a number, a shape
//! the geometry cannot build). A judgment about design is a [`Warning`] here: two openings that overlap, a door wider than its
//! wall's run or past its end, a sill under the wall's base or over its top, walls that overlap or sit on each other, a wall
//! slightly off axis, a room that is not enclosed or that another room already has, a family of devices with no connector.
//! The model keeps all of them, as Revit does, and says so to whoever made the change ([`Session::execute`] adds the ones a
//! command made to its result) and to whoever asks (`warnings`, the Warnings list of the window).
//!
//! The warnings are worked out from the model, not stored in it, so a file holds none and an undo takes them back with the
//! change. [`Model::review`] works out all of them; [`Model::review_since`] brings a model's up to date from those of the
//! model it was made of, working out again only what the change reached. Tests hold the two to the same answer.

use crate::geom::P2;
use crate::ids::{Changed, Guid, Id};
use crate::model::{Element, Model, TOL, Unbound};
use crate::walls::{self, Axis};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// How much a warning matters. Revit lists them all as warnings; a note is one that changes nothing a person would see or
/// measure (an element a hair off axis).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Note,
}

/// One thing amiss: which rule it breaks, what to say about it, and the elements it is about.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Warning {
    /// The rule, one of [`RULES`]'s ids.
    pub rule: &'static str,
    pub severity: Severity,
    /// What is amiss and what would put it right, in words an architect reads.
    pub message: String,
    /// The elements it is about, by GUID: what Show selects.
    pub elements: Vec<Guid>,
    /// What makes it the same warning after a change that leaves it standing: the rule and this.
    #[serde(skip)]
    key: String,
}

impl Warning {
    fn new(rule: &'static str, severity: Severity, message: String, elements: Vec<Guid>) -> Warning {
        let mut key: Vec<Guid> = elements.clone();
        key.sort_unstable();
        let key = key.iter().map(ToString::to_string).collect::<Vec<_>>().join(",");
        Warning { rule, severity, message, elements, key }
    }

    /// The same warning of a later model: the same rule about the same elements (or, for a rule that is about a family, the
    /// same family). Its message may have changed with the numbers in it.
    pub fn same_as(&self, other: &Warning) -> bool {
        self.rule == other.rule && self.key == other.key
    }

    /// As an agent reads it: the rule, the severity, the message, and the elements by id and by GUID.
    pub fn json(&self, m: &Model) -> Value {
        json!({
            "rule": self.rule,
            "severity": self.severity,
            "message": self.message,
            "elements": self.elements.iter().filter_map(|g| m.elements.id(*g)).collect::<Vec<Id>>(),
            "guids": self.elements,
        })
    }
}

/// The rules, each with the id its warnings carry and what it says is amiss.
pub const RULES: &[(&str, &str)] = &[
    ("openings_overlap", "doors or windows that overlap in their wall"),
    ("opening_past_wall", "a door or window wider than its wall's run, or past its end"),
    ("opening_outside_height", "a door or window whose sill is under its wall's base or at or over its top"),
    ("walls_overlap", "walls whose bodies overlap"),
    ("identical_instances", "identical elements in the same place"),
    ("wall_off_axis", "a wall or a grid slightly off axis"),
    ("room_not_enclosed", "a room the walls and separation lines do not close round"),
    ("room_redundant", "a room in a space another room has"),
    ("device_no_connector", "a family of devices with no connector, counted as a load of zero"),
    ("duplicate_type_mark", "types of one category that share a Type Mark"),
];

/// The most overlapping pairs one wall's openings list: a wall of thousands of doors on one spot is one problem.
const MAX_PAIRS_PER_WALL: usize = 500;
/// How far off an axis, in degrees, a wall is "slightly off axis": more is a choice, less a rounding.
const OFF_AXIS: (f64, f64) = (1e-4, 1.0);

/// What [`compute`] works out: every element, or those of a set.
struct Scope<'a> {
    ids: Option<&'a BTreeSet<Id>>,
    /// The model the elements changed from: where what has gone, or has moved, stood.
    before: Option<&'a Model>,
}

impl Scope<'_> {
    fn takes(&self, id: Id) -> bool {
        self.ids.is_none_or(|s| s.contains(&id))
    }
}

impl Model {
    /// Every warning of the model, the worst first, then by rule and element.
    pub fn review(&self) -> Vec<Warning> {
        let mut out = Vec::new();
        compute(self, &Scope { ids: None, before: None }, true, &mut out);
        sort(self, &mut out);
        out
    }

    /// [`Model::review`] of this model, which a command made of `before`, whose warnings `prev` are: those about what changed
    /// (and what points at it) are dropped and worked out again, the rest are as they were. The same answer as `review`, which
    /// it is where what changed reaches the whole project (a level, a roof or a floor a wall is attached to, a type or any
    /// setting) or isn't known.
    pub fn review_since(&self, before: &Model, prev: &[Warning]) -> Vec<Warning> {
        let Changed::Ids(changed) = self.changed_since(before) else { return self.review() };
        if changed.is_empty() {
            return prev.to_vec();
        }
        let both = |id: Id| [self, before].into_iter().filter_map(move |m| m.elements.get(id));
        let reaches_all = |e: &Element| matches!(e, Element::Level(_) | Element::Roof(_) | Element::Floor(_) | Element::Toposolid(_));
        if changed.iter().any(|id| both(*id).any(reaches_all)) {
            return self.review();
        }
        let bounds = |e: &Element| {
            matches!(e, Element::Wall(_) | Element::Room(_) | Element::RoomSeparator(_) | Element::Column(_) | Element::StructuralColumn(_))
        };
        let rooms = changed.iter().any(|id| both(*id).any(bounds));
        // What changed, and what points at it (an opening's wall's change is the opening's).
        let lookup = self.lookup();
        let mut ids = changed.clone();
        for id in &changed {
            if let Some(g) = self.elements.guid(*id) {
                ids.extend(lookup.referrers(&g));
            }
        }
        let gone: BTreeSet<Guid> = ids.iter().filter_map(|id| self.elements.guid(*id).or_else(|| before.elements.guid(*id))).collect();
        let room_rule = |w: &Warning| w.rule.starts_with("room_");
        let scope = Scope { ids: Some(&ids), before: Some(before) };
        // A group of twins grows or shrinks with one of its members' change, though the others are as they were.
        let twins = twin_keys(self, &scope).unwrap_or_default();
        let twinned = |w: &Warning| {
            w.rule == "identical_instances"
                && w.elements.iter().any(|g| self.elements.get(*g).and_then(|e| identity(self, e)).is_some_and(|k| twins.contains(&k)))
        };
        let mut out: Vec<Warning> = prev
            .iter()
            .filter(|w| {
                w.rule != "device_no_connector" && !(rooms && room_rule(w)) && !twinned(w) && !w.elements.iter().any(|g| gone.contains(g))
            })
            .cloned()
            .collect();
        compute(self, &scope, rooms, &mut out);
        sort(self, &mut out);
        out
    }

    /// What the warnings are now that were not among `prev`: those of the same rule about other elements.
    pub fn new_warnings<'a>(now: &'a [Warning], prev: &[Warning]) -> Vec<&'a Warning> {
        let held: BTreeSet<(&str, &str)> = prev.iter().map(|w| (w.rule, w.key.as_str())).collect();
        now.iter().filter(|w| !held.contains(&(w.rule, w.key.as_str()))).collect()
    }

    /// What is amiss in the model but doesn't make it wrong, as messages: those of [`Model::review`]. `check` refuses a model;
    /// these it accepts.
    pub fn warnings(&self) -> Vec<String> {
        self.review().into_iter().map(|w| w.message).collect()
    }
}

fn sort(m: &Model, out: &mut [Warning]) {
    let ids = |w: &Warning| w.elements.iter().map(|g| m.elements.id(*g).unwrap_or(0)).collect::<Vec<Id>>();
    out.sort_by_cached_key(|w| (w.severity, w.rule, ids(w), w.key.clone()));
}

/// The warnings about `scope`'s elements, with (`rooms`) those about rooms, added to `out`.
fn compute(m: &Model, scope: &Scope, rooms: bool, out: &mut Vec<Warning>) {
    openings(m, scope, out);
    walls_and_grids(m, scope, out);
    identical(m, scope, out);
    if rooms {
        room_warnings(m, out);
    }
    devices(m, out);
    // Types are settings: a change to one is a full review, and a change to elements keeps these warnings.
    if scope.ids.is_none() {
        duplicate_type_marks(m, out);
    }
}

fn cat(m: &Model, id: Id) -> &'static str {
    m.elements.get(id).map_or("element", Element::category)
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A door or window in a wall: what the rules about openings ask of it.
struct Placed {
    id: Id,
    start: f64,
    end: f64,
    sill: f64,
    head: f64,
}

/// The rules about doors and windows, of those in `scope` and those they share a wall with.
fn openings(m: &Model, scope: &Scope, out: &mut Vec<Warning>) {
    let mut hosts: BTreeSet<Guid> = BTreeSet::new();
    match scope.ids {
        None => {
            for e in m.elements.values() {
                if let Element::Door(o) | Element::Window(o) = e {
                    hosts.insert(o.host);
                }
            }
        }
        Some(ids) => {
            for id in ids {
                match m.elements.get(*id) {
                    Some(Element::Door(o) | Element::Window(o)) => {
                        hosts.insert(o.host);
                    }
                    Some(Element::Wall(_)) => {
                        hosts.extend(m.elements.guid(*id));
                    }
                    _ => {}
                }
            }
        }
    }
    let lookup = m.lookup();
    for host in hosts {
        let (Some(wall_id), Some(w)) = (m.elements.id(host), m.wall(host)) else { continue };
        let (length, base) = (Axis::of(w).length(), m.own_base(w));
        let mut placed: Vec<Placed> = Vec::new();
        for id in lookup.referrers(&host) {
            let Some(e @ (Element::Door(o) | Element::Window(o))) = m.elements.get(id) else { continue };
            if o.host != host {
                continue;
            }
            let Ok(t) = m.opening_dims(e) else { continue };
            placed.push(Placed {
                id,
                start: o.offset - t.width / 2.0,
                end: o.offset + t.width / 2.0,
                sill: o.sill,
                head: o.sill + t.height,
            });
            if !scope.takes(id) {
                continue;
            }
            let (a, b) = (o.offset - t.width / 2.0, o.offset + t.width / 2.0);
            let who = format!("{} {id}", e.category());
            if t.width > length + TOL {
                out.push(Warning::new(
                    "opening_past_wall",
                    Severity::Warning,
                    format!(
                        "{who} is {} mm wide, wider than wall {wall_id}'s run, {} mm: shorten it or lengthen the wall",
                        round(t.width),
                        round(length)
                    ),
                    vec![m.guid(id)],
                ));
            } else if a < -TOL || b > length + TOL {
                let (side, by) = if a < -TOL { ("start", -a) } else { ("end", b - length) };
                out.push(Warning::new(
                    "opening_past_wall",
                    Severity::Warning,
                    format!(
                        "{who} reaches {} mm past the {side} of wall {wall_id}, {} mm long (it spans {} to {} mm along it): move it along the wall",
                        round(by),
                        round(length),
                        round(a),
                        round(b)
                    ),
                    vec![m.guid(id)],
                ));
            }
            if o.sill < 0.0 {
                out.push(Warning::new(
                    "opening_outside_height",
                    Severity::Warning,
                    format!("{who}: its sill, {} mm, is under the base of wall {wall_id}; give it a sill of 0 or more", round(o.sill)),
                    vec![m.guid(id)],
                ));
            }
            let top = [a, o.offset, b].map(|s| walls::top_at(m, w, s)).into_iter().fold(f64::INFINITY, f64::min) - base;
            if o.sill > top - TOL {
                out.push(Warning::new(
                    "opening_outside_height",
                    Severity::Warning,
                    format!(
                        "{who}: its sill, {} mm, is at or over the top of wall {wall_id}, {} mm up from its base there, so nothing of it is in the wall; lower its sill or raise the wall's top",
                        round(o.sill),
                        round(top)
                    ),
                    vec![m.guid(id)],
                ));
            }
        }
        // Openings share a wall's place only where they share its height too: one over the other is not an overlap, and a
        // storefront wall with a window a storey is a wall whose windows overlap in plan alone.
        placed.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
        let mut pairs = 0;
        'sweep: for (i, a) in placed.iter().enumerate() {
            for b in placed.iter().skip(i + 1) {
                if b.start > a.end + TOL {
                    break;
                }
                if a.end > b.start + TOL && a.head > b.sill + TOL && b.head > a.sill + TOL && (scope.takes(a.id) || scope.takes(b.id)) {
                    let (low, high) = if a.id < b.id { (a.id, b.id) } else { (b.id, a.id) };
                    out.push(Warning::new(
                        "openings_overlap",
                        Severity::Warning,
                        format!(
                            "{} {low} and {} {high} overlap in wall {wall_id}: move one of them along the wall",
                            cat(m, low),
                            cat(m, high)
                        ),
                        vec![m.guid(low), m.guid(high)],
                    ));
                    pairs += 1;
                    if pairs >= MAX_PAIRS_PER_WALL {
                        break 'sweep;
                    }
                }
            }
        }
    }
}

/// Degrees a direction is off the nearest axis: 0 along x or y, up to 45.
fn off_axis(d: P2) -> f64 {
    let angle = d.1.atan2(d.0).to_degrees();
    ((angle + 45.0).rem_euclid(90.0) - 45.0).abs()
}

/// The walls in `scope` that overlap another, and the walls and grids in it that are slightly off axis.
fn walls_and_grids(m: &Model, scope: &Scope, out: &mut Vec<Warning>) {
    let mut taken: Vec<Id> = Vec::new();
    match scope.ids {
        None => taken.extend(m.elements.iter().filter(|(_, e)| matches!(e, Element::Wall(_) | Element::Grid(_))).map(|(id, _)| *id)),
        Some(ids) => {
            taken.extend(ids.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::Wall(_) | Element::Grid(_)))))
        }
    }
    let off = |who: String, id: Id, d: P2, out: &mut Vec<Warning>| {
        let dev = off_axis(d);
        if dev > OFF_AXIS.0 && dev <= OFF_AXIS.1 {
            out.push(Warning::new(
                "wall_off_axis",
                Severity::Note,
                format!("{who} is {dev:.4}° off the {} axis: make it exactly {}, or turn it clearly", axis_name(d), axis_name(d)),
                vec![m.guid(id)],
            ));
        }
    };
    let lookup = m.lookup();
    for id in taken {
        match m.elements.get(id) {
            Some(Element::Grid(g)) => off(format!("grid {id}"), id, g.end - g.start, out),
            Some(Element::Wall(w)) => {
                let axis = Axis::of(w);
                if axis.radius().is_some() {
                    continue;
                }
                off(format!("wall {id}"), id, w.end - w.start, out);
                let Some(line) = centre_line(m, w) else { continue };
                // The walls near this one, which the lookup finds by plan tile and height.
                let reach = walls::reach(m, w);
                let Some(b) = reach else { continue };
                let grow = m.types.wall.values().map(|t| t.thickness.abs()).fold(0.0, f64::max) + 2.0 * TOL;
                for other in lookup.walls_near_at(&[(b, m.base_z(w), m.top_z(w))], grow) {
                    // Each pair once, from the lower id, or from whichever is in scope.
                    if other == id || (other < id && scope.takes(other)) {
                        continue;
                    }
                    let Some(v) = m.wall(other) else { continue };
                    if !walls::stacked(m, w, v) {
                        continue;
                    }
                    let Some(second) = centre_line(m, v) else { continue };
                    if let Some(along) = overlap_of(&line, m.thickness(w), &second, m.thickness(v))
                        && !same_wall(m, w, v)
                    {
                        let (low, high) = if id < other { (id, other) } else { (other, id) };
                        out.push(Warning::new(
                            "walls_overlap",
                            Severity::Warning,
                            format!("walls {low} and {high} overlap for {} mm: join them, move one, or delete one", round(along)),
                            vec![m.guid(low), m.guid(high)],
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

fn axis_name(d: P2) -> &'static str {
    if d.0.abs() >= d.1.abs() { "x" } else { "y" }
}

/// A straight wall's centre line, as its two ends.
fn centre_line(m: &Model, w: &crate::model::Wall) -> Option<[P2; 2]> {
    let axis = Axis::of(w);
    if axis.radius().is_some() {
        return None;
    }
    let c = walls::centre(m, w);
    let line = [axis.at(0.0, c), axis.at(axis.length(), c)];
    (line[0].is_finite() && line[1].is_finite() && line[0].dist(line[1]) > TOL).then_some(line)
}

/// How far, mm, two straight walls' bodies overlap along their length, when they lie side by side (parallel, their centre lines
/// closer than their thicknesses' half-sum, or on one another); `None` when they only meet or cross.
fn overlap_of(a: &[P2; 2], ta: f64, b: &[P2; 2], tb: f64) -> Option<f64> {
    let (d, e) = ((a[1] - a[0]).unit(), (b[1] - b[0]).unit());
    if d.cross(e).abs() > 1e-4 {
        return None;
    }
    let lateral = d.cross(b[0] - a[0]).abs();
    if lateral >= (ta + tb) / 2.0 - TOL && lateral > TOL {
        return None;
    }
    let along = |p: P2| d.dot(p - a[0]);
    let (b0, b1) = (along(b[0]), along(b[1]));
    let len = a[0].dist(a[1]);
    let overlap = b0.max(b1).min(len) - b0.min(b1).max(0.0);
    (overlap > TOL).then_some(overlap)
}

/// Whether two walls are one wall twice: the same ends (either way round), type, base and height.
fn same_wall(m: &Model, a: &crate::model::Wall, b: &crate::model::Wall) -> bool {
    wall_identity(m, a) == wall_identity(m, b)
}

fn wall_identity(m: &Model, w: &crate::model::Wall) -> String {
    let ends = {
        let (a, b) = (point(w.start), point(w.end));
        if w.arc == 0.0 && b < a { (b, a, 0) } else { (a, b, tenths(w.arc)) }
    };
    let _ = m;
    format!(
        "wall {} {} {ends:?} {} {:?} {} {}",
        w.type_name,
        w.level,
        tenths(w.base_offset),
        w.top_level,
        tenths(w.height),
        tenths(w.top_offset),
    )
}

fn tenths(v: f64) -> i64 {
    (v * 10.0).round() as i64
}

fn point(p: P2) -> (i64, i64) {
    (tenths(p.0), tenths(p.1))
}

/// What makes an element the same as another standing where it does: all it is made of, to a tenth of a millimetre. `None` for
/// what the rule does not look at.
fn identity(m: &Model, e: &Element) -> Option<String> {
    Some(match e {
        Element::Wall(w) => wall_identity(m, w),
        Element::Column(c) | Element::StructuralColumn(c) | Element::Footing(c) => format!(
            "{} {} {} {:?} {} {} {:?} {}",
            e.category(),
            c.type_name,
            c.level,
            point(c.at),
            tenths(c.angle),
            tenths(c.base_offset),
            c.top_level,
            tenths(c.height)
        ),
        Element::Component(c) => format!(
            "component {} {} {} {:?} {} {} {} {}",
            c.family,
            c.type_name,
            c.level,
            point(c.at),
            tenths(c.angle),
            tenths(c.offset),
            c.mirrored,
            serde_json::to_string(&c.params).unwrap_or_default()
        ),
        _ => return None,
    })
}

/// What the elements of `scope` are the same as, where one has twins: their identities now, and before the change.
fn twin_keys(m: &Model, scope: &Scope) -> Option<BTreeSet<String>> {
    scope.ids.map(|ids| {
        let keys = |m: &Model| ids.iter().filter_map(|id| m.elements.get(*id).and_then(|e| identity(m, e))).collect::<Vec<_>>();
        keys(m).into_iter().chain(scope.before.map(keys).unwrap_or_default()).collect()
    })
}

/// Elements of one kind that are one another's twins: each group of them, when one is in `scope`.
fn identical(m: &Model, scope: &Scope, out: &mut Vec<Warning>) {
    let looked_at = |e: &Element| {
        matches!(e, Element::Wall(_) | Element::Column(_) | Element::StructuralColumn(_) | Element::Footing(_) | Element::Component(_))
    };
    // With a scope, only the keys of what it holds are looked for, among the elements of the kinds that can be twins.
    let wanted = twin_keys(m, scope);
    if wanted.as_ref().is_some_and(BTreeSet::is_empty) {
        return;
    }
    let mut groups: HashMap<String, Vec<Id>> = HashMap::new();
    for (id, e) in m.elements.iter().filter(|(_, e)| looked_at(e)) {
        if let Some(key) = identity(m, e)
            && wanted.as_ref().is_none_or(|w| w.contains(&key))
        {
            groups.entry(key).or_default().push(*id);
        }
    }
    for (_, ids) in groups {
        if ids.len() < 2 {
            continue;
        }
        let what = cat(m, ids[0]);
        let list = ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        out.push(Warning::new(
            "identical_instances",
            Severity::Warning,
            format!("{what}s {list} are identical and in the same place: delete the extra{}", if ids.len() > 2 { "s" } else { "" }),
            ids.iter().map(|id| m.guid(*id)).collect(),
        ));
    }
}

/// The rooms the walls don't close round, and those in a space another room has.
fn room_warnings(m: &Model, out: &mut Vec<Warning>) {
    let rooms = crate::rooms::Rooms::new(m);
    for (id, e) in m.elements.iter() {
        let Element::Room(r) = e else { continue };
        match rooms.bound(r, m.room_area) {
            Ok(_) => {}
            Err(Unbound::NotEnclosed) => out.push(Warning::new(
                "room_not_enclosed",
                Severity::Warning,
                format!("room {id} ({} {}) is not enclosed: the walls around it no longer close", r.number, r.name),
                vec![m.guid(*id)],
            )),
            Err(Unbound::Shared(other)) => out.push(Warning::new(
                "room_redundant",
                Severity::Warning,
                format!(
                    "room {id} ({} {}) is in the space room {} has; two rooms can't share one, so it is unbounded: delete one, \
                     or part them with a room separation line",
                    r.number,
                    r.name,
                    m.shown(other)
                ),
                vec![m.guid(*id), m.guid(other)],
            )),
        }
    }
}

/// The families of lighting, outlets or switches in use that hold no connector, so their devices count as a load of zero (a
/// family of plan symbols alone, a symbol set's, is no device). One warning for each family, about its placed devices.
fn devices(m: &Model, out: &mut Vec<Warning>) {
    let mut bare: BTreeMap<&str, Vec<Id>> = BTreeMap::new();
    for (id, e) in m.elements.iter() {
        if let Element::Component(c) = e
            && let Some(f) = m.families.get(&c.family)
            && f.category.electrical_device()
            && f.connectors.is_empty()
            && !(f.forms.is_empty() && !f.symbols.is_empty())
        {
            bare.entry(c.family.as_str()).or_default().push(*id);
        }
    }
    for (name, ids) in bare {
        let k = ids.len();
        let mut w = Warning::new(
            "device_no_connector",
            Severity::Warning,
            format!(
                "family `{name}` has no connector, so its {k} placed device{} count{} as a load of zero: give it one with `family` `connectors`",
                if k == 1 { "" } else { "s" },
                if k == 1 { "s" } else { "" }
            ),
            ids.iter().map(|id| m.guid(*id)).collect(),
        );
        // About the family: placing one more device of it is no new warning.
        w.key = name.to_string();
        out.push(w);
    }
}

/// Types of one category that share a non-empty Type Mark (Revit's "Elements have duplicate 'Type Mark' values"): a mark names one
/// type, so a schedule or a tag that reads it names two. One warning for each mark, about those types (by their library GUIDs: a
/// type is no element of the plan, so Show has nothing to select). A project's types and a family's are of one category when
/// both are doors, both windows, or both of one component category. Only types change it, and a changed type is a changed
/// setting, a full review: a review of a change to elements keeps the warnings it had.
fn duplicate_type_marks(m: &Model, out: &mut Vec<Warning>) {
    use crate::library::Kind;
    let mut marked: BTreeMap<(String, String), Vec<(Guid, String)>> = BTreeMap::new();
    for (_, entry) in m.library.entries() {
        let (category, label, mark) = match &entry.kind {
            Kind::Type(category) => {
                let Some(identity) = m.identity_of(category, &entry.name) else { continue };
                (category.clone(), entry.name.clone(), identity.type_mark.clone())
            }
            Kind::FamilyType => {
                let Some((name, ty)) = entry.name.split_once(": ") else { continue };
                let Some(family) = m.families.get(name) else { continue };
                let Some(identity) = family.identity.get(ty) else { continue };
                (format!("{:?}", family.category).to_lowercase(), entry.name.clone(), identity.type_mark.clone())
            }
            _ => continue,
        };
        if !mark.is_empty() {
            marked.entry((category, mark)).or_default().push((entry.guid, label));
        }
    }
    for ((category, mark), types) in marked {
        if types.len() < 2 {
            continue;
        }
        let names = types.iter().map(|(_, label)| format!("`{label}`")).collect::<Vec<_>>().join(", ");
        out.push(Warning::new(
            "duplicate_type_mark",
            Severity::Warning,
            format!("{category} types {names} have the same Type Mark, `{mark}`: give each its own, so a mark names one type"),
            types.iter().map(|(guid, _)| *guid).collect(),
        ));
    }
}

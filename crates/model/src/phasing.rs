//! Phases and design options. A project has phases in order (Existing, New Construction): each
//! element is made in one (its `Phase Created`, the first unless it says) and may be taken down in
//! a later one (`Phase Demolished`); a view has a phase, and a phase filter that says how each of
//! the four things an element can be in that phase (new, existing, demolished, temporary: made and
//! taken down within it) shows: as its category does, as the project's overrides for it say, or not
//! at all. A project may also have sets of design options: an element in one option of a set shows
//! only in a view that shows that option (a view's own choice, else the set's primary). Both are
//! values elements hold among their parameters, under names no project parameter may take, so
//! copies, `modify`, schedules and view filters reach them as they reach any other.

use bimcraft_i18n::msg;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::graphics::{Dash, Graphics, Override, Style};
use crate::language::Language;
use crate::model::{Element, Id, MAX_TEXT, Model};

/// The parameters an element holds its phase and its design option in.
pub const CREATED: &str = "Phase Created";
pub const DEMOLISHED: &str = "Phase Demolished";
pub const OPTION: &str = "Design Option";

pub const MAX_PHASES: usize = 64;
pub const MAX_PHASE_FILTERS: usize = 64;
pub const MAX_OPTION_SETS: usize = 32;
pub const MAX_OPTIONS: usize = 16;

/// Whether `name` is one of the parameters phases and design options live in.
pub fn reserved(name: &str) -> bool {
    matches!(name, CREATED | DEMOLISHED | OPTION)
}

/// What an element is in a view's phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Made in it, and still standing at its end.
    New,
    /// Made before it, and still standing at its end.
    Existing,
    /// Made before it, and taken down in it.
    Demolished,
    /// Made and taken down in it.
    Temporary,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::New, Status::Existing, Status::Demolished, Status::Temporary];

    pub fn name(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Existing => "existing",
            Status::Demolished => "demolished",
            Status::Temporary => "temporary",
        }
    }
}

/// How a phase filter shows what is in a status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presentation {
    /// As its category's overrides, and the view's, say.
    #[default]
    ByCategory,
    /// As the project's overrides for the status say.
    Overridden,
    NotDisplayed,
}

/// A phase filter: how each status shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhaseFilter {
    pub new: Presentation,
    pub existing: Presentation,
    pub demolished: Presentation,
    pub temporary: Presentation,
}

impl PhaseFilter {
    pub fn of(&self, status: Status) -> Presentation {
        match status {
            Status::New => self.new,
            Status::Existing => self.existing,
            Status::Demolished => self.demolished,
            Status::Temporary => self.temporary,
        }
    }

    pub fn set(&mut self, status: Status, p: Presentation) {
        match status {
            Status::New => self.new = p,
            Status::Existing => self.existing = p,
            Status::Demolished => self.demolished = p,
            Status::Temporary => self.temporary = p,
        }
    }
}

/// A set of design options: the options, and which is the primary, the one a view shows unless it
/// chooses.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionSet {
    pub options: Vec<String>,
    pub primary: String,
}

/// A project's phases to start from, in `language`.
pub fn phases(language: Language) -> Vec<String> {
    [msg!("Existing"), msg!("New Construction")].map(|w| language.word(w).to_string()).to_vec()
}

pub fn default_phases() -> Vec<String> {
    phases(Language::English)
}

/// The phase filters a project starts with, in `language`.
pub fn filters(language: Language) -> BTreeMap<String, PhaseFilter> {
    use Presentation::{ByCategory as B, NotDisplayed as N, Overridden as O};
    let f = |new, existing, demolished, temporary| PhaseFilter { new, existing, demolished, temporary };
    [
        (msg!("Show All"), f(B, B, B, B)),
        (msg!("Show Complete"), f(B, B, N, B)),
        (msg!("Show New"), f(B, N, N, N)),
        (msg!("Show Previous + Demolished"), f(N, B, O, N)),
        (msg!("Show Previous + New"), f(B, O, N, B)),
    ]
    .into_iter()
    .map(|(n, f)| (language.word(n).to_string(), f))
    .collect()
}

pub fn default_filters() -> BTreeMap<String, PhaseFilter> {
    filters(Language::English)
}

/// What each status is drawn as when a phase filter says so: what stood before in halftone, what
/// comes down dashed and red.
pub fn overrides() -> BTreeMap<Status, Override> {
    let red = Some([200, 40, 30]);
    let dashed = Style { color: red, dash: Some(Dash::Dashed), ..Style::default() };
    [
        (Status::Existing, Override { halftone: true, ..Override::default() }),
        (Status::Demolished, Override { projection: dashed.clone(), cut: dashed, ..Override::default() }),
    ]
    .into_iter()
    .collect()
}

pub fn default_overrides() -> BTreeMap<Status, Override> {
    overrides()
}

/// Whether a project's phases, phase filters and overrides are the ones a new English project has:
/// then its file leaves them out.
pub fn is_default_phases(p: &Vec<String>) -> bool {
    *p == default_phases()
}

pub fn is_default_filters(f: &BTreeMap<String, PhaseFilter>) -> bool {
    *f == default_filters()
}

pub fn is_default_overrides(o: &BTreeMap<Status, Override>) -> bool {
    *o == default_overrides()
}

/// Where phase `name` stands in the project's order.
pub fn index(m: &Model, name: &str) -> Option<usize> {
    m.phases.iter().position(|p| p == name)
}

/// The value of built-in parameter `key` an element has: its own, or a door's or window's none, its
/// wall's.
pub fn value<'a>(m: &'a Model, e: &'a Element, key: &str) -> Option<&'a str> {
    let own = e.params().and_then(|p| p.get(key)).and_then(Value::as_str);
    own.or_else(|| match e {
        Element::Door(o) | Element::Window(o) => m.wall(o.host)?.params.get(key)?.as_str(),
        _ => None,
    })
}

/// What element `e` is in the phase at `at` in the project's order; none when it isn't there: made
/// after it, or taken down before it. Only what holds parameters is phased: annotation is a view's.
pub fn status(m: &Model, e: &Element, at: usize) -> Option<Status> {
    e.params()?;
    let created = value(m, e, CREATED).and_then(|n| index(m, n)).unwrap_or(0);
    let demolished = value(m, e, DEMOLISHED).and_then(|n| index(m, n));
    if created > at {
        return None;
    }
    match demolished {
        Some(d) if d < at => None,
        Some(d) if d == at => Some(if created == at { Status::Temporary } else { Status::Demolished }),
        _ => Some(if created == at { Status::New } else { Status::Existing }),
    }
}

/// Whether anything in `m` is phased or in a design option.
pub fn any(m: &Model) -> bool {
    match m.derived.made(&m.elements) {
        Some(look) => look.any_phased(),
        None => m.elements.values().any(|e| e.params().is_some_and(|p| p.keys().any(|k| reserved(k)))),
    }
}

/// Whether a view that shows the options `g` chooses shows element `e`: one in no option shows
/// everywhere; one in an option, where its set's chosen option, else its primary, is that one.
pub fn option_shown(m: &Model, g: &Graphics, e: &Element) -> bool {
    let Some((set, option)) = value(m, e, OPTION).and_then(|v| v.split_once(": ")) else { return true };
    let Some(s) = m.option_sets.get(set) else { return true };
    g.options.get(set).unwrap_or(&s.primary) == option
}

/// What a project's names of phases, phase filters and option sets and their options must be.
fn named(who: &str, n: &str) -> Result<(), String> {
    if n.trim().is_empty() || n.trim() != n || n.chars().count() > MAX_TEXT {
        return Err(format!(
            "{who} `{n}`: a name is 1 to {MAX_TEXT} characters, with nothing before or after it but letters, digits and the like"
        ));
    }
    Ok(())
}

/// Whether the project's phases, phase filters, overrides and option sets are sound.
pub(crate) fn check_project(m: &Model) -> Result<(), String> {
    if m.phases.is_empty() || m.phases.len() > MAX_PHASES {
        return Err(format!("a project has 1 to {MAX_PHASES} phases, not {}", m.phases.len()));
    }
    let mut seen = BTreeSet::new();
    for p in &m.phases {
        named("phase", p)?;
        if !seen.insert(p.as_str()) {
            return Err(format!("phase `{p}` is in the project twice"));
        }
    }
    if m.phase_filters.len() > MAX_PHASE_FILTERS {
        return Err(format!("the project has {} phase filters; it holds up to {MAX_PHASE_FILTERS}", m.phase_filters.len()));
    }
    for n in m.phase_filters.keys() {
        named("phase filter", n)?;
    }
    for (s, o) in &m.phase_overrides {
        o.check(&format!("the {} phase's overrides", s.name()))?;
        o.check_patterns(m)?;
        if o.hidden || o.transparency > 0 {
            return Err(format!("the {} phase's overrides: a phase filter hides what it doesn't display; overrides draw", s.name()));
        }
    }
    if m.option_sets.len() > MAX_OPTION_SETS {
        return Err(format!("the project has {} option sets; it holds up to {MAX_OPTION_SETS}", m.option_sets.len()));
    }
    for (n, s) in &m.option_sets {
        named("design option set", n)?;
        if n.contains(':') {
            return Err(format!("design option set `{n}`: a name has no colon in it"));
        }
        if s.options.is_empty() || s.options.len() > MAX_OPTIONS {
            return Err(format!("design option set `{n}` has 1 to {MAX_OPTIONS} options, not {}", s.options.len()));
        }
        let mut seen = BTreeSet::new();
        for o in &s.options {
            named(&format!("design option set `{n}`: option"), o)?;
            if o.contains(':') {
                return Err(format!("design option set `{n}`: option `{o}`: a name has no colon in it"));
            }
            if !seen.insert(o.as_str()) {
                return Err(format!("design option set `{n}` has option `{o}` twice"));
            }
        }
        if !s.options.contains(&s.primary) {
            return Err(format!("design option set `{n}`: its primary `{}` isn't one of its options", s.primary));
        }
    }
    Ok(())
}

/// Whether the project has design option `value` (`set: option`).
pub fn option_exists(m: &Model, value: &str) -> bool {
    value.split_once(": ").is_some_and(|(s, o)| m.option_sets.get(s).is_some_and(|set| set.options.iter().any(|x| x == o)))
}

/// Whether `v`, the value of built-in parameter `key`, is one the project has; none for another key.
pub(crate) fn check_value(m: &Model, who: &str, key: &str, v: &Value) -> Option<Result<(), String>> {
    let text = v.as_str();
    Some(match key {
        CREATED | DEMOLISHED => match text {
            Some(n) if index(m, n).is_some() => Ok(()),
            _ => Err(format!("{who}: `{key}` is one of the project's phases ({}), not {v}", m.phases.join(", "))),
        },
        OPTION => match text.and_then(|t| t.split_once(": ")) {
            Some((s, o)) if m.option_sets.get(s).is_some_and(|set| set.options.iter().any(|x| x == o)) => Ok(()),
            _ => Err(format!("{who}: `{key}` is `set: option`, one the project has, not {v}")),
        },
        _ => return None,
    })
}

/// Whether the elements' phases are in order: nothing taken down before it was made.
pub(crate) fn check_order(m: &Model, who: &str, e: &Element) -> Result<(), String> {
    let created = value(m, e, CREATED).and_then(|n| index(m, n)).unwrap_or(0);
    if let Some(d) = value(m, e, DEMOLISHED).and_then(|n| index(m, n))
        && d < created
    {
        return Err(format!("{who}: it can't be demolished in a phase before the one it was made in"));
    }
    Ok(())
}

/// Every element holding built-in parameter `key` with value `value`.
#[doc(hidden)]
pub fn holders(m: &Model, key: &str, value: &str) -> Vec<Id> {
    m.elements
        .iter()
        .filter(|(_, e)| e.params().and_then(|p| p.get(key)).and_then(Value::as_str) == Some(value))
        .map(|(id, _)| *id)
        .collect()
}

/// Name the first phase in what was made in it, which holds no value: before another comes first, or it goes.
#[doc(hidden)]
pub fn pin_first(m: &mut Model) {
    let Some(first) = m.phases.first().cloned() else { return };
    // A door or window follows its wall, which is named here too.
    let loose: Vec<Id> = m
        .elements
        .iter()
        .filter(|(_, e)| !matches!(e, Element::Door(_) | Element::Window(_)) && e.params().is_some() && value(m, e, CREATED).is_none())
        .map(|(id, _)| *id)
        .collect();
    for id in loose {
        if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(CREATED.into(), Value::String(first.clone()));
        }
    }
}

/// Whether anything holds phase `name`: an element made or taken down in it (the first phase holds what has no value),
/// a view or a template showing it, or the session.
#[doc(hidden)]
pub fn holds_phase(m: &Model, session: Option<&str>, name: &str) -> bool {
    session == Some(name)
        || !holders(m, CREATED, name).is_empty()
        || !holders(m, DEMOLISHED, name).is_empty()
        || (m.phases.first().map(String::as_str) == Some(name)
            && m.elements.values().any(|e| e.params().is_some() && value(m, e, CREATED).is_none()))
        || m.elements.values().any(|e| crate::graphics::graphics_of_element(e).is_some_and(|g| g.phase.as_deref() == Some(name)))
        || m.view_templates.values().any(|t| t.graphics.phase.as_deref() == Some(name))
}

/// What was made, taken down and shown in phase `from` is now `to`'s (phase `from` is already out of the list or renamed):
/// the elements, the views and the templates. Nothing is taken down before it was made.
#[doc(hidden)]
pub fn move_phase(m: &mut Model, from: &str, to: Option<&str>) {
    let Some(to) = to else { return };
    for key in [CREATED, DEMOLISHED] {
        for id in holders(m, key, from) {
            if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                params.insert(key.into(), Value::String(to.into()));
            }
        }
    }
    for g in m.elements.values_mut().filter_map(crate::graphics::graphics_mut) {
        if g.phase.as_deref() == Some(from) {
            g.phase = Some(to.into());
        }
    }
    for t in m.view_templates.values_mut() {
        if t.graphics.phase.as_deref() == Some(from) {
            t.graphics.phase = Some(to.into());
        }
    }
    rename_in_rules(m, CREATED, from, to);
    rename_in_rules(m, DEMOLISHED, from, to);
    // Taken down where they were made, at the earliest: where a door or window is made is where its wall is.
    let at = |n: &str| m.phases.iter().position(|p| p == n);
    let early: Vec<(Id, String)> = m
        .elements
        .iter()
        .filter_map(|(id, e)| {
            let down = e.params()?.get(DEMOLISHED)?.as_str().and_then(at)?;
            let made = value(m, e, CREATED).and_then(at).unwrap_or(0);
            (down < made).then(|| (*id, m.phases.get(made).cloned().unwrap_or_default()))
        })
        .collect();
    for (id, name) in early {
        if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(DEMOLISHED.into(), Value::String(name));
        }
    }
}

/// The rules of view filters and schedules that say field `field` is `old` say it is `new`.
fn rename_in_rules(m: &mut Model, field: &str, old: &str, new: &str) {
    let swap = |f: &mut crate::schedule::Filter| {
        if f.field == field && f.value.as_str() == Some(old) {
            f.value = Value::String(new.into());
        }
    };
    m.view_filters.values_mut().flat_map(|v| v.rules.iter_mut()).for_each(swap);
    for e in m.elements.values_mut() {
        if let Element::Schedule(sc) = e {
            sc.filters.iter_mut().for_each(swap);
        }
    }
}

/// Option `option` of set `set` is gone: no view or template chooses it.
#[doc(hidden)]
pub fn forget_option(m: &mut Model, set: &str, option: &str) {
    let forget = |g: &mut Graphics| {
        if g.options.get(set).is_some_and(|o| o == option) {
            g.options.remove(set);
        }
    };
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(forget);
    m.view_templates.values_mut().for_each(|t| forget(&mut t.graphics));
}

/// Option `old` of set `set` is `new` now: in the set, in the elements in it and in the views that choose it.
#[doc(hidden)]
pub fn rename_option(m: &mut Model, set: &str, old: &str, new: &str) {
    m.library.renamed(&crate::library::Kind::DesignOption, &format!("{set}: {old}"), &format!("{set}: {new}"));
    if let Some(s) = m.option_sets.get_mut(set) {
        for o in s.options.iter_mut().filter(|o| *o == old) {
            *o = new.into();
        }
        if s.primary == old {
            s.primary = new.into();
        }
    }
    for id in holders(m, OPTION, &format!("{set}: {old}")) {
        if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
            params.insert(OPTION.into(), Value::String(format!("{set}: {new}")));
        }
    }
    rename_in_rules(m, OPTION, &format!("{set}: {old}"), &format!("{set}: {new}"));
    let choose = |g: &mut Graphics| {
        if let Some(o) = g.options.get_mut(set).filter(|o| *o == old) {
            *o = new.into();
        }
    };
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(choose);
    m.view_templates.values_mut().for_each(|t| choose(&mut t.graphics));
}

/// Set `old` is `new` now: its options, the elements in them and the views that choose among them.
#[doc(hidden)]
pub fn rename_set(m: &mut Model, old: &str, new: &str) {
    m.library.set_renamed(old, new);
    let Some(s) = m.option_sets.remove(old) else { return };
    for o in &s.options {
        for id in holders(m, OPTION, &format!("{old}: {o}")) {
            if let Some(params) = m.elements.get_mut(id).and_then(Element::params_mut) {
                params.insert(OPTION.into(), Value::String(format!("{new}: {o}")));
            }
        }
    }
    for o in &s.options {
        rename_in_rules(m, OPTION, &format!("{old}: {o}"), &format!("{new}: {o}"));
    }
    m.option_sets.insert(new.into(), s);
    let choose = |g: &mut Graphics| {
        if let Some(o) = g.options.remove(old) {
            g.options.insert(new.into(), o);
        }
    };
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(choose);
    m.view_templates.values_mut().for_each(|t| choose(&mut t.graphics));
}

/// Set `set` is gone: no view chooses among its options.
#[doc(hidden)]
pub fn forget_set(m: &mut Model, set: &str) {
    m.elements.values_mut().filter_map(crate::graphics::graphics_mut).for_each(|g| {
        g.options.remove(set);
    });
    m.view_templates.values_mut().for_each(|t| {
        t.graphics.options.remove(set);
    });
}

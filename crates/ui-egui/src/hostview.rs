//! What the person sees, for the page that hosts the window (`host.rs`), and for its agent: the selection, the active view and
//! what part of it is on screen. The page reads it ([`BimApp::host_context`]) when [`BimApp::host_context_key`] says it
//! changed, and the page's agent can show the person what it means: select elements ([`BimApp::host_select`]) and open a view
//! ([`BimApp::host_open_view`]). Both only move the window: no command runs and nothing in the model changes.

use serde_json::{Value, json};

use bimcraft_core::inquiry;
use bimcraft_core::model::{Element, Id};

use crate::{BimApp, Sheet};

/// How the selection is told in full: past this many elements the list stops and `count` says how many there are.
pub const MAX_LISTED: usize = 500;

/// What changes when what the person sees does: the selection, the view, where it looks. Compared a frame to the next; the
/// camera is rounded so that a hand holding still is no change.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContextKey {
    selection: Vec<Id>,
    sheet: Option<Sheet>,
    three: Option<Id>,
    camera: Vec<i64>,
}

/// A view the agent names, found.
enum Target {
    Sheet(Sheet),
    /// A 3D view: a camera view by id, or none for the default `{3D}`.
    Three(Option<Id>),
}

impl BimApp {
    /// A compact stand-in for what the person sees now, for noticing that it changed.
    pub fn host_context_key(&self) -> ContextKey {
        let round = |v: f64| if v.is_finite() { v.round() as i64 } else { 0 };
        let camera = match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) | Sheet::Section(key) => {
                self.canvas.view(key).map_or_else(Vec::new, |v| vec![round(v.cx), round(v.cy), round(v.scale.ln() * 100.0)])
            }
            Sheet::View3D => self.orbit.camera.map_or_else(Vec::new, |c| {
                vec![
                    round(c.azimuth * 5.0),
                    round(c.elevation * 5.0),
                    round(c.target[0]),
                    round(c.target[1]),
                    round(c.target[2]),
                    round(c.scale.ln() * 100.0),
                ]
            }),
            _ => Vec::new(),
        };
        ContextKey {
            selection: self.selection.clone(),
            sheet: Some(self.sheet),
            three: matches!(self.sheet, Sheet::View3D).then(|| self.active_3d()).flatten(),
            camera,
        }
    }

    /// What the person sees, as the page's agent reads it:
    /// `{selection: [{id, guid, category, name}], count, view: {id?, name, kind, number?, level?: {id, name}, ceiling?},
    /// camera?}`. The camera of a plan, a section or an elevation is the part of it on screen, `visible: {min, max}` in the
    /// view's own millimetres (a plan's: x east, y north) with its `scale` in pixels per millimetre and its `centre`; the
    /// 3D view's is `{azimuth, elevation, target, scale, lens?}`. A camera's `kind` is its view's (`view.kind`). `selection` lists the first [`MAX_LISTED`] elements picked.
    pub fn host_context(&self) -> Value {
        let m = &self.session.model;
        // What is selected and still there: an element deleted since is no part of the selection the person sees.
        let mut names = self.selection.iter().filter_map(|id| inquiry::names(m, *id));
        let selection: Vec<Value> = names
            .by_ref()
            .take(MAX_LISTED)
            .map(|n| json!({"id": n.id, "guid": n.guid.to_string(), "category": n.category, "name": n.name}))
            .collect();
        let count = selection.len() + names.count();
        json!({
            "selection": selection,
            "count": count,
            "view": self.host_view(),
            "camera": self.host_camera(),
        })
    }

    /// The active view: its `id` (none for a standard schedule), its `name` as the Project Browser has it, what `kind` of
    /// view it is, a sheet's `number`, and for a plan the `level` it is of.
    fn host_view(&self) -> Value {
        let m = &self.session.model;
        let (kind, id) = self.host_view_kind();
        let mut v = json!({"id": id, "name": self.view_name(self.sheet), "kind": kind});
        if let Sheet::Paper(k) = self.sheet
            && let Some(s) = m.sheet(k)
        {
            v["number"] = json!(s.number);
            v["name"] = json!(s.name);
        }
        if let Some(level) = self.plan_level() {
            v["level"] = json!({"id": level, "name": m.view_name(level)});
        }
        v
    }

    /// What kind of view is active, and its id (none for a standard schedule).
    fn host_view_kind(&self) -> (&'static str, Option<Id>) {
        let m = &self.session.model;
        match self.sheet {
            Sheet::Plan(k) => ("floor plan", Some(k)),
            Sheet::Ceiling(k) => ("ceiling plan", Some(k)),
            Sheet::View3D => ("3D view", self.active_3d()),
            Sheet::Section(k) if matches!(m.elements.get(k), Some(Element::Elevation(_))) => ("elevation", Some(k)),
            Sheet::Section(k) => ("section", Some(k)),
            Sheet::Paper(k) => ("sheet", Some(k)),
            Sheet::Schedule(_) => ("schedule", None),
            Sheet::Table(k) => ("schedule", Some(k)),
        }
    }

    /// Where the active view looks: see [`BimApp::host_context`]. Null for a view with no camera (a sheet, a schedule).
    fn host_camera(&self) -> Value {
        match self.sheet {
            Sheet::Plan(key) | Sheet::Ceiling(key) | Sheet::Section(key) => match self.canvas.view(key) {
                Some(v) => {
                    let (lo, hi) = (v.mm((0.0, v.height as f32)), v.mm((v.width as f32, 0.0)));
                    json!({
                        "kind": self.host_view_kind().0,
                        "scale": v.scale,
                        "centre": [v.cx.round(), v.cy.round()],
                        "visible": {"min": [lo.0.round(), lo.1.round()], "max": [hi.0.round(), hi.1.round()]},
                    })
                }
                None => Value::Null,
            },
            Sheet::View3D => match self.orbit.camera {
                Some(c) => json!({
                    "kind": self.host_view_kind().0,
                    "azimuth": c.azimuth,
                    "elevation": c.elevation,
                    "target": [c.target[0].round(), c.target[1].round(), c.target[2].round()],
                    "scale": c.scale,
                    "lens": c.lens,
                }),
                None => Value::Null,
            },
            _ => Value::Null,
        }
    }

    /// Select what `text` names (ids, GUIDs or IFC GlobalIds, space or comma apart), as the person picked them: in place of
    /// what was selected, or added to it with `add`. With `show`, the plan that shows them opens, zoomed to them. Empty text
    /// clears the selection. Replies `{selected, ids, missing}`.
    pub fn host_select(&mut self, text: &str, add: bool, show: bool) -> Value {
        if text.trim().is_empty() {
            if !add {
                self.selection.clear();
            }
            return json!({"selected": self.selection.len(), "ids": self.selection, "missing": Vec::<String>::new()});
        }
        // Answered from what was done: what was selected, and what named no element (what is in the library is no element).
        let found = self.select_named(text, add, show);
        let m = &self.session.model;
        let library = found.library.iter().filter_map(|id| inquiry::names(m, *id)).map(|n| format!("{} ({})", n.name, n.id));
        let missing: Vec<String> = found.missing.iter().cloned().chain(library).collect();
        json!({"selected": self.selection.len(), "ids": found.ids, "missing": missing})
    }

    /// Open the view `text` names, as the person opens it from the Project Browser: a level's name (its floor plan; with
    /// `ceiling`, its ceiling plan), a plan view's, section's, elevation's or schedule's name, a sheet's number or name, `3D`,
    /// or any of those's id or GUID. The reply is the context that follows, or the views there are, in the refusal.
    pub fn host_open_view(&mut self, text: &str, ceiling: bool) -> Result<Value, String> {
        let Some(target) = self.view_target(text.trim(), ceiling) else {
            let names: Vec<String> =
                self.session.model.views().iter().filter_map(|id| self.session.model.view_name(*id).map(String::from)).collect();
            return Err(format!("No view is named `{}`; the views are {}, `3D`, and the sheets by number", text.trim(), names.join(", ")));
        };
        match target {
            Target::Three(id) => self.open_3d(id),
            Target::Sheet(sheet) => self.open_sheet(sheet),
        }
        Ok(self.host_context())
    }

    /// The view `text` names: an id, GUID or IFC GlobalId, else a name (a case apart), a sheet's number, or `3D`.
    fn view_target(&self, text: &str, ceiling: bool) -> Option<Target> {
        let m = &self.session.model;
        let plan = |id: Id| Target::Sheet(if ceiling { Sheet::Ceiling(id) } else { Sheet::Plan(id) });
        let of = |id: Id| match m.elements.get(id)? {
            Element::Level(_) | Element::PlanView(_) => Some(plan(id)),
            Element::Section(_) | Element::Elevation(_) => Some(Target::Sheet(Sheet::Section(id))),
            Element::Sheet(_) => Some(Target::Sheet(Sheet::Paper(id))),
            Element::Schedule(_) => Some(Target::Sheet(Sheet::Table(id))),
            Element::View3D(_) => Some(Target::Three(Some(id))),
            _ => None,
        };
        if let Some(target) = inquiry::one(m, text).and_then(of) {
            return Some(target);
        }
        if text.eq_ignore_ascii_case("3d") || text.eq_ignore_ascii_case("{3d}") {
            return Some(Target::Three(None));
        }
        let by_name = m.views().into_iter().find(|id| m.view_name(*id).is_some_and(|n| n.eq_ignore_ascii_case(text)));
        if let Some(target) = by_name.and_then(of) {
            return Some(target);
        }
        let sheet = m.sheet_numbered(text).or_else(|| {
            m.sheets().into_iter().find(|id| m.sheet(*id).is_some_and(|s| format!("{} - {}", s.number, s.name).eq_ignore_ascii_case(text)))
        });
        sheet.map(|id| Target::Sheet(Sheet::Paper(id)))
    }
}

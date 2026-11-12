//! The window inside a page that keeps the project: Arqiflow's editor, which shows this interface on a canvas
//! (`apps/bimcraft-web`). The page keeps the project on its server, so the window has no files of its own: no start page,
//! no New, Open, Save As or recent files, no updater, plugins or control port. Save asks the page
//! ([`BimApp::take_save_request`]); the page puts in a newer project when its agent changed it ([`BimApp::replace_model`]) and
//! runs its agent's commands on the open one ([`BimApp::execute_for_agent`]); it dresses the window in its own colours
//! ([`BimApp::set_host_look`]).
//!
//! Its files are the page's too. Open, Link, Import CAD, Point Cloud and the rest ask the page for a file
//! ([`BimApp::file_request`]): the page gives its bytes under a name ([`BimApp::give_file`]) and answers with the name
//! ([`BimApp::answer_file_request`]). Export asks it where to save, the same way, and what is written waits for the page to
//! take it ([`BimApp::take_written`]). A model the project links is read from the page too: [`BimApp::files_wanted`] names
//! the files its links read that are not read yet, and [`BimApp::reload_links`] reads them once the page gave them.
//!
//! The page checks the model against its firm's rules and its client's brief: it gives the checks to the Checks palette
//! ([`BimApp::set_checks`], `checks.rs`), open from the start, and shows a rule the person asks for
//! ([`BimApp::take_rule_request`]).
//!
//! The page renders too, with its own renderer (Arqiflow's Render dialog, through Arqina): the Project Browser's Render… on a
//! 3D view or camera, and the View tab's Render (Revit's View > Presentation > Render), ask the page to render that view
//! ([`BimApp::take_render_request`]). The window draws nothing of its own for it; a window no page hosts offers neither.

use std::collections::BTreeSet;
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::sync::{Changes, Merged};
use egui::{Context, ThemePreference};
use serde_json::Value;

pub use crate::checks::Waive;
pub use crate::dialog::FileRequest;
pub use crate::style::{Controls, Skin};
use crate::{BimApp, Pending};

/// A 3D view the person asked the page to render: its name (`{3D}` for the default), its id and GUID (none for a default
/// `{3D}` not made yet), and whether it is a camera's (an eye and a target of its own).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderRequest {
    pub view: String,
    pub id: Option<Id>,
    pub guid: Option<Guid>,
    pub camera: bool,
}

/// The hosting page's look: light or dark, and its colours, `[r, g, b]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostLook {
    pub dark: bool,
    /// The cards (the palettes, the bars), their lettering, and the colour of what is active, picked and in focus.
    pub panel: Option<[u8; 3]>,
    pub text: Option<[u8; 3]>,
    pub accent: Option<[u8; 3]>,
    /// The paper the views are drawn on.
    pub canvas: Option<[u8; 3]>,
    /// The top bar, the borders, the dimmer lettering, the bands and the fields.
    pub skin: Skin,
    /// The bars and palettes flush to the edges (Compact) rather than floating cards; none keeps the user's.
    pub compact: Option<bool>,
}

impl BimApp {
    /// A window for a hosting page, on `session`'s project: straight onto its first floor plan.
    pub fn hosted_on(session: Session) -> BimApp {
        let mut app = BimApp::new(session);
        app.hosted = true;
        // The page checks the model against the firm's rules and the brief: the Checks palette is open from the start.
        app.docking = app.first_docking();
        // What it writes (Export) is the page's to keep, not a file on this computer's disk.
        app.session.files.keep_writes = true;
        app
    }

    /// Whether the window is inside a page that keeps the project.
    pub fn hosted(&self) -> bool {
        self.hosted
    }

    /// Save, as the page does it: asked for here, taken by the page after the frame. What was to follow a save (closing,
    /// opening another) a hosted window never asks for.
    pub(crate) fn ask_host_to_save(&mut self, ctx: &Context, _then: Option<Pending>) {
        self.save_asked = true;
        ctx.request_repaint();
    }

    /// Whether Save was asked for since the last call: Ctrl+S, the quick access bar's Save, File > Save.
    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.save_asked)
    }

    /// Whether the window offers Render: a page hosts it, and renders with its own renderer.
    pub(crate) fn offers_render(&self) -> bool {
        self.hosted
    }

    /// Render, as the page does it: the 3D view `view` (the default `{3D}` when none, made or not) asked for here, taken by
    /// the page after the frame, which opens its Render dialog for it.
    pub(crate) fn ask_host_to_render(&mut self, ctx: &Context, view: Option<Id>) {
        let m = &self.session.model;
        let view = view.or_else(|| m.default_3d());
        let held = view.and_then(|id| match m.elements.get(id) {
            Some(Element::View3D(v)) => Some((id, v.name.clone(), v.camera.is_some())),
            _ => None,
        });
        self.render_asked = Some(match held {
            Some((id, name, camera)) => RenderRequest { view: name, id: Some(id), guid: m.elements.guid(id), camera },
            None => RenderRequest { view: "{3D}".into(), id: None, guid: None, camera: false },
        });
        ctx.request_repaint();
    }

    /// The 3D view the person asked to render since the last call (the Project Browser's Render…, the View tab's Render): the
    /// page opens its Render dialog for it.
    pub fn take_render_request(&mut self) -> Option<RenderRequest> {
        self.render_asked.take()
    }

    /// The model the page saved: it is no longer unsaved, unless it changed since.
    pub fn mark_saved(&mut self, model: &Arc<Model>) {
        self.saved = Arc::clone(model);
    }

    /// Put `model` in place of the project, as the page has it now (its agent changed it): the history goes, as with Open, and
    /// the views open stay open where the new model still has them.
    pub fn replace_model(&mut self, model: Model) {
        let (views, sheet, front) = (self.views.clone(), self.sheet, self.front);
        let same = model.uid == self.session.model.uid;
        self.session.reset(model, None);
        if same {
            self.saved = Arc::clone(&self.session.model);
        } else {
            self.reopened();
            (self.views, self.sheet, self.front) = (views, sheet, front);
        }
        self.tidy();
    }

    /// Run a command for the page's agent on the open project, as a button does: one undo step. A refusal is the agent's to
    /// read, so it is returned and not put in front of the person.
    pub fn execute_for_agent(&mut self, name: &str, params: &Value) -> Result<Value, String> {
        self.made_in();
        let result = self.session.execute(name, params);
        self.active_option = self.session.option.clone();
        if self.session.model.uid != self.project {
            self.reopened();
        }
        self.tidy();
        result
    }

    /// Gives the window the file `name` (a path as the page names its files, "files/<id>/Plan.dxf"), its `bytes`, to read
    /// as a file at that path.
    pub fn give_file(&mut self, name: &str, bytes: Vec<u8>) {
        self.session.files.give(name, bytes);
    }

    /// What was written for the page (Export) since the last call: each file's name and bytes.
    pub fn take_written(&mut self) -> Vec<(String, Vec<u8>)> {
        self.session.files.take_written()
    }

    /// Whether something was written that the page has not taken yet.
    pub fn has_written(&self) -> bool {
        self.session.files.has_written()
    }

    /// The files the project's links read that are not read: a model reopened from its text (the page's `load`) holds its
    /// links and not what they read. A link made from a project's text (the site) is the page's to make again, and is not
    /// among them.
    pub fn files_wanted(&self) -> Vec<String> {
        let m = &self.session.model;
        let mut out: Vec<String> = m
            .elements
            .values()
            .filter_map(|e| match e {
                Element::Link(l) if !l.path.starts_with("text:") && !m.loaded.contains_key(&l.path) => Some(l.path.clone()),
                _ => None,
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Reads the files of the project's links again, from those the page gave: `{reloaded, failed}`. Reading them is no
    /// change of the person's (Reload Links is no undo step), and a saved model stays saved.
    pub fn reload_links(&mut self) -> Result<Value, String> {
        let was_saved = !self.dirty();
        let reply = self.session.execute("reload", &serde_json::json!({}));
        if was_saved {
            self.saved = Arc::clone(&self.session.model);
        }
        self.tidy();
        reply
    }

    /// Take in what others editing the same project changed (`bimcraft_core::sync`): the model checked, what would break it
    /// refused and named, and the elements of `keep` (this person's changes the page has not sent yet) left as they are.
    /// Undo and redo stay this person's: they take back and put back their own commands, not the others' changes.
    pub fn merge_remote(&mut self, c: &Changes, keep: &BTreeSet<Guid>) -> Result<Merged, String> {
        let merged = self.session.merge(c, keep)?;
        self.tidy();
        Ok(merged)
    }

    /// Letter the window in the page's own fonts from the next frame: its sans (variable in weight or not: semibold titles
    /// take its 600) and its monospace, each the bytes of a TrueType or OpenType file. IBM Plex stays behind them for what
    /// they haven't; none puts Plex back.
    pub fn set_host_fonts(&mut self, sans: Option<Vec<u8>>, mono: Option<Vec<u8>>) {
        let fonts = crate::style::PageFonts { sans: sans.map(Arc::new), mono: mono.map(Arc::new) };
        let fonts = (fonts != crate::style::PageFonts::default()).then_some(fonts);
        if fonts != self.page_fonts {
            self.page_fonts = fonts;
            self.refont = true;
        }
    }

    /// Wear the page's look from the next frame: its light or dark and the colours it sends (none, BIMCraft's own). What the
    /// person picked for themselves stays: the Light Canvas is theirs, and the layout changes only when the page asks for a
    /// different one than it last did, so a page that sends its look again with each theme change wipes no choice.
    pub fn set_host_look(&mut self, look: HostLook) {
        self.theme_wanted = Some(if look.dark { ThemePreference::Dark } else { ThemePreference::Light });
        (self.panel_colour, self.text_colour, self.accent, self.canvas_colour) = (look.panel, look.text, look.accent, look.canvas);
        self.skin = look.skin;
        if let Some(compact) = look.compact
            && self.host_compact != Some(compact)
        {
            self.host_compact = Some(compact);
            self.compact = compact;
        }
    }
}

/// Whether a key pressed while the window's canvas has the keyboard is the editor's, so the page must stop the browser acting on
/// it too: `key` is the browser's `KeyboardEvent.key` ("Tab", " ", "Delete", "F8", "d"), `command` whether Ctrl or Cmd is down.
/// With any modifiers: Tab, Space, Backspace, Delete, the arrows (Alt with one is the browser's back and forward), Home, End,
/// Page Up and Page Down, which would move focus, scroll the page or go back, and the function keys the editor and the browser
/// both use, F1 to F4 and F6 to F10. With Ctrl or Cmd: A (select all), D (bookmark; the editor's Home), K, O, P, S, Y, Z, `,` and 1.
/// Never F5 (reload), F11 (full screen), F12 (the developer tools), Escape, or Ctrl or Cmd with C, V and X: the browser's own
/// copy, paste and cut events carry the clipboard, and the window handles those. The window sees only what its canvas has: a text
/// field of the page's own has its keys.
pub fn editor_takes_key(key: &str, command: bool) -> bool {
    match key {
        "Tab" | " " | "Spacebar" | "Backspace" | "Delete" | "Home" | "End" | "PageUp" | "PageDown" => true,
        "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" => true,
        "F1" | "F2" | "F3" | "F4" | "F6" | "F7" | "F8" | "F9" | "F10" => true,
        k => command && matches!(k.to_ascii_lowercase().as_str(), "a" | "d" | "k" | "o" | "p" | "s" | "y" | "z" | "," | "1"),
    }
}

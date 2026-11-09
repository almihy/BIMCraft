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

use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::model::{Element, Model};
use egui::{Context, ThemePreference};
use serde_json::Value;

pub use crate::dialog::FileRequest;
pub use crate::style::Skin;
use crate::{BimApp, Pending};

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

    /// Wear the page's look from the next frame.
    pub fn set_host_look(&mut self, look: HostLook) {
        self.theme_wanted = Some(if look.dark { ThemePreference::Dark } else { ThemePreference::Light });
        (self.panel_colour, self.text_colour, self.accent, self.canvas_colour) = (look.panel, look.text, look.accent, look.canvas);
        self.skin = look.skin;
        if let Some(compact) = look.compact {
            self.compact = compact;
        }
        // Light lines on a dark drawing, as the page's own drawings are in its dark theme.
        self.light_canvas = false;
    }
}

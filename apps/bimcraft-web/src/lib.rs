//! BIMCraft's own interface (crates/ui-egui) running in a browser page, on a canvas: Arqiflow's editor. The page keeps the
//! project on its server, so the window is hosted (`bimcraft_ui_egui::host`): no files of its own, and Save asks the page.
//!
//! ```js
//! const app = new WebApp();
//! app.set_theme('{"dark": false, "panel": "#ffffff", "accent": "#2b50d8"}');  // before or after the start
//! app.set_fonts(sansTtfBytes, monoTtfBytes);  // the page's lettering, before or after the start
//! await app.start(canvas, projectJsonOrNull, onSave, onChange);
//! app.project();          // the model's JSON, to save
//! app.saved();            // what project() last gave is saved
//! app.rev();              // a counter that goes up when the model changes
//! app.load(projectJson);  // put a newer model in place (the agent changed it on the server)
//! app.execute("wall", '{"points": [[0, 0], [6000, 0]]}');  // a command on the open model, an undo step
//! app.link_site(siteProjectJson);  // the site around the model, linked: halftone, never edited (again: the newer site)
//!
//! // Editing together (bimcraft_core::sync): changes keyed by GUID, to and from the others' copies.
//! app.take_changes();       // what changed here since last taken: '{"elements": {guid: json | null}, "project": {...}}'
//! app.apply_changes(json);  // the others' changes taken in, checked: '{"taken": n, "refused": [...]}'
//! app.history();            // this person's undo history, to keep across a reload (by GUID)
//! app.restore_history(text);  // put back on the project opened again: '{"undo": n, "redo": n}'
//! ```
//!
//! Its files are the page's (`bimcraft_ui_egui::host`). Open, Link, Import CAD, Point Cloud and Export ask the page:
//!
//! ```js
//! app.on_files(onAsk, onWritten, onWanted);
//! // onAsk({id, save, multiple, what, extensions, name, title}): the person wants a file (or, `save`, where to save one).
//! app.give_file("files/doc_1/Plan.dxf", bytes);   // the bytes of a file picked, under a name of the page's
//! app.answer_files(id, ["files/doc_1/Plan.dxf"]); // the names, or null when cancelled; for `save`, the name to save as
//! // onWritten(): what Export wrote is ready
//! app.take_written();     // [{name, bytes}]
//! // onWanted(["files/doc_2/House.ifc"]): the files the model's links read, which the page gives and then
//! app.reload_links();
//! ```
//!
//! The model's checks against the firm's rules and the client's brief are the page's (its server runs them after a save);
//! the Checks palette lists them, flagged first, and asks the page to show a rule:
//!
//! ```js
//! app.set_checks(checksJson);  // GET /api/proposals/{id}/checks's answer; null while they are being made again
//! app.on_rule(ruleId => openRule(ruleId));  // Open Rule: the page shows the rule where the project's rules are kept
//! app.on_waive(ask => waive(JSON.parse(ask)));  // {rule, reason}: accept a flag as it is; {rule, reason: null}: undo
//! app.waive_refused(ruleId, why);  // the server would not: its reason, under the check (else set_checks again)
//! ```
#![cfg(target_arch = "wasm32")]
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::ids::Guid;
use bimcraft_core::model::Model;
use bimcraft_core::sync::{self, Changes};
use bimcraft_ui_egui::BimApp;
use bimcraft_ui_egui::host::{Controls, HostLook, Skin, Waive};
use serde_json::Value;
use wasm_bindgen::prelude::*;

/// The interface and what the page hears from it: a change to the model, Save, and its files.
struct Host {
    app: BimApp,
    /// The model as last seen after a frame, and how many times it has changed since the start.
    seen: Arc<Model>,
    changes: Rc<Cell<u64>>,
    on_save: Option<js_sys::Function>,
    on_change: Option<js_sys::Function>,
    files: Rc<RefCell<FileHooks>>,
    /// Called with a rule's id when the person asks to see it (the Checks palette's Open Rule).
    on_rule: Rc<RefCell<Option<js_sys::Function>>>,
    /// Called with `{rule, reason}` when the person waives a check, `reason` null to take a waiver back.
    on_waive: Rc<RefCell<Option<js_sys::Function>>>,
}

/// What the page hears of files, and what it was last told.
#[derive(Default)]
struct FileHooks {
    on_ask: Option<js_sys::Function>,
    on_written: Option<js_sys::Function>,
    on_wanted: Option<js_sys::Function>,
    /// The ask last told, so each is told once.
    asked: u64,
    /// The files the links want, as last told.
    wanted: Vec<String>,
}

impl Host {
    /// After a frame: tells the page of a new ask for a file, of files written for it, and of files its links want.
    fn tell_files(&mut self) {
        let mut hooks = self.files.borrow_mut();
        if let Some(r) = self.app.file_request()
            && r.id != hooks.asked
        {
            hooks.asked = r.id;
            let ask = serde_json::json!({
                "id": r.id, "save": r.save, "multiple": r.multiple, "what": r.what, "extensions": r.extensions,
                "name": r.name, "title": r.title,
            });
            later_with(hooks.on_ask.clone(), JsValue::from_str(&ask.to_string()));
        }
        if self.app.has_written() {
            later_with(hooks.on_written.clone(), JsValue::UNDEFINED);
        }
        let wanted = self.app.files_wanted();
        if wanted != hooks.wanted {
            hooks.wanted = wanted;
            if !hooks.wanted.is_empty() {
                later_with(hooks.on_wanted.clone(), JsValue::from_str(&Value::from(hooks.wanted.clone()).to_string()));
            }
        }
    }
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.app.ui(ui, frame);
        if !Arc::ptr_eq(&self.seen, &self.app.session.model) {
            self.seen = Arc::clone(&self.app.session.model);
            self.changes.set(self.changes.get() + 1);
            later(self.on_change.clone(), Some(self.changes.get()));
        }
        if self.app.take_save_request() {
            later(self.on_save.clone(), None);
        }
        self.tell_files();
        if let Some(rule) = self.app.take_rule_request() {
            later_with(self.on_rule.borrow().clone(), JsValue::from_str(&rule));
        }
        if let Some(w) = self.app.take_waive_request() {
            let ask = match w {
                Waive::Add { rule, reason } => serde_json::json!({"rule": rule, "reason": reason}),
                Waive::Remove { rule } => serde_json::json!({"rule": rule, "reason": null}),
            };
            later_with(self.on_waive.borrow().clone(), JsValue::from_str(&ask.to_string()));
        }
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(ctx, raw);
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }
}

/// Calls `f` once the frame is over: the page's callback may well ask the interface for its model, which it can't give while
/// it draws.
fn later(f: Option<js_sys::Function>, arg: Option<u64>) {
    let Some(f) = f else { return };
    wasm_bindgen_futures::spawn_local(async move {
        let _ = match arg {
            Some(n) => f.call1(&JsValue::NULL, &JsValue::from_f64(n as f64)),
            None => f.call0(&JsValue::NULL),
        };
    });
}

/// Calls `f` with `arg` once the frame is over, as [`later`] does.
fn later_with(f: Option<js_sys::Function>, arg: JsValue) {
    let Some(f) = f else { return };
    wasm_bindgen_futures::spawn_local(async move {
        let _ = f.call1(&JsValue::NULL, &arg);
    });
}

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// BIMCraft's sample house as a project file, to start on.
#[wasm_bindgen]
pub fn sample_project() -> Result<String, JsValue> {
    let mut session = Session::new();
    session.script(bimcraft_core::sample::SAMPLE).map_err(err)?;
    session.model.to_json().map_err(err)
}

/// The page's fonts: the bytes of its sans and of its monospace.
type PageFonts = (Option<Vec<u8>>, Option<Vec<u8>>);

#[wasm_bindgen]
pub struct WebApp {
    runner: eframe::WebRunner,
    ctx: Rc<RefCell<Option<egui::Context>>>,
    changes: Rc<Cell<u64>>,
    /// The model `project` last gave, which `saved` marks as saved.
    given: RefCell<Option<Arc<Model>>>,
    /// The look asked for, kept for the start when asked before it or while it is under way.
    look: Rc<RefCell<Option<HostLook>>>,
    /// The page's fonts, kept for the start when given before it or while it is under way: its sans and its monospace.
    fonts: Rc<RefCell<PageFonts>>,
    /// What the page hears of files, set before the start or after it.
    files: Rc<RefCell<FileHooks>>,
    /// The model as `take_changes` last gave it, with the others' changes since: what changed here since is this person's,
    /// not sent yet.
    sent: RefCell<Option<Arc<Model>>>,
    /// The checks the page gave, kept for the start when given before it or while it is under way.
    checks: Rc<RefCell<Option<String>>>,
    /// What the page hears when the person asks to see a rule.
    on_rule: Rc<RefCell<Option<js_sys::Function>>>,
    /// What the page hears when the person waives a check, or takes a waiver back.
    on_waive: Rc<RefCell<Option<js_sys::Function>>>,
}

#[wasm_bindgen]
impl WebApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WebApp {
        WebApp {
            runner: eframe::WebRunner::new(),
            ctx: Rc::new(RefCell::new(None)),
            changes: Rc::new(Cell::new(0)),
            given: RefCell::new(None),
            look: Rc::new(RefCell::new(None)),
            fonts: Rc::new(RefCell::new((None, None))),
            files: Rc::new(RefCell::new(FileHooks::default())),
            sent: RefCell::new(None),
            checks: Rc::new(RefCell::new(None)),
            on_rule: Rc::new(RefCell::new(None)),
            on_waive: Rc::new(RefCell::new(None)),
        }
    }

    /// Starts the interface on `canvas`, on `project` (the .bimcraft JSON), or a new project when none, straight onto its first
    /// floor plan. `on_save` is called on Save (Ctrl+S, the app's Save); `on_change` with the new `rev` when the model changes.
    pub async fn start(
        &self,
        canvas: web_sys::HtmlCanvasElement,
        project: Option<String>,
        on_save: Option<js_sys::Function>,
        on_change: Option<js_sys::Function>,
    ) -> Result<(), JsValue> {
        let mut session = Session::new();
        if let Some(text) = project.filter(|t| !t.trim().is_empty()) {
            let (model, _) = Model::read(&text).map_err(err)?;
            session.reset(model, None);
        }
        let app = BimApp::hosted_on(session);
        *self.sent.borrow_mut() = Some(Arc::clone(&app.session.model));
        let mut host = Host {
            seen: Arc::clone(&app.session.model),
            app,
            changes: Rc::clone(&self.changes),
            on_save,
            on_change,
            files: Rc::clone(&self.files),
            on_rule: Rc::clone(&self.on_rule),
            on_waive: Rc::clone(&self.on_waive),
        };
        let ctx = Rc::clone(&self.ctx);
        let (look, fonts, checks) = (Rc::clone(&self.look), Rc::clone(&self.fonts), Rc::clone(&self.checks));
        self.runner
            .start(
                canvas,
                web_options(),
                Box::new(move |cc| {
                    *ctx.borrow_mut() = Some(cc.egui_ctx.clone());
                    // The look and the fonts the page asked for, before the start or while the runner was setting up (until
                    // this runs, the page's set_theme and set_fonts have no window to go to).
                    if let Some(look) = *look.borrow() {
                        host.app.set_host_look(look);
                    }
                    let (sans, mono) = fonts.borrow().clone();
                    host.app.set_host_fonts(sans, mono);
                    if let Some(json) = checks.borrow().as_deref() {
                        let _ = host.app.set_checks(json);
                    }
                    // The 3D view, and the plans' kept triangles, drawn on the card the page draws on (WebGPU; WebGL2 where
                    // there is none, which the 3D view can't use, so it is drawn on the processor there), as the desktop
                    // app does.
                    if let Some(state) = cc.wgpu_render_state.as_ref() {
                        host.app.use_graphics_card(state);
                    }
                    Ok(Box::new(host))
                }),
            )
            .await
    }

    /// The interface, between frames; an error once it has crashed, or before it started.
    fn host(&self) -> Result<std::cell::RefMut<'_, Host>, JsValue> {
        self.runner.app_mut::<Host>().ok_or_else(|| err("the interface is not running"))
    }

    fn repaint(&self) {
        if let Some(ctx) = self.ctx.borrow().as_ref() {
            ctx.request_repaint();
        }
    }

    /// The model as the project file (.bimcraft JSON), to save.
    pub fn project(&self) -> Result<String, JsValue> {
        let model = Arc::clone(&self.host()?.app.session.model);
        let text = model.to_json().map_err(err)?;
        *self.given.borrow_mut() = Some(model);
        Ok(text)
    }

    /// What `project` last gave was saved.
    pub fn saved(&self) -> Result<(), JsValue> {
        let given = self.given.borrow_mut().take();
        if let Some(model) = given {
            self.host()?.app.mark_saved(&model);
        }
        Ok(())
    }

    /// How many times the model has changed since the start: commands, undo and redo, the agent's `execute`; not `load`.
    pub fn rev(&self) -> f64 {
        self.changes.get() as f64
    }

    /// Puts `project` (.bimcraft JSON) in place of the model, keeping the views open that it still has; the undo history goes.
    pub fn load(&self, project: &str) -> Result<(), JsValue> {
        let (model, _) = Model::read(project).map_err(err)?;
        {
            let mut host = self.host()?;
            host.app.replace_model(model);
            host.seen = Arc::clone(&host.app.session.model);
            *self.sent.borrow_mut() = Some(Arc::clone(&host.app.session.model));
        }
        self.repaint();
        Ok(())
    }

    /// Runs command `name` with `params` (JSON) on the open model, as one undo step: its reply as JSON, or its refusal thrown.
    pub fn execute(&self, name: &str, params: &str) -> Result<String, JsValue> {
        let params: Value =
            if params.trim().is_empty() { Value::Object(Default::default()) } else { serde_json::from_str(params).map_err(err)? };
        let reply = self.host()?.app.execute_for_agent(name, &params);
        self.repaint();
        reply.map(|v| v.to_string()).map_err(err)
    }

    /// Shows the site (`project`, an engine project of it, .bimcraft JSON) around the model, as Revit shows a linked site:
    /// halftone in the plans, sections and 3D, snapped to and never edited. It is the link named "Site", made from text;
    /// given again, the newer site takes its place. Its reply as JSON, or its refusal thrown.
    pub fn link_site(&self, project: &str) -> Result<String, JsValue> {
        self.execute("link", &serde_json::json!({"text": project, "name": SITE}).to_string())
    }

    /// What changed in the model since this was last called (or since the start, or `load`), to send to the others editing
    /// it: `{"elements": {guid: json | null}, "project": {key: json | null}}`, by GUID, each element as the file holds it.
    pub fn take_changes(&self) -> Result<String, JsValue> {
        let now = Arc::clone(&self.host()?.app.session.model);
        let sent = self.sent.borrow_mut().replace(Arc::clone(&now));
        let Some(sent) = sent else { return Ok(Changes::default().to_json()) };
        match sync::changes(&sent, &now) {
            Ok(c) => Ok(c.to_json()),
            Err(e) => {
                *self.sent.borrow_mut() = Some(sent);
                Err(err(e))
            }
        }
    }

    /// Takes in what the others changed (`{"elements": {guid: json | null}, "project": {key: json | null}, "complete"?}`;
    /// `complete`: everything the shared model holds, what it doesn't name being removed). The model is checked: what would
    /// break it is left out and named. This person's changes not taken yet stay as they are, to be sent; their undo stays
    /// theirs. Replies `{"taken": n, "refused": [{what, error}]}`.
    pub fn apply_changes(&self, changes: &str) -> Result<String, JsValue> {
        let c = Changes::read(changes).map_err(err)?;
        let merged = {
            let mut host = self.host()?;
            let now = Arc::clone(&host.app.session.model);
            let sent = self.sent.borrow().clone().unwrap_or_else(|| Arc::clone(&now));
            let mine: BTreeSet<Guid> = sync::changes(&sent, &now).map_err(err)?.elements.into_keys().collect();
            let merged = host.app.merge_remote(&c, &mine).map_err(err)?;
            // What was sent takes the same changes, so that what changed here since is still this person's alone.
            let after = sent.with_changes(&merged.taken, &merged.ids).map(Arc::new).unwrap_or_else(|_| Arc::clone(&host.app.session.model));
            *self.sent.borrow_mut() = Some(after);
            merged
        };
        self.repaint();
        Ok(merged.to_json().to_string())
    }

    /// This person's undo history, to keep outside the page so a reload keeps it (`bimcraft_core::sync::History`, JSON):
    /// their own commands' steps, by GUID.
    pub fn history(&self) -> Result<String, JsValue> {
        Ok(self.host()?.app.session.kept_history().map_err(err)?.to_json())
    }

    /// Puts back a history `history` gave, on the same project opened again, once it says what the room says: what the
    /// others changed since is brought to every step, so undo takes back this person's own work alone. The model stays as
    /// it is. Replies `{"undo": n, "redo": n}`, or throws (another project's history).
    pub fn restore_history(&self, history: &str) -> Result<String, JsValue> {
        let h = bimcraft_core::sync::History::read(history).map_err(err)?;
        let (undo, redo) = self.host()?.app.session.restore_history(&h).map_err(err)?;
        self.repaint();
        Ok(serde_json::json!({"undo": undo, "redo": redo}).to_string())
    }

    /// Dresses the interface in the page's look: `{"dark": bool, "compact": bool}` and colours as `"#rrggbb"`: `panel`, `text`,
    /// `accent`, `canvas`, `app_bar`, `app_bar_text`, `border`, `muted`, `bar`, `surface`, `input` (a control's outline), `hover`
    /// (the wash of what is hovered), `context` (the contextual tabs, Revit's green); `"outlined": true` draws the controls as
    /// the page's (outlined, hover a wash), their corners `"radius"` px (6 if not said). Any left out keeps BIMCraft's own.
    pub fn set_theme(&self, theme: &str) -> Result<(), JsValue> {
        let v: Value = serde_json::from_str(theme).map_err(err)?;
        let colour = |key: &str| v.get(key).and_then(Value::as_str).and_then(hex);
        let look = HostLook {
            dark: v.get("dark").and_then(Value::as_bool).unwrap_or(false),
            panel: colour("panel"),
            text: colour("text"),
            accent: colour("accent"),
            canvas: colour("canvas"),
            skin: Skin {
                app_bar: colour("app_bar"),
                app_bar_text: colour("app_bar_text"),
                border: colour("border"),
                muted: colour("muted"),
                bar: colour("bar"),
                surface: colour("surface"),
                input: colour("input"),
                hover: colour("hover"),
                context: colour("context"),
                // Outlined controls and their corners, as the page draws its own: `"outlined": true`, `"radius": 6`.
                controls: v
                    .get("outlined")
                    .and_then(Value::as_bool)
                    .filter(|o| *o)
                    .map(|outlined| Controls { outlined, radius: v.get("radius").and_then(Value::as_u64).map_or(6, |r| r.min(12) as u8) }),
            },
            compact: v.get("compact").and_then(Value::as_bool),
        };
        *self.look.borrow_mut() = Some(look);
        if let Some(mut host) = self.runner.app_mut::<Host>() {
            host.app.set_host_look(look);
        }
        self.repaint();
        Ok(())
    }

    /// Letters the interface in the page's own fonts: `sans` (variable in weight or not) and `mono`, each the bytes of a
    /// TrueType or OpenType file (not WOFF); none keeps BIMCraft's IBM Plex. Before or after the start.
    pub fn set_fonts(&self, sans: Option<Vec<u8>>, mono: Option<Vec<u8>>) {
        *self.fonts.borrow_mut() = (sans.clone(), mono.clone());
        if let Some(mut host) = self.runner.app_mut::<Host>() {
            host.app.set_host_fonts(sans, mono);
        }
        self.repaint();
    }

    /// Gives the Checks palette the model's checks: the answer of Arqiflow's `GET /api/proposals/{id}/checks`
    /// (`{results, integrity}`, and `error` when the page could not check), or null while they are being made again. Before
    /// or after the start; after it, throws what is not checks.
    pub fn set_checks(&self, checks: Option<String>) -> Result<(), JsValue> {
        let json = checks.unwrap_or_default();
        if let Some(mut host) = self.runner.app_mut::<Host>() {
            host.app.set_checks(&json).map_err(err)?;
        }
        *self.checks.borrow_mut() = Some(json);
        self.repaint();
        Ok(())
    }

    /// What the page hears when the person asks to see a rule (the Checks palette's Open Rule): `on_rule(ruleId)`, once the
    /// frame is over.
    pub fn on_rule(&self, on_rule: Option<js_sys::Function>) {
        *self.on_rule.borrow_mut() = on_rule;
    }

    /// What the page hears when the person waives a flagged check (a reason given) or takes a waiver back: `on_waive(askJson)`,
    /// `{"rule": id, "reason": text}` or `{"rule": id, "reason": null}`, once the frame is over. The page asks its server, then
    /// gives the checks again (`set_checks`), or says why it could not (`waive_refused`).
    pub fn on_waive(&self, on_waive: Option<js_sys::Function>) {
        *self.on_waive.borrow_mut() = on_waive;
    }

    /// The page could not make or take back the waiver of `rule`: `why` (its server's reason), shown under the check.
    pub fn waive_refused(&self, rule: &str, why: &str) -> Result<(), JsValue> {
        self.host()?.app.waive_refused(rule, why);
        self.repaint();
        Ok(())
    }

    /// What the page hears of files: `on_ask(askJson)` when the person wants a file, or where to save one; `on_written()` when
    /// Export wrote something for it to take; `on_wanted(namesJson)` with the files the model's links read and have not, after
    /// the start and each `load`. Each is called once the frame is over.
    pub fn on_files(&self, on_ask: Option<js_sys::Function>, on_written: Option<js_sys::Function>, on_wanted: Option<js_sys::Function>) {
        let mut hooks = self.files.borrow_mut();
        (hooks.on_ask, hooks.on_written, hooks.on_wanted) = (on_ask, on_written, on_wanted);
        hooks.wanted.clear();
        drop(hooks);
        self.repaint();
    }

    /// The ask for a file open now, as `on_ask` gave it; null when there is none.
    pub fn file_request(&self) -> Result<Option<String>, JsValue> {
        Ok(self.host()?.app.file_request().map(|r| {
            serde_json::json!({
                "id": r.id, "save": r.save, "multiple": r.multiple, "what": r.what, "extensions": r.extensions,
                "name": r.name, "title": r.title,
            })
            .to_string()
        }))
    }

    /// Gives the interface the file `name` (a name of the page's, ending in its kind: "files/doc_1/Plan.dxf"), its `bytes`,
    /// read as the file at that path would be.
    pub fn give_file(&self, name: &str, bytes: Vec<u8>) -> Result<(), JsValue> {
        self.host()?.app.give_file(name, bytes);
        Ok(())
    }

    /// Answers ask `id`: `names` (JSON, a list of the names given), or null when the person cancelled. For a `save`, the one
    /// name to save as; what is written comes to `on_written`.
    pub fn answer_files(&self, id: f64, names: Option<String>) -> Result<(), JsValue> {
        let names: Option<Vec<String>> = match names.filter(|n| !n.trim().is_empty() && n.trim() != "null") {
            Some(text) => Some(serde_json::from_str(&text).map_err(err)?),
            None => None,
        };
        self.host()?.app.answer_file_request(id as u64, names);
        self.repaint();
        Ok(())
    }

    /// What was written for the page since the last call: `[{name, bytes}]`, `bytes` a Uint8Array.
    pub fn take_written(&self) -> Result<js_sys::Array, JsValue> {
        let out = js_sys::Array::new();
        for (name, bytes) in self.host()?.app.take_written() {
            let o = js_sys::Object::new();
            js_sys::Reflect::set(&o, &"name".into(), &JsValue::from_str(&name))?;
            js_sys::Reflect::set(&o, &"bytes".into(), &js_sys::Uint8Array::from(bytes.as_slice()))?;
            out.push(&o);
        }
        Ok(out)
    }

    /// The files the model's links read and have not (JSON, a list of names): the page gives them, then `reload_links`.
    pub fn files_wanted(&self) -> Result<String, JsValue> {
        Ok(Value::from(self.host()?.app.files_wanted()).to_string())
    }

    /// Reads the links' files again, from those given: `{reloaded, failed}` as JSON. No change of the person's.
    pub fn reload_links(&self) -> Result<String, JsValue> {
        let reply = {
            let mut host = self.host()?;
            let reply = host.app.reload_links();
            host.seen = Arc::clone(&host.app.session.model);
            reply
        };
        self.repaint();
        reply.map(|v| v.to_string()).map_err(err)
    }

    /// Whether the interface crashed: the page then starts it again from the last saved model.
    pub fn has_panicked(&self) -> bool {
        self.runner.has_panicked()
    }

    /// Stops the interface and lets go of its canvas.
    pub fn destroy(&self) {
        self.runner.destroy();
    }
}

impl Default for WebApp {
    fn default() -> WebApp {
        WebApp::new()
    }
}

/// eframe's web runner on wgpu, asking the card for buffers as big as it has (the desktop app's `limits_for`): the 3D view
/// draws models of any size.
fn web_options() -> eframe::WebOptions {
    let mut options = eframe::WebOptions::default();
    if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
        let asked = Arc::clone(&setup.device_descriptor);
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = asked(adapter);
            descriptor.required_limits = bimcraft_ui_egui::limits_for(adapter, descriptor.required_limits);
            descriptor
        });
    }
    options
}

/// The name of the link `WebApp::link_site` makes.
const SITE: &str = "Site";

/// `#rrggbb` as `[r, g, b]`.
fn hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().strip_prefix('#')?;
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

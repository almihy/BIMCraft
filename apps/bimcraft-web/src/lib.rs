//! BIMCraft's own interface (crates/ui-egui) running in a browser page, on a canvas: Arqiflow's editor. The page keeps the
//! project on its server, so the window is hosted (`bimcraft_ui_egui::host`): no files of its own, and Save asks the page.
//!
//! ```js
//! const app = new WebApp();
//! app.set_theme('{"dark": false, "panel": "#ffffff", "accent": "#2b50d8"}');  // before or after the start
//! await app.start(canvas, projectJsonOrNull, onSave, onChange);
//! app.project();          // the model's JSON, to save
//! app.saved();            // what project() last gave is saved
//! app.rev();              // a counter that goes up when the model changes
//! app.load(projectJson);  // put a newer model in place (the agent changed it on the server)
//! app.execute("wall", '{"points": [[0, 0], [6000, 0]]}');  // a command on the open model, an undo step
//! app.link_site(siteProjectJson);  // the site around the model, linked: halftone, never edited (again: the newer site)
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
#![cfg(target_arch = "wasm32")]
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::model::Model;
use bimcraft_ui_egui::BimApp;
use bimcraft_ui_egui::host::{HostLook, Skin};
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

#[wasm_bindgen]
pub struct WebApp {
    runner: eframe::WebRunner,
    ctx: Rc<RefCell<Option<egui::Context>>>,
    changes: Rc<Cell<u64>>,
    /// The model `project` last gave, which `saved` marks as saved.
    given: RefCell<Option<Arc<Model>>>,
    /// The look asked for, kept for the start when asked before it.
    look: RefCell<Option<HostLook>>,
    /// What the page hears of files, set before the start or after it.
    files: Rc<RefCell<FileHooks>>,
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
            look: RefCell::new(None),
            files: Rc::new(RefCell::new(FileHooks::default())),
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
        let mut app = BimApp::hosted_on(session);
        if let Some(look) = *self.look.borrow() {
            app.set_host_look(look);
        }
        let host = Host {
            seen: Arc::clone(&app.session.model),
            app,
            changes: Rc::clone(&self.changes),
            on_save,
            on_change,
            files: Rc::clone(&self.files),
        };
        let ctx = Rc::clone(&self.ctx);
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    *ctx.borrow_mut() = Some(cc.egui_ctx.clone());
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

    /// Dresses the interface in the page's look: `{"dark": bool, "compact": bool}` and colours as `"#rrggbb"`: `panel`, `text`,
    /// `accent`, `canvas`, `app_bar`, `app_bar_text`, `border`, `muted`, `bar`, `surface`. Any left out keeps BIMCraft's own.
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

//! The session every front end drives: one model, its undo history and the command registry.
//! Every user-visible action is a [`Command`]; the CLI, scripts and MCP all reach the same ones.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{Element, Id, Model};
use crate::warnings::{Review, Warning};

pub type Run = fn(&mut Session, &Value) -> Result<Value, String>;

/// How a command from outside the binary runs: a plugin's function, which needs the instance it lives in.
/// The core knows nothing of WASM; it only keeps the closure a plugin crate hands it.
pub type ExternalRun = Arc<dyn Fn(&mut Session, &Value) -> Result<Value, String> + Send + Sync>;

/// A command registered at run time, by a plugin: the same shape as [`Command`], with the texts owned,
/// and a closure in place of the static function.
#[derive(Clone)]
pub struct ExternalCommand {
    pub id: String,
    pub label: String,
    pub aliases: Vec<String>,
    pub params: String,
    pub menu: Option<String>,
    /// The plugin it belongs to, as the catalog marks it.
    pub plugin: String,
    pub run: ExternalRun,
}

impl ExternalCommand {
    /// Whether `name` is this command's id or one of its aliases, in any case.
    pub fn matches(&self, name: &str) -> bool {
        self.id.eq_ignore_ascii_case(name) || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }
}

pub struct Command {
    /// Lower-case id, after Revit's tool name.
    pub id: &'static str,
    pub label: &'static str,
    /// Typed shortcuts, Revit style (WA, DR, WN…).
    pub aliases: &'static [&'static str],
    /// The JSON parameters, documented for agents.
    pub params: &'static str,
    pub run: Run,
    /// Whether a change it makes is an undo step.
    pub undoable: bool,
    /// Where the command sits in the ribbon: `"Manage > Family Editor"`. `None` for what has no
    /// place in the interface (a shortcut, or a command no menu reaches) — the field is always
    /// read, filled in as the catalog learns the interface.
    pub menu: Option<&'static str>,
    /// Whether it reads or writes files, and so needs a plugin's `files` permission. A command that only
    /// touches the disk with some parameters says [`Files::Conditional`] here and decides in
    /// [`touches_files`].
    pub files: Files,
}

/// What a command does with the disk. A plugin call may only reach the disk when its plugin is given the
/// `files` permission, so every command that does is marked here, next to its parameters, rather than in a
/// list somewhere else that a new command would forget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Files {
    /// It never touches the disk.
    None,
    /// It reads files.
    Read,
    /// It writes, moves or replaces files.
    Write,
    /// It may: [`touches_files`] reads the parameters to say which.
    Conditional,
}

impl Files {
    /// The catalog's word for it, or `None` for a command that never touches the disk.
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Files::None => None,
            Files::Read => Some("read"),
            Files::Write => Some("write"),
            Files::Conditional => Some("conditional"),
        }
    }
}

impl Command {
    /// The same command, reached from a menu: `edit("family", …).menu("Manage > Family Editor")`.
    pub const fn menu(self, menu: &'static str) -> Command {
        Command {
            id: self.id,
            label: self.label,
            aliases: self.aliases,
            params: self.params,
            run: self.run,
            undoable: self.undoable,
            menu: Some(menu),
            files: self.files,
        }
    }
}

pub const UNDO_LIMIT: usize = 200;

/// An undo (or redo) step: the command it was, and the elements it made and took away, by category, each category
/// counted in the order of its first element's id, so a list can name it: Create Walls, Delete Door.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub label: String,
    pub made: Vec<(&'static str, usize)>,
    pub removed: Vec<(&'static str, usize)>,
    /// Whether the model the step goes back to passed [`Model::check`]: going back to it needs no check.
    pub(crate) checked: bool,
    /// The 3D views the step itself turned (their orientation, where they were left, a camera's eye and target): undo and
    /// redo put back only theirs. Every other view stays where it was turned since, as navigating is no undo step in Revit.
    pub(crate) turned: Vec<Id>,
}

impl Step {
    /// Without a look at every element: what's new has an id past every one `before` had (ids only go up), and only when
    /// the counts say something went is `before` searched for what.
    pub(crate) fn new(label: &str, before: &Model, after: &Model) -> Step {
        let tally = |elements: &mut dyn Iterator<Item = &Element>| {
            let mut counts: Vec<(&'static str, usize)> = Vec::new();
            for e in elements {
                match counts.iter_mut().find(|(c, _)| *c == e.category()) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((e.category(), 1)),
                }
            }
            counts
        };
        let first_new = before.elements.keys().next_back().map_or(0, |id| id + 1).max(before.next_id);
        let made = tally(&mut after.elements.range(first_new..).map(|(_, e)| e));
        let kept = after.elements.len() - made.iter().map(|(_, n)| n).sum::<usize>();
        let removed = if before.elements.len() > kept {
            tally(&mut before.elements.iter().filter(|(id, _)| !after.elements.contains_key(**id)).map(|(_, e)| e))
        } else {
            Vec::new()
        };
        Step { label: label.to_string(), made, removed, checked: false, turned: turned(before, after, &[]) }
    }
}

/// The 3D views `after` stands elsewhere than `before` (see [`View3D::navigation`](crate::model::View3D::navigation)), or
/// that only one of them has, but for those in `skip`: by what changed between the two, so a step with no view in it looks
/// at none.
fn turned(before: &Model, after: &Model, skip: &[Id]) -> Vec<Id> {
    let ids: Vec<Id> = match after.changed_since(before) {
        crate::ids::Changed::Ids(ids) => ids.into_iter().collect(),
        crate::ids::Changed::All => before.elements.keys().chain(after.elements.keys()).copied().collect(),
    };
    let mut out: Vec<Id> = ids
        .into_iter()
        .filter(|id| !skip.contains(id))
        .filter(|id| match (before.elements.get(*id), after.elements.get(*id)) {
            (Some(Element::View3D(a)), Some(Element::View3D(b))) => a.navigation() != b.navigation(),
            (Some(Element::View3D(_)), None) | (None, Some(Element::View3D(_))) => true,
            _ => false,
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// `model`, gone back to by undo or redo from `left`, with every 3D view but those the step turned, made or took away itself
/// (`own`) standing where it stands in `left`: where a person navigated since stays, and so does a view navigating made (the
/// default `{3D}`, made when it is first turned), so long as the model still passes with it.
fn keep_navigation(model: &mut Arc<Model>, left: &Model, own: &[Id]) {
    let moved: Vec<(Id, Option<crate::ids::Guid>, crate::model::View3D)> = turned(model, left, own)
        .into_iter()
        .filter_map(|id| match left.elements.get(id) {
            Some(Element::View3D(v)) => Some((id, model.elements.get(id).is_none().then(|| left.elements.guid(id)).flatten(), v.clone())),
            _ => None,
        })
        .collect();
    if moved.is_empty() {
        return;
    }
    let mut kept = Arc::clone(model);
    let m = Arc::make_mut(&mut kept);
    let mut made = false;
    for (id, guid, there) in moved {
        match guid {
            Some(guid) => {
                made |= m.elements.insert_as(id, guid, Element::View3D(there)).is_ok();
                m.next_id = m.next_id.max(id + 1);
            }
            None => {
                if let Some(Element::View3D(v)) = m.elements.get_mut(id) {
                    v.navigate_as(&there);
                }
            }
        }
    }
    if !made || kept.check().is_ok() {
        *model = kept;
    }
}

/// The commands that put another project in the session's place: a session that may only read, and a window's agent, never runs them.
pub const REPLACES: &[&str] = &["new", "open", "recover", "highrise", "tower"];

/// What a refusal of a session that may only read says, after what the command would do and before what would let it edit.
pub const READ_ONLY: &str = "which is read-only for agents";

/// The commands that copy what stands: a copy is made in the session's phase and isn't taken down with its original.
const COPIES: [&str; 4] = ["copy", "mirror", "rotate", "array"];

#[cfg_attr(test, derive(Clone))]
pub struct Session {
    pub model: Arc<Model>,
    /// Where the model was opened from or last saved to.
    pub path: Option<PathBuf>,
    /// The phase, and the design option (`set: option`), what a command makes is made in; none: the
    /// project's first phase, and no option. Not the model's: set with `phase` and `option`.
    pub phase: Option<String>,
    pub option: Option<String>,
    pub(crate) undo: Vec<(Step, Arc<Model>)>,
    pub(crate) redo: Vec<(Step, Arc<Model>)>,
    /// How many times a command's model was checked, for tests that it was or wasn't.
    #[cfg(test)]
    pub(crate) checks: usize,
    /// How many times the project was replaced, so a transaction knows its history went with it.
    resets: usize,
    /// What reads a DWG drawing, when the app has one (`cad::DwgReader`); none says to save it as DXF.
    pub dwg: Option<crate::cad::DwgReader>,
    /// The files a front end with no disk gave the session to read, and what its commands wrote while it keeps its writes
    /// (`files::Files`).
    pub files: crate::files::Files,
    /// The commands plugins registered, beside the static [`commands`]. They belong to this session, not the
    /// process, so two sessions and the tests never share them.
    external: Vec<ExternalCommand>,
    /// Whether the session may only read its project, and if so what would let it edit, which each refusal ends with: an agent's,
    /// when the user lets agents only read (the window's File > Settings > Agents, `bimcraft-cli mcp --read-only`). Then a command
    /// that would replace the project, undo or redo, or write over its own file is refused before it runs, and one that changed the
    /// project, its history or the phase and option the session makes things in is taken back and refused ([`Session::execute`]).
    /// What reads it (inspect, schedules, render, exports to new files, the sheets to PDF) answers; a copy saved as another file
    /// leaves the session with its own.
    pub read_only: Option<&'static str>,
    /// Whether a dry run is under way ([`Session::script_dry_run`]): then each command is looked at as it is about to run, with
    /// the params it is given, and one that would touch a file or the history is refused ([`dry_run_refuses`]).
    dry_run: bool,
    /// The last model a command left that passed [`Model::check`]. Held here, it is never changed in place: a change
    /// outside a command copies it first, so a model still this one is one that passed, and the next command need only
    /// check what it changes ([`Model::check_changed`]).
    pub(crate) verified: Option<Arc<Model>>,
    /// What Copy to Clipboard took, which Paste makes again (`commands/clipboard.rs`). Not the model's: undo does not touch it.
    pub(crate) clipboard: Option<crate::commands::clipboard::Clipboard>,
    /// The warnings of the last model asked ([`Session::warnings`]), kept with it: the next are brought up to date from them.
    warned: Option<Box<(Arc<Model>, Review)>>,
    /// Elements another copy told that the model would not take as they were (a wall of a type not heard of yet), by GUID with their
    /// JSON: offered again when the settings change ([`Session::merge`]).
    pub(crate) waiting: std::collections::BTreeMap<crate::ids::Guid, crate::sync::Wait>,
    /// The same for settings (a type made of a material not heard of yet), by key with their JSON.
    pub(crate) waiting_project: std::collections::BTreeMap<String, crate::sync::Wait>,
    /// How many commands, scripts and transactions are under way, one inside another. A command made inside another (a script's
    /// lines, a transaction's) does not work out its warnings: the one outermost says what the whole made, once. Raised only
    /// through [`Nesting`], which lowers it again however the work ends.
    pub(crate) nested: u8,
    /// Tests only: the warnings of the model after the last command made inside another, brought up to date from those after the
    /// one before it, one command at a time, and held to the whole model's: what the outermost's look at the end can't check.
    #[cfg(test)]
    shadow: Option<Box<(Arc<Model>, Review)>>,
    /// Tests only: how many times that was looked at.
    #[cfg(test)]
    pub(crate) shadowed: usize,
}

/// One more command, script or transaction under way ([`Session::nested`]), until it is dropped: a panic that comes out of the
/// work leaves the count as it was, so the next command is the outermost it is.
struct Nesting<'a>(&'a mut Session);

impl<'a> Nesting<'a> {
    fn enter(s: &'a mut Session) -> Nesting<'a> {
        s.nested = s.nested.saturating_add(1);
        Nesting(s)
    }
}

impl std::ops::Deref for Nesting<'_> {
    type Target = Session;
    fn deref(&self) -> &Session {
        self.0
    }
}

impl std::ops::DerefMut for Nesting<'_> {
    fn deref_mut(&mut self) -> &mut Session {
        self.0
    }
}

impl Drop for Nesting<'_> {
    fn drop(&mut self) {
        self.0.nested = self.0.nested.saturating_sub(1);
    }
}

#[cfg(test)]
thread_local! {
    /// Tests only: where the next call of [`forced_panic`] with that name panics, to hold what a panic leaves behind to account.
    pub(crate) static FORCED_PANIC: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}

/// Tests only: panics here when [`FORCED_PANIC`] names `at`, once.
#[cfg(test)]
fn forced_panic(at: &'static str) {
    if FORCED_PANIC.with(|f| f.get()) == Some(at) {
        FORCED_PANIC.with(|f| f.set(None));
        panic!("forced panic at {at}");
    }
}

impl Default for Session {
    fn default() -> Self {
        Session::new()
    }
}

pub fn commands() -> &'static [Command] {
    crate::commands::ALL
}

/// A command by id or alias, in any case.
pub fn find(name: &str) -> Option<&'static Command> {
    commands().iter().find(|c| c.id.eq_ignore_ascii_case(name) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
}

/// The command catalog as agents read it.
pub fn catalog() -> Value {
    commands()
        .iter()
        .map(|c| {
            let mut entry = json!({"id": c.id, "label": c.label, "aliases": c.aliases, "params": c.params, "menu": c.menu});
            if let Some(files) = c.files.as_str() {
                entry["files"] = json!(files);
            }
            entry
        })
        .collect()
}

/// Whether a call of `cmd` with `params` reads or writes a file, which a plugin may only do with the `files`
/// permission. A command marked at the [`Command`] is the answer, unless [`Files::Conditional`] says the
/// parameters decide: a command that only sometimes takes a path. The plugin does not keep a list of its own.
pub fn touches_files(cmd: &Command, params: &Value) -> Option<Files> {
    let given = |key: &str| params.get(key).is_some_and(|v| !v.is_null());
    match cmd.files {
        Files::None => None,
        Files::Read => Some(Files::Read),
        Files::Write => Some(Files::Write),
        Files::Conditional => {
            // A texture's picture is a file of its own, nested in the material's parameters.
            let texture = params.get("texture").is_some_and(|t| t.get("path").is_some_and(|p| !p.is_null()));
            // The favorites are a file of their own: saving, loading or deleting one writes it, listing only reads it.
            let favorites = params.get("action").and_then(Value::as_str).is_some_and(|a| a != "list");
            if given("path") || given("save") || texture || favorites { Some(Files::Write) } else { None }
        }
    }
}

impl Session {
    pub fn new() -> Session {
        Session {
            model: Arc::new(Model::new("Project")),
            path: None,
            phase: None,
            option: None,
            undo: Vec::new(),
            redo: Vec::new(),
            #[cfg(test)]
            checks: 0,
            resets: 0,
            dwg: None,
            files: crate::files::Files::default(),
            external: Vec::new(),
            read_only: None,
            dry_run: false,
            verified: None,
            clipboard: None,
            warned: None,
            nested: 0,
            #[cfg(test)]
            shadow: None,
            #[cfg(test)]
            shadowed: 0,
            waiting: std::collections::BTreeMap::new(),
            waiting_project: std::collections::BTreeMap::new(),
        }
    }

    /// The model to change, copied first when an undo step still shares it.
    pub fn model_mut(&mut self) -> &mut Model {
        Arc::make_mut(&mut self.model)
    }

    /// Replace the model (new, open), forgetting the history.
    pub fn reset(&mut self, model: Model, path: Option<PathBuf>) {
        self.model = Arc::new(model);
        self.path = path;
        (self.phase, self.option) = (None, None);
        self.clipboard = None;
        self.waiting.clear();
        self.waiting_project.clear();
        self.resets += 1;
        self.warned = None;
        #[cfg(test)]
        {
            self.shadow = None;
        }
        self.undo.clear();
        self.redo.clear();
    }

    /// Labels of the undo steps, oldest first.
    pub fn history(&self) -> Vec<&str> {
        self.undo.iter().map(|(st, _)| st.label.as_str()).collect()
    }

    /// The undo steps, or with `redo` the redo steps, the next to go first.
    pub fn steps(&self, redo: bool) -> Vec<&Step> {
        let from = if redo { &self.redo } else { &self.undo };
        from.iter().rev().map(|(st, _)| st).collect()
    }

    /// Registers a plugin's command. A command may not take the id or an alias of one there already, static
    /// or of another plugin: a plugin is refused, not allowed to shadow BIMCraft.
    pub fn register(&mut self, command: ExternalCommand) -> Result<(), String> {
        let taken = |s: &str| {
            find(s).is_some()
                || self.external.iter().any(|c| c.id.eq_ignore_ascii_case(s) || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(s)))
        };
        if taken(&command.id) {
            return Err(format!("the command `{}` is already taken", command.id));
        }
        if let Some(alias) = command.aliases.iter().find(|a| taken(a)) {
            return Err(format!("the command `{}` would take the alias `{alias}`, already taken", command.id));
        }
        self.external.push(command);
        Ok(())
    }

    /// Takes out every command a plugin registered.
    pub fn unregister_plugin(&mut self, plugin: &str) {
        self.external.retain(|c| c.plugin != plugin);
    }

    /// Takes out every command from outside, as a plugin folder is read again.
    pub fn clear_external(&mut self) {
        self.external.clear();
    }

    /// The commands from outside, as a front end lists them.
    pub fn external(&self) -> &[ExternalCommand] {
        &self.external
    }

    /// The command catalog as agents read it: [`catalog`] with this session's plugin commands, each marked
    /// with the plugin it belongs to. Without plugins it is exactly [`catalog`].
    pub fn catalog(&self) -> Value {
        let mut all = catalog();
        if let (Some(list), false) = (all.as_array_mut(), self.external.is_empty()) {
            list.extend(self.external.iter().map(
                |c| json!({"id": c.id, "label": c.label, "aliases": c.aliases, "params": c.params, "menu": c.menu, "plugin": c.plugin}),
            ));
        }
        all
    }

    /// A command from outside by id or alias, in any case.
    pub fn external_command(&self, name: &str) -> Option<&ExternalCommand> {
        self.external.iter().find(|c| c.matches(name))
    }

    /// Run a command by id or alias with JSON parameters. A command that fails, panics, or leaves
    /// the model failing [`Model::check`] changes nothing. In a session that may only read ([`Session::read_only`]), one that would
    /// change the project is refused, and changes nothing either.
    pub fn execute(&mut self, name: &str, params: &Value) -> Result<Value, String> {
        if let Some(cmd) = find(name) {
            return self.execute_found(cmd, params);
        }
        let Some(cmd) = self.external.iter().find(|c| c.matches(name)).cloned() else {
            return Err(format!("unknown command `{name}`; list_commands shows them"));
        };
        self.execute_external_checked(cmd, name, params)
    }

    /// [`Session::execute`] of a command in the static list, refused or taken back when the session may only read.
    fn execute_found(&mut self, cmd: &'static Command, params: &Value) -> Result<Value, String> {
        if let Some(what) = dry_run_refuses(cmd, params).filter(|_| self.dry_run) {
            return Err(format!(
                "`{}` would {what}, and a dry run touches no file and keeps the history: run the script without `dry_run`",
                cmd.id
            ));
        }
        let Some(how) = self.read_only else { return self.perform(cmd, params) };
        self.may_read(cmd, params).map_err(|what| format!("`{}` would {what}, {READ_ONLY}: {how}", cmd.id))?;
        let kept = Kept::of(self);
        let result = self.perform(cmd, params);
        kept.keep(self).map_err(|()| format!("`{}` would change the project, {READ_ONLY}: {how}", cmd.id)).and(result)
    }

    /// What a session that may only read refuses `cmd` before it runs, by what it would do: replace the project, step through its
    /// history, or write over its own file (`save` with no path saves there, and no command is given it as its `path` or `save`).
    fn may_read(&self, cmd: &Command, params: &Value) -> Result<(), String> {
        if REPLACES.contains(&cmd.id) {
            return Err("replace the project".into());
        }
        if matches!(cmd.id, "undo" | "redo") {
            return Err("change the project".into());
        }
        let Some(own) = self.path.as_deref() else { return Ok(()) };
        if cmd.id == "save" && params.get("path").is_none_or(Value::is_null) {
            return Err("write over the project's own file".into());
        }
        let given =
            ["path", "save"].into_iter().find(|key| params.get(key).and_then(Value::as_str).is_some_and(|p| same_file(own, Path::new(p))));
        match given {
            Some(_) if cmd.id == "save" => Err("write over the project's own file".into()),
            Some(key) => Err(format!("use the project's own file (`{key}`)")),
            None => Ok(()),
        }
    }

    /// [`Session::execute`] of a command found, as any session runs it.
    fn perform(&mut self, cmd: &'static Command, params: &Value) -> Result<Value, String> {
        // A library entry's GUID, wherever a command takes a name, is that name.
        let named = named(&self.model, params);
        let params = named.as_ref().unwrap_or(params);
        let before = Arc::clone(&self.model);
        let verified = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &before));
        let (run, first_new, session) = (cmd.run, self.model.next_id, (self.phase.clone(), self.option.clone()));
        let outermost = self.nested == 0;
        let ran = {
            let mut nest = Nesting::enter(self);
            catch_unwind(AssertUnwindSafe(|| run(&mut nest, params)))
        };
        let result = ran.unwrap_or_else(|p| Err(format!("internal error in `{}`: {}", cmd.id, panic_text(p.as_ref())))).and_then(|v| {
            if Arc::ptr_eq(&before, &self.model) {
                return Ok(v);
            }
            #[cfg(test)]
            forced_panic("post");
            // Back to a model that passed (undo, redo): it is as it was then.
            if self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.model)) {
                #[cfg(test)]
                assert_eq!(self.model.check(), Ok(()), "`{}` went back to a model that doesn't pass", cmd.id);
                return Ok(v);
            }
            // A command that splits what stands keeps its phase; one that makes something makes it in the session's;
            // undo, redo and the commands that replace the project make nothing.
            if cmd.undoable && cmd.id != "split" {
                self.stamp(first_new, COPIES.contains(&cmd.id));
            }
            // A scan goes with the last point cloud that placed it.
            let unplaced = self.model.unplaced_clouds();
            if !unplaced.is_empty() {
                let m = self.model_mut();
                for name in unplaced {
                    m.clouds.remove(&name);
                }
            }
            // A CAD drawing, with the last import.
            let unplaced = self.model.unplaced_cads();
            if !unplaced.is_empty() {
                let m = self.model_mut();
                for name in &unplaced {
                    m.cads.remove(name);
                }
                m.forget_cad_layers(&unplaced);
            }
            // A pin goes with its element.
            if self.model.has_stale_pins() {
                self.model_mut().forget_stale_pins();
            }
            // What a command was given by id points at the element's GUID; what it put in the library has an id and a GUID.
            if verified {
                self.model_mut().resolve_changed(&before);
            } else {
                self.model_mut().resolve();
            }
            // A library in step needs nothing; after a verified model, what the command changed says whether it is.
            let in_step = if verified { self.model.library_in_step_since(&before) } else { self.model.library_in_step() };
            #[cfg(test)]
            if verified {
                assert_eq!(in_step, self.model.library_in_step(), "library_in_step_since differs after `{}` {params}", cmd.id);
            }
            if !in_step {
                // After a verified model, what the command changed is synced; else (or when more than elements
                // changed) the whole library.
                #[cfg(test)]
                let oracle = verified.then(|| sync_oracle(&self.model, &before));
                if !(verified && self.model_mut().sync_since(&before)) {
                    self.model_mut().sync_library();
                }
                #[cfg(test)]
                if let Some(Some((incremental, whole))) = oracle {
                    assert!(
                        incremental.elements.iter().eq(whole.elements.iter()) && incremental.library.parts().eq(whole.library.parts()),
                        "the library synced by what `{}` changed isn't as the whole sync makes it",
                        cmd.id
                    );
                    assert!(self.model.library_in_step(), "the library isn't in step after `{}`", cmd.id);
                }
            }
            // A room frozen keeps the boundary it has now; one thawed lets it go. After a verified model, only a room the
            // command changed can be either.
            let unsettled = |e: &Element| matches!(e, Element::Room(r) if r.frozen != r.kept.is_some());
            let unsettled = match (verified, self.model.changed_since(&before)) {
                (true, crate::ids::Changed::Ids(ids)) => ids.iter().any(|id| self.model.elements.get(*id).is_some_and(unsettled)),
                _ => self.model.elements.values().any(unsettled),
            };
            if unsettled {
                self.model_mut().keep_rooms();
            }
            // The site's shapes worked out again where what they are drawn from changed (masterplan.rs): a site element
            // moved, a lane that meets it, a car park round what it keeps clear of.
            if cmd.undoable && !crate::masterplan::SHAPES_ITSELF.contains(&cmd.id) {
                crate::masterplan::keep(self, &before);
            }
            // Locked and equal dimensions moved what they measure to as the command moved it.
            if cmd.undoable {
                crate::constraints::keep(self, &before)?;
            }
            // What was made or changed must be as a sketch of it solves too (an outline that is simple), which a file
            // from before may break; Insert from File brings in what it finds.
            // (Before the check, which would name a bow tie of no area by that, not by what to redraw.)
            let edited = if cmd.undoable && cmd.id != "insert" { self.model.check_edits(&before, cmd.id, params) } else { Ok(()) };
            edited
                .and_then(|()| {
                    #[cfg(test)]
                    {
                        self.checks += 1;
                    }
                    let checked = if verified { self.model.check_changed(&before) } else { self.model.check() };
                    // Tests hold the two to the same answer after every command they run.
                    #[cfg(test)]
                    if verified {
                        let full = self.model.check();
                        assert_eq!(checked, full, "check_changed differs from check after `{}` {params}", cmd.id);
                    }
                    checked
                })
                .map(|()| v)
        });
        match result {
            Err(e) => {
                // A refused command changes nothing: not the model, nor the phase and the option it asked the session for.
                self.model = before;
                (self.phase, self.option) = session;
                Err(e)
            }
            Ok(mut v) => {
                let changed = cmd.undoable && !Arc::ptr_eq(&before, &self.model);
                // The warnings are looked at before the command is kept: a panic there refuses it, as one in the command does.
                if changed
                    && outermost
                    && let Err(e) = self.report_warnings_guarded(&before, &mut v)
                {
                    self.model = before;
                    (self.phase, self.option) = session;
                    return Err(e);
                }
                self.verified = Some(Arc::clone(&self.model));
                if changed {
                    #[cfg(test)]
                    if !outermost {
                        self.shadow_review(&before);
                    }
                    let step = Step { checked: verified, ..Step::new(cmd.label, &before, &self.model) };
                    self.undo.push((step, before));
                    if self.undo.len() > UNDO_LIMIT {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                }
                Ok(v)
            }
        }
    }

    /// The warnings of the model now ([`Model::review`]), the worst first. Kept with the model: asked again of the same one,
    /// they cost nothing, and of one a command made, they are brought up to date from those before it.
    pub fn warnings(&mut self) -> Arc<Vec<Warning>> {
        let model = Arc::clone(&self.model);
        let now = self.review_of(&model);
        let warnings = Arc::clone(&now.warnings);
        self.warned = Some(Box::new((model, now)));
        warnings
    }

    /// The review of `model`, whichever model the kept one is of: the model itself, or one the warnings are brought up to
    /// date from (an undo, a redo, a change another copy made, a transaction taken back: each leaves the kept warnings on
    /// another model, which is near this one, so only what differs is looked at again). Whole only when nothing is kept.
    fn review_of(&self, model: &Arc<Model>) -> Review {
        match self.warned.as_deref() {
            Some((m, r)) if Arc::ptr_eq(m, model) => r.clone(),
            Some((old, prev)) => model.reviewed_since(old, prev),
            None => model.reviewed(),
        }
    }

    /// The warnings the change from `before` to the model now made, those that weren't among the warnings before it, as a
    /// command's result gives them. The warnings of the model now are kept.
    fn fresh_warnings(&mut self, before: &Arc<Model>) -> Vec<Value> {
        #[cfg(test)]
        forced_panic("report");
        let prev = self.review_of(before);
        let now = self.model.reviewed_since(before, &prev);
        #[cfg(test)]
        now.assert_whole(&self.model);
        let fresh: Vec<Value> = Model::new_warnings(&now.warnings, &prev.warnings).into_iter().map(|w| w.json(&self.model)).collect();
        self.warned = Some(Box::new((Arc::clone(&self.model), now)));
        fresh
    }

    /// The warnings the command that made the model of `before` the one now made, added to `result`, when it is an object, as
    /// `new_warnings`: those that weren't there before it, so whoever made the change sees what it left questionable. A panic in
    /// it is an error that refuses the command: working them out is never what crashes it.
    fn report_warnings_guarded(&mut self, before: &Arc<Model>, result: &mut Value) -> Result<(), String> {
        let fresh = self.fresh_warnings_guarded(before)?;
        insert_warnings(result, fresh);
        Ok(())
    }

    /// [`Session::fresh_warnings`], a panic in it an error.
    fn fresh_warnings_guarded(&mut self, before: &Arc<Model>) -> Result<Vec<Value>, String> {
        catch_unwind(AssertUnwindSafe(|| self.fresh_warnings(before)))
            .map_err(|p| format!("internal error working out the warnings: {}", panic_text(p.as_ref())))
    }

    /// Tests only: the warnings after a command made inside another, brought up to date from those after the command before it
    /// and held to those of the whole model, as the outermost's are after each command of its own. (What this works out is no
    /// part of what the tests count.)
    #[cfg(test)]
    fn shadow_review(&mut self, before: &Arc<Model>) {
        let counted = crate::warnings::REVIEWS.with(std::cell::Cell::get);
        let prev = match self.shadow.take() {
            Some(kept) if Arc::ptr_eq(&kept.0, before) => kept.1,
            _ => before.whole_review(),
        };
        let now = self.model.reviewed_since(before, &prev);
        now.assert_whole(&self.model);
        self.shadowed += 1;
        crate::warnings::REVIEWS.with(|c| c.set(counted));
        self.shadow = Some(Box::new((Arc::clone(&self.model), now)));
    }

    /// [`Session::execute`] of a command a plugin registered. The plugin's closure runs the function in its
    /// sandbox and wraps it in its own transaction; here the model is only guarded, as for any command.
    fn execute_external(&mut self, cmd: ExternalCommand, name: &str, params: &Value) -> Result<Value, String> {
        let named = named(&self.model, params);
        let params = named.as_ref().unwrap_or(params);
        let before = Arc::clone(&self.model);
        let verified = self.verified.clone();
        let (undo, redo, session) = (self.undo.clone(), self.redo.clone(), (self.phase.clone(), self.option.clone()));
        let outermost = self.nested == 0;
        let ran = {
            let mut nest = Nesting::enter(self);
            catch_unwind(AssertUnwindSafe(|| (cmd.run)(&mut nest, params)))
        };
        let result = ran.unwrap_or_else(|p| Err(format!("internal error in `{name}`: {}", panic_text(p.as_ref())))).and_then(|mut v| {
            if Arc::ptr_eq(&before, &self.model) {
                return Ok(v);
            }
            self.model_mut().resolve();
            self.model_mut().sync_library();
            self.model.check()?;
            // What the plugin made questionable, said once, as a script's or a command's are: its own commands said none.
            if outermost {
                self.report_warnings_guarded(&before, &mut v)?;
            }
            Ok(v)
        });
        match result {
            Err(e) => {
                (self.model, self.phase, self.option) = (before, session.0, session.1);
                (self.undo, self.redo) = (undo, redo);
                self.verified = verified;
                Err(e)
            }
            Ok(v) => {
                self.verified = Some(Arc::clone(&self.model));
                Ok(v)
            }
        }
    }

    /// [`Session::execute_external`] of a session that may only read: what the plugin changed is taken back and refused.
    fn execute_external_checked(&mut self, cmd: ExternalCommand, name: &str, params: &Value) -> Result<Value, String> {
        let Some(how) = self.read_only else { return self.execute_external(cmd, name, params) };
        let kept = Kept::of(self);
        let result = self.execute_external(cmd, name, params);
        kept.keep(self).map_err(|()| format!("`{name}` would change the project, {READ_ONLY}: {how}")).and(result)
    }

    /// Whether the model now is the one the last command left that passed the check, so the next checks only what it changes.
    #[cfg(test)]
    pub(crate) fn verified_now(&self) -> bool {
        self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.model))
    }

    /// Runs `f` as a session that may only read ([`Session::read_only`]), `how` saying what would let it edit, unless the
    /// session already says; then gives the session back as it was. What `f` changed of the project, however it got at it, is
    /// taken back and refused too.
    pub fn reading<T>(&mut self, how: &'static str, f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
        let was = self.read_only;
        let how = *self.read_only.get_or_insert(how);
        let kept = Kept::of(self);
        let result = catch_unwind(AssertUnwindSafe(|| f(self)))
            .unwrap_or_else(|p| Err(format!("internal error while reading: {}", panic_text(p.as_ref()))));
        self.read_only = was;
        kept.keep(self).map_err(|()| format!("that would change the project, {READ_ONLY}: {how}")).and(result)
    }

    /// Run a script: one command per line, `id` or `id {json params}`; `#` starts a comment.
    /// Stops at the first failing line, naming it; the lines before it stay done. The warnings the whole script made, those that
    /// were not among the warnings before it, are worked out once, at its end, and are in the last result that is an object as
    /// `new_warnings`: a line does not pay for them.
    pub fn script(&mut self, text: &str) -> Result<Vec<Value>, String> {
        let report = self.nested == 0;
        self.run_script(text, report, false)
    }

    /// [`Session::script`] all or nothing: a line that fails takes back the lines before it, and one that goes through is
    /// one undo step, `Script`.
    pub fn script_atomic(&mut self, text: &str) -> Result<Vec<Value>, String> {
        let report = self.nested == 0;
        self.transaction_in("Script", false, |s| s.run_script(text, report, true)).map(|(results, _)| results)
    }

    /// The lines of a script, none of them working out its warnings; with `report`, the script's as a whole in its result. A
    /// failure working them out refuses an `atomic` script, which takes its lines back; of one that is not, whose lines are done
    /// and stay so, it is logged and the results are given without them.
    fn run_script(&mut self, text: &str, report: bool, atomic: bool) -> Result<Vec<Value>, String> {
        let (before, resets) = (Arc::clone(&self.model), self.resets);
        let ran = Nesting::enter(self).script_lines(text);
        let mut out = ran?;
        // A script that replaced the project (`open`, `new`) made no change to a project that was there before it.
        if report
            && self.resets == resets
            && !Arc::ptr_eq(&before, &self.model)
            && let Some(last) = out.iter_mut().rev().find(|v| v.is_object())
        {
            match self.report_warnings_guarded(&before, last) {
                Err(e) if !atomic => eprintln!("the script's lines are done; its warnings could not be worked out: {e}"),
                other => other?,
            }
        }
        Ok(out)
    }

    fn script_lines(&mut self, text: &str) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (id, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let params = if rest.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(rest).map_err(|e| format!("line {}: bad JSON after `{id}`: {e}", n + 1))?
            };
            out.push(self.execute(id, &params).map_err(|e| format!("line {} ({id}): {e}", n + 1))?);
        }
        Ok(out)
    }

    /// [`Session::script_atomic`] tried and taken back: what the script would make, change and take away, element by element,
    /// whether it would change anything else (the library, the phases, the settings), and the warnings the model would then
    /// have, with each line's result. The project, its history, the phase and option things are made in and its file are left
    /// as they were, and the next id with them. A dry run touches no file and keeps the history: a line that would open, write
    /// or name a file (a `path` or `save`), start or replace the project, save, undo or redo is refused as it comes, with the params
    /// it is given (a named line's and a reference's too), before it runs; the lines before it are taken back with the rest.
    /// It only reads the project, so a session that may only read ([`Session::read_only`]) may try a script too.
    pub fn script_dry_run(&mut self, text: &str) -> Result<Value, String> {
        let kept = Kept::of(self);
        let read_only = self.read_only.take();
        let dry_run = std::mem::replace(&mut self.dry_run, true);
        let result = self.transaction_in("Script", false, |s| s.run_script(text, false, true)).map(|(results, _)| results);
        let after = Arc::clone(&self.model);
        (self.read_only, self.dry_run) = (read_only, dry_run);
        // The project as it was, whatever the script did to it; what a dry run says is only what it would do.
        let _ = kept.keep(self);
        let results = result?;
        let before = Arc::clone(&self.model);
        let before = &before;
        let (mut made, mut changed, mut removed) = (Vec::new(), Vec::new(), Vec::new());
        let ids: Vec<Id> = match after.elements.changed_since(&before.elements) {
            crate::ids::Changed::Ids(ids) => ids.into_iter().collect(),
            crate::ids::Changed::All => {
                let mut all: Vec<Id> = before.elements.iter().map(|(id, _)| *id).chain(after.elements.iter().map(|(id, _)| *id)).collect();
                all.sort_unstable();
                all.dedup();
                all
            }
        };
        for id in ids {
            match (before.elements.get(id), after.elements.get(id)) {
                (None, Some(e)) => made.push(json!({"id": id, "category": e.category()})),
                (Some(a), Some(b)) if a != b || before.elements.guid(id) != after.elements.guid(id) => {
                    changed.push(json!({"id": id, "guid": before.elements.guid(id), "category": b.category()}))
                }
                (Some(a), None) => removed.push(json!({"id": id, "guid": before.elements.guid(id), "category": a.category()})),
                _ => {}
            }
        }
        let other = {
            let mut rest = (*after).clone();
            (rest.elements, rest.next_id) = (before.elements.clone(), before.next_id);
            rest != **before
        };
        // What the script would make questionable: the warnings the model would then have that it has not now, as a command's
        // result gives them.
        let (held, now) = (self.review_of(before), self.review_of(&after));
        let new_warnings: Vec<Value> = Model::new_warnings(&now.warnings, &held.warnings).into_iter().map(|w| w.json(&after)).collect();
        Ok(json!({
            "dry_run": true, "results": results, "made": made, "changed": changed, "removed": removed, "other_changes": other,
            "new_warnings": new_warnings,
        }))
    }

    /// Run `f` as one undo step named `label`, all or nothing: when it fails, the model, the path, the session's phase
    /// and option and the undo and redo history are as they were before it. Inside, undo reaches back no further than
    /// its start. Files a command wrote meanwhile (`save`, `export`) stay written, and one that replaced the project (`new`,
    /// `open`) left the history it began, so what came after it stays the steps it was. The warnings it made are worked out
    /// when something asks ([`Session::transaction_reporting`] says them at its end).
    pub fn transaction<T>(&mut self, label: &str, f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, String> {
        self.transaction_in(label, false, f).map(|(v, _)| v)
    }

    /// [`Session::transaction`], and with what it gives the warnings the whole made, those that weren't among the warnings before
    /// it, each as a command's result gives them (`new_warnings`): `f`'s commands say none of their own. None are given for a
    /// transaction inside another command, script or transaction, whose outermost says them, nor for one that replaced the
    /// project.
    pub fn transaction_reporting<T>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<(T, Vec<Value>), String> {
        self.transaction_in(label, true, f)
    }

    /// [`Session::transaction_reporting`], which says its warnings only when `report`.
    fn transaction_in<T>(
        &mut self,
        label: &str,
        report: bool,
        f: impl FnOnce(&mut Session) -> Result<T, String>,
    ) -> Result<(T, Vec<Value>), String> {
        let before = Arc::clone(&self.model);
        let checked = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &before));
        let (path, phase, option) = (self.path.clone(), self.phase.clone(), self.option.clone());
        let (mut undo, redo, resets) = (std::mem::take(&mut self.undo), std::mem::take(&mut self.redo), self.resets);
        let outermost = report && self.nested == 0;
        let ran = {
            let mut nest = Nesting::enter(self);
            catch_unwind(AssertUnwindSafe(|| f(&mut nest)))
        };
        let result = ran.unwrap_or_else(|p| Err(format!("internal error in `{label}`: {}", panic_text(p.as_ref()))));
        // What the whole made questionable, unless it replaced the project, which made no change to the one that was there.
        let result = result.and_then(|v| {
            let reported = outermost && self.resets == resets && !Arc::ptr_eq(&before, &self.model);
            let fresh = if reported { self.fresh_warnings_guarded(&before)? } else { Vec::new() };
            Ok((v, fresh))
        });
        match result {
            Err(e) => {
                (self.model, self.path, self.phase, self.option) = (before, path, phase, option);
                (self.undo, self.redo) = (undo, redo);
                Err(e)
            }
            Ok(done) if self.resets != resets => Ok(done),
            Ok(done) => {
                // Only an undoable command made a step; one that changed nothing, or only the view of it, makes none here either.
                if self.undo.is_empty() {
                    self.redo = redo;
                } else {
                    undo.push((Step { checked, ..Step::new(label, &before, &self.model) }, before));
                    if undo.len() > UNDO_LIMIT {
                        undo.remove(0);
                    }
                    self.redo.clear();
                }
                self.undo = undo;
                Ok(done)
            }
        }
    }

    /// What elements from id `first` on, which a command just made, are made in: the session's phase
    /// (the first phase is no value at all, but for a door or window, which would follow its wall) and design
    /// option. A name the project no longer has is left out. A copy is made in them whatever it was copied from,
    /// and isn't taken down with its original; anything else holds the phase or the option it was given.
    fn stamp(&mut self, first: Id, copies: bool) {
        if self.phase.is_none() && self.option.is_none() || self.model.next_id <= first {
            return;
        }
        let m = Arc::make_mut(&mut self.model);
        let phase = self.phase.clone().filter(|p| m.phases.contains(p));
        let option = self.option.clone().filter(|o| crate::phasing::option_exists(m, o));
        let is_first = phase.as_ref() == m.phases.first();
        for (_, e) in m.elements.range_mut(first..) {
            let follows = matches!(e, Element::Door(_) | Element::Window(_));
            let Some(params) = e.params_mut() else { continue };
            if let Some(p) = &phase
                && (copies || !params.contains_key(crate::phasing::CREATED))
            {
                if is_first && !follows {
                    params.remove(crate::phasing::CREATED);
                } else {
                    params.insert(crate::phasing::CREATED.into(), json!(p));
                }
                if copies {
                    params.remove(crate::phasing::DEMOLISHED);
                }
            }
            if let Some(o) = &option
                && (copies || !params.contains_key(crate::phasing::OPTION))
            {
                params.insert(crate::phasing::OPTION.into(), json!(o));
            }
        }
    }

    /// Undo (or with `redo`, redo) `count` steps; the reply names the last of them.
    pub(crate) fn step_back(&mut self, redo: bool, count: usize) -> Result<Value, String> {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let (key, what) = if redo { ("redone", "redo") } else { ("undone", "undo") };
        if from.is_empty() {
            return Err(format!("nothing to {what}"));
        }
        if count == 0 || count > from.len() {
            return Err(format!("`count` must be from 1 to {}, the steps there are to {what}", from.len()));
        }
        let mut label = String::new();
        // Whether the model now passed: the step that comes back to it says so.
        let mut checked = self.verified.as_ref().is_some_and(|v| Arc::ptr_eq(v, &self.model));
        for _ in 0..count {
            let Some((mut step, model)) = from.pop() else { break };
            label = step.label.clone();
            std::mem::swap(&mut step.checked, &mut checked);
            let left = std::mem::replace(&mut self.model, model);
            keep_navigation(&mut self.model, &left, &step.turned);
            to.push((step, left));
        }
        if checked {
            self.verified = Some(Arc::clone(&self.model));
        }
        Ok(json!({ key: label, "count": count }))
    }
}

/// What a session that may only read keeps through a command: its project, its history, its file, the phase and option it makes
/// things in, and how many times it was replaced.
/// What a dry run refuses `cmd` with `params` for, before it runs: replacing the project, saving it, stepping through its history,
/// or any file read or written ([`touches_files`], and a `path` or `save` any command is given).
fn dry_run_refuses(cmd: &Command, params: &Value) -> Option<String> {
    if REPLACES.contains(&cmd.id) {
        return Some("replace the project".into());
    }
    if matches!(cmd.id, "save" | "autosave" | "undo" | "redo") {
        return Some(format!("{} the project", if cmd.id == "autosave" { "save" } else { cmd.id }));
    }
    if let Some(key) = ["path", "save"].into_iter().find(|key| params.get(key).is_some_and(|v| !v.is_null())) {
        return Some(format!("name a file (`{key}`)"));
    }
    touches_files(cmd, params).map(|files| match files {
        Files::Read => "read a file".to_string(),
        _ => "write a file".to_string(),
    })
}

struct Kept {
    model: Arc<Model>,
    /// The model the last command left that passed the check, as it was: put back with the model, the next command checks
    /// only what it changes.
    verified: Option<Arc<Model>>,
    path: Option<PathBuf>,
    phase: Option<String>,
    option: Option<String>,
    undo: Vec<(Step, Arc<Model>)>,
    redo: Vec<(Step, Arc<Model>)>,
    resets: usize,
    /// The warnings the session kept, of the model it kept: a try that worked out the warnings of another leaves them as they were.
    warned: Option<Box<(Arc<Model>, Review)>>,
}

impl Kept {
    fn of(s: &Session) -> Kept {
        Kept {
            model: Arc::clone(&s.model),
            verified: s.verified.clone(),
            path: s.path.clone(),
            phase: s.phase.clone(),
            option: s.option.clone(),
            undo: s.undo.clone(),
            redo: s.redo.clone(),
            resets: s.resets,
            warned: s.warned.clone(),
        }
    }

    /// Puts the session back as it was kept, and says whether anything had changed but its file: a copy saved as another file
    /// leaves the session with its own, and is no change.
    fn keep(self, s: &mut Session) -> Result<(), ()> {
        let same = (Arc::ptr_eq(&self.model, &s.model) || *self.model == *s.model)
            && (s.phase == self.phase && s.option == self.option && s.resets == self.resets)
            && (s.undo.len() == self.undo.len() && s.redo.len() == self.redo.len());
        (s.model, s.verified, s.path, s.phase, s.option) = (self.model, self.verified, self.path, self.phase, self.option);
        (s.undo, s.redo, s.resets) = (self.undo, self.redo, self.resets);
        s.warned = self.warned;
        if same { Ok(()) } else { Err(()) }
    }
}

/// Whether two paths name the same file: as the system resolves them where both are there, else as they are written.
fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// `params` with every string that is a library entry's GUID put as the entry's name; none when there's no such string.
fn named(m: &Model, params: &Value) -> Option<Value> {
    fn walk(m: &Model, v: &mut Value, changed: &mut bool) {
        match v {
            Value::String(s) => {
                if let Some((_, e)) = crate::ids::Guid::parse(s).filter(|g| g.pending().is_none()).and_then(|g| m.library.by_guid(g)) {
                    // A design option's name is `set: option` already.
                    *s = e.name.clone();
                    *changed = true;
                }
            }
            Value::Array(a) => a.iter_mut().for_each(|x| walk(m, x, changed)),
            Value::Object(o) => o.values_mut().for_each(|x| walk(m, x, changed)),
            _ => {}
        }
    }
    let mut v = params.clone();
    let mut changed = false;
    walk(m, &mut v, &mut changed);
    changed.then_some(v)
}

/// `fresh` (what [`Session::transaction_reporting`] gives) as the `new_warnings` of `result`, when it is an object and there are
/// some: how a batch of commands says its warnings in its last result.
pub fn insert_warnings(result: &mut Value, fresh: Vec<Value>) {
    if let Some(o) = result.as_object_mut().filter(|_| !fresh.is_empty()) {
        o.insert("new_warnings".into(), Value::Array(fresh));
    }
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "panic".into())
}

/// The library synced both ways, by what changed since `before` and whole, each new GUID made the same way in order:
/// what the engine holds the incremental sync to in tests. None where the incremental one can't say.
#[cfg(test)]
fn sync_oracle(m: &Model, before: &Model) -> Option<(Model, Model)> {
    let counter = || {
        let mut n = 0u64;
        move |_: &Model, _: Option<&crate::library::Kind>, _: &str| {
            n += 1;
            crate::ids::Guid::seeded(0x5eed, n)
        }
    };
    let mut incremental = m.clone();
    if !incremental.sync_since_with(before, &mut counter()) {
        return None;
    }
    let mut whole = m.clone();
    whole.sync_library_with(&mut counter());
    Some((incremental, whole))
}

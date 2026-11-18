//! The bridge (issue 41): how an agent works on the project a window has open. The window, started with `--control PORT`,
//! listens on a local port; `bimcraft-cli mcp --connect` is the MCP server of a headless session, and with a port it runs
//! its tools on the window's session instead. One JSON request a line, one JSON reply a line:
//!
//! - `{"token": T, "op": "hello"}` asks which app is there, what project it has open and whether agents may only read it;
//! - `{"token": T, "op": "execute", "command": "wall", "params": {...}}` runs a command, as `Session::execute` does;
//! - `{"token": T, "op": "script", "text": "...", "atomic": false}` runs a script (`Session::script`, or all or nothing);
//!   with `"dry_run": true` it is tried and taken back, and the reply says what it would do (`Session::script_dry_run`);
//! - `{"token": T, "op": "catalog"}` is the command catalog, with the window's plugin commands (`Session::catalog`).
//!
//! `execute` and `script` may say `"read_only": true`: the agent's side asks to only read (`bimcraft-cli mcp --connect
//! --read-only`). The reply is `{"ok": value}` or `{"error": text}`. The token is made when the window starts and kept in a file
//! only the user can read ([`Entry`]), so another user's process on the machine, or a web page that tries the port, is turned
//! away. An agent never replaces the window's project ([`REFUSED`]), and while the user lets agents only read it
//! ([`Access::Read`]) the session runs its requests read-only ([`Session::reading`]): what would change the project is refused,
//! however it is asked. This is the engine's side: the parts that need a thread or a window are in the apps.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::engine::{REPLACES, Session, find};
use crate::favorites;

/// The longest request line read: a script of a whole building is a few megabytes.
pub const MAX_LINE: usize = 16 << 20;

/// What a window's agent may not do to its project: replace it. It is the user's, and the window starts another or opens one by
/// its own menus, which ask about unsaved changes first.
pub const REFUSED: &[&str] = REPLACES;

/// What the window lets its agents do to its project: change it, as the user does, or only read it (File > Settings > Agents,
/// Allow agents to edit this project).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Edit,
    Read,
}

/// What a refusal says lets the window's agents edit, when the window lets them only read.
pub const LET_EDIT: &str = "the user lets agents edit it in the window, with File > Settings > Agents > Allow agents to edit this project";

/// What a refusal says when the agent's own side asked to only read.
pub const ASKED: &str =
    "this agent's connection asked to only read it (bimcraft-cli mcp --connect --read-only): connect without --read-only to edit";

/// The file the window leaves its port and token in, in the folder of the user's favorites.
pub const FILE: &str = "bridge.json";

/// A request, read.
#[derive(Debug, PartialEq)]
pub enum Request {
    Hello,
    /// `read_only`: the agent's side asks to only read, whatever the window lets.
    Execute {
        command: String,
        params: Value,
        read_only: bool,
    },
    /// `dry_run`: tried and taken back ([`Session::script_dry_run`]), what it would do said and nothing of it kept.
    Script {
        text: String,
        atomic: bool,
        dry_run: bool,
        read_only: bool,
    },
    /// The command catalog, with the session's plugin commands.
    Catalog,
}

/// Why a request line was not taken.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The token is missing or wrong: the connection is dropped.
    Token,
    /// The line is no request: what is wrong with it.
    Request(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Token => f.write_str("the token is missing or wrong: the window's bridge file has it"),
            Error::Request(why) => f.write_str(why),
        }
    }
}

/// Whether two tokens are the same, in a time that does not say how much of one was right.
#[doc(hidden)]
pub fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// Reads a request line, for a window whose token is `token`.
pub fn parse(line: &str, token: &str) -> Result<Request, Error> {
    let bad = |why: &str| Error::Request(why.to_string());
    let request: Value = serde_json::from_str(line).map_err(|e| Error::Request(format!("not JSON: {e}")))?;
    let Some(o) = request.as_object() else { return Err(bad("a request is an object")) };
    if !o.get("token").and_then(Value::as_str).is_some_and(|t| same(t, token)) {
        return Err(Error::Token);
    }
    let read_only = match o.get("read_only") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad("`read_only` is true or false")),
    };
    match o.get("op").and_then(Value::as_str) {
        Some("hello") => Ok(Request::Hello),
        Some("execute") => {
            let command = o.get("command").and_then(Value::as_str).ok_or_else(|| bad("`execute` needs a `command`, a string"))?;
            let params = match o.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(p @ Value::Object(_)) => p.clone(),
                Some(_) => return Err(bad("`params` is an object")),
            };
            Ok(Request::Execute { command: command.to_string(), params, read_only })
        }
        Some("catalog") => Ok(Request::Catalog),
        Some("script") => {
            let text = o.get("text").and_then(Value::as_str).ok_or_else(|| bad("`script` needs `text`, a string"))?;
            let atomic = match o.get("atomic") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(bad("`atomic` is true or false")),
            };
            let dry_run = match o.get("dry_run") {
                None | Some(Value::Null) => false,
                Some(Value::Bool(b)) => *b,
                Some(_) => return Err(bad("`dry_run` is true or false")),
            };
            Ok(Request::Script { text: text.to_string(), atomic, dry_run, read_only })
        }
        Some(other) => Err(Error::Request(format!("unknown op `{other}`: hello, execute, script or catalog"))),
        None => Err(bad("a request has an `op`")),
    }
}

/// Refuses a command that would replace the project, by whatever name (an id or a shortcut, in any case).
fn refuse(command: &str) -> Result<(), String> {
    match find(command) {
        Some(c) if REFUSED.contains(&c.id) => Err(format!(
            "`{}` would replace the project the window has open, which is the user's: an agent never starts, opens or recovers \
             a project in a window; the user does, with File > New and File > Open",
            c.id
        )),
        _ => Ok(()),
    }
}

/// Runs a request on `session`, as `access` lets the window's agents: read-only ([`Session::reading`]) when the window lets
/// them only read or the request asks to. A script is read through before it runs, so a line that would replace the project
/// stops it before any line has.
pub fn run(session: &mut Session, request: &Request, access: Access) -> Result<Value, String> {
    let how = match (access, request) {
        (Access::Read, _) => Some(LET_EDIT),
        (_, Request::Execute { read_only: true, .. } | Request::Script { read_only: true, .. }) => Some(ASKED),
        _ => None,
    };
    match how {
        Some(how) => session.reading(how, |s| answer(s, request, access)),
        None => answer(session, request, access),
    }
}

fn answer(session: &mut Session, request: &Request, access: Access) -> Result<Value, String> {
    match request {
        Request::Hello => Ok(json!({
            "app": "bimcraft",
            "version": env!("CARGO_PKG_VERSION"),
            "project": session.model.name,
            "read_only": access == Access::Read,
        })),
        Request::Execute { command, params, .. } => {
            refuse(command)?;
            session.execute(command, params)
        }
        Request::Catalog => Ok(session.catalog()),
        Request::Script { text, atomic, dry_run, .. } => {
            for (n, line) in text.lines().map(str::trim).enumerate() {
                if let Some(id) = line.split_whitespace().next().filter(|id| !id.starts_with('#')) {
                    refuse(id).map_err(|e| format!("line {}: {e}", n + 1))?;
                }
            }
            if *dry_run {
                return session.script_dry_run(text);
            }
            let results = if *atomic { session.script_atomic(text) } else { session.script(text) }?;
            Ok(json!(results))
        }
    }
}

/// The reply line for a result.
pub fn reply(result: Result<Value, String>) -> String {
    match result {
        Ok(v) => json!({"ok": v}),
        Err(e) => json!({"error": e}),
    }
    .to_string()
}

/// A window's way in, as the file keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub port: u16,
    pub token: String,
}

/// Where the file is: beside the user's favorites.
pub fn entry_path() -> Option<PathBuf> {
    favorites::default_path().map(|p| p.with_file_name(FILE))
}

/// Writes the file for the window listening on `entry.port`, readable by the user alone where the system has such a thing.
pub fn write_entry(path: &Path, entry: &Entry) -> io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(json!({"port": entry.port, "token": entry.token}).to_string().as_bytes())
}

/// Reads the file, if it is there and says a port and a token.
pub fn read_entry(path: &Path) -> Option<Entry> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(4096).read_to_string(&mut text).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some(Entry { port: u16::try_from(v["port"].as_u64()?).ok()?, token: v["token"].as_str().filter(|t| !t.is_empty())?.to_string() })
}

/// Removes the file, when it is still the one `entry` wrote: another window's is not this one's to remove.
pub fn remove_entry(path: &Path, entry: &Entry) {
    if read_entry(path).as_ref() == Some(entry) {
        let _ = std::fs::remove_file(path);
    }
}

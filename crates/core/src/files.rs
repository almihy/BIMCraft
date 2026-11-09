//! The files a session's commands read and write. On a desktop they are on disk, by their paths. A front end with no disk
//! of its own (a browser page, a server that keeps its files in a store) gives the engine a file's bytes under a name
//! ([`Files::give`]) and then names it as a command's `path`, as it would a file on disk: `open`, `link`, `insert`,
//! `import_cad`, `point_cloud`, a `material`'s picture and `family` read it from there. What a command writes (`save` as
//! IFC, glTF, PDF, PNG or SVG, `render` to a file, a schedule's CSV) stays here for it to take ([`Files::take_written`])
//! when the session keeps its writes ([`Files::keep_writes`]), which in a browser it always does.

use std::collections::BTreeMap;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::sync::Arc;

/// The files given to a session by name, each read as a file at that path would be. Cheap to clone: the bytes are shared.
#[derive(Clone, Debug, Default)]
pub struct Given(BTreeMap<String, Arc<Vec<u8>>>);

/// A path as files are given under it: with `/` between its parts, so `a\b.ifc` and `a/b.ifc` are the same file.
fn key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

impl Given {
    /// The bytes given at `path`, if they were.
    pub fn get(&self, path: &Path) -> Option<&Arc<Vec<u8>>> {
        if self.0.is_empty() {
            return None;
        }
        self.0.get(&key(path))
    }

    /// Whether a file was given at `path`.
    pub fn has(&self, path: &Path) -> bool {
        self.get(path).is_some()
    }

    /// The names given, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// The size of the file at `path`: the one given there, else the one on disk.
    pub fn size(&self, path: &Path) -> std::io::Result<u64> {
        match self.get(path) {
            Some(bytes) => Ok(bytes.len() as u64),
            None => disk(path).and_then(|_| std::fs::metadata(path)).map(|m| m.len()),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The bytes of the file at `path`: the one given there, else the one on disk.
    pub fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        match self.get(path) {
            Some(bytes) => Ok(bytes.as_ref().clone()),
            None => disk(path).and_then(|_| std::fs::read(path)),
        }
    }

    /// The text of the file at `path`, as [`Given::read`] reads it.
    pub fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        String::from_utf8(self.read(path)?).map_err(|_| Error::new(ErrorKind::InvalidData, "stream did not contain valid UTF-8"))
    }
}

/// Whether the disk may be read: never in a browser, which has none, and the page names what it gave.
fn disk(path: &Path) -> std::io::Result<()> {
    if cfg!(target_arch = "wasm32") {
        return Err(Error::new(ErrorKind::NotFound, format!("no file was given as `{}`", key(path))));
    }
    Ok(())
}

/// What a session's commands read and write besides the disk.
#[derive(Clone, Debug, Default)]
pub struct Files {
    pub given: Given,
    /// What commands wrote, by the path each was written to, oldest first, while the session keeps its writes.
    written: Vec<(String, Vec<u8>)>,
    /// Whether what commands write stays here rather than going to disk. In a browser it always does.
    pub keep_writes: bool,
}

impl Files {
    /// Gives the file `name` (a path, as a command names it: "files/12/Plan.dxf"), its `bytes`; given again, the newer bytes.
    pub fn give(&mut self, name: &str, bytes: Vec<u8>) {
        self.given.0.insert(key(Path::new(name)), Arc::new(bytes));
    }

    /// Lets go of the file given as `name`, or of all of them with none.
    pub fn forget(&mut self, name: Option<&str>) {
        match name {
            Some(n) => {
                self.given.0.remove(&key(Path::new(n)));
            }
            None => self.given.0.clear(),
        }
    }

    /// Whether what commands write stays here.
    pub fn keeps_writes(&self) -> bool {
        self.keep_writes || cfg!(target_arch = "wasm32")
    }

    /// Writes `bytes` to `path`: kept here when the session keeps its writes (a file written twice is kept as the second),
    /// else on disk.
    pub fn write(&mut self, path: &Path, bytes: Vec<u8>) -> std::io::Result<()> {
        if self.keeps_writes() {
            let name = key(path);
            self.written.retain(|(n, _)| *n != name);
            self.written.push((name, bytes));
            return Ok(());
        }
        std::fs::write(path, bytes)
    }

    /// The files written since the last call, by the paths they were written to, oldest first.
    pub fn take_written(&mut self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.written)
    }

    /// Whether anything was written that is not taken yet.
    pub fn has_written(&self) -> bool {
        !self.written.is_empty()
    }
}

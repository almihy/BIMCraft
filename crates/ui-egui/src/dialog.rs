//! The file dialogs. On a desktop, the system's: one run inside the window's event handler nests a run loop of its own in it,
//! and on macOS a drag that reaches the window meanwhile re-enters the handler and aborts the app (issue 43). So a dialog is
//! only begun on the window's thread (AppKit wants that, and the sheet it opens does not block) and waited for on a thread of
//! its own: the window keeps its event loop, and the path comes back in a later frame.
//!
//! In a window a page hosts (`host.rs`) there is no disk to pick from: the page is asked instead ([`FileRequest`]). It shows
//! its own picker (Arqiflow's project Files), gives the engine the files picked as bytes under names
//! (`bimcraft_core::files`), and answers with those names, which then go where a path from the system's dialog would.

#[cfg(not(target_arch = "wasm32"))]
use std::future::Future;
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use egui::Context;

use crate::BimApp;

/// What is done with the paths a dialog gives.
pub(crate) type Then = Box<dyn FnOnce(&mut BimApp, Vec<PathBuf>)>;

/// A dialog that is open: what it answers, and what to do with its paths; in a hosted window, what the page was asked and
/// where its answer goes.
pub(crate) struct Dialog {
    pub(crate) answer: Receiver<Option<Vec<PathBuf>>>,
    pub(crate) then: Then,
    pub(crate) asked: Option<(FileRequest, Sender<Option<Vec<PathBuf>>>)>,
}

/// What a file dialog asks for: the kinds of file (named, with their extensions), the name to save under, its title. Built as
/// the system's dialog is (`rfd::AsyncFileDialog`), and shown as the system's on a desktop or as the page's picker in a page.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ask {
    filters: Vec<(String, Vec<String>)>,
    name: Option<String>,
    title: Option<String>,
}

impl Ask {
    pub(crate) fn new() -> Ask {
        Ask::default()
    }

    pub(crate) fn add_filter(mut self, name: impl Into<String>, extensions: &[impl ToString]) -> Ask {
        self.filters.push((name.into(), extensions.iter().map(ToString::to_string).collect()));
        self
    }

    pub(crate) fn set_file_name(mut self, name: impl Into<String>) -> Ask {
        self.name = Some(name.into());
        self
    }

    pub(crate) fn set_title(mut self, title: impl Into<String>) -> Ask {
        self.title = Some(title.into());
        self
    }

    /// The system's dialog for it.
    #[cfg(not(target_arch = "wasm32"))]
    fn system(&self) -> rfd::AsyncFileDialog {
        let mut d = rfd::AsyncFileDialog::new();
        for (name, extensions) in &self.filters {
            d = d.add_filter(name, extensions);
        }
        if let Some(n) = &self.name {
            d = d.set_file_name(n);
        }
        if let Some(t) = &self.title {
            d = d.set_title(t);
        }
        d
    }

    /// What the page is asked for.
    fn request(&self, save: bool, multiple: bool) -> FileRequest {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let mut extensions: Vec<String> = Vec::new();
        for e in self.filters.iter().flat_map(|(_, e)| e) {
            let e = e.to_ascii_lowercase();
            if !extensions.contains(&e) {
                extensions.push(e);
            }
        }
        FileRequest {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            save,
            multiple,
            what: self.filters.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "),
            extensions,
            name: self.name.clone(),
            title: self.title.clone(),
        }
    }
}

/// A file a hosted window asks its page for: one to read (or several, `multiple`), or where to save one (`save`). The page
/// answers with [`BimApp::answer_file_request`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRequest {
    /// Which ask this is: a later one has a higher id.
    pub id: u64,
    pub save: bool,
    pub multiple: bool,
    /// The kinds of file, as people call them: "CAD drawings (DWG, DXF)".
    pub what: String,
    /// The extensions they have, lowercase, without the dot.
    pub extensions: Vec<String>,
    /// The name to save under, ending in its extension.
    pub name: Option<String>,
    pub title: Option<String>,
}

impl BimApp {
    /// Asks for a file to open, or for where to save one (`save`), and calls `then` with the path in a later frame. Nothing
    /// happens if the dialog is cancelled, and a second dialog is not opened while one is.
    pub(crate) fn ask_file(&mut self, ctx: &Context, ask: Ask, save: bool, then: impl FnOnce(&mut BimApp, PathBuf) + 'static) {
        self.begin(
            ctx,
            ask,
            save,
            false,
            Box::new(|app, paths| {
                if let Some(path) = paths.into_iter().next() {
                    then(app, path);
                }
            }),
        );
    }

    /// Asks for one file or several to open, and calls `then` with their paths in a later frame, as [`BimApp::ask_file`] does.
    pub(crate) fn ask_files(&mut self, ctx: &Context, ask: Ask, then: impl FnOnce(&mut BimApp, Vec<PathBuf>) + 'static) {
        self.begin(ctx, ask, false, true, Box::new(then));
    }

    fn begin(&mut self, ctx: &Context, ask: Ask, save: bool, multiple: bool, then: Then) {
        if self.dialog.is_some() {
            return;
        }
        let (tx, answer) = channel();
        if self.hosted() {
            // The page's picker: it hears of the ask after the frame, and answers in a later one. What it gave for the last
            // ask has been read by now (a box such as Import CAD's reads it on its OK), so its bytes are let go of; a link
            // reads its file again when the page gives it again (`reload`).
            self.session.files.forget(None);
            self.dialog = Some(Dialog { answer, then, asked: Some((ask.request(save, multiple), tx)) });
            ctx.request_repaint();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let dialog = ask.system();
            let future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>> = match (save, multiple) {
                (true, _) => Box::pin(async move { dialog.save_file().await.map(|f| vec![f]) }),
                (false, true) => Box::pin(dialog.pick_files()),
                (false, false) => Box::pin(async move { dialog.pick_file().await.map(|f| vec![f]) }),
            };
            wait(ctx, future, tx);
            self.dialog = Some(Dialog { answer, then, asked: None });
        }
        // A browser window no page hosts has nowhere to pick from.
        #[cfg(target_arch = "wasm32")]
        drop((tx, answer, then));
    }

    /// What the page is asked for, while it is: the ask of a hosted window's open dialog.
    pub fn file_request(&self) -> Option<&FileRequest> {
        self.dialog.as_ref()?.asked.as_ref().map(|(r, _)| r)
    }

    /// The page's answer to ask `id`: the names it gave the files picked under (`files.give`), or where to save, or none when
    /// the person cancelled. An answer to an ask that is no longer open is dropped.
    pub fn answer_file_request(&mut self, id: u64, names: Option<Vec<String>>) {
        let Some(Dialog { asked: Some((r, tx)), .. }) = &self.dialog else { return };
        if r.id != id {
            return;
        }
        let _ = tx.send(names.map(|n| n.into_iter().map(PathBuf::from).collect()));
    }

    /// Once a frame: acts on the path an open dialog has given. A thread that died leaves no answer, and the dialog is dropped.
    pub(crate) fn dialog_answered(&mut self) {
        let Some(open) = &self.dialog else { return };
        match open.answer.try_recv() {
            Ok(paths) => {
                if let Some(Dialog { then, .. }) = self.dialog.take()
                    && let Some(paths) = paths.filter(|p| !p.is_empty())
                {
                    then(self, paths);
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.dialog = None,
        }
    }
}

/// Waits for `future`'s answer on a thread of its own and sends it on `tx`. The future is made by the caller, on the window's
/// thread: on macOS that is where the dialog is begun (its sheet opens at once and does not block), and only waiting for it
/// is done on the thread.
#[cfg(all(not(test), not(target_arch = "wasm32")))]
fn wait(ctx: &Context, future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>>, tx: Sender<Option<Vec<PathBuf>>>) {
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let files = pollster::block_on(future);
        let _ = tx.send(files.map(|f| f.iter().map(|f| f.path().to_path_buf()).collect()));
        ctx.request_repaint();
    });
}

/// A test opens no system dialog: the dialog it begins is answered through `BEGUN`'s sender.
#[cfg(test)]
fn wait(_: &Context, future: Pin<Box<dyn Future<Output = Option<Vec<rfd::FileHandle>>> + Send>>, tx: Sender<Option<Vec<PathBuf>>>) {
    drop(future);
    BEGUN.with(|b| b.borrow_mut().push(tx));
}

#[cfg(test)]
thread_local! {
    /// The answer to each dialog begun in the test's thread, oldest first.
    pub(crate) static BEGUN: std::cell::RefCell<Vec<Sender<Option<Vec<PathBuf>>>>> = const { std::cell::RefCell::new(Vec::new()) };
}

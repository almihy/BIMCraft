//! PDF files read apart from the window, so a hostile one can't take it down. On the desktop each read runs in a thread of its
//! own, where a panic is caught. In a hosting page (the browser, where a panic aborts the whole WebAssembly instance) the page
//! reads it in a Web Worker with an instance of its own: it takes the asks ([`BimApp::take_pdf_requests`]), runs each as
//! `bimcraft_core::raster::run_pdf_job` there, and answers ([`BimApp::answer_pdf_request`]); a worker that died is answered
//! as an error, and the editor says the file could not be read.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

use bimcraft_core::raster::{PdfJob, run_pdf_job};

use crate::BimApp;

/// An ask of the page to read a PDF apart: its id, the file's path as the window knows it, the job (JSON of
/// `bimcraft_core::raster::PdfJob`: `{"job": "outline"}`, `{"job": "thumbnail", "page", "side"}`, `{"job": "page", "page",
/// "dpi"}`) and the file's bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct PdfRequest {
    pub id: u64,
    pub path: String,
    pub job: String,
    pub bytes: Vec<u8>,
}

/// What came back of a read: its id, the job, and its answer as bytes, or why there is none.
pub(crate) type Answer = (u64, PdfJob, Result<Vec<u8>, String>);

/// The reads under way and their answers.
pub(crate) struct PdfReader {
    next: u64,
    /// Asked and not answered, by id.
    open: BTreeMap<u64, PdfJob>,
    /// Asks the page hasn't taken yet (in a hosting page).
    queue: Vec<PdfRequest>,
    tx: Sender<(u64, Result<Vec<u8>, String>)>,
    rx: Receiver<(u64, Result<Vec<u8>, String>)>,
}

impl Default for PdfReader {
    fn default() -> PdfReader {
        let (tx, rx) = channel();
        PdfReader { next: 1, open: BTreeMap::new(), queue: Vec::new(), tx, rx }
    }
}

/// What the window says when a PDF can't be read: the reader's own words when it gave them, else that it gave up.
pub(crate) fn could_not_read(name: &str, why: &str) -> String {
    crate::lang::tf("{} could not be read: {}", &[name, why])
}

impl PdfReader {
    /// Asks for `job` on the file at `path`, its `bytes`: given to the page when `hosted`, else read in a thread of its
    /// own. Its id, which its answer comes with.
    fn ask(&mut self, ctx: Option<&egui::Context>, hosted: bool, path: &str, bytes: Vec<u8>, job: PdfJob) -> u64 {
        let id = self.next;
        self.next += 1;
        self.open.insert(id, job.clone());
        if hosted {
            let text = serde_json::to_string(&job).unwrap_or_default();
            self.queue.push(PdfRequest { id, path: path.to_string(), job: text, bytes });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
            return id;
        }
        let tx = self.tx.clone();
        let ctx = ctx.cloned();
        let spawned = std::thread::Builder::new().name("pdf".into()).spawn(move || {
            let r = run_pdf_job(&job, bytes);
            let _ = tx.send((id, r));
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
        if let Err(e) = spawned {
            let _ = self.tx.send((id, Err(e.to_string())));
        }
        id
    }

    /// The answers that came in since the last call.
    fn answers(&mut self) -> Vec<Answer> {
        let mut out = Vec::new();
        while let Ok((id, r)) = self.rx.try_recv() {
            if let Some(job) = self.open.remove(&id) {
                out.push((id, job, r));
            }
        }
        out
    }
}

impl BimApp {
    /// Asks for `job` on the PDF at `path`, read apart: its bytes as the window reads files, checked against what a file to
    /// import may be. Its id, or why the file can't be read at all.
    pub(crate) fn ask_pdf(&mut self, ctx: Option<&egui::Context>, path: &Path, job: PdfJob) -> Result<u64, String> {
        let given = &self.session.files.given;
        let len = given.size(path).map_err(|e| e.to_string())?;
        if len > bimcraft_core::raster::MAX_FILE as u64 {
            return Err(format!("it is {len} bytes; a file to import is up to {}", bimcraft_core::raster::MAX_FILE));
        }
        let bytes = given.read(path).map_err(|e| e.to_string())?;
        let hosted = self.hosted();
        Ok(self.pdf_reader.ask(ctx, hosted, &path.to_string_lossy(), bytes, job))
    }

    /// The answers to the PDF reads asked for that came in since the last call.
    pub(crate) fn pdf_answers(&mut self) -> Vec<Answer> {
        self.pdf_reader.answers()
    }

    /// Whether a PDF read is under way.
    pub fn pdf_reading(&self) -> bool {
        !self.pdf_reader.open.is_empty()
    }

    /// The asks to read a PDF apart that the page hasn't taken: a hosting page runs each in its worker and answers with
    /// [`BimApp::answer_pdf_request`].
    pub fn take_pdf_requests(&mut self) -> Vec<PdfRequest> {
        std::mem::take(&mut self.pdf_reader.queue)
    }

    /// The page's answer to read `id`: the job's answer as bytes, or why there is none (the worker said so, or died: "the
    /// reader stopped"). An answer to no open read is dropped.
    pub fn answer_pdf_request(&mut self, id: u64, answer: Result<Vec<u8>, String>) {
        if self.pdf_reader.open.contains_key(&id) {
            let _ = self.pdf_reader.tx.send((id, answer));
        }
    }
}

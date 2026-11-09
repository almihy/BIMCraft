//! BIMCraft's own interface (crates/ui-egui) running in a browser page, on a canvas.
#![cfg(target_arch = "wasm32")]
use bimcraft_core::engine::Session;
use bimcraft_ui_egui::BimApp;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct WebApp {
    runner: eframe::WebRunner,
}

#[wasm_bindgen]
impl WebApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WebApp {
        WebApp { runner: eframe::WebRunner::new() }
    }

    /// Starts the interface on `canvas`, with the sample house when `sample`.
    pub async fn start(&self, canvas: web_sys::HtmlCanvasElement, sample: bool) -> Result<(), JsValue> {
        let mut session = Session::new();
        if sample {
            session.script(bimcraft_core::sample::SAMPLE).map_err(|e| JsValue::from_str(&e))?;
            let (model, path) = ((*session.model).clone(), session.path.clone());
            session.reset(model, path);
        }
        let app = BimApp::new(session);
        self.runner.start(canvas, eframe::WebOptions::default(), Box::new(move |_cc| Ok(Box::new(app)))).await
    }
}

impl Default for WebApp {
    fn default() -> WebApp {
        WebApp::new()
    }
}

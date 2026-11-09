//! BIMCraft's engine in the browser: a session the page sends commands to, as the CLI and MCP do.
use std::sync::Arc;

use bimcraft_core::engine::Session;
use bimcraft_core::sample::SAMPLE;
use bimcraft_core::{gltf, ifc, link, render};
use serde_json::Value;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Engine {
    s: Session,
}

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

impl Default for Engine {
    fn default() -> Engine {
        Engine::new()
    }
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine { s: Session::new() }
    }

    /// Runs one command (`name`, its params as JSON); its reply as JSON.
    pub fn execute(&mut self, name: &str, params: &str) -> Result<String, JsError> {
        let p: Value =
            if params.trim().is_empty() { Value::Object(Default::default()) } else { serde_json::from_str(params).map_err(err)? };
        self.s.execute(name, &p).map(|v| v.to_string()).map_err(err)
    }

    /// Runs a script: one `ID {json}` per line.
    pub fn script(&mut self, text: &str) -> Result<String, JsError> {
        self.s.script(text).map(|v| Value::Array(v).to_string()).map_err(err)
    }

    /// Builds the sample house.
    pub fn sample(&mut self) -> Result<(), JsError> {
        self.s.script(SAMPLE).map(|_| ()).map_err(err)
    }

    /// Every command and its parameters, as JSON.
    pub fn commands(&self) -> String {
        self.s.catalog().to_string()
    }

    /// The plan of a level (by name; the lowest when empty) as SVG.
    pub fn plan_svg(&self, level: &str, width: u32, height: u32) -> Result<String, JsError> {
        let m = link::seen(&self.s.model);
        let levels = m.levels();
        let id = if level.is_empty() {
            levels.first().map(|(id, _)| *id)
        } else {
            levels.iter().find(|(_, l)| l.name == level).map(|(id, _)| *id)
        };
        let id = id.ok_or_else(|| err("no such level"))?;
        Ok(render::svg(&render::plan(&m, id, width, height)))
    }

    /// The 3D scene as glTF binary.
    pub fn glb(&self) -> Result<Vec<u8>, JsError> {
        gltf::export(&link::seen(&self.s.model)).map(|o| o.bytes).map_err(err)
    }

    /// The model as IFC4.
    pub fn ifc(&self) -> String {
        ifc::export(&self.s.model)
    }

    /// The project file (.bimcraft, JSON).
    pub fn project(&self) -> Result<String, JsError> {
        self.s.model.to_json().map_err(err)
    }

    /// Opens a project from its JSON text.
    pub fn open_project(&mut self, text: &str) -> Result<String, JsError> {
        let (m, left_out) = bimcraft_core::model::Model::read(text).map_err(err)?;
        self.s = Session::new();
        self.s.model = Arc::new(m);
        Ok(serde_json::json!({ "left_out": left_out }).to_string())
    }
}

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

    /// Links the project `project` (its .bimcraft JSON) under `name`, a reference drawn halftone in the plans, sections and
    /// 3D and never edited: the `link {text, name}` command, an undo step. Linked again under the same name, the newer text
    /// takes its place. Its reply as JSON (`id`, `name`, `elements`, `replaced`).
    pub fn link_text(&mut self, name: &str, project: &str) -> Result<String, JsError> {
        let p = serde_json::json!({"text": project, "name": name});
        self.s.execute("link", &p).map(|v| v.to_string()).map_err(err)
    }

    /// Gives the engine the file `name` (a path as the server names its files: "files/doc_1/Plan.dxf"), its `bytes`: a command
    /// that reads a file (`open`, `link`, `insert`, `import_cad`, `point_cloud`, a material's picture) reads it from there.
    pub fn give_file(&mut self, name: &str, bytes: Vec<u8>) {
        self.s.files.give(name, bytes);
    }

    /// Lets go of the file given as `name`, or of all of them when it is empty.
    pub fn forget_files(&mut self, name: &str) {
        self.s.files.forget(Some(name).filter(|n| !n.is_empty()));
    }

    /// What the commands wrote since the last call (`save` as IFC, glTF, PDF, PNG or SVG; `render` to a path), in place of
    /// files on disk: `[{name, bytes}]`, `bytes` a Uint8Array.
    pub fn take_written(&mut self) -> js_sys::Array {
        let out = js_sys::Array::new();
        for (name, bytes) in self.s.files.take_written() {
            let o = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&o, &"name".into(), &JsValue::from_str(&name));
            let _ = js_sys::Reflect::set(&o, &"bytes".into(), &js_sys::Uint8Array::from(bytes.as_slice()));
            out.push(&o);
        }
        out
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

    /// The model's views a sheet places, in element order, its plan views, then its sections, then its elevations:
    /// `[{"guid", "name", "kind", "level", "scale", "crop"}]`, `kind` "plan", "section" or "elevation", `level` a plan
    /// view's level's name (null for the others), `scale` the n of its 1 : n and `crop` its crop region (a plan's
    /// `[[x, y], [x, y]]`, mm; a section's `[bottom, top]` heights) or null. A level's own plan is not one of them:
    /// `levels` names those.
    pub fn views(&self) -> String {
        let m = &self.s.model;
        let levels = m.levels();
        let scale = |id| bimcraft_core::graphics::scale_of(m, id);
        let plans = m.rare("planview").into_iter().filter_map(|id| {
            let v = m.plan_view(id)?;
            let level = levels.iter().find(|(l, _)| m.guid(*l) == v.level).map(|(_, l)| l.name.clone());
            Some(serde_json::json!({
                "guid": m.guid(id), "name": v.name, "kind": "plan", "level": level, "scale": scale(id), "crop": m.crop_of(id),
            }))
        });
        let cuts = ["section", "elevation"].into_iter().flat_map(|kind| {
            m.rare(kind).into_iter().filter_map(move |id| {
                let v = m.section(id)?;
                Some(serde_json::json!({
                    "guid": m.guid(id), "name": v.name, "kind": kind, "level": null, "scale": scale(id), "crop": v.crop,
                }))
            })
        });
        Value::Array(plans.chain(cuts).collect()).to_string()
    }

    /// View `view` as a sheet's viewport shows it, as SVG `width` px across: a plan view, a section or an elevation by its
    /// guid, or a level's own plan by the level's name (or a view by its name), the lowest level's plan when empty; fitted
    /// to its crop at its scale (`render::viewport`), a section's or elevation's picture inside the SVG. `height` 0 makes
    /// the picture the view's own shape. Replies `{"svg", "view": {"scale", "cx", "cy", "width", "height"}}`: the view it
    /// is drawn through, `scale` pixels to the model's millimetre and (`cx`, `cy`) the model point at its centre (a plan's
    /// mm east and north; a section's along its line from its start, and up), which places anything drawn over it.
    pub fn view_svg(&self, view: &str, width: u32, height: u32) -> Result<String, JsError> {
        let m = link::seen(&self.s.model);
        let id = plan_of(&m, view)?;
        let (d, v) = render::viewport(&m, id, width.max(1), height);
        Ok(serde_json::json!({
            "svg": render::svg(&d),
            "view": {"scale": v.scale, "cx": v.cx, "cy": v.cy, "width": v.width, "height": v.height},
        })
        .to_string())
    }

    /// The plan of a level (by name; the lowest when empty) as PNG.
    pub fn plan_png(&self, level: &str, width: u32, height: u32) -> Result<Vec<u8>, JsError> {
        let m = link::seen(&self.s.model);
        let levels = m.levels();
        let id = if level.is_empty() {
            levels.first().map(|(id, _)| *id)
        } else {
            levels.iter().find(|(_, l)| l.name == level).map(|(id, _)| *id)
        };
        let id = id.ok_or_else(|| err("no such level"))?;
        render::png(&render::plan(&m, id, width, height)).map_err(err)
    }

    /// The levels, lowest first: `[{"name", "elevation"}]` (mm).
    pub fn levels(&self) -> String {
        let levels: Vec<Value> =
            self.s.model.levels().iter().map(|(_, l)| serde_json::json!({"name": l.name, "elevation": l.elevation})).collect();
        Value::Array(levels).to_string()
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

    /// Everything the model holds, as changes that make another copy of it say the same (`bimcraft_core::sync`):
    /// `{"elements": {guid: json}, "project": {key: json}, "complete": true}`. A link from a project's text is left out.
    pub fn entries(&self) -> Result<String, JsError> {
        bimcraft_core::sync::entries(&self.s.model).map(|c| c.to_json()).map_err(err)
    }

    /// Takes in another copy's changes (`{"elements": {guid: json | null}, "project": {key: json | null}, "complete"?}`):
    /// the model checked, and what would break it left out. Replies `{"taken": n, "refused": [{what, error}], "fixes": {key: json text | null}, "kept": [{entry, kind, name, uses}]}`: the `fixes` are what this copy tells the room that it did not hear, and `kept` the entries it kept though they were taken out, because something names them (see `Merged`).
    pub fn apply_changes(&mut self, changes: &str) -> Result<String, JsError> {
        let c = bimcraft_core::sync::Changes::read(changes).map_err(err)?;
        let merged = self.s.merge(&c, &Default::default()).map_err(err)?;
        Ok(merged.to_json().to_string())
    }

    /// How this engine tells another copy of a project its settings (`bimcraft_core::sync::FORMAT`): a room and a page whose
    /// numbers differ do not edit together.
    pub fn sync_format() -> u32 {
        bimcraft_core::sync::FORMAT
    }

    /// What changed from the project `before` (its .bimcraft JSON) to `after`, as another copy is told it: the engine's
    /// `sync::changes`, `{"elements": {guid: json | null}, "project": {key: json | null}}`. A change made outside the room (the
    /// agent's, a save over the API) travels exactly as a page's own does.
    pub fn changes(before: &str, after: &str) -> Result<String, JsError> {
        let (before, _) = bimcraft_core::model::Model::read(before).map_err(err)?;
        let (after, _) = bimcraft_core::model::Model::read(after).map_err(err)?;
        bimcraft_core::sync::changes(&before, &after).map(|c| c.to_json()).map_err(err)
    }

    /// Opens a project from its JSON text.
    pub fn open_project(&mut self, text: &str) -> Result<String, JsError> {
        let (m, left_out) = bimcraft_core::model::Model::read(text).map_err(err)?;
        // The files given stay: they are given to the engine, not to a project.
        let files = std::mem::take(&mut self.s.files);
        self.s = Session::new();
        self.s.files = files;
        self.s.model = Arc::new(m);
        Ok(serde_json::json!({ "left_out": left_out }).to_string())
    }
}

/// The view `view` names: a plan view, section or elevation by its guid, else a level by its name, else a view by its name;
/// the lowest level's plan when it is empty.
fn plan_of(m: &bimcraft_core::model::Model, view: &str) -> Result<bimcraft_core::model::Id, JsError> {
    use bimcraft_core::model::Element;
    let view = view.trim();
    let levels = m.levels();
    let found = if view.is_empty() {
        levels.first().map(|(id, _)| *id)
    } else if let Some(g) = bimcraft_core::ids::Guid::parse(view) {
        m.elements.id(g).filter(|&id| m.plan_level(id).is_some() || m.section(id).is_some())
    } else {
        let named = |(id, e): (&bimcraft_core::model::Id, &Element)| match e {
            Element::PlanView(v) if v.name == view => Some(*id),
            Element::Section(v) | Element::Elevation(v) if v.name == view => Some(*id),
            _ => None,
        };
        levels.iter().find(|(_, l)| l.name == view).map(|(id, _)| *id).or_else(|| m.elements.iter().find_map(named))
    };
    found.ok_or_else(|| err(format!("no view or level `{view}`")))
}

/// The Manifold trial (docs/manifold-trial.md): a 100 mm box minus a cylinder, cut by Manifold running in this module:
/// `{"volume", "triangles"}` as JSON.
#[wasm_bindgen]
pub fn manifold_trial() -> String {
    let t = bimcraft_geom::solid::solid_trial();
    serde_json::json!({ "volume": t.volume, "triangles": t.triangles }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slide's plan block names a plan view by its guid, or a level by its name: the views listed, and either drawn with
    /// the view it is drawn through.
    #[test]
    fn a_plan_view_is_listed_and_drawn_with_its_view() {
        let mut e = Engine::new();
        e.sample().unwrap();
        e.execute("duplicate_view", r#"{"name": "Presentation plan"}"#).unwrap();
        let views: Value = serde_json::from_str(&e.views()).unwrap();
        let v = views.as_array().unwrap().iter().find(|v| v["name"] == "Presentation plan").expect("listed");
        assert!(v["level"].is_string() && v["scale"].as_f64().is_some_and(|n| n > 0.0), "{v}");
        let guid = v["guid"].as_str().unwrap();
        for named in [guid, v["level"].as_str().unwrap(), ""] {
            let got: Value = serde_json::from_str(&e.view_svg(named, 1000, 0).unwrap()).unwrap();
            assert!(got["svg"].as_str().unwrap().starts_with("<svg"), "{named}");
            let view = &got["view"];
            assert_eq!(view["width"], 1000);
            assert!(view["height"].as_u64().unwrap() > 0 && view["scale"].as_f64().unwrap() > 0.0, "{view}");
            assert!(view["cx"].is_number() && view["cy"].is_number());
        }
        // A section is listed with its kind and drawn as a sheet draws it: its picture inside the SVG.
        e.execute("section", r#"{"start": [-2000, 3000], "end": [14000, 3000], "name": "Section A"}"#).unwrap();
        let views: Value = serde_json::from_str(&e.views()).unwrap();
        let cut = views.as_array().unwrap().iter().find(|v| v["name"] == "Section A").expect("listed");
        assert_eq!(cut["kind"], "section");
        let got: Value = serde_json::from_str(&e.view_svg(cut["guid"].as_str().unwrap(), 1000, 0).unwrap()).unwrap();
        assert!(got["svg"].as_str().unwrap().contains("<image"), "its picture");
        assert_eq!(got["view"]["width"], 1000);
    }
}

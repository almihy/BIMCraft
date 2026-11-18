//! A wall type's Type Properties as Revit's, and its Edit Assembly window: the layers from the exterior side to the interior
//! side with the two Core Boundary rows among them, each layer's Function, Material, Thickness, Wraps and Structural
//! Material; Insert, Delete, Up and Down on the row picked; Default Wrapping at Inserts and at Ends. What is typed here is
//! a draft: OK puts it in the Type Properties dialog, whose OK or Apply runs the `type` command.

use bimcraft_core::model::{Function, Model, WallFunction, WallType, Wrapping};
use bimcraft_core::units::Units;
use bimcraft_i18n::msg;
use egui::{Button, Context, Grid, Id as UiId, RichText, TextEdit, Ui, WidgetInfo, WidgetType, vec2};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::style::pal;

/// What Revit calls a layer with no material of its own.
const BY_CATEGORY: &str = msg!("<By Category>");

/// A layer as typed: what it does, what it's made of ("" for none), how thick, whether it wraps and whether its material is
/// the type's structural material.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Row {
    pub(crate) function: Function,
    pub(crate) material: String,
    pub(crate) thickness: String,
    pub(crate) wraps: bool,
    pub(crate) structural: bool,
}

/// A wall type's structure as typed: its layers, exterior side first, how many lie outside the core on each side, and its
/// default wrapping.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Assembly {
    pub(crate) rows: Vec<Row>,
    pub(crate) exterior: usize,
    pub(crate) interior: usize,
    pub(crate) wrap_inserts: Wrapping,
    pub(crate) wrap_ends: Wrapping,
}

/// A line of Edit Assembly's table: a layer, or one of the two Core Boundary rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    Layer(usize),
    /// The boundary on the exterior side of the core (`false`) or on its interior side.
    Boundary(bool),
}

impl Assembly {
    fn of(t: &WallType, units: Units) -> Assembly {
        let rows: Vec<Row> = match t.layers.as_slice() {
            // A type without layers is one structure layer as thick as it.
            [] => vec![Row {
                function: Function::Structure,
                material: String::new(),
                thickness: units.length(t.thickness),
                wraps: false,
                structural: false,
            }],
            layers => layers
                .iter()
                .map(|l| Row {
                    function: l.function,
                    material: l.material.clone(),
                    thickness: units.length(l.thickness),
                    wraps: l.wraps,
                    structural: l.structural,
                })
                .collect(),
        };
        let core = if t.layers.is_empty() { 0..1 } else { t.core_layers() };
        Assembly { exterior: core.start, interior: rows.len() - core.end, rows, wrap_inserts: t.wrap_inserts, wrap_ends: t.wrap_ends }
    }

    /// The table's lines, top to bottom: the exterior layers, a boundary, the core's, a boundary, the interior ones.
    fn lines(&self) -> Vec<Line> {
        let n = self.rows.len();
        let mut out: Vec<Line> = (0..self.exterior).map(Line::Layer).collect();
        out.push(Line::Boundary(false));
        out.extend((self.exterior..n - self.interior).map(Line::Layer));
        out.push(Line::Boundary(true));
        out.extend((n - self.interior..n).map(Line::Layer));
        out
    }

    fn in_core(&self, k: usize) -> bool {
        k >= self.exterior && k < self.rows.len() - self.interior
    }

    fn core_count(&self) -> usize {
        self.rows.len() - self.exterior - self.interior
    }

    /// How thick it is, when every thickness typed is a length.
    pub(crate) fn thickness(&self, units: Units) -> Option<f64> {
        self.rows.iter().map(|r| units.parse(&r.thickness)).sum()
    }

    /// The material ticked Structural Material, or Revit's word for none.
    pub(crate) fn structural_material(&self) -> &str {
        self.rows.iter().find(|r| r.structural).map(|r| r.material.as_str()).filter(|m| !m.is_empty()).unwrap_or(BY_CATEGORY)
    }

    /// A new layer above line `at` (none picked: at the top of the core), as Revit's Insert makes it: Structure [1], no
    /// material, no thickness yet. It is in the core when the line under it is, or is the core's interior boundary. Which
    /// line it is.
    fn insert(&mut self, at: Option<usize>) -> usize {
        let lines = self.lines();
        let line = at.and_then(|i| lines.get(i).copied()).unwrap_or(Line::Layer(self.exterior));
        let n = self.rows.len();
        let k = match line {
            Line::Layer(k) => k,
            Line::Boundary(false) => self.exterior,
            Line::Boundary(true) => n - self.interior,
        };
        match line {
            Line::Layer(k) if k < self.exterior => self.exterior += 1,
            Line::Boundary(false) => self.exterior += 1,
            Line::Layer(k) if k >= n - self.interior => self.interior += 1,
            _ => {}
        }
        let row = Row { function: Function::Structure, material: String::new(), thickness: "0".into(), wraps: false, structural: false };
        self.rows.insert(k.min(n), row);
        self.lines().iter().position(|l| *l == Line::Layer(k.min(n))).unwrap_or(0)
    }

    /// Whether line `at` is a layer Delete may take: any but the core's only one.
    fn can_delete(&self, at: usize) -> bool {
        matches!(self.lines().get(at), Some(Line::Layer(k)) if !(self.in_core(*k) && self.core_count() == 1))
    }

    fn delete(&mut self, at: usize) {
        if !self.can_delete(at) {
            return;
        }
        if let Some(Line::Layer(k)) = self.lines().get(at).copied() {
            if k < self.exterior {
                self.exterior -= 1;
            } else if !self.in_core(k) {
                self.interior -= 1;
            }
            self.rows.remove(k);
        }
    }

    /// Whether the layer on line `at` may go up a line (`down`: down one): past another layer, or across a Core Boundary
    /// row into or out of the core, which keeps one layer at least.
    fn can_move(&self, at: usize, down: bool) -> bool {
        let lines = self.lines();
        let Some(Line::Layer(k)) = lines.get(at).copied() else { return false };
        let to = if down { at.checked_add(1) } else { at.checked_sub(1) };
        match to.and_then(|i| lines.get(i)) {
            Some(Line::Layer(_)) => true,
            // Out of the core only when it leaves a layer there.
            Some(Line::Boundary(_)) => !self.in_core(k) || self.core_count() > 1,
            None => false,
        }
    }

    /// The layer on line `at` moved a line; the line it is on after.
    fn shift(&mut self, at: usize, down: bool) -> usize {
        if !self.can_move(at, down) {
            return at;
        }
        let lines = self.lines();
        let to = if down { at + 1 } else { at - 1 };
        match (lines.get(at).copied(), lines.get(to).copied()) {
            (Some(Line::Layer(a)), Some(Line::Layer(b))) => self.rows.swap(a, b),
            // Up past the exterior boundary: out of the core. Down past it: into the core.
            (Some(Line::Layer(_)), Some(Line::Boundary(false))) => {
                if down {
                    self.exterior -= 1;
                } else {
                    self.exterior += 1;
                }
            }
            (Some(Line::Layer(_)), Some(Line::Boundary(true))) => {
                if down {
                    self.interior += 1;
                } else {
                    self.interior -= 1;
                }
            }
            _ => {}
        }
        // A layer taken into the core wraps nothing; one taken out of it is no structural material.
        for k in 0..self.rows.len() {
            let core = self.in_core(k);
            if let Some(r) = self.rows.get_mut(k) {
                r.wraps &= !core;
                r.structural &= core;
            }
        }
        to
    }

    /// The `type` command's `layers` and `core` for it, or which thickness is no length.
    fn params(&self, units: Units) -> Result<(Value, Value), String> {
        let layers = self
            .rows
            .iter()
            .map(|r| {
                let thickness = units.parse(&r.thickness).ok_or_else(|| tf("{}: `{}` is not a length", &[t("Thickness"), &r.thickness]))?;
                let mut l = json!({"function": r.function, "material": r.material.trim(), "thickness": thickness});
                if r.wraps {
                    l["wraps"] = json!(true);
                }
                if r.structural {
                    l["structural"] = json!(true);
                }
                Ok(l)
            })
            .collect::<Result<Vec<Value>, String>>()?;
        // Core Boundary rows where a type that sets none has them say nothing: the core is from the first structure layer to
        // the last by itself.
        let is = |r: &Row| r.function == Function::Structure;
        let implied = match (self.rows.iter().position(is), self.rows.iter().rposition(is)) {
            (Some(a), Some(b)) => (a, self.rows.len() - 1 - b),
            _ => (0, 0),
        };
        let core = if implied == (self.exterior, self.interior) { Value::Null } else { json!({"exterior": self.exterior, "interior": self.interior}) };
        Ok((Value::Array(layers), core))
    }
}

/// The Edit Assembly window while it is open: its draft, and the line picked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Editing {
    draft: Assembly,
    picked: Option<usize>,
    error: Option<String>,
}

/// A wall type's parameters as Type Properties shows them, as typed and as they were.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WallForm {
    pub(crate) assembly: Assembly,
    pub(crate) function: WallFunction,
    /// The Coarse Scale Fill Pattern's name, "" for none.
    pub(crate) coarse_fill: String,
    pub(crate) coarse_color: [u8; 3],
    pub(crate) editing: Option<Editing>,
}

impl WallForm {
    pub(crate) fn of(m: &Model, name: &str) -> WallForm {
        let none = WallType::default();
        let t = m.types.wall.get(name).unwrap_or(&none);
        WallForm {
            assembly: Assembly::of(t, m.units),
            function: t.function,
            coarse_fill: t.coarse_fill.clone().unwrap_or_default(),
            coarse_color: t.coarse_color,
            editing: None,
        }
    }

    /// What the `type` command takes for what changed since `was` (for a `copy` all of it, its `layers` too when asked), or why
    /// something typed is wrong.
    pub(crate) fn params(&self, was: &WallForm, units: Units, layers: bool, copy: bool) -> Result<serde_json::Map<String, Value>, String> {
        let mut p = serde_json::Map::new();
        let (a, b) = (&self.assembly, &was.assembly);
        if layers || a.rows != b.rows || (a.exterior, a.interior) != (b.exterior, b.interior) {
            let (layers, core) = a.params(units)?;
            p.insert("layers".into(), layers);
            p.insert("core".into(), core);
        }
        if copy || a.wrap_inserts != b.wrap_inserts {
            p.insert("wrap_inserts".into(), json!(a.wrap_inserts.name()));
        }
        if copy || a.wrap_ends != b.wrap_ends {
            p.insert("wrap_ends".into(), json!(a.wrap_ends.name()));
        }
        if copy || self.function != was.function {
            p.insert("function".into(), json!(self.function.name()));
        }
        if copy || self.coarse_fill != was.coarse_fill {
            p.insert("coarse_fill".into(), if self.coarse_fill.is_empty() { Value::Null } else { json!(self.coarse_fill) });
        }
        if copy || self.coarse_color != was.coarse_color {
            p.insert("coarse_color".into(), json!(self.coarse_color));
        }
        Ok(p)
    }

    /// Opens Edit Assembly on a draft of the structure.
    pub(crate) fn edit(&mut self) {
        self.editing = Some(Editing { draft: self.assembly.clone(), picked: None, error: None });
    }

    /// The Edit Assembly window, while it is open, over Type Properties.
    pub(crate) fn window(&mut self, ctx: &Context, type_name: &str, units: Units, materials: &[String]) {
        let Some(mut e) = self.editing.take() else { return };
        let (mut ok, mut cancel) = (false, false);
        let r = Modal::new(UiId::new("edit-assembly"), t("Edit Assembly")).width(680.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                Grid::new("assembly-head").num_columns(2).spacing(vec2(12.0, 4.0)).show(ui, |ui| {
                    ui.label(t("Family:"));
                    ui.label(t("Basic Wall"));
                    ui.end_row();
                    ui.label(t("Type:"));
                    ui.label(type_name);
                    ui.end_row();
                    ui.label(t("Total thickness:"));
                    ui.label(e.draft.thickness(units).map_or_else(|| "—".to_string(), |x| units.length(x)));
                    ui.end_row();
                });
                ui.add_space(4.0);
                ui.label(RichText::new(t("Layers")).strong());
                ui.vertical_centered(|ui| ui.label(RichText::new(t("EXTERIOR SIDE")).color(pal().muted).small()));
                layers(ui, &mut e, units, materials);
                ui.vertical_centered(|ui| ui.label(RichText::new(t("INTERIOR SIDE")).color(pal().muted).small()));
                ui.horizontal(|ui| {
                    let size = vec2(76.0, 0.0);
                    let at = e.picked;
                    if ui.add(Button::new(t("Insert")).min_size(size)).clicked() {
                        e.picked = Some(e.draft.insert(at));
                    }
                    if ui.add_enabled(at.is_some_and(|i| e.draft.can_delete(i)), Button::new(t("Delete")).min_size(size)).clicked() {
                        e.draft.delete(at.unwrap_or_default());
                        e.picked = None;
                    }
                    for (label, down) in [(msg!("Up"), false), (msg!("Down"), true)] {
                        if ui.add_enabled(at.is_some_and(|i| e.draft.can_move(i, down)), Button::new(t(label)).min_size(size)).clicked() {
                            e.picked = at.map(|i| e.draft.shift(i, down));
                        }
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new(t("Default Wrapping")).strong());
                Grid::new("assembly-wrapping").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                    let l = ui.label(t("At Inserts:"));
                    wrapping(ui, "assembly-inserts", l.id, &mut e.draft.wrap_inserts, true);
                    ui.end_row();
                    let l = ui.label(t("At Ends:"));
                    wrapping(ui, "assembly-ends", l.id, &mut e.draft.wrap_ends, false);
                    ui.end_row();
                });
                crate::typeprops::error(ui, e.error.as_deref());
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let size = vec2(76.0, 0.0);
                    cancel = ui.add(Button::new(t("Cancel")).min_size(size)).clicked();
                    ok = ui.add(Button::new(t("OK")).min_size(size)).clicked();
                });
            });
        });
        if ok {
            // Every thickness a length, before Type Properties takes the draft.
            match e.draft.params(units) {
                Ok(_) => {
                    self.assembly = e.draft;
                    return;
                }
                Err(why) => e.error = Some(why),
            }
        }
        if !(cancel || r.should_close()) {
            self.editing = Some(e);
        }
    }
}

/// How a wrapping reads in its list: at inserts Revit's "Do not wrap", at ends "None".
pub(crate) fn wrapping_label(w: Wrapping, inserts: bool) -> &'static str {
    match w {
        Wrapping::None if !inserts => msg!("None"),
        w => w.label(),
    }
}

/// A list of the wrappings: all four at inserts, without Both at ends.
pub(crate) fn wrapping(ui: &mut Ui, salt: &str, label: UiId, value: &mut Wrapping, inserts: bool) {
    let c = crate::icons::combo(salt).selected_text(t(wrapping_label(*value, inserts))).width(140.0).show_ui(ui, |ui| {
        for w in Wrapping::ALL.into_iter().filter(|w| inserts || *w != Wrapping::Both) {
            if ui.selectable_label(*value == w, t(wrapping_label(w, inserts))).clicked() {
                *value = w;
            }
        }
    });
    c.response.labelled_by(label);
}

/// Edit Assembly's table: a numbered line for each layer and for the two Core Boundary rows; a click on a line's number
/// picks it.
fn layers(ui: &mut Ui, e: &mut Editing, units: Units, materials: &[String]) {
    let muted = |s: &str| RichText::new(s).color(pal().muted);
    let mut structural: Option<usize> = None;
    Grid::new("assembly-layers").num_columns(6).striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
        ui.label("");
        let function = ui.label(muted(t("Function")));
        let material = ui.label(muted(t("Material")));
        let thickness = ui.label(muted(t("Thickness")));
        let wraps = ui.label(muted(t("Wraps")));
        let structure = ui.label(muted(t("Structural Material")));
        ui.end_row();
        for (i, line) in e.draft.lines().into_iter().enumerate() {
            let number = (i + 1).to_string();
            let pick = ui.selectable_label(e.picked == Some(i), &number);
            let name = tf("Row {}", &[&number]);
            let on = e.picked == Some(i);
            pick.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, &name));
            if pick.clicked() {
                e.picked = Some(i);
            }
            match line {
                Line::Boundary(interior) => {
                    ui.label(t("Core Boundary"));
                    ui.label(t(if interior { msg!("Layers Below Wrap") } else { msg!("Layers Above Wrap") }));
                    ui.label(units.length(0.0));
                    ui.label("");
                    ui.label("");
                }
                Line::Layer(k) => {
                    let core = e.draft.in_core(k);
                    let Some(row) = e.draft.rows.get_mut(k) else { continue };
                    let c = crate::icons::combo(("assembly-function", k)).selected_text(t(row.function.label())).width(160.0).show_ui(
                        ui,
                        |ui| {
                            for f in Function::ALL {
                                if ui.selectable_label(row.function == f, t(f.label())).clicked() {
                                    row.function = f;
                                }
                            }
                        },
                    );
                    c.response.labelled_by(function.id);
                    let shown = if row.material.is_empty() { t(BY_CATEGORY) } else { row.material.as_str() };
                    let c = crate::icons::combo(("assembly-material", k)).selected_text(shown).width(150.0).show_ui(ui, |ui| {
                        if ui.selectable_label(row.material.is_empty(), t(BY_CATEGORY)).clicked() {
                            row.material.clear();
                        }
                        for n in materials {
                            if ui.selectable_label(row.material == *n, n).clicked() {
                                row.material.clone_from(n);
                            }
                        }
                    });
                    c.response.labelled_by(material.id);
                    ui.add_sized([70.0, 20.0], TextEdit::singleline(&mut row.thickness).hint_text(units.name())).labelled_by(thickness.id);
                    // Only a layer outside the core wraps; only one in it is the structural material.
                    let r = ui.add_enabled(!core, egui::Checkbox::without_text(&mut row.wraps));
                    let now = row.wraps;
                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, !core, now, t("Wraps")));
                    r.labelled_by(wraps.id);
                    let r = ui.add_enabled(core, egui::Checkbox::without_text(&mut row.structural));
                    let now = row.structural;
                    r.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, core, now, t("Structural Material")));
                    if r.labelled_by(structure.id).changed() && now {
                        structural = Some(k);
                    }
                }
            }
            ui.end_row();
        }
    });
    // One structural material: the one just ticked.
    if let Some(k) = structural {
        for (j, row) in e.draft.rows.iter_mut().enumerate() {
            row.structural = j == k;
        }
    }
}

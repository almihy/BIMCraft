//! Manage > Object Styles, as Revit's dialog: the pen each model category draws its lines in, in projection and where a view
//! cuts it, and under each category the subcategories its families bring (`Frame/Mullion`, `Panel`, `Plan Swing`), each with its
//! own line weights, colour, line pattern and material; edited as a draft and applied as one `object_styles` command (OK and
//! Apply), or dropped (Cancel).

use crate::popup::Modal;

use std::collections::BTreeMap;

use bimcraft_core::graphics::MODEL;
use bimcraft_core::model::Model;
use bimcraft_core::pen::{MAX_PEN, Pen};
use bimcraft_core::styles::{ObjectStyle, SubcategoryStyle, default_of};
use egui::{Align, Context, Grid, Id as UiId, Layout, RichText, ScrollArea, Ui, WidgetInfo, WidgetType};
use serde_json::{Value, json};

use crate::BimApp;
use crate::graphics::pen_name;
use crate::lang::{category, t};

/// One subcategory's row: where it is (`Category/Subcategory`) and the settings as edited.
struct SubRow {
    key: String,
    category: String,
    name: String,
    style: SubcategoryStyle,
}

/// The Object Styles dialog: every model category's pens, and each subcategory's settings, as edited and not yet applied.
pub(crate) struct ObjectStylesDialog {
    draft: BTreeMap<&'static str, ObjectStyle>,
    subs: Vec<SubRow>,
    /// The names the pattern and material pickers offer: the project's.
    patterns: Vec<String>,
    materials: Vec<String>,
}

impl ObjectStylesDialog {
    /// Open on the project's object styles.
    pub(crate) fn new(m: &Model) -> ObjectStylesDialog {
        ObjectStylesDialog {
            draft: MODEL.iter().map(|c| (*c, m.object_style(c))).collect(),
            subs: m
                .subcategory_rows()
                .into_iter()
                .map(|(key, category, name)| SubRow {
                    style: m.subcategory_styles.get(&key).cloned().unwrap_or_default(),
                    key,
                    category,
                    name,
                })
                .collect(),
            patterns: m.line_patterns.keys().cloned().collect(),
            materials: m.materials.keys().cloned().collect(),
        }
    }
}

/// A drop-down of the pens, named `name` for screen readers.
fn pen_field(ui: &mut Ui, pen: &mut Pen, name: String) {
    let c = crate::icons::combo(&name).selected_text(pen_name(*pen)).width(120.0).show_ui(ui, |ui| {
        for each in (0..=MAX_PEN).map(Pen) {
            ui.selectable_value(pen, each, pen_name(each));
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
}

/// A drop-down of a subcategory's pen: its category's, or one of Revit's weights (1 to 16: it has none for a hairline).
fn sub_pen_field(ui: &mut Ui, pen: &mut Option<Pen>, name: String) {
    let none = t("By Category").to_string();
    let shown = pen.map_or_else(|| none.clone(), pen_name);
    let c = crate::icons::combo(&name).selected_text(shown).width(120.0).show_ui(ui, |ui| {
        ui.selectable_value(pen, None, none.clone());
        for each in (1..=MAX_PEN).map(Pen) {
            ui.selectable_value(pen, Some(each), pen_name(each));
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
}

/// A drop-down of names, none being `none`: the pattern or the material a subcategory is in.
fn name_field(ui: &mut Ui, chosen: &mut Option<String>, names: &[String], none: String, name: String) {
    let shown = chosen.clone().unwrap_or_else(|| none.clone());
    let c = crate::icons::combo(&name).selected_text(shown).width(140.0).show_ui(ui, |ui| {
        ui.selectable_value(chosen, None, none.clone());
        for each in names {
            ui.selectable_value(chosen, Some(each.clone()), each.as_str());
        }
    });
    c.response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name.clone()));
}

/// A subcategory's colour: a swatch to pick it from, and a small button taking it back to its category's.
fn color_field(ui: &mut Ui, color: &mut Option<[u8; 3]>, name: String) {
    ui.horizontal(|ui| {
        let mut shown = color.unwrap_or([0, 0, 0]);
        let r = ui.color_edit_button_srgb(&mut shown);
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
        if r.changed() {
            *color = Some(shown);
        }
        if color.is_some() && ui.small_button("×").on_hover_text(t("By Category")).clicked() {
            *color = None;
        }
    });
}

/// The settings of `row` as a command gives them: each, null where it is none.
fn given(style: &SubcategoryStyle) -> Value {
    json!({"projection": style.projection, "cut": style.cut, "color": style.color, "pattern": style.pattern, "material": style.material})
}

impl BimApp {
    /// The Object Styles dialog, while it's open.
    pub(crate) fn object_styles_window(&mut self, ctx: &Context) {
        let Some(mut d) = self.object_styles_dialog.take() else { return };
        let (mut ok, mut apply, mut cancel) = (false, false, false);
        let r = Modal::new(UiId::new("object-styles"), t("Object Styles")).width(860.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.label(t(
                    "The pen each category draws its lines in, seen and cut, in every plan and section. A view's Visibility/Graphics overrides them.",
                ));
                ui.add_space(4.0);
                ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
                    Grid::new("object-styles").striped(true).num_columns(6).show(ui, |ui| {
                        for head in ["Category", "Projection", "Cut", "Color", "Line Pattern", "Material"] {
                            ui.label(RichText::new(t(head)).strong());
                        }
                        ui.end_row();
                        let ObjectStylesDialog { draft, subs, patterns, materials } = &mut d;
                        // A category's own row, then the subcategories its families bring, indented under it. A category the list
                        // has none of (a family's `component`) comes after.
                        let mut shown: Vec<(String, String)> = draft.keys().map(|c| (category(c, true), c.to_string())).collect();
                        for row in subs.iter() {
                            if !draft.contains_key(row.category.as_str()) && !shown.iter().any(|(_, c)| *c == row.category) {
                                shown.push((category(&row.category, true), row.category.clone()));
                            }
                        }
                        shown.sort();
                        for (name, c) in shown {
                            ui.label(&name);
                            match draft.iter_mut().find(|(k, _)| **k == c) {
                                Some((_, style)) => {
                                    pen_field(ui, &mut style.projection, format!("{name}: {}", t("Projection")));
                                    pen_field(ui, &mut style.cut, format!("{name}: {}", t("Cut")));
                                }
                                None => {
                                    ui.label("");
                                    ui.label("");
                                }
                            }
                            for _ in 0..3 {
                                ui.label("");
                            }
                            ui.end_row();
                            for row in subs.iter_mut().filter(|r| r.category == c) {
                                let which = format!("{name}: {}", row.name);
                                ui.horizontal(|ui| {
                                    ui.add_space(16.0);
                                    ui.label(&row.name);
                                });
                                sub_pen_field(ui, &mut row.style.projection, format!("{which}: {}", t("Projection")));
                                sub_pen_field(ui, &mut row.style.cut, format!("{which}: {}", t("Cut")));
                                color_field(ui, &mut row.style.color, format!("{which}: {}", t("Color")));
                                name_field(ui, &mut row.style.pattern, patterns, t("Solid").to_string(), format!("{which}: {}", t("Line Pattern")));
                                name_field(ui, &mut row.style.material, materials, t("None").to_string(), format!("{which}: {}", t("Material")));
                                ui.end_row();
                            }
                        }
                    });
                });
            });
            crate::popup::footer(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(t("Reset")).on_hover_text(t("Back to the pens a project starts with")).clicked() {
                        for (c, s) in &mut d.draft {
                            *s = default_of(c);
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        apply = ui.button(t("Apply")).clicked();
                        cancel = ui.button(t("Cancel")).clicked();
                        ok = ui.button(t("OK")).clicked();
                    });
                });
            });
        });
        // What changed, as one command: every category whose pens aren't the project's now, every subcategory whose settings
        // aren't. A refusal keeps the dialog open to fix it.
        let mut failed = false;
        if ok || apply {
            let m = &self.session.model;
            let changed: serde_json::Map<String, Value> = (d.draft.iter())
                .filter(|(c, s)| m.object_style(c) != **s)
                .map(|(c, s)| (c.to_string(), json!({"projection": s.projection, "cut": s.cut})))
                .collect();
            let subs: serde_json::Map<String, Value> = (d.subs.iter())
                .filter(|r| m.subcategory_styles.get(&r.key).cloned().unwrap_or_default() != r.style)
                .map(|r| (r.key.clone(), given(&r.style)))
                .collect();
            let mut params = serde_json::Map::new();
            if !changed.is_empty() {
                params.insert("categories".into(), Value::Object(changed));
            }
            if !subs.is_empty() {
                params.insert("subcategories".into(), Value::Object(subs));
            }
            if !params.is_empty() {
                failed = self.run("object_styles", Value::Object(params)).is_none();
            }
        }
        if failed || !(ok || cancel || r.should_close()) {
            self.object_styles_dialog = Some(d);
        } else if self.vg_dialog.is_some() {
            // Visibility/Graphics, which opened it, comes back on the next frame.
            ctx.request_repaint();
        }
    }
}

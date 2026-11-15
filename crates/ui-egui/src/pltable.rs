//! Property Line's dialogs, as Revit's: Create Property Line (by entering distances and bearings, or by sketching), the
//! Edit Table of a property line's segments (distance, bearing, line or arc, radius, L/R; Insert, Delete, Up, Down and Add
//! Line to Close), and our addition, each segment's setback and what it faces, which draw its buildable line. Each makes its
//! change with the core's `property_line` and `property_line_edit`.

use bimcraft_core::ground;
use bimcraft_core::masterplan::EDGE_KINDS;
use bimcraft_core::model::{Element, Id};
use bimcraft_core::units::Units;
use egui::{Align, Button, Context, Grid, Id as UiId, Layout, RichText, ScrollArea, TextEdit};
use serde_json::{Value, json};

use crate::lang::{t, tf};
use crate::popup::Modal;
use crate::sketch::Of;
use crate::site::SiteTool;
use crate::style::pal;
use crate::{BimApp, Status, Tool};

/// A row of the Edit Table, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Row {
    pub(crate) distance: String,
    pub(crate) bearing: String,
    pub(crate) arc: bool,
    pub(crate) radius: String,
    /// The arc's centre on the right of the chord (R), else on its left (L).
    pub(crate) right: bool,
}

/// The Edit Table: the property line it edits (none: a new one on `level`), where it starts, its rows, the row picked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Table {
    pub(crate) id: Option<Id>,
    pub(crate) level: Id,
    pub(crate) start: String,
    pub(crate) rows: Vec<Row>,
    pub(crate) picked: usize,
}

/// A segment's setback and what it faces, as typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct EdgeRow {
    pub(crate) segment: String,
    pub(crate) setback: String,
    pub(crate) kind: String,
    pub(crate) street: String,
    pub(crate) width: String,
}

/// Property Line's dialog open now.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PlDialog {
    /// Revit's Create Property Line: how it is made, on `level`.
    Choose { level: Id },
    Table(Table),
    Setbacks { id: Id, rows: Vec<EdgeRow> },
}

/// What a row's bearing comes to, an azimuth in degrees, and its distance, mm.
fn read_row(r: &Row, units: Units, i: usize) -> Result<(f64, f64), String> {
    let d = units.parse(r.distance.trim()).filter(|d| *d > 0.0).ok_or_else(|| tf("Row {}: the distance is a length", &[&(i + 1).to_string()]))?;
    let text = r.bearing.trim();
    let az = text
        .parse::<f64>()
        .ok()
        .map(Value::from)
        .or_else(|| Some(Value::from(text)))
        .and_then(|v| ground::bearing(&v))
        .ok_or_else(|| tf("Row {}: the bearing is N 45°30'00\" E, or degrees from north", &[&(i + 1).to_string()]))?;
    Ok((d, az))
}

/// The rows as the core's `table` takes them.
fn table_json(rows: &[Row], units: Units) -> Result<Vec<Value>, String> {
    rows.iter()
        .enumerate()
        .map(|(i, r)| {
            let (d, az) = read_row(r, units, i)?;
            let mut v = json!({"distance": d, "bearing": az});
            if r.arc {
                let radius = units.parse(r.radius.trim()).filter(|x| *x > 0.0).ok_or_else(|| tf("Row {}: an arc needs its radius", &[&(i + 1).to_string()]))?;
                v["type"] = json!("arc");
                v["radius"] = json!(radius);
                v["side"] = json!(if r.right { "R" } else { "L" });
            }
            Ok(v)
        })
        .collect()
}

/// `x, y` in the project's units.
fn read_point(text: &str, units: Units) -> Option<[f64; 2]> {
    let mut it = text.split(',').map(|s| units.parse(s.trim()));
    let (x, y) = (it.next()??, it.next()??);
    it.next().is_none().then_some([x, y])
}

/// Revit's Add Line to Close: the row from where the rows end back to where they began, when they don't close.
pub(crate) fn closing_row(rows: &[Row], units: Units) -> Result<Option<Row>, String> {
    let (mut east, mut north) = (0.0, 0.0);
    for (i, r) in rows.iter().enumerate() {
        let (d, az) = read_row(r, units, i)?;
        east += d * az.to_radians().sin();
        north += d * az.to_radians().cos();
    }
    let gap = east.hypot(north);
    if gap <= 1.0 {
        return Ok(None);
    }
    let az = (-east).atan2(-north).to_degrees().rem_euclid(360.0);
    Ok(Some(Row { distance: units.length(gap), bearing: ground::bearing_text(az), ..Row::default() }))
}

impl BimApp {
    /// Property Line picked in a plan: Revit's Create Property Line dialog, asking how it is made.
    pub(crate) fn property_line_choice(&mut self, level: Id) {
        self.pl_dialog = Some(PlDialog::Choose { level });
    }

    /// Edit Table on property line `id`: its segments as rows (a sketched one's too, as Revit converts it).
    pub(crate) fn open_property_line_table(&mut self, id: Id) {
        let m = &self.session.model;
        let Some(Element::PropertyLine(pl)) = m.elements.get(id) else { return };
        let Some(level) = m.elements.id(pl.level) else { return };
        let units = m.units;
        let rows = ground::table_of(m, pl)
            .iter()
            .map(|v| Row {
                distance: units.length(v["distance"].as_f64().unwrap_or(0.0)),
                bearing: v["bearing"].as_str().unwrap_or_default().to_string(),
                arc: v["type"] == "arc",
                radius: v["radius"].as_f64().map(|r| units.length(r)).unwrap_or_default(),
                right: v["side"] == "R",
            })
            .collect();
        let start = pl.points.first().map(|q| format!("{}, {}", units.length(q[0]), units.length(q[1]))).unwrap_or_default();
        self.pl_dialog = Some(PlDialog::Table(Table { id: Some(id), level, start, rows, picked: 0 }));
    }

    /// The setbacks of property line `id`, a row a segment.
    pub(crate) fn open_property_line_setbacks(&mut self, id: Id) {
        let m = &self.session.model;
        let Some(Element::PropertyLine(pl)) = m.elements.get(id) else { return };
        let units = m.units;
        let table = ground::table_of(m, pl);
        let rows = (0..pl.points.len())
            .map(|i| {
                let e = pl.edge(i);
                let seg = &table[i];
                let len = |x: f64| if x > 0.0 { units.length(x) } else { String::new() };
                EdgeRow {
                    segment: format!("{}  {}  {}", i + 1, units.length(seg["distance"].as_f64().unwrap_or(0.0)), seg["bearing"].as_str().unwrap_or("")),
                    setback: len(e.setback),
                    kind: e.kind.clone(),
                    street: e.street.clone(),
                    width: len(e.street_width),
                }
            })
            .collect();
        self.pl_dialog = Some(PlDialog::Setbacks { id, rows });
    }

    /// Property Line's dialog, while one is open.
    pub(crate) fn property_line_window(&mut self, ctx: &Context) {
        match self.pl_dialog.clone() {
            Some(PlDialog::Choose { level }) => self.property_line_choose_window(ctx, level),
            Some(PlDialog::Table(_)) => self.property_line_table_window(ctx),
            Some(PlDialog::Setbacks { .. }) => self.property_line_setbacks_window(ctx),
            None => {}
        }
    }

    fn property_line_choose_window(&mut self, ctx: &Context, level: Id) {
        let (mut table, mut sketch, mut close) = (false, false, false);
        let r = Modal::new(UiId::new("create-property-line"), t("Create Property Line")).width(420.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                table = ui.add(Button::new(t("Create by entering distances and bearings")).min_size(egui::vec2(380.0, 30.0))).clicked();
                ui.add_space(4.0);
                sketch = ui.add(Button::new(t("Create by sketching")).min_size(egui::vec2(380.0, 30.0))).clicked();
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| close = ui.button(t("Cancel")).clicked());
            });
        });
        if table {
            let row = Row::default();
            self.pl_dialog = Some(PlDialog::Table(Table { id: None, level, start: "0, 0".into(), rows: vec![row.clone(), row], picked: 0 }));
        } else if sketch {
            self.pl_dialog = None;
            self.start_sketch(Of::Site { level, tool: SiteTool::PropertyLine }, Vec::new());
        } else if close || r.should_close() {
            self.pl_dialog = None;
            self.set_tool(Tool::Modify);
        }
    }

    fn property_line_table_window(&mut self, ctx: &Context) {
        let units = self.session.model.units;
        let Some(PlDialog::Table(d)) = self.pl_dialog.as_mut() else { return };
        let (mut ok, mut close, mut to_close) = (false, false, false);
        let r = Modal::new(UiId::new("property-line-table"), t("Property Lines")).width(720.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let l = ui.label(t("Start:"));
                    ui.add(TextEdit::singleline(&mut d.start).desired_width(180.0).hint_text("x, y")).labelled_by(l.id);
                });
                ui.add_space(4.0);
                ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    Grid::new("property-line-table-grid").num_columns(6).striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                        for h in ["", "Distance", "Bearing", "Type", "Radius", "L/R"] {
                            ui.label(RichText::new(t(h)).strong());
                        }
                        ui.end_row();
                        for (i, row) in d.rows.iter_mut().enumerate() {
                            if ui.selectable_label(d.picked == i, (i + 1).to_string()).clicked() {
                                d.picked = i;
                            }
                            ui.add(TextEdit::singleline(&mut row.distance).desired_width(110.0).hint_text(units.name()))
                                .on_hover_text(t("Distance"));
                            ui.add(TextEdit::singleline(&mut row.bearing).desired_width(150.0).hint_text("N 45°30'00\" E"))
                                .on_hover_text(t("Bearing"));
                            crate::icons::combo(format!("property-line-type-{i}")).selected_text(t(if row.arc { "Arc" } else { "Line" })).width(70.0).show_ui(
                                ui,
                                |ui| {
                                    ui.selectable_value(&mut row.arc, false, t("Line"));
                                    ui.selectable_value(&mut row.arc, true, t("Arc"));
                                },
                            );
                            ui.add_enabled(row.arc, TextEdit::singleline(&mut row.radius).desired_width(90.0)).on_hover_text(t("Radius"));
                            ui.add_enabled_ui(row.arc, |ui| {
                                crate::icons::combo(format!("property-line-side-{i}")).selected_text(if row.right { "R" } else { "L" }).width(44.0).show_ui(
                                    ui,
                                    |ui| {
                                        ui.selectable_value(&mut row.right, false, "L");
                                        ui.selectable_value(&mut row.right, true, "R");
                                    },
                                );
                            });
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(t("Insert")).clicked() {
                        let at = (d.picked + 1).min(d.rows.len());
                        d.rows.insert(at, Row::default());
                        d.picked = at;
                    }
                    if ui.add_enabled(d.rows.len() > 2, Button::new(t("Delete"))).clicked() && d.picked < d.rows.len() {
                        d.rows.remove(d.picked);
                        d.picked = d.picked.min(d.rows.len().saturating_sub(1));
                    }
                    if ui.add_enabled(d.picked > 0, Button::new(t("Up"))).clicked() {
                        d.rows.swap(d.picked, d.picked - 1);
                        d.picked -= 1;
                    }
                    if ui.add_enabled(d.picked + 1 < d.rows.len(), Button::new(t("Down"))).clicked() {
                        d.rows.swap(d.picked, d.picked + 1);
                        d.picked += 1;
                    }
                    to_close = ui.button(t("Add Line to Close")).clicked();
                });
                ui.label(RichText::new(t("Bearings are against true north. A table that doesn't close is closed by a last line.")).color(pal().muted));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add(Button::new(t("OK"))).clicked();
                });
            });
        });
        if to_close {
            match closing_row(&d.rows, units) {
                Ok(Some(row)) => {
                    d.rows.push(row);
                    d.picked = d.rows.len() - 1;
                }
                Ok(None) => self.status = Status { text: t("The lines close already.").into(), error: false },
                Err(e) => self.status = Status { text: e, error: true },
            }
            return;
        }
        if ok {
            let d = d.clone();
            let made = table_json(&d.rows, units).and_then(|rows| {
                let start = read_point(&d.start, units).ok_or_else(|| t("Start is x, y").to_string())?;
                Ok(match d.id {
                    Some(id) => ("property_line_edit", json!({"id": id, "table": rows, "start": start})),
                    None => ("property_line", json!({"table": rows, "start": start, "level": d.level})),
                })
            });
            match made {
                Ok((command, p)) => {
                    if let Some(v) = self.run(command, p) {
                        self.pl_dialog = None;
                        if let Some(id) = v["id"].as_u64() {
                            self.selection = vec![id];
                        }
                        if d.id.is_none() {
                            self.set_tool(Tool::Modify);
                        }
                    }
                }
                Err(e) => self.status = Status { text: e, error: true },
            }
        } else if close || r.should_close() {
            let new = d.id.is_none();
            self.pl_dialog = None;
            if new {
                self.set_tool(Tool::Modify);
            }
        }
    }

    fn property_line_setbacks_window(&mut self, ctx: &Context) {
        let units = self.session.model.units;
        let Some(PlDialog::Setbacks { id, rows }) = self.pl_dialog.as_mut() else { return };
        let id = *id;
        let (mut ok, mut close) = (false, false);
        let kinds: Vec<&str> = std::iter::once("").chain(EDGE_KINDS).collect();
        let kind_name = |k: &str| -> String {
            match k {
                "" => t("(none)").to_string(),
                "street" => t("Street").to_string(),
                "side" => t("Side").to_string(),
                "rear" => t("Rear").to_string(),
                _ => t("Corner").to_string(),
            }
        };
        let r = Modal::new(UiId::new("property-line-setbacks"), t("Setbacks")).width(760.0).show(ctx, |ui| {
            crate::popup::body(ui, |ui| {
                ui.add_space(6.0);
                ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    Grid::new("property-line-setbacks-grid").num_columns(5).striped(true).spacing([8.0, 4.0]).show(ui, |ui| {
                        for h in ["Segment", "Setback", "Faces", "Street", "Street Width"] {
                            ui.label(RichText::new(t(h)).strong());
                        }
                        ui.end_row();
                        for (i, row) in rows.iter_mut().enumerate() {
                            ui.label(&row.segment);
                            ui.add(TextEdit::singleline(&mut row.setback).desired_width(90.0).hint_text(units.name())).on_hover_text(t("Setback"));
                            crate::icons::combo(format!("property-line-kind-{i}")).selected_text(kind_name(&row.kind)).width(90.0).show_ui(ui, |ui| {
                                for k in &kinds {
                                    if ui.selectable_label(row.kind == *k, kind_name(k)).clicked() {
                                        row.kind = (*k).to_string();
                                    }
                                }
                            });
                            let street = row.kind == "street" || row.kind == "corner";
                            ui.add_enabled(street, TextEdit::singleline(&mut row.street).desired_width(150.0)).on_hover_text(t("Street"));
                            ui.add_enabled(street, TextEdit::singleline(&mut row.width).desired_width(90.0).hint_text(units.name()))
                                .on_hover_text(t("Street Width"));
                            ui.end_row();
                        }
                    });
                });
                ui.label(RichText::new(t("The buildable line keeps each setback from its segment.")).color(pal().muted));
            });
            crate::popup::footer(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    close = ui.button(t("Cancel")).clicked();
                    ok = ui.add(Button::new(t("OK"))).clicked();
                });
            });
        });
        if ok {
            let length = |s: &str, what: &str, i: usize| -> Result<f64, String> {
                if s.trim().is_empty() {
                    return Ok(0.0);
                }
                units.parse(s.trim()).filter(|x| *x >= 0.0).ok_or_else(|| tf("Segment {}: the {} is a length", &[&(i + 1).to_string(), what]))
            };
            let edges: Result<Vec<Value>, String> = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let street = r.kind == "street" || r.kind == "corner";
                    Ok(json!({
                        "setback": length(&r.setback, "setback", i)?,
                        "kind": r.kind,
                        "street": if street { r.street.trim() } else { "" },
                        "street_width": if street { length(&r.width, "street width", i)? } else { 0.0 },
                    }))
                })
                .collect();
            match edges {
                Ok(edges) => {
                    if self.run("property_line_edit", json!({"id": id, "edges": edges})).is_some() {
                        self.pl_dialog = None;
                    }
                }
                Err(e) => self.status = Status { text: e, error: true },
            }
        } else if close || r.should_close() {
            self.pl_dialog = None;
        }
    }
}

//! Array (AR) on the desktop, as Revit's Modify tab has it: the selection repeated along a line (Linear) or about a point
//! (Radial). The Options Bar takes Linear or Radial, the Number of members (the original among them), Move To 2nd or Last,
//! Constrain for a linear array and the angle for a radial one. A linear array is two clicks, the first point and the one
//! that says how far; a radial one turns about a centre (the selection's middle until Place Centre moves it), by two clicks
//! that make the angle, or an angle typed. The core's `array` command makes the copies (`bimcraft_core` commands).

use egui::{Key, TextEdit, Ui};
use serde_json::{Value, json};

use bimcraft_core::geom::P2;

use crate::lang::{t, tf};
use crate::{BimApp, Status, Tool};

/// The most members the preview draws; the array itself may have more.
const PREVIEW_MEMBERS: usize = 60;

/// The Options Bar's choices for Array.
pub(crate) struct Options {
    /// Radial, else Linear.
    pub radial: bool,
    /// Move To Last, else 2nd.
    pub last: bool,
    /// Number: the members in all, the original among them.
    pub count: String,
}

impl Default for Options {
    fn default() -> Self {
        Options { radial: false, last: false, count: "2".into() }
    }
}

impl BimApp {
    /// A fresh Array tool: its centre is set from the selection when it starts (`set_tool`).
    pub(crate) fn new_array() -> Tool {
        Tool::Array { centre: P2(0.0, 0.0), start: None, place: false }
    }

    /// The Options Bar's Number, if it is a whole number of members; else the status says so.
    fn array_count(&mut self) -> Option<usize> {
        match self.array.count.trim().parse::<usize>() {
            Ok(n) if n >= 2 => Some(n),
            _ => {
                self.status = Status { text: tf("array: `{}` is not a number of members, 2 or more", &[&self.array.count]), error: true };
                None
            }
        }
    }

    /// The array's parameters for the selection, common to both kinds.
    fn array_params(&mut self, mode: &str) -> Option<Value> {
        let count = self.array_count()?;
        Some(json!({"ids": self.selection, "mode": mode, "count": count, "move_to": if self.array.last { "last" } else { "second" }}))
    }

    /// A linear array from `from` to `to`.
    pub(crate) fn array_linear(&mut self, from: P2, to: P2) {
        let Some(mut p) = self.array_params("linear") else { return };
        p["from"] = json!([from.0, from.1]);
        p["to"] = json!([to.0, to.1]);
        self.array_done(p);
    }

    /// A radial array about `centre`, `angle` degrees counter-clockwise.
    pub(crate) fn array_radial(&mut self, centre: P2, angle: f64) {
        let Some(mut p) = self.array_params("radial") else { return };
        p["centre"] = json!([centre.0, centre.1]);
        p["angle"] = json!(angle);
        self.array_done(p);
    }

    /// Run `array`: the originals and their copies are selected afterwards, and the tool is done.
    fn array_done(&mut self, p: Value) {
        if let Some(made) = self.run("array", p) {
            let copies = made["ids"].as_array().map(|a| a.iter().filter_map(Value::as_u64)).into_iter().flatten();
            self.selection.extend(copies);
            self.tool = Tool::Modify;
        } else if let Tool::Array { start, .. } = &mut self.tool {
            *start = None;
        }
    }

    /// How far each member after the first moves for a linear array from `from` to `to`, in the same units: the offsets to
    /// draw the copies at.
    pub(crate) fn array_offsets(&self, delta: egui::Vec2) -> Vec<egui::Vec2> {
        let count = self.array.count.trim().parse::<usize>().unwrap_or(2).clamp(2, PREVIEW_MEMBERS);
        let step = if self.array.last { delta / (count - 1) as f32 } else { delta };
        (1..count).map(|i| step * i as f32).collect()
    }

    /// The angles each member after the first turns by for a radial array of `angle` degrees.
    pub(crate) fn array_angles(&self, angle: f64) -> Vec<f64> {
        let count = self.array.count.trim().parse::<usize>().unwrap_or(2).clamp(2, PREVIEW_MEMBERS);
        let step = match (self.array.last, angle.abs() == 360.0) {
            (false, _) => angle,
            (true, true) => angle / count as f64,
            (true, false) => angle / (count - 1) as f64,
        };
        (1..count).map(|i| step * i as f64).collect()
    }

    /// Array's Options Bar.
    pub(crate) fn array_options(&mut self, ui: &mut Ui) {
        if ui.selectable_label(!self.array.radial, t("Linear")).clicked() {
            self.array.radial = false;
        }
        if ui.selectable_label(self.array.radial, t("Radial")).clicked() {
            self.array.radial = true;
            if let Tool::Array { start, .. } = &mut self.tool {
                *start = None;
            }
        }
        let l = ui.label(t("Number:"));
        ui.add(TextEdit::singleline(&mut self.array.count).desired_width(40.0)).labelled_by(l.id);
        ui.label(t("Move To:"));
        if ui.selectable_label(!self.array.last, t("2nd")).clicked() {
            self.array.last = false;
        }
        if ui.selectable_label(self.array.last, t("Last")).clicked() {
            self.array.last = true;
        }
        if !self.array.radial {
            ui.checkbox(&mut self.constrain, t("Constrain"));
            return;
        }
        let l = ui.label(t("Angle:"));
        let r = ui.add(TextEdit::singleline(&mut self.angle).desired_width(60.0).hint_text("°")).labelled_by(l.id);
        if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match (self.angle.trim().parse::<f64>(), &self.tool) {
                (Ok(a), &Tool::Array { centre, .. }) if a.is_finite() => self.array_radial(centre, a),
                _ => self.status = Status { text: tf("array: `{}` is not an angle in degrees", &[&self.angle]), error: true },
            }
        }
        if ui.button(t("Place Centre")).clicked()
            && let Tool::Array { place, .. } = &mut self.tool
        {
            *place = true;
        }
    }
}

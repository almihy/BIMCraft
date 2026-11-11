//! Pin and Unpin on the desktop, as Revit's: Pin (PN) and Unpin (UP) on the Modify panel, a pin on what is pinned and selected
//! (a click on it unpins), and a drag, Move, Rotate or Mirror of a pinned element that does nothing but say it is pinned. The core
//! keeps the pins and the commands from scripts and the agent are not stopped by them (`bimcraft_core`'s `pin`).

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use egui::{Align2, Rect, Sense, Ui, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::json;

use crate::icons::Icon;
use crate::lang::{t, tf};
use crate::style::pal;
use crate::{BimApp, Status};

/// Where an element's pin sits in a plan: its middle, as a tag finds it, a grid's middle, a dimension's line.
fn pin_spot(m: &Model, id: Id) -> Option<P2> {
    match m.elements.get(id)? {
        Element::Grid(g) => Some((g.start + g.end) * 0.5),
        Element::Dimension(d) => Some(d.at),
        _ => m.anchor(id).or_else(|| {
            let lines = m.reference_lines(id);
            let (_, (a, b)) = lines.first().copied()?;
            Some((a + b) * 0.5)
        }),
    }
}

impl BimApp {
    /// What of the selection is pinned.
    pub(crate) fn pinned_selection(&self) -> Vec<Id> {
        self.session.model.pinned_among(&self.selection)
    }

    /// Whether the selection holds a pinned element, which is then not to be moved or turned: the status bar says so.
    pub(crate) fn refuse_pinned(&mut self) -> bool {
        let selection = self.selection.clone();
        self.refuse_pinned_ids(&selection)
    }

    /// Whether `ids` hold a pinned element, which is then not to be changed in place (a typed dimension, an angle, Align, a
    /// nudge, a drag on a sheet): the status bar says so.
    pub(crate) fn refuse_pinned_ids(&mut self, ids: &[Id]) -> bool {
        let pinned = self.session.model.pinned_among(ids);
        if pinned.is_empty() {
            return false;
        }
        let text = match pinned.as_slice() {
            [one] => tf("element {} is pinned and stays where it is: Unpin (UP) frees it", &[&one.to_string()]),
            many => tf("{} of the selected elements are pinned and stay where they are: Unpin (UP) frees them", &[&many.len().to_string()]),
        };
        self.status = Status { text, error: true };
        true
    }

    /// Pin (PN) or Unpin (UP) the selection.
    pub(crate) fn pin_selection(&mut self, pin: bool) {
        if self.selection.is_empty() {
            let text = if pin { t("select what to pin first") } else { t("select what to unpin first") };
            self.status = Status { text: text.into(), error: true };
            return;
        }
        self.run(if pin { "pin" } else { "unpin" }, json!({"ids": self.selection}));
    }

    /// A pin over each selected element that is pinned; a click on it unpins that element.
    pub(crate) fn pin_overlay(&mut self, ui: &mut Ui, m: &Model, v: &bimcraft_core::render::View, rect: Rect) {
        if self.selection.is_empty() || self.canvas.dragging() {
            return;
        }
        let p = ui.painter_at(rect);
        let mut unpin = Vec::new();
        for id in self.pinned_selection() {
            let Some(spot) = pin_spot(m, id) else { continue };
            let (x, y) = v.px(spot);
            let at = pos2(rect.min.x + x, rect.min.y + y) + vec2(14.0, -14.0);
            let r = Rect::from_center_size(at, vec2(20.0, 20.0));
            let response = ui.interact(r, ui.id().with(("pin", id)), Sense::click());
            let name = tf("Unpin element {}", &[&id.to_string()]);
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
            let ink = if response.hovered() { pal().hover } else { pal().accent };
            p.rect_filled(r, 3.0, pal().surface);
            crate::icons::paint(&p, Icon::Pin, r.shrink(2.0), ink);
            if response.hovered() {
                crate::canvas::tag(&p, r.right_bottom() + vec2(2.0, 4.0), Align2::LEFT_TOP, t("Pinned: click to unpin"));
            }
            if response.clicked() {
                unpin.push(id);
            }
        }
        if !unpin.is_empty() {
            self.run("unpin", json!({"ids": unpin}));
        }
    }
}

//! A selected wall's temporary dimensions to its neighbours, as Revit shows them: on each side, to the nearest parallel wall
//! or grid (`bimcraft_core::tempdims`). Typing a distance moves the wall (the `move` command); the dot on the witness line
//! at the wall cycles which line of the wall it measures from (centre line, exterior face, interior face); the icon beside
//! the number makes the dimension permanent (the `dimension` command).

use bimcraft_core::tempdims::{self, Side, TempDim, Witness};

use crate::icons::Icon;

use super::*;

/// Where each wall's choice of witness lines is kept, in egui's memory: it holds for as long as the wall is selected again.
fn memory(wall: Id) -> egui::Id {
    egui::Id::new(("wall witness lines", wall))
}

impl BimApp {
    /// The selected wall's temporary dimensions to the nearest parallel walls and grids, beside the one that gives its length
    /// (`temporary_dimension`): each with its number (a click types another, and the wall moves so that distance is met),
    /// the grip at its witness line, and the icon that makes it permanent. They are measured in the plan `key` (the walls and
    /// grids it shows, cut at its view range) and go into it, whatever the wall's own level is.
    pub(super) fn wall_neighbour_dimensions(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, key: Id) {
        let &[id] = self.selection.as_slice() else { return };
        if self.tool != Tool::Modify || self.canvas.drag.is_some() || self.canvas.grip.is_some() || self.drawing.grip.is_some() {
            return;
        }
        if m.wall(id).is_none() {
            return;
        }
        let witness: Witness = ui.ctx().data(|d| d.get_temp(memory(id))).unwrap_or_default();
        let p = ui.painter_at(rect);
        for d in tempdims::wall(m, id, key, witness) {
            let n = if d.side == Side::Left { 1 } else { 2 };
            let (a, b) = (pos(rect.min, v.px(d.start)), pos(rect.min, v.px(d.end)));
            if a.distance(b) < 24.0 {
                continue;
            }
            // The line runs from the wall to what it measures to, its number beside it.
            let along = (b - a).normalized();
            let middle = dimension_line(&p, a, b, along.rot90(), 0.0);
            let text = m.units.length(d.distance);
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size() + vec2(10.0, 4.0);
            let at = Rect::from_center_size(middle, size);
            self.witness_grip(ui, id, &d, a, witness);
            self.make_permanent(ui, key, &d, at.right_center() + vec2(14.0, 0.0));
            let Some(typed) = self.dimension_number(ui, at, (id, n), &text) else { continue };
            match typed.map(|t| (m.units.parse(&t), t)) {
                // The wall goes toward what it is measured to by what the distance is to shrink.
                Some((Some(len), _)) if len >= 0.0 => {
                    let by = d.toward * (d.distance - len);
                    if by.length() > 1e-6 && !self.refuse_pinned_ids(&[id]) {
                        self.run("move", json!({"ids": [id], "by": point(by)}));
                    }
                }
                Some((_, typed)) => self.status = Status { text: tf("`{}` is not a length", &[typed.trim()]), error: true },
                None => {}
            }
        }
    }

    /// The dot where a temporary dimension starts on the wall: a click moves the start to the next line of the wall, its
    /// centre line, then its exterior face, then its interior face, and round again.
    fn witness_grip(&mut self, ui: &mut Ui, id: Id, d: &TempDim, at: Pos2, witness: Witness) {
        let name = t("Witness line: centre line, exterior face or interior face");
        let r = ui.interact(
            Rect::from_center_size(at, vec2(12.0, 12.0)),
            ui.id().with(("witness grip", id, d.side == Side::Left)),
            Sense::click(),
        );
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
        let ink = if r.hovered() { pal().hover } else { pal().accent };
        ui.painter().circle_filled(at, 4.5, pal().surface);
        ui.painter().circle_stroke(at, 4.5, Stroke::new(1.5, ink));
        if witness.of(d.side) != bimcraft_core::model::Face::Centre {
            ui.painter().circle_filled(at, 2.0, ink);
        }
        if r.clicked() {
            ui.ctx().data_mut(|data| data.insert_temp(memory(id), witness.cycled(d.side)));
        }
    }

    /// The icon beside a temporary dimension's number that lays it as a dimension of the plan `key`, between the same two
    /// lines, where it stands.
    fn make_permanent(&mut self, ui: &mut Ui, key: Id, d: &TempDim, at: Pos2) {
        let name = t("Make this temporary dimension permanent");
        let r = ui.interact(
            Rect::from_center_size(at, vec2(20.0, 20.0)),
            ui.id().with(("make permanent", d.side == Side::Left)),
            Sense::click(),
        );
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
        let ink = if r.hovered() { pal().hover } else { pal().accent };
        ui.painter().rect_filled(r.rect, 3.0, pal().surface);
        crate::icons::paint(ui.painter(), Icon::Dimension, Rect::from_center_size(at, vec2(16.0, 16.0)), ink);
        let r = r.on_hover_text(name);
        if r.clicked() {
            let mut p = json!({"refs": [d.from, d.to], "at": point(d.start), "level": key});
            if let Some(t) = self.dimension_type_for(bimcraft_core::model::DimKind::Aligned) {
                p["type"] = json!(t);
            }
            self.run("dimension", p);
        }
    }
}

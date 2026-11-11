//! Grips on a selected dimension, as Revit's: a grip on its line (drag it to put the line nearer or farther from what it
//! measures, `move`) and one on each number (drag it to stand elsewhere, `dimension_text_move`), the numbers' of aligned and
//! linear dimensions of the continuous or ordinate string. Pinned, it stays.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{DimKind, Dimension, Element, Id, Model};
use bimcraft_core::render::View;
use egui::{Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType, pos2, vec2};
use serde_json::json;

use crate::lang::tf;
use crate::style::pal;
use crate::{BimApp, Tool};

/// Which grip of a dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grip {
    Line,
    Text(usize),
}

impl BimApp {
    /// The selected dimension's grips, drawn and worked: with Modify, one dimension selected alone, aligned or linear.
    pub(crate) fn dimension_grips(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect) {
        let (&[id], Tool::Modify) = (self.selection.as_slice(), &self.tool) else { return };
        let Some(Element::Dimension(d)) = m.elements.get(id) else { return };
        if !matches!(d.kind, DimKind::Aligned | DimKind::Linear) {
            return;
        }
        let Some((n, u)) = m.dimension_axes(d) else { return };
        let screen = |q: P2| {
            let (x, y) = v.px(q);
            pos2(rect.min.x + x, rect.min.y + y)
        };
        let model = |q: Pos2| v.mm((q.x - rect.min.x, q.y - rect.min.y));
        let (_, cross) = m.crossings(d);
        let (Some(&(first, _)), Some(&(last, _))) = (cross.first(), cross.last()) else { return };
        let p = ui.painter_at(rect);
        // The line's grip: past its first end, where nothing else stands.
        let on = |s: f64| d.at + n * s;
        let line_end = screen(on(first)) + (screen(on(first)) - screen(on(last))).normalized() * 14.0;
        let line_end = if line_end.x.is_finite() { line_end } else { screen(on(first)) };
        let mut grips = vec![(Grip::Line, line_end)];
        // The numbers' grips: on the line at each number's place, a little to the side the number stands on.
        let side = (screen(d.at + u) - screen(d.at)).normalized();
        for (i, spot) in m.dimension_text_spots(d) {
            let (along, across) = d.text_shift(i);
            let at = screen(spot + n * along + u * across) + if side.x.is_finite() { side * 12.0 } else { vec2(0.0, -12.0) };
            grips.push((Grip::Text(i), at));
        }
        let held_key = ui.id().with(("dimension grip held", id));
        for (grip, at) in grips {
            let r = Rect::from_center_size(at, vec2(12.0, 12.0));
            let key = ui.id().with(("dimension grip", id, format!("{grip:?}")));
            let response = ui.interact(r, key, Sense::drag());
            let name = match grip {
                Grip::Line => tf("Dimension line of element {}", &[&id.to_string()]),
                Grip::Text(i) => tf("Dimension text {} of element {}", &[&(i + 1).to_string(), &id.to_string()]),
            };
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name.clone()));
            let mut drawn = at;
            if response.drag_started()
                && let Some(o) = ui.input(|i| i.pointer.press_origin())
            {
                ui.memory_mut(|mem| mem.data.insert_temp(held_key, model(o)));
            }
            let from: Option<P2> = ui.memory(|mem| mem.data.get_temp(held_key));
            let by = match (response.dragged() || response.drag_stopped(), from, ui.input(|i| i.pointer.latest_pos())) {
                (true, Some(from), Some(now)) => Some(model(now) - from),
                _ => None,
            };
            if let Some(by) = by {
                // The line goes square to itself; a number goes where it is taken.
                let moved = match grip {
                    Grip::Line => u * by.dot(u),
                    Grip::Text(_) => by,
                };
                drawn = at + (screen(P2(0.0, 0.0) + moved) - screen(P2(0.0, 0.0)));
                if response.dragged() {
                    p.extend(Shape::dashed_line(&[at, drawn], Stroke::new(1.0, pal().accent), 4.0, 3.0));
                }
                if response.drag_stopped() {
                    ui.memory_mut(|mem| mem.data.remove::<P2>(held_key));
                    if moved.length() > 1e-6 && !self.refuse_pinned() {
                        match grip {
                            Grip::Line => {
                                self.run("move", json!({"ids": [id], "by": [moved.0, moved.1]}));
                            }
                            Grip::Text(i) => self.move_text(d, id, i, (moved.dot(n), moved.dot(u))),
                        }
                    }
                }
            }
            let live = response.dragged() || response.hovered();
            let stroke = Stroke::new(1.5, pal().accent);
            match grip {
                Grip::Line => {
                    let r = Rect::from_center_size(drawn, vec2(9.0, 9.0));
                    p.rect_filled(r, 1.0, if live { pal().accent } else { pal().surface });
                    p.rect_stroke(r, 1.0, stroke, StrokeKind::Middle);
                }
                Grip::Text(_) => {
                    p.circle_filled(drawn, 5.0, if live { pal().accent } else { pal().surface });
                    p.circle_stroke(drawn, 5.0, stroke);
                }
            }
        }
    }

    /// Number `segment` of dimension `id` shifted a further `by` (along, across) from where it stands.
    fn move_text(&mut self, d: &Dimension, id: Id, segment: usize, by: (f64, f64)) {
        let (along, across) = d.text_shift(segment);
        self.run("dimension_text_move", json!({"id": id, "segment": segment, "along": along + by.0, "across": across + by.1}));
    }
}

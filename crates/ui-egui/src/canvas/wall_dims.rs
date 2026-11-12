//! A selected wall's temporary dimensions to its neighbours, as Revit shows them: on each side, to the nearest parallel wall
//! or grid (`bimcraft_core::tempdims`), measuring the lines the project's Temporary Dimension Properties say. Typing a distance
//! moves the wall (the `move` command). The dot on a witness line at the wall cycles which line of the wall it measures from
//! (its centre line, its exterior face, its interior face and, for a wall with a core, the core's centre and faces); the dot at
//! the other end does the same for the line it measures to; a drag from either dot, or Move Witness Line in the menu of a right
//! click on it and a click on the line wanted, puts the witness line on another reference. What a person chose is kept for the
//! session, as Revit does. The icon beside the number makes the dimension permanent (the `dimension` command).

use bimcraft_core::model::{Face, Line, Reference};
use bimcraft_core::tempdims::{self, Side, TempDim, Witness};
use bimcraft_i18n::msg;

use crate::icons::Icon;

use super::*;

/// How a witness line's dot is named to a screen reader and the tooltip: near the wall, and at the other end.
const NEAR: &str = msg!("Witness line: click for its next line, drag to another reference");
const FAR: &str = msg!("Witness line at the other end: click for its next line, drag to another reference");

/// The line among `lines` that `at` is nearest to across the lines' direction, and how far it is.
fn nearest_line(lines: &[(Reference, Line)], at: P2) -> Option<(Reference, f64)> {
    lines
        .iter()
        .filter(|(_, (a, b))| a.dist(*b) > 1e-6)
        .map(|(r, (a, b))| (*r, ((at - *a).dot((*b - *a).unit().perp())).abs()))
        .min_by(|x, y| x.1.total_cmp(&y.1))
}

impl BimApp {
    /// The lines the witness line at one end of `d` can be put on: at the wall, its own centre line and faces (and core's); at
    /// the other end, the lines of the parallel walls and grids on that side of it.
    fn witness_lines(&self, m: &Model, key: Id, id: Id, d: &TempDim, far: bool) -> Vec<(Reference, Line)> {
        if !far {
            return tempdims::own_lines(m, id);
        }
        let Ok((from, _)) = m.reference_line(&d.from) else { return Vec::new() };
        tempdims::neighbour_lines(m, id, key).into_iter().filter(|(_, (a, _))| (*a - from).dot(d.toward) > 1e-6).collect()
    }

    /// `side`'s witness line at the wall (or `far`, at the other end) put on `to`: kept for the wall, whatever is selected next.
    fn set_witness(&mut self, id: Id, side: Side, far: bool, to: Reference) {
        let witness = self.witnesses.get(&id).copied().unwrap_or_default();
        let witness = if far { witness.with_target(side, Some(to)) } else { witness.with_line(side, Some(to.face)) };
        self.witnesses.insert(id, witness);
    }

    /// The selected wall's temporary dimensions to the nearest parallel walls and grids, beside the one that gives its length
    /// (`temporary_dimension`): each with its number (a click types another, and the wall moves so that distance is met),
    /// the dots at its witness lines, and the icon that makes it permanent. They are measured in the plan `key` (the walls and
    /// grids it shows, cut at its view range) and go into it, whatever the wall's own level is.
    pub(super) fn wall_neighbour_dimensions(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, key: Id) {
        let &[id] = self.selection.as_slice() else { return };
        if !matches!(self.tool, Tool::Modify | Tool::MoveWitness { .. })
            || self.canvas.drag.is_some()
            || self.canvas.grip.is_some()
            || self.drawing.grip.is_some()
        {
            return;
        }
        if m.wall(id).is_none() {
            return;
        }
        let witness = self.witnesses.get(&id).copied().unwrap_or_default();
        let core = tempdims::has_core(m, id);
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
            self.witness_grip(ui, (m, v, rect, key), id, &d, (a, false), core);
            self.witness_grip(ui, (m, v, rect, key), id, &d, (b, true), core);
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
        // Move Witness Line waits for a click: the line the cursor would pick, drawn bold.
        if let Tool::MoveWitness { side, far, .. } = self.tool
            && let Some(q) = ui.input(|i| i.pointer.hover_pos()).filter(|q| rect.contains(*q))
            && let Some(d) = tempdims::wall(m, id, key, witness).into_iter().find(|d| d.side == side)
        {
            let lines = self.witness_lines(m, key, id, &d, far);
            let at = v.mm((q.x - rect.min.x, q.y - rect.min.y));
            if let Some((r, _)) = nearest_line(&lines, at)
                && let Some((_, (la, lb))) = lines.iter().find(|(l, _)| *l == r)
            {
                p.line_segment([pos(rect.min, v.px(*la)), pos(rect.min, v.px(*lb))], Stroke::new(2.5, pal().hover));
            }
        }
    }

    /// The dot where a temporary dimension's witness line is, at `at` (screen): at the wall, or `far`, at the other end. A click
    /// moves the line to the next one (the wall's centre line, exterior face, interior face and, with a core, the core's
    /// centre and faces; at the other end the lines of the wall it measures to), a drag puts it on another reference and a
    /// right click offers Move Witness Line.
    fn witness_grip(
        &mut self,
        ui: &mut Ui,
        (m, v, rect, key): (&Model, &View, Rect, Id),
        id: Id,
        d: &TempDim,
        (at, far): (Pos2, bool),
        core: bool,
    ) {
        let name = t(if far { FAR } else { NEAR });
        let r = ui.interact(
            Rect::from_center_size(at, vec2(12.0, 12.0)),
            ui.id().with(("witness grip", id, d.side == Side::Left, far)),
            Sense::click_and_drag(),
        );
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
        let ink = if r.hovered() || r.dragged() { pal().hover } else { pal().accent };
        ui.painter().circle_filled(at, 4.5, pal().surface);
        ui.painter().circle_stroke(at, 4.5, Stroke::new(1.5, ink));
        let line = if far { d.to.face } else { d.from.face };
        if line != Face::Centre {
            ui.painter().circle_filled(at, 2.0, ink);
        }
        let lines = || self.witness_lines(m, key, id, d, far);
        let near_at = |q: Pos2| v.mm((q.x - rect.min.x, q.y - rect.min.y));
        // Dragged: the line the dot would land on, bold; let go, it is there.
        if r.dragged()
            && let Some(q) = r.interact_pointer_pos()
        {
            let lines = lines();
            if let Some((found, _)) = nearest_line(&lines, near_at(q))
                && let Some((_, (la, lb))) = lines.iter().find(|(l, _)| *l == found)
            {
                ui.painter().line_segment([pos(rect.min, v.px(*la)), pos(rect.min, v.px(*lb))], Stroke::new(2.5, pal().hover));
            }
        }
        if r.drag_stopped()
            && let Some(q) = r.interact_pointer_pos()
            && let Some((found, _)) = nearest_line(&lines(), near_at(q))
        {
            self.set_witness(id, d.side, far, found);
        } else if r.clicked() {
            let next = if far {
                // The lines of the wall it ends at, one after another, from the one it is on.
                let mine: Vec<Reference> =
                    tempdims::neighbour_lines(m, id, key).into_iter().map(|(r, _)| r).filter(|r| r.id == d.to.id).collect();
                mine.iter().position(|r| *r == d.to).map(|i| mine[(i + 1) % mine.len()])
            } else {
                let after = Witness::default().cycled(d.side, d.from.face, core).of(d.side);
                after.map(|f| Reference::line(d.from.id, f))
            };
            if let Some(next) = next {
                self.set_witness(id, d.side, far, next);
            }
        }
        r.context_menu(|ui| {
            if ui.button(t("Move Witness Line")).clicked() {
                self.tool = Tool::MoveWitness { wall: id, side: d.side, far };
                self.status = Status::default();
                ui.close();
            }
        });
    }

    /// A click with Move Witness Line: the line nearest the click, of those the witness line can go to, is where it goes, and the
    /// tool ends. A click that is near none says so and the tool goes on.
    pub(crate) fn pick_witness(&mut self, m: &Model, key: Id, at: P2) {
        let Tool::MoveWitness { wall, side, far } = self.tool.clone() else { return };
        let witness = self.witnesses.get(&wall).copied().unwrap_or_default();
        let found = tempdims::wall(m, wall, key, witness)
            .into_iter()
            .find(|d| d.side == side)
            .and_then(|d| nearest_line(&self.witness_lines(m, key, wall, &d, far), at));
        match found {
            Some((to, _)) => {
                self.set_witness(wall, side, far, to);
                self.tool = Tool::Modify;
            }
            None => self.status = Status { text: t("there is no line there to move the witness line to").into(), error: true },
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

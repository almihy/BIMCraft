//! How a hand picks and repeats, as Revit's does: Move, Copy, Rotate and Mirror started with nothing selected ask for the
//! elements and go on at Enter; Enter with no tool running repeats the last one (RR is the same); Tab over a wall selects the
//! walls joined to it and Shift+Tab goes back; Ctrl+A selects what the view shows; the arrow keys nudge the selection; the
//! status bar names what is under the cursor; and the canvas shows the tool in the cursor.

use bimcraft_i18n::msg;

use bimcraft_core::element_json;
use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use egui::{Context, Modifiers, Response, Ui};
use serde_json::json;
use std::sync::Arc;

use crate::canvas::point;
use crate::lang::{category, t};
use crate::{BimApp, Tool};

/// What the status bar says while Move, Copy, Rotate or Mirror waits for its elements.
const SELECT_FIRST: &str = msg!("Select elements, then press Enter");

/// How far an arrow key nudges, in px on screen: the step is this much of the view at its zoom, so a nudge looks the same
/// zoomed in or out.
const NUDGE: f64 = 3.0;

/// What a Shift on the arrow keys multiplies the nudge by.
const BIGGER: f64 = 10.0;

/// The state of selecting and repeating, kept with the app.
#[derive(Default)]
pub(crate) struct Flow {
    /// The tool Enter is waiting to start: it was asked for with nothing selected.
    pub(crate) pending: Option<Tool>,
    /// The tool last started, which Enter and RR start again.
    pub(crate) last: Option<Tool>,
    /// Shift was held for the Tab that came this frame.
    pub(crate) back: bool,
    /// Tab has selected the walls joined to the one it is on; the selection from before, to give back at the next Tab.
    pub(crate) chain: Option<Vec<Id>>,
    /// The walls Tab selected, in id order: it is theirs while the selection is still exactly them.
    pub(crate) joined: Vec<Id>,
    /// What the status bar names under the cursor: the element, the model it was read from (an address), and the words.
    named: Option<(Id, usize, String)>,
}

/// The name Repeat shows for a tool, in English: the menu translates it.
pub(crate) fn tool_name(tool: &Tool) -> &'static str {
    match tool {
        Tool::Wall(_) => msg!("Wall"),
        Tool::Door => msg!("Door"),
        Tool::Window => msg!("Window"),
        Tool::Room => msg!("Room"),
        Tool::RoomSeparator(_) => msg!("Room Separator"),
        Tool::Level => msg!("Level"),
        Tool::Grid(_) => msg!("Grid"),
        Tool::Move(_) => msg!("Move"),
        Tool::Copy(_) => msg!("Copy"),
        Tool::Rotate { .. } => msg!("Rotate"),
        Tool::Mirror { .. } => msg!("Mirror"),
        Tool::Align(_) => msg!("Align"),
        Tool::Trim(_) => msg!("Trim/Extend to Corner"),
        Tool::Split => msg!("Split Element"),
        Tool::Offset => msg!("Offset"),
        Tool::Dimension(..) => msg!("Aligned Dimension"),
        Tool::Text => msg!("Text"),
        Tool::Tag => msg!("Tag by Category"),
        Tool::Section(_) => msg!("Section"),
        Tool::Elevation => msg!("Elevation"),
        Tool::Column => msg!("Column"),
        Tool::Component => msg!("Component"),
        Tool::Stair(_) => msg!("Stair"),
        Tool::Ramp(_) => msg!("Ramp"),
        Tool::Railing(_) => msg!("Railing"),
        Tool::Roof | Tool::ExtrusionRoof(_) => msg!("Roof"),
        Tool::Floor => msg!("Floor"),
        Tool::Ceiling => msg!("Ceiling"),
        Tool::DetailLine(_) => msg!("Detail Line"),
        Tool::Measure(_) => msg!("Measure"),
        _ => msg!("Last Command"),
    }
}

/// The nudge for an arrow key at `scale` px per mm: a few pixels of the view as a round number of mm (1, 2 or 5 of a power of
/// ten), never less than a millimetre.
pub(crate) fn nudge_step(scale: f64) -> f64 {
    let raw = NUDGE / scale;
    if !raw.is_finite() || raw <= 1.0 {
        return 1.0;
    }
    let ten = 10_f64.powf(raw.log10().floor());
    [5.0, 2.0, 1.0].into_iter().map(|m| m * ten).find(|step| *step <= raw).unwrap_or(ten)
}

impl BimApp {
    /// Start `tool` now, or, for one that works on a selection with none made, ask for the elements and start it at Enter.
    pub(crate) fn ask_selection_for(&mut self, tool: Tool) {
        self.tool = Tool::Modify;
        self.flow.pending = Some(tool);
        self.flow.chain = None;
        self.typed.clear();
        self.clear_typed();
        self.forget_picks();
        self.status = crate::Status::default();
    }

    /// What the status bar says while a tool waits for its elements.
    pub(crate) fn selection_prompt(&self) -> Option<&'static str> {
        self.flow.pending.as_ref().map(|_| SELECT_FIRST)
    }

    /// Enter, with a tool waiting for its elements: start it if some are selected. Whether it took the key.
    pub(crate) fn enter_for_pending(&mut self) -> bool {
        let Some(tool) = self.flow.pending.take() else { return false };
        if self.selection.is_empty() {
            self.flow.pending = Some(tool);
        } else {
            self.set_tool(tool);
        }
        true
    }

    /// Whether no tool is running, so that Enter repeats the last one.
    fn idle(&self) -> bool {
        self.tool == Tool::Modify
            && self.flow.pending.is_none()
            && self.sketch.is_none()
            && self.typed_length.is_empty()
            && self.drawing.other.is_empty()
            && !self.finishable()
            && !self.picking_pattern_lines()
    }

    /// Enter with no tool running: the last tool again. Whether it took the key.
    pub(crate) fn enter_to_repeat(&mut self) -> bool {
        self.idle() && self.repeat_last()
    }

    /// Repeat the last tool started (Enter, RR, and the right-click menu's Repeat). Whether there was one.
    pub(crate) fn repeat_last(&mut self) -> bool {
        match self.flow.last.clone() {
            Some(tool) => {
                self.set_tool(tool);
                true
            }
            None => false,
        }
    }

    /// Ctrl+A: everything the open view shows, selected.
    pub(crate) fn select_all(&mut self) {
        let found: Vec<Id> = self.shown().into_iter().collect();
        if !found.is_empty() {
            self.pick_up(found, Modifiers::NONE);
        }
    }

    /// Keep the arrow keys for the canvas while it has the keyboard, so they nudge the selection and don't walk the keyboard
    /// from one widget to the next.
    pub(crate) fn keep_arrows(ui: &Ui, response: &Response) {
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, tab: false, escape: false },
                );
            });
        }
    }

    /// The arrow keys, with Modify and a selection: move it a step the way the key points, ten steps with Shift.
    pub(crate) fn nudge(&mut self, ctx: &Context) {
        if self.tool != Tool::Modify || self.sketch.is_some() || self.selection.is_empty() || self.plan_level().is_none() {
            return;
        }
        let Some(scale) = self.plan_key().and_then(|k| self.canvas.view(k)).map(|v| v.scale) else { return };
        let step = nudge_step(scale);
        let mut by = P2(0.0, 0.0);
        for (key, dx, dy) in [
            (egui::Key::ArrowLeft, -1.0, 0.0),
            (egui::Key::ArrowRight, 1.0, 0.0),
            (egui::Key::ArrowUp, 0.0, 1.0),
            (egui::Key::ArrowDown, 0.0, -1.0),
        ] {
            // Shift first: a key pressed with it would match the plain key too.
            let (big, small) = ctx.input_mut(|i| {
                (i.count_and_consume_key(Modifiers::SHIFT, key).min(20), i.count_and_consume_key(Modifiers::NONE, key).min(20))
            });
            let n = small as f64 + BIGGER * big as f64;
            by = P2(by.0 + dx * n * step, by.1 + dy * n * step);
        }
        if by.0 != 0.0 || by.1 != 0.0 {
            self.run("move", json!({"ids": self.selection, "by": point(by)}));
        }
    }

    /// Forget the joined walls Tab selected: the cursor has gone on to something else.
    pub(crate) fn tab_forget(&mut self) {
        self.flow.chain = None;
        self.flow.joined.clear();
    }

    /// Whether the selection is still the joined walls Tab picked.
    pub(crate) fn chain_selected(&self) -> bool {
        let mut now = self.selection.clone();
        now.sort_unstable();
        self.flow.chain.is_some() && !self.flow.joined.is_empty() && now == self.flow.joined
    }

    /// What a click on the element under the cursor, `hovered`, selects: it, or the walls joined to it while Tab has them.
    pub(crate) fn click_set(&self, hovered: Option<Id>) -> Vec<Id> {
        match hovered {
            Some(id) if self.chain_selected() && self.flow.joined.contains(&id) => self.selection.clone(),
            Some(id) => vec![id],
            None => Vec::new(),
        }
    }

    /// What the status bar says of the element under the cursor, as Revit's: "Walls : Basic Wall : Generic - 200mm".
    pub(crate) fn hover_text(&mut self) -> Option<String> {
        let id = self.canvas.hovered?;
        let model = Arc::clone(&self.session.model);
        let key = Arc::as_ptr(&model) as usize;
        if let Some((known, at, words)) = &self.flow.named
            && *known == id
            && *at == key
        {
            return Some(words.clone());
        }
        let words = name_of(&model, id)?;
        self.flow.named = Some((id, key, words.clone()));
        Some(words)
    }
}

/// The words for element `id`: its category, family and type, as Revit's status bar names them.
fn name_of(m: &Model, id: Id) -> Option<String> {
    let e = m.elements.get(id)?;
    let json = element_json(m, id);
    let mut parts = vec![category(e.category(), true)];
    let family = json["family"].as_str().filter(|f| !f.is_empty()).map(str::to_string).or_else(|| system_family(e));
    parts.extend(family);
    parts.extend(json["type"].as_str().filter(|t| !t.is_empty()).map(str::to_string));
    Some(parts.join(" : "))
}

/// The family of an element that has types but no family of its own: Revit's system families.
fn system_family(e: &Element) -> Option<String> {
    let name = match e {
        Element::Wall(_) => msg!("Basic Wall"),
        Element::Floor(_) => msg!("Floor"),
        Element::Roof(_) => msg!("Basic Roof"),
        Element::Ceiling(_) => msg!("Compound Ceiling"),
        _ => return None,
    };
    Some(t(name).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nudge_is_a_few_pixels_as_a_round_number_of_millimetres() {
        // 0.1 px/mm: 3 px is 30 mm, so 20.
        assert_eq!(nudge_step(0.1), 20.0);
        // Zoomed in so far a pixel is under a millimetre: 1 mm.
        assert_eq!(nudge_step(10.0), 1.0);
        assert_eq!(nudge_step(f64::NAN), 1.0);
        // Zoomed out: 3 px is 3000 mm, so 2000.
        assert_eq!(nudge_step(0.001), 2000.0);
        assert_eq!(nudge_step(0.0006), 5000.0);
    }
}

//! What a hand does over the plan that the canvas keeps to itself: Tab stepping through what is under the cursor, and the
//! cursor's shape. The rest of selecting and repeating is `selecting.rs`.

use bimcraft_core::model::{Id, Model};
use bimcraft_core::render::View;
use bimcraft_core::wallchain::wall_chain;
use egui::{CursorIcon, Ui, vec2};

use super::Drag;
use super::navigation::Mode;
use crate::{BimApp, Tool};

impl BimApp {
    /// Tab over the plan: step through what is under the cursor, a wall and then the walls joined to it, before the next
    /// element; Shift+Tab goes the other way. The joined walls are selected while they are the step, and the selection from
    /// before comes back when Tab goes on.
    pub(crate) fn tab_step(&mut self, model: &Model, back: bool) {
        let n = self.canvas.under.len();
        if n == 0 {
            return;
        }
        // Joined walls Tab picked and the selection has since left are no longer its.
        let on = self.chain_selected();
        if !on {
            self.tab_forget();
        }
        let chain = |i: usize| -> Vec<Id> {
            let found = self.canvas.under.get(i).map(|id| wall_chain(model, *id)).unwrap_or_default();
            if found.len() > 1 { found } else { Vec::new() }
        };
        let cycle = self.canvas.cycle;
        let (cycle, on) = match (back, on) {
            (false, false) if !chain(cycle).is_empty() => (cycle, true),
            (false, _) => ((cycle + 1) % n, false),
            (true, true) => (cycle, false),
            (true, false) => {
                let before = (cycle + n - 1) % n;
                (before, !chain(before).is_empty())
            }
        };
        let joined = if on { chain(cycle) } else { Vec::new() };
        self.canvas.cycle = cycle;
        if on {
            let prior = self.flow.chain.take().unwrap_or_else(|| self.selection.clone());
            self.flow.chain = Some(prior);
            self.selection = joined.clone();
            self.flow.joined = joined;
            self.raise_context();
        } else if let Some(prior) = self.flow.chain.take() {
            self.selection = prior;
            self.flow.joined.clear();
        }
    }

    /// The cursor over the plan, in `view`: a crosshair for a tool that draws, a pointing hand over what a picking tool would
    /// take, the move cross over a grip or over what a drag would move. `local` is the cursor's pixel in the view and
    /// `hovered` what lies under it.
    pub(crate) fn plan_cursor(&self, ui: &Ui, view: &View, local: Option<(f32, f32)>, hovered: Option<Id>) {
        let Some(q) = local else { return };
        if self.canvas.nav.mode != Mode::Off || self.passive {
            return;
        }
        let moving = matches!(self.canvas.drag, Some(Drag::Move(_))) || self.canvas.grip.is_some() || self.on_grip(view, q);
        let icon = if moving {
            CursorIcon::Move
        } else if self.tool == Tool::Text {
            CursorIcon::Text
        } else if self.tool != Tool::Modify {
            if hovered.is_some() { CursorIcon::PointingHand } else { CursorIcon::Crosshair }
        } else if self.flow.pending.is_none() && hovered.is_some_and(|id| self.selection.contains(&id)) {
            CursorIcon::Move
        } else {
            return;
        };
        self.canvas.nav.cursor(ui, icon);
    }

    /// Whether pixel `q` is on an end grip of the wall or run selected alone.
    fn on_grip(&self, view: &View, q: (f32, f32)) -> bool {
        let Some(id) = self.gripped() else { return false };
        let Some((a, b)) = Self::grip_ends(&self.session.model, id) else { return false };
        [a, b].into_iter().any(|p| {
            let (x, y) = view.px(p);
            (vec2(x, y) - vec2(q.0, q.1)).length() <= 8.0
        })
    }
}

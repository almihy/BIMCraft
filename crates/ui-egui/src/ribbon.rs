//! The ribbon's panels: each panel's tools over its name, and the name a button that lists them all. A tab too wide for the window
//! makes room from the right: a panel's large buttons become small icons, two high, and when even those don't fit it hands its
//! last columns of tools over to its list; what still doesn't fit scrolls sideways.

use std::cell::{Cell, RefCell};

use egui::{Align2, FontId, Id, Popup, Rect, Response, Sense, Stroke, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::lang::t;
use crate::presets::Scale;
use crate::style::pal;

/// How long the tools of a folded ribbon's tab stay once the pointer has left the tab and them, seconds: long enough to cross from the
/// tab down into them.
pub(crate) const GRACE: f64 = 0.35;

/// The hover that shows a folded ribbon's tools under a tab.
#[derive(Default)]
pub(crate) struct Hover {
    /// The tab whose tools are shown because the pointer is on it or on them.
    pub tab: Option<crate::Tab>,
    /// When they go, by the input clock, unless the pointer is on the tab or them again.
    pub until: f64,
    /// Put away by a click or Esc with the pointer still on the tab: not shown again until it has left the tabs.
    pub blocked: bool,
    /// Whether the pointer is on a tab this frame.
    pub on_tab: bool,
}

/// How wide a column of small tools is: a button and the gap to the next.
const COLUMN: f32 = 32.0;
/// How tall a panel's tools are on the ribbon: a large button's height.
const HEIGHT: f32 = 60.0;
/// How big a small tool's button is.
pub(crate) const SMALL_TOOL: Vec2 = vec2(30.0, 25.0);
/// How tall two rows of small tools are.
const SMALL_HEIGHT: f32 = 52.0;
/// The rows a panel's small tools stand in.
const ROWS: usize = 2;
/// The hairline between panels, with its room.
const RULE: f32 = 9.0;
/// What's kept clear at the tab's end, so a fit judged on last frame's widths isn't a pixel over.
const SLACK: f32 = 8.0;

/// How a panel fits: its tools as large buttons or, `compact`, as small icons two high, and how many of its columns show (the
/// rest are in its list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fit {
    pub compact: bool,
    pub shown: usize,
}

impl Fit {
    /// Every tool, as large buttons.
    pub(crate) const WHOLE: Fit = Fit { compact: false, shown: usize::MAX };

    fn all(self) -> bool {
        self.shown == usize::MAX
    }
}

/// A panel as the last frame drew it: its name, how wide it is with every tool shown as large buttons and as it was drawn (and
/// as small icons, once it has been), how many tools it has and, a panel of rows, how many columns, and how wide its name is.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Seen {
    name: String,
    natural: f32,
    drawn: f32,
    /// How wide it is as small icons with every tool shown; none until it has been drawn so.
    small: f32,
    tools: usize,
    /// The columns of a panel whose tools are drawn in rows (`row`); none for the others.
    cols: usize,
    /// The least a panel is: its name.
    label: f32,
}

impl Seen {
    /// How many units a panel's tools come in, as large buttons or as small ones two high: the columns of one in rows.
    fn units(&self, compact: bool) -> usize {
        match (self.cols, compact) {
            (0, true) => self.tools.div_ceil(ROWS),
            (0, false) => self.tools,
            (cols, _) => cols,
        }
    }

    /// How wide it is as small icons with every tool shown: as it was when it was, else as its tools' columns say. A panel of rows
    /// is small already.
    fn compact_width(&self) -> f32 {
        if self.small > 0.0 {
            self.small
        } else if self.cols > 0 {
            self.natural
        } else {
            (self.tools.div_ceil(ROWS) as f32 * COLUMN + 4.0).max(self.label).min(self.natural)
        }
    }

    /// How narrow it can be made: small, its first column alone.
    fn least(&self) -> f32 {
        if self.tools < 2 {
            return self.natural;
        }
        self.width(1, self.cols == 0).min(self.natural)
    }

    /// How wide it is with `shown` of its units, large or small.
    fn width(&self, shown: usize, compact: bool) -> f32 {
        let units = self.units(compact);
        let full = if compact { self.compact_width() } else { self.natural };
        let each = if compact || self.cols > 0 { COLUMN } else { self.natural / units.max(1) as f32 };
        (full - units.saturating_sub(shown) as f32 * each).max(self.label)
    }
}

thread_local! {
    /// Whether the tools drawn now are a panel's list: rows of an icon and its name, not the ribbon's buttons.
    static LISTING: Cell<bool> = const { Cell::new(false) };
    /// What the ribbon leaves out this frame, and whether Edit Tools is on (`presets.rs`).
    static TOOL_RULES: RefCell<(std::collections::BTreeSet<String>, Scale, bool)> = RefCell::new(Default::default());
    /// Whether a tab's tools are being drawn on the ribbon: the rules apply there alone, not to the view
    /// control bar's buttons, which are drawn the same way.
    static IN_TOOLS: Cell<bool> = const { Cell::new(false) };
    /// The tools whose box was ticked or cleared in Edit Tools this frame, by name.
    static TOGGLED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// How the panels of the tab being drawn fit, by name; one not named shows all its tools.
    static FITS: RefCell<Vec<(String, Fit)>> = const { RefCell::new(Vec::new()) };
    /// The panels of the tab being drawn, as they're drawn.
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
    /// The tools of the panel being drawn so far, and how many columns of them it shows.
    static TOOLS: Cell<(usize, usize)> = const { Cell::new((0, usize::MAX)) };
    /// Whether the panel being drawn shows its tools as small icons, two high.
    static SMALL: Cell<bool> = const { Cell::new(false) };
    /// The column the next tool of a row is in (`row`), and the most columns a row of the panel being drawn has had.
    static ROW: Cell<Option<usize>> = const { Cell::new(None) };
    static COLS: Cell<usize> = const { Cell::new(0) };
    /// How many tools of the panel being drawn were in rows.
    static ROWED: Cell<usize> = const { Cell::new(0) };
    /// Where the small tools of the panel being drawn start, and how many are drawn so far (`small_tools`).
    static SLOTS: Cell<Option<(egui::Pos2, usize)>> = const { Cell::new(None) };
    /// A panel's list: how wide its rows were last frame, and how wide the widest row is so far.
    static LIST: Cell<(f32, f32)> = const { Cell::new((0.0, 0.0)) };
    /// Where the last panel drawn ends.
    static END: Cell<f32> = const { Cell::new(0.0) };
    /// The tab being drawn: the room it has, where it starts, and how much of what it has drawn is gaps (`push_right`'s).
    static LAID: Cell<(f32, f32, f32)> = const { Cell::new((0.0, 0.0, 0.0)) };
    /// A tool's choices were asked for (a shortcut's) while it has no button, being handed over: its panel opens its list.
    static OPEN_LIST: Cell<bool> = const { Cell::new(false) };
    /// Whether the tab being drawn has been drawn before, so its panels' fit (which tools show) is known: its first frame lays out
    /// every tool, whatever the room.
    static FITTED: Cell<bool> = const { Cell::new(true) };
}

/// Whether the tools drawn now go in a panel's list.
pub(crate) fn listing() -> bool {
    LISTING.with(Cell::get)
}

/// What the tools drawn this frame leave out (the user's hidden tools, the project's scale), and whether Edit Tools is on.
pub(crate) fn tool_rules(hidden: std::collections::BTreeSet<String>, scale: Scale, editing: bool) {
    TOOL_RULES.with(|r| *r.borrow_mut() = (hidden, scale, editing));
}

/// Whether Edit Tools is on, for a tool of a tab being drawn: every one is a box to tick, what it shows.
pub(crate) fn editing_tools() -> bool {
    IN_TOOLS.with(Cell::get) && TOOL_RULES.with(|r| r.borrow().2)
}

/// Whether the user took the tool named `name` (its English name) out.
pub(crate) fn tool_hidden(name: &str) -> bool {
    TOOL_RULES.with(|r| r.borrow().0.contains(name))
}

/// Whether the project's scale leaves out the panel or the tool named `name`.
pub(crate) fn scale_hides(name: &str) -> bool {
    TOOL_RULES.with(|r| r.borrow().1.hides(name))
}

/// Whether the tool named `name` is left out of what is drawn: the scale's, or the user's but in Edit Tools, where it shows unticked.
pub(crate) fn left_out(name: &str) -> bool {
    IN_TOOLS.with(Cell::get) && (scale_hides(name) || (tool_hidden(name) && !editing_tools()))
}

/// Ticks or clears the tool named `name` in Edit Tools.
pub(crate) fn toggle(name: &str) {
    TOGGLED.with(|t| t.borrow_mut().push(name.to_string()));
}

/// The tools ticked or cleared since the last call.
pub(crate) fn take_toggled() -> Vec<String> {
    TOGGLED.with(|t| std::mem::take(&mut *t.borrow_mut()))
}

/// Whether the panel being drawn shows its tools as small icons, two high.
pub(crate) fn small() -> bool {
    SMALL.with(Cell::get)
}

/// Counts a ribbon tool, and says whether its panel has handed it over to its list: then it isn't drawn on the ribbon. A
/// handed-over tool takes no room and has no name, so it's found only in the list. A panel hands its tools over by columns: the
/// ones in a row (`row`) by where each is in its row, the others two to a column when small.
pub(crate) fn handed_over(ui: &mut Ui) -> Option<Response> {
    if listing() {
        return None;
    }
    let (n, shown) = TOOLS.with(Cell::get);
    TOOLS.with(|t| t.set((n + 1, shown)));
    let column = match ROW.with(Cell::get) {
        Some(c) => {
            ROW.with(|r| r.set(Some(c + 1)));
            COLS.with(|k| k.set(k.get().max(c + 1)));
            ROWED.with(|k| k.set(k.get() + 1));
            c
        }
        None if small() => n / ROWS,
        None => n,
    };
    (column >= shown).then(|| ui.allocate_response(Vec2::ZERO, Sense::hover()))
}

/// A row of small tools in a panel, as Revit's Modify panel has two: in the panel's list its tools are rows of the list like any
/// other, one under the next. Handed over by column: the tools last in every row go together.
pub(crate) fn row(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    if listing() {
        ui.vertical(add);
        return;
    }
    ui.horizontal(|ui| {
        ROW.with(|r| r.set(Some(0)));
        add(ui);
        ROW.with(|r| r.set(None));
    });
}

/// How wide a row of the panel's list is: as wide as its widest, which the rows drawn so far and the last frame's list say.
pub(crate) fn list_width(own: f32) -> f32 {
    LIST.with(|l| {
        let (known, widest) = l.get();
        l.set((known, widest.max(own)));
        own.max(known)
    })
}

/// Draw a tab's panels, `add` drawing them, in `room`: those it can't hold make room from the right, as the last frame's
/// widths say they must.
pub(crate) fn tab(ui: &mut Ui, key: Id, room: f32, add: impl FnOnce(&mut Ui)) {
    // The last frame's panels, how wide the whole tab would have been with every tool shown, and what the tools' average widths
    // missed at the room it had then: asked of the panels again, so a tab they misjudged still ends up fitting.
    let (seen, whole, missed, then): (Vec<Seen>, f32, f32, f32) = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
    let missed = if (then - room).abs() < 0.5 { missed } else { 0.0 };
    let fitted = !seen.is_empty();
    FITTED.with(|f| f.set(fitted));
    FITS.with(|f| *f.borrow_mut() = fits(&seen, whole + missed, room - SLACK));
    SEEN.with(|s| s.borrow_mut().clear());
    let left = ui.cursor().left();
    END.with(|e| e.set(left));
    LAID.with(|l| l.set((room - SLACK, left, 0.0)));
    IN_TOOLS.with(|i| i.set(true));
    add(ui);
    IN_TOOLS.with(|i| i.set(false));
    let drawn = END.with(Cell::get) - left - LAID.with(Cell::get).2;
    let seen = SEEN.with(|s| std::mem::take(&mut *s.borrow_mut()));
    let whole = drawn + seen.iter().map(|p| (p.natural - p.drawn).max(0.0)).sum::<f32>();
    // Still over though fitted (a first frame lays out every tool): another frame, asking more of the panels, until it fits or all
    // there is to ask is asked.
    let over = if fitted { (drawn - (room - SLACK)).max(0.0) } else { 0.0 };
    // What was missed is given back when it leaves room to spare: a measure that was off once is not asked of the panels for good.
    let spare = if fitted { (room - SLACK) - drawn } else { 0.0 };
    // Asking more of the panels than they have to give is for nothing: a tab that cannot fit scrolls.
    let give: f32 = seen.iter().map(|p| (p.natural - p.least()).max(0.0)).sum();
    let (missed, again) = if over > 0.0 {
        let missed = (missed + over).min(whole);
        (missed, whole + missed - (room - SLACK) < give)
    } else if missed > 0.0 && spare > 2.0 * COLUMN {
        ((missed - (spare - COLUMN)).max(0.0), true)
    } else {
        (missed, false)
    };
    if again {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(key, (seen, whole, missed, room)));
    FITS.with(|f| f.borrow_mut().clear());
    FITTED.with(|f| f.set(true));
}

/// Pushes what is drawn next to the right end of the tab being drawn, a panel `width` wide (one that stands apart, as Edit Mode's
/// does). The gap is not width the tab needs, so it never makes the panels before it hand their tools over.
pub(crate) fn push_right(ui: &mut Ui, width: f32) {
    let (room, left, gaps) = LAID.with(Cell::get);
    let gap = (room - (END.with(Cell::get) - left - gaps) - width).max(0.0);
    ui.add_space(gap);
    LAID.with(|l| l.set((room, left, gaps + gap)));
}

/// How the panels `seen` fit, for a tab `whole` wide with every tool shown, in `room`: from the last panel back, each draws its
/// tools as small icons two high, as few as make it fit; if that is not room enough for the whole tab, from the last back again each
/// hands its last columns over to its list, as few as make it fit, and never its first.
pub(crate) fn fits(seen: &[Seen], whole: f32, room: f32) -> Vec<(String, Fit)> {
    let mut over = whole - room;
    let mut fit = vec![Fit::WHOLE; seen.len()];
    for (i, p) in seen.iter().enumerate().rev() {
        if over <= 0.0 {
            break;
        }
        // A panel of one tool is not made small: a lone icon beside a name saves next to nothing.
        let small = p.compact_width();
        if p.tools > 1 && small + 1.0 < p.natural {
            over -= p.natural - small;
            fit[i].compact = true;
        }
    }
    for (i, p) in seen.iter().enumerate().rev() {
        if over <= 0.0 {
            break;
        }
        let compact = fit[i].compact;
        let units = p.units(compact);
        if units < 2 {
            continue;
        }
        let now = p.width(units, compact);
        let saves = |k: usize| now - p.width(k, compact);
        let shown = (1..units).rev().find(|&k| saves(k) >= over).unwrap_or(1);
        over -= saves(shown);
        fit[i].shown = shown;
    }
    seen.iter().zip(fit).filter(|(_, f)| *f != Fit::WHOLE).map(|(p, f)| (p.name.clone(), f)).collect()
}

/// A ribbon panel: its tools over its name, which lists them. Its tools are large buttons, or small icons two high, as `fits` says,
/// its last columns in its list rather than on the ribbon when they didn't fit.
pub(crate) fn panel(ui: &mut Ui, title: &str, mut add: impl FnMut(&mut Ui)) {
    // A panel the project's scale leaves out: not drawn.
    if scale_hides(title) {
        return;
    }
    if listing() {
        // Inside another panel's list: the tools as rows.
        add(ui);
        return;
    }
    let name = t(title);
    let id = ui.id().with(("ribbon-panel", title));
    let fit = FITS.with(|f| f.borrow().iter().find(|(n, _)| n == title).map(|(_, fit)| *fit)).unwrap_or(Fit::WHOLE);
    TOOLS.with(|t| t.set((0, fit.shown)));
    SMALL.with(|s| s.set(fit.compact));
    ROW.with(|r| r.set(None));
    COLS.with(|c| c.set(0));
    ROWED.with(|c| c.set(0));
    let font = FontId::proportional(11.5);
    let text = ui.painter().layout_no_wrap(name.to_string(), font.clone(), pal().muted).size().x;
    let drawn = ui
        .vertical(|ui| {
            let tools = if fit.compact {
                small_tools(ui, &mut add)
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    // The panels' names on one line, whatever their tools' height (the Modify panel's are two rows).
                    ui.set_min_height(HEIGHT);
                    add(ui);
                })
                .response
                .rect
            };
            let (r, title_row) = ui.allocate_exact_size(vec2(tools.width().max(text + 24.0), 16.0), Sense::click());
            if title_row.hovered() {
                ui.painter().rect_filled(Rect::from_center_size(r.center(), vec2(text + 22.0, 16.0)), 4.0, pal().hover_fill);
            }
            ui.painter().text(r.center() - vec2(5.0, 0.0), Align2::CENTER_CENTER, name, font, pal().muted);
            caret(ui, pos2(r.center().x + text / 2.0 + 3.0, r.center().y), pal().muted);
            let label = format!("{name} ▾");
            title_row.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
            open_if_asked(ui, &title_row);
            list(&title_row.on_hover_text(name), &mut add);
        })
        .response
        .rect
        .width();
    let tools = TOOLS.with(Cell::get).0;
    // A panel is one of rows when all its tools are in them; one that has others too (large ones beside a few rows) is counted tool by tool.
    let cols = if ROWED.with(Cell::take) == tools { COLS.with(Cell::take) } else { 0 };
    COLS.with(|c| c.set(0));
    TOOLS.with(|t| t.set((0, usize::MAX)));
    SMALL.with(|s| s.set(false));
    ROW.with(|r| r.set(None));
    // How wide it is as large buttons, and as small icons, with every tool shown: as drawn when it was, else as it was then.
    let (natural_id, small_id) = (id, id.with("small"));
    let natural = match (fit.compact, fit.all()) {
        (false, true) => {
            ui.ctx().data_mut(|d| d.insert_temp(natural_id, drawn));
            drawn
        }
        _ => ui.ctx().data(|d| d.get_temp::<f32>(natural_id)).unwrap_or(drawn),
    };
    let small = match (fit.compact, fit.all()) {
        (true, true) => {
            ui.ctx().data_mut(|d| d.insert_temp(small_id, drawn));
            drawn
        }
        _ => ui.ctx().data(|d| d.get_temp::<f32>(small_id)).unwrap_or(0.0),
    };
    SEEN.with(|s| s.borrow_mut().push(Seen { name: title.to_string(), natural, drawn, small, tools, cols, label: text + 24.0 }));
    // A hairline between panels, short of the card's edges.
    let (rule, _) = ui.allocate_exact_size(vec2(RULE, ui.available_height().max(70.0)), Sense::hover());
    ui.painter().vline(rule.center().x, rule.y_range().shrink(4.0), Stroke::new(1.0, pal().border));
    END.with(|e| e.set(rule.right()));
}

/// A panel's tools as small icons in two rows, filled a column at a time, as tall as large buttons would stand: where they are.
/// Each tool takes its place from `slot` rather than from the layout, so the columns are as many as the tools drawn make.
fn small_tools(ui: &mut Ui, add: &mut impl FnMut(&mut Ui)) -> Rect {
    let origin = ui.cursor().min + vec2(0.0, (HEIGHT - SMALL_HEIGHT) / 2.0);
    SLOTS.with(|s| s.set(Some((origin, 0))));
    // A child that takes no room: what isn't a tool of the grid (a handed-over tool's nothing) stays out of the way.
    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(origin, Vec2::ZERO)));
    add(&mut child);
    let drawn = SLOTS.with(|s| s.take()).map_or(0, |(_, n)| n);
    let columns = drawn.div_ceil(ROWS);
    let width = if columns == 0 { 0.0 } else { columns as f32 * COLUMN - 2.0 };
    ui.allocate_exact_size(vec2(width, HEIGHT), Sense::hover()).0
}

/// Where the next small tool of the panel being drawn goes, when its tools are in two rows (`small_tools`): none for a panel that is
/// not.
pub(crate) fn slot() -> Option<Rect> {
    SLOTS.with(|s| {
        s.get().map(|(origin, n)| {
            s.set(Some((origin, n + 1)));
            let at = origin + vec2((n / ROWS) as f32 * COLUMN, (n % ROWS) as f32 * (SMALL_TOOL.y + 2.0));
            Rect::from_min_size(at, SMALL_TOOL)
        })
    })
}

/// Opens the list under `r`, the name of the panel just drawn, when a tool of it asked for its choices and had no button to show them.
fn open_if_asked(ui: &Ui, r: &Response) {
    if OPEN_LIST.with(Cell::take) {
        Popup::open_id(ui.ctx(), Popup::default_response_id(r));
    }
}

/// The choices a tool drops down from its button `r` (a ribbon button or, in a panel's list, its row), `open` asking for them at
/// once (a shortcut's): true when that is done, or handed to the panel (a tab's first frame lays out every tool, so what shows
/// is not known until the next). egui keeps one popup open: a popup of the row's own would close the list that holds the row, and
/// with it itself, so in a list the choices are a submenu of it. A tool the panel has handed over has no button on the
/// ribbon to hang them from: a request to open them opens the panel's list, where its row is.
pub(crate) fn choices(ui: &Ui, r: &Response, open: bool, content: impl FnOnce(&mut Ui)) -> bool {
    if listing() {
        egui::containers::menu::SubMenu::new().show(ui, r, content);
        return false;
    }
    let drawn = r.rect.is_positive() && ui.is_rect_visible(r.rect);
    let asked = open && FITTED.with(Cell::get);
    if drawn {
        if asked {
            Popup::open_id(ui.ctx(), Popup::default_response_id(r));
        }
        Popup::menu(r).show(content);
    } else if asked {
        OPEN_LIST.with(|o| o.set(true));
    }
    asked
}

/// The list of a panel's tools, under `r` while it's open: `add` draws them as rows, every tool of the panel in its order, as wide as
/// the widest.
fn list(r: &Response, add: &mut impl FnMut(&mut Ui)) {
    let key = Popup::default_response_id(r).with("list-width");
    Popup::menu(r).show(|ui| {
        let known = ui.ctx().data(|d| d.get_temp::<f32>(key)).unwrap_or(0.0);
        LIST.with(|l| l.set((known, 0.0)));
        ui.spacing_mut().item_spacing.y = 0.0;
        LISTING.with(|l| l.set(true));
        ui.vertical(|ui| add(ui));
        LISTING.with(|l| l.set(false));
        let widest = LIST.with(|l| l.get().1);
        if (widest - known).abs() > 0.5 {
            ui.ctx().data_mut(|d| d.insert_temp(key, widest));
            ui.ctx().request_repaint();
        }
    });
}

/// A small chevron pointing down, at `c`: the drop-downs' arrow, a size smaller.
fn caret(ui: &Ui, c: egui::Pos2, ink: egui::Color32) {
    crate::icons::paint_chevron_at(ui.painter(), c, 12.0, ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panels of large buttons: how wide each is and how many tools.
    fn panels(list: &[(f32, usize)]) -> Vec<Seen> {
        list.iter()
            .enumerate()
            .map(|(i, &(w, n))| Seen { name: format!("p{i}"), natural: w, drawn: w, tools: n, label: 60.0, ..Default::default() })
            .collect()
    }

    fn fit(list: &[(f32, usize)], room: f32) -> Vec<(String, Fit)> {
        let whole = list.iter().map(|(w, _)| w + RULE).sum();
        fits(&panels(list), whole, room)
    }

    const SMALL: Fit = Fit { compact: true, shown: usize::MAX };

    #[test]
    fn a_tab_that_fits_is_left_alone_and_one_too_wide_draws_small_icons_from_the_right() {
        assert!(fit(&[(200.0, 4), (300.0, 6)], 600.0).is_empty());
        // 518 wide in 480: the last panel's six tools as three columns of small icons is room enough.
        assert_eq!(fit(&[(200.0, 4), (300.0, 6)], 480.0), [("p1".into(), SMALL)]);
        // In 300 the first goes small too: two columns.
        assert_eq!(fit(&[(200.0, 4), (300.0, 6)], 300.0), [("p0".into(), SMALL), ("p1".into(), SMALL)]);
    }

    #[test]
    fn small_icons_that_are_not_room_enough_hand_their_last_columns_over_from_the_right() {
        // 368 over, and all small makes 332 of it: the last panel keeps one column, the 36 left over being as much as it can give.
        assert_eq!(fit(&[(200.0, 4), (300.0, 6)], 150.0), [("p0".into(), SMALL), ("p1".into(), Fit { compact: true, shown: 1 })]);
        // Only as many columns as make it fit: 32 over after both are small, so one column of the three goes.
        assert_eq!(fit(&[(200.0, 4), (300.0, 6)], 154.0), [("p0".into(), SMALL), ("p1".into(), Fit { compact: true, shown: 2 })]);
    }

    #[test]
    fn a_panel_of_two_rows_hands_over_by_columns_and_only_what_does_not_fit() {
        // Modify's: 11 columns of two small tools, 352 wide, 21 tools in all. 40 over is two columns, not twenty tools' worth.
        let modify = Seen { name: "Modify".into(), natural: 352.0, drawn: 352.0, tools: 21, cols: 11, label: 60.0, ..Default::default() };
        let got = fits(std::slice::from_ref(&modify), 352.0 + RULE, 352.0 + RULE - 40.0);
        assert_eq!(got, [("Modify".into(), Fit { compact: false, shown: 9 })]);
        // Room for all of it: none handed over.
        assert!(fits(&[modify], 352.0 + RULE, 352.0 + RULE).is_empty());
    }
}

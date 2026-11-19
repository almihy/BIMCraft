//! A curtain wall worked on by hand in the window, as Revit's tools: Curtain Grid (a line previewed across the wall, dashed,
//! with its temporary dimensions and its snaps, put down by a click), Add/Remove Segments (a click on a segment takes it out
//! or puts it back), Mullion (on the grid line, the segment or every grid line), and a hand-made grid line selected (dragged
//! along its wall, or moved by a temporary dimension typed). One view's worth of it is [`BimApp::curtain_over`]: a plan, a
//! section, an elevation and the 3D view each say where a model point is on the screen and what place on a curtain wall is
//! under the cursor ([`Over`]), and the rest is the same everywhere. The geometry is the core's (`render::curtain_grid`), and
//! what a click does is a command: `curtain_grid`, `add_remove_segments`, `mullion`, `modify`.

use bimcraft_core::curtain::{self, Grid, Segment};
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::render::curtain_grid::{self as grid, Aim, Place, Snap};
use bimcraft_core::view3d::P3;
use bimcraft_core::walls::Axis;
use bimcraft_i18n::msg;
use egui::{Align, Align2, Color32, FontId, Key, Painter, Pos2, Rect, Shape, Stroke, TextEdit, Ui, WidgetInfo, WidgetType, vec2};
use serde_json::json;

use crate::canvas::tag;
use crate::lang::{t, tf};
use crate::style::{canvas_pal, pal};
use crate::{BimApp, Status, Tool};

/// How near a snap the cursor takes it, and how near a grid line it is on it, px.
const SNAP: f32 = 8.0;
const ON_LINE: f32 = 10.0;
/// The dashes of a grid line shown over its wall, px.
const DASH: (f32, f32) = (6.0, 4.0);

/// Where the Mullion tool puts mullions, as Revit's Placement panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum On {
    #[default]
    GridLine,
    Segment,
    All,
}

impl On {
    pub(crate) const ALL: [On; 3] = [On::GridLine, On::Segment, On::All];

    /// Its button, in English.
    pub(crate) fn label(self) -> &'static str {
        match self {
            On::GridLine => msg!("Grid Line"),
            On::Segment => msg!("Grid Line Segment"),
            On::All => msg!("All Grid Lines"),
        }
    }

    pub(crate) fn tip(self) -> &'static str {
        match self {
            On::GridLine => msg!("Grid Line: a mullion along the whole grid line clicked"),
            On::Segment => msg!("Grid Line Segment: a mullion on the one segment clicked (or hold Shift)"),
            On::All => msg!("All Grid Lines: mullions on every grid line of the wall clicked (or hold Ctrl)"),
        }
    }

    /// The `mullion` command's `on`.
    fn param(self) -> &'static str {
        match self {
            On::GridLine => "grid_line",
            On::Segment => "segment",
            On::All => "all",
        }
    }
}

/// What Select Panels and Select Mullions pick of a curtain wall, as Revit's right-click menu names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Among {
    PanelsVertical,
    PanelsHorizontal,
    PanelsHost,
    MullionsGridline,
    MullionsInside,
    MullionsBorder,
    MullionsHost,
}

impl Among {
    /// Its item in its submenu, in English.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Among::PanelsVertical => msg!("Along Vertical Grid"),
            Among::PanelsHorizontal => msg!("Along Horizontal Grid"),
            Among::PanelsHost | Among::MullionsHost => msg!("On Host"),
            Among::MullionsGridline => msg!("On Gridline"),
            Among::MullionsInside => msg!("On Inside"),
            Among::MullionsBorder => msg!("On Border"),
        }
    }

    /// Whether it picks mullions (Select Mullions) and not panels (Select Panels).
    pub(crate) fn mullions(self) -> bool {
        matches!(self, Among::MullionsGridline | Among::MullionsInside | Among::MullionsBorder | Among::MullionsHost)
    }
}

/// What the curtain tools keep between frames.
#[derive(Default)]
pub(crate) struct State {
    /// The Mullion tool's Placement.
    pub(crate) on: On,
    /// The selected grid line being dragged, and where it would be let go.
    drag: Option<(Id, Option<f64>)>,
    /// The temporary dimension being typed: its grid line, whether it measures to the line after it, and what is typed.
    edit: Option<(Id, bool, String)>,
    /// The line the Curtain Grid tool previews at the cursor.
    pub(crate) aimed: Option<Aim>,
}

/// A view the curtain tools work in: its rectangle and its response to the pointer, where a model point is on the screen, the cursor and the click with
/// the place on a curtain wall under each, whether it is a plan (which shows a wall from above: its standing lines only),
/// and whether the view draws nothing for a wall hovered or selected (the 3D view: its outline is drawn here).
pub(crate) struct Over<'a> {
    pub rect: Rect,
    /// The view's own response: its drag is a selected grid line's when it begins on the line.
    pub response: &'a egui::Response,
    pub screen: &'a dyn Fn(P3) -> Pos2,
    pub hover: Option<(Pos2, Option<Place>)>,
    pub click: Option<(Pos2, Option<Place>)>,
    pub plan: bool,
    pub outline: bool,
}

impl Over<'_> {
    /// A line of the grid as this view shows it: across the wall, or over `span` of it. In a plan a standing line is a
    /// tick across the wall, and a lying one is not shown.
    fn path(&self, m: &Model, wall: Id, vertical: bool, at: f64, span: Option<(f64, f64)>) -> Vec<Pos2> {
        if self.plan {
            let Some(w) = m.wall(wall).filter(|_| vertical) else { return Vec::new() };
            let (axis, half) = (Axis::of(w), m.thickness(w) / 2.0 + 150.0);
            return [-half, half].into_iter().map(|off| axis.at(at, off)).map(|q| (self.screen)([q.0, q.1, m.own_base(w)])).collect();
        }
        grid::line(m, wall, vertical, at, span).into_iter().map(|q| (self.screen)(q)).collect()
    }

    /// How many px a mm along the wall is at `place`, and a mm up it.
    fn scales(&self, m: &Model, place: Place) -> (f32, f32) {
        let at = |s: f64, z: f64| grid::point(m, place.wall, s, z).map(|q| (self.screen)(q));
        match (at(place.s, place.z), at(place.s + 100.0, place.z), at(place.s, place.z + 100.0)) {
            (Some(o), Some(a), Some(b)) => (o.distance(a) / 100.0, o.distance(b) / 100.0),
            _ => (0.0, 0.0),
        }
    }

    /// The line the Curtain Grid tool would put at `place`: square to the nearer edge on the screen, a standing one in a plan.
    fn aim(&self, m: &Model, place: Place) -> Option<Aim> {
        let g = curtain::grid(m, place.wall)?;
        let (along, up) = self.scales(m, place);
        let vertical = self.plan || nearest(&g.v, place.z) * f64::from(up) <= nearest(&g.u, place.s) * f64::from(along);
        let scale = if vertical { along } else { up };
        let reach = if scale > 1e-6 { f64::from(SNAP / scale) } else { 0.0 };
        grid::aim(m, place.wall, &g, place.s, place.z, Some(vertical), reach)
    }

    /// The segment of the grid line the cursor is on at `place`, with the wall's grid: none further than a few px from any.
    fn segment(&self, m: &Model, place: Place) -> Option<(Grid, Segment)> {
        let g = curtain::grid(m, place.wall)?;
        let (along, up) = self.scales(m, place);
        let (du, dv) = (nearest(&g.u, place.s) * f64::from(along), nearest(&g.v, place.z) * f64::from(up));
        let vertical = self.plan || du <= dv;
        let seg = g.segment_near(place.s, place.z, Some(vertical))?;
        (if vertical { du } else { dv } <= f64::from(ON_LINE)).then_some((g, seg))
    }
}

/// How far `x` is from the nearest of `lines`, mm.
fn nearest(lines: &[curtain::Line], x: f64) -> f64 {
    lines.iter().map(|l| (l.at - x).abs()).fold(f64::INFINITY, f64::min)
}

/// How far `at` is from the line through `pts`, px.
fn off(pts: &[Pos2], at: Pos2) -> f32 {
    let one = |a: Pos2, b: Pos2| {
        let d = b - a;
        let len = d.length_sq();
        let k = if len > 0.0 { ((at - a).dot(d) / len).clamp(0.0, 1.0) } else { 0.0 };
        (a + d * k).distance(at)
    };
    match pts {
        [only] => only.distance(at),
        _ => pts
            .windows(2)
            .map(|w| match w {
                [a, b] => one(*a, *b),
                _ => f32::INFINITY,
            })
            .fold(f32::INFINITY, f32::min),
    }
}

fn dashed(p: &Painter, pts: &[Pos2], stroke: Stroke) {
    if pts.len() > 1 {
        p.extend(Shape::dashed_line(pts, stroke, DASH.0, DASH.1));
    }
}

fn solid(p: &Painter, pts: &[Pos2], stroke: Stroke) {
    if pts.len() > 1 {
        p.add(Shape::line(pts.to_vec(), stroke));
    }
}

/// What the Curtain Grid tool says of a snap, beside the cursor.
fn snap_label(m: &Model, snap: Snap) -> String {
    match snap {
        Snap::Midpoint => t(msg!("Midpoint")).to_string(),
        Snap::Third => t(msg!("1/3 of curtain panel")).to_string(),
        Snap::Level(id) => m.level(id).map(|l| l.name.clone()).unwrap_or_default(),
    }
}

/// Where a selected panel, mullion or grid line shows its pin in a view that is no plan: its middle.
fn middle(m: &Model, id: Id) -> Option<P3> {
    match m.elements.get(id)? {
        Element::CurtainPanel(x) => {
            let (c, ..) = curtain::panel_face(m, x)?;
            Some([0, 1, 2].map(|k| c.iter().map(|q| q[k]).sum::<f64>() / 4.0))
        }
        Element::Mullion(x) => {
            let (a, b, ..) = curtain::mullion_line(m, x)?;
            Some([0, 1, 2].map(|k| (a[k] + b[k]) / 2.0))
        }
        Element::CurtainGrid(l) => {
            let wall = m.elements.id(l.host)?;
            let g = curtain::grid(m, wall)?;
            if l.vertical { grid::point(m, wall, l.at, g.height / 2.0) } else { grid::point(m, wall, g.length / 2.0, l.at) }
        }
        _ => None,
    }
}

/// Tab over a section, an elevation or the 3D view, as over a plan (`BimApp::tab_step`): `under` is what a click at the
/// cursor may mean, in order; Tab steps on through it and Shift+Tab back, and a cursor over something else starts again.
/// What is lit and what a click takes is what it gives; `over` says the cursor is over the view. The app's own fields, so a view holding its own state can call it.
pub(crate) fn tab_through(
    canvas: &mut crate::canvas::Canvas,
    tab_cycle: &mut bool,
    flow: &mut crate::selecting::Flow,
    ui: &Ui,
    over: bool,
    under: Vec<Id>,
) -> Option<Id> {
    if under != canvas.under {
        (canvas.under, canvas.cycle) = (under, 0);
        flow.preview.clear();
    }
    // Tab is the view's while the cursor is over it; elsewhere it moves between a palette's fields, as ever.
    let tab = over && ui.input(|i| i.key_pressed(Key::Tab));
    if tab {
        ui.ctx().memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
    }
    let n = canvas.under.len();
    if (std::mem::take(tab_cycle) || tab) && n > 0 {
        let back = std::mem::take(&mut flow.back) || (tab && ui.input(|i| i.modifiers.shift));
        canvas.cycle = (canvas.cycle.min(n - 1) + if back { n - 1 } else { 1 }) % n;
    }
    canvas.under.get(canvas.cycle).copied()
}

/// What a click may mean where a view finds `found` under the cursor and the hand-made grid line `line` runs there, in
/// Tab's order. With `wall_first` (the 3D view, as a plan), a curtain wall's panel or mullion is its wall first, then the
/// grid line, then itself; without, what the view found comes first and the grid line after it.
pub(crate) fn chain(m: &Model, found: &[Id], line: Option<Id>, wall_first: bool) -> Vec<Id> {
    let mut out: Vec<Id> = Vec::new();
    let mut push = |id: Id| {
        if !out.contains(&id) {
            out.push(id);
        }
    };
    for id in found {
        let part = matches!(m.elements.get(*id), Some(Element::CurtainPanel(_) | Element::Mullion(_) | Element::CurtainGrid(_)));
        match grid::wall_of(m, *id).filter(|_| wall_first && part) {
            Some(wall) => {
                push(wall);
                line.into_iter().for_each(&mut push);
                push(*id);
            }
            None => push(*id),
        }
    }
    line.into_iter().for_each(&mut push);
    out
}

impl BimApp {
    /// The curtain wall the selection is of, for Add/Remove Segments: a curtain wall selected, or the one wall the
    /// selected panels, mullions and grid lines are all of.
    pub(crate) fn curtain_selected(&self) -> Option<Id> {
        let m = &self.session.model;
        let mut walls: Vec<Id> = self.selection.iter().map(|id| grid::wall_of(m, *id)).collect::<Option<_>>()?;
        walls.dedup();
        match walls.as_slice() {
            [wall] => Some(*wall),
            _ => None,
        }
    }

    /// Select Panels and Select Mullions, in the right-click menu of a curtain wall, a panel or a mullion selected: the
    /// panels' ways for a wall or a panel (along a grid only from a panel), the mullions' for a wall or a mullion (on a
    /// grid line only from a mullion).
    pub(crate) fn curtain_select_items(&self) -> Vec<crate::context::Item> {
        use crate::context::{Action, Item};
        let m = &self.session.model;
        let has = |f: &dyn Fn(&Element) -> bool| self.selection.iter().filter_map(|id| m.elements.get(*id)).any(f);
        let wall = self.selection.iter().any(|id| grid::wall_of(m, *id) == Some(*id));
        let panel = has(&|e| matches!(e, Element::CurtainPanel(_)));
        let mullion = has(&|e| matches!(e, Element::Mullion(_)));
        let mut items = Vec::new();
        if wall || panel {
            items.extend([
                Item::new(Action::SelectCurtain(Among::PanelsVertical), panel),
                Item::new(Action::SelectCurtain(Among::PanelsHorizontal), panel),
                Item::new(Action::SelectCurtain(Among::PanelsHost), true),
            ]);
        }
        if wall || mullion {
            items.extend([
                Item::new(Action::SelectCurtain(Among::MullionsGridline), mullion),
                Item::new(Action::SelectCurtain(Among::MullionsInside), true),
                Item::new(Action::SelectCurtain(Among::MullionsBorder), true),
                Item::new(Action::SelectCurtain(Among::MullionsHost), true),
            ]);
        }
        items
    }

    /// Select Panels and Select Mullions: of the selected elements' curtain walls, the panels or mullions `among` names.
    pub(crate) fn select_among(&mut self, among: Among) {
        let m = &self.session.model;
        let mut walls: Vec<Id> = self.selection.iter().filter_map(|id| grid::wall_of(m, *id)).collect();
        walls.sort_unstable();
        walls.dedup();
        let picked: Vec<&Element> = self.selection.iter().filter_map(|id| m.elements.get(*id)).collect();
        // Two stretches share more than their ends.
        let overlap = |a: (f64, f64), b: (f64, f64)| a.0 < b.1 - curtain::NEAR && b.0 < a.1 - curtain::NEAR;
        let wanted = |e: &Element| {
            match (among, e) {
            (Among::PanelsHost, Element::CurtainPanel(_)) | (Among::MullionsHost, Element::Mullion(_)) => true,
            (Among::PanelsVertical | Among::PanelsHorizontal, Element::CurtainPanel(x)) => {
                let k = if among == Among::PanelsVertical { 0 } else { 2 };
                picked.iter().any(|q| {
                    matches!(q, Element::CurtainPanel(q) if q.host == x.host && overlap((q.span[k], q.span[k + 1]), (x.span[k], x.span[k + 1])))
                })
            }
            (Among::MullionsInside, Element::Mullion(x)) => x.border == 0,
            (Among::MullionsBorder, Element::Mullion(x)) => x.border != 0,
            (Among::MullionsGridline, Element::Mullion(x)) => picked.iter().any(|q| {
                matches!(q, Element::Mullion(q) if q.host == x.host && q.vertical == x.vertical && (q.at - x.at).abs() <= curtain::NEAR)
            }),
            _ => false,
        }
        };
        let mut found: Vec<Id> =
            walls.iter().flat_map(|wall| m.hosted_by(m.guid(*wall))).filter(|id| m.elements.get(*id).is_some_and(&wanted)).collect();
        found.sort_unstable();
        if !found.is_empty() {
            self.selection = found;
        }
    }

    /// The Placement panel of Modify | Place Curtain Grid and Modify | Place Mullion, and the Curtain Grid panel of a
    /// selected curtain wall, panel, mullion or grid line, with Add/Remove Segments.
    pub(crate) fn curtain_panels(&mut self, ui: &mut Ui) {
        use crate::icons::Icon;
        match self.tool {
            Tool::CurtainGrid => crate::ribbon::panel(ui, msg!("Placement"), |ui| {
                let tip = msg!("All Segments: a grid line across every panel under it, from border to border");
                let _ = crate::big_button(ui, Icon::CurtainGrid, msg!("All Segments"), true, tip);
            }),
            Tool::Mullion => crate::ribbon::panel(ui, msg!("Placement"), |ui| {
                for on in On::ALL {
                    if crate::big_button(ui, Icon::Mullion, on.label(), self.curtain_tools.on == on, on.tip()).clicked() {
                        self.curtain_tools.on = on;
                    }
                }
            }),
            Tool::Modify | Tool::GridSegments(_) => {
                let wall = match self.tool {
                    Tool::GridSegments(wall) => Some(wall),
                    _ => self.curtain_selected(),
                };
                let Some(wall) = wall else { return };
                crate::ribbon::panel(ui, msg!("Curtain Grid"), |ui| {
                    let tip = msg!(
                        "Add/Remove Segments: click a segment of a grid line to remove it, joining the panels either side, or to put it back"
                    );
                    let active = matches!(self.tool, Tool::GridSegments(_));
                    if crate::big_button(ui, Icon::CurtainGrid, msg!("Add/Remove Segments"), active, tip).clicked() {
                        self.start_tool(if active { Tool::Modify } else { Tool::GridSegments(wall) });
                    }
                });
            }
            _ => {}
        }
    }

    /// The curtain tools in one view, over its picture: see the module's notes.
    pub(crate) fn curtain_over(&mut self, ui: &mut Ui, p: &Painter, o: &Over) {
        let m = self.view_model();
        self.curtain_tools.aimed = None;
        match self.tool {
            Tool::CurtainGrid => self.grid_tool(&m, p, o),
            Tool::Mullion => self.mullion_tool(ui, &m, p, o),
            Tool::GridSegments(wall) => self.segments_tool(&m, p, o, wall),
            Tool::Modify if self.sketch.is_none() => self.curtain_selection(ui, &m, p, o),
            _ => {}
        }
    }

    /// Every grid line of `wall`, thin and dashed in `ink`; its outline too where the view draws none.
    fn show_grid(&self, m: &Model, p: &Painter, o: &Over, wall: Id, ink: Color32) {
        if o.outline {
            let mut ring: Vec<Pos2> = grid::outline(m, wall).into_iter().map(|q| (o.screen)(q)).collect();
            ring.extend(ring.first().copied());
            dashed(p, &ring, Stroke::new(2.0, ink));
        }
        for k in grid::marks(m, wall).iter().filter(|k| k.border == 0) {
            dashed(p, &o.path(m, wall, k.vertical, k.at, None), Stroke::new(1.0, ink));
        }
    }

    /// A temporary dimension from `a` to `b` on the screen, reading `text`.
    fn curtain_dim(p: &Painter, a: Pos2, b: Pos2, text: String) -> Pos2 {
        let ink = Stroke::new(1.0, pal().accent);
        p.line_segment([a, b], ink);
        let across = (b - a).normalized().rot90() * 4.0;
        for end in [a, b] {
            p.line_segment([end - across, end + across], ink);
        }
        let mid = a + (b - a) / 2.0;
        tag(p, mid, Align2::CENTER_CENTER, text);
        mid
    }

    fn grid_tool(&mut self, m: &Model, p: &Painter, o: &Over) {
        if let Some((cursor, Some(place))) = o.hover {
            self.show_grid(m, p, o, place.wall, canvas_pal().hover);
            if let Some(aim) = o.aim(m, place) {
                dashed(p, &o.path(m, place.wall, aim.vertical, aim.at, None), Stroke::new(1.5, canvas_pal().accent));
                // To the line before it and to the line after it, level with the cursor.
                let level = if aim.vertical { place.z } else { place.s };
                let at = |x: f64| {
                    let q = if aim.vertical { grid::point(m, place.wall, x, level) } else { grid::point(m, place.wall, level, x) };
                    q.map(|q| (o.screen)(q))
                };
                for (from, to) in [(aim.before, aim.at), (aim.at, aim.after)] {
                    if let (Some(a), Some(b)) = (at(from), at(to)) {
                        Self::curtain_dim(p, a, b, m.units.length(to - from));
                    }
                }
                if let Some(snap) = aim.snap {
                    tag(p, cursor + vec2(14.0, 18.0), Align2::LEFT_TOP, snap_label(m, snap));
                }
                self.curtain_tools.aimed = Some(aim);
            }
        }
        if let Some((_, place)) = o.click {
            match place.and_then(|place| Some((place.wall, o.aim(m, place)?))) {
                Some((wall, aim)) => {
                    self.run("curtain_grid", json!({"wall": wall, "vertical": aim.vertical, "at": aim.at}));
                }
                None => self.status = Status { text: t("Click a curtain wall, on or beside one of its edges.").into(), error: true },
            }
        }
    }

    fn mullion_tool(&mut self, ui: &Ui, m: &Model, p: &Painter, o: &Over) {
        // Shift is the one segment and Ctrl every grid line, whatever the Placement panel says.
        let placement = self.curtain_tools.on;
        let held = |mods: egui::Modifiers| {
            if mods.command {
                On::All
            } else if mods.shift {
                On::Segment
            } else {
                placement
            }
        };
        let on = held(ui.input(|i| i.modifiers));
        let lit = Stroke::new(3.0, canvas_pal().hover);
        if let Some((_, Some(place))) = o.hover {
            self.show_grid(m, p, o, place.wall, canvas_pal().hover);
            match (on, o.segment(m, place)) {
                (On::All, _) => {
                    for k in grid::marks(m, place.wall) {
                        solid(p, &o.path(m, place.wall, k.vertical, k.at, None), lit);
                    }
                }
                (On::GridLine, Some((_, seg))) => solid(p, &o.path(m, place.wall, seg.vertical, seg.at, None), lit),
                (On::Segment, Some((_, seg))) => {
                    solid(p, &o.path(m, place.wall, seg.vertical, seg.at, Some((seg.span[0], seg.span[1]))), lit)
                }
                _ => {}
            }
        }
        let Some((_, place)) = o.click else { return };
        // The keys held as the button came up.
        let on = held(crate::canvas::click_modifiers(ui));
        let params = match (on, place, place.and_then(|place| o.segment(m, place))) {
            (On::All, Some(place), _) => Some(json!({"wall": place.wall, "on": on.param()})),
            (_, Some(place), Some((_, seg))) => {
                Some(json!({"wall": place.wall, "on": on.param(), "at": seg.middle(), "vertical": seg.vertical}))
            }
            _ => None,
        };
        match params {
            Some(params) => {
                let mut params = self.typed("mullion", params);
                // The project's first mullion: of the type the Type Selector shows, which the command then makes.
                if params.get("type").is_none() && m.types.mullion.is_empty() {
                    params["type"] = json!(curtain::MULLION);
                }
                self.run("mullion", params);
            }
            None => self.status = Status { text: t("Click a grid line of a curtain wall.").into(), error: true },
        }
    }

    fn segments_tool(&mut self, m: &Model, p: &Painter, o: &Over, wall: Id) {
        let (Some(w), Some(g)) = (m.wall(wall), curtain::grid(m, wall)) else {
            // The wall is gone: nothing to work on.
            self.set_tool(Tool::Modify);
            return;
        };
        let merged = &w.curtain.merged;
        let ink = canvas_pal().accent;
        if o.outline {
            let mut ring: Vec<Pos2> = grid::outline(m, wall).into_iter().map(|q| (o.screen)(q)).collect();
            ring.extend(ring.first().copied());
            dashed(p, &ring, Stroke::new(2.0, ink));
        }
        // Each segment between the borders: thin where it stands, dashed where it was removed, as Revit shows them.
        for seg in g.segments().iter().filter(|x| x.border == 0) {
            let pts = o.path(m, wall, seg.vertical, seg.at, Some((seg.span[0], seg.span[1])));
            if merged.contains(&seg.key) { dashed(p, &pts, Stroke::new(1.5, ink)) } else { solid(p, &pts, Stroke::new(1.0, ink)) }
        }
        let at =
            |place: Option<Place>| place.filter(|x| x.wall == wall).and_then(|x| o.segment(m, x)).map(|(_, s)| s).filter(|s| s.border == 0);
        if let Some(seg) = o.hover.and_then(|(_, place)| at(place)) {
            solid(p, &o.path(m, wall, seg.vertical, seg.at, Some((seg.span[0], seg.span[1]))), Stroke::new(3.0, canvas_pal().hover));
        }
        let Some((_, place)) = o.click else { return };
        if o.plan {
            let text = t("Add/Remove Segments works in an elevation, a section or a 3D view, where a grid line's segments show.");
            self.status = Status { text: text.into(), error: true };
            return;
        }
        match at(place) {
            Some(seg) => {
                let remove = !merged.contains(&seg.key);
                self.run("add_remove_segments", json!({"wall": wall, "at": seg.middle(), "vertical": seg.vertical, "remove": remove}));
            }
            // A click in empty space finishes, as Revit's.
            None => self.set_tool(Tool::Modify),
        }
    }

    /// With nothing in hand: the grid of each curtain wall selected (and of a grid line's wall), the selected hand-made
    /// grid line's temporary dimensions and drag, and the pins of what is selected and pinned.
    fn curtain_selection(&mut self, ui: &mut Ui, m: &Model, p: &Painter, o: &Over) {
        let hovered = self.canvas.hovered.filter(|id| !self.selection.contains(id));
        if let Some(wall) = hovered.filter(|id| grid::wall_of(m, *id) == Some(*id)).filter(|_| o.outline) {
            let mut ring: Vec<Pos2> = grid::outline(m, wall).into_iter().map(|q| (o.screen)(q)).collect();
            ring.extend(ring.first().copied());
            dashed(p, &ring, Stroke::new(2.0, canvas_pal().hover));
        }
        if let Some(line) = hovered.filter(|id| matches!(m.elements.get(*id), Some(Element::CurtainGrid(_)))) {
            self.show_line(m, p, o, line, Stroke::new(2.0, canvas_pal().hover));
        }
        let selection = self.selection.clone();
        let mut walls: Vec<Id> = Vec::new();
        for id in &selection {
            match m.elements.get(*id) {
                Some(Element::Wall(_) | Element::CurtainGrid(_)) => {
                    if let Some(wall) = grid::wall_of(m, *id).filter(|w| !walls.contains(w)) {
                        walls.push(wall);
                    }
                }
                _ => {}
            }
        }
        for wall in walls {
            let whole = selection.contains(&wall);
            let over = Over { outline: o.outline && whole, ..*o };
            self.show_grid(m, p, &over, wall, canvas_pal().accent.gamma_multiply(0.7));
        }
        for id in &selection {
            self.show_line(m, p, o, *id, Stroke::new(2.0, canvas_pal().accent));
        }
        if let [line] = selection.as_slice()
            && matches!(m.elements.get(*line), Some(Element::CurtainGrid(_)))
        {
            self.grid_line_selected(ui, m, p, o, *line);
        }
        // A plan shows its pins itself (`pin_overlay`).
        if !o.plan && !self.canvas.dragging() {
            let mut unpin = Vec::new();
            for id in self.pinned_selection() {
                if let Some(at) = middle(m, id).map(|q| (o.screen)(q)).filter(|at| o.rect.contains(*at))
                    && self.pin_glyph(ui, p, at + vec2(14.0, -14.0), id)
                {
                    unpin.push(id);
                }
            }
            if !unpin.is_empty() {
                self.run("unpin", json!({"ids": unpin}));
            }
        }
    }

    /// Hand-made grid line `id` across its wall, dashed in `stroke`; nothing for another element.
    fn show_line(&self, m: &Model, p: &Painter, o: &Over, id: Id, stroke: Stroke) {
        if let Some(Element::CurtainGrid(l)) = m.elements.get(id)
            && let Some(wall) = m.elements.id(l.host)
        {
            dashed(p, &o.path(m, wall, l.vertical, l.at, None), stroke);
        }
    }

    /// The one hand-made grid line selected: its temporary dimensions to the lines either side, each typed to move it, and
    /// a drag of it along its wall.
    fn grid_line_selected(&mut self, ui: &mut Ui, m: &Model, p: &Painter, o: &Over, line: Id) {
        let Some(Element::CurtainGrid(l)) = m.elements.get(line) else { return };
        let Some(wall) = m.elements.id(l.host) else { return };
        let Some(g) = curtain::grid(m, wall) else { return };
        if o.plan && !l.vertical {
            return;
        }
        let (lines, extent, other) = if l.vertical { (&g.u, g.length, g.height) } else { (&g.v, g.height, g.length) };
        let before = lines.iter().map(|x| x.at).filter(|x| *x < l.at - curtain::NEAR).fold(0.0, f64::max);
        let after = lines.iter().map(|x| x.at).filter(|x| *x > l.at + curtain::NEAR).fold(extent, f64::min);
        let at = |x: f64| {
            let q = if l.vertical { grid::point(m, wall, x, other / 2.0) } else { grid::point(m, wall, other / 2.0, x) };
            q.map(|q| (o.screen)(q))
        };
        let path = o.path(m, wall, l.vertical, l.at, None);
        let dragging = self.curtain_tools.drag.is_some_and(|(id, _)| id == line);
        let cursor = ui.input(|i| i.pointer.latest_pos());
        let on_line = o.hover.map(|(c, _)| c).filter(|c| off(&path, *c) <= 6.0);
        // A press on it is the line's (the view's own drags leave a control alone), and the view's drag moves it.
        if let Some(grip) = on_line.or(cursor.filter(|_| dragging)) {
            self.control_rect(ui, Rect::from_center_size(grip, vec2(14.0, 14.0)));
            ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
        }
        let origin = ui.input(|i| i.pointer.press_origin());
        if o.response.drag_started_by(egui::PointerButton::Primary) && origin.is_some_and(|from| off(&path, from) <= 6.0) {
            self.curtain_tools.drag = Some((line, None));
        }
        if self.curtain_tools.drag.is_some_and(|(id, _)| id == line)
            && let (Some(from), Some(now)) = (origin, cursor)
        {
            let project = |q: P3| {
                let s = (o.screen)(q);
                (f64::from(s.x), f64::from(s.y))
            };
            let by = now - from;
            let to = grid::dragged(m, line, &project, (f64::from(by.x), f64::from(by.y)));
            self.curtain_tools.drag = Some((line, to));
            if let Some(to) = to {
                dashed(p, &o.path(m, wall, l.vertical, to, None), Stroke::new(1.5, canvas_pal().hover));
                if let (Some(a), Some(b)) = (at(before), at(to)) {
                    Self::curtain_dim(p, a, b, m.units.length(to - before));
                }
            }
        }
        if o.response.drag_stopped()
            && let Some((id, to)) = self.curtain_tools.drag.take()
        {
            if let Some(to) = to.filter(|to| (to - l.at).abs() > curtain::NEAR)
                && !self.refuse_pinned_ids(&[id])
            {
                self.run("modify", json!({"id": id, "at": to}));
            }
            return;
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            self.curtain_tools.drag = None;
        }
        if self.curtain_tools.drag.is_some() {
            return;
        }
        // Its temporary dimensions: to the line before it and to the line after it.
        for (far, from, to) in [(false, before, l.at), (true, l.at, after)] {
            let (Some(a), Some(b)) = (at(from), at(to)) else { continue };
            let text = m.units.length(to - from);
            let mid = a + (b - a) / 2.0;
            let ink = Stroke::new(1.0, pal().accent);
            p.line_segment([a, b], ink);
            let field = ui.id().with(("curtain grid dimension", line, far));
            let size = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), pal().accent).size();
            let rect = Rect::from_center_size(mid, size).expand2(vec2(3.0, 1.0));
            let mut done = None;
            match &mut self.curtain_tools.edit {
                Some((id, side, typed)) if *id == line && *side == far => {
                    let edit = ui.put(rect.expand2(vec2(18.0, 2.0)), TextEdit::singleline(typed).id(field).horizontal_align(Align::Center));
                    if edit.lost_focus() {
                        done = Some(ui.input(|i| !i.key_pressed(Key::Escape)).then(|| typed.clone()));
                    } else if !edit.has_focus() {
                        edit.request_focus();
                    }
                }
                _ => {
                    let r = self.control(ui, rect, field);
                    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, tf("Grid line distance {}", &[&text])));
                    p.rect_filled(rect, 2.0, if r.hovered() { pal().accent } else { pal().surface.gamma_multiply(0.85) });
                    let ink = if r.hovered() { pal().surface } else { pal().accent };
                    p.text(rect.center(), Align2::CENTER_CENTER, &text, FontId::proportional(12.0), ink);
                    if r.clicked() {
                        self.curtain_tools.edit = Some((line, far, text.clone()));
                    }
                }
            }
            if let Some(typed) = done {
                self.curtain_tools.edit = None;
                let Some(typed) = typed.filter(|x| x.trim() != text) else { continue };
                match m.units.parse(&typed) {
                    Some(length) if !self.refuse_pinned_ids(&[line]) => {
                        let to = if far { after - length } else { before + length };
                        self.run("modify", json!({"id": line, "at": to}));
                        return;
                    }
                    Some(_) => {}
                    None => self.status = Status { text: tf("`{}` is not a length", &[&typed]), error: true },
                }
            }
        }
    }
}

//! Revit's flip controls on the plan. A door selected alone shows two arrows beside it: across the wall (flip facing: its
//! swing to the wall's other side) and along it (flip hand: its hinge on the other jamb), and a window the same two; a wall
//! selected alone shows one across it on its exterior side, along the wall from
//! its middle so it never lies over the pin. A click on an arrow is the `flip` command. Space does the same from the keyboard
//! on a selection. While a door or a window is placed the cursor's side of the wall is its facing (`toward` of the `door` and
//! `window` commands) and Space flips its hand, which the next one placed keeps; while a
//! wall is drawn, Space puts its exterior side on the other side of the line. Flipping is not a move: a pinned element flips.

use bimcraft_core::geom::P2;
use bimcraft_core::model::{Element, Id, Model};
use bimcraft_core::render::View;
use bimcraft_core::walls::{self, Axis};
use egui::{Align2, Painter, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType, vec2};
use serde_json::{Value, json};

use crate::lang::t;
use crate::style::pal;
use crate::{BimApp, Tool};

/// How far a control's arrow reaches from its middle either way, and its button's size, in px.
const REACH: f32 = 8.0;
const BUTTON: f32 = 24.0;
/// How far clear of the wall's face an arrow stands, in px.
const CLEAR: f32 = 16.0;
/// How far from a wall's middle, along it, the wall's arrow stands at least, in px: more than the pin's reach and the button's.
const AWAY: f64 = 56.0;

/// What a flip control turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Control {
    /// A door's hinge to its other jamb.
    Hand,
    /// A door's swing, or a window's face, to the wall's other side.
    Facing,
    /// A wall's exterior side to the other side of its location line.
    Wall,
}

/// One arrow: what it flips, where its middle is on the screen from the canvas's corner, and the way it points
/// (a unit vector, either way along it).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrow {
    pub control: Control,
    pub id: Id,
    pub at: Pos2,
    pub along: Vec2,
}

impl Arrow {
    /// What the arrow's click runs.
    pub(crate) fn command(&self) -> Value {
        match self.control {
            Control::Wall => json!({"ids": [self.id]}),
            Control::Hand => json!({"ids": [self.id], "hand": true}),
            Control::Facing => json!({"ids": [self.id], "facing": true}),
        }
    }

    fn label(&self) -> String {
        match self.control {
            Control::Wall => t("Flip exterior side"),
            Control::Hand => t("Flip hand"),
            Control::Facing => t("Flip facing"),
        }
        .to_string()
    }
}

/// The flip arrows of what is selected: one door or window, or one wall. `origin` is the canvas's corner.
pub(crate) fn arrows(m: &Model, v: &View, origin: Pos2, selection: &[Id]) -> Vec<Arrow> {
    let &[id] = selection else { return Vec::new() };
    let screen = |q: P2| {
        let (x, y) = v.px(q);
        origin + vec2(x, y)
    };
    // The unit vector on the screen from `a` to `b`.
    let toward = |a: P2, b: P2| {
        let d = screen(b) - screen(a);
        if d.length() > 1e-3 { d.normalized() } else { Vec2::X }
    };
    match m.elements.get(id) {
        Some(e @ (Element::Door(o) | Element::Window(o))) => {
            let Some(w) = m.wall(o.host) else { return Vec::new() };
            let (axis, centre) = (Axis::of(w), walls::centre(m, w));
            let thick = m.thickness(w) * v.scale;
            let half_width = m.opening_type(e).map_or(450.0, |t| t.width / 2.0) * v.scale;
            let c = axis.at(o.offset, centre);
            let along = toward(c, axis.at(o.offset + 100.0, centre));
            // To the wall's left, its exterior side; the door swings, and the window faces, there unless flipped.
            let left = toward(c, axis.at(o.offset, centre + 100.0));
            let front = if o.flip_facing { -left } else { left };
            let here = screen(c);
            // Across the wall, beside the opening, clear of its leaf.
            let mut arrows = vec![Arrow { control: Control::Facing, id, at: here + along * (half_width as f32 + CLEAR), along: left }];
            // Its hand, along the wall, on the side it doesn't open to: a door's or a window's.
            arrows.push(Arrow { control: Control::Hand, id, at: here - front * (thick as f32 / 2.0 + CLEAR), along });
            arrows
        }
        Some(Element::Wall(w)) => {
            let (axis, (ext, _)) = (Axis::of(w), walls::faces(m, w));
            // Toward the wall's start, clear of its middle, where the pin of a pinned wall is: a quarter of its length, and
            // never nearer than `AWAY` px (the pin's icon and this button never overlap, whichever way the wall runs, so a wall
            // too short on the screen has its arrow past its start).
            let length = axis.length();
            let away = (length * v.scale / 4.0).max(AWAY) / v.scale;
            let s = length / 2.0 - away;
            let face = axis.at(s, ext);
            let left = toward(face, axis.at(s, ext + 100.0));
            vec![Arrow { control: Control::Wall, id, at: screen(face) + left * CLEAR, along: left }]
        }
        _ => Vec::new(),
    }
}

/// A double-headed arrow through `at`, along `along`.
fn arrow_shapes(p: &Painter, at: Pos2, along: Vec2, hot: bool) {
    let colour = if hot { pal().hover } else { pal().accent };
    let across = vec2(-along.y, along.x);
    p.circle_filled(at, BUTTON / 2.0, pal().surface);
    p.circle_stroke(at, BUTTON / 2.0, Stroke::new(1.0, colour));
    let stroke = Stroke::new(1.5, colour);
    p.line_segment([at - along * REACH, at + along * REACH], stroke);
    for end in [-1.0f32, 1.0] {
        let tip = at + along * (REACH * end);
        let back = tip - along * (4.0 * end);
        p.add(Shape::line(vec![back + across * 3.5, tip, back - across * 3.5], stroke));
    }
}

impl BimApp {
    /// The flip arrows of the selection, drawn over the plan, and the one clicked this frame run. Only with Modify and
    /// nothing being dragged, as Revit shows its controls.
    pub(crate) fn flip_overlay(&mut self, ui: &mut Ui, m: &Model, v: &View, rect: Rect, dragging: bool) {
        if self.tool != Tool::Modify || self.sketch.is_some() || self.passive || dragging {
            return;
        }
        let p = ui.painter_at(rect);
        let mut clicked = None;
        for a in arrows(m, v, rect.min, &self.selection) {
            let r = ui.interact(
                Rect::from_center_size(a.at, vec2(BUTTON, BUTTON)),
                ui.id().with(("flip", a.control as u8, a.id)),
                Sense::click(),
            );
            let label = a.label();
            r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
            arrow_shapes(&p, a.at, a.along, r.hovered());
            if r.clicked() {
                clicked = Some(a);
            }
            r.on_hover_text(format!("{label} (Space)"));
        }
        if let Some(a) = clicked {
            self.run("flip", a.command());
        }
    }

    /// Space in Modify: the selected walls turn round, and one door, or a few, turn one step through their four
    /// ways (facing, hand, facing, hand), each from its own state, and a window the same. While a door or a window is placed,
    /// the cursor's side of the wall is its facing and Space flips its hand, which the next one put down in the same tool keeps;
    /// while a wall is drawn, Space is the wall's exterior side. Whether Space was used.
    pub(crate) fn flip_key(&mut self) -> bool {
        self.keep_place_flip();
        match &self.tool {
            Tool::Modify => {
                self.flip();
                true
            }
            Tool::Door | Tool::Window => {
                self.place_flip.hand = !self.place_flip.hand;
                true
            }
            Tool::Wall(_) => {
                self.wall_flipped = !self.wall_flipped;
                true
            }
            _ => false,
        }
    }

    /// The selected doors and windows Space steps: each door one step through its four ways from its own state, each window
    /// its facing (the `step` of the `flip` command).
    pub(crate) fn opening_step(&self) -> Vec<Id> {
        let m = &self.session.model;
        self.selection.iter().copied().filter(|id| matches!(m.elements.get(*id), Some(Element::Door(_) | Element::Window(_)))).collect()
    }

    /// The `door` or `window` command's parameters with where the cursor is, for the side of the wall it faces, and the hand
    /// Space has flipped.
    pub(crate) fn opening_flips(&self, mut params: Value, cursor: P2) -> Value {
        params["toward"] = json!([cursor.0, cursor.1]);
        if self.place_flip.hand && matches!(self.tool, Tool::Door | Tool::Window) {
            params["flip_hand"] = json!(true);
        }
        params
    }

    /// A drawn wall's exterior side as a short arrow across the line from `a` to `b` (on the screen), at its middle: to the
    /// left of the way it is drawn, or to the right once Space has flipped it.
    pub(crate) fn exterior_mark(&self, p: &Painter, a: Pos2, b: Pos2) {
        let d = b - a;
        if d.length() < 30.0 {
            return;
        }
        // Left of a to b, on a screen whose y runs down.
        let left = vec2(d.y, -d.x).normalized();
        let side = if self.wall_flipped { -left } else { left };
        let (from, to) = (a + d / 2.0 + side * 3.0, a + d / 2.0 + side * 15.0);
        let stroke = Stroke::new(1.5, pal().accent);
        p.line_segment([from, to], stroke);
        let across = vec2(-side.y, side.x);
        p.add(Shape::line(vec![to - side * 4.0 + across * 3.5, to, to - side * 4.0 - across * 3.5], stroke));
        p.text(to + side * 8.0, Align2::CENTER_CENTER, t("Exterior"), egui::FontId::proportional(10.0), pal().accent);
    }

    /// A tool other than the ones Space flips before it puts something down has no flip waiting (Esc ends a tool without
    /// going through `set_tool`, so the frame checks).
    pub(crate) fn reset_flips(&mut self) {
        self.keep_place_flip();
        if !matches!(self.tool, Tool::Wall(_)) {
            self.wall_flipped = false;
        }
    }

    /// What Space set for the doors or windows being placed lasts while that tool does: another tool starts it afresh.
    fn keep_place_flip(&mut self) {
        let door = match self.tool {
            Tool::Door => Some(true),
            Tool::Window => Some(false),
            _ => None,
        };
        if self.place_flip.door != door {
            self.place_flip = PlaceFlip { door, ..PlaceFlip::default() };
        }
    }
}

/// The hand Space has flipped for the doors (or the windows) being placed; their facing is the cursor's side of the wall.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PlaceFlip {
    /// The tool it is for: the Door tool's, the Window tool's, or none.
    door: Option<bool>,
    pub(crate) hand: bool,
}

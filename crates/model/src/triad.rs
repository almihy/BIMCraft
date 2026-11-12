//! The navigation triad: the x, y and z axes drawn in a corner of the 3D view, turning with its camera, each end of which turns
//! the camera to look from that side of the model. What any front end needs of it is here: where the ends show ([`layout`]),
//! which one a point is on ([`pick`]), the camera a click turns to ([`turned`]), and the work plane square to an end's axis
//! ([`plane`]). The desktop draws it and takes the clicks (`ui-egui/src/triad.rs`).

use bimcraft_i18n::ui_msg;

use crate::geom::v3::{P3, dot};
use crate::view3d::Camera;
use crate::workplane::{Frame, Surface};

/// How far the camera turns up or down: it never looks straight along z (`Camera::orbit`), so the top and bottom views stand one
/// degree short of it.
const POLE: f64 = 89.0;

/// How near, in degrees, the camera must look from an end's side for it to be there already.
const HERE: f64 = 1.5;

/// An end of the triad: the positive or the negative end of the x, y or z axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum End {
    PlusX,
    MinusX,
    PlusY,
    MinusY,
    PlusZ,
    MinusZ,
}

/// The six ends, an axis after an axis, the positive end first.
pub const ENDS: [End; 6] = [End::PlusX, End::MinusX, End::PlusY, End::MinusY, End::PlusZ, End::MinusZ];

impl End {
    /// 0 for x, 1 for y, 2 for z.
    pub fn axis(self) -> usize {
        match self {
            End::PlusX | End::MinusX => 0,
            End::PlusY | End::MinusY => 1,
            End::PlusZ | End::MinusZ => 2,
        }
    }

    pub fn positive(self) -> bool {
        matches!(self, End::PlusX | End::PlusY | End::PlusZ)
    }

    /// The unit vector it points along.
    pub fn direction(self) -> P3 {
        let mut d = [0.0; 3];
        d[self.axis()] = if self.positive() { 1.0 } else { -1.0 };
        d
    }

    /// The end at the other side of the same axis.
    pub fn opposite(self) -> End {
        match self {
            End::PlusX => End::MinusX,
            End::MinusX => End::PlusX,
            End::PlusY => End::MinusY,
            End::MinusY => End::PlusY,
            End::PlusZ => End::MinusZ,
            End::MinusZ => End::PlusZ,
        }
    }

    /// The axis' letter, `X`, `Y` or `Z`.
    pub fn letter(self) -> &'static str {
        ["X", "Y", "Z"][self.axis()]
    }

    /// The axis as the end names it: `X`, `-X`, `Y`, `-Y`, `Z` or `-Z`.
    pub fn axis_name(self) -> &'static str {
        match self {
            End::PlusX => "X",
            End::MinusX => "-X",
            End::PlusY => "Y",
            End::MinusY => "-Y",
            End::PlusZ => "Z",
            End::MinusZ => "-Z",
        }
    }

    /// The side of the model the camera looks from when this end is turned to: the east side (x) is seen from the right, the south
    /// (-y) from the front, and the top (z) from above.
    pub fn side(self) -> Side {
        match self {
            End::PlusX => Side::Right,
            End::MinusX => Side::Left,
            End::PlusY => Side::Back,
            End::MinusY => Side::Front,
            End::PlusZ => Side::Top,
            End::MinusZ => Side::Bottom,
        }
    }
}

/// A side of the model the camera looks from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
}

impl Side {
    /// Its name, in English: the interface translates it (context `side`).
    pub fn name(self) -> &'static str {
        match self {
            Side::Top => ui_msg!("side", "Top"),
            Side::Bottom => ui_msg!("side", "Bottom"),
            Side::Front => ui_msg!("side", "Front"),
            Side::Back => ui_msg!("side", "Back"),
            Side::Left => ui_msg!("side", "Left"),
            Side::Right => ui_msg!("side", "Right"),
        }
    }

    /// The camera's azimuth and elevation, degrees, for looking from this side: front from the south (-y), back from the north, left
    /// from the west and right from the east, all level; top and bottom as steeply as the camera turns, east to the right in
    /// both (the model turned over, as a part is on a bench), north up in the top view.
    pub fn angles(self) -> (f64, f64) {
        match self {
            Side::Top => (-90.0, POLE),
            Side::Bottom => (-90.0, -POLE),
            Side::Front => (-90.0, 0.0),
            Side::Back => (90.0, 0.0),
            Side::Left => (180.0, 0.0),
            Side::Right => (0.0, 0.0),
        }
    }
}

/// An end as the camera shows it: `x` to the right and `y` down the screen, as fractions of the axis' length (-1 to 1), and how
/// near the viewer it stands (`depth`, 1 straight at the viewer, -1 straight away).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub end: End,
    pub x: f64,
    pub y: f64,
    pub depth: f64,
}

/// The six ends as `cam` shows them, the farthest first, so that drawing them in order leaves the nearest on top.
pub fn layout(cam: &Camera) -> [Placed; 6] {
    let (back, right, up) = cam.axes();
    let mut placed = ENDS.map(|end| {
        let d = end.direction();
        Placed { end, x: dot(d, right) + 0.0, y: -dot(d, up) + 0.0, depth: dot(d, back) + 0.0 }
    });
    placed.sort_by(|a, b| a.depth.total_cmp(&b.depth));
    placed
}

/// The end at `at` (in the same fractions as [`Placed`]), within `reach` of its middle: the nearest to the viewer where two
/// overlap, such as the two ends of the axis the camera looks along.
pub fn pick(placed: &[Placed], at: (f64, f64), reach: f64) -> Option<End> {
    placed.iter().filter(|p| (p.x - at.0).hypot(p.y - at.1) <= reach).max_by(|a, b| a.depth.total_cmp(&b.depth)).map(|p| p.end)
}

/// Whether the camera looks from the side of `end` already, to within a degree or so (the top view stands one short of straight
/// down).
pub fn looks_from(cam: &Camera, end: End) -> bool {
    let (back, ..) = cam.axes();
    dot(back, end.direction()) >= HERE.to_radians().cos()
}

/// The side a click on `end` turns the camera to: that end's, or if the camera is there already, the opposite one.
pub fn side_after(cam: &Camera, end: End) -> Side {
    if looks_from(cam, end) { end.opposite().side() } else { end.side() }
}

/// `cam` turned by a click on `end` ([`side_after`]): the same target, closeness, picture and lens, looking from the other side.
pub fn turned(cam: Camera, end: End) -> Camera {
    let (azimuth, elevation) = side_after(&cam, end).angles();
    Camera { azimuth, elevation, ..cam }
}

/// The work plane square to `end`'s axis (facing the way the end points, but a plane square to z always faces up): through the
/// point of the plane `current` that lies nearest `target`, so it crosses the plane the view was on there (through the target
/// itself with none), to the millimetre as a surface picked on the model is.
pub fn plane(current: Option<&Frame>, target: P3, end: End) -> Surface {
    let through = if target.iter().all(|v| v.is_finite()) { current.map_or(target, |f| f.nearest(target)) } else { [0.0; 3] };
    let through = if through.iter().all(|v| v.is_finite()) { through } else { [0.0; 3] };
    let normal = if end == End::MinusZ { End::PlusZ.direction() } else { end.direction() };
    Surface { point: through.map(|v| v.round() + 0.0), normal }
}

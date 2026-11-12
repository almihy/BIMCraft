//! A selected wall's temporary dimensions, as Revit shows them: from the wall to the nearest parallel wall or grid on each
//! side of it, square to the wall, and which line of each they measure (the wall's centre line, its faces, its core's centre or
//! its core's faces: what the project's settings say, Manage > Additional Settings > Temporary Dimensions, until a person
//! changes one witness line, which the dot on it cycles and a drag or Move Witness Line puts on another reference). The canvas
//! draws them and types into them (a typed distance moves the wall: the `move` command); the `temporary_dimensions` command
//! reads them for the agent; Make Permanent lays one as a `dimension`.

use serde::{Deserialize, Serialize};

use crate::geom::{self, P2};
use crate::model::{Element, Face, Key, Line, Model, Reference, TOL};
use crate::walls;

/// Which side of a wall, by the way it runs: its left is the side its exterior face (`Face::Left`) is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    /// The wall's face on this side: its exterior face on the left, its interior face on the right.
    pub fn face(self) -> Face {
        match self {
            Side::Left => Face::Left,
            Side::Right => Face::Right,
        }
    }

    /// The core's face on this side.
    pub fn core_face(self) -> Face {
        match self {
            Side::Left => Face::CoreLeft,
            Side::Right => Face::CoreRight,
        }
    }
}

/// What a temporary dimension measures to and from on a wall: Revit's Temporary Dimension Properties, Walls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WallLines {
    /// The centre of all the wall's layers.
    #[default]
    Centerlines,
    /// The faces of all its layers: the one toward what is measured.
    Faces,
    /// The centre of its core, which finishes outside it leave out. A wall with no core of its own has its centre line.
    CoreCentre,
    /// The faces of its core. A wall with no core of its own has its faces.
    CoreFaces,
}

impl WallLines {
    pub const ALL: [WallLines; 4] = [WallLines::Centerlines, WallLines::Faces, WallLines::CoreCentre, WallLines::CoreFaces];

    pub fn name(self) -> &'static str {
        match self {
            WallLines::Centerlines => "centerlines",
            WallLines::Faces => "faces",
            WallLines::CoreCentre => "core-centre",
            WallLines::CoreFaces => "core-faces",
        }
    }

    pub fn parse(name: &str) -> Option<WallLines> {
        WallLines::ALL.into_iter().find(|l| l.name() == name)
    }

    /// The line of a wall these settings measure on `side` of it, `core` saying it has a core of its own.
    pub fn line(self, side: Side, core: bool) -> Face {
        match self {
            WallLines::Centerlines => Face::Centre,
            WallLines::Faces => side.face(),
            WallLines::CoreCentre if core => Face::CoreCentre,
            WallLines::CoreFaces if core => side.core_face(),
            WallLines::CoreCentre => Face::Centre,
            WallLines::CoreFaces => side.face(),
        }
    }
}

/// What a temporary dimension measures to and from on a door or a window: Revit's Temporary Dimension Properties, Doors and
/// Windows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpeningLines {
    /// Its centre line.
    #[default]
    Centerlines,
    /// Its opening: the jamb on each side.
    Openings,
}

impl OpeningLines {
    pub const ALL: [OpeningLines; 2] = [OpeningLines::Centerlines, OpeningLines::Openings];

    pub fn name(self) -> &'static str {
        match self {
            OpeningLines::Centerlines => "centerlines",
            OpeningLines::Openings => "openings",
        }
    }

    pub fn parse(name: &str) -> Option<OpeningLines> {
        OpeningLines::ALL.into_iter().find(|l| l.name() == name)
    }
}

/// The project's Temporary Dimension Properties: kept in the file, so every person on the project sees the same dimensions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    pub walls: WallLines,
    pub openings: OpeningLines,
}

impl Settings {
    /// Whether these are the ones a project starts with, which a file leaves out.
    pub fn is_default(&self) -> bool {
        *self == Settings::default()
    }
}

/// Which line of the wall each side's temporary dimension starts at, and where its far witness line was put: nothing, to
/// follow the project's settings (`Settings`) and the nearest neighbour; or a person's choice, which Revit keeps for the
/// session. `left` and `right` are the lines of the wall (its centre line, faces, core centre or core faces).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Witness {
    pub left: Option<Face>,
    pub right: Option<Face>,
    /// The line each side's dimension ends at, where the far witness line was moved to another reference.
    pub to_left: Option<Reference>,
    pub to_right: Option<Reference>,
}

impl Witness {
    /// The line of the wall `side`'s dimension was set to start at; none when it follows the settings.
    pub fn of(self, side: Side) -> Option<Face> {
        match side {
            Side::Left => self.left,
            Side::Right => self.right,
        }
    }

    /// The reference `side`'s dimension was set to end at; none for the nearest neighbour's.
    pub fn target(self, side: Side) -> Option<Reference> {
        match side {
            Side::Left => self.to_left,
            Side::Right => self.to_right,
        }
    }

    /// The same with `side`'s line the next one after `now` (the line it measures from at the moment): the centre line, the
    /// exterior face, the interior face and, for a wall with a core, the core's centre line, its exterior face and its interior
    /// face, round to the centre line.
    pub fn cycled(self, side: Side, now: Face, core: bool) -> Witness {
        let order: &[Face] = if core {
            &[Face::Centre, Face::Left, Face::Right, Face::CoreCentre, Face::CoreLeft, Face::CoreRight]
        } else {
            &[Face::Centre, Face::Left, Face::Right]
        };
        let next = order.iter().position(|f| *f == now).map_or(Face::Centre, |i| order[(i + 1) % order.len()]);
        self.with_line(side, Some(next))
    }

    /// The same with `side`'s line of the wall `face` (none to follow the settings).
    pub fn with_line(self, side: Side, face: Option<Face>) -> Witness {
        match side {
            Side::Left => Witness { left: face, ..self },
            Side::Right => Witness { right: face, ..self },
        }
    }

    /// The same with `side`'s dimension ending at `to` (none for the nearest neighbour's).
    pub fn with_target(self, side: Side, to: Option<Reference>) -> Witness {
        match side {
            Side::Left => Witness { to_left: to, ..self },
            Side::Right => Witness { to_right: to, ..self },
        }
    }
}

/// One temporary dimension of a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempDim {
    pub side: Side,
    /// The line of the selected wall it starts at.
    pub from: Reference,
    /// The line of the wall or grid it ends at.
    pub to: Reference,
    /// How far apart the two lines are, square to the wall.
    pub distance: f64,
    /// The way from the wall to the other line: a unit vector square to the wall.
    pub toward: P2,
    /// Where it starts on the wall's line: a third (left) or two thirds (right) of the way along the stretch both lines share,
    /// and where it ends, `distance` on.
    pub start: P2,
    pub end: P2,
}

/// How far a door's or window's temporary dimensions stop short of its middle: nothing for the centre line, half its width for
/// its opening (the project's settings, `Model::temporary_dimensions`).
pub fn opening_reach(m: &Model, e: &Element) -> f64 {
    match m.temporary_dimensions.openings {
        OpeningLines::Centerlines => 0.0,
        OpeningLines::Openings => m.opening_type(e).map_or(0.0, |t| t.width / 2.0),
    }
}

/// Whether wall `id` has a core of its own, so the dot on its witness line has the core's lines to go to.
pub fn has_core(m: &Model, id: impl Key) -> bool {
    m.wall(id).is_some_and(|w| walls::core_faces(m, w).is_some())
}

/// `face` of a wall that has a core or not: a core line of one with none is the line it stands for.
fn fitted(face: Face, core: bool) -> Face {
    match face {
        Face::CoreCentre if !core => Face::Centre,
        Face::CoreLeft if !core => Face::Left,
        Face::CoreRight if !core => Face::Right,
        other => other,
    }
}

/// The lines of wall `id` a witness line can start at, with where each runs: its centre line, faces and, if it has one, its
/// core's. None for anything but a straight wall.
pub fn own_lines(m: &Model, id: impl Key) -> Vec<(Reference, Line)> {
    let me = m.guid(id);
    let Some(w) = m.wall(me).filter(|w| w.arc == 0.0 && w.start.dist(w.end) > TOL) else { return Vec::new() };
    let core = walls::core_faces(m, w).is_some();
    [Face::Centre, Face::Left, Face::Right, Face::CoreCentre, Face::CoreLeft, Face::CoreRight]
        .into_iter()
        .filter(|f| core || !matches!(f, Face::CoreCentre | Face::CoreLeft | Face::CoreRight))
        .filter_map(|f| {
            let r = Reference::line(me, f);
            Some((r, m.reference_line(&r).ok()?))
        })
        .collect()
}

/// The walls and grids the plan `level` shows that run parallel to wall `me` (a straight one): each's GUID and whether it is a
/// wall, for a temporary dimension to end at.
fn parallels(m: &Model, me: crate::model::Guid, level: crate::model::Guid) -> Vec<(crate::model::Guid, bool)> {
    let Some(w) = m.wall(me).filter(|w| w.arc == 0.0 && w.start.dist(w.end) > TOL) else { return Vec::new() };
    let look = crate::graphics::Look::of(m, level);
    if !m.elements.id(me).is_some_and(|i| look.shows(i)) {
        return Vec::new();
    }
    m.elements
        .with_guids()
        .filter(|&(i, g, e)| g != me && look.shows(i) && matches!(e, Element::Grid(_) | Element::Wall(_)))
        .filter(|&(_, _, e)| !matches!(e, Element::Wall(o) if o.arc != 0.0 || !m.cut_by(o, level)))
        .filter_map(|(_, g, e)| {
            let (a, b) = m.reference_line(&Reference::line(g, Face::Centre)).ok()?;
            (a.dist(b) > TOL && geom::parallel(w.start, w.end, a, b)).then_some((g, matches!(e, Element::Wall(_))))
        })
        .collect()
}

/// The lines a far witness line of wall `id`'s temporary dimensions can be moved to in the plan `level`: the centre line, the
/// faces and, where there is one, the core's of each parallel wall the plan shows, and each parallel grid's line.
pub fn neighbour_lines(m: &Model, id: impl Key, level: impl Key) -> Vec<(Reference, Line)> {
    let (me, level) = (m.guid(id), m.guid(level));
    parallels(m, me, level)
        .into_iter()
        .flat_map(|(g, is_wall)| {
            let faces: &[Face] = if is_wall {
                &[Face::Centre, Face::Left, Face::Right, Face::CoreCentre, Face::CoreLeft, Face::CoreRight]
            } else {
                &[Face::Centre]
            };
            faces.iter().filter_map(move |f| {
                let r = Reference::line(g, *f);
                Some((r, m.reference_line(&r).ok()?))
            })
        })
        .collect()
}

/// Wall `id`'s temporary dimensions in the plan `level` (a level, or a plan view of one): on each side the nearest line
/// parallel to it (another straight wall the plan cuts, or a grid) that the plan shows (its phase, design option and hidden
/// categories) and that stands clear of the wall and shares some of its run. The lines they measure are the project's
/// settings' (`Model::temporary_dimensions`) unless `witness` says otherwise. None for anything but a straight wall, for a
/// wall the plan doesn't show, and for a side with no such line.
pub fn wall(m: &Model, id: impl Key, level: impl Key, witness: Witness) -> Vec<TempDim> {
    let (me, level) = (m.guid(id), m.guid(level));
    let Some(w) = m.wall(me).filter(|w| w.arc == 0.0 && w.start.dist(w.end) > TOL) else { return Vec::new() };
    let others = parallels(m, me, level);
    if others.is_empty() {
        return Vec::new();
    }
    let settings = m.temporary_dimensions;
    let core = walls::core_faces(m, w).is_some();
    let u = (w.end - w.start).unit();
    let along = |p: P2| (p - w.start).dot(u);
    // The lines of the others the plan shows, as their centre lines run: what the nearest is found by.
    let centres: Vec<(crate::model::Guid, bool, Line)> =
        others.iter().filter_map(|&(g, is_wall)| Some((g, is_wall, m.reference_line(&Reference::line(g, Face::Centre)).ok()?))).collect();
    let moved_to = if witness.to_left.is_some() || witness.to_right.is_some() { neighbour_lines(m, me, level) } else { Vec::new() };
    let mut out = Vec::new();
    for side in [Side::Left, Side::Right] {
        let face = witness.of(side).map_or_else(|| settings.walls.line(side, core), |f| fitted(f, core));
        let from = Reference::line(me, face);
        let Ok((fa, fb)) = m.reference_line(&from) else { continue };
        let toward = if side == Side::Left { u.perp() } else { u.perp() * -1.0 };
        let (f_lo, f_hi) = (along(fa).min(along(fb)), along(fa).max(along(fb)));
        // How far a line is from the wall's, square to it, and the stretch of the wall's run it shares.
        let measure = |(a, b): Line| ((a - fa).dot(toward), along(a).min(along(b)).max(f_lo), along(a).max(along(b)).min(f_hi));
        // A far witness line a person moved: any parallel line the plan shows, clear of the wall's own, whether or not it shares
        // some of the run. One that is none of those is left where it was.
        let moved = witness.target(side).filter(|r| moved_to.iter().any(|(n, _)| n == r)).and_then(|r| {
            let (distance, lo, hi) = measure(m.reference_line(&r).ok()?);
            let (lo, hi) = if hi - lo > TOL { (lo, hi) } else { (f_lo, f_hi) };
            (distance > TOL).then_some((r, distance, lo, hi))
        });
        let (to, distance, lo, hi) = match moved {
            Some(found) => found,
            None => {
                let nearest = centres
                    .iter()
                    .filter_map(|&(g, is_wall, line)| {
                        let (distance, lo, hi) = measure(line);
                        // Clear of the wall's own line, and over some of its length.
                        (distance > TOL && hi - lo > TOL).then_some((g, is_wall, distance))
                    })
                    .min_by(|x, y| x.2.total_cmp(&y.2));
                let Some((g, is_wall, _)) = nearest else { continue };
                // The line of it the settings measure to, never one on the wrong side of the wall's own line.
                let target = if is_wall { target_line(m, g, settings.walls, &measure) } else { Reference::line(g, Face::Centre) };
                let Ok(line) = m.reference_line(&target) else { continue };
                let (distance, lo, hi) = measure(line);
                if distance <= TOL || hi - lo <= TOL {
                    continue;
                }
                (target, distance, lo, hi)
            }
        };
        // A third of the way along what the two share for the left, two thirds for the right, so that the two don't
        // meet at one point of the wall, nor at its middle, where its length is.
        let share = if side == Side::Left { 1.0 } else { 2.0 };
        let start = fa + u * (lo + (hi - lo) * share / 3.0 - along(fa));
        out.push(TempDim { side, from, to, distance, toward, start, end: start + toward * distance });
    }
    out
}

/// The line of wall `g` the settings `lines` measure to from a line on the other side of it: its centre line, the nearest of
/// its faces, its core's centre line or the nearest of its core's faces (a wall with no core of its own has its centre line or
/// its faces for them). Falls back to its centre line when the choice would lie behind the line measured from.
fn target_line(m: &Model, g: crate::model::Guid, lines: WallLines, measure: &impl Fn(Line) -> (f64, f64, f64)) -> Reference {
    let centre = Reference::line(g, Face::Centre);
    let core = m.wall(g).is_some_and(|w| walls::core_faces(m, w).is_some());
    let candidates: &[Face] = match (lines, core) {
        (WallLines::Centerlines, _) | (WallLines::CoreCentre, false) => return centre,
        (WallLines::CoreCentre, true) => &[Face::CoreCentre],
        (WallLines::Faces, _) | (WallLines::CoreFaces, false) => &[Face::Left, Face::Right],
        (WallLines::CoreFaces, true) => &[Face::CoreLeft, Face::CoreRight],
    };
    candidates
        .iter()
        .filter_map(|&f| {
            let r = Reference::line(g, f);
            let (distance, lo, hi) = measure(m.reference_line(&r).ok()?);
            (distance > TOL && hi - lo > TOL).then_some((r, distance))
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map_or(centre, |(r, _)| r)
}

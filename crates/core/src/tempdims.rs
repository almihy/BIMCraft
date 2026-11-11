//! A selected wall's temporary dimensions, as Revit shows them: from the wall to the nearest parallel wall or grid on each
//! side of it, square to the wall, and which line of the wall they start at (its centre line or one of its faces, which the
//! witness line's grip cycles). The canvas draws them and types into them (a typed distance moves the wall: the `move`
//! command); the `temporary_dimensions` command reads them for the agent; Make Permanent lays one as a `dimension`.

use crate::geom::{self, P2};
use crate::model::{Element, Face, Key, Model, Reference, TOL};

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
}

/// Which line of the wall each side's temporary dimension starts at: its centre line, its exterior face (`Face::Left`) or its
/// interior face (`Face::Right`). Anything else counts as the centre line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Witness {
    pub left: Face,
    pub right: Face,
}

impl Default for Witness {
    fn default() -> Witness {
        Witness { left: Face::Centre, right: Face::Centre }
    }
}

impl Witness {
    /// The line `side`'s dimension starts at.
    pub fn of(self, side: Side) -> Face {
        let face = match side {
            Side::Left => self.left,
            Side::Right => self.right,
        };
        if matches!(face, Face::Left | Face::Right) { face } else { Face::Centre }
    }

    /// The same with `side`'s line the next one: the centre line, the exterior face, the interior face, the centre line.
    pub fn cycled(self, side: Side) -> Witness {
        let next = match self.of(side) {
            Face::Centre => Face::Left,
            Face::Left => Face::Right,
            _ => Face::Centre,
        };
        match side {
            Side::Left => Witness { left: next, ..self },
            Side::Right => Witness { right: next, ..self },
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
    /// Where it starts on the wall's line: the middle of the stretch both lines share, and where it ends, `distance` on.
    pub start: P2,
    pub end: P2,
}

/// Wall `id`'s temporary dimensions in the plan of `level`: on each side the nearest line parallel to it (the centre line of
/// another straight wall the plan cuts, or a grid) that stands clear of it and shares some of its run. None for anything
/// but a straight wall, and for a side with no such line.
pub fn wall(m: &Model, id: impl Key, level: impl Key, witness: Witness) -> Vec<TempDim> {
    let (me, level) = (m.guid(id), m.guid(level));
    let Some(w) = m.wall(me).filter(|w| w.arc == 0.0 && w.start.dist(w.end) > TOL) else { return Vec::new() };
    let u = (w.end - w.start).unit();
    let along = |p: P2| (p - w.start).dot(u);
    // The lines it could end at, with how far each runs along the wall.
    let others: Vec<(Reference, (P2, P2))> = m
        .elements
        .with_guids()
        .filter(|&(_, g, e)| g != me && matches!(e, Element::Grid(_) | Element::Wall(_)))
        .filter(|&(_, _, e)| !matches!(e, Element::Wall(o) if o.arc != 0.0 || !m.cut_by(o, level)))
        .filter_map(|(_, g, _)| {
            let r = Reference::line(g, Face::Centre);
            Some((r, m.reference_line(&r).ok()?))
        })
        .filter(|(_, (a, b))| a.dist(*b) > TOL && geom::parallel(w.start, w.end, *a, *b))
        .collect();
    let mut out = Vec::new();
    for side in [Side::Left, Side::Right] {
        let from = Reference::line(me, witness.of(side));
        let Ok((fa, fb)) = m.reference_line(&from) else { continue };
        let toward = if side == Side::Left { u.perp() } else { u.perp() * -1.0 };
        let (f_lo, f_hi) = (along(fa).min(along(fb)), along(fa).max(along(fb)));
        let nearest = others
            .iter()
            .filter_map(|&(to, (a, b))| {
                let distance = (a - fa).dot(toward);
                let (lo, hi) = (along(a).min(along(b)).max(f_lo), along(a).max(along(b)).min(f_hi));
                // Clear of the wall's own line, and over some of its length.
                (distance > TOL && hi - lo > TOL).then_some((to, distance, (lo + hi) / 2.0))
            })
            .min_by(|x, y| x.1.total_cmp(&y.1));
        if let Some((to, distance, middle)) = nearest {
            let start = fa + u * (middle - along(fa));
            out.push(TempDim { side, from, to, distance, toward, start, end: start + toward * distance });
        }
    }
    out
}

//! Curtain walls: which walls are one, and their grid from the boxes of their panels.
//!
//! Nothing new is read from the file here. A wall whose type has no compound structure is a **curtain wall** where the box of a
//! curtain panel's record (`OST_CurtainWallPanels`, which `generic` reads too) lies in its own: a type with no layers and no
//! panel (a tilted solar panel, a trellis member, a type named `_Not Defined`) stays the plane of its box, as before. A panel
//! whose box lies in the boxes of two such walls is told to neither, and neither has a grid read.
//!
//! The file's curtain grid lines are not read: no finding says where their records are or what their data holds. Where the wall
//! is straight and runs along an axis, its **grid** is worked out from its panels' boxes instead, which are then true to it: a
//! line stands midway between the facing edges of two neighbouring panels (they are a mullion's width apart, or touch), each
//! way. The result is held to the file before it is used ([`Grid::of`]): the lines must make as many cells as the wall has
//! panels, and each panel must lie in a cell of its own and fill it but for the mullions round it. A grid with a segment
//! removed (a panel over two cells), a panel that is no rectangle, a wall at an angle or a curved one does not pass, and the
//! wall then comes in with no grid, as one panel of glass, its panels and mullions as the boxes `generic` makes of them.

use std::collections::BTreeMap;

use bimcraft_core::geom::P2;

use crate::import::{Facts, mm};

/// `OST_CurtainWallPanels`, as `inventory`'s and `generic`'s tables have it.
pub(crate) const OST_CURTAIN_PANELS: i64 = -2_000_170;

/// How far past its wall's box a panel's box may reach and still lie in it, feet (5 mm).
const SLACK: f64 = 5.0 / 304.8;
/// The widest gap between two panels that is one mullion's, mm: wider, and they are not neighbours across a line.
const MAX_GAP: f64 = 600.0;
/// Two edges nearer than this are one, mm.
const NEAR: f64 = 1.0;
/// The most panels a wall's grid is worked out from.
const MAX_PANELS: usize = 20_000;

/// Whether box `part` lies within box `whole`.
fn holds(whole: &[f64; 6], part: &[f64; 6]) -> bool {
    (0..3).all(|k| part[k] >= whole[k] - SLACK && part[k + 3] <= whole[k + 3] + SLACK)
}

/// The curtain panels of each of `walls` (a wall's id and its record's box): the panels whose box lies in that wall's alone,
/// and whether a panel lies in its box and another's too (`true`: its grid is not to be read from them).
pub(crate) fn panels_of(f: &Facts, walls: &[(u32, [f64; 6])]) -> BTreeMap<u32, (Vec<u32>, bool)> {
    let mut out: BTreeMap<u32, (Vec<u32>, bool)> = BTreeMap::new();
    if walls.is_empty() {
        return out;
    }
    for p in f.of_category(OST_CURTAIN_PANELS) {
        let mut holders = walls.iter().filter(|(_, b)| holds(b, &p.bbox)).map(|(id, _)| *id);
        match (holders.next(), holders.next()) {
            (Some(one), None) => out.entry(one).or_default().0.push(p.id),
            (Some(a), Some(b)) => {
                for id in [a, b].into_iter().chain(holders) {
                    out.entry(id).or_default().1 = true;
                }
            }
            _ => {}
        }
    }
    out
}

/// A curtain wall's grid as its panels' boxes give it: its lines between its borders, mm along the wall from its start and mm
/// over its base, and how thick its panels are where they agree, mm.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grid {
    pub vertical: Vec<f64>,
    pub horizontal: Vec<f64>,
    pub thickness: Option<f64>,
}

/// From where to where a panel's box reaches one way, mm.
type Span = (f64, f64);

/// The lines between intervals over `extent`: one midway between each interval's end and the nearest start at or after it
/// that is no more than a mullion away, each once.
fn lines(spans: &[Span], extent: f64) -> Vec<f64> {
    let mut starts: Vec<f64> = spans.iter().map(|s| s.0).collect();
    starts.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for (_, end) in spans {
        let at = starts.partition_point(|s| *s < end - NEAR);
        let Some(next) = starts.get(at).filter(|s| **s - end <= MAX_GAP) else { continue };
        let line = (end + next) / 2.0;
        if line > NEAR && line < extent - NEAR {
            out.push(line);
        }
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|b, a| (*b - *a).abs() <= NEAR);
    out
}

/// The cell of `lines` over `extent` that `(lo, hi)` lies in and fills but for a mullion each side: its index.
fn cell(lines: &[f64], extent: f64, (lo, hi): Span) -> Option<usize> {
    let i = lines.partition_point(|l| *l <= (lo + hi) / 2.0);
    let from = if i == 0 { 0.0 } else { *lines.get(i - 1)? };
    let to = lines.get(i).copied().unwrap_or(extent);
    (lo >= from - NEAR && hi <= to + NEAR && lo - from <= MAX_GAP && to - hi <= MAX_GAP).then_some(i)
}

impl Grid {
    /// The grid of a wall from `start` to `end` (mm), `arc` degrees round, its base at `base` mm and `height` mm tall, from
    /// the boxes of its panels (feet): none where the wall is curved or runs along no axis, or where the lines the boxes give
    /// do not hold every panel in a cell of its own.
    pub(crate) fn of(start: P2, end: P2, arc: f64, base: f64, height: f64, panels: &[[f64; 6]]) -> Option<Grid> {
        let d = end - start;
        let length = d.length();
        if arc != 0.0 || panels.is_empty() || panels.len() > MAX_PANELS || !(length > NEAR && height > NEAR) {
            return None;
        }
        // The axis it runs along (x: 0, y: 1) and which way.
        let (axis, way) = if d.1.abs() <= NEAR {
            (0, d.0.signum())
        } else if d.0.abs() <= NEAR {
            (1, d.1.signum())
        } else {
            return None;
        };
        let origin = if axis == 0 { start.0 } else { start.1 };
        let along = |b: &[f64; 6]| {
            let (a, z) = ((mm(b[axis]) - origin) * way, (mm(b[axis + 3]) - origin) * way);
            (a.min(z), a.max(z))
        };
        let up = |b: &[f64; 6]| (mm(b[2]) - base, mm(b[5]) - base);
        let (us, vs): (Vec<Span>, Vec<Span>) = panels.iter().map(|b| (along(b), up(b))).unzip();
        let (vertical, horizontal) = (lines(&us, length), lines(&vs, height));
        // Held to the file: as many cells as panels, and each panel in a cell of its own.
        if (vertical.len() + 1).checked_mul(horizontal.len() + 1) != Some(panels.len()) {
            return None;
        }
        let mut taken = vec![false; panels.len()];
        for (u, v) in us.iter().zip(&vs) {
            let (i, j) = (cell(&vertical, length, *u)?, cell(&horizontal, height, *v)?);
            let slot = taken.get_mut(j * (vertical.len() + 1) + i)?;
            if std::mem::replace(slot, true) {
                return None;
            }
        }
        // How thick the panels are: their boxes across the wall, where they agree to half a millimetre.
        let across = 1 - axis;
        let thick: Vec<f64> = panels.iter().map(|b| mm(b[across + 3]) - mm(b[across])).collect();
        let first = thick.first().copied().filter(|t| t.is_finite() && *t >= 1.0);
        let thickness = first.filter(|t| thick.iter().all(|x| (x - t).abs() <= 0.5)).map(|t| (t * 10.0).round() / 10.0);
        Some(Grid { vertical, horizontal, thickness })
    }
}

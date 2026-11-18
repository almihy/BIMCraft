//! Layer wrapping, as Revit's: where a wall's body stops free (an end that joins nothing, the jamb of a door or a window),
//! the layers outside its core that are ticked Wraps turn the corner and run across the core's end, one inside another,
//! on the side its type says (Wrapping at Ends, Wrapping at Inserts). Worked out along the wall and across it, with no
//! plan in it: `s` is how far along the wall, `d` how deep from its exterior face.
//!
//! A wrapping layer keeps its thickness round the corner. Those nearer the face lie outside those deeper, so a layer's
//! return stands in from the stop by the thickness of the wrapping layers outside it, and every layer the returns cross
//! (the core, and what lies between them) stops short by as much. A return runs from its layer to the core's far boundary;
//! where both sides wrap (an insert's Both), each side's to the middle of the core.

use crate::model::{WallType, Wrapping};

/// A stretch of a wall's body between two stops, with the side its layers wrap on at each (none: they don't, there).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Body {
    pub(super) from: f64,
    pub(super) to: f64,
    pub(super) start: Wrapping,
    pub(super) end: Wrapping,
}

/// A piece of a layer: from `s0` to `s1` along the wall, `d0` to `d1` deep.
pub(super) type Piece = [f64; 4];
/// A line a plan draws between layers: from (`s`, `d`) to (`s`, `d`).
pub(super) type Seam = [[f64; 2]; 2];

/// A wrapping layer's return at a stop: the layer, how deep it runs from and to, how far in from the stop it stands (the
/// wrapping layers outside it) and how thick it is.
struct Return {
    layer: usize,
    d0: f64,
    d1: f64,
    inset: f64,
    thick: f64,
    /// Of the exterior side (it runs inward from its layer), else of the interior (outward).
    exterior: bool,
}

/// How deep each layer of `t` starts, and the last ends: from its exterior face.
fn depths(t: &WallType) -> Vec<f64> {
    let mut at = 0.0;
    let mut out = vec![0.0];
    for l in &t.layers {
        at += l.thickness;
        out.push(at);
    }
    out
}

/// The returns at a stop where `sides` wrap.
fn returns(t: &WallType, sides: Wrapping) -> Vec<Return> {
    let (db, core, (c0, c1)) = (depths(t), t.core_layers(), t.core_span());
    let middle = (c0 + c1) / 2.0;
    let wraps = |k: usize| t.layers.get(k).is_some_and(|l| l.wraps && l.thickness > 0.0);
    let depth = |k: usize| db.get(k).copied().unwrap_or(0.0);
    let mut out = Vec::new();
    if matches!(sides, Wrapping::Exterior | Wrapping::Both) {
        let to = if sides == Wrapping::Both { middle } else { c1 };
        let mut inset = 0.0;
        for k in (0..core.start).filter(|k| wraps(*k)) {
            let thick = depth(k + 1) - depth(k);
            out.push(Return { layer: k, d0: depth(k + 1), d1: to, inset, thick, exterior: true });
            inset += thick;
        }
    }
    if matches!(sides, Wrapping::Interior | Wrapping::Both) {
        let to = if sides == Wrapping::Both { middle } else { c0 };
        let mut inset = 0.0;
        for k in (core.end..t.layers.len()).rev().filter(|k| wraps(*k)) {
            let thick = depth(k + 1) - depth(k);
            out.push(Return { layer: k, d0: to, d1: depth(k), inset, thick, exterior: false });
            inset += thick;
        }
    }
    out.retain(|r| r.d1 > r.d0);
    out
}

/// How far short of a stop what lies `d` deep ends: the thickness of the returns that cross it there.
fn inset(db: &[f64], returns: &[Return], d: f64) -> f64 {
    let depth = |k: usize| db.get(k).copied().unwrap_or(0.0);
    returns
        .iter()
        .filter(|r| if r.exterior { depth(r.layer + 1) <= d && d < r.d1 } else { depth(r.layer) >= d && d > r.d0 })
        .map(|r| r.thick)
        .sum()
}

/// Each layer's pieces over `bodies`, and the lines between layers: the layers' own boundaries, as far as both layers run,
/// and round each return.
pub(super) fn shape(t: &WallType, bodies: &[Body]) -> (Vec<Vec<Piece>>, Vec<Seam>) {
    const EPS: f64 = 1e-6;
    let db = depths(t);
    let n = t.layers.len();
    let (c0, c1) = t.core_span();
    let middle = (c0 + c1) / 2.0;
    let mut pieces: Vec<Vec<Piece>> = vec![Vec::new(); n];
    let mut seams: Vec<Seam> = Vec::new();
    for b in bodies {
        let (at_start, at_end) = (returns(t, b.start), returns(t, b.end));
        let both = b.start == Wrapping::Both || b.end == Wrapping::Both;
        for k in 0..n {
            let (Some(&x0), Some(&x1)) = (db.get(k), db.get(k + 1)) else { continue };
            // A layer the two sides' returns meet in is stepped: one piece either side of the core's middle.
            let parts = if both && x0 + EPS < middle && middle < x1 - EPS { vec![(x0, middle), (middle, x1)] } else { vec![(x0, x1)] };
            for (y0, y1) in parts {
                let mid = (y0 + y1) / 2.0;
                let (s0, s1) = (b.from + inset(&db, &at_start, mid), b.to - inset(&db, &at_end, mid));
                if s1 - s0 > EPS && y1 - y0 > EPS {
                    if let Some(p) = pieces.get_mut(k) {
                        p.push([s0, s1, y0, y1]);
                    }
                }
            }
        }
        // The line between two layers, as far as the shorter of them runs.
        for k in 1..n {
            let Some(&d) = db.get(k) else { continue };
            let short = |r: &[Return]| inset(&db, r, d - EPS).max(inset(&db, r, d + EPS));
            let (s0, s1) = (b.from + short(&at_start), b.to - short(&at_end));
            if s1 - s0 > EPS {
                seams.push([[s0, d], [s1, d]]);
            }
        }
        // Each return: its piece, the line along its inner side, the one across its far end, and the one between it and
        // the return outside it.
        for (stop, way, list) in [(b.from, 1.0, &at_start), (b.to, -1.0, &at_end)] {
            for r in list {
                let (outer, inner) = (stop + way * r.inset, stop + way * (r.inset + r.thick));
                if let Some(p) = pieces.get_mut(r.layer) {
                    p.push([outer.min(inner), outer.max(inner), r.d0, r.d1]);
                }
                seams.push([[inner, r.d0], [inner, r.d1]]);
                let far = if r.exterior { r.d1 } else { r.d0 };
                seams.push([[outer, far], [inner, far]]);
                if r.inset > EPS {
                    seams.push([[outer, r.d0], [outer, r.d1]]);
                }
            }
        }
    }
    (pieces, seams)
}

/// The stretches of a wall `len` long between its ends and its `openings` (each from where to where along it), with the
/// side its layers wrap on at each stop: `ends` at its start and at its end (none where the end joins another wall),
/// `inserts` at each jamb.
pub(super) fn bodies(len: f64, openings: &[(f64, f64)], ends: [Wrapping; 2], inserts: Wrapping) -> Vec<Body> {
    let mut gaps: Vec<(f64, f64)> = openings.iter().map(|(a, b)| (a.min(*b), a.max(*b))).filter(|(a, b)| *b > 0.0 && *a < len).collect();
    gaps.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut out = Vec::new();
    let (mut from, mut start) = (0.0, ends[0]);
    for (a, b) in gaps {
        if a > from {
            out.push(Body { from, to: a, start, end: inserts });
        }
        if b > from {
            (from, start) = (b, inserts);
        }
    }
    if from < len {
        out.push(Body { from, to: len, start, end: ends[1] });
    }
    out
}

#[cfg(test)]
#[path = "wrap_tests.rs"]
mod tests;
